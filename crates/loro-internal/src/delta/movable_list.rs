use fxhash::FxHashMap;
use loro_common::{CompactIdLp, IdFull, IdLp, LoroValue};
use smallvec::SmallVec;

use super::{Delta, DeltaValue};

#[derive(Clone, Debug)]
pub(crate) struct MovableListInnerDelta {
    pub list: Delta<SmallVec<[IdFull; 1]>, ()>,
    pub elements: FxHashMap<CompactIdLp, ElementDelta>,
}

impl DeltaValue for SmallVec<[IdFull; 1]> {
    fn value_extend(&mut self, other: Self) -> Result<(), Self> {
        for v in other {
            self.push(v)
        }

        Ok(())
    }

    fn take(&mut self, length: usize) -> Self {
        self.drain(..length).collect()
    }

    fn length(&self) -> usize {
        self.len()
    }
}

impl MovableListInnerDelta {
    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty() && self.elements.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct ElementDelta {
    pub pos: IdLp,
    pub pos_updated: bool,
    pub value: LoroValue,
    pub value_updated: bool,
    pub value_id: IdLp,
}

impl ElementDelta {
    pub fn placeholder() -> Self {
        Self {
            pos: IdLp::NONE_ID,
            pos_updated: false,
            value: LoroValue::Null,
            value_updated: false,
            value_id: IdLp::NONE_ID,
        }
    }
}
