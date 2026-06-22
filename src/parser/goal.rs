//! Goal tracker parser.
//!
//! Parses `## GOAL TRACKER` markdown sections into a [`Goal`] of nested
//! [`GoalItem`]s. Both checkbox lists and tables are recognized; a table row
//! becomes a leaf `GoalItem` whose checked state comes from the done-heuristic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::NaiveDate;
use regex::Regex;

use crate::metadata;
use crate::model::{Goal, GoalItem, Metadata, NodeState, Priority, Reference, Span};
use crate::parser::ParseContext;

/// Parse a file's contents into a [`Goal`], if it contains a goal section.
///
/// `rel` is the file path relative to the scan root (used for the title
/// fallback and the location badge).
pub fn parse(content: &str, rel: &Path, ctx: &ParseContext) -> Option<Goal> {
    let lines: Vec<&str> = content.lines().collect();
    let names: Vec<String> = ctx
        .goal_section_names()
        .iter()
        .map(|s| s.trim().to_lowercase())
        .collect();

    // Locate the first matching heading at any level.
    let (start, level) = find_section_start(&lines, &names)?;

    // Collect the section body until a same-or-higher-level heading.
    let mut body: Vec<(usize, &str)> = Vec::new();
    for (idx, line) in lines.iter().enumerate().skip(start + 1) {
        if let Some(caps) = heading_re().captures(line) {
            if caps[1].len() <= level {
                break;
            }
        }
        body.push((idx + 1, line));
    }

    let items = parse_body(&body, rel, level, ctx);
    let title = title_of(&lines).unwrap_or_else(|| filename_stem(rel));
    let badge = badge(rel);

    Some(Goal {
        title,
        source_file: PathBuf::from(rel),
        badge,
        items,
    })
}

/// Find the first heading whose text matches a section name. Returns
/// `(line_index, heading_level)`.
fn find_section_start(lines: &[&str], names: &[String]) -> Option<(usize, usize)> {
    for (i, line) in lines.iter().enumerate() {
        if let Some(caps) = heading_re().captures(line) {
            let text = caps[2].trim().to_lowercase();
            if names.iter().any(|n| n == &text) {
                return Some((i, caps[1].len()));
            }
        }
    }
    None
}

