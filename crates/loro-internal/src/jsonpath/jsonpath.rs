use std::collections::VecDeque;
use loro_common::{ContainerID, LoroValue};
use thiserror::Error;
use crate::handler::{Handler, ListHandler, MapHandler, MovableListHandler, TextHandler, TreeHandler, ValueOrHandler};
use crate::{HandlerTrait, LoroDoc};
use std::ops::ControlFlow;
use crate::jsonpath::ast::{ComparisonOperator, FilterExpression, LogicalOperator, Segment, Selector};
use crate::jsonpath::{JSONPathParser, Query};

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
        evaluate_jsonpath(self, &jsonpath)
    }
}

enum ExprValue {
    Bool(bool),
    Value(LoroValue),
    Nodes(Vec<ValueOrHandler>),
}

fn evaluate_jsonpath(root: &dyn PathValue, jsonpath: &str) -> Result<Vec<ValueOrHandler>, JsonPathError> {
    let parser = JSONPathParser::new();
    let query = parser.parse(jsonpath).map_err(|e| JsonPathError::InvalidJsonPath(e.to_string()))?;
    evaluate_query(root, &query)
}

fn evaluate_query(root: &dyn PathValue, query: &Query) -> Result<Vec<ValueOrHandler>, JsonPathError> {
    let current: Vec<ValueOrHandler> = vec![root.clone_this()?];
    apply_segment(root, current, &query.segments)
}

fn apply_segment(root: &dyn PathValue, mut current: Vec<ValueOrHandler>, segment: &Segment) -> Result<Vec<ValueOrHandler>, JsonPathError> {
    match segment {
        Segment::Root {} => Ok(current),
        Segment::Child { left, selectors } => {
            current = apply_segment(root, current, left)?;
            let mut new_current = vec![];
            for node in current {
                apply_selectors(root, &node, selectors, &mut new_current)?;
            }
            Ok(new_current)
        }
        Segment::Recursive { left, selectors } => {
            current = apply_segment(root, current, left)?;
            let mut new_current = vec![];
            let mut queue: VecDeque<ValueOrHandler> = current.into_iter().collect();
            while !queue.is_empty() {
                let node = queue.pop_front().unwrap();
                apply_selectors(root, &node, selectors, &mut new_current)?;
                node.for_each_for_path(&mut |child| {
                    queue.push_back(child);
                    ControlFlow::Continue(())
                });
            }
            Ok(new_current)
        }
    }
}

fn apply_selectors(root: &dyn PathValue, node: &ValueOrHandler, selectors: &[Selector], results: &mut Vec<ValueOrHandler>) -> Result<(), JsonPathError> {
    for selector in selectors {
        match selector {
            Selector::Name { name } => {
                if let Some(child) = node.get_by_key(name) {
                    results.push(child);
                }
            }
            Selector::Index { index } => {
                let mut idx = *index;
                if idx < 0 {
                    idx += node.length_for_path() as i64;
                }
                if idx >= 0 {
                    if let Some(child) = node.get_by_index(idx as isize) {
                        results.push(child);
                    }
                }
            }
            Selector::Slice { start, stop, step } => {
                let len = node.length_for_path() as i64;
                let eff_start = start.unwrap_or(if step.unwrap_or(1) > 0 { 0 } else { len - 1 });
                let eff_stop = stop.unwrap_or(if step.unwrap_or(1) > 0 { len } else { -len - 1 });
                let eff_step = step.unwrap_or(1);
                if eff_step == 0 {
                    continue;
                }
                let mut start_idx = if eff_start < 0 { eff_start + len } else { eff_start };
                let mut stop_idx = if eff_stop < 0 { eff_stop + len } else { eff_stop };
                start_idx = start_idx.max(0).min(len);
                stop_idx = stop_idx.max(-1).min(len);
                if eff_step > 0 {
                    let mut i = start_idx;
                    while i < stop_idx {
                        if let Some(child) = node.get_by_index(i as isize) {
                            results.push(child);
                        }
                        i += eff_step;
                    }
                } else {
                    let mut i = start_idx;
                    while i > stop_idx {
                        if let Some(child) = node.get_by_index(i as isize) {
                            results.push(child);
                        }
                        i += eff_step;
                    }
                }
            }
            Selector::Wild {} => {
                node.for_each_for_path(&mut |child| {
                    results.push(child);
                    ControlFlow::Continue(())
                });
            }
            Selector::Filter { expression } => {
                node.for_each_for_path(&mut |child| {
                    if eval_filter_expr(root, &child, expression) {
                        results.push(child);
                    }
                    ControlFlow::Continue(())
                });
            }
        }
    }
    Ok(())
}

