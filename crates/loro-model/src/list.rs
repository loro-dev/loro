//! List reconciliation.
//!
//! - Plain `Vec<T>`: positional, into a `LoroList`.
//! - Keyed `Vec<T>` (element type declares a key field): a "keyed vec", stored
//!   as a map with two mergeable children:
//!
//!   ```text
//!   todos: Map {
//!     items: Map { <key>: <element> }   // each element is a mergeable child
//!     order: MovableList [<key>, ...]   // order only; scalar keys
//!   }
//!   ```
//!
//!   `items` makes `by_key` one map lookup, and two peers that create the same
//!   key concurrently write into the same deterministic child container, so the
//!   element's fields merge instead of producing two elements.
//!
//!   `items` decides membership; `order` only orders. Concurrent edits can
//!   leave `order` with a repeated key, a key whose item is gone, or no entry
//!   for a live item (two peers each dedupe a repeated key after a concurrent
//!   move and delete different copies). Readers use the first occurrence, skip
//!   dead keys, and append unordered live items sorted by key; the next
//!   `update_from` writes that order back.

use loro::{
    ContainerRef, ContainerTrait, ContainerType, LoroListValue, LoroMap, LoroValue, ReadEntry,
    StateReader, ValueOrContainer,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::slot::{ListRef, Slot};
use crate::{ErrorKind, FromLoro, ModelError, Result, ToLoro};

pub const ITEMS: &str = "items";
pub const ORDER: &str = "order";

pub fn reconcile_positional<T: ToLoro>(list: &ListRef, items: &[T]) -> Result<()> {
    let old_len = list.len();
    let common = old_len.min(items.len());
    for (i, item) in items[..common].iter().enumerate() {
        item.to_loro(Slot::ListAt(list, i))
            .map_err(|e| e.in_index(i))?;
    }
    if old_len > items.len() {
        list.delete(items.len(), old_len - items.len())?;
    }
    for (i, item) in items.iter().enumerate().skip(old_len) {
        item.to_loro(Slot::ListInsert(list, i))
            .map_err(|e| e.in_index(i))?;
    }
    Ok(())
}

/// The `items` and `order` containers of a keyed vec, creating them if needed.
pub struct KeyedParts {
    pub items: LoroMap,
    pub order: ListRef,
}

impl KeyedParts {
    pub fn ensure(slot: Slot<'_>) -> Result<Self> {
        let root = slot.ensure_map()?;
        let items = Slot::MapKey(&root, ITEMS)
            .ensure_map()
            .map_err(|e| e.in_key(ITEMS))?;
        let order = Slot::MapKey(&root, ORDER)
            .ensure_list(loro::ContainerType::MovableList)
            .map_err(|e| e.in_key(ORDER))?;
        Ok(Self { items, order })
    }

    pub fn get(root: &LoroMap) -> Result<Option<Self>> {
        let items = match root.get(ITEMS) {
            Some(ValueOrContainer::Container(loro::Container::Map(m))) => m,
            None => return Ok(None),
            Some(other) => return Err(ModelError::mismatch("map", other).in_key(ITEMS)),
        };
        let order = match root.get(ORDER) {
            Some(ValueOrContainer::Container(c)) => ListRef::from_container(&c)
                .ok_or_else(|| ModelError::mismatch("list", c.get_type()).in_key(ORDER))?,
            None => return Ok(None),
            Some(other) => return Err(ModelError::mismatch("list", other).in_key(ORDER)),
        };
        Ok(Some(Self { items, order }))
    }

    /// Visible keys in order, as `(raw index in order, key)`. See the module
    /// docs; unordered live items come last, sorted, with no raw index.
    pub fn visible(&self) -> Vec<(Option<usize>, String)> {
        self.visible_in(&raw_keys(&self.order))
    }

    fn visible_in(&self, order: &[LoroValue]) -> Vec<(Option<usize>, String)> {
        let mut live: FxHashSet<String> = self.items.keys().map(|k| k.to_string()).collect();
        let mut out = Vec::with_capacity(live.len());
        for (i, v) in order.iter().enumerate() {
            let LoroValue::String(k) = v else {
                continue;
            };
            if live.remove(k.as_str()) {
                out.push((Some(i), k.to_string()));
            }
        }
        let mut rest: Vec<String> = live.into_iter().collect();
        rest.sort_unstable();
        out.extend(rest.into_iter().map(|k| (None, k)));
        out
    }

    /// Make raw positions agree with visible positions before an indexed edit.
    /// Moving only the first occurrence of a repeated key can expose another
    /// copy at the old position; unordered items also need order entries before
    /// they can be used as insertion anchors. Preserve the current visible order.
    pub(crate) fn normalize_order(&self) -> Result<Vec<String>> {
        let raw = raw_keys(&self.order);
        let keys: Vec<String> = self.visible_in(&raw).into_iter().map(|(_, k)| k).collect();
        let canonical = raw.len() == keys.len()
            && raw
                .iter()
                .zip(&keys)
                .all(|(v, k)| matches!(v, LoroValue::String(s) if s.as_str() == k));
        if !canonical {
            let want: Vec<&str> = keys.iter().map(String::as_str).collect();
            let index = want.iter().enumerate().map(|(i, &k)| (k, i)).collect();
            reconcile_order(&self.order, &want, &index, &raw)?;
        }
        Ok(keys)
    }
}

fn raw_keys(order: &ListRef) -> LoroListValue {
    match order.get_deep_value() {
        LoroValue::List(l) => l,
        _ => LoroListValue::default(),
    }
}

fn container_of<'r>(cur: Option<&ReadEntry<'r>>, ty: ContainerType) -> Option<ContainerRef<'r>> {
    crate::convert::container_of(cur, ty)
}

