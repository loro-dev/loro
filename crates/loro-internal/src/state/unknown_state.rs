use std::sync::Weak;

use loro_common::{ContainerID, LoroResult, LoroValue};

use crate::{
    configure::Configure,
    container::idx::ContainerIdx,
    event::{Diff, Index, InternalDiff},
    op::{Op, RawOp},
    LoroDocInner,
};

use super::{ApplyLocalOpReturn, ContainerState, DiffApplyContext};

#[derive(Debug, Clone)]
pub struct UnknownState {
    idx: ContainerIdx,
}

impl UnknownState {
    pub fn new(idx: ContainerIdx) -> Self {
        Self { idx }
    }
}

impl ContainerState for UnknownState {
    fn container_idx(&self) -> ContainerIdx {
        self.idx
    }

    fn is_state_empty(&self) -> bool {
        false
    }

    fn apply_diff_and_convert(&mut self, _diff: InternalDiff, _ctx: DiffApplyContext) -> Diff {
        unreachable!()
    }

    fn apply_diff(&mut self, _diff: InternalDiff, _ctx: DiffApplyContext) -> LoroResult<()> {
        unreachable!()
    }

    fn apply_local_op(&mut self, _raw_op: &RawOp, _op: &Op) -> LoroResult<ApplyLocalOpReturn> {
        unreachable!()
    }

    #[doc = r" Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied."]
    fn to_diff(&mut self, _doc: &Weak<LoroDocInner>) -> Diff {
        Diff::Unknown
    }

    fn get_value(&mut self) -> LoroValue {
        unreachable!()
    }

    #[doc = r" Get the index of the child container"]
    #[allow(unused)]
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        None
    }

    fn contains_child(&self, _id: &ContainerID) -> bool {
        false
    }

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID> {
        vec![]
    }

    fn fork(&self, _config: &Configure) -> Self {
        self.clone()
    }
}

mod snapshot {
    use loro_common::LoroValue;

    use crate::state::FastStateSnapshot;

    use super::UnknownState;

    impl FastStateSnapshot for UnknownState {
        fn encode_snapshot_fast<W: std::io::prelude::Write>(&mut self, _w: W) {}

        fn decode_value(bytes: &[u8]) -> loro_common::LoroResult<(loro_common::LoroValue, &[u8])> {
            Ok((LoroValue::Null, bytes))
        }

        fn decode_snapshot_fast(
            idx: crate::container::idx::ContainerIdx,
            _v: (loro_common::LoroValue, &[u8]),
            _ctx: crate::state::ContainerCreationContext,
        ) -> loro_common::LoroResult<Self>
        where
            Self: Sized,
        {
            Ok(UnknownState::new(idx))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use loro_common::{ContainerID, ContainerType};

    use crate::{
        configure::Configure,
        container::idx::ContainerIdx,
        event::Diff,
        state::{ContainerCreationContext, ContainerState, FastStateSnapshot},
        InternalString,
    };

    use super::UnknownState;

    fn unknown_idx() -> ContainerIdx {
        ContainerIdx::from_index_and_type(7, ContainerType::Unknown(3))
    }

    #[test]
    fn unknown_state_reports_identity_without_material_value() {
        let mut state = UnknownState::new(unknown_idx());
        assert_eq!(state.container_idx(), unknown_idx());
        assert!(!state.is_state_empty());

        assert!(matches!(state.to_diff(&Weak::new()), Diff::Unknown));
        assert_eq!(state.get_child_containers(), Vec::<ContainerID>::new());
        assert!(!state.contains_child(&ContainerID::Root {
            name: InternalString::from("child"),
            container_type: ContainerType::Text,
        }));
        assert_eq!(
            state.get_child_index(&ContainerID::Root {
                name: InternalString::from("child"),
                container_type: ContainerType::Text,
            }),
            None
        );
    }

    #[test]
    fn unknown_state_fork_keeps_container_identity() {
        let state = UnknownState::new(unknown_idx());
        let forked = state.fork(&Configure::default());

        assert_eq!(forked.container_idx(), unknown_idx());
        assert!(!forked.is_state_empty());
    }

    #[test]
    fn unknown_snapshot_roundtrip_preserves_bytes_and_index() {
        let mut state = UnknownState::new(unknown_idx());
        let mut encoded = Vec::new();
        state.encode_snapshot_fast(&mut encoded);
        assert!(encoded.is_empty());

        let input = b"opaque-unknown-payload";
        let (value, rest) = UnknownState::decode_value(input).unwrap();
        assert_eq!(value, loro_common::LoroValue::Null);
        assert_eq!(rest, input);

        let decoded = UnknownState::decode_snapshot_fast(
            unknown_idx(),
            (value, rest),
            ContainerCreationContext {
                configure: &Configure::default(),
                peer: 0,
            },
        )
        .unwrap();
        assert_eq!(decoded.container_idx(), unknown_idx());
    }
}
