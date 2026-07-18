use std::process::{self, ExitCode};

use crate::config::{RepoContext, read_config};

/// Runs an arbitrary git command inside the named sidecar's directory,
/// propagating git's exit code. `args[0]` is the sidecar nickname; the
/// remaining arguments are the git command.
pub fn run(args: &[String]) -> ExitCode {
    let [sidecar_name, git_args @ ..] = args else {
        eprintln!("usage: git sidecar <sidecar-name> <git-command> [args...]");
        return ExitCode::FAILURE;
    };
    if git_args.is_empty() {
        eprintln!("usage: git sidecar <sidecar-name> <git-command> [args...]");
        return ExitCode::FAILURE;
    }

    match exec(sidecar_name, git_args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("git-sidecar: {e}");
            ExitCode::FAILURE
        }
    }
}

fn exec(sidecar_name: &str, git_args: &[String]) -> Result<ExitCode, String> {
    let ctx = RepoContext::discover()?;
    let config = read_config(&ctx.config_path)?;

    let sidecar = config.sidecars.get(sidecar_name).ok_or_else(|| {
        format!(
            "sidecar '{sidecar_name}' not found in {}",
            ctx.config_path.display()
        )
    })?;

    let sidecar_dir = ctx.parent_repo.join(&sidecar.mapping);

    let status = process::Command::new("git")
        .args(git_args)
        .current_dir(&sidecar_dir)
        .status()
        .map_err(|e| format!("failed to run git in {}: {e}", sidecar_dir.display()))?;

    Ok(match status.code() {
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    })
}
