//! The egui-free half of Zenith: the document, the folder walk, the review, the motions
//! and the language servers.
//!
//! This crate must not depend on egui. The window decides what ink to use; the core
//! decides what the ink *is*. That split is what lets a TUI reuse the reading model
//! without dragging a GPU renderer along with it.

pub mod catalog;
pub mod distill;
pub mod folder;
pub mod fuzzy;
pub mod lsp;
pub mod markdown;
pub mod outline;
pub mod review;
pub mod syntax;
pub mod tone;
pub mod vim;

#[cfg(test)]
mod testutil;
