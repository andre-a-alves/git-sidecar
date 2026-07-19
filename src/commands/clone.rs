use std::process::ExitCode;

use crate::commands::sync::clone_standalone;
use crate::config::{
    RESERVED_NICKNAMES, RepoContext, config_with_sidecar, parse_config, sidecar_config_snippet,
};
use crate::exclude::{ensure_mappings_excluded, exclude_entry};
use crate::layout::{DiskState, clone_unified, disk_state, external_gitdir};
use crate::paths::relative_mapping;
use crate::remote::repo_name_from_url;

pub fn run(
    repo: &str,
    directory: Option<String>,
    name: Option<String>,
    standalone: bool,
) -> ExitCode {
    match clone_sidecar(repo, directory, name, standalone) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("git-sidecar: {e}");
            ExitCode::FAILURE
        }
    }
}

fn ensure_target_is_cloneable(
    nickname: &str,
    target: &std::path::Path,
    gitdir: &std::path::Path,
) -> Result<(), String> {
    match disk_state(target, gitdir) {
        DiskState::Missing => Ok(()),
        DiskState::Standalone | DiskState::Unified => Err(format!(
            "{} already contains a git repository",
            target.display()
        )),
        DiskState::MissingWorktree => Err(format!(
            "a leftover git dir for '{nickname}' already exists at {}; remove it first",
            gitdir.display()
        )),
        DiskState::NotARepo => Err(format!("{} exists and is not empty", target.display())),
        DiskState::NotADirectory => Err(format!(
            "{} exists and is not a directory",
            target.display()
        )),
    }
}

/// Clones a new sidecar repo and registers it in the parent repo's config,
/// creating the config file if it does not exist yet. Refuses to touch
/// anything on nickname/mapping conflicts or a non-empty target directory.
fn clone_sidecar(
    repo: &str,
    directory: Option<String>,
    name: Option<String>,
    standalone: bool,
) -> Result<(), String> {
    let ctx = RepoContext::discover()?;

    let nickname = match name {
        Some(name) => name,
        None => repo_name_from_url(repo)?,
    };
    if nickname.trim().is_empty() {
        return Err("sidecar nickname cannot be empty".to_string());
    }
    if RESERVED_NICKNAMES.contains(&nickname.as_str()) {
        return Err(format!("sidecar nickname '{nickname}' is reserved"));
    }

    let dir = match directory {
        Some(dir) => dir,
        None => repo_name_from_url(repo)?,
    };
    let target = ctx.cwd.join(&dir);
    let mapping = relative_mapping(&ctx.parent_repo, &target)?;

    let existing = if ctx.config_path.exists() {
        Some(
            std::fs::read_to_string(&ctx.config_path)
                .map_err(|e| format!("failed to read {}: {e}", ctx.config_path.display()))?,
        )
    } else {
        None
    };

    if let Some(content) = &existing {
        let config = parse_config(content)
            .map_err(|e| format!("failed to parse {}: {e}", ctx.config_path.display()))?;

        if config.sidecars.contains_key(&nickname) {
            return Err(format!(
                "sidecar '{nickname}' already exists in {}",
                ctx.config_path.display()
            ));
        }
        for (other, sidecar) in &config.sidecars {
            if sidecar.mapping.trim_end_matches('/') == mapping.trim_end_matches('/') {
                return Err(format!(
                    "mapping '{mapping}' is already used by sidecar '{other}'"
                ));
            }
        }
    }

    let gitdir = external_gitdir(&ctx.parent_repo, &nickname)?;
    ensure_target_is_cloneable(&nickname, &target, &gitdir)?;

    let snippet = sidecar_config_snippet(&nickname, repo, &mapping, standalone);
    let new_content = config_with_sidecar(existing.as_deref(), &snippet);
    parse_config(&new_content).map_err(|e| format!("refusing to write an invalid config: {e}"))?;

    println!("{nickname}: cloning {repo} into {mapping}");
    if standalone {
        clone_standalone(repo, &target)?;
    } else {
        clone_unified(repo, &target, &gitdir)?;
    }

    if let Some(config_dir) = ctx.config_path.parent() {
        std::fs::create_dir_all(config_dir)
            .map_err(|e| format!("failed to create {}: {e}", config_dir.display()))?;
    }
    std::fs::write(&ctx.config_path, new_content)
        .map_err(|e| format!("failed to write {}: {e}", ctx.config_path.display()))?;

    println!(
        "registered sidecar '{nickname}' with mapping '{mapping}' in {}",
        ctx.config_path.display()
    );

    match ensure_mappings_excluded(&ctx.parent_repo, &[&mapping]) {
        Ok(Some(exclude_path)) => {
            println!(
                "added '{}' to {}",
                exclude_entry(&mapping),
                exclude_path.display()
            );
        }
        Ok(None) => {}
        Err(e) => {
            return Err(format!(
                "sidecar was cloned and registered, but updating git exclude failed: {e}"
            ));
        }
    }
    Ok(())
}
