use loro_core::{content, ContentTypeID, InsertContent, ID};
use rle::{HasLength, Mergable};

#[derive(Debug, Clone)]
pub struct TextInsertContent {
    origin_left: ID,
    origin_right: ID,
    id: ID,
    text: String,
}

impl Mergable for TextInsertContent {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        other.id.client_id == self.id.client_id
            && self.id.counter + self.len() as u32 == other.id.counter
            && self.id.client_id == other.origin_left.client_id
            && self.id.counter + self.len() as u32 - 1 == other.origin_left.counter
            && self.origin_right == other.origin_right
    }

    fn merge(&mut self, other: &Self, _: &()) {
        self.text.push_str(&other.text);
    }
}

impl InsertContent for TextInsertContent {
    fn id(&self) -> ContentTypeID {
        ContentTypeID::Text
    }

    fn slice(&self, from: usize, to: usize) -> Box<dyn InsertContent> {
        if from == 0 {
            Box::new(TextInsertContent {
                origin_left: self.origin_left,
                origin_right: self.origin_right,
                id: self.id,
                text: self.text[..to].to_owned(),
            })
        } else {
            Box::new(TextInsertContent {
                origin_left: ID {
                    client_id: self.id.client_id,
                    counter: self.id.counter + from as u32 - 1,
                },
                origin_right: self.origin_right,
                id: ID {
                    client_id: self.id.client_id,
                    counter: self.id.counter + from as u32,
                },
                text: self.text[from..to].to_owned(),
            })
        }
    }

    fn clone_content(&self) -> Box<dyn InsertContent> {
        Box::new(self.clone())
    }
}

impl HasLength for TextInsertContent {
    fn len(&self) -> usize {
        self.text.len()
    }
}

#[cfg(test)]
mod test {
    use loro_core::{content, ContentTypeID, Op, OpContent, ID};
    use rle::RleVec;

    use crate::TextInsertContent;

    #[test]
    fn test_merge() {
        let mut vec: RleVec<Op> = RleVec::new();
        vec.push(Op::new(
            ID::new(0, 1),
            OpContent::Insert {
                container: ID::new(0, 0),
                content: Box::new(TextInsertContent {
                    origin_left: ID::new(0, 0),
                    origin_right: ID::null(),
                    id: ID::new(0, 1),
                    text: "a".to_owned(),
                }),
            },
        ));
        vec.push(Op::new(
            ID::new(0, 2),
            OpContent::Insert {
                container: ID::new(0, 0),
                content: Box::new(TextInsertContent {
                    origin_left: ID::new(0, 1),
                    origin_right: ID::null(),
                    id: ID::new(0, 2),
                    text: "b".to_owned(),
                }),
            },
        ));
        assert_eq!(vec.merged_len(), 1);
        let merged = vec.get_merged(0);
        assert_eq!(merged.content().id(), ContentTypeID::Text);
        let text_content = content::downcast_ref::<TextInsertContent>(&**merged.content()).unwrap();
        assert_eq!(text_content.text, "ab");
    }

    #[test]
    fn slice() {
        let mut vec: RleVec<Op> = RleVec::new();
        vec.push(Op::new(
            ID::new(0, 1),
            OpContent::Insert {
                container: ID::new(0, 0),
                content: Box::new(TextInsertContent {
                    origin_left: ID::new(0, 0),
                    origin_right: ID::null(),
                    id: ID::new(0, 1),
                    text: "1234".to_owned(),
                }),
            },
        ));
        vec.push(Op::new(
            ID::new(0, 2),
            OpContent::Insert {
                container: ID::new(0, 0),
                content: Box::new(TextInsertContent {
                    origin_left: ID::new(0, 0),
                    origin_right: ID::new(0, 1),
                    id: ID::new(0, 5),
                    text: "5678".to_owned(),
                }),
            },
        ));
        assert_eq!(vec.merged_len(), 2);
        assert_eq!(
            vec.slice_iter(2, 6)
                .map(
                    |x| content::downcast_ref::<TextInsertContent>(&**x.into_inner().content())
                        .unwrap()
                        .text
                        .clone()
                )
                .collect::<Vec<String>>(),
            vec!["34", "56"]
        )
    }
}
