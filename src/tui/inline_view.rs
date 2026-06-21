//! Inline tasks view: a foldable directory tree.
//!
//! Tasks are grouped by path into a directory tree, then flattened into
//! display rows according to the expand state. Directories containing any
//! high-priority task start expanded. The tree build and flatten are pure
//! functions so they can be unit-tested without a terminal.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::model::{InlineTask, Priority};

/// A directory in the tree. Self-referential but fixed-size (BTreeMap is
/// heap-allocated), so no `Box` is needed.
#[derive(Default)]
pub(super) struct TreeNode {
    dirs: BTreeMap<String, TreeNode>,
    files: BTreeMap<String, FileNode>,
    /// Total tasks anywhere beneath this directory.
    count: usize,
}

struct FileNode {
    task_indices: Vec<usize>,
}

pub(super) enum InlineRowKind {
    Dir(String),
    File(String),
    /// A leaf task. `parent_key` is the file it lives in (for fold actions);
    /// `line` is its source line (for a stable cursor identity).
    Task {
        parent_key: String,
        line: usize,
    },
    /// Source-context lines shown under an expanded task (display-only,
    /// non‑selectable).
    Context,
}

pub(super) struct InlineRow {
    pub(super) kind: InlineRowKind,
    pub(super) text: String,
    pub(super) style: Style,
}

/// Build the directory tree from inline tasks. Task indices reference the
/// original slice, so callers must keep the slice and tree in sync.
pub(super) fn build_tree(tasks: &[InlineTask]) -> TreeNode {
    let mut root = TreeNode::default();
    for (ti, task) in tasks.iter().enumerate() {
        let rel = task.span.path.to_string_lossy().replace('\\', "/");
        let components: Vec<&str> = rel.split('/').filter(|c| !c.is_empty()).collect();
        if components.is_empty() {
            continue;
        }
        let (dirs, file) = components.split_at(components.len() - 1);
        let filename = file[0].to_string();

        let mut cur = &mut root;
        cur.count += 1;
        for d in dirs {
            cur = cur.dirs.entry((*d).to_string()).or_default();
            cur.count += 1;
        }
        cur.files
            .entry(filename)
            .or_insert_with(|| FileNode {
                task_indices: Vec::new(),
            })
            .task_indices
            .push(ti);
    }
    root
}

/// Flatten the tree into display rows. Expanded dirs/files reveal their
/// contents; collapsed ones show a single line with an item count.
pub(super) fn flatten_inline(
    root: &TreeNode,
    tasks: &[InlineTask],
    expanded: &HashSet<String>,
    show_blame: bool,
    file_contents: &HashMap<PathBuf, String>,
    context_lines: u32,
) -> Vec<InlineRow> {
    let mut rows = Vec::new();
    flatten_dir(
        root,
        "",
        0,
        tasks,
        expanded,
        show_blame,
        file_contents,
        context_lines,
        &mut rows,
    );
    rows
}

