use std::path::Path;

use crate::package::types::{Package, Version};

pub trait Adapter {
    fn detect(&self, path: &Path) -> anyhow::Result<Option<Package>>;
    fn read_version(&self, path: &Path) -> anyhow::Result<Version>;
    fn write_version(&self, path: &Path, version: &Version) -> anyhow::Result<()>;
    fn dependencies(&self, _path: &Path) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
}