pub fn reconcile_keyed<T: ToLoro>(slot: Slot<'_>, elems: &[T]) -> Result<()> {
    let parts = KeyedParts::ensure(slot)?;
    let mut want: Vec<&str> = Vec::with_capacity(elems.len());
    let mut want_index: FxHashMap<&str, usize> =
        FxHashMap::with_capacity_and_hasher(elems.len(), Default::default());
    for (i, e) in elems.iter().enumerate() {
        let k = e
            .key()
            .ok_or_else(|| ModelError::new(ErrorKind::Missing).in_index(i))?;
        if want_index.insert(k, i).is_some() {
            return Err(ModelError::new(ErrorKind::DuplicateKey(k.to_string())).in_index(i));
        }
        want.push(k);
    }

    // One read pass under one lock: current item keys, current order, and
    // which elements already hold their input value.
    let doc = parts.items.doc().expect("keyed vec parts are attached");
    let (item_keys, order, unchanged) = doc.read_state(|r| {
        let items = r.container(&parts.items).unwrap();
        let order = parts.order.container_ref(r).unwrap();
        let order: Vec<LoroValue> = r
            .list_entries(order)
            .into_iter()
            .map(|e| match e {
                ReadEntry::Value(v) => v,
                ReadEntry::Container(_) => LoroValue::Null,
            })
            .collect();
        let mut present = 0;
        let unchanged: Vec<bool> = elems
            .iter()
            .zip(&want)
            .map(|(e, k)| {
                let cur = r.map_get(items, k);
                present += cur.is_some() as usize;
                cur.is_some() && e.same_as(r, cur.as_ref())
            })
            .collect();
        // Input keys are distinct, so if every item is one of them there is
        // nothing stale and the key scan can be skipped.
        let item_keys = if r.map_len(items) == present {
            Vec::new()
        } else {
            r.map_keys(items)
        };
        (item_keys, order, unchanged)
    });

    // Items: delete entries that are no longer wanted.
    for k in item_keys {
        if !want_index.contains_key(k.as_str()) {
            parts.items.delete(&k)?;
        }
    }

    // Order: scalar keys, reconciled with moves.
    reconcile_order(&parts.order, &want, &want_index, &order).map_err(|e| e.in_key(ORDER))?;

    // Content: each element lives under its key.
    for (j, e) in elems.iter().enumerate() {
        if !unchanged[j] {
            e.to_loro(Slot::MapKey(&parts.items, want[j]))
                .map_err(|e| e.in_index(j))?;
        }
    }
    Ok(())
}

