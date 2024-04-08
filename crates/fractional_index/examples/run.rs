use fraction_index::FractionalIndex;
use fractional_index::FractionalIndex as MyIndex;

fn main() {
    let mut after = MyIndex::default();
    for _ in 0..10u64.pow(4) {
        after = MyIndex::new_after(&after);
        println!("after {:?}", after.as_bytes().len());
    }
}
