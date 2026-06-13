//! Directory walking with the six-stage filter pipeline.

use std::path::{Path, PathBuf};

use ignore::{DirEntry, WalkBuilder};

use super::ScanOptions;

/// Walk `options.root`, yielding the paths of files that survive the filter
/// pipeline. Errors for individual entries are logged and skipped; the walk
/// never aborts for a single bad entry.
pub(super) fn walk(options: &ScanOptions) -> Vec<PathBuf> {
    let mut builder = WalkBuilder::new(&options.root);
    // Stage 1: .gitignore-aware via standard filters (git_ignore, parents, …).
    builder.standard_filters(true);
    // Stage 4: hidden files/dirs.
    builder.hidden(!options.scan_hidden);

    let exclude = options.exclude.clone();
    let include = options.include.clone();
    let root = options.root.clone();
    let max_bytes = options.max_bytes;
    // Stages 2, 3, 5: exclude / include / max_file_size.
    builder.filter_entry(move |entry| accept(entry, &root, &exclude, &include, max_bytes));

    let mut out = Vec::new();
    for result in builder.build() {
        match result {
            Ok(entry) => {
                if entry.file_type().map(|t| t.is_file()) == Some(true) {
                    out.push(entry.into_path());
                }
            }
            Err(e) => log::warn!("walk error: {e}"),
        }
    }
    out
}

/// Per-entry accept decision. Returning `false` prunes a directory (it is not
/// descended into) or skips a file.
#[allow(clippy::too_many_arguments)]
fn accept(
    entry: &DirEntry,
    root: &Path,
    exclude: &Option<globset::GlobSet>,
    include: &Option<globset::GlobSet>,
    max_bytes: u64,
) -> bool {
    // Always keep the root itself so the walk begins.
    if entry.depth() == 0 {
        return true;
    }

    let Some(rel) = entry.path().strip_prefix(root).ok() else {
        return true;
    };
    let rel_str = rel.to_string_lossy().replace('\\', "/");

    if let Some(set) = exclude {
        if set.is_match(&rel_str) {
            return false;
        }
    }
    if let Some(set) = include {
        if !set.is_match(&rel_str) {
            return false;
        }
    }

    if entry.file_type().map(|t| t.is_file()) == Some(true) {
        if let Ok(meta) = entry.metadata() {
            if meta.len() > max_bytes {
                return false;
            }
        }
    }
    true
}
