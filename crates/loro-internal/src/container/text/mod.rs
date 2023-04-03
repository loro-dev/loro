pub(crate) mod string_pool;
mod text_container;
pub use text_container::Text;
pub(crate) use text_container::TextContainer;
mod rope;
pub mod text_content;
pub mod tracker;
mod unicode;
#[cfg(feature = "test_utils")]
pub use unicode::test::{apply, Action};
mod utf16;
