//! Typed views. A view stores a location, never a copy of the data: every read
//! goes to the current document state, so views stay correct across local
//! edits, imports, checkout, undo and `revert_to` without any refresh step.

use std::marker::PhantomData;
use std::sync::Arc;

use loro::{
    Container, ContainerID, ContainerTrait, ContainerType, Index, LoroDoc, LoroMap, LoroText,
    LoroValue, ReadEntry, StateReader, Subscription, ValueOrContainer,
};

use crate::slot::{ListRef, Slot};
use crate::{list, ErrorKind, FromLoro, ModelError, Result, ToLoro};

/// Where a view points.
#[derive(Clone, Debug)]
pub enum Loc {
    /// A concrete container. Used for roots and list elements; it keeps
    /// pointing at the same element when list indices shift. The document is
    /// captured up front: `ContainerTrait::doc()` takes the transaction lock,
    /// so calling it from a subscriber during a commit would deadlock.
    Container(Container, Option<LoroDoc>),
    /// A map field, resolved on every access, so replacing the field (locally
    /// or remotely) is observed instead of leaving a stale handle.
    Field(Arc<Loc>, Arc<str>),
    /// A keyed vec item, with the same path semantics as `Field`, whose key
    /// cannot be changed by reconciling a keyed value through this view.
    KeyedItem(Arc<Loc>, Arc<str>),
}

impl Loc {
    /// A view root. Call outside event callbacks (see [`Loc::Container`]).
    pub fn root(c: Container) -> Loc {
        let doc = c.doc();
        Loc::Container(c, doc)
    }

    pub fn field(parent: &Loc, key: &str) -> Loc {
        Loc::Field(Arc::new(parent.clone()), key.into())
    }

    /// Current value or container at this location. `None` if absent.
    pub fn resolve(&self) -> Result<Option<ValueOrContainer>> {
        match self {
            Loc::Container(c, _) => {
                if c.is_deleted() {
                    return Err(ModelError::new(ErrorKind::Deleted));
                }
                Ok(Some(ValueOrContainer::Container(c.clone())))
            }
            Loc::Field(parent, key) | Loc::KeyedItem(parent, key) => match parent.resolve_map()? {
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
            Loc::Container(c, _) => {
                if c.is_deleted() {
                    return Err(ModelError::new(ErrorKind::Deleted));
                }
                f(Slot::Container(c))
            }
            Loc::Field(parent, key) | Loc::KeyedItem(parent, key) => {
                let m = parent.ensure_map()?;
                f(Slot::MapKey(&m, key))
            }
        }
    }

    pub fn deep_value(&self) -> Result<Option<LoroValue>> {
        Ok(self.resolve()?.map(|v| v.get_deep_value()))
    }

    /// [`Loc::resolve`] through a [`StateReader`]: no handlers, no extra locks.
    pub fn resolve_in<'r>(&self, r: &mut StateReader<'r>) -> Result<Option<ReadEntry<'r>>> {
        match self {
            Loc::Container(c, _) => {
                let c = r
                    .container(c)
                    .ok_or_else(|| ModelError::new(ErrorKind::Missing))?;
                if r.is_deleted(c) {
                    return Err(ModelError::new(ErrorKind::Deleted));
                }
                Ok(Some(ReadEntry::Container(c)))
            }
            Loc::Field(parent, key) | Loc::KeyedItem(parent, key) => match parent.resolve_in(r)? {
                None => Ok(None),
                Some(ReadEntry::Container(m)) if m.container_type() == ContainerType::Map => {
                    Ok(r.map_get(m, key))
                }
                Some(ReadEntry::Container(c)) => {
                    Err(ModelError::mismatch("map", c.container_type()))
                }
                Some(ReadEntry::Value(v)) => Err(ModelError::mismatch("map", v)),
            },
        }
    }

    /// Read this location as `T` under one `read_state` lock.
    pub fn read<T: FromLoro>(&self) -> Result<T> {
        let Some(doc) = self.doc() else {
            // Detached: no state to borrow.
            return match self.deep_value()? {
                Some(v) => T::from_loro(&v),
                None => T::from_missing(),
            };
        };
        doc.read_state(|r| match self.resolve_in(r)? {
            Some(e) => T::read(r, e),
            None => T::from_missing(),
        })
    }

    /// The document of the nearest attached container.
    pub fn doc(&self) -> Option<LoroDoc> {
        match self {
            Loc::Container(_, doc) => doc.clone(),
            Loc::Field(parent, _) | Loc::KeyedItem(parent, _) => parent.doc(),
        }
    }

    /// Subscribe at the concrete anchor and filter the complete field path.
    /// Resolving just the immediate parent would pin the subscription to a
    /// stale container after replacement, and cannot watch missing parents.
    fn subscription_target(&self) -> (ContainerID, Vec<Arc<str>>) {
        let mut keys = Vec::new();
        let mut loc = self;
        loop {
            match loc {
                Loc::Container(c, _) => {
                    keys.reverse();
                    return (c.id(), keys);
                }
                Loc::Field(parent, key) | Loc::KeyedItem(parent, key) => {
                    keys.push(key.clone());
                    loc = parent;
                }
            }
        }
    }
}

