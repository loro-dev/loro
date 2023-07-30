pub(crate) mod string_pool;

mod rope;
pub mod text_content;
pub mod tracker;
mod unicode;
#[cfg(feature = "test_utils")]
pub use unicode::test::{apply, Action};
pub(crate) mod utf16;
