//! Filesystem scanner: walk, filter, and read repository files.
//!
//! The scanner applies the six-stage filter pipeline described in
//! `docs/requirements.md` → Scan Filtering Semantics, then reads the
//! surviving text files in parallel. It produces [`FileContents`] for the
//! parsers; it does no parsing itself.

use std::path::PathBuf;

use anyhow::Result;
use globset::{Glob, GlobSetBuilder};

use crate::Config;

mod reader;
mod walker;

/// The full text contents of a scanned file.
#[derive(Debug, Clone)]
pub struct FileContents {
    pub path: PathBuf,
    pub content: String,
}

/// Options controlling a scan. Build with [`ScanOptions::new`] or
/// [`ScanOptions::from_config`]; the globsets are compiled internally so the
/// `globset` crate does not leak into the public API.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Repository root to scan.
    pub root: PathBuf,
    exclude: Option<globset::GlobSet>,
    include: Option<globset::GlobSet>,
    /// When false, dotfiles and dot-directories are skipped.
    pub scan_hidden: bool,
    /// Files larger than this (in bytes) are skipped.
    pub max_bytes: u64,
    /// When true, files not tracked by git are skipped.
    pub only_tracked: bool,
}

impl ScanOptions {
    /// Build options from raw pattern lists.
    pub fn new(
        root: PathBuf,
        exclude: &[String],
        include: &[String],
        scan_hidden: bool,
        max_bytes: u64,
        only_tracked: bool,
    ) -> Result<Self> {
        Ok(Self {
            root,
            exclude: compile_patterns(exclude)?,
            include: compile_patterns(include)?,
            scan_hidden,
            max_bytes,
            only_tracked,
        })
    }

    /// Build options from resolved [`Config`].
    pub fn from_config(root: PathBuf, config: &Config) -> Result<Self> {
        Self::new(
            root,
            &config.scan.exclude,
            &config.scan.include,
            config.scan.scan_hidden,
            config.scan.max_bytes().unwrap_or(u64::MAX),
            config.scan.only_tracked,
        )
    }
}

/// Walk the tree, apply the filter pipeline, and read all surviving text
/// files. Binary files (those containing a `0x00` byte) are skipped.
pub fn collect_files(options: &ScanOptions) -> Result<Vec<FileContents>> {
    let paths = walker::walk(options);
    Ok(reader::read_all(paths))
}

/// Compile a list of patterns into a globset. Each pattern `p` matches the
/// entry itself (`p`), its contents (`p/**`), and any nested copy
/// (`**/p`) so that both directory excludes (`docs/`) and recursive file
/// includes (`*.md`) behave intuitively.
fn compile_patterns(patterns: &[String]) -> Result<Option<globset::GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for raw in patterns {
        let p = raw.trim_end_matches('/');
        builder.add(Glob::new(p)?);
        builder.add(Glob::new(&format!("{p}/**"))?);
        builder.add(Glob::new(&format!("**/{p}"))?);
    }
    Ok(Some(builder.build()?))
}
