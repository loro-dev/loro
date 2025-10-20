use loro_common::{ContainerID, LoroValue};
use thiserror::Error;
use crate::handler::{Handler, ListHandler, MapHandler, MovableListHandler, TextHandler, TreeHandler, ValueOrHandler};
use crate::LoroDoc;
use std::ops::ControlFlow;
use std::sync::Arc;

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

// Enhanced JSONPath tokens to support logical operations
#[derive(Clone)]
pub enum JSONPathToken {
    Root,
    Child(String),
    RecursiveDescend,
    Wildcard,
    Union(Vec<UnionPart>),
    Slice(Option<isize>, Option<isize>, Option<isize>),
    Filter(Arc<dyn Fn(&ValueOrHandler) -> bool + Send + Sync>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnionPart {
    Index(isize),
    Key(String),
}

impl std::fmt::Debug for JSONPathToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JSONPathToken::Root => write!(f, "Root"),
            JSONPathToken::Child(s) => write!(f, "Child({})", s),
            JSONPathToken::RecursiveDescend => write!(f, "RecursiveDescend"),
            JSONPathToken::Wildcard => write!(f, "Wildcard"),
            JSONPathToken::Union(parts) => write!(f, "Union({:?})", parts),
            JSONPathToken::Slice(start, end, step) => write!(f, "Slice({:?}, {:?}, {:?})", start, end, step),
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
            (JSONPathToken::Union(a), JSONPathToken::Union(b)) => a == b,
            (JSONPathToken::Slice(a1, a2, a3), JSONPathToken::Slice(b1, b2, b3)) => {
                a1 == b1 && a2 == b2 && a3 == b3
            }
            (JSONPathToken::Filter(_), JSONPathToken::Filter(_)) => false,
            _ => false,
        }
    }
}

#[derive(Copy, Clone, Debug)]
enum Op {
    Eq, Neq, Lt, Le, Gt, Ge, In, Contains, Matches,
}

#[derive(Clone)]
enum JSONFilterExpr {
    Literal(LoroValue),
    Path(Vec<JSONPathToken>),
    LogicalAnd(Box<JSONFilterExpr>, Box<JSONFilterExpr>),
    LogicalOr(Box<JSONFilterExpr>, Box<JSONFilterExpr>),
    Not(Box<JSONFilterExpr>),
    Comparison {
        left: Box<JSONFilterExpr>,
        op: Op,
        right: Box<JSONFilterExpr>,
    },
}

fn compare(a: &LoroValue, b: &LoroValue, op: Op) -> bool {
    use LoroValue::*;
    match (a, b, op) {
        (I64(x), I64(y), _) => match op {
            Op::Eq => x == y,
            Op::Neq => x != y,
            Op::Lt => x < y,
            Op::Le => x <= y,
            Op::Gt => x > y,
            Op::Ge => x >= y,
            _ => false,
        },
        (Double(x), Double(y), _) => match op {
            Op::Eq => (x - y).abs() < f64::EPSILON,
            Op::Neq => (x - y).abs() >= f64::EPSILON,
            Op::Lt => x < y,
            Op::Le => x <= y,
            Op::Gt => x > y,
            Op::Ge => x >= y,
            _ => false,
        },
        (String(x), String(y), op) => {
            let x_str = x.as_str();
            let y_str = y.as_str();
            match op {
                Op::Eq => x_str == y_str,
                Op::Neq => x_str != y_str,
                Op::Lt => x_str < y_str,
                Op::Le => x_str <= y_str,
                Op::Gt => x_str > y_str,
                Op::Ge => x_str >= y_str,
                Op::Contains => x_str.contains(y_str),
                Op::In => y_str.contains(x_str),
                Op::Matches => false, // regex matching would go here
                _ => false,
            }
        }
        (String(x), List(list), Op::In) => {
            list.iter().any(|v| matches!(v, String(s) if s.as_str() == x.as_str()))
        }
        (List(list), String(y), Op::Contains) => {
            list.iter().any(|v| matches!(v, String(s) if s.as_str() == y.as_str()))
        }
        (Bool(x), Bool(y), _) => match op {
            Op::Eq => x == y,
            Op::Neq => x != y,
            _ => false,
        },
        (Null, Null, _) => match op {
            Op::Eq => true,
            Op::Neq => false,
            _ => false,
        },
        _ => false,
    }
}


