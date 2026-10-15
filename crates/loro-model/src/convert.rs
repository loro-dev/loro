//! The two data-type traits: [`FromLoro`] (projection, `read_as`) and
//! [`ToLoro`] (reconcile, `update_from`), with impls for std types.

use std::collections::{BTreeMap, HashMap};
use std::hash::BuildHasher;

use loro::{ContainerRef, ContainerType, LoroValue, ReadEntry, StateReader, ValueOrContainer};
use rustc_hash::FxHashSet;

use crate::slot::Slot;
use crate::{list, ErrorKind, ModelError, Result};

/// Build an owned value from the document.
///
/// `read_as` calls [`FromLoro::read`] under one `LoroDoc::read_state` lock, so an
/// implementation reads only the fields it needs and nothing is materialized as
/// an intermediate `LoroValue` tree. [`FromLoro::from_loro`] converts an
/// already materialized value (a scalar, or a deep value).
pub trait FromLoro: Sized {
    fn from_loro(v: &LoroValue) -> Result<Self>;

    /// Called when a map field is absent. `Option<T>` and `#[loro(default)]`
    /// fields override this.
    fn from_missing() -> Result<Self> {
        Err(ModelError::new(ErrorKind::Missing))
    }

    /// Read from document state. The default materializes the entry's deep
    /// value; container-shaped types override it.
    fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<Self> {
        match e {
            ReadEntry::Value(v) => Self::from_loro(&v),
            ReadEntry::Container(c) => Self::from_loro(&r.deep_value(c)),
        }
    }
}

/// Write a value into a [`Slot`], reusing existing containers and skipping
/// writes whose value is already stored.
pub trait ToLoro {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()>;

    /// Field that identifies this value inside a list. When set, `Vec<Self>`
    /// is stored as a keyed vec (see [`crate::list`]) and reconciled by key,
    /// with moves.
    const KEY_FIELD: Option<&'static str> = None;

    /// The value of [`ToLoro::KEY_FIELD`] for this element. Keys are map keys in the keyed
    /// vec layout, so they are strings.
    fn key(&self) -> Option<&str> {
        None
    }

    /// Whether the slot already holds this value (`cur` is `None` for an absent
    /// map key), i.e. [`ToLoro::to_loro`] would emit no ops. Reconcile uses it
    /// to compare whole collections under one `read_state` lock before writing.
    ///
    /// `true` must be exact; `false` is always safe and only costs the write
    /// path's own comparisons. The default is `false`.
    fn same_as<'r>(&self, _r: &mut StateReader<'r>, _cur: Option<&ReadEntry<'r>>) -> bool {
        false
    }
}

impl<T: ToLoro + ?Sized> ToLoro for &T {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        (**self).to_loro(slot)
    }

    const KEY_FIELD: Option<&'static str> = T::KEY_FIELD;

    fn key(&self) -> Option<&str> {
        (**self).key()
    }

    fn same_as<'r>(&self, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        (**self).same_as(r, cur)
    }
}

/// `same_as` for a value written with `Slot::set_value`.
fn same_value(cur: Option<&ReadEntry<'_>>, v: &LoroValue) -> bool {
    matches!(cur, Some(ReadEntry::Value(c)) if c == v)
}

/// The container in `cur`, if it is one of type `ty` (what `ensure_container` reuses).
pub fn container_of<'r>(
    cur: Option<&ReadEntry<'r>>,
    ty: ContainerType,
) -> Option<ContainerRef<'r>> {
    match cur {
        Some(ReadEntry::Container(c)) if c.container_type() == ty => Some(*c),
        _ => None,
    }
}

impl FromLoro for LoroValue {
    fn from_loro(v: &LoroValue) -> Result<Self> {
        Ok(v.clone())
    }
}

impl ToLoro for LoroValue {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        slot.set_value(self.clone())
    }

    fn same_as<'r>(&self, _r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        same_value(cur, self)
    }
}

impl FromLoro for bool {
    fn from_loro(v: &LoroValue) -> Result<Self> {
        match v {
            LoroValue::Bool(b) => Ok(*b),
            other => Err(ModelError::mismatch("bool", other)),
        }
    }
}

impl ToLoro for bool {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        slot.set_value(LoroValue::Bool(*self))
    }

    fn same_as<'r>(&self, _r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        same_value(cur, &LoroValue::Bool(*self))
    }
}

