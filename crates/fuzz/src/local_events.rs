use std::sync::{atomic::AtomicU64, Arc};

use crate::Action;

use loro::ContainerType;
use tracing::trace;

use crate::{actions::ActionWrapper, actor::Actor};

// Fuzz local events on a single document. It sets up a root subscriber that
// mirrors all diffs into a tracker, applies actions locally, commits, and
// asserts tracker == doc.get_deep_value().
pub fn fuzz_local_events(actions: Vec<Action>) {
    if actions.is_empty() {
        return;
    }

    // Single-site actor with root-diff tracker subscriber already wired.
    let mut actor = Actor::new(0);

    // Register all container types we want to exercise.
    actor.register(ContainerType::Map);
    actor.register(ContainerType::List);
    actor.register(ContainerType::MovableList);
    actor.register(ContainerType::Text);
    actor.register(ContainerType::Tree);

    let valid_targets = [
        ContainerType::Map,
        ContainerType::List,
        ContainerType::MovableList,
        ContainerType::Text,
        ContainerType::Tree,
    ];

    let count = Arc::new(AtomicU64::new(0));
    let count_clone = Arc::clone(&count);
    let _sub = actor.loro.subscribe_root(Arc::new(move |e| {
        count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));

    for mut a in actions.into_iter() {
        if let Action::Handle {
            site: _,
            mut target,
            mut container,
            ref mut action,
        } = a
        {
            assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
            // Convert to concrete action based on target container type.
            target %= valid_targets.len() as u8;
            if let ActionWrapper::Generic(_) = action {
                action.convert_to_inner(&valid_targets[target as usize]);
            }

            assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
            // Preprocess against current doc state to ensure valid ranges, etc.
            if let Some(inner) = action.as_action_mut() {
                actor.pre_process_without_commit(inner, &mut container);
            } else {
                continue;
            }
            assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);

            // Apply action and commit so subscribers fire and tracker updates.
            let inner = action.as_action().unwrap();
            actor.apply_without_commit(inner, container);
            assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
        }
    }

    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
    actor.loro.commit();
    let v = count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(v == 0 || v == 1);
    actor.check_tracker();
}
