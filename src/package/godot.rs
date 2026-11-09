use std::path::{Path, PathBuf};

use crate::errors::ChangesetterError;
use crate::package::adapter::Adapter;
use crate::package::types::{Package, PackageType, Version};

/// Reads and writes `project.godot`, Godot's INI-like ConfigFile.
///
/// The name comes from `config/name` and the version from `config/version`,
/// both under `[application]`. Godot leaves `config/version` out until a
/// version is set. Auto-detection skips such projects, so a demo or example
/// project inside another package's repo doesn't become a package (and turn
/// a single-package repo into a monorepo). A project listed under
/// `[[package]]` reads as `0.0.0` instead, and the first write inserts the
/// key directly after `config/name`.
pub struct GodotAdapter;

const MANIFEST: &str = "project.godot";
const SECTION: &str = "application";
const NAME_KEY: &str = "config/name";
const VERSION_KEY: &str = "config/version";

fn manifest_path(path: &Path) -> PathBuf {
    if path.is_file() {
        path.to_path_buf()
    } else {
        path.join(MANIFEST)
    }
}

fn read_manifest(path: &Path) -> Result<String, ChangesetterError> {
    std::fs::read_to_string(path).map_err(|e| ChangesetterError::ManifestRead {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

/// A `key=value` line inside `[application]`, as a byte range into the file.
struct Entry<'a> {
    /// Offset of the line's first byte.
    start: usize,
    /// The line without its line ending.
    text: &'a str,
    /// The line ending (`"\r\n"`, `"\n"` or `""` on a final unterminated line).
    eol: &'a str,
}

/// Splits `line` (as yielded by `split_inclusive('\n')`) into text and ending.
fn split_eol(line: &str) -> (&str, &str) {
    if let Some(text) = line.strip_suffix("\r\n") {
        (text, "\r\n")
    } else if let Some(text) = line.strip_suffix('\n') {
        (text, "\n")
    } else {
        (line, "")
    }
}

/// Returns the section header name if `line` is one, e.g. `[application]`.
fn section_header(text: &str) -> Option<&str> {
    let t = text.trim();
    t.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

/// Returns the value of `key` if `text` is a `key=value` line for it.
fn key_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let (k, v) = text.split_once('=')?;
    (k.trim() == key).then(|| v.trim())
}

/// Where the `[application]` header and its name and version lines are.
struct Scan<'a> {
    header: Option<Entry<'a>>,
    name: Option<Entry<'a>>,
    version: Option<Entry<'a>>,
}

fn scan(content: &str) -> Scan<'_> {
    let mut result = Scan {
        header: None,
        name: None,
        version: None,
    };
    let mut in_section = false;
    let mut offset = 0;

    for line in content.split_inclusive('\n') {
        let (text, eol) = split_eol(line);
        let entry = || Entry {
            start: offset,
            text,
            eol,
        };

        if let Some(section) = section_header(text) {
            in_section = section == SECTION;
            if in_section && result.header.is_none() {
                result.header = Some(entry());
            }
        } else if in_section {
            if result.name.is_none() && key_value(text, NAME_KEY).is_some() {
                result.name = Some(entry());
            } else if result.version.is_none() && key_value(text, VERSION_KEY).is_some() {
                result.version = Some(entry());
            }
        }

        offset += line.len();
    }

    result
}

/// Decodes a ConfigFile string value: `"Slopocalypse"` becomes `Slopocalypse`.
fn unquote(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);

    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn entry_value(entry: &Entry<'_>, key: &str) -> String {
    unquote(key_value(entry.text, key).unwrap_or_default())
}

fn version_line(version: &Version) -> String {
    format!("{VERSION_KEY}=\"{version}\"")
}

impl GodotAdapter {
    fn detect_inner(
        &self,
        path: &Path,
        allow_missing_version: bool,
    ) -> anyhow::Result<Option<Package>> {
        let manifest = path.join(MANIFEST);
        if !manifest.is_file() {
            return Ok(None);
        }

        let content = read_manifest(&manifest)?;
        let found = scan(&content);

        let version = match &found.version {
            Some(e) => parse_version(&entry_value(e, VERSION_KEY), &manifest)?,
            None if allow_missing_version => Version::new(0, 0, 0),
            None => return Ok(None),
        };

        let name = found
            .name
            .as_ref()
            .map(|e| entry_value(e, NAME_KEY))
            .unwrap_or_default();

        Ok(Some(Package {
            name,
            path: path.to_path_buf(),
            package_type: PackageType::Godot,
            version,
        }))
    }
}

