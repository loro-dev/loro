use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;

use crate::{container::ContainerIdx, event::Diff, version::Frontiers, VersionVector};

use super::arena::SharedArena;

mod list_state;
mod map_state;
mod text_state;

use list_state::ListState;
use map_state::MapState;
use text_state::TextState;

#[derive(Clone)]
pub struct AppState {
    vv: VersionVector,
    frontiers: Frontiers,
    state: FxHashMap<ContainerIdx, State>,
    arena: SharedArena,
}

#[enum_dispatch]
pub trait ContainerState: Clone {
    fn apply_diff(&mut self, diff: Diff);
}

#[enum_dispatch(ContainerState)]
#[derive(Clone)]
pub enum State {
    ListState,
    MapState,
    TextState,
}

pub struct AppStateDiff {
    pub changes: Vec<ContainerStateDiff>,
}

pub struct ContainerStateDiff {
    pub idx: ContainerIdx,
    pub diff: Diff,
}
