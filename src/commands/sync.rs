use std::path::Path;
use std::process::{self, ExitCode};

use crate::config::{RepoContext, Sidecar, read_config};
use crate::exclude::ensure_mappings_excluded;
use crate::git::remote_origin_url;
use crate::remote::same_remote;

pub fn run(target: Option<&str>) -> ExitCode {
    match sync_sidecars(target) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("git-sidecar: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Syncs sidecars for the current repo, cloning any that are not present.
/// Returns Ok(true) when every selected sidecar is present or was cloned.
fn sync_sidecars(target: Option<&str>) -> Result<bool, String> {
    let ctx = RepoContext::discover()?;

    if !ctx.config_path.exists() {
        if let Some(name) = target {
            return Err(format!(
                "sidecar '{name}' not found; no config at {}",
                ctx.config_path.display()
            ));
        }
        println!(
            "no sidecars configured for {} ({})",
            ctx.origin_url,
            ctx.config_path.display()
        );
        return Ok(true);
    }

    let config = read_config(&ctx.config_path)?;

    let mut selected: Vec<(&String, &Sidecar)> = config
        .sidecars
        .iter()
        .filter(|(name, _)| target.is_none_or(|t| t == name.as_str()))
        .collect();
    selected.sort_by(|a, b| a.0.cmp(b.0));

    if selected.is_empty() {
        if let Some(name) = target {
            return Err(format!(
                "sidecar '{name}' not found in {}",
                ctx.config_path.display()
            ));
        }
        println!(
            "no sidecars configured for {} ({})",
            ctx.origin_url,
            ctx.config_path.display()
        );
        return Ok(true);
    }

    let mut all_ok = true;
    let mut present_mappings: Vec<&str> = Vec::new();
    for (name, sidecar) in selected {
        let outcome = sync_sidecar(name, sidecar, &ctx.parent_repo);
        if !outcome.ok {
            all_ok = false;
        }
        if outcome.present {
            present_mappings.push(&sidecar.mapping);
        }
    }

    match ensure_mappings_excluded(&ctx.parent_repo, &present_mappings) {
        Ok(Some(exclude_path)) => {
            println!("updated exclude entries in {}", exclude_path.display());
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("git-sidecar: warning: failed to update git exclude: {e}");
            all_ok = false;
        }
    }

    Ok(all_ok)
}

/// Result of processing one sidecar during sync: whether it completed
/// cleanly, and whether a git repository now exists at its mapping.
struct SyncOutcome {
    ok: bool,
    present: bool,
}

fn sync_sidecar(name: &str, sidecar: &Sidecar, parent_repo: &Path) -> SyncOutcome {
    let sidecar_dir = parent_repo.join(&sidecar.mapping);

    match sync_action(&sidecar_dir) {
        SyncAction::Clone => {
            println!("{name}: cloning {} into {}", sidecar.repo, sidecar.mapping);
            let status = process::Command::new("git")
                .args(["clone", &sidecar.repo])
                .arg(&sidecar_dir)
                .status();
            match status {
                Ok(status) if status.success() => SyncOutcome {
                    ok: true,
                    present: true,
                },
                Ok(_) => {
                    eprintln!("git-sidecar: {name}: git clone failed");
                    SyncOutcome {
                        ok: false,
                        present: false,
                    }
                }
                Err(e) => {
                    eprintln!("git-sidecar: {name}: failed to run git clone: {e}");
                    SyncOutcome {
                        ok: false,
                        present: false,
                    }
                }
            }
        }
        SyncAction::AlreadyPresent => match remote_origin_url(&sidecar_dir) {
            Ok(actual) if !same_remote(&actual, &sidecar.repo) => {
                eprintln!(
                    "git-sidecar: warning: {name}: origin is {actual}, config says {}",
                    sidecar.repo
                );
                SyncOutcome {
                    ok: false,
                    present: true,
                }
            }
            Ok(_) => {
                println!("{name}: already present");
                SyncOutcome {
                    ok: true,
                    present: true,
                }
            }
            Err(_) => {
                eprintln!(
                    "git-sidecar: warning: {name}: existing repo in {} has no readable origin",
                    sidecar.mapping
                );
                SyncOutcome {
                    ok: false,
                    present: true,
                }
            }
        },
        SyncAction::NotARepo => {
            eprintln!(
                "git-sidecar: warning: {name}: mapping '{}' exists but is not a git repository; skipping",
                sidecar.mapping
            );
            SyncOutcome {
                ok: false,
                present: false,
            }
        }
        SyncAction::NotADirectory => {
            eprintln!(
                "git-sidecar: warning: {name}: mapping '{}' exists but is not a directory; skipping",
                sidecar.mapping
            );
            SyncOutcome {
                ok: false,
                present: false,
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum SyncAction {
    /// Mapping is missing or an empty directory: clone into it.
    Clone,
    /// Mapping holds a git repository.
    AlreadyPresent,
    /// Mapping is a non-empty directory without a git repository.
    NotARepo,
    /// Mapping exists but is not a directory.
    NotADirectory,
}

pub fn sync_action(dir: &Path) -> SyncAction {
    if !dir.exists() {
        return SyncAction::Clone;
    }
    if !dir.is_dir() {
        return SyncAction::NotADirectory;
    }
    if dir.join(".git").exists() {
        return SyncAction::AlreadyPresent;
    }
    match std::fs::read_dir(dir) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                SyncAction::Clone
            } else {
                SyncAction::NotARepo
            }
        }
        Err(_) => SyncAction::NotARepo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_mapping_needs_a_clone() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(sync_action(&root.path().join("missing")), SyncAction::Clone);
    }

    #[test]
    fn empty_mapping_directory_needs_a_clone() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("empty");
        std::fs::create_dir(&dir).unwrap();

        assert_eq!(sync_action(&dir), SyncAction::Clone);
    }

    #[test]
    fn mapping_with_git_dir_is_already_present() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("repo");
        std::fs::create_dir_all(dir.join(".git")).unwrap();

        assert_eq!(sync_action(&dir), SyncAction::AlreadyPresent);
    }

    #[test]
    fn non_empty_mapping_without_git_dir_is_not_a_repo() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("files");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();

        assert_eq!(sync_action(&dir), SyncAction::NotARepo);
    }

    #[test]
    fn mapping_that_is_a_file_is_not_a_directory() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("mapping");
        std::fs::write(&file, "").unwrap();

        assert_eq!(sync_action(&file), SyncAction::NotADirectory);
    }
}
