#![no_main]

use libfuzzer_sys::fuzz_target;

use fuzz::{test_multi_sites, test_multi_sites_on_one_doc, Action, FuzzTarget};

fuzz_target!(|actions: Vec<Action>| {
    if actions.is_empty() {
        return;
    }
    // Split corpus 50/50 between multi-site sync and single-doc (detached editing) fuzzing.
    // Using the first action's site field parity makes the split deterministic per input.
    let is_one_doc = match &actions[0] {
        Action::Handle { site, .. } => site % 2 == 1,
        Action::Checkout { site, .. } => site % 2 == 1,
        Action::Undo { site, .. } => site % 2 == 1,
        Action::SyncAllUndo { site, .. } => site % 2 == 1,
        Action::Sync { from, .. } => from % 2 == 1,
        Action::ForkAt { site, .. } => site % 2 == 1,
        Action::DiffApply { from, .. } => from % 2 == 1,
        Action::Query { site, .. } => site % 2 == 1,
        Action::ExportShallow { site, .. } => site % 2 == 1,
        Action::ImportShallow { site, .. } => site % 2 == 1,
        Action::StateOnlyRoundTrip { site } => site % 2 == 1,
        Action::Commit { site } => site % 2 == 1,
        Action::SetCommitOptions { site, .. } => site % 2 == 1,
        Action::SyncAll => false,
    };
    if is_one_doc {
        test_multi_sites_on_one_doc(5, &mut actions.clone());
    } else {
        test_multi_sites(5, vec![FuzzTarget::All], &mut actions.clone());
    }
});
