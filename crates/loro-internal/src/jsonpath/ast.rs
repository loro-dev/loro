use crate::jsonpath::errors::JSONPathError;
use crate::jsonpath::JSONPathParser;
use std::fmt::{self, Write};

#[derive(Debug, Clone)]
pub struct Query {
    pub segments: Segment,
}

#[derive(Debug, Clone)]
pub enum Segment {
    Root {},
    Child {
        left: Box<Segment>,
        selectors: Vec<Selector>,
    },
    Recursive {
        left: Box<Segment>,
        selectors: Vec<Selector>,
    },
}

#[derive(Debug, Clone)]
pub enum Selector {
    Name {
        name: String,
    },
    Index {
        index: i64,
    },
    Slice {
        start: Option<i64>,
        stop: Option<i64>,
        step: Option<i64>,
    },
    Wild {},
    Filter {
        expression: Box<FilterExpression>,
    },
}

#[derive(Debug, Clone)]
pub enum FilterExpression {
    True_ {},
    False_ {},
    Null {},
    Array {
        values: Vec<FilterExpression>,
    },
    StringLiteral {
        value: String,
    },
    Int {
        value: i64,
    },
    Float {
        value: f64,
    },
    Not {
        expression: Box<FilterExpression>,
    },
    Logical {
        left: Box<FilterExpression>,
        operator: LogicalOperator,
        right: Box<FilterExpression>,
    },
    Comparison {
        left: Box<FilterExpression>,
        operator: ComparisonOperator,
        right: Box<FilterExpression>,
    },
    RelativeQuery {
        query: Box<Query>,
    },
    RootQuery {
        query: Box<Query>,
    },
    Function {
        name: String,
        args: Vec<FilterExpression>,
    },
}

#[derive(Debug, Clone)]
pub enum LogicalOperator {
    And,
    Or,
}

#[derive(Debug, Clone)]
pub enum ComparisonOperator {
    Eq,
    Ne,
    Ge,
    Gt,
    Le,
    Lt,
    Contains,
    In,
}

impl Query {
    pub fn standard(expr: &str) -> Result<Self, JSONPathError> {
        JSONPathParser::new().parse(expr)
    }

    pub fn is_singular(&self) -> bool {
        self.segments.is_singular()
    }
}

impl Segment {
    // Returns `true` if this query can resolve to at most one node, or `false` otherwise.
    pub fn is_singular(&self) -> bool {
        match self {
            Segment::Child { left, selectors } => {
                selectors.len() == 1
                    && selectors.first().is_some_and(|selector| {
                        matches!(selector, Selector::Name { .. } | Selector::Index { .. })
                    })
                    && left.is_singular()
            }
            Segment::Recursive { .. } => false,
            Segment::Root {} => true,
        }
    }
}

impl FilterExpression {
    pub fn is_literal(&self) -> bool {
        matches!(
            self,
            FilterExpression::True_ { .. }
                | FilterExpression::False_ { .. }
                | FilterExpression::Null { .. }
                | FilterExpression::StringLiteral { .. }
                | FilterExpression::Int { .. }
                | FilterExpression::Float { .. }
                | FilterExpression::Array { .. }
        )
    }
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.segments)
    }
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Segment::Child { left, selectors } => write!(
                f,
                "{}[{}]",
                left,
                selectors
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
                    .join(", ")
            ),
            Segment::Recursive { left, selectors } => write!(
                f,
                "{}..[{}]",
                left,
                selectors
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
                    .join(", ")
            ),
            Segment::Root {} => Ok(()),
        }
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Selector::Name { name, .. } => write!(f, "'{name}'"),
            Selector::Index {
                index: array_index, ..
            } => write!(f, "{array_index}"),
            Selector::Slice {
                start, stop, step, ..
            } => {
                write!(
                    f,
                    "{}:{}:{}",
                    start
                        .and_then(|i| Some(i.to_string()))
                        .unwrap_or(String::from("")),
                    stop.and_then(|i| Some(i.to_string()))
                        .unwrap_or(String::from("")),
                    step.and_then(|i| Some(i.to_string()))
                        .unwrap_or(String::from("1")),
                )
            }
            Selector::Wild { .. } => f.write_char('*'),
            Selector::Filter { expression, .. } => write!(f, "?{expression}"),
        }
    }
}

