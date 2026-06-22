//! Cross-document reference resolution (Pass 2).
//!
//! After every file's goal tracker has been parsed independently (Pass 1),
//! this module walks each [`Goal`](crate::model::Goal)'s tree and resolves
//! [`Reference::Pending`](crate::model::Reference) nodes into one of the
//! resolved variants:
//!
//! - [`Reference::Resolved`] — the target was found; its items are deep-cloned
//!   as the children of the referencing node.
//! - [`Reference::Broken`] — the target does not exist or has no tracker.
//! - [`Reference::Cycle`] — the target is already on the active expansion
//!   chain (A→B→A); a marker is left and expansion stops.
//!
//! Paths in references resolve relative to the **referencing doc's**
//! directory, matching how `[text](path)` markdown links work in renderers.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use crate::model::{BrokenReason, Goal, GoalItem, Reference};

/// Resolve all `Reference::Pending` nodes in `goals`. Called once after the
/// per-file Pass 1 parse has produced the initial `Vec<Goal>`.
///
/// `scanned_files` is the set of scan-root-relative paths that were read by
/// the scanner, regardless of whether they contained a goal tracker. It is
/// used to distinguish `NoGoalTracker` (file was scanned but has no tracker)
/// from `NotFound` (file is not in the scan set at all).
pub fn resolve_references(goals: &mut [Goal], scanned_files: &HashSet<PathBuf>) {
    // Snapshot each goal's title and items into an owned map keyed by
    // normalized source_file. The resolver needs to read items from goals
    // *other than* the one it's currently mutating — a snapshot avoids the
    // borrow conflict and gives each diamond-reference expansion its own
    // independent clone source.
    let mut title_and_items: HashMap<PathBuf, (String, Vec<GoalItem>)> = HashMap::new();
    for g in goals.iter() {
        title_and_items.insert(
            normalize_key(&g.source_file),
            (g.title.clone(), g.items.clone()),
        );
    }
    let scanned_keys: HashSet<PathBuf> = scanned_files.iter().map(|p| normalize_key(p)).collect();

    for goal in goals.iter_mut() {
        // Each top-level goal starts its own expansion chain. The chain
        // carries the active path stack so cycles are detected per-root.
        let start_chain = vec![normalize_key(&goal.source_file)];
        resolve_items(
            &mut goal.items,
            &start_chain,
            &title_and_items,
            &scanned_keys,
        );
    }
}

fn resolve_items(
    items: &mut [GoalItem],
    chain: &[PathBuf],
    sources: &HashMap<PathBuf, (String, Vec<GoalItem>)>,
    scanned_keys: &HashSet<PathBuf>,
) {
    for item in items.iter_mut() {
        resolve_item(item, chain, sources, scanned_keys);
    }
}

fn resolve_item(
    item: &mut GoalItem,
    chain: &[PathBuf],
    sources: &HashMap<PathBuf, (String, Vec<GoalItem>)>,
    scanned_keys: &HashSet<PathBuf>,
) {
    // Take the reference (if any) by value so we can replace it with the
    // resolved form. Non-Pending variants (already resolved by an outer
    // call) are put back unchanged.
    let Some(reference) = item.reference.take() else {
        // No reference: recurse into existing children with the same chain.
        resolve_items(&mut item.children, chain, sources, scanned_keys);
        return;
    };
    let Reference::Pending {
        raw_target,
        display_text,
    } = reference
    else {
        item.reference = Some(reference);
        resolve_items(&mut item.children, chain, sources, scanned_keys);
        return;
    };

    let target_key = normalize_target(&item.span.path, &raw_target);

    if chain.iter().any(|p| p == &target_key) {
        // Cycle: this target is already on the active expansion chain.
        item.reference = Some(Reference::Cycle {
            chain: chain.to_vec(),
        });
        item.children.clear();
        return;
    }

    if let Some((title, items)) = sources.get(&target_key).cloned() {
        // Resolved. Clone the target's items as this node's children.
        item.children = items;
        // For wikilinks (empty display_text), fill the text from the target's
        // title so the tree shows something meaningful.
        if item.text.is_empty() {
            item.text = title;
        }
        item.reference = Some(Reference::Resolved {
            target_path: target_key.clone(),
            display_text,
        });
        // Recurse into the freshly-attached children with the updated chain.
        let mut new_chain = chain.to_vec();
        new_chain.push(target_key);
        resolve_items(&mut item.children, &new_chain, sources, scanned_keys);
        return;
    }

    // Broken. Distinguish NoGoalTracker from NotFound using scanned_keys.
    let reason = if scanned_keys.contains(&target_key) {
        BrokenReason::NoGoalTracker
    } else {
        BrokenReason::NotFound
    };
    item.reference = Some(Reference::Broken {
        raw_target: raw_target.clone(),
        reason,
    });
    // For wikilinks with empty text, surface the raw target so the TUI has
    // something to render besides an empty string.
    if item.text.is_empty() {
        item.text = format!("[[{raw_target}]]");
    }
    item.children.clear();
}

