use std::collections::HashMap;
use std::env;
use std::path::{Component, Path, PathBuf};
use std::process;

use serde::Deserialize;

const CONFIG_DIR_NAME: &str = "git-shadow";
const CONFIG_FILE_NAME: &str = "config.toml";
const CONFIG_VERSION: u32 = 1;
const RESERVED_NICKNAMES: [&str; 3] = ["list", "sync", "clone"];

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

    if args.len() >= 2 && args[1] == "list" {
        run_list(&args[2..]);
    }

    if args.len() >= 2 && args[1] == "sync" {
        run_sync(&args[2..]);
    }

    if args.len() >= 2 && args[1] == "clone" {
        run_clone(&args[2..]);
    }

    if args.len() < 3 {
        eprintln!("usage: git shadow <shadow-name> <git-command> [args...]");
        eprintln!("       git shadow list [--global]");
        eprintln!("       git shadow sync [<shadow-name>]");
        eprintln!("       git shadow clone <repo-url> [<directory>] [--name <nickname>]");
        process::exit(1);
    }

    let shadow_name = &args[1];
    let git_args = &args[2..];

    let cwd = env::current_dir().unwrap_or_else(|e| {
        eprintln!("git-shadow: failed to get current directory: {e}");
        process::exit(1);
    });

    let parent_repo = nearest_git_repo(&cwd).unwrap_or_else(|e| {
        eprintln!("git-shadow: {e}");
        process::exit(1);
    });

    let origin_url = remote_origin_url(&parent_repo).unwrap_or_else(|e| {
        eprintln!("git-shadow: {e}");
        process::exit(1);
    });

    let config_path = config_path_for_origin(&origin_url).unwrap_or_else(|e| {
        eprintln!("git-shadow: {e}");
        process::exit(1);
    });

    let config = read_config(&config_path).unwrap_or_else(|e| {
        eprintln!("git-shadow: {e}");
        process::exit(1);
    });

    let shadow = config.shadows.get(shadow_name).unwrap_or_else(|| {
        eprintln!(
            "git-shadow: shadow '{shadow_name}' not found in {}",
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
                "git-shadow: failed to run git in {}: {e}",
                shadow_dir.display()
            );
            process::exit(1);
        });

    process::exit(status.code().unwrap_or(1));
}

fn run_list(args: &[String]) -> ! {
    let global = parse_list_args(args).unwrap_or_else(|e| {
        eprintln!("{e}");
        process::exit(1);
    });

    let result = if global { list_global() } else { list_local() };

    if let Err(e) = result {
        eprintln!("git-shadow: {e}");
        process::exit(1);
    }
    process::exit(0);
}

fn parse_list_args(args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--global" => Ok(true),
        _ => Err("usage: git shadow list [--global]".to_string()),
    }
}

fn list_local() -> Result<(), String> {
    let cwd = env::current_dir().map_err(|e| format!("failed to get current directory: {e}"))?;
    let parent_repo = nearest_git_repo(&cwd)?;
    let origin_url = remote_origin_url(&parent_repo)?;
    let config_path = config_path_for_origin(&origin_url)?;

    if !config_path.exists() {
        println!(
            "no shadows configured for {origin_url} ({})",
            config_path.display()
        );
        return Ok(());
    }

    let config = read_config(&config_path)?;
    if config.shadows.is_empty() {
        println!(
            "no shadows configured for {origin_url} ({})",
            config_path.display()
        );
        return Ok(());
    }

    for line in format_shadow_rows(&sorted_shadow_rows(&config)) {
        println!("{line}");
    }
    Ok(())
}

fn list_global() -> Result<(), String> {
    let root = platform_config_dir()?.join(CONFIG_DIR_NAME);
    let config_files = find_config_files(&root);

    if config_files.is_empty() {
        println!("no shadows configured under {}", root.display());
        return Ok(());
    }

    let mut first = true;
    for config_path in config_files {
        let config = match read_config(&config_path) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("git-shadow: warning: skipping {e}");
                continue;
            }
        };

        let label = config_path
            .parent()
            .and_then(|dir| dir.strip_prefix(&root).ok())
            .map_or_else(
                || config_path.display().to_string(),
                |repo| repo.display().to_string(),
            );

        if !first {
            println!();
        }
        first = false;

        println!("{label}:");
        for line in format_shadow_rows(&sorted_shadow_rows(&config)) {
            println!("  {line}");
        }
    }
    Ok(())
}