impl fmt::Display for FilterExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilterExpression::True_ { .. } => f.write_str("true"),
            FilterExpression::False_ { .. } => f.write_str("false"),
            FilterExpression::Null { .. } => f.write_str("null"),
            FilterExpression::StringLiteral { value, .. } => write!(f, "'{value}'"),
            FilterExpression::Array { values, .. } => {
                write!(
                    f,
                    "[{}]",
                    values
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<String>>()
                        .join(", ")
                )
            }
            FilterExpression::Int { value, .. } => write!(f, "{value}"),
            FilterExpression::Float { value, .. } => write!(f, "{value}"),
            FilterExpression::Not { expression, .. } => write!(f, "!{expression}"),
            FilterExpression::Logical {
                left,
                operator,
                right,
                ..
            } => write!(f, "({left} {operator} {right})"),
            FilterExpression::Comparison {
                left,
                operator,
                right,
                ..
            } => write!(f, "{left} {operator} {right}"),
            FilterExpression::RelativeQuery { query, .. } => {
                write!(f, "@{}", query.segments)
            }
            FilterExpression::RootQuery { query, .. } => {
                write!(f, "${}", query.segments)
            }
            FilterExpression::Function { name, args, .. } => {
                write!(
                    f,
                    "{}({})",
                    name,
                    args.iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<String>>()
                        .join(", ")
                )
            }
        }
    }
}

impl fmt::Display for LogicalOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogicalOperator::And => f.write_str("&&"),
            LogicalOperator::Or => f.write_str("||"),
        }
    }
}

