use std::path::Path;
use std::process::Command;

use changesetter::cli::add::AddArgs;
use changesetter::cli::check::CheckArgs;
use changesetter::cli::init::InitArgs;
use changesetter::cli::pre::{PreArgs, PreCommand};
use changesetter::cli::release::ReleaseArgs;
use changesetter::package::adapter::Adapter;
use changesetter::package::cargo::CargoAdapter;
use changesetter::package::dotnet::DotnetAdapter;
use changesetter::package::helm::HelmAdapter;
use changesetter::package::npm::NpmAdapter;
use changesetter::package::python::PythonAdapter;
use changesetter::package::types::Version;
use changesetter::release::pre;

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
    changesetter::cli::check::run_in(
        dir.path(),
        &CheckArgs {
            base: None,
            output: None,
        },
    )
    .unwrap();

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

    changesetter::cli::check::run_in(
        dir.path(),
        &CheckArgs {
            base: None,
            output: None,
        },
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
    changesetter::cli::check::run_in(
        dir.path(),
        &CheckArgs {
            base: None,
            output: None,
        },
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

    // Version should NOT change
    let v = CargoAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 0, 0));

    // Changelog should have Internal section
    let changelog = std::fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("### Internal"));
    assert!(changelog.contains("Updated CI config"));

    assert_eq!(count_changeset_files(dir.path()), 0);
}

fn write_changeset(repo: &Path, name: &str, content: &str) {
    let changeset_dir = repo.join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(changeset_dir.join(format!("{name}.md")), content).unwrap();
}

fn release_no_commit(repo: &Path) {
    changesetter::cli::release::run_in(
        repo,
        &ReleaseArgs {
            dry_run: false,
            no_commit: true,
            output: None,
        },
    )
    .unwrap();
}

#[test]
fn none_bump_next_to_minor_goes_under_internal() {
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "a-feature",
        "---\ndefault: minor\n---\n\n#### New feature\n\nDoes things.\n",
    );
    write_changeset(
        dir.path(),
        "b-ci",
        "---\ndefault: none\n---\n\n#### CI: fail PRs whose changesets don't parse\n",
    );
    git_add_commit(dir.path(), "add changesets");

    release_no_commit(dir.path());

    let v = CargoAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 1, 0));

    let changelog = std::fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    let internal = changelog.find("### Internal").expect("internal heading");
    let ci = changelog.find("#### CI: fail PRs").expect("ci entry");
    assert!(changelog.find("#### New feature").unwrap() < internal);
    assert!(internal < ci);
}

#[test]
fn none_bump_omit_keeps_it_out_of_changelog() {
    let dir = setup_cargo_repo();
    std::fs::write(
        dir.path().join("changesetter.toml"),
        "[changelog]\nnone_bump = \"omit\"\n",
    )
    .unwrap();
    write_changeset(
        dir.path(),
        "a-feature",
        "---\ndefault: minor\n---\n\n#### New feature\n",
    );
    write_changeset(
        dir.path(),
        "b-ci",
        "---\ndefault: none\n---\n\n#### CI: fail PRs whose changesets don't parse\n",
    );
    git_add_commit(dir.path(), "add changesets");

    release_no_commit(dir.path());

    let changelog = std::fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("#### New feature"));
    assert!(!changelog.contains("CI: fail PRs"));
    assert!(!changelog.contains("### Internal"));
    assert_eq!(count_changeset_files(dir.path()), 0);
}

fn release_commit(repo: &Path) {
    changesetter::cli::release::run_in(
        repo,
        &ReleaseArgs {
            dry_run: false,
            no_commit: false,
            output: None,
        },
    )
    .unwrap();
}

fn assert_clean(repo: &Path) {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "", "dirty tree");
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn use_per_package(repo: &Path, extra: &str) {
    std::fs::write(
        repo.join("changesetter.toml"),
        format!("[changelog]\nper_package = true\n{extra}"),
    )
    .unwrap();
    git_add_commit(repo, "config");
}

#[test]
fn none_only_release_goes_under_unreleased() {
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "ci",
        "---\ndefault: none\n---\n\n#### CI tweak\n",
    );
    git_add_commit(dir.path(), "add changeset");

    release_commit(dir.path());

    assert_eq!(
        CargoAdapter.read_version(dir.path()).unwrap(),
        Version::new(1, 0, 0)
    );
    assert_eq!(
        read(&dir.path().join("CHANGELOG.md")),
        "# Changelog\n\n## Unreleased\n\n### Internal\n\n#### CI tweak\n"
    );
    assert_eq!(count_changeset_files(dir.path()), 0);
    assert_clean(dir.path());
}

