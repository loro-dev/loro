//! Typed views. A view stores a location, never a copy of the data: every read
//! goes to the current document state, so views stay correct across local
//! edits, imports, checkout, undo and `revert_to` without any refresh step.

use std::marker::PhantomData;
use std::sync::Arc;

use loro::{
    Container, ContainerID, ContainerTrait, ContainerType, Index, LoroDoc, LoroMap, LoroText,
    LoroValue, Subscription, ValueOrContainer,
};

use crate::slot::{ListRef, Slot};
use crate::{list, ErrorKind, FromLoro, ModelError, Result, ToLoro};

/// Where a view points.
#[derive(Clone, Debug)]
pub enum Loc {
    /// A concrete container. Used for roots and list elements; it keeps
    /// pointing at the same element when list indices shift.
    Container(Container),
    /// A map field, resolved on every access, so replacing the field (locally
    /// or remotely) is observed instead of leaving a stale handle.
    Field(Arc<Loc>, Arc<str>),
}

impl Loc {
    pub fn field(parent: &Loc, key: &str) -> Loc {
        Loc::Field(Arc::new(parent.clone()), key.into())
    }

    /// Current value or container at this location. `None` if absent.
    pub fn resolve(&self) -> Result<Option<ValueOrContainer>> {
        match self {
            Loc::Container(c) => {
                if c.is_deleted() {
                    return Err(ModelError::new(ErrorKind::Deleted));
                }
                Ok(Some(ValueOrContainer::Container(c.clone())))
            }
            Loc::Field(parent, key) => match parent.resolve_map()? {
                Some(m) => Ok(m.get(key)),
                None => Ok(None),
            },
        }
    }

    fn resolve_map(&self) -> Result<Option<LoroMap>> {
        match self.resolve()? {
            None => Ok(None),
            Some(ValueOrContainer::Container(Container::Map(m))) => Ok(Some(m)),
            Some(other) => Err(ModelError::mismatch("map", other)),
        }
    }

    /// The map at this location, created (as a mergeable child) if absent.
    pub fn ensure_map(&self) -> Result<LoroMap> {
        self.with_slot(|slot| slot.ensure_map())
    }

    /// Run `f` with a writable slot for this location, creating parents.
    pub fn with_slot<R>(&self, f: impl FnOnce(Slot<'_>) -> Result<R>) -> Result<R> {
        match self {
            Loc::Container(c) => {
                if c.is_deleted() {
                    return Err(ModelError::new(ErrorKind::Deleted));
                }
                f(Slot::Container(c))
            }
            Loc::Field(parent, key) => {
                let m = parent.ensure_map()?;
                f(Slot::MapKey(&m, key))
            }
        }
    }

    pub fn deep_value(&self) -> Result<Option<LoroValue>> {
        Ok(self.resolve()?.map(|v| v.get_deep_value()))
    }

    /// The document of the nearest attached container.
    pub fn doc(&self) -> Option<LoroDoc> {
        match self {
            Loc::Container(c) => c.doc(),
            Loc::Field(parent, _) => parent.doc(),
        }
    }

    /// The container to subscribe to and, for a map field, the key to filter
    /// on. Subscribes to the parent map for fields, so a field that is
    /// replaced or created later is still observed.
    fn subscription_target(&self) -> Result<(ContainerID, Option<Arc<str>>)> {
        match self {
            Loc::Container(c) => Ok((c.id(), None)),
            Loc::Field(parent, key) => match parent.resolve_map()? {
                Some(m) => Ok((m.id(), Some(key.clone()))),
                None => Err(ModelError::new(ErrorKind::Missing)),
            },
        }
    }
}

/// A typed view. Implemented by library views and by generated `XxxModel`s.
pub trait View: Sized {
    fn from_loc(loc: Loc) -> Self;
    fn loc(&self) -> &Loc;

    /// Project this subtree into any owned type. `O(output size)`.
    fn read_as<T: FromLoro>(&self) -> Result<T> {
        match self.loc().deep_value()? {
            Some(v) => T::from_loro(&v),
            None => T::from_missing(),
        }
    }

    /// Reconcile a plain value into this subtree, reusing containers and
    /// skipping unchanged values.
    fn update_from<T: ToLoro + ?Sized>(&self, value: &T) -> Result<()> {
        self.loc().with_slot(|slot| value.to_loro(slot))
    }

