//! Zenith's library half: everything except the window.
//!
//! The binary in `main.rs` is the command-line shell; `app` owns the window. The
//! modules below are ordinary Rust: the document, the folder walk, the review, the
//! motions. A second front end — a TUI, a test harness — can use them without going
//! through egui, except where a module says otherwise.
//!
//! The one deliberate seam is [`tone`]: the parser speaks in semantic tones, and each
//! front end decides what ink to give them.

pub mod app;
pub mod catalog;
pub mod distill;
pub mod folder;
pub mod fuzzy;
pub mod lsp;
pub mod markdown;
pub mod outline;
pub mod png;
pub mod reader;
pub mod review;
pub mod syntax;
pub mod theme;
pub mod tone;
pub mod typography;
pub mod vim;
