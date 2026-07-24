use changesetter::changeset::parser;
use changesetter::changeset::types::BumpLevel;

fn fixture(name: &str) -> String {
    let path = format!("tests/fixtures/changesets-compat/parse/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

#[test]
fn compat_parse_simple() {
    let cs = parser::parse(&fixture("simple.md"), Some("simple".into())).unwrap();
    assert_eq!(cs.packages.len(), 1);
    assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
    assert!(cs.body.contains("Fixed null handling"));
    assert_eq!(cs.filename.as_deref(), Some("simple"));
}

#[test]
fn compat_parse_multiple_packages() {
    let cs = parser::parse(&fixture("multiple.md"), None).unwrap();
    assert_eq!(cs.packages.len(), 2);
    assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
    assert_eq!(cs.packages["my-api"], BumpLevel::Minor);
    assert!(cs.body.contains("Fixed null handling"));
    assert!(cs.body.contains("deserializer"));
}

#[test]
fn compat_parse_scoped_npm_packages() {
    let cs = parser::parse(&fixture("scoped.md"), None).unwrap();
    assert_eq!(cs.packages.len(), 2);
    assert_eq!(cs.packages["@myorg/utils"], BumpLevel::Patch);
    assert_eq!(cs.packages["@myorg/core"], BumpLevel::Minor);
}

#[test]
fn compat_parse_windows_line_endings() {
    let cs = parser::parse(&fixture("windows.md"), None).unwrap();
    assert_eq!(cs.packages.len(), 1);
    assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
    assert_eq!(cs.body, "#### Fixed line endings");
}

#[test]
fn compat_parse_dashes_in_body() {
    let cs = parser::parse(&fixture("dashes-in-body.md"), None).unwrap();
    assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
    assert!(cs.body.contains("---"));
    assert!(cs.body.contains("Fixed parsing edge case"));
    assert!(cs.body.contains("Another paragraph"));
}

#[test]
fn compat_parse_empty_body() {
    let cs = parser::parse(&fixture("empty-body.md"), None).unwrap();
    assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
    assert_eq!(cs.body, "");
}

#[test]
fn compat_parse_none_bump() {
    let cs = parser::parse(&fixture("none-bump.md"), None).unwrap();
    assert_eq!(cs.packages["mylib"], BumpLevel::None);
    assert!(cs.body.contains("Updated CI configuration"));
}

#[test]
fn compat_parse_major() {
    let cs = parser::parse(&fixture("major.md"), None).unwrap();
    assert_eq!(cs.packages["mylib"], BumpLevel::Major);
    assert!(cs.body.contains("Breaking"));
}

#[test]
fn compat_parse_whitespace_body() {
    let cs = parser::parse(&fixture("whitespace-body.md"), None).unwrap();
    assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
    assert_eq!(cs.body, "");
}

#[test]
fn compat_parse_multiline_body() {
    let cs = parser::parse(&fixture("multiline.md"), None).unwrap();
    assert_eq!(cs.packages["mylib"], BumpLevel::Minor);
    assert!(cs.body.contains("Added batch processing"));
    assert!(cs.body.contains("round trips"));
    assert!(cs.body.contains("migration guide"));
}
