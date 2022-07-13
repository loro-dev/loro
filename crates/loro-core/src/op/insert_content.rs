use rle::{HasLength, Mergable, Sliceable};

use crate::id::ID;

#[non_exhaustive]
#[derive(Debug, Clone)]
pub(crate) enum InsertContent {
    Text {
        origin_left: ID,
        origin_right: ID,
        id: ID,
        text: String,
    },
}

impl Mergable for InsertContent {
    fn is_mergable(&self, other: &Self) -> bool {
        match self {
            InsertContent::Text {
                id, origin_right, ..
            } => match other {
                InsertContent::Text {
                    origin_left: other_origin_left,
                    origin_right: other_origin_right,
                    id: other_id,
                    ..
                } => {
                    other_id.client_id == id.client_id
                        && id.counter + self.len() as u32 == other_id.counter
                        && id.client_id == other_origin_left.client_id
                        && id.counter + self.len() as u32 - 1 == other_origin_left.counter
                        && origin_right == other_origin_right
                }
            },
        }
    }

    fn merge(&mut self, other: &Self) {
        match self {
            InsertContent::Text { text, .. } => match other {
                InsertContent::Text {
                    text: other_text, ..
                } => {
                    text.push_str(other_text);
                }
            },
        }
    }
}

impl HasLength for InsertContent {
    fn len(&self) -> usize {
        match self {
            InsertContent::Text { text, .. } => text.len(),
        }
    }
}

impl Sliceable for InsertContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            InsertContent::Text {
                origin_left,
                origin_right,
                id,
                text,
            } => {
                if from == 0 {
                    InsertContent::Text {
                        origin_left: *origin_left,
                        origin_right: *origin_right,
                        id: *id,
                        text: text[..to].to_owned(),
                    }
                } else {
                    InsertContent::Text {
                        origin_left: ID {
                            client_id: id.client_id,
                            counter: id.counter + from as u32 - 1,
                        },
                        origin_right: *origin_right,
                        id: ID {
                            client_id: id.client_id,
                            counter: id.counter + from as u32,
                        },
                        text: text[from..to].to_owned(),
                    }
                }
            }
        }
    }
}
