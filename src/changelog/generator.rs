use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::ChangelogConfig;
use crate::package::types::{Package, PackageType};
use crate::release::plan::{NoneEntry, ReleasePlan};

/// Holds none-bump entries that have no version section to live in yet. The
/// next release written to the same changelog folds it into its own section.
pub const UNRELEASED_HEADING: &str = "## Unreleased";
const UNRELEASED_HEADING_BRACKETED: &str = "## [Unreleased]";

/// Whitespace as release-notes.sh sees it (awk's `[ \t\r]` plus newlines).
/// Trimming only these keeps the Rust side's notes byte for byte the same as
/// what the script reads back, where Unicode-aware `trim` would also eat
/// characters like U+00A0 that awk keeps.
const WS: [char; 4] = [' ', '\t', '\r', '\n'];

/// The package name a changeset uses for the whole repo. A single-package
/// repo resolves it to the real package; in a monorepo it stays as is.
const REPO_WIDE_PACKAGE: &str = "default";

/// What a release run wrote to the changelogs.
#[derive(Debug, Default)]
pub struct ChangelogWrites {
    /// Changelogs that changed, to stage in the release commit.
    pub paths: Vec<PathBuf>,
    /// Per released package, what its version section holds below its own
    /// entries (absorbed Unreleased notes and the none-bump section), so tag
    /// messages and release notes match the changelog.
    pub extra_notes: BTreeMap<String, String>,
}

impl ChangelogWrites {
    /// Appends each release's extra notes to its `changelog`.
    pub fn apply_to(&self, plan: &mut ReleasePlan) {
        for release in &mut plan.releases {
            let Some(extra) = self.extra_notes.get(&release.name) else {
                continue;
            };
            if extra.is_empty() {
                continue;
            }
            release.changelog = if release.changelog.trim_matches(WS).is_empty() {
                extra.clone()
            } else {
                format!("{}\n\n{extra}", release.changelog.trim_end_matches(WS))
            };
        }
    }
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

/// The contents of a changelog's Unreleased sections, split around their
/// none-bump subsections so new none-bump entries join the existing ones.
#[derive(Debug, Default)]
struct Folded<'a> {
    before: Vec<&'a str>,
    internal: Vec<&'a str>,
    has_internal: bool,
    after: Vec<&'a str>,
}

impl<'a> Folded<'a> {
    fn new(bodies: &'a [String], heading: &str) -> Self {
        let mut folded = Self::default();
        for body in bodies {
            let (before, internal, after) = split_unreleased(body, heading);
            folded.before.extend(Some(before).filter(|s| !s.is_empty()));
            if let Some(internal) = internal {
                folded.has_internal = true;
                folded
                    .internal
                    .extend(Some(internal).filter(|s| !s.is_empty()));
            }
            folded.after.extend(Some(after).filter(|s| !s.is_empty()));
        }
        folded
    }
}

/// Builds the section to put at the top of one changelog. `release` is the
/// version section (heading and entries), or `None` to write an Unreleased
/// section under `unreleased_heading`. Returns the section and the part of it
/// below the release's own entries, or `None` when there's nothing to write.
fn compose_section(
    release: Option<&str>,
    unreleased_heading: &str,
    folded: &Folded,
    none: &[&NoneEntry],
    config: &ChangelogConfig,
) -> Option<(String, String)> {
    let omit = config.none_bump == "omit";
    let new_internal: Vec<&str> = if omit {
        Vec::new()
    } else {
        none.iter()
            .map(|n| n.body.trim_matches(WS))
            .filter(|body| !body.is_empty())
            .collect()
    };
    let mut extra: Vec<&str> = folded.before.clone();
    let heading = format!("### {}", config.none_bump_heading);
    // A heading left with nothing under it isn't worth carrying over.
    if (!omit && !none.is_empty()) || !folded.internal.is_empty() {
        extra.push(&heading);
        extra.extend(&folded.internal);
        extra.extend(new_internal);
    }
    extra.extend(&folded.after);
    let extra = extra.join("\n\n");

    if release.is_none() && extra.is_empty() {
        return None;
    }
    let head = release.map_or(unreleased_heading, |r| r.trim_end_matches(WS));
    let section = if extra.is_empty() {
        format!("{head}\n")
    } else {
        format!("{head}\n\n{extra}\n")
    };
    Some((section, extra))
}

