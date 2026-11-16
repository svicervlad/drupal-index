//! Installs the PHP dev tools the Drupal gate uses (`phpcs` + `drupal/coder`, `phpstan`) into the
//! mirror's `vendor/`, from the `require-dev` of `drupal/drupal` itself.

use crate::error::{IndexError, Result};
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

/// Outcome of [`install_dev_tools`].
#[derive(Debug, PartialEq, Eq)]
pub enum ToolsStatus {
    /// `composer update` finished; `vendor/bin` holds the tools.
    Installed,
    /// `composer` is not on `PATH`; nothing was changed.
    ComposerMissing,
}

/// Returns `true` when `repo_dir` has no `vendor/bin/phpcs` yet.
#[must_use]
pub fn tools_missing(repo_dir: &Path) -> bool {
    !repo_dir.join("vendor").join("bin").join("phpcs").is_file()
}

/// Deletes `composer.lock` in `repo_dir` and runs `composer update` there.
///
/// Scripts are skipped (`post-update-cmd` would regenerate tracked metapackage files); plugins
/// still run, so `dealerdirect/phpcodesniffer-composer-installer` registers the Drupal sniffs.
///
/// # Errors
/// Returns [`IndexError::Io`] if the lock cannot be removed, or [`IndexError::Composer`] if
/// `composer` fails.
pub fn install_dev_tools(repo_dir: &Path) -> Result<ToolsStatus> {
    remove_lock(repo_dir)?;
    let output = match Command::new("composer")
        .args([
            "update",
            "--no-interaction",
            "--no-progress",
            "--no-scripts",
        ])
        .current_dir(repo_dir)
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(ToolsStatus::ComposerMissing),
        Err(e) => {
            return Err(IndexError::Composer(format!(
                "failed to spawn composer: {e}"
            )));
        }
    };
    if output.status.success() {
        return Ok(ToolsStatus::Installed);
    }
    Err(IndexError::Composer(format!(
        "composer update exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

// The lock is tracked in drupal/drupal: the next `git reset --hard` in `sync` restores it.
fn remove_lock(repo_dir: &Path) -> Result<()> {
    let lock = repo_dir.join("composer.lock");
    match std::fs::remove_file(&lock) {
        Err(source) if source.kind() != ErrorKind::NotFound => {
            Err(IndexError::Io { path: lock, source })
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_missing_until_phpcs_exists() {
        let dir = tempfile::tempdir().expect("create temp dir");
        assert!(tools_missing(dir.path()));
        let bin = dir.path().join("vendor").join("bin");
        std::fs::create_dir_all(&bin).expect("create vendor/bin");
        std::fs::write(bin.join("phpcs"), "").expect("write phpcs stub");
        assert!(!tools_missing(dir.path()));
    }

    #[test]
    fn remove_lock_deletes_file_and_tolerates_absence() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let lock = dir.path().join("composer.lock");
        std::fs::write(&lock, "{}").expect("write lock");
        remove_lock(dir.path()).expect("remove existing lock");
        assert!(!lock.exists());
        remove_lock(dir.path()).expect("remove absent lock");
    }
}