#[allow(clippy::too_many_arguments)]
fn flatten_dir(
    node: &TreeNode,
    prefix: &str,
    depth: usize,
    tasks: &[InlineTask],
    expanded: &HashSet<String>,
    show_blame: bool,
    file_contents: &HashMap<PathBuf, String>,
    context_lines: u32,
    rows: &mut Vec<InlineRow>,
) {
    let indent = "  ".repeat(depth);
    let task_indent = "  ".repeat(depth + 1);

    for (name, dir) in &node.dirs {
        let key = join_key(prefix, name);
        let marker = if expanded.contains(&key) {
            '▼'
        } else {
            '▸'
        };
        rows.push(InlineRow {
            kind: InlineRowKind::Dir(key.clone()),
            text: format!("{indent}{marker} {name}/  [{}]", dir.count),
            style: Style::default(),
        });
        if expanded.contains(&key) {
            flatten_dir(
                dir,
                &key,
                depth + 1,
                tasks,
                expanded,
                show_blame,
                file_contents,
                context_lines,
                rows,
            );
        }
    }

    for (name, file) in &node.files {
        let key = join_key(prefix, name);
        let marker = if expanded.contains(&key) {
            '▼'
        } else {
            '▸'
        };
        rows.push(InlineRow {
            kind: InlineRowKind::File(key.clone()),
            text: format!("{indent}{marker} {name}  [{}]", file.task_indices.len()),
            style: Style::default(),
        });
        if expanded.contains(&key) {
            for &ti in &file.task_indices {
                let task = &tasks[ti];
                let scope = task
                    .scope
                    .as_deref()
                    .map(|s| format!("({s})"))
                    .unwrap_or_default();
                let badge = task
                    .metadata
                    .priority
                    .as_ref()
                    .map_or(String::new(), |p| format!("  [{}]", p.label()));
                let mut text = format!(
                    "{task_indent}L{}  {}{}  {}",
                    task.span.line, task.keyword, scope, task.description
                );
                if show_blame {
                    if let Some(author) = &task.blame_author {
                        text.push_str(&format!("  ({author}"));
                        if let Some(date) = &task.blame_date {
                            text.push_str(&format!(" {})", date.format("%Y-%m-%d")));
                        } else {
                            text.push(')');
                        }
                    }
                }
                text.push_str(&badge);
                rows.push(InlineRow {
                    kind: InlineRowKind::Task {
                        parent_key: key.clone(),
                        line: task.span.line,
                    },
                    text,
                    style: keyword_style(&task.keyword),
                });
                if task.is_stale(365) {
                    let last = rows.last_mut().unwrap();
                    last.text.push_str("  [stale]");
                    last.style = last.style.add_modifier(Modifier::DIM);
                }

                // Inline expansion: show context lines when the task is
                // expanded.
                let task_key = format!("{key}::{}", task.span.line);
                if expanded.contains(&task_key) && context_lines > 0 {
                    if let Some(content) = file_contents.get(&task.span.path) {
                        let file_lines: Vec<&str> = content.split('\n').collect();
                        let ctx = context_lines as i32;
                        let line_idx = task.span.line.saturating_sub(1);
                        let start = (line_idx as i32 - ctx).max(0) as usize;
                        let end = (line_idx + ctx as usize + 1).min(file_lines.len());
                        for (offset, content) in file_lines[start..end].iter().enumerate() {
                            let actual_idx = start + offset;
                            let is_task_line = actual_idx == line_idx;
                            let marker = if is_task_line { '▸' } else { '▎' };
                            let style = if is_task_line {
                                Style::default()
                            } else {
                                Style::default().add_modifier(Modifier::DIM)
                            };
                            let label = format!("{marker}L{}", actual_idx + 1);
                            rows.push(InlineRow {
                                kind: InlineRowKind::Context,
                                text: format!("{task_indent}  {label}  {}", content),
                                style,
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Directories containing any high-priority task start expanded.
pub(super) fn auto_expand_keys(root: &TreeNode, tasks: &[InlineTask]) -> HashSet<String> {
    let mut set = HashSet::new();
    collect_auto(root, "", tasks, &mut set);
    set
}

fn collect_auto(node: &TreeNode, prefix: &str, tasks: &[InlineTask], set: &mut HashSet<String>) {
    for (name, dir) in &node.dirs {
        let key = join_key(prefix, name);
        if dir_has_high(dir, tasks) {
            set.insert(key.clone());
        }
        collect_auto(dir, &key, tasks, set);
    }
}

fn dir_has_high(dir: &TreeNode, tasks: &[InlineTask]) -> bool {
    for file in dir.files.values() {
        for &ti in &file.task_indices {
            if tasks[ti].metadata.priority == Some(Priority::High) {
                return true;
            }
        }
    }
    dir.dirs.values().any(|d| dir_has_high(d, tasks))
}

fn join_key(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

/// Every directory and file key in the tree, for expand-all.
pub(super) fn all_node_keys(root: &TreeNode) -> Vec<String> {
    let mut keys = Vec::new();
    collect_node_keys(root, "", &mut keys);
    keys
}

fn collect_node_keys(node: &TreeNode, prefix: &str, out: &mut Vec<String>) {
    for (name, dir) in &node.dirs {
        let key = join_key(prefix, name);
        out.push(key.clone());
        collect_node_keys(dir, &key, out);
    }
    for name in node.files.keys() {
        out.push(join_key(prefix, name));
    }
}

/// Foreground color for a task row based on its keyword.
fn keyword_style(keyword: &str) -> Style {
    match keyword.to_ascii_lowercase().as_str() {
        "todo" => Style::default().fg(Color::Cyan),
        "fixme" | "bug" => Style::default().fg(Color::Red),
        "hack" | "xxx" => Style::default().fg(Color::Magenta),
        "note" => Style::default().fg(Color::Blue),
        _ => Style::default(),
    }
}

/// Count tasks by triage level: `(high, med, low, untagged)`. Anything that is
/// not high/med/low (including `None` and custom `Other` values) is untagged.
fn priority_breakdown(tasks: &[InlineTask]) -> (usize, usize, usize, usize) {
    let (mut high, mut med, mut low, mut untagged) = (0, 0, 0, 0);
    for t in tasks {
        match &t.metadata.priority {
            Some(Priority::High) => high += 1,
            Some(Priority::Med) => med += 1,
            Some(Priority::Low) => low += 1,
            _ => untagged += 1,
        }
    }
    (high, med, low, untagged)
}

/// Render the inline tasks view. Stateful so the viewport follows the cursor.
pub(super) fn draw(f: &mut Frame, app: &super::App, area: Rect) {
    let items: Vec<ListItem> = app
        .inline_rows
        .iter()
        .map(|row| {
            ListItem::new(super::search::highlighted_line(
                &row.text,
                row.style,
                &app.search_query,
            ))
        })
        .collect();

    let (high, med, low, untagged) = priority_breakdown(&app.inline_displayed);
    let count = if app.filter.is_some() {
        format!("{}/{}", app.inline_displayed.len(), app.inline_tasks.len())
    } else {
        app.inline_displayed.len().to_string()
    };
    let sort_label = match app.sort_mode {
        super::SortMode::Path => "path",
        super::SortMode::Priority => "priority",
        super::SortMode::Keyword => "keyword",
        super::SortMode::Age => "age",
    };
    let stale_count = app
        .inline_displayed
        .iter()
        .filter(|t| t.is_stale(365))
        .count();
    let blame_indicator = if app.show_blame { " blame" } else { "" };
    let title = format!(
        "Inline Tasks  ({count})  [{sort_label}{blame_indicator}]  high:{high} med:{med} low:{low} untagged:{untagged} stale:{stale_count}"
    );
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = ListState::default();
    if app.inline_rows.is_empty() {
        state.select(None);
    } else {
        state.select(Some(app.inline_selected));
    }
    f.render_stateful_widget(list, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Metadata, Span};
    use std::path::PathBuf;

    fn task(path: &str, line: usize, kw: &str, prio: Option<Priority>) -> InlineTask {
        InlineTask {
            keyword: kw.into(),
            scope: None,
            description: "desc".into(),
            metadata: Metadata {
                priority: prio,
                ..Default::default()
            },
            span: Span {
                path: PathBuf::from(path),
                line,
            },
            blame_author: None,
            blame_date: None,
            blame_commit: None,
        }
    }

    #[test]
    fn builds_tree_with_counts() {
        let tasks = vec![
            task("src/a.rs", 1, "TODO", None),
            task("src/a.rs", 9, "FIXME", None),
            task("src/b.rs", 2, "TODO", None),
            task("docs/g.md", 3, "TODO", None),
        ];
        let root = build_tree(&tasks);
        assert_eq!(root.count, 4);
        assert_eq!(root.dirs["src"].count, 3);
        assert_eq!(root.dirs["docs"].count, 1);
        assert_eq!(root.dirs["src"].files["a.rs"].task_indices.len(), 2);
    }

    #[test]
    fn collapsed_shows_only_top_level() {
        let tasks = vec![task("src/a.rs", 1, "TODO", None)];
        let root = build_tree(&tasks);
        let rows = flatten_inline(&root, &tasks, &HashSet::new(), false, &HashMap::new(), 0);
        assert_eq!(rows.len(), 1); // just "src/"
        assert!(rows[0].text.contains("src/"));
        assert!(rows[0].text.contains("[1]"));
    }

    #[test]
    fn expanded_reveals_files_and_tasks() {
        let tasks = vec![
            task("src/a.rs", 1, "TODO", None),
            task("src/a.rs", 9, "FIXME", None),
        ];
        let root = build_tree(&tasks);
        let mut expanded = HashSet::new();
        expanded.insert("src".to_string());
        expanded.insert("src/a.rs".to_string());
        let rows = flatten_inline(&root, &tasks, &expanded, false, &HashMap::new(), 0);
        // src/ (expanded), a.rs (expanded), task@1, task@9
        assert_eq!(rows.len(), 4);
        assert!(rows[2].text.contains("L1"));
        assert!(rows[3].text.contains("L9"));
        // a leaf carries its file's key, so fold acts on the parent file
        assert!(matches!(
            &rows[2].kind,
            InlineRowKind::Task { parent_key, .. } if parent_key == "src/a.rs"
        ));
    }

    #[test]
    fn auto_expand_marks_high_priority_dirs_only() {
        let tasks = vec![
            task("a/x.rs", 1, "FIXME", Some(Priority::High)),
            task("b/y.rs", 2, "TODO", None),
        ];
        let root = build_tree(&tasks);
        let keys = auto_expand_keys(&root, &tasks);
        assert!(keys.contains("a"));
        assert!(!keys.contains("b"));
    }

    #[test]
    fn keyword_style_colors() {
        // Tests that each keyword gets a non-default style (panics on mismatch)
        assert_ne!(keyword_style("TODO"), Style::default());
        assert_ne!(keyword_style("fixme"), Style::default());
        assert_ne!(keyword_style("HACK"), Style::default());
        assert_ne!(keyword_style("note"), Style::default());
        assert_ne!(keyword_style("BUG"), Style::default());
    }

    #[test]
    fn flatten_shows_blame_when_flag_set() {
        let task = InlineTask {
            keyword: "TODO".into(),
            scope: None,
            description: "a task".into(),
            metadata: Metadata::default(),
            span: Span {
                path: PathBuf::from("a.rs"),
                line: 1,
            },
            blame_author: Some("alice".into()),
            blame_date: None,
            blame_commit: None,
        };
        let tasks = vec![task];
        let root = build_tree(&tasks);
        let mut expanded = HashSet::new();
        expanded.insert("a.rs".to_string());
        let rows = flatten_inline(&root, &tasks, &expanded, false, &HashMap::new(), 0);
        assert!(!rows[1].text.contains("alice"));
        let rows = flatten_inline(&root, &tasks, &expanded, true, &HashMap::new(), 0);
        assert!(rows[1].text.contains("alice"));
    }
}
