use std::path::PathBuf;

/// Normalizes a git remote URL into a repo-shaped relative path
/// (`host/owner/repo`) used to locate the per-repo config file.
pub fn normalize_remote_url(remote_url: &str) -> Result<PathBuf, String> {
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

/// Compares two remote URLs by their normalized host/owner/repo path, so the
/// same repository reached over SSH and HTTPS still matches. Falls back to a
/// literal comparison when either URL cannot be normalized.
pub fn same_remote(a: &str, b: &str) -> bool {
    match (normalize_remote_url(a), normalize_remote_url(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.trim() == b.trim(),
    }
}

/// Derives a repository name from a remote URL or local path: the last
/// path segment with any trailing `.git` removed.
pub fn repo_name_from_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next().unwrap_or_default();
    let name = last.strip_suffix(".git").unwrap_or(last);

    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("cannot derive a repository name from '{url}'"));
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
