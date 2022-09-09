use crate::{
    op::{utils::downcast_ref, Op},
    span::IdSpan,
    VersionVector,
};

use self::{content_map::ContentMap, cursor_map::CursorMap};

use super::text_content::TextOpContent;

mod content_map;
mod cursor_map;
mod y_span;

struct Tracker {
    content: ContentMap,
    index: CursorMap,
}

impl Tracker {
    fn turn_on(&mut self, _id: IdSpan) {}
    fn turn_off(&mut self, _id: IdSpan) {}
    fn checkout(&mut self, _vv: VersionVector) {}

    fn apply(&mut self, op: &Op) {
        match &op.content {
            crate::op::OpContent::Normal { content } => {
                if let Some(textContent) = downcast_ref::<TextOpContent>(&**content) {
                    match textContent {
                        TextOpContent::Insert { id, text, pos } => {
                            let yspan = self.content.new_yspan_at_pos(*id, *pos, text.clone());
                        }
                        TextOpContent::Delete { id, pos, len } => todo!(),
                    }
                }
            }
            crate::op::OpContent::Undo { .. } => todo!(),
            crate::op::OpContent::Redo { .. } => todo!(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn create_tracker() -> Tracker {
        Tracker {
            content: Default::default(),
            index: Default::default(),
        }
    }

    #[test]
    fn test_turn_off() {
        let mut tracker = create_tracker();
        tracker.turn_off(IdSpan::new(1, 1, 2));
    }
}
