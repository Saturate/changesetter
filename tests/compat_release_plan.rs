use std::path::PathBuf;

use changesetter::changeset::types::{BumpLevel, Changeset};
use changesetter::package::types::{Package, PackageType, Version};
use changesetter::config::Config;
use changesetter::release::plan;

fn make_pkg(name: &str, version: &str) -> Package {
    Package {
        name: name.to_string(),
        path: PathBuf::from("."),
        package_type: PackageType::Cargo,
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
fn compat_bump_precedence_patch_minor() {
    let pkgs = vec![make_pkg("mylib", "1.0.0")];
    let css = vec![
        make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix", "cs1"),
        make_cs(&[("mylib", BumpLevel::Minor)], "#### Feature", "cs2"),
    ];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert_eq!(plan.releases.len(), 1);
    assert_eq!(plan.releases[0].version, Version::new(1, 1, 0));
    assert_eq!(plan.releases[0].bump, BumpLevel::Minor);
}

#[test]
fn compat_bump_precedence_minor_major() {
    let pkgs = vec![make_pkg("mylib", "1.2.3")];
    let css = vec![
        make_cs(&[("mylib", BumpLevel::Minor)], "#### Feature", "cs1"),
        make_cs(&[("mylib", BumpLevel::Major)], "#### Breaking", "cs2"),
    ];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert_eq!(plan.releases.len(), 1);
    assert_eq!(plan.releases[0].version, Version::new(2, 0, 0));
    assert_eq!(plan.releases[0].bump, BumpLevel::Major);
}

#[test]
fn compat_none_plus_patch_equals_patch() {
    let pkgs = vec![make_pkg("mylib", "1.0.0")];
    let css = vec![
        make_cs(&[("mylib", BumpLevel::None)], "#### CI", "cs1"),
        make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix", "cs2"),
    ];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert_eq!(plan.releases.len(), 1);
    assert_eq!(plan.releases[0].version, Version::new(1, 0, 1));
    assert!(plan.none_entries.is_empty());
}

#[test]
fn compat_none_only_no_version_change() {
    let pkgs = vec![make_pkg("mylib", "1.0.0")];
    let css = vec![make_cs(
        &[("mylib", BumpLevel::None)],
        "#### CI update",
        "cs1",
    )];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert!(plan.releases.is_empty());
    assert_eq!(plan.none_entries.len(), 1);
}

#[test]
fn compat_default_maps_to_single_package() {
    let pkgs = vec![make_pkg("my-crate", "0.5.0")];
    let css = vec![make_cs(&[("default", BumpLevel::Patch)], "#### Fix", "cs1")];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert_eq!(plan.releases.len(), 1);
    assert_eq!(plan.releases[0].name, "my-crate");
    assert_eq!(plan.releases[0].version, Version::new(0, 5, 1));
}

#[test]
fn compat_monorepo_independent_bumps() {
    let pkgs = vec![make_pkg("backend", "1.0.0"), make_pkg("frontend", "2.0.0")];
    let css = vec![
        make_cs(&[("backend", BumpLevel::Minor)], "#### API", "cs1"),
        make_cs(&[("frontend", BumpLevel::Patch)], "#### UI fix", "cs2"),
    ];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert_eq!(plan.releases.len(), 2);

    let backend = plan.releases.iter().find(|r| r.name == "backend").unwrap();
    assert_eq!(backend.version, Version::new(1, 1, 0));
    assert_eq!(backend.previous_version, Version::new(1, 0, 0));

    let frontend = plan.releases.iter().find(|r| r.name == "frontend").unwrap();
    assert_eq!(frontend.version, Version::new(2, 0, 1));
    assert_eq!(frontend.previous_version, Version::new(2, 0, 0));
}

#[test]
fn compat_multi_package_changeset() {
    let pkgs = vec![make_pkg("mylib", "1.0.0"), make_pkg("my-api", "2.0.0")];
    let css = vec![make_cs(
        &[("mylib", BumpLevel::Patch), ("my-api", BumpLevel::Minor)],
        "#### Shared change",
        "cs1",
    )];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert_eq!(plan.releases.len(), 2);

    let mylib = plan.releases.iter().find(|r| r.name == "mylib").unwrap();
    assert_eq!(mylib.version, Version::new(1, 0, 1));
    assert_eq!(mylib.bump, BumpLevel::Patch);

    let api = plan.releases.iter().find(|r| r.name == "my-api").unwrap();
    assert_eq!(api.version, Version::new(2, 1, 0));
    assert_eq!(api.bump, BumpLevel::Minor);
}

#[test]
fn compat_major_resets_minor_and_patch() {
    let pkgs = vec![make_pkg("mylib", "1.5.3")];
    let css = vec![make_cs(
        &[("mylib", BumpLevel::Major)],
        "#### Breaking",
        "cs1",
    )];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert_eq!(plan.releases[0].version, Version::new(2, 0, 0));
}

#[test]
fn compat_minor_resets_patch() {
    let pkgs = vec![make_pkg("mylib", "1.5.3")];
    let css = vec![make_cs(
        &[("mylib", BumpLevel::Minor)],
        "#### Feature",
        "cs1",
    )];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert_eq!(plan.releases[0].version, Version::new(1, 6, 0));
}

#[test]
fn compat_empty_changesets_empty_plan() {
    let pkgs = vec![make_pkg("mylib", "1.0.0")];
    let plan = plan::assemble(&[], &pkgs, &Config::default());
    assert!(plan.releases.is_empty());
    assert!(plan.none_entries.is_empty());
}

#[test]
fn compat_changelog_bodies_combined() {
    let pkgs = vec![make_pkg("mylib", "1.0.0")];
    let css = vec![
        make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix one", "cs1"),
        make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix two", "cs2"),
    ];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert_eq!(plan.releases.len(), 1);
    assert!(plan.releases[0].changelog.contains("Fix one"));
    assert!(plan.releases[0].changelog.contains("Fix two"));
}

#[test]
fn compat_changesets_tracked_in_release() {
    let pkgs = vec![make_pkg("mylib", "1.0.0")];
    let css = vec![
        make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix", "cool-dogs"),
        make_cs(&[("mylib", BumpLevel::Patch)], "#### Fix 2", "red-lions"),
    ];
    let plan = plan::assemble(&css, &pkgs, &Config::default());
    assert!(
        plan.releases[0]
            .changesets
            .contains(&"cool-dogs".to_string())
    );
    assert!(
        plan.releases[0]
            .changesets
            .contains(&"red-lions".to_string())
    );
}
