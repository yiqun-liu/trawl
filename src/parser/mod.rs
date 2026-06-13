//! Parsers for the two annotation types.
//!
//! Both share [`ParseContext`], which compiles the keyword matcher and holds
//! the token configuration derived from [`Config`](crate::Config).

use std::collections::HashMap;

use anyhow::Result;
use regex::Regex;

use crate::config::Config;
use crate::model::Priority;

pub mod goal;
pub mod inline;

/// Shared parsing context, compiled once from configuration.
#[derive(Debug)]
pub struct ParseContext {
    /// Word-boundary regex matching any configured keyword.
    keyword_re: Regex,
    /// Default priority per keyword (uppercase canonical form).
    keyword_priorities: HashMap<String, Priority>,
    /// Token field → prefix (e.g. `owner → "@"`).
    tokens: HashMap<String, String>,
    /// Goal section heading names to detect (e.g. `GOAL TRACKER`).
    goal_section_names: Vec<String>,
    /// Table header field → keyword list, for column mapping.
    headers: HashMap<String, Vec<String>>,
}

impl ParseContext {
    /// Compile a context from resolved configuration.
    pub fn from_config(config: &Config) -> Result<Self> {
        let alternation = config
            .scan
            .keywords
            .iter()
            .map(|k| regex::escape(k))
            .collect::<Vec<_>>()
            .join("|");
        let pattern = if config.scan.keyword_case_sensitive {
            format!("\\b({alternation})\\b")
        } else {
            format!("(?i)\\b({alternation})\\b")
        };
        let keyword_re = Regex::new(&pattern)?;

        let mut keyword_priorities = HashMap::new();
        keyword_priorities.insert("FIXME".to_string(), Priority::High);
        keyword_priorities.insert("BUG".to_string(), Priority::High);
        keyword_priorities.insert("HACK".to_string(), Priority::Med);
        keyword_priorities.insert("XXX".to_string(), Priority::Med);

        Ok(Self {
            keyword_re,
            keyword_priorities,
            tokens: config.tokens.clone(),
            goal_section_names: config.scan.goal_section_names.clone(),
            headers: config.headers.clone(),
        })
    }

    /// The compiled keyword matcher (used by the inline parser).
    pub(crate) fn keyword_re(&self) -> &Regex {
        &self.keyword_re
    }

    /// Token field → prefix map (used by metadata extraction).
    pub(crate) fn tokens(&self) -> &HashMap<String, String> {
        &self.tokens
    }

    /// Default priority for a keyword (case-insensitive), if any.
    pub(crate) fn keyword_priority(&self, keyword: &str) -> Option<Priority> {
        self.keyword_priorities
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(keyword))
            .map(|(_, v)| v.clone())
    }

    /// Goal section heading names to detect.
    pub(crate) fn goal_section_names(&self) -> &[String] {
        &self.goal_section_names
    }

    /// Table header field → keyword list.
    pub(crate) fn headers(&self) -> &HashMap<String, Vec<String>> {
        &self.headers
    }
}
