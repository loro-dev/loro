use std::fmt::Debug;

use crate::container::CounterAction;
pub use crate::container::MovableListAction;

use super::{
    actor::ActionExecutor,
    container::{ListAction, MapAction, TextAction, TreeAction},
    crdt_fuzzer::FuzzValue,
};
use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use loro::{Container, ContainerType};
use tabled::Tabled;

#[enum_dispatch(Actionable)]
#[derive(Clone)]
pub enum ActionInner {
    Map(MapAction),
    List(ListAction),
    MovableList(MovableListAction),
    Text(TextAction),
    Tree(TreeAction),
    Counter(CounterAction),
}

impl Debug for ActionInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionInner::Map(m) => write!(f, "ActionInner::Map({:?})", m),
            ActionInner::List(l) => write!(f, "ActionInner::List({:?})", l),
            ActionInner::Text(t) => write!(f, "ActionInner::Text({:?})", t),
            ActionInner::Tree(t) => write!(f, "ActionInner::Tree({:?})", t),
            ActionInner::MovableList(m) => write!(f, "ActionInner::MovableList({:?})", m),
            ActionInner::Counter(c) => write!(f, "ActionInner::Counter({:?})", c),
        }
    }
}

impl ActionInner {
    fn from_generic_action(action: &GenericAction, ty: &ContainerType) -> Self {
        match ty {
            ContainerType::Map => Self::Map(MapAction::from_generic_action(action)),
            ContainerType::List => Self::List(ListAction::from_generic_action(action)),
            ContainerType::MovableList => {
                Self::MovableList(MovableListAction::from_generic_action(action))
            }
            ContainerType::Text => Self::Text(TextAction::from_generic_action(action)),
            ContainerType::Tree => Self::Tree(TreeAction::from_generic_action(action)),
            ContainerType::Counter => Self::Counter(CounterAction::from_generic_action(action)),
            ContainerType::Unknown(_) => unreachable!(),
        }
    }
}

#[derive(Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Handle {
        site: u8,
        target: u8,
        container: u8,
        action: ActionWrapper,
    },
    Checkout {
        site: u8,
        to: u32,
    },
    Undo {
        site: u8,
        op_len: u32,
    },
    // For concurrent undo
    SyncAllUndo {
        site: u8,
        op_len: u32,
    },
    Sync {
        from: u8,
        to: u8,
    },
    SyncAll,
}

#[derive(Debug, Clone, EnumAsInner)]
pub enum ActionWrapper {
    Generic(GenericAction),
    Action(ActionInner),
}

impl PartialEq for ActionWrapper {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ActionWrapper::Generic(g1), ActionWrapper::Generic(g2)) => g1 == g2,
            (ActionWrapper::Action(_), ActionWrapper::Action(_)) => unreachable!(),
            _ => false,
        }
    }
}

impl Eq for ActionWrapper {}

impl<'a> Arbitrary<'a> for ActionWrapper {
    fn arbitrary(u: &mut arbitrary::Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(ActionWrapper::Generic(GenericAction::arbitrary(u)?))
    }
}

impl ActionWrapper {
    pub fn convert_to_inner(&mut self, ty: &ContainerType) {
        if let ActionWrapper::Generic(g) = self {
            *self = ActionWrapper::Action(ActionInner::from_generic_action(g, ty));
        }
    }
}

#[derive(Arbitrary, Clone, PartialEq, Eq, Debug)]
pub struct GenericAction {
    pub value: FuzzValue,
    pub bool: bool,
    pub key: u32,
    pub pos: usize,
    pub length: usize,
    pub prop: u64,
}

pub trait FromGenericAction {
    fn from_generic_action(action: &GenericAction) -> Self;
}

impl Tabled for Action {
    const LENGTH: usize = 5;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        match self {
            Action::Sync { from, to } => vec![
                "sync".into(),
                format!("{} with {}", from, to).into(),
                "".into(),
                "".into(),
            ],
            Action::SyncAll => vec!["sync all".into(), "".into(), "".into()],
            Action::Checkout { site, to } => vec![
                "checkout".into(),
                format!("{}", site).into(),
                format!("to {}", to).into(),
                "".into(),
            ],
            Action::Handle {
                site,
                target: _,
                container,
                action,
            } => {
                let mut fields = vec![
                    action.as_action().unwrap().type_name().into(),
                    format!("{}", site).into(),
                    format!("{}", container).into(),
                ];
                fields.extend(action.as_action().unwrap().table_fields());
                fields
            }
            Action::Undo { site, op_len } => vec![
                "undo".into(),
                format!("{}", site).into(),
                format!("{} op len", op_len).into(),
                "".into(),
            ],
            Action::SyncAllUndo { site, op_len } => vec![
                "sync all undo".into(),
                format!("{}", site).into(),
                format!("{} op len", op_len).into(),
                "".into(),
            ],
        }
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "type".into(),
            "site".into(),
            "container".into(),
            "action".into(),
            "value".into(),
        ]
    }
}

#[enum_dispatch]
pub trait Actionable: Debug {
    fn pre_process(&mut self, actor: &mut ActionExecutor, container: usize);
    fn apply(&self, actor: &mut ActionExecutor, container: usize) -> Option<Container>;
    fn ty(&self) -> ContainerType;
    fn table_fields(&self) -> [std::borrow::Cow<'_, str>; 2];
    fn type_name(&self) -> &'static str;
    fn pre_process_container_value(&mut self) -> Option<&mut ContainerType>;
}
