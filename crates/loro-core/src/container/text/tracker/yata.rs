use crdt_list::{crdt::{ListCrdt, OpSet}, yata::Yata};
use rle::rle_tree::{iter::IterMut, SafeCursorMut, RleTreeRaw};

use crate::id::{ID, Counter};

use super::{
    y_span::{YSpan, YSpanTreeTrait},
    Tracker, cursor_map::make_notify,
};

#[derive(Default, Debug)]
struct OpSpanSet {}

impl OpSet<YSpan, ID> for OpSpanSet {
    fn insert(&mut self, _value: &YSpan) {
        todo!()
    }

    fn contain(&self, _id: ID) -> bool {
        todo!()
    }

    fn clear(&mut self) {
        todo!()
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
        crdt_list::yata::integrate::<Self>(container, op)
    }

    fn can_integrate(_container: &Self::Container, _op: &Self::OpUnit) -> bool {
        todo!()
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

    fn insert_after(anchor: &mut Self::Cursor<'_>, op: Self::OpUnit) {
        todo!()
    }
}
