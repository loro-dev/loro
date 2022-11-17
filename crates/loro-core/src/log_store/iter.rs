use crate::Op;

use crate::id::ClientID;
use crate::id::ContainerIdx;
use crate::op::RichOp;

use crate::span::HasId;
use crate::span::IdSpan;

use fxhash::FxHashMap;
use rle::HasLength;

use crate::change::ChangeMergeCfg;

use crate::change::Change;

use rle::RleVecWithIndex;

pub struct ClientOpIter<'a> {
    pub(crate) change_index: usize,
    pub(crate) op_index: usize,
    pub(crate) changes: Option<&'a RleVecWithIndex<Change, ChangeMergeCfg>>,
}

impl<'a> Iterator for ClientOpIter<'a> {
    type Item = (&'a Change, &'a Op);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(change) = self.changes?.get_merged(self.change_index) {
                if let Some(op) = change.ops.get_merged(self.op_index) {
                    self.op_index += 1;
                    return Some((change, op));
                } else {
                    self.op_index = 0;
                    self.change_index += 1;
                }
            } else {
                return None;
            }
        }
    }
}

pub struct OpSpanIter<'a> {
    changes: &'a [Change],
    change_index: usize,
    op_index: usize,
    container: ContainerIdx,
    span: IdSpan,
}

impl<'a> OpSpanIter<'a> {
    pub fn new(
        changes: &'a FxHashMap<ClientID, RleVecWithIndex<Change, ChangeMergeCfg>>,
        target_span: IdSpan,
        container: ContainerIdx,
    ) -> Self {
        let rle_changes = changes.get(&target_span.client_id).unwrap();
        let changes = rle_changes.vec();
        let change_index = rle_changes
            .get(target_span.id_start().counter as usize)
            .map(|x| x.merged_index)
            .unwrap_or(changes.len());

        Self {
            span: target_span,
            container,
            changes,
            change_index,
            op_index: rle_changes[change_index]
                .ops
                .get(target_span.counter.start)
                .unwrap()
                .merged_index,
        }
    }
}

impl<'a> Iterator for OpSpanIter<'a> {
    type Item = RichOp<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.change_index == self.changes.len() {
                return None;
            }

            let change = &self.changes[self.change_index];
            let ops = change.ops.vec();
            if let Some(op) = ops.get(self.op_index) {
                if op.counter >= self.span.counter.end {
                    return None;
                }

                self.op_index += 1;
                if op.container != self.container {
                    continue;
                }

                let start = (self.span.counter.min() - op.counter).max(0) as usize;
                let end = ((self.span.counter.end() - op.counter) as usize).min(op.atom_len());
                assert!(start < end, "{:?} {:#?}", self.span, op);
                return Some(RichOp::new_by_change(change, op));
            } else {
                self.op_index = 0;
                self.change_index += 1;
                continue;
            }
        }
    }
}
