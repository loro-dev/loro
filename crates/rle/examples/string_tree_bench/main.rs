mod string_tree;
use loro_rle::RleTree;
use string_tree::{CustomString, StringTreeTrait};

pub fn main() {
    let mut tree: RleTree<CustomString, StringTreeTrait> = RleTree::default();
    let len = 1e6 as usize;
    let mut seed = 2;

    for i in 0..(len) {
        seed = (seed * 2) % 10000007;
        if tree.len() > 100000 {
            let start = i % tree.len();
            let len = seed % std::cmp::min(tree.len(), 1);
            let end = std::cmp::min(start + len, tree.len());
            tree.delete_range(Some(start), Some(end))
        } else if tree.len() == 0 {
            tree.insert(0, "0".into());
        } else {
            tree.insert(seed % tree.len(), "a".into());
        }
    }

    println!("{} op, with tree size of {}", len, tree.len());
}
