//! Shared inline-metadata extraction.
//!
//! Both the inline task parser and the goal tracker parser extract the same
//! `@owner`, `#tag`, `!priority`, `~due` tokens (plus any custom configured
//! token types) from text. This module implements that extraction once.
//!
//! Rules (from `docs/syntax.md` → Shared Metadata Tokens):
//! - A token starts at a configured prefix preceded by whitespace or
//!   start-of-text, and ends at the next whitespace or end of string.
//! - Extracted tokens are removed from the text; the remainder is the
//!   description, trimmed and whitespace-collapsed.
//! - Prefixes not in the configured token set are left in the description.

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::model::{Metadata, Priority};

/// Extract metadata tokens from `text`, returning the cleaned description and
/// the populated [`Metadata`]. `tokens` maps field name → prefix string.
pub fn extract(text: &str, tokens: &HashMap<String, String>) -> (String, Metadata) {
    // Reverse map: prefix → field. Prefixes are assumed unique across fields.
    let reverse: HashMap<&str, &str> = tokens
        .iter()
        .map(|(f, p)| (p.as_str(), f.as_str()))
        .collect();

    let mut meta = Metadata::default();
    let mut description: Vec<&str> = Vec::new();

    for chunk in text.split_whitespace() {
        if let Some((field, value)) = match_token(chunk, &reverse) {
            assign(&field, value, &mut meta);
        } else {
            description.push(chunk);
        }
    }

    (description.join(" "), meta)
}

/// If `chunk` begins with a configured prefix, return the field name and the
/// remaining value (non-empty). The longest matching prefix wins. The field
/// name is returned owned to keep lifetimes simple.
fn match_token<'a>(chunk: &'a str, reverse: &HashMap<&str, &str>) -> Option<(String, &'a str)> {
    let mut best: Option<(&str, usize, &'a str)> = None;
    for (&prefix, &field) in reverse {
        if let Some(rest) = chunk.strip_prefix(prefix) {
            let is_better = best.is_none_or(|(_, best_len, _)| prefix.len() > best_len);
            if !rest.is_empty() && is_better {
                best = Some((field, prefix.len(), rest));
            }
        }
    }
    best.map(|(field, _, value)| (field.to_string(), value))
}

/// Assign a parsed token value to the right field of [`Metadata`].
fn assign(field: &str, value: &str, meta: &mut Metadata) {
    match field {
        "owner" => meta.owner = Some(value.to_string()),
        "tag" => meta.tags.push(value.to_string()),
        "priority" => meta.priority = Some(Priority::parse(value)),
        "due" => meta.due = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok(),
        other => {
            meta.custom
                .entry(other.to_string())
                .or_default()
                .push(value.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> HashMap<String, String> {
        [
            ("owner".to_string(), "@".to_string()),
            ("tag".to_string(), "#".to_string()),
            ("priority".to_string(), "!".to_string()),
            ("due".to_string(), "~".to_string()),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn extracts_all_known_tokens_and_cleans_description() {
        let (desc, meta) = extract(
            "Lecture 3: Power Prompting #security @yiqun !high ~2025-02-15",
            &tokens(),
        );
        assert_eq!(desc, "Lecture 3: Power Prompting");
        assert_eq!(meta.owner.as_deref(), Some("yiqun"));
        assert_eq!(meta.tags, vec!["security".to_string()]);
        assert_eq!(meta.priority, Some(Priority::High));
        assert_eq!(
            meta.due,
            Some(NaiveDate::from_ymd_opt(2025, 2, 15).unwrap())
        );
    }

    #[test]
    fn multiple_tags_accumulate() {
        let (_, meta) = extract("support both #arch #perf", &tokens());
        assert_eq!(meta.tags, vec!["arch".to_string(), "perf".to_string()]);
    }

    #[test]
    fn unknown_priority_stored_verbatim() {
        let (_, meta) = extract("ship it !critical", &tokens());
        assert_eq!(meta.priority, Some(Priority::Other("critical".to_string())));
    }

    #[test]
    fn non_token_punctuation_stays_in_description() {
        let (desc, meta) = extract("see file.cpp#anchor here", &tokens());
        // "#anchor" is not preceded by whitespace, so it is not a token.
        assert_eq!(desc, "see file.cpp#anchor here");
        assert!(meta.tags.is_empty());
    }

    #[test]
    fn bare_prefix_without_value_is_not_a_token() {
        let (desc, meta) = extract("notify @ about this", &tokens());
        assert_eq!(desc, "notify @ about this");
        assert_eq!(meta.owner, None);
    }

    #[test]
    fn custom_tokens_land_in_custom_map() {
        let mut t = tokens();
        t.insert("effort".to_string(), "%".to_string());
        let (_, meta) = extract("rewrite parser %2h", &t);
        assert_eq!(meta.custom.get("effort"), Some(&vec!["2h".to_string()]));
    }
}
