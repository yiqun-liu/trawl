//! Layered configuration for trawl.
//!
//! Configuration is loaded from (later sources extend/override earlier):
//!
//! ```text
//! built-in defaults  →  ~/.config/trawl/config.toml  →  <repo>/.trawl.toml  →  CLI flags
//! ```
//!
//! Scalars are replaced when a layer provides them; `exclude`/`include`
//! **merge** (union, de-duplicated) across all layers with the built-in
//! defaults (`target/`, `node_modules/`, `.git/`); the `tokens` and `headers`
//! maps merge entry-by-entry so users can add custom metadata types.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Built-in default excludes, always in effect (merged with every layer).
const BUILTIN_EXCLUDE: &[&str] = &["target/", "node_modules/", ".git/"];

/// Resolved configuration after merging all layers.
#[derive(Debug, Clone)]
pub struct Config {
    pub scan: ScanConfig,
    /// Token field name → prefix string (typically one character).
    /// Built-in: `owner="@"`, `tag="#"`, `priority="!"`, `due="~"`.
    pub tokens: HashMap<String, String>,
    /// Header field name → list of header keywords that map to it.
    pub headers: HashMap<String, Vec<String>>,
    pub display: DisplayConfig,
}

/// Scanner-related configuration.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub keywords: Vec<String>,
    pub keyword_case_sensitive: bool,
    pub goal_section_names: Vec<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub max_file_size: String,
    pub scan_hidden: bool,
    pub only_tracked: bool,
    pub skip_quoted_keywords: bool,
}

impl ScanConfig {
    /// Parse [`ScanConfig::max_file_size`] into bytes.
    pub fn max_bytes(&self) -> Result<u64> {
        parse_size(&self.max_file_size)
    }
}

/// Display-related configuration.
#[derive(Debug, Clone)]
pub struct DisplayConfig {
    pub default_sort: String,
    pub show_git_blame: bool,
    pub context_lines: u32,
    pub auto_expand_priority: String,
    pub stale_threshold_days: u32,
    pub verbose: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scan: ScanConfig {
                keywords: vec![
                    "TODO".into(),
                    "FIXME".into(),
                    "HACK".into(),
                    "XXX".into(),
                    "BUG".into(),
                ],
                keyword_case_sensitive: false,
                goal_section_names: vec!["GOAL TRACKER".into(), "TODO".into()],
                include: Vec::new(),
                exclude: Vec::new(),
                max_file_size: "1MB".into(),
                scan_hidden: false,
                only_tracked: true,
                skip_quoted_keywords: true,
            },
            tokens: default_tokens(),
            headers: default_headers(),
            display: DisplayConfig {
                default_sort: "path".into(),
                show_git_blame: true,
                context_lines: 2,
                auto_expand_priority: "high".into(),
                stale_threshold_days: 365,
                verbose: false,
            },
        }
    }
}

fn default_tokens() -> HashMap<String, String> {
    [
        ("owner".to_string(), "@".to_string()),
        ("tag".to_string(), "#".to_string()),
        ("priority".to_string(), "!".to_string()),
        ("due".to_string(), "~".to_string()),
    ]
    .into_iter()
    .collect()
}

fn default_headers() -> HashMap<String, Vec<String>> {
    [
        (
            "task".to_string(),
            vec!["task", "item", "name", "todo", "work"]
                .into_iter()
                .map(String::from)
                .collect(),
        ),
        (
            "state".to_string(),
            vec!["state", "status", "done", "progress", "check"]
                .into_iter()
                .map(String::from)
                .collect(),
        ),
        (
            "owner".to_string(),
            vec!["owner", "assignee", "who"]
                .into_iter()
                .map(String::from)
                .collect(),
        ),
        (
            "priority".to_string(),
            vec!["priority", "pri"]
                .into_iter()
                .map(String::from)
                .collect(),
        ),
        (
            "tag".to_string(),
            vec!["tag", "category", "label"]
                .into_iter()
                .map(String::from)
                .collect(),
        ),
        (
            "due".to_string(),
            vec!["due", "deadline", "target"]
                .into_iter()
                .map(String::from)
                .collect(),
        ),
    ]
    .into_iter()
    .collect()
}

