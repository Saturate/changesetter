use std::collections::BTreeMap;
use std::path::Path;

use crate::changeset::types::BumpLevel;
use crate::changeset::words;

pub fn write_changeset(
    changeset_dir: &Path,
    packages: &BTreeMap<String, BumpLevel>,
    body: &str,
) -> anyhow::Result<String> {
    write_changeset_typed(changeset_dir, packages, None, None, body)
}

/// Writes a changeset with an optional `type:` and `scope:` in its front
/// matter, after the packages.
pub fn write_changeset_typed(
    changeset_dir: &Path,
    packages: &BTreeMap<String, BumpLevel>,
    kind: Option<&str>,
    scope: Option<&str>,
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
    for (key, value) in [("type", kind), ("scope", scope)] {
        if let Some(value) = value {
            content.push_str(&format!("{key}: {}\n", yaml_text(value)));
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

/// A value as YAML text that reads back as the same string: plain when it
/// does (`fix`), quoted when YAML would read it as something else, like a
/// number (`1.10`), bool (`true`) or null.
fn yaml_text(value: &str) -> String {
    match serde_yaml::from_str::<serde_yaml::Value>(value) {
        Ok(serde_yaml::Value::String(s)) if s == value => value.to_string(),
        _ => serde_yaml::to_string(value)
            .map(|s| s.trim_end().to_string())
            .unwrap_or_else(|_| format!("{value:?}")),
    }
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
    fn write_type_and_scope_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let packages = BTreeMap::from([("mylib".to_string(), BumpLevel::Minor)]);
        let name = write_changeset_typed(
            dir.path(),
            &packages,
            Some("feat"),
            Some("world"),
            "New biome",
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join(format!("{name}.md"))).unwrap();
        assert_eq!(
            content,
            "---\nmylib: minor\ntype: feat\nscope: world\n---\n\nNew biome\n"
        );
        let cs = parser::parse(&content, Some(name)).unwrap();
        assert_eq!(cs.kind.as_deref(), Some("feat"));
        assert_eq!(cs.scope.as_deref(), Some("world"));
    }

    #[test]
    fn write_quotes_values_yaml_would_misread() {
        let dir = tempfile::tempdir().unwrap();
        let packages = BTreeMap::from([("mylib".to_string(), BumpLevel::Patch)]);
        for scope in ["2024", "1.10", "true", "null", "no", "world"] {
            let name =
                write_changeset_typed(dir.path(), &packages, Some("fix"), Some(scope), "Fix")
                    .unwrap();
            let content = std::fs::read_to_string(dir.path().join(format!("{name}.md"))).unwrap();
            let cs = parser::parse(&content, Some(name)).unwrap();
            assert_eq!(cs.scope.as_deref(), Some(scope), "{content}");
            assert_eq!(cs.kind.as_deref(), Some("fix"));
        }
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
