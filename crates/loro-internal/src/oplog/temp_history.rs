use loro_common::{ContainerID, HasLamportSpan, Lamport, PeerID, ID};
use rle::RleVec;

use crate::{arena::SharedArena, change::Change, op::Op, version::Frontiers, OpLog};

pub(crate) struct TemporaryHistoryMarker {
    peer: PeerID,
}

pub(crate) struct TemporaryHistoryRecord {
    pub(super) peer: PeerID,
    origin_next_lamport: Lamport,
    pub new_containers: Vec<ContainerID>,
    frontiers: Frontiers,
}

impl TemporaryHistoryMarker {
    pub(crate) fn peer(&self) -> PeerID {
        self.peer
    }
}

impl OpLog {
    /// This needs to be used cautiously, because it will remove the ops from the temp peer.
    /// It might break the internal invariants.
    ///
    /// No change should depend on a change from the temporary peer.
    pub(crate) fn dangerous_remove_ops_from_temp_peer(
        &mut self,
        temp_history: TemporaryHistoryMarker,
    ) -> TemporaryHistoryRecord {
        assert!(self.temp_history.is_some());
        let Some(temp_history_record) = self.temp_history.take() else {
            panic!("Temporary history record not found");
        };
        let temp_peer = temp_history.peer;
        self.dag.vv.remove(&temp_peer);
        self.dag.frontiers = temp_history_record.frontiers.clone();
        self.changes.remove(&temp_peer);
        self.dag.map.remove(&temp_peer);
        self.next_lamport = temp_history_record.origin_next_lamport;
        if cfg!(debug_assertions) {
            let next_lamport = self
                .dag
                .frontiers
                .iter()
                .map(|id| {
                    self.changes
                        .get(&id.peer)
                        .unwrap()
                        .last()
                        .unwrap()
                        .lamport_end()
                })
                .max()
                .unwrap_or_default();
            assert_eq!(next_lamport, self.next_lamport);
        }
        temp_history_record
    }

    pub(crate) fn enter_temp_history_mode(
        &mut self,
        random_peer_id: PeerID,
        dep: ID,
    ) -> (TemporaryHistoryMarker, Option<Frontiers>) {
        assert!(!self.changes.contains_key(&random_peer_id));
        assert!(self.temp_history.is_none());
        let lamport = self.get_lamport_at(dep).unwrap() + 1;
        let diff = self.next_lamport - lamport;
        self.temp_history = Some(TemporaryHistoryRecord {
            peer: random_peer_id,
            origin_next_lamport: self.next_lamport,
            new_containers: vec![],
            frontiers: self.dag.frontiers.clone(),
        });

        if diff > 0 {
            // create a empty change that make up the diff
            let mut empty_change = Change {
                ops: RleVec::new(),
                deps: dep.into(),
                id: ID::new(random_peer_id, 0),
                lamport,
                timestamp: self.latest_timestamp,
                has_dependents: false,
            };
            let id = ContainerID::new_normal(
                ID::new(random_peer_id, 0),
                loro_common::ContainerType::Unknown(8),
            );
            let idx = self.arena.register_container(&id);
            self.temp_history.as_mut().unwrap().new_containers.push(id);
            for _ in 0..diff {
                empty_change.ops.push(Op {
                    counter: 0,
                    container: idx,
                    content: crate::op::InnerContent::Future(
                        crate::op::FutureInnerContent::Unknown {
                            prop: 0,
                            value: crate::encoding::OwnedValue::I64(0),
                        },
                    ),
                });
            }
            self.import_local_change(empty_change).unwrap();
            (
                TemporaryHistoryMarker {
                    peer: random_peer_id,
                },
                Some(self.dag.frontiers.clone()),
            )
        } else {
            (
                TemporaryHistoryMarker {
                    peer: random_peer_id,
                },
                None,
            )
        }
    }
}

impl TemporaryHistoryRecord {
    pub(super) fn on_new_change(&mut self, arena: &SharedArena, change: &Change) {
        for op in change.ops().iter() {
            op.content.visit_created_children(arena, &mut |c| {
                self.new_containers.push(c.clone());
            });
        }
    }
}
