use std::process::{self, ExitCode};

use crate::config::{RepoContext, read_config};

/// Runs an arbitrary git command inside the named shadow's directory,
/// propagating git's exit code. `args[0]` is the shadow nickname; the
/// remaining arguments are the git command.
pub fn run(args: &[String]) -> ExitCode {
    let [shadow_name, git_args @ ..] = args else {
        eprintln!("usage: git shadow <shadow-name> <git-command> [args...]");
        return ExitCode::FAILURE;
    };
    if git_args.is_empty() {
        eprintln!("usage: git shadow <shadow-name> <git-command> [args...]");
        return ExitCode::FAILURE;
    }

    match exec(shadow_name, git_args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("git-shadow: {e}");
            ExitCode::FAILURE
        }
    }
}

fn exec(shadow_name: &str, git_args: &[String]) -> Result<ExitCode, String> {
    let ctx = RepoContext::discover()?;
    let config = read_config(&ctx.config_path)?;

    let shadow = config.shadows.get(shadow_name).ok_or_else(|| {
        format!(
            "shadow '{shadow_name}' not found in {}",
            ctx.config_path.display()
        )
    })?;

    let shadow_dir = ctx.parent_repo.join(&shadow.mapping);

    let status = process::Command::new("git")
        .args(git_args)
        .current_dir(&shadow_dir)
        .status()
        .map_err(|e| format!("failed to run git in {}: {e}", shadow_dir.display()))?;

    Ok(match status.code() {
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    })
}
