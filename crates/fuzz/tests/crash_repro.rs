use std::fs;
use arbitrary::{Arbitrary, Unstructured};
use fuzz::{test_multi_sites, Action, FuzzTarget};

fn run_from_bytes(bytes: &[u8]) {
    let mut u = Unstructured::new(bytes);
    let mut actions: Vec<Action> = Vec::arbitrary(&mut u).unwrap();
    eprintln!("Parsed {} actions:", actions.len());
    for (i, a) in actions.iter().enumerate() {
        eprintln!("  [{i}] {a:?}");
    }
    test_multi_sites(5, vec![FuzzTarget::All], &mut actions);
}

#[test]
#[ignore = "run manually with cargo test -- --ignored"]
fn repro_crash_6044ee() {
    let bytes = fs::read(
        "fuzz/artifacts/all/minimized-from-6044ee2550f38e09837a1de90d5f0651b201950a",
    )
    .unwrap();
    run_from_bytes(&bytes);
}

#[test]
#[ignore = "run manually with cargo test -- --ignored"]
fn repro_crash_b2b3d9() {
    let bytes = fs::read(
        "fuzz/artifacts/all/crash-b2b3d977f1a39847187e0ac6e71b0dbe8cd0d592",
    )
    .unwrap();
    run_from_bytes(&bytes);
}
