//! Prototype of the typed model layer for issue #888.
//!
//! Design and validation results: `plans/20260427-loro-model-typed-projection.md`.
//!
//! - Data types implement [`FromLoro`] (projection) and [`ToLoro`] (reconcile).
//! - [`LoroSchema`] links a data type to its typed [`View`].
//! - Views store locations, not data; `read_as`, `update_from`, `subscribe`
//!   and `watch` are provided methods on [`View`].
//!
//! There is no derive macro yet. `tests/common/mod.rs` contains the code the
//! derive is expected to generate, written only from information a macro has.

mod convert;
mod error;
pub mod list;
mod slot;
mod view;

pub use convert::{counter, read_field, text, FromLoro, ToLoro};
pub use error::{ErrorKind, ModelError, PathSeg, Result};
pub use slot::{ListRef, Slot};
pub use view::{KeyedVec, ListView, Loc, LoroSchema, TextField, ValueField, View};
