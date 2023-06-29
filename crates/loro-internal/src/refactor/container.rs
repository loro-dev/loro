use crate::{event::Diff, VersionVector};

use super::oplog::OpLog;

pub trait Container {
    fn diff(&self, log: &OpLog, before: &VersionVector, after: &VersionVector) -> Vec<Diff>;
}
