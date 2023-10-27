use thiserror::Error;

use crate::{PeerID, TreeID, ID};

pub type LoroResult<T> = Result<T, LoroError>;

#[derive(Error, Debug)]
pub enum LoroError {
    #[error("Context's client_id({found:?}) does not match Container's client_id({expected:?})")]
    UnmatchedContext { expected: PeerID, found: PeerID },
    #[error("Decode version vector error. Please provide correct version.")]
    DecodeVersionVectorError,
    #[error("Decode error ({0})")]
    DecodeError(Box<str>),
    #[error("Js error ({0})")]
    JsError(Box<str>),
    #[error("Cannot get lock or the lock is poisoned")]
    LockError,
    #[error("LoroValue::Unresolved cannot be converted to PrelimValue")]
    PrelimError,
    #[error("Each AppState can only have one transaction at a time")]
    DuplicatedTransactionError,
    #[error("Cannot find ({0}) ")]
    NotFoundError(Box<str>),
    // TODO: more details transaction error
    #[error("Transaction error ({0})")]
    TransactionError(Box<str>),
    // TODO:
    #[error("TempContainer cannot execute this function")]
    TempContainerError,
    #[error("Index out of bound. The given pos is {pos}, but the length is {len}")]
    OutOfBound { pos: usize, len: usize },
    #[error("Every op id should be unique. ID {id} has been used. You should use a new PeerID to edit the content. ")]
    UsedOpID { id: ID },
    #[error("Movable Tree Error")]
    TreeError(#[from] LoroTreeError),
    #[error("Invalid argument ({0})")]
    ArgErr(Box<str>),
    // #[error("the data for key `{0}` is not available")]
    // Redaction(String),
    // #[error("invalid header (expected {expected:?}, found {found:?})")]
    // InvalidHeader { expected: String, found: String },
    // #[error("unknown data store error")]
    // Unknown,
}

#[derive(Error, Debug)]
pub enum LoroTreeError {
    #[error("`Cycle move` occurs when moving tree nodes.")]
    CyclicMoveError,
    #[error("The parent of tree node is not found {0:?}")]
    TreeNodeParentNotFound(TreeID),
    #[error("TreeID {0:?} doesn't exist")]
    TreeNodeNotExist(TreeID),
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use wasm_bindgen::JsValue;

    use crate::LoroError;

    impl From<LoroError> for JsValue {
        fn from(value: LoroError) -> Self {
            JsValue::from_str(&value.to_string())
        }
    }

    impl From<JsValue> for LoroError {
        fn from(v: JsValue) -> Self {
            Self::JsError(
                v.as_string()
                    .unwrap_or_else(|| "unknown error".to_owned())
                    .into_boxed_str(),
            )
        }
    }
}
