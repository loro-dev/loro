use fxhash::FxHashSet;
use loro_common::{ContainerType, LoroResult};
use serde_json::Value;
use thiserror::Error;

use crate::{
    handler::{self, Handler},
    ListHandler, LoroDoc, MovableListHandler, TreeParentId,
};

#[derive(Error, Debug)]
pub enum JsonInitError {
    #[error("Failed to initialize container from JSON: {0}")]
    ContainerCreation(String),
    #[error("Invalid jsonpath configuration: {0}")]
    PathValidationError(String),
}

impl LoroDoc {
    #[inline]
    /// Initializes a new [LoroDoc] from a JSON value and a list of [PathMapping]s.
    /// mappings use JSONPath syntax to map a JSON value to a container.
    /// ```
    pub fn try_from_json(
        json: serde_json::Value,
        mappings: &[PathMapping],
    ) -> Result<Self, JsonInitError> {
        let mut doc = Self::new_auto_commit();
        initialize_from_json(&mut doc, &json, mappings)?;
        Ok(doc)
    }
}

#[derive(Debug, Clone)]
pub struct PathMapping {
    path: String,
    container_type: ContainerType,
}

impl PathMapping {
    pub fn new(path: impl Into<String>, container_type: ContainerType) -> Self {
        Self {
            path: path.into(),
            container_type,
        }
    }
}

/// Validates the path mappings before initializing a LoroDoc from JSON
fn validate_path_mappings(path_mappings: &[PathMapping]) -> Result<(), JsonInitError> {
    let mut path_set = FxHashSet::default();

    for path_mapping in path_mappings {
        if path_set.contains(&path_mapping.path) {
            return Err(JsonInitError::PathValidationError(format!(
                "duplicate path mapping {}",
                path_mapping.path
            )));
        }

        path_set.insert(path_mapping.path.clone());
    }

    Ok(())
}

// NOTE: my assumption is that if the text is at the root level we should use a container
// othwerwhise we should default to a regular string
fn infer_container_type(value: &Value, is_root: bool) -> Option<ContainerType> {
    match (value, is_root) {
        (Value::Object(_), _) => Some(ContainerType::Map),
        (Value::Array(_), _) => Some(ContainerType::List),
        (Value::String(_), true) => Some(ContainerType::Text),
        _ => None,
    }
}

fn get_root_container_type(
    key: &str,
    value: &Value,
    mappings: &[PathMapping],
) -> Option<ContainerType> {
    let this_mapping = format!("$.{}", key);

    let mapping = mappings.iter().find(|m| m.path == this_mapping);

    mapping
        .map(|m| m.container_type)
        .or_else(|| infer_container_type(value, true))
}

fn create_root_container(
    doc: &mut LoroDoc,
    name: &str,
    container_type: ContainerType,
) -> Result<Handler, JsonInitError> {
    match container_type {
        ContainerType::Map => Ok(Handler::Map(doc.get_map(name))),
        ContainerType::List => Ok(Handler::List(doc.get_list(name))),
        ContainerType::MovableList => Ok(Handler::MovableList(doc.get_movable_list(name))),
        ContainerType::Text => Ok(Handler::Text(doc.get_text(name))),
        ContainerType::Tree => Ok(Handler::Tree(doc.get_tree(name))),
        #[cfg(feature = "counter")]
        ContainerType::Counter => Ok(Handler::Counter(doc.get_counter(name))),
        ContainerType::Unknown(_) => Err(JsonInitError::ContainerCreation(format!(
            "Unknown container type: {:?}",
            container_type
        ))),
    }
}

trait GenericListContainer {
    fn insert_new_container(&mut self, index: usize, container: Handler) -> LoroResult<Handler>;
    fn insert_new(&mut self, index: usize, value: serde_json::Value) -> LoroResult<()>;
}

impl GenericListContainer for ListHandler {
    fn insert_new_container(&mut self, index: usize, container: Handler) -> LoroResult<Handler> {
        self.insert_container(index, container)
    }

    fn insert_new(&mut self, index: usize, value: serde_json::Value) -> LoroResult<()> {
        self.insert(index, value)
    }
}

impl GenericListContainer for MovableListHandler {
    fn insert_new_container(&mut self, index: usize, container: Handler) -> LoroResult<Handler> {
        self.insert_container(index, container)
    }

    fn insert_new(&mut self, index: usize, value: serde_json::Value) -> LoroResult<()> {
        self.insert(index, value)
    }
}

