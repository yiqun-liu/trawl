//! Goals view: flatten the goal forest into display rows and render it.
//!
//! Folding is hierarchical and key-string-addressed: a goal header has key
//! `g{idx}` and each milestone has key `g{idx}/{child_path}` (e.g. `g0/1/0`).
//! Expanding a goal shows its top-level items; each milestone folds/unfolds
//! independently. Flattening is a pure function over the data so it can be
//! unit-tested without a terminal.

use std::collections::HashSet;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::model::{BrokenReason, Goal, GoalItem, NodeState, Priority, Reference, Status};

/// Which goal a row belongs to. Headers and milestones are foldable; tasks
/// (leaves) are not.
pub(super) enum GoalRowKind {
    Header {
        key: String,
    },
    Milestone {
        key: String,
    },
    /// A leaf task. `key` is its own position (for toggle); `parent_key` is
    /// the foldable node it belongs to (a milestone or the goal header), so
    /// expand/collapse act on the parent.
    Task {
        key: String,
        parent_key: String,
    },
}

pub(super) struct GoalRow {
    pub(super) kind: GoalRowKind,
    pub(super) text: String,
    pub(super) style: Style,
}

/// Flatten goals into display rows according to the expand set.
/// Uses two passes so all progress bars align at the same column.
pub(super) fn flatten_goals(
    goals: &[Goal],
    expanded: &HashSet<String>,
    show_blame: bool,
) -> Vec<GoalRow> {
    if goals.is_empty() {
        return Vec::new();
    }

    // Pass 1: compute max prefix width (marker + title + badge + padding).
    let max_prefix = goals
        .iter()
        .map(|goal| format!("▸ {}  {}  ", goal.title, goal.badge).len())
        .max()
        .unwrap_or(40);

    // Pass 2: render each header, padding the prefix so the right chunk
    // (progress bar + percentage) starts at a fixed column.
    let mut rows = Vec::new();
    for (gi, goal) in goals.iter().enumerate() {
        let key = format!("g{gi}");
        let marker = if expanded.contains(&key) {
            '▼'
        } else {
            '▸'
        };
        let pct = (goal.progress() * 100.0).round() as u32;
        let completed = goal.status() == Status::Completed;
        let prefix = format!("{marker} {}  {}  ", goal.title, goal.badge);
        let right = format!("[{}]  {}%", progress_bar(pct), pct);
        let pad = max_prefix.saturating_sub(prefix.len()) + 2;
        rows.push(GoalRow {
            kind: GoalRowKind::Header { key: key.clone() },
            text: format!("{prefix}{:pad$}{right}", ""),
            style: if completed {
                Style::default().add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)
            } else {
                Style::default()
            },
        });
        if expanded.contains(&key) {
            for (ci, item) in goal.items.iter().enumerate() {
                push_item(
                    item,
                    &format!("{key}/{ci}"),
                    &key,
                    1,
                    expanded,
                    show_blame,
                    &mut rows,
                );
            }
        }
    }
    rows
}

/// A text-based progress bar: `[=====     ]` for 50%.
fn progress_bar(pct: u32) -> String {
    const W: usize = 10;
    let filled = ((pct as f64 / 100.0) * W as f64).round() as usize;
    format!("{}{}", "=".repeat(filled), "-".repeat(W - filled))
}

/// Every foldable node key in the goal forest (goals + milestones), for
/// expand-all.
pub(super) fn all_node_keys(goals: &[Goal]) -> Vec<String> {
    let mut keys = Vec::new();
    for (gi, goal) in goals.iter().enumerate() {
        keys.push(format!("g{gi}"));
        for (ci, item) in goal.items.iter().enumerate() {
            collect_milestone_keys(item, &format!("g{gi}/{ci}"), &mut keys);
        }
    }
    keys
}

fn collect_milestone_keys(item: &GoalItem, key: &str, out: &mut Vec<String>) {
    if item.children.is_empty() {
        return;
    }
    out.push(key.to_string());
    for (ci, child) in item.children.iter().enumerate() {
        collect_milestone_keys(child, &format!("{key}/{ci}"), out);
    }
}

/// Style for a goal item: high priority is red; a checked leaf is dimmed.
/// Group nodes are never dimmed (they have no completion state of their own).
fn item_style(item: &GoalItem) -> Style {
    if item.metadata.priority.as_ref() == Some(&Priority::High) {
        return Style::default().fg(Color::Red);
    }
    if item.children.is_empty() && item.checked() == Some(true) {
        return Style::default().add_modifier(Modifier::DIM);
    }
    Style::default()
}

