use serde_columnar::ColumnarError;
use thiserror::Error;

use crate::{ContainerID, InternalString, PeerID, TreeID, ID};

pub type LoroResult<T> = Result<T, LoroError>;

#[derive(Error, Debug, PartialEq)]
pub enum LoroError {
    #[error("Context's client_id({found:?}) does not match Container's client_id({expected:?})")]
    UnmatchedContext { expected: PeerID, found: PeerID },
    #[error("Decode error: Version vector error. Please provide correct version.")]
    DecodeVersionVectorError,
    #[error("Decode error: ({0})")]
    DecodeError(Box<str>),
    #[error(
        // This should not happen after v1.0.0
        "Decode error: The data is either corrupted or originates from an older version that is incompatible due to a breaking change."
    )]
    DecodeDataCorruptionError,
    #[error("Decode error: Checksum mismatch. The data is corrupted.")]
    DecodeChecksumMismatchError,
    #[error("Decode error: Encoding version \"{0}\" is incompatible. Loro's encoding is backward compatible but not forward compatible. Please upgrade the version of Loro to support this version of the exported data.")]
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
    #[error("Index out of bound. The given pos is {pos}, but the length is {len}. {info}")]
    OutOfBound {
        pos: usize,
        len: usize,
        info: Box<str>,
    },
    #[error("Every op id should be unique. ID {id} has been used. You should use a new PeerID to edit the content. ")]
    UsedOpID { id: ID },
    #[error("Concurrent ops with the same peer id is not allowed. PeerID: {peer}, LastCounter: {last_counter}, CurrentCounter: {current}")]
    ConcurrentOpsWithSamePeerID {
        peer: PeerID,
        last_counter: i32,
        current: i32,
    },
    #[error("Movable Tree Error: {0}")]
    TreeError(#[from] LoroTreeError),
    #[error("Invalid argument ({0})")]
    ArgErr(Box<str>),
    #[error("Auto commit has not started. The doc is readonly when detached and detached editing is not enabled.")]
    AutoCommitNotStarted,
    #[error("Style configuration missing for \"({0:?})\". Please provide the style configuration using `configTextStyle` on your Loro doc.")]
    StyleConfigMissing(InternalString),
    #[error("Unknown Error ({0})")]
    Unknown(Box<str>),
    #[error("The given ID ({0}) is not contained by the doc")]
    FrontiersNotFound(ID),
    #[error("Cannot import when the doc is in a transaction")]
    ImportWhenInTxn,
    #[error("The given method ({method}) is not allowed when the container is detached. You should insert the container to the doc first.")]
    MisuseDetachedContainer { method: &'static str },
    #[error("Not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("Reattach a container that is already attached")]
    ReattachAttachedContainer,
    #[error("Edit is not allowed when the doc is in the detached mode.")]
    EditWhenDetached,
    #[error("The given ID ({0}) is not contained by the doc")]
    UndoInvalidIdSpan(ID),
    #[error("PeerID cannot be changed. Expected: {expected:?}, Actual: {actual:?}")]
    UndoWithDifferentPeerId { expected: PeerID, actual: PeerID },
    #[error("There is already an active undo group, call `group_end` first")]
    UndoGroupAlreadyStarted,
    #[error("There is no active undo group, call `group_start` first")]
    InvalidJsonSchema,
    #[error("Cannot insert or delete utf-8 in the middle of the codepoint in Unicode")]
    UTF8InUnicodeCodePoint { pos: usize },
    #[error("Cannot insert or delete utf-16 in the middle of the codepoint in Unicode")]
    UTF16InUnicodeCodePoint { pos: usize },
    #[error("The end index cannot be less than the start index")]
    EndIndexLessThanStartIndex { start: usize, end: usize },
    #[error("Invalid root container name! Don't include '/' or '\\0'")]
    InvalidRootContainerName,
    #[error("Import Failed: The dependencies of the importing updates are not included in the shallow history of the doc.")]
    ImportUpdatesThatDependsOnOutdatedVersion,
    #[error(
        "You cannot switch a document to a version before the shallow history's start version."
    )]
    SwitchToVersionBeforeShallowRoot,
    #[error(
        "The container {container} is deleted. You cannot apply the op on a deleted container."
    )]
    ContainerDeleted { container: Box<ContainerID> },
    #[error("You cannot set the `PeerID` with `PeerID::MAX`, which is an internal specific value")]
    InvalidPeerID,
    #[error("The containers {containers:?} are not found in the doc")]
    ContainersNotFound { containers: Box<Vec<ContainerID>> },
}

#[derive(Error, Debug, PartialEq)]
pub enum LoroTreeError {
    #[error("`Cycle move` occurs when moving tree nodes.")]
    CyclicMoveError,
    #[error("The provided parent id is invalid")]
    InvalidParent,
    #[error("The parent of tree node is not found {0:?}")]
    TreeNodeParentNotFound(TreeID),
    #[error("TreeID {0:?} doesn't exist")]
    TreeNodeNotExist(TreeID),
    #[error("The index({index}) should be <= the length of children ({len})")]
    IndexOutOfBound { len: usize, index: usize },
    #[error("Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`")]
    FractionalIndexNotEnabled,
    #[error("TreeID {0:?} is deleted or does not exist")]
    TreeNodeDeletedOrNotExist(TreeID),
}

#[non_exhaustive]
#[derive(Error, Debug, PartialEq)]
pub enum LoroEncodeError {
    #[error("The frontiers are not found in this doc: {0}")]
    FrontiersNotFound(String),
    #[error("Shallow snapshot incompatible with old snapshot format. Use new snapshot format or avoid shallow snapshots for storage.")]
    ShallowSnapshotIncompatibleWithOldFormat,
    #[error("Cannot export shallow snapshot with unknown container type. Please upgrade the Loro version.")]
    UnknownContainer,
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use wasm_bindgen::JsValue;

    use crate::{LoroEncodeError, LoroError};

    impl From<LoroError> for JsValue {
        fn from(value: LoroError) -> Self {
            JsValue::from_str(&value.to_string())
        }
    }

    impl From<LoroEncodeError> for JsValue {
        fn from(value: LoroEncodeError) -> Self {
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
                LoroError::DecodeError(format!("Failed to decode Columnar: {e}").into_boxed_str())
            }
            e => LoroError::Unknown(e.to_string().into_boxed_str()),
        }
    }
}
