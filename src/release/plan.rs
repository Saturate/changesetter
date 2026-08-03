use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::changeset::types::{BumpLevel, Changeset};
use crate::config::Config;
use crate::package::types::{Package, Version};

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
}

#[derive(Debug, Clone)]
pub struct NoneEntry {
    pub title: String,
    pub body: String,
    pub changesets: Vec<String>,
}

type BumpMap = BTreeMap<String, (BumpLevel, Vec<(String, String)>)>;

pub fn assemble(changesets: &[Changeset], packages: &[Package], config: &Config) -> ReleasePlan {
    let mut bumps_per_package: BumpMap = BTreeMap::new();

    let is_single_package = packages.len() <= 1;

    for cs in changesets {
        let cs_name = cs.filename.clone().unwrap_or_else(|| "unknown".to_string());

        for (pkg_name, bump) in &cs.packages {
            let resolved_name = if pkg_name == "default" && is_single_package {
                packages
                    .first()
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "default".to_string())
            } else {
                pkg_name.clone()
            };

            let entry = bumps_per_package
                .entry(resolved_name)
                .or_insert((BumpLevel::None, Vec::new()));

            if *bump > entry.0 {
                entry.0 = *bump;
            }

            entry.1.push((cs_name.clone(), cs.body.clone()));
        }
    }

    apply_fixed_groups(&mut bumps_per_package, packages, config);
    apply_linked_groups(&mut bumps_per_package, config);

    let mut releases = Vec::new();
    let mut none_entries = Vec::new();

    for (pkg_name, (bump, entries)) in &bumps_per_package {
        let changesets_names: Vec<String> = entries.iter().map(|(n, _)| n.clone()).collect();

        if *bump == BumpLevel::None {
            for (cs_name, body) in entries {
                let title = extract_title(body);
                none_entries.push(NoneEntry {
                    title,
                    body: body.clone(),
                    changesets: vec![cs_name.clone()],
                });
            }
            continue;
        }

        let pkg = packages.iter().find(|p| &p.name == pkg_name);
        let previous_version = pkg
            .map(|p| p.version.clone())
            .unwrap_or_else(|| Version::new(0, 0, 0));
        let pkg_path = pkg
            .map(|p| p.path.clone())
            .unwrap_or_else(|| PathBuf::from("."));

        let new_version = apply_bump(&previous_version, *bump);

        let changelog = entries
            .iter()
            .map(|(_, body)| body.as_str())
            .collect::<Vec<&str>>()
            .join("\n\n");

        releases.push(PlannedRelease {
            name: pkg_name.clone(),
            path: pkg_path,
            version: new_version,
            previous_version,
            bump: *bump,
            changelog,
            changesets: changesets_names,
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
                entry.1.push((
                    "fixed-group".to_string(),
                    format!("#### Bumped as part of fixed group (was {pkg_version})"),
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

fn apply_bump(version: &Version, bump: BumpLevel) -> Version {
    match bump {
        BumpLevel::Major => version.bump_major(),
        BumpLevel::Minor => version.bump_minor(),
        BumpLevel::Patch => version.bump_patch(),
        BumpLevel::None => version.clone(),
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
        }
    }

    #[test]
    fn single_package_patch() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix", "cs1")];

        let plan = assemble(&changesets, &packages, &Config::default());
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

        let plan = assemble(&changesets, &packages, &Config::default());
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

        let plan = assemble(&changesets, &packages, &Config::default());
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

        let plan = assemble(&changesets, &packages, &Config::default());
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].version, Version::new(1, 0, 1));
        assert!(plan.none_entries.is_empty());
    }

    #[test]
    fn default_maps_to_single_package() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![make_cs(&[("default", BumpLevel::Patch)], "#### Fix", "cs1")];

        let plan = assemble(&changesets, &packages, &Config::default());
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

        let plan = assemble(&changesets, &packages, &Config::default());
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

        let plan = assemble(&changesets, &packages, &Config::default());
        assert_eq!(plan.releases.len(), 2);
    }

    #[test]
    fn changelog_combines_bodies() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![
            make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix one", "cs1"),
            make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix two", "cs2"),
        ];

        let plan = assemble(&changesets, &packages, &Config::default());
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

        let plan = assemble(&changesets, &packages, &Config::default());
        assert_eq!(plan.releases[0].version, Version::new(2, 0, 0));
    }

    #[test]
    fn empty_changesets() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let plan = assemble(&[], &packages, &Config::default());
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
        let packages = vec![make_pkg("core-lib", "1.0.0"), make_pkg("core-macros", "1.0.0")];
        let changesets = vec![make_cs(
            &[("core-lib", BumpLevel::Minor)],
            "#### Feature",
            "cs1",
        )];
        let config = make_config_with_fixed("core", &["core-lib", "core-macros"]);

        let plan = assemble(&changesets, &packages, &config);
        assert_eq!(plan.releases.len(), 2);

        let lib = plan.releases.iter().find(|r| r.name == "core-lib").unwrap();
        assert_eq!(lib.version, Version::new(1, 1, 0));
        assert_eq!(lib.bump, BumpLevel::Minor);

        let macros = plan.releases.iter().find(|r| r.name == "core-macros").unwrap();
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

        let plan = assemble(&changesets, &packages, &config);
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

        let plan = assemble(&[], &packages, &config);
        assert!(plan.releases.is_empty());
    }

    #[test]
    fn linked_group_only_changed_members_bump() {
        let packages = vec![make_pkg("util-a", "1.0.0"), make_pkg("util-b", "1.0.0")];
        let changesets = vec![make_cs(
            &[("util-a", BumpLevel::Patch)],
            "#### Fix",
            "cs1",
        )];
        let config = make_config_with_linked("utils", &["util-a", "util-b"]);

        let plan = assemble(&changesets, &packages, &config);
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

        let plan = assemble(&changesets, &packages, &config);
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

        let plan = assemble(&[], &packages, &config);
        assert!(plan.releases.is_empty());
    }
}
