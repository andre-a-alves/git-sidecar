//! On-disk layout of a sidecar's git directory.
//!
//! Unified sidecars keep their git directory outside the mapping directory,
//! under `<parent git dir>/git-sidecar/<name>/gitdir`. With no `.git` inside
//! the mapping, git's upward repository discovery walks straight past the
//! sidecar to the parent, so bare `git` run inside a sidecar operates on the
//! parent repo. Standalone sidecars keep the traditional `<mapping>/.git`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process;

use crate::git::git_common_dir;
use crate::paths::normalize_lexically;

pub const GITDIRS_DIR_NAME: &str = "git-sidecar";
pub const GITDIR_LEAF: &str = "gitdir";

/// Where a unified sidecar's git directory lives, derived deterministically
/// from the parent's git directory and the sidecar's nickname.
pub fn external_gitdir(parent_repo: &Path, name: &str) -> Result<PathBuf, String> {
    Ok(git_common_dir(parent_repo)?
        .join(GITDIRS_DIR_NAME)
        .join(name)
        .join(GITDIR_LEAF))
}

/// A cheap validity check: a real git directory always has a HEAD file.
pub fn is_valid_gitdir(gitdir: &Path) -> bool {
    gitdir.join("HEAD").is_file()
}

/// Whether `.git` is a gitlink file pointing at `gitdir`.
fn gitfile_points_at(dotgit: &Path, gitdir: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(dotgit) else {
        return false;
    };
    let Some(target) = content.trim().strip_prefix("gitdir:") else {
        return false;
    };
    normalize_lexically(Path::new(target.trim())) == normalize_lexically(gitdir)
}

/// What is physically on disk for one sidecar, given its mapping directory
/// and the external git-dir location its nickname maps to.
#[derive(Debug, PartialEq)]
pub enum DiskState {
    /// Mapping is missing or an empty directory, and there is no external
    /// git dir: clone into it.
    Missing,
    /// Mapping directory does not exist but a valid external git dir does:
    /// only the working tree needs restoring.
    MissingWorktree,
    /// Mapping holds its own `.git`: traditional (standalone / old) layout.
    Standalone,
    /// Mapping has no `.git` of its own and the external git dir is valid.
    Unified,
    /// Mapping is a non-empty directory with no git repository anywhere.
    NotARepo,
    /// Mapping exists but is not a directory.
    NotADirectory,
}

pub fn disk_state(dir: &Path, gitdir: &Path) -> DiskState {
    if dir.exists() && !dir.is_dir() {
        return DiskState::NotADirectory;
    }

    let dotgit = dir.join(".git");
    if dotgit.exists() {
        if dotgit.is_file() && gitfile_points_at(&dotgit, gitdir) {
            // leftover gitlink from an interrupted migration
            return DiskState::Unified;
        }
        return DiskState::Standalone;
    }

    if is_valid_gitdir(gitdir) {
        return if dir.exists() {
            DiskState::Unified
        } else {
            DiskState::MissingWorktree
        };
    }

    if !dir.exists() {
        return DiskState::Missing;
    }
    match std::fs::read_dir(dir) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                DiskState::Missing
            } else {
                DiskState::NotARepo
            }
        }
        Err(_) => DiskState::NotARepo,
    }
}

/// Clones `repo` into `target` with its git directory at `gitdir`, then
/// detaches the working tree so `target` holds no `.git` entry.
pub fn clone_unified(repo: &str, target: &Path, gitdir: &Path) -> Result<(), String> {
    if let Some(parent) = gitdir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }

    let mut separate = OsString::from("--separate-git-dir=");
    separate.push(gitdir);
    let status = process::Command::new("git")
        .arg("clone")
        .arg(separate)
        .arg(repo)
        .arg(target)
        .status()
        .map_err(|e| format!("failed to run git clone: {e}"))?;
    if !status.success() {
        return Err("git clone failed".to_string());
    }

    detach_worktree(target, gitdir)
}

/// Removes the `.git` gitlink from the mapping directory and pins the work
/// tree in the git dir's config instead, so plain git run inside the mapping
/// directory discovers the parent repo rather than the sidecar.
pub fn detach_worktree(target: &Path, gitdir: &Path) -> Result<(), String> {
    let dotgit = target.join(".git");
    if dotgit.is_file() {
        std::fs::remove_file(&dotgit)
            .map_err(|e| format!("failed to remove {}: {e}", dotgit.display()))?;
    }
    set_core_worktree(gitdir, target)
}

/// Moves an in-mapping `.git` directory out to the external location
/// (standalone/old layout -> unified).
pub fn relocate_gitdir_out(mapping_dir: &Path, gitdir: &Path) -> Result<(), String> {
    if gitdir.exists() {
        return Err(format!(
            "{} already exists; refusing to overwrite it",
            gitdir.display()
        ));
    }
    if let Some(parent) = gitdir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }

    let dotgit = mapping_dir.join(".git");
    std::fs::rename(&dotgit, gitdir).map_err(|e| {
        format!(
            "failed to move {} to {}: {e}",
            dotgit.display(),
            gitdir.display()
        )
    })?;
    set_core_worktree(gitdir, mapping_dir)
}

/// Moves the external git directory back inside the mapping directory
/// (unified -> standalone layout).
pub fn relocate_gitdir_in(mapping_dir: &Path, gitdir: &Path) -> Result<(), String> {
    let dotgit = mapping_dir.join(".git");
    if dotgit.is_file() && gitfile_points_at(&dotgit, gitdir) {
        std::fs::remove_file(&dotgit)
            .map_err(|e| format!("failed to remove {}: {e}", dotgit.display()))?;
    }
    if dotgit.exists() {
        return Err(format!(
            "{} already exists; refusing to overwrite it",
            dotgit.display()
        ));
    }

    unset_core_worktree(gitdir);
    std::fs::rename(gitdir, &dotgit).map_err(|e| {
        format!(
            "failed to move {} to {}: {e}",
            gitdir.display(),
            dotgit.display()
        )
    })?;
    remove_external_dirs(gitdir);
    Ok(())
}