fn parse_jsonpath(path: &str) -> Result<Vec<JSONPathToken>, JsonPathError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = path.chars().collect();
    let mut i = 0;
    let mut expect_root = true;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        match chars[i] {
            '$' if expect_root => {
                tokens.push(JSONPathToken::Root);
                i += 1;
                expect_root = false;
            }
            '.' => parse_dot_notation(&chars, &mut i, &mut tokens)?,
            '[' => parse_bracket_notation(&chars, &mut i, &mut tokens)?,
            '*' => {
                tokens.push(JSONPathToken::Wildcard);
                i += 1;
            }
            c if !expect_root && (c.is_alphabetic() || c == '_' || c == '-') => {
                parse_unquoted_child(&chars, &mut i, &mut tokens)?;
            }
            _ => return Err(JsonPathError::InvalidJsonPath(format!(
                "Unexpected character '{}' at position {}",
                chars[i], i
            ))),
        }
    }
    if expect_root {
        return Err(JsonPathError::InvalidJsonPath("Path must start with $".to_string()));
    }
    Ok(tokens)
}

fn parse_relative_jsonpath(path: &str) -> Result<Vec<JSONPathToken>, JsonPathError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = path.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        match chars[i] {
            '.' => parse_dot_notation(&chars, &mut i, &mut tokens)?,
            '[' => parse_bracket_notation(&chars, &mut i, &mut tokens)?,
            '*' => {
                tokens.push(JSONPathToken::Wildcard);
                i += 1;
            }
            c if (c.is_alphabetic() || c == '_' || c == '-') => {
                parse_unquoted_child(&chars, &mut i, &mut tokens)?;
            }
            '$' => return Err(JsonPathError::InvalidJsonPath("Relative path cannot start with $".to_string())),
            _ => return Err(JsonPathError::InvalidJsonPath(format!(
                "Unexpected character '{}' at position {}",
                chars[i], i
            ))),
        }
    }
    if tokens.is_empty() {
        return Err(JsonPathError::InvalidJsonPath("Empty relative path".to_string()));
    }
    Ok(tokens)
}

fn parse_dot_notation(
    chars: &[char],
    i: &mut usize,
    tokens: &mut Vec<JSONPathToken>,
) -> Result<(), JsonPathError> {
    *i += 1; // Skip '.'
    if *i < chars.len() && chars[*i] == '.' {
        tokens.push(JSONPathToken::RecursiveDescend);
        *i += 1;
        return Ok(());
    }
    if *i < chars.len() && chars[*i] == '*' {
        tokens.push(JSONPathToken::Wildcard);
        *i += 1;
        return Ok(());
    }
    let key = parse_identifier(&chars, i)?;
    if key.is_empty() {
        return Err(JsonPathError::InvalidJsonPath("Empty key after dot".to_string()));
    }
    tokens.push(JSONPathToken::Child(key));
    Ok(())
}

