use std::fmt;

#[derive(Debug)]
pub enum JSONPathErrorType {
    LexerError,
    SyntaxError,
    TypeError,
    NameError,
}

#[derive(Debug)]
pub struct JSONPathError {
    pub kind: JSONPathErrorType,
    pub msg: String,
}

impl JSONPathError {
    pub fn new(error: JSONPathErrorType, msg: String) -> Self {
        Self { kind: error, msg }
    }

    pub fn syntax(msg: String) -> Self {
        Self {
            kind: JSONPathErrorType::SyntaxError,
            msg,
        }
    }

    pub fn typ(msg: String) -> Self {
        Self {
            kind: JSONPathErrorType::TypeError,
            msg,
        }
    }

    pub fn name(msg: String) -> Self {
        Self {
            kind: JSONPathErrorType::NameError,
            msg,
        }
    }
}

impl fmt::Display for JSONPathErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JSONPathErrorType::LexerError => f.write_str("lexer error:"),
            JSONPathErrorType::SyntaxError => f.write_str("syntax error:"),
            JSONPathErrorType::TypeError => f.write_str("type error:"),
            JSONPathErrorType::NameError => f.write_str("name error:"),
        }
    }
}

impl std::error::Error for JSONPathError {}

impl fmt::Display for JSONPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}