/// The `items` and `order` containers of a keyed vec read through `r`.
fn keyed_parts_in<'r>(
    r: &mut StateReader<'r>,
    root: ContainerRef<'r>,
) -> Option<(ContainerRef<'r>, Option<ContainerRef<'r>>)> {
    let items = container_of(r.map_get(root, ITEMS).as_ref(), ContainerType::Map)?;
    let order = match r.map_get(root, ORDER) {
        Some(ReadEntry::Container(c))
            if matches!(
                c.container_type(),
                ContainerType::List | ContainerType::MovableList
            ) =>
        {
            Some(c)
        }
        _ => None,
    };
    Some((items, order))
}

/// [`crate::FromLoro::read`] of a keyed vec, in the order defined in the module docs.
pub fn read_keyed_in<'r, T: FromLoro>(
    r: &mut StateReader<'r>,
    root: ContainerRef<'r>,
) -> Result<Vec<T>> {
    let Some((items, order)) = keyed_parts_in(r, root) else {
        return Err(ModelError::mismatch(
            "keyed vec with items and order",
            r.deep_value(root),
        ));
    };
    let mut out = Vec::new();
    let mut seen: FxHashSet<String> = FxHashSet::default();
    if let Some(order) = order {
        for e in r.list_entries(order) {
            let ReadEntry::Value(LoroValue::String(k)) = e else {
                continue;
            };
            let Some(item) = r.map_get(items, &k) else {
                continue;
            };
            if !seen.insert(k.to_string()) {
                continue;
            }
            let i = out.len();
            out.push(T::read(r, item).map_err(|e| e.in_index(i))?);
        }
    }
    if out.len() < r.map_len(items) {
        let mut rest: Vec<String> = r
            .map_keys(items)
            .into_iter()
            .filter(|k| !seen.contains(k.as_str()))
            .map(|k| k.to_string())
            .collect();
        rest.sort_unstable();
        for k in rest {
            let item = r.map_get(items, &k).unwrap();
            let i = out.len();
            out.push(T::read(r, item).map_err(|e| e.in_index(i))?);
        }
    }
    Ok(out)
}

/// [`ToLoro::same_as`] of a keyed vec: same item set, `order` exactly the input
/// keys (no repeats or dead keys), and every item the same.
pub fn keyed_same_as<'r, T: ToLoro>(
    r: &mut StateReader<'r>,
    cur: Option<&ReadEntry<'r>>,
    elems: &[T],
) -> bool {
    let Some(root) = container_of(cur, ContainerType::Map) else {
        return false;
    };
    let Some((items, Some(order))) = keyed_parts_in(r, root) else {
        return false;
    };
    if r.map_len(items) != elems.len() {
        return false;
    }
    let order = r.list_entries(order);
    if order.len() != elems.len() {
        return false;
    }
    elems.iter().zip(&order).all(|(e, o)| {
        let Some(k) = e.key() else { return false };
        if !matches!(o, ReadEntry::Value(LoroValue::String(s)) if s.as_str() == k) {
            return false;
        }
        let item = r.map_get(items, k);
        item.is_some() && e.same_as(r, item.as_ref())
    })
}