/// Splits an Unreleased body around its none-bump subsection: the text above
/// it, the subsection's entries (`None` when there's no such heading), and
/// the text after it. The subsection ends at the next heading of level 3 or
/// higher, so hand-written subsections like "### Added" stay separate.
fn split_unreleased<'a>(body: &'a str, heading: &str) -> (&'a str, Option<&'a str>, &'a str) {
    let marker = format!("### {heading}");
    let mut offset = 0;
    let mut internal_start = None;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(WS);
        match internal_start {
            None if trimmed == marker => internal_start = Some((offset, offset + line.len())),
            Some((head, start)) if trimmed.starts_with("### ") || trimmed.starts_with("## ") => {
                return (
                    body[..head].trim_matches(WS),
                    Some(body[start..offset].trim_matches(WS)),
                    body[offset..].trim_matches(WS),
                );
            }
            _ => {}
        }
        offset += line.len();
    }
    match internal_start {
        Some((head, start)) => (
            body[..head].trim_matches(WS),
            Some(body[start..].trim_matches(WS)),
            "",
        ),
        None => (body.trim_matches(WS), None, ""),
    }
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

/// A version heading in any of the spellings changelogs use, including
/// keep-a-changelog's "## [1.0.0]" and "## v1.0.0", which other tools write.
fn is_any_version_heading(line: &str) -> bool {
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
    line == UNRELEASED_HEADING || line == UNRELEASED_HEADING_BRACKETED
}

/// Keep a Changelog ends the file with "[1.0.0]: https://..." link lines.
/// Only version and Unreleased labels count, since changeset bodies may use
/// reference links of their own.
/// The label runs to the first `]`, and is `Unreleased` or a version with an
/// optional `v`, like release-notes.sh's `\[(Unreleased|[vV]?<semver>[^]]*)\]: `.
pub(crate) fn is_link_reference(line: &str) -> bool {
    let Some((label, after)) = line.strip_prefix('[').and_then(|rest| rest.split_once(']')) else {
        return false;
    };
    if !after.starts_with(": ") {
        return false;
    }
    let version = label
        .strip_prefix('v')
        .or_else(|| label.strip_prefix('V'))
        .unwrap_or(label);
    label == "Unreleased" || is_version_heading(&format!("## {version}"))
}

/// Whether a line ends a version's notes, the same set release-notes.sh stops
/// at (besides the plain "## <semver>" heading it also starts at): any version
/// heading, an Unreleased heading, or a version or Unreleased link reference.
pub fn is_section_end(line: &str) -> bool {
    is_any_version_heading(line) || is_unreleased_heading(line) || is_link_reference(line)
}

/// Lines in a changeset body's fenced code that the changelog parsers would
/// take for a section boundary. Parsers read plain lines, not fences, so such
/// a line ends the entry's section early once it's in the changelog. Bodies
/// keep their code as written (changing it would change the sample), so this
/// is only reported.
pub fn fenced_boundary_lines(body: &str) -> Vec<&str> {
    body.split_inclusive('\n')
        .zip(crate::changelog::markdown::fenced_lines(body))
        .filter(|(_, code)| *code)
        .map(|(line, _)| line.trim_end_matches(WS))
        .filter(|line| {
            line.starts_with("## ") || line.starts_with("### ") || is_link_reference(line)
        })
        .collect()
}

/// Prints a warning for each line `fenced_boundary_lines` finds.
pub fn warn_fenced_boundaries(file: &str, body: &str) {
    for line in fenced_boundary_lines(body) {
        eprintln!(
            "warning: {file}: \"{line}\" in a code block reads as a changelog heading and ends the entry there; indent it or change the line"
        );
    }
}

/// A changelog with its Unreleased sections taken out.
#[derive(Debug)]
struct Split {
    rest: String,
    bodies: Vec<String>,
    /// The heading to write an Unreleased section under, in the file's own
    /// spelling.
    heading: &'static str,
}

