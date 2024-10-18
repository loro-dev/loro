pub mod counter;
pub mod list;
pub mod map;
pub mod movable_list;
pub mod text;
pub mod tree;
pub use counter::*;
pub use list::*;
use loro::{LoroError, LoroResult};
pub use map::*;
pub use movable_list::*;
pub use text::*;
pub use tree::*;

/// ignore_container_delete_error
fn unwrap<T>(r: LoroResult<T>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(LoroError::ContainerDeleted { .. }) => None,
        Err(e) => panic!("Error: {}", e),
    }
}
