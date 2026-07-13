use std::process::{self, ExitCode};

use crate::commands::sync::{SyncAction, sync_action};
use crate::config::{
    RESERVED_NICKNAMES, RepoContext, config_with_shadow, parse_config, shadow_config_snippet,
};
use crate::exclude::{ensure_mappings_excluded, exclude_entry};
use crate::paths::relative_mapping;
use crate::remote::repo_name_from_url;

pub fn run(repo: &str, directory: Option<String>, name: Option<String>) -> ExitCode {
    match clone_shadow(repo, directory, name) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("git-shadow: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Clones a new shadow repo and registers it in the parent repo's config,
/// creating the config file if it does not exist yet. Refuses to touch
/// anything on nickname/mapping conflicts or a non-empty target directory.
fn clone_shadow(repo: &str, directory: Option<String>, name: Option<String>) -> Result<(), String> {
    let ctx = RepoContext::discover()?;

    let nickname = match name {
        Some(name) => name,
        None => repo_name_from_url(repo)?,
    };
    if nickname.trim().is_empty() {
        return Err("shadow nickname cannot be empty".to_string());
    }
    if RESERVED_NICKNAMES.contains(&nickname.as_str()) {
        return Err(format!("shadow nickname '{nickname}' is reserved"));
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

        if config.shadows.contains_key(&nickname) {
            return Err(format!(
                "shadow '{nickname}' already exists in {}",
                ctx.config_path.display()
            ));
        }
        for (other, shadow) in &config.shadows {
            if shadow.mapping.trim_end_matches('/') == mapping.trim_end_matches('/') {
                return Err(format!(
                    "mapping '{mapping}' is already used by shadow '{other}'"
                ));
            }
        }
    }

    match sync_action(&target) {
        SyncAction::Clone => {}
        SyncAction::AlreadyPresent => {
            return Err(format!(
                "{} already contains a git repository",
                target.display()
            ));
        }
        SyncAction::NotARepo => {
            return Err(format!("{} exists and is not empty", target.display()));
        }
        SyncAction::NotADirectory => {
            return Err(format!(
                "{} exists and is not a directory",
                target.display()
            ));
        }
    }

    let snippet = shadow_config_snippet(&nickname, repo, &mapping);
    let new_content = config_with_shadow(existing.as_deref(), &snippet);
    parse_config(&new_content).map_err(|e| format!("refusing to write an invalid config: {e}"))?;

    println!("{nickname}: cloning {repo} into {mapping}");
    let status = process::Command::new("git")
        .args(["clone", repo])
        .arg(&target)
        .status()
        .map_err(|e| format!("failed to run git clone: {e}"))?;
    if !status.success() {
        return Err("git clone failed".to_string());
    }

    if let Some(config_dir) = ctx.config_path.parent() {
        std::fs::create_dir_all(config_dir)
            .map_err(|e| format!("failed to create {}: {e}", config_dir.display()))?;
    }
    std::fs::write(&ctx.config_path, new_content)
        .map_err(|e| format!("failed to write {}: {e}", ctx.config_path.display()))?;

    println!(
        "registered shadow '{nickname}' with mapping '{mapping}' in {}",
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
                "shadow was cloned and registered, but updating git exclude failed: {e}"
            ));
        }
    }
    Ok(())
}
