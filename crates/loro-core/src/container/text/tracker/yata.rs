use crdt_list::{
    crdt::{ListCrdt, OpSet},
    yata::Yata,
};
use rle::{
    range_map::{RangeMap, WithStartEnd},
    rle_tree::{iter::IterMut, SafeCursorMut},
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
        container.content.insert_notify(pos, op, &mut notify);
    }

    fn id(op: &Self::OpUnit) -> Self::OpId {
        op.id
    }

    fn cmp_id(op_a: &Self::OpUnit, op_b: &Self::OpUnit) -> std::cmp::Ordering {
        op_a.id.cmp(&op_b.id)
    }

    fn contains(op: &Self::OpUnit, id: Self::OpId) -> bool {
        op.id.contains(op.len as Counter, id)
    }

    fn integrate(container: &mut Self::Container, op: Self::OpUnit) {
        container.vv.set_end(op.id.inc(op.len as i32));
        // SAFETY: we know this is safe because in [YataImpl::insert_after] there is no access to shared elements
        unsafe { crdt_list::yata::integrate::<Self>(container, op) };
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
                let mut ans = Vec::new();
                for iter in a.content.iter() {
                    ans.push((iter.id, iter.len));
                }
                ans
            };
            let bb = {
                let mut ans = Vec::new();
                for iter in b.content.iter() {
                    ans.push((iter.id, iter.len));
                }
                ans
            };

            if aa != bb {
                dbg!(a);
                dbg!(b);
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
            container.content.get_yspan_at_pos(
                ID::new(
                    container.client_id,
                    *container.vv.get(&container.client_id).unwrap_or(&0),
                ),
                pos % container.content.len(),
                pos % 10 + 1,
            )
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
            len = std::cmp::min(len % 10 + 1, container.content.len() - pos);
            if len == 0 {
                return RleVec::new();
            }

            let spans = container.content.get_id_spans(pos, len);
            spans
        }

        fn integrate_delete_op(container: &mut Self::Container, op: Self::DeleteOp) {
            container.update_spans(&op, StatusChange::Delete);
        }

        fn can_apply_del_op(container: &Self::Container, op: &Self::DeleteOp) -> bool {
            true
        }
    }

    use Action::*;
    #[test]
    fn issue_0() {
        crdt_list::test::test_with_actions::<YataImpl>(
            5,
            &[
                NewOp {
                    client_id: 16573246628723425271,
                    pos: 16565899579919523301,
                },
                NewOp {
                    client_id: 16504256534250120677,
                    pos: 16565899579919523301,
                },
                NewOp {
                    client_id: 16565899579910645221,
                    pos: 182786533,
                },
            ],
        )
    }

    #[test]
    fn issue_1() {
        crdt_list::test::test_with_actions::<YataImpl>(
            5,
            &[
                NewOp {
                    client_id: 72057319153112726,
                    pos: 18446743116487664383,
                },
                Delete {
                    client_id: 18446742978492891135,
                    pos: 18446744073709551615,
                    len: 18446744073695461375,
                },
                Delete {
                    client_id: 65535,
                    pos: 281178623508480,
                    len: 18446742974197923840,
                },
                Delete {
                    client_id: 13107135066100727807,
                    pos: 532050712311190,
                    len: 18446744073701163007,
                },
                NewOp {
                    client_id: 35184372089087,
                    pos: 18446462598732840960,
                },
                Sync {
                    from: 18446744073692774400,
                    to: 16565899692026626047,
                },
                Delete {
                    client_id: 18446462606851290549,
                    pos: 18446744073709551487,
                    len: 9910603680803979263,
                },
                NewOp {
                    client_id: 9910603678816504201,
                    pos: 9910603678816504201,
                },
                NewOp {
                    client_id: 9910603678816504201,
                    pos: 9910603678816504201,
                },
                NewOp {
                    client_id: 9910603678816504201,
                    pos: 18446744073701788041,
                },
            ],
        )
    }
}
