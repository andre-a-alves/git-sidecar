use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process;

use serde::Deserialize;

const CONFIG_DIR_NAME: &str = "git-shadow";
const CONFIG_FILE_NAME: &str = "config.toml";
const CONFIG_VERSION: u32 = 1;

/// Parsed representation of a v1 git-shadow config file.
#[derive(Debug, Deserialize)]
struct Config {
    shadows: HashMap<String, Shadow>,
}

/// A single shadow entry from the v1 config.
#[derive(Debug, Deserialize)]
struct Shadow {
    /// Remote URL for the shadow repository.
    repo: String,
    /// Path to the shadow git repository, relative to the parent repo root.
    mapping: String,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    version: Option<u32>,
    shadows: HashMap<String, Shadow>,
}

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

    let parent_repo = nearest_git_repo(&cwd).unwrap_or_else(|e| {
        eprintln!("git-shad: {e}");
        process::exit(1);
    });

    let origin_url = remote_origin_url(&parent_repo).unwrap_or_else(|e| {
        eprintln!("git-shad: {e}");
        process::exit(1);
    });

    let config_path = config_path_for_origin(&origin_url).unwrap_or_else(|e| {
        eprintln!("git-shad: {e}");
        process::exit(1);
    });

    let config = read_config(&config_path).unwrap_or_else(|e| {
        eprintln!("git-shad: {e}");
        process::exit(1);
    });

    let shadow = config.shadows.get(shadow_name).unwrap_or_else(|| {
        eprintln!(
            "git-shad: shadow '{shadow_name}' not found in {}",
            config_path.display()
        );
        process::exit(1);
    });

    let shadow_dir = parent_repo.join(&shadow.mapping);

    let status = process::Command::new("git")
        .args(git_args)
        .current_dir(&shadow_dir)
        .status()
        .unwrap_or_else(|e| {
            eprintln!(
                "git-shad: failed to run git in {}: {e}",
                shadow_dir.display()
            );
            process::exit(1);
        });

    process::exit(status.code().unwrap_or(1));
}

fn nearest_git_repo(start: &Path) -> Result<PathBuf, String> {
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

fn remote_origin_url(repo: &Path) -> Result<String, String> {
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
            "{} has no remote.origin.url; git-shadow v1 config lookup requires one",
            repo.display()
        ));
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return Err(format!(
            "{} has an empty remote.origin.url; git-shadow v1 config lookup requires one",
            repo.display()
        ));
    }

    Ok(url)
}

fn config_path_for_origin(origin_url: &str) -> Result<PathBuf, String> {
    let mut path = platform_config_dir()?.join(CONFIG_DIR_NAME);
    path.push(normalize_remote_url(origin_url)?);
    path.push(CONFIG_FILE_NAME);
    Ok(path)
}

fn platform_config_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = env::var_os("APPDATA").filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(appdata));
        }
        return Err("APPDATA is not set; cannot locate the Windows config directory".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(home)
                .join("Library")
                .join("Application Support"));
        }
        return Err("HOME is not set; cannot locate the macOS config directory".to_string());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(xdg_config_home) =
            env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty())
        {
            return Ok(PathBuf::from(xdg_config_home));
        }

        if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(home).join(".config"));
        }

        Err(
            "neither XDG_CONFIG_HOME nor HOME is set; cannot locate the config directory"
                .to_string(),
        )
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(home).join(".config"));
        }

        Err("HOME is not set; cannot locate the config directory".to_string())
    }
}

fn normalize_remote_url(remote_url: &str) -> Result<PathBuf, String> {
    let trimmed = remote_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("remote.origin.url is empty".to_string());
    }

    let (host, repo_path) = if let Some((_, after_scheme)) = trimmed.split_once("://") {
        parse_scheme_url(after_scheme)?
    } else if let Some((host_part, path_part)) = trimmed.split_once(':') {
        parse_scp_like_url(host_part, path_part)?
    } else {
        return Err(format!(
            "unsupported remote.origin.url '{remote_url}'; expected SSH or HTTPS-style Git URL"
        ));
    };

    remote_path(host, repo_path).ok_or_else(|| {
        format!("unsupported remote.origin.url '{remote_url}'; could not derive config path")
    })
}

fn parse_scheme_url(after_scheme: &str) -> Result<(&str, &str), String> {
    let (authority, path) = after_scheme
        .split_once('/')
        .ok_or_else(|| "remote URL is missing a repository path".to_string())?;

    let host_with_optional_port = strip_userinfo(authority);
    let host = host_with_optional_port
        .split_once(':')
        .map_or(host_with_optional_port, |(host, _)| host);

    Ok((host, path))
}

fn parse_scp_like_url<'a>(
    host_part: &'a str,
    path_part: &'a str,
) -> Result<(&'a str, &'a str), String> {
    if host_part.contains('/') {
        return Err("local-path remotes are not supported for config lookup".to_string());
    }

    let host = strip_userinfo(host_part);
    Ok((host, path_part))
}

fn strip_userinfo(authority: &str) -> &str {
    authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
}

