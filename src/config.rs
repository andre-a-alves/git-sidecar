use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::git::{nearest_git_repo, remote_origin_url};
use crate::paths::is_portable_relative_path;
use crate::remote::normalize_remote_url;

pub const CONFIG_DIR_NAME: &str = "git-sidecar";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const CONFIG_VERSION: u32 = 1;
pub const RESERVED_NICKNAMES: [&str; 6] = ["list", "sync", "clone", "remove", "rm", "help"];

/// Parsed representation of a v1 git-sidecar config file.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub sidecars: HashMap<String, Sidecar>,
}

/// A single sidecar entry from the v1 config.
#[derive(Debug, Deserialize)]
pub struct Sidecar {
    /// Remote URL for the sidecar repository.
    pub repo: String,
    /// Path to the sidecar git repository, relative to the parent repo root.
    pub mapping: String,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    version: Option<u32>,
    #[serde(default)]
    sidecars: HashMap<String, Sidecar>,
}

/// Everything needed to work with the current parent repo: its root, its
/// origin URL, and the config file that origin maps to.
pub struct RepoContext {
    pub cwd: PathBuf,
    pub parent_repo: PathBuf,
    pub origin_url: String,
    pub config_path: PathBuf,
}

impl RepoContext {
    /// Resolves the context from the current working directory.
    ///
    /// Both paths are canonicalized so they can be compared lexically:
    /// on Windows the working directory may come back in DOS 8.3 short
    /// form (`RUNNER~1`) while git reports the long form, which would
    /// otherwise make `strip_prefix`-based checks fail.
    pub fn discover() -> Result<Self, String> {
        let cwd =
            env::current_dir().map_err(|e| format!("failed to get current directory: {e}"))?;
        let cwd = dunce::canonicalize(&cwd).unwrap_or(cwd);
        let parent_repo = nearest_git_repo(&cwd)?;
        let parent_repo = dunce::canonicalize(&parent_repo).unwrap_or(parent_repo);
        let origin_url = remote_origin_url(&parent_repo)?;
        let config_path = config_path_for_origin(&origin_url)?;
        Ok(RepoContext {
            cwd,
            parent_repo,
            origin_url,
            config_path,
        })
    }
}

pub fn config_path_for_origin(origin_url: &str) -> Result<PathBuf, String> {
    let mut path = platform_config_dir()?.join(CONFIG_DIR_NAME);
    path.push(normalize_remote_url(origin_url)?);
    path.push(CONFIG_FILE_NAME);
    Ok(path)
}

pub fn platform_config_dir() -> Result<PathBuf, String> {
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

pub fn read_config(config_path: &Path) -> Result<Config, String> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("failed to read {}: {e}", config_path.display()))?;

    parse_config(&content).map_err(|e| format!("failed to parse {}: {e}", config_path.display()))
}

pub fn parse_config(content: &str) -> Result<Config, String> {
    let raw: RawConfig = toml::from_str(content).map_err(|e| e.to_string())?;

    let version = raw
        .version
        .ok_or_else(|| format!("missing required top-level version = {CONFIG_VERSION}"))?;

    if version != CONFIG_VERSION {
        return Err(format!(
            "unsupported config version {version}; expected {CONFIG_VERSION}"
        ));
    }

    for (nickname, sidecar) in &raw.sidecars {
        if nickname.trim().is_empty() {
            return Err("sidecar nickname cannot be empty".to_string());
        }
        if RESERVED_NICKNAMES.contains(&nickname.as_str()) {
            return Err(format!("sidecar nickname '{nickname}' is reserved"));
        }
        if sidecar.repo.trim().is_empty() {
            return Err(format!("sidecar '{nickname}' has an empty repo"));
        }
        if sidecar.mapping.trim().is_empty() {
            return Err(format!("sidecar '{nickname}' has an empty mapping"));
        }
        if !is_portable_relative_path(&sidecar.mapping) {
            return Err(format!("sidecar '{nickname}' mapping must be relative"));
        }
    }

    Ok(Config {
        sidecars: raw.sidecars,
    })
}

