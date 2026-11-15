use js_sys::{Array, Object, Reflect, Uint8Array};
use loro_internal::{
    container::idx::ContainerIdx,
    loro_common::{self, ContainerID, ContainerType, LoroValue},
    DocState, FxHashMap, LoroDoc as InnerLoroDoc,
};
use wasm_bindgen::JsValue;

use crate::JsResult;

const MAX_CACHED_KEYS: usize = 4096;

enum TargetParent {
    Array { arr: Array, index: u32 },
    Object { obj: JsValue, key: JsValue },
}

enum WorkItem {
    Container {
        idx: ContainerIdx,
        id: ContainerID,
        shallow_preloaded: Option<LoroValue>,
        parent: TargetParent,
    },
    Plain {
        value: LoroValue,
        parent: TargetParent,
    },
    TreeNodes {
        nodes: LoroValue,
        parent: TargetParent,
    },
}

struct KeyCache {
    map: FxHashMap<String, JsValue>,
}

impl KeyCache {
    fn new() -> Self {
        Self {
            map: FxHashMap::with_capacity_and_hasher(64, Default::default()),
        }
    }

    fn get_or_insert(&mut self, key: &str) -> JsValue {
        if let Some(js) = self.map.get(key) {
            return js.clone();
        }
        let js = JsValue::from_str(key);
        if self.map.len() < MAX_CACHED_KEYS {
            self.map.insert(key.to_owned(), js.clone());
        }
        js
    }
}

#[inline]
fn scalar_to_js_or_none(value: &LoroValue) -> Option<JsValue> {
    Some(match value {
        LoroValue::Null => JsValue::NULL,
        LoroValue::Bool(b) => JsValue::from_bool(*b),
        LoroValue::Double(f) => JsValue::from_f64(*f),
        LoroValue::I64(i) => JsValue::from_f64(*i as f64),
        LoroValue::String(s) => JsValue::from_str(s),
        LoroValue::Binary(binary) => Uint8Array::from(binary.as_slice()).into(),
        LoroValue::Container(id) => JsValue::from_str(&id.to_string()),
        LoroValue::List(_) | LoroValue::Map(_) => return None,
    })
}

#[inline]
fn assign_to_parent(parent: TargetParent, value: JsValue) -> JsResult<()> {
    match parent {
        TargetParent::Array { arr, index } => arr.set(index, value),
        TargetParent::Object { obj, key } => {
            Reflect::set(&obj, &key, &value)?;
        }
    }
    Ok(())
}

#[inline]
fn push_plain_or_assign(
    value: LoroValue,
    parent: TargetParent,
    stack: &mut Vec<WorkItem>,
) -> JsResult<()> {
    if let Some(scalar) = scalar_to_js_or_none(&value) {
        return assign_to_parent(parent, scalar);
    }
    stack.push(WorkItem::Plain { value, parent });
    Ok(())
}

fn expand_tree_nodes(
    nodes: &LoroValue,
    parent: TargetParent,
    state: &DocState,
    stack: &mut Vec<WorkItem>,
    keys: &mut KeyCache,
) -> JsResult<()> {
    let LoroValue::List(list) = nodes else {
        let js = scalar_to_js_or_none(nodes).unwrap_or(JsValue::NULL);
        return assign_to_parent(parent, js);
    };

    let arr = Array::new_with_length(list.len() as u32);
    assign_to_parent(parent, arr.clone().into())?;
    if list.is_empty() {
        return Ok(());
    }

    for (idx, node) in list.iter().enumerate().rev() {
        let LoroValue::Map(map) = node else {
            let item_parent = TargetParent::Array {
                arr: arr.clone(),
                index: idx as u32,
            };
            push_plain_or_assign(node.clone(), item_parent, stack)?;
            continue;
        };

        let obj = Object::new();
        let obj_js: JsValue = obj.into();
        arr.set(idx as u32, obj_js.clone());
        let start = stack.len();
        for (k, child_v) in map.iter() {
            let key_js = keys.get_or_insert(k);
            let child_parent = TargetParent::Object {
                obj: obj_js.clone(),
                key: key_js,
            };
            if k == "meta" {
                if let LoroValue::Container(meta_cid) = child_v {
                    let meta_idx = state.register_container_id(meta_cid);
                    stack.push(WorkItem::Container {
                        idx: meta_idx,
                        id: meta_cid.clone(),
                        shallow_preloaded: None,
                        parent: child_parent,
                    });
                } else {
                    push_plain_or_assign(child_v.clone(), child_parent, stack)?;
                }
            } else if k == "children" {
                stack.push(WorkItem::TreeNodes {
                    nodes: child_v.clone(),
                    parent: child_parent,
                });
            } else {
                push_plain_or_assign(child_v.clone(), child_parent, stack)?;
            }
        }
        stack[start..].reverse();
    }
    Ok(())
}