/// Parse the section body into a goal item forest. Recognizes four line
/// kinds: checkbox items (`- [ ]`/`- [x]`), plain bullets with children
/// (`- group`), subsection headings (`### ...`), and tables. Headings and
/// indentation form parallel stacks — a heading resets the indent stack and
/// establishes a fresh subtree context; checkboxes/plain-bullets nest within
/// the current heading context by 2-space indentation.
fn parse_body(
    body: &[(usize, &str)],
    rel: &Path,
    section_level: usize,
    ctx: &ParseContext,
) -> Vec<GoalItem> {
    let mut arena: Vec<Node> = Vec::new();
    // (indent_level, arena_index) — for checkbox/plain-bullet nesting within
    // the current heading context.
    let mut indent_stack: Vec<(usize, usize)> = Vec::new();
    // (relative_heading_level, arena_index) — for heading-based nesting.
    // Reset to empty at start; cleared implicitly when a shallower heading
    // pops deeper ones off the top.
    let mut heading_stack: Vec<(usize, usize)> = Vec::new();

    let mut in_code = false;
    let mut i = 0;
    while i < body.len() {
        let (lineno, line) = body[i];

        // Fenced code block: a line starting with ``` or ~~~ opens or
        // closes a code span. While inside, every line is ignored so that
        // pipe-rows, `- [ ]`, etc. in a code block are not mistaken for
        // tracker content (the parser does not otherwise special-case code).
        if is_fence(line) {
            in_code = !in_code;
            i += 1;
            continue;
        }
        if in_code {
            i += 1;
            continue;
        }

        // Heading within the section: becomes a Group node.
        if let Some(caps) = heading_re().captures(line) {
            let heading_level = caps[1].len();
            let relative_level = heading_level.saturating_sub(section_level);
            // A heading resets the indentation context.
            indent_stack.clear();
            // Pop the heading stack until we find a strictly shallower heading.
            while heading_stack
                .last()
                .is_some_and(|(lvl, _)| *lvl >= relative_level)
            {
                heading_stack.pop();
            }
            let parent = heading_stack.last().map(|&(_, idx)| idx);
            let (desc, metadata) = metadata::extract(caps[2].trim(), ctx.tokens());
            let item = GoalItem {
                text: desc,
                state: NodeState::Group,
                metadata,
                reference: None,
                warning: None,
                children: Vec::new(),
                span: Span {
                    path: PathBuf::from(rel),
                    line: lineno,
                },
                blame_author: None,
                blame_date: None,
                blame_commit: None,
            };
            let idx = arena.len();
            arena.push(Node {
                parent,
                item,
                drop_if_leaf: false,
            });
            heading_stack.push((relative_level, idx));
            i += 1;
            continue;
        }

        // Checkbox item.
        if let Some(caps) = checkbox_re().captures(line) {
            let level = caps[1].len() / 2;
            let checked = matches!(caps[2].chars().next(), Some('x') | Some('X') | Some('✓'));
            let (desc, metadata) = metadata::extract(caps[3].trim(), ctx.tokens());
            let (text, reference) = split_reference(desc);
            let item = GoalItem {
                text,
                state: NodeState::Checkbox { checked },
                metadata,
                reference,
                warning: None,
                children: Vec::new(),
                span: Span {
                    path: PathBuf::from(rel),
                    line: lineno,
                },
                blame_author: None,
                blame_date: None,
                blame_commit: None,
            };
            let parent = resolve_parent(&indent_stack, &heading_stack, level);
            // Pop indent stack while top is at same-or-deeper level.
            while indent_stack.last().is_some_and(|(lvl, _)| *lvl >= level) {
                indent_stack.pop();
            }
            let idx = arena.len();
            arena.push(Node {
                parent,
                item,
                drop_if_leaf: false,
            });
            indent_stack.push((level, idx));
            i += 1;
            continue;
        }

        // Plain bullet (no checkbox): becomes a Group node if it ends up with
        // children; dropped at assembly otherwise so context notes inside the
        // section continue to be ignored.
        if let Some(caps) = plain_bullet_re().captures(line) {
            let level = caps[1].len() / 2;
            let (desc, metadata) = metadata::extract(caps[2].trim(), ctx.tokens());
            let (text, reference) = split_reference(desc);
            // A reference-bearing bullet survives even without children (the
            // resolver attaches cloned children in Pass 2). A bare note like
            // "- see also: foo.md" does not.
            let drop_if_leaf = reference.is_none();
            let item = GoalItem {
                text,
                state: NodeState::Group,
                metadata,
                reference,
                warning: None,
                children: Vec::new(),
                span: Span {
                    path: PathBuf::from(rel),
                    line: lineno,
                },
                blame_author: None,
                blame_date: None,
                blame_commit: None,
            };
            let parent = resolve_parent(&indent_stack, &heading_stack, level);
            while indent_stack.last().is_some_and(|(lvl, _)| *lvl >= level) {
                indent_stack.pop();
            }
            let idx = arena.len();
            arena.push(Node {
                parent,
                item,
                drop_if_leaf,
            });
            indent_stack.push((level, idx));
            i += 1;
            continue;
        }

        // Standalone reference line: `[[target]]` or `[text](target)` on its
        // own, without a `- ` bullet. Becomes a Group node with a reference.
        // Indentation (leading whitespace) is respected, mirroring bullet
        // nesting.
        if let Some((raw_target, display_text)) = match_reference(line.trim()) {
            let level = line.len().saturating_sub(line.trim_start().len()) / 2;
            let item = GoalItem {
                text: display_text.clone(),
                state: NodeState::Group,
                metadata: Metadata::default(),
                reference: Some(Reference::Pending {
                    raw_target,
                    display_text,
                }),
                warning: None,
                children: Vec::new(),
                span: Span {
                    path: PathBuf::from(rel),
                    line: lineno,
                },
                blame_author: None,
                blame_date: None,
                blame_commit: None,
            };
            let parent = resolve_parent(&indent_stack, &heading_stack, level);
            while indent_stack.last().is_some_and(|(lvl, _)| *lvl >= level) {
                indent_stack.pop();
            }
            let idx = arena.len();
            arena.push(Node {
                parent,
                item,
                drop_if_leaf: false,
            });
            indent_stack.push((level, idx));
            i += 1;
            continue;
        }

        // Table: this line is a row and the next line is a separator. Table
        // rows are flat leaves attached to the current heading context (or
        // the goal root if no heading is open).
        if is_table_line(line) && body.get(i + 1).is_some_and(|(_, next)| is_table_sep(next)) {
            let (consumed, rows) = parse_table(&body[i..], rel, ctx);
            let parent = heading_stack.last().map(|&(_, idx)| idx);
            for row in rows {
                arena.push(Node {
                    parent,
                    item: row,
                    drop_if_leaf: false,
                });
            }
            i += consumed;
            continue;
        } else if is_table_line(line)
            && body
                .get(i + 1)
                .is_some_and(|(_, next)| is_table_line(next) && !is_table_sep(next))
        {
            // Malformed table: a header-looking pipe row immediately followed
            // by another pipe row with no separator between them. Emit a
            // warning marker rather than silently dropping the block.
            let mut consumed = 0;
            while body
                .get(i + consumed)
                .is_some_and(|(_, l)| is_table_line(l))
            {
                consumed += 1;
            }
            let lineno = body[i].0;
            let parent = heading_stack.last().map(|&(_, idx)| idx);
            arena.push(Node {
                parent,
                item: table_warning_marker(rel, lineno, "malformed table: missing separator row"),
                drop_if_leaf: false,
            });
            i += consumed;
            continue;
        }

        i += 1; // ignored line
    }

    assemble_forest(&arena)
}