fn initialize_list_container(
    doc: &mut LoroDoc,
    mappings: &[PathMapping],
    container: &mut impl GenericListContainer,
    value: &Value,
    path: &str,
) -> Result<(), JsonInitError> {
    let list_value = value.as_array().ok_or(JsonInitError::ContainerCreation(
        "list value to be array".to_string(),
    ))?;

    let list_path_wildcard = format!("{}[*]", path);

    let wildcard_container_type: Option<ContainerType> = mappings
        .iter()
        .find(|m| m.path == list_path_wildcard)
        .map(|m| m.container_type);

    for (index, value) in list_value.iter().enumerate() {
        let index_path = format!("{}[{}]", path, index);

        let container_type = wildcard_container_type
            .or_else(|| {
                mappings
                    .iter()
                    .find(|m| m.path == index_path)
                    .map(|m| m.container_type)
            })
            .or_else(|| infer_container_type(value, false));

        if let Some(container_type) = container_type {
            let mut sub_container = container
                .insert_new_container(index, handler::Handler::new_unattached(container_type))
                .map_err(|e| JsonInitError::ContainerCreation(e.to_string()))?;

            initialize_container(doc, mappings, &mut sub_container, &index_path, value)?;
        } else {
            container
                .insert_new(index, value.clone())
                .map_err(|e| JsonInitError::ContainerCreation(e.to_string()))?;
        }
    }

    Ok(())
}

fn initialize_map_container(
    doc: &mut LoroDoc,
    mappings: &[PathMapping],
    container: &mut handler::MapHandler,
    value: &Value,
    path: &str,
) -> Result<(), JsonInitError> {
    let map_value = value.as_object().ok_or(JsonInitError::ContainerCreation(
        "map value to be object".to_string(),
    ))?;

    let map_path_wildcard = format!("{}[*]", path);

    let wildcard_container_type: Option<ContainerType> = mappings
        .iter()
        .find(|m| m.path == map_path_wildcard)
        .map(|m| m.container_type);

    for (key, value) in map_value {
        let key_path = format!("{}.{}", path, key);

        let container_type = wildcard_container_type
            .or_else(|| {
                mappings
                    .iter()
                    .find(|m| m.path == key_path)
                    .map(|m| m.container_type)
            })
            .or_else(|| infer_container_type(value, false));

        if let Some(container_type) = container_type {
            let mut sub_container = container
                .insert_container(key, handler::Handler::new_unattached(container_type))
                .map_err(|e| JsonInitError::ContainerCreation(e.to_string()))?;
            initialize_container(doc, mappings, &mut sub_container, &key_path, value)?;
        } else {
            container.insert(key, value.clone()).map_err(|e| {
                JsonInitError::ContainerCreation(format!("cannot insert key {}", key))
            })?;
        }
    }
    Ok(())
}

fn initialize_text_container(
    container: &mut handler::TextHandler,
    value: &Value,
) -> Result<(), JsonInitError> {
    let value = value.as_str().ok_or_else(|| {
        JsonInitError::ContainerCreation("Cannot convert value to string".to_string())
    })?;

    container
        .update(value, Default::default())
        .map_err(|e| JsonInitError::ContainerCreation(e.to_string()))?;
    Ok(())
}

fn initialize_tree_container(
    doc: &mut LoroDoc,
    mappings: &[PathMapping],
    container: &mut handler::TreeHandler,
    root_path: &str,
    value: &Value,
    tree_parent_id: Option<TreeParentId>,
) -> Result<(), JsonInitError> {
    let tree_parent_id = tree_parent_id.unwrap_or(TreeParentId::Root);
    let tree_value = value.as_object().ok_or(JsonInitError::ContainerCreation(
        "tree value to be object".to_string(),
    ))?;

    let node_id = match tree_parent_id {
        TreeParentId::Root => {
            if container.roots().is_empty() {
                container
                    .create(TreeParentId::Root)
                    .map_err(|_| JsonInitError::ContainerCreation("root node".to_string()))?
            } else {
                container
                    .roots()
                    .into_iter()
                    .next()
                    .ok_or(JsonInitError::ContainerCreation("root node".to_string()))?
            }
        }
        TreeParentId::Node(parent_id) => container
            .create(TreeParentId::Node(parent_id))
            .map_err(|e| JsonInitError::ContainerCreation(e.to_string()))?,
        _ => panic!("Cannot create tree node with parent: {:?}", tree_parent_id),
    };

    let node_meta = container
        .get_meta(node_id)
        .map_err(|_| JsonInitError::ContainerCreation("node meta data".to_string()))?;

    for (key, value) in tree_value {
        if key == "children" {
            let children = value.as_array().ok_or(JsonInitError::ContainerCreation(
                "children to be array".to_string(),
            ))?;

            for child in children.iter() {
                initialize_tree_container(
                    doc,
                    mappings,
                    container,
                    root_path,
                    child,
                    Some(TreeParentId::Node(node_id)),
                )?;
            }
        } else {
            let key_path = format!("{}.{}", root_path, key);
            let container_type: Option<ContainerType> = mappings
                .iter()
                .find(|m| m.path == key_path)
                .map(|m| m.container_type)
                .or_else(|| infer_container_type(value, false));

            if let Some(container_type) = container_type {
                let mut sub_container = node_meta
                    .insert_container(key, handler::Handler::new_unattached(container_type))
                    .map_err(|e| JsonInitError::ContainerCreation(e.to_string()))?;
                initialize_container(doc, mappings, &mut sub_container, &key_path, value)?;
            } else {
                node_meta.insert(key, value.clone()).map_err(|_| {
                    JsonInitError::ContainerCreation(format!("cannot insert key {}", key))
                })?;
            }
        }
    }

    Ok(())
}

