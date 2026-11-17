//! The import undo log: what each container state needs to undo an applied
//! diff, read from the state before the diff is applied.
//!
//! An import that a hard memory quota may reject applies its diff to the state
//! first and checks the quota afterwards. Undoing it by computing the inverse
//! diff from history could fail (a corrupt block) after the state had changed,
//! and computing that diff up front cost several times the import. Instead,
//! [`DocState::begin_import_undo`](super::DocState::begin_import_undo) makes
//! `DocState::apply_diff` ask each container for a [`StateUndo`] before applying
//! its diff (`ContainerState::undo_record`), and a rejected import replays them
//! in reverse (`ContainerState::apply_undo`). Reading a record costs about as
//! much as the part of the state the diff changes; nothing reads history and
//! nothing is copied wholesale. See context/memory-budget.md, "Import rollback
//! without reading history".
//!
//! Each record restores the exact internal state, ids included, not only the
//! value: later local ops depend on element ids and positions.

use loro_common::{IdFull, LoroValue, TreeID};

use crate::{
    container::{idx::ContainerIdx, richtext::richtext_state::RichtextStateChunk},
    delta::MapValue,
    InternalString,
};

use super::tree_state::TreeStateNode;

/// How to undo one diff applied to one container.
#[derive(Debug)]
pub(crate) enum StateUndo {
    /// Steps in the order they were applied; undone in reverse.
    List(Vec<ListUndoStep>),
    /// Steps in the order they were applied; undone in reverse.
    Richtext(Vec<RichtextUndoStep>),
    /// Each key's value before the diff. `None`: the key was absent.
    Map(Vec<(InternalString, Option<MapValue>)>),
    /// Each node's record before the diff, in the order first touched.
    /// `None`: the node did not exist.
    Tree(Vec<(TreeID, Option<TreeStateNode>)>),
    MovableList(super::movable_list_state::MovableListUndo),
    /// The value before the diff (subtracting the increment again may not
    /// give back the same float).
    #[cfg(feature = "counter")]
    Counter(f64),
    /// Nothing changes.
    None,
}

/// One step of a list diff, in the coordinates of the list just before it.
#[derive(Debug)]
pub(crate) enum ListUndoStep {
    Inserted {
        index: usize,
        len: usize,
    },
    Deleted {
        index: usize,
        elems: Vec<(LoroValue, IdFull)>,
    },
}

/// One step of a rich text diff, in entity indexes just before it.
#[derive(Debug)]
pub(crate) enum RichtextUndoStep {
    Inserted {
        index: usize,
        len: usize,
    },
    Deleted {
        index: usize,
        chunks: Vec<RichtextStateChunk>,
    },
}

/// The records of one import, in application order.
#[derive(Debug, Default)]
pub(crate) struct ImportUndoLog {
    pub(crate) entries: Vec<(ContainerIdx, StateUndo)>,
}
