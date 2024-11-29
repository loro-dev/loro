use std::sync::{Arc, RwLock};

use loro::LoroResult;

use crate::{Cursor, LoroDoc, LoroValue, Side};

pub struct UndoManager(RwLock<loro::UndoManager>);

impl UndoManager {
    /// Create a new UndoManager.
    pub fn new(doc: &LoroDoc) -> Self {
        Self(RwLock::new(loro::UndoManager::new(doc)))
    }

    /// Undo the last change made by the peer.
    pub fn undo(&self, doc: &LoroDoc) -> LoroResult<bool> {
        self.0.write().unwrap().undo(doc)
    }

    /// Redo the last change made by the peer.
    pub fn redo(&self, doc: &LoroDoc) -> LoroResult<bool> {
        self.0.write().unwrap().redo(doc)
    }

    /// Record a new checkpoint.
    pub fn record_new_checkpoint(&self, doc: &LoroDoc) -> LoroResult<()> {
        self.0.write().unwrap().record_new_checkpoint(doc)
    }

    /// Whether the undo manager can undo.
    pub fn can_undo(&self) -> bool {
        self.0.read().unwrap().can_undo()
    }

    /// Whether the undo manager can redo.
    pub fn can_redo(&self) -> bool {
        self.0.read().unwrap().can_redo()
    }

    /// If a local event's origin matches the given prefix, it will not be recorded in the
    /// undo stack.
    pub fn add_exclude_origin_prefix(&self, prefix: &str) {
        self.0.write().unwrap().add_exclude_origin_prefix(prefix)
    }

    /// Set the maximum number of undo steps. The default value is 100.
    pub fn set_max_undo_steps(&self, size: u32) {
        self.0.write().unwrap().set_max_undo_steps(size as usize)
    }

    /// Set the merge interval in ms. The default value is 0, which means no merge.
    pub fn set_merge_interval(&self, interval: i64) {
        self.0.write().unwrap().set_merge_interval(interval)
    }

    /// Set the listener for push events.
    /// The listener will be called when a new undo/redo item is pushed into the stack.
    pub fn set_on_push(&self, on_push: Option<Arc<dyn OnPush>>) {
        let on_push = on_push.map(|x| {
            Box::new(move |u, c| loro::undo::UndoItemMeta::from(x.on_push(u, c)))
                as loro::undo::OnPush
        });
        self.0.write().unwrap().set_on_push(on_push)
    }

    /// Set the listener for pop events.
    /// The listener will be called when an undo/redo item is popped from the stack.
    pub fn set_on_pop(&self, on_pop: Option<Arc<dyn OnPop>>) {
        let on_pop = on_pop.map(|x| {
            Box::new(move |u, c, m| (x.on_pop(u, c, UndoItemMeta::from(m)))) as loro::undo::OnPop
        });

        self.0.write().unwrap().set_on_pop(on_pop)
    }
}

pub trait OnPush: Send + Sync {
    fn on_push(
        &self,
        undo_or_redo: loro::undo::UndoOrRedo,
        couter_span: loro::CounterSpan,
    ) -> UndoItemMeta;
}

pub trait OnPop: Send + Sync {
    fn on_pop(
        &self,
        undo_or_redo: loro::undo::UndoOrRedo,
        couter_span: loro::CounterSpan,
        undo_meta: UndoItemMeta,
    );
}

#[derive(Debug, Clone)]
pub struct UndoItemMeta {
    pub value: LoroValue,
    pub cursors: Vec<CursorWithPos>,
}

impl From<loro::undo::UndoItemMeta> for UndoItemMeta {
    fn from(meta: loro::undo::UndoItemMeta) -> Self {
        Self {
            value: meta.value.into(),
            cursors: meta
                .cursors
                .into_iter()
                .map(|c| CursorWithPos {
                    cursor: Arc::new(c.cursor.into()),
                    pos: AbsolutePosition {
                        pos: c.pos.pos as u32,
                        side: c.pos.side,
                    },
                })
                .collect(),
        }
    }
}

impl From<&UndoItemMeta> for loro::undo::UndoItemMeta {
    fn from(meta: &UndoItemMeta) -> Self {
        loro::undo::UndoItemMeta {
            value: (&meta.value).into(),
            cursors: meta
                .cursors
                .iter()
                .map(|c| loro::undo::CursorWithPos {
                    cursor: c.cursor.as_ref().clone().into(),
                    pos: loro::cursor::AbsolutePosition {
                        pos: c.pos.pos as usize,
                        side: c.pos.side,
                    },
                })
                .collect(),
        }
    }
}

impl From<UndoItemMeta> for loro::undo::UndoItemMeta {
    fn from(meta: UndoItemMeta) -> Self {
        loro::undo::UndoItemMeta {
            value: (meta.value).into(),
            cursors: meta
                .cursors
                .into_iter()
                .map(|c| loro::undo::CursorWithPos {
                    cursor: c.cursor.as_ref().clone().into(),
                    pos: loro::cursor::AbsolutePosition {
                        pos: c.pos.pos as usize,
                        side: c.pos.side,
                    },
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CursorWithPos {
    pub cursor: Arc<Cursor>,
    pub pos: AbsolutePosition,
}

#[derive(Debug, Clone, Copy)]
pub struct AbsolutePosition {
    pub pos: u32,
    pub side: Side,
}
