use crate::{id::Counter, ContentType, InsertContent, ID};
use rle::{rle_tree::tree_trait::CumulateTreeTrait, HasLength, Mergable, RleTreeTrait, Sliceable};

#[derive(Debug, Clone)]
pub(super) struct YSpan {
    pub origin_left: ID,
    pub origin_right: ID,
    pub id: ID,
    pub text: String,
}

pub(super) type YSpanTreeTrait = CumulateTreeTrait<YSpan, 10>;

impl Mergable for YSpan {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        other.id.client_id == self.id.client_id
            && self.id.counter + self.len() as Counter == other.id.counter
            && self.id.client_id == other.origin_left.client_id
            && self.id.counter + self.len() as Counter - 1 == other.origin_left.counter
            && self.origin_right == other.origin_right
    }

    fn merge(&mut self, other: &Self, _: &()) {
        self.text.push_str(&other.text);
    }
}

impl Sliceable for YSpan {
    fn slice(&self, from: usize, to: usize) -> Self {
        if from == 0 {
            YSpan {
                origin_left: self.origin_left,
                origin_right: self.origin_right,
                id: self.id,
                text: self.text[..to].to_owned(),
            }
        } else {
            YSpan {
                origin_left: ID {
                    client_id: self.id.client_id,
                    counter: self.id.counter + from as Counter - 1,
                },
                origin_right: self.origin_right,
                id: ID {
                    client_id: self.id.client_id,
                    counter: self.id.counter + from as Counter,
                },
                text: self.text[from..to].to_owned(),
            }
        }
    }
}

impl InsertContent for YSpan {
    fn id(&self) -> ContentType {
        ContentType::Text
    }
}

impl HasLength for YSpan {
    fn len(&self) -> usize {
        self.text.len()
    }
}

#[cfg(test)]
mod test {
    use crate::{
        container::{ContainerID, ContainerType},
        id::ROOT_ID,
        ContentType, Op, OpContent, ID,
    };
    use rle::RleVec;

    use super::YSpan;

    #[test]
    fn test_merge() {
        let mut vec: RleVec<Op> = RleVec::new();
        vec.push(Op::new(
            ID::new(0, 1),
            OpContent::Normal {
                content: Box::new(YSpan {
                    origin_left: ID::new(0, 0),
                    origin_right: ID::null(),
                    id: ID::new(0, 1),
                    text: "a".to_owned(),
                }),
            },
            ContainerID::Normal {
                id: ROOT_ID,
                container_type: ContainerType::Text,
            },
        ));
        vec.push(Op::new(
            ID::new(0, 2),
            OpContent::Normal {
                content: Box::new(YSpan {
                    origin_left: ID::new(0, 1),
                    origin_right: ID::null(),
                    id: ID::new(0, 2),
                    text: "b".to_owned(),
                }),
            },
            ContainerID::Normal {
                id: ROOT_ID,
                container_type: ContainerType::Text,
            },
        ));
        assert_eq!(vec.merged_len(), 1);
        let merged = vec.get_merged(0).unwrap();
        assert_eq!(merged.insert_content().id(), ContentType::Text);
        let text_content =
            crate::op::utils::downcast_ref::<YSpan>(&**merged.insert_content()).unwrap();
        assert_eq!(text_content.text, "ab");
    }

    #[test]
    fn slice() {
        let mut vec: RleVec<Op> = RleVec::new();
        vec.push(Op::new(
            ID::new(0, 1),
            OpContent::Normal {
                content: Box::new(YSpan {
                    origin_left: ID::new(0, 0),
                    origin_right: ID::null(),
                    id: ID::new(0, 1),
                    text: "1234".to_owned(),
                }),
            },
            ContainerID::Normal {
                id: ROOT_ID,
                container_type: ContainerType::Text,
            },
        ));
        vec.push(Op::new(
            ID::new(0, 2),
            OpContent::Normal {
                content: Box::new(YSpan {
                    origin_left: ID::new(0, 0),
                    origin_right: ID::new(0, 1),
                    id: ID::new(0, 5),
                    text: "5678".to_owned(),
                }),
            },
            ContainerID::Normal {
                id: ROOT_ID,
                container_type: ContainerType::Text,
            },
        ));
        assert_eq!(vec.merged_len(), 2);
        assert_eq!(
            vec.slice_iter(2, 6)
                .map(|x| crate::op::utils::downcast_ref::<YSpan>(
                    &**x.into_inner().insert_content()
                )
                .unwrap()
                .text
                .clone())
                .collect::<Vec<String>>(),
            vec!["34", "56"]
        )
    }
}

// impl RleTreeTrait<YSpan> for YSpanTreeTrait {
//     const MAX_CHILDREN_NUM: usize;

//     const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;

//     type Int;

//     type InternalCache;

//     type LeafCache;

//     fn update_cache_leaf(node: &mut rle::rle_tree::node::LeafNode<'_, YSpan, Self>) {
//         todo!()
//     }

//     fn update_cache_internal(node: &mut rle::rle_tree::node::InternalNode<'_, YSpan, Self>) {
//         todo!()
//     }

//     fn find_pos_internal(
//         node: &rle::rle_tree::node::InternalNode<'_, YSpan, Self>,
//         index: Self::Int,
//     ) -> (usize, Self::Int, rle::rle_tree::tree_trait::Position) {
//         todo!()
//     }

//     fn find_pos_leaf(
//         node: &rle::rle_tree::node::LeafNode<'_, YSpan, Self>,
//         index: Self::Int,
//     ) -> (usize, usize, rle::rle_tree::tree_trait::Position) {
//         todo!()
//     }

//     fn len_leaf(node: &rle::rle_tree::node::LeafNode<'_, YSpan, Self>) -> usize {
//         todo!()
//     }

//     fn len_internal(node: &rle::rle_tree::node::InternalNode<'_, YSpan, Self>) -> usize {
//         todo!()
//     }
// }