macro_rules! int_impl {
    ($($t:ty),*) => {$(
        impl FromLoro for $t {
            fn from_loro(v: &LoroValue) -> Result<Self> {
                match v {
                    LoroValue::I64(i) => <$t>::try_from(*i).map_err(|_| {
                        ModelError::new(ErrorKind::OutOfRange { target: stringify!($t) })
                    }),
                    other => Err(ModelError::mismatch("integer", other)),
                }
            }
        }

        impl ToLoro for $t {
            fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
                // Loro stores integers as i64; u64/usize above i64::MAX are rejected.
                let v = i64::try_from(*self).map_err(|_| {
                    ModelError::new(ErrorKind::OutOfRange { target: "i64" })
                })?;
                slot.set_value(LoroValue::I64(v))
            }

            fn same_as<'r>(&self, _r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
                i64::try_from(*self).is_ok_and(|v| same_value(cur, &LoroValue::I64(v)))
            }
        }
    )*};
}

int_impl!(i8, i16, i32, i64, u8, u16, u32, u64, usize);

impl FromLoro for f64 {
    fn from_loro(v: &LoroValue) -> Result<Self> {
        match v {
            LoroValue::Double(d) => Ok(*d),
            LoroValue::I64(i) => Ok(*i as f64),
            other => Err(ModelError::mismatch("number", other)),
        }
    }
}

impl ToLoro for f64 {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        slot.set_value(LoroValue::Double(*self))
    }

    fn same_as<'r>(&self, _r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        same_value(cur, &LoroValue::Double(*self))
    }
}

impl FromLoro for String {
    fn from_loro(v: &LoroValue) -> Result<Self> {
        match v {
            LoroValue::String(s) => Ok(s.to_string()),
            other => Err(ModelError::mismatch("string", other)),
        }
    }
}

impl ToLoro for String {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        self.as_str().to_loro(slot)
    }

    fn same_as<'r>(&self, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        self.as_str().same_as(r, cur)
    }
}

impl ToLoro for str {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        if let Some(ValueOrContainer::Value(LoroValue::String(s))) = slot.current() {
            if s.as_str() == self {
                return Ok(());
            }
        }
        slot.set_value(LoroValue::from(self))
    }

    fn same_as<'r>(&self, _r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        matches!(cur, Some(ReadEntry::Value(LoroValue::String(s))) if s.as_str() == self)
    }
}

impl<T: FromLoro> FromLoro for Option<T> {
    fn from_loro(v: &LoroValue) -> Result<Self> {
        match v {
            LoroValue::Null => Ok(None),
            v => T::from_loro(v).map(Some),
        }
    }

    fn from_missing() -> Result<Self> {
        Ok(None)
    }

    fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<Self> {
        match e {
            ReadEntry::Value(LoroValue::Null) => Ok(None),
            e => T::read(r, e).map(Some),
        }
    }
}

impl<T: ToLoro> ToLoro for Option<T> {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        match self {
            Some(v) => v.to_loro(slot),
            None => slot.delete(),
        }
    }

    fn same_as<'r>(&self, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        match self {
            Some(v) => v.same_as(r, cur),
            // An absent map key. (A list slot holds `Null` instead; answering
            // `false` there is safe.)
            None => cur.is_none(),
        }
    }
}

impl<T: FromLoro> FromLoro for Vec<T> {
    /// Accepts both list layouts, so a projection type does not need to know
    /// whether the writer used a key.
    fn from_loro(v: &LoroValue) -> Result<Self> {
        let items = match v {
            LoroValue::List(items) => items,
            LoroValue::Map(_) => return read_keyed(v),
            _ => return Err(ModelError::mismatch("list", v)),
        };
        let mut out = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            out.push(T::from_loro(item).map_err(|e| e.in_index(i))?);
        }
        Ok(out)
    }

    fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<Self> {
        let c = match e {
            ReadEntry::Value(v) => return Self::from_loro(&v),
            ReadEntry::Container(c) => c,
        };
        match c.container_type() {
            ContainerType::List | ContainerType::MovableList => {
                let entries = r.list_entries(c);
                let mut out = Vec::with_capacity(entries.len());
                for (i, e) in entries.into_iter().enumerate() {
                    out.push(T::read(r, e).map_err(|e| e.in_index(i))?);
                }
                Ok(out)
            }
            ContainerType::Map => list::read_keyed_in(r, c),
            ty => Err(ModelError::mismatch("list", ty)),
        }
    }
}

/// Read a keyed vec in the order defined in [`crate::list`].
fn read_keyed<T: FromLoro>(v: &LoroValue) -> Result<Vec<T>> {
    let LoroValue::Map(m) = v else {
        return Err(ModelError::mismatch("keyed vec (map)", v));
    };
    let Some(LoroValue::Map(items)) = m.get(list::ITEMS) else {
        return Err(ModelError::mismatch("keyed vec with items", v));
    };
    let mut keys: Vec<&str> = Vec::with_capacity(items.len());
    let mut seen = FxHashSet::default();
    if let Some(LoroValue::List(order)) = m.get(list::ORDER) {
        for k in order.iter() {
            if let LoroValue::String(k) = k {
                if items.contains_key(k.as_str()) && seen.insert(k.as_str()) {
                    keys.push(k.as_str());
                }
            }
        }
    }
    if keys.len() < items.len() {
        let mut rest: Vec<&str> = items
            .keys()
            .map(|k| k.as_str())
            .filter(|k| !seen.contains(k))
            .collect();
        rest.sort_unstable();
        keys.extend(rest);
    }
    let mut out = Vec::with_capacity(keys.len());
    for (i, k) in keys.into_iter().enumerate() {
        out.push(T::from_loro(&items[k]).map_err(|e| e.in_index(i))?);
    }
    Ok(out)
}

