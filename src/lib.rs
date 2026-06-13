//! trawl — discover and visualize work items embedded in a repository.
//!
//! This crate exposes the scan/parse pipeline as a library so that
//! integration tests can drive it independently of the CLI. The binary
//! entry point lives in `main.rs`.

pub mod config;
pub mod metadata;
pub mod model;
pub mod parser;
pub mod scanner;

use std::path::Path;

use anyhow::Result;

pub use config::Config;
pub use model::{Goal, GoalItem, InlineTask, Metadata, Priority, Span, Status};
pub use parser::ParseContext;
pub use scanner::{FileContents, ScanOptions};

/// The outcome of a scan: discovered goals and inline tasks.
pub struct ScanResult {
    pub goals: Vec<Goal>,
    pub inline_tasks: Vec<InlineTask>,
}

/// Walk the repository, then parse every file for goals and inline tasks.
pub fn scan(options: &ScanOptions, ctx: &ParseContext) -> Result<ScanResult> {
    let files = scanner::collect_files(options)?;
    let root = &options.root;

    let mut goals = Vec::new();
    let mut inline_tasks = Vec::new();
    for fc in &files {
        let rel: &Path = fc.path.strip_prefix(root).unwrap_or(&fc.path);
        inline_tasks.extend(parser::inline::parse_content(&fc.content, rel, ctx));
        if is_markdown(&fc.path) {
            if let Some(goal) = parser::goal::parse(&fc.content, rel, ctx) {
                goals.push(goal);
            }
        }
    }
    Ok(ScanResult {
        goals,
        inline_tasks,
    })
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}