    /// Read, modify, reconcile. Prefer this over holding a DTO from an earlier
    /// `read_as`: reconciling a stale DTO reverts whatever changed since it was
    /// read (including imported remote edits).
    fn edit<T: FromLoro + ToLoro>(&self, f: impl FnOnce(&mut T)) -> Result<()> {
        let mut v: T = self.read_as()?;
        f(&mut v);
        self.update_from(&v)
    }

    /// Low-level subscription: fires (after commit/import/checkout) with the
    /// raw Loro event whenever this subtree may have changed. Carries new
    /// values and deltas, not old values; see [`View::watch`].
    fn subscribe(
        &self,
        callback: impl Fn(&loro::event::DiffEvent) + Send + Sync + 'static,
    ) -> Result<Subscription> {
        let (target, key) = self.loc().subscription_target()?;
        let doc = self
            .loc()
            .doc()
            .ok_or_else(|| ModelError::new(ErrorKind::Missing))?;
        Ok(doc.subscribe(
            &target.clone(),
            Arc::new(move |e| {
                if touches(&e, &target, key.as_deref()) {
                    callback(&e);
                }
            }),
        ))
    }

    /// Watch this subtree projected as `T`, calling `on_change(old, new)` when
    /// the projected value changes.
    ///
    /// The watcher keeps only the last projected `T`, so old values cost
    /// `O(size of T)`, not a model-wide cache. Each relevant event re-reads
    /// the subtree once.
    fn watch<T>(&self, on_change: impl FnMut(&T, &T) + Send + 'static) -> Result<Subscription>
    where
        T: FromLoro + PartialEq + Send + 'static,
    {
        let loc = self.loc().clone();
        let read = move || -> Result<T> {
            match loc.deep_value()? {
                Some(v) => T::from_loro(&v),
                None => T::from_missing(),
            }
        };
        let state = std::sync::Mutex::new((read().ok(), on_change));
        self.subscribe(move |_| {
            let Ok(new) = read() else { return };
            let (last, on_change) = &mut *state.lock().unwrap();
            match last {
                Some(old) if *old == new => {}
                Some(old) => {
                    on_change(old, &new);
                    *last = Some(new);
                }
                None => *last = Some(new),
            }
        })
    }
}

fn touches(e: &loro::event::DiffEvent, target: &ContainerID, key: Option<&str>) -> bool {
    let Some(key) = key else { return true };
    e.events.iter().any(|d| {
        if d.target == target {
            return match &d.diff {
                loro::event::Diff::Map(m) => m.updated.contains_key(key),
                _ => true,
            };
        }
        d.path
            .iter()
            .any(|(id, idx)| id == target && matches!(idx, Index::Key(k) if &**k == key))
    })
}

/// Associates a plain Rust type with its typed view, so derive output for
/// `todos: Vec<Todo>` can name `<Vec<Todo> as LoroSchema>::View` without
/// knowing how `Todo` was derived.
pub trait LoroSchema: FromLoro + ToLoro {
    type View: View;
    /// View of `Vec<Self>`: [`KeyedVec`] when `Self` has a key field, else
    /// [`ListView`]. Lets `by_key` exist only where it means something.
    type VecView: View;
}

/// View of a scalar (or any value read and written as a whole).
pub struct ValueField<T> {
    loc: Loc,
    _t: PhantomData<fn() -> T>,
}

impl<T> View for ValueField<T> {
    fn from_loc(loc: Loc) -> Self {
        Self {
            loc,
            _t: PhantomData,
        }
    }
    fn loc(&self) -> &Loc {
        &self.loc
    }
}

impl<T: FromLoro + ToLoro> ValueField<T> {
    pub fn get(&self) -> Result<T> {
        self.read_as()
    }

    pub fn set(&self, v: impl Into<T>) -> Result<()> {
        self.update_from(&v.into())
    }
}

macro_rules! scalar_schema {
    ($($t:ty),*) => {$(
        impl LoroSchema for $t {
            type View = ValueField<$t>;
            type VecView = ListView<$t>;
        }
    )*};
}

scalar_schema!(bool, i32, i64, u32, u64, f64, String);

impl<T: LoroSchema> LoroSchema for Option<T> {
    type View = ValueField<Option<T>>;
    type VecView = ListView<Option<T>>;
}

/// View of a `#[loro(text)]` field.
pub struct TextField {
    loc: Loc,
}

impl View for TextField {
    fn from_loc(loc: Loc) -> Self {
        Self { loc }
    }
    fn loc(&self) -> &Loc {
        &self.loc
    }
}

impl TextField {
    pub fn get(&self) -> Result<String> {
        self.read_as()
    }

