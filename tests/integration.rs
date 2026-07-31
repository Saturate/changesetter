use std::path::Path;
use std::process::Command;

use changesetter::cli::add::AddArgs;
use changesetter::cli::check::CheckArgs;
use changesetter::cli::init::InitArgs;
use changesetter::cli::release::ReleaseArgs;
use changesetter::package::adapter::Adapter;
use changesetter::package::cargo::CargoAdapter;
use changesetter::package::npm::NpmAdapter;
use changesetter::package::types::Version;

fn setup_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "test@test.com"],
        vec!["config", "user.name", "Test"],
        vec!["config", "commit.gpgsign", "false"],
        vec!["config", "tag.gpgsign", "false"],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(dir.path())
            .output()
            .unwrap();
    }
    Command::new("git")
        .args(["commit", "-q", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    dir
}

fn setup_cargo_repo() -> tempfile::TempDir {
    let dir = setup_repo();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"testpkg\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    git_add_commit(dir.path(), "add cargo manifest");
    dir
}

fn setup_npm_repo() -> tempfile::TempDir {
    let dir = setup_repo();
    std::fs::write(
        dir.path().join("package.json"),
        "{\n  \"name\": \"my-app\",\n  \"version\": \"1.0.0\"\n}\n",
    )
    .unwrap();
    git_add_commit(dir.path(), "add npm manifest");
    dir
}

fn setup_polyglot_repo() -> tempfile::TempDir {
    let dir = setup_repo();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"backend\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("web")).unwrap();
    std::fs::write(
        dir.path().join("web/package.json"),
        "{\n  \"name\": \"frontend\",\n  \"version\": \"2.0.0\"\n}\n",
    )
    .unwrap();
    git_add_commit(dir.path(), "add manifests");
    dir
}

fn git_add_commit(dir: &Path, msg: &str) {
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-q", "-m", msg])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn count_changeset_files(dir: &Path) -> usize {
    let changeset_dir = dir.join(".changeset");
    if !changeset_dir.exists() {
        return 0;
    }
    std::fs::read_dir(&changeset_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "md") && e.file_name() != "README.md"
        })
        .count()
}

// ---------- Full round-trip: Cargo ----------

#[test]
fn roundtrip_cargo() {
    let dir = setup_cargo_repo();

    // init
    changesetter::cli::init::run_in(dir.path(), &InitArgs { config: false }).unwrap();
    assert!(dir.path().join(".changeset").exists());

    // add
    changesetter::cli::add::run_in(
        dir.path(),
        &AddArgs {
            package: vec!["testpkg".to_string()],
            bump: Some("patch".to_string()),
            no_bump: false,
            message: Some("Fixed a bug".to_string()),
        },
    )
    .unwrap();
    assert_eq!(count_changeset_files(dir.path()), 1);

    // check
    changesetter::cli::check::run_in(dir.path(), &CheckArgs { base: None }).unwrap();

    // status
    changesetter::cli::status::run_in(dir.path()).unwrap();

    // release --no-commit
    changesetter::cli::release::run_in(
        dir.path(),
        &ReleaseArgs {
            dry_run: false,
            no_commit: true,
            output: None,
        },
    )
    .unwrap();

    // Verify version bumped
    let v = CargoAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 0, 1));

    // Verify changelog created
    let changelog = std::fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("## 1.0.1"));
    assert!(changelog.contains("Fixed a bug"));

    // Verify changeset consumed
    assert_eq!(count_changeset_files(dir.path()), 0);
}

// ---------- Full round-trip: npm ----------

#[test]
fn roundtrip_npm() {
    let dir = setup_npm_repo();

    changesetter::cli::init::run_in(dir.path(), &InitArgs { config: false }).unwrap();

    changesetter::cli::add::run_in(
        dir.path(),
        &AddArgs {
            package: vec!["my-app".to_string()],
            bump: Some("minor".to_string()),
            no_bump: false,
            message: Some("Added feature".to_string()),
        },
    )
    .unwrap();

    changesetter::cli::check::run_in(dir.path(), &CheckArgs { base: None }).unwrap();

    changesetter::cli::release::run_in(
        dir.path(),
        &ReleaseArgs {
            dry_run: false,
            no_commit: true,
            output: None,
        },
    )
    .unwrap();

    let v = NpmAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 1, 0));

    let changelog = std::fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("## 1.1.0"));
    assert!(changelog.contains("Added feature"));

    // package.json should preserve formatting
    let pkg_content = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
    assert!(pkg_content.contains("\"version\": \"1.1.0\""));
    assert!(pkg_content.ends_with('\n'));

    assert_eq!(count_changeset_files(dir.path()), 0);
}

// ---------- Full round-trip: polyglot ----------

