use std::path::Path;

use crate::config::ChangelogConfig;
use crate::release::plan::{NoneEntry, PlannedRelease, ReleasePlan};

pub fn generate_changelog_entry(
    plan: &ReleasePlan,
    config: &ChangelogConfig,
    date: &str,
    is_monorepo: bool,
) -> String {
    let mut output = String::new();

    if is_monorepo && !config.per_package {
        generate_grouped_entry(&mut output, plan, config, date);
    } else {
        generate_single_entry(&mut output, plan, config, date);
    }

    output
}

fn generate_single_entry(
    output: &mut String,
    plan: &ReleasePlan,
    config: &ChangelogConfig,
    date: &str,
) {
    for release in &plan.releases {
        output.push_str(&format!("## {} - {}\n\n", release.version, date));
        if !release.changelog.is_empty() {
            output.push_str(&release.changelog);
            output.push('\n');
        }
    }

    append_none_entries(output, &plan.none_entries, config);
}

fn generate_grouped_entry(
    output: &mut String,
    plan: &ReleasePlan,
    config: &ChangelogConfig,
    date: &str,
) {
    let versions: Vec<String> = plan
        .releases
        .iter()
        .map(|r| r.version.to_string())
        .collect();
    let version_str = versions.join(", ");

    output.push_str(&format!("## {version_str} - {date}\n\n"));

    for release in &plan.releases {
        output.push_str(&format!("### {}\n\n", release.name));
        if !release.changelog.is_empty() {
            output.push_str(&release.changelog);
            output.push_str("\n\n");
        }
    }

    append_none_entries(output, &plan.none_entries, config);
}

fn append_none_entries(output: &mut String, entries: &[NoneEntry], config: &ChangelogConfig) {
    if entries.is_empty() || config.none_bump == "omit" {
        return;
    }

    output.push_str(&format!("### {}\n\n", config.none_bump_heading));
    for entry in entries {
        if !entry.body.is_empty() {
            output.push_str(&entry.body);
            output.push_str("\n\n");
        }
    }
}

pub fn update_changelog_file(changelog_path: &Path, new_entry: &str) -> anyhow::Result<()> {
    if changelog_path.exists() {
        let existing = std::fs::read_to_string(changelog_path)?;
        let content = insert_after_header(&existing, new_entry);
        std::fs::write(changelog_path, content)?;
    } else {
        let content = format!("# Changelog\n\n{new_entry}");
        std::fs::write(changelog_path, content)?;
    }
    Ok(())
}

fn insert_after_header(existing: &str, new_entry: &str) -> String {
    if let Some(pos) = existing.find("\n## ") {
        let header = &existing[..pos];
        let rest = &existing[pos..];
        format!("{header}\n\n{new_entry}{rest}")
    } else if existing.starts_with("# ") {
        let first_line_end = existing.find('\n').unwrap_or(existing.len());
        let header = &existing[..first_line_end];
        let rest = &existing[first_line_end..];
        format!("{header}\n\n{new_entry}{rest}")
    } else {
        format!("{new_entry}\n{existing}")
    }
}

pub fn write_per_package_changelogs(
    plan: &ReleasePlan,
    config: &ChangelogConfig,
    date: &str,
) -> anyhow::Result<()> {
    for release in &plan.releases {
        let changelog_path = release_changelog_path(release, config);
        let entry = format!(
            "## {} - {}\n\n{}\n",
            release.version, date, release.changelog
        );
        update_changelog_file(&changelog_path, &entry)?;
    }
    Ok(())
}

