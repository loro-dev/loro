//! The postcard value layout is recursive, but reading it must not use the call
//! stack. Keep the shallow header variants in the same order as LoroValue's
//! binary Serialize implementation in loro-common/src/value.rs.

use loro_common::{ContainerID, LoroError, LoroResult, LoroValue};
use rustc_hash::FxHashMap;
use serde::Deserialize;

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
    let mut stack = vec![Frame::new(read(&mut bytes)?, is_map)];
    loop {
        let frame = stack.last_mut().unwrap();
        if frame.left == 0 {
            let value = stack.pop().unwrap().into_value();
            if let Some(parent) = stack.last_mut() {
                parent.push(value);
                continue;
            }
            return Ok((value, bytes));
        }

        if matches!(frame.collection, Collection::Map(_)) {
            frame.key = Some(read(&mut bytes)?);
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
        frame.push(value);
    }
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