/// Takes every Unreleased section above the first version heading out of a
/// changelog. Lower ones are left alone, so a changeset body that happens to
/// contain the heading isn't touched. A section runs to the next version
/// heading, Unreleased heading or link reference.
fn take_unreleased(content: &str) -> Split {
    let mut rest = String::new();
    let mut bodies = Vec::new();
    let mut current: Option<String> = None;
    let mut found_heading = None;
    let mut bracketed_versions = false;
    let mut done = false;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(WS);
        if !done {
            if is_any_version_heading(trimmed) {
                bodies.extend(current.take());
                bracketed_versions = trimmed.starts_with("## [");
                done = true;
            } else if is_unreleased_heading(trimmed) {
                bodies.extend(current.take());
                found_heading.get_or_insert(trimmed == UNRELEASED_HEADING_BRACKETED);
                current = Some(String::new());
                continue;
            } else if current.is_some() && is_link_reference(trimmed) {
                bodies.extend(current.take());
                done = true;
            } else if let Some(body) = current.as_mut() {
                body.push_str(line);
                continue;
            }
        }
        rest.push_str(line);
    }
    bodies.extend(current);
    let heading = if found_heading.unwrap_or(bracketed_versions) {
        UNRELEASED_HEADING_BRACKETED
    } else {
        UNRELEASED_HEADING
    };
    Split {
        rest,
        bodies,
        heading,
    }
}

/// Writes one changelog's part of a release: `release` is its version section,
/// or `None` when this changelog has no release and only none-bump entries.
/// Existing Unreleased sections are folded into a new version section, or
/// merged and extended when there's still no release. Returns `None` when the
/// file is unchanged, otherwise what the section holds below the release's own
/// entries.
pub fn write_changelog(
    path: &Path,
    release: Option<&str>,
    none: &[&NoneEntry],
    config: &ChangelogConfig,
) -> anyhow::Result<Option<String>> {
    if release.is_none() && (none.is_empty() || config.none_bump == "omit") {
        return Ok(None);
    }
    let raw = if path.exists() {
        Some(std::fs::read_to_string(path)?)
    } else {
        None
    };
    // Rewritten in the file's main line ending, so it doesn't end up mixed.
    let crlf = raw
        .as_deref()
        .is_some_and(|r| r.matches("\r\n").count() * 2 > r.matches('\n').count());
    let existing = raw.map(|r| r.replace("\r\n", "\n"));
    let split = existing.as_deref().map(take_unreleased);
    let (heading, bodies): (&str, &[String]) = split
        .as_ref()
        .map_or((UNRELEASED_HEADING, &[]), |s| (s.heading, &s.bodies));
    let folded = Folded::new(bodies, &config.none_bump_heading);
    let Some((section, extra)) = compose_section(release, heading, &folded, none, config) else {
        return Ok(None);
    };
    let content = match &split {
        Some(split) => insert_after_header(&split.rest, &section),
        None => format!("# Changelog\n\n{section}"),
    };
    let content = if crlf {
        content.replace('\n', "\r\n")
    } else {
        content
    };
    std::fs::write(path, content)?;
    Ok(Some(extra))
}

/// Puts a new section at the top: below the header (everything before the
/// first "## " heading, which may be nothing) and above every existing section.
/// A file with no sections yet keeps its link references at the end.
fn insert_after_header(existing: &str, new_entry: &str) -> String {
    let mut offset = 0;
    let mut split_at = None;
    for line in existing.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(WS);
        if trimmed.starts_with("## ") || is_link_reference(trimmed) {
            split_at = Some(offset);
            break;
        }
        offset += line.len();
    }
    let (header, rest) = existing.split_at(split_at.unwrap_or(existing.len()));
    let header = header.trim_end_matches(WS);
    let rest = rest.trim_start_matches(['\n', '\r']);
    let mut out = String::new();
    if !header.is_empty() {
        out.push_str(header);
        out.push_str("\n\n");
    }
    out.push_str(new_entry.trim_end_matches(WS));
    out.push('\n');
    if !rest.is_empty() {
        out.push('\n');
        out.push_str(rest);
    }
    out
}

