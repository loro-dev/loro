use loro_common::{ContainerID, LoroValue};
use thiserror::Error;
use tracing::trace;

use crate::handler::{
    Handler, ListHandler, MapHandler, MovableListHandler, TextHandler, TreeHandler, ValueOrHandler,
};

use crate::LoroDoc;
use std::ops::ControlFlow;

#[derive(Error, Debug)]
pub enum JsonPathError {
    #[error("Invalid JSONPath: {0}")]
    InvalidJsonPath(String),
    #[error("JSONPath evaluation error: {0}")]
    EvaluationError(String),
}

impl LoroDoc {
    #[inline]
    pub fn jsonpath(&self, jsonpath: &str) -> Result<Vec<ValueOrHandler>, JsonPathError> {
        evaluate_jsonpath(self, jsonpath)
    }
}

// Define JSONPath tokens
pub enum JSONPathToken {
    Root,
    Child(String),
    RecursiveDescend,
    Wildcard,
    Index(isize),
    UnionIndex(Vec<isize>),
    UnionKey(Vec<String>),
    Slice(Option<isize>, Option<isize>, Option<isize>),
    Filter(Box<dyn for<'a> Fn(&'a ValueOrHandler) -> bool>),
}

use std::fmt;

impl fmt::Debug for JSONPathToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JSONPathToken::Root => write!(f, "Root"),
            JSONPathToken::Child(s) => write!(f, "Child({})", s),
            JSONPathToken::RecursiveDescend => write!(f, "RecursiveDescend"),
            JSONPathToken::Wildcard => write!(f, "Wildcard"),
            JSONPathToken::Index(i) => write!(f, "Index({})", i),
            JSONPathToken::Slice(start, end, step) => {
                write!(f, "Slice({:?}, {:?}, {:?})", start, end, step)
            }
            JSONPathToken::UnionIndex(indices) => write!(f, "UnionIndex({:?})", indices),
            JSONPathToken::UnionKey(keys) => write!(f, "UnionKey({:?})", keys),
            JSONPathToken::Filter(_) => write!(f, "Filter(<function>)"),
        }
    }
}

impl PartialEq for JSONPathToken {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JSONPathToken::Root, JSONPathToken::Root) => true,
            (JSONPathToken::Child(a), JSONPathToken::Child(b)) => a == b,
            (JSONPathToken::RecursiveDescend, JSONPathToken::RecursiveDescend) => true,
            (JSONPathToken::Wildcard, JSONPathToken::Wildcard) => true,
            (JSONPathToken::Index(a), JSONPathToken::Index(b)) => a == b,
            (JSONPathToken::Slice(a1, a2, a3), JSONPathToken::Slice(b1, b2, b3)) => {
                a1 == b1 && a2 == b2 && a3 == b3
            }
            (JSONPathToken::Filter(_), JSONPathToken::Filter(_)) => {
                // We can't compare functions for equality, so we'll consider all filters unequal
                false
            }
            _ => false,
        }
    }
}

