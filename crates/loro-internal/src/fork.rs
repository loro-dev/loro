//! The fork module provides functionality to create a new LoroDoc instance at a specified version
//! (Frontiers) with minimal overhead.
//!
//! # Implementation Overview
//!
//! The `fork_at` function in this module allows for the creation of a new document that reflects
//! the state of the original document at a given version. The function achieves this by:
//!
//! ## Exporting Necessary Data:
//!
//! - **Change Store Data**: Collects all changes up to the specified version from the change
//!   store's key-value (kv) data store. It includes the version vector and frontiers for accurate
//!   identification of the version.
//!
//! - **Container Store Data**: Exports the container store's kv data representing the document's
//!   state at the specified version. This involves checking out to the desired version, exporting
//!   the state, and efficiently checking back to the latest version.
//!
//! - **GC Store Data**: If applicable, exports the gc store's kv data, ensuring that version
//!   identifiers are included.
//!
//! ## Reconstructing the New Document:
//!
//! Imports the exported data into a new LoroDoc instance using optimized import mechanisms
//! similar to those used in fast snapshot imports.
//!
//! By focusing on exporting only the necessary data and optimizing state transitions during
//! version checkout, the `fork_at` function minimizes overhead and efficiently creates new
//! document instances representing past versions.
//!
use std::borrow::Cow;

use crate::{version::Frontiers, LoroDoc};

impl LoroDoc {
    /// Creates a new LoroDoc at a specified version (Frontiers)
    pub fn fork_at(&self, frontiers: &Frontiers) -> LoroDoc {
        let bytes = self
            .export(crate::loro::ExportMode::SnapshotAt {
                version: Cow::Borrowed(frontiers),
            })
            .unwrap();
        let doc = LoroDoc::new();
        doc.import(&bytes).unwrap();
        doc
    }
}