/// Make `list` (whose elements have keys `cur`) equal to `want`.
///
/// 1. Delete elements whose key is not wanted, unkeyed elements, and all but
///    the first element of each duplicated key, in runs from the back.
/// 2. Keep a longest increasing subsequence (by old position) in place, then
///    walk `want` backwards: insert new keys and move non-LIS keys to just
///    before the already-final next element.
///
/// Ops: one delete per run of removed elements, one move per element outside
/// the LIS, one insert per new key. Bookkeeping is a `Vec<u32>` simulation:
/// `O(n log n)` plus an `O(n)` memmove/scan per emitted move or insert.
fn reconcile_order(
    list: &ListRef,
    want: &[&str],
    want_index: &FxHashMap<&str, usize>,
    cur: &[LoroValue],
) -> Result<()> {
    let mut seen = vec![false; want.len()];
    let keep: Vec<Option<u32>> = cur
        .iter()
        .map(|k| {
            let LoroValue::String(k) = k else {
                return None;
            };
            let &j = want_index.get(k.as_str())?;
            (!std::mem::replace(&mut seen[j], true)).then_some(j as u32)
        })
        .collect();
    let mut i = keep.len();
    while i > 0 {
        i -= 1;
        if keep[i].is_none() {
            let end = i + 1;
            while i > 0 && keep[i - 1].is_none() {
                i -= 1;
            }
            list.delete(i, end - i)?;
        }
    }
    // `cur[p]` = index into `want` of the element at position p.
    let mut cur: Vec<u32> = keep.into_iter().flatten().collect();

    if cur.is_empty() {
        for (j, k) in want.iter().enumerate() {
            Slot::ListInsert(list, j).set_value((*k).into())?;
        }
        return Ok(());
    }

    let mut old_pos: Vec<Option<usize>> = vec![None; want.len()];
    for (p, &j) in cur.iter().enumerate() {
        old_pos[j as usize] = Some(p);
    }
    let stable = lis_mask(&old_pos);

    // Position of want[j + 1] in `cur`, when known without a scan.
    let mut next_pos: Option<usize> = None;
    for j in (0..want.len()).rev() {
        if stable[j] {
            next_pos = None;
            continue;
        }
        let anchor = if j + 1 == want.len() {
            cur.len()
        } else {
            next_pos.unwrap_or_else(|| position(&cur, j + 1))
        };
        let placed = match old_pos[j] {
            None => {
                Slot::ListInsert(list, anchor).set_value(want[j].into())?;
                cur.insert(anchor, j as u32);
                anchor
            }
            Some(_) => {
                let from = position(&cur, j);
                let to = if from < anchor { anchor - 1 } else { anchor };
                if from != to {
                    list.mov(from, to)?;
                    let v = cur.remove(from);
                    cur.insert(to, v);
                }
                to
            }
        };
        next_pos = Some(placed);
    }
    debug_assert!(cur.iter().enumerate().all(|(p, &j)| p == j as usize));
    Ok(())
}

fn position(cur: &[u32], j: usize) -> usize {
    cur.iter()
        .position(|&x| x as usize == j)
        .expect("keyed reconcile bookkeeping lost a key")
}

/// Mark the entries forming a longest strictly increasing subsequence of the
/// `Some` values. `None` entries are never stable.
fn lis_mask(seq: &[Option<usize>]) -> Vec<bool> {
    // tails[l] = index in `seq` of the smallest tail of an increasing run of length l+1
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; seq.len()];
    for (i, v) in seq.iter().enumerate() {
        let Some(v) = *v else { continue };
        let l = tails.partition_point(|&t| seq[t].unwrap() < v);
        if l > 0 {
            prev[i] = Some(tails[l - 1]);
        }
        if l == tails.len() {
            tails.push(i);
        } else {
            tails[l] = i;
        }
    }
    let mut mask = vec![false; seq.len()];
    let mut cur = tails.last().copied();
    while let Some(i) = cur {
        mask[i] = true;
        cur = prev[i];
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::lis_mask;

    #[test]
    fn lis() {
        let seq = [Some(3), None, Some(0), Some(1), Some(4), Some(2)];
        let mask = lis_mask(&seq);
        let picked: Vec<usize> = seq
            .iter()
            .zip(mask)
            .filter(|(_, m)| *m)
            .map(|(v, _)| v.unwrap())
            .collect();
        assert_eq!(picked.len(), 3);
        assert!(picked.windows(2).all(|w| w[0] < w[1]));
    }
}