/// Writes the root changelog for single-package repos and grouped monorepos.
pub fn write_root_changelog(
    repo_root: &Path,
    plan: &ReleasePlan,
    config: &ChangelogConfig,
    date: &str,
    is_monorepo: bool,
) -> anyhow::Result<ChangelogWrites> {
    let path = repo_root.join(&config.file);
    let release = release_section(plan, date, is_monorepo, config);
    let none: Vec<&NoneEntry> = plan.none_entries.iter().collect();
    let mut writes = ChangelogWrites::default();
    if let Some(extra) = write_changelog(&path, release.as_deref(), &none, config)? {
        writes.paths.push(path);
        for release in &plan.releases {
            writes
                .extra_notes
                .insert(release.name.clone(), extra.clone());
        }
    }
    Ok(writes)
}

/// A workspace root like `{"private": true, "workspaces": [...]}` or a
/// manifest without a name is detected as a package, but it's never released,
/// so per-package mode doesn't give it a changelog of its own.
fn has_own_changelog(pkg: &Package) -> bool {
    if pkg.name.is_empty() {
        return false;
    }
    if pkg.package_type != PackageType::Npm {
        return true;
    }
    let Ok(content) = std::fs::read_to_string(pkg.path.join("package.json")) else {
        return true;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return true;
    };
    let private = json.get("private").and_then(serde_json::Value::as_bool) == Some(true);
    !(private && json.get("workspaces").is_some())
}

