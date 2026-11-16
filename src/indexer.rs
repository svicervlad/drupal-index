//! Parallel repository walker and indexing pipeline.

use crate::db::IndexDb;
use crate::error::{IndexError, Result};
use crate::model::SymbolRecord;
use crate::php::{is_php_file, parse_php_file};
use crate::yaml::{YamlKind, parse_yaml_file};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

/// Indexes PHP and Drupal YAML files under `repo_dir` into `db`.
///
/// Skips `vendor/` and `.git/` directories, as well as test fixtures under `tests/fixtures`.
///
/// # Errors
/// Returns [`IndexError`] if file reads, parsing, or `SQLite` writes fail.
pub fn index_repository(
    repo_dir: &Path,
    db: &mut IndexDb,
    commit_sha: &str,
    repo_url: &str,
) -> Result<usize> {
    let files = collect_candidate_files(repo_dir);

    let per_file: Result<Vec<Vec<SymbolRecord>>> = files
        .par_iter()
        .map(|abs_path| {
            let rel = abs_path
                .strip_prefix(repo_dir)
                .unwrap_or(abs_path)
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(source) = std::fs::read_to_string(abs_path) else {
                return Ok(Vec::new());
            };
            if is_php_file(&rel) {
                parse_php_file(&rel, &source)
            } else if YamlKind::from_path(&rel).is_some() {
                Ok(parse_yaml_file(&rel, &source))
            } else {
                Ok(Vec::new())
            }
        })
        .collect();

    let all_records: Vec<SymbolRecord> = per_file?.into_iter().flatten().collect();
    let count = all_records.len();
    let ts = unix_timestamp_string();
    db.replace_all(commit_sha, &ts, repo_url, &all_records)?;
    Ok(count)
}

fn collect_candidate_files(repo_dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(repo_dir)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            if entry.file_type().is_dir() {
                return !matches!(
                    name.as_ref(),
                    ".git" | "vendor" | "node_modules" | "fixtures"
                );
            }
            true
        })
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|p| {
            let s = p.to_string_lossy();
            is_php_file(&s) || YamlKind::from_path(&s).is_some()
        })
        .collect()
}

fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or_else(|_| "0".to_string(), |d| d.as_secs().to_string())
}

/// Resolves the default data directory (`~/.local/share/drupal-index`).
///
/// # Errors
/// Returns [`IndexError::InvalidArg`] if neither `XDG_DATA_HOME` nor `HOME` is set.
pub fn default_data_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.trim().is_empty()
    {
        return Ok(PathBuf::from(xdg).join("drupal-index"));
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return Ok(PathBuf::from(home).join(".local/share/drupal-index"));
    }
    Err(IndexError::InvalidArg(
        "cannot determine data directory: set DRUPAL_INDEX_DATA_DIR or HOME".into(),
    ))
}
