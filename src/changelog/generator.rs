use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::ChangelogConfig;
use crate::package::types::Package;
use crate::release::plan::{NoneEntry, ReleasePlan};

/// Holds none-bump entries that have no version section to live in yet. The
/// next release written to the same changelog folds it into its own section.
pub const UNRELEASED_HEADING: &str = "## Unreleased";

/// The package name a changeset uses for the whole repo. A single-package
/// repo resolves it to the real package; in a monorepo it stays as is.
const REPO_WIDE_PACKAGE: &str = "default";

/// The text a release puts at the top of a new changelog: the version section
/// with its none-bump entries, or only an Unreleased section when nothing is
/// released.
pub fn generate_changelog_entry(
    plan: &ReleasePlan,
    config: &ChangelogConfig,
    date: &str,
    is_monorepo: bool,
) -> String {
    let none: Vec<&NoneEntry> = plan.none_entries.iter().collect();
    let release = release_section(plan, date, is_monorepo, config);
    compose_section(release.as_deref(), None, &none, config).unwrap_or_default()
}

fn release_section(
    plan: &ReleasePlan,
    date: &str,
    is_monorepo: bool,
    config: &ChangelogConfig,
) -> Option<String> {
    if plan.releases.is_empty() {
        return None;
    }
    let mut output = String::new();
    if is_monorepo && !config.per_package {
        let versions: Vec<String> = plan
            .releases
            .iter()
            .map(|r| r.version.to_string())
            .collect();
        output.push_str(&format!("## {} - {date}\n\n", versions.join(", ")));
        for release in &plan.releases {
            output.push_str(&format!("### {}\n\n", release.name));
            if !release.changelog.is_empty() {
                output.push_str(&release.changelog);
                output.push_str("\n\n");
            }
        }
    } else {
        for release in &plan.releases {
            output.push_str(&format!("## {} - {}\n\n", release.version, date));
            if !release.changelog.is_empty() {
                output.push_str(&release.changelog);
                output.push('\n');
            }
        }
    }
    Some(output)
}

/// Builds the section to put at the top of one changelog. `release` is the
/// version section (heading and entries), or `None` to write an Unreleased
/// section. `unreleased` is the body of the changelog's current Unreleased
/// section, which ends up in the new section either way. Returns `None` when
/// there's nothing to write.
fn compose_section(
    release: Option<&str>,
    unreleased: Option<&str>,
    none: &[&NoneEntry],
    config: &ChangelogConfig,
) -> Option<String> {
    let omit = config.none_bump == "omit";
    let (carried, carried_internal) =
        split_unreleased(unreleased.unwrap_or(""), &config.none_bump_heading);
    let has_new_internal = !omit && !none.is_empty();
    // A heading left with nothing under it isn't worth carrying into a release.
    let carried_internal = carried_internal.filter(|t| !t.is_empty());
    if release.is_none() && carried.is_empty() && carried_internal.is_none() && !has_new_internal {
        return None;
    }

    let mut parts: Vec<&str> = vec![release.map_or(UNRELEASED_HEADING, str::trim_end)];
    if !carried.is_empty() {
        parts.push(carried);
    }
    let heading = format!("### {}", config.none_bump_heading);
    if has_new_internal || carried_internal.is_some() {
        parts.push(&heading);
        if let Some(text) = carried_internal {
            parts.push(text);
        }
        if !omit {
            parts.extend(
                none.iter()
                    .map(|n| n.body.trim())
                    .filter(|body| !body.is_empty()),
            );
        }
    }
    Some(format!("{}\n", parts.join("\n\n")))
}

/// Splits an Unreleased body into what sits above its none-bump heading and
/// the entries under it (`None` when it has no such heading), both trimmed.
fn split_unreleased<'a>(body: &'a str, heading: &str) -> (&'a str, Option<&'a str>) {
    let marker = format!("### {heading}");
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        if line.trim_end() == marker {
            let above = body[..offset].trim();
            let below = body[offset + line.len()..].trim();
            return (above, Some(below));
        }
        offset += line.len();
    }
    (body.trim(), None)
}