fn eval_filter_expr(root: &dyn PathValue, current: &ValueOrHandler, expr: &FilterExpression) -> bool {
    to_logical(eval_expr(root, current, expr))
}

fn eval_expr(root: &dyn PathValue, current: &ValueOrHandler, expr: &FilterExpression) -> ExprValue {
    match expr {
        FilterExpression::True_ {} => ExprValue::Bool(true),
        FilterExpression::False_ {} => ExprValue::Bool(false),
        FilterExpression::Null {} => ExprValue::Value(LoroValue::Null),
        FilterExpression::StringLiteral { value } => ExprValue::Value(LoroValue::String(value.clone().into())),
        FilterExpression::Int { value } => ExprValue::Value(LoroValue::I64(*value)),
        FilterExpression::Float { value } => ExprValue::Value(LoroValue::Double(*value)),
        FilterExpression::Not { expression } => ExprValue::Bool(!to_logical(eval_expr(root, current, expression))),
        FilterExpression::Logical { left, operator, right } => {
            let l = to_logical(eval_expr(root, current, left));
            let r = to_logical(eval_expr(root, current, right));
            ExprValue::Bool(match operator {
                LogicalOperator::And => l && r,
                LogicalOperator::Or => l || r,
            })
        }
        FilterExpression::Comparison { left, operator, right } => {
            let l = eval_expr(root, current, left);
            let r = eval_expr(root, current, right);
            ExprValue::Bool(compare_expr(l, operator, r))
        }
        FilterExpression::RelativeQuery { query } => {
            let res = evaluate_query(current as &dyn PathValue, query).unwrap_or_default();
            ExprValue::Nodes(res)
        }
        FilterExpression::RootQuery { query } => {
            let res = evaluate_query(root, query).unwrap_or_default();
            ExprValue::Nodes(res)
        }
        FilterExpression::Function { name, args } => {
            let mut eval_args = vec![];
            for arg in args {
                eval_args.push(eval_expr(root, current, arg));
            }
            match name.as_str() {
                // "search" => {
                //     if eval_args.len() == 2 {
                //         if let (ExprValue::Value(LoroValue::String(l)), ExprValue::Value(LoroValue::String(r))) = (eval_args[0], eval_args[1]) {
                //             ExprValue::Bool(l.contains(&*r))
                //         } else {
                //             ExprValue::Bool(false)
                //         }
                //     } else {
                //         ExprValue::Bool(false)
                //     }
                // }
                // "match" => {
                //     if eval_args.len() == 2 {
                //         if let (ExprValue::Value(LoroValue::String(l)), ExprValue::Value(LoroValue::String(r))) = (eval_args[0], eval_args[1]) {
                //             ExprValue::Bool(l.as_str() == r.as_str())
                //         } else {
                //             ExprValue::Bool(false)
                //         }
                //     } else {
                //         ExprValue::Bool(false)
                //     }
                // }
                "count" => {
                    if eval_args.len() == 1 {
                        if let ExprValue::Nodes(ns) = &eval_args[0] {
                            ExprValue::Value(LoroValue::I64(ns.len() as i64))
                        } else {
                            ExprValue::Value(LoroValue::I64(0))
                        }
                    } else {
                        ExprValue::Value(LoroValue::I64(0))
                    }
                }
                "length" => {
                    if eval_args.len() == 1 {
                        if let ExprValue::Value(v) = &eval_args[0] {
                            let len = match v {
                                LoroValue::List(l) => l.len(),
                                LoroValue::Map(m) => m.len(),
                                LoroValue::String(s) => s.len(),
                                _ => 0,
                            };
                            ExprValue::Value(LoroValue::I64(len as i64))
                        } else {
                            ExprValue::Value(LoroValue::I64(0))
                        }
                    } else {
                        ExprValue::Value(LoroValue::I64(0))
                    }
                }
                "value" => {
                    if eval_args.len() == 1 {
                        if let ExprValue::Nodes(ns) = &eval_args[0] {
                            if ns.len() == 1 {
                                ExprValue::Value(get_value(&ns[0]))
                            } else {
                                ExprValue::Value(LoroValue::Null)
                            }
                        } else {
                            ExprValue::Value(LoroValue::Null)
                        }
                    } else {
                        ExprValue::Value(LoroValue::Null)
                    }
                }
                _ => ExprValue::Bool(false),
            }
        }
    }
}

