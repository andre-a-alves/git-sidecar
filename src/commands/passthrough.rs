use std::collections::HashMap;
use std::path::Path;
use std::process::{self, ExitCode};

use crate::config::{RepoContext, Sidecar, read_config};
use crate::layout::{external_gitdir, is_valid_gitdir};
use crate::paths::normalize_lexically;

const USAGE: &str = "usage: git sidecar [<sidecar-name>] <git-command> [args...]\n\
                     (the name may be omitted when run from inside a sidecar's directory)";

/// Runs an arbitrary git command against a sidecar, propagating git's exit
/// code. `args[0]` is either a sidecar nickname (the remaining arguments
/// are the git command) or, when run from inside a sidecar's mapping
/// directory, the start of the git command itself.
pub fn run(args: &[String]) -> ExitCode {
    match exec(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("git-sidecar: {e}");
            ExitCode::FAILURE
        }
    }
}

fn exec(args: &[String]) -> Result<ExitCode, String> {
    let [first, rest @ ..] = args else {
        return Err(USAGE.to_string());
    };

    let ctx = RepoContext::discover()?;
    if !ctx.config_path.exists() {
        return Err(format!(
            "sidecar '{first}' not found; no config at {}\n{USAGE}",
            ctx.config_path.display()
        ));
    }
    let config = read_config(&ctx.config_path)?;

    let (name, sidecar, git_args) = match config.sidecars.get_key_value(first.as_str()) {
        // explicit name always wins, regardless of cwd
        Some((name, sidecar)) => {
            if rest.is_empty() {
                return Err(USAGE.to_string());
            }
            (name.as_str(), sidecar, rest)
        }
        None => match sidecar_for_cwd(&ctx.cwd, &ctx.parent_repo, &config.sidecars) {
            Some((name, sidecar)) => (name, sidecar, args),
            None => {
                return Err(format!(
                    "sidecar '{first}' not found in {} and the current directory is not inside a sidecar\n{USAGE}",
                    ctx.config_path.display()
                ));
            }
        },
    };

    let sidecar_dir = ctx.parent_repo.join(&sidecar.mapping);
    if !sidecar_dir.is_dir() {
        return Err(format!(
            "sidecar '{name}' is not present at {}; run 'git sidecar sync'",
            sidecar_dir.display()
        ));
    }

    let mut cmd = process::Command::new("git");
    if !sidecar_dir.join(".git").exists() {
        // unified layout: point git at the relocated git dir explicitly
        let gitdir = external_gitdir(&ctx.parent_repo, name)?;
        if !is_valid_gitdir(&gitdir) {
            return Err(format!(
                "sidecar '{name}' has no git directory: neither {} nor {} exists; run 'git sidecar sync'",
                sidecar_dir.join(".git").display(),
                gitdir.display()
            ));
        }
        cmd.arg("--git-dir")
            .arg(&gitdir)
            .arg("--work-tree")
            .arg(&sidecar_dir);
    }

    let status = cmd
        .args(git_args)
        .current_dir(&sidecar_dir)
        .status()
        .map_err(|e| format!("failed to run git in {}: {e}", sidecar_dir.display()))?;

    Ok(match status.code() {
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    })
}

/// The configured sidecar whose mapping directory contains `cwd`, if any.
/// The deepest matching mapping wins, so behavior stays sane even if
/// mappings nest.
fn sidecar_for_cwd<'a>(
    cwd: &Path,
    parent_repo: &Path,
    sidecars: &'a HashMap<String, Sidecar>,
) -> Option<(&'a str, &'a Sidecar)> {
    let cwd = normalize_lexically(cwd);
    let parent = normalize_lexically(parent_repo);
    if !cwd.starts_with(&parent) {
        return None;
    }

    sidecars
        .iter()
        .filter_map(|(name, sidecar)| {
            let mapping_dir = normalize_lexically(&parent.join(&sidecar.mapping));
            cwd.starts_with(&mapping_dir)
                .then(|| (name.as_str(), sidecar, mapping_dir.components().count()))
        })
        .max_by_key(|(_, _, depth)| *depth)
        .map(|(name, sidecar, _)| (name, sidecar))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;

    fn sidecars() -> HashMap<String, Sidecar> {
        parse_config(
            r#"
version = 1

[sidecars.foo]
repo = "git@github.com:example/foo.git"
mapping = ".vendor/foo/"

[sidecars.bar]
repo = "git@github.com:example/bar.git"
mapping = "bar/"
"#,
        )
        .unwrap()
        .sidecars
    }

    const PARENT: &str = "/home/user/project";

    fn resolve(cwd: &str) -> Option<&'static str> {
        // leak keeps the borrowed return value simple for assertions
        let sidecars = Box::leak(Box::new(sidecars()));
        sidecar_for_cwd(Path::new(cwd), Path::new(PARENT), sidecars).map(|(name, _)| name)
    }

    #[test]
    fn cwd_inside_a_mapping_resolves_to_that_sidecar() {
        assert_eq!(resolve("/home/user/project/.vendor/foo"), Some("foo"));
        assert_eq!(
            resolve("/home/user/project/.vendor/foo/src/deep"),
            Some("foo")
        );
        assert_eq!(resolve("/home/user/project/bar"), Some("bar"));
    }

    #[test]
    fn cwd_in_the_parent_outside_any_mapping_resolves_to_none() {
        assert_eq!(resolve("/home/user/project"), None);
        assert_eq!(resolve("/home/user/project/src"), None);
        assert_eq!(resolve("/home/user/project/.vendor"), None);
    }

    #[test]
    fn cwd_outside_the_parent_resolves_to_none() {
        assert_eq!(resolve("/home/user/elsewhere/bar"), None);
        assert_eq!(resolve("/home/user"), None);
    }

    #[test]
    fn sibling_directory_with_mapping_prefix_does_not_match() {
        // "barbell" starts with "bar" as a string but not as a path
        assert_eq!(resolve("/home/user/project/barbell"), None);
    }

    #[test]
    fn deepest_mapping_wins_when_mappings_nest() {
        let mut sidecars = sidecars();
        sidecars.insert(
            "nested".to_string(),
            Sidecar {
                repo: "url".to_string(),
                mapping: ".vendor/foo/nested/".to_string(),
                standalone: false,
            },
        );

        let hit = sidecar_for_cwd(
            Path::new("/home/user/project/.vendor/foo/nested/src"),
            Path::new(PARENT),
            &sidecars,
        );
        assert_eq!(hit.map(|(name, _)| name), Some("nested"));
    }
}