/// Initializes a container with a JSON value
fn initialize_container(
    doc: &mut LoroDoc,
    mappings: &[PathMapping],
    container: &mut Handler,
    path: &str,
    value: &Value,
) -> Result<(), JsonInitError> {
    match container {
        Handler::List(list) => initialize_list_container(doc, mappings, list, value, path),
        Handler::MovableList(list) => initialize_list_container(doc, mappings, list, value, path),
        Handler::Map(map) => initialize_map_container(doc, mappings, map, value, path),
        Handler::Tree(tree) => initialize_tree_container(doc, mappings, tree, path, value, None),
        Handler::Text(text) => initialize_text_container(text, value),
        _ => unimplemented!(),
    }
    .map_err(|e| JsonInitError::ContainerCreation(e.to_string()))?;

    Ok(())
}

pub fn initialize_from_json(
    doc: &mut LoroDoc,
    json_value: &serde_json::Value,
    mappings: &[PathMapping],
) -> Result<(), JsonInitError> {
    validate_path_mappings(mappings)?;

    let root_object = json_value
        .as_object()
        .ok_or(JsonInitError::ContainerCreation(
            "root to be object".to_string(),
        ))?;

    for (key, value) in root_object {
        let container_type = get_root_container_type(key, value, mappings);

        match container_type {
            Some(container_type) => {
                let path = format!("$.{}", key);
                let mut container = create_root_container(doc, key, container_type)?;
                initialize_container(doc, mappings, &mut container, &path, value)?;
            }
            None => {
                panic!("Cannot infer container type for key {}", key);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{jsoninit::PathMapping, HandlerTrait, LoroDoc, ToJson, TreeParentId};
    use loro_common::ContainerType;
    use serde_json::json;

    #[test]
    fn test_basic_initialization() {
        let json = json!({
            "map": {
                "key": "value"
            },
            "list": [1, 2, 3],
            "movable_list": [1, 2, 3],
        });

        let mappings = vec![
            PathMapping::new("$.map", ContainerType::Map),
            PathMapping::new("$.list", ContainerType::List),
            PathMapping::new("$.movable_list", ContainerType::MovableList),
        ];

        let mut doc = LoroDoc::new_auto_commit();
        initialize_from_json(&mut doc, &json, &mappings).unwrap();

        let map_container = doc.get_map("map");
        assert_eq!(
            map_container.get_value().to_json_value(),
            json!({"key": "value"})
        );

        let moveable_list_container = doc.get_movable_list("movable_list");
        assert_eq!(
            moveable_list_container.get_value().to_json_value(),
            json!([1, 2, 3])
        );

        let list_container = doc.get_list("list");
        assert_eq!(list_container.get_value().to_json_value(), json!([1, 2, 3]));
    }

    #[test]
    fn test_container_type_identification() {
        let mappings = vec![
            PathMapping::new("$.map", ContainerType::Map),
            PathMapping::new("$.list", ContainerType::List),
            PathMapping::new("$.movable_list", ContainerType::MovableList),
            PathMapping::new("$.text", ContainerType::Text),
        ];

        assert_eq!(
            get_root_container_type(
                "map",
                &json!({
                    "map": {
                        "key": "value"
                    }
                }),
                &mappings
            ),
            Some(ContainerType::Map)
        );

        assert_eq!(
            get_root_container_type("list", &json!([1, 2, 3]), &mappings),
            Some(ContainerType::List)
        );

        assert_eq!(
            get_root_container_type("movable_list", &json!([1, 2, 3]), &mappings),
            Some(ContainerType::MovableList)
        );

        assert_eq!(
            get_root_container_type("text", &json!("hello world"), &mappings),
            Some(ContainerType::Text)
        );
    }

    #[test]
    fn test_nested_containers() {
        let json = json!({
            "root": {
                "map": {
                    "list": [
                        {
                            "key": "value"
                        }
                    ]
                }
            }
        });

        let mappings = vec![
            PathMapping::new("$.root", ContainerType::Map),
            PathMapping::new("$.root.map", ContainerType::Map),
            PathMapping::new("$.root.map.list", ContainerType::List),
            PathMapping::new("$.root.map.list[*]", ContainerType::Map),
        ];

        let mut loro_doc = LoroDoc::new_auto_commit();

        initialize_from_json(&mut loro_doc, &json, &mappings).unwrap();

        let root = loro_doc.get_map("root");

        assert!(!root.is_empty(), "root map should have items");
        assert!(
            root.get("map").unwrap().as_container().is_some(),
            "root.map should be a container"
        );

        let map_handler = root.get_child_handler("map").unwrap();
        let map_container = map_handler.as_map().unwrap();

        assert!(!map_container.is_empty(), "map container should have items");

        assert!(
            map_container.get("list").unwrap().as_container().is_some(),
            "map.list should be a container"
        );

        let list_container = map_container.get_child_handler("list").unwrap();
        let list_container = list_container.as_list().unwrap();

        let list_map_item = list_container.get(0).unwrap();
        assert!(list_map_item.as_container().is_some());
    }

    #[test]
    fn test_wildcard_container() {
        let json = json!({
            "2d": [
                [1, 2, 3],
                [4, 5, 6],
                [7, 8, 9]
            ]
        });

        let mappings = vec![
            PathMapping::new("$.2d", ContainerType::MovableList),
            PathMapping::new("$.2d[*]", ContainerType::MovableList),
        ];

        let mut loro_doc = LoroDoc::new_auto_commit();

        initialize_from_json(&mut loro_doc, &json, &mappings).unwrap();

        let movable_list_container = loro_doc.get_movable_list("2d");

        assert_eq!(movable_list_container.len(), 3);

        let list_container = movable_list_container.get_child_handler(0).unwrap();
        let list_container = list_container.as_movable_list().unwrap();

        assert_eq!(list_container.len(), 3);
    }

    #[test]
    fn test_without_mappings() {
        let json = json!({
            "map": {
                "key": "value"
            },
            "list": [1, 2, 3],
            "text": "hello world"
        });

        let mappings = vec![];

        let mut doc = LoroDoc::new_auto_commit();
        initialize_from_json(&mut doc, &json, &mappings).unwrap();

        let map_container = doc.get_map("map");
        assert_eq!(
            map_container.get_value().to_json_value(),
            json!({"key": "value"})
        );

        let list_container = doc.get_list("list");
        assert_eq!(list_container.get_value().to_json_value(), json!([1, 2, 3]));

        let text_container = doc.get_text("text");
        dbg!(doc.get_value());

        assert!(!text_container.is_empty());
    }

    #[test]
    fn test_tree() {
        let json = json!({
            "root": {
                "children": [
                    {
                        "name": "child1",
                        "children": [
                            {
                                "name": "child2"
                            }
                        ]
                    }
                ]
            }
        });

        let mappings = vec![
            PathMapping::new("$.root", ContainerType::Tree),
            PathMapping::new("$.root.children[*].name", ContainerType::Text),
        ];

        let mut loro_doc = LoroDoc::new_auto_commit();

        initialize_from_json(&mut loro_doc, &json, &mappings).unwrap();

        let tree = loro_doc.get_tree("root");
        let root = tree.roots().into_iter().next().unwrap();
        let child = tree
            .children(&TreeParentId::Node(root))
            .unwrap_or_default()
            .into_iter()
            .next()
            .unwrap();

        let child_meta = tree.get_meta(child).unwrap();
        assert!(
            child_meta
                .get("name")
                .unwrap()
                .as_string()
                .unwrap()
                .to_string()
                == "child1"
        );

        let granchildren = tree
            .children(&TreeParentId::Node(child))
            .unwrap_or_default();
        assert!(granchildren.len() == 1);

        let grandchild_id = granchildren.into_iter().next().unwrap();
        let grandchild_meta = tree.get_meta(grandchild_id).unwrap();
        assert!(
            grandchild_meta
                .get("name")
                .unwrap()
                .as_string()
                .unwrap()
                .to_string()
                == "child2"
        );
    }
}
