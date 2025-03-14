#[cfg(feature = "counter")]
use crate::handler::counter::CounterHandler;
use crate::{
    handler::{Handler, UpdateOptions, ValueOrHandler},
    jsonpath::{parse_jsonpath, JsonPathError},
    ListHandler, LoroDoc, MapHandler, MovableListHandler, TextHandler, ToJson, TreeHandler,
};
use fxhash::FxHashSet;
use loro_common::{ContainerType, LoroValue};
use serde_json::Value as JsonValue;
use std::{any::Any, str::FromStr};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum PathValidationError {
    #[error("Duplicate container definition at path {path:?}")]
    DuplicateContainer { path: String },
    #[error("Assigned path mapping to non existent container: {path:?}")]
    BoundsError { path: String },
    #[error("Invalid JSONPath: {0}")]
    InvalidPath(String),
}

#[derive(Error, Debug)]
pub enum JsonInitError {
    /// When a container cannot be created
    #[error("Error creating container: {0}")]
    ContainerCreationError(String),

    /// When data cannot be converted to the appropriate format
    #[error("Error converting data: {0}")]
    DataConversionError(String),

    /// Error from the JSONPath parser
    #[error("JSONPath error: {0}")]
    JsonPathError(#[from] JsonPathError),

    /// Error from the JSON parser
    #[error("JSON parsing error: {0}")]
    JsonParseError(#[from] serde_json::Error),

    /// When the JSON data contains a null value
    #[error("{0}")]
    ValidationError(#[from] PathValidationError),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl LoroDoc {
    #[inline]
    pub fn try_from_json(
        json: impl Into<JsonValue>,
        path_mappings: Vec<PathMapping>,
    ) -> Result<Self, JsonInitError> {
        let mut doc = Self::new();
        doc.start_auto_commit();
        let json = json.into();
        initialize_from_json(&mut doc, &json, path_mappings.as_slice())?;
        Ok(doc)
    }
}

/// Defines a mapping between a JSONPath and a Loro container
///
/// A `PathMapping` describes how a section of JSON data should be
/// converted to a specific Loro container type.
///
/// # Fields
///
/// * `name` - The container name in the Loro document
/// * `path` - The JSONPath to where the container data is located in the JSON
/// * `container_type` - The type of Loro container to create
#[derive(Debug, Clone)]
pub struct PathMapping {
    /// The name of the container
    name: String,
    /// The JSONpath to the container
    path: String,
    /// The type of the container
    container_type: ContainerType,
}

impl PathMapping {
    pub fn new(
        name: impl Into<String>,
        path: impl Into<String>,
        container_type: ContainerType,
    ) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            container_type,
        }
    }
}

/// Returns the parent path of any given JSONPath.
fn get_parent_path(path: &str) -> Result<&str, JsonInitError> {
    let tokens = parse_jsonpath(path)?;

    if tokens.len() < 2 {
        return Err(JsonInitError::ValidationError(
            PathValidationError::InvalidPath(
                "Path must have at least a root and one segment".to_string(),
            ),
        ));
    }

    if let Some(last_dot) = path.rfind('.') {
        Ok(&path[0..last_dot])
    } else if let Some(last_bracket) = path.rfind('[') {
        Ok(&path[0..last_bracket])
    } else {
        Ok("$")
    }
}

/// Sorts the `PathMapping`s topologically by their paths.
fn topologically_sort_paths(paths: &[PathMapping]) -> Vec<&PathMapping> {
    let mut sorted_paths = paths.iter().collect::<Vec<_>>();
    sorted_paths.sort_by_key(|p| p.path.matches(['.', '[']).count());
    sorted_paths
}

/// Checks if a path has nested containers within the provided mappings.
fn has_nested_containers(path: &str, paths: &[PathMapping]) -> bool {
    paths
        .iter()
        .any(|p| p.path != path && p.path.starts_with(path))
}

/// Extracts non-container data from a JSON value at a given path.
fn get_non_container_data(
    path: &str,
    paths: &[PathMapping],
    data: &JsonValue,
) -> Result<Option<JsonValue>, JsonInitError> {
    let json_data = extract_json_at_path(data, path)?;

    match json_data {
        JsonValue::Object(obj) => {
            let mut non_container_map = serde_json::Map::new();

            for (key, value) in obj {
                let key_path = if path == "$" {
                    format!("$.{}", key)
                } else {
                    format!("{}.{}", path, key)
                };

                let is_container = paths.iter().any(|p| p.path == key_path);

                if !is_container {
                    non_container_map.insert(key.clone(), value.clone());
                }
            }

            if non_container_map.is_empty() {
                return Ok(None);
            }

            Ok(Some(JsonValue::Object(non_container_map)))
        }
        JsonValue::Array(arr) => {
            let mut non_container_array: Vec<JsonValue> = Vec::new();

            for (idx, value) in arr.iter().enumerate() {
                let idx_path = format!("{}[{}]", path, idx);

                let is_container = paths.iter().any(|p| p.path == idx_path);

                if !is_container {
                    non_container_array.push(value.clone());
                }
            }

            if non_container_array.is_empty() {
                return Ok(None);
            }

            Ok(Some(JsonValue::Array(non_container_array)))
        }
        _ => Ok(None),
    }
}

/// Creates a new detached [Handler] based on the [ContainerType].
fn create_container(container_type: ContainerType) -> Result<Handler, JsonInitError> {
    match container_type {
        ContainerType::Map => Ok(Handler::Map(MapHandler::new_detached())),
        ContainerType::List => Ok(Handler::List(ListHandler::new_detached())),
        ContainerType::MovableList => Ok(Handler::MovableList(MovableListHandler::new_detached())),
        ContainerType::Text => Ok(Handler::Text(TextHandler::new_detached())),
        ContainerType::Tree => Ok(Handler::Tree(TreeHandler::new_detached())),
        #[cfg(feature = "counter")]
        ContainerType::Counter => Ok(Handler::Counter(CounterHandler::new_detached())),
        ContainerType::Unknown(_) => Err(JsonInitError::Unknown(format!(
            "Unknown container type: {:?}",
            container_type
        ))),
    }
}

/// Creates a root container in a LoroDoc.
fn insert_root_container(
    doc: &mut LoroDoc,
    name: &str,
    container_type: ContainerType,
) -> Result<(), JsonInitError> {
    match container_type {
        ContainerType::Map => {
            doc.get_map(name);
        }
        ContainerType::List => {
            doc.get_list(name);
        }
        ContainerType::MovableList => {
            doc.get_movable_list(name);
        }
        ContainerType::Text => {
            doc.get_text(name);
        }
        ContainerType::Tree => {
            doc.get_tree(name);
        }
        #[cfg(feature = "counter")]
        ContainerType::Counter => {
            doc.get_counter(name);
        }
        ContainerType::Unknown(_) => {
            return Err(JsonInitError::Unknown(format!(
                "Unknown container type: {:?}",
                container_type
            )));
        }
    };

    Ok(())
}

/// Inserts a container into a parent container.
fn insert_container(
    parent: &mut Handler,
    name: &str,
    container: Handler,
) -> Result<(), JsonInitError> {
    match parent {
        Handler::Map(map) => {
            map.insert_container(name, container)
                .map_err(|e| JsonInitError::ContainerCreationError(e.to_string()))?;
        }
        Handler::List(list) => {
            if let Ok(index) = name.parse::<usize>() {
                list.insert_container(index, container)
                    .map_err(|e| JsonInitError::ContainerCreationError(e.to_string()))?;
            } else {
                list.push_container(container)
                    .map_err(|e| JsonInitError::ContainerCreationError(e.to_string()))?;
            }
        }
        Handler::MovableList(list) => {
            if let Ok(index) = name.parse::<usize>() {
                list.insert_container(index, container)
                    .map_err(|e| JsonInitError::ContainerCreationError(e.to_string()))?;
            } else {
                list.push_container(container)
                    .map_err(|e| JsonInitError::ContainerCreationError(e.to_string()))?;
            }
        }
        _ => {
            return Err(JsonInitError::ContainerCreationError(format!(
                "Cannot insert container into handler type: {:?}",
                parent
            )))
        }
    };

    Ok(())
}

/// Extracts a JSON value at the specified JSONPath.
fn extract_json_at_path(
    json: &JsonValue,
    path: impl AsRef<str>,
) -> Result<JsonValue, JsonInitError> {
    use jsonpath_rust::{JsonPath, JsonPathValue};

    let json_path = JsonPath::from_str(path.as_ref()).map_err(|_| {
        JsonInitError::ValidationError(PathValidationError::InvalidPath(format!(
            "Invalid JSONPath: {}",
            path.as_ref()
        )))
    })?;

    let result = json_path.find_slice(json);
    if result.is_empty() {
        return Err(JsonInitError::ValidationError(
            PathValidationError::BoundsError {
                path: path.as_ref().to_string(),
            },
        ));
    }

    match &result[0] {
        JsonPathValue::Slice(value, _) => Ok(value.to_owned().to_owned()),
        JsonPathValue::NewValue(value) => Ok(value.clone()),
        JsonPathValue::NoValue => Err(JsonInitError::ValidationError(
            PathValidationError::BoundsError {
                path: path.as_ref().to_string(),
            },
        )),
    }
}

/// Fills a container with data from a JSON value.
fn fill_container(
    handler: &mut Handler,
    data: &JsonValue,
    path: &str,
) -> Result<(), JsonInitError> {
    if data.is_null() {
        return Err(JsonInitError::ValidationError(
            PathValidationError::BoundsError {
                path: path.to_string(),
            },
        ));
    }

    match (handler, data) {
        (Handler::Map(map), JsonValue::Object(obj)) => {
            for (k, v) in obj {
                if v.is_null() {
                    return Err(JsonInitError::ValidationError(
                        PathValidationError::BoundsError {
                            path: format!("{}.{}", path, k),
                        },
                    ));
                }
                let loro_value = LoroValue::from_json(&v.to_string());
                map.insert(k, loro_value)
                    .map_err(|e| JsonInitError::DataConversionError(e.to_string()))?;
            }
        }
        (Handler::List(list), JsonValue::Array(arr)) => {
            for (i, v) in arr.iter().enumerate() {
                if v.is_null() {
                    return Err(JsonInitError::ValidationError(
                        PathValidationError::BoundsError {
                            path: format!("{}.[{}]", path, i),
                        },
                    ));
                }
                let loro_value = LoroValue::from_json(&v.to_string());
                list.push(loro_value)
                    .map_err(|e| JsonInitError::DataConversionError(e.to_string()))?;
            }
        }
        (Handler::MovableList(list), JsonValue::Array(arr)) => {
            for (i, v) in arr.iter().enumerate() {
                if v.is_null() {
                    return Err(JsonInitError::ValidationError(
                        PathValidationError::BoundsError {
                            path: format!("{}.[{}]", path, i),
                        },
                    ));
                }
                let loro_value = LoroValue::from_json(&v.to_string());
                list.push(loro_value)
                    .map_err(|e| JsonInitError::DataConversionError(e.to_string()))?;
            }
        }
        (Handler::Text(text), JsonValue::String(s)) => {
            text.update(s, UpdateOptions::default())
                .map_err(|e| JsonInitError::DataConversionError(e.to_string()))?;
        }
        (Handler::Tree(_), _) => {
            // Cannot directly fill a tree with a JSON object
            return Err(JsonInitError::DataConversionError(
                "Cannot fill a tree with a JSON object".to_string(),
            ));
        }
        #[cfg(feature = "counter")]
        (Handler::Counter(_), _) => {
            // Cannot directly fill a counter with a JSON number
            // Counter should only be initialized as an empty handler without a value
            return Err(JsonInitError::DataConversionError(
                "Cannot fill a counter with a JSON number".to_string(),
            ));
        }
        _ => {
            return Err(JsonInitError::DataConversionError(format!(
                "Mismatched container type and data type: {:?}",
                data.type_id()
            )))
        }
    }

    Ok(())
}

fn validate_path_mappings(path_mappings: &[PathMapping]) -> Result<(), JsonInitError> {
    // Check for duplicates
    let mut path_set = FxHashSet::default();

    for path_mapping in path_mappings {
        if !path_set.insert(&path_mapping.path) {
            return Err(JsonInitError::ValidationError(
                PathValidationError::DuplicateContainer {
                    path: path_mapping.path.clone(),
                },
            ));
        }
    }

    Ok(())
}

#[derive(Debug)]
enum ParentContainers<'c> {
    Root(&'c mut LoroDoc),
    Handlers(Vec<ValueOrHandler>),
}

/// Initializes a LoroDoc from JSON using provided path mappings.
///
/// This function takes a JSON structure and a set of path mappings that define
/// which parts of the JSON should be treated as which types of Loro containers.
/// It creates the container hierarchy and populates the containers with data.
///
/// The initialization process happens in two phases:
/// 1. Create all containers in topological order (parents before children)
/// 2. Fill all containers with their data
///
/// # Arguments
///
/// * `doc` - The LoroDoc to initialize
/// * `json_value` - The JSON data to initialize from
/// * `path_mappings` - Array of path mappings defining container structure
///
/// # Returns
///
/// * `Ok(())` - If initialization was successful
/// * `Err(JsonInitError)` - If there was an error during initialization
///
/// # Example
///
/// ```ignore
/// let mut doc = LoroDoc::new();
/// doc.start_auto_commit();
/// let json = json!({ "users": [{ "name": "John" }] });
/// let mappings = vec![
///     PathMapping::new("users", "$.users", ContainerType::List),
///     PathMapping::new("user0", "$.users[0]", ContainerType::Map),
/// ];
/// initialize_from_json(&mut doc, &json, &mappings)?;
/// ```
pub fn initialize_from_json(
    doc: &mut LoroDoc,
    json_value: &JsonValue,
    path_mappings: &[PathMapping],
) -> Result<(), JsonInitError> {
    validate_path_mappings(path_mappings)?;

    let sorted_paths = topologically_sort_paths(path_mappings);

    // Create / initialize all containers in topological order
    for path_mapping in &sorted_paths {
        let parent_containers = match get_parent_path(path_mapping.path.as_str()) {
            Ok("$") => ParentContainers::Root(doc), // Root-level container
            Ok(parent_path) => {
                let parent_containers = doc.jsonpath(parent_path)?;
                ParentContainers::Handlers(parent_containers)
            }
            Err(e) => return Err(e),
        };

        match parent_containers {
            ParentContainers::Root(doc) => {
                insert_root_container(doc, &path_mapping.name, path_mapping.container_type)?;
            }
            ParentContainers::Handlers(parent_containers) => {
                for parent in parent_containers {
                    if let ValueOrHandler::Handler(mut handler) = parent {
                        let name = if let Some(last_dot) = path_mapping.path.rfind('.') {
                            &path_mapping.path[last_dot + 1..]
                        } else if let Some(last_bracket) = path_mapping.path.rfind('[') {
                            let bracket_end = path_mapping
                                .path
                                .rfind(']')
                                .unwrap_or(path_mapping.path.len());
                            &path_mapping.path[last_bracket + 1..bracket_end]
                        } else {
                            &path_mapping.name
                        };

                        let container = create_container(path_mapping.container_type)?;
                        insert_container(&mut handler, name, container)?;
                    }
                }
            }
        }
    }

    // Fill containers with data
    for path_mapping in &sorted_paths {
        let non_container_data = if has_nested_containers(&path_mapping.path, path_mappings) {
            get_non_container_data(&path_mapping.path, path_mappings, json_value)?
        } else {
            Some(extract_json_at_path(json_value, &path_mapping.path)?)
        };

        if let Some(non_container_data) = non_container_data {
            let containers = doc.jsonpath(&path_mapping.path)?;
            if containers.is_empty() {
                continue;
            }

            for container in containers {
                if let ValueOrHandler::Handler(mut handler) = container {
                    fill_container(&mut handler, &non_container_data, &path_mapping.path)?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{handler::MapHandler, ListHandler, MovableListHandler, TextHandler};
    use serde_json::json;

    #[test]
    fn test_topological_sort() {
        let paths = vec![
            PathMapping::new("a.a.a", "$.a.a.a", ContainerType::Map),
            PathMapping::new("c", "$.c", ContainerType::Map),
            PathMapping::new("b.b.b", "$.b.b.b", ContainerType::Map),
            PathMapping::new("b.b", "$.b.b", ContainerType::Map),
            PathMapping::new("a.a", "$.a.a", ContainerType::Map),
            PathMapping::new("c.c", "$.c.c", ContainerType::Map),
            PathMapping::new("b", "$.b", ContainerType::Map),
            PathMapping::new("c.c.c", "$.c.c.c", ContainerType::Map),
            PathMapping::new("a", "$.a", ContainerType::Map),
            PathMapping::new("d", "$.d", ContainerType::Map),
        ];

        let sorted = topologically_sort_paths(&paths);

        let parent_before_child = |parent: &str, child: &str| {
            let parent_idx = sorted.iter().position(|p| p.path == parent).unwrap();
            let child_idx = sorted.iter().position(|p| p.path == child).unwrap();
            assert!(
                parent_idx < child_idx,
                "{} should come before {}",
                parent,
                child
            );
        };

        parent_before_child("$.a", "$.a.a");
        parent_before_child("$.a.a", "$.a.a.a");
        parent_before_child("$.b", "$.b.b");
        parent_before_child("$.b.b", "$.b.b.b");
        parent_before_child("$.c", "$.c.c");
        parent_before_child("$.c.c", "$.c.c.c");
    }

    #[test]
    fn test_get_non_container_data() {
        let json = json!({
            // this is a LoroMap
            "nested": {
                "map": {},
                "basic_text": "hello",
                // this is LoroText
                "loro_text": "hello",
                // this is a LoroMap
                "nested_map": {
                    "key": "value",
                    // this is a LoroList
                    "nested_list": [1, 2, 3],
                    "normal_list": [1, 2, 3]
                },
            }
        });

        let mappings = vec![
            PathMapping::new("nested", "$.nested", ContainerType::Map),
            PathMapping::new(
                "nested.map.nested_list",
                "$.nested.map.nested_list",
                ContainerType::List,
            ),
            PathMapping::new(
                "nested.loro_text",
                "$.nested.loro_text",
                ContainerType::Text,
            ),
            PathMapping::new(
                "nested.nested_map",
                "$.nested.nested_map",
                ContainerType::Map,
            ),
            PathMapping::new(
                "nested.nested_map.nested_list",
                "$.nested.nested_map.nested_list",
                ContainerType::List,
            ),
        ];

        let non_container_data =
            get_non_container_data("$.nested", mappings.as_slice(), &json).unwrap();

        assert_eq!(
            non_container_data,
            Some(json!({
                "basic_text": "hello",
                "map": {},
            })),
            "no container data should be extracted"
        );

        let non_container_data_nested =
            get_non_container_data("$.nested.nested_map", mappings.as_slice(), &json).unwrap();

        assert_eq!(
            non_container_data_nested,
            Some(json!({
                "key": "value",
                "normal_list": [1, 2, 3]
            })),
            "no container data should be extracted"
        );
    }

    #[test]
    fn test_initialize_from_json() {
        let json = json!({
            "map": {
                "key": "value"
            },
            "list": [1, 2, 3],
            "movable_list": [1, 2, 3],
            "text": "Hello, world!"
        });

        let mappings = vec![
            PathMapping::new("map", "$.map", ContainerType::Map),
            PathMapping::new("list", "$.list", ContainerType::List),
            PathMapping::new("movable_list", "$.movable_list", ContainerType::MovableList),
            PathMapping::new("text", "$.text", ContainerType::Text),
        ];

        let doc = LoroDoc::try_from_json(json, mappings).unwrap();

        let map_value = doc.get_map("map");
        assert_eq!(
            map_value.get("key").unwrap().to_json(),
            "\"value\"",
            "map value should be extracted"
        );
        assert_eq!(map_value.len(), 1);

        let list_value = doc.get_list("list");
        assert_eq!(list_value.len(), 3, "list value should be extracted");

        let movable_list_value = doc.get_movable_list("movable_list");
        assert_eq!(
            movable_list_value.len(),
            3,
            "movable list value should be extracted"
        );

        let text_value = doc.get_text("text");
        assert_eq!(
            text_value.to_string(),
            "Hello, world!",
            "text value should be extracted"
        );
    }

    #[test]
    fn test_nested_containers() {
        let json = json!({
            "nested": {
                "map": {
                    "key": "value",
                    "nested_list": [1, 2, 3]
                },
                "list": [1, 2, 3],
                "movable_list": [1, 2, 3],
                "text": "Hello, world!"
            }
        });

        let mappings = vec![
            PathMapping::new("nested", "$.nested", ContainerType::Map),
            PathMapping::new("nested.map", "$.nested.map", ContainerType::Map),
            PathMapping::new(
                "nested.map.nested_list",
                "$.nested.map.nested_list",
                ContainerType::List,
            ),
            PathMapping::new("nested.list", "$.nested.list", ContainerType::List),
            PathMapping::new(
                "nested.movable_list",
                "$.nested.movable_list",
                ContainerType::MovableList,
            ),
            PathMapping::new("nested.text", "$.nested.text", ContainerType::Text),
        ];

        let doc = LoroDoc::try_from_json(json, mappings).unwrap();

        let nested = doc.get_map("nested");
        let nested_map = nested
            .get_or_create_container("map", MapHandler::new_detached())
            .unwrap();
        assert_eq!(nested_map.get("key").unwrap().to_json(), "\"value\"");
        assert_eq!(nested_map.len(), 2);

        let nested_map_nested_list = nested_map
            .get_or_create_container("nested_list", ListHandler::new_detached())
            .unwrap();

        assert_eq!(nested_map_nested_list.len(), 3);

        let nested_list = nested
            .get_or_create_container("list", ListHandler::new_detached())
            .unwrap();
        assert_eq!(nested_list.len(), 3);

        let nested_movable_list = nested
            .get_or_create_container("movable_list", MovableListHandler::new_detached())
            .unwrap();

        assert_eq!(nested_movable_list.len(), 3);

        let nested_text = nested
            .get_or_create_container("text", TextHandler::new_detached())
            .unwrap();
        assert_eq!(nested_text.to_string(), "Hello, world!");
    }

    #[test]
    fn test_null_value_handling() {
        let json = json!({
            "map": {
                "key": null
            },
            "list": [1, null, 3]
        });

        let mappings = vec![
            PathMapping::new("map", "$.map", ContainerType::Map),
            PathMapping::new("list", "$.list", ContainerType::List),
        ];

        let result = LoroDoc::try_from_json(json, mappings);
        assert!(result.is_err(), "Should reject null values");
        assert_eq!(
            result.unwrap_err().to_string(),
            PathValidationError::BoundsError {
                path: "$.map.key".to_string()
            }
            .to_string()
        );
    }

    #[test]
    fn test_duplicate_paths() {
        let json = json!({
            "a": { "value": 1 },
            "b": { "value": 2 }
        });

        let mappings = vec![
            PathMapping::new("same_name", "$.a", ContainerType::Map),
            PathMapping::new("same_name", "$.a", ContainerType::Map),
        ];

        let result = LoroDoc::try_from_json(json, mappings);
        assert!(result.is_err(), "Should reject duplicate container names");
        assert_eq!(
            result.unwrap_err().to_string(),
            PathValidationError::DuplicateContainer {
                path: "$.a".to_string()
            }
            .to_string()
        );
    }

    #[test]
    fn test_array_index_validation() {
        let json = json!({
            "array": [1, 2, 3]
        });

        let mappings = vec![
            PathMapping::new("array", "$.array", ContainerType::List),
            PathMapping::new("out_of_bounds", "$.array[5]", ContainerType::Map),
        ];

        let result = LoroDoc::try_from_json(json, mappings);
        assert!(result.is_err(), "Should reject out of bounds array index");
        assert_eq!(
            result.unwrap_err().to_string(),
            PathValidationError::BoundsError {
                path: "$.array[5]".to_string()
            }
            .to_string()
        );
    }
}