fn release_changelog_path(
    release: &PlannedRelease,
    config: &ChangelogConfig,
) -> std::path::PathBuf {
    release.path.join(&config.file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changeset::types::BumpLevel;
    use crate::package::types::Version;

    fn make_plan(releases: Vec<PlannedRelease>, none: Vec<NoneEntry>) -> ReleasePlan {
        ReleasePlan {
            releases,
            none_entries: none,
        }
    }

    fn make_release(
        name: &str,
        prev: &str,
        next: &str,
        bump: BumpLevel,
        changelog: &str,
    ) -> PlannedRelease {
        PlannedRelease {
            name: name.to_string(),
            path: std::path::PathBuf::from("."),
            version: Version::parse(next).unwrap(),
            previous_version: Version::parse(prev).unwrap(),
            bump,
            changelog: changelog.to_string(),
            changesets: vec!["cs1".to_string()],
        }
    }

    #[test]
    fn single_release_entry() {
        let plan = make_plan(
            vec![make_release(
                "mylib",
                "0.1.0",
                "0.2.0",
                BumpLevel::Minor,
                "#### Added retry logic\n\nRetries 3 times.",
            )],
            vec![],
        );
        let config = ChangelogConfig::default();
        let entry = generate_changelog_entry(&plan, &config, "2026-07-25", false);

        assert!(entry.contains("## 0.2.0 - 2026-07-25"));
        assert!(entry.contains("#### Added retry logic"));
        assert!(entry.contains("Retries 3 times."));
    }

    #[test]
    fn none_bump_section() {
        let plan = make_plan(
            vec![make_release(
                "mylib",
                "0.1.0",
                "0.2.0",
                BumpLevel::Minor,
                "#### Feature",
            )],
            vec![NoneEntry {
                title: "Updated CI".to_string(),
                body: "#### Updated CI\n\nSwitched runners.".to_string(),
                changesets: vec!["cs2".to_string()],
            }],
        );
        let config = ChangelogConfig::default();
        let entry = generate_changelog_entry(&plan, &config, "2026-07-25", false);

        assert!(entry.contains("### Internal"));
        assert!(entry.contains("Updated CI"));
    }

    #[test]
    fn none_bump_omit() {
        let plan = make_plan(
            vec![],
            vec![NoneEntry {
                title: "CI".to_string(),
                body: "#### CI".to_string(),
                changesets: vec![],
            }],
        );
        let config = ChangelogConfig {
            none_bump: "omit".to_string(),
            ..Default::default()
        };
        let entry = generate_changelog_entry(&plan, &config, "2026-07-25", false);
        assert!(!entry.contains("Internal"));
    }

    #[test]
    fn grouped_monorepo_entry() {
        let plan = make_plan(
            vec![
                make_release(
                    "mylib",
                    "0.1.0",
                    "0.2.0",
                    BumpLevel::Minor,
                    "#### API change",
                ),
                make_release(
                    "frontend",
                    "1.0.0",
                    "1.0.1",
                    BumpLevel::Patch,
                    "#### UI fix",
                ),
            ],
            vec![],
        );
        let config = ChangelogConfig {
            per_package: false,
            ..Default::default()
        };
        let entry = generate_changelog_entry(&plan, &config, "2026-07-25", true);

        assert!(entry.contains("### mylib"));
        assert!(entry.contains("### frontend"));
        assert!(entry.contains("API change"));
        assert!(entry.contains("UI fix"));
    }

    #[test]
    fn update_existing_changelog() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CHANGELOG.md");
        std::fs::write(
            &path,
            "# Changelog\n\n## 0.1.0 - 2026-07-01\n\nInitial release.\n",
        )
        .unwrap();

        let new_entry = "## 0.2.0 - 2026-07-25\n\n#### Fix\n\n";
        update_changelog_file(&path, new_entry).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("# Changelog\n"));
        let v020_pos = content.find("## 0.2.0").unwrap();
        let v010_pos = content.find("## 0.1.0").unwrap();
        assert!(v020_pos < v010_pos);
    }

    #[test]
    fn create_new_changelog() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CHANGELOG.md");

        let new_entry = "## 0.1.0 - 2026-07-25\n\n#### Initial\n\n";
        update_changelog_file(&path, new_entry).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("# Changelog"));
        assert!(content.contains("## 0.1.0"));
    }

    #[test]
    fn custom_none_heading() {
        let plan = make_plan(
            vec![],
            vec![NoneEntry {
                title: "CI".to_string(),
                body: "#### CI".to_string(),
                changesets: vec![],
            }],
        );
        let config = ChangelogConfig {
            none_bump_heading: "Misc".to_string(),
            ..Default::default()
        };
        let entry = generate_changelog_entry(&plan, &config, "2026-07-25", false);
        assert!(entry.contains("### Misc"));
    }
}
