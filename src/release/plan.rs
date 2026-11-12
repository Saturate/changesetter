use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::changelog::groups;
use crate::changelog::links::Linker;
use crate::changeset::types::{BumpLevel, Changeset, Origin};
use crate::config::Config;
use crate::package::types::{Package, Version};
use crate::release::pre::PreState;

#[derive(Debug, Clone)]
pub struct ReleasePlan {
    pub releases: Vec<PlannedRelease>,
    pub none_entries: Vec<NoneEntry>,
}

#[derive(Debug, Clone)]
pub struct PlannedRelease {
    pub name: String,
    pub path: PathBuf,
    pub version: Version,
    pub previous_version: Version,
    pub bump: BumpLevel,
    pub changelog: String,
    pub changesets: Vec<String>,
    /// The entries behind `changelog`, in changeset order.
    pub entries: Vec<ReleaseEntry>,
}

/// One changeset's entry in a release.
#[derive(Debug, Clone)]
pub struct ReleaseEntry {
    /// The changeset's file name without `.md`, or a name changesetter made
    /// up for an entry it wrote itself (`fixed-group`, `dependency-cascade`).
    pub changeset: String,
    /// The bump this changeset gives the package.
    pub bump: BumpLevel,
    pub kind: String,
    pub scope: Option<String>,
    pub summary: String,
    /// The group it's listed under in a grouped changelog: `None` when the
    /// changelog isn't grouped, or no group takes it.
    pub group: Option<String>,
    /// Left out of the changelog by a `skip` group.
    pub skip: bool,
    /// The body with its headings demoted, as the flat changelog has it.
    body: String,
    /// The body as written, which a grouped changelog renders from.
    raw: String,
    /// The commit and pull request that added the changeset.
    pub origin: Option<Origin>,
}

#[derive(Debug, Clone, Default)]
pub struct NoneEntry {
    pub title: String,
    pub body: String,
    pub changesets: Vec<String>,
    /// Packages the changeset names, with `default` resolved, so per-package
    /// changelogs can pick out their own entries.
    pub packages: Vec<String>,
    pub kind: String,
    pub scope: Option<String>,
    pub summary: String,
    /// Its group in a grouped changelog, as for `ReleaseEntry::group`. It's
    /// still listed under the none-bump heading.
    pub group: Option<String>,
    /// Left out of the changelog by a `skip` group.
    pub skip: bool,
    /// The commit and pull request that added the changeset.
    pub origin: Option<Origin>,
}

type BumpMap = BTreeMap<String, (BumpLevel, Vec<ReleaseEntry>)>;

/// Where an entry goes in the changelog: its group (`None` when not grouped
/// or for the fallback group) and whether a `skip` group takes it.
fn place(config: &Config, kind: &str, summary: &str) -> (Option<String>, bool) {
    if !config.changelog.is_grouped() {
        return (None, false);
    }
    let groups = groups::groups(&config.changelog);
    match groups::find_group(&groups, kind, summary) {
        Some(g) => (Some(g.title.clone()), g.skip),
        None => (None, false),
    }
}

fn new_entry(
    changeset: &str,
    bump: BumpLevel,
    body: &str,
    kind: Option<&str>,
    scope: Option<&str>,
    origin: Option<&Origin>,
    config: &Config,
) -> ReleaseEntry {
    let c = groups::classify(body, kind, scope, bump, &config.changelog);
    let (group, skip) = place(config, &c.kind, &c.summary);
    ReleaseEntry {
        changeset: changeset.to_string(),
        bump,
        kind: c.kind,
        scope: c.scope,
        summary: c.summary,
        group,
        skip,
        body: crate::changelog::markdown::demote_headings(body),
        raw: body.to_string(),
        origin: origin.cloned(),
    }
}

impl ReleaseEntry {
    /// The entry as the changelog has it: its body (with issue links when
    /// `links` is on), or in a grouped changelog a list item, which also
    /// gets its pull request and commit.
    fn text(&self, config: &Config, linker: Option<&Linker>) -> String {
        if !config.changelog.is_grouped() {
            return linker.map_or_else(|| self.body.clone(), |l| l.linkify(&self.body));
        }
        let item = groups::render_item(&self.raw, &self.summary, self.scope.as_deref());
        let Some(linker) = linker else {
            return item;
        };
        let item = linker.linkify(&item);
        let suffix = linker.origin_suffix(self.origin.as_ref());
        match item.split_once('\n') {
            Some((first, rest)) => format!("{first}{suffix}\n{rest}"),
            None => format!("{item}{suffix}"),
        }
    }
}

/// A release's changelog text: the bodies one after another, or in a grouped
/// changelog a list per group. Groups sit under a combined monorepo
/// changelog's package headings, so they go a level deeper there.
fn release_changelog(entries: &[ReleaseEntry], config: &Config, nested: bool) -> String {
    let linker = Linker::new(&config.changelog);
    if !config.changelog.is_grouped() {
        return entries
            .iter()
            .map(|e| e.text(config, linker.as_ref()))
            .collect::<Vec<String>>()
            .join("\n\n");
    }
    let items: Vec<(Option<String>, String)> = entries
        .iter()
        .filter(|e| !e.skip && !e.raw.trim().is_empty())
        .map(|e| (e.group.clone(), e.text(config, linker.as_ref())))
        .collect();
    let groups = groups::groups(&config.changelog);
    groups::render_sections(&groups, &items, if nested { 4 } else { 3 })
}