/// Writes each package's own changelog. A none-bump entry goes to every
/// package it names, released or not, and a repo-wide `default` entry goes to
/// every package with a changelog of its own, since per-package mode has no
/// root changelog.
pub fn write_per_package_changelogs(
    plan: &ReleasePlan,
    packages: &[Package],
    config: &ChangelogConfig,
    date: &str,
) -> anyhow::Result<ChangelogWrites> {
    #[derive(Default)]
    struct Target<'a> {
        names: Vec<&'a str>,
        releases: Vec<(&'a str, String)>,
        repo_wide: bool,
        none: Vec<&'a NoneEntry>,
    }
    // Keyed by path: two packages in one directory share a changelog.
    let mut targets: BTreeMap<PathBuf, Target> = BTreeMap::new();
    for release in &plan.releases {
        let t = targets.entry(release.path.join(&config.file)).or_default();
        t.names.push(&release.name);
        t.repo_wide = true;
        t.releases.push((
            &release.name,
            format!("## {} - {}\n\n{}", release.version, date, release.changelog),
        ));
    }
    for pkg in packages {
        let t = targets.entry(pkg.path.join(&config.file)).or_default();
        t.names.push(&pkg.name);
        t.repo_wide |= has_own_changelog(pkg);
    }

    let has_real_default = packages.iter().any(|p| p.name == REPO_WIDE_PACKAGE);
    for entry in &plan.none_entries {
        let repo_wide = !has_real_default && entry.packages.iter().any(|p| p == REPO_WIDE_PACKAGE);
        for t in targets.values_mut() {
            let named = t
                .names
                .iter()
                .any(|n| !n.is_empty() && entry.packages.iter().any(|p| p == n));
            if named || (repo_wide && t.repo_wide) {
                t.none.push(entry);
            }
        }
    }

    let mut writes = ChangelogWrites::default();
    for (path, t) in &targets {
        let release = (!t.releases.is_empty()).then(|| {
            t.releases
                .iter()
                .map(|(_, s)| s.as_str())
                .collect::<Vec<_>>()
                .join("\n\n")
        });
        if let Some(extra) = write_changelog(path, release.as_deref(), &t.none, config)? {
            writes.paths.push(path.clone());
            for (name, _) in &t.releases {
                writes.extra_notes.insert(name.to_string(), extra.clone());
            }
        }
    }
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changeset::types::BumpLevel;
    use crate::package::types::{PackageType, Version};
    use crate::release::plan::PlannedRelease;

    /// What a release run writes into a new root changelog, below its title.
    fn entry_text(
        plan: &ReleasePlan,
        config: &ChangelogConfig,
        date: &str,
        is_monorepo: bool,
    ) -> String {
        let dir = tempfile::tempdir().unwrap();
        write_root_changelog(dir.path(), plan, config, date, is_monorepo).unwrap();
        std::fs::read_to_string(dir.path().join(&config.file))
            .unwrap_or_default()
            .trim_start_matches("# Changelog\n\n")
            .to_string()
    }

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
        let entry = entry_text(&plan, &config, "2026-07-25", false);

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
        let entry = entry_text(&plan, &config, "2026-07-25", false);

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
        let entry = entry_text(&plan, &config, "2026-07-25", false);
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
        let entry = entry_text(&plan, &config, "2026-07-25", true);

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
        write_changelog(&path, Some(new_entry), &[], &ChangelogConfig::default()).unwrap();

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
        write_changelog(&path, Some(new_entry), &[], &ChangelogConfig::default()).unwrap();

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
        assert_eq!(written.paths.len(), 2);

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
        assert_eq!(written.paths, vec![dir.path().join("a/CHANGELOG.md")]);

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
        assert!(
            write_changelog(&path, None, &[&entry], &config)
                .unwrap()
                .is_none()
        );
        assert!(!path.exists());
    }

    #[test]
    fn unreleased_stops_at_headings_other_tools_write() {
        for older in ["## [1.0.0] - 2020", "## v1.0.0"] {
            let content = format!(
                "# C\n\n## [Unreleased]\n\n### Internal\n\n- old\n\n{older}\n\n### Internal\n\n- ancient\n"
            );
            let split = take_unreleased(&content);
            assert_eq!(
                split.bodies,
                vec!["\n### Internal\n\n- old\n\n".to_string()]
            );
            assert_eq!(split.heading, UNRELEASED_HEADING_BRACKETED);
            assert_eq!(
                split.rest,
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
        let split = take_unreleased(content);
        assert_eq!(split.rest, content);
        assert!(split.bodies.is_empty());
    }

    fn write_and_read(
        initial: &str,
        release: Option<&str>,
        none: &[&NoneEntry],
    ) -> (String, Option<String>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CHANGELOG.md");
        std::fs::write(&path, initial).unwrap();
        let extra = write_changelog(&path, release, none, &ChangelogConfig::default()).unwrap();
        (read(&path), extra)
    }

    #[test]
    fn new_sections_go_above_every_version_without_a_title() {
        let ci_one = none_entry("#### CI one", &["x"]);
        let ci_two = none_entry("#### CI two", &["x"]);
        let feature = Some("## 1.1.0 - d\n\n#### Feature");
        let (one, _) = write_and_read(
            "## 1.0.0 - d\n\n- A\n\n## 0.9.0 - d\n\n- B\n",
            None,
            &[&ci_one],
        );
        assert_eq!(
            one,
            "## Unreleased\n\n### Internal\n\n#### CI one\n\n## 1.0.0 - d\n\n- A\n\n## 0.9.0 - d\n\n- B\n"
        );
        let (two, _) = write_and_read(&one, None, &[&ci_two]);
        let (three, _) = write_and_read(&two, feature, &[]);
        assert_eq!(
            three,
            "## 1.1.0 - d\n\n#### Feature\n\n### Internal\n\n#### CI one\n\n#### CI two\n\n## 1.0.0 - d\n\n- A\n\n## 0.9.0 - d\n\n- B\n"
        );
    }

    #[test]
    fn parsing_ignores_fences_so_an_old_unclosed_one_stays_put() {
        // An Unreleased note with a fence it never closed, from before bodies
        // got their fences closed, and a later version with a closed fence.
        // Reading lines without fence state keeps 1.0.0 a version heading.
        let initial = "# C\n\n## Unreleased\n\n### Internal\n\n```sh\nold unclosed\n\n## 1.0.0 - d\n\n```\ncode\n```\n\n- A\n";
        let (content, extra) = write_and_read(initial, Some("## 1.0.1 - d\n\n#### Fix"), &[]);
        assert_eq!(
            content,
            "# C\n\n## 1.0.1 - d\n\n#### Fix\n\n### Internal\n\n```sh\nold unclosed\n\n## 1.0.0 - d\n\n```\ncode\n```\n\n- A\n"
        );
        assert_eq!(
            extra.as_deref(),
            Some("### Internal\n\n```sh\nold unclosed")
        );
    }

    #[test]
    fn fenced_boundary_lines_are_reported() {
        let body = "#### T\n\n```md\n## 9.9.9\n### Internal\n[1.0.0]: https://x\n# fine\n```\n\n## outside is demoted\n";
        assert_eq!(
            fenced_boundary_lines(body),
            vec!["## 9.9.9", "### Internal", "[1.0.0]: https://x"]
        );
        assert!(fenced_boundary_lines("#### T\n\n## demoted\n").is_empty());
    }

    #[test]
    fn a_long_changelog_is_read_in_linear_time() {
        // The parsers read each line once with no fence state, so 5,000
        // versions, each with a fence it never closes, take milliseconds.
        // The bound is loose so a slow debug build on CI still passes.
        let mut initial = String::from("# C\n\n## Unreleased\n\n### Internal\n\n- old\n\n");
        for v in (0..5000).rev() {
            initial.push_str(&format!(
                "## 1.0.{v} - d\n\n- note {v}\n\n```sh\nunclosed {v}\n\n"
            ));
        }
        let started = std::time::Instant::now();
        let (content, _) = write_and_read(&initial, Some("## 2.0.0 - d\n\n#### Big"), &[]);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert!(content.starts_with(
            "# C\n\n## 2.0.0 - d\n\n#### Big\n\n### Internal\n\n- old\n\n## 1.0.4999 - d\n"
        ));
    }

    #[test]
    fn several_unreleased_sections_are_merged() {
        let initial = "# C\n\n## Unreleased\n\n### Internal\n\n- two\n\n## Unreleased\n\n### Internal\n\n- one\n\n## 1.0.0 - d\n\n- A\n";
        let (content, extra) = write_and_read(initial, Some("## 1.1.0 - d\n\n#### Feature"), &[]);
        assert_eq!(
            content,
            "# C\n\n## 1.1.0 - d\n\n#### Feature\n\n### Internal\n\n- two\n\n- one\n\n## 1.0.0 - d\n\n- A\n"
        );
        assert_eq!(extra.as_deref(), Some("### Internal\n\n- two\n\n- one"));
    }

    #[test]
    fn keeps_the_bracketed_unreleased_spelling() {
        let ci = none_entry("#### CI", &["x"]);
        let (kept, _) = write_and_read(
            "# C\n\n## [Unreleased]\n\n- hand\n\n## [1.0.0] - d\n",
            None,
            &[&ci],
        );
        assert_eq!(
            kept,
            "# C\n\n## [Unreleased]\n\n- hand\n\n### Internal\n\n#### CI\n\n## [1.0.0] - d\n"
        );
        let (created, _) = write_and_read("# C\n\n## [1.0.0] - d\n\n- A\n", None, &[&ci]);
        assert!(
            created.starts_with("# C\n\n## [Unreleased]\n\n### Internal\n"),
            "{created}"
        );
    }

    #[test]
    fn new_none_entries_join_the_internal_subsection() {
        let ci = none_entry("#### CI new", &["x"]);
        let initial = "# C\n\n## Unreleased\n\n### Internal\n\n#### CI old\n\n### Added\n\n- Thing\n\n## 1.0.0 - d\n";
        let (content, _) = write_and_read(initial, None, &[&ci]);
        assert_eq!(
            content,
            "# C\n\n## Unreleased\n\n### Internal\n\n#### CI old\n\n#### CI new\n\n### Added\n\n- Thing\n\n## 1.0.0 - d\n"
        );
    }

    #[test]
    fn link_references_stay_at_the_end() {
        let ci = none_entry("#### CI", &["x"]);
        let (content, _) = write_and_read("# C\n\n[1.0.0]: https://x/1.0.0\n", None, &[&ci]);
        assert_eq!(
            content,
            "# C\n\n## Unreleased\n\n### Internal\n\n#### CI\n\n[1.0.0]: https://x/1.0.0\n"
        );
        let initial = "# C\n\n## [Unreleased]\n\n- x\n\n[Unreleased]: https://x/HEAD\n";
        let (content, _) = write_and_read(initial, Some("## [1.1.0] - d\n\n- y"), &[]);
        assert_eq!(
            content,
            "# C\n\n## [1.1.0] - d\n\n- y\n\n- x\n\n[Unreleased]: https://x/HEAD\n"
        );
    }

    #[test]
    fn crlf_changelogs_stay_crlf() {
        let ci = none_entry("#### CI", &["x"]);
        let initial = "# C\r\n\r\n## 1.0.0 - d\r\n\r\n- A\r\n";
        let (content, _) = write_and_read(initial, None, &[&ci]);
        assert_eq!(
            content,
            "# C\r\n\r\n## Unreleased\r\n\r\n### Internal\r\n\r\n#### CI\r\n\r\n## 1.0.0 - d\r\n\r\n- A\r\n"
        );
        let (content, _) = write_and_read(&content, Some("## 1.1.0 - d\n\n- B"), &[]);
        assert!(!content.replace("\r\n", "").contains('\n'), "{content:?}");
        assert!(content.contains("## 1.1.0 - d\r\n\r\n- B\r\n\r\n### Internal\r\n\r\n#### CI\r\n"));
    }

    #[test]
    fn extra_notes_extend_the_release_changelog() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("CHANGELOG.md"),
            "# C\n\n## Unreleased\n\n### Internal\n\n- old\n",
        )
        .unwrap();
        let mut release = make_release("lib", "1.0.0", "1.1.0", BumpLevel::Minor, "#### Feature");
        release.path = dir.path().to_path_buf();
        let mut plan = make_plan(vec![release], vec![none_entry("#### New", &["lib"])]);
        let writes =
            write_root_changelog(dir.path(), &plan, &ChangelogConfig::default(), "d", false)
                .unwrap();
        writes.apply_to(&mut plan);
        assert_eq!(
            plan.releases[0].changelog,
            "#### Feature\n\n### Internal\n\n- old\n\n#### New"
        );
    }

    #[test]
    fn private_workspace_root_gets_no_repo_wide_notes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"private":true,"workspaces":["a"]}"#,
        )
        .unwrap();
        let root = Package {
            name: String::new(),
            path: dir.path().to_path_buf(),
            package_type: PackageType::Npm,
            version: Version::new(0, 0, 0),
        };
        let named_root = Package {
            name: "root".to_string(),
            ..root.clone()
        };
        for root in [root, named_root] {
            let packages = vec![root, package(dir.path(), "a")];
            let plan = make_plan(vec![], vec![none_entry("#### Docs", &["default"])]);
            let writes =
                write_per_package_changelogs(&plan, &packages, &ChangelogConfig::default(), "d")
                    .unwrap();
            assert_eq!(writes.paths, vec![dir.path().join("a/CHANGELOG.md")]);
            assert!(!dir.path().join("CHANGELOG.md").exists());
            std::fs::remove_file(dir.path().join("a/CHANGELOG.md")).unwrap();
        }
    }

    #[test]
    fn version_headings_need_a_semver() {
        assert!(is_version_heading("## 1.0.0 - d"));
        assert!(is_version_heading("## 1.0.0-rc.1, 2.0.0"));
        assert!(!is_version_heading("## Unreleased"));
        assert!(!is_version_heading("## 2FA support"));
        assert!(!is_version_heading("## 1.x.0"));
        assert!(!is_version_heading("### 1.0.0"));
        for heading in ["## [1.0.0] - d", "## v1.0.0", "## [v1.0.0]", "## [V1.0.0]"] {
            assert!(is_any_version_heading(heading), "{heading}");
        }
        assert!(!is_any_version_heading("## [Unreleased]"));
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
        let entry = entry_text(&plan, &config, "2026-07-25", false);
        assert!(entry.contains("### Misc"));
    }
}