fn to_logical(v: ExprValue) -> bool {
    match v {
        ExprValue::Bool(b) => b,
        ExprValue::Value(v) => match v {
            LoroValue::Bool(b) => b,
            LoroValue::Null => false,
            LoroValue::String(s) => !s.is_empty(),
            LoroValue::I64(0) => false,
            LoroValue::I64(_) => true,
            LoroValue::Double(f) => (f - 0.0).abs() > f64::EPSILON,
            LoroValue::List(l) => !l.is_empty(),
            LoroValue::Map(m) => !m.is_empty(),
            _ => false,
        },
        ExprValue::Nodes(ns) => !ns.is_empty(),
    }
}

fn compare_expr(l: ExprValue, op: &ComparisonOperator, r: ExprValue) -> bool {
    match (l, r) {
        (ExprValue::Nodes(ls), ExprValue::Nodes(rs)) => ls.iter().any(|a| rs.iter().any(|b| compare_values(&get_value(a), op, &get_value(b)))),
        (ExprValue::Nodes(ls), v) => ls.iter().any(|a| compare_values(&get_value(a), op, &get_other_value(&v))),
        (v, ExprValue::Nodes(rs)) => rs.iter().any(|b| compare_values(&get_other_value(&v), op, &get_value(b))),
        (ExprValue::Value(l), ExprValue::Value(r)) => compare_values(&l, op, &r),
        _ => false,
    }
}

fn compare_values(l: &LoroValue, op: &ComparisonOperator, r: &LoroValue) -> bool {
    match (l, r) {
        (LoroValue::Double(a), LoroValue::Double(b)) => compare_nums(*a, op, *b),
        (LoroValue::I64(a), LoroValue::I64(b)) => compare_nums(*a as f64, op, *b as f64),
        (LoroValue::Double(a), LoroValue::I64(b)) => compare_nums(*a, op, *b as f64),
        (LoroValue::I64(a), LoroValue::Double(b)) => compare_nums(*a as f64, op, *b),
        (LoroValue::String(a), LoroValue::String(b)) => compare_strs(a.as_ref(), op, b.as_ref()),
        (LoroValue::Bool(a), LoroValue::Bool(b)) => compare_bools(*a, op, *b),
        (LoroValue::Null, LoroValue::Null) => match op {
            ComparisonOperator::Eq => true,
            ComparisonOperator::Ne => false,
            _ => false,
        },
        _ => false,
    }
}

fn compare_nums(a: f64, op: &ComparisonOperator, b: f64) -> bool {
    match op {
        ComparisonOperator::Eq => (a - b).abs() < f64::EPSILON,
        ComparisonOperator::Ne => (a - b).abs() >= f64::EPSILON,
        ComparisonOperator::Lt => a < b,
        ComparisonOperator::Le => a <= b,
        ComparisonOperator::Gt => a > b,
        ComparisonOperator::Ge => a >= b,
        _ => false,
    }
}

fn compare_strs(a: &str, op: &ComparisonOperator, b: &str) -> bool {
    match op {
        ComparisonOperator::Eq => a == b,
        ComparisonOperator::Ne => a != b,
        ComparisonOperator::Lt => a < b,
        ComparisonOperator::Le => a <= b,
        ComparisonOperator::Gt => a > b,
        ComparisonOperator::Ge => a >= b,
        ComparisonOperator::Contains => a.contains(b)
    }
}