impl fmt::Display for ComparisonOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComparisonOperator::Eq => f.write_str("=="),
            ComparisonOperator::Ne => f.write_str("!="),
            ComparisonOperator::Ge => f.write_str(">="),
            ComparisonOperator::Gt => f.write_str(">"),
            ComparisonOperator::Le => f.write_str("<="),
            ComparisonOperator::Lt => f.write_str("<"),
            ComparisonOperator::Contains => f.write_str("contains"),
            ComparisonOperator::In => f.write_str("in"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(expr: &str) -> Query {
        Query::standard(expr).unwrap()
    }

    fn relative_query(selector: Selector) -> Query {
        Query {
            segments: Segment::Child {
                left: Box::new(Segment::Root {}),
                selectors: vec![selector],
            },
        }
    }

    #[test]
    fn query_standard_reports_singular_queries_correctly() {
        let cases = [
            ("$", true),
            ("$.store", true),
            ("$['store']", true),
            ("$.store.books[0]", true),
            ("$.store.books[0][1]", true),
            ("$.store.books[0, 1]", false),
            ("$.store.books[*]", false),
            ("$.store.books[?(@.available == true)]", false),
            ("$.store..title", false),
        ];

        for (expr, expected) in cases {
            assert_eq!(
                parse(expr).is_singular(),
                expected,
                "{expr} should have singular={expected}"
            );
        }
    }

    #[test]
    fn filter_expression_is_literal_matches_jsonpath_contract() {
        let literal_cases = [
            FilterExpression::True_ {},
            FilterExpression::False_ {},
            FilterExpression::Null {},
            FilterExpression::StringLiteral {
                value: "hello".to_owned(),
            },
            FilterExpression::Int { value: 42 },
            FilterExpression::Float { value: 3.5 },
            FilterExpression::Array {
                values: vec![
                    FilterExpression::Int { value: 1 },
                    FilterExpression::StringLiteral {
                        value: "two".to_owned(),
                    },
                    FilterExpression::Array {
                        values: vec![FilterExpression::Null {}],
                    },
                ],
            },
        ];

        for expr in literal_cases {
            assert!(expr.is_literal(), "{expr:?} should be literal");
        }

        let non_literal_cases = [
            FilterExpression::RelativeQuery {
                query: Box::new(relative_query(Selector::Name {
                    name: "title".to_owned(),
                })),
            },
            FilterExpression::RootQuery {
                query: Box::new(parse("$.store.books[0]")),
            },
            FilterExpression::Function {
                name: "count".to_owned(),
                args: vec![FilterExpression::RelativeQuery {
                    query: Box::new(relative_query(Selector::Name {
                        name: "title".to_owned(),
                    })),
                }],
            },
            FilterExpression::Not {
                expression: Box::new(FilterExpression::True_ {}),
            },
            FilterExpression::Logical {
                left: Box::new(FilterExpression::True_ {}),
                operator: LogicalOperator::And,
                right: Box::new(FilterExpression::False_ {}),
            },
            FilterExpression::Comparison {
                left: Box::new(FilterExpression::Int { value: 1 }),
                operator: ComparisonOperator::Eq,
                right: Box::new(FilterExpression::Int { value: 2 }),
            },
        ];

        for expr in non_literal_cases {
            assert!(!expr.is_literal(), "{expr:?} should not be literal");
        }
    }

    #[test]
    fn query_and_ast_display_are_stable() {
        let query = parse("$.store.books[0]");
        assert_eq!(query.to_string(), "$['store']['books'][0]");
        assert_eq!(
            parse("$.store.books[0, 1]").to_string(),
            "$['store']['books'][0, 1]"
        );
        assert_eq!(parse("$.store..title").to_string(), "$['store']..['title']");

        let selector_display = [
            (
                Selector::Name {
                    name: "field".into(),
                },
                "'field'",
            ),
            (Selector::Index { index: -2 }, "-2"),
            (
                Selector::Slice {
                    start: Some(1),
                    stop: Some(3),
                    step: None,
                },
                "1:3:1",
            ),
            (Selector::Wild {}, "*"),
        ];

        for (selector, expected) in selector_display {
            assert_eq!(selector.to_string(), expected);
        }

        let filter_display = FilterExpression::Logical {
            left: Box::new(FilterExpression::Comparison {
                left: Box::new(FilterExpression::RelativeQuery {
                    query: Box::new(relative_query(Selector::Name {
                        name: "available".to_owned(),
                    })),
                }),
                operator: ComparisonOperator::Eq,
                right: Box::new(FilterExpression::True_ {}),
            }),
            operator: LogicalOperator::And,
            right: Box::new(FilterExpression::Not {
                expression: Box::new(FilterExpression::Function {
                    name: "count".to_owned(),
                    args: vec![FilterExpression::RootQuery {
                        query: Box::new(parse("$.store.books[*]")),
                    }],
                }),
            }),
        };
        assert_eq!(
            filter_display.to_string(),
            "(@['available'] == true && !count($['store']['books'][*]))"
        );

        assert_eq!(LogicalOperator::And.to_string(), "&&");
        assert_eq!(LogicalOperator::Or.to_string(), "||");
        assert_eq!(ComparisonOperator::Eq.to_string(), "==");
        assert_eq!(ComparisonOperator::Ne.to_string(), "!=");
        assert_eq!(ComparisonOperator::Ge.to_string(), ">=");
        assert_eq!(ComparisonOperator::Gt.to_string(), ">");
        assert_eq!(ComparisonOperator::Le.to_string(), "<=");
        assert_eq!(ComparisonOperator::Lt.to_string(), "<");
        assert_eq!(ComparisonOperator::Contains.to_string(), "contains");
        assert_eq!(ComparisonOperator::In.to_string(), "in");
    }
}
