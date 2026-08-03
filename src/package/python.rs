use std::path::Path;

use crate::errors::ChangesetterError;
use crate::package::adapter::Adapter;
use crate::package::types::{Package, PackageType, Version};

pub struct PythonAdapter;

impl PythonAdapter {
    fn read_document(path: &Path) -> anyhow::Result<toml_edit::DocumentMut> {
        let content =
            std::fs::read_to_string(path).map_err(|e| ChangesetterError::ManifestRead {
                path: path.to_path_buf(),
                reason: e.to_string(),
            })?;
        let doc: toml_edit::DocumentMut =
            content
                .parse()
                .map_err(|e: toml_edit::TomlError| ChangesetterError::ManifestRead {
                    path: path.to_path_buf(),
                    reason: e.to_string(),
                })?;
        Ok(doc)
    }

    fn pyproject_path(path: &Path) -> std::path::PathBuf {
        if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("pyproject.toml")
        }
    }
}

impl Adapter for PythonAdapter {
    fn detect(&self, path: &Path) -> anyhow::Result<Option<Package>> {
        let pyproject = path.join("pyproject.toml");
        if !pyproject.exists() {
            return Ok(None);
        }

        let doc = Self::read_document(&pyproject)?;

        // PEP 621: [project] table
        if let Some(project) = doc.get("project") {
            let name = project
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            let version_str = match project.get("version").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => return Ok(None),
            };

            let version = Version::parse(version_str)?;
            return Ok(Some(Package {
                name,
                path: path.to_path_buf(),
                package_type: PackageType::Python,
                version,
            }));
        }

        // Poetry: [tool.poetry] table
        if let Some(tool) = doc.get("tool") {
            if let Some(poetry) = tool.get("poetry") {
                let name = poetry
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();

                let version_str = match poetry.get("version").and_then(|v| v.as_str()) {
                    Some(v) => v,
                    None => return Ok(None),
                };

                let version = Version::parse(version_str)?;
                return Ok(Some(Package {
                    name,
                    path: path.to_path_buf(),
                    package_type: PackageType::Python,
                    version,
                }));
            }
        }

        Ok(None)
    }

    fn read_version(&self, path: &Path) -> anyhow::Result<Version> {
        let pyproject = Self::pyproject_path(path);
        let doc = Self::read_document(&pyproject)?;

        if let Some(v) = doc
            .get("project")
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
        {
            return Ok(Version::parse(v)?);
        }

        if let Some(v) = doc
            .get("tool")
            .and_then(|t| t.get("poetry"))
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
        {
            return Ok(Version::parse(v)?);
        }

        anyhow::bail!("no version found in {}", pyproject.display());
    }

    fn write_version(&self, path: &Path, version: &Version) -> anyhow::Result<()> {
        let pyproject = Self::pyproject_path(path);
        let mut doc = Self::read_document(&pyproject)?;

        if doc.get("project").and_then(|p| p.get("version")).is_some() {
            doc["project"]["version"] = toml_edit::value(version.to_string());
        } else if doc
            .get("tool")
            .and_then(|t| t.get("poetry"))
            .and_then(|p| p.get("version"))
            .is_some()
        {
            doc["tool"]["poetry"]["version"] = toml_edit::value(version.to_string());
        } else {
            anyhow::bail!("no version field to update in {}", pyproject.display());
        }

        std::fs::write(&pyproject, doc.to_string()).map_err(|e| {
            ChangesetterError::ManifestWrite {
                path: pyproject,
                reason: e.to_string(),
            }
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_pep621() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "my-python-lib"
version = "1.2.3"
description = "A test library"
"#,
        )
        .unwrap();

        let adapter = PythonAdapter;
        let pkg = adapter.detect(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "my-python-lib");
        assert_eq!(pkg.version, Version::new(1, 2, 3));
        assert_eq!(pkg.package_type, PackageType::Python);
    }

    #[test]
    fn detect_poetry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"[tool.poetry]
name = "my-poetry-app"
version = "0.5.0"
description = "A poetry project"

[tool.poetry.dependencies]
python = "^3.9"
"#,
        )
        .unwrap();

        let adapter = PythonAdapter;
        let pkg = adapter.detect(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "my-poetry-app");
        assert_eq!(pkg.version, Version::new(0, 5, 0));
        assert_eq!(pkg.package_type, PackageType::Python);
    }

    #[test]
    fn detect_no_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = PythonAdapter;
        assert!(adapter.detect(dir.path()).unwrap().is_none());
    }

    #[test]
    fn detect_skips_no_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "my-lib"
dynamic = ["version"]
"#,
        )
        .unwrap();

        let adapter = PythonAdapter;
        assert!(adapter.detect(dir.path()).unwrap().is_none());
    }

    #[test]
    fn read_write_roundtrip_pep621() {
        let dir = tempfile::tempdir().unwrap();
        let pyproject = dir.path().join("pyproject.toml");
        std::fs::write(
            &pyproject,
            r#"[project]
name = "my-lib"
version = "1.0.0"
description = "Test"

[project.optional-dependencies]
dev = ["pytest"]
"#,
        )
        .unwrap();

        let adapter = PythonAdapter;
        let v = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(1, 0, 0));

        adapter
            .write_version(dir.path(), &Version::new(1, 1, 0))
            .unwrap();

        let v2 = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v2, Version::new(1, 1, 0));

        let content = std::fs::read_to_string(&pyproject).unwrap();
        assert!(content.contains("name = \"my-lib\""));
        assert!(content.contains("[project.optional-dependencies]"));
    }

    #[test]
    fn read_write_roundtrip_poetry() {
        let dir = tempfile::tempdir().unwrap();
        let pyproject = dir.path().join("pyproject.toml");
        std::fs::write(
            &pyproject,
            r#"[tool.poetry]
name = "my-poetry-app"
version = "2.0.0"
description = "Test"

[tool.poetry.dependencies]
python = "^3.9"
requests = "^2.28"
"#,
        )
        .unwrap();

        let adapter = PythonAdapter;
        let v = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(2, 0, 0));

        adapter
            .write_version(dir.path(), &Version::new(2, 1, 0))
            .unwrap();

        let v2 = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v2, Version::new(2, 1, 0));

        let content = std::fs::read_to_string(&pyproject).unwrap();
        assert!(content.contains("name = \"my-poetry-app\""));
        assert!(content.contains("requests = \"^2.28\""));
    }
}
