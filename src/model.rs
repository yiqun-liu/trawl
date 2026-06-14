//! Domain model shared by the scanner and both parsers.
//!
//! These types are the contract between every stage of the pipeline:
//! the scanner produces [`InlineTask`] and [`Goal`] values, and every
//! future view consumes them. See `docs/design/architecture.md` for the
//! rationale behind each shape.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::NaiveDate;

/// A priority level parsed from a `!` token.
///
/// `Option<Priority>::None` (no `!` token at all) means "untagged" and is
/// represented at the [`Metadata`] level. [`Priority::Other`] preserves
/// unrecognized values verbatim, per the spec's "stored as-is" rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Priority {
    High,
    Med,
    Low,
    Other(String),
}

impl Priority {
    /// Parse a raw priority value, case-insensitively. Recognized forms are
    /// `high`, `med`/`medium`, and `low`; anything else becomes [`Priority::Other`].
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "high" => Priority::High,
            "med" | "medium" => Priority::Med,
            "low" => Priority::Low,
            _ => Priority::Other(raw.trim().to_string()),
        }
    }

    /// Short display label for badges (`high`/`med`/`low`/the custom value).
    pub fn label(&self) -> &str {
        match self {
            Priority::High => "high",
            Priority::Med => "med",
            Priority::Low => "low",
            Priority::Other(s) => s.as_str(),
        }
    }

    /// Sort rank: higher means more urgent. [`Priority::Other`] sorts below
    /// the defined levels so unknown custom values never outrank the triage set.
    pub fn rank(&self) -> u8 {
        match self {
            Priority::High => 3,
            Priority::Med => 2,
            Priority::Low => 1,
            Priority::Other(_) => 0,
        }
    }
}

/// Inline metadata extracted by prefix scan. Shared by inline tasks and goal
/// items. The four built-in fields are typed; any other configured token type
/// lands in [`Metadata::custom`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata {
    /// `@owner` — last value wins.
    pub owner: Option<String>,
    /// `#tag` — accumulates all occurrences.
    pub tags: Vec<String>,
    /// `!priority` — last value wins.
    pub priority: Option<Priority>,
    /// `~due` — last value wins.
    pub due: Option<NaiveDate>,
    /// Any other configured token type (e.g. `effort = "%"`).
    pub custom: HashMap<String, Vec<String>>,
}

/// A 1-based source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub path: PathBuf,
    pub line: usize,
}

/// A `TODO`/`FIXME`/... marker discovered in a file.
#[derive(Debug, Clone)]
pub struct InlineTask {
    pub keyword: String,
    pub scope: Option<String>,
    pub description: String,
    pub metadata: Metadata,
    pub span: Span,
}

/// One checkbox node or one table row in a goal tracker.
///
/// A node with no children is a *task*; a node with children is a
/// *milestone*. The distinction is structural, not lexical.
#[derive(Debug, Clone)]
pub struct GoalItem {
    pub text: String,
    pub checked: bool,
    pub metadata: Metadata,
    pub children: Vec<GoalItem>,
    pub span: Span,
}

impl GoalItem {
    /// A milestone is any item that has child items.
    pub fn is_milestone(&self) -> bool {
        !self.children.is_empty()
    }

    /// A task is any item with no children.
    pub fn is_task(&self) -> bool {
        self.children.is_empty()
    }
}

/// One parsed `## GOAL TRACKER` section.
#[derive(Debug, Clone)]
pub struct Goal {
    pub title: String,
    pub source_file: PathBuf,
    pub badge: String,
    pub items: Vec<GoalItem>,
}

impl Goal {
    /// Leaf-ratio progress in `[0.0, 1.0]`, where a leaf is any [`GoalItem`]
    /// with no children. A goal with **zero leaf tasks** returns `0.0`
    /// (no division is performed).
    pub fn progress(&self) -> f64 {
        let (total, done) = leaf_counts(&self.items);
        if total == 0 {
            0.0
        } else {
            done as f64 / total as f64
        }
    }

    /// Derived status from [`Goal::progress`].
    pub fn status(&self) -> Status {
        Status::from_progress(self.progress())
    }
}

/// Count `(total_leaf, done_leaf)` across a forest of items.
fn leaf_counts(items: &[GoalItem]) -> (usize, usize) {
    let mut total = 0usize;
    let mut done = 0usize;
    for item in items {
        count_leaves(item, &mut total, &mut done);
    }
    (total, done)
}

fn count_leaves(item: &GoalItem, total: &mut usize, done: &mut usize) {
    if item.children.is_empty() {
        *total += 1;
        if item.checked {
            *done += 1;
        }
    } else {
        for child in &item.children {
            count_leaves(child, total, done);
        }
    }
}

/// Derived goal status from progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Planned,
    Active,
    Completed,
}

impl Status {
    /// `1.0` → [`Status::Completed`], `0.0` → [`Status::Planned`] (this includes
    /// goals with zero leaf tasks), anything in between → [`Status::Active`].
    pub fn from_progress(p: f64) -> Self {
        if p >= 1.0 {
            Status::Completed
        } else if p <= 0.0 {
            Status::Planned
        } else {
            Status::Active
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(text: &str, checked: bool) -> GoalItem {
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
    fn priority_parses_known_levels_case_insensitively() {
        assert_eq!(Priority::parse("High"), Priority::High);
        assert_eq!(Priority::parse("MED"), Priority::Med);
        assert_eq!(Priority::parse("medium"), Priority::Med);
        assert_eq!(Priority::parse("low"), Priority::Low);
    }

    #[test]
    fn priority_preserves_unknown_values() {
        assert_eq!(
            Priority::parse("critical"),
            Priority::Other("critical".into())
        );
        assert_eq!(Priority::Other("x".into()).rank(), 0);
        assert!(Priority::High.rank() > Priority::Other("x".into()).rank());
    }

    #[test]
    fn progress_is_leaf_ratio() {
        let goal = Goal {
            title: "T".into(),
            source_file: PathBuf::from("x.md"),
            badge: "(root)".into(),
            items: vec![milestone(
                "week 1",
                true,
                vec![task("a", true), task("b", false), task("c", true)],
            )],
        };
        // milestone checkbox (checked=true) is NOT a leaf and does not count
        assert_eq!(goal.progress(), 2.0 / 3.0);
        assert_eq!(goal.status(), Status::Active);
    }

    #[test]
    fn progress_zero_leaf_is_planned() {
        let goal = Goal {
            title: "T".into(),
            source_file: PathBuf::from("x.md"),
            badge: "(root)".into(),
            items: vec![milestone("week 1", false, vec![])],
        };
        assert_eq!(goal.progress(), 0.0);
        assert_eq!(goal.status(), Status::Planned);
    }

    #[test]
    fn progress_all_done_is_completed() {
        let goal = Goal {
            title: "T".into(),
            source_file: PathBuf::from("x.md"),
            badge: "(root)".into(),
            items: vec![task("a", true), task("b", true)],
        };
        assert_eq!(goal.progress(), 1.0);
        assert_eq!(goal.status(), Status::Completed);
    }

    #[test]
    fn status_thresholds() {
        assert_eq!(Status::from_progress(1.0), Status::Completed);
        assert_eq!(Status::from_progress(0.0), Status::Planned);
        assert_eq!(Status::from_progress(0.5), Status::Active);
        assert_eq!(Status::from_progress(-0.0), Status::Planned);
    }

    #[test]
    fn milestone_vs_task_classification() {
        assert!(milestone("m", true, vec![task("c", true)]).is_milestone());
        assert!(!task("t", true).is_milestone());
        assert!(task("t", true).is_task());
    }
}