#[test]
fn unreleased_is_folded_into_the_next_version() {
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "ci",
        "---\ndefault: none\n---\n\n#### CI tweak\n",
    );
    git_add_commit(dir.path(), "add changeset");
    release_commit(dir.path());

    write_changeset(dir.path(), "fix", "---\ndefault: patch\n---\n\n#### Fix\n");
    write_changeset(dir.path(), "docs", "---\ndefault: none\n---\n\n#### Docs\n");
    git_add_commit(dir.path(), "add changesets");
    release_commit(dir.path());

    let changelog = read(&dir.path().join("CHANGELOG.md"));
    assert!(!changelog.contains("Unreleased"), "{changelog}");
    assert_eq!(changelog.matches("### Internal").count(), 1, "{changelog}");
    let notes = release_notes("1.0.1", &changelog);
    assert!(notes.starts_with("#### Fix\n\n### Internal\n\n"), "{notes}");
    assert!(notes.contains("#### CI tweak") && notes.contains("#### Docs"));
    assert_clean(dir.path());
}

#[test]
fn none_only_per_package_writes_each_named_package() {
    let dir = setup_polyglot_repo();
    use_per_package(dir.path(), "");
    write_changeset(
        dir.path(),
        "ui",
        "---\nfrontend: none\n---\n\n#### UI lint\n",
    );
    git_add_commit(dir.path(), "add changeset");

    release_commit(dir.path());

    assert_eq!(
        read(&dir.path().join("web/CHANGELOG.md")),
        "# Changelog\n\n## Unreleased\n\n### Internal\n\n#### UI lint\n"
    );
    assert!(!dir.path().join("CHANGELOG.md").exists());
    assert_clean(dir.path());
}

#[test]
fn repo_wide_none_per_package_goes_to_every_package() {
    let dir = setup_polyglot_repo();
    use_per_package(dir.path(), "");
    write_changeset(dir.path(), "docs", "---\ndefault: none\n---\n\n#### Docs\n");
    git_add_commit(dir.path(), "add changeset");

    release_commit(dir.path());

    for path in ["CHANGELOG.md", "web/CHANGELOG.md"] {
        let changelog = read(&dir.path().join(path));
        assert!(
            changelog.contains("## Unreleased\n\n### Internal\n\n#### Docs\n"),
            "{path}: {changelog}"
        );
    }
    assert_clean(dir.path());
}

#[test]
fn per_package_release_with_none_for_an_unreleased_package() {
    let dir = setup_polyglot_repo();
    use_per_package(dir.path(), "");
    write_changeset(dir.path(), "api", "---\nbackend: minor\n---\n\n#### API\n");
    write_changeset(
        dir.path(),
        "ui",
        "---\nfrontend: none\n---\n\n#### UI lint\n",
    );
    git_add_commit(dir.path(), "add changesets");

    release_commit(dir.path());

    let backend = read(&dir.path().join("CHANGELOG.md"));
    assert!(backend.contains("## 1.1.0 - "), "{backend}");
    assert!(!backend.contains("UI lint") && !backend.contains("Internal"));
    assert_eq!(
        read(&dir.path().join("web/CHANGELOG.md")),
        "# Changelog\n\n## Unreleased\n\n### Internal\n\n#### UI lint\n"
    );
    assert_clean(dir.path());

    // frontend's next release picks the entry up.
    write_changeset(
        dir.path(),
        "fix",
        "---\nfrontend: patch\n---\n\n#### UI fix\n",
    );
    git_add_commit(dir.path(), "add changeset");
    release_commit(dir.path());

    let frontend = read(&dir.path().join("web/CHANGELOG.md"));
    assert!(!frontend.contains("Unreleased"), "{frontend}");
    assert_eq!(
        release_notes("2.0.1", &frontend),
        "#### UI fix\n\n### Internal\n\n#### UI lint\n"
    );
    assert_clean(dir.path());
}

#[test]
fn omit_writes_no_unreleased_section() {
    let dir = setup_polyglot_repo();
    use_per_package(dir.path(), "none_bump = \"omit\"\n");
    write_changeset(
        dir.path(),
        "ui",
        "---\nfrontend: none\n---\n\n#### UI lint\n",
    );
    write_changeset(dir.path(), "docs", "---\ndefault: none\n---\n\n#### Docs\n");
    git_add_commit(dir.path(), "add changesets");

    release_commit(dir.path());

    assert!(!dir.path().join("CHANGELOG.md").exists());
    assert!(!dir.path().join("web/CHANGELOG.md").exists());
    assert_eq!(count_changeset_files(dir.path()), 0);
    assert_clean(dir.path());

    let dir = setup_cargo_repo();
    std::fs::write(
        dir.path().join("changesetter.toml"),
        "[changelog]\nnone_bump = \"omit\"\n",
    )
    .unwrap();
    write_changeset(dir.path(), "ci", "---\ndefault: none\n---\n\n#### CI\n");
    git_add_commit(dir.path(), "add changeset");
    release_commit(dir.path());
    assert!(!dir.path().join("CHANGELOG.md").exists());
    assert_clean(dir.path());
}

fn git_out(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn none_only_release_commits_notes_without_tags() {
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "ci",
        "---\ndefault: none\n---\n\n#### CI tweak\n",
    );
    git_add_commit(dir.path(), "add changeset");

    release_commit(dir.path());

    assert_eq!(
        git_out(dir.path(), &["log", "-1", "--format=%s"]).trim(),
        "chore: add unreleased changelog notes"
    );
    assert_eq!(git_out(dir.path(), &["tag", "-l"]), "");
}

