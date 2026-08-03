use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreState {
    pub mode: String,
    pub tag: String,
    #[serde(default)]
    pub packages_released: BTreeMap<String, u64>,
}

pub fn read_pre_state(changeset_dir: &Path) -> Option<PreState> {
    let path = changeset_dir.join("pre.json");
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn write_pre_state(changeset_dir: &Path, state: &PreState) -> anyhow::Result<()> {
    let path = changeset_dir.join("pre.json");
    let content = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn remove_pre_state(changeset_dir: &Path) -> anyhow::Result<()> {
    let path = changeset_dir.join("pre.json");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_pre_state(dir.path()).is_none());
    }

    #[test]
    fn write_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let state = PreState {
            mode: "pre".to_string(),
            tag: "rc".to_string(),
            packages_released: BTreeMap::from([("mylib".to_string(), 2)]),
        };
        write_pre_state(dir.path(), &state).unwrap();

        let read = read_pre_state(dir.path()).unwrap();
        assert_eq!(read.mode, "pre");
        assert_eq!(read.tag, "rc");
        assert_eq!(read.packages_released["mylib"], 2);
    }

    #[test]
    fn remove_deletes_file() {
        let dir = tempfile::tempdir().unwrap();
        let state = PreState {
            mode: "pre".to_string(),
            tag: "rc".to_string(),
            packages_released: BTreeMap::new(),
        };
        write_pre_state(dir.path(), &state).unwrap();
        assert!(dir.path().join("pre.json").exists());

        remove_pre_state(dir.path()).unwrap();
        assert!(!dir.path().join("pre.json").exists());
    }

    #[test]
    fn remove_missing_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        remove_pre_state(dir.path()).unwrap();
    }

    #[test]
    fn write_creates_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let state = PreState {
            mode: "pre".to_string(),
            tag: "beta".to_string(),
            packages_released: BTreeMap::new(),
        };
        write_pre_state(dir.path(), &state).unwrap();

        let content = std::fs::read_to_string(dir.path().join("pre.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["mode"], "pre");
        assert_eq!(parsed["tag"], "beta");
    }
}
