use std::path::{Path, PathBuf};
use std::process;

/// Root of the nearest git repository containing `start`.
pub fn nearest_git_repo(start: &Path) -> Result<PathBuf, String> {
    let output = process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()
        .map_err(|e| format!("failed to locate nearest git repository: {e}"))?;

    if !output.status.success() {
        return Err("not inside a git repository".to_string());
    }

    let repo = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if repo.is_empty() {
        return Err("git did not report a repository root".to_string());
    }

    Ok(PathBuf::from(repo))
}

/// The repository's `remote.origin.url`, required for config lookup.
pub fn remote_origin_url(repo: &Path) -> Result<String, String> {
    let output = process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(repo)
        .output()
        .map_err(|e| {
            format!(
                "failed to read remote.origin.url for {}: {e}",
                repo.display()
            )
        })?;

    if !output.status.success() {
        return Err(format!(
            "{} has no remote.origin.url; git-sidecar v1 config lookup requires one",
            repo.display()
        ));
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return Err(format!(
            "{} has an empty remote.origin.url; git-sidecar v1 config lookup requires one",
            repo.display()
        ));
    }

    Ok(url)
}

/// Path of the repo's exclude file, honoring worktrees and relocated git
/// directories via `git rev-parse --git-common-dir`.
pub fn git_exclude_path(repo: &Path) -> Result<PathBuf, String> {
    let output = process::Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(repo)
        .output()
        .map_err(|e| format!("failed to locate git directory for {}: {e}", repo.display()))?;

    if !output.status.success() {
        return Err(format!(
            "failed to locate git directory for {}",
            repo.display()
        ));
    }

    let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if git_dir.is_empty() {
        return Err(format!(
            "git did not report a git directory for {}",
            repo.display()
        ));
    }

    let git_dir = PathBuf::from(git_dir);
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        repo.join(git_dir)
    };
    Ok(git_dir.join("info").join("exclude"))
}
