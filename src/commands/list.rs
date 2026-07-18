use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::config::{
    CONFIG_DIR_NAME, CONFIG_FILE_NAME, Config, RepoContext, platform_config_dir, read_config,
};

pub fn run(global: bool) -> ExitCode {
    let result = if global { list_global() } else { list_local() };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("git-sidecar: {e}");
            ExitCode::FAILURE
        }
    }
}

fn list_local() -> Result<(), String> {
    let ctx = RepoContext::discover()?;

    if !ctx.config_path.exists() {
        println!(
            "no sidecars configured for {} ({})",
            ctx.origin_url,
            ctx.config_path.display()
        );
        return Ok(());
    }

    let config = read_config(&ctx.config_path)?;
    if config.sidecars.is_empty() {
        println!(
            "no sidecars configured for {} ({})",
            ctx.origin_url,
            ctx.config_path.display()
        );
        return Ok(());
    }

    for line in format_sidecar_rows(&sorted_sidecar_rows(&config)) {
        println!("{line}");
    }
    Ok(())
}

fn list_global() -> Result<(), String> {
    let root = platform_config_dir()?.join(CONFIG_DIR_NAME);
    let config_files = find_config_files(&root);

    if config_files.is_empty() {
        println!("no sidecars configured under {}", root.display());
        return Ok(());
    }

    let mut first = true;
    for config_path in config_files {
        let config = match read_config(&config_path) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("git-sidecar: warning: skipping {e}");
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
        for line in format_sidecar_rows(&sorted_sidecar_rows(&config)) {
            println!("  {line}");
        }
    }
    Ok(())
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

fn sorted_sidecar_rows(config: &Config) -> Vec<(&str, &str, &str)> {
    let mut rows: Vec<_> = config
        .sidecars
        .iter()
        .map(|(name, sidecar)| (name.as_str(), sidecar.repo.as_str(), sidecar.mapping.as_str()))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    rows
}

fn format_sidecar_rows(rows: &[(&str, &str, &str)]) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;

    #[test]
    fn formats_sidecar_rows_with_aligned_columns() {
        let rows = vec![
            (
                "cardlet",
                "git@github.com:andre-a-alves/cardlet.git",
                ".test/",
            ),
            ("fb", "git@github.com:example/foobar.git", ".vendor/foobar/"),
        ];

        let lines = format_sidecar_rows(&rows);

        assert_eq!(
            lines,
            vec![
                "cardlet   git@github.com:andre-a-alves/cardlet.git   .test/",
                "fb        git@github.com:example/foobar.git          .vendor/foobar/",
            ]
        );
    }

    #[test]
    fn sorts_sidecar_rows_by_nickname() {
        let config = parse_config(
            r#"
version = 1

[sidecars.zeta]
repo = "git@github.com:example/zeta.git"
mapping = ".vendor/zeta/"

[sidecars.alpha]
repo = "git@github.com:example/alpha.git"
mapping = ".vendor/alpha/"
"#,
        )
        .unwrap();

        let rows = sorted_sidecar_rows(&config);
        assert_eq!(rows[0].0, "alpha");
        assert_eq!(rows[1].0, "zeta");
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
