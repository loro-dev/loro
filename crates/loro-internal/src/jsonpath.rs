use loro_common::{ContainerID, LoroValue, LoroListValue};
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
    Filter(Arc<dyn Fn(&dyn PathValue, &ValueOrHandler) -> bool + Send + Sync>),
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

fn values_equal(a: &LoroValue, b: &LoroValue) -> bool {
    match (a, b) {
        (LoroValue::I64(x), LoroValue::I64(y)) => x == y,
        (LoroValue::Double(x), LoroValue::Double(y)) => (x - y).abs() < f64::EPSILON,
        (LoroValue::String(x), LoroValue::String(y)) => x.as_str() == y.as_str(),
        (LoroValue::Bool(x), LoroValue::Bool(y)) => x == y,
        (LoroValue::Null, LoroValue::Null) => true,
        (LoroValue::List(x), LoroValue::List(y)) => {
            if x.len() != y.len() {
                return false;
            }
            x.iter().zip(y.iter()).all(|(aa, bb)| values_equal(aa, bb))
        }
        (LoroValue::Map(x), LoroValue::Map(y)) => {
            if x.len() != y.len() {
                return false;
            }
            for (k, v) in x.iter() {
                if let Some(vv) = y.get(k) {
                    if !values_equal(v, vv) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
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
        (I64(x), List(list), Op::In) => {
            list.iter().any(|v| matches!(v, I64(n) if n == x))
        }
        (Double(x), List(list), Op::In) => {
            list.iter().any(|v| matches!(v, Double(n) if (n - x).abs() < f64::EPSILON))
        }
        (Null, List(list), Op::In) => {
            list.iter().any(|v| matches!(v, Null))
        }
        (Bool(x), List(list), Op::In) => {
            list.iter().any(|v| matches!(v, Bool(b) if b == x))
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

fn unescape_string(inner: &str) -> Result<String, JsonPathError> {
    let mut result = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(&c) = chars.peek() {
        chars.next();
        if c == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    'b' => result.push('\x08'),
                    'f' => result.push('\x0c'),
                    'n' => result.push('\n'),
                    'r' => result.push('\r'),
                    't' => result.push('\t'),
                    '"' => result.push('"'),
                    '\'' => result.push('\''),
                    '\\' => result.push('\\'),
                    '/' => result.push('/'),
                    'u' => {
                        let mut hex = String::with_capacity(4);
                        for _ in 0..4 {
                            if let Some(h) = chars.next() {
                                if h.is_ascii_hexdigit() {
                                    hex.push(h);
                                } else {
                                    return Err(JsonPathError::InvalidJsonPath(format!("Invalid hex digit in \\u: {}", h)));
                                }
                            } else {
                                return Err(JsonPathError::InvalidJsonPath("Incomplete \\u escape".to_string()));
                            }
                        }
                        let code = u32::from_str_radix(&hex, 16).map_err(|_| JsonPathError::InvalidJsonPath("Invalid \\u code".to_string()))?;
                        if let Some(ch) = char::from_u32(code) {
                            result.push(ch);
                        } else {
                            return Err(JsonPathError::InvalidJsonPath(format!("Invalid Unicode code point: {}", code)));
                        }
                    }
                    _ => return Err(JsonPathError::InvalidJsonPath(format!("Invalid escape: \\{}", next))),
                }
            } else {
                return Err(JsonPathError::InvalidJsonPath("Incomplete escape sequence".to_string()));
            }
        } else {
            result.push(c);
        }
    }
    Ok(result)
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
            c if !expect_root && (c.is_alphabetic() || c == '_') => {
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
            c if c.is_alphabetic() || c == '_' => {
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
    let key = parse_identifier(chars, i)?;
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
    let content = parse_bracket_content(chars, i)?;
    let content = content.trim();
    if content.is_empty() {
        return Err(JsonPathError::InvalidJsonPath("Empty bracket content".to_string()));
    }
    if content == "*" {
        tokens.push(JSONPathToken::Wildcard);
    } else if content.contains(':') && !content.contains('?') {
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
            } else if part.starts_with('\'') || part.starts_with('"') {
                let key = unescape_string(&part[1..part.len() - 1])?;
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
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
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
            '\'' if !in_double_quote && (*i == 0 || chars.get(*i - 1) != Some(&'\\')) => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote && (*i == 0 || chars.get(*i - 1) != Some(&'\\')) => {
                in_double_quote = !in_double_quote;
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
    let start_i = *i;
    while *i < chars.len() {
        let c = chars[*i];
        if key.is_empty() {
            if !(c.is_alphabetic() || c == '_') {
                break;
            }
        } else if !(c.is_alphanumeric() || c == '_' || c == '-') {
            break;
        }
        key.push(c);
        *i += 1;
    }
    if key.is_empty() && *i > start_i {
        return Err(JsonPathError::InvalidJsonPath("Invalid identifier start".to_string()));
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

fn parse_slice(content: &str) -> Result<(Option<isize>, Option<isize>, Option<isize>), JsonPathError> {
    let parts: Vec<&str> = content.split(':').collect();
    let start = if parts[0].is_empty() { None } else { parts[0].trim().parse().ok() };
    let end = if parts.len() > 1 && !parts[1].trim().is_empty() { parts[1].trim().parse().ok() } else { None };
    let step = if parts.len() > 2 && !parts[2].trim().is_empty() { parts[2].trim().parse().ok() } else { None };
    Ok((start, end, step))
}

fn parse_array(s: &str) -> Result<LoroValue, JsonPathError> {
    let s = s.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return Err(JsonPathError::InvalidJsonPath(format!("Invalid array literal: {}", s)));
    }

    let content = &s[1..s.len() - 1].trim();
    if content.is_empty() {
        return Ok(LoroValue::List(Default::default()));
    }

    let mut items = Vec::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '\0';
    let mut bracket_depth = 0;

    while i < chars.len() {
        let c = chars[i];

        if c == '\\' && in_quotes && i + 1 < chars.len() {
            current.push(c);
            current.push(chars[i + 1]);
            i += 2;
            continue;
        }

        match c {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = c;
                current.push(c);
            }
            c if c == quote_char && in_quotes => {
                in_quotes = false;
                current.push(c);
            }
            '[' if !in_quotes => {
                bracket_depth += 1;
                current.push(c);
            }
            ']' if !in_quotes => {
                bracket_depth -= 1;
                current.push(c);
            }
            ',' if !in_quotes && bracket_depth == 0 => {
                items.push(parse_literal(&current.trim())?);
                current.clear();
            }
            _ => {
                current.push(c);
            }
        }
        i += 1;
    }

    if bracket_depth != 0 {
        return Err(JsonPathError::InvalidJsonPath("Unmatched brackets in array".to_string()));
    }
    if in_quotes {
        return Err(JsonPathError::InvalidJsonPath("Unclosed quote in array".to_string()));
    }
    if !current.is_empty() {
        items.push(parse_literal(&current.trim())?);
    }

    Ok(LoroValue::List(LoroListValue::from(items)))
}

fn parse_filter_expression(predicate: &str) -> Result<JSONFilterExpr, JsonPathError> {
    let mut predicate = predicate.trim();
    if predicate.starts_with('(') && predicate.ends_with(')') {
        predicate = &predicate[1..predicate.len() - 1].trim();
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
            _ => unreachable!(),
        }
    }
    // Handle NOT operator
    if predicate.starts_with('!') || predicate.starts_with("not ") {
        let inner = if predicate.starts_with('!') {
            &predicate[1..].trim()
        } else {
            &predicate[4..].trim()
        };
        let inner_expr = parse_filter_expression(inner)?;
        return Ok(JSONFilterExpr::Not(Box::new(inner_expr)));
    }
    // Handle comparisons
    match parse_comparison(predicate) {
        Ok((left, op_str, right)) => {
            let left_expr = parse_path_or_literal(left)?;
            let right_expr = parse_path_or_literal(right)?;
            if let JSONFilterExpr::Path(ref tokens) = left_expr {
                if !is_singular(tokens) {
                    return Err(JsonPathError::InvalidJsonPath("Non-singular query in comparison left".to_string()));
                }
            }
            if let JSONFilterExpr::Path(ref tokens) = right_expr {
                if !is_singular(tokens) {
                    return Err(JsonPathError::InvalidJsonPath("Non-singular query in comparison right".to_string()));
                }
            }
            let op = parse_operator(op_str)?;
            Ok(JSONFilterExpr::Comparison {
                left: Box::new(left_expr),
                op,
                right: Box::new(right_expr),
            })
        }
        Err(e) => Err(e),
    }
}

fn is_singular(tokens: &[JSONPathToken]) -> bool {
    let start = if matches!(tokens.first(), Some(&JSONPathToken::Root)) { 1 } else { 0 };
    for token in &tokens[start..] {
        match token {
            JSONPathToken::Child(_) => (),
            JSONPathToken::Union(parts) if parts.len() == 1 => (),
            _ => return false,
        }
    }
    true
}

fn find_highest_precedence_operator(s: &str, operators: &[&str]) -> Option<usize> {
    let mut min_pos = usize::MAX;
    let mut selected = None;
    for &op in operators {
        if let Some(pos) = s.find(op) {
            if pos < min_pos {
                min_pos = pos;
                selected = Some(op);
            }
        }
    }
    if selected.is_some() {
        Some(min_pos)
    } else {
        None
    }
}

fn split_at_operator<'a>(s: &'a str, pos: usize, operators: &[&'a str]) -> Result<(&'a str, &'a str, &'a str), JsonPathError> {
    for &op in operators {
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
    let mut min_pos = usize::MAX;
    let mut selected_op = "";
    for op in ops {
        if let Some(pos) = predicate.find(op) {
            if pos < min_pos {
                min_pos = pos;
                selected_op = op;
            }
        }
    }
    if !selected_op.is_empty() {
        let left = predicate[..min_pos].trim();
        let right = predicate[min_pos + selected_op.len()..].trim();
        if !left.is_empty() && !right.is_empty() {
            return Ok((left, selected_op, right));
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
    } else if s.starts_with('$') {
        let path_tokens = parse_jsonpath(s)?;
        Ok(JSONFilterExpr::Path(path_tokens))
    } else {
        let literal = parse_literal(s)?;
        Ok(JSONFilterExpr::Literal(literal))
    }
}

fn parse_literal(s: &str) -> Result<LoroValue, JsonPathError> {
    let s = s.trim();
    if s.starts_with('[') && s.ends_with(']') {
        parse_array(s)
    } else if s.starts_with('\'') && s.ends_with('\'') {
        let inner = &s[1..s.len() - 1];
        let unescaped = unescape_string(inner)?;
        Ok(LoroValue::String(unescaped.into()))
    } else if s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        let unescaped = unescape_string(inner)?;
        Ok(LoroValue::String(unescaped.into()))
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

fn create_filter_predicate(expr: JSONFilterExpr) -> impl Fn(&dyn PathValue, &ValueOrHandler) -> bool + Send + Sync {
    move |root: &dyn PathValue, current: &ValueOrHandler| -> bool {
        eval_filter_expr(root, current, &expr)
    }
}

fn eval_filter_expr(root: &dyn PathValue, current: &ValueOrHandler, expr: &JSONFilterExpr) -> bool {
    match expr {
        JSONFilterExpr::Literal(val) => current.as_value().map_or(false, |v| values_equal(v, val)),
        JSONFilterExpr::Path(path) => {
            let mut results = Vec::new();
            let target = if !path.is_empty() && matches!(path[0], JSONPathToken::Root) {
                root
            } else {
                current as &dyn PathValue
            };
            let path_slice = if !path.is_empty() && matches!(path[0], JSONPathToken::Root) { &path[1..] } else { path };
            evaluate_tokens(root, target, path_slice, &mut results);
            !results.is_empty()
        }
        JSONFilterExpr::LogicalAnd(left, right) => {
            eval_filter_expr(root, current, left) && eval_filter_expr(root, current, right)
        }
        JSONFilterExpr::LogicalOr(left, right) => {
            eval_filter_expr(root, current, left) || eval_filter_expr(root, current, right)
        }
        JSONFilterExpr::Not(inner) => !eval_filter_expr(root, current, inner),
        JSONFilterExpr::Comparison { left, op, right } => {
            let left_val = eval_filter_to_value(root, current, left);
            let right_val = eval_filter_to_value(root, current, right);
            match (left_val, right_val) {
                (Some(a), Some(b)) => compare(&a, &b, *op),
                (None, None) => match op {
                    Op::Eq => true,
                    Op::Neq => false,
                    _ => false,
                },
                _ => false,
            }
        }
    }
}

fn eval_filter_to_value(root: &dyn PathValue, current: &ValueOrHandler, expr: &JSONFilterExpr) -> Option<LoroValue> {
    match expr {
        JSONFilterExpr::Literal(val) => Some(val.clone()),
        JSONFilterExpr::Path(path) => {
            let mut results = Vec::new();
            let target = if !path.is_empty() && matches!(path[0], JSONPathToken::Root) {
                root
            } else {
                current as &dyn PathValue
            };
            let path_slice = if !path.is_empty() && matches!(path[0], JSONPathToken::Root) { &path[1..] } else { path };
            evaluate_tokens(root, target, path_slice, &mut results);
            if results.len() == 1 {
                results[0].as_value().cloned()
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn evaluate_jsonpath(doc: &dyn PathValue, path: &str) -> Result<Vec<ValueOrHandler>, JsonPathError> {
    let tokens = parse_jsonpath(path)?;
    let mut results = Vec::new();
    if tokens.first() == Some(&JSONPathToken::Root) {
        evaluate_tokens(doc, doc, &tokens[1..], &mut results);
    } else {
        return Err(JsonPathError::InvalidJsonPath("JSONPath must start with $".to_string()));
    }
    Ok(results)
}

fn evaluate_tokens(root: &dyn PathValue, value: &dyn PathValue, tokens: &[JSONPathToken], results: &mut Vec<ValueOrHandler>) {
    if tokens.is_empty() {
        if let Ok(cloned) = value.clone_this() {
            results.push(cloned);
        }
        return;
    }
    match &tokens[0] {
        JSONPathToken::Child(key) => {
            if let Some(child) = value.get_by_key(key) {
                evaluate_tokens(root, &child, &tokens[1..], results);
            }
        }
        JSONPathToken::RecursiveDescend => {
            evaluate_tokens(root, value, &tokens[1..], results);
            value.for_each_for_path(&mut |child| {
                evaluate_tokens(root, &child, tokens, results);
                ControlFlow::Continue(())
            });
        }
        JSONPathToken::Wildcard => {
            value.for_each_for_path(&mut |child| {
                evaluate_tokens(root, &child, &tokens[1..], results);
                ControlFlow::Continue(())
            });
        }
        JSONPathToken::Union(parts) => {
            for part in parts {
                match part {
                    UnionPart::Index(idx) => {
                        if let Some(child) = value.get_by_index(*idx) {
                            evaluate_tokens(root, &child, &tokens[1..], results);
                        }
                    }
                    UnionPart::Key(key) => {
                        if let Some(child) = value.get_by_key(key) {
                            evaluate_tokens(root, &child, &tokens[1..], results);
                        }
                    }
                }
            }
        }
        JSONPathToken::Slice(start, end, step) => {
            let len = value.length_for_path() as isize;
            let mut eff_start = start.unwrap_or(if step.unwrap_or(1) >= 0 { 0 } else { len - 1 });
            if eff_start < 0 {
                eff_start += len;
            }
            eff_start = eff_start.max(0).min(len);

            let mut eff_end = end.unwrap_or(if step.unwrap_or(1) >= 0 { len } else { -len - 1 });
            if eff_end < 0 {
                eff_end += len;
            }
            eff_end = eff_end.max(0).min(len);

            let eff_step = step.unwrap_or(1);
            if eff_step == 0 {
                return;
            }
            if eff_step > 0 {
                let mut i = eff_start;
                while i < eff_end {
                    if let Some(child) = value.get_by_index(i) {
                        evaluate_tokens(root, &child, &tokens[1..], results);
                    }
                    i += eff_step;
                }
            } else {
                let mut i = eff_start;
                while i > eff_end {
                    if let Some(child) = value.get_by_index(i) {
                        evaluate_tokens(root, &child, &tokens[1..], results);
                    }
                    i += eff_step;
                }
            }
        }
        JSONPathToken::Filter(predicate) => {
            value.for_each_for_path(&mut |child| {
                if predicate(root, &child) {
                    evaluate_tokens(root, &child, &tokens[1..], results);
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
        featured_authors.push( "Aldous Huxley").unwrap();
        featured_authors.push( "Ray Bradbury").unwrap();
        store.insert("min_price", 10).unwrap();

        doc
    }

    mod basic_jsonpath_parsing {
        use super::*;

        #[test]
        fn parses_basic_path_correctly() -> Result<(), JsonPathError> {
            let path = "$.store.books[0].title";
            let tokens = parse_jsonpath(path)?;
            assert_eq!(
                tokens,
                vec![
                    JSONPathToken::Root,
                    JSONPathToken::Child("store".to_string()),
                    JSONPathToken::Child("books".to_string()),
                    JSONPathToken::Union(vec![UnionPart::Index(0)]),
                    JSONPathToken::Child("title".to_string()),
                ]
            );
            Ok(())
        }
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

    mod in_operator {
        use super::*;

        #[test]
        fn filters_by_author_in_list() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.author in ['George Orwell', 'Jane Austen'])].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 3);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["1984", "Animal Farm", "Pride and Prejudice"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn filters_by_price_in_list() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.price in [7, 10, 14])].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 4);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["1984", "Pride and Prejudice", "The Catcher in the Rye", "The Hobbit"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn filters_with_in_operator_and_null_values() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$.store.books[?(@.price in [null, 9])].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 3);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["Fahrenheit 451", "Lord of the Flies", "The Great Gatsby"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }

        #[test]
        fn filters_with_in_operator_in_recursive_descent() -> Result<(), JsonPathError> {
            let doc = setup_test_doc();
            let path = "$..[?(@.author in ['George Orwell', 'Ray Bradbury'])].title";
            let result = evaluate_jsonpath(&doc, path)?;
            assert_eq!(result.len(), 3);
            let mut titles: Vec<&str> = result
                .iter()
                .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
                .collect();
            titles.sort();
            let mut expected = vec!["1984", "Animal Farm", "Fahrenheit 451"];
            expected.sort();
            assert_eq!(titles, expected);
            Ok(())
        }
    }

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

        // #[test]
        // fn filters_with_root_list_in() -> Result<(), JsonPathError> {
        //     let doc = setup_test_doc();
        //     let path = "$.store.books[?(@.author in $.store.featured_authors)].title";
        //     let result = evaluate_jsonpath(&doc, path)?;
        //     assert_eq!(result.len(), 3);
        //     let mut titles: Vec<&str> = result
        //         .iter()
        //         .map(|v| v.as_value().unwrap().as_string().unwrap().as_str())
        //         .collect();
        //     titles.sort();
        //     let mut expected = vec!["1984", "Animal Farm", "Pride and Prejudice"];
        //     expected.sort();
        //     assert_eq!(titles, expected);
        //     Ok(())
        // }

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
}