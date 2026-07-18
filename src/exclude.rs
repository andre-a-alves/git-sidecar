use std::path::{Path, PathBuf};

use crate::git::git_exclude_path;

pub const EXCLUDE_SECTION_START: &str = "# >>> git-sidecar (managed) >>>";
pub const EXCLUDE_SECTION_END: &str = "# <<< git-sidecar (managed) <<<";

/// The exclude pattern for a mapping: root-anchored and directory-only.
pub fn exclude_entry(mapping: &str) -> String {
    format!("/{}/", mapping.trim_matches('/'))
}

/// Ensures the git-sidecar managed section of the parent repo's
/// `.git/info/exclude` contains an entry for every given mapping.
/// Returns the exclude file's path when it was actually rewritten.
pub fn ensure_mappings_excluded(
    parent_repo: &Path,
    mappings: &[&str],
) -> Result<Option<PathBuf>, String> {
    if mappings.is_empty() {
        return Ok(None);
    }

    let entries: Vec<String> = mappings.iter().map(|m| exclude_entry(m)).collect();
    let exclude_path = git_exclude_path(parent_repo)?;

    let content = if exclude_path.exists() {
        std::fs::read_to_string(&exclude_path)
            .map_err(|e| format!("failed to read {}: {e}", exclude_path.display()))?
    } else {
        String::new()
    };

    let (new_content, changed) = with_excluded_entries(&content, &entries);
    if !changed {
        return Ok(None);
    }

    if let Some(info_dir) = exclude_path.parent() {
        std::fs::create_dir_all(info_dir)
            .map_err(|e| format!("failed to create {}: {e}", info_dir.display()))?;
    }
    std::fs::write(&exclude_path, new_content)
        .map_err(|e| format!("failed to write {}: {e}", exclude_path.display()))?;

    Ok(Some(exclude_path))
}

/// Drops a mapping's entry from the managed section of the parent repo's
/// exclude file. Returns the file's path when it was actually rewritten.
pub fn remove_mapping_exclusion(
    parent_repo: &Path,
    mapping: &str,
) -> Result<Option<PathBuf>, String> {
    let exclude_path = git_exclude_path(parent_repo)?;
    if !exclude_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&exclude_path)
        .map_err(|e| format!("failed to read {}: {e}", exclude_path.display()))?;

    let (new_content, changed) = without_excluded_entry(&content, &exclude_entry(mapping));
    if !changed {
        return Ok(None);
    }

    std::fs::write(&exclude_path, new_content)
        .map_err(|e| format!("failed to write {}: {e}", exclude_path.display()))?;
    Ok(Some(exclude_path))
}

/// Adds any missing entries to the git-sidecar managed section of an
/// exclude file's content, creating the section if needed. Lines outside
/// the section are never touched. Returns the new content and whether it
/// differs from the input.
fn with_excluded_entries(content: &str, entries: &[String]) -> (String, bool) {
    if entries.is_empty() {
        return (content.to_string(), false);
    }

    let lines: Vec<&str> = content.lines().collect();

    if let (Some(start), Some(end)) = managed_section(&lines) {
        let missing: Vec<String> = entries
            .iter()
            .filter(|entry| {
                !lines[start + 1..end]
                    .iter()
                    .any(|line| line.trim() == entry.as_str())
            })
            .cloned()
            .collect();
        if missing.is_empty() {
            return (content.to_string(), false);
        }

        let mut out_lines: Vec<String> = lines.iter().map(|line| (*line).to_string()).collect();
        out_lines.splice(end..end, missing);
        let mut out = out_lines.join("\n");
        out.push('\n');
        return (out, true);
    }

    // No complete managed section yet: append a fresh one.
    let mut out = content.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(EXCLUDE_SECTION_START);
    out.push('\n');
    for entry in entries {
        out.push_str(entry);
        out.push('\n');
    }
    out.push_str(EXCLUDE_SECTION_END);
    out.push('\n');
    (out, true)
}

/// Removes an entry from the git-sidecar managed section of an exclude
/// file's content. Lines outside the section are never touched.
fn without_excluded_entry(content: &str, entry: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();

    let (Some(start), Some(end)) = managed_section(&lines) else {
        return (content.to_string(), false);
    };

    let kept_in_section: Vec<&str> = lines[start + 1..end]
        .iter()
        .filter(|line| line.trim() != entry)
        .copied()
        .collect();
    if kept_in_section.len() == end - start - 1 {
        return (content.to_string(), false);
    }

    let mut out_lines: Vec<&str> = Vec::new();
    out_lines.extend(&lines[..=start]);
    out_lines.extend(kept_in_section);
    out_lines.extend(&lines[end..]);

    let mut out = out_lines.join("\n");
    out.push('\n');
    (out, true)
}

