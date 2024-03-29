use serde_columnar::ColumnarError;
use thiserror::Error;

use crate::{InternalString, PeerID, TreeID, ID};

pub type LoroResult<T> = Result<T, LoroError>;

#[derive(Error, Debug)]
pub enum LoroError {
    #[error("Context's client_id({found:?}) does not match Container's client_id({expected:?})")]
    UnmatchedContext { expected: PeerID, found: PeerID },
    #[error("Decode version vector error. Please provide correct version.")]
    DecodeVersionVectorError,
    #[error("Decode error ({0})")]
    DecodeError(Box<str>),
    #[error("Checksum mismatch. The data is corrupted.")]
    DecodeDataCorruptionError,
    #[error("Encountered an incompatible Encoding version \"{0}\". Loro's encoding is backward compatible but not forward compatible. Please upgrade the version of Loro to support this version of the exported data.")]
    IncompatibleFutureEncodingError(usize),
    #[error("Js error ({0})")]
    JsError(Box<str>),
    #[error("Cannot get lock or the lock is poisoned")]
    LockError,
    #[error("Each AppState can only have one transaction at a time")]
    DuplicatedTransactionError,
    #[error("Cannot find ({0})")]
    NotFoundError(Box<str>),
    #[error("Transaction error ({0})")]
    TransactionError(Box<str>),
    #[error("Index out of bound. The given pos is {pos}, but the length is {len}")]
    OutOfBound { pos: usize, len: usize },
    #[error("Every op id should be unique. ID {id} has been used. You should use a new PeerID to edit the content. ")]
    UsedOpID { id: ID },
    #[error("Movable Tree Error")]
    TreeError(#[from] LoroTreeError),
    #[error("Invalid argument ({0})")]
    ArgErr(Box<str>),
    #[error("Auto commit has not started. The doc is readonly when detached. You should ensure autocommit is on and the doc and the state is attached.")]
    AutoCommitNotStarted,
    #[error("You need to specify the style flag for \"({0:?})\" before mark with this key")]
    StyleConfigMissing(InternalString),
    #[error("Unknown Error ({0})")]
    Unknown(Box<str>),
    #[error("The given ID ({0}) is not contained by the doc")]
    InvalidFrontierIdNotFound(ID),
    #[error("Cannot import when the doc is in a transaction")]
    ImportWhenInTxn,
    #[error("The given method ({method}) is not allowed when the container is detached. You should insert the container to the doc first.")]
    MisuseDettachedContainer { method: &'static str },
    #[error("Not implemented: {0}")]
    NotImplemented(&'static str),
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

impl From<ColumnarError> for LoroError {
    fn from(e: ColumnarError) -> Self {
        match e {
            ColumnarError::ColumnarDecodeError(_)
            | ColumnarError::RleEncodeError(_)
            | ColumnarError::RleDecodeError(_)
            | ColumnarError::OverflowError => {
                LoroError::DecodeError(format!("Failed to decode Columnar: {}", e).into_boxed_str())
            }
            e => LoroError::Unknown(e.to_string().into_boxed_str()),
        }
    }
}
