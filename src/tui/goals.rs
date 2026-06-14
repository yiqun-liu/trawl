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
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::model::{Goal, GoalItem, Priority, Status};

/// Which goal a row belongs to. Headers and milestones are foldable; tasks
/// (leaves) are not.
pub(super) enum GoalRowKind {
    Header {
        key: String,
    },
    Milestone {
        key: String,
    },
    /// A leaf task. `parent_key` is the foldable node it belongs to (a
    /// milestone or the goal header), so expand/collapse act on the parent.
    Task {
        parent_key: String,
    },
}

pub(super) struct GoalRow {
    pub(super) kind: GoalRowKind,
    pub(super) text: String,
    pub(super) style: Style,
}

/// Flatten goals into display rows according to the expand set.
pub(super) fn flatten_goals(goals: &[Goal], expanded: &HashSet<String>) -> Vec<GoalRow> {
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
        rows.push(GoalRow {
            kind: GoalRowKind::Header { key: key.clone() },
            text: format!("{marker} {}  {}  {}%", goal.title, goal.badge, pct),
            style: if completed {
                Style::default().add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)
            } else {
                Style::default()
            },
        });
        if expanded.contains(&key) {
            for (ci, item) in goal.items.iter().enumerate() {
                push_item(item, &format!("{key}/{ci}"), &key, 1, expanded, &mut rows);
            }
        }
    }
    rows
}

/// Style for a goal item: high priority is red; a checked leaf is dimmed.
fn item_style(item: &GoalItem) -> Style {
    if item.metadata.priority.as_ref() == Some(&Priority::High) {
        return Style::default().fg(Color::Red);
    }
    if item.children.is_empty() && item.checked {
        return Style::default().add_modifier(Modifier::DIM);
    }
    Style::default()
}

fn push_item(
    item: &GoalItem,
    key: &str,
    parent_key: &str,
    depth: usize,
    expanded: &HashSet<String>,
    rows: &mut Vec<GoalRow>,
) {
    let indent = "  ".repeat(depth);
    let check = if item.checked { 'x' } else { ' ' };
    let style = item_style(item);

    if item.children.is_empty() {
        // Leaf task: not foldable; remembers its parent so keys act on it.
        rows.push(GoalRow {
            kind: GoalRowKind::Task {
                parent_key: parent_key.to_string(),
            },
            text: format!("{indent}[{check}] {}", item.text),
            style,
        });
    } else {
        // Milestone: foldable.
        let marker = if expanded.contains(key) { '▼' } else { '▸' };
        rows.push(GoalRow {
            kind: GoalRowKind::Milestone {
                key: key.to_string(),
            },
            text: format!("{indent}{marker} [{check}] {}", item.text),
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
        .map(|row| ListItem::new(Line::from(row.text.clone()).style(row.style)))
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
    use crate::model::{Metadata, Span};
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
            checked,
            metadata: Metadata::default(),
            children: Vec::new(),
            span: Span {
                path: PathBuf::from("x.md"),
                line: 1,
            },
        }
    }

    fn milestone(text: &str, checked: bool, children: Vec<GoalItem>) -> GoalItem {
        GoalItem {
            text: text.into(),
            checked,
            metadata: Metadata::default(),
            children,
            span: Span {
                path: PathBuf::from("x.md"),
                line: 1,
            },
        }
    }

    #[test]
    fn collapsed_shows_one_header_per_goal() {
        let goals = vec![
            goal("A", vec![leaf("a1", false)]),
            goal("B", vec![leaf("b1", false)]),
        ];
        let rows = flatten_goals(&goals, &HashSet::new());
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
        let rows = flatten_goals(&goals, &expanded);
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
        let rows = flatten_goals(&goals, &expanded);
        assert_eq!(rows.len(), 4); // header + milestone + 2 tasks
        assert!(rows[1].text.starts_with("  ▼ [ ] week 1"));
        assert!(rows[2].text.contains("[x] task 1"));
        assert!(rows[3].text.contains("[ ] task 2"));
        // indentation increases with depth
        assert!(rows[3].text.starts_with("    "));
        // a leaf carries its parent milestone's key, so fold acts on the parent
        assert!(matches!(
            &rows[2].kind,
            GoalRowKind::Task { parent_key } if parent_key == "g0/0"
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
        let rows = flatten_goals(&goals, &expanded);
        // header + week1 + sub (folded); "deep" stays hidden under "sub"
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[2].kind, GoalRowKind::Milestone { .. }));
        assert!(rows[2].text.contains("▸ [ ] sub"));
    }

    #[test]
    fn header_text_includes_progress_and_badge() {
        let goals = vec![goal("A", vec![leaf("done", true)])];
        let rows = flatten_goals(&goals, &HashSet::new());
        assert!(rows[0].text.contains("100%"));
        assert!(rows[0].text.contains("(root)"));
    }
}
