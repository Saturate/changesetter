use std::path::Path;

use crate::changeset::parser;
use crate::changeset::types::Changeset;

const IGNORED_FILES: &[&str] = &["config.json", "pre.json", "README.md"];

pub fn read_changesets(changeset_dir: &Path) -> anyhow::Result<Vec<Changeset>> {
    if !changeset_dir.exists() {
        return Ok(Vec::new());
    }

    let mut changesets = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(changeset_dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or_default();

        if !filename.ends_with(".md") {
            continue;
        }

        if IGNORED_FILES.contains(&filename) {
            continue;
        }

        let content = std::fs::read_to_string(&path)?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        let changeset = parser::parse(&content, Some(stem))?;
        changesets.push(changeset);
    }

    Ok(changesets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changeset::types::BumpLevel;

    #[test]
    fn read_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        let result = read_changesets(&changeset_dir).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn read_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");

        let result = read_changesets(&changeset_dir).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn read_valid_changesets() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        std::fs::write(
            changeset_dir.join("cool-dogs-dance.md"),
            "---\nmylib: patch\n---\n\n#### Fix\n",
        )
        .unwrap();
        std::fs::write(
            changeset_dir.join("red-lions-run.md"),
            "---\nmylib: minor\n---\n\n#### Feature\n",
        )
        .unwrap();

        let result = read_changesets(&changeset_dir).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].filename.as_deref(), Some("cool-dogs-dance"));
        assert_eq!(result[0].packages["mylib"], BumpLevel::Patch);
        assert_eq!(result[1].filename.as_deref(), Some("red-lions-run"));
        assert_eq!(result[1].packages["mylib"], BumpLevel::Minor);
    }

    #[test]
    fn ignores_config_json() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        std::fs::write(changeset_dir.join("config.json"), "{}").unwrap();
        std::fs::write(changeset_dir.join("pre.json"), "{}").unwrap();
        std::fs::write(changeset_dir.join("README.md"), "# Changesets\n").unwrap();
        std::fs::write(
            changeset_dir.join("valid.md"),
            "---\nmylib: patch\n---\n\n#### Fix\n",
        )
        .unwrap();

        let result = read_changesets(&changeset_dir).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].filename.as_deref(), Some("valid"));
    }

    #[test]
    fn ignores_non_md_files() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        std::fs::write(changeset_dir.join("notes.txt"), "some notes").unwrap();
        std::fs::write(changeset_dir.join(".gitkeep"), "").unwrap();

        let result = read_changesets(&changeset_dir).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn returns_error_on_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        std::fs::write(changeset_dir.join("bad.md"), "no frontmatter here\n").unwrap();

        let result = read_changesets(&changeset_dir);
        assert!(result.is_err());
    }

    #[test]
    fn deterministic_order() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        std::fs::write(
            changeset_dir.join("zzz.md"),
            "---\na: patch\n---\n\n#### Z\n",
        )
        .unwrap();
        std::fs::write(
            changeset_dir.join("aaa.md"),
            "---\nb: minor\n---\n\n#### A\n",
        )
        .unwrap();

        let result = read_changesets(&changeset_dir).unwrap();
        assert_eq!(result[0].filename.as_deref(), Some("aaa"));
        assert_eq!(result[1].filename.as_deref(), Some("zzz"));
    }
}
