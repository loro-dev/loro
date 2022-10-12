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

        // dbg!(&from, &to);
        // dbg!(&container.content);
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

    fn insert_after_id(container: &mut Self::Container, id: Option<Self::OpId>, op: Self::OpUnit) {
        if let Some(id) = id {
            let left = container.id_to_cursor.get(id.into()).unwrap();
            let left = left.as_cursor(id).unwrap();
            let mut notify = make_notify(&mut container.id_to_cursor);
            // SAFETY: we own the tree here
            unsafe {
                left.unwrap()
                    .shift(1)
                    .unwrap()
                    .insert_notify(op, &mut notify);
            }
        } else {
            let mut notify = make_notify(&mut container.id_to_cursor);
            container.content.insert_at_first(op, &mut notify);
        }
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
    fn issue_1() {
        crdt_list::test::test_with_actions::<YataImpl>(
            2,
            5,
            vec!       [
            NewOp {
                client_id: 18446743798824736406,
                pos: 18446744073699196927,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 0,
                len: 7411535208244772857,
            },
            Delete {
                client_id: 18446540664058413055,
                pos: 10873349650923257855,
                len: 18446603336204419555,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744073702670335,
                len: 18446744073709486335,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 11719107999768421119,
                len: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768945314,
                pos: 11719107999768421026,
            },
            Delete {
                client_id: 10851025925718409122,
                pos: 540508742418326,
                len: 18446504380166307839,
            },
            Delete {
                client_id: 18446744070052118527,
                pos: 18446744073709551615,
                len: 18446744073709524735,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744073709551615,
                len: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107997996196514,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11739092723114877602,
                pos: 10880594044147376802,
            },
            Delete {
                client_id: 18446743137406648319,
                pos: 18446744073709551615,
                len: 18446744073702670335,
            },
            Sync {
                from: 18374686479688400896,
                to: 18446744073709551615,
            },
            Delete {
                client_id: 11719107999768421119,
                pos: 11719107999768421026,
                len: 18446744071947943842,
            },
            Delete {
                client_id: 4294967297,
                pos: 18446744073709551615,
                len: 11719210655348162559,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 18446743672711193250,
            },
            Delete {
                client_id: 11745387828182253567,
                pos: 11719107999768421026,
                len: 11719107999768421538,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            Delete {
                client_id: 18417073299693934335,
                pos: 18446744073709551510,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 15914838024966373375,
                pos: 15914838024376868060,
                len: 15914635714237357276,
            },
            Sync {
                from: 18374686479671623680,
                to: 18446744073709551615,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744073709551615,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744073695461375,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744073709551615,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 18446744073558556672,
                pos: 18446744073642442557,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744073709551614,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 0,
                pos: 0,
                len: 0,
            },
            Sync {
                from: 0,
                to: 0,
            },
            Sync {
                from: 0,
                to: 0,
            },
            Sync {
                from: 0,
                to: 0,
            },
            Sync {
                from: 0,
                to: 0,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744073709551615,
                len: 1099511627775,
            },
            Sync {
                from: 11719107999774539526,
                to: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 18446744072143151778,
                pos: 18446744073709551615,
            },
            Delete {
                client_id: 17144620962624171493,
                pos: 16550640557684026861,
                len: 18391177001763530213,
            },
            Delete {
                client_id: 12659530246668681215,
                pos: 12659530246663417775,
                len: 17144611899198910383,
            },
            Delete {
                client_id: 12659589887623556589,
                pos: 4221573655528072677,
                len: 18446744073707847679,
            },
            Delete {
                client_id: 12659530246663417775,
                pos: 17127101077014949807,
                len: 17144620962624171501,
            },
        ],
        )
    }

    #[test]
    fn normalize() {
        let mut actions = vec![
            NewOp {
                client_id: 18446743798824736406,
                pos: 18446744073709551615,
            },
            Delete {
                client_id: 18446744069431361535,
                pos: 18446744073709551615,
                len: 18446744073709496575,
            },
            Delete {
                client_id: 255,
                pos: 1098353998080,
                len: 18446744069414584320,
            },
            Delete {
                client_id: 13093571490658779135,
                pos: 18374688556288311293,
                len: 12659530248010825727,
            },
            Delete {
                client_id: 18446744073709551535,
                pos: 10880696699727118335,
                len: 18374967954648334335,
            },
            Delete {
                client_id: 18417189201154932735,
                pos: 10880696699727118335,
                len: 10851025326177714175,
            },
            Delete {
                client_id: 18402271027389267903,
                pos: 18446743150291582975,
                len: 18446744073709551615,
            },
            Sync {
                from: 18427322270251745280,
                to: 18374686481397256192,
            },
            Delete {
                client_id: 16565928279328900863,
                pos: 18374688556672476645,
                len: 18446743137406648319,
            },
            Delete {
                client_id: 18417189201154932735,
                pos: 18446463698244468735,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 11719108400766779391,
                pos: 11719107999768421026,
                len: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 12872029504375268002,
            },
            NewOp {
                client_id: 18417188748414850710,
                pos: 18410715272395746198,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744071947943935,
                len: 8573222911,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 11719107999768444927,
                len: 16565928279328924322,
            },
            NewOp {
                client_id: 18446603336204419555,
                pos: 18446744073709551397,
            },
            Delete {
                client_id: 18446744073702670335,
                pos: 18446744073709486335,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 11719107999768421119,
                pos: 11719107999768421026,
                len: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719108000959603362,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 18446641486849286818,
            },
            NewOp {
                client_id: 136118438245406358,
                pos: 18385382526639144704,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 18446744072143151778,
                pos: 18446744073709551615,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            Delete {
                client_id: 10880580798266119830,
                pos: 18446744073709551615,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 18436853815706648575,
                pos: 18446744073709551615,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744073709551615,
                len: 17798225731663167487,
            },
            Delete {
                client_id: 18446744073642442557,
                pos: 18446744073709551615,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 18446744073709551614,
                pos: 11719108400766779391,
                len: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 18446744073709527714,
                pos: 18446744073709551615,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 18446744073709551522,
            },
            Delete {
                client_id: 18446744073709551400,
                pos: 18446744073709524480,
                len: 18391293503297552383,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744073709551615,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 18446744035054846207,
                pos: 72057576858009087,
                len: 18383412203949654016,
            },
            Delete {
                client_id: 18446744073709551615,
                pos: 18446744073709551615,
                len: 18446744073709551615,
            },
            Delete {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
                len: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 18446743672711193250,
            },
            Delete {
                client_id: 11745387828182253567,
                pos: 11719107999768421026,
                len: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
            NewOp {
                client_id: 11719107999768421026,
                pos: 11719107999768421026,
            },
        ];

        crdt_list::test::normalize_actions(&mut actions, 2, 5);
        dbg!(actions);
    }
}
