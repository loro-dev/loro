use std::fmt;

use loro::LoroError;

pub type Result<T, E = ModelError> = std::result::Result<T, E>;

/// One step of the path from the model root to the failing node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSeg {
    Key(String),
    Index(usize),
}

#[derive(Debug)]
pub enum ErrorKind {
    TypeMismatch {
        expected: &'static str,
        found: String,
    },
    Missing,
    OutOfRange {
        target: &'static str,
    },
    /// The input value has two list elements with the same key.
    DuplicateKey(String),
    /// The view points at a container that has been deleted.
    Deleted,
    Loro(LoroError),
    TextUpdateTimeout,
}

/// Error with a path-rich context.
///
/// Segments are pushed while the error unwinds, so the success path pays
/// nothing for path tracking.
#[derive(Debug)]
pub struct ModelError {
    /// Innermost segment first.
    rev_path: Vec<PathSeg>,
    pub kind: ErrorKind,
}

impl ModelError {
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            rev_path: Vec::new(),
            kind,
        }
    }

    pub fn mismatch(expected: &'static str, found: impl fmt::Debug) -> Self {
        Self::new(ErrorKind::TypeMismatch {
            expected,
            found: format!("{found:?}"),
        })
    }

    pub fn in_key(mut self, key: &str) -> Self {
        self.rev_path.push(PathSeg::Key(key.to_string()));
        self
    }

    pub fn in_index(mut self, index: usize) -> Self {
        self.rev_path.push(PathSeg::Index(index));
        self
    }

    pub fn path(&self) -> impl Iterator<Item = &PathSeg> {
        self.rev_path.iter().rev()
    }

    pub fn path_string(&self) -> String {
        let mut s = String::new();
        for seg in self.path() {
            match seg {
                PathSeg::Key(k) => {
                    if !s.is_empty() {
                        s.push('.');
                    }
                    s.push_str(k);
                }
                PathSeg::Index(i) => {
                    s.push_str(&format!("[{i}]"));
                }
            }
        }
        s
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path_string();
        let path = if path.is_empty() { "<root>" } else { &path };
        match &self.kind {
            ErrorKind::TypeMismatch { expected, found } => {
                write!(f, "{path}: expected {expected}, found {found}")
            }
            ErrorKind::Missing => write!(f, "{path}: missing"),
            ErrorKind::OutOfRange { target } => write!(f, "{path}: out of range for {target}"),
            ErrorKind::DuplicateKey(k) => write!(f, "{path}: duplicate key {k}"),
            ErrorKind::Deleted => write!(f, "{path}: container was deleted"),
            ErrorKind::Loro(e) => write!(f, "{path}: {e}"),
            ErrorKind::TextUpdateTimeout => write!(f, "{path}: text update timed out"),
        }
    }
}

impl std::error::Error for ModelError {}

impl From<LoroError> for ModelError {
    fn from(e: LoroError) -> Self {
        Self::new(ErrorKind::Loro(e))
    }
}
