#[cfg(not(feature = "fuzzing"))]
fn main() {}

#[cfg(feature = "fuzzing")]
fn main() {
    use crdt_list::test;
    use crdt_list::test::Action::*;
    use loro_core::container::text::tracker::yata_impl::YataImpl;
    let mut actions = vec![];
    for i in 0..500_usize {
        actions.push(if i % 2 == 0 {
            NewOp {
                client_id: i as u8,
                pos: i as u8,
            }
        } else {
            Delete {
                client_id: i as u8,
                pos: i as u8,
                len: (i + 1) as u8,
            }
        })
    }

    for _ in 0..1000 {
        test::test_with_actions::<YataImpl>(5, 100, actions.clone());
    }

    println!("HAHA");
}
