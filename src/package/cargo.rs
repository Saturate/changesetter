use std::path::Path;

use crate::errors::ChangesetterError;
use crate::package::adapter::Adapter;
use crate::package::types::{Package, PackageType, Version};

pub struct CargoAdapter;

impl CargoAdapter {
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
}

impl Adapter for CargoAdapter {
    fn detect(&self, path: &Path) -> anyhow::Result<Option<Package>> {
        let cargo_toml = path.join("Cargo.toml");
        if !cargo_toml.exists() {
            return Ok(None);
        }

        let doc = Self::read_document(&cargo_toml)?;

        if let Some(pkg) = doc.get("package") {
            let name = pkg
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let version_str = pkg
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0");
            let version = Version::parse(version_str)?;

            return Ok(Some(Package {
                name,
                path: path.to_path_buf(),
                package_type: PackageType::Cargo,
                version,
            }));
        }

        if let Some(ws) = doc.get("workspace") {
            if let Some(ws_pkg) = ws.get("package") {
                let name = ws_pkg
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let version_str = ws_pkg
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0.0.0");
                let version = Version::parse(version_str)?;

                return Ok(Some(Package {
                    name,
                    path: path.to_path_buf(),
                    package_type: PackageType::CargoWorkspace,
                    version,
                }));
            }
        }

        Ok(None)
    }

    fn read_version(&self, path: &Path) -> anyhow::Result<Version> {
        let cargo_toml = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("Cargo.toml")
        };
        let doc = Self::read_document(&cargo_toml)?;

        if let Some(v) = doc
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
        {
            return Ok(Version::parse(v)?);
        }

        if let Some(v) = doc
            .get("workspace")
            .and_then(|w| w.get("package"))
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
        {
            return Ok(Version::parse(v)?);
        }

        anyhow::bail!("no version found in {}", cargo_toml.display());
    }

    fn write_version(&self, path: &Path, version: &Version) -> anyhow::Result<()> {
        let cargo_toml = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("Cargo.toml")
        };
        let mut doc = Self::read_document(&cargo_toml)?;

        if doc.get("package").and_then(|p| p.get("version")).is_some() {
            doc["package"]["version"] = toml_edit::value(version.to_string());
        } else if doc
            .get("workspace")
            .and_then(|w| w.get("package"))
            .and_then(|p| p.get("version"))
            .is_some()
        {
            doc["workspace"]["package"]["version"] = toml_edit::value(version.to_string());
        } else {
            anyhow::bail!("no version field to update in {}", cargo_toml.display());
        }

        std::fs::write(&cargo_toml, doc.to_string()).map_err(|e| {
            ChangesetterError::ManifestWrite {
                path: cargo_toml,
                reason: e.to_string(),
            }
        })?;

        Ok(())
    }

    fn post_bump_hook(&self, _path: &Path) -> Option<String> {
        Some("cargo check".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_single_crate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "mylib"
version = "1.2.3"
edition = "2024"
"#,
        )
        .unwrap();

        let adapter = CargoAdapter;
        let pkg = adapter.detect(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "mylib");
        assert_eq!(pkg.version, Version::new(1, 2, 3));
        assert_eq!(pkg.package_type, PackageType::Cargo);
    }

    #[test]
    fn detect_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[workspace]
members = ["crates/*"]

[workspace.package]
name = "my-workspace"
version = "0.5.0"
"#,
        )
        .unwrap();

        let adapter = CargoAdapter;
        let pkg = adapter.detect(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "my-workspace");
        assert_eq!(pkg.version, Version::new(0, 5, 0));
        assert_eq!(pkg.package_type, PackageType::CargoWorkspace);
    }

    #[test]
    fn detect_no_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = CargoAdapter;
        assert!(adapter.detect(dir.path()).unwrap().is_none());
    }

    #[test]
    fn read_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        std::fs::write(
            &cargo_toml,
            r#"[package]
name = "mylib"
version = "1.0.0"
edition = "2024"

[dependencies]
serde = "1"
"#,
        )
        .unwrap();

        let adapter = CargoAdapter;
        let v = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(1, 0, 0));

        let new_v = Version::new(1, 1, 0);
        adapter.write_version(dir.path(), &new_v).unwrap();

        let v2 = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v2, Version::new(1, 1, 0));

        // Verify other fields preserved
        let content = std::fs::read_to_string(&cargo_toml).unwrap();
        assert!(content.contains("name = \"mylib\""));
        assert!(content.contains("serde = \"1\""));
        assert!(content.contains("edition = \"2024\""));
    }

    #[test]
    fn write_workspace_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[workspace]
members = ["crates/*"]

[workspace.package]
name = "my-ws"
version = "0.1.0"
"#,
        )
        .unwrap();

        let adapter = CargoAdapter;
        adapter
            .write_version(dir.path(), &Version::new(0, 2, 0))
            .unwrap();
        let v = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(0, 2, 0));
    }

    #[test]
    fn post_bump_hook_returns_cargo_check() {
        let adapter = CargoAdapter;
        assert_eq!(
            adapter.post_bump_hook(Path::new(".")),
            Some("cargo check".to_string())
        );
    }
}
