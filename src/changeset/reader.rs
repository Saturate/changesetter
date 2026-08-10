use std::path::{Path, PathBuf};

use crate::changeset::parser;
use crate::changeset::types::Changeset;

const IGNORED_FILES: &[&str] = &["config.json", "pre.json", "README.md"];

pub fn read_changesets(changeset_dir: &Path) -> anyhow::Result<Vec<Changeset>> {
    if !changeset_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    collect_md_files(changeset_dir, changeset_dir, &mut paths)?;
    paths.sort();

    let mut changesets = Vec::new();
    for (rel_stem, full_path) in paths {
        let content = std::fs::read_to_string(&full_path)?;
        let changeset = parser::parse(&content, Some(rel_stem))?;
        changesets.push(changeset);
    }

    Ok(changesets)
}

fn collect_md_files(
    base: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_md_files(base, &path, out)?;
            continue;
        }

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

        let rel = path.strip_prefix(base).unwrap_or(&path).with_extension("");
        let rel_stem = rel.to_string_lossy().to_string();

        out.push((rel_stem, path));
    }
    Ok(())
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
    fn read_nested_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        let sub_dir = changeset_dir.join("changesets");
        std::fs::create_dir_all(&sub_dir).unwrap();

        std::fs::write(
            sub_dir.join("cool-dogs-dance.md"),
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
        assert_eq!(
            result[0].filename.as_deref(),
            Some("changesets/cool-dogs-dance")
        );
        assert_eq!(result[1].filename.as_deref(), Some("red-lions-run"));
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
