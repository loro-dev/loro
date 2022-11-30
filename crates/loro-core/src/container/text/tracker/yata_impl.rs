#![allow(unused)]
use crdt_list::{
    crdt::{GetOp, ListCrdt, OpSet},
    yata::Yata,
};
use rle::{
    range_map::{RangeMap, WithStartEnd},
    rle_tree::{iter::IterMut, SafeCursorMut},
    HasLength,
};

use crate::id::{Counter, ID};

use super::{
    cursor_map::{make_notify, CursorMap},
    y_span::{YSpan, YSpanTreeTrait},
    Tracker,
};

// TODO: may use a simpler data structure here
#[derive(Default, Debug)]
pub struct OpSpanSet {
    map: RangeMap<u128, WithStartEnd<u128, bool>>,
}

impl OpSet<YSpan, ID> for OpSpanSet {
    fn insert(&mut self, value: &YSpan) {
        self.map.set_small_range(
            value.id.into(),
            WithStartEnd {
                start: value.id.into(),
                end: value.id.inc(value.atom_len() as i32).into(),
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
        let from = from
            .and_then(|x| {
                container
                    .id_to_cursor
                    .get(x.into())
                    .and_then(|m| m.as_cursor(x))
            })
            .and_then(|x| x.shift(1));
        let to = to.and_then(|x| {
            container
                .id_to_cursor
                .get(x.into())
                .and_then(|m| m.as_cursor(x))
        });

        // dbg!(&container.content);
        // SAFETY: loosen lifetime requirement here. It's safe because the function
        // signature can limit the lifetime of the returned iterator
        container.content.iter_mut_in(from, to)
    }

    fn id(op: &Self::OpUnit) -> Self::OpId {
        op.id
    }

    fn cmp_id(op_a: &Self::OpUnit, op_b: &Self::OpUnit) -> std::cmp::Ordering {
        op_a.id.client_id.cmp(&op_b.id.client_id)
    }

    fn contains(op: &Self::OpUnit, id: Self::OpId) -> bool {
        op.id.contains(op.atom_len() as Counter, id)
    }
}

impl Yata for YataImpl {
    type Context = CursorMap;
    fn left_origin(op: &Self::OpUnit) -> Option<Self::OpId> {
        op.origin_left
    }

    fn right_origin(op: &Self::OpUnit) -> Option<Self::OpId> {
        op.origin_right
    }

    fn insert_after(anchor: Self::Cursor<'_>, op: Self::OpUnit, ctx: &mut CursorMap) {
        let mut notify = make_notify(ctx);
        anchor.insert_after_notify(op, &mut notify)
    }

    fn insert_after_id(
        container: &mut Self::Container,
        id: Option<Self::OpId>,
        op: Self::OpUnit,
        ctx: &mut CursorMap,
    ) {
        if let Some(id) = id {
            let left = container.id_to_cursor.get(id.into()).unwrap();
            let left = left.as_cursor(id).unwrap();
            let mut notify = make_notify(ctx);
            // SAFETY: we own the tree here
            unsafe {
                left.unwrap()
                    .shift(1)
                    .unwrap()
                    .insert_notify(op, &mut notify);
            }
        } else {
            let mut notify = make_notify(ctx);
            container.content.insert_at_first(op, &mut notify);
        }
    }
}

#[cfg(test)]
mod test {
    use crdt_list::crdt::OpSet;

    use crate::{
        container::text::{
            text_content::ListSlice,
            tracker::y_span::{Status, YSpan},
        },
        id::ID,
    };

    use super::OpSpanSet;

    #[test]
    fn test() {
        let mut set = OpSpanSet::default();
        set.insert(&YSpan {
            id: ID::new(1, 10),
            origin_left: Some(ID::new(0, 1)),
            origin_right: Some(ID::new(0, 2)),
            status: Status::new(),
            slice: ListSlice::unknown_range(10),
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

#[cfg(feature = "test_utils")]
pub mod fuzz {
    use std::borrow::Cow;
    use tabled::Tabled;
    impl Tabled for YSpan {
        const LENGTH: usize = 7;

        fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
            vec![
                self.id.to_string().into(),
                self.atom_len().to_string().into(),
                self.status.future.to_string().into(),
                self.status.delete_times.to_string().into(),
                self.status.undo_times.to_string().into(),
                self.origin_left
                    .map(|id| id.to_string())
                    .unwrap_or_default()
                    .into(),
                self.origin_right
                    .map(|id| id.to_string())
                    .unwrap_or_default()
                    .into(),
            ]
        }

        fn headers() -> Vec<Cow<'static, str>> {
            vec![
                "id".into(),
                "len".into(),
                "future".into(),
                "del".into(),
                "undo".into(),
                "origin\nleft".into(),
                "origin\nright".into(),
            ]
        }
    }

    use crdt_list::test::{Action, TestFramework};
    use rle::{HasLength, RleVecWithIndex, RleVecWithLen};
    use tabled::TableIteratorExt;
    use Action::*;

    use crate::{
        container::text::{
            text_content::ListSlice,
            tracker::{
                y_span::{StatusChange, YSpan},
                Tracker,
            },
        },
        id::{ClientID, Counter, ID},
        span::IdSpan,
    };

    use super::YataImpl;

    impl TestFramework for YataImpl {
        fn integrate(container: &mut Self::Container, op: Self::OpUnit) {
            container
                .current_vv
                .set_end(op.id.inc(op.atom_len() as i32));
            container.with_cursor_map(|this, map| {
                crdt_list::yata::integrate::<Self>(this, op, map);
            });
        }

        #[inline]
        fn can_integrate(container: &Self::Container, op: &Self::OpUnit) -> bool {
            if let Some(value) = op.origin_left {
                if !value.is_unknown() && !container.current_vv.includes_id(value) {
                    return false;
                }
            }

            if let Some(value) = op.origin_right {
                if !value.is_unknown() && !container.current_vv.includes_id(value) {
                    return false;
                }
            }

            if op.id.counter != 0 && !container.current_vv.includes_id(op.id.inc(-1)) {
                return false;
            }

            true
        }

        fn is_content_eq(a: &Self::Container, b: &Self::Container) -> bool {
            let aa = {
                let mut ans = RleVecWithIndex::new();
                for iter in a.content.iter() {
                    ans.push(iter.as_ref().clone());
                }
                ans
            };
            let bb = {
                let mut ans = RleVecWithIndex::new();
                for iter in b.content.iter() {
                    ans.push(iter.as_ref().clone());
                }
                ans
            };

            if aa != bb {
                dbg!(a.client_id);
                dbg!(b.client_id);
                println!("{}", aa.vec().table());
                println!("{}", bb.vec().table());
                // dbg!(&a.content);
                // dbg!(&b.content);
            }

            assert_eq!(aa, bb);
            aa == bb
        }

        fn new_container(client_id: usize) -> Self::Container {
            let mut tracker = Tracker::new(Default::default(), Counter::MAX / 2);
            #[cfg(feature = "test_utils")]
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
            let len = pos % 10 + 1;
            let ans = container.content.get_yspan_at_pos(
                ID::new(
                    container.client_id,
                    *container.current_vv.get(&container.client_id).unwrap_or(&0),
                ),
                pos % container.content.len(),
                len,
                ListSlice::unknown_range(len),
            );
            ans
        }

        type DeleteOp = RleVecWithLen<[IdSpan; 2]>;

        fn new_del_op(
            container: &Self::Container,
            mut pos: usize,
            mut len: usize,
        ) -> Self::DeleteOp {
            if container.content.len() == 0 {
                return RleVecWithLen::new();
            }

            pos %= container.content.len();
            len = std::cmp::min(len % 10, container.content.len() - pos);
            if len == 0 {
                return RleVecWithLen::new();
            }

            container.content.get_active_id_spans(pos, len)
        }

        fn integrate_delete_op(container: &mut Self::Container, op: Self::DeleteOp) {
            container.update_spans(&op, StatusChange::Delete);
        }
    }

    #[test]
    fn issue_global_tree_trait() {
        crdt_list::test::test_with_actions::<YataImpl>(
            5,
            100,
            vec![
                Delete {
                    client_id: 252,
                    pos: 58,
                    len: 179,
                },
                Delete {
                    client_id: 227,
                    pos: 227,
                    len: 126,
                },
                Delete {
                    client_id: 227,
                    pos: 227,
                    len: 227,
                },
                Delete {
                    client_id: 177,
                    pos: 177,
                    len: 202,
                },
                Delete {
                    client_id: 202,
                    pos: 177,
                    len: 177,
                },
                Delete {
                    client_id: 202,
                    pos: 202,
                    len: 202,
                },
                Delete {
                    client_id: 176,
                    pos: 177,
                    len: 177,
                },
                Delete {
                    client_id: 177,
                    pos: 177,
                    len: 162,
                },
                NewOp {
                    client_id: 217,
                    pos: 0,
                },
                Sync { from: 126, to: 126 },
                Sync { from: 177, to: 6 },
                NewOp {
                    client_id: 96,
                    pos: 64,
                },
                NewOp {
                    client_id: 217,
                    pos: 227,
                },
                Delete {
                    client_id: 227,
                    pos: 227,
                    len: 227,
                },
                Delete {
                    client_id: 227,
                    pos: 227,
                    len: 227,
                },
                Delete {
                    client_id: 176,
                    pos: 177,
                    len: 177,
                },
                Delete {
                    client_id: 202,
                    pos: 202,
                    len: 202,
                },
                Delete {
                    client_id: 202,
                    pos: 202,
                    len: 202,
                },
                Delete {
                    client_id: 241,
                    pos: 177,
                    len: 176,
                },
                Delete {
                    client_id: 177,
                    pos: 101,
                    len: 101,
                },
                NewOp {
                    client_id: 101,
                    pos: 101,
                },
                Delete {
                    client_id: 153,
                    pos: 255,
                    len: 126,
                },
                NewOp {
                    client_id: 232,
                    pos: 156,
                },
                Delete {
                    client_id: 177,
                    pos: 176,
                    len: 177,
                },
                Delete {
                    client_id: 241,
                    pos: 177,
                    len: 202,
                },
                Delete {
                    client_id: 202,
                    pos: 177,
                    len: 177,
                },
                Delete {
                    client_id: 202,
                    pos: 202,
                    len: 202,
                },
                Delete {
                    client_id: 176,
                    pos: 177,
                    len: 177,
                },
                Delete {
                    client_id: 177,
                    pos: 176,
                    len: 177,
                },
                NewOp {
                    client_id: 153,
                    pos: 153,
                },
                Delete {
                    client_id: 0,
                    pos: 0,
                    len: 126,
                },
                Sync { from: 0, to: 162 },
                NewOp {
                    client_id: 96,
                    pos: 96,
                },
                Delete {
                    client_id: 232,
                    pos: 126,
                    len: 126,
                },
                Delete {
                    client_id: 126,
                    pos: 126,
                    len: 126,
                },
                Delete {
                    client_id: 227,
                    pos: 227,
                    len: 227,
                },
                Delete {
                    client_id: 227,
                    pos: 227,
                    len: 227,
                },
                NewOp {
                    client_id: 158,
                    pos: 107,
                },
                Delete {
                    client_id: 126,
                    pos: 126,
                    len: 43,
                },
            ],
        )
    }

    #[test]
    fn issue_set_range() {
        crdt_list::test::test_with_actions::<YataImpl>(
            5,
            100,
            vec![
                Sync { from: 1, to: 2 },
                NewOp {
                    client_id: 2,
                    pos: 7,
                },
                NewOp {
                    client_id: 2,
                    pos: 7,
                },
                Delete {
                    client_id: 2,
                    pos: 37,
                    len: 37,
                },
                Delete {
                    client_id: 2,
                    pos: 7,
                    len: 17,
                },
                Sync { from: 2, to: 2 },
                NewOp {
                    client_id: 2,
                    pos: 52,
                },
                Sync { from: 3, to: 2 },
                Delete {
                    client_id: 2,
                    pos: 52,
                    len: 52,
                },
                Delete {
                    client_id: 2,
                    pos: 25,
                    len: 17,
                },
                NewOp {
                    client_id: 2,
                    pos: 46,
                },
                Delete {
                    client_id: 2,
                    pos: 52,
                    len: 52,
                },
            ],
        )
    }
    #[test]
    fn issue_range_map() {
        crdt_list::test::test_with_actions::<YataImpl>(
            5,
            100,
            vec![
                NewOp {
                    client_id: 124,
                    pos: 124,
                },
                Delete {
                    client_id: 8,
                    pos: 47,
                    len: 68,
                },
                Delete {
                    client_id: 255,
                    pos: 255,
                    len: 255,
                },
                Delete {
                    client_id: 184,
                    pos: 184,
                    len: 48,
                },
                Sync { from: 158, to: 0 },
                Delete {
                    client_id: 182,
                    pos: 182,
                    len: 182,
                },
                NewOp {
                    client_id: 255,
                    pos: 252,
                },
                Sync { from: 134, to: 2 },
                Delete {
                    client_id: 246,
                    pos: 246,
                    len: 246,
                },
                Delete {
                    client_id: 246,
                    pos: 246,
                    len: 246,
                },
            ],
        )
    }

    #[test]
    fn issue_1() {
        crdt_list::test::test_with_actions::<YataImpl>(
            5,
            100,
            vec![NewOp {
                client_id: 153,
                pos: 153,
            }],
        )
    }

    #[test]
    fn normalize() {
        let mut actions = vec![
            Sync { from: 1, to: 107 },
            NewOp {
                client_id: 107,
                pos: 107,
            },
            NewOp {
                client_id: 107,
                pos: 107,
            },
            Delete {
                client_id: 237,
                pos: 237,
                len: 237,
            },
            Delete {
                client_id: 107,
                pos: 107,
                len: 117,
            },
            Sync { from: 107, to: 252 },
            NewOp {
                client_id: 252,
                pos: 252,
            },
            Sync { from: 3, to: 117 },
            Delete {
                client_id: 252,
                pos: 252,
                len: 252,
            },
            Delete {
                client_id: 252,
                pos: 25,
                len: 217,
            },
            NewOp {
                client_id: 157,
                pos: 146,
            },
            Delete {
                client_id: 252,
                pos: 252,
                len: 252,
            },
        ];

        crdt_list::test::normalize_actions(&mut actions, 5, 100);
        dbg!(actions);
    }
}