/// Normalize a path for cross-platform comparison: forward slashes, no
/// leading `./`, `..` components collapsed lexically.
fn normalize_key(path: &Path) -> PathBuf {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut normalized = PathBuf::new();
    for component in Path::new(&s).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir => {
                normalized.push("/");
            }
            Component::Normal(s) => {
                normalized.push(s);
            }
            Component::Prefix(_) => {}
        }
    }
    normalized
}

/// Resolve a reference's raw target text against the referencing doc's
/// directory. Strips a trailing `#anchor` (anchor-level resolution is future
/// work). Appends `.md` if the target has no extension.
fn normalize_target(referencing_path: &Path, raw_target: &str) -> PathBuf {
    // Strip an optional `#anchor` (sub-section targeting is future work; for
    // now the whole target doc is inlined).
    let raw = raw_target
        .split_once('#')
        .map(|(p, _)| p)
        .unwrap_or(raw_target);
    let trimmed = raw.trim();

    // Build a path with .md extension if none was given.
    let mut target = PathBuf::from(trimmed);
    if target.extension().is_none() {
        target = target.with_extension("md");
    }

    // Join against the referencing doc's directory.
    let parent = referencing_path.parent().unwrap_or(Path::new(""));
    let joined = parent.join(&target);

    normalize_key(&joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Metadata, NodeState, Span};

    fn checkbox_leaf(text: &str, checked: bool, ref_path: &str, line: usize) -> GoalItem {
        GoalItem {
            text: text.into(),
            state: NodeState::Checkbox { checked },
            metadata: Metadata::default(),
            reference: None,
            warning: None,
            children: Vec::new(),
            span: Span {
                path: PathBuf::from(ref_path),
                line,
            },
            blame_author: None,
            blame_date: None,
            blame_commit: None,
        }
    }

    fn pending_ref(raw_target: &str, display_text: &str, ref_path: &str, line: usize) -> GoalItem {
        GoalItem {
            text: display_text.into(),
            state: NodeState::Group,
            metadata: Metadata::default(),
            reference: Some(Reference::Pending {
                raw_target: raw_target.into(),
                display_text: display_text.into(),
            }),
            warning: None,
            children: Vec::new(),
            span: Span {
                path: PathBuf::from(ref_path),
                line,
            },
            blame_author: None,
            blame_date: None,
            blame_commit: None,
        }
    }

    fn goal(title: &str, source: &str, items: Vec<GoalItem>) -> Goal {
        Goal {
            title: title.into(),
            source_file: PathBuf::from(source),
            badge: "(root)".into(),
            items,
        }
    }

    fn empty_scanned() -> HashSet<PathBuf> {
        HashSet::new()
    }

    #[test]
    fn resolves_simple_chain() {
        let target = goal(
            "Target Title",
            "target.md",
            vec![checkbox_leaf("task", true, "target.md", 1)],
        );
        let source = goal(
            "Source",
            "source.md",
            vec![pending_ref("target", "", "source.md", 1)],
        );
        let mut goals = vec![source, target];
        resolve_references(&mut goals, &empty_scanned());

        let source = &goals[0];
        let item = &source.items[0];
        match &item.reference {
            Some(Reference::Resolved {
                target_path,
                display_text,
            }) => {
                assert_eq!(target_path, &PathBuf::from("target.md"));
                assert!(display_text.is_empty(), "wikilink has no display text");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
        assert_eq!(
            item.text, "Target Title",
            "wikilink text filled from target H1"
        );
        assert_eq!(item.children.len(), 1);
        assert!(item.children[0].is_checkbox());
    }

    #[test]
    fn markdown_link_keeps_display_text() {
        let target = goal("T", "t.md", vec![checkbox_leaf("x", true, "t.md", 1)]);
        let source = goal(
            "S",
            "s.md",
            vec![pending_ref("t", "Custom Label", "s.md", 1)],
        );
        let mut goals = vec![source, target];
        resolve_references(&mut goals, &empty_scanned());

        assert_eq!(goals[0].items[0].text, "Custom Label");
    }

    #[test]
    fn detects_cycle() {
        // a.md references b.md, b.md references a.md.
        let a = goal("A", "a.md", vec![pending_ref("b", "", "a.md", 1)]);
        let b = goal("B", "b.md", vec![pending_ref("a", "", "b.md", 1)]);
        let mut goals = vec![a, b];
        resolve_references(&mut goals, &empty_scanned());

        // a.md's reference resolves (b exists).
        let a_ref = goals[0].items[0].reference.as_ref().unwrap();
        assert!(matches!(a_ref, Reference::Resolved { .. }));
        // The cloned child (originally from b.md) is itself a Pending ref to
        // a.md — which is now in the chain, so it becomes a Cycle.
        let nested = &goals[0].items[0].children[0];
        match &nested.reference {
            Some(Reference::Cycle { chain }) => {
                assert!(chain.contains(&PathBuf::from("a.md")));
                assert!(chain.contains(&PathBuf::from("b.md")));
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn broken_ref_not_found() {
        let source = goal("S", "s.md", vec![pending_ref("missing", "", "s.md", 1)]);
        let mut goals = vec![source];
        resolve_references(&mut goals, &empty_scanned());

        match &goals[0].items[0].reference {
            Some(Reference::Broken { reason, .. }) => {
                assert_eq!(*reason, BrokenReason::NotFound);
            }
            other => panic!("expected Broken(NotFound), got {other:?}"),
        }
    }

    #[test]
    fn broken_ref_no_goal_tracker() {
        // The target path IS in scanned_files (so it exists) but has no
        // tracker (not in goals).
        let source = goal("S", "s.md", vec![pending_ref("notes", "", "s.md", 1)]);
        let mut goals = vec![source];
        let mut scanned = HashSet::new();
        scanned.insert(PathBuf::from("notes.md"));
        resolve_references(&mut goals, &scanned);

        match &goals[0].items[0].reference {
            Some(Reference::Broken { reason, .. }) => {
                assert_eq!(*reason, BrokenReason::NoGoalTracker);
            }
            other => panic!("expected Broken(NoGoalTracker), got {other:?}"),
        }
    }

    #[test]
    fn diamond_reference_independent_clones() {
        // B referenced from both A and C; each gets its own deep clone.
        let b = goal("B", "b.md", vec![checkbox_leaf("b-task", false, "b.md", 1)]);
        let a = goal("A", "a.md", vec![pending_ref("b", "", "a.md", 1)]);
        let c = goal("C", "c.md", vec![pending_ref("b", "", "c.md", 1)]);
        let mut goals = vec![a, b, c];
        resolve_references(&mut goals, &empty_scanned());

        // Both A and C reference B and have one cloned child.
        assert_eq!(goals[0].items[0].children.len(), 1);
        assert_eq!(goals[2].items[0].children.len(), 1);
        // They are independent (different paths under the parent).
        assert_eq!(
            goals[0].items[0].children[0].span.path,
            PathBuf::from("b.md")
        );
    }

    #[test]
    fn relative_path_resolves_from_referencing_doc_directory() {
        // source at ml/llm/a.md references "../b/c.md" → ml/b/c.md
        let target = goal(
            "T",
            "ml/b/c.md",
            vec![checkbox_leaf("x", true, "ml/b/c.md", 1)],
        );
        let source = goal(
            "S",
            "ml/llm/a.md",
            vec![pending_ref("../b/c", "", "ml/llm/a.md", 1)],
        );
        let mut goals = vec![source, target];
        resolve_references(&mut goals, &empty_scanned());

        assert!(matches!(
            goals[0].items[0].reference.as_ref().unwrap(),
            Reference::Resolved { .. }
        ));
    }

    #[test]
    fn anchor_is_stripped_before_resolution() {
        let target = goal("T", "t.md", vec![checkbox_leaf("x", true, "t.md", 1)]);
        let source = goal("S", "s.md", vec![pending_ref("t#section", "", "s.md", 1)]);
        let mut goals = vec![source, target];
        resolve_references(&mut goals, &empty_scanned());

        assert!(matches!(
            goals[0].items[0].reference.as_ref().unwrap(),
            Reference::Resolved { .. }
        ));
    }
}