    /// The text container, created if absent.
    pub fn text(&self) -> Result<LoroText> {
        self.loc.with_slot(|s| s.ensure_text())
    }

    /// Character-level edit (Unicode scalar positions).
    pub fn splice(&self, pos: usize, delete: usize, insert: &str) -> Result<()> {
        let t = self.text()?;
        if delete > 0 {
            t.delete(pos, delete)?;
        }
        if !insert.is_empty() {
            t.insert(pos, insert)?;
        }
        Ok(())
    }

    /// Replace the whole text with a minimal diff.
    pub fn set(&self, s: &str) -> Result<()> {
        self.loc.with_slot(|slot| crate::text::to_loro(s, slot))
    }
}

/// View of a plain (positional) list. Element views of container elements are
/// bound to the element container, so they survive index shifts.
pub struct ListView<T> {
    loc: Loc,
    _t: PhantomData<fn() -> T>,
}

impl<T> View for ListView<T> {
    fn from_loc(loc: Loc) -> Self {
        Self {
            loc,
            _t: PhantomData,
        }
    }
    fn loc(&self) -> &Loc {
        &self.loc
    }
}

impl<T: LoroSchema> LoroSchema for Vec<T> {
    type View = T::VecView;
    type VecView = ListView<Vec<T>>;
}

impl<T: LoroSchema> ListView<T> {
    fn list(&self) -> Result<Option<ListRef>> {
        match self.loc.resolve()? {
            None => Ok(None),
            Some(ValueOrContainer::Container(c)) => match ListRef::from_container(&c) {
                Some(l) => Ok(Some(l)),
                None => Err(ModelError::mismatch("list", c.get_type())),
            },
            Some(other) => Err(ModelError::mismatch("list", other)),
        }
    }

    fn list_mut(&self) -> Result<ListRef> {
        self.loc.with_slot(|s| s.ensure_list(ContainerType::List))
    }

    pub fn len(&self) -> Result<usize> {
        Ok(self.list()?.map_or(0, |l| l.len()))
    }

    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// View of the element at `index`, bound to the element's container.
    pub fn at(&self, index: usize) -> Result<Option<T::View>> {
        let Some(list) = self.list()? else {
            return Ok(None);
        };
        Ok(match list.get(index) {
            Some(ValueOrContainer::Container(c)) => Some(T::View::from_loc(Loc::Container(c))),
            _ => None,
        })
    }

    /// Read the element at `index` as an owned value.
    pub fn get(&self, index: usize) -> Result<Option<T>> {
        let Some(list) = self.list()? else {
            return Ok(None);
        };
        list.get(index)
            .map(|v| T::from_loro(&v.get_deep_value()).map_err(|e| e.in_index(index)))
            .transpose()
    }

    pub fn insert(&self, index: usize, value: &T) -> Result<()> {
        let list = self.list_mut()?;
        value.to_loro(Slot::ListInsert(&list, index))
    }

    pub fn push(&self, value: &T) -> Result<()> {
        let list = self.list_mut()?;
        let len = list.len();
        value.to_loro(Slot::ListInsert(&list, len))
    }

    pub fn remove(&self, index: usize) -> Result<()> {
        self.list_mut()?.delete(index, 1)
    }
}

/// View of a keyed vec (see [`crate::list`] for the storage layout).
///
/// Lookup by key is one map read. Index-based operations walk `order`, which
/// is `O(n)` scalar reads.
pub struct KeyedVec<T> {
    loc: Loc,
    _t: PhantomData<fn() -> T>,
}

impl<T> View for KeyedVec<T> {
    fn from_loc(loc: Loc) -> Self {
        Self {
            loc,
            _t: PhantomData,
        }
    }
    fn loc(&self) -> &Loc {
        &self.loc
    }
}

impl<T: LoroSchema> KeyedVec<T> {
    fn items_loc(&self) -> Loc {
        Loc::field(&self.loc, list::ITEMS)
    }

