mod list;
mod map;
mod text;

pub trait ContainerState: Clone {
    fn apply_diff(&mut self);
}