// Parse JSONPath string into tokens
pub fn parse_jsonpath(path: &str) -> Result<Vec<JSONPathToken>, JsonPathError> {
    let mut tokens = Vec::new();
    let chars = path.chars().collect::<Vec<char>>();
    let mut iter = chars.iter().peekable();

    while let Some(&c) = iter.next() {
        match c {
            '$' => tokens.push(JSONPathToken::Root),
            '.' => {
                if iter.peek() == Some(&&'.') {
                    iter.next();
                    tokens.push(JSONPathToken::RecursiveDescend);
                } else if iter.peek() == Some(&&'*') {
                    iter.next();
                    tokens.push(JSONPathToken::Wildcard);
                } else {
                    let mut key = String::new();
                    while let Some(&c) = iter.peek() {
                        if c.is_alphanumeric() || *c == '_' {
                            key.push(*c);
                            iter.next();
                        } else {
                            break;
                        }
                    }
                    tokens.push(JSONPathToken::Child(key));
                }
            }
            '[' => {
                // Handle array index, slice, filter, or wildcard
                let mut content = String::new();
                let mut in_quotes = false;
                for &c in iter.by_ref() {
                    if c == ']' && !in_quotes {
                        break;
                    }
                    if c == '\'' {
                        in_quotes = !in_quotes;
                    }
                    content.push(c);
                }

                if content == "*" {
                    tokens.push(JSONPathToken::Wildcard);
                } else if let Ok(index) = content.parse::<isize>() {
                    tokens.push(JSONPathToken::Index(index));
                } else if content.contains(':') {
                    let slice: Vec<&str> = content.split(':').collect();
                    let start = slice.first().and_then(|s| s.parse().ok());
                    let end = slice.get(1).and_then(|s| s.parse().ok());
                    let step = slice.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
                    tokens.push(JSONPathToken::Slice(start, end, Some(step as isize)));
                } else if content.starts_with('?') {
                    // let predicate = content[1..].to_string();
                    tokens.push(JSONPathToken::Filter(Box::new(|_v| {
                        // let result = evaluate_predicate(predicate, v);
                        // result
                        unimplemented!("JSONPath filter not implemented")
                    })));
                } else if content.starts_with('\'') && content.ends_with('\'') {
                    // Handle quoted keys
                    tokens.push(JSONPathToken::Child(
                        content[1..content.len() - 1].to_string(),
                    ));
                } else if let Some(ans) = try_parse_union_index(&content) {
                    tokens.push(JSONPathToken::UnionIndex(ans));
                } else if let Some(ans) = try_parse_union_key(&content) {
                    tokens.push(JSONPathToken::UnionKey(ans));
                } else {
                    return Err(JsonPathError::InvalidJsonPath(format!(
                        "Invalid array accessor: [{}]",
                        content
                    )));
                }
            }
            '*' => {
                tokens.push(JSONPathToken::Wildcard);
            }
            c if c.is_alphabetic() => {
                // Handle cases like "$.books.store[0]" where there's no dot before "books"
                let mut key = String::new();
                key.push(c);
                while let Some(&c) = iter.peek() {
                    if c.is_alphanumeric() || *c == '_' {
                        key.push(*c);
                        iter.next();
                    } else {
                        break;
                    }
                }
                tokens.push(JSONPathToken::Child(key));
            }
            _ => {
                return Err(JsonPathError::InvalidJsonPath(format!(
                    "Unexpected character '{}' in JSONPath: {}",
                    c, path
                )))
            }
        }
    }

    Ok(tokens)
}

