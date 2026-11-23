use std::path::Path;

use crate::package::types::{Package, Version};

pub trait Adapter {
    fn detect(&self, path: &Path) -> anyhow::Result<Option<Package>>;
    /// Detection for a path listed under `[[package]]` in changesetter.toml.
    /// Adapters that skip manifests during auto-detection (e.g. a Godot
    /// project without a version) can accept them here.
    fn detect_configured(&self, path: &Path) -> anyhow::Result<Option<Package>> {
        self.detect(path)
    }
    fn read_version(&self, path: &Path) -> anyhow::Result<Version>;
    fn write_version(&self, path: &Path, version: &Version) -> anyhow::Result<()>;
    fn dependencies(&self, _path: &Path) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
}
