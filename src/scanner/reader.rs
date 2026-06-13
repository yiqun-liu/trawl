//! Parallel file reading with binary detection.

use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use super::FileContents;

/// Read all files in parallel, skipping any that cannot be read or that look
/// binary (contain a `0x00` byte). Text is decoded lossily so invalid UTF-8
/// never aborts a scan.
pub(super) fn read_all(paths: Vec<PathBuf>) -> Vec<FileContents> {
    paths.par_iter().filter_map(|p| read_one(p)).collect()
}

fn read_one(path: &Path) -> Option<FileContents> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("skip {}: {e}", path.display());
            return None;
        }
    };
    // Stage 6: binary detection via null-byte heuristic.
    if bytes.contains(&0u8) {
        log::debug!("skip binary {}", path.display());
        return None;
    }
    let content = String::from_utf8_lossy(&bytes).into_owned();
    Some(FileContents {
        path: path.to_path_buf(),
        content,
    })
}