#[test]
fn roundtrip_polyglot() {
    let dir = setup_polyglot_repo();

    changesetter::cli::init::run_in(dir.path(), &InitArgs { config: false }).unwrap();

    // Add changeset for backend
    changesetter::cli::add::run_in(
        dir.path(),
        &AddArgs {
            package: vec!["backend".to_string()],
            bump: Some("minor".to_string()),
            no_bump: false,
            message: Some("New API endpoint".to_string()),
        },
    )
    .unwrap();

    // Add changeset for frontend
    changesetter::cli::add::run_in(
        dir.path(),
        &AddArgs {
            package: vec!["frontend".to_string()],
            bump: Some("patch".to_string()),
            no_bump: false,
            message: Some("UI fix".to_string()),
        },
    )
    .unwrap();

    assert_eq!(count_changeset_files(dir.path()), 2);

    changesetter::cli::release::run_in(
        dir.path(),
        &ReleaseArgs {
            dry_run: false,
            no_commit: true,
            output: None,
        },
    )
    .unwrap();

    let cargo_v = CargoAdapter.read_version(dir.path()).unwrap();
    assert_eq!(cargo_v, Version::new(1, 1, 0));

    let npm_v = NpmAdapter.read_version(&dir.path().join("web")).unwrap();
    assert_eq!(npm_v, Version::new(2, 0, 1));

    assert_eq!(count_changeset_files(dir.path()), 0);
}

// ---------- None-bump round-trip ----------

#[test]
fn roundtrip_none_bump() {
    let dir = setup_cargo_repo();

    changesetter::cli::init::run_in(dir.path(), &InitArgs { config: false }).unwrap();

    changesetter::cli::add::run_in(
        dir.path(),
        &AddArgs {
            package: vec!["testpkg".to_string()],
            bump: None,
            no_bump: true,
            message: Some("Updated CI config".to_string()),
        },
    )
    .unwrap();

    // check should pass
    changesetter::cli::check::run_in(dir.path(), &CheckArgs { base: None }).unwrap();

    changesetter::cli::release::run_in(
        dir.path(),
        &ReleaseArgs {
            dry_run: false,
            no_commit: true,
            output: None,
        },
    )
    .unwrap();

    // Version should NOT change
    let v = CargoAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 0, 0));

    // Changelog should have Internal section
    let changelog = std::fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("### Internal"));
    assert!(changelog.contains("Updated CI config"));

    assert_eq!(count_changeset_files(dir.path()), 0);
}

// ---------- Release with tagging ----------

#[test]
fn release_creates_git_tag() {
    let dir = setup_cargo_repo();

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("test-change.md"),
        "---\ntestpkg: minor\n---\n\n#### New feature\n",
    )
    .unwrap();
    git_add_commit(dir.path(), "add changeset");

    changesetter::cli::release::run_in(
        dir.path(),
        &ReleaseArgs {
            dry_run: false,
            no_commit: false,
            output: None,
        },
    )
    .unwrap();

    let output = Command::new("git")
        .args(["tag", "-l"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let tags = String::from_utf8_lossy(&output.stdout);
    assert!(tags.contains("v1.1.0"), "expected v1.1.0 tag, got: {tags}");

    // Verify it's annotated (cat-file -t returns "tag" for annotated, "commit" for lightweight)
    let output = Command::new("git")
        .args(["cat-file", "-t", "v1.1.0"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let obj_type = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(obj_type, "tag", "v1.1.0 should be an annotated tag");
}

// ---------- Release dry-run ----------

#[test]
fn release_dry_run_no_changes() {
    let dir = setup_cargo_repo();

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("test.md"),
        "---\ntestpkg: patch\n---\n\n#### Fix\n",
    )
    .unwrap();

    changesetter::cli::release::run_in(
        dir.path(),
        &ReleaseArgs {
            dry_run: true,
            no_commit: false,
            output: None,
        },
    )
    .unwrap();

    // Version should NOT have changed
    let v = CargoAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 0, 0));

    // No CHANGELOG.md created
    assert!(!dir.path().join("CHANGELOG.md").exists());

    // Changeset still present
    assert!(changeset_dir.join("test.md").exists());
}

// ---------- Check with --base ----------

#[test]
fn check_with_base_finds_changeset_on_branch() {
    let dir = setup_cargo_repo();

    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("test.md"),
        "---\ntestpkg: patch\n---\n\n#### Fix\n",
    )
    .unwrap();
    git_add_commit(dir.path(), "add changeset");

    changesetter::cli::check::run_in(
        dir.path(),
        &CheckArgs {
            base: Some("main".to_string()),
        },
    )
    .unwrap();
}

// ---------- Check fails when empty ----------

#[test]
fn check_fails_no_changesets() {
    let dir = setup_cargo_repo();

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();

    let result = changesetter::cli::check::run_in(dir.path(), &CheckArgs { base: None });
    assert!(result.is_err());
}