pub fn assemble(
    changesets: &[Changeset],
    packages: &[Package],
    config: &Config,
    pre_state: Option<&PreState>,
) -> ReleasePlan {
    let mut bumps_per_package: BumpMap = BTreeMap::new();
    let mut none_entries = Vec::new();

    let is_single_package = packages.len() <= 1;

    for cs in changesets {
        let cs_name = cs.filename.clone().unwrap_or_else(|| "unknown".to_string());

        // A none-bump changeset goes under the none-bump heading even when the
        // package is released for other changesets, so internal notes never mix
        // into the release's changelog, tag message or release notes. When the
        // same changeset also bumps a package, its body is already in that
        // release, so it's not repeated as a none entry.
        let resolved: Vec<(String, BumpLevel)> = cs
            .packages
            .iter()
            .map(|(pkg_name, bump)| {
                let name = if pkg_name == "default" && is_single_package {
                    packages
                        .first()
                        .map_or_else(|| "default".to_string(), |p| p.name.clone())
                } else {
                    pkg_name.clone()
                };
                (name, *bump)
            })
            .collect();

        let has_real_bump = resolved.iter().any(|(_, b)| *b > BumpLevel::None);
        if !has_real_bump && !resolved.is_empty() {
            let e = new_entry(
                &cs_name,
                BumpLevel::None,
                &cs.body,
                cs.kind.as_deref(),
                cs.scope.as_deref(),
                cs.origin.as_ref(),
                config,
            );
            let body = if !config.changelog.is_grouped() || !e.raw.trim().is_empty() {
                e.text(config, Linker::new(&config.changelog).as_ref())
            } else {
                e.body.clone()
            };
            none_entries.push(NoneEntry {
                title: extract_title(&cs.body),
                body,
                changesets: vec![cs_name.clone()],
                packages: resolved.iter().map(|(n, _)| n.clone()).collect(),
                kind: e.kind,
                scope: e.scope,
                summary: e.summary,
                group: e.group,
                skip: e.skip,
                origin: e.origin,
            });
        }

        for (resolved_name, bump) in resolved {
            let entry = bumps_per_package
                .entry(resolved_name)
                .or_insert((BumpLevel::None, Vec::new()));

            if bump > entry.0 {
                entry.0 = bump;
            }

            if bump > BumpLevel::None {
                entry.1.push(new_entry(
                    &cs_name,
                    bump,
                    &cs.body,
                    cs.kind.as_deref(),
                    cs.scope.as_deref(),
                    cs.origin.as_ref(),
                    config,
                ));
            }
        }
    }

    apply_fixed_groups(&mut bumps_per_package, packages, config);
    apply_linked_groups(&mut bumps_per_package, config);
    apply_dependency_cascading(&mut bumps_per_package, packages, config);
    let nested = packages.len() > 1 && !config.changelog.per_package;

    let mut releases = Vec::new();

    for (pkg_name, (bump, entries)) in &bumps_per_package {
        if *bump == BumpLevel::None {
            continue;
        }

        let changesets_names: Vec<String> = entries.iter().map(|e| e.changeset.clone()).collect();

        let pkg = packages.iter().find(|p| &p.name == pkg_name);
        let previous_version = pkg
            .map(|p| p.version.clone())
            .unwrap_or_else(|| Version::new(0, 0, 0));
        let pkg_path = pkg
            .map(|p| p.path.clone())
            .unwrap_or_else(|| PathBuf::from("."));

        let new_version = apply_bump(&previous_version, *bump);
        let new_version = apply_pre_version(new_version, pkg_name, pre_state);

        let changelog = release_changelog(entries, config, nested);

        releases.push(PlannedRelease {
            name: pkg_name.clone(),
            path: pkg_path,
            version: new_version,
            previous_version,
            bump: *bump,
            changelog,
            changesets: changesets_names,
            entries: entries.clone(),
        });
    }

    ReleasePlan {
        releases,
        none_entries,
    }
}

fn apply_fixed_groups(bumps: &mut BumpMap, packages: &[Package], config: &Config) {
    for group in config.groups.values() {
        if group.fixed.is_empty() {
            continue;
        }

        let highest_bump = group
            .fixed
            .iter()
            .filter_map(|name| bumps.get(name).map(|(b, _)| *b))
            .filter(|b| *b > BumpLevel::None)
            .max();

        let Some(highest_bump) = highest_bump else {
            continue;
        };

        for member in &group.fixed {
            let entry = bumps
                .entry(member.clone())
                .or_insert((BumpLevel::None, Vec::new()));

            if entry.0 < highest_bump {
                entry.0 = highest_bump;
            }

            if entry.1.is_empty() {
                let pkg_version = packages
                    .iter()
                    .find(|p| &p.name == member)
                    .map(|p| p.version.to_string())
                    .unwrap_or_default();
                entry.1.push(new_entry(
                    "fixed-group",
                    highest_bump,
                    &format!("#### Bumped as part of fixed group (was {pkg_version})"),
                    Some(groups::DEPS_TYPE),
                    None,
                    None,
                    config,
                ));
            }
        }
    }
}