/// Same rule as actions/release/release-notes.sh: only a heading that starts
/// with a semver is a version heading, since changeset bodies may carry "## "
/// headings of their own.
fn is_version_heading(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("## ") else {
        return false;
    };
    let mut parts = rest.splitn(3, '.');
    let numeric = |s: Option<&str>, whole: bool| {
        s.is_some_and(|s| {
            let digits = if whole {
                s
            } else {
                let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
                &s[..end]
            };
            !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
        })
    };
    numeric(parts.next(), true) && numeric(parts.next(), true) && numeric(parts.next(), false)
}

/// Where an Unreleased section ends. Wider than `is_version_heading` because
/// it also has to stop at history other tools wrote, like keep-a-changelog's
/// "## [1.0.0]" or "## v1.0.0", or new entries would land at the end of the file.
fn ends_unreleased(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("## ") else {
        return false;
    };
    let rest = rest.strip_prefix('[').unwrap_or(rest);
    let rest = rest
        .strip_prefix('v')
        .or_else(|| rest.strip_prefix('V'))
        .unwrap_or(rest);
    is_version_heading(&format!("## {rest}"))
}

fn is_unreleased_heading(line: &str) -> bool {
    line == UNRELEASED_HEADING || line == "## [Unreleased]"
}

/// Removes the Unreleased section from a changelog, returning the rest of the
/// changelog and the section's body. Only a heading above the first version
/// heading counts, so a changeset body that happens to contain one is left
/// alone. The section runs to the next version heading.
fn take_unreleased(content: &str) -> (String, Option<String>) {
    let mut offset = 0;
    let mut start = None;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end();
        match start {
            None if ends_unreleased(trimmed) => break,
            None if is_unreleased_heading(trimmed) => start = Some((offset, offset + line.len())),
            Some((head, body)) if ends_unreleased(trimmed) => {
                let rest = format!("{}{}", &content[..head], &content[offset..]);
                return (rest, Some(content[body..offset].to_string()));
            }
            _ => {}
        }
        offset += line.len();
    }
    match start {
        Some((head, body)) => (
            content[..head].to_string(),
            Some(content[body..].to_string()),
        ),
        None => (content.to_string(), None),
    }
}

/// Writes one changelog's part of a release: `release` is its version section,
/// or `None` when this changelog has no release and only none-bump entries.
/// An existing Unreleased section is folded into a new version section, or
/// extended when there's still no release. Returns whether the file changed.
pub fn write_changelog(
    path: &Path,
    release: Option<&str>,
    none: &[&NoneEntry],
    config: &ChangelogConfig,
) -> anyhow::Result<bool> {
    if release.is_none() && (none.is_empty() || config.none_bump == "omit") {
        return Ok(false);
    }
    let existing = if path.exists() {
        Some(std::fs::read_to_string(path)?)
    } else {
        None
    };
    let (rest, unreleased) = existing
        .as_deref()
        .map_or((String::new(), None), take_unreleased);
    let Some(section) = compose_section(release, unreleased.as_deref(), none, config) else {
        return Ok(false);
    };
    let content = if existing.is_some() {
        insert_after_header(&rest, &section)
    } else {
        format!("# Changelog\n\n{section}")
    };
    std::fs::write(path, content)?;
    Ok(true)
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
    let entry = new_entry.trim_end();
    let (header, rest) = if let Some(pos) = existing.find("\n## ") {
        (&existing[..pos], &existing[pos..])
    } else if existing.starts_with("# ") {
        existing.split_at(existing.find('\n').unwrap_or(existing.len()))
    } else {
        ("", existing)
    };
    let header = header.trim_end();
    let rest = rest.trim_start_matches(['\n', '\r']);
    let mut out = String::new();
    if !header.is_empty() {
        out.push_str(header);
        out.push_str("\n\n");
    }
    out.push_str(entry);
    out.push('\n');
    if !rest.is_empty() {
        out.push('\n');
        out.push_str(rest);
    }
    out
}