/// Resolve the parent arena index for a checkbox/plain-bullet at `level`:
/// the nearest open indent ancestor, falling back to the current heading
/// context if the indent stack is empty.
fn resolve_parent(
    indent_stack: &[(usize, usize)],
    heading_stack: &[(usize, usize)],
    level: usize,
) -> Option<usize> {
    indent_stack
        .iter()
        .rev()
        .find(|(lvl, _)| *lvl < level)
        .map(|&(_, idx)| idx)
        .or_else(|| heading_stack.last().map(|&(_, idx)| idx))
}

struct Node {
    parent: Option<usize>,
    item: GoalItem,
    /// Plain bullets are dropped at assembly if they end up with no children
    /// (preserving the "context notes inside the section stay ignored"
    /// behavior). Headings and checkboxes are always retained.
    drop_if_leaf: bool,
}

fn assemble_forest(arena: &[Node]) -> Vec<GoalItem> {
    let mut children_of: Vec<Vec<usize>> = vec![Vec::new(); arena.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (i, node) in arena.iter().enumerate() {
        match node.parent {
            Some(p) => children_of[p].push(i),
            None => roots.push(i),
        }
    }
    roots
        .iter()
        .filter(|&&r| !should_prune(r, arena, &children_of))
        .map(|&r| assemble(r, arena, &children_of))
        .collect()
}

fn assemble(idx: usize, arena: &[Node], children_of: &[Vec<usize>]) -> GoalItem {
    let node = &arena[idx];
    let children: Vec<GoalItem> = children_of[idx]
        .iter()
        .filter(|&&c| !should_prune(c, arena, children_of))
        .map(|&c| assemble(c, arena, children_of))
        .collect();
    let mut item = node.item.clone();
    item.children = children;
    item
}

/// Whether arena node `idx` should be pruned from the assembled tree: a
/// plain-bullet group node (drop_if_leaf = true) that ended up with no
/// children. Such nodes represent context notes ("see also: ...") that the
/// user did not intend as structural — preserving the long-standing behavior
/// that ignored non-checkbox content inside a goal tracker section.
fn should_prune(idx: usize, arena: &[Node], children_of: &[Vec<usize>]) -> bool {
    arena[idx].drop_if_leaf && children_of[idx].is_empty()
}

/// Build a non-checkbox warning marker leaf for a table trawl could not
/// parse (malformed structure, or no task column). The marker carries the
/// human-readable `message` in [`GoalItem::warning`] so the TUI and
/// `--no-tui` surface it consistently.
fn table_warning_marker(rel: &Path, line: usize, message: &str) -> GoalItem {
    GoalItem {
        text: String::new(),
        state: NodeState::Group,
        metadata: Metadata::default(),
        reference: None,
        warning: Some(message.to_string()),
        children: Vec::new(),
        span: Span {
            path: PathBuf::from(rel),
            line,
        },
        blame_author: None,
        blame_date: None,
        blame_commit: None,
    }
}

/// Parse a contiguous table block beginning at `block[0]`. Returns the number
/// of lines consumed and the flat list of leaf items.
fn parse_table(block: &[(usize, &str)], rel: &Path, ctx: &ParseContext) -> (usize, Vec<GoalItem>) {
    let header = parse_row(block[0].1);
    let colmap = map_columns(&header, ctx.headers());

    // Count data rows (contiguous pipe-lines after header + separator).
    let mut consumed = 2; // header + separator
    let mut data: Vec<&(usize, &str)> = Vec::new();
    while let Some(entry) = block.get(consumed) {
        if !is_table_line(entry.1) {
            break;
        }
        data.push(entry);
        consumed += 1;
    }

    // A table without a task column cannot be parsed.
    let has_task = colmap.iter().any(|c| c.as_deref() == Some("task"));
    if !has_task {
        return (
            consumed,
            vec![table_warning_marker(
                rel,
                block[0].0,
                "table skipped: no task column",
            )],
        );
    }

    let mut items = Vec::new();
    for entry in data {
        let cells = parse_row(entry.1);
        let (text, checked, metadata) = build_row(&cells, &header, &colmap, ctx);
        items.push(GoalItem {
            text,
            state: NodeState::Checkbox { checked },
            metadata,
            reference: None,
            warning: None,
            children: Vec::new(),
            span: Span {
                path: PathBuf::from(rel),
                line: entry.0,
            },
            blame_author: None,
            blame_date: None,
            blame_commit: None,
        });
    }
    (consumed, items)
}

/// Map each header cell to a known field name, or `None` for a custom column.
fn map_columns(header: &[String], headers: &HashMap<String, Vec<String>>) -> Vec<Option<String>> {
    let order = ["task", "state", "owner", "priority", "tag", "due"];
    header
        .iter()
        .map(|cell| {
            let lc = cell.to_lowercase();
            for field in order {
                if let Some(keywords) = headers.get(field) {
                    if keywords.iter().any(|k| lc.contains(&k.to_lowercase())) {
                        return Some(field.to_string());
                    }
                }
            }
            None
        })
        .collect()
}

/// Build one row's `(description, checked, metadata)`. Column values override
/// any inline tokens embedded in the task cell.
fn build_row(
    cells: &[String],
    header: &[String],
    colmap: &[Option<String>],
    ctx: &ParseContext,
) -> (String, bool, Metadata) {
    let task_text = colmap
        .iter()
        .position(|c| c.as_deref() == Some("task"))
        .and_then(|i| cells.get(i))
        .map(String::as_str)
        .unwrap_or("");
    let (description, mut meta) = metadata::extract(task_text, ctx.tokens());

    let mut checked = false;
    for (i, cell) in cells.iter().enumerate() {
        let Some(field) = colmap.get(i).and_then(|c| c.as_deref()) else {
            // Custom column: key = header cell, value = this cell.
            if let Some(key) = header.get(i) {
                if !key.is_empty() && !cell.is_empty() {
                    meta.custom
                        .entry(key.clone())
                        .or_default()
                        .push(cell.clone());
                }
            }
            continue;
        };
        match field {
            "task" => {}
            "state" => checked = done_heuristic(cell),
            "owner" if !cell.is_empty() => meta.owner = Some(cell.clone()),
            "priority" if !cell.is_empty() => meta.priority = Some(Priority::parse(cell)),
            "due" if !cell.is_empty() => {
                meta.due = NaiveDate::parse_from_str(cell.trim(), "%Y-%m-%d").ok();
            }
            "tag" if !cell.is_empty() => {
                meta.tags = cell
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
            }
            _ => {}
        }
    }

    (description, checked, meta)
}

/// Done heuristic: a cell counts as done unless it is empty or contains `TODO`.
pub(crate) fn done_heuristic(cell: &str) -> bool {
    let trimmed = cell.trim();
    !(trimmed.is_empty() || trimmed.to_lowercase().contains("todo"))
}

fn is_table_line(line: &str) -> bool {
    line.contains('|')
}

/// Whether `line` opens or closes a fenced code block — three or more
/// backticks or tildes at the start (ignoring leading whitespace), per the
/// CommonMark fence rule. The fence line itself is not tracker content.
fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

fn is_table_sep(line: &str) -> bool {
    if !line.contains('|') {
        return false;
    }
    let cells = parse_row(line);
    if cells.is_empty() {
        return false;
    }
    cells
        .iter()
        .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
}

/// Split a table row into trimmed cells, dropping empty outer cells.
fn parse_row(line: &str) -> Vec<String> {
    let mut cells: Vec<String> = line.split('|').map(|c| c.trim().to_string()).collect();
    while cells.first().is_some_and(String::is_empty) {
        cells.remove(0);
    }
    while cells.last().is_some_and(String::is_empty) {
        cells.pop();
    }
    cells
}

/// For a table data row at 1-based `lineno` in `lines`, locate the state
/// column (by scanning up to the header) and return `(state_col_index,
/// current_value)`. `None` if the line is not in a table or has no state
/// column.
pub(crate) fn table_state_cell(
    lines: &[&str],
    lineno: usize,
    headers: &HashMap<String, Vec<String>>,
) -> Option<(usize, String)> {
    let row_idx = lineno.checked_sub(1)?;
    let row_line = lines.get(row_idx)?;
    if !is_table_line(row_line) {
        return None;
    }
    // Scan upward for the separator, then the header just above it.
    let sep_idx = (0..row_idx).rev().find(|&i| is_table_sep(lines[i]))?;
    if sep_idx == 0 {
        return None;
    }
    let header = parse_row(lines[sep_idx - 1]);
    let colmap = map_columns(&header, headers);
    let state_col = colmap.iter().position(|c| c.as_deref() == Some("state"))?;
    let cells = parse_row(row_line);
    let value = cells.get(state_col).cloned().unwrap_or_default();
    Some((state_col, value))
}

/// Rebuild a table row with the cell at `state_col` replaced by `new_value`,
/// preserving the outer pipes and other cells (padding is normalized).
pub(crate) fn rewrite_state_cell(row_line: &str, state_col: usize, new_value: &str) -> String {
    let had_outer = row_line.trim_start().starts_with('|');
    let mut cells = parse_row(row_line);
    if state_col < cells.len() {
        cells[state_col] = new_value.trim().to_string();
    }
    let body = cells.join(" | ");
    if had_outer {
        format!("| {body} |")
    } else {
        body
    }
}

/// Inspect a post-metadata-extraction description. If it is exactly a
/// reference (whole text matches `[[target]]` or `[text](target)`), return
/// `(display_text, Some(Pending))`. Otherwise return `(original_text, None)`
/// so the item is treated as a literal task/milestone as before.
fn split_reference(desc: String) -> (String, Option<Reference>) {
    if let Some((raw_target, display_text)) = match_reference(&desc) {
        let text = display_text.clone();
        (
            text,
            Some(Reference::Pending {
                raw_target,
                display_text,
            }),
        )
    } else {
        (desc, None)
    }
}

fn heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(#{1,6})\s+(.+?)\s*$").unwrap())
}

fn checkbox_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\s*)[-*+]\s+\[([ xX✓])\]\s*(.*)$").unwrap())
}