impl<T: ToLoro> ToLoro for Vec<T> {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        self.as_slice().to_loro(slot)
    }

    fn same_as<'r>(&self, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        self.as_slice().same_as(r, cur)
    }
}

impl<T: ToLoro> ToLoro for [T] {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        if T::KEY_FIELD.is_some() {
            return list::reconcile_keyed(slot, self);
        }
        let list = slot.ensure_list(ContainerType::List)?;
        list::reconcile_positional(&list, self)
    }

    fn same_as<'r>(&self, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        if T::KEY_FIELD.is_some() {
            return list::keyed_same_as(r, cur, self);
        }
        let Some(ReadEntry::Container(c)) = cur else {
            return false;
        };
        if !matches!(
            c.container_type(),
            ContainerType::List | ContainerType::MovableList
        ) {
            return false;
        }
        let entries = r.list_entries(*c);
        entries.len() == self.len()
            && self
                .iter()
                .zip(&entries)
                .all(|(v, e)| v.same_as(r, Some(e)))
    }
}

fn read_map_in<'r, V: FromLoro>(
    r: &mut StateReader<'r>,
    e: ReadEntry<'r>,
    mut put: impl FnMut(String, V),
) -> Result<()> {
    let m = match e {
        ReadEntry::Value(v) => return read_map(&v, put),
        ReadEntry::Container(c) if c.container_type() == ContainerType::Map => c,
        ReadEntry::Container(c) => return Err(ModelError::mismatch("map", c.container_type())),
    };
    for k in r.map_keys(m) {
        let e = r.map_get(m, &k).unwrap();
        put(k.to_string(), V::read(r, e).map_err(|e| e.in_key(&k))?);
    }
    Ok(())
}

fn map_same_as<'r, 'a, V: ToLoro + 'a>(
    r: &mut StateReader<'r>,
    cur: Option<&ReadEntry<'r>>,
    len: usize,
    mut entries: impl Iterator<Item = (&'a String, &'a V)>,
) -> bool {
    let Some(m) = container_of(cur, ContainerType::Map) else {
        return false;
    };
    r.map_len(m) == len
        && entries.all(|(k, v)| {
            let e = r.map_get(m, k);
            e.is_some() && v.same_as(r, e.as_ref())
        })
}

fn read_map<V: FromLoro>(v: &LoroValue, mut put: impl FnMut(String, V)) -> Result<()> {
    let LoroValue::Map(m) = v else {
        return Err(ModelError::mismatch("map", v));
    };
    for (k, v) in m.iter() {
        put(k.to_string(), V::from_loro(v).map_err(|e| e.in_key(k))?);
    }
    Ok(())
}

/// Collection maps own all their keys: keys missing from the input are deleted.
/// (Struct maps, by contrast, keep unknown keys for forward compatibility.)
fn write_map<'a, V: ToLoro + 'a>(
    slot: Slot<'_>,
    contains: impl Fn(&str) -> bool,
    entries: impl Iterator<Item = (&'a String, &'a V)>,
) -> Result<()> {
    let map = slot.ensure_map()?;
    // Equal lengths do not mean equal key sets, so always look for stale keys.
    if !map.is_empty() {
        let stale: Vec<String> = map
            .keys()
            .filter(|k| !contains(k))
            .map(|k| k.to_string())
            .collect();
        for k in stale {
            map.delete(&k)?;
        }
    }
    for (k, v) in entries {
        v.to_loro(Slot::MapKey(&map, k)).map_err(|e| e.in_key(k))?;
    }
    Ok(())
}

impl<V: FromLoro, S: BuildHasher + Default> FromLoro for HashMap<String, V, S> {
    fn from_loro(v: &LoroValue) -> Result<Self> {
        let mut out = HashMap::default();
        read_map(v, |k, v| {
            out.insert(k, v);
        })?;
        Ok(out)
    }

    fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<Self> {
        let mut out = HashMap::default();
        read_map_in(r, e, |k, v| {
            out.insert(k, v);
        })?;
        Ok(out)
    }
}

impl<V: ToLoro, S: BuildHasher> ToLoro for HashMap<String, V, S> {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        write_map(slot, |k| self.contains_key(k), self.iter())
    }

    fn same_as<'r>(&self, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        map_same_as(r, cur, self.len(), self.iter())
    }
}