/// Restores a missing working tree from a still-valid external git dir.
pub fn restore_worktree(mapping_dir: &Path, gitdir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(mapping_dir)
        .map_err(|e| format!("failed to create {}: {e}", mapping_dir.display()))?;
    set_core_worktree(gitdir, mapping_dir)?;

    let status = process::Command::new("git")
        .arg("--git-dir")
        .arg(gitdir)
        .arg("--work-tree")
        .arg(mapping_dir)
        .args(["reset", "--hard", "-q"])
        .current_dir(mapping_dir)
        .status()
        .map_err(|e| format!("failed to run git reset: {e}"))?;
    if !status.success() {
        return Err("git reset failed while restoring the working tree".to_string());
    }
    Ok(())
}

/// Removes the now-empty `git-sidecar/<name>/` holder (and `git-sidecar/`
/// itself when it has no other sidecars) after a git dir moved back inside
/// its mapping. Failures are ignored: a non-empty directory just stays.
pub fn remove_external_dirs(gitdir: &Path) {
    if let Some(name_dir) = gitdir.parent() {
        let _ = std::fs::remove_dir(name_dir);
        if let Some(root) = name_dir.parent() {
            let _ = std::fs::remove_dir(root);
        }
    }
}

fn set_core_worktree(gitdir: &Path, worktree: &Path) -> Result<(), String> {
    let worktree = dunce::canonicalize(worktree).unwrap_or_else(|_| worktree.to_path_buf());
    let status = process::Command::new("git")
        .arg("--git-dir")
        .arg(gitdir)
        .arg("config")
        .arg("core.worktree")
        .arg(&worktree)
        .status()
        .map_err(|e| format!("failed to run git config: {e}"))?;
    if !status.success() {
        return Err(format!(
            "failed to set core.worktree in {}",
            gitdir.display()
        ));
    }
    Ok(())
}

fn unset_core_worktree(gitdir: &Path) {
    // A stale core.worktree is harmless once `.git` is back inside the
    // mapping directory, so failures (including "not set") are ignored.
    let _ = process::Command::new("git")
        .arg("--git-dir")
        .arg(gitdir)
        .args(["config", "--unset", "core.worktree"])
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_gitdir(root: &Path) -> PathBuf {
        let gitdir = root.join("gitdir");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        gitdir
    }

    #[test]
    fn missing_mapping_without_gitdir_needs_a_clone() {
        let root = tempfile::tempdir().unwrap();
        let gitdir = root.path().join("no-gitdir");

        assert_eq!(
            disk_state(&root.path().join("missing"), &gitdir),
            DiskState::Missing
        );
    }

    #[test]
    fn empty_mapping_without_gitdir_needs_a_clone() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("empty");
        std::fs::create_dir(&dir).unwrap();

        assert_eq!(
            disk_state(&dir, &root.path().join("no-gitdir")),
            DiskState::Missing
        );
    }

    #[test]
    fn missing_mapping_with_valid_gitdir_needs_worktree_restore() {
        let root = tempfile::tempdir().unwrap();
        let gitdir = fake_gitdir(root.path());

        assert_eq!(
            disk_state(&root.path().join("missing"), &gitdir),
            DiskState::MissingWorktree
        );
    }

    #[test]
    fn empty_mapping_with_valid_gitdir_is_unified() {
        let root = tempfile::tempdir().unwrap();
        let gitdir = fake_gitdir(root.path());
        let dir = root.path().join("empty");
        std::fs::create_dir(&dir).unwrap();

        assert_eq!(disk_state(&dir, &gitdir), DiskState::Unified);
    }

    #[test]
    fn mapping_with_git_dir_is_standalone() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("repo");
        std::fs::create_dir_all(dir.join(".git")).unwrap();

        assert_eq!(
            disk_state(&dir, &root.path().join("no-gitdir")),
            DiskState::Standalone
        );
    }

    #[test]
    fn mapping_without_git_dir_but_valid_external_gitdir_is_unified() {
        let root = tempfile::tempdir().unwrap();
        let gitdir = fake_gitdir(root.path());
        let dir = root.path().join("repo");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("file.txt"), "").unwrap();

        assert_eq!(disk_state(&dir, &gitdir), DiskState::Unified);
    }

    #[test]
    fn leftover_gitlink_pointing_at_external_gitdir_is_unified() {
        let root = tempfile::tempdir().unwrap();
        let gitdir = fake_gitdir(root.path());
        let dir = root.path().join("repo");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();

        assert_eq!(disk_state(&dir, &gitdir), DiskState::Unified);
    }

    #[test]
    fn gitlink_pointing_elsewhere_is_standalone() {
        let root = tempfile::tempdir().unwrap();
        let gitdir = fake_gitdir(root.path());
        let dir = root.path().join("repo");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(".git"), "gitdir: /somewhere/else\n").unwrap();

        assert_eq!(disk_state(&dir, &gitdir), DiskState::Standalone);
    }

    #[test]
    fn non_empty_mapping_without_any_git_dir_is_not_a_repo() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("files");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();

        assert_eq!(
            disk_state(&dir, &root.path().join("no-gitdir")),
            DiskState::NotARepo
        );
    }

    #[test]
    fn mapping_that_is_a_file_is_not_a_directory() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("mapping");
        std::fs::write(&file, "").unwrap();

        assert_eq!(
            disk_state(&file, &root.path().join("no-gitdir")),
            DiskState::NotADirectory
        );
    }
}