fn run_sync(args: &[String]) -> ! {
    let target = parse_sync_args(args).unwrap_or_else(|e| {
        eprintln!("{e}");
        process::exit(1);
    });

    match sync_shadows(target.as_deref()) {
        Ok(true) => process::exit(0),
        Ok(false) => process::exit(1),
        Err(e) => {
            eprintln!("git-shadow: {e}");
            process::exit(1);
        }
    }
}

fn parse_sync_args(args: &[String]) -> Result<Option<String>, String> {
    match args {
        [] => Ok(None),
        [name] if !name.starts_with('-') => Ok(Some(name.clone())),
        _ => Err("usage: git shadow sync [<shadow-name>]".to_string()),
    }
}

/// Syncs shadows for the current repo, cloning any that are not present.
/// Returns Ok(true) when every selected shadow is present or was cloned.
fn sync_shadows(target: Option<&str>) -> Result<bool, String> {
    let cwd = env::current_dir().map_err(|e| format!("failed to get current directory: {e}"))?;
    let parent_repo = nearest_git_repo(&cwd)?;
    let origin_url = remote_origin_url(&parent_repo)?;
    let config_path = config_path_for_origin(&origin_url)?;

    if !config_path.exists() {
        if let Some(name) = target {
            return Err(format!(
                "shadow '{name}' not found; no config at {}",
                config_path.display()
            ));
        }
        println!(
            "no shadows configured for {origin_url} ({})",
            config_path.display()
        );
        return Ok(true);
    }

    let config = read_config(&config_path)?;

    let mut selected: Vec<(&String, &Shadow)> = config
        .shadows
        .iter()
        .filter(|(name, _)| target.is_none_or(|t| t == name.as_str()))
        .collect();
    selected.sort_by(|a, b| a.0.cmp(b.0));

    if selected.is_empty() {
        if let Some(name) = target {
            return Err(format!(
                "shadow '{name}' not found in {}",
                config_path.display()
            ));
        }
        println!(
            "no shadows configured for {origin_url} ({})",
            config_path.display()
        );
        return Ok(true);
    }

    let mut all_ok = true;
    for (name, shadow) in selected {
        if !sync_shadow(name, shadow, &parent_repo) {
            all_ok = false;
        }
    }
    Ok(all_ok)
}

fn sync_shadow(name: &str, shadow: &Shadow, parent_repo: &Path) -> bool {
    let shadow_dir = parent_repo.join(&shadow.mapping);

    match sync_action(&shadow_dir) {
        SyncAction::Clone => {
            println!("{name}: cloning {} into {}", shadow.repo, shadow.mapping);
            let status = process::Command::new("git")
                .args(["clone", &shadow.repo])
                .arg(&shadow_dir)
                .status();
            match status {
                Ok(status) if status.success() => true,
                Ok(_) => {
                    eprintln!("git-shadow: {name}: git clone failed");
                    false
                }
                Err(e) => {
                    eprintln!("git-shadow: {name}: failed to run git clone: {e}");
                    false
                }
            }
        }
        SyncAction::AlreadyPresent => match remote_origin_url(&shadow_dir) {
            Ok(actual) if !same_remote(&actual, &shadow.repo) => {
                eprintln!(
                    "git-shadow: warning: {name}: origin is {actual}, config says {}",
                    shadow.repo
                );
                false
            }
            Ok(_) => {
                println!("{name}: already present");
                true
            }
            Err(_) => {
                eprintln!(
                    "git-shadow: warning: {name}: existing repo in {} has no readable origin",
                    shadow.mapping
                );
                false
            }
        },
        SyncAction::NotARepo => {
            eprintln!(
                "git-shadow: warning: {name}: mapping '{}' exists but is not a git repository; skipping",
                shadow.mapping
            );
            false
        }
        SyncAction::NotADirectory => {
            eprintln!(
                "git-shadow: warning: {name}: mapping '{}' exists but is not a directory; skipping",
                shadow.mapping
            );
            false
        }
    }
}

