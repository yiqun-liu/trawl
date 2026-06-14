use std::fs;

use tempfile::tempdir;

use trawl::scanner::{collect_files, ScanOptions};

fn names(files: &[trawl::scanner::FileContents]) -> Vec<String> {
    let mut v: Vec<String> = files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn walks_filters_and_skips_binary() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::write(root.join("a.rs"), "// TODO: fix me\n").unwrap();
    fs::write(root.join("b.md"), "# Hello\n").unwrap();
    fs::write(root.join("plain.txt"), "just text\n").unwrap();

    // Excluded directory.
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("docs/c.md"), "# doc\n").unwrap();

    // Binary file (contains a null byte).
    fs::write(root.join("blob.dat"), vec![1u8, 2, 0, 3]).unwrap();

    let opts = ScanOptions::new(
        root.to_path_buf(),
        &["docs/".to_string()],
        &[],
        false,
        u64::MAX,
        false,
    )
    .unwrap();

    let files = collect_files(&opts).unwrap();
    let got = names(&files);

    assert!(got.contains(&"a.rs".to_string()), "got {got:?}");
    assert!(got.contains(&"b.md".to_string()), "got {got:?}");
    assert!(got.contains(&"plain.txt".to_string()), "got {got:?}");
    assert!(
        !got.contains(&"c.md".to_string()),
        "docs/ should be excluded"
    );
    assert!(
        !got.contains(&"blob.dat".to_string()),
        "binary should be skipped"
    );
}

#[test]
fn include_whitelist_restricts_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("a.rs"), "fn main(){}\n").unwrap();
    fs::write(root.join("b.md"), "# hi\n").unwrap();
    fs::write(root.join("c.py"), "pass\n").unwrap();

    let opts = ScanOptions::new(
        root.to_path_buf(),
        &[],
        &["*.md".to_string()],
        false,
        u64::MAX,
        false,
    )
    .unwrap();
    let got = names(&collect_files(&opts).unwrap());
    assert_eq!(got, vec!["b.md".to_string()]);
}

#[test]
fn max_file_size_skips_large_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("small.txt"), "hi\n").unwrap();
    fs::write(root.join("big.txt"), "x".repeat(2048)).unwrap();

    let opts = ScanOptions::new(root.to_path_buf(), &[], &[], false, 1024, false).unwrap();
    let got = names(&collect_files(&opts).unwrap());
    assert!(got.contains(&"small.txt".to_string()));
    assert!(!got.contains(&"big.txt".to_string()));
}
