use crdt_list::{crdt::{ListCrdt, OpSet}, yata::Yata};
use rle::{rle_tree::{iter::IterMut, SafeCursorMut, RleTreeRaw}, range_map::{RangeMap, WithStartEnd}};

use crate::id::{ID, Counter};

use super::{
    y_span::{YSpan, YSpanTreeTrait},
    Tracker, cursor_map::make_notify,
};

#[derive(Default, Debug)]
struct OpSpanSet {
    map: RangeMap<u128, WithStartEnd<u128, bool>>
}

impl OpSet<YSpan, ID> for OpSpanSet {
    fn insert(&mut self, value: &YSpan) {
        self.map.set(value.id.into(), WithStartEnd { start: value.id.into(), end: value.id.inc(value.len as i32).into(), value: true })
    }

    fn contain(&self, id: ID) -> bool {
        self.map.has(id.into())
    }

    fn clear(&mut self) {
        self.map.clear();
    }
}

struct YataImpl;

impl ListCrdt for YataImpl {
    type OpUnit = YSpan;

    type OpId = ID;

    type Container = Tracker;

    type Set = OpSpanSet;

    type Cursor<'a> = SafeCursorMut<'a, 'static, YSpan, YSpanTreeTrait>;

    type Iterator<'a> = IterMut<'a, 'static, YSpan, YSpanTreeTrait>;

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

        container
        .content
        .with_tree_mut(|tree| 
            // SAFETY: loosen lifetime requirement here. It's safe because the function
            // signature can limit the lifetime of the returned iterator
            unsafe {std::mem::transmute::<_, &mut &mut RleTreeRaw<_, _>>(tree)}.iter_mut_in(from, to)
        )
    }

    fn insert_at(container: &mut Self::Container, op: Self::OpUnit, pos: usize) {
        let mut notify = make_notify(&mut container.id_to_cursor);
        container.content.with_tree_mut(|tree| {
            tree.insert_notify(pos, op,  &mut notify);
        })
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
        // SAFETY: we know this is safe because in [YataImpl::insert_after] there is no access to shared elements
        unsafe {crdt_list::yata::integrate::<Self>(container, op)}
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
        container.content.with_tree(|tree|tree.len())
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

    use crate::{container::text::tracker::y_span::{YSpan, Status}, id::ID};

    use super::OpSpanSet;

    #[test]
    fn test() {
        let mut set = OpSpanSet::default();
        set.insert(
            &YSpan {
                id: ID::new(1, 10), 
                len: 10, 
                origin_left: Some(ID::new(0, 1)), 
                origin_right: Some(ID::new(0, 2)), 
                status: Status::new() 
            }
        );
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
