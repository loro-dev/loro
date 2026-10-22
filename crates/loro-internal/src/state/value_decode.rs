//! The postcard value layout is recursive, but reading it must not use the call
//! stack. Keep the shallow header variants in the same order as LoroValue's
//! binary Serialize implementation in loro-common/src/value.rs.

use loro_common::{ContainerID, LoroError, LoroResult, LoroValue};
use rustc_hash::FxHashMap;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
enum Header {
    Null,
    Bool(bool),
    Double(f64),
    I32(i64),
    String(String),
    List(usize),
    Map(usize),
    Container(ContainerID),
    Binary(Vec<u8>),
}

fn read<'a, T: Deserialize<'a>>(bytes: &mut &'a [u8]) -> LoroResult<T> {
    let (value, rest) = postcard::take_from_bytes(bytes).map_err(|err| {
        LoroError::DecodeError(format!("Decode snapshot value failed: {err}").into_boxed_str())
    })?;
    *bytes = rest;
    Ok(value)
}

enum Collection {
    List(Vec<LoroValue>),
    Map(FxHashMap<String, LoroValue>),
}

struct Frame {
    left: usize,
    collection: Collection,
    key: Option<String>,
}

impl Frame {
    fn new(left: usize, is_map: bool) -> Self {
        Self {
            left,
            // Do not reserve an untrusted length before reading any elements.
            collection: if is_map {
                Collection::Map(FxHashMap::default())
            } else {
                Collection::List(Vec::new())
            },
            key: None,
        }
    }

    fn push(&mut self, value: LoroValue) {
        match &mut self.collection {
            Collection::List(list) => list.push(value),
            Collection::Map(map) => {
                map.insert(self.key.take().unwrap(), value);
            }
        }
        self.left -= 1;
    }

    fn into_value(self) -> LoroValue {
        match self.collection {
            Collection::List(list) => LoroValue::List(list.into()),
            Collection::Map(map) => LoroValue::Map(map.into()),
        }
    }
}

/// Read a bare postcard map/list followed by the remaining state metadata.
pub(super) fn decode_collection(mut bytes: &[u8], is_map: bool) -> LoroResult<(LoroValue, &[u8])> {
    let root = Frame::new(read(&mut bytes)?, is_map);
    decode(bytes, Some(root))
}

/// Read a bare postcard map directly into the ordered lazy-value cache.
/// Nested values still use heap frames; never reserve an untrusted map length.
pub(super) fn decode_btree_map(
    mut bytes: &[u8],
) -> LoroResult<(BTreeMap<String, LoroValue>, &[u8])> {
    let len: usize = read(&mut bytes)?;
    let mut map = BTreeMap::new();
    for _ in 0..len {
        let key = read(&mut bytes)?;
        let (value, rest) = decode_value(bytes)?;
        bytes = rest;
        map.insert(key, value);
    }
    Ok((map, bytes))
}

/// Read one enum-tagged value embedded in an internal binary field.
pub(super) fn decode_value(mut bytes: &[u8]) -> LoroResult<(LoroValue, &[u8])> {
    let value = match read::<Header>(&mut bytes)? {
        Header::Null => LoroValue::Null,
        Header::Bool(v) => LoroValue::Bool(v),
        Header::Double(v) => LoroValue::Double(v),
        Header::I32(v) => LoroValue::I64(v),
        Header::String(v) => LoroValue::String(v.into()),
        Header::Container(v) => LoroValue::Container(v),
        Header::Binary(v) => LoroValue::Binary(v.into()),
        Header::List(len) => return decode(bytes, Some(Frame::new(len, false))),
        Header::Map(len) => return decode(bytes, Some(Frame::new(len, true))),
    };
    Ok((value, bytes))
}

