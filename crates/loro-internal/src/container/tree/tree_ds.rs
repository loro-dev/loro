use fxhash::FxHashMap;
use loro_common::{IdFull, IdLp, TreeID, ID};

use crate::VersionVector;

use super::bool_rle_vec::BoolRleVec;

pub(crate) struct TreeLinearHistory {
    ops: Vec<TreeOpWrap>,
    has_effect: BoolRleVec,
    target_to_op_idx: FxHashMap<TreeID, usize>,
}

struct TreeOpWrap {
    op: TreeOp,
    last_effective_update_on_target: Option<usize>,
}

pub(crate) struct TreeOp {
    pub id: IdFull,
    pub target: TreeID,
}

pub(crate) struct PoppedTreeOp {
    pub op: TreeOp,
    pub has_effect: bool,
    pub last_update_on_target: Option<IdFull>,
}

impl TreeLinearHistory {
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            has_effect: BoolRleVec::new(),
            target_to_op_idx: FxHashMap::default(),
        }
    }

    pub fn from_vec(ops: Vec<TreeOp>, has_effect: BoolRleVec) -> Self {
        let mut this = Self::new();
        for (op, e) in ops.into_iter().zip(has_effect.iter()) {
            this.push(op, e);
        }

        this
    }

    pub fn push(&mut self, op: TreeOp, has_effect: bool) {
        if has_effect {
            let last = self.target_to_op_idx.insert(op.target, self.ops.len());
            self.ops.push(TreeOpWrap {
                op,
                last_effective_update_on_target: last,
            });
        } else {
            self.ops.push(TreeOpWrap {
                op,
                last_effective_update_on_target: None,
            });
        }

        self.has_effect.push(has_effect);
    }

    pub fn pop_util(&mut self, threshold: IdLp) -> Vec<PoppedTreeOp> {
        let index = match self
            .ops
            .binary_search_by(|op_wrap| op_wrap.op.id.idlp().cmp(&threshold))
        {
            Ok(index) => index,
            Err(index) => index,
        };

        self._pop_util(index)
    }

    // Some of them need to be pushed back later if they are inside the vv
    #[must_use]
    pub fn pop_until_all_inside_vv(&mut self, vv: &VersionVector) -> Vec<PoppedTreeOp> {
        for (i, op_wrap) in self.ops.iter().enumerate() {
            if vv.get(&op_wrap.op.id.peer).unwrap() <= &op_wrap.op.id.counter {
                return self._pop_util(i);
            }
        }

        vec![]
    }

    fn _pop_util(&mut self, index: usize) -> Vec<PoppedTreeOp> {
        todo!()
    }

    pub fn last_update_on(&self, target: TreeID) -> Option<IdFull> {
        self.target_to_op_idx
            .get(&target)
            .and_then(|&idx| self.ops.get(idx))
            .map(|op_wrap| op_wrap.op.id)
    }

    pub fn greatest_idlp(&self) -> Option<IdLp> {
        self.ops.last().map(|op| op.op.id.idlp())
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    #[inline]
    pub fn get_bool_rle_vec(&self) -> &BoolRleVec {
        &self.has_effect
    }
}
