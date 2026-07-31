use std::path::PathBuf;

use changesetter::changelog::generator::{generate_changelog_entry, update_changelog_file};
use changesetter::changeset::types::BumpLevel;
use changesetter::config::ChangelogConfig;
use changesetter::package::types::Version;
use changesetter::release::plan::{NoneEntry, PlannedRelease, ReleasePlan};

fn make_release(
    name: &str,
    prev: &str,
    next: &str,
    bump: BumpLevel,
    changelog: &str,
) -> PlannedRelease {
    PlannedRelease {
        name: name.to_string(),
        path: PathBuf::from("."),
        version: Version::parse(next).unwrap(),
        previous_version: Version::parse(prev).unwrap(),
        bump,
        changelog: changelog.to_string(),
        changesets: vec!["cs1".to_string()],
    }
}

fn make_plan(releases: Vec<PlannedRelease>, none_entries: Vec<NoneEntry>) -> ReleasePlan {
    ReleasePlan {
        releases,
        none_entries,
    }
}

#[test]
fn single_release_patch() {
    let plan = make_plan(
        vec![make_release(
            "mylib",
            "1.2.3",
            "1.2.4",
            BumpLevel::Patch,
            "#### Fixed null handling in response parser\n\nThe API was returning null for optional fields.",
        )],
        vec![],
    );
    let entry = generate_changelog_entry(&plan, &ChangelogConfig::default(), "2026-07-25", false);
    insta::assert_snapshot!(entry);
}

#[test]
fn single_release_minor() {
    let plan = make_plan(
        vec![make_release(
            "mylib",
            "1.0.0",
            "1.1.0",
            BumpLevel::Minor,
            "#### Added batch processing endpoint",
        )],
        vec![],
    );
    let entry = generate_changelog_entry(&plan, &ChangelogConfig::default(), "2026-07-25", false);
    insta::assert_snapshot!(entry);
}

#[test]
fn single_release_major() {
    let plan = make_plan(
        vec![make_release(
            "mylib",
            "1.5.3",
            "2.0.0",
            BumpLevel::Major,
            "#### Removed deprecated v1 API\n\nAll v1 endpoints have been removed. Migrate to v2.",
        )],
        vec![],
    );
    let entry = generate_changelog_entry(&plan, &ChangelogConfig::default(), "2026-07-25", false);
    insta::assert_snapshot!(entry);
}

#[test]
fn multiple_releases() {
    let plan = make_plan(
        vec![
            make_release(
                "backend",
                "1.0.0",
                "1.1.0",
                BumpLevel::Minor,
                "#### New authentication flow",
            ),
            make_release(
                "frontend",
                "2.0.0",
                "2.0.1",
                BumpLevel::Patch,
                "#### Fixed login button alignment",
            ),
        ],
        vec![],
    );
    let entry = generate_changelog_entry(&plan, &ChangelogConfig::default(), "2026-07-25", false);
    insta::assert_snapshot!(entry);
}

#[test]
fn release_with_none_entries() {
    let plan = make_plan(
        vec![make_release(
            "mylib",
            "1.0.0",
            "1.0.1",
            BumpLevel::Patch,
            "#### Fixed timeout handling",
        )],
        vec![NoneEntry {
            title: "Updated CI configuration".to_string(),
            body: "#### Updated CI configuration\n\nSwitched from ubuntu-20.04 to ubuntu-24.04 runners.".to_string(),
            changesets: vec!["cs2".to_string()],
        }],
    );
    let entry = generate_changelog_entry(&plan, &ChangelogConfig::default(), "2026-07-25", false);
    insta::assert_snapshot!(entry);
}

#[test]
fn none_only() {
    let plan = make_plan(
        vec![],
        vec![
            NoneEntry {
                title: "Updated CI configuration".to_string(),
                body: "#### Updated CI configuration\n\nSwitched runners.".to_string(),
                changesets: vec!["cs1".to_string()],
            },
            NoneEntry {
                title: "Added CONTRIBUTING.md".to_string(),
                body: "#### Added CONTRIBUTING.md".to_string(),
                changesets: vec!["cs2".to_string()],
            },
        ],
    );
    let entry = generate_changelog_entry(&plan, &ChangelogConfig::default(), "2026-07-25", false);
    insta::assert_snapshot!(entry);
}

#[test]
fn grouped_monorepo() {
    let plan = make_plan(
        vec![
            make_release(
                "mylib",
                "0.1.0",
                "0.2.0",
                BumpLevel::Minor,
                "#### Added retry logic",
            ),
            make_release(
                "my-frontend",
                "1.0.0",
                "1.0.1",
                BumpLevel::Patch,
                "#### Updated dashboard layout",
            ),
        ],
        vec![],
    );
    let config = ChangelogConfig {
        per_package: false,
        ..Default::default()
    };
    let entry = generate_changelog_entry(&plan, &config, "2026-07-25", true);
    insta::assert_snapshot!(entry);
}

#[test]
fn custom_none_heading() {
    let plan = make_plan(
        vec![],
        vec![NoneEntry {
            title: "Docs".to_string(),
            body: "#### Updated README".to_string(),
            changesets: vec!["cs1".to_string()],
        }],
    );
    let config = ChangelogConfig {
        none_bump_heading: "Misc".to_string(),
        ..Default::default()
    };
    let entry = generate_changelog_entry(&plan, &config, "2026-07-25", false);
    insta::assert_snapshot!(entry);
}

#[test]
fn none_omit() {
    let plan = make_plan(
        vec![make_release(
            "mylib",
            "1.0.0",
            "1.0.1",
            BumpLevel::Patch,
            "#### Fix",
        )],
        vec![NoneEntry {
            title: "CI".to_string(),
            body: "#### CI update".to_string(),
            changesets: vec!["cs2".to_string()],
        }],
    );
    let config = ChangelogConfig {
        none_bump: "omit".to_string(),
        ..Default::default()
    };
    let entry = generate_changelog_entry(&plan, &config, "2026-07-25", false);
    insta::assert_snapshot!(entry);
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

    let new_entry = "## 0.2.0 - 2026-07-25\n\n#### Added retry logic\n\nRetries up to 3 times.\n";
    update_changelog_file(&path, new_entry).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    insta::assert_snapshot!(content);
}
