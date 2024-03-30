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
            site: 99,
            target: 63,
            container: 99,
            action: Generic(GenericAction {
                value: Container(List),
                bool: true,
                key: 53041,
                pos: 61924494876278784,
                length: 41959640056856624,
                prop: 10922656675085166354,
            }),
        },
        SyncAll,
        Handle {
            site: 45,
            target: 45,
            container: 255,
            action: Generic(GenericAction {
                value: Container(Text),
                bool: false,
                key: 4281317946,
                pos: 18446744073694347263,
                length: 10922800942124619263,
                prop: 38293,
            }),
        },
    ])
}
