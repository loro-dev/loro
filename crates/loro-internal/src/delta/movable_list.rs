use enum_as_inner::EnumAsInner;
use loro_common::{IdFull, IdLp, LoroValue};
use smallvec::SmallVec;

use super::{Delta, DeltaValue};

#[derive(Clone, Debug)]
pub(crate) struct MovableListInnerDelta {
    pub list: Delta<SmallVec<[IdFull; 1]>, ()>,
    pub elements: Vec<ElementDelta>,
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

#[derive(Clone, Debug, EnumAsInner)]
pub enum ElementDelta {
    PosChange {
        id: IdLp,
        new_pos: IdLp,
    },
    ValueChange {
        id: IdLp,
        new_value: LoroValue,
        value_id: IdLp,
    },
    New {
        id: IdLp,
        new_pos: IdLp,
        new_value: LoroValue,
        value_id: IdLp,
    },
}

impl ElementDelta {
    pub fn value(&self) -> Option<&LoroValue> {
        match self {
            ElementDelta::ValueChange { new_value, .. } => Some(new_value),
            _ => None,
        }
    }
}
