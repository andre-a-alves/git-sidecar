use std::path::Path;
use std::process::{self, ExitCode};

use crate::config::{RepoContext, Sidecar, config_set_standalone, parse_config, read_config};
use crate::exclude::ensure_mappings_excluded;
use crate::git::{remote_origin_url, remote_origin_url_from_gitdir};
use crate::layout::{
    DiskState, clone_unified, detach_worktree, disk_state, external_gitdir, relocate_gitdir_in,
    relocate_gitdir_out, restore_worktree,
};
use crate::remote::same_remote;

pub fn run(target: Option<&str>, standalone: bool, unify: bool) -> ExitCode {
    match sync_sidecars(target, standalone, unify) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("git-sidecar: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Syncs sidecars for the current repo: clones any that are missing, moves
/// old-layout git dirs out to the unified external location (unless the
/// sidecar is marked standalone in config), and applies `--standalone` /
/// `--unify` layout changes. Returns Ok(true) when every selected sidecar
/// completed cleanly.
fn sync_sidecars(target: Option<&str>, standalone: bool, unify: bool) -> Result<bool, String> {
    let ctx = RepoContext::discover()?;

    if standalone && target.is_none() {
        return Err("--standalone requires a sidecar name".to_string());
    }
    if unify && target.is_none() {
        return Err("--unify requires a sidecar name".to_string());
    }

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

    // Config is the source of truth for each sidecar's layout, so a flag
    // first persists the new setting, then the disk is reconciled to it.
    if standalone || unify {
        set_standalone_in_config(&ctx, target.expect("flag requires a name"), standalone)?;
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
        let outcome = sync_sidecar(name, sidecar, &ctx.parent_repo, standalone);
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

fn set_standalone_in_config(ctx: &RepoContext, name: &str, standalone: bool) -> Result<(), String> {
    let content = std::fs::read_to_string(&ctx.config_path)
        .map_err(|e| format!("failed to read {}: {e}", ctx.config_path.display()))?;
    let config = parse_config(&content)
        .map_err(|e| format!("failed to parse {}: {e}", ctx.config_path.display()))?;

    let Some(sidecar) = config.sidecars.get(name) else {
        return Err(format!(
            "sidecar '{name}' not found in {}",
            ctx.config_path.display()
        ));
    };
    if sidecar.standalone == standalone {
        return Ok(());
    }

    let new_content = config_set_standalone(&content, name, standalone)?;
    parse_config(&new_content).map_err(|e| format!("refusing to write an invalid config: {e}"))?;
    std::fs::write(&ctx.config_path, new_content)
        .map_err(|e| format!("failed to write {}: {e}", ctx.config_path.display()))?;

    let layout = if standalone { "standalone" } else { "unified" };
    println!(
        "marked sidecar '{name}' as {layout} in {}",
        ctx.config_path.display()
    );
    Ok(())
}

/// Result of processing one sidecar during sync: whether it completed
/// cleanly, and whether a git repository now exists for its mapping.
struct SyncOutcome {
    ok: bool,
    present: bool,
}

impl SyncOutcome {
    fn ok(present: bool) -> Self {
        SyncOutcome { ok: true, present }
    }

    fn failed(present: bool) -> Self {
        SyncOutcome { ok: false, present }
    }

    /// Reports a step's error under the sidecar's name; `present` says
    /// whether a repo exists at the mapping even when the step failed.
    fn of(name: &str, present_on_err: bool, result: Result<(), String>) -> Self {
        match result {
            Ok(()) => SyncOutcome::ok(true),
            Err(e) => {
                eprintln!("git-sidecar: {name}: {e}");
                SyncOutcome::failed(present_on_err)
            }
        }
    }
}

fn sync_sidecar(
    name: &str,
    sidecar: &Sidecar,
    parent_repo: &Path,
    force_standalone: bool,
) -> SyncOutcome {
    let sidecar_dir = parent_repo.join(&sidecar.mapping);
    let gitdir = match external_gitdir(parent_repo, name) {
        Ok(gitdir) => gitdir,
        Err(e) => {
            eprintln!("git-sidecar: {name}: {e}");
            return SyncOutcome::failed(false);
        }
    };

    match disk_state(&sidecar_dir, &gitdir) {
        DiskState::Missing => {
            println!("{name}: cloning {} into {}", sidecar.repo, sidecar.mapping);
            let result = if sidecar.standalone {
                clone_standalone(&sidecar.repo, &sidecar_dir)
            } else {
                clone_unified(&sidecar.repo, &sidecar_dir, &gitdir)
            };
            SyncOutcome::of(name, false, result)
        }
        DiskState::MissingWorktree => {
            println!(
                "{name}: restoring working tree in {} from {}",
                sidecar.mapping,
                gitdir.display()
            );
            SyncOutcome::of(name, false, restore_worktree(&sidecar_dir, &gitdir))
        }
        DiskState::Standalone => {
            // origin sanity check comes before any relocation
            if let Some(outcome) = check_origin(name, sidecar, remote_origin_url(&sidecar_dir)) {
                return outcome;
            }
            if sidecar.standalone {
                println!("{name}: already present");
                return SyncOutcome::ok(true);
            }
            println!(
                "{name}: moving git dir of {} to {}",
                sidecar.mapping,
                gitdir.display()
            );
            SyncOutcome::of(name, true, relocate_gitdir_out(&sidecar_dir, &gitdir))
        }
        DiskState::Unified => {
            if let Some(outcome) =
                check_origin(name, sidecar, remote_origin_url_from_gitdir(&gitdir))
            {
                return outcome;
            }
            if force_standalone {
                println!(
                    "{name}: moving git dir back into {} from {}",
                    sidecar.mapping,
                    gitdir.display()
                );
                return SyncOutcome::of(name, true, relocate_gitdir_in(&sidecar_dir, &gitdir));
            }
            if sidecar.standalone {
                // deliberately left alone: only --standalone moves it back
                println!("{name}: already present");
                return SyncOutcome::ok(true);
            }
            // finish an interrupted migration if a stale gitlink remains
            let outcome = SyncOutcome::of(name, true, detach_worktree(&sidecar_dir, &gitdir));
            if outcome.ok {
                println!("{name}: already present");
            }
            outcome
        }
        DiskState::NotARepo => {
            eprintln!(
                "git-sidecar: warning: {name}: mapping '{}' exists but is not a git repository; skipping",
                sidecar.mapping
            );
            SyncOutcome::failed(false)
        }
        DiskState::NotADirectory => {
            eprintln!(
                "git-sidecar: warning: {name}: mapping '{}' exists but is not a directory; skipping",
                sidecar.mapping
            );
            SyncOutcome::failed(false)
        }
    }
}

/// Warns and short-circuits when the on-disk origin cannot be read or does
/// not match the configured repo; returns None when the origin is fine.
fn check_origin(
    name: &str,
    sidecar: &Sidecar,
    origin: Result<String, String>,
) -> Option<SyncOutcome> {
    match origin {
        Ok(actual) if !same_remote(&actual, &sidecar.repo) => {
            eprintln!(
                "git-sidecar: warning: {name}: origin is {actual}, config says {}",
                sidecar.repo
            );
            Some(SyncOutcome::failed(true))
        }
        Ok(_) => None,
        Err(_) => {
            eprintln!(
                "git-sidecar: warning: {name}: existing repo in {} has no readable origin",
                sidecar.mapping
            );
            Some(SyncOutcome::failed(true))
        }
    }
}

pub fn clone_standalone(repo: &str, target: &Path) -> Result<(), String> {
    let status = process::Command::new("git")
        .args(["clone", repo])
        .arg(target)
        .status()
        .map_err(|e| format!("failed to run git clone: {e}"))?;
    if !status.success() {
        return Err("git clone failed".to_string());
    }
    Ok(())
}
