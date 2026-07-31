//! DataMapper — library root.
//!
//! Public modules used by both `main.rs` (the binary entrypoint) and
//! `tests/it_end_to_end.rs` (integration tests spinning up a full
//! axum server against a temp DSL tree).
//!
//! See `docs/DESIGN.md` for the domain overview and
//! `book/src/introduction.md` for user-facing docs.

pub mod config;
pub mod error;
pub mod helpers;
pub mod renderer;
pub mod router;
