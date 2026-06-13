//! Inline task parser.
//!
//! Extracts `TODO`/`FIXME`/... markers from a single line. Every shortening
//! of the form — fully structured, scope-only, separator-only, bare, minimal
//! — yields a valid [`InlineTask`]. Malformed input is skipped, never panicked.

use std::path::Path;

use crate::metadata;
use crate::model::{InlineTask, Span};
use crate::parser::ParseContext;
use crate::scanner::FileContents;

/// Parse a single line into an [`InlineTask`], if it contains a keyword.
///
/// `lineno` is 1-based.
pub fn parse_line(
    line: &str,
    path: &Path,
    lineno: usize,
    ctx: &ParseContext,
) -> Option<InlineTask> {
    let m = ctx.keyword_re().find(line)?;
    let keyword = m.as_str().to_string();
    let rest = &line[m.end()..];

    let mut s = rest.trim_start();
    let mut scope = None;
    if let Some(after_paren) = s.strip_prefix('(') {
        if let Some(close) = after_paren.find(')') {
            scope = Some(after_paren[..close].to_string());
            s = after_paren[close + 1..].trim_start();
        }
    }

    // Optional ':' separator. A space separator is consumed by trim_start above.
    let description_raw = s.strip_prefix(':').map(str::trim_start).unwrap_or(s);
    let description_raw = strip_comment_close(description_raw);

    let (description, mut metadata) = metadata::extract(description_raw, ctx.tokens());

    // Apply the keyword's default priority only if none was given explicitly.
    if metadata.priority.is_none() {
        if let Some(p) = ctx.keyword_priority(&keyword) {
            metadata.priority = Some(p);
        }
    }

    Some(InlineTask {
        keyword,
        scope,
        description,
        metadata,
        span: Span {
            path: path.to_path_buf(),
            line: lineno,
        },
    })
}

/// Parse every line of a file into inline tasks, preserving line numbers.
pub fn parse_content(content: &str, path: &Path, ctx: &ParseContext) -> Vec<InlineTask> {
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| parse_line(line, path, i + 1, ctx))
        .collect()
}

/// Parse every line of a [`FileContents`] into inline tasks.
pub fn parse_file(contents: &FileContents, ctx: &ParseContext) -> Vec<InlineTask> {
    parse_content(&contents.content, &contents.path, ctx)
}

/// Strip a trailing block-comment closer (`*/` or `-->`) so descriptions like
/// `fix this */` are cleaned. Only acts when the line literally ends with the
/// closer.
fn strip_comment_close(mut s: &str) -> &str {
    s = s.trim();
    for suffix in ["*/", "-->"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.trim();
            break;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::Priority;

    fn ctx() -> ParseContext {
        ParseContext::from_config(&Config::default()).unwrap()
    }

    #[test]
    fn fully_structured_form() {
        let t = parse_line(
            "// TODO(auth): handle null user @yiqun #security !high ~2025-12-01",
            Path::new("a.c"),
            3,
            &ctx(),
        )
        .unwrap();
        assert_eq!(t.keyword, "TODO");
        assert_eq!(t.scope.as_deref(), Some("auth"));
        assert_eq!(t.description, "handle null user");
        assert_eq!(t.metadata.owner.as_deref(), Some("yiqun"));
        assert_eq!(t.metadata.tags, vec!["security".to_string()]);
        assert_eq!(t.metadata.priority, Some(Priority::High));
        assert!(t.metadata.due.is_some());
        assert_eq!(t.span.line, 3);
    }

    #[test]
    fn scope_only_form() {
        let t = parse_line(
            "// TODO(auth): handle null user",
            Path::new("a.c"),
            1,
            &ctx(),
        )
        .unwrap();
        assert_eq!(t.scope.as_deref(), Some("auth"));
        assert_eq!(t.description, "handle null user");
        assert!(t.metadata.priority.is_none());
    }

    #[test]
    fn separator_only_form() {
        let t = parse_line("// TODO: handle null user", Path::new("a.c"), 1, &ctx()).unwrap();
        assert_eq!(t.keyword, "TODO");
        assert!(t.scope.is_none());
        assert_eq!(t.description, "handle null user");
    }

    #[test]
    fn bare_form() {
        let t = parse_line("// TODO handle null user", Path::new("a.c"), 1, &ctx()).unwrap();
        assert_eq!(t.description, "handle null user");
    }

    #[test]
    fn minimal_form_has_empty_description() {
        let t = parse_line("// TODO", Path::new("a.c"), 1, &ctx()).unwrap();
        assert_eq!(t.keyword, "TODO");
        assert!(t.description.is_empty());
    }

    #[test]
    fn fixme_gets_default_high_priority() {
        let t = parse_line("// FIXME: known bug", Path::new("a.c"), 1, &ctx()).unwrap();
        assert_eq!(t.keyword, "FIXME");
        assert_eq!(t.metadata.priority, Some(Priority::High));
    }

    #[test]
    fn explicit_priority_overrides_keyword_default() {
        let t = parse_line("// FIXME: known bug !low", Path::new("a.c"), 1, &ctx()).unwrap();
        assert_eq!(t.metadata.priority, Some(Priority::Low));
    }

    #[test]
    fn case_insensitive_matching_by_default() {
        let t = parse_line("# todo(perf): optimize", Path::new("a.py"), 1, &ctx()).unwrap();
        assert_eq!(t.keyword.to_ascii_uppercase(), "TODO");
        assert_eq!(t.scope.as_deref(), Some("perf"));
    }

    #[test]
    fn markdown_heading_task() {
        let t = parse_line("## TODO: review this section", Path::new("a.md"), 1, &ctx()).unwrap();
        assert_eq!(t.keyword, "TODO");
        assert_eq!(t.description, "review this section");
    }

    #[test]
    fn block_comment_closer_is_stripped() {
        let t = parse_line("/* TODO: fix this */", Path::new("a.c"), 1, &ctx()).unwrap();
        assert_eq!(t.description, "fix this");
    }

    #[test]
    fn word_boundary_avoids_substring_match() {
        // "TODO" embedded in a larger word must not match.
        assert!(parse_line("// TODOLIST of items", Path::new("a.c"), 1, &ctx()).is_none());
        // "BUG" embedded in "debugging" must not match either.
        assert!(parse_line("// debugging this", Path::new("a.c"), 1, &ctx()).is_none());
    }

    #[test]
    fn standalone_bug_keyword_matches() {
        let t = parse_line("// BUG: crash on empty input", Path::new("a.c"), 1, &ctx()).unwrap();
        assert_eq!(t.keyword.to_ascii_uppercase(), "BUG");
        assert_eq!(t.metadata.priority, Some(Priority::High));
    }

    #[test]
    fn non_keyword_line_is_none() {
        assert!(parse_line("// just a normal comment", Path::new("a.c"), 1, &ctx()).is_none());
    }

    #[test]
    fn multiple_tags_accumulate() {
        let t = parse_line(
            "// TODO: support both #arch #perf",
            Path::new("a.c"),
            1,
            &ctx(),
        )
        .unwrap();
        assert_eq!(
            t.metadata.tags,
            vec!["arch".to_string(), "perf".to_string()]
        );
    }
}
