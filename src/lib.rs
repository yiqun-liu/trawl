//! trawl — discover and visualize work items embedded in a repository.
//!
//! This crate exposes the scan/parse pipeline as a library so that
//! integration tests can drive it independently of the CLI. The binary
//! entry point lives in `main.rs`.

pub mod config;
pub mod model;
pub mod scanner;

pub use config::Config;
pub use model::{Goal, GoalItem, InlineTask, Metadata, Priority, Span, Status};
pub use scanner::{FileContents, ScanOptions};
