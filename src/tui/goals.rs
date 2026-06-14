//! Goals view: flatten the goal forest into display rows and render it.
//!
//! The flattening is a pure function over [`Goal`](crate::model::Goal) data so
//! it can be unit-tested without a terminal. Expand state is a set of goal
//! indices; expanding re-runs the flatten.

use std::collections::HashSet;

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

use crate::model::{Goal, GoalItem};

/// Which goal a row belongs to (headers carry the goal index; items don't).
pub(super) enum GoalRowKind {
    Header(usize),
    Item,
}

pub(super) struct GoalRow {
    pub(super) kind: GoalRowKind,
    pub(super) text: String,
}

/// Flatten goals into display rows. Expanded goals expose their item tree.
pub(super) fn flatten_goals(goals: &[Goal], expanded: &HashSet<usize>) -> Vec<GoalRow> {
    let mut rows = Vec::new();
    for (gi, goal) in goals.iter().enumerate() {
        let marker = if expanded.contains(&gi) { '▼' } else { '▸' };
        let pct = (goal.progress() * 100.0).round() as u32;
        rows.push(GoalRow {
            kind: GoalRowKind::Header(gi),
            text: format!("{marker} {}  {}  {}%", goal.title, goal.badge, pct),
        });
        if expanded.contains(&gi) {
            for item in &goal.items {
                push_item(item, 1, &mut rows);
            }
        }
    }
    rows
}

fn push_item(item: &GoalItem, depth: usize, rows: &mut Vec<GoalRow>) {
    let check = if item.checked { 'x' } else { ' ' };
    let indent = "  ".repeat(depth);
    rows.push(GoalRow {
        kind: GoalRowKind::Item,
        text: format!("{indent}[{check}] {}", item.text),
    });
    for child in &item.children {
        push_item(child, depth + 1, rows);
    }
}

/// Render the goals view.
pub(super) fn draw(f: &mut Frame, app: &super::App, area: Rect) {
    let selected = app.goal_selected;
    let items: Vec<ListItem> = app
        .goal_rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let line = Line::from(row.text.clone());
            if i == selected {
                ListItem::new(line.style(Style::default().add_modifier(Modifier::REVERSED)))
            } else {
                ListItem::new(line)
            }
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Goals & Milestones"),
    );
    f.render_widget(list, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Metadata, Span, Status};
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
    fn expanded_exposes_nested_items() {
        let goals = vec![goal(
            "A",
            vec![milestone(
                "week 1",
                false,
                vec![leaf("task 1", true), leaf("task 2", false)],
            )],
        )];
        let mut expanded = HashSet::new();
        expanded.insert(0);
        let rows = flatten_goals(&goals, &expanded);
        assert_eq!(rows.len(), 4); // header + milestone + 2 tasks
        assert!(rows[0].text.starts_with("▼ A"));
        assert!(rows[1].text.contains("[ ] week 1"));
        assert!(rows[2].text.contains("[x] task 1"));
        assert!(rows[3].text.contains("[ ] task 2"));
        // indentation increases with depth
        assert!(rows[3].text.starts_with("    "));
    }

    #[test]
    fn header_text_includes_progress_and_badge() {
        let goals = vec![goal("A", vec![leaf("done", true)])];
        let rows = flatten_goals(&goals, &HashSet::new());
        assert!(rows[0].text.contains("100%"));
        assert!(rows[0].text.contains("(root)"));
        let _ = Status::Active; // keep the import meaningful
    }
}
