use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action, Action::*, FuzzTarget, FuzzValue::*},
};
use loro::ContainerType::*;

fn test_actions(mut actions: Vec<Action>) {
    test_multi_sites(5, vec![FuzzTarget::Tree], &mut actions)
}

#[ctor::ctor]
fn init_color_backtrace() {
    color_backtrace::install();
    use tracing_subscriber::{prelude::*, registry::Registry};
    if option_env!("DEBUG").is_some() {
        tracing::subscriber::set_global_default(
            Registry::default().with(tracing_subscriber::fmt::Layer::default()),
        )
        .unwrap();
    }
}

#[test]
fn tree() {
    test_actions(vec![
        Handle {
            site: 39,
            target: 89,
            container: 0,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: false,
                key: 3840206052,
                pos: 16565899576681489636,
                length: 103235241436389,
                prop: 16493407079447038225,
            }),
        },
        Handle {
            site: 0,
            target: 0,
            container: 0,
            action: Generic(GenericAction {
                value: I32(0),
                bool: false,
                key: 0,
                pos: 0,
                length: 0,
                prop: 0,
            }),
        },
    ])
}