#[derive(Debug, PartialEq)]
enum SyncAction {
    /// Mapping is missing or an empty directory: clone into it.
    Clone,
    /// Mapping holds a git repository.
    AlreadyPresent,
    /// Mapping is a non-empty directory without a git repository.
    NotARepo,
    /// Mapping exists but is not a directory.
    NotADirectory,
}

fn sync_action(dir: &Path) -> SyncAction {
    if !dir.exists() {
        return SyncAction::Clone;
    }
    if !dir.is_dir() {
        return SyncAction::NotADirectory;
    }
    if dir.join(".git").exists() {
        return SyncAction::AlreadyPresent;
    }
    match std::fs::read_dir(dir) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                SyncAction::Clone
            } else {
                SyncAction::NotARepo
            }
        }
        Err(_) => SyncAction::NotARepo,
    }
}

/// Compares two remote URLs by their normalized host/owner/repo path, so the
/// same repository reached over SSH and HTTPS still matches. Falls back to a
/// literal comparison when either URL cannot be normalized.
fn same_remote(a: &str, b: &str) -> bool {
    match (normalize_remote_url(a), normalize_remote_url(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.trim() == b.trim(),
    }
}

fn run_clone(args: &[String]) -> ! {
    let parsed = parse_clone_args(args).unwrap_or_else(|e| {
        eprintln!("{e}");
        process::exit(1);
    });

    if let Err(e) = clone_shadow(parsed) {
        eprintln!("git-shadow: {e}");
        process::exit(1);
    }
    process::exit(0);
}

#[derive(Debug)]
struct CloneArgs {
    repo: String,
    dir: Option<String>,
    name: Option<String>,
}

fn parse_clone_args(args: &[String]) -> Result<CloneArgs, String> {
    const USAGE: &str = "usage: git shadow clone <repo-url> [<directory>] [--name <nickname>]";

    let mut repo = None;
    let mut dir = None;
    let mut name: Option<String> = None;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--name=") {
            if name.is_some() {
                return Err(USAGE.to_string());
            }
            name = Some(value.to_string());
        } else if arg == "--name" {
            if name.is_some() {
                return Err(USAGE.to_string());
            }
            name = Some(iter.next().ok_or_else(|| USAGE.to_string())?.clone());
        } else if arg.starts_with('-') {
            return Err(USAGE.to_string());
        } else if repo.is_none() {
            repo = Some(arg.clone());
        } else if dir.is_none() {
            dir = Some(arg.clone());
        } else {
            return Err(USAGE.to_string());
        }
    }

    if matches!(&name, Some(n) if n.trim().is_empty()) {
        return Err(USAGE.to_string());
    }

    Ok(CloneArgs {
        repo: repo.ok_or_else(|| USAGE.to_string())?,
        dir,
        name,
    })
}