#[test]
fn absorbed_notes_reach_the_tag_message_and_json_once() {
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "ci",
        "---\ndefault: none\n---\n\n#### CI tweak\n",
    );
    git_add_commit(dir.path(), "add changeset");
    release_commit(dir.path());

    write_changeset(dir.path(), "fix", "---\ndefault: patch\n---\n\n#### Fix\n");
    git_add_commit(dir.path(), "add changeset");
    // The binary, as the action runs it, since the JSON goes to stdout.
    let out = Command::new(env!("CARGO_BIN_EXE_changesetter"))
        .args(["release", "--output", "json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let expected = "#### Fix\n\n### Internal\n\n#### CI tweak";
    assert_eq!(json["releases"][0]["changelog"], expected);

    let message = git_out(dir.path(), &["tag", "-l", "--format=%(contents)", "v1.0.1"]);
    assert_eq!(message.trim_end(), expected);
    assert_eq!(message.matches("CI tweak").count(), 1);
    let notes = release_notes("1.0.1", &read(&dir.path().join("CHANGELOG.md")));
    assert_eq!(notes.trim_end(), expected);
}

#[test]
fn nameless_workspace_root_release_has_no_stray_separator() {
    let dir = setup_repo();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    write_changeset(
        dir.path(),
        "feat",
        "---\ndefault: minor\n---\n\n#### Feat\n",
    );
    git_add_commit(dir.path(), "workspace");

    let status = Command::new(env!("CARGO_BIN_EXE_changesetter"))
        .arg("status")
        .current_dir(dir.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&status.stdout);
    assert!(text.contains("\n  0.0.0 -> 0.1.0 (minor)"), "{text}");

    release_commit(dir.path());
    assert_eq!(
        git_out(dir.path(), &["log", "-1", "--format=%s"]).trim(),
        "chore: release 0.1.0"
    );
}

#[test]
fn private_workspace_root_gets_no_changelog() {
    let dir = setup_repo();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();
    for name in ["a", "b"] {
        let pkg = dir.path().join("packages").join(name);
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
        )
        .unwrap();
    }
    git_add_commit(dir.path(), "workspace");
    use_per_package(dir.path(), "");
    write_changeset(dir.path(), "docs", "---\ndefault: none\n---\n\n#### Docs\n");
    git_add_commit(dir.path(), "add changeset");

    release_commit(dir.path());

    assert!(!dir.path().join("CHANGELOG.md").exists());
    for name in ["a", "b"] {
        let changelog = read(&dir.path().join("packages").join(name).join("CHANGELOG.md"));
        assert!(changelog.contains("#### Docs"), "{changelog}");
    }
    assert_clean(dir.path());
}

