//! Git blame enrichment for inline tasks.
//!
//! For each file that has inline tasks, this module runs a single `git2`
//! blame pass and annotates each [`InlineTask`](crate::model::InlineTask) with
//! its author, commit date, and short commit hash.

use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::DateTime;

use crate::model::InlineTask;

/// Enrich every inline task with git blame data. Files without corresponding
/// tasks are never touched; repos that fail to open are silently skipped.
pub fn enrich_tasks(root: &Path, tasks: &mut [InlineTask]) -> Result<()> {
    let repo = match git2::Repository::open(root) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    // Group task indices by file so we only blame each file once.
    let mut by_file: std::collections::BTreeMap<PathBuf, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, task) in tasks.iter().enumerate() {
        by_file.entry(task.span.path.clone()).or_default().push(i);
    }

    for (file_path, indices) in &by_file {
        let blob = match repo.blame_file(file_path, None) {
            Ok(b) => b,
            Err(_) => continue,
        };

        for &idx in indices {
            let line = tasks[idx].span.line;
            let hunk = blob.iter().find(|h| {
                let start = h.final_start_line();
                let end = start + h.lines_in_hunk();
                line >= start && line < end
            });
            let Some(hunk) = hunk else { continue };
            let sig = hunk.final_signature();
            tasks[idx].blame_author = sig.name().map(|s| s.to_string());
            tasks[idx].blame_commit = hunk
                .orig_commit_id()
                .to_string()
                .get(..8)
                .map(|s| s.to_string());
            tasks[idx].blame_date =
                DateTime::from_timestamp(sig.when().seconds(), 0).map(|dt| dt.naive_utc());
        }
    }

    Ok(())
}