/// Clones a new shadow repo and registers it in the parent repo's config,
/// creating the config file if it does not exist yet. Refuses to touch
/// anything on nickname/mapping conflicts or a non-empty target directory.
fn clone_shadow(args: CloneArgs) -> Result<(), String> {
    let cwd = env::current_dir().map_err(|e| format!("failed to get current directory: {e}"))?;
    let parent_repo = nearest_git_repo(&cwd)?;
    let origin_url = remote_origin_url(&parent_repo)?;
    let config_path = config_path_for_origin(&origin_url)?;

    let nickname = match args.name {
        Some(name) => name,
        None => repo_name_from_url(&args.repo)?,
    };
    if RESERVED_NICKNAMES.contains(&nickname.as_str()) {
        return Err(format!("shadow nickname '{nickname}' is reserved"));
    }

    let dir = match args.dir {
        Some(dir) => dir,
        None => repo_name_from_url(&args.repo)?,
    };
    let target = cwd.join(&dir);
    let mapping = relative_mapping(&parent_repo, &target)?;

    let existing = if config_path.exists() {
        Some(
            std::fs::read_to_string(&config_path)
                .map_err(|e| format!("failed to read {}: {e}", config_path.display()))?,
        )
    } else {
        None
    };

    if let Some(content) = &existing {
        let config = parse_config(content)
            .map_err(|e| format!("failed to parse {}: {e}", config_path.display()))?;

        if config.shadows.contains_key(&nickname) {
            return Err(format!(
                "shadow '{nickname}' already exists in {}",
                config_path.display()
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

    let snippet = shadow_config_snippet(&nickname, &args.repo, &mapping);
    let new_content = config_with_shadow(existing.as_deref(), &snippet);
    parse_config(&new_content).map_err(|e| format!("refusing to write an invalid config: {e}"))?;

    println!("{nickname}: cloning {} into {mapping}", args.repo);
    let status = process::Command::new("git")
        .args(["clone", &args.repo])
        .arg(&target)
        .status()
        .map_err(|e| format!("failed to run git clone: {e}"))?;
    if !status.success() {
        return Err("git clone failed".to_string());
    }

    if let Some(config_dir) = config_path.parent() {
        std::fs::create_dir_all(config_dir)
            .map_err(|e| format!("failed to create {}: {e}", config_dir.display()))?;
    }
    std::fs::write(&config_path, new_content)
        .map_err(|e| format!("failed to write {}: {e}", config_path.display()))?;

    println!(
        "registered shadow '{nickname}' with mapping '{mapping}' in {}",
        config_path.display()
    );
    Ok(())
}

/// Derives a repository name from a remote URL or local path: the last
/// path segment with any trailing `.git` removed.
fn repo_name_from_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next().unwrap_or_default();
    let name = last.strip_suffix(".git").unwrap_or(last);

    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("cannot derive a repository name from '{url}'"));
    }
    Ok(name.to_string())
}

/// Resolves `target` against the parent repo root and returns the config
/// `mapping` string: forward-slash separated, relative, with a trailing `/`.
fn relative_mapping(parent_repo: &Path, target: &Path) -> Result<String, String> {
    let parent = normalize_lexically(parent_repo);
    let target = normalize_lexically(target);

    let rel = target.strip_prefix(&parent).map_err(|_| {
        format!(
            "target directory {} is outside the parent repository {}",
            target.display(),
            parent.display()
        )
    })?;

    if rel.as_os_str().is_empty() {
        return Err("target directory is the parent repository root".to_string());
    }

    let parts: Vec<String> = rel
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(format!("{}/", parts.join("/")))
}

/// Resolves `.` and `..` components without touching the filesystem, so
/// paths that do not exist yet can still be compared.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn shadow_config_snippet(name: &str, repo: &str, mapping: &str) -> String {
    format!(
        "[shadows.{}]\nrepo = {}\nmapping = {}\n",
        toml_key(name),
        toml_string(repo),
        toml_string(mapping)
    )
}

/// Appends a shadow snippet to an existing config's text (preserving
/// whatever formatting it has), or starts a fresh v1 config.
fn config_with_shadow(existing: Option<&str>, snippet: &str) -> String {
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

fn find_config_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_config_files(root, &mut found);
    found.sort();
    found
}

fn collect_config_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_config_files(&path, found);
        } else if entry.file_name() == CONFIG_FILE_NAME {
            found.push(path);
        }
    }
}

fn sorted_shadow_rows(config: &Config) -> Vec<(&str, &str, &str)> {
    let mut rows: Vec<_> = config
        .shadows
        .iter()
        .map(|(name, shadow)| (name.as_str(), shadow.repo.as_str(), shadow.mapping.as_str()))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    rows
}