fn version_no_commit(repo: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_changesetter"))
        .args(["version", "--no-commit"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn headings_in_a_none_body_do_not_split_the_internal_section() {
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "one",
        "---\ndefault: none\n---\n\n#### CI one\n\nDetails:\n\n### Sub heading\n\nmore one\n\n## Big heading in body\n\ntail one\n",
    );
    version_no_commit(dir.path());
    write_changeset(
        dir.path(),
        "two",
        "---\ndefault: none\n---\n\n#### CI two\n",
    );
    version_no_commit(dir.path());

    assert_eq!(
        read(&dir.path().join("CHANGELOG.md")),
        "# Changelog\n\n## Unreleased\n\n### Internal\n\n#### CI one\n\nDetails:\n\n##### Sub heading\n\nmore one\n\n##### Big heading in body\n\ntail one\n\n#### CI two\n"
    );
}

#[test]
fn headings_in_a_release_body_keep_the_notes_whole() {
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "fix",
        "---\ndefault: patch\n---\n\n#### Fix\n\n## Migration\n\nDo X.\n\n```md\n# a comment stays as written\n```\n\nEnd.\n",
    );
    git_add_commit(dir.path(), "add changeset");
    release_commit(dir.path());

    let notes = release_notes("1.0.1", &read(&dir.path().join("CHANGELOG.md")));
    assert_eq!(
        notes,
        "#### Fix\n\n##### Migration\n\nDo X.\n\n```md\n# a comment stays as written\n```\n\nEnd.\n"
    );
}

#[test]
fn a_section_heading_inside_a_code_block_is_warned_about() {
    // Unsupported: code is kept as written and the parsers don't track
    // fences, so this line ends the entry. `status` and `add` say so.
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "fix",
        "---\ndefault: patch\n---\n\n#### Fix\n\n```md\n## 9.9.9 looks like a version\n```\n",
    );
    let out = Command::new(env!("CARGO_BIN_EXE_changesetter"))
        .arg("status")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("warning: fix.md: \"## 9.9.9 looks like a version\" in a code block"),
        "{stderr}"
    );

    let out = Command::new(env!("CARGO_BIN_EXE_changesetter"))
        .args([
            "add",
            "--package",
            "testpkg",
            "--bump",
            "patch",
            "--message",
        ])
        .arg("Fix\n\n```\n### Internal\n```")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("\"### Internal\" in a code block"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn tag_message(repo: &Path, tag: &str) -> String {
    git_out(repo, &["tag", "-l", "--format=%(contents)", tag])
        .trim_end()
        .to_string()
}

/// Notes for `version` from release-notes.sh, which must match the tag
/// message the Rust side wrote for it.
fn notes_agree(repo: &Path, version: &str) -> String {
    let notes = release_notes(version, &read(&repo.join("CHANGELOG.md")));
    assert_eq!(
        notes.trim_end(),
        tag_message(repo, &format!("v{version}")),
        "release-notes.sh and the tag disagree on {version}"
    );
    notes
}

#[test]
fn an_unclosed_fence_in_a_release_body_stays_in_its_section() {
    let dir = setup_cargo_repo();
    for (name, body) in [
        ("a", "---\ndefault: minor\n---\n\n#### First feature\n"),
        (
            "b",
            "---\ndefault: patch\n---\n\nFix with sample\n\n```sh\necho unclosed\n",
        ),
        ("c", "---\ndefault: patch\n---\n\n#### Another fix\n"),
    ] {
        write_changeset(dir.path(), name, body);
        git_add_commit(dir.path(), name);
        release_commit(dir.path());
    }

    assert_eq!(notes_agree(dir.path(), "1.1.2"), "#### Another fix\n\n");
    assert_eq!(
        notes_agree(dir.path(), "1.1.1"),
        "Fix with sample\n\n```sh\necho unclosed\n```\n\n"
    );
    assert_eq!(notes_agree(dir.path(), "1.1.0"), "#### First feature\n");
}

#[test]
fn an_unclosed_fence_in_a_none_body_does_not_swallow_the_file() {
    let dir = setup_cargo_repo();
    for (name, body) in [
        ("a", "---\ndefault: minor\n---\n\n#### Feature one\n"),
        (
            "b",
            "---\ndefault: none\n---\n\nCI note\n\n```yaml\nkey: v\n",
        ),
        ("c", "---\ndefault: none\n---\n\n#### Second CI note\n"),
        ("d", "---\ndefault: minor\n---\n\n#### Feature two\n"),
    ] {
        write_changeset(dir.path(), name, body);
        git_add_commit(dir.path(), name);
        release_commit(dir.path());
    }

    let changelog = read(&dir.path().join("CHANGELOG.md"));
    assert!(!changelog.contains("Unreleased"), "{changelog}");
    assert_eq!(
        notes_agree(dir.path(), "1.2.0"),
        "#### Feature two\n\n### Internal\n\nCI note\n\n```yaml\nkey: v\n```\n\n#### Second CI note\n\n"
    );
    assert_eq!(notes_agree(dir.path(), "1.1.0"), "#### Feature one\n");
}

#[test]
fn a_legacy_unclosed_fence_is_not_closed_by_a_later_one() {
    // An old Unreleased note opened a fence and never closed it, and the
    // version below has a fence of its own. Lines are read without fence
    // state, so 1.0.0 stays a version and isn't folded into the release.
    let dir = setup_cargo_repo();
    std::fs::write(
        dir.path().join("CHANGELOG.md"),
        "# Changelog\n\n## Unreleased\n\n### Internal\n\n```sh\nold unclosed\n\n## 1.0.0 - d\n\n```\ncode\n```\n\n- A\n",
    )
    .unwrap();
    write_changeset(
        dir.path(),
        "fix",
        "---\ndefault: patch\n---\n\n#### Fix\u{a0}\n\nMore.\u{a0}\u{a0}\n\nDone.\n",
    );
    git_add_commit(dir.path(), "legacy");
    release_commit(dir.path());

    // The U+00A0s stay in the tag as in the file: Rust trims only what awk does.
    assert_eq!(
        notes_agree(dir.path(), "1.0.1"),
        "#### Fix\u{a0}\n\nMore.\u{a0}\u{a0}\n\nDone.\n\n### Internal\n\n```sh\nold unclosed\n\n"
    );
    let changelog = read(&dir.path().join("CHANGELOG.md"));
    assert_eq!(
        release_notes("1.0.0", &changelog),
        "```\ncode\n```\n\n- A\n"
    );
}

#[test]
fn a_version_link_reference_in_a_body_keeps_the_notes_whole() {
    let body = "Fix, see [1.0.0].\n\n[1.0.0]: https://x/1.0.0\n\nTail text\n";
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "fix",
        &format!("---\ndefault: patch\n---\n\n{body}"),
    );
    git_add_commit(dir.path(), "fix");
    release_commit(dir.path());
    let notes = notes_agree(dir.path(), "1.0.1");
    assert_eq!(
        notes,
        "Fix, see [1.0.0].\n\n [1.0.0]: https://x/1.0.0\n\nTail text\n"
    );

    // As a none entry, absorbed by the next release.
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "note",
        &format!("---\ndefault: none\n---\n\n{body}"),
    );
    git_add_commit(dir.path(), "note");
    release_commit(dir.path());
    write_changeset(dir.path(), "fix", "---\ndefault: patch\n---\n\n#### Fix\n");
    git_add_commit(dir.path(), "fix");
    release_commit(dir.path());
    let notes = notes_agree(dir.path(), "1.0.1");
    assert!(notes.contains("\n [1.0.0]: https://x/1.0.0\n"), "{notes}");
    assert!(notes.contains("Tail text"), "{notes}");
}

