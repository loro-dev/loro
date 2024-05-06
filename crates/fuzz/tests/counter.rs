use fuzz::{test_multi_sites, FuzzTarget};

#[test]
fn counter() {
    test_multi_sites(5, vec![FuzzTarget::Counter], &mut [])
}