pub fn sidecar_config_snippet(name: &str, repo: &str, mapping: &str) -> String {
    format!(
        "[sidecars.{}]\nrepo = {}\nmapping = {}\n",
        toml_key(name),
        toml_string(repo),
        toml_string(mapping)
    )
}

/// Appends a sidecar snippet to an existing config's text (preserving
/// whatever formatting it has), or starts a fresh v1 config.
pub fn config_with_sidecar(existing: Option<&str>, snippet: &str) -> String {
    match existing {
        Some(content) => {
            let mut out = content.trim_end().to_string();
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(snippet);
            out
        }
        None => format!("version = {CONFIG_VERSION}\n\n{snippet}"),
    }
}

/// Removes a sidecar's table from the config's text, preserving everything
/// else. Fails if the table's header cannot be located textually.
pub fn config_without_sidecar(content: &str, name: &str) -> Result<String, String> {
    let lines: Vec<&str> = content.lines().collect();

    let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
    let header_forms = [
        format!("[sidecars.{name}]"),
        format!("[sidecars.\"{escaped}\"]"),
        format!("[sidecars.'{name}']"),
    ];
    let start = lines
        .iter()
        .position(|line| header_forms.iter().any(|form| line.trim() == form))
        .ok_or_else(|| {
            format!(
                "could not locate the [sidecars.{name}] entry in the config; remove it manually"
            )
        })?;
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |offset| start + 1 + offset);

    let mut kept: Vec<&str> = Vec::new();
    kept.extend(&lines[..start]);
    kept.extend(&lines[end..]);

    let mut out = kept.join("\n");
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    let out = out.trim_matches('\n');
    if out.is_empty() {
        return Ok(String::new());
    }
    Ok(format!("{out}\n"))
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

fn toml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if bare {
        key.to_string()
    } else {
        toml_string(key)
    }
}

#[cfg(test)]
pub const EXAMPLE_TOML: &str = r#"
version = 1