fn compare_bools(a: bool, op: &ComparisonOperator, b: bool) -> bool {
    match op {
        ComparisonOperator::Eq => a == b,
        ComparisonOperator::Ne => a != b,
        _ => false,
    }
}

fn get_value(node: &ValueOrHandler) -> LoroValue {
    match node {
        ValueOrHandler::Value(v) => v.clone(),
        ValueOrHandler::Handler(h) => h.get_value(),
    }
}

fn get_other_value(v: &ExprValue) -> LoroValue {
    match v {
        ExprValue::Value(v) => v.clone(),
        ExprValue::Bool(b) => LoroValue::Bool(*b),
        _ => LoroValue::Null,
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
        let x = self.state.lock().unwrap().store.load_all();
        let arena = self.arena();
        for c in arena.root_containers(x) {
            let cid = arena.idx_to_id(c).unwrap();
            let h = self.get_handler(cid).unwrap();
            if f(ValueOrHandler::Handler(h)) == ControlFlow::Break(()) {
                break;
            }
        }
    }

    fn length_for_path(&self) -> usize {
        let x = self.state.lock().unwrap().store.load_all();
        let state = self.app_state().lock().unwrap();
        state.arena.root_containers(x).len()
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
    use crate::{ListHandler, LoroDoc, LoroValue, MapHandler};

    fn setup_test_doc() -> LoroDoc {
        let doc = LoroDoc::new();
        doc.start_auto_commit();
        let store = doc.get_map("store");
        let books = store.insert_container("books", ListHandler::new_detached()).unwrap();

        // Book 1: 1984
        let book = books.insert_container(0, MapHandler::new_detached()).unwrap();
        book.insert("title", "1984").unwrap();
        book.insert("author", "George Orwell").unwrap();
        book.insert("price", 10).unwrap();
        book.insert("available", true).unwrap();
        book.insert("isbn", "978-0451524935").unwrap();

        // Book 2: Animal Farm
        let book = books.insert_container(1, MapHandler::new_detached()).unwrap();
        book.insert("title", "Animal Farm").unwrap();
        book.insert("author", "George Orwell").unwrap();
        book.insert("price", 8).unwrap();
        book.insert("available", true).unwrap();

        // Book 3: Brave New World
        let book = books.insert_container(2, MapHandler::new_detached()).unwrap();
        book.insert("title", "Brave New World").unwrap();
        book.insert("author", "Aldous Huxley").unwrap();
        book.insert("price", 12).unwrap();
        book.insert("available", false).unwrap();
        book.insert("isbn", "978-0060850524").unwrap();

        // Book 4: Fahrenheit 451
        let book = books.insert_container(3, MapHandler::new_detached()).unwrap();
        book.insert("title", "Fahrenheit 451").unwrap();
        book.insert("author", "Ray Bradbury").unwrap();
        book.insert("price", 9).unwrap();
        book.insert("available", true).unwrap();
        book.insert("isbn", "978-1451673319").unwrap();

        // Book 5: The Great Gatsby
        let book = books.insert_container(4, MapHandler::new_detached()).unwrap();
        book.insert("title", "The Great Gatsby").unwrap();
        book.insert("author", "F. Scott Fitzgerald").unwrap();
        book.insert("price", LoroValue::Null).unwrap();
        book.insert("available", true).unwrap();

        // Book 6: To Kill a Mockingbird
        let book = books.insert_container(5, MapHandler::new_detached()).unwrap();
        book.insert("title", "To Kill a Mockingbird").unwrap();
        book.insert("author", "Harper Lee").unwrap();
        book.insert("price", 11).unwrap();
        book.insert("available", true).unwrap();

        // Book 7: The Catcher in the Rye
        let book = books.insert_container(6, MapHandler::new_detached()).unwrap();
        book.insert("title", "The Catcher in the Rye").unwrap();
        book.insert("author", "J.D. Salinger").unwrap();
        book.insert("price", 10).unwrap();
        book.insert("available", false).unwrap();

        // Book 8: Lord of the Flies
        let book = books.insert_container(7, MapHandler::new_detached()).unwrap();
        book.insert("title", "Lord of the Flies").unwrap();
        book.insert("author", "William Golding").unwrap();
        book.insert("price", 9).unwrap();
        book.insert("available", true).unwrap();

        // Book 9: Pride and Prejudice
        let book = books.insert_container(8, MapHandler::new_detached()).unwrap();
        book.insert("title", "Pride and Prejudice").unwrap();
        book.insert("author", "Jane Austen").unwrap();
        book.insert("price", 7).unwrap();
        book.insert("available", true).unwrap();

        // Book 10: The Hobbit
        let book = books.insert_container(9, MapHandler::new_detached()).unwrap();
        book.insert("title", "The Hobbit").unwrap();
        book.insert("author", "J.R.R. Tolkien").unwrap();
        book.insert("price", 14).unwrap();
        book.insert("available", true).unwrap();

        // Additional metadata
        store.insert("featured_author", "George Orwell").unwrap();
        let featured_authors = store.insert_container("featured_authors", ListHandler::new_detached()).unwrap();
        featured_authors.push("George Orwell").unwrap();
        featured_authors.push("Aldous Huxley").unwrap();
        featured_authors.push("Ray Bradbury").unwrap();
        store.insert("min_price", 10).unwrap();
        doc
    }


    mod jsonpath_selectors {
        use super::*;

        #[test]
        fn handles_child_selectors() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[0].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 1);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "1984"
            );
            Ok(())
        }

        #[test]
        fn handles_wildcard_selector() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[*].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 10);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec![
                "1984",
                "Animal Farm",
                "Brave New World",
                "Fahrenheit 451",
                "The Great Gatsby",
                "To Kill a Mockingbird",
                "The Catcher in the Rye",
                "Lord of the Flies",
                "Pride and Prejudice",
                "The Hobbit",
            ];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn handles_recursive_descent() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$..title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 10);
            Ok(())
        }

        #[test]
        fn handles_quoted_keys() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store['books'][0]['title']";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 1);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "1984"
            );
            Ok(())
        }
    }

    mod string_filters {
        use super::*;

        #[test]
        fn filters_by_exact_string_match() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.title == '1984')].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 1);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "1984"
            );
            Ok(())
        }

        #[test]
        fn filters_by_string_contains() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.title contains 'Farm')].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 1);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "Animal Farm"
            );
            Ok(())
        }

        #[test]
        fn filters_by_recursive_string_match() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$..[?(@.author contains 'Orwell')].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 2);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["1984", "Animal Farm"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }
    }

    mod logical_operators {
        use super::*;

        #[test]
        fn filters_with_and_operator() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.author == 'George Orwell' && @.price < 10)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 1);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "Animal Farm"
            );
            Ok(())
        }

        #[test]
        fn filters_with_or_operator() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.author == 'George Orwell' || @.price >= 10)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 6);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec![
                "1984",
                "Animal Farm",
                "Brave New World",
                "To Kill a Mockingbird",
                "The Catcher in the Rye",
                "The Hobbit",
            ];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn filters_with_complex_and_or_combination() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.author == 'George Orwell' && (@.price < 10 || @.available == true))].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 2);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["1984", "Animal Farm"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn filters_with_not_operator() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(!(@.available == false))].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 8);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec![
                "1984",
                "Animal Farm",
                "Fahrenheit 451",
                "The Great Gatsby",
                "To Kill a Mockingbird",
                "Lord of the Flies",
                "Pride and Prejudice",
                "The Hobbit",
            ];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }
    }

    // mod in_operator {
    //     use super::*;
    //
    //     #[test]
    //     fn filters_by_author_in_list() -> Result<(), JsonPathError> {
    //         let doc = setup_test_doc();
    //         let path = "$.store.books[?(@.author in ['George Orwell', 'Jane Austen'])].title";
    //         let result = evaluate_jsonpath(&doc, path)?;
    //         assert_eq!(result.len(), 3);
    //         let mut titles: Vec<&str> = result
    //             .iter()
    //             .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
    //             .collect();
    //         titles.sort();
    //         let mut expected = vec!["1984", "Animal Farm", "Pride and Prejudice"];
    //         expected.sort();
    //         assert_eq!(titles, expected);
    //         Ok(())
    //     }
    //
    //     #[test]
    //     fn filters_by_price_in_list() -> Result<(), JsonPathError> {
    //         let doc = setup_test_doc();
    //         let path = "$.store.books[?(@.price in [7, 10, 14])].title";
    //         let result = evaluate_jsonpath(&doc, path)?;
    //         assert_eq!(result.len(), 4);
    //         let mut titles: Vec<&str> = result
    //             .iter()
    //             .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
    //             .collect();
    //         titles.sort();
    //         let mut expected = vec!["1984", "Pride and Prejudice", "The Catcher in the Rye", "The Hobbit"];
    //         expected.sort();
    //         assert_eq!(titles, expected);
    //         Ok(())
    //     }
    //
    //     #[test]
    //     fn filters_with_in_operator_and_null_values() -> Result<(), JsonPathError> {
    //         let doc = setup_test_doc();
    //         let path = "$.store.books[?(@.price in [null, 9])].title";
    //         let result = evaluate_jsonpath(&doc, path)?;
    //         assert_eq!(result.len(), 3);
    //         let mut titles: Vec<&str> = result
    //             .iter()
    //             .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
    //             .collect();
    //         titles.sort();
    //         let mut expected = vec!["Fahrenheit 451", "Lord of the Flies", "The Great Gatsby"];
    //         expected.sort();
    //         assert_eq!(titles, expected);
    //         Ok(())
    //     }
    //
    //     #[test]
    //     fn filters_with_in_operator_in_recursive_descent() -> Result<(), JsonPathError> {
    //         let doc = setup_test_doc();
    //         let path = "$..[?(@.author in ['George Orwell', 'Ray Bradbury'])].title";
    //         let result = evaluate_jsonpath(&doc, path)?;
    //         assert_eq!(result.len(), 3);
    //         let mut titles: Vec<&str> = result
    //             .iter()
    //             .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
    //             .collect();
    //         titles.sort();
    //         let mut expected = vec!["1984", "Animal Farm", "Fahrenheit 451"];
    //         expected.sort();
    //         assert_eq!(titles, expected);
    //         Ok(())
    //     }
    //
    //     #[test]
    //     fn filters_with_root_list_in() -> Result<(), JsonPathError> {
    //         let doc = setup_test_doc();
    //         let path = "$.store.books[?(@.author in $.store.featured_authors)].title";
    //         let result = evaluate_jsonpath(&doc, path)?;
    //         assert_eq!(result.len(), 4);
    //         let mut titles: Vec<&str> = result
    //             .iter()
    //             .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
    //             .collect();
    //         titles.sort();
    //         let mut expected = vec!["1984", "Animal Farm", "Brave New World", "Fahrenheit 451"];
    //         expected.sort();
    //         assert_eq!(titles, expected);
    //         Ok(())
    //     }
    // }

    mod union_and_slice_operations {
        use super::*;

        #[test]
        fn handles_union_indexes() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[0,2].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 2);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "1984"
            );
            assert_eq!(
                &**result[1].as_value().unwrap().as_string().unwrap(),
                "Brave New World"
            );
            Ok(())
        }

        #[test]
        fn handles_union_keys() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[0]['title','author']";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 2);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "1984"
            );
            assert_eq!(
                &**result[1].as_value().unwrap().as_string().unwrap(),
                "George Orwell"
            );
            Ok(())
        }

        #[test]
        fn handles_union_with_negative_indexes() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[-2,-1].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 2);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "Pride and Prejudice"
            );
            assert_eq!(
                &**result[1].as_value().unwrap().as_string().unwrap(),
                "The Hobbit"
            );
            Ok(())
        }

        #[test]
        fn handles_basic_slice() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[0:3].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 3);
            let titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            assert_eq!(titles, vec!["1984", "Animal Farm", "Brave New World"]);
            Ok(())
        }

        #[test]
        fn handles_slice_with_step() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[0:5:2].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 3);
            let titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            assert_eq!(
                titles,
                vec!["1984", "Brave New World", "The Great Gatsby"]
            );
            Ok(())
        }

        #[test]
        fn handles_negative_slice() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[-2:].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 2);
            let titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            assert_eq!(titles, vec!["Pride and Prejudice", "The Hobbit"]);
            Ok(())
        }
    }

    mod complex_and_recursive_filters {
        use super::*;

        #[test]
        fn filters_with_multiple_conditions() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.price >= 10 && @.available == true && @.title contains '1984')].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 1);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "1984"
            );
            Ok(())
        }

        #[test]
        fn filters_with_path_expressions() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.author == 'George Orwell' && @.title != '1984')].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 1);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "Animal Farm"
            );
            Ok(())
        }

        #[test]
        fn filters_with_null_checks() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.price == null || @.price < 10)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 5);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec![
                "Animal Farm",
                "Fahrenheit 451",
                "The Great Gatsby",
                "Pride and Prejudice",
                "Lord of the Flies",
            ];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn handles_recursive_filter_with_price_condition() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$..[?(@.price > 10)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 3);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["Brave New World", "To Kill a Mockingbird", "The Hobbit"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn handles_recursive_filter_with_logical_operators() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$..[?(@.author == 'George Orwell' || @.price > 10)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 5);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec![
                "1984",
                "Animal Farm",
                "Brave New World",
                "To Kill a Mockingbird",
                "The Hobbit",
            ];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }
    }

    mod edge_cases_and_error_handling {
        use super::*;

        #[test]
        fn handles_quoted_keys_with_special_characters() -> Result<(), JsonPathError> {
            let doc = LoroDoc::new();
            doc.start_auto_commit();
            let map = doc.get_map("store");
            let book = map
                .insert_container("book-with-dash", MapHandler::new_detached())
                .unwrap();
            book.insert("price-$10", "cheap").unwrap();
            let path = "$['store']['book-with-dash']['price-$10']";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 1);
            assert_eq!(
                &**result[0].as_value().unwrap().as_string().unwrap(),
                "cheap"
            );
            Ok(())
        }

        #[test]
        fn handles_quoted_keys_with_escaped_quotes() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.author == 'George Orwell')].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 2);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["1984", "Animal Farm"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }
    }

    mod root_filters {
        use super::*;

        #[test]
        fn filters_with_root_reference() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.author == $.store.featured_author)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 2);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["1984", "Animal Farm"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn filters_with_root_numerical_comparison() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.price > $.store.min_price)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 3);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["Brave New World", "The Hobbit", "To Kill a Mockingbird"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn filters_with_root_not_equal() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.author != $.store.featured_author)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 8);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec![
                "Brave New World",
                "Fahrenheit 451",
                "The Great Gatsby",
                "To Kill a Mockingbird",
                "The Catcher in the Rye",
                "Lord of the Flies",
                "Pride and Prejudice",
                "The Hobbit",
            ];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn filters_with_root_complex() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.author == $.store.featured_author && @.price <= $.store.min_price)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 2);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["1984", "Animal Farm"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }
    }

    mod existence_filters {
        use super::*;
        #[test]
        fn filters_by_existence_of_isbn() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.isbn)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 3);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["1984", "Brave New World", "Fahrenheit 451"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn filters_by_non_existence_of_isbn() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(!@.isbn)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 7);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec![
                "Animal Farm",
                "The Great Gatsby",
                "To Kill a Mockingbird",
                "The Catcher in the Rye",
                "Lord of the Flies",
                "Pride and Prejudice",
                "The Hobbit",
            ];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn filters_by_existence_of_price() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.price)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 10);
            Ok(())
        }

        #[test]
        fn filters_by_non_existence_of_price() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(!@.price)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 0);
            Ok(())
        }

        #[test]
        fn filters_by_non_existence_of_root_path() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?($.store.nonexistent)].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 0);
            Ok(())
        }
    }
}