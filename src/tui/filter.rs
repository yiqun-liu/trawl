//! Inline task filtering.
//!
//! A compact query language parsed from a single input line. Each whitespace-
//! separated token is either `field:value` (one of `kw`/`keyword`, `tag`,
//! `owner`, `pri`/`priority`, `path`) or free text (matched as a substring of
//! the task description). All specified terms are AND-ed.
//!
//! Stale-only (Phase 3 git blame) and full path globs are out of scope here;
//! `path:` is a substring match.

use crate::model::{InlineTask, Priority};

#[derive(Default, Clone)]
pub(super) struct Filter {
    keyword: Option<String>,
    tag: Option<String>,
    owner: Option<String>,
    priority: Option<Priority>,
    path: Option<String>,
    text: Option<String>,
}

impl Filter {
    /// Parse a query string into a [`Filter`].
    pub(super) fn parse(query: &str) -> Self {
        let mut f = Filter::default();
        let mut text_parts: Vec<&str> = Vec::new();
        for token in query.split_whitespace() {
            if let Some((field, value)) = token.split_once(':') {
                let value = value.to_string();
                match field.to_ascii_lowercase().as_str() {
                    "kw" | "keyword" => f.keyword = Some(value),
                    "tag" => f.tag = Some(value),
                    "owner" => f.owner = Some(value),
                    "pri" | "priority" => f.priority = Some(Priority::parse(&value)),
                    "path" => f.path = Some(value),
                    // Unknown field: ignore the token.
                    _ => {}
                }
            } else {
                text_parts.push(token);
            }
        }
        if !text_parts.is_empty() {
            f.text = Some(text_parts.join(" "));
        }
        f
    }

    /// True if no filter term is set (matches everything).
    pub(super) fn is_empty(&self) -> bool {
        self.keyword.is_none()
            && self.tag.is_none()
            && self.owner.is_none()
            && self.priority.is_none()
            && self.path.is_none()
            && self.text.is_none()
    }

    /// True if the task satisfies every set term (AND).
    pub(super) fn matches(&self, task: &InlineTask) -> bool {
        if let Some(kw) = &self.keyword {
            if !task.keyword.eq_ignore_ascii_case(kw) {
                return false;
            }
        }
        if let Some(tag) = &self.tag {
            let wanted = tag.to_ascii_lowercase();
            if !task
                .metadata
                .tags
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&wanted))
            {
                return false;
            }
        }
        if let Some(owner) = &self.owner {
            match &task.metadata.owner {
                Some(o) if o.eq_ignore_ascii_case(owner) => {}
                _ => return false,
            }
        }
        if let Some(pri) = &self.priority {
            if task.metadata.priority.as_ref() != Some(pri) {
                return false;
            }
        }
        if let Some(path) = &self.path {
            let p = task.span.path.to_string_lossy().to_ascii_lowercase();
            if !p.contains(&path.to_ascii_lowercase()) {
                return false;
            }
        }
        if let Some(text) = &self.text {
            if !task
                .description
                .to_ascii_lowercase()
                .contains(&text.to_ascii_lowercase())
            {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Metadata, Span};
    use std::path::PathBuf;

    fn task(keyword: &str, desc: &str, meta: Metadata, path: &str) -> InlineTask {
        InlineTask {
            keyword: keyword.into(),
            scope: None,
            description: desc.into(),
            metadata: meta,
            span: Span {
                path: PathBuf::from(path),
                line: 1,
            },
        }
    }

    #[test]
    fn empty_filter_matches_all() {
        let f = Filter::parse("");
        assert!(f.is_empty());
        let t = task("TODO", "x", Metadata::default(), "a.rs");
        assert!(f.matches(&t));
    }

    #[test]
    fn keyword_filter_is_case_insensitive() {
        let f = Filter::parse("kw:fixme");
        assert!(f.matches(&task("FIXME", "bug", Metadata::default(), "a.rs")));
        assert!(!f.matches(&task("TODO", "bug", Metadata::default(), "a.rs")));
    }

    #[test]
    fn priority_filter_matches_set_priority() {
        // A parsed FIXME task carries priority High (applied by the parser).
        let high = Metadata {
            priority: Some(Priority::High),
            ..Default::default()
        };
        let f = Filter::parse("pri:high");
        assert!(f.matches(&task("FIXME", "bug", high, "a.rs")));
        assert!(!f.matches(&task("TODO", "bug", Metadata::default(), "a.rs")));
    }

    #[test]
    fn tag_owner_path_and_text_and_together() {
        let meta = Metadata {
            owner: Some("alice".into()),
            tags: vec!["security".into()],
            ..Default::default()
        };
        let t = task("TODO", "handle null user", meta, "src/auth/login.rs");
        let f = Filter::parse("tag:security owner:alice path:auth null");
        assert!(f.matches(&t), "all terms match");
        // Dropping any one term that no longer matches should fail.
        assert!(!Filter::parse("tag:security owner:bob").matches(&t));
        assert!(!Filter::parse("path:kernel").matches(&t));
        assert!(!Filter::parse("nonexistent").matches(&t));
    }

    #[test]
    fn unknown_field_tokens_are_ignored() {
        let t = task("TODO", "refactor something", Metadata::default(), "a.rs");
        // "foo:bar" is an unknown field and is dropped; "refactor" still applies.
        let f = Filter::parse("foo:bar refactor");
        assert!(f.matches(&t));
    }

    #[test]
    fn explicit_priority_alias_pri() {
        let meta = Metadata {
            priority: Some(Priority::Med),
            ..Default::default()
        };
        assert!(Filter::parse("pri:med").matches(&task("TODO", "x", meta, "a.rs")));
    }
}