fn parse_bracket_notation(
    chars: &[char],
    i: &mut usize,
    tokens: &mut Vec<JSONPathToken>,
) -> Result<(), JsonPathError> {
    let content = parse_bracket_content(&chars, i)?;
    let content = content.trim();
    if content.is_empty() {
        return Err(JsonPathError::InvalidJsonPath("Empty bracket content".to_string()));
    }
    if content == "*" {
        tokens.push(JSONPathToken::Wildcard);
    } else if content.contains(':') {
        let slice = parse_slice(content)?;
        tokens.push(JSONPathToken::Slice(slice.0, slice.1, slice.2));
    } else if content.starts_with('?') {
        let filter_expr = parse_filter_expression(&content[1..])?;
        let predicate = create_filter_predicate(filter_expr);
        tokens.push(JSONPathToken::Filter(Arc::new(predicate)));
    } else {
        // Union or single
        let parts: Vec<&str> = content.split(',').map(str::trim).collect();
        if parts.is_empty() {
            return Err(JsonPathError::InvalidJsonPath("Empty union".to_string()));
        }
        let mut union_parts = Vec::new();
        for part in parts {
            if let Ok(idx) = part.parse::<isize>() {
                union_parts.push(UnionPart::Index(idx));
            } else if let Ok(key) = parse_quoted_string(part) {
                union_parts.push(UnionPart::Key(key));
            } else if is_valid_identifier(part) {
                union_parts.push(UnionPart::Key(part.to_string()));
            } else {
                return Err(JsonPathError::InvalidJsonPath(format!(
                    "Invalid union part: [{}]",
                    part
                )));
            }
        }
        tokens.push(JSONPathToken::Union(union_parts));
    }
    Ok(())
}

fn is_valid_identifier(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

fn parse_bracket_content(chars: &[char], i: &mut usize) -> Result<String, JsonPathError> {
    let mut content = String::new();
    let mut depth = 1;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    *i += 1; // Skip opening '['
    while *i < chars.len() && depth > 0 {
        let c = chars[*i];
        match c {
            '\'' => {
                if !in_double_quote && (*i == 0 || chars.get(*i - 1).map_or(true, |prev| *prev != '\\')) {
                    in_single_quote = !in_single_quote;
                }
            }
            '"' => {
                if !in_single_quote && (*i == 0 || chars.get(*i - 1).map_or(true, |prev| *prev != '\\')) {
                    in_double_quote = !in_double_quote;
                }
            }
            _ => {}
        }
        if !in_single_quote && !in_double_quote {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        *i += 1;
                        break;
                    }
                }
                _ => {},
            }
        }
        if depth > 0 {
            content.push(c);
        }
        *i += 1;
    }
    if depth > 0 {
        return Err(JsonPathError::InvalidJsonPath("Unmatched brackets".to_string()));
    }
    Ok(content)
}

fn parse_identifier(chars: &[char], i: &mut usize) -> Result<String, JsonPathError> {
    let mut key = String::new();
    while *i < chars.len() && (chars[*i].is_alphanumeric() || chars[*i] == '_' || chars[*i] == '-') {
        key.push(chars[*i]);
        *i += 1;
    }
    Ok(key)
}

fn parse_unquoted_child(
    chars: &[char],
    i: &mut usize,
    tokens: &mut Vec<JSONPathToken>,
) -> Result<(), JsonPathError> {
    let key = parse_identifier(chars, i)?;
    if key.is_empty() {
        return Err(JsonPathError::InvalidJsonPath("Empty unquoted key".to_string()));
    }
    tokens.push(JSONPathToken::Child(key));
    Ok(())
}

fn parse_quoted_string(content: &str) -> Result<String, JsonPathError> {
    let content = content.trim();
    if (content.starts_with('\'') && content.ends_with('\'')) || (content.starts_with('"') && content.ends_with('"')) {
        let unquoted = &content[1..content.len() - 1];
        Ok(unquoted.to_string())
    } else {
        Err(JsonPathError::InvalidJsonPath("Not a quoted string".to_string()))
    }
}

fn parse_slice(content: &str) -> Result<(Option<isize>, Option<isize>, Option<isize>), JsonPathError> {
    let parts: Vec<&str> = content.split(':').collect();
    let start = if parts[0].is_empty() { None } else { parts[0].parse().ok() };
    let end = if parts.len() > 1 && parts[1].is_empty() { None } else {
        parts.get(1).and_then(|s| s.parse().ok())
    };
    let step = parts.get(2).and_then(|s| if s.is_empty() { None } else { s.parse().ok() });
    Ok((start, end, step))
}

