use std::env;
use std::path::{Path, PathBuf};
use std::process;

use serde::Deserialize;

/// Parsed representation of a `.gitshadow.toml` file.
#[derive(Deserialize)]
struct Config {
    shadows: Vec<Shadow>,
}

/// A single shadow entry from `.gitshadow.toml`.
#[derive(Deserialize)]
struct Shadow {
    /// The alias used on the command line (`git shad <name> ...`).
    name: String,
    /// Path to the shadow git repository, relative to `.gitshadow.toml`.
    mapping: String,
}

/// Walks up the directory tree from `start`, returning the path to the first
/// `.gitshadow.toml` found, or `None` if the filesystem root is reached.
fn find_config(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join(".gitshadow.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Entry point. Parses CLI arguments, locates `.gitshadow.toml`, resolves the
/// named shadow, and delegates to `git` running inside the shadow directory.
/// Exits with git's own exit code, or 1 on any configuration error.
fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("usage: git shad <shadow-name> <git-command> [args...]");
        process::exit(1);
    }

    let shadow_name = &args[1];
    let git_args = &args[2..];

    let cwd = env::current_dir().unwrap_or_else(|e| {
        eprintln!("git-shad: failed to get current directory: {e}");
        process::exit(1);
    });

    let config_path = find_config(&cwd).unwrap_or_else(|| {
        eprintln!("git-shad: .gitshadow.toml not found in current directory or any parent");
        process::exit(1);
    });

    let config_dir = config_path.parent().unwrap();

    let content = std::fs::read_to_string(&config_path).unwrap_or_else(|e| {
        eprintln!("git-shad: failed to read {}: {e}", config_path.display());
        process::exit(1);
    });

    let config: Config = toml::from_str(&content).unwrap_or_else(|e| {
        eprintln!("git-shad: failed to parse .gitshadow.toml: {e}");
        process::exit(1);
    });

    let shadow = config
        .shadows
        .iter()
        .find(|s| s.name == *shadow_name)
        .unwrap_or_else(|| {
            eprintln!("git-shad: shadow '{shadow_name}' not found in .gitshadow.toml");
            process::exit(1);
        });

    let shadow_dir = config_dir.join(&shadow.mapping);

    let status = process::Command::new("git")
        .args(git_args)
        .current_dir(&shadow_dir)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("git-shad: failed to run git: {e}");
            process::exit(1);
        });

    process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_TOML: &str = r#"
[[shadows]]
name = "cardlet"
repo = "git@github.com:andre-a-alves/cardlet.git"
mapping = ".test/"
"#;

    #[test]
    fn parses_config() {
        let config: Config = toml::from_str(EXAMPLE_TOML).unwrap();
        assert_eq!(config.shadows.len(), 1);
        assert_eq!(config.shadows[0].name, "cardlet");
        assert_eq!(config.shadows[0].mapping, ".test/");
    }

    #[test]
    fn finds_shadow_by_name() {
        let config: Config = toml::from_str(EXAMPLE_TOML).unwrap();
        assert!(config.shadows.iter().any(|s| s.name == "cardlet"));
    }

    #[test]
    fn missing_shadow_returns_none() {
        let config: Config = toml::from_str(EXAMPLE_TOML).unwrap();
        assert!(
            config
                .shadows
                .iter()
                .find(|s| s.name == "nonexistent")
                .is_none()
        );
    }

    #[test]
    fn find_config_returns_none_when_not_found() {
        assert!(find_config(Path::new("/tmp")).is_none());
    }
}