impl Config {
    /// Load configuration for the repository at `root`, merging the
    /// user-global and project config files over the built-in defaults.
    pub fn load(root: &Path) -> Result<Self> {
        let mut config = Config::default();

        if let Some(path) = user_config_path() {
            if path.exists() {
                let layer = read_layer(&path)
                    .with_context(|| format!("reading user config {}", path.display()))?;
                config.merge_layer(layer);
            }
        }

        let project = root.join(".trawl.toml");
        if project.exists() {
            let layer = read_layer(&project)
                .with_context(|| format!("reading project config {}", project.display()))?;
            config.merge_layer(layer);
        }

        // Built-in excludes are always in effect, merged with every layer.
        for builtin in BUILTIN_EXCLUDE {
            config.scan.exclude.push((*builtin).to_string());
        }
        config.scan.exclude = dedup(config.scan.exclude);
        config.scan.include = dedup(config.scan.include);

        Ok(config)
    }

    /// Apply one layer's values over `self`, using the documented semantics.
    fn merge_layer(&mut self, file: ConfigFile) {
        if let Some(scan) = file.scan {
            if let Some(v) = scan.keywords {
                self.scan.keywords = v;
            }
            if let Some(v) = scan.keyword_case_sensitive {
                self.scan.keyword_case_sensitive = v;
            }
            if let Some(v) = scan.goal_section_names {
                self.scan.goal_section_names = v;
            }
            if let Some(v) = scan.include {
                self.scan.include.extend(v);
            }
            if let Some(v) = scan.exclude {
                self.scan.exclude.extend(v);
            }
            if let Some(v) = scan.max_file_size {
                self.scan.max_file_size = v;
            }
            if let Some(v) = scan.scan_hidden {
                self.scan.scan_hidden = v;
            }
            if let Some(v) = scan.only_tracked {
                self.scan.only_tracked = v;
            }
            if let Some(v) = scan.skip_quoted_keywords {
                self.scan.skip_quoted_keywords = v;
            }
        }
        if let Some(tokens) = file.tokens {
            for (k, v) in tokens {
                self.tokens.insert(k, v);
            }
        }
        if let Some(headers) = file.headers {
            for (k, v) in headers {
                self.headers.insert(k, v);
            }
        }
        if let Some(display) = file.display {
            if let Some(v) = display.default_sort {
                self.display.default_sort = v;
            }
            if let Some(v) = display.show_git_blame {
                self.display.show_git_blame = v;
            }
            if let Some(v) = display.context_lines {
                self.display.context_lines = v;
            }
            if let Some(v) = display.auto_expand_priority {
                self.display.auto_expand_priority = v;
            }
            if let Some(v) = display.stale_threshold_days {
                self.display.stale_threshold_days = v;
            }
        }
    }
}

/// The on-disk representation of a config file. Every field is optional so a
/// layer can specify only the keys it cares about.
#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    scan: Option<ScanFile>,
    #[serde(default)]
    tokens: Option<HashMap<String, String>>,
    #[serde(default)]
    headers: Option<HashMap<String, Vec<String>>>,
    #[serde(default)]
    display: Option<DisplayFile>,
}

#[derive(Debug, Default, Deserialize)]
struct ScanFile {
    #[serde(default)]
    keywords: Option<Vec<String>>,
    #[serde(default)]
    keyword_case_sensitive: Option<bool>,
    #[serde(default)]
    goal_section_names: Option<Vec<String>>,
    #[serde(default)]
    include: Option<Vec<String>>,
    #[serde(default)]
    exclude: Option<Vec<String>>,
    #[serde(default)]
    max_file_size: Option<String>,
    #[serde(default)]
    scan_hidden: Option<bool>,
    #[serde(default)]
    only_tracked: Option<bool>,
    #[serde(default)]
    skip_quoted_keywords: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct DisplayFile {
    #[serde(default)]
    default_sort: Option<String>,
    #[serde(default)]
    show_git_blame: Option<bool>,
    #[serde(default)]
    context_lines: Option<u32>,
    #[serde(default)]
    auto_expand_priority: Option<String>,
    #[serde(default)]
    stale_threshold_days: Option<u32>,
}

fn read_layer(path: &Path) -> Result<ConfigFile> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading config file {}", path.display()))?;
    let parsed =
        toml::from_str(&text).with_context(|| format!("parsing config file {}", path.display()))?;
    Ok(parsed)
}

