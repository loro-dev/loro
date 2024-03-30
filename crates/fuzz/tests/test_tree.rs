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
            site: 51,
            target: 51,
            container: 51,
            action: Generic(GenericAction {
                value: I32(858993459),
                bool: true,
                key: 858993459,
                pos: 3689348814741910323,
                length: 3689348814741910323,
                prop: 3689348814741910323,
            }),
        },
        Handle {
            site: 51,
            target: 51,
            container: 51,
            action: Generic(GenericAction {
                value: I32(858993467),
                bool: true,
                key: 858993459,
                pos: 3689348814741910323,
                length: 16083254989265515315,
                prop: 18446744073659220225,
            }),
        },
        Handle {
            site: 247,
            target: 45,
            container: 255,
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
