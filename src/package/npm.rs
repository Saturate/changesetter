use std::path::Path;

use serde::Serialize;

use crate::errors::ChangesetterError;
use crate::package::adapter::Adapter;
use crate::package::types::{Package, PackageType, Version};

pub struct NpmAdapter;

impl Adapter for NpmAdapter {
    fn detect(&self, path: &Path) -> anyhow::Result<Option<Package>> {
        let pkg_json = path.join("package.json");
        if !pkg_json.exists() {
            return Ok(None);
        }

        let content =
            std::fs::read_to_string(&pkg_json).map_err(|e| ChangesetterError::ManifestRead {
                path: pkg_json.clone(),
                reason: e.to_string(),
            })?;

        let json: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| ChangesetterError::ManifestRead {
                path: pkg_json.clone(),
                reason: e.to_string(),
            })?;

        let name = json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let version_str = json
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0");

        let version = Version::parse(version_str)?;

        Ok(Some(Package {
            name,
            path: path.to_path_buf(),
            package_type: PackageType::Npm,
            version,
        }))
    }

    fn read_version(&self, path: &Path) -> anyhow::Result<Version> {
        let pkg_json = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("package.json")
        };

        let content =
            std::fs::read_to_string(&pkg_json).map_err(|e| ChangesetterError::ManifestRead {
                path: pkg_json.clone(),
                reason: e.to_string(),
            })?;

        let json: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| ChangesetterError::ManifestRead {
                path: pkg_json.clone(),
                reason: e.to_string(),
            })?;

        let version_str = json
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("no version field in {}", pkg_json.display()))?;

        Ok(Version::parse(version_str)?)
    }

    fn write_version(&self, path: &Path, version: &Version) -> anyhow::Result<()> {
        let pkg_json = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("package.json")
        };

        let content =
            std::fs::read_to_string(&pkg_json).map_err(|e| ChangesetterError::ManifestRead {
                path: pkg_json.clone(),
                reason: e.to_string(),
            })?;

        // Detect indent style from the file
        let indent = detect_indent(&content);
        let had_trailing_newline = content.ends_with('\n');

        let mut json: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| ChangesetterError::ManifestRead {
                path: pkg_json.clone(),
                reason: e.to_string(),
            })?;

        json["version"] = serde_json::Value::String(version.to_string());

        let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
        let mut buf = Vec::new();
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        serde_json::Value::serialize(&json, &mut ser).map_err(|e| {
            ChangesetterError::ManifestWrite {
                path: pkg_json.clone(),
                reason: e.to_string(),
            }
        })?;

        let mut output = String::from_utf8(buf).map_err(|e| ChangesetterError::ManifestWrite {
            path: pkg_json.clone(),
            reason: e.to_string(),
        })?;

        if had_trailing_newline && !output.ends_with('\n') {
            output.push('\n');
        }

        std::fs::write(&pkg_json, &output).map_err(|e| ChangesetterError::ManifestWrite {
            path: pkg_json,
            reason: e.to_string(),
        })?;

        Ok(())
    }
}

fn detect_indent(content: &str) -> String {
    for line in content.lines().skip(1) {
        if line.starts_with('\t') {
            return "\t".to_string();
        }
        let spaces = line.len() - line.trim_start_matches(' ').len();
        if spaces > 0 {
            return " ".repeat(spaces);
        }
    }
    "  ".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_npm_package() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
  "name": "@myorg/utils",
  "version": "2.1.0"
}
"#,
        )
        .unwrap();

        let adapter = NpmAdapter;
        let pkg = adapter.detect(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "@myorg/utils");
        assert_eq!(pkg.version, Version::new(2, 1, 0));
        assert_eq!(pkg.package_type, PackageType::Npm);
    }

    #[test]
    fn detect_no_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = NpmAdapter;
        assert!(adapter.detect(dir.path()).unwrap().is_none());
    }

    #[test]
    fn read_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let pkg_json = dir.path().join("package.json");
        std::fs::write(
            &pkg_json,
            r#"{
  "name": "my-app",
  "version": "1.0.0",
  "description": "A test app",
  "main": "index.js"
}
"#,
        )
        .unwrap();

        let adapter = NpmAdapter;
        let v = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(1, 0, 0));

        adapter
            .write_version(dir.path(), &Version::new(1, 1, 0))
            .unwrap();

        let v2 = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v2, Version::new(1, 1, 0));

        let content = std::fs::read_to_string(&pkg_json).unwrap();
        assert!(content.contains("\"name\": \"my-app\""));
        assert!(content.contains("\"main\": \"index.js\""));
        assert!(content.ends_with('\n'));
    }

    #[test]
    fn preserves_two_space_indent() {
        let dir = tempfile::tempdir().unwrap();
        let pkg_json = dir.path().join("package.json");
        std::fs::write(
            &pkg_json,
            "{\n  \"name\": \"test\",\n  \"version\": \"1.0.0\"\n}\n",
        )
        .unwrap();

        let adapter = NpmAdapter;
        adapter
            .write_version(dir.path(), &Version::new(2, 0, 0))
            .unwrap();

        let content = std::fs::read_to_string(&pkg_json).unwrap();
        assert!(content.contains("\n  \""));
    }

    #[test]
    fn preserves_four_space_indent() {
        let dir = tempfile::tempdir().unwrap();
        let pkg_json = dir.path().join("package.json");
        std::fs::write(
            &pkg_json,
            "{\n    \"name\": \"test\",\n    \"version\": \"1.0.0\"\n}\n",
        )
        .unwrap();

        let adapter = NpmAdapter;
        adapter
            .write_version(dir.path(), &Version::new(2, 0, 0))
            .unwrap();

        let content = std::fs::read_to_string(&pkg_json).unwrap();
        assert!(content.contains("\n    \""));
    }
}
