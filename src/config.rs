use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::errors::ChangesetterError;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    #[serde(rename = "package")]
    pub packages: Vec<PackageConfig>,
    #[serde(rename = "groups")]
    pub groups: BTreeMap<String, GroupConfig>,
    pub ignore: Vec<String>,
    pub update_internal_dependencies: Option<String>,
    pub changeset_dir: Option<String>,
    pub changelog: ChangelogConfig,
    pub tag: TagConfig,
    pub release: ReleaseConfig,
    pub hooks: HooksConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageConfig {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub package_type: Option<String>,
    pub members: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct GroupConfig {
    pub fixed: Vec<String>,
    pub linked: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ChangelogConfig {
    pub file: String,
    pub per_package: bool,
    pub none_bump: String,
    pub none_bump_file: Option<String>,
    pub none_bump_heading: String,
}

impl Default for ChangelogConfig {
    fn default() -> Self {
        Self {
            file: "CHANGELOG.md".to_string(),
            per_package: false,
            none_bump: "section".to_string(),
            none_bump_file: None,
            none_bump_heading: "Internal".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TagConfig {
    pub format: Option<String>,
}

impl TagConfig {
    pub fn format_tag(&self, name: &str, version: &str, is_monorepo: bool) -> String {
        if let Some(fmt) = &self.format {
            return fmt.replace("{name}", name).replace("{version}", version);
        }
        if is_monorepo {
            format!("{name}@v{version}")
        } else {
            format!("v{version}")
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ReleaseConfig {
    pub commit_message: String,
    pub tag_annotated: bool,
}

impl Default for ReleaseConfig {
    fn default() -> Self {
        Self {
            commit_message: "chore: release {versions}".to_string(),
            tag_annotated: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct HooksConfig {
    pub post_bump: Vec<String>,
}

impl Config {
    pub fn load(repo_root: &Path) -> anyhow::Result<Self> {
        let config_path = repo_root.join("changesetter.toml");
        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)?;
        let mut config: Config = toml_edit::de::from_str(&content)?;
        config.changeset_dir = config
            .changeset_dir
            .map(|dir| validate_changeset_dir(&dir))
            .transpose()?;
        Ok(config)
    }

    pub fn changeset_dir(&self, repo_root: &Path) -> PathBuf {
        match &self.changeset_dir {
            Some(dir) => repo_root.join(dir),
            None => repo_root.join(".changeset"),
        }
    }

    pub fn changeset_dir_relative(&self) -> &str {
        match &self.changeset_dir {
            Some(dir) => dir.as_str(),
            None => ".changeset",
        }
    }
}

// The dir is used both as a filesystem path and as a git pathspec, so it has
// to stay inside the repo and must not end in `/` (callers append their own).
fn validate_changeset_dir(dir: &str) -> Result<String, ChangesetterError> {
    let invalid = |reason: &str| ChangesetterError::InvalidChangesetDir {
        dir: dir.to_string(),
        reason: reason.to_string(),
    };

    // `\` is a separator on Windows and a plain character elsewhere, so the
    // same config would point at different directories per platform.
    if dir.contains('\\') {
        return Err(invalid("must use `/` as the path separator"));
    }

    let mut has_normal = false;
    for component in Path::new(dir).components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                return Err(invalid("must be a relative path inside the repository"));
            }
            Component::ParentDir => {
                return Err(invalid("must not contain `..`"));
            }
            Component::Normal(_) => has_normal = true,
            Component::CurDir => {}
        }
    }

    if !has_normal {
        return Err(invalid("must name a directory below the repository root"));
    }

    Ok(dir.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = Config::default();
        assert!(config.packages.is_empty());
        assert!(config.ignore.is_empty());
        assert!(config.changeset_dir.is_none());
        assert_eq!(config.changeset_dir_relative(), ".changeset");
        assert_eq!(config.changelog.file, "CHANGELOG.md");
        assert!(!config.changelog.per_package);
        assert_eq!(config.changelog.none_bump, "section");
        assert_eq!(config.changelog.none_bump_heading, "Internal");
        assert!(config.release.tag_annotated);
        assert_eq!(config.release.commit_message, "chore: release {versions}");
    }

    #[test]
    fn load_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.packages.is_empty());
    }

    #[test]
    fn load_full_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("changesetter.toml"),
            r#"
ignore = ["examples"]
update_internal_dependencies = "patch"

[[package]]
name = "mylib"
path = "crates/mylib"
type = "cargo"

[[package]]
name = "my-frontend"
path = "apps/web"
type = "npm"

[groups.core]
fixed = ["core-lib", "core-macros"]

[changelog]
file = "CHANGELOG.md"
per_package = true
none_bump = "section"
none_bump_heading = "Other"

[tag]
format = "{name}@v{version}"

[release]
commit_message = "release: {versions}"
tag_annotated = false

[hooks]
post_bump = ["cargo check", "cargo fmt"]
"#,
        )
        .unwrap();

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.packages.len(), 2);
        assert_eq!(config.packages[0].name, "mylib");
        assert_eq!(config.packages[1].name, "my-frontend");
        assert_eq!(config.ignore, vec!["examples"]);
        assert!(config.changelog.per_package);
        assert_eq!(config.changelog.none_bump_heading, "Other");
        assert_eq!(config.release.commit_message, "release: {versions}");
        assert!(!config.release.tag_annotated);
        assert_eq!(config.hooks.post_bump.len(), 2);
        assert_eq!(config.groups["core"].fixed, vec!["core-lib", "core-macros"]);
    }

    #[test]
    fn load_minimal_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("changesetter.toml"),
            "ignore = [\"test-fixtures\"]\n",
        )
        .unwrap();

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.ignore, vec!["test-fixtures"]);
        assert_eq!(config.changelog.file, "CHANGELOG.md");
    }

    #[test]
    fn custom_changeset_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("changesetter.toml"),
            "changeset_dir = \".changeset/changesets\"\n",
        )
        .unwrap();

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.changeset_dir_relative(), ".changeset/changesets");
        assert_eq!(
            config.changeset_dir(dir.path()),
            dir.path().join(".changeset/changesets")
        );
    }

    fn load_changeset_dir(value: &str) -> anyhow::Result<Config> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("changesetter.toml"),
            format!("changeset_dir = {value:?}\n"),
        )
        .unwrap();
        Config::load(dir.path())
    }

    #[test]
    fn changeset_dir_trailing_slash_normalised() {
        let config = load_changeset_dir(".changeset/").unwrap();
        assert_eq!(config.changeset_dir_relative(), ".changeset");

        let config = load_changeset_dir("docs/changesets//").unwrap();
        assert_eq!(config.changeset_dir_relative(), "docs/changesets");
    }

    #[test]
    fn changeset_dir_backslash_rejected() {
        for value in [".changeset\\\\", "..\\\\x", "C:\\\\x"] {
            let err = load_changeset_dir(value).unwrap_err();
            assert!(err.to_string().contains("`/`"), "{value}: {err}");
        }
    }

    #[test]
    fn changeset_dir_absolute_rejected() {
        let err = load_changeset_dir("/tmp/changesets").unwrap_err();
        assert!(err.to_string().contains("relative path"), "{err}");
    }

    #[test]
    fn changeset_dir_parent_rejected() {
        for value in ["../changesets", ".changeset/../../x", ".."] {
            let err = load_changeset_dir(value).unwrap_err();
            assert!(err.to_string().contains("`..`"), "{value}: {err}");
        }
    }

    #[test]
    fn changeset_dir_repo_root_rejected() {
        for value in ["", ".", "./"] {
            assert!(load_changeset_dir(value).is_err(), "{value}");
        }
    }

    #[test]
    fn tag_format_single_package() {
        let tag = TagConfig::default();
        assert_eq!(tag.format_tag("mylib", "1.0.0", false), "v1.0.0");
    }

    #[test]
    fn tag_format_monorepo() {
        let tag = TagConfig::default();
        assert_eq!(tag.format_tag("mylib", "1.0.0", true), "mylib@v1.0.0");
    }

    #[test]
    fn tag_format_custom() {
        let tag = TagConfig {
            format: Some("{name}-{version}".to_string()),
        };
        assert_eq!(tag.format_tag("mylib", "1.0.0", true), "mylib-1.0.0");
    }
}
