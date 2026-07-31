use changesetter::changeset::reader::read_changesets;
use changesetter::changeset::types::BumpLevel;

fn copy_fixture_dir(fixture_name: &str, dest: &std::path::Path) {
    let src = std::path::PathBuf::from(format!(
        "tests/fixtures/changesets-compat/read/{fixture_name}"
    ));
    for entry in std::fs::read_dir(&src).unwrap() {
        let entry = entry.unwrap();
        let dest_path = dest.join(entry.file_name());
        std::fs::copy(entry.path(), dest_path).unwrap();
    }
}

#[test]
fn compat_read_filters_non_changeset_files() {
    let dir = tempfile::tempdir().unwrap();
    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir(&changeset_dir).unwrap();
    copy_fixture_dir("mixed-files", &changeset_dir);

    let result = read_changesets(&changeset_dir).unwrap();
    assert_eq!(
        result.len(),
        2,
        "should only read the two .md changeset files"
    );
}

#[test]
fn compat_read_parses_content_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir(&changeset_dir).unwrap();
    copy_fixture_dir("mixed-files", &changeset_dir);

    let result = read_changesets(&changeset_dir).unwrap();

    let brave = result
        .iter()
        .find(|c| c.filename.as_deref() == Some("brave-foxes-leap"))
        .unwrap();
    assert_eq!(brave.packages.len(), 1);
    assert_eq!(brave.packages["mylib"], BumpLevel::Patch);
    assert!(brave.body.contains("Fixed null handling"));

    let cool = result
        .iter()
        .find(|c| c.filename.as_deref() == Some("cool-dogs-dance"))
        .unwrap();
    assert_eq!(cool.packages.len(), 2);
    assert_eq!(cool.packages["mylib"], BumpLevel::Minor);
    assert_eq!(cool.packages["my-api"], BumpLevel::Patch);
    assert!(cool.body.contains("batch processing"));
}

#[test]
fn compat_read_deterministic_order() {
    let dir = tempfile::tempdir().unwrap();
    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir(&changeset_dir).unwrap();
    copy_fixture_dir("mixed-files", &changeset_dir);

    let result = read_changesets(&changeset_dir).unwrap();
    assert_eq!(result[0].filename.as_deref(), Some("brave-foxes-leap"));
    assert_eq!(result[1].filename.as_deref(), Some("cool-dogs-dance"));
}

#[test]
fn compat_read_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir(&changeset_dir).unwrap();

    let result = read_changesets(&changeset_dir).unwrap();
    assert!(result.is_empty());
}

#[test]
fn compat_read_missing_directory() {
    let dir = tempfile::tempdir().unwrap();
    let changeset_dir = dir.path().join(".changeset");

    let result = read_changesets(&changeset_dir).unwrap();
    assert!(result.is_empty());
}

#[test]
fn compat_read_malformed_frontmatter_errors() {
    let dir = tempfile::tempdir().unwrap();
    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join("bad-changeset.md"),
        "this has no frontmatter at all\n",
    )
    .unwrap();

    let result = read_changesets(&changeset_dir);
    assert!(result.is_err());
}

#[test]
fn compat_read_only_ignored_files_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let changeset_dir = dir.path().join(".changeset");
    std::fs::create_dir(&changeset_dir).unwrap();
    std::fs::write(changeset_dir.join("config.json"), "{}").unwrap();
    std::fs::write(changeset_dir.join("pre.json"), "{}").unwrap();
    std::fs::write(changeset_dir.join("README.md"), "# Changesets\n").unwrap();
    std::fs::write(changeset_dir.join("notes.txt"), "notes").unwrap();

    let result = read_changesets(&changeset_dir).unwrap();
    assert!(result.is_empty());
}
