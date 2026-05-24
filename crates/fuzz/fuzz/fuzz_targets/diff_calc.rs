#![no_main]

use fuzz::{
    actions::ActionWrapper, test_multi_sites,
    test_multi_sites_on_one_doc_with_peer_seed_and_targets, Action, FuzzTarget,
};
use libfuzzer_sys::fuzz_target;
use loro::ContainerType;

fn lca_biased_actions(actions: Vec<Action>) -> Vec<Action> {
    let mut biased = Vec::with_capacity(actions.len().saturating_mul(2).min(128));
    for (i, action) in actions.into_iter().take(48).enumerate() {
        biased.push(action);

        let site = ((i * 37) % 251) as u8;
        let other = site.wrapping_add(1);
        let version = (i as u32).wrapping_mul(97);
        let injected = match i % 8 {
            0 => Action::Sync {
                from: site,
                to: other,
            },
            1 => Action::DiffApply {
                from: site,
                to: other,
            },
            2 => Action::Checkout { site, to: version },
            3 => Action::ForkAt { site, to: version },
            4 => Action::ImportShallow { site, from: other },
            5 => Action::ExportShallow { site },
            6 => Action::StateOnlyRoundTrip { site },
            _ => Action::Commit { site },
        };
        biased.push(injected);
    }

    biased
}

fn run_text_diff_calc(actions: Vec<Action>) {
    let mut actions = lca_biased_actions(actions);
    test_multi_sites(5, vec![FuzzTarget::Text], &mut actions);
}

fn run_one_doc_diff_calc(actions: Vec<Action>) {
    let peer_seed = peer_seed_from_actions(&actions);
    let mut actions = lca_biased_actions(actions);
    test_multi_sites_on_one_doc_with_peer_seed_and_targets(
        5,
        peer_seed,
        vec![ContainerType::Text],
        &mut actions,
    );
}

fn mix_seed(seed: u64, value: u64) -> u64 {
    seed ^ value
        .wrapping_add(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(seed << 6)
        .wrapping_add(seed >> 2)
}

fn peer_seed_from_actions(actions: &[Action]) -> u64 {
    let mut seed = mix_seed(0xD1FF_CA1C_7E57_0001, actions.len() as u64);
    for action in actions.iter().take(8) {
        seed = match action {
            Action::Handle {
                site,
                target,
                container,
                action,
            } => {
                let mut seed = mix_seed(seed, 0);
                seed = mix_seed(seed, *site as u64);
                seed = mix_seed(seed, *target as u64);
                seed = mix_seed(seed, *container as u64);
                if let ActionWrapper::Generic(g) = action {
                    seed = mix_seed(seed, g.bool as u64);
                    seed = mix_seed(seed, g.key as u64);
                    seed = mix_seed(seed, g.pos as u64);
                    seed = mix_seed(seed, g.length as u64);
                    seed = mix_seed(seed, g.prop);
                }
                seed
            }
            Action::Checkout { site, to } => mix_seed(mix_seed(seed, 1), ((*site as u64) << 32) | *to as u64),
            Action::Undo { site, op_len } => {
                mix_seed(mix_seed(seed, 2), ((*site as u64) << 32) | *op_len as u64)
            }
            Action::SyncAllUndo { site, op_len } => {
                mix_seed(mix_seed(seed, 3), ((*site as u64) << 32) | *op_len as u64)
            }
            Action::Sync { from, to } => {
                mix_seed(mix_seed(seed, 4), ((*from as u64) << 8) | *to as u64)
            }
            Action::SyncAll => mix_seed(seed, 5),
            Action::ForkAt { site, to } => {
                mix_seed(mix_seed(seed, 6), ((*site as u64) << 32) | *to as u64)
            }
            Action::DiffApply { from, to } => {
                mix_seed(mix_seed(seed, 7), ((*from as u64) << 8) | *to as u64)
            }
            Action::Query {
                site,
                target,
                query_type,
            } => mix_seed(
                mix_seed(seed, 8),
                ((*site as u64) << 16) | ((*target as u64) << 8) | *query_type as u64,
            ),
            Action::ExportShallow { site } => mix_seed(mix_seed(seed, 9), *site as u64),
            Action::ImportShallow { site, from } => {
                mix_seed(mix_seed(seed, 10), ((*site as u64) << 8) | *from as u64)
            }
            Action::StateOnlyRoundTrip { site } => mix_seed(mix_seed(seed, 11), *site as u64),
            Action::Commit { site } => mix_seed(mix_seed(seed, 12), *site as u64),
            Action::SetCommitOptions { site, origin, msg } => mix_seed(
                mix_seed(seed, 13),
                ((*site as u64) << 16) | ((*origin as u64) << 8) | *msg as u64,
            ),
        };
    }
    seed
}

fuzz_target!(|actions: Vec<Action>| {
    if actions.is_empty() {
        return;
    }

    let use_one_doc = match &actions[0] {
        Action::Handle { site, .. }
        | Action::Checkout { site, .. }
        | Action::Undo { site, .. }
        | Action::SyncAllUndo { site, .. }
        | Action::ForkAt { site, .. }
        | Action::Query { site, .. }
        | Action::ExportShallow { site }
        | Action::ImportShallow { site, .. }
        | Action::StateOnlyRoundTrip { site }
        | Action::Commit { site }
        | Action::SetCommitOptions { site, .. } => site % 2 == 1,
        Action::Sync { from, .. } | Action::DiffApply { from, .. } => from % 2 == 1,
        Action::SyncAll => false,
    };

    if use_one_doc {
        run_one_doc_diff_calc(actions);
    } else {
        run_text_diff_calc(actions);
    }
});
