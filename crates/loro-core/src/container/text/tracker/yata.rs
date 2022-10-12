use crdt_list::{
    crdt::{GetOp, ListCrdt, OpSet},
    yata::Yata,
};
use rle::{
    range_map::{RangeMap, WithStartEnd},
    rle_tree::{iter::IterMut, SafeCursorMut},
    Sliceable,
};

use crate::id::{Counter, ID};

use super::{
    cursor_map::make_notify,
    y_span::{YSpan, YSpanTreeTrait},
    Tracker,
};

#[derive(Default, Debug)]
pub struct OpSpanSet {
    map: RangeMap<u128, WithStartEnd<u128, bool>>,
}

impl OpSet<YSpan, ID> for OpSpanSet {
    fn insert(&mut self, value: &YSpan) {
        self.map.set(
            value.id.into(),
            WithStartEnd {
                start: value.id.into(),
                end: value.id.inc(value.len as i32).into(),
                value: true,
            },
        )
    }

    fn contain(&self, id: ID) -> bool {
        self.map.has(id.into())
    }

    fn clear(&mut self) {
        self.map.clear();
    }
}

pub struct YataImpl;

impl ListCrdt for YataImpl {
    type OpUnit = YSpan;

    type OpId = ID;

    type Container = Tracker;

    type Set = OpSpanSet;

    type Cursor<'a> = SafeCursorMut<'a, YSpan, YSpanTreeTrait>;

    type Iterator<'a> = IterMut<'a, YSpan, YSpanTreeTrait>;

    fn iter(
        container: &mut Self::Container,
        from: Option<Self::OpId>,
        to: Option<Self::OpId>,
    ) -> Self::Iterator<'_> {
        let from = from.and_then(|x| {
            container
                .id_to_cursor
                .get(x.into())
                .and_then(|m| m.as_cursor(x))
        });
        let to = to.and_then(|x| {
            container
                .id_to_cursor
                .get(x.into())
                .and_then(|m| m.as_cursor(x))
        });

        // SAFETY: loosen lifetime requirement here. It's safe because the function
        // signature can limit the lifetime of the returned iterator
        unsafe { std::mem::transmute(container.content.iter_mut_in(from, to)) }
    }

    fn insert_at(container: &mut Self::Container, op: Self::OpUnit, pos: usize) {
        let mut notify = make_notify(&mut container.id_to_cursor);
        if pos == 0 {
            container.content.insert_at_first(op, &mut notify);
        } else {
            container.content.insert_notify(pos, op, &mut notify);
        }
    }

    fn id(op: &Self::OpUnit) -> Self::OpId {
        op.id
    }

    fn cmp_id(op_a: &Self::OpUnit, op_b: &Self::OpUnit) -> std::cmp::Ordering {
        op_a.id.client_id.cmp(&op_b.id.client_id)
    }

    fn contains(op: &Self::OpUnit, id: Self::OpId) -> bool {
        op.id.contains(op.len as Counter, id)
    }

    fn integrate(container: &mut Self::Container, op: Self::OpUnit) {
        container.vv.set_end(op.id.inc(op.len as i32));
        // SAFETY: we know this is safe because in [YataImpl::insert_after] there is no access to shared elements
        unsafe { crdt_list::yata::integrate::<Self>(container, op) };
        container.check_consistency();
    }

    fn can_integrate(container: &Self::Container, op: &Self::OpUnit) -> bool {
        if let Some(value) = op.origin_left {
            if !container.id_to_cursor.has(value.into()) {
                return false;
            }
        }

        if let Some(value) = op.origin_right {
            if !container.id_to_cursor.has(value.into()) {
                return false;
            }
        }

        true
    }

    fn len(container: &Self::Container) -> usize {
        container.content.len()
    }
}

impl Yata for YataImpl {
    fn left_origin(op: &Self::OpUnit) -> Option<Self::OpId> {
        op.origin_left
    }

    fn right_origin(op: &Self::OpUnit) -> Option<Self::OpId> {
        op.origin_right
    }

    fn insert_after(container: &mut Self::Container, anchor: Self::Cursor<'_>, op: Self::OpUnit) {
        let mut notify = make_notify(&mut container.id_to_cursor);
        anchor.insert_after_notify(op, &mut notify)
    }

    fn insert_immediately_after(
        container: &mut Self::Container,
        anchor: Self::Cursor<'_>,
        op: Self::OpUnit,
    ) {
        let mut notify = make_notify(&mut container.id_to_cursor);
        anchor.insert_shift_notify(op, 1, &mut notify)
    }
}

#[cfg(test)]
mod test {
    use crdt_list::crdt::OpSet;

    use crate::{
        container::text::tracker::y_span::{Status, YSpan},
        id::ID,
    };

    use super::OpSpanSet;

    #[test]
    fn test() {
        let mut set = OpSpanSet::default();
        set.insert(&YSpan {
            id: ID::new(1, 10),
            len: 10,
            origin_left: Some(ID::new(0, 1)),
            origin_right: Some(ID::new(0, 2)),
            status: Status::new(),
        });
        assert!(set.contain(ID::new(1, 10)));
        assert!(set.contain(ID::new(1, 11)));
        assert!(set.contain(ID::new(1, 18)));
        assert!(set.contain(ID::new(1, 19)));

        assert!(!set.contain(ID::new(1, 8)));
        assert!(!set.contain(ID::new(1, 9)));
        assert!(!set.contain(ID::new(1, 20)));
        assert!(!set.contain(ID::new(1, 21)));
    }
}

