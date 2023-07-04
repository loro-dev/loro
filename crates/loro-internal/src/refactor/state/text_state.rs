use jumprope::JumpRope;

use crate::{delta::DeltaItem, event::Diff};

use super::ContainerState;

#[derive(Clone)]
pub struct TextState {
    pub(crate) rope: JumpRope,
}

impl ContainerState for TextState {
    fn apply_diff(&mut self, diff: Diff) {
        if let Diff::Text(delta) = diff {
            let mut index = 0;
            for span in delta.iter() {
                match span {
                    DeltaItem::Retain { len, meta: _ } => {
                        index += len;
                    }
                    DeltaItem::Insert { value, .. } => {
                        self.rope.insert(index, value);
                        index += value.len();
                    }
                    DeltaItem::Delete { len, .. } => {
                        self.rope.remove(index..index + len);
                    }
                }
            }
        }
    }
}
