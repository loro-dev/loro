//! Optional type annotations on JSON operations. Older readers ignore the field
//! and see the same byte arrays. Paths address value slots (slot 0 for scalar
//! operations), then list indices or map keys within that value.
use super::json::{JsonOpContent, ListOp, MapOp, MovableListOp, TextOp};
use loro_common::LoroValue;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum Step<K> {
    Index(usize),
    Key(K),
}

pub(super) fn collect(content: &JsonOpContent) -> Vec<Vec<Step<&str>>> {
    let values: &[LoroValue] = match content {
        JsonOpContent::List(ListOp::Insert { value, .. })
        | JsonOpContent::MovableList(MovableListOp::Insert { value, .. }) => value,
        JsonOpContent::Map(MapOp::Insert { value, .. })
        | JsonOpContent::MovableList(MovableListOp::Set { value, .. })
        | JsonOpContent::Text(TextOp::Mark {
            style_value: value, ..
        }) => std::slice::from_ref(value),
        _ => return Vec::new(),
    };
    fn visit<'a>(
        value: &'a LoroValue,
        step: Step<&'a str>,
        path: &mut Vec<Step<&'a str>>,
        found: &mut Vec<Vec<Step<&'a str>>>,
    ) {
        if !matches!(
            value,
            LoroValue::Binary(_) | LoroValue::Map(_) | LoroValue::List(_)
        ) {
            return;
        }
        path.push(step);
        match value {
            LoroValue::Binary(_) => found.push(path.clone()),
            LoroValue::List(list) => {
                for (index, child) in list.iter().enumerate() {
                    visit(child, Step::Index(index), path, found);
                }
            }
            LoroValue::Map(map) => {
                for (key, child) in map.iter() {
                    visit(child, Step::Key(key), path, found);
                }
            }
            _ => unreachable!(),
        }
        path.pop();
    }
    let mut found = Vec::new();
    let mut path = Vec::new();
    for (index, value) in values.iter().enumerate() {
        visit(value, Step::Index(index), &mut path, &mut found);
    }
    found
}

pub(super) fn restore(
    content: &mut JsonOpContent,
    paths: Vec<Vec<Step<String>>>,
) -> Result<(), &'static str> {
    let values: &mut [LoroValue] = match content {
        JsonOpContent::List(ListOp::Insert { value, .. })
        | JsonOpContent::MovableList(MovableListOp::Insert { value, .. }) => value,
        JsonOpContent::Map(MapOp::Insert { value, .. })
        | JsonOpContent::MovableList(MovableListOp::Set { value, .. })
        | JsonOpContent::Text(TextOp::Mark {
            style_value: value, ..
        }) => std::slice::from_mut(value),
        _ => return Err("binary_paths on an operation without values"),
    };
    for path in paths {
        let Some((Step::Index(slot), nested)) = path.split_first() else {
            return Err("binary path must begin with a value slot");
        };
        let mut value = values.get_mut(*slot).ok_or("unknown binary value slot")?;
        for step in nested {
            value = match (step, value) {
                (Step::Index(i), LoroValue::List(list)) => list.make_mut().get_mut(*i),
                (Step::Key(k), LoroValue::Map(map)) => map.make_mut().get_mut(k),
                _ => None,
            }
            .ok_or("invalid binary path")?;
        }
        let LoroValue::List(list) = value else {
            return Err("expected a byte array at binary path");
        };
        let bytes: Vec<u8> = list
            .iter()
            .map(|v| {
                v.as_i64()
                    .and_then(|i| u8::try_from(*i).ok())
                    .ok_or("invalid byte at binary path")
            })
            .collect::<Result<_, _>>()?;
        *value = LoroValue::Binary(bytes.into());
    }
    Ok(())
}