fn parse_version(value: &str, manifest: &Path) -> anyhow::Result<Version> {
    Version::parse(value).map_err(|_| {
        anyhow::anyhow!(
            "{VERSION_KEY}=\"{value}\" in {} is not a semver version (expected x.y.z, e.g. \"1.0.0\")",
            manifest.display()
        )
    })
}

impl Adapter for GodotAdapter {
    fn detect(&self, path: &Path) -> anyhow::Result<Option<Package>> {
        self.detect_inner(path, false)
    }

    fn detect_configured(&self, path: &Path) -> anyhow::Result<Option<Package>> {
        self.detect_inner(path, true)
    }

    fn read_version(&self, path: &Path) -> anyhow::Result<Version> {
        let manifest = manifest_path(path);
        let content = read_manifest(&manifest)?;
        let entry = scan(&content).version.ok_or_else(|| {
            anyhow::anyhow!(
                "no {VERSION_KEY} under [{SECTION}] in {}",
                manifest.display()
            )
        })?;
        parse_version(&entry_value(&entry, VERSION_KEY), &manifest)
    }

    fn write_version(&self, path: &Path, version: &Version) -> anyhow::Result<()> {
        let manifest = manifest_path(path);
        let content = read_manifest(&manifest)?;
        let found = scan(&content);
        let new_line = version_line(version);

        let output = if let Some(existing) = &found.version {
            // Replace only the line's text; keep its line ending.
            let end = existing.start + existing.text.len();
            format!(
                "{}{new_line}{}",
                &content[..existing.start],
                &content[end..]
            )
        } else {
            // Insert after config/name, or after the header if there is no
            // name. The new line reuses the anchor's line ending, falling back
            // to the file's first line ending when the anchor is the last,
            // unterminated line.
            let anchor = found
                .name
                .as_ref()
                .or(found.header.as_ref())
                .ok_or_else(|| {
                    anyhow::anyhow!("no [{SECTION}] section in {}", manifest.display())
                })?;
            let file_eol = if content.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            let at = anchor.start + anchor.text.len() + anchor.eol.len();
            if anchor.eol.is_empty() {
                format!("{}{file_eol}{new_line}{}", &content[..at], &content[at..])
            } else {
                format!(
                    "{}{new_line}{}{}",
                    &content[..at],
                    anchor.eol,
                    &content[at..]
                )
            }
        };

        std::fs::write(&manifest, output).map_err(|e| ChangesetterError::ManifestWrite {
            path: manifest,
            reason: e.to_string(),
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/adapters/godot/project.godot");

    fn write_project(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join(MANIFEST);
        std::fs::write(&path, content).unwrap();
        path
    }

    /// The fixture with LF endings regardless of how git checked it out.
    fn fixture_lf() -> String {
        FIXTURE.replace("\r\n", "\n")
    }

    #[test]
    fn auto_detect_skips_project_without_version() {
        let dir = tempfile::tempdir().unwrap();
        write_project(dir.path(), &fixture_lf());
        assert!(GodotAdapter.detect(dir.path()).unwrap().is_none());
    }

    #[test]
    fn non_semver_version_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        write_project(
            dir.path(),
            "[application]

config/name=\"G\"
config/version=\"1.0\"
",
        );
        let err = GodotAdapter.detect(dir.path()).unwrap_err().to_string();
        assert!(err.contains("project.godot"), "{err}");
        assert!(err.contains("\"1.0\""), "{err}");
    }

    #[test]
    fn configured_without_version_reads_zero() {
        let dir = tempfile::tempdir().unwrap();
        write_project(dir.path(), &fixture_lf());

        let pkg = GodotAdapter.detect_configured(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "Slopocalypse");
        assert_eq!(pkg.version, Version::new(0, 0, 0));
        assert_eq!(pkg.package_type, PackageType::Godot);
    }

    #[test]
    fn detect_no_project_godot() {
        let dir = tempfile::tempdir().unwrap();
        assert!(GodotAdapter.detect(dir.path()).unwrap().is_none());
    }

    #[test]
    fn first_write_inserts_after_name_lf() {
        let dir = tempfile::tempdir().unwrap();
        let original = fixture_lf();
        let path = write_project(dir.path(), &original);

        GodotAdapter
            .write_version(dir.path(), &Version::new(0, 1, 0))
            .unwrap();

        let expected = original.replace(
            "config/name=\"Slopocalypse\"\n",
            "config/name=\"Slopocalypse\"\nconfig/version=\"0.1.0\"\n",
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        assert_eq!(
            GodotAdapter.read_version(dir.path()).unwrap(),
            Version::new(0, 1, 0)
        );
    }

    #[test]
    fn first_write_and_bump_keep_crlf_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let original = fixture_lf().replace('\n', "\r\n");
        let path = write_project(dir.path(), &original);

        GodotAdapter
            .write_version(dir.path(), &Version::new(0, 1, 0))
            .unwrap();
        let after_insert = original.replace(
            "config/name=\"Slopocalypse\"\r\n",
            "config/name=\"Slopocalypse\"\r\nconfig/version=\"0.1.0\"\r\n",
        );
        assert_eq!(std::fs::read(&path).unwrap(), after_insert.as_bytes());

        GodotAdapter
            .write_version(dir.path(), &Version::new(0, 2, 0))
            .unwrap();
        let after_bump = after_insert.replace("\"0.1.0\"", "\"0.2.0\"");
        assert_eq!(std::fs::read(&path).unwrap(), after_bump.as_bytes());
    }

    #[test]
    fn bump_replaces_only_version_line() {
        let dir = tempfile::tempdir().unwrap();
        let original = fixture_lf().replace(
            "config/name=\"Slopocalypse\"\n",
            "config/name=\"Slopocalypse\"\nconfig/version=\"0.1.0\"\n",
        );
        let path = write_project(dir.path(), &original);

        let pkg = GodotAdapter.detect(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.version, Version::new(0, 1, 0));

        GodotAdapter
            .write_version(dir.path(), &Version::new(0, 2, 0))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original.replace("config/version=\"0.1.0\"", "config/version=\"0.2.0\"")
        );
    }

    #[test]
    fn ignores_keys_in_other_sections() {
        let dir = tempfile::tempdir().unwrap();
        let content = "config_version=5\n\n[other]\n\nconfig/name=\"Wrong\"\nconfig/version=\"9.9.9\"\n\n[application]\n\nconfig/name=\"Right\"\n";
        let path = write_project(dir.path(), content);

        let pkg = GodotAdapter.detect_configured(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "Right");
        assert_eq!(pkg.version, Version::new(0, 0, 0));

        GodotAdapter
            .write_version(dir.path(), &Version::new(1, 0, 0))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            content.to_string() + "config/version=\"1.0.0\"\n"
        );
    }

