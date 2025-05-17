#[cfg(not(feature = "logging"))]
pub mod log {
    #[macro_export]
    macro_rules! info { $($(t:tt)*) => {}}
    #[macro_export]
    macro_rules! trace { $($(t:tt)*) => {}}
    #[macro_export]
    macro_rules! debug { $($(t:tt)*) => {}}
    #[macro_export]
    macro_rules! error { $($(t:tt)*) => {}}
    #[macro_export]
    macro_rules! warn { $($(t:tt)*) => {}}
}

#[cfg(feature = "logging")]
pub mod log {
    pub use tracing::{debug, error, info, trace, warn};
}