fn try_parse_union_key(content: &str) -> Option<Vec<String>> {
    let keys = content
        .split(',')
        .map(|s| {
            let trimmed = s.trim();
            if trimmed.starts_with('\'') || trimmed.starts_with('"') {
                let stripped = trimmed.trim_matches(|c| c == '\'' || c == '"');
                if stripped.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    Some(stripped.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect::<Option<Vec<_>>>();
    keys
}

fn try_parse_union_index(content: &str) -> Option<Vec<isize>> {
    let indices = content
        .split(',')
        .map(|s| s.trim().parse().ok())
        .collect::<Option<Vec<_>>>();

    indices
}

// Evaluate JSONPath against a LoroDoc
pub fn evaluate_jsonpath(
    doc: &dyn PathValue,
    path: &str,
) -> Result<Vec<ValueOrHandler>, JsonPathError> {
    let tokens = parse_jsonpath(path)?;
    trace!("tokens: {:#?}", tokens);
    let mut results = Vec::new();

    // Start with the root
    if let Some(JSONPathToken::Root) = tokens.first() {
        evaluate_tokens(doc, &tokens[1..], &mut results);
    } else {
        return Err(JsonPathError::InvalidJsonPath(
            "JSONPath must start with $".to_string(),
        ));
    }

    Ok(results)
}

fn evaluate_tokens(
    value: &dyn PathValue,
    tokens: &[JSONPathToken],
    results: &mut Vec<ValueOrHandler>,
) {
    if tokens.is_empty() {
        results.push(value.clone_this().unwrap());
        return;
    }

    match &tokens[0] {
        JSONPathToken::Child(key) => {
            if let Some(child) = value.get_by_key(key) {
                evaluate_tokens(&child, &tokens[1..], results);
            }
        }
        JSONPathToken::RecursiveDescend => {
            // Implement recursive descent
            value.for_each_for_path(&mut |child| {
                evaluate_tokens(&child, tokens, results);
                ControlFlow::Continue(())
            });
            evaluate_tokens(value, &tokens[1..], results);
        }
        JSONPathToken::Wildcard => {
            value.for_each_for_path(&mut |child| {
                evaluate_tokens(&child, &tokens[1..], results);
                ControlFlow::Continue(())
            });
        }
        JSONPathToken::Index(index) => {
            if let Some(child) = value.get_by_index(*index) {
                evaluate_tokens(&child, &tokens[1..], results);
            }
        }
        JSONPathToken::UnionIndex(indices) => {
            for index in indices {
                if let Some(child) = value.get_by_index(*index) {
                    evaluate_tokens(&child, &tokens[1..], results);
                }
            }
        }
        JSONPathToken::UnionKey(keys) => {
            for key in keys {
                if let Some(child) = value.get_by_key(key) {
                    evaluate_tokens(&child, &tokens[1..], results);
                }
            }
        }
        JSONPathToken::Slice(start, end, step) => {
            let len = value.length_for_path() as isize;
            let start = start.unwrap_or(0);
            let start = if start < 0 {
                (len + start).max(0).min(len)
            } else {
                start.max(0).min(len)
            };

            let end = end.unwrap_or(len);
            let end = if end < 0 {
                (len + end).max(0).min(len)
            } else {
                end.max(0).min(len)
            };

            let step = step.unwrap_or(1);
            if step > 0 {
                for i in (start..end).step_by(step as usize) {
                    if let Some(child) = value.get_by_index(i) {
                        evaluate_tokens(&child, &tokens[1..], results);
                    }
                }
            } else {
                for i in (start..end).rev().step_by((-step) as usize) {
                    if let Some(child) = value.get_by_index(i) {
                        evaluate_tokens(&child, &tokens[1..], results);
                    }
                }
            }
        }
        JSONPathToken::Filter(predicate) => {
            // Implement filter logic
            value.for_each_for_path(&mut |child| {
                if predicate(&child) {
                    evaluate_tokens(&child, &tokens[1..], results);
                }
                ControlFlow::Continue(())
            });
        }
        JSONPathToken::Root => {
            // Root should only appear at the beginning, which is handled in evaluate_jsonpath
            panic!("Unexpected root token in path");
        }
    }
}

// Implement necessary trait bounds for PathValue
pub trait PathValue {
    fn get_by_key(&self, key: &str) -> Option<ValueOrHandler>;
    fn get_by_index(&self, index: isize) -> Option<ValueOrHandler>;
    fn for_each_for_path(&self, f: &mut dyn FnMut(ValueOrHandler) -> ControlFlow<()>);
    fn length_for_path(&self) -> usize;
    fn get_child_by_id(&self, id: ContainerID) -> Option<Handler>;
    fn clone_this(&self) -> Result<ValueOrHandler, JsonPathError>;
}

// Implement PathValue for ValueOrHandler
impl PathValue for ValueOrHandler {
    fn get_by_key(&self, key: &str) -> Option<ValueOrHandler> {
        match self {
            ValueOrHandler::Value(v) => v.get_by_key(key).cloned().map(ValueOrHandler::Value),
            ValueOrHandler::Handler(h) => h.get_by_key(key),
        }
    }

    fn get_by_index(&self, index: isize) -> Option<ValueOrHandler> {
        match self {
            ValueOrHandler::Value(v) => v.get_by_index(index).cloned().map(ValueOrHandler::Value),
            ValueOrHandler::Handler(h) => h.get_by_index(index),
        }
    }

    fn for_each_for_path(&self, f: &mut dyn FnMut(ValueOrHandler) -> ControlFlow<()>) {
        match self {
            ValueOrHandler::Value(v) => v.for_each_for_path(f),
            ValueOrHandler::Handler(h) => h.for_each_for_path(f),
        }
    }

    fn length_for_path(&self) -> usize {
        match self {
            ValueOrHandler::Value(v) => v.length_for_path(),
            ValueOrHandler::Handler(h) => h.length_for_path(),
        }
    }

    fn get_child_by_id(&self, id: ContainerID) -> Option<Handler> {
        match self {
            ValueOrHandler::Handler(h) => h.get_child_by_id(id),
            _ => None,
        }
    }

    fn clone_this(&self) -> Result<ValueOrHandler, JsonPathError> {
        match self {
            ValueOrHandler::Value(v) => Ok(ValueOrHandler::Value(v.clone())),
            ValueOrHandler::Handler(h) => Ok(ValueOrHandler::Handler(h.clone())),
        }
    }
}

// Implement PathValue for LoroDoc
impl PathValue for LoroDoc {
    fn get_by_key(&self, key: &str) -> Option<ValueOrHandler> {
        self.get_by_str_path(key)
    }

    fn get_by_index(&self, _index: isize) -> Option<ValueOrHandler> {
        None // LoroDoc doesn't support index-based access
    }

    fn for_each_for_path(&self, f: &mut dyn FnMut(ValueOrHandler) -> ControlFlow<()>) {
        let arena = self.arena();
        for c in arena.root_containers() {
            let cid = arena.idx_to_id(c).unwrap();
            let h = self.get_handler(cid).unwrap();
            if f(ValueOrHandler::Handler(h)) == ControlFlow::Break(()) {
                break;
            }
        }
    }

    fn length_for_path(&self) -> usize {
        let state = self.app_state().try_lock().unwrap();
        state.arena.root_containers().len()
    }

    fn get_child_by_id(&self, id: ContainerID) -> Option<Handler> {
        self.get_handler(id)
    }

    fn clone_this(&self) -> Result<ValueOrHandler, JsonPathError> {
        Ok(ValueOrHandler::Value(self.get_deep_value()))
    }
}

// Implement PathValue for Handler
impl PathValue for Handler {
    fn get_by_key(&self, key: &str) -> Option<ValueOrHandler> {
        match self {
            Handler::Map(h) => h.get_by_key(key),
            Handler::Tree(h) => h.get_by_key(key),
            _ => None,
        }
    }

    fn get_by_index(&self, index: isize) -> Option<ValueOrHandler> {
        match self {
            Handler::List(h) => h.get_by_index(index),
            Handler::MovableList(h) => h.get_by_index(index),
            _ => None,
        }
    }

    fn for_each_for_path(&self, f: &mut dyn FnMut(ValueOrHandler) -> ControlFlow<()>) {
        match self {
            Handler::Map(h) => h.for_each_for_path(f),
            Handler::List(h) => h.for_each_for_path(f),
            Handler::MovableList(h) => h.for_each_for_path(f),
            Handler::Tree(h) => h.for_each_for_path(f),
            _ => {}
        }
    }

    fn length_for_path(&self) -> usize {
        match self {
            Handler::Map(h) => h.length_for_path(),
            Handler::List(h) => h.length_for_path(),
            Handler::MovableList(h) => h.length_for_path(),
            Handler::Tree(h) => h.length_for_path(),
            Handler::Text(h) => h.length_for_path(),
            _ => 0,
        }
    }

    fn get_child_by_id(&self, id: ContainerID) -> Option<Handler> {
        match self {
            Handler::Map(h) => h.get_child_by_id(id),
            Handler::List(h) => h.get_child_by_id(id),
            Handler::MovableList(h) => h.get_child_by_id(id),
            Handler::Tree(h) => h.get_child_by_id(id),
            _ => None,
        }
    }

    fn clone_this(&self) -> Result<ValueOrHandler, JsonPathError> {
        Ok(ValueOrHandler::Handler(self.clone()))
    }
}

// Implementations for specific handlers
impl PathValue for MapHandler {
    fn get_by_key(&self, key: &str) -> Option<ValueOrHandler> {
        self.get_(key)
    }

    fn get_by_index(&self, _index: isize) -> Option<ValueOrHandler> {
        None
    }

    fn for_each_for_path(&self, f: &mut dyn FnMut(ValueOrHandler) -> ControlFlow<()>) {
        let mut done = false;
        self.for_each(|_, v| {
            if done {
                return;
            }

            if let ControlFlow::Break(_) = f(v) {
                done = true;
            }
        });
    }

    fn length_for_path(&self) -> usize {
        self.len()
    }

    fn get_child_by_id(&self, id: ContainerID) -> Option<Handler> {
        self.get_child_handler(id.to_string().as_str()).ok()
    }

    fn clone_this(&self) -> Result<ValueOrHandler, JsonPathError> {
        Ok(ValueOrHandler::Handler(Handler::Map(self.clone())))
    }
}

impl PathValue for ListHandler {
    fn get_by_key(&self, _key: &str) -> Option<ValueOrHandler> {
        None
    }

    fn get_by_index(&self, index: isize) -> Option<ValueOrHandler> {
        if index < 0 {
            self.get_(self.len() - (-index) as usize)
        } else {
            self.get_(index as usize)
        }
    }

    fn for_each_for_path(&self, f: &mut dyn FnMut(ValueOrHandler) -> ControlFlow<()>) {
        let mut done = false;
        self.for_each(|v| {
            if done {
                return;
            }

            if let ControlFlow::Break(_) = f(v) {
                done = true;
            }
        });
    }

    fn length_for_path(&self) -> usize {
        self.len()
    }

    fn get_child_by_id(&self, _id: ContainerID) -> Option<Handler> {
        unimplemented!()
    }

    fn clone_this(&self) -> Result<ValueOrHandler, JsonPathError> {
        Ok(ValueOrHandler::Handler(Handler::List(self.clone())))
    }
}

impl PathValue for MovableListHandler {
    fn get_by_key(&self, _key: &str) -> Option<ValueOrHandler> {
        None
    }

    fn get_by_index(&self, index: isize) -> Option<ValueOrHandler> {
        if index < 0 {
            if self.len() > (-index) as usize {
                self.get_(self.len() - (-index) as usize)
            } else {
                None
            }
        } else {
            self.get_(index as usize)
        }
    }

    fn for_each_for_path(&self, f: &mut dyn FnMut(ValueOrHandler) -> ControlFlow<()>) {
        let mut done = false;
        self.for_each(|v| {
            if done {
                return;
            }

            if let ControlFlow::Break(_) = f(v) {
                done = true;
            }
        })
    }

    fn length_for_path(&self) -> usize {
        self.len()
    }

    fn get_child_by_id(&self, _id: ContainerID) -> Option<Handler> {
        unimplemented!()
    }

    fn clone_this(&self) -> Result<ValueOrHandler, JsonPathError> {
        Ok(ValueOrHandler::Handler(Handler::MovableList(self.clone())))
    }
}

impl PathValue for TextHandler {
    fn get_by_key(&self, _key: &str) -> Option<ValueOrHandler> {
        None
    }

    fn get_by_index(&self, _index: isize) -> Option<ValueOrHandler> {
        None
    }

    fn for_each_for_path(&self, _f: &mut dyn FnMut(ValueOrHandler) -> ControlFlow<()>) {
        // TextHandler doesn't have children to iterate over
    }

    fn length_for_path(&self) -> usize {
        self.len_unicode()
    }

    fn get_child_by_id(&self, _id: ContainerID) -> Option<Handler> {
        None
    }

    fn clone_this(&self) -> Result<ValueOrHandler, JsonPathError> {
        Ok(ValueOrHandler::Handler(Handler::Text(self.clone())))
    }
}

impl PathValue for TreeHandler {
    fn get_by_key(&self, _key: &str) -> Option<ValueOrHandler> {
        None
    }

    fn get_by_index(&self, _index: isize) -> Option<ValueOrHandler> {
        None
    }

    fn for_each_for_path(&self, _f: &mut dyn FnMut(ValueOrHandler) -> ControlFlow<()>) {
        unimplemented!()
    }

    fn length_for_path(&self) -> usize {
        unimplemented!()
    }

    fn get_child_by_id(&self, _id: ContainerID) -> Option<Handler> {
        unimplemented!()
    }

    fn clone_this(&self) -> Result<ValueOrHandler, JsonPathError> {
        Ok(ValueOrHandler::Handler(Handler::Tree(self.clone())))
    }
}

impl PathValue for LoroValue {
    fn get_by_key(&self, key: &str) -> Option<ValueOrHandler> {
        match self {
            LoroValue::Map(map) => map.get(key).map(|v| ValueOrHandler::Value(v.clone())),
            _ => None,
        }
    }

    fn get_by_index(&self, index: isize) -> Option<ValueOrHandler> {
        match self {
            LoroValue::List(list) => {
                let index = if index < 0 {
                    if list.len() > (-index) as usize {
                        list.len() - (-index) as usize
                    } else {
                        return None;
                    }
                } else {
                    index as usize
                };
                list.get(index).map(|v| ValueOrHandler::Value(v.clone()))
            }
            _ => None,
        }
    }

    fn for_each_for_path(&self, f: &mut dyn FnMut(ValueOrHandler) -> ControlFlow<()>) {
        match self {
            LoroValue::List(list) => {
                for item in list.iter() {
                    if let ControlFlow::Break(_) = f(ValueOrHandler::Value(item.clone())) {
                        break;
                    }
                }
            }
            LoroValue::Map(map) => {
                for (_, value) in map.iter() {
                    if let ControlFlow::Break(_) = f(ValueOrHandler::Value(value.clone())) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    fn length_for_path(&self) -> usize {
        match self {
            LoroValue::List(list) => list.len(),
            LoroValue::Map(map) => map.len(),
            LoroValue::String(s) => s.len(),
            _ => 0,
        }
    }

    fn get_child_by_id(&self, _id: ContainerID) -> Option<Handler> {
        None
    }

    fn clone_this(&self) -> Result<ValueOrHandler, JsonPathError> {
        Ok(ValueOrHandler::Value(self.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jsonpath() -> Result<(), JsonPathError> {
        let path = "$.store.book[0].title";
        let tokens = parse_jsonpath(path)?;
        assert_eq!(
            tokens,
            vec![
                JSONPathToken::Root,
                JSONPathToken::Child("store".to_string()),
                JSONPathToken::Child("book".to_string()),
                JSONPathToken::Index(0),
                JSONPathToken::Child("title".to_string()),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_evaluate_jsonpath() -> Result<(), JsonPathError> {
        let doc = LoroDoc::new();
        doc.start_auto_commit();
        let map = doc.get_map("map");
        map.insert("key", "value").unwrap();
        let books = map
            .insert_container("books", ListHandler::new_detached())
            .unwrap();
        let book = books
            .insert_container(0, MapHandler::new_detached())
            .unwrap();
        book.insert("title", "1984").unwrap();
        book.insert("author", "George Orwell").unwrap();
        let path = "$['map'].books[0].title";
        let result = evaluate_jsonpath(&doc, path)?;
        assert_eq!(result.len(), 1);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "1984"
        );
        Ok(())
    }

    #[test]
    fn test_jsonpath_on_loro_doc() -> Result<(), JsonPathError> {
        let doc = LoroDoc::new();
        doc.start_auto_commit();
        let map = doc.get_map("map");
        map.insert("key", "value").unwrap();
        let books = map
            .insert_container("books", ListHandler::new_detached())
            .unwrap();
        let book = books
            .insert_container(0, MapHandler::new_detached())
            .unwrap();
        book.insert("title", "1984").unwrap();
        book.insert("author", "George Orwell").unwrap();

        // Test child selectors
        let path = "$['map'].books[0].title";
        let result = evaluate_jsonpath(&doc, path).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "1984"
        );

        // Test wildcard
        let path = "$['map'].books[*].title";
        let result = evaluate_jsonpath(&doc, path).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "1984"
        );

        // Test recursive descent
        let path = "$..title";
        let result = evaluate_jsonpath(&doc, path).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "1984"
        );
        Ok(())
    }

    #[test]
    fn test_complex_jsonpath_queries() -> Result<(), JsonPathError> {
        let doc = LoroDoc::new();
        doc.start_auto_commit();
        let map = doc.get_map("map");
        map.insert("key", "value").unwrap();
        let books = map
            .insert_container("books", ListHandler::new_detached())
            .unwrap();
        let book = books
            .insert_container(0, MapHandler::new_detached())
            .unwrap();
        book.insert("title", "1984").unwrap();
        book.insert("author", "George Orwell").unwrap();
        book.insert("price", 10).unwrap();
        let book = books
            .insert_container(1, MapHandler::new_detached())
            .unwrap();
        book.insert("title", "Animal Farm").unwrap();
        book.insert("author", "George Orwell").unwrap();
        book.insert("price", 8).unwrap();

        // Test array indexing
        let path = "$['map'].books[0].title";
        let result = evaluate_jsonpath(&doc, path).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "1984"
        );

        // Test recursive descent
        let path = "$..title";
        let result = evaluate_jsonpath(&doc, path).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "1984"
        );
        assert_eq!(
            &**result[1].as_value().unwrap().as_string().unwrap(),
            "Animal Farm"
        );
        Ok(())
    }
}