    #[test]
    fn insert_after_unterminated_last_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_project(dir.path(), "[application]\r\n\r\nconfig/name=\"Game\"");

        GodotAdapter
            .write_version(dir.path(), &Version::new(0, 1, 0))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[application]\r\n\r\nconfig/name=\"Game\"\r\nconfig/version=\"0.1.0\""
        );
    }

    #[test]
    fn insert_after_header_when_name_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_project(
            dir.path(),
            "[application]\n\nrun/main_scene=\"res://a.tscn\"\n",
        );

        GodotAdapter
            .write_version(dir.path(), &Version::new(0, 1, 0))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[application]\nconfig/version=\"0.1.0\"\n\nrun/main_scene=\"res://a.tscn\"\n"
        );
    }

    #[test]
    fn write_without_application_section_fails() {
        let dir = tempfile::tempdir().unwrap();
        write_project(dir.path(), "config_version=5\n\n[display]\n");
        assert!(
            GodotAdapter
                .write_version(dir.path(), &Version::new(0, 1, 0))
                .is_err()
        );
    }

    #[test]
    fn unquote_handles_escapes() {
        assert_eq!(unquote(r#""Slop \"ocalypse\"""#), "Slop \"ocalypse\"");
        assert_eq!(unquote("bare"), "bare");
    }
}
