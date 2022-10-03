use crdt_list::crdt::{ListCrdt, OpSet};
use rle::rle_tree::{iter::IterMut, SafeCursorMut, RleTreeRaw};

use crate::id::ID;

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

    fn id(_op: &Self::OpUnit) -> Self::OpId {
        todo!()
    }

    fn cmp_id(_op_a: &Self::OpUnit, _op_b: &Self::OpUnit) -> std::cmp::Ordering {
        todo!()
    }

    fn contains(_op: &Self::OpUnit, _id: Self::OpId) -> bool {
        todo!()
    }

    fn integrate(_container: &mut Self::Container, _op: Self::OpUnit) {
        todo!()
    }

    fn can_integrate(_container: &Self::Container, _op: &Self::OpUnit) -> bool {
        todo!()
    }

    fn len(_container: &Self::Container) -> usize {
        todo!()
    }
}
