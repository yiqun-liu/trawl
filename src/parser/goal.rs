//! Goal tracker parser.
//!
//! Parses `## GOAL TRACKER` markdown sections into a [`Goal`] of nested
//! [`GoalItem`]s. Both checkbox lists and tables are recognized; a table row
//! becomes a leaf `GoalItem` whose checked state comes from the done-heuristic.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::NaiveDate;
use regex::Regex;

use crate::metadata;
use crate::model::{Goal, GoalItem, Metadata, Priority, Span};
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

    let items = parse_body(&body, rel, ctx);
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

/// Parse the section body into checkbox forest + table items, in order.
fn parse_body(body: &[(usize, &str)], rel: &Path, ctx: &ParseContext) -> Vec<GoalItem> {
    let mut arena: Vec<Node> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new(); // (level, arena index)
    let mut table_items: Vec<GoalItem> = Vec::new();

    let mut i = 0;
    while i < body.len() {
        let (lineno, line) = body[i];

        if let Some(caps) = checkbox_re().captures(line) {
            let level = caps[1].len() / 2;
            let checked = matches!(caps[2].chars().next(), Some('x') | Some('X') | Some('✓'));
            let (desc, metadata) = metadata::extract(caps[3].trim(), ctx.tokens());
            let item = GoalItem {
                text: desc,
                checked,
                metadata,
                children: Vec::new(),
                span: Span {
                    path: PathBuf::from(rel),
                    line: lineno,
                },
            };
            // Resolve parent: nearest ancestor with a strictly smaller level.
            while stack.last().is_some_and(|(lvl, _)| *lvl >= level) {
                stack.pop();
            }
            let parent = stack.last().map(|&(_, idx)| idx);
            let idx = arena.len();
            arena.push(Node { parent, item });
            stack.push((level, idx));
            i += 1;
            continue;
        }

        // Table: this line is a row and the next line is a separator.
        if is_table_line(line) && body.get(i + 1).is_some_and(|(_, next)| is_table_sep(next)) {
            let (consumed, items) = parse_table(&body[i..], rel, ctx);
            table_items.extend(items);
            i += consumed;
            continue;
        }

        i += 1; // ignored line
    }

    let mut forest = assemble_forest(&arena);
    forest.extend(table_items);
    forest
}

struct Node {
    parent: Option<usize>,
    item: GoalItem,
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
        .map(|&r| assemble(r, arena, &children_of))
        .collect()
}

fn assemble(idx: usize, arena: &[Node], children_of: &[Vec<usize>]) -> GoalItem {
    let mut item = arena[idx].item.clone();
    item.children = children_of[idx]
        .iter()
        .map(|&c| assemble(c, arena, children_of))
        .collect();
    item
}

/// Parse a contiguous table block beginning at `block[0]`. Returns the number
/// of lines consumed and the flat list of leaf items.
fn parse_table(block: &[(usize, &str)], rel: &Path, ctx: &ParseContext) -> (usize, Vec<GoalItem>) {
    let header = parse_row(block[0].1);
    let colmap = map_columns(&header, ctx);

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
        return (consumed, Vec::new());
    }

    let mut items = Vec::new();
    for entry in data {
        let cells = parse_row(entry.1);
        let (text, checked, metadata) = build_row(&cells, &header, &colmap, ctx);
        items.push(GoalItem {
            text,
            checked,
            metadata,
            children: Vec::new(),
            span: Span {
                path: PathBuf::from(rel),
                line: entry.0,
            },
        });
    }
    (consumed, items)
}

/// Map each header cell to a known field name, or `None` for a custom column.
fn map_columns(header: &[String], ctx: &ParseContext) -> Vec<Option<String>> {
    let order = ["task", "state", "owner", "priority", "tag", "due"];
    header
        .iter()
        .map(|cell| {
            let lc = cell.to_lowercase();
            for field in order {
                if let Some(keywords) = ctx.headers().get(field) {
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

/// Done heuristic: a cell counts as done unless it is empty or contains TODO.
fn done_heuristic(cell: &str) -> bool {
    let trimmed = cell.trim();
    !(trimmed.is_empty() || trimmed.to_lowercase().contains("todo"))
}

fn is_table_line(line: &str) -> bool {
    line.contains('|')
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

fn heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(#{1,6})\s+(.+?)\s*$").unwrap())
}

fn checkbox_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\s*)[-*+]\s+\[([ xX✓])\]\s*(.*)$").unwrap())
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
        assert!(!goal.items[0].checked); // "TODO" => not done
        assert!(goal.items[1].checked); // "done" => done
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
    fn malformed_table_without_task_column_is_skipped() {
        let md = "## GOAL TRACKER\n\n| State | Notes |\n|-------|-------|\n| done | hi |\n";
        let goal = parse(md, Path::new("x.md"), &ctx()).unwrap();
        assert!(
            goal.items.is_empty(),
            "table with no task column is skipped"
        );
    }

    #[test]
    fn checkbox_state_characters() {
        let md = "## GOAL TRACKER\n\n- [x] a\n- [ ] b\n- [X] c\n";
        let goal = parse(md, Path::new("x.md"), &ctx()).unwrap();
        let states: Vec<bool> = goal.items.iter().map(|i| i.checked).collect();
        assert_eq!(states, vec![true, false, true]);
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