fn apply_linked_groups(bumps: &mut BumpMap, config: &Config) {
    for group in config.groups.values() {
        if group.linked.is_empty() {
            continue;
        }

        let highest_bump = group
            .linked
            .iter()
            .filter_map(|name| bumps.get(name).map(|(b, _)| *b))
            .filter(|b| *b > BumpLevel::None)
            .max();

        let Some(highest_bump) = highest_bump else {
            continue;
        };

        for member in &group.linked {
            if let Some(entry) = bumps.get_mut(member) {
                if entry.0 > BumpLevel::None && entry.0 < highest_bump {
                    entry.0 = highest_bump;
                }
            }
        }
    }
}

fn apply_dependency_cascading(bumps: &mut BumpMap, packages: &[Package], config: &Config) {
    let mode = match config.update_internal_dependencies.as_deref() {
        Some("patch") | Some("minor") => BumpLevel::Patch,
        _ => return,
    };

    let dependents_map = crate::release::deps::build_dependents_map(packages);

    loop {
        let bumped: Vec<String> = bumps
            .iter()
            .filter(|(_, (b, _))| *b > BumpLevel::None)
            .map(|(name, _)| name.clone())
            .collect();

        let mut changed = false;
        for bumped_pkg in &bumped {
            if let Some(dependents) = dependents_map.get(bumped_pkg) {
                for dependent in dependents {
                    if dependent == bumped_pkg {
                        continue;
                    }
                    let entry = bumps
                        .entry(dependent.clone())
                        .or_insert((BumpLevel::None, Vec::new()));

                    if entry.0 < mode {
                        entry.0 = mode;
                        changed = true;
                    }

                    let already_has_cascade_entry =
                        entry.1.iter().any(|e| e.changeset == "dependency-cascade");
                    if !already_has_cascade_entry {
                        entry.1.push(new_entry(
                            "dependency-cascade",
                            mode,
                            &format!(
                                "#### Dependency update\n\nBumped due to dependency on {bumped_pkg}"
                            ),
                            Some(groups::DEPS_TYPE),
                            None,
                            None,
                            config,
                        ));
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn apply_pre_version(version: Version, pkg_name: &str, pre_state: Option<&PreState>) -> Version {
    let Some(state) = pre_state else {
        return version;
    };

    if state.mode == "exit" {
        return Version {
            major: version.major,
            minor: version.minor,
            patch: version.patch,
            pre: None,
        };
    }

    if state.mode == "pre" {
        let counter = state.packages_released.get(pkg_name).copied().unwrap_or(0);
        return Version {
            major: version.major,
            minor: version.minor,
            patch: version.patch,
            pre: Some(format!("{}.{counter}", state.tag)),
        };
    }

    version
}

fn apply_bump(version: &Version, bump: BumpLevel) -> Version {
    match bump {
        BumpLevel::Major => version.bump_major(),
        BumpLevel::Minor => version.bump_minor(),
        BumpLevel::Patch => version.bump_patch(),
        BumpLevel::None => version.clone(),
    }
}

impl ReleaseEntry {
    /// The entry as `--output json` lists it.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.changeset,
            "bump": self.bump.to_string(),
            "type": self.kind,
            "scope": self.scope,
            "group": self.group,
            "skip": self.skip,
            "summary": self.summary,
            "commit": self.origin.as_ref().map(|o| &o.commit),
            "author": self.origin.as_ref().map(|o| &o.author),
            "pr": self.origin.as_ref().and_then(|o| o.pr),
        })
    }
}

impl NoneEntry {
    /// The entry as `--output json` lists it, besides its title and body.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.changesets.first(),
            "bump": BumpLevel::None.to_string(),
            "type": self.kind,
            "scope": self.scope,
            "group": self.group,
            "skip": self.skip,
            "summary": self.summary,
            "commit": self.origin.as_ref().map(|o| &o.commit),
            "author": self.origin.as_ref().map(|o| &o.author),
            "pr": self.origin.as_ref().and_then(|o| o.pr),
            "title": self.title,
            "body": self.body,
        })
    }
}

impl ReleasePlan {
    /// A warning per entry typed `breaking` whose bump isn't major.
    pub fn breaking_warnings(&self) -> Vec<String> {
        let releases = self.releases.iter().flat_map(|r| {
            r.entries.iter().map(move |e| {
                (
                    e.changeset.as_str(),
                    e.kind.as_str(),
                    r.name.as_str(),
                    e.bump,
                )
            })
        });
        let none = self.none_entries.iter().flat_map(|n| {
            let id = n.changesets.first().map_or("changeset", String::as_str);
            n.packages
                .iter()
                .map(move |p| (id, n.kind.as_str(), p.as_str(), BumpLevel::None))
        });
        releases
            .chain(none)
            .filter_map(|(id, kind, package, bump)| {
                groups::breaking_warning(&format!("{id}.md"), kind, package, bump)
            })
            .collect()
    }
}

