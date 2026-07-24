use std::collections::BTreeMap;

use crate::changeset::types::{BumpLevel, Changeset};
use crate::package::types::{Package, Version};

#[derive(Debug, Clone)]
pub struct ReleasePlan {
    pub releases: Vec<PlannedRelease>,
    pub none_entries: Vec<NoneEntry>,
}

#[derive(Debug, Clone)]
pub struct PlannedRelease {
    pub name: String,
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

pub fn assemble(changesets: &[Changeset], packages: &[Package]) -> ReleasePlan {
    let mut bumps_per_package: BTreeMap<String, (BumpLevel, Vec<(String, String)>)> =
        BTreeMap::new();

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

        let new_version = apply_bump(&previous_version, *bump);

        let changelog = entries
            .iter()
            .map(|(_, body)| body.as_str())
            .collect::<Vec<&str>>()
            .join("\n\n");

        releases.push(PlannedRelease {
            name: pkg_name.clone(),
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

        let plan = assemble(&changesets, &packages);
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

        let plan = assemble(&changesets, &packages);
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

        let plan = assemble(&changesets, &packages);
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

        let plan = assemble(&changesets, &packages);
        assert_eq!(plan.releases.len(), 1);
        assert_eq!(plan.releases[0].version, Version::new(1, 0, 1));
        assert!(plan.none_entries.is_empty());
    }

    #[test]
    fn default_maps_to_single_package() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![make_cs(&[("default", BumpLevel::Patch)], "#### Fix", "cs1")];

        let plan = assemble(&changesets, &packages);
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

        let plan = assemble(&changesets, &packages);
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

        let plan = assemble(&changesets, &packages);
        assert_eq!(plan.releases.len(), 2);
    }

    #[test]
    fn changelog_combines_bodies() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let changesets = vec![
            make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix one", "cs1"),
            make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix two", "cs2"),
        ];

        let plan = assemble(&changesets, &packages);
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

        let plan = assemble(&changesets, &packages);
        assert_eq!(plan.releases[0].version, Version::new(2, 0, 0));
    }

    #[test]
    fn empty_changesets() {
        let packages = vec![make_pkg("mylib", "1.0.0")];
        let plan = assemble(&[], &packages);
        assert!(plan.releases.is_empty());
        assert!(plan.none_entries.is_empty());
    }
}
