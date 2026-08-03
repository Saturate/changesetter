use std::path::Path;

use crate::errors::ChangesetterError;
use crate::package::adapter::Adapter;
use crate::package::types::{Package, PackageType, Version};

pub struct HelmAdapter;

impl Adapter for HelmAdapter {
    fn detect(&self, path: &Path) -> anyhow::Result<Option<Package>> {
        let chart_yaml = path.join("Chart.yaml");
        if !chart_yaml.exists() {
            return Ok(None);
        }

        let content =
            std::fs::read_to_string(&chart_yaml).map_err(|e| ChangesetterError::ManifestRead {
                path: chart_yaml.clone(),
                reason: e.to_string(),
            })?;

        let doc: serde_yaml::Value =
            serde_yaml::from_str(&content).map_err(|e| ChangesetterError::ManifestRead {
                path: chart_yaml.clone(),
                reason: e.to_string(),
            })?;

        let name = doc
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let version_str = doc
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0");

        let version = Version::parse(version_str)?;

        Ok(Some(Package {
            name,
            path: path.to_path_buf(),
            package_type: PackageType::Helm,
            version,
        }))
    }

    fn read_version(&self, path: &Path) -> anyhow::Result<Version> {
        let chart_yaml = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("Chart.yaml")
        };

        let content =
            std::fs::read_to_string(&chart_yaml).map_err(|e| ChangesetterError::ManifestRead {
                path: chart_yaml.clone(),
                reason: e.to_string(),
            })?;

        let doc: serde_yaml::Value =
            serde_yaml::from_str(&content).map_err(|e| ChangesetterError::ManifestRead {
                path: chart_yaml.clone(),
                reason: e.to_string(),
            })?;

        let version_str = doc
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("no version found in {}", chart_yaml.display()))?;

        Ok(Version::parse(version_str)?)
    }

    fn write_version(&self, path: &Path, version: &Version) -> anyhow::Result<()> {
        let chart_yaml = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("Chart.yaml")
        };

        let content =
            std::fs::read_to_string(&chart_yaml).map_err(|e| ChangesetterError::ManifestRead {
                path: chart_yaml.clone(),
                reason: e.to_string(),
            })?;

        let mut result = String::new();
        let mut replaced = false;

        for line in content.lines() {
            let trimmed = line.trim_start();
            if !replaced && trimmed.starts_with("version:") {
                let indent = &line[..line.len() - trimmed.len()];
                result.push_str(&format!("{indent}version: {version}"));
                replaced = true;
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }

        if !replaced {
            anyhow::bail!("no version field to update in {}", chart_yaml.display());
        }

        std::fs::write(&chart_yaml, &result).map_err(|e| ChangesetterError::ManifestWrite {
            path: chart_yaml,
            reason: e.to_string(),
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_helm_chart() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Chart.yaml"),
            "apiVersion: v2\nname: my-chart\nversion: 1.2.3\nappVersion: \"4.5.6\"\ndescription: A test chart\n",
        )
        .unwrap();

        let adapter = HelmAdapter;
        let pkg = adapter.detect(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "my-chart");
        assert_eq!(pkg.version, Version::new(1, 2, 3));
        assert_eq!(pkg.package_type, PackageType::Helm);
    }

    #[test]
    fn detect_no_chart_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = HelmAdapter;
        assert!(adapter.detect(dir.path()).unwrap().is_none());
    }

    #[test]
    fn read_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let chart_yaml = dir.path().join("Chart.yaml");
        std::fs::write(
            &chart_yaml,
            "apiVersion: v2\nname: my-chart\nversion: 1.0.0\nappVersion: \"2.0.0\"\ndescription: A Helm chart\ntype: application\n",
        )
        .unwrap();

        let adapter = HelmAdapter;
        let v = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(1, 0, 0));

        adapter
            .write_version(dir.path(), &Version::new(1, 1, 0))
            .unwrap();

        let v2 = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v2, Version::new(1, 1, 0));

        let content = std::fs::read_to_string(&chart_yaml).unwrap();
        assert!(
            content.contains("appVersion: \"2.0.0\""),
            "appVersion must not be touched"
        );
        assert!(content.contains("name: my-chart"));
        assert!(content.contains("description: A Helm chart"));
        assert!(content.contains("type: application"));
    }

    #[test]
    fn write_preserves_field_order() {
        let dir = tempfile::tempdir().unwrap();
        let chart_yaml = dir.path().join("Chart.yaml");
        let original = "apiVersion: v2\nname: my-chart\nversion: 0.1.0\nappVersion: \"1.0.0\"\n# A comment\ndescription: Test\n";
        std::fs::write(&chart_yaml, original).unwrap();

        let adapter = HelmAdapter;
        adapter
            .write_version(dir.path(), &Version::new(0, 2, 0))
            .unwrap();

        let content = std::fs::read_to_string(&chart_yaml).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines[0], "apiVersion: v2");
        assert_eq!(lines[1], "name: my-chart");
        assert_eq!(lines[2], "version: 0.2.0");
        assert_eq!(lines[3], "appVersion: \"1.0.0\"");
        assert_eq!(lines[4], "# A comment");
        assert_eq!(lines[5], "description: Test");
    }
}
