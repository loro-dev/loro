use fuzz::{test_multi_sites, Action, FuzzTarget};
use fuzz::actions::{ActionInner, ActionWrapper, FuzzValue};
use loro::ContainerType;

#[test]
#[ignore = "run manually with cargo test -- --ignored"]
fn repro_crash_b612_correct_actions() {
    let actions = vec![
        Action::Handle {
            site: 2,
            target: 1,
            container: 0,
            action: ActionWrapper::Action(ActionInner::MovableList(
                fuzz::actions::MovableListAction::Insert {
                    pos: 0,
                    value: FuzzValue::Container(ContainerType::Text),
                },
            )),
        },
        Action::ImportShallow { site: 1, from: 2 },
        Action::ForkAt { site: 0, to: 0 },
        Action::Handle {
            site: 1,
            target: 5,
            container: 0,
            action: ActionWrapper::Action(ActionInner::List(
                fuzz::actions::ListAction::Insert {
                    pos: 0,
                    value: FuzzValue::I32(671162369),
                },
            )),
        },
        Action::ImportShallow { site: 0, from: 1 },
    ];
    let mut actions = actions;
    test_multi_sites(5, vec![FuzzTarget::All], &mut actions);
}