#[cfg(feature = "fuzzing")]
pub mod fuzz {
    #![allow(unused_imports)]
    use crdt_list::{
        test::{Action, TestFramework},
        yata::Yata,
    };
    use moveit::New;
    use rle::RleVec;

    use crate::{
        container::text::tracker::{y_span::StatusChange, Tracker},
        id::{ClientID, ID},
        span::{self, IdSpan},
    };

    use super::YataImpl;

    impl TestFramework for YataImpl {
        fn is_content_eq(a: &Self::Container, b: &Self::Container) -> bool {
            let aa = {
                let mut ans = RleVec::new();
                for iter in a.content.iter() {
                    ans.push(iter.as_ref().clone());
                }
                ans
            };
            let bb = {
                let mut ans = RleVec::new();
                for iter in b.content.iter() {
                    ans.push(iter.as_ref().clone());
                }
                ans
            };

            if aa != bb {
                dbg!(aa.vec());
                dbg!(bb.vec());
                dbg!(&a.content);
                dbg!(&b.content);
            }

            assert_eq!(aa, bb);
            aa == bb
        }

        fn new_container(client_id: usize) -> Self::Container {
            let mut tracker = Tracker::new();
            #[cfg(feature = "fuzzing")]
            {
                tracker.client_id = client_id as ClientID;
            }

            tracker
        }

        fn new_op(
            _: &mut impl rand::Rng,
            container: &mut Self::Container,
            pos: usize,
        ) -> Self::OpUnit {
            let ans = container.content.get_yspan_at_pos(
                ID::new(
                    container.client_id,
                    *container.vv.get(&container.client_id).unwrap_or(&0),
                ),
                pos % container.content.len(),
                pos % 10 + 1,
            );
            ans
        }

        type DeleteOp = RleVec<IdSpan>;

        fn new_del_op(
            container: &Self::Container,
            mut pos: usize,
            mut len: usize,
        ) -> Self::DeleteOp {
            if container.content.len() == 0 {
                return RleVec::new();
            }

            pos %= container.content.len();
            len = std::cmp::min(len % 10, container.content.len() - pos);
            if len == 0 {
                return RleVec::new();
            }

            let spans = container.content.get_id_spans(pos, len);
            spans
        }

        fn integrate_delete_op(container: &mut Self::Container, op: Self::DeleteOp) {
            container.update_spans(&op, StatusChange::Delete);
            container.check_consistency();
        }
    }

    use Action::*;
    #[test]
    fn issue_0() {
        crdt_list::test::test_with_actions::<YataImpl>(
            5,
            5,
            vec![
                NewOp {
                    client_id: 1,
                    pos: 0,
                },
                Sync { from: 1, to: 0 },
                NewOp {
                    client_id: 0,
                    pos: 0,
                },
                Delete {
                    client_id: 0,
                    pos: 0,
                    len: 2,
                },
            ],
        )
    }

    #[test]
    fn issue_1() {
        crdt_list::test::test_with_actions::<YataImpl>(
            3,
            5,
            vec![
                Delete {
                    client_id: 1,
                    pos: 3,
                    len: 3,
                },
                NewOp {
                    client_id: 0,
                    pos: 4,
                },
                NewOp {
                    client_id: 0,
                    pos: 4,
                },
                NewOp {
                    client_id: 0,
                    pos: 3,
                },
                NewOp {
                    client_id: 0,
                    pos: 4,
                },
                NewOp {
                    client_id: 0,
                    pos: 0,
                },
                Delete {
                    client_id: 1,
                    pos: 1,
                    len: 1,
                },
                NewOp {
                    client_id: 0,
                    pos: 1,
                },
                Sync { from: 1, to: 0 },
                Sync { from: 0, to: 1 },
                Delete {
                    client_id: 1,
                    pos: 0,
                    len: 2,
                },
                NewOp {
                    client_id: 1,
                    pos: 0,
                },
            ],
        )
    }

    #[test]
    fn normalize() {
        let mut actions = vec![
            Delete {
                client_id: 18446744073709551615,
                pos: 18446462602589896703,
                len: 18374687467077894143,
            },
            Delete {
                client_id: 18374939255676862463,
                pos: 64710657328087551,
                len: 11429747308416114334,
            },
            NewOp {
                client_id: 4872506250964672158,
                pos: 11429747308416114334,
            },
            NewOp {
                client_id: 11429747308416114334,
                pos: 11429747308416114334,
            },
            NewOp {
                client_id: 11429738512323092126,
                pos: 11429747306828660733,
            },
            NewOp {
                client_id: 18446744073709524638,
                pos: 10876193100099747839,
            },
            NewOp {
                client_id: 18374687126443038358,
                pos: 18446744073709551615,
            },
            Delete {
                client_id: 12796479807323897855,
                pos: 7450921,
                len: 11429747308416114176,
            },
            NewOp {
                client_id: 18275218707659529886,
                pos: 10811735328793034751,
            },
            Sync { from: 29105, to: 0 },
            Sync {
                from: 16565750046338121728,
                to: 18446744069414584549,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744004990074879,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 9,
                pos: 0,
                len: 18446229502267752447,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 9223367630218330111,
                len: 18446742974332141567,
            },
            Delete {
                client_id: 7451205583484485631,
                pos: 7451037802321897319,
                len: 7451037802321897319,
            },
            NewOp {
                client_id: 18446743620969457511,
                pos: 18446744073702670335,
            },
        ];

        crdt_list::test::normalize_actions(&mut actions, 2, 5);
        dbg!(actions);
    }
}