fn decode(mut bytes: &[u8], root: Option<Frame>) -> LoroResult<(LoroValue, &[u8])> {
    let mut stack: Vec<_> = root.into_iter().collect();
    loop {
        if let Some(frame) = stack.pop_if(|frame| frame.left == 0) {
            let value = frame.into_value();
            if let Some(parent) = stack.last_mut() {
                parent.push(value);
                continue;
            }
            return Ok((value, bytes));
        }

        if let Some(frame) = stack.last_mut() {
            if matches!(frame.collection, Collection::Map(_)) {
                frame.key = Some(read(&mut bytes)?);
            }
        }
        let value = match read::<Header>(&mut bytes)? {
            Header::Null => LoroValue::Null,
            Header::Bool(v) => LoroValue::Bool(v),
            Header::Double(v) => LoroValue::Double(v),
            Header::I32(v) => LoroValue::I64(v),
            Header::String(v) => LoroValue::String(v.into()),
            Header::Container(v) => LoroValue::Container(v),
            Header::Binary(v) => LoroValue::Binary(v.into()),
            Header::List(len) => {
                stack.push(Frame::new(len, false));
                continue;
            }
            Header::Map(len) => {
                stack.push(Frame::new(len, true));
                continue;
            }
        };
        if let Some(frame) = stack.last_mut() {
            frame.push(value);
        } else {
            return Ok((value, bytes));
        }
    }
}