/// Line indices of the managed section's start and end markers.
fn managed_section(lines: &[&str]) -> (Option<usize>, Option<usize>) {
    let start = lines
        .iter()
        .position(|line| line.trim() == EXCLUDE_SECTION_START);
    let end = start.and_then(|start| {
        lines[start + 1..]
            .iter()
            .position(|line| line.trim() == EXCLUDE_SECTION_END)
            .map(|offset| start + 1 + offset)
    });
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclude_entries_are_root_anchored_directories() {
        assert_eq!(exclude_entry("foobar/"), "/foobar/");
        assert_eq!(exclude_entry("sub/vendor/fb/"), "/sub/vendor/fb/");
        assert_eq!(exclude_entry(".test"), "/.test/");
    }

    #[test]
    fn creates_managed_section_in_empty_exclude() {
        let (out, changed) = with_excluded_entries("", &["/foobar/".to_string()]);

        assert!(changed);
        assert_eq!(
            out,
            "# >>> git-sidecar (managed) >>>\n/foobar/\n# <<< git-sidecar (managed) <<<\n"
        );
    }

    #[test]
    fn appends_managed_section_after_existing_rules() {
        let (out, changed) = with_excluded_entries("*.log\nbuild/\n", &["/foobar/".to_string()]);

        assert!(changed);
        assert_eq!(
            out,
            "*.log\nbuild/\n\n# >>> git-sidecar (managed) >>>\n/foobar/\n# <<< git-sidecar (managed) <<<\n"
        );
    }

    #[test]
    fn adds_missing_entry_to_existing_managed_section() {
        let existing =
            "*.log\n\n# >>> git-sidecar (managed) >>>\n/foobar/\n# <<< git-sidecar (managed) <<<\n";
        let (out, changed) =
            with_excluded_entries(existing, &["/foobar/".to_string(), "/fb/".to_string()]);

        assert!(changed);
        assert_eq!(
            out,
            "*.log\n\n# >>> git-sidecar (managed) >>>\n/foobar/\n/fb/\n# <<< git-sidecar (managed) <<<\n"
        );
    }

    #[test]
    fn present_entries_leave_exclude_unchanged() {
        let existing =
            "# >>> git-sidecar (managed) >>>\n/foobar/\n# <<< git-sidecar (managed) <<<\n";
        let (out, changed) = with_excluded_entries(existing, &["/foobar/".to_string()]);

        assert!(!changed);
        assert_eq!(out, existing);
    }

    #[test]
    fn lines_outside_managed_section_are_ignored_for_matching() {
        // an identical rule outside the section doesn't count as managed
        let existing = "/foobar/\n";
        let (out, changed) = with_excluded_entries(existing, &["/foobar/".to_string()]);

        assert!(changed);
        assert!(out.starts_with("/foobar/\n\n# >>> git-sidecar (managed) >>>\n"));
    }

    #[test]
    fn no_entries_never_changes_exclude() {
        let (out, changed) = with_excluded_entries("*.log\n", &[]);

        assert!(!changed);
        assert_eq!(out, "*.log\n");
    }

    #[test]
    fn removes_entry_from_managed_exclude_section() {
        let content = "*.log\n\n# >>> git-sidecar (managed) >>>\n/foobar/\n/fb/\n# <<< git-sidecar (managed) <<<\n";
        let (out, changed) = without_excluded_entry(content, "/foobar/");

        assert!(changed);
        assert_eq!(
            out,
            "*.log\n\n# >>> git-sidecar (managed) >>>\n/fb/\n# <<< git-sidecar (managed) <<<\n"
        );
    }

    #[test]
    fn absent_exclude_entry_changes_nothing() {
        let content = "# >>> git-sidecar (managed) >>>\n/fb/\n# <<< git-sidecar (managed) <<<\n";
        let (out, changed) = without_excluded_entry(content, "/foobar/");

        assert!(!changed);
        assert_eq!(out, content);
    }

    #[test]
    fn matching_rule_outside_managed_section_is_not_removed() {
        let content = "/foobar/\n";
        let (out, changed) = without_excluded_entry(content, "/foobar/");

        assert!(!changed);
        assert_eq!(out, content);
    }
}