#[test]
fn check_fails_no_changeset_dir() {
    let dir = setup_cargo_repo();
    let result = changesetter::cli::check::run_in(dir.path(), &CheckArgs { base: None });
    assert!(result.is_err());
}

// ---------- Init idempotent ----------

#[test]
fn init_twice_no_error() {
    let dir = setup_cargo_repo();
    let args = InitArgs { config: false };
    changesetter::cli::init::run_in(dir.path(), &args).unwrap();
    changesetter::cli::init::run_in(dir.path(), &args).unwrap();
    assert!(dir.path().join(".changeset").exists());
}

#[test]
fn init_with_config_creates_toml() {
    let dir = setup_cargo_repo();
    changesetter::cli::init::run_in(dir.path(), &InitArgs { config: true }).unwrap();
    assert!(dir.path().join("changesetter.toml").exists());
    let content = std::fs::read_to_string(dir.path().join("changesetter.toml")).unwrap();
    assert!(content.contains("changesetter.toml"));
}

// ---------- Add unknown package fails ----------

#[test]
fn add_unknown_package_errors() {
    let dir = setup_cargo_repo();
    std::fs::create_dir_all(dir.path().join(".changeset")).unwrap();

    let result = changesetter::cli::add::run_in(
        dir.path(),
        &AddArgs {
            package: vec!["nonexistent".to_string()],
            bump: Some("patch".to_string()),
            no_bump: false,
            message: Some("Fix".to_string()),
        },
    );
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("nonexistent"),
        "error should name the unknown package, got: {err}"
    );
}

// ---------- Release no pending changesets ----------

#[test]
fn release_no_changesets_succeeds() {
    let dir = setup_cargo_repo();
    std::fs::create_dir_all(dir.path().join(".changeset")).unwrap();

    changesetter::cli::release::run_in(
        dir.path(),
        &ReleaseArgs {
            dry_run: false,
            no_commit: true,
            output: None,
        },
    )
    .unwrap();

    // Version unchanged
    let v = CargoAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 0, 0));
}

// ---------- Monorepo tagging produces per-package tags ----------

#[test]
fn monorepo_release_creates_scoped_tags() {
    let dir = setup_polyglot_repo();

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("cs1.md"),
        "---\nbackend: minor\n---\n\n#### API\n",
    )
    .unwrap();
    std::fs::write(
        changeset_dir.join("cs2.md"),
        "---\nfrontend: patch\n---\n\n#### UI fix\n",
    )
    .unwrap();
    git_add_commit(dir.path(), "add changesets");

    changesetter::cli::release::run_in(
        dir.path(),
        &ReleaseArgs {
            dry_run: false,
            no_commit: false,
            output: None,
        },
    )
    .unwrap();

    let output = Command::new("git")
        .args(["tag", "-l"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let tags = String::from_utf8_lossy(&output.stdout);
    assert!(
        tags.contains("backend@v1.1.0"),
        "expected backend@v1.1.0, got: {tags}"
    );
    assert!(
        tags.contains("frontend@v2.0.1"),
        "expected frontend@v2.0.1, got: {tags}"
    );
}

// ---------- Multiple changesets for same package ----------

#[test]
fn multiple_changesets_highest_bump_wins() {
    let dir = setup_cargo_repo();

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("fix.md"),
        "---\ntestpkg: patch\n---\n\n#### Bug fix\n",
    )
    .unwrap();
    std::fs::write(
        changeset_dir.join("feat.md"),
        "---\ntestpkg: minor\n---\n\n#### New feature\n",
    )
    .unwrap();

    changesetter::cli::release::run_in(
        dir.path(),
        &ReleaseArgs {
            dry_run: false,
            no_commit: true,
            output: None,
        },
    )
    .unwrap();

    let v = CargoAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 1, 0));

    let changelog = std::fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("Bug fix"));
    assert!(changelog.contains("New feature"));
}

// ---------- Changelog prepends to existing ----------

#[test]
fn changelog_prepends_to_existing() {
    let dir = setup_cargo_repo();

    std::fs::write(
        dir.path().join("CHANGELOG.md"),
        "# Changelog\n\n## 0.9.0 - 2026-01-01\n\nInitial release.\n",
    )
    .unwrap();

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("test.md"),
        "---\ntestpkg: patch\n---\n\n#### Fix\n",
    )
    .unwrap();

    changesetter::cli::release::run_in(
        dir.path(),
        &ReleaseArgs {
            dry_run: false,
            no_commit: true,
            output: None,
        },
    )
    .unwrap();

    let changelog = std::fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    let new_pos = changelog.find("## 1.0.1").unwrap();
    let old_pos = changelog.find("## 0.9.0").unwrap();
    assert!(
        new_pos < old_pos,
        "new version should appear before old version"
    );
}
