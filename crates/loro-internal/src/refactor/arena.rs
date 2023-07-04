use im::Vector;

use crate::container::{ContainerID, ContainerIdx};

/// This is shared between [OpLog] and [AppState].
/// It uses a immutable data structure inside so that we have O(1) clone time.
/// It can make sharing data between threads easier.
///
#[derive(Clone)]
pub(super) struct SharedArena {
    containers: Vector<ContainerID>,
    id_to_idx: im::HashMap<ContainerID, ContainerIdx>,
    /// The parent of each container.
    parents: Vector<Option<ContainerIdx>>,
}
