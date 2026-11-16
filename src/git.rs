//! Git synchronization for the local `drupal/drupal` mirror.

use crate::error::{IndexError, Result};
use std::path::Path;
use std::process::Command;

/// Default upstream repository URL.
pub const DEFAULT_REPO_URL: &str = "https://github.com/drupal/drupal.git";

/// Default branch to track.
pub const DEFAULT_BRANCH: &str = "main";

/// Clones `repo_url` (`branch`, shallow depth 1) into `repo_dir` or updates it in place.
///
/// # Errors
/// Returns [`IndexError::Git`] or [`IndexError::Io`] if `git` fails.
pub fn sync_repo(repo_dir: &Path, repo_url: &str, branch: &str) -> Result<()> {
    if repo_dir.join(".git").is_dir() {
        run_git(Some(repo_dir), &["fetch", "--depth", "1", "origin", branch])?;
        let target = format!("origin/{branch}");
        run_git(Some(repo_dir), &["reset", "--hard", &target])?;
    } else {
        if let Some(parent) = repo_dir.parent() {
            std::fs::create_dir_all(parent).map_err(|source| IndexError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let dest = repo_dir.to_string_lossy();
        run_git(
            None,
            &[
                "clone",
                "--depth",
                "1",
                "--branch",
                branch,
                "--single-branch",
                repo_url,
                &dest,
            ],
        )?;
    }
    Ok(())
}

/// Returns the current `HEAD` commit SHA of `repo_dir`.
///
/// # Errors
/// Returns [`IndexError::Git`] if `git rev-parse HEAD` fails.
pub fn head_sha(repo_dir: &Path) -> Result<String> {
    let out = run_git(Some(repo_dir), &["rev-parse", "HEAD"])?;
    Ok(out.trim().to_string())
}

fn run_git(cwd: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd
        .output()
        .map_err(|e| IndexError::Git(format!("failed to spawn git {}: {e}", args.join(" "))))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(IndexError::Git(format!(
            "git {} exited with {}: {}",
            args.join(" "),
            output.status,
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