fn remote_path(host: &str, repo_path: &str) -> Option<PathBuf> {
    let host = host.trim();
    if host.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    parts.push(host.to_string());

    let mut path_parts: Vec<String> = repo_path
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();

    if path_parts.is_empty() {
        return None;
    }

    if let Some(last) = path_parts.last_mut() {
        if let Some(stripped) = last.strip_suffix(".git") {
            *last = stripped.to_string();
        }
    }

    parts.extend(path_parts);

    if parts
        .iter()
        .any(|part| part.is_empty() || part == "." || part == ".." || part.contains('\\'))
    {
        return None;
    }

    let mut path = PathBuf::new();
    for part in parts {
        path.push(part);
    }
    Some(path)
}

fn read_config(config_path: &Path) -> Result<Config, String> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("failed to read {}: {e}", config_path.display()))?;

    parse_config(&content).map_err(|e| format!("failed to parse {}: {e}", config_path.display()))
}

fn parse_config(content: &str) -> Result<Config, String> {
    let raw: RawConfig = toml::from_str(content).map_err(|e| e.to_string())?;

    let version = raw
        .version
        .ok_or_else(|| format!("missing required top-level version = {CONFIG_VERSION}"))?;

    if version != CONFIG_VERSION {
        return Err(format!(
            "unsupported config version {version}; expected {CONFIG_VERSION}"
        ));
    }

    for (nickname, shadow) in &raw.shadows {
        if nickname.trim().is_empty() {
            return Err("shadow nickname cannot be empty".to_string());
        }
        if shadow.repo.trim().is_empty() {
            return Err(format!("shadow '{nickname}' has an empty repo"));
        }
        if shadow.mapping.trim().is_empty() {
            return Err(format!("shadow '{nickname}' has an empty mapping"));
        }
        if Path::new(&shadow.mapping).is_absolute() {
            return Err(format!("shadow '{nickname}' mapping must be relative"));
        }
    }

    Ok(Config {
        shadows: raw.shadows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_TOML: &str = r#"
version = 1

[shadows.cardlet]
repo = "git@github.com:andre-a-alves/cardlet.git"
mapping = ".test/"
"#;

    #[test]
    fn parses_config() {
        let config = parse_config(EXAMPLE_TOML).unwrap();
        assert_eq!(config.shadows.len(), 1);

        let shadow = config.shadows.get("cardlet").unwrap();
        assert_eq!(shadow.repo, "git@github.com:andre-a-alves/cardlet.git");
        assert_eq!(shadow.mapping, ".test/");
    }

    #[test]
    fn finds_shadow_by_nickname() {
        let config = parse_config(EXAMPLE_TOML).unwrap();
        assert!(config.shadows.contains_key("cardlet"));
        assert!(!config.shadows.contains_key("nonexistent"));
    }

    #[test]
    fn missing_version_is_an_error() {
        let err = parse_config(
            r#"
[shadows.cardlet]
repo = "git@github.com:andre-a-alves/cardlet.git"
mapping = ".test/"
"#,
        )
        .unwrap_err();

        assert!(err.contains("missing required top-level version"));
    }

    #[test]
    fn unsupported_version_is_an_error() {
        let err = parse_config(
            r#"
version = 2

[shadows.cardlet]
repo = "git@github.com:andre-a-alves/cardlet.git"
mapping = ".test/"
"#,
        )
        .unwrap_err();

        assert!(err.contains("unsupported config version 2"));
    }

    #[test]
    fn empty_shadow_repo_is_an_error() {
        let err = parse_config(
            r#"
version = 1

[shadows.cardlet]
repo = ""
mapping = ".test/"
"#,
        )
        .unwrap_err();

        assert!(err.contains("shadow 'cardlet' has an empty repo"));
    }

    #[test]
    fn absolute_shadow_mapping_is_an_error() {
        let err = parse_config(
            r#"
version = 1

[shadows.cardlet]
repo = "git@github.com:andre-a-alves/cardlet.git"
mapping = "/tmp/cardlet"
"#,
        )
        .unwrap_err();

        assert!(err.contains("shadow 'cardlet' mapping must be relative"));
    }

    #[test]
    fn normalizes_scp_like_ssh_url() {
        assert_eq!(
            normalize_remote_url("git@github.com:andre-a-alves/git-shadow.git").unwrap(),
            PathBuf::from("github.com/andre-a-alves/git-shadow")
        );
    }

    #[test]
    fn normalizes_ssh_scheme_url() {
        assert_eq!(
            normalize_remote_url("ssh://git@github.com/andre-a-alves/git-shadow.git").unwrap(),
            PathBuf::from("github.com/andre-a-alves/git-shadow")
        );
    }

    #[test]
    fn normalizes_https_url() {
        assert_eq!(
            normalize_remote_url("https://github.com/andre-a-alves/git-shadow.git").unwrap(),
            PathBuf::from("github.com/andre-a-alves/git-shadow")
        );
    }

    #[test]
    fn normalizes_https_url_without_dot_git() {
        assert_eq!(
            normalize_remote_url("https://github.com/andre-a-alves/git-shadow").unwrap(),
            PathBuf::from("github.com/andre-a-alves/git-shadow")
        );
    }

    #[test]
    fn rejects_local_path_remote() {
        let err = normalize_remote_url("/home/andre/repo.git").unwrap_err();
        assert!(err.contains("unsupported remote.origin.url"));
    }

    #[test]
    fn resolves_config_path_under_git_shadow_dir() {
        let path = config_path_for_origin("git@github.com:andre-a-alves/git-shadow.git").unwrap();
        assert!(path.ends_with("git-shadow/github.com/andre-a-alves/git-shadow/config.toml"));
    }
}
