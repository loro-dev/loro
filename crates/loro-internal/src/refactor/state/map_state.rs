use crate::event::Diff;

use super::ContainerState;

#[derive(Clone)]
pub struct MapState {}
impl ContainerState for MapState {
    fn apply_diff(&mut self, diff: Diff) {}
}