/// Path to the user-global config, if the home directory is known.
fn user_config_path() -> Option<PathBuf> {
    if let Ok(home) = env::var("HOME") {
        return Some(PathBuf::from(home).join(".config/trawl/config.toml"));
    }
    if let Ok(userprofile) = env::var("USERPROFILE") {
        return Some(
            PathBuf::from(userprofile)
                .join(".config")
                .join("trawl")
                .join("config.toml"),
        );
    }
    None
}

/// De-duplicate a vector of strings, preserving first-seen order.
fn dedup(mut items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items.retain(|s| seen.insert(s.clone()));
    items
}

/// Parse a human-readable size string (`"1MB"`, `"512k"`, `"2GB"`) into bytes.
fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: u64 = num
        .parse()
        .with_context(|| format!("invalid size number {num:?}"))?;
    let mult: u64 = match unit.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024 * 1024,
        "G" | "GB" => 1024 * 1024 * 1024,
        other => anyhow::bail!("unknown size unit {other:?}"),
    };
    Ok(n * mult)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_builtin_keywords_and_tokens() {
        let c = Config::default();
        assert_eq!(c.scan.keywords, vec!["TODO", "FIXME", "HACK", "XXX", "BUG"]);
        assert_eq!(c.tokens.get("owner").map(String::as_str), Some("@"));
        assert_eq!(c.tokens.get("priority").map(String::as_str), Some("!"));
        assert_eq!(c.display.context_lines, 2);
    }

    #[test]
    fn parse_size_handles_units() {
        assert_eq!(parse_size("1MB").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("512k").unwrap(), 512 * 1024);
        assert_eq!(parse_size("2GB").unwrap(), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("1000").unwrap(), 1000);
        assert!(parse_size("abc").is_err());
        assert!(parse_size("1PB").is_err());
    }

    #[test]
    fn merge_layer_unions_exclude_and_keeps_builtins() {
        let mut c = Config::default();
        let layer = ConfigFile {
            scan: Some(ScanFile {
                exclude: Some(vec!["docs/".into()]),
                ..Default::default()
            }),
            ..Default::default()
        };
        c.merge_layer(layer);
        for builtin in BUILTIN_EXCLUDE {
            c.scan.exclude.push((*builtin).to_string());
        }
        c.scan.exclude = dedup(c.scan.exclude);

        assert!(c.scan.exclude.contains(&"docs/".to_string()));
        assert!(c.scan.exclude.contains(&"target/".to_string()));
        assert!(c.scan.exclude.contains(&"node_modules/".to_string()));
        assert!(c.scan.exclude.contains(&".git/".to_string()));
    }

    #[test]
    fn merge_layer_tokens_extend_defaults() {
        let mut c = Config::default();
        let layer = ConfigFile {
            tokens: Some(
                [("effort".to_string(), "%".to_string())]
                    .into_iter()
                    .collect(),
            ),
            ..Default::default()
        };
        c.merge_layer(layer);
        assert_eq!(c.tokens.get("effort").map(String::as_str), Some("%"));
        // default tokens are retained
        assert_eq!(c.tokens.get("tag").map(String::as_str), Some("#"));
    }

    #[test]
    fn merge_layer_keywords_replace() {
        let mut c = Config::default();
        let layer = ConfigFile {
            scan: Some(ScanFile {
                keywords: Some(vec!["TODO".into(), "FIXME".into()]),
                ..Default::default()
            }),
            ..Default::default()
        };
        c.merge_layer(layer);
        assert_eq!(c.scan.keywords, vec!["TODO", "FIXME"]);
    }

    #[test]
    fn toml_layer_parses() {
        let text = r#"
[scan]
exclude = ["docs/", "tests/fixtures/"]
keywords = ["TODO", "FIXME"]

[tokens]
effort = "%"

[display]
context_lines = 5
"#;
        let file: ConfigFile = toml::from_str(text).unwrap();
        assert_eq!(
            file.scan.unwrap().exclude.unwrap(),
            vec!["docs/", "tests/fixtures/"]
        );
        assert_eq!(
            file.tokens.unwrap().get("effort").map(String::as_str),
            Some("%")
        );
    }
}