impl<V: FromLoro> FromLoro for BTreeMap<String, V> {
    fn from_loro(v: &LoroValue) -> Result<Self> {
        let mut out = BTreeMap::new();
        read_map(v, |k, v| {
            out.insert(k, v);
        })?;
        Ok(out)
    }

    fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<Self> {
        let mut out = BTreeMap::new();
        read_map_in(r, e, |k, v| {
            out.insert(k, v);
        })?;
        Ok(out)
    }
}

impl<V: ToLoro> ToLoro for BTreeMap<String, V> {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        write_map(slot, |k| self.contains_key(k), self.iter())
    }

    fn same_as<'r>(&self, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        map_same_as(r, cur, self.len(), self.iter())
    }
}

/// `#[loro(text)]` fields: a `String` stored in a `LoroText` container.
/// Used as `#[loro(with = "loro_model::text")]`.
pub mod text {
    use loro::UpdateOptions;

    use super::*;

    pub fn from_loro(v: &LoroValue) -> Result<String> {
        String::from_loro(v)
    }

    pub fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<String> {
        match e {
            ReadEntry::Container(c) if c.container_type() == ContainerType::Text => {
                String::from_loro(&r.value(c))
            }
            e => String::read(r, e),
        }
    }

    pub fn same_as<'r>(s: &str, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        container_of(cur, ContainerType::Text)
            .is_some_and(|c| matches!(r.value(c), LoroValue::String(t) if t.as_str() == s))
    }

    pub fn to_loro(s: &str, slot: Slot<'_>) -> Result<()> {
        let text = slot.ensure_text()?;
        if text.to_string() != s {
            text.update(s, UpdateOptions::default())
                .map_err(|_| ModelError::new(ErrorKind::TextUpdateTimeout))?;
        }
        Ok(())
    }
}

/// `#[loro(counter)]` fields: an `f64` stored in a `LoroCounter`. Reconcile
/// increments by the difference, so concurrent updates add up.
pub mod counter {
    use super::*;

    pub fn from_loro(v: &LoroValue) -> Result<f64> {
        f64::from_loro(v)
    }

    pub fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<f64> {
        match e {
            ReadEntry::Container(c) if c.container_type() == ContainerType::Counter => {
                f64::from_loro(&r.value(c))
            }
            e => f64::read(r, e),
        }
    }

    pub fn same_as<'r>(value: f64, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        container_of(cur, ContainerType::Counter)
            .is_some_and(|c| matches!(r.value(c), LoroValue::Double(v) if value - v == 0.0))
    }

    pub fn to_loro(value: f64, slot: Slot<'_>) -> Result<()> {
        let counter = slot.ensure_counter()?;
        let delta = value - counter.get();
        if delta != 0.0 {
            counter.increment(delta)?;
        }
        Ok(())
    }
}

/// The map a struct is stored in, for derive output: `Ok(None)` when the entry
/// is a plain map value, to be read with `from_loro`.
pub fn struct_map<'r>(e: &ReadEntry<'r>) -> Result<Option<ContainerRef<'r>>> {
    match e {
        ReadEntry::Container(c) if c.container_type() == ContainerType::Map => Ok(Some(*c)),
        ReadEntry::Container(c) => Err(ModelError::mismatch("map", c.container_type())),
        ReadEntry::Value(_) => Ok(None),
    }
}

/// Read a struct field through a [`StateReader`]; used by derive output.
pub fn read_field_in<'r, T>(
    r: &mut StateReader<'r>,
    map: ContainerRef<'r>,
    key: &str,
    read: impl FnOnce(&mut StateReader<'r>, ReadEntry<'r>) -> Result<T>,
    missing: impl FnOnce() -> Result<T>,
) -> Result<T> {
    match r.map_get(map, key) {
        Some(e) => read(r, e),
        None => missing(),
    }
    .map_err(|e| e.in_key(key))
}

/// `same_as` of one struct field; used by derive output.
pub fn field_same_as<'r>(
    r: &mut StateReader<'r>,
    map: ContainerRef<'r>,
    key: &str,
    same: impl FnOnce(&mut StateReader<'r>, Option<&ReadEntry<'r>>) -> bool,
) -> bool {
    let e = r.map_get(map, key);
    same(r, e.as_ref())
}

/// Read a struct field from a deep map value; used by derive output.
pub fn read_field<T>(
    map: &loro::LoroMapValue,
    key: &str,
    read: impl FnOnce(&LoroValue) -> Result<T>,
    missing: impl FnOnce() -> Result<T>,
) -> Result<T> {
    match map.get(key) {
        Some(v) => read(v),
        None => missing(),
    }
    .map_err(|e| e.in_key(key))
}