fn format_shadow_rows(rows: &[(&str, &str, &str)]) -> Vec<String> {
    let name_width = rows
        .iter()
        .map(|(name, _, _)| name.len())
        .max()
        .unwrap_or(0);
    let repo_width = rows
        .iter()
        .map(|(_, repo, _)| repo.len())
        .max()
        .unwrap_or(0);

    rows.iter()
        .map(|(name, repo, mapping)| {
            format!("{name:<name_width$}   {repo:<repo_width$}   {mapping}")
        })
        .collect()
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
        if RESERVED_NICKNAMES.contains(&nickname.as_str()) {
            return Err(format!("shadow nickname '{nickname}' is reserved"));
        }
        if shadow.repo.trim().is_empty() {
            return Err(format!("shadow '{nickname}' has an empty repo"));
        }
        if shadow.mapping.trim().is_empty() {
            return Err(format!("shadow '{nickname}' has an empty mapping"));
        }
        if !is_portable_relative_path(&shadow.mapping) {
            return Err(format!("shadow '{nickname}' mapping must be relative"));
        }
    }

    Ok(Config {
        shadows: raw.shadows,
    })
}

fn is_portable_relative_path(path: &str) -> bool {
    if Path::new(path).is_absolute() {
        return false;
    }

    let bytes = path.as_bytes();
    if matches!(bytes.first(), Some(b'/' | b'\\')) {
        return false;
    }

    !matches!(
        bytes,
        [drive, b':', ..] if drive.is_ascii_alphabetic()
    )
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
    fn windows_absolute_shadow_mapping_is_an_error() {
        let err = parse_config(
            r#"
version = 1

[shadows.cardlet]
repo = "git@github.com:andre-a-alves/cardlet.git"
mapping = "C:\\tmp\\cardlet"
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

    #[test]
    fn reserved_list_nickname_is_an_error() {
        let err = parse_config(
            r#"
version = 1

[shadows.list]
repo = "git@github.com:example/list.git"
mapping = ".vendor/list/"
"#,
        )
        .unwrap_err();

        assert!(err.contains("shadow nickname 'list' is reserved"));
    }

    #[test]
    fn list_args_default_to_local() {
        assert_eq!(parse_list_args(&[]), Ok(false));
    }

    #[test]
    fn list_args_accept_global_flag() {
        assert_eq!(parse_list_args(&["--global".to_string()]), Ok(true));
    }

    #[test]
    fn list_args_reject_unknown_arguments() {
        let err = parse_list_args(&["--bogus".to_string()]).unwrap_err();
        assert!(err.contains("usage: git shadow list [--global]"));

        let err = parse_list_args(&["--global".to_string(), "extra".to_string()]).unwrap_err();
        assert!(err.contains("usage: git shadow list [--global]"));
    }

    #[test]
    fn formats_shadow_rows_with_aligned_columns() {
        let rows = vec![
            (
                "cardlet",
                "git@github.com:andre-a-alves/cardlet.git",
                ".test/",
            ),
            ("fb", "git@github.com:example/foobar.git", ".vendor/foobar/"),
        ];

        let lines = format_shadow_rows(&rows);

        assert_eq!(
            lines,
            vec![
                "cardlet   git@github.com:andre-a-alves/cardlet.git   .test/",
                "fb        git@github.com:example/foobar.git          .vendor/foobar/",
            ]
        );
    }

    #[test]
    fn sorts_shadow_rows_by_nickname() {
        let config = parse_config(
            r#"
version = 1

[shadows.zeta]
repo = "git@github.com:example/zeta.git"
mapping = ".vendor/zeta/"

[shadows.alpha]
repo = "git@github.com:example/alpha.git"
mapping = ".vendor/alpha/"
"#,
        )
        .unwrap();

        let rows = sorted_shadow_rows(&config);
        assert_eq!(rows[0].0, "alpha");
        assert_eq!(rows[1].0, "zeta");
    }

    #[test]
    fn reserved_sync_nickname_is_an_error() {
        let err = parse_config(
            r#"
version = 1

[shadows.sync]
repo = "git@github.com:example/sync.git"
mapping = ".vendor/sync/"
"#,
        )
        .unwrap_err();

        assert!(err.contains("shadow nickname 'sync' is reserved"));
    }

    #[test]
    fn sync_args_default_to_all_shadows() {
        assert_eq!(parse_sync_args(&[]), Ok(None));
    }

    #[test]
    fn sync_args_accept_a_shadow_name() {
        assert_eq!(
            parse_sync_args(&["cardlet".to_string()]),
            Ok(Some("cardlet".to_string()))
        );
    }

    #[test]
    fn sync_args_reject_flags_and_extra_arguments() {
        let err = parse_sync_args(&["--global".to_string()]).unwrap_err();
        assert!(err.contains("usage: git shadow sync [<shadow-name>]"));

        let err = parse_sync_args(&["a".to_string(), "b".to_string()]).unwrap_err();
        assert!(err.contains("usage: git shadow sync [<shadow-name>]"));
    }

    #[test]
    fn missing_mapping_needs_a_clone() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(sync_action(&root.path().join("missing")), SyncAction::Clone);
    }

    #[test]
    fn empty_mapping_directory_needs_a_clone() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("empty");
        std::fs::create_dir(&dir).unwrap();

        assert_eq!(sync_action(&dir), SyncAction::Clone);
    }

    #[test]
    fn mapping_with_git_dir_is_already_present() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("repo");
        std::fs::create_dir_all(dir.join(".git")).unwrap();

        assert_eq!(sync_action(&dir), SyncAction::AlreadyPresent);
    }

    #[test]
    fn non_empty_mapping_without_git_dir_is_not_a_repo() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("files");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();

        assert_eq!(sync_action(&dir), SyncAction::NotARepo);
    }

    #[test]
    fn mapping_that_is_a_file_is_not_a_directory() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("mapping");
        std::fs::write(&file, "").unwrap();

        assert_eq!(sync_action(&file), SyncAction::NotADirectory);
    }

    #[test]
    fn reserved_clone_nickname_is_an_error() {
        let err = parse_config(
            r#"
version = 1

[shadows.clone]
repo = "git@github.com:example/clone.git"
mapping = ".vendor/clone/"
"#,
        )
        .unwrap_err();

        assert!(err.contains("shadow nickname 'clone' is reserved"));
    }

    #[test]
    fn clone_args_require_a_repo_url() {
        let err = parse_clone_args(&[]).unwrap_err();
        assert!(err.contains("usage: git shadow clone"));
    }

    #[test]
    fn clone_args_accept_url_directory_and_name() {
        let args = parse_clone_args(&[
            "git@github.com:example/foobar.git".to_string(),
            ".vendor/fb".to_string(),
            "--name".to_string(),
            "fb".to_string(),
        ])
        .unwrap();

        assert_eq!(args.repo, "git@github.com:example/foobar.git");
        assert_eq!(args.dir.as_deref(), Some(".vendor/fb"));
        assert_eq!(args.name.as_deref(), Some("fb"));
    }

    #[test]
    fn clone_args_accept_name_with_equals() {
        let args = parse_clone_args(&[
            "git@github.com:example/foobar.git".to_string(),
            "--name=fb".to_string(),
        ])
        .unwrap();

        assert_eq!(args.name.as_deref(), Some("fb"));
        assert_eq!(args.dir, None);
    }

    #[test]
    fn clone_args_reject_unknown_flags_extra_args_and_dangling_name() {
        for args in [
            vec!["url".to_string(), "--force".to_string()],
            vec!["url".to_string(), "dir".to_string(), "extra".to_string()],
            vec!["url".to_string(), "--name".to_string()],
            vec!["url".to_string(), "--name=".to_string()],
        ] {
            let err = parse_clone_args(&args).unwrap_err();
            assert!(err.contains("usage: git shadow clone"), "args: {args:?}");
        }
    }

    #[test]
    fn derives_repo_name_from_urls() {
        for (url, expected) in [
            ("git@github.com:example/foobar.git", "foobar"),
            ("https://github.com/example/foobar.git", "foobar"),
            ("https://github.com/example/foobar", "foobar"),
            ("https://github.com/example/foobar/", "foobar"),
            ("/srv/git/foobar.git", "foobar"),
        ] {
            assert_eq!(repo_name_from_url(url).unwrap(), expected, "url: {url}");
        }
    }

    #[test]
    fn repo_name_derivation_fails_on_empty_result() {
        assert!(repo_name_from_url("git@github.com:").is_err());
        assert!(repo_name_from_url("/").is_err());
    }

    #[test]
    fn computes_mapping_relative_to_parent_repo() {
        let parent = Path::new("/home/user/project");

        assert_eq!(
            relative_mapping(parent, Path::new("/home/user/project/foobar")).unwrap(),
            "foobar/"
        );
        assert_eq!(
            relative_mapping(parent, Path::new("/home/user/project/sub/./x/../fb")).unwrap(),
            "sub/fb/"
        );
    }

    #[test]
    fn mapping_outside_parent_repo_is_an_error() {
        let parent = Path::new("/home/user/project");

        let err = relative_mapping(parent, Path::new("/home/user/elsewhere")).unwrap_err();
        assert!(err.contains("outside the parent repository"));

        let err = relative_mapping(parent, Path::new("/home/user/project/sub/../..")).unwrap_err();
        assert!(err.contains("outside the parent repository"));
    }

    #[test]
    fn mapping_at_parent_repo_root_is_an_error() {
        let parent = Path::new("/home/user/project");

        let err = relative_mapping(parent, Path::new("/home/user/project")).unwrap_err();
        assert!(err.contains("parent repository root"));
    }

    #[test]
    fn new_config_with_shadow_parses() {
        let snippet =
            shadow_config_snippet("foobar", "git@github.com:example/foobar.git", "foobar/");
        let content = config_with_shadow(None, &snippet);

        let config = parse_config(&content).unwrap();
        let shadow = config.shadows.get("foobar").unwrap();
        assert_eq!(shadow.repo, "git@github.com:example/foobar.git");
        assert_eq!(shadow.mapping, "foobar/");
    }

    #[test]
    fn appended_config_keeps_existing_shadows() {
        let snippet =
            shadow_config_snippet("foobar", "git@github.com:example/foobar.git", "foobar/");
        let content = config_with_shadow(Some(EXAMPLE_TOML), &snippet);

        let config = parse_config(&content).unwrap();
        assert_eq!(config.shadows.len(), 2);
        assert!(config.shadows.contains_key("cardlet"));
        assert!(config.shadows.contains_key("foobar"));
    }

    #[test]
    fn config_snippet_escapes_special_characters() {
        let snippet = shadow_config_snippet("has spaces", "url\"with\"quotes", "dir/");
        let content = config_with_shadow(None, &snippet);

        let config = parse_config(&content).unwrap();
        let shadow = config.shadows.get("has spaces").unwrap();
        assert_eq!(shadow.repo, "url\"with\"quotes");
    }

    #[test]
    fn same_remote_matches_across_ssh_and_https() {
        assert!(same_remote(
            "git@github.com:andre-a-alves/cardlet.git",
            "https://github.com/andre-a-alves/cardlet"
        ));
    }

    #[test]
    fn same_remote_rejects_different_repos() {
        assert!(!same_remote(
            "git@github.com:andre-a-alves/cardlet.git",
            "git@github.com:andre-a-alves/git-shadow.git"
        ));
    }

    #[test]
    fn same_remote_falls_back_to_literal_comparison() {
        assert!(same_remote("/srv/git/repo.git", "/srv/git/repo.git"));
        assert!(!same_remote("/srv/git/repo.git", "/srv/git/other.git"));
    }

    #[test]
    fn finds_nested_config_files() {
        let root = tempfile::tempdir().unwrap();

        let repo_a = root.path().join("github.com/example/alpha");
        let repo_b = root.path().join("gitlab.com/example/beta");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();
        std::fs::write(repo_a.join("config.toml"), "").unwrap();
        std::fs::write(repo_b.join("config.toml"), "").unwrap();
        std::fs::write(repo_a.join("notes.txt"), "").unwrap();

        let found = find_config_files(root.path());

        assert_eq!(
            found,
            vec![repo_a.join("config.toml"), repo_b.join("config.toml")]
        );
    }

    #[test]
    fn finding_config_files_in_missing_root_returns_empty() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("does-not-exist");

        assert!(find_config_files(&missing).is_empty());
    }
}
