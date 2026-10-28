//! The window's half of Zenith: the reading page, the typography, the palette and the
//! shell around them.
//!
//! The model lives in [`zenith_core`]: the document, the folder walk, the review, the
//! motions, the language servers. Nothing here is needed by a second front end, and
//! nothing there knows about egui. A TUI depends on `zenith-core` only.

pub mod app;
pub mod png;
pub mod reader;
pub mod theme;
pub mod typography;