/// If `line` contains a markdown checkbox `- [x]`/`[ ]`/`[X]`/`[✓]`, return a
/// copy with that box flipped (`[ ]` <-> `[x]`). Operates at char level so
/// the multibyte `✓` is handled correctly.
pub(super) fn flip_checkbox(line: &str) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i + 2 < chars.len() {
        if chars[i] == '[' && chars[i + 2] == ']' {
            let c = chars[i + 1];
            if c == 'x' || c == 'X' || c == ' ' || c == '✓' {
                let new_c = if c == ' ' { 'x' } else { ' ' };
                let mut out = String::with_capacity(line.len());
                for ch in &chars[..i] {
                    out.push(*ch);
                }
                out.push('[');
                out.push(new_c);
                out.push(']');
                for ch in &chars[i + 3..] {
                    out.push(*ch);
                }
                return Some(out);
            }
        }
        i += 1;
    }
    None
}

fn push_item(
    item: &GoalItem,
    key: &str,
    parent_key: &str,
    depth: usize,
    expanded: &HashSet<String>,
    show_blame: bool,
    rows: &mut Vec<GoalRow>,
) {
    let indent = "  ".repeat(depth);
    let style = item_style(item);
    let mut badges: Vec<String> = Vec::new();
    if let Some(p) = &item.metadata.priority {
        badges.push(format!("[{}]", p.label()));
    }
    if let Some(o) = &item.metadata.owner {
        badges.push(format!("@{}", o));
    }
    for t in &item.metadata.tags {
        badges.push(format!("#{}", t));
    }
    let badge = if badges.is_empty() {
        String::new()
    } else {
        format!("  {}", badges.join(" "))
    };
    let blame_info = if show_blame {
        match &item.blame_author {
            Some(a) => {
                let date = item
                    .blame_date
                    .map(|d| format!(" {}", d.format("%Y-%m-%d")))
                    .unwrap_or_default();
                format!("  ({a}{date})")
            }
            None => String::new(),
        }
    } else {
        String::new()
    };

    // Cycle references are always rendered as a single non-foldable marker
    // leaf, regardless of node state — they have no children (the resolver
    // cleared them) and the chain is the diagnostic.
    if let Some(Reference::Cycle { chain }) = &item.reference {
        let chain_str = chain
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" → ");
        rows.push(GoalRow {
            kind: GoalRowKind::Task {
                key: key.to_string(),
                parent_key: parent_key.to_string(),
            },
            text: format!("{indent}↻ (cycle: {chain_str})"),
            style,
        });
        return;
    }

    // Table warnings (malformed/skipped tables) render as a non-foldable
    // `⚠` marker leaf, mirroring the broken-reference glyph below.
    if let Some(warning) = &item.warning {
        rows.push(GoalRow {
            kind: GoalRowKind::Task {
                key: key.to_string(),
                parent_key: parent_key.to_string(),
            },
            text: format!("{indent}⚠ {warning}"),
            style,
        });
        return;
    }

    // Reference glyph: `→` for resolved, `⚠` for broken, nothing for
    // non-references. Pending should not occur after Pass 2 resolution.
    let (ref_glyph, ref_suffix) = match &item.reference {
        Some(Reference::Resolved { .. }) => ("→ ".to_string(), String::new()),
        Some(Reference::Broken { raw_target, reason }) => {
            let reason_str = match reason {
                BrokenReason::NotFound => "not found",
                BrokenReason::NoGoalTracker => "no goal tracker",
            };
            ("⚠ ".to_string(), format!("  ({reason_str}: {raw_target})"))
        }
        _ => (String::new(), String::new()),
    };

    if item.children.is_empty() {
        // Leaf: not foldable. Render with or without checkbox depending on
        // node state.
        let body = match item.state {
            NodeState::Checkbox { checked } => {
                let check = if checked { 'x' } else { ' ' };
                format!(
                    "{indent}[{check}] {ref_glyph}{}{badge}{blame_info}{ref_suffix}",
                    item.text
                )
            }
            NodeState::Group => {
                format!("{indent}{ref_glyph}{}{badge}{ref_suffix}", item.text)
            }
        };
        rows.push(GoalRow {
            kind: GoalRowKind::Task {
                key: key.to_string(),
                parent_key: parent_key.to_string(),
            },
            text: body,
            style,
        });
    } else {
        // Internal node (checkbox milestone or group container): foldable.
        // The direct-children ratio counts only checkbox children — all-group
        // children would produce a misleading "0/0" otherwise.
        let checkbox_children: Vec<&GoalItem> =
            item.children.iter().filter(|c| c.is_checkbox()).collect();
        let ratio = if !checkbox_children.is_empty() {
            let checked = checkbox_children
                .iter()
                .filter(|c| c.checked() == Some(true))
                .count();
            format!("    {}/{}", checked, checkbox_children.len())
        } else {
            String::new()
        };
        let marker = if expanded.contains(key) { '▼' } else { '▸' };
        let check_str = match item.state {
            NodeState::Checkbox { checked } => {
                let check = if checked { 'x' } else { ' ' };
                format!("[{check}] ")
            }
            NodeState::Group => String::new(),
        };
        rows.push(GoalRow {
            kind: GoalRowKind::Milestone {
                key: key.to_string(),
            },
            text: format!(
                "{indent}{marker} {check_str}{ref_glyph}{}{badge}{ratio}{blame_info}",
                item.text
            ),
            style,
        });
        if expanded.contains(key) {
            for (ci, child) in item.children.iter().enumerate() {
                push_item(
                    child,
                    &format!("{key}/{ci}"),
                    key,
                    depth + 1,
                    expanded,
                    show_blame,
                    rows,
                );
            }
        }
    }
}