#[test]
fn a_none_body_keeps_its_leading_indent_when_absorbed() {
    for (body, kept) in [
        (
            "[1.0.0]: https://x/1.0.0\n\nTail text\n",
            "### Internal\n\n [1.0.0]: https://x/1.0.0\n\nTail text",
        ),
        (
            "    let x = 1;\n\nTail text\n",
            "### Internal\n\n    let x = 1;\n\nTail text",
        ),
    ] {
        let dir = setup_cargo_repo();
        write_changeset(
            dir.path(),
            "note",
            &format!("---\ndefault: none\n---\n\n{body}"),
        );
        git_add_commit(dir.path(), "note");
        release_commit(dir.path());
        write_changeset(dir.path(), "fix", "---\ndefault: patch\n---\n\n#### Fix\n");
        git_add_commit(dir.path(), "fix");
        release_commit(dir.path());

        let notes = notes_agree(dir.path(), "1.0.1");
        assert_eq!(notes.trim_end(), format!("#### Fix\n\n{kept}"), "{body:?}");
        let changelog = read(&dir.path().join("CHANGELOG.md"));
        assert!(!changelog.contains("Unreleased"), "{changelog}");
    }
}

#[test]
fn a_release_body_starting_with_a_link_reference_keeps_its_indent() {
    let dir = setup_cargo_repo();
    write_changeset(
        dir.path(),
        "fix",
        "---\ndefault: patch\n---\n\n[1.0.0]: https://x/1.0.0\n\nTail text\n",
    );
    git_add_commit(dir.path(), "fix");
    release_commit(dir.path());
    // The tag keeps the leading space too.
    let notes = notes_agree(dir.path(), "1.0.1");
    assert_eq!(notes, " [1.0.0]: https://x/1.0.0\n\nTail text\n");
}

#[test]
fn a_legacy_unclosed_fence_reads_as_text() {
    // Written before bodies got their fences closed.
    let dir = setup_cargo_repo();
    std::fs::write(
        dir.path().join("CHANGELOG.md"),
        "# Changelog\n\n## Unreleased\n\n### Internal\n\n- old note\n\n## 1.0.0 - d\n\nSample\n\n```sh\necho unclosed\n\n## 0.9.0 - d\n\n- Older\n",
    )
    .unwrap();
    write_changeset(
        dir.path(),
        "a",
        "---\ndefault: none\n---\n\n#### New note\n",
    );
    git_add_commit(dir.path(), "legacy");
    release_commit(dir.path());
    write_changeset(dir.path(), "b", "---\ndefault: patch\n---\n\n#### Fix\n");
    git_add_commit(dir.path(), "fix");
    release_commit(dir.path());

    let changelog = read(&dir.path().join("CHANGELOG.md"));
    assert_eq!(
        release_notes("1.0.1", &changelog),
        "#### Fix\n\n### Internal\n\n- old note\n\n#### New note\n\n"
    );
    assert_eq!(
        notes_agree(dir.path(), "1.0.1"),
        release_notes("1.0.1", &changelog)
    );
    assert_eq!(
        release_notes("1.0.0", &changelog),
        "Sample\n\n```sh\necho unclosed\n\n"
    );
    assert_eq!(release_notes("0.9.0", &changelog), "- Older\n");
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
            output: None,
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

    let result = changesetter::cli::check::run_in(
        dir.path(),
        &CheckArgs {
            base: None,
            output: None,
        },
    );
    assert!(result.is_err());
}

#[test]
fn check_fails_no_changeset_dir() {
    let dir = setup_cargo_repo();
    let result = changesetter::cli::check::run_in(
        dir.path(),
        &CheckArgs {
            base: None,
            output: None,
        },
    );
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

// ========== v0.2 integration tests ==========

// ---------- Python adapter round-trip ----------

#[test]
fn roundtrip_python() {
    let dir = setup_repo();
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"mypy\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    git_add_commit(dir.path(), "add pyproject.toml");

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("test.md"),
        "---\nmypy: patch\n---\n\n#### Fix\n",
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

    let v = PythonAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 0, 1));
    assert_eq!(count_changeset_files(dir.path()), 0);
}

// ---------- .NET adapter round-trip ----------

#[test]
fn roundtrip_dotnet() {
    let dir = setup_repo();
    std::fs::write(
        dir.path().join("MyLib.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <Version>1.0.0</Version>
    <AssemblyName>MyLib</AssemblyName>
  </PropertyGroup>
</Project>"#,
    )
    .unwrap();
    git_add_commit(dir.path(), "add csproj");

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("test.md"),
        "---\nMyLib: minor\n---\n\n#### Feature\n",
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

    let v = DotnetAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 1, 0));
    assert_eq!(count_changeset_files(dir.path()), 0);
}

// ---------- Helm adapter round-trip ----------

