use std::path::{Component, Path, PathBuf};

/// Returns whether a mapping is safe to store in the config: relative on
/// every platform (no leading slash or backslash, no Windows drive prefix).
pub fn is_portable_relative_path(path: &str) -> bool {
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

/// Resolves `.` and `..` components without touching the filesystem, so
/// paths that do not exist yet can still be compared.
pub fn normalize_lexically(path: &Path) -> PathBuf {
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

/// Resolves `target` against the parent repo root and returns the config
/// `mapping` string: forward-slash separated, relative, with a trailing `/`.
pub fn relative_mapping(parent_repo: &Path, target: &Path) -> Result<String, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
