use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;

use crate::{
    container::{ContainerID, ContainerIdx},
    event::Diff,
    version::Frontiers,
    InternalString, VersionVector,
};

use super::arena::SharedArena;

mod list;
mod map;
mod text;

#[enum_dispatch]
pub trait ContainerState: Clone {
    fn apply_diff(&mut self, diff: Diff);
}

#[derive(Clone)]
pub struct AppState {
    vv: VersionVector,
    frontiers: Frontiers,
    state: FxHashMap<ContainerIdx, State>,
    arena: SharedArena,
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

impl ContainerState for ListState {
    fn apply_diff(&mut self, diff: Diff) {}
}

#[derive(Clone)]
pub struct MapState {}
impl ContainerState for MapState {
    fn apply_diff(&mut self, diff: Diff) {}
}

#[derive(Clone)]
pub struct TextState {}
impl ContainerState for TextState {
    fn apply_diff(&mut self, diff: Diff) {}
}

pub struct AppStateDiff {
    from: VersionVector,
    to: VersionVector,
    changes: Vec<ContainerStateDiff>,
}

pub struct ContainerStateDiff {
    id: ContainerIdx,
    diff: Diff,
}