fn parse_filter_expression(predicate: &str) -> Result<JSONFilterExpr, JsonPathError> {
    let mut predicate = predicate.trim();
    if predicate.starts_with('(') && predicate.ends_with(')') {
        predicate = predicate[1..predicate.len() - 1].trim();
    }
    if predicate.is_empty() {
        return Err(JsonPathError::InvalidJsonPath("Empty filter predicate".to_string()));
    }
    // Handle logical operators && and ||
    if let Some(pos) = find_highest_precedence_operator(predicate, &["&&", "||"]) {
        let (left, op, right) = split_at_operator(predicate, pos, &["&&", "||"])?;
        let left_expr = parse_filter_expression(left)?;
        let right_expr = parse_filter_expression(right)?;
        match op {
            "&&" => return Ok(JSONFilterExpr::LogicalAnd(Box::new(left_expr), Box::new(right_expr))),
            "||" => return Ok(JSONFilterExpr::LogicalOr(Box::new(left_expr), Box::new(right_expr))),
            _ => {}
        }
    }
    // Handle NOT operator
    if predicate.starts_with('!') || predicate.starts_with("not ") {
        let inner = if predicate.starts_with('!') {
            predicate[1..].trim()
        } else {
            predicate[4..].trim()
        };
        let inner_expr = parse_filter_expression(inner)?;
        return Ok(JSONFilterExpr::Not(Box::new(inner_expr)));
    }
    // Handle comparisons with better error reporting
    match parse_comparison(predicate) {
        Ok((left, op_str, right)) => {
            let left_expr = parse_path_or_literal(left)?;
            let right_expr = parse_path_or_literal(right)?;
            let op = parse_operator(op_str)?;
            Ok(JSONFilterExpr::Comparison {
                left: Box::new(left_expr),
                op,
                right: Box::new(right_expr),
            })
        }
        Err(e) => {
            // Provide more specific error message for common issues
            if predicate.contains('(') && !predicate.contains(')') {
                return Err(JsonPathError::InvalidJsonPath(format!(
                    "Missing closing parenthesis in filter: {}",
                    predicate
                )));
            }
            Err(e)
        }
    }
}

fn find_highest_precedence_operator(s: &str, operators: &[&str]) -> Option<usize> {
    let mut min_pos = None;
    let mut min_len = usize::MAX;
    for op in operators {
        if let Some(pos) = s.find(op) {
            if pos < min_pos.unwrap_or(usize::MAX) || (pos == min_pos.unwrap_or(usize::MAX) && op.len() > min_len) {
                min_pos = Some(pos);
                min_len = op.len();
            }
        }
    }
    min_pos
}

fn split_at_operator<'a>(s: &'a str, pos: usize, operators: &[&'a str]) -> Result<(&'a str, &'a str, &'a str), JsonPathError> {
    for op in operators {
        if s[pos..].starts_with(op) {
            let left = s[..pos].trim();
            let right = s[pos + op.len()..].trim();
            if left.is_empty() || right.is_empty() {
                return Err(JsonPathError::InvalidJsonPath("Missing operand for logical operator".to_string()));
            }
            return Ok((left, op, right));
        }
    }
    Err(JsonPathError::InvalidJsonPath("Could not split at operator".to_string()))
}

fn parse_comparison(predicate: &str) -> Result<(&str, &str, &str), JsonPathError> {
    let ops = vec!["==", "!=", "<=", ">=", "<", ">", "contains", "in", "=~"];
    for op in ops {
        if let Some(pos) = predicate.find(op) {
            let left = predicate[..pos].trim();
            let right = predicate[pos + op.len()..].trim();
            if !left.is_empty() && !right.is_empty() {
                // Validate that left starts with @ for path expressions
                if left.starts_with('@') || left.starts_with('$') {
                    return Ok((left, op, right));
                }
            }
        }
    }
    Err(JsonPathError::InvalidJsonPath(format!("No valid comparison found in: {}", predicate)))
}