    fn parts(&self) -> Result<Option<list::KeyedParts>> {
        match self.loc.resolve()? {
            None => Ok(None),
            Some(ValueOrContainer::Container(Container::Map(m))) => list::KeyedParts::get(&m),
            Some(other) => Err(ModelError::mismatch("keyed vec (map)", other)),
        }
    }

    fn parts_mut(&self) -> Result<list::KeyedParts> {
        self.loc.with_slot(list::KeyedParts::ensure)
    }

    /// Number of elements. `O(1)`: counts `items`, not `order`.
    pub fn len(&self) -> Result<usize> {
        Ok(self.parts()?.map_or(0, |p| p.items.len()))
    }

    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    pub fn contains_key(&self, key: &str) -> Result<bool> {
        Ok(self.parts()?.is_some_and(|p| p.items.get(key).is_some()))
    }

    /// View of the element with this key. The view is bound to the key, not to
    /// a position, so it stays valid when the element moves.
    pub fn by_key(&self, key: &str) -> Result<Option<T::View>> {
        if !self.contains_key(key)? {
            return Ok(None);
        }
        Ok(Some(T::View::from_loc(Loc::field(&self.items_loc(), key))))
    }

    /// Visible keys in order.
    pub fn keys(&self) -> Result<Vec<String>> {
        Ok(self
            .parts()?
            .map(|p| p.visible().into_iter().map(|(_, k)| k).collect())
            .unwrap_or_default())
    }

    /// Position of a key among visible elements. `O(n)`.
    pub fn position_of(&self, key: &str) -> Result<Option<usize>> {
        Ok(self.keys()?.iter().position(|k| k == key))
    }

    /// Insert a new element before visible position `index`. Errors if the key
    /// already exists; use `by_key(..).update_from(..)` to change one.
    pub fn insert(&self, index: usize, value: &T) -> Result<()> {
        let key = value
            .key()
            .ok_or_else(|| ModelError::new(ErrorKind::Missing))?;
        let key = list::key_string(&key)?.to_string();
        let parts = self.parts_mut()?;
        if parts.items.get(&key).is_some() {
            return Err(ModelError::new(ErrorKind::DuplicateKey(key)));
        }
        let visible = parts.visible();
        let raw = match visible.get(index) {
            Some((Some(raw), _)) => *raw,
            _ => parts.order.len(),
        };
        Slot::ListInsert(&parts.order, raw).set_value(key.as_str().into())?;
        value.to_loro(Slot::MapKey(&parts.items, &key))
    }

    pub fn push(&self, value: &T) -> Result<()> {
        self.insert(usize::MAX, value)
    }

    /// Remove the element with this key. Returns whether it existed.
    pub fn remove(&self, key: &str) -> Result<bool> {
        let Some(parts) = self.parts()? else {
            return Ok(false);
        };
        let existed = parts.items.get(key).is_some();
        if existed {
            parts.items.delete(key)?;
        }
        // Also drop every occurrence in `order`, from the back.
        if let LoroValue::List(order) = parts.order.get_deep_value() {
            for i in (0..order.len()).rev() {
                if matches!(&order[i], LoroValue::String(s) if s.as_str() == key) {
                    parts.order.delete(i, 1)?;
                }
            }
        }
        Ok(existed)
    }

    /// Move the element with this key to visible position `to`.
    pub fn move_to(&self, key: &str, to: usize) -> Result<()> {
        let parts = self.parts_mut()?;
        let visible = parts.visible();
        let Some(from_vis) = visible.iter().position(|(_, k)| k == key) else {
            return Err(ModelError::new(ErrorKind::Missing).in_key(key));
        };
        // Target: before the element that will be at visible `to` once the
        // moved element is taken out. Unordered items have no raw index, so
        // the anchor is the end of `order`.
        let raw_anchor = visible
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != from_vis)
            .nth(to)
            .and_then(|(_, (raw, _))| *raw)
            .unwrap_or(parts.order.len());
        match visible[from_vis].0 {
            // An unordered item gets its first order entry.
            None => Slot::ListInsert(&parts.order, raw_anchor).set_value(key.into())?,
            Some(raw_from) => {
                let raw_to = if raw_from < raw_anchor {
                    raw_anchor - 1
                } else {
                    raw_anchor
                };
                if raw_from != raw_to {
                    parts.order.mov(raw_from, raw_to)?;
                }
            }
        }
        Ok(())
    }
}