/// A typed view. Implemented by library views and by generated `XxxModel`s.
pub trait View: Sized {
    fn from_loc(loc: Loc) -> Self;
    fn loc(&self) -> &Loc;

    /// Project this subtree into any owned type. `O(output size)`.
    fn read_as<T: FromLoro>(&self) -> Result<T> {
        self.loc().read()
    }

    /// Reconcile a plain value into this subtree, reusing containers and
    /// skipping unchanged values.
    fn update_from<T: ToLoro + ?Sized>(&self, value: &T) -> Result<()> {
        if let Loc::KeyedItem(_, key) = self.loc() {
            if let Some(found) = value.key() {
                if found != key.as_ref() {
                    return Err(ModelError::new(ErrorKind::KeyMismatch {
                        expected: key.to_string(),
                        found: found.to_string(),
                    }));
                }
            }
        }
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
        let (target, keys) = self.loc().subscription_target();
        let doc = self
            .loc()
            .doc()
            .ok_or_else(|| ModelError::new(ErrorKind::Missing))?;
        Ok(doc.subscribe(
            &target.clone(),
            Arc::new(move |e| {
                if touches(&e, &target, &keys) {
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
        let read = move || -> Result<T> { loc.read() };
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

fn touches(e: &loro::event::DiffEvent, target: &ContainerID, keys: &[Arc<str>]) -> bool {
    if keys.is_empty() {
        return true;
    }
    e.events.iter().any(|d| {
        // Each path segment names the child container and the index by which
        // its parent reaches it. Start *after* the concrete anchor's segment.
        let path = match d.path.iter().position(|(id, _)| id == target) {
            Some(i) => &d.path[i + 1..],
            None if d.target == target => &[],
            None => return false,
        };
        if !path
            .iter()
            .zip(keys)
            .all(|((_, index), key)| matches!(index, Index::Key(k) if k.as_str() == key.as_ref()))
        {
            return false;
        }
        if path.len() >= keys.len() {
            return true; // The watched container or one of its descendants.
        }
        // A change to an ancestor matters only when it updates the next edge
        // of the watched path (including deleting or replacing that edge).
        match &d.diff {
            loro::event::Diff::Map(m) => m.updated.contains_key(keys[path.len()].as_ref()),
            _ => true,
        }
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
            Some(ValueOrContainer::Container(c)) => {
                Some(T::View::from_loc(Loc::Container(c, self.loc.doc())))
            }
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
        Ok(Some(T::View::from_loc(Loc::KeyedItem(
            Arc::new(self.items_loc()),
            key.into(),
        ))))
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
            .ok_or_else(|| ModelError::new(ErrorKind::Missing))?
            .to_string();
        let parts = self.parts_mut()?;
        if parts.items.get(&key).is_some() {
            return Err(ModelError::new(ErrorKind::DuplicateKey(key)));
        }
        let keys = parts.normalize_order()?;
        let raw = index.min(keys.len());
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
        let keys = parts.normalize_order()?;
        let Some(from) = keys.iter().position(|k| k == key) else {
            return Err(ModelError::new(ErrorKind::Missing).in_key(key));
        };
        let to = to.min(keys.len() - 1);
        if from != to {
            parts.order.mov(from, to)?;
        }
        Ok(())
    }
}