/// Writes the root changelog for single-package repos and grouped monorepos.
/// Returns the path when the file changed.
pub fn write_root_changelog(
    repo_root: &Path,
    plan: &ReleasePlan,
    config: &ChangelogConfig,
    date: &str,
    is_monorepo: bool,
) -> anyhow::Result<Vec<PathBuf>> {
    let path = repo_root.join(&config.file);
    let release = release_section(plan, date, is_monorepo, config);
    let none: Vec<&NoneEntry> = plan.none_entries.iter().collect();
    let changed = write_changelog(&path, release.as_deref(), &none, config)?;
    Ok(if changed { vec![path] } else { vec![] })
}

/// Writes each package's own changelog. A none-bump entry goes to every
/// package it names, released or not, and a repo-wide `default` entry goes to
/// every package, since per-package mode has no root changelog. Returns the
/// paths that changed.
pub fn write_per_package_changelogs(
    plan: &ReleasePlan,
    packages: &[Package],
    config: &ChangelogConfig,
    date: &str,
) -> anyhow::Result<Vec<PathBuf>> {
    #[derive(Default)]
    struct Target<'a> {
        names: Vec<&'a str>,
        sections: Vec<String>,
        none: Vec<&'a NoneEntry>,
    }
    // Keyed by path: two packages in one directory share a changelog.
    let mut targets: BTreeMap<PathBuf, Target> = BTreeMap::new();
    for release in &plan.releases {
        let t = targets.entry(release.path.join(&config.file)).or_default();
        t.names.push(&release.name);
        t.sections.push(format!(
            "## {} - {}\n\n{}",
            release.version, date, release.changelog
        ));
    }
    for pkg in packages {
        targets
            .entry(pkg.path.join(&config.file))
            .or_default()
            .names
            .push(&pkg.name);
    }

    let has_real_default = packages.iter().any(|p| p.name == REPO_WIDE_PACKAGE);
    for entry in &plan.none_entries {
        let repo_wide = !has_real_default && entry.packages.iter().any(|p| p == REPO_WIDE_PACKAGE);
        for t in targets.values_mut() {
            if repo_wide
                || t.names
                    .iter()
                    .any(|n| entry.packages.iter().any(|p| p == n))
            {
                t.none.push(entry);
            }
        }
    }

    let mut written = Vec::new();
    for (path, t) in &targets {
        let release = (!t.sections.is_empty()).then(|| t.sections.join("\n\n"));
        if write_changelog(path, release.as_deref(), &t.none, config)? {
            written.push(path.clone());
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changeset::types::BumpLevel;
    use crate::package::types::{PackageType, Version};
    use crate::release::plan::PlannedRelease;

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
                packages: vec!["mylib".to_string()],
            }],
        );
        let config = ChangelogConfig::default();
        let entry = generate_changelog_entry(&plan, &config, "2026-07-25", false);

        assert!(entry.contains("#### Feature\n\n### Internal\n\n"));
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
                packages: vec!["mylib".to_string()],
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
            vec![NoneEntry {
                title: "CI".to_string(),
                body: "#### CI".to_string(),
                changesets: vec!["cs3".to_string()],
                packages: vec!["mylib".to_string()],
            }],
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
        assert!(entry.contains("#### UI fix\n\n### Internal\n\n"));
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

    fn none_entry(body: &str, packages: &[&str]) -> NoneEntry {
        NoneEntry {
            title: body.trim_start_matches("#### ").to_string(),
            body: body.to_string(),
            changesets: vec!["cs".to_string()],
            packages: packages.iter().map(|p| p.to_string()).collect(),
        }
    }

    fn package(dir: &Path, name: &str) -> Package {
        let path = dir.join(name);
        std::fs::create_dir_all(&path).unwrap();
        Package {
            name: name.to_string(),
            path,
            package_type: PackageType::Npm,
            version: Version::new(1, 0, 0),
        }
    }

    /// Packages `a` and `b` under `dir`, with `a` released.
    fn per_package_plan(dir: &Path, none: Vec<NoneEntry>) -> (ReleasePlan, Vec<Package>) {
        let packages = vec![package(dir, "a"), package(dir, "b")];
        let mut release = make_release("a", "1.0.0", "1.1.0", BumpLevel::Minor, "#### Feature");
        release.path = packages[0].path.clone();
        (make_plan(vec![release], none), packages)
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn per_package_writes_own_none_entries() {
        let dir = tempfile::tempdir().unwrap();
        let (plan, packages) = per_package_plan(
            dir.path(),
            vec![
                none_entry("#### CI for a", &["a"]),
                none_entry("#### CI for b", &["b"]),
            ],
        );
        let written =
            write_per_package_changelogs(&plan, &packages, &ChangelogConfig::default(), "d")
                .unwrap();
        assert_eq!(written.len(), 2);

        let a = read(&dir.path().join("a/CHANGELOG.md"));
        assert_eq!(
            a,
            "# Changelog\n\n## 1.1.0 - d\n\n#### Feature\n\n### Internal\n\n#### CI for a\n"
        );
        let b = read(&dir.path().join("b/CHANGELOG.md"));
        assert_eq!(
            b,
            "# Changelog\n\n## Unreleased\n\n### Internal\n\n#### CI for b\n"
        );
    }

    #[test]
    fn per_package_repo_wide_none_goes_to_every_package() {
        let dir = tempfile::tempdir().unwrap();
        let (mut plan, packages) =
            per_package_plan(dir.path(), vec![none_entry("#### Docs", &["default"])]);
        plan.releases.clear();
        write_per_package_changelogs(&plan, &packages, &ChangelogConfig::default(), "d").unwrap();

        for name in ["a", "b"] {
            let content = read(&dir.path().join(name).join("CHANGELOG.md"));
            assert!(content.contains("## Unreleased\n\n### Internal\n\n#### Docs\n"));
        }
    }

    #[test]
    fn per_package_without_none_entries_leaves_other_packages_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (plan, packages) = per_package_plan(dir.path(), vec![]);
        write_per_package_changelogs(&plan, &packages, &ChangelogConfig::default(), "d").unwrap();
        assert!(dir.path().join("a/CHANGELOG.md").exists());
        assert!(!dir.path().join("b/CHANGELOG.md").exists());
    }

    #[test]
    fn per_package_omits_none_entries() {
        let dir = tempfile::tempdir().unwrap();
        let (plan, packages) = per_package_plan(
            dir.path(),
            vec![
                none_entry("#### CI for a", &["a"]),
                none_entry("#### CI for b", &["b"]),
            ],
        );
        let config = ChangelogConfig {
            none_bump: "omit".to_string(),
            ..Default::default()
        };
        let written = write_per_package_changelogs(&plan, &packages, &config, "d").unwrap();
        assert_eq!(written, vec![dir.path().join("a/CHANGELOG.md")]);

        let content = read(&dir.path().join("a/CHANGELOG.md"));
        assert!(content.contains("#### Feature"));
        assert!(!content.contains("Internal"));
        assert!(!content.contains("CI for"));
        assert!(!dir.path().join("b/CHANGELOG.md").exists());
    }

    #[test]
    fn none_only_appends_to_an_existing_unreleased_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CHANGELOG.md");
        std::fs::write(
            &path,
            "# Changelog\n\n## Unreleased\n\n### Internal\n\n#### Old\n\n## 1.0.0 - d\n\n- First\n",
        )
        .unwrap();
        let entry = none_entry("#### New", &["x"]);
        write_changelog(&path, None, &[&entry], &ChangelogConfig::default()).unwrap();
        assert_eq!(
            read(&path),
            "# Changelog\n\n## Unreleased\n\n### Internal\n\n#### Old\n\n#### New\n\n## 1.0.0 - d\n\n- First\n"
        );
    }

    #[test]
    fn release_folds_the_unreleased_section_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CHANGELOG.md");
        std::fs::write(
            &path,
            "# Changelog\n\n## Unreleased\n\n### Internal\n\n#### Old\n\n## 1.0.0 - d\n\n- First\n",
        )
        .unwrap();
        let entry = none_entry("#### New", &["x"]);
        write_changelog(
            &path,
            Some("## 1.1.0 - d\n\n#### Feature\n"),
            &[&entry],
            &ChangelogConfig::default(),
        )
        .unwrap();
        assert_eq!(
            read(&path),
            "# Changelog\n\n## 1.1.0 - d\n\n#### Feature\n\n### Internal\n\n#### Old\n\n#### New\n\n## 1.0.0 - d\n\n- First\n"
        );
    }

    #[test]
    fn release_keeps_hand_written_unreleased_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CHANGELOG.md");
        std::fs::write(&path, "# Changelog\n\n## Unreleased\n\n- Hand-written\n").unwrap();
        let config = ChangelogConfig {
            none_bump: "omit".to_string(),
            ..Default::default()
        };
        write_changelog(&path, Some("## 1.1.0 - d\n\n#### Feature"), &[], &config).unwrap();
        assert_eq!(
            read(&path),
            "# Changelog\n\n## 1.1.0 - d\n\n#### Feature\n\n- Hand-written\n"
        );
    }

    #[test]
    fn omit_never_writes_an_unreleased_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CHANGELOG.md");
        let config = ChangelogConfig {
            none_bump: "omit".to_string(),
            ..Default::default()
        };
        let entry = none_entry("#### CI", &["x"]);
        assert!(!write_changelog(&path, None, &[&entry], &config).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn unreleased_stops_at_headings_other_tools_write() {
        for older in ["## [1.0.0] - 2020", "## v1.0.0"] {
            let content = format!(
                "# C\n\n## [Unreleased]\n\n### Internal\n\n- old\n\n{older}\n\n### Internal\n\n- ancient\n"
            );
            let (rest, unreleased) = take_unreleased(&content);
            assert_eq!(unreleased.as_deref(), Some("\n### Internal\n\n- old\n\n"));
            assert_eq!(
                rest,
                format!("# C\n\n{older}\n\n### Internal\n\n- ancient\n")
            );
        }
    }

    #[test]
    fn an_empty_internal_heading_is_not_carried_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CHANGELOG.md");
        std::fs::write(&path, "# Changelog\n\n## Unreleased\n\n### Internal\n").unwrap();
        write_changelog(
            &path,
            Some("## 1.1.0 - d\n\n#### Feature"),
            &[],
            &ChangelogConfig::default(),
        )
        .unwrap();
        assert_eq!(read(&path), "# Changelog\n\n## 1.1.0 - d\n\n#### Feature\n");
    }

    #[test]
    fn unreleased_below_a_version_is_not_taken() {
        let content = "# C\n\n## 1.0.0 - d\n\n## Unreleased\n\nbody\n";
        let (rest, unreleased) = take_unreleased(content);
        assert_eq!(rest, content);
        assert!(unreleased.is_none());
    }

    #[test]
    fn version_headings_need_a_semver() {
        assert!(is_version_heading("## 1.0.0 - d"));
        assert!(is_version_heading("## 1.0.0-rc.1, 2.0.0"));
        assert!(!is_version_heading("## Unreleased"));
        assert!(!is_version_heading("## 2FA support"));
        assert!(!is_version_heading("## 1.x.0"));
        assert!(!is_version_heading("### 1.0.0"));
    }

    #[test]
    fn custom_none_heading() {
        let plan = make_plan(
            vec![],
            vec![NoneEntry {
                title: "CI".to_string(),
                body: "#### CI".to_string(),
                changesets: vec![],
                packages: vec!["mylib".to_string()],
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