/// Plain bullet (`- text`, `* text`, `+ text`) without a checkbox. Must be
/// checked *after* [`checkbox_re`] so checkbox lines are not misclassified.
fn plain_bullet_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\s*)[-*+]\s+(.+?)\s*$").unwrap())
}

/// Wikilink reference form: `[[target]]` (optionally with `#anchor`,
/// which the resolver strips). Must match the whole trimmed text.
fn wikilink_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\[\[([^\]]+)\]\]$").unwrap())
}

/// Markdown link reference form: `[display](target)`. Must match the whole
/// trimmed text.
fn mdlink_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\[([^\]]+)\]\(([^)\s]+)\)$").unwrap())
}

/// If `text` is exactly a reference (entire content is a wikilink or
/// markdown link), return `(raw_target, display_text)`. `display_text` is
/// empty for wikilinks and the link text for markdown links. Returns `None`
/// for embedded references (e.g., `"see [[x]] for details"`) — those stay
/// literal text; only line-as-reference is structurally meaningful.
fn match_reference(text: &str) -> Option<(String, String)> {
    let text = text.trim();
    if let Some(caps) = wikilink_re().captures(text) {
        let target = caps[1].trim();
        if target.is_empty() {
            return None;
        }
        return Some((target.to_string(), String::new()));
    }
    if let Some(caps) = mdlink_re().captures(text) {
        let display = caps[1].trim();
        let target = caps[2].trim();
        if target.is_empty() {
            return None;
        }
        return Some((target.to_string(), display.to_string()));
    }
    None
}

