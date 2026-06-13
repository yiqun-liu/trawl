use std::path::PathBuf;

use trawl::parser::{inline, ParseContext};
use trawl::scanner::FileContents;
use trawl::Config;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/inline")
        .join(name)
}

fn load(name: &str) -> FileContents {
    let path = fixture(name);
    let content = std::fs::read_to_string(&path).unwrap();
    FileContents { path, content }
}

#[test]
fn parses_all_forms_in_fixture() {
    let ctx = ParseContext::from_config(&Config::default()).unwrap();
    let tasks = inline::parse_file(&load("forms.c"), &ctx);

    // Every keyword-bearing line should be detected.
    assert!(
        tasks.len() >= 9,
        "expected at least 9 tasks, got {}",
        tasks.len()
    );

    let fully = tasks
        .iter()
        .find(|t| t.description == "handle null user" && t.scope.as_deref() == Some("auth"))
        .expect("fully structured form should parse");
    assert_eq!(fully.metadata.owner.as_deref(), Some("yiqun"));
    assert_eq!(fully.metadata.priority, Some(trawl::Priority::High));

    let minimal = tasks
        .iter()
        .find(|t| t.keyword == "TODO" && t.description.is_empty());
    assert!(
        minimal.is_some(),
        "minimal TODO should parse with empty description"
    );

    // Block-comment trailing "*/" must be stripped.
    let block = tasks
        .iter()
        .find(|t| t.description == "fix this")
        .expect("block comment task should have clean description");
    assert_eq!(block.keyword, "TODO");
}

#[test]
fn parses_markdown_contexts() {
    let ctx = ParseContext::from_config(&Config::default()).unwrap();
    let tasks = inline::parse_file(&load("notes.md"), &ctx);

    let heading = tasks
        .iter()
        .find(|t| t.description == "review this section")
        .expect("## TODO heading should parse");
    assert_eq!(heading.keyword, "TODO");

    let tagged = tasks
        .iter()
        .find(|t| t.description == "add examples for cache types")
        .expect("inline markdown TODO should parse");
    assert_eq!(tagged.metadata.tags, vec!["arch".to_string()]);
}