fn extract_title(body: &str) -> String {
    body.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim_start_matches('#')
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pkg(name: &str, version: &str) -> Package {
        Package {
            name: name.to_string(),
            path: std::path::PathBuf::from("."),
            package_type: crate::package::types::PackageType::Cargo,
            version: Version::parse(version).unwrap(),
        }
    }

    fn make_cs(packages: &[(&str, BumpLevel)], body: &str, name: &str) -> Changeset {
        Changeset {
            packages: packages.iter().map(|(n, b)| (n.to_string(), *b)).collect(),
            body: body.to_string(),
            filename: Some(name.to_string()),
            kind: None,
            scope: None,
            origin: None,
        }
    }

    #[test]
    fn single_package_patch() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix", "cs1")];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].name, "mylib");
        assert_eq!(plan.releases[0].version, Version::new(1, 0, 1));
        assert_eq!(plan.releases[0].bump, BumpLevel::Patch);
    }

    #[test]
    fn highest_bump_wins() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![
            make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix", "cs1"),
            make_cs(&[("mylib", BumpLevel::Minor)], "#### Feature", "cs2"),
        ];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].version, Version::new(1, 1, 0));
        assert_eq!(plan.releases[0].bump, BumpLevel::Minor);
    }

    #[test]
    fn none_bump_no_version_change() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![make_cs(
            &[("mylib", BumpLevel::None)],
            "#### CI update",
            "cs1",
        )];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert!(plan.releases.is_empty());
        assert_eq!(plan.none_entries.len(), 1);
        assert_eq!(plan.none_entries[0].title, "CI update");
    }

    #[test]
    fn none_plus_patch_equals_patch() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![
            make_cs(&[("mylib", BumpLevel::None)], "#### CI", "cs1"),
            make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix", "cs2"),
        ];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].version, Version::new(1, 0, 1));
        assert!(!plan.releases[0].changelog.contains("CI"));
        assert_eq!(plan.releases[0].changesets, vec!["cs2".to_string()]);
        assert_eq!(plan.none_entries.len(), 1);
        assert_eq!(plan.none_entries[0].title, "CI");
    }

    #[test]
    fn none_bump_on_default_package_stays_out_of_release() {
        let packages = vec![make_pkg("mylib", "0.0.0")];
        let changesets = vec![
            make_cs(&[("default", BumpLevel::Minor)], "#### Feature", "cs1"),
            make_cs(
                &[("default", BumpLevel::None)],
                "#### CI: fail PRs whose changesets don't parse",
                "cs2",
            ),
        ];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].changelog, "#### Feature");
        assert_eq!(plan.none_entries.len(), 1);
        assert_eq!(plan.none_entries[0].changesets, vec!["cs2".to_string()]);
    }

    #[test]
    fn none_bump_listed_once_across_packages() {
        let packages = vec![make_pkg("a", "1.0.0"), make_pkg("b", "1.0.0")];
        let changesets = vec![make_cs(
            &[("a", BumpLevel::None), ("b", BumpLevel::None)],
            "#### CI",
            "cs1",
        )];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert!(plan.releases.is_empty());
        assert_eq!(plan.none_entries.len(), 1);
    }

    #[test]
    fn mixed_changeset_not_repeated_as_none_entry() {
        let packages = vec![make_pkg("a", "1.0.0"), make_pkg("b", "1.0.0")];
        let changesets = vec![make_cs(
            &[("a", BumpLevel::Patch), ("b", BumpLevel::None)],
            "#### Fix",
            "cs1",
        )];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].name, "a");
        assert!(plan.none_entries.is_empty());
    }

    #[test]
    fn default_maps_to_single_package() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![make_cs(&[("default", BumpLevel::Patch)], "#### Fix", "cs1")];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].name, "mylib");
    }

    #[test]
    fn monorepo_independent_bumps() {
        let packages = vec![make_pkg("backend", "1.0.0"), make_pkg("frontend", "2.0.0")];
        let changesets = vec![
            make_cs(&[("backend", BumpLevel::Minor)], "#### API", "cs1"),
            make_cs(&[("frontend", BumpLevel::Patch)], "#### UI fix", "cs2"),
        ];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert_eq!(plan.releases.len(), 2);

        let backend = plan.releases.iter().find(|r| r.name == "backend").unwrap();
        assert_eq!(backend.version, Version::new(1, 1, 0));

        let frontend = plan.releases.iter().find(|r| r.name == "frontend").unwrap();
        assert_eq!(frontend.version, Version::new(2, 0, 1));
    }

    #[test]
    fn multi_package_changeset() {
        let packages = vec![make_pkg("mylib", "1.0.0"), make_pkg("my-api", "2.0.0")];
        let changesets = vec![make_cs(
            &[("mylib", BumpLevel::Patch), ("my-api", BumpLevel::Minor)],
            "#### Shared fix",
            "cs1",
        )];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert_eq!(plan.releases.len(), 2);
    }

    #[test]
    fn changelog_combines_bodies() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![
            make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix one", "cs1"),
            make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix two", "cs2"),
        ];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert!(plan.releases[0].changelog.contains("Fix one"));
        assert!(plan.releases[0].changelog.contains("Fix two"));
    }

    #[test]
    fn major_bump() {
        let packages = vec![make_pkg("mylib", "1.2.3")];
        let changesets = vec![make_cs(
            &[("mylib", BumpLevel::Major)],
            "#### Breaking",
            "cs1",
        )];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert_eq!(plan.releases[0].version, Version::new(2, 0, 0));
    }

    #[test]
    fn empty_changesets() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let plan = assemble(&[], &packages, &Config::default(), None);
        assert!(plan.releases.is_empty());
        assert!(plan.none_entries.is_empty());
    }

    fn make_config_with_fixed(group_name: &str, members: &[&str]) -> Config {
        let mut groups = BTreeMap::new();
        groups.insert(
            group_name.to_string(),
            crate::config::GroupConfig {
                fixed: members.iter().map(|s| s.to_string()).collect(),
                linked: vec![],
            },
        );
        Config {
            groups,
            ..Default::default()
        }
    }

    fn make_config_with_linked(group_name: &str, members: &[&str]) -> Config {
        let mut groups = BTreeMap::new();
        groups.insert(
            group_name.to_string(),
            crate::config::GroupConfig {
                fixed: vec![],
                linked: members.iter().map(|s| s.to_string()).collect(),
            },
        );
        Config {
            groups,
            ..Default::default()
        }
    }

    #[test]
    fn fixed_group_all_members_bump() {
        let packages = vec![
            make_pkg("core-lib", "1.0.0"),
            make_pkg("core-macros", "1.0.0"),
        ];
        let changesets = vec![make_cs(
            &[("core-lib", BumpLevel::Minor)],
            "#### Feature",
            "cs1",
        )];
        let config = make_config_with_fixed("core", &["core-lib", "core-macros"]);

        let plan = assemble(&changesets, &packages, &config, None);
        assert_eq!(plan.releases.len(), 2);

        let lib = plan.releases.iter().find(|r| r.name == "core-lib").unwrap();
        assert_eq!(lib.version, Version::new(1, 1, 0));
        assert_eq!(lib.bump, BumpLevel::Minor);

        let macros = plan
            .releases
            .iter()
            .find(|r| r.name == "core-macros")
            .unwrap();
        assert_eq!(macros.version, Version::new(1, 1, 0));
        assert_eq!(macros.bump, BumpLevel::Minor);
    }

    #[test]
    fn fixed_group_highest_bump_wins() {
        let packages = vec![make_pkg("a", "1.0.0"), make_pkg("b", "1.0.0")];
        let changesets = vec![
            make_cs(&[("a", BumpLevel::Patch)], "#### Fix", "cs1"),
            make_cs(&[("b", BumpLevel::Minor)], "#### Feature", "cs2"),
        ];
        let config = make_config_with_fixed("group", &["a", "b"]);

        let plan = assemble(&changesets, &packages, &config, None);
        assert_eq!(plan.releases.len(), 2);

        let a = plan.releases.iter().find(|r| r.name == "a").unwrap();
        assert_eq!(a.bump, BumpLevel::Minor);
        let b = plan.releases.iter().find(|r| r.name == "b").unwrap();
        assert_eq!(b.bump, BumpLevel::Minor);
    }

    #[test]
    fn fixed_group_no_changesets_no_bump() {
        let packages = vec![make_pkg("a", "1.0.0"), make_pkg("b", "1.0.0")];
        let config = make_config_with_fixed("group", &["a", "b"]);

        let plan = assemble(&[], &packages, &config, None);
        assert!(plan.releases.is_empty());
    }

    #[test]
    fn linked_group_only_changed_members_bump() {
        let packages = vec![make_pkg("util-a", "1.0.0"), make_pkg("util-b", "1.0.0")];
        let changesets = vec![make_cs(&[("util-a", BumpLevel::Patch)], "#### Fix", "cs1")];
        let config = make_config_with_linked("utils", &["util-a", "util-b"]);

        let plan = assemble(&changesets, &packages, &config, None);
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].name, "util-a");
    }

    #[test]
    fn linked_group_both_bump_to_highest() {
        let packages = vec![make_pkg("util-a", "1.0.0"), make_pkg("util-b", "1.0.0")];
        let changesets = vec![
            make_cs(&[("util-a", BumpLevel::Patch)], "#### Fix", "cs1"),
            make_cs(&[("util-b", BumpLevel::Minor)], "#### Feature", "cs2"),
        ];
        let config = make_config_with_linked("utils", &["util-a", "util-b"]);

        let plan = assemble(&changesets, &packages, &config, None);
        assert_eq!(plan.releases.len(), 2);

        let a = plan.releases.iter().find(|r| r.name == "util-a").unwrap();
        assert_eq!(a.bump, BumpLevel::Minor);
        let b = plan.releases.iter().find(|r| r.name == "util-b").unwrap();
        assert_eq!(b.bump, BumpLevel::Minor);
    }

    #[test]
    fn linked_group_no_changesets_no_bump() {
        let packages = vec![make_pkg("a", "1.0.0"), make_pkg("b", "1.0.0")];
        let config = make_config_with_linked("group", &["a", "b"]);

        let plan = assemble(&[], &packages, &config, None);
        assert!(plan.releases.is_empty());
    }

    fn make_cascade_config(mode: &str) -> Config {
        Config {
            update_internal_dependencies: Some(mode.to_string()),
            ..Default::default()
        }
    }

    fn setup_cargo_dep_pair() -> (tempfile::TempDir, Package, Package) {
        let dir = tempfile::tempdir().unwrap();

        let a_dir = dir.path().join("a");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::write(
            a_dir.join("Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();

        let b_dir = dir.path().join("b");
        std::fs::create_dir_all(&b_dir).unwrap();
        std::fs::write(
            b_dir.join("Cargo.toml"),
            "[package]\nname = \"b\"\nversion = \"1.0.0\"\n\n[dependencies]\na = \"1\"\n",
        )
        .unwrap();

        let pkg_a = Package {
            name: "a".to_string(),
            path: a_dir,
            package_type: crate::package::types::PackageType::Cargo,
            version: Version::parse("1.0.0").unwrap(),
        };
        let pkg_b = Package {
            name: "b".to_string(),
            path: b_dir,
            package_type: crate::package::types::PackageType::Cargo,
            version: Version::parse("1.0.0").unwrap(),
        };

        (dir, pkg_a, pkg_b)
    }

    #[test]
    fn cascade_patch_to_dependent() {
        let (_dir, pkg_a, pkg_b) = setup_cargo_dep_pair();
        let packages = vec![pkg_a, pkg_b];
        let changesets = vec![make_cs(&[("a", BumpLevel::Patch)], "#### Fix", "cs1")];
        let config = make_cascade_config("patch");

        let plan = assemble(&changesets, &packages, &config, None);
        assert_eq!(plan.releases.len(), 2);

        let a = plan.releases.iter().find(|r| r.name == "a").unwrap();
        assert_eq!(a.version, Version::new(1, 0, 1));

        let b = plan.releases.iter().find(|r| r.name == "b").unwrap();
        assert_eq!(b.version, Version::new(1, 0, 1));
        assert_eq!(b.bump, BumpLevel::Patch);
    }

    #[test]
    fn cascade_disabled_by_default() {
        let (_dir, pkg_a, pkg_b) = setup_cargo_dep_pair();
        let packages = vec![pkg_a, pkg_b];
        let changesets = vec![make_cs(&[("a", BumpLevel::Patch)], "#### Fix", "cs1")];
        let config = Config::default();

        let plan = assemble(&changesets, &packages, &config, None);
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].name, "a");
    }

    #[test]
    fn cascade_none_mode_disabled() {
        let (_dir, pkg_a, pkg_b) = setup_cargo_dep_pair();
        let packages = vec![pkg_a, pkg_b];
        let changesets = vec![make_cs(&[("a", BumpLevel::Patch)], "#### Fix", "cs1")];
        let config = make_cascade_config("none");

        let plan = assemble(&changesets, &packages, &config, None);
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].name, "a");
    }

    #[test]
    fn cascade_does_not_override_higher_bump() {
        let (_dir, pkg_a, pkg_b) = setup_cargo_dep_pair();
        let packages = vec![pkg_a, pkg_b];
        let changesets = vec![
            make_cs(&[("a", BumpLevel::Patch)], "#### Fix", "cs1"),
            make_cs(&[("b", BumpLevel::Minor)], "#### Feature", "cs2"),
        ];
        let config = make_cascade_config("patch");

        let plan = assemble(&changesets, &packages, &config, None);
        assert_eq!(plan.releases.len(), 2);

        let b = plan.releases.iter().find(|r| r.name == "b").unwrap();
        assert_eq!(b.bump, BumpLevel::Minor);
    }

    #[test]
    fn cascade_transitive_three_deep() {
        let dir = tempfile::tempdir().unwrap();

        let a_dir = dir.path().join("a");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::write(
            a_dir.join("Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();

        let b_dir = dir.path().join("b");
        std::fs::create_dir_all(&b_dir).unwrap();
        std::fs::write(
            b_dir.join("Cargo.toml"),
            "[package]\nname = \"b\"\nversion = \"1.0.0\"\n\n[dependencies]\na = \"1\"\n",
        )
        .unwrap();

        let c_dir = dir.path().join("c");
        std::fs::create_dir_all(&c_dir).unwrap();
        std::fs::write(
            c_dir.join("Cargo.toml"),
            "[package]\nname = \"c\"\nversion = \"1.0.0\"\n\n[dependencies]\nb = \"1\"\n",
        )
        .unwrap();

        let packages = vec![
            Package {
                name: "a".to_string(),
                path: a_dir,
                package_type: crate::package::types::PackageType::Cargo,
                version: Version::parse("1.0.0").unwrap(),
            },
            Package {
                name: "b".to_string(),
                path: b_dir,
                package_type: crate::package::types::PackageType::Cargo,
                version: Version::parse("1.0.0").unwrap(),
            },
            Package {
                name: "c".to_string(),
                path: c_dir,
                package_type: crate::package::types::PackageType::Cargo,
                version: Version::parse("1.0.0").unwrap(),
            },
        ];
        let changesets = vec![make_cs(&[("a", BumpLevel::Patch)], "#### Fix", "cs1")];
        let config = make_cascade_config("patch");

        let plan = assemble(&changesets, &packages, &config, None);
        assert_eq!(plan.releases.len(), 3);
        assert!(plan.releases.iter().any(|r| r.name == "c"));
    }

    #[test]
    fn pre_release_adds_tag_and_counter() {
        let packages = vec![make_pkg("mylib", "0.5.0")];
        let changesets = vec![make_cs(
            &[("mylib", BumpLevel::Minor)],
            "#### Feature",
            "cs1",
        )];
        let pre = PreState {
            mode: "pre".to_string(),
            tag: "rc".to_string(),
            packages_released: BTreeMap::new(),
        };

        let plan = assemble(&changesets, &packages, &Config::default(), Some(&pre));
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].version.to_string(), "0.6.0-rc.0");
    }

    #[test]
    fn pre_release_increments_counter() {
        let packages = vec![make_pkg("mylib", "0.5.0")];
        let changesets = vec![make_cs(
            &[("mylib", BumpLevel::Minor)],
            "#### Feature",
            "cs1",
        )];
        let pre = PreState {
            mode: "pre".to_string(),
            tag: "rc".to_string(),
            packages_released: BTreeMap::from([("mylib".to_string(), 2)]),
        };

        let plan = assemble(&changesets, &packages, &Config::default(), Some(&pre));
        assert_eq!(plan.releases[0].version.to_string(), "0.6.0-rc.2");
    }

    #[test]
    fn pre_exit_produces_stable_version() {
        let packages = vec![make_pkg("mylib", "0.5.0")];
        let changesets = vec![make_cs(
            &[("mylib", BumpLevel::Minor)],
            "#### Feature",
            "cs1",
        )];
        let pre = PreState {
            mode: "exit".to_string(),
            tag: "rc".to_string(),
            packages_released: BTreeMap::from([("mylib".to_string(), 3)]),
        };

        let plan = assemble(&changesets, &packages, &Config::default(), Some(&pre));
        assert_eq!(plan.releases[0].version.to_string(), "0.6.0");
        assert!(plan.releases[0].version.pre.is_none());
    }

    #[test]
    fn no_pre_state_produces_stable_version() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix", "cs1")];

        let plan = assemble(&changesets, &packages, &Config::default(), None);
        assert_eq!(plan.releases[0].version.to_string(), "1.0.1");
        assert!(plan.releases[0].version.pre.is_none());
    }

    fn grouped() -> Config {
        let mut config = Config::default();
        config.changelog.group = true;
        config
    }

    fn typed(mut cs: Changeset, kind: &str) -> Changeset {
        cs.kind = Some(kind.to_string());
        cs
    }

    #[test]
    fn flat_changelog_is_unchanged_but_entries_are_typed() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![
            make_cs(
                &[("mylib", BumpLevel::Patch)],
                "#### fix(ui): Button",
                "cs1",
            ),
            make_cs(&[("mylib", BumpLevel::Minor)], "Biomes", "cs2"),
        ];
        let plan = assemble(&changesets, &packages, &Config::default(), None);
        let r = &plan.releases[0];
        assert_eq!(r.changelog, "#### fix(ui): Button\n\nBiomes");
        assert_eq!(r.entries[0].kind, "fix");
        assert_eq!(r.entries[0].scope.as_deref(), Some("ui"));
        assert_eq!(r.entries[0].summary, "Button");
        assert_eq!(r.entries[0].group, None);
        assert_eq!(r.entries[1].kind, "feat");
        assert_eq!(r.entries[1].bump, BumpLevel::Minor);
    }

    #[test]
    fn grouped_changelog_lists_by_type() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![
            make_cs(&[("mylib", BumpLevel::Patch)], "Fix the crash", "cs1"),
            make_cs(
                &[("mylib", BumpLevel::Minor)],
                "#### feat(world): Biomes\n\nThree of them.",
                "cs2",
            ),
            typed(
                make_cs(&[("mylib", BumpLevel::Patch)], "Faster saves", "cs3"),
                "perf",
            ),
            typed(
                make_cs(&[("mylib", BumpLevel::Patch)], "Odd one", "cs4"),
                "wat",
            ),
        ];
        let plan = assemble(&changesets, &packages, &grouped(), None);
        assert_eq!(
            plan.releases[0].changelog,
            "### Features\n\n- **world:** Biomes\n\n  Three of them.\n\n\
             ### Fixes\n\n- Fix the crash\n\n\
             ### Performance\n\n- Faster saves\n\n\
             ### Other changes\n\n- Odd one"
        );
        assert_eq!(plan.releases[0].bump, BumpLevel::Minor);
        assert_eq!(
            plan.releases[0].entries[1].group.as_deref(),
            Some("Features")
        );
        assert_eq!(plan.releases[0].entries[3].group, None);
    }

    #[test]
    fn grouped_skip_drops_from_changelog_but_still_bumps() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let mut config = Config::default();
        config.changelog.groups = vec![
            crate::config::ChangelogGroup {
                title: "Hidden".into(),
                types: vec!["test".into()],
                pattern: None,
                skip: true,
            },
            crate::config::ChangelogGroup {
                title: "Changes".into(),
                types: vec![],
                pattern: None,
                skip: false,
            },
        ];
        let changesets = vec![
            typed(
                make_cs(&[("mylib", BumpLevel::Minor)], "More tests", "cs1"),
                "test",
            ),
            make_cs(&[("mylib", BumpLevel::Patch)], "A fix", "cs2"),
            typed(
                make_cs(&[("mylib", BumpLevel::None)], "Test CI", "cs3"),
                "test",
            ),
        ];
        let plan = assemble(&changesets, &packages, &config, None);
        assert_eq!(plan.releases[0].version, Version::new(1, 1, 0));
        assert_eq!(plan.releases[0].changelog, "### Changes\n\n- A fix");
        assert!(plan.releases[0].entries[0].skip);
        assert!(plan.none_entries[0].skip);
    }

    #[test]
    fn grouped_none_entries_are_list_items() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![make_cs(
            &[("mylib", BumpLevel::None)],
            "ci(actions): Cache cargo",
            "cs1",
        )];
        let plan = assemble(&changesets, &packages, &grouped(), None);
        let n = &plan.none_entries[0];
        assert_eq!(n.body, "- **actions:** Cache cargo");
        assert_eq!(n.kind, "ci");
        assert_eq!(n.group.as_deref(), Some("Under the hood"));
    }

    #[test]
    fn grouped_combined_monorepo_nests_groups_under_packages() {
        let packages = vec![make_pkg("a", "1.0.0"), make_pkg("b", "1.0.0")];
        let changesets = vec![make_cs(&[("a", BumpLevel::Patch)], "Fix", "cs1")];
        let plan = assemble(&changesets, &packages, &grouped(), None);
        assert_eq!(plan.releases[0].changelog, "#### Fixes\n\n- Fix");
    }

    #[test]
    fn breaking_type_below_major_warns() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![
            make_cs(&[("mylib", BumpLevel::Minor)], "feat!: New saves", "cs1"),
            make_cs(&[("mylib", BumpLevel::Major)], "Drop old saves", "cs2"),
        ];
        let plan = assemble(&changesets, &packages, &Config::default(), None);
        let warnings = plan.breaking_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("cs1.md"), "{}", warnings[0]);
    }

    fn linked(grouped: bool) -> Config {
        let mut config = Config::default();
        config.changelog.group = grouped;
        config.changelog.links = true;
        config.changelog.repo_url = Some("git@github.com:o/r.git".to_string());
        config
    }

    fn from_pr(mut cs: Changeset, pr: Option<u64>) -> Changeset {
        cs.origin = Some(Origin {
            commit: "a1b2c3d4e5f60718".into(),
            author: "Ada".into(),
            pr,
        });
        cs
    }

    #[test]
    fn grouped_links_show_pr_commit_and_issues() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![
            from_pr(
                make_cs(
                    &[("mylib", BumpLevel::Patch)],
                    "Fix #12\n\nMore on #13.",
                    "cs1",
                ),
                Some(24),
            ),
            make_cs(&[("mylib", BumpLevel::Patch)], "Local fix", "cs2"),
        ];
        let plan = assemble(&changesets, &packages, &linked(true), None);
        assert_eq!(
            plan.releases[0].changelog,
            "### Fixes\n\n\
             - Fix [#12](https://github.com/o/r/issues/12) \
             ([#24](https://github.com/o/r/pull/24), [a1b2c3d](https://github.com/o/r/commit/a1b2c3d4e5f60718))\n\n  \
             More on [#13](https://github.com/o/r/issues/13).\n\n\
             - Local fix"
        );
        let json = plan.releases[0].entries[0].to_json();
        assert_eq!(json["pr"], 24);
        assert_eq!(json["commit"], "a1b2c3d4e5f60718");
        assert_eq!(json["author"], "Ada");
        assert_eq!(
            plan.releases[0].entries[1].to_json()["pr"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn flat_links_only_link_issues() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![from_pr(
            make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix #12", "cs1"),
            Some(24),
        )];
        let plan = assemble(&changesets, &packages, &linked(false), None);
        assert_eq!(
            plan.releases[0].changelog,
            "#### Fix [#12](https://github.com/o/r/issues/12)"
        );
    }

    #[test]
    fn links_off_leave_text_alone() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![from_pr(
            make_cs(&[("mylib", BumpLevel::Patch)], "Fix #12", "cs1"),
            Some(24),
        )];
        let plan = assemble(&changesets, &packages, &grouped(), None);
        assert_eq!(plan.releases[0].changelog, "### Fixes\n\n- Fix #12");
    }

    #[test]
    fn grouped_none_entries_get_links_too() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![from_pr(
            make_cs(&[("mylib", BumpLevel::None)], "ci: Cache", "cs1"),
            None,
        )];
        let plan = assemble(&changesets, &packages, &linked(true), None);
        assert_eq!(
            plan.none_entries[0].body,
            "- Cache ([a1b2c3d](https://github.com/o/r/commit/a1b2c3d4e5f60718))"
        );
    }
}
