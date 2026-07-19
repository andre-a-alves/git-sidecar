use std::process::ExitCode;

use crate::config::{RepoContext, config_without_sidecar, parse_config};
use crate::exclude::{exclude_entry, remove_mapping_exclusion};
use crate::layout::{external_gitdir, is_valid_gitdir, relocate_gitdir_in, remove_external_dirs};
use crate::paths::normalize_lexically;

pub fn run(name: &str, delete: bool) -> ExitCode {
    match remove_sidecar(name, delete) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("git-sidecar: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Removes a sidecar from the config and the exclude file. The mapping
/// directory is left on disk unless `delete` is set.
fn remove_sidecar(name: &str, delete: bool) -> Result<(), String> {
    let ctx = RepoContext::discover()?;

    if !ctx.config_path.exists() {
        return Err(format!(
            "sidecar '{name}' not found; no config at {}",
            ctx.config_path.display()
        ));
    }
    let content = std::fs::read_to_string(&ctx.config_path)
        .map_err(|e| format!("failed to read {}: {e}", ctx.config_path.display()))?;
    let config = parse_config(&content)
        .map_err(|e| format!("failed to parse {}: {e}", ctx.config_path.display()))?;

    let sidecar = config.sidecars.get(name).ok_or_else(|| {
        format!(
            "sidecar '{name}' not found in {}",
            ctx.config_path.display()
        )
    })?;
    let mapping = sidecar.mapping.clone();

    let new_content = config_without_sidecar(&content, name)?;
    let new_config = parse_config(&new_content)
        .map_err(|e| format!("refusing to write an invalid config: {e}"))?;
    if new_config.sidecars.len() != config.sidecars.len() - 1
        || new_config.sidecars.contains_key(name)
    {
        return Err(format!(
            "refusing to write config: removal would not drop exactly sidecar '{name}'"
        ));
    }

    std::fs::write(&ctx.config_path, new_content)
        .map_err(|e| format!("failed to write {}: {e}", ctx.config_path.display()))?;
    println!(
        "removed sidecar '{name}' from {}",
        ctx.config_path.display()
    );

    match remove_mapping_exclusion(&ctx.parent_repo, &mapping) {
        Ok(Some(exclude_path)) => {
            println!(
                "removed '{}' from {}",
                exclude_entry(&mapping),
                exclude_path.display()
            );
        }
        Ok(None) => {}
        Err(e) => {
            return Err(format!(
                "sidecar was removed from config, but updating git exclude failed: {e}"
            ));
        }
    }

    let gitdir = external_gitdir(&ctx.parent_repo, name)?;

    if delete {
        let dir = normalize_lexically(&ctx.parent_repo.join(&mapping));
        let parent = normalize_lexically(&ctx.parent_repo);
        if !dir.starts_with(&parent) || dir == parent {
            return Err(format!(
                "refusing to delete {}: it is not inside the parent repository",
                dir.display()
            ));
        }
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| format!("failed to delete {}: {e}", dir.display()))?;
            println!("deleted {}", dir.display());
        }
        if let Some(name_dir) = gitdir.parent() {
            if name_dir.exists() {
                std::fs::remove_dir_all(name_dir)
                    .map_err(|e| format!("failed to delete {}: {e}", name_dir.display()))?;
                println!("deleted {}", name_dir.display());
                remove_external_dirs(&gitdir);
            }
        }
        return Ok(());
    }

    // The directory stays on disk; if it was in the unified layout, move
    // the git dir back inside so what remains is a normal standalone repo.
    let dir = ctx.parent_repo.join(&mapping);
    if dir.is_dir() && !dir.join(".git").exists() && is_valid_gitdir(&gitdir) {
        relocate_gitdir_in(&dir, &gitdir)?;
        println!(
            "moved git dir back into {}; the directory is now a standalone repository",
            dir.display()
        );
    }

    Ok(())
}
