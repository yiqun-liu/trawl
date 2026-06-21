//! Incremental search support: locating query matches within rendered rows
//! and building highlight-styled lines for the TUI.
//!
//! Matching is ASCII case-insensitive. [`match_offsets`] uses
//! `str::to_ascii_lowercase`, which is byte-length-preserving, so every
//! returned offset is a valid char boundary in the original text — the
//! highlight splicing never panics on multibyte content. Non-ASCII case
//! folding (e.g. `İ`, `ß`) is intentionally not supported; this keeps the
//! matcher O(n) and panic-free for the common code-search case.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

/// Style applied to the matching substring within a row.
const SEARCH_HIT: Style = Style::new().fg(Color::Black).bg(Color::Yellow);

/// Byte offsets of non-overlapping, ASCII case-insensitive occurrences of
/// `needle` in `haystack`. Empty `needle` yields no matches.
pub(super) fn match_offsets(haystack: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    let lower = haystack.to_ascii_lowercase();
    let needle_l = needle.to_ascii_lowercase();
    let qlen = needle_l.len();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(&needle_l) {
        let abs = from + rel;
        out.push(abs);
        from = abs + qlen;
    }
    out
}

/// True if `line` contains `needle` (ASCII case-insensitive). An empty needle
/// never matches, so an empty search query yields no highlighted rows.
pub(super) fn matches(line: &str, needle: &str) -> bool {
    !match_offsets(line, needle).is_empty()
}

/// Build a `Line` from `text` with every occurrence of `query` highlighted via
/// [`SEARCH_HIT`]; non-matching portions keep `base`. When `query` is empty or
/// absent, the line is returned with `base` style (identical to the renderer's
/// previous behaviour).
pub(super) fn highlighted_line(text: &str, base: Style, query: &str) -> Line<'static> {
    if query.is_empty() {
        return Line::from(text.to_owned()).style(base);
    }
    let offsets = match_offsets(text, query);
    if offsets.is_empty() {
        return Line::from(text.to_owned()).style(base);
    }
    let qlen = query.len();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut prev = 0usize;
    for &start in &offsets {
        let end = start + qlen;
        if start > prev {
            spans.push(Span::styled(text[prev..start].to_owned(), base));
        }
        spans.push(Span::styled(text[start..end].to_owned(), SEARCH_HIT));
        prev = end;
    }
    if prev < text.len() {
        spans.push(Span::styled(text[prev..].to_owned(), base));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_and_multiple_offsets() {
        assert_eq!(match_offsets("hello world", "world"), vec![6]);
        // Case-insensitive across the whole line.
        assert_eq!(match_offsets("Foo BAR foo", "foo"), vec![0, 8]);
    }

    #[test]
    fn offsets_are_non_overlapping() {
        assert_eq!(match_offsets("aaa", "aa"), vec![0]);
    }

    #[test]
    fn empty_or_overlong_needle_matches_nothing() {
        assert!(match_offsets("abc", "").is_empty());
        assert!(match_offsets("ab", "abc").is_empty());
        assert!(!matches("abc", ""));
    }

    #[test]
    fn matches_predicate() {
        assert!(matches("Handle null user", "null"));
        assert!(matches("FIXME: bug", "fixme"));
        assert!(!matches("nothing here", "todo"));
    }

    #[test]
    fn highlighted_line_no_query_is_plain() {
        let line = highlighted_line("TODO: x", Style::new(), "");
        // One span, no highlight splitting.
        assert_eq!(line.spans.len(), 1);
    }

    #[test]
    fn highlighted_line_splits_at_matches() {
        // "ab ab" with query "ab" -> [hit, " ", hit] = 3 spans.
        let line = highlighted_line("ab ab", Style::new(), "ab");
        assert_eq!(line.spans.len(), 3);
    }

    #[test]
    fn highlighted_line_no_match_is_plain() {
        let line = highlighted_line("nothing", Style::new(), "xyz");
        assert_eq!(line.spans.len(), 1);
    }
}