fn parse_operator(op_str: &str) -> Result<Op, JsonPathError> {
    match op_str {
        "==" => Ok(Op::Eq),
        "!=" => Ok(Op::Neq),
        "<" => Ok(Op::Lt),
        "<=" => Ok(Op::Le),
        ">" => Ok(Op::Gt),
        ">=" => Ok(Op::Ge),
        "in" => Ok(Op::In),
        "contains" => Ok(Op::Contains),
        "=~" => Ok(Op::Matches),
        _ => Err(JsonPathError::InvalidJsonPath(format!("Invalid operator: {}", op_str))),
    }
}

fn parse_path_or_literal(s: &str) -> Result<JSONFilterExpr, JsonPathError> {
    let s = s.trim();
    if s.starts_with('@') {
        let relative = &s[1..];
        let path_tokens = parse_relative_jsonpath(relative)?;
        Ok(JSONFilterExpr::Path(path_tokens))
    } else {
        let literal = parse_literal(s)?;
        Ok(JSONFilterExpr::Literal(literal))
    }
}

fn parse_literal(s: &str) -> Result<LoroValue, JsonPathError> {
    let s = s.trim();
    if s.starts_with('\'') && s.ends_with('\'') {
        let literal = &s[1..s.len() - 1];
        Ok(LoroValue::String(literal.into()))
    } else if s.starts_with('"') && s.ends_with('"') {
        let literal = &s[1..s.len() - 1];
        Ok(LoroValue::String(literal.into()))
    } else if let Ok(i) = s.parse::<i64>() {
        Ok(LoroValue::I64(i))
    } else if let Ok(f) = s.parse::<f64>() {
        Ok(LoroValue::Double(f))
    } else if s == "true" {
        Ok(LoroValue::Bool(true))
    } else if s == "false" {
        Ok(LoroValue::Bool(false))
    } else if s == "null" {
        Ok(LoroValue::Null)
    } else {
        Err(JsonPathError::InvalidJsonPath(format!("Invalid literal: {}", s)))
    }
}

fn create_filter_predicate(expr: JSONFilterExpr) -> impl Fn(&ValueOrHandler) -> bool + Send + Sync {
    move |current: &ValueOrHandler| -> bool {
        eval_filter_expr(current, &expr)
    }
}

fn eval_filter_expr(current: &ValueOrHandler, expr: &JSONFilterExpr) -> bool {
    match expr {
        JSONFilterExpr::Literal(val) => matches!(current.as_value(), Some(v) if v == val),
        JSONFilterExpr::Path(path) => {
            let mut results = Vec::new();
            evaluate_tokens(current, path, &mut results);
            results.len() == 1 && matches!(&results[0], ValueOrHandler::Value(val) if !val.is_null())
        }
        JSONFilterExpr::LogicalAnd(left, right) => {
            eval_filter_expr(current, left) && eval_filter_expr(current, right)
        }
        JSONFilterExpr::LogicalOr(left, right) => {
            eval_filter_expr(current, left) || eval_filter_expr(current, right)
        }
        JSONFilterExpr::Not(inner) => !eval_filter_expr(current, inner),
        JSONFilterExpr::Comparison { left, op, right } => {
            let left_val = eval_filter_to_value(current, left);
            let right_val = match right.as_ref() {
                JSONFilterExpr::Literal(val) => Some(val.clone()),
                _ => eval_filter_to_value(current, right),
            };
            if let (Some(left_val), Some(right_val)) = (left_val, right_val) {
                compare(&left_val, &right_val, *op)
            } else {
                false
            }
        }
    }
}