[sidecars.cardlet]
repo = "git@github.com:andre-a-alves/cardlet.git"
mapping = ".test/"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config() {
        let config = parse_config(EXAMPLE_TOML).unwrap();
        assert_eq!(config.sidecars.len(), 1);

        let sidecar = config.sidecars.get("cardlet").unwrap();
        assert_eq!(sidecar.repo, "git@github.com:andre-a-alves/cardlet.git");
        assert_eq!(sidecar.mapping, ".test/");
    }

    #[test]
    fn finds_sidecar_by_nickname() {
        let config = parse_config(EXAMPLE_TOML).unwrap();
        assert!(config.sidecars.contains_key("cardlet"));
        assert!(!config.sidecars.contains_key("nonexistent"));
    }

    #[test]
    fn missing_version_is_an_error() {
        let err = parse_config(
            r#"
[sidecars.cardlet]
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

[sidecars.cardlet]
repo = "git@github.com:andre-a-alves/cardlet.git"
mapping = ".test/"
"#,
        )
        .unwrap_err();

        assert!(err.contains("unsupported config version 2"));
    }

    #[test]
    fn empty_sidecar_repo_is_an_error() {
        let err = parse_config(
            r#"
version = 1

[sidecars.cardlet]
repo = ""
mapping = ".test/"
"#,
        )
        .unwrap_err();

        assert!(err.contains("sidecar 'cardlet' has an empty repo"));
    }

    #[test]
    fn absolute_sidecar_mapping_is_an_error() {
        let err = parse_config(
            r#"
version = 1

[sidecars.cardlet]
repo = "git@github.com:andre-a-alves/cardlet.git"
mapping = "/tmp/cardlet"
"#,
        )
        .unwrap_err();

        assert!(err.contains("sidecar 'cardlet' mapping must be relative"));
    }

    #[test]
    fn windows_absolute_sidecar_mapping_is_an_error() {
        let err = parse_config(
            r#"
version = 1

[sidecars.cardlet]
repo = "git@github.com:andre-a-alves/cardlet.git"
mapping = "C:\\tmp\\cardlet"
"#,
        )
        .unwrap_err();

        assert!(err.contains("sidecar 'cardlet' mapping must be relative"));
    }

    #[test]
    fn reserved_nicknames_are_errors() {
        for nickname in RESERVED_NICKNAMES {
            let err = parse_config(&format!(
                r#"
version = 1

[sidecars.{nickname}]
repo = "git@github.com:example/x.git"
mapping = ".vendor/x/"
"#
            ))
            .unwrap_err();

            assert!(err.contains(&format!("sidecar nickname '{nickname}' is reserved")));
        }
    }

    #[test]
    fn version_only_config_parses_with_no_sidecars() {
        let config = parse_config("version = 1\n").unwrap();
        assert!(config.sidecars.is_empty());
    }

    #[test]
    fn resolves_config_path_under_git_sidecar_dir() {
        let path = config_path_for_origin("git@github.com:andre-a-alves/git-sidecar.git").unwrap();
        assert!(path.ends_with("git-sidecar/github.com/andre-a-alves/git-sidecar/config.toml"));
    }

    #[test]
    fn new_config_with_sidecar_parses() {
        let snippet =
            sidecar_config_snippet("foobar", "git@github.com:example/foobar.git", "foobar/");
        let content = config_with_sidecar(None, &snippet);

        let config = parse_config(&content).unwrap();
        let sidecar = config.sidecars.get("foobar").unwrap();
        assert_eq!(sidecar.repo, "git@github.com:example/foobar.git");
        assert_eq!(sidecar.mapping, "foobar/");
    }

    #[test]
    fn appended_config_keeps_existing_sidecars() {
        let snippet =
            sidecar_config_snippet("foobar", "git@github.com:example/foobar.git", "foobar/");
        let content = config_with_sidecar(Some(EXAMPLE_TOML), &snippet);

        let config = parse_config(&content).unwrap();
        assert_eq!(config.sidecars.len(), 2);
        assert!(config.sidecars.contains_key("cardlet"));
        assert!(config.sidecars.contains_key("foobar"));
    }

    #[test]
    fn config_snippet_escapes_special_characters() {
        let snippet = sidecar_config_snippet("has spaces", "url\"with\"quotes", "dir/");
        let content = config_with_sidecar(None, &snippet);

        let config = parse_config(&content).unwrap();
        let sidecar = config.sidecars.get("has spaces").unwrap();
        assert_eq!(sidecar.repo, "url\"with\"quotes");
    }

    #[test]
    fn removes_sidecar_table_and_keeps_the_rest() {
        let content = "\
version = 1

# keep me
[sidecars.cardlet]
repo = \"git@github.com:andre-a-alves/cardlet.git\"
mapping = \".test/\"

[sidecars.foobar]
repo = \"git@github.com:example/foobar.git\"
mapping = \".vendor/foobar/\"
";

        let out = config_without_sidecar(content, "cardlet").unwrap();
        let config = parse_config(&out).unwrap();

        assert_eq!(config.sidecars.len(), 1);
        assert!(config.sidecars.contains_key("foobar"));
        assert!(out.contains("# keep me"));
        assert!(!out.contains("cardlet"));
    }

    #[test]
    fn removes_last_sidecar_leaving_version_only() {
        let out = config_without_sidecar(EXAMPLE_TOML, "cardlet").unwrap();

        assert_eq!(out.trim(), "version = 1");
        let config = parse_config(&out).unwrap();
        assert!(config.sidecars.is_empty());
    }

    #[test]
    fn removes_quoted_key_sidecar_table() {
        let snippet = sidecar_config_snippet("has spaces", "url", "dir/");
        let content = config_with_sidecar(Some(EXAMPLE_TOML), &snippet);

        let out = config_without_sidecar(&content, "has spaces").unwrap();
        let config = parse_config(&out).unwrap();

        assert_eq!(config.sidecars.len(), 1);
        assert!(config.sidecars.contains_key("cardlet"));
    }

    #[test]
    fn unlocatable_sidecar_table_is_an_error() {
        let err = config_without_sidecar(EXAMPLE_TOML, "missing").unwrap_err();
        assert!(err.contains("remove it manually"));
    }
}