/// Write the same postcard enum layout without recursing through child values.
pub(super) fn encode_value<W: std::io::Write>(
    value: &LoroValue,
    mut writer: W,
) -> postcard::Result<()> {
    enum Task<'a> {
        Value(&'a LoroValue),
        Key(&'a str),
    }
    let mut pending = vec![Task::Value(value)];
    while let Some(task) = pending.pop() {
        let value = match task {
            Task::Value(value) => value,
            Task::Key(key) => {
                postcard::to_io(key, &mut writer)?;
                continue;
            }
        };
        let tag: u32 = match value {
            LoroValue::Null => 0,
            LoroValue::Bool(_) => 1,
            LoroValue::Double(_) => 2,
            LoroValue::I64(_) => 3,
            LoroValue::String(_) => 4,
            LoroValue::List(_) => 5,
            LoroValue::Map(_) => 6,
            LoroValue::Container(_) => 7,
            LoroValue::Binary(_) => 8,
        };
        postcard::to_io(&tag, &mut writer)?;
        match value {
            LoroValue::Null => {}
            LoroValue::Bool(v) => {
                postcard::to_io(v, &mut writer)?;
            }
            LoroValue::Double(v) => {
                postcard::to_io(v, &mut writer)?;
            }
            LoroValue::I64(v) => {
                postcard::to_io(v, &mut writer)?;
            }
            LoroValue::String(v) => {
                postcard::to_io(&**v, &mut writer)?;
            }
            LoroValue::Container(v) => {
                postcard::to_io(v, &mut writer)?;
            }
            LoroValue::Binary(v) => {
                postcard::to_io(&**v, &mut writer)?;
            }
            LoroValue::List(v) => {
                postcard::to_io(&v.len(), &mut writer)?;
                pending.extend(v.iter().rev().map(Task::Value));
            }
            LoroValue::Map(v) => {
                postcard::to_io(&v.len(), &mut writer)?;
                let start = pending.len();
                for (key, value) in v.iter() {
                    pending.push(Task::Key(key));
                    pending.push(Task::Value(value));
                }
                pending[start..].reverse();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loro_common::{ContainerType, ID};

    #[test]
    fn postcard_layout_and_remaining_metadata() {
        let values = vec![
            LoroValue::Null,
            true.into(),
            1.25.into(),
            i64::MIN.into(),
            "a string".into(),
            LoroValue::Binary(vec![0, 128, 255].into()),
            ContainerID::new_root("text", ContainerType::Text).into(),
            ContainerID::new_normal(ID::new(42, 5), ContainerType::Map).into(),
            LoroValue::List(vec![1.into(), LoroValue::List(vec![].into())].into()),
            LoroValue::Map(vec![("key".into(), "value".into())].into()),
        ];
        for value in &values {
            let mut encoded = Vec::new();
            encode_value(value, &mut encoded).unwrap();
            assert_eq!(encoded, postcard::to_stdvec(value).unwrap());
            encoded.extend([99, 100]);
            let (decoded, rest) = decode_value(&encoded).unwrap();
            assert_eq!(&decoded, value);
            assert_eq!(rest, &[99, 100]);
        }
        let mut bytes = postcard::to_stdvec(&values).unwrap();
        bytes.extend([99, 100]);
        let (decoded, rest) = decode_collection(&bytes, false).unwrap();
        assert_eq!(decoded, LoroValue::List(values.clone().into()));
        assert_eq!(rest, &[99, 100]);
        let map: FxHashMap<_, _> = values
            .into_iter()
            .enumerate()
            .map(|(i, value)| (i.to_string(), value))
            .collect();
        let bytes = postcard::to_stdvec(&map).unwrap();
        let (decoded, rest) = decode_collection(&bytes, true).unwrap();
        assert_eq!(decoded, LoroValue::Map(map.into()));
        assert!(rest.is_empty());
    }

    #[test]
    fn ordered_map_layout_duplicate_keys_and_remaining_metadata() {
        let values = vec![
            LoroValue::Null,
            true.into(),
            1.25.into(),
            i64::MIN.into(),
            "a string".into(),
            LoroValue::Binary(vec![0, 128, 255].into()),
            ContainerID::new_root("text", ContainerType::Text).into(),
            ContainerID::new_normal(ID::new(42, 5), ContainerType::Map).into(),
            LoroValue::List(vec![1.into(), LoroValue::List(vec![].into())].into()),
            LoroValue::Map(vec![("nested".into(), "value".into())].into()),
        ];
        let expected: BTreeMap<_, _> = values
            .into_iter()
            .enumerate()
            .map(|(i, value)| (format!("key{i}"), value))
            .collect();
        let mut bytes = postcard::to_stdvec(&expected).unwrap();
        bytes.extend([99, 100]);
        let (decoded, rest) = decode_btree_map(&bytes).unwrap();
        assert_eq!(decoded, expected);
        assert_eq!(rest, &[99, 100]);
        let (old, old_rest): (BTreeMap<String, LoroValue>, _) =
            postcard::take_from_bytes(&bytes).unwrap();
        assert_eq!(decoded, old);
        assert_eq!(rest, old_rest);

        let mut duplicate = postcard::to_stdvec(&2usize).unwrap();
        for value in [1, 2] {
            duplicate.extend(postcard::to_stdvec("same").unwrap());
            encode_value(&LoroValue::I64(value), &mut duplicate).unwrap();
        }
        let (decoded, rest) = decode_btree_map(&duplicate).unwrap();
        assert_eq!(decoded, BTreeMap::from([("same".into(), 2.into())]));
        assert!(rest.is_empty());
        let old: BTreeMap<String, LoroValue> = postcard::from_bytes(&duplicate).unwrap();
        assert_eq!(decoded, old);
        let (empty, rest) = decode_btree_map(&[0, 99]).unwrap();
        assert!(empty.is_empty());
        assert_eq!(rest, &[99]);
    }

    #[test]
    fn ordered_map_rejects_truncated_values_and_untrusted_lengths() {
        let map = BTreeMap::from([
            ("first".to_string(), LoroValue::from("a complete string")),
            (
                "second".to_string(),
                LoroValue::List(vec![1.into(), true.into()].into()),
            ),
        ]);
        let bytes = postcard::to_stdvec(&map).unwrap();
        for end in 0..bytes.len() {
            assert!(decode_btree_map(&bytes[..end]).is_err(), "prefix {end}");
        }
        assert_eq!(decode_btree_map(&bytes).unwrap().0, map);
        // A huge declared size with no entries must fail on input, without an
        // allocation proportional to that size or a loop over absent entries.
        let huge_len = postcard::to_stdvec(&usize::MAX).unwrap();
        assert!(decode_btree_map(&huge_len).is_err());
        assert!(decode_btree_map(&[255; 20]).is_err());
        // One valid key followed by an invalid enum tag must return an error.
        let mut invalid = postcard::to_stdvec(&1usize).unwrap();
        invalid.extend(postcard::to_stdvec("key").unwrap());
        invalid.push(255);
        assert!(decode_btree_map(&invalid).is_err());
    }

    #[test]
    fn ordered_map_deep_values_and_error_cleanup_use_bounded_stack() {
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(|| {
                let mut bytes = postcard::to_stdvec(&1usize).unwrap();
                bytes.extend(postcard::to_stdvec("value").unwrap());
                bytes.extend_from_slice(&deep_bytes(20_000)[1..]);
                let (map, rest) = decode_btree_map(&bytes).unwrap();
                assert!(rest.is_empty());
                assert_eq!(map["value"].get_depth(), 20_000);
                drop(map);
                // Keep a fully decoded deep sibling alive until the next entry
                // fails, exercising both returned-map and partial-map cleanup.
                bytes[0] = 2;
                let complete_entry_len = bytes.len();
                bytes.extend(postcard::to_stdvec("broken").unwrap());
                bytes.extend([6, 1, 1, b'a', 255]);
                assert!(decode_btree_map(&bytes).is_err());
                // Replacing a duplicate key must also drop its previous deep
                // value without recursing, before a later entry can fail.
                bytes.truncate(complete_entry_len);
                bytes.extend(postcard::to_stdvec("value").unwrap());
                encode_value(&LoroValue::I64(7), &mut bytes).unwrap();
                let (map, rest) = decode_btree_map(&bytes).unwrap();
                assert_eq!(map, BTreeMap::from([("value".into(), 7.into())]));
                assert!(rest.is_empty());
                bytes[0] = 3;
                bytes.extend(postcard::to_stdvec("broken").unwrap());
                bytes.push(255);
                assert!(decode_btree_map(&bytes).is_err());
            })
            .unwrap()
            .join()
            .unwrap();
    }

    fn deep_bytes(depth: usize) -> Vec<u8> {
        let mut bytes = vec![1]; // a bare one-element list
        for i in 0..depth {
            if i % 2 == 0 {
                bytes.extend([6, 1, 1, b'a']);
            } else {
                bytes.extend([5, 1]);
            }
        }
        bytes.extend([3, 0]); // I64(0)
        bytes
    }

    #[test]
    fn deeply_nested_values_and_error_cleanup_use_bounded_stack() {
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(|| {
                let mut bytes = deep_bytes(20_000);
                let (value, rest) = decode_collection(&bytes, false).unwrap();
                assert!(rest.is_empty());
                assert_eq!(value.get_depth(), 20_001);
                drop(value);
                // A completed deep sibling must also be safely dropped on failure.
                bytes[0] = 2;
                bytes.push(255);
                assert!(decode_collection(&bytes, false).is_err());
                assert!(decode_collection(&[255], true).is_err());
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn generic_serde_depth_is_bounded_without_resetting_at_each_child() {
        for tag in [5, 6] {
            for depth in [128, 129, 20_000] {
                let mut bytes = Vec::new();
                for _ in 0..depth {
                    bytes.extend([tag, 1]);
                    if tag == 6 {
                        bytes.extend([1, b'a']);
                    }
                }
                bytes.extend([3, 0]);
                let result = postcard::from_bytes::<LoroValue>(&bytes);
                assert_eq!(result.is_ok(), depth <= 128);
            }
        }
        let siblings = vec![LoroValue::List(vec![].into()); 300];
        let bytes = postcard::to_stdvec(&LoroValue::List(siblings.into())).unwrap();
        assert!(postcard::from_bytes::<LoroValue>(&bytes).is_ok());
    }
}
