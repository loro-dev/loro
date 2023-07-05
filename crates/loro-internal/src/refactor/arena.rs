use crate::container::ContainerID;

/// This is shared between [OpLog] and [AppState].
/// It uses a immutable data structure inside so that we have O(1) clone time.
/// It can make sharing data between threads easier.
///
#[derive(Clone)]
pub(super) struct SharedArena {
    /// The parent of each container.
    parents: im::HashMap<ContainerID, ContainerID>,
}