/// First H1 (`#`) heading text in the file, if any.
fn title_of(lines: &[&str]) -> Option<String> {
    for line in lines {
        if let Some(caps) = heading_re().captures(line) {
            if caps[1].len() == 1 {
                return Some(caps[2].trim().to_string());
            }
        }
    }
    None
}

fn filename_stem(rel: &Path) -> String {
    rel.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "untitled".to_string())
}

/// Location badge: the super-directory of the file's owning directory, with a
/// trailing slash; `(root)` when the file sits at or one level under the root.
fn badge(rel: &Path) -> String {
    let Some(owning) = rel.parent() else {
        return "(root)".to_string();
    };
    if owning.as_os_str().is_empty() {
        return "(root)".to_string();
    }
    let Some(superdir) = owning.parent() else {
        return "(root)".to_string();
    };
    let s = superdir.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        "(root)".to_string()
    } else {
        format!("{s}/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn ctx() -> ParseContext {
        ParseContext::from_config(&Config::default()).unwrap()
    }

    #[test]
    fn parses_checkbox_tree_with_progress() {
        let md = "# Complete Course\n\n## GOAL TRACKER\n\n- [x] Week 1\n  - [x] Lecture 1\n  - [ ] Lecture 2\n- [ ] Week 2\n  - [ ] Lecture 3\n";
        let goal = parse(md, Path::new("course/README.md"), &ctx()).unwrap();
        assert_eq!(goal.title, "Complete Course");
        // leaves: Lecture1(done), Lecture2, Lecture3 => 1/3
        assert!((goal.progress() - (1.0 / 3.0)).abs() < 1e-9);
        assert_eq!(goal.items.len(), 2); // Week1 (milestone), Week2 (milestone)
        assert!(goal.items[0].is_milestone());
        assert_eq!(goal.items[0].children.len(), 2);
    }

    #[test]
    fn parses_table_format() {
        let md = "## GOAL TRACKER\n\n| Task | State | Priority |\n|------|-------|----------|\n| OAuth flow | TODO | high |\n| Token refresh | done | med |\n";
        let goal = parse(md, Path::new("sprint/plan.md"), &ctx()).unwrap();
        assert_eq!(goal.items.len(), 2);
        assert!(!goal.items[0].checked().unwrap(), "TODO state is not done");
        assert!(goal.items[1].checked().unwrap(), "done state is done");
        assert_eq!(goal.items[0].metadata.priority, Some(Priority::High));
    }

    #[test]
    fn ignores_content_outside_section() {
        let md = "# T\n\n- [x] not in section\n\n## GOAL TRACKER\n\n- [x] real task\n\n## References\n\n- [x] after section\n";
        let goal = parse(md, Path::new("x.md"), &ctx()).unwrap();
        let total: usize = goal.items.len();
        assert_eq!(total, 1, "only the in-section task should parse");
    }

    #[test]
    fn returns_none_without_section() {
        let md = "# T\n\n- [x] just a list\n";
        assert!(parse(md, Path::new("x.md"), &ctx()).is_none());
    }

    #[test]
    fn zero_leaf_goal_is_planned() {
        // A milestone with no tasks underneath has zero leaves.
        let md = "## GOAL TRACKER\n\n- [ ] Phase 1\n";
        let goal = parse(md, Path::new("x.md"), &ctx()).unwrap();
        assert_eq!(goal.progress(), 0.0);
        assert_eq!(goal.status(), crate::model::Status::Planned);
    }

    #[test]
    fn title_falls_back_to_filename_without_h1() {
        let md = "## GOAL TRACKER\n\n- [x] a task\n";
        let goal = parse(md, Path::new("notes/project.md"), &ctx()).unwrap();
        assert_eq!(goal.title, "project");
    }

    #[test]
    fn badge_is_super_directory() {
        let g = parse(
            "## GOAL TRACKER\n\n- [x] x\n",
            Path::new("ml/llm/cs146s/README.md"),
            &ctx(),
        )
        .unwrap();
        assert_eq!(g.badge, "ml/llm/");
    }

    #[test]
    fn badge_is_root_for_top_level_file() {
        let g = parse(
            "## GOAL TRACKER\n\n- [x] x\n",
            Path::new("README.md"),
            &ctx(),
        )
        .unwrap();
        assert_eq!(g.badge, "(root)");
    }

    #[test]
    fn table_without_task_column_produces_warning_marker() {
        let md = "## GOAL TRACKER\n\n| State | Notes |\n|-------|-------|\n| done | hi |\n";
        let goal = parse(md, Path::new("x.md"), &ctx()).unwrap();
        assert_eq!(
            goal.items.len(),
            1,
            "table with no task column yields a warning marker, not silence"
        );
        let w = goal.items[0]
            .warning
            .as_deref()
            .expect("marker carries a warning");
        assert!(
            w.contains("no task column"),
            "warning mentions no task column: {w:?}"
        );
    }

    #[test]
    fn malformed_table_missing_separator_produces_warning_marker() {
        // Header directly followed by a data row — no `|---|` separator.
        let md = "## GOAL TRACKER\n\n| Task | Priority |\n| a | high |\n| b | low |\n";
        let goal = parse(md, Path::new("x.md"), &ctx()).unwrap();
        assert_eq!(
            goal.items.len(),
            1,
            "malformed table collapses to one warning marker"
        );
        let m = &goal.items[0];
        let w = m.warning.as_deref().expect("marker carries a warning");
        assert!(
            w.contains("missing separator"),
            "warning mentions missing separator: {w:?}"
        );
        assert!(m.children.is_empty());
    }

    #[test]
    fn fenced_code_block_is_ignored() {
        // A fenced block containing a fake checkbox and two pipe-rows (which
        // would otherwise look like a malformed table) must produce no items
        // and no warning marker. Only the real task after the fence counts.
        let md =
            "## GOAL TRACKER\n\n```\n- [ ] fake task\n| a | b |\n| c | d |\n```\n- [x] real task\n";
        let goal = parse(md, Path::new("x.md"), &ctx()).unwrap();
        assert_eq!(goal.items.len(), 1, "only the real task after the fence");
        assert_eq!(goal.items[0].text, "real task");
        assert!(
            goal.items[0].warning.is_none(),
            "no malformed-table marker from the fenced block"
        );
    }

    #[test]
    fn tilde_fence_is_ignored() {
        let md = "## GOAL TRACKER\n\n~~~\n- [ ] fake\n~~~\n- [ ] real\n";
        let goal = parse(md, Path::new("x.md"), &ctx()).unwrap();
        assert_eq!(goal.items.len(), 1);
        assert_eq!(goal.items[0].text, "real");
    }

    #[test]
    fn checkbox_state_characters() {
        let md = "## GOAL TRACKER\n\n- [x] a\n- [ ] b\n- [X] c\n";
        let goal = parse(md, Path::new("x.md"), &ctx()).unwrap();
        let states: Vec<Option<bool>> = goal.items.iter().map(|i| i.checked()).collect();
        assert_eq!(states, vec![Some(true), Some(false), Some(true)]);
    }

    #[test]
    fn does_not_panic_on_malformed_input() {
        // Unclosed paren in scope-like text, weird indentation, broken table.
        let md =
            "## GOAL TRACKER\n\n- [ ] task (unclosed\n      - [z] bad state\n| a |\n|---|\n| b |\n";
        let goal = parse(md, Path::new("x.md"), &ctx());
        assert!(goal.is_some()); // parsed something without panicking
    }
}