/// Render the goals view. Uses a stateful list so the viewport scrolls to
/// follow the cursor, with the selection shown via the highlight style.
pub(super) fn draw(f: &mut Frame, app: &super::App, area: Rect) {
    let items: Vec<ListItem> = app
        .goal_rows
        .iter()
        .map(|row| {
            ListItem::new(super::search::highlighted_line(
                &row.text,
                row.style,
                &app.search_query,
            ))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Goals & Milestones"),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = ListState::default();
    if app.goal_rows.is_empty() {
        state.select(None);
    } else {
        state.select(Some(app.goal_selected));
    }
    f.render_stateful_widget(list, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Metadata, NodeState, Span};
    use std::path::PathBuf;

    fn goal(title: &str, items: Vec<GoalItem>) -> Goal {
        Goal {
            title: title.into(),
            source_file: PathBuf::from("x.md"),
            badge: "(root)".into(),
            items,
        }
    }

    fn leaf(text: &str, checked: bool) -> GoalItem {
        GoalItem {
            text: text.into(),
            state: NodeState::Checkbox { checked },
            metadata: Metadata::default(),
            reference: None,
            warning: None,
            children: Vec::new(),
            span: Span {
                path: PathBuf::from("x.md"),
                line: 1,
            },
            blame_author: None,
            blame_date: None,
            blame_commit: None,
        }
    }

    fn milestone(text: &str, checked: bool, children: Vec<GoalItem>) -> GoalItem {
        GoalItem {
            text: text.into(),
            state: NodeState::Checkbox { checked },
            metadata: Metadata::default(),
            reference: None,
            warning: None,
            children,
            span: Span {
                path: PathBuf::from("x.md"),
                line: 1,
            },
            blame_author: None,
            blame_date: None,
            blame_commit: None,
        }
    }

    fn group(text: &str, children: Vec<GoalItem>) -> GoalItem {
        GoalItem {
            text: text.into(),
            state: NodeState::Group,
            metadata: Metadata::default(),
            reference: None,
            warning: None,
            children,
            span: Span {
                path: PathBuf::from("x.md"),
                line: 1,
            },
            blame_author: None,
            blame_date: None,
            blame_commit: None,
        }
    }

    fn with_reference(mut item: GoalItem, reference: Reference) -> GoalItem {
        item.reference = Some(reference);
        item
    }

    #[test]
    fn collapsed_shows_one_header_per_goal() {
        let goals = vec![
            goal("A", vec![leaf("a1", false)]),
            goal("B", vec![leaf("b1", false)]),
        ];
        let rows = flatten_goals(&goals, &HashSet::new(), false);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].text.starts_with("▸ A"));
        assert!(rows[1].text.starts_with("▸ B"));
    }

    #[test]
    fn expanding_a_goal_shows_top_level_milestones_folded() {
        let goals = vec![goal(
            "A",
            vec![milestone(
                "week 1",
                false,
                vec![leaf("task 1", true), leaf("task 2", false)],
            )],
        )];
        let mut expanded = HashSet::new();
        expanded.insert("g0".to_string());
        let rows = flatten_goals(&goals, &expanded, false);
        // header + milestone only (milestone's children stay folded)
        assert_eq!(rows.len(), 2);
        assert!(rows[1].text.contains("▸ [ ] week 1"));
    }

    #[test]
    fn expanding_a_milestone_reveals_its_tasks() {
        let goals = vec![goal(
            "A",
            vec![milestone(
                "week 1",
                false,
                vec![leaf("task 1", true), leaf("task 2", false)],
            )],
        )];
        let mut expanded = HashSet::new();
        expanded.insert("g0".to_string());
        expanded.insert("g0/0".to_string());
        let rows = flatten_goals(&goals, &expanded, false);
        assert_eq!(rows.len(), 4); // header + milestone + 2 tasks
        assert!(rows[1].text.starts_with("  ▼ [ ] week 1"));
        assert!(rows[2].text.contains("[x] task 1"));
        assert!(rows[3].text.contains("[ ] task 2"));
        // indentation increases with depth
        assert!(rows[3].text.starts_with("    "));
        // a leaf carries its parent milestone's key, so fold acts on the parent
        assert!(matches!(
            &rows[2].kind,
            GoalRowKind::Task { parent_key, .. } if parent_key == "g0/0"
        ));
    }

    #[test]
    fn milestone_keys_are_nested_paths() {
        let goals = vec![goal(
            "A",
            vec![milestone(
                "week 1",
                false,
                vec![milestone("sub", false, vec![leaf("deep", false)])],
            )],
        )];
        let mut expanded = HashSet::new();
        expanded.insert("g0".to_string());
        expanded.insert("g0/0".to_string());
        let rows = flatten_goals(&goals, &expanded, false);
        // header + week1 + sub (folded); "deep" stays hidden under "sub"
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[2].kind, GoalRowKind::Milestone { .. }));
        assert!(rows[2].text.contains("▸ [ ] sub"));
    }

    #[test]
    fn header_text_includes_progress_and_badge() {
        let goals = vec![goal("A", vec![leaf("done", true)])];
        let rows = flatten_goals(&goals, &HashSet::new(), false);
        assert!(rows[0].text.contains("100%"));
        assert!(rows[0].text.contains("(root)"));
    }

    #[test]
    fn flip_checkbox_toggles_state_char() {
        assert_eq!(
            flip_checkbox("- [x] Week 1").as_deref(),
            Some("- [ ] Week 1")
        );
        assert_eq!(
            flip_checkbox("  - [ ] task").as_deref(),
            Some("  - [x] task")
        );
        // uppercase X and the multibyte check mark both count as checked
        assert_eq!(flip_checkbox("- [X] a").as_deref(), Some("- [ ] a"));
        assert_eq!(flip_checkbox("- [✓] a").as_deref(), Some("- [ ] a"));
        // no checkbox -> None
        assert!(flip_checkbox("just text").is_none());
        assert!(flip_checkbox("- not a checkbox").is_none());
    }

    #[test]
    fn header_progress_bars_are_aligned() {
        let goals = vec![
            goal("Short", vec![leaf("a", true)]),
            goal(
                "A Very Long Goal Title That Spans Many Characters",
                vec![leaf("b", false)],
            ),
        ];
        let rows = flatten_goals(&goals, &HashSet::new(), false);

        let bar_starts: Vec<usize> = rows
            .iter()
            .filter(|r| matches!(r.kind, GoalRowKind::Header { .. }))
            .map(|r| r.text.rfind('[').unwrap())
            .collect();

        assert_eq!(bar_starts.len(), 2);
        assert_eq!(
            bar_starts[0], bar_starts[1],
            "progress bars should align at the same column: {:?}",
            bar_starts
        );
    }

    #[test]
    fn progress_bar_format() {
        assert_eq!(progress_bar(0), "----------");
        assert_eq!(progress_bar(50), "=====-----");
        assert_eq!(progress_bar(100), "==========");
    }

    #[test]
    fn group_node_renders_without_checkbox() {
        // A subsection-style group node has no [ ] / [x]; the title appears
        // next to the fold chevron directly.
        let goals = vec![goal(
            "A",
            vec![group(
                "Foundations",
                vec![leaf("task 1", true), leaf("task 2", false)],
            )],
        )];
        let mut expanded = HashSet::new();
        expanded.insert("g0".to_string());
        let rows = flatten_goals(&goals, &expanded, false);
        // header + group node only (children folded)
        assert_eq!(rows.len(), 2);
        assert!(
            rows[1].text.contains("▸ Foundations"),
            "group node renders without [ ]: {}",
            rows[1].text
        );
        assert!(
            !rows[1].text.contains("[]"),
            "no empty checkbox bracket should appear: {}",
            rows[1].text
        );
    }

    #[test]
    fn group_node_with_resolved_reference_shows_arrow_glyph() {
        let item = with_reference(
            group("Imported Goal", vec![leaf("a", true)]),
            Reference::Resolved {
                target_path: PathBuf::from("other.md"),
                display_text: String::new(),
            },
        );
        let goals = vec![goal("A", vec![item])];
        let mut expanded = HashSet::new();
        expanded.insert("g0".to_string());
        let rows = flatten_goals(&goals, &expanded, false);
        assert!(
            rows[1].text.contains("→"),
            "resolved reference shows → glyph: {}",
            rows[1].text
        );
    }

    #[test]
    fn broken_reference_renders_warning_glyph_and_reason() {
        let item = with_reference(
            group("[[missing]]", Vec::new()),
            Reference::Broken {
                raw_target: "missing".into(),
                reason: BrokenReason::NotFound,
            },
        );
        let goals = vec![goal("A", vec![item])];
        let mut expanded = HashSet::new();
        expanded.insert("g0".to_string());
        let rows = flatten_goals(&goals, &expanded, false);
        // Broken ref is a leaf — header + leaf.
        assert_eq!(rows.len(), 2);
        assert!(
            rows[1].text.contains("⚠"),
            "broken ref shows ⚠: {}",
            rows[1].text
        );
        assert!(
            rows[1].text.contains("not found"),
            "broken ref shows reason: {}",
            rows[1].text
        );
    }

    #[test]
    fn cycle_reference_renders_marker_with_chain() {
        let item = with_reference(
            group("cycled", Vec::new()),
            Reference::Cycle {
                chain: vec![PathBuf::from("a.md"), PathBuf::from("b.md")],
            },
        );
        let goals = vec![goal("A", vec![item])];
        let mut expanded = HashSet::new();
        expanded.insert("g0".to_string());
        let rows = flatten_goals(&goals, &expanded, false);
        assert_eq!(rows.len(), 2);
        assert!(
            rows[1].text.contains("↻"),
            "cycle shows ↻ glyph: {}",
            rows[1].text
        );
        assert!(
            rows[1].text.contains("a.md"),
            "cycle chain visible: {}",
            rows[1].text
        );
    }

    #[test]
    fn ratio_counts_only_checkbox_children() {
        // A group containing 1 checked leaf + 1 nested empty group must show
        // ratio 1/1, not 1/2 — the empty group is a placeholder, not a task.
        let goals = vec![goal(
            "A",
            vec![group(
                "Subtree",
                vec![leaf("done", true), group("Empty", Vec::new())],
            )],
        )];
        let mut expanded = HashSet::new();
        expanded.insert("g0".to_string());
        let rows = flatten_goals(&goals, &expanded, false);
        assert!(
            rows[1].text.contains("1/1"),
            "ratio counts checkbox children only: {}",
            rows[1].text
        );
    }

    #[test]
    fn group_leaf_renders_as_non_foldable_task() {
        // An empty subsection (group with no children) is not foldable.
        let goals = vec![goal("A", vec![group("Empty Placeholder", Vec::new())])];
        let mut expanded = HashSet::new();
        expanded.insert("g0".to_string());
        let rows = flatten_goals(&goals, &expanded, false);
        assert_eq!(rows.len(), 2);
        // The group leaf should NOT carry a chevron (▸/▼).
        assert!(
            !rows[1].text.contains('▸') && !rows[1].text.contains('▼'),
            "group leaf is not foldable: {}",
            rows[1].text
        );
        assert!(
            rows[1].text.contains("Empty Placeholder"),
            "group leaf shows its title: {}",
            rows[1].text
        );
        // Carries the parent_key (goal header), like a Task.
        assert!(matches!(
            &rows[1].kind,
            GoalRowKind::Task { parent_key, .. } if parent_key == "g0"
        ));
    }
}
