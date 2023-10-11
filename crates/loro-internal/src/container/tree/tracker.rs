use fxhash::FxHashMap;
use loro_common::{Counter, HasId, IdSpan, ID};
use rle::{HasLength, RleVec, Sliceable};

use crate::{
    container::text::tracker::id_to_u128,
    op::{InnerContent, RichOp},
    VersionVector,
};

use super::tree_op::TreeOp;

#[derive(Debug, Default)]
pub(crate) struct TreeTracker {
    /// from start_vv to latest vv are applied
    start_vv: VersionVector,
    /// latest applied ops version vector
    all_vv: VersionVector,
    /// current content version vector
    current_vv: VersionVector,
    id_to_op: FxHashMap<u128, TreeOp>,
    content: RleVec<[TreeOp; 0]>,
}

impl TreeTracker {
    pub fn new(start_vv: VersionVector) -> Self {
        TreeTracker {
            all_vv: start_vv.clone(),
            current_vv: start_vv.clone(),
            start_vv,
            id_to_op: Default::default(),
            content: Default::default(),
        }
    }

    pub fn checkout(&mut self, vv: &VersionVector) {
        if &self.current_vv == vv {
            // we can return here even if in for_diff mode.
            // because by default after_status will use the status in the current version
            return;
        }

        let self_vv = std::mem::take(&mut self.current_vv);
        {
            let diff = self_vv.diff_iter(vv);
            self.retreat(diff.0);
            self.forward(diff.1);
        }
        self.current_vv = vv.clone();
        todo!();
    }

    pub fn track_apply(&mut self, rich_op: &RichOp) {
        let content = rich_op.get_sliced().content;
        let id = rich_op.id_start();
        if self
            .all_vv()
            .includes_id(id.inc(content.atom_len() as Counter - 1))
        {
            self.forward(std::iter::once(id.to_span(content.atom_len())));
            return;
        }

        if self.all_vv().includes_id(id) {
            let this_ctr = self.all_vv().get(&id.peer).unwrap();
            let shift = this_ctr - id.counter;
            self.forward(std::iter::once(id.to_span(shift as usize)));
            if shift as usize >= content.atom_len() {
                unreachable!();
            }
            self.apply(
                id.inc(shift),
                &content.slice(shift as usize, content.atom_len()),
            );
        } else {
            self.apply(id, &content)
        }
    }

    /// apply an operation directly to the current tracker
    fn apply(&mut self, id: ID, content: &InnerContent) {
        assert!(*self.current_vv.get(&id.peer).unwrap_or(&0) <= id.counter);
        assert!(*self.all_vv.get(&id.peer).unwrap_or(&0) <= id.counter);
        self.current_vv
            .set_end(id.inc(content.content_len() as i32));
        self.all_vv.set_end(id.inc(content.content_len() as i32));
        let tree_op = content.as_tree().expect("Content is not for tree");
        self.id_to_op.insert(id_to_u128(id), *tree_op);
    }

    fn forward(&mut self, spans: impl Iterator<Item = IdSpan>) {
        for span in spans {
            let end_id = ID::new(span.client_id, span.counter.end);
            // if !for_diff {
            //     self.current_vv.set_end(end_id);
            // }
            if let Some(all_end_ctr) = self.all_vv.get(&span.client_id) {
                let all_end = *all_end_ctr;
                if all_end < span.counter.end {
                    // there may be holes when there are multiple containers
                    self.all_vv.insert(span.client_id, span.counter.end);
                }
                if all_end <= span.counter.start {
                    continue;
                }
            } else {
                self.all_vv.set_end(end_id);
                continue;
            }
        }
    }

    fn retreat(&mut self, spans: impl Iterator<Item = IdSpan>) {}

    #[inline]
    pub fn start_vv(&self) -> &VersionVector {
        &self.start_vv
    }

    pub fn all_vv(&self) -> &VersionVector {
        &self.all_vv
    }

    pub fn contains(&self, id: ID) -> bool {
        self.all_vv.includes_id(id)
    }
}
