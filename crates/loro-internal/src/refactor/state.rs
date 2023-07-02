use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;

use crate::{container::ContainerID, InternalString, VersionVector};

mod list;
mod map;
mod text;

#[enum_dispatch]
pub trait ContainerState: Clone {
    fn apply_diff(&mut self);
}

pub struct AppState {
    vv: VersionVector,
    state: FxHashMap<ContainerID, State>,
}

#[enum_dispatch(ContainerState)]
#[derive(Clone)]
pub enum State {
    ListState,
    MapState,
    TextState,
}

#[derive(Clone)]
pub struct ListState {}

#[derive(Clone)]
pub struct MapState {}

#[derive(Clone)]
pub struct TextState {}
