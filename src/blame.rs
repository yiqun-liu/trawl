//! Git blame enrichment for inline tasks and goal items.
//!
//! For each file that has tasks or goal items, this module runs `git2`
//! blame and annotates each item with its author, commit date, and short
//! commit hash.

use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::DateTime;

use crate::model::{Goal, GoalItem, InlineTask};

/// Enrich every inline task with git blame data.
pub fn enrich_tasks(root: &Path, tasks: &mut [InlineTask]) -> Result<()> {
    let Some(repo) = open_repo(root) else {
        return Ok(());
    };
    let mut by_file: std::collections::BTreeMap<PathBuf, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, t) in tasks.iter().enumerate() {
        by_file.entry(t.span.path.clone()).or_default().push(i);
    }
    for (file_path, indices) in &by_file {
        if let Some(blame) = blame_file(&repo, file_path) {
            for &idx in indices {
                set_blame(&blame, tasks[idx].span.line, |b, d, c| {
                    tasks[idx].blame_author = b;
                    tasks[idx].blame_date = d;
                    tasks[idx].blame_commit = c;
                });
            }
        }
    }
    Ok(())
}

/// Enrich every goal item with git blame data.
pub fn enrich_goals(root: &Path, goals: &mut [Goal]) -> Result<()> {
    let Some(repo) = open_repo(root) else {
        return Ok(());
    };
    for goal in goals.iter_mut() {
        enrich_items(&repo, &mut goal.items);
    }
    Ok(())
}

fn enrich_items(repo: &git2::Repository, items: &mut [GoalItem]) {
    for item in items.iter_mut() {
        if let Some(blame) = blame_file(repo, &item.span.path) {
            set_blame(&blame, item.span.line, |b, d, c| {
                item.blame_author = b;
                item.blame_date = d;
                item.blame_commit = c;
            });
        }
        enrich_items(repo, &mut item.children);
    }
}

fn open_repo(root: &Path) -> Option<git2::Repository> {
    git2::Repository::open(root).ok()
}

fn blame_file<'a>(repo: &'a git2::Repository, path: &Path) -> Option<git2::Blame<'a>> {
    repo.blame_file(path, None).ok()
}

fn set_blame<F>(blame: &git2::Blame<'_>, line: usize, mut set: F)
where
    F: FnMut(Option<String>, Option<chrono::NaiveDateTime>, Option<String>),
{
    let hunk = blame.iter().find(|h| {
        let start = h.final_start_line();
        let end = start + h.lines_in_hunk();
        line >= start && line < end
    });
    if let Some(hunk) = hunk {
        let sig = hunk.final_signature();
        let author = sig.name().map(|s| s.to_string());
        let commit = hunk
            .orig_commit_id()
            .to_string()
            .get(..8)
            .map(|s| s.to_string());
        let date = DateTime::from_timestamp(sig.when().seconds(), 0).map(|dt| dt.naive_utc());
        set(author, date, commit);
    }
}
