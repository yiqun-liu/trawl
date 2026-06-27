//! Persisted ephemeral view state.
//!
//! trawl does not manage its items — the files are the database. The one piece
//! of state worth remembering across runs is *how the dashboard was laid out*:
//! which nodes were expanded and which view (Goals / Inline) was active. That is
//! UI state, not derived data, so it lives in the XDG state directory, one file
//! per repository, keyed on the canonical repository path.
//!
//! All operations here are best-effort: a missing file, a parse error, or an
//! unwritable directory is logged and ignored. trawl never blocks or crashes on
//! view state, and stale keys (from files that changed between runs) are dropped
//! by the caller after intersecting them with the freshly-scanned tree.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A snapshot of the dashboard layout for one repository.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewSnapshot {
    /// The active view: `"goals"` or `"inline"`. `None` means "unset".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// Foldable goal/milestone keys that were expanded (`g0`, `g0/1`, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub goals_expanded: Vec<String>,
    /// Expanded inline keys: dir/file keys (`src/a.rs`) and task-context keys
    /// (`src/a.rs::42`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_expanded: Vec<String>,
}

/// Load the persisted snapshot for the repository at `root`. Any failure
/// (missing file, unreadable, unparseable, no home directory) yields an empty
/// snapshot; it is never propagated to the caller.
pub fn load(root: &Path) -> ViewSnapshot {
    match repo_state_path(root) {
        Some(path) => load_from(&path),
        None => ViewSnapshot::default(),
    }
}

/// Persist the snapshot for the repository at `root`, written atomically.
/// Best-effort: IO or serialization failures are logged and otherwise ignored.
pub fn save(root: &Path, snapshot: &ViewSnapshot) {
    if let Some(path) = repo_state_path(root) {
        save_to(&path, snapshot);
    }
}

/// Human-readable state-directory path, for help text. Falls back to a
/// placeholder when no state directory can be resolved.
pub fn state_dir_display() -> String {
    state_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no state directory)".to_string())
}

fn load_from(path: &Path) -> ViewSnapshot {
    match fs::read_to_string(path) {
        Ok(text) => match toml::from_str::<ViewSnapshot>(&text) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                log::debug!("state: parse error in {}: {e}", path.display());
                ViewSnapshot::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ViewSnapshot::default(),
        Err(e) => {
            log::debug!("state: read error in {}: {e}", path.display());
            ViewSnapshot::default()
        }
    }
}

fn save_to(path: &Path, snapshot: &ViewSnapshot) {
    let Some(parent) = path.parent() else {
        return;
    };
    if let Err(e) = fs::create_dir_all(parent) {
        log::warn!("state: cannot create {}: {e}", parent.display());
        return;
    }
    let text = match toml::to_string_pretty(snapshot) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("state: serialize error: {e}");
            return;
        }
    };
    // Write a sibling temp file then rename, so a crash mid-write cannot leave a
    // half-written state file. The temp file sits in the same directory, so the
    // rename never crosses a filesystem boundary.
    let tmp = path.with_extension("tmp");
    if let Err(e) = fs::write(&tmp, &text).and_then(|()| fs::rename(&tmp, path)) {
        log::warn!("state: save failed for {}: {e}", path.display());
        let _ = fs::remove_file(&tmp);
    }
}

/// Resolve the per-repository state file: `<state_dir>/<fnv1a(abs-root)>.toml`.
fn repo_state_path(root: &Path) -> Option<PathBuf> {
    let abs = root.canonicalize().ok()?;
    let dir = state_dir()?;
    Some(dir.join(format!(
        "{}.toml",
        fnv1a_hex(abs.as_os_str().as_encoded_bytes())
    )))
}

/// The repository state directory, honoring XDG on Unix and `%LOCALAPPDATA%` on
/// Windows. `None` when no home directory can be determined.
fn state_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("LOCALAPPDATA").ok()?;
        Some(PathBuf::from(base).join("trawl").join("state"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
            if PathBuf::from(&xdg).is_absolute() {
                return Some(PathBuf::from(xdg).join("trawl"));
            }
        }
        let home = std::env::var("HOME").ok()?;
        Some(
            PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("trawl"),
        )
    }
}

/// FNV-1a (64-bit) over `bytes`, rendered as 16 lowercase hex characters.
/// Non-cryptographic and dependency-free; collisions on repository paths are
/// astronomically unlikely and harmless (a stale file is simply ignored).
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn snapshot() -> ViewSnapshot {
        ViewSnapshot {
            view: Some("inline".into()),
            goals_expanded: vec!["g0".into(), "g0/1".into()],
            inline_expanded: vec!["src".into(), "src/a.rs".into(), "src/a.rs::42".into()],
        }
    }

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.toml");
        assert_eq!(load_from(&path), ViewSnapshot::default());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("abcd0123.toml");
        save_to(&path, &snapshot());
        assert_eq!(load_from(&path), snapshot());
    }

    #[test]
    fn load_malformed_returns_default_without_panicking() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(&path, "this is = not = valid = toml {{{{").unwrap();
        assert_eq!(load_from(&path), ViewSnapshot::default());
    }

    #[test]
    fn save_empty_snapshot_round_trips() {
        // An all-default snapshot serializes to (near) nothing and round-trips.
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.toml");
        save_to(&path, &ViewSnapshot::default());
        assert_eq!(load_from(&path), ViewSnapshot::default());
    }

    #[test]
    fn fnv1a_is_deterministic_and_distinct() {
        assert_eq!(fnv1a_hex(b"/home/me/trawl"), fnv1a_hex(b"/home/me/trawl"));
        assert_ne!(fnv1a_hex(b"/home/me/trawl"), fnv1a_hex(b"/home/me/other"));
        assert_eq!(fnv1a_hex(b"").len(), 16, "renders 16 hex chars");
    }

    #[test]
    fn repo_state_path_is_stable_and_path_specific() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        // Environment-dependent (needs a resolvable state dir); skip if absent.
        let Some(pa) = repo_state_path(a.path()) else {
            return;
        };
        let Some(pb) = repo_state_path(b.path()) else {
            return;
        };
        assert_eq!(pa.parent(), pb.parent(), "both files live in the state dir");
        assert_ne!(
            pa.file_name(),
            pb.file_name(),
            "distinct repos map to distinct files"
        );
        assert_eq!(
            pa,
            repo_state_path(a.path()).unwrap(),
            "stable across calls"
        );
        assert!(
            PathBuf::from(pa.file_name().unwrap())
                .to_string_lossy()
                .ends_with(".toml"),
            "filename has .toml extension"
        );
    }
}