/// Convert the document state directly into a JS JSON value without allocating an
/// intermediate whole-document deep `LoroValue` tree. Runs strictly iteratively
/// using a heap worklist so arbitrarily nested documents cannot overflow the stack.
pub(crate) fn doc_to_json_js_value(doc: &InnerLoroDoc) -> JsResult<JsValue> {
    let mut state = doc.app_state().lock();
    let mut visible_roots = state.visible_root_shallow_values()?;

    let root_obj = Object::new();
    let root_js: JsValue = root_obj.into();
    if visible_roots.is_empty() {
        return Ok(root_js);
    }

    visible_roots.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

    let mut keys = KeyCache::new();
    let mut stack = Vec::with_capacity(visible_roots.len());
    for (name, idx, id, shallow) in visible_roots.into_iter().rev() {
        let key_js = keys.get_or_insert(name.as_str());
        stack.push(WorkItem::Container {
            idx,
            id,
            shallow_preloaded: Some(shallow),
            parent: TargetParent::Object {
                obj: root_js.clone(),
                key: key_js,
            },
        });
    }

    while let Some(item) = stack.pop() {
        match item {
            WorkItem::Container {
                idx,
                id,
                shallow_preloaded,
                parent,
            } => {
                let kind = id.container_type();
                let shallow = match shallow_preloaded {
                    Some(v) => v,
                    None => state.try_get_container_shallow_value_ephemeral(idx)?,
                };
                match (shallow, kind) {
                    (LoroValue::Map(map), ContainerType::Map) => {
                        let obj = Object::new();
                        let obj_js: JsValue = obj.into();
                        assign_to_parent(parent, obj_js.clone())?;
                        let start = stack.len();
                        for (k, v) in map.iter() {
                            let key_js = keys.get_or_insert(k);
                            let child_parent = TargetParent::Object {
                                obj: obj_js.clone(),
                                key: key_js,
                            };
                            if let Some(child_kind) = loro_common::parse_mergeable_marker(&id, k, v)
                            {
                                let child_cid = ContainerID::new_mergeable(&id, k, child_kind);
                                let child_idx = state.register_container_id(&child_cid);
                                stack.push(WorkItem::Container {
                                    idx: child_idx,
                                    id: child_cid,
                                    shallow_preloaded: None,
                                    parent: child_parent,
                                });
                            } else if let LoroValue::Container(child_cid) = v {
                                let child_idx = state.register_container_id(child_cid);
                                stack.push(WorkItem::Container {
                                    idx: child_idx,
                                    id: child_cid.clone(),
                                    shallow_preloaded: None,
                                    parent: child_parent,
                                });
                            } else {
                                push_plain_or_assign(v.clone(), child_parent, &mut stack)?;
                            }
                        }
                        stack[start..].reverse();
                    }
                    (LoroValue::List(list), ContainerType::List | ContainerType::MovableList) => {
                        let arr = Array::new_with_length(list.len() as u32);
                        assign_to_parent(parent, arr.clone().into())?;
                        for (i, v) in list.iter().enumerate().rev() {
                            let child_parent = TargetParent::Array {
                                arr: arr.clone(),
                                index: i as u32,
                            };
                            if let LoroValue::Container(child_cid) = v {
                                let child_idx = state.register_container_id(child_cid);
                                stack.push(WorkItem::Container {
                                    idx: child_idx,
                                    id: child_cid.clone(),
                                    shallow_preloaded: None,
                                    parent: child_parent,
                                });
                            } else {
                                push_plain_or_assign(v.clone(), child_parent, &mut stack)?;
                            }
                        }
                    }
                    (nodes, ContainerType::Tree) => {
                        expand_tree_nodes(&nodes, parent, &state, &mut stack, &mut keys)?;
                    }
                    (value, _) => {
                        push_plain_or_assign(value, parent, &mut stack)?;
                    }
                }
            }
            WorkItem::Plain { value, parent } => {
                if let Some(scalar) = scalar_to_js_or_none(&value) {
                    assign_to_parent(parent, scalar)?;
                    continue;
                }
                match value {
                    LoroValue::List(list) => {
                        let arr = Array::new_with_length(list.len() as u32);
                        assign_to_parent(parent, arr.clone().into())?;
                        for (i, child) in list.iter().enumerate().rev() {
                            let child_parent = TargetParent::Array {
                                arr: arr.clone(),
                                index: i as u32,
                            };
                            push_plain_or_assign(child.clone(), child_parent, &mut stack)?;
                        }
                    }
                    LoroValue::Map(map) => {
                        let obj = Object::new();
                        let obj_js: JsValue = obj.into();
                        assign_to_parent(parent, obj_js.clone())?;
                        let start = stack.len();
                        for (k, child) in map.iter() {
                            let key_js = keys.get_or_insert(k);
                            let child_parent = TargetParent::Object {
                                obj: obj_js.clone(),
                                key: key_js,
                            };
                            push_plain_or_assign(child.clone(), child_parent, &mut stack)?;
                        }
                        stack[start..].reverse();
                    }
                    _ => unreachable!("scalars already handled above"),
                }
            }
            WorkItem::TreeNodes { nodes, parent } => {
                expand_tree_nodes(&nodes, parent, &state, &mut stack, &mut keys)?;
            }
        }
    }

    Ok(root_js)
}