fn eval_filter_to_value(current: &ValueOrHandler, expr: &JSONFilterExpr) -> Option<LoroValue> {
    match expr {
        JSONFilterExpr::Literal(val) => Some(val.clone()),
        JSONFilterExpr::Path(path) => {
            let mut results = Vec::new();
            evaluate_tokens(current, path, &mut results);
            results.first().and_then(|r| r.as_value().cloned())
        }
        JSONFilterExpr::Comparison { left, op: _, right } => {
            // For nested comparisons, evaluate the primary path
            eval_filter_to_value(current, left)
        }
        _ => None,
    }
}

pub fn evaluate_jsonpath(doc: &dyn PathValue, path: &str) -> Result<Vec<ValueOrHandler>, JsonPathError> {
    let tokens = parse_jsonpath(path)?;
    let mut results = Vec::new();
    if let Some(JSONPathToken::Root) = tokens.first() {
        evaluate_tokens(doc, &tokens[1..], &mut results);
    } else {
        return Err(JsonPathError::InvalidJsonPath("JSONPath must start with $".to_string()));
    }
    Ok(results)
}

fn evaluate_tokens(value: &dyn PathValue, tokens: &[JSONPathToken], results: &mut Vec<ValueOrHandler>) {
    if tokens.is_empty() {
        if let Ok(cloned) = value.clone_this() {
            results.push(cloned);
        }
        return;
    }
    match &tokens[0] {
        JSONPathToken::Child(key) => {
            if let Some(child) = value.get_by_key(key) {
                evaluate_tokens(&child, &tokens[1..], results);
            }
        }
        JSONPathToken::RecursiveDescend => {
            evaluate_tokens(value, &tokens[1..], results);
            value.for_each_for_path(&mut |child| {
                evaluate_tokens(&child, tokens, results);
                ControlFlow::Continue(())
            });
        }
        JSONPathToken::Wildcard => {
            value.for_each_for_path(&mut |child| {
                evaluate_tokens(&child, &tokens[1..], results);
                ControlFlow::Continue(())
            });
        }
        JSONPathToken::Union(parts) => {
            for part in parts {
                match part {
                    UnionPart::Index(idx) => {
                        if let Some(child) = value.get_by_index(*idx) {
                            evaluate_tokens(&child, &tokens[1..], results);
                        }
                    }
                    UnionPart::Key(key) => {
                        if let Some(child) = value.get_by_key(key) {
                            evaluate_tokens(&child, &tokens[1..], results);
                        }
                    }
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
            value.for_each_for_path(&mut |child| {
                if predicate(&child) {
                    evaluate_tokens(&child, &tokens[1..], results);
                }
                ControlFlow::Continue(())
            });
        }
        JSONPathToken::Root => panic!("Unexpected root token in path"),
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
    use crate::LoroValue;

    fn setup_test_doc() -> LoroDoc {
        let doc = LoroDoc::new();
        doc.start_auto_commit();
        let map = doc.get_map("map");

        let books = map.insert_container("books", ListHandler::new_detached()).unwrap();

        // Book 1: 1984
        let book = books.insert_container(0, MapHandler::new_detached()).unwrap();
        book.insert("title", "1984").unwrap();
        book.insert("author", "George Orwell").unwrap();
        book.insert("price", 10).unwrap();
        book.insert("available", true).unwrap();

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

        // Book 4: Fahrenheit 451
        let book = books.insert_container(3, MapHandler::new_detached()).unwrap();
        book.insert("title", "Fahrenheit 451").unwrap();
        book.insert("author", "Ray Bradbury").unwrap();
        book.insert("price", 9).unwrap();
        book.insert("available", true).unwrap();

        // Book 5: The Great Gatsby
        let book = books.insert_container(4, MapHandler::new_detached()).unwrap();
        book.insert("title", "The Great Gatsby").unwrap();
        book.insert("author", "F. Scott Fitzgerald").unwrap();
        book.insert("price", LoroValue::Null).unwrap();
        book.insert("available", true).unwrap();

        doc
    }

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
                JSONPathToken::Union(vec![UnionPart::Index(0)]),
                JSONPathToken::Child("title".to_string()),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_basic_jsonpath() -> Result<(), JsonPathError> {
        let doc = LoroDoc::new();
        doc.start_auto_commit();
        let map = doc.get_map("map");
        map.insert("key", "value").unwrap();
        let books = map.insert_container("books", ListHandler::new_detached()).unwrap();
        let book = books.insert_container(0, MapHandler::new_detached()).unwrap();
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
    fn test_jsonpath_selectors() -> Result<(), JsonPathError> {
        let doc = setup_test_doc();

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
        assert_eq!(result.len(), 5);

        // Test recursive descent
        let path = "$..title";
        let result = evaluate_jsonpath(&doc, path).unwrap();
        assert_eq!(result.len(), 5);

        // Test quoted keys
        let path = "$['map']['books'][0]['title']";
        let result = evaluate_jsonpath(&doc, path).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "1984"
        );
        Ok(())
    }

    #[test]
    fn test_string_filters() -> Result<(), JsonPathError> {
        let doc = setup_test_doc();

        // Test exact string match
        let result = evaluate_jsonpath(&doc, "$['map'].books[?(@.title == '1984')]")?;
        assert_eq!(result.len(), 1);

        // Test string contains
        let result = evaluate_jsonpath(&doc, "$['map'].books[?(@.title contains 'Farm')]")?;
        assert_eq!(result.len(), 1);

        // // Test string comparison (lexicographical)
        // let result = evaluate_jsonpath(&doc, "$['map'].books[?(@.title > 'A')]")?;
        // assert_eq!(result.len(), 5); // All titles start with letters > 'A'

        // // Test case-sensitive string comparison
        // let result = evaluate_jsonpath(&doc, "$['map'].books[?(@.title < 'Fahrenheit 451')]")?;
        // assert_eq!(result.len(), 2); // "1984" and "Animal Farm"

        Ok(())
    }

    #[test]
    fn test_logical_operators() -> Result<(), JsonPathError> {
        let doc = setup_test_doc();

        // Test AND operator - find cheap Orwell books
        let result = evaluate_jsonpath(
            &doc,
            "$['map'].books[?(@.author == 'George Orwell' && @.price < 10)]",
        )?;
        assert_eq!(result.len(), 1); // Animal Farm

        // Test OR operator - Orwell books OR expensive books
        let result = evaluate_jsonpath(
            &doc,
            "$['map'].books[?(@.author == 'George Orwell' || @.price >= 10)]",
        )?;
        assert_eq!(result.len(), 3); // 2 Orwell + 1 expensive

        // Test complex AND/OR combination
        let result = evaluate_jsonpath(
            &doc,
            "$['map'].books[?(@.author == 'George Orwell' && (@.price < 10 || @.available == true))]",
        )?;
        assert_eq!(result.len(), 2); // Both Orwell books

        // Test NOT operator
        let result = evaluate_jsonpath(
            &doc,
            "$['map'].books[?(!(@.available == false))]",
        )?;
        assert_eq!(result.len(), 4); // All available books

        // // Test NOT with complex condition
        // let result = evaluate_jsonpath(
        //     &doc,
        //     "$['map'].books[?(!(@.price >= 10 && @.available == false))]",
        // )?;
        // assert_eq!(result.len(), 5); // Everything except expensive unavailable books
        Ok(())
    }

    #[test]
    fn test_union_operations() -> Result<(), JsonPathError> {
        let doc = setup_test_doc();

        // Test union indexes
        let result = evaluate_jsonpath(&doc, "$['map'].books[0,2].title")?;
        assert_eq!(result.len(), 2);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "1984"
        );
        assert_eq!(
            &**result[1].as_value().unwrap().as_string().unwrap(),
            "Brave New World"
        );

        // Test union keys
        let result = evaluate_jsonpath(&doc, "$['map'].books[0]['title','author']")?;
        assert_eq!(result.len(), 2);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "1984"
        );
        assert_eq!(
            &**result[1].as_value().unwrap().as_string().unwrap(),
            "George Orwell"
        );

        // Test mixed union (indexes and quoted keys)
        let result = evaluate_jsonpath(&doc, "$['map'].books[0]['title',1]")?;
        assert!(result.len() >= 1);

        // Test union with negative indexes
        let result = evaluate_jsonpath(&doc, "$['map'].books[-1,-2].title")?;
        assert_eq!(result.len(), 2);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "The Great Gatsby"
        );
        Ok(())
    }

    #[test]
    fn test_slice_operations() -> Result<(), JsonPathError> {
        let doc = setup_test_doc();

        // Test basic slice
        let result = evaluate_jsonpath(&doc, "$['map'].books[0:3].title")?;
        assert_eq!(result.len(), 3);

        // Test slice with step
        let result = evaluate_jsonpath(&doc, "$['map'].books[0:5:2].title")?;
        assert_eq!(result.len(), 3); // 0, 2, 4

        // Test negative slice
        let result = evaluate_jsonpath(&doc, "$['map'].books[-2:].title")?;
        assert_eq!(result.len(), 2);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "Fahrenheit 451"
        );

        Ok(())
    }

    #[test]
    fn test_complex_filters() -> Result<(), JsonPathError> {
        let doc = setup_test_doc();

        // Complex filter with multiple conditions
        let path = "$['map'].books[?(@.price >= 10 && @.available == true && @.title contains '1984')]";
        let result = evaluate_jsonpath(&doc, path)?;
        assert_eq!(result.len(), 1);

        // Filter with path expressions
        let path = "$['map'].books[?(@.author == 'George Orwell' && @.title != '1984')]";
        let result = evaluate_jsonpath(&doc, path)?;
        assert_eq!(result.len(), 1); // Only Animal Farm

        // Filter with null checks
        let path = "$['map'].books[?(@.price == null || @.price < 10)]";
        let result = evaluate_jsonpath(&doc, path)?;
        assert_eq!(result.len(), 3); // Gatsby + cheap books
        Ok(())
    }

    #[test]
    fn test_recursive_filters() -> Result<(), JsonPathError> {
        let doc = setup_test_doc();

        // Recursive filter
        let result = evaluate_jsonpath(&doc, "$..[?(@.price > 10)]")?;
        assert_eq!(result.len(), 1); // Brave New World

        // Recursive filter with logical operators
        let result = evaluate_jsonpath(&doc, "$..[?(@.author == 'George Orwell' || @.price > 10)]")?;
        assert!(result.len() >= 3);

        Ok(())
    }

    // #[test]
    // fn test_edge_cases() -> Result<(), JsonPathError> {
    //     let doc = setup_test_doc();
    //
    //     // Empty filter should match nothing
    //     let result = evaluate_jsonpath(&doc, "$['map'].books[?(@.nonexistent == 'foo')]")?;
    //     assert_eq!(result.len(), 0);
    //
    //     // Filter with missing property
    //     let result = evaluate_jsonpath(&doc, "$['map'].books[?(@.missing == null)]")?;
    //     assert_eq!(result.len(), 5); // All books match since missing == null
    //
    //     // Boolean filters
    //     let result = evaluate_jsonpath(&doc, "$['map'].books[?(@.available == true)]")?;
    //     assert_eq!(result.len(), 4);
    //
    //     Ok(())
    // }

    #[test]
    fn test_quoted_keys_with_special_chars() -> Result<(), JsonPathError> {
        let doc = LoroDoc::new();
        doc.start_auto_commit();
        let map = doc.get_map("map");
        let book = map.insert_container("book-with-dash", MapHandler::new_detached()).unwrap();
        book.insert("price-$10", "cheap").unwrap();

        // Test quoted key with special characters
        let path = "$['map']['book-with-dash']['price-$10']";
        let result = evaluate_jsonpath(&doc, path).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            &**result[0].as_value().unwrap().as_string().unwrap(),
            "cheap"
        );
        Ok(())
    }
}