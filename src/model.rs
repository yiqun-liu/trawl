//! Domain model shared by the scanner and both parsers.
//!
//! These types are the contract between every stage of the pipeline:
//! the scanner produces [`InlineTask`] and [`Goal`] values, and every
//! future view consumes them. See `docs/design/architecture.md` for the
//! rationale behind each shape.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{NaiveDate, NaiveDateTime};

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
    /// Populated by blame enrichment (git2) when `display.show_git_blame`
    /// is true. `None` when blame is disabled or data is unavailable.
    pub blame_author: Option<String>,
    pub blame_date: Option<NaiveDateTime>,
    pub blame_commit: Option<String>,
}

impl InlineTask {
    /// Whether this task is stale (older than `threshold_days`).
    /// Tasks without blame data are never stale.
    pub fn is_stale(&self, threshold_days: u32) -> bool {
        let Some(date) = self.blame_date else {
            return false;
        };
        let now = chrono::Utc::now().naive_utc();
        let age = now - date;
        age.num_days() > threshold_days as i64
    }
}

/// The structural kind of a [`GoalItem`]. Determines checkbox rendering and
/// whether the node participates in leaf-ratio progress.
///
/// - [`NodeState::Checkbox`] covers both leaf tasks (`- [ ] task`) and
///   checkbox milestones (`- [x] Week 1` with children). The `checked` value
///   is user-controlled and independent of children, preserving the
///   "milestone checkbox independence" rule.
/// - [`NodeState::Group`] covers named containers that have no checkbox:
///   subsection headings (`### Title`), plain bullets with children
///   (`- Group` followed by indented items), and reference roots. A group
///   node has no `[ ]` / `[x]` state of its own and never counts toward
///   leaf-ratio progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeState {
    /// Named container without a checkbox.
    Group,
    /// Checkbox-bearing node. `checked` is independent of children.
    Checkbox { checked: bool },
}

/// Cross-document reference attached to a [`GoalItem`]. Set by Pass 1 of the
/// goal parser as [`Reference::Pending`]; converted to one of the resolved
/// variants by the Pass 2 resolver (`parser::resolve`).
#[derive(Debug, Clone)]
pub enum Reference {
    /// Pass 1 form. The resolver rewrites this into one of the other
    /// variants. `raw_target` is the path text as written in the source;
    /// `display_text` is the link text for `[display](target)` markdown
    /// links, or empty for `[[target]]` wikilinks.
    Pending {
        raw_target: String,
        display_text: String,
    },
    /// Resolver success. The referenced doc's items are attached as children
    /// of the carrying [`GoalItem`].
    Resolved {
        target_path: PathBuf,
        display_text: String,
    },
    /// Target not found / not scanned / has no goal tracker.
    Broken {
        raw_target: String,
        reason: BrokenReason,
    },
    /// Cycle detected while expanding. `chain` is the active path stack at
    /// the point of detection, for diagnostics.
    Cycle { chain: Vec<PathBuf> },
}

/// Why a [`Reference::Broken`] reference could not be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokenReason {
    /// File does not exist on disk.
    NotFound,
    /// File exists but was not scanned (excluded, binary, etc.).
    NotScanned,
    /// File was scanned but has no goal tracker section.
    NoGoalTracker,
}

/// One node in a goal tracker tree.
///
/// A node with no children is a *task* (if [`NodeState::Checkbox`]) or a
/// *planned placeholder* (if [`NodeState::Group`]); a node with children is
/// a *milestone* or nested container. The distinction is structural, not
/// lexical.
#[derive(Debug, Clone)]
pub struct GoalItem {
    pub text: String,
    pub state: NodeState,
    pub metadata: Metadata,
    /// Cross-document reference, if this node originated from a `[[...]]`
    /// wikilink or `[text](path)` markdown link. `None` for normal nodes.
    pub reference: Option<Reference>,
    pub children: Vec<GoalItem>,
    pub span: Span,
    pub blame_author: Option<String>,
    pub blame_date: Option<NaiveDateTime>,
    pub blame_commit: Option<String>,
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

    /// `Some(true)` if this node is a checked checkbox, `Some(false)` if an
    /// unchecked checkbox, `None` if it is a group node (no checkbox).
    pub fn checked(&self) -> Option<bool> {
        match self.state {
            NodeState::Checkbox { checked } => Some(checked),
            NodeState::Group => None,
        }
    }

    /// True if this node has a `[ ]` / `[x]` checkbox.
    pub fn is_checkbox(&self) -> bool {
        matches!(self.state, NodeState::Checkbox { .. })
    }

    /// True if this node is a named container without a checkbox.
    pub fn is_group(&self) -> bool {
        matches!(self.state, NodeState::Group)
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

/// Count `(total_leaf, done_leaf)` across a forest of items. Only checkbox
/// leaves participate — group leaves (empty subsection, broken reference,
/// cycle marker) are planned placeholders and do not affect progress.
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
        // Leaf: count only if it carries a checkbox.
        if let NodeState::Checkbox { checked } = item.state {
            *total += 1;
            if checked {
                *done += 1;
            }
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
            state: NodeState::Checkbox { checked },
            metadata: Metadata::default(),
            reference: None,
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
