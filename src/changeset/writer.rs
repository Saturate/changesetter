use std::collections::BTreeMap;
use std::path::Path;

use crate::changeset::types::BumpLevel;
use crate::changeset::words;

pub fn write_changeset(
    changeset_dir: &Path,
    packages: &BTreeMap<String, BumpLevel>,
    body: &str,
) -> anyhow::Result<String> {
    let mut name = words::generate_name();
    let mut path = changeset_dir.join(format!("{name}.md"));

    let mut attempt = 2;
    while path.exists() {
        name = format!("{}-{attempt}", words::generate_name());
        path = changeset_dir.join(format!("{name}.md"));
        attempt += 1;
        if attempt > 100 {
            anyhow::bail!("failed to generate a unique changeset filename after 100 attempts");
        }
    }

    let mut content = String::from("---\n");
    for (pkg, bump) in packages {
        if pkg.contains('@') || pkg.contains('/') {
            content.push_str(&format!("\"{pkg}\": {bump}\n"));
        } else {
            content.push_str(&format!("{pkg}: {bump}\n"));
        }
    }
    content.push_str("---\n");

    if !body.is_empty() {
        content.push('\n');
        content.push_str(body);
        if !body.ends_with('\n') {
            content.push('\n');
        }
    }

    std::fs::write(&path, &content)?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changeset::parser;

    #[test]
    fn write_and_parse_roundtrip() {
        let dir = tempfile::tempdir().unwrap();

        let packages = BTreeMap::from([("mylib".to_string(), BumpLevel::Patch)]);
        let body = "#### Fixed a bug\n\nDetails here.";

        let name = write_changeset(dir.path(), &packages, body).unwrap();

        let path = dir.path().join(format!("{name}.md"));
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        let cs = parser::parse(&content, Some(name)).unwrap();
        assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
        assert!(cs.body.contains("Fixed a bug"));
    }

    #[test]
    fn write_none_bump() {
        let dir = tempfile::tempdir().unwrap();
        let packages = BTreeMap::from([("default".to_string(), BumpLevel::None)]);
        let body = "#### Docs update";

        let name = write_changeset(dir.path(), &packages, body).unwrap();
        let content = std::fs::read_to_string(dir.path().join(format!("{name}.md"))).unwrap();
        assert!(content.contains("default: none"));
    }

    #[test]
    fn write_scoped_package() {
        let dir = tempfile::tempdir().unwrap();
        let packages = BTreeMap::from([("@myorg/utils".to_string(), BumpLevel::Minor)]);
        let body = "";

        let name = write_changeset(dir.path(), &packages, body).unwrap();
        let content = std::fs::read_to_string(dir.path().join(format!("{name}.md"))).unwrap();
        assert!(content.contains("\"@myorg/utils\": minor"));
    }

    #[test]
    fn write_empty_body() {
        let dir = tempfile::tempdir().unwrap();
        let packages = BTreeMap::from([("mylib".to_string(), BumpLevel::Patch)]);

        let name = write_changeset(dir.path(), &packages, "").unwrap();
        let content = std::fs::read_to_string(dir.path().join(format!("{name}.md"))).unwrap();
        assert!(content.ends_with("---\n"));
    }
}
