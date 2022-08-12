mod string_tree;
use rle::HasLength;
use rle::RleTree;
use string_tree::{CustomString, StringTreeTrait};

pub fn main() {
    let mut tree: RleTree<CustomString, StringTreeTrait> = RleTree::default();
    for i in 0..(1e6 as usize) {
        tree.with_tree_mut(|tree| {
            if i % 3 == 0 && tree.len() > 0 {
                let start = i % tree.len();
                let len = (i * i) % std::cmp::min(tree.len(), 10);
                let end = std::cmp::min(start + len, tree.len());
                tree.delete_range(Some(start), Some(end))
            } else if tree.len() == 0 {
                tree.insert(0, "a".to_string().into());
            } else {
                tree.insert(i % tree.len(), "a".to_string().into());
            }

            tree.debug_check();
        });
    }
    println!("1M");
}