#[test]
fn roundtrip_helm() {
    let dir = setup_repo();
    std::fs::write(
        dir.path().join("Chart.yaml"),
        "apiVersion: v2\nname: mychart\nversion: 1.0.0\nappVersion: \"1.0\"\n",
    )
    .unwrap();
    git_add_commit(dir.path(), "add Chart.yaml");

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("test.md"),
        "---\nmychart: patch\n---\n\n#### Fix\n",
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

    let v = HelmAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v, Version::new(1, 0, 1));

    let content = std::fs::read_to_string(dir.path().join("Chart.yaml")).unwrap();
    assert!(
        content.contains("appVersion: \"1.0\""),
        "appVersion should not be touched"
    );
    assert_eq!(count_changeset_files(dir.path()), 0);
}

// ---------- Fixed group release ----------

#[test]
fn fixed_group_bumps_all_members() {
    let dir = setup_repo();

    std::fs::create_dir_all(dir.path().join("crates/lib-a")).unwrap();
    std::fs::write(
        dir.path().join("crates/lib-a/Cargo.toml"),
        "[package]\nname = \"lib-a\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("crates/lib-b")).unwrap();
    std::fs::write(
        dir.path().join("crates/lib-b/Cargo.toml"),
        "[package]\nname = \"lib-b\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    std::fs::write(
        dir.path().join("changesetter.toml"),
        "[groups.core]\nfixed = [\"lib-a\", \"lib-b\"]\n",
    )
    .unwrap();

    git_add_commit(dir.path(), "add packages and config");

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("test.md"),
        "---\nlib-a: minor\n---\n\n#### Feature in lib-a\n",
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

    let v_a = CargoAdapter
        .read_version(&dir.path().join("crates/lib-a"))
        .unwrap();
    let v_b = CargoAdapter
        .read_version(&dir.path().join("crates/lib-b"))
        .unwrap();
    assert_eq!(v_a, Version::new(1, 1, 0));
    assert_eq!(
        v_b,
        Version::new(1, 1, 0),
        "lib-b should bump with fixed group"
    );
}

// ---------- Linked group release ----------

#[test]
fn linked_group_only_changed_bumps() {
    let dir = setup_repo();

    std::fs::create_dir_all(dir.path().join("crates/util-a")).unwrap();
    std::fs::write(
        dir.path().join("crates/util-a/Cargo.toml"),
        "[package]\nname = \"util-a\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("crates/util-b")).unwrap();
    std::fs::write(
        dir.path().join("crates/util-b/Cargo.toml"),
        "[package]\nname = \"util-b\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    std::fs::write(
        dir.path().join("changesetter.toml"),
        "[groups.utils]\nlinked = [\"util-a\", \"util-b\"]\n",
    )
    .unwrap();

    git_add_commit(dir.path(), "add packages and config");

    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("test.md"),
        "---\nutil-a: patch\n---\n\n#### Fix in util-a\n",
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

    let v_a = CargoAdapter
        .read_version(&dir.path().join("crates/util-a"))
        .unwrap();
    let v_b = CargoAdapter
        .read_version(&dir.path().join("crates/util-b"))
        .unwrap();
    assert_eq!(v_a, Version::new(1, 0, 1));
    assert_eq!(
        v_b,
        Version::new(1, 0, 0),
        "util-b should NOT bump in linked group"
    );
}

// ---------- Pre-release cycle ----------

#[test]
fn pre_release_cycle() {
    let dir = setup_cargo_repo();

    // Enter pre-release mode
    changesetter::cli::pre::run_in(
        dir.path(),
        &PreArgs {
            command: PreCommand::Enter {
                tag: "rc".to_string(),
            },
        },
    )
    .unwrap();

    let pre_json = dir.path().join(".changeset/pre.json");
    assert!(pre_json.exists());

    // First pre-release
    let changeset_dir = dir.path().join(".changeset");
    std::fs::write(
        changeset_dir.join("feat1.md"),
        "---\ntestpkg: minor\n---\n\n#### Feature one\n",
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
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 1);
    assert_eq!(v.patch, 0);
    assert!(
        v.pre.as_ref().unwrap().starts_with("rc."),
        "expected rc.N pre-release suffix, got: {}",
        v
    );

    // Verify counter incremented in pre.json
    let state = pre::read_pre_state(&changeset_dir).unwrap();
    assert_eq!(state.mode, "pre");
    assert!(
        state.packages_released.get("testpkg").copied().unwrap_or(0) >= 1,
        "counter should be incremented"
    );

    // Second pre-release: write version back to 1.0.0 to simulate fresh state,
    // then release again with a new changeset
    CargoAdapter
        .write_version(dir.path(), &Version::new(1, 0, 0))
        .unwrap();
    std::fs::write(
        changeset_dir.join("feat2.md"),
        "---\ntestpkg: minor\n---\n\n#### Feature two\n",
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

    let v2 = CargoAdapter.read_version(dir.path()).unwrap();
    let pre_str = v2.pre.as_ref().unwrap();
    assert!(
        pre_str.starts_with("rc."),
        "second release should still have rc prefix, got: {v2}"
    );
    let counter: u64 = pre_str.strip_prefix("rc.").unwrap().parse().unwrap();
    assert!(
        counter >= 1,
        "counter should have incremented, got: {counter}"
    );

    // Exit pre-release mode
    changesetter::cli::pre::run_in(
        dir.path(),
        &PreArgs {
            command: PreCommand::Exit,
        },
    )
    .unwrap();

    // Reset version and add another changeset for stable release
    CargoAdapter
        .write_version(dir.path(), &Version::new(1, 0, 0))
        .unwrap();
    std::fs::write(
        changeset_dir.join("feat3.md"),
        "---\ntestpkg: minor\n---\n\n#### Feature three\n",
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

    let v3 = CargoAdapter.read_version(dir.path()).unwrap();
    assert_eq!(v3.pre, None, "stable release should have no pre suffix");
    assert_eq!(v3, Version::new(1, 1, 0));

    // pre.json should be removed after exit-mode release
    assert!(
        !pre_json.exists(),
        "pre.json should be removed after stable release"
    );
}

// ---------- Dependency cascading ----------

#[test]
fn dependency_cascade_bumps_dependent() {
    let dir = setup_repo();

    // Package A
    std::fs::create_dir_all(dir.path().join("crates/a")).unwrap();
    std::fs::write(
        dir.path().join("crates/a/Cargo.toml"),
        "[package]\nname = \"a\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    // Package B depends on A
    std::fs::create_dir_all(dir.path().join("crates/b")).unwrap();
    std::fs::write(
        dir.path().join("crates/b/Cargo.toml"),
        "[package]\nname = \"b\"\nversion = \"1.0.0\"\nedition = \"2024\"\n\n[dependencies]\na = { path = \"../a\" }\n",
    )
    .unwrap();

    // Config enabling cascading
    std::fs::write(
        dir.path().join("changesetter.toml"),
        "update_internal_dependencies = \"patch\"\n",
    )
    .unwrap();

    git_add_commit(dir.path(), "add packages with dependency");

    // Only changeset for A
    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("test.md"),
        "---\na: minor\n---\n\n#### Feature in A\n",
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

    let v_a = CargoAdapter
        .read_version(&dir.path().join("crates/a"))
        .unwrap();
    let v_b = CargoAdapter
        .read_version(&dir.path().join("crates/b"))
        .unwrap();

    assert_eq!(v_a, Version::new(1, 1, 0));
    assert_eq!(
        v_b,
        Version::new(1, 0, 1),
        "b should get a cascaded patch bump"
    );
}

fn release_notes(version: &str, changelog: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("CHANGELOG.md"), changelog).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("actions/release/release-notes.sh");
    // Run through its shebang, as the action does.
    let out = Command::new(script)
        .arg(version)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn release_notes_keep_the_whole_body_of_the_only_section() {
    let changelog = "# Changelog\n\n## 0.1.0 - 2026-10-16\n\n#### First release\n\nA paragraph.\n";
    assert_eq!(
        release_notes("0.1.0", changelog),
        "#### First release\n\nA paragraph.\n"
    );
}

#[test]
fn release_notes_stop_at_the_next_version() {
    let changelog =
        "# Changelog\n\n## 0.2.0 - 2026-10-17\n\n- New\n\n## 0.1.0 - 2026-10-16\n\n- Old\n";
    assert_eq!(release_notes("0.2.0", changelog), "- New\n\n");
    assert_eq!(release_notes("0.1.0", changelog), "- Old\n");
}

#[test]
fn release_notes_match_the_version_exactly() {
    let changelog = "## 1.0.10 - 2026-10-17\n\n- Ten\n\n## 1.0.1 - 2026-10-16\n\n- One\n";
    assert_eq!(release_notes("1.0.1", changelog), "- One\n");
    assert_eq!(
        release_notes("1.0.1", "## 1x0x1 - 2026-10-16\n\n- No\n"),
        ""
    );
}

#[test]
fn release_notes_find_a_shared_monorepo_heading() {
    let changelog =
        "## 1.0.0, 2.0.0 - 2026-10-17\n\n### a\n\n- x\n\n## 0.9.0 - 2026-10-16\n\n- Old\n";
    assert_eq!(release_notes("1.0.0", changelog), "### a\n\n- x\n\n");
}

#[test]
fn release_notes_read_crlf_changelogs() {
    let changelog = "## 0.2.0\r\n\r\n- New\r\n";
    assert_eq!(release_notes("0.2.0", changelog), "- New\n");
    assert_eq!(release_notes("0.2.0", "## 0.2.0 \t\n\n- New\n"), "- New\n");
    assert_eq!(
        release_notes("2.0.0-rc.1", "## 1.0.0, 2.0.0-rc.1\n\n- Rc\n"),
        "- Rc\n"
    );
}

#[test]
fn release_notes_keep_headings_from_changeset_bodies() {
    let changelog = "## 0.2.0 - d\n\nIntro.\n\n## Migration\n\nDo X.\n\n## 0.1.0 - d\n\n- Old\n";
    assert_eq!(
        release_notes("0.2.0", changelog),
        "Intro.\n\n## Migration\n\nDo X.\n\n"
    );
}

#[test]
fn release_notes_find_any_version_in_a_shared_heading() {
    let changelog =
        "## 1.0.0, 2.0.0 - 2026-10-17\n\n### b\n\n- y\n\n## 0.9.0 - 2026-10-16\n\n- Old\n";
    assert_eq!(release_notes("2.0.0", changelog), "### b\n\n- y\n\n");
    assert_eq!(release_notes("0.0", changelog), "");
}

#[test]
fn release_notes_keep_body_headings_that_start_with_a_digit() {
    let changelog = "## 0.2.0 - d\n\nIntro.\n\n## 2FA support\n\nOn.\n\n## 0.1.0 - d\n\n- Old\n";
    assert_eq!(
        release_notes("0.2.0", changelog),
        "Intro.\n\n## 2FA support\n\nOn.\n\n"
    );
}

#[test]
fn release_notes_take_only_the_first_of_duplicate_sections() {
    let changelog = "## 0.2.0 - d\n\n- A\n\n## 0.2.0 - d\n\n- B\n";
    assert_eq!(release_notes("0.2.0", changelog), "- A\n\n");
}

#[test]
fn release_notes_are_empty_without_a_matching_section() {
    assert_eq!(release_notes("0.3.0", "## 0.2.0 - d\n\n- A\n"), "");

    let dir = tempfile::tempdir().unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("actions/release/release-notes.sh");
    let out = Command::new(script)
        .arg("0.1.0")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn release_notes_never_treat_unreleased_as_a_version() {
    let changelog =
        "# Changelog\n\n## Unreleased\n\n### Internal\n\n- CI\n\n## 1.0.0 - d\n\n- First\n";
    assert_eq!(release_notes("1.0.0", changelog), "- First\n");
    assert_eq!(release_notes("Unreleased", changelog), "");

    // A stray Unreleased section below a version still ends that version's notes.
    let changelog = "## 1.1.0 - d\n\n- New\n\n## [Unreleased]\n\n- Later\n";
    assert_eq!(release_notes("1.1.0", changelog), "- New\n\n");
}

#[test]
fn release_notes_end_at_other_tools_headings_and_links() {
    for next in [
        "## [1.0.0] - 2020-01-01",
        "## v1.0.0",
        "## [v1.0.0] - 2020-01-01",
        "## [V1.0.0]",
        "## [Unreleased]",
        "## Unreleased",
        "[1.1.0]: https://example.com/compare/v1.0.0...v1.1.0",
        "[Unreleased]: https://example.com/compare/v1.1.0...HEAD",
    ] {
        let changelog = format!("## 1.1.0 - d\n\n- New\n\n{next}\n\n- Old\n");
        assert_eq!(release_notes("1.1.0", &changelog), "- New\n\n", "{next}");
    }
    // Reference links in a changeset body are part of the notes.
    let changelog = "## 1.1.0 - d\n\nSee [docs].\n\n[docs]: https://example.com\n";
    assert_eq!(
        release_notes("1.1.0", changelog),
        "See [docs].\n\n[docs]: https://example.com\n"
    );
}

#[test]
fn release_notes_read_lines_without_fence_state() {
    // Same reading as before fences mattered anywhere: a version heading ends
    // the section wherever it is, so an old changelog with an unclosed fence
    // still splits at its versions.
    let changelog = "## 0.2.0 - d\n\n```sh\nunclosed\n\n## 0.1.0 - d\n\n```\nx\n```\n";
    assert_eq!(release_notes("0.2.0", changelog), "```sh\nunclosed\n\n");
    assert_eq!(release_notes("0.1.0", changelog), "```\nx\n```\n");

    // 5,000 versions are one pass; the bound is loose for slow CI machines.
    let mut long = String::default();
    for v in (0..5000).rev() {
        long.push_str(&format!(
            "## 1.0.{v} - d\n\n- note {v}\n\n```sh\nunclosed\n\n"
        ));
    }
    let started = std::time::Instant::now();
    assert_eq!(
        release_notes("1.0.0", &long),
        "- note 0\n\n```sh\nunclosed\n\n"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn rust_and_release_notes_agree_on_section_ends() {
    for (line, ends) in [
        ("## Unreleased", true),
        ("## [Unreleased]", true),
        ("## [Unreleased", false),
        ("## Unreleased]", false),
        ("## [1.0.0] - d", true),
        ("## [v1.0.0]", true),
        ("## v1.0.0", true),
        ("[1.0.0]: https://x", true),
        ("[v1.0.0]: https://x", true),
        ("[Unreleased]: https://x", true),
        ("[1.0.0-rc.1]: https://x", true),
        ("[1.0.0] x]: https://x", false),
        ("[[1.0.0]: https://x", false),
        ("[docs]: https://x", false),
        (" [1.0.0]: https://x", false),
        ("[1.0.0]:https://x", false),
    ] {
        let changelog = format!("## 0.2.0 - d\n\n- a\n\n{line}\n\n- b\n");
        let awk_ends = release_notes("0.2.0", &changelog) == "- a\n\n";
        assert_eq!(awk_ends, ends, "release-notes.sh on {line:?}");
        assert_eq!(
            changesetter::changelog::generator::is_section_end(line),
            ends,
            "Rust on {line:?}"
        );
    }
}
