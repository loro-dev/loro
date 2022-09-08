use std::ops::{Deref, DerefMut};

use rle::{rle_tree::SafeCursorMut, RleTree};

use super::y_span::{StatusChange, YSpan, YSpanTreeTrait};

#[repr(transparent)]
#[derive(Debug)]
pub(super) struct ContentMap(RleTree<YSpan, YSpanTreeTrait>);

impl Deref for ContentMap {
    type Target = RleTree<YSpan, YSpanTreeTrait>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ContentMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub(super) fn change_status<'a, 'b: 'a>(
    cursor: &mut SafeCursorMut<'a, 'b, YSpan, YSpanTreeTrait>,
    change: StatusChange,
) {
    let value = cursor.as_mut();
    if value.status.apply(change) {
        cursor.update_cache_recursively();
    }
}
