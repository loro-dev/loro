//! The two data-type traits: [`FromLoro`] (projection, `read_as`) and
//! [`ToLoro`] (reconcile, `update_from`), with impls for std types.

use std::collections::{BTreeMap, HashMap};
use std::hash::BuildHasher;

use loro::{ContainerType, LoroValue, ValueOrContainer};
use rustc_hash::FxHashSet;

use crate::slot::Slot;
use crate::{list, ErrorKind, ModelError, Result};

/// Build an owned value from a Loro deep value.
///
/// Projection works on `LoroValue` rather than on handlers: `read_as` takes one
/// `get_deep_value()` of the subtree and converts it, instead of taking the
/// document lock once per field.
pub trait FromLoro: Sized {
    fn from_loro(v: &LoroValue) -> Result<Self>;

    /// Called when a map field is absent. `Option<T>` and `#[loro(default)]`
    /// fields override this.
    fn from_missing() -> Result<Self> {
        Err(ModelError::new(ErrorKind::Missing))
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

    /// The value of [`ToLoro::KEY_FIELD`] for this element. Must be a string.
    fn key(&self) -> Option<LoroValue> {
        None
    }
}

impl<T: ToLoro + ?Sized> ToLoro for &T {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        (**self).to_loro(slot)
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
}

impl<T: ToLoro> ToLoro for Option<T> {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        match self {
            Some(v) => v.to_loro(slot),
            None => slot.delete(),
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
}

impl<T: ToLoro> ToLoro for [T] {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        if T::KEY_FIELD.is_some() {
            return list::reconcile_keyed(slot, self);
        }
        let list = slot.ensure_list(ContainerType::List)?;
        list::reconcile_positional(&list, self)
    }
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
    len: usize,
    contains: impl Fn(&str) -> bool,
    entries: impl Iterator<Item = (&'a String, &'a V)>,
) -> Result<()> {
    let map = slot.ensure_map()?;
    if map.len() > len {
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
}

impl<V: ToLoro, S: BuildHasher> ToLoro for HashMap<String, V, S> {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        write_map(slot, self.len(), |k| self.contains_key(k), self.iter())
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
}

impl<V: ToLoro> ToLoro for BTreeMap<String, V> {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        write_map(slot, self.len(), |k| self.contains_key(k), self.iter())
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

    pub fn to_loro(value: f64, slot: Slot<'_>) -> Result<()> {
        let counter = slot.ensure_counter()?;
        let delta = value - counter.get();
        if delta != 0.0 {
            counter.increment(delta)?;
        }
        Ok(())
    }
}

/// Read a struct field from a deep map value; used by derive output.
pub fn read_field<T: FromLoro>(
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
