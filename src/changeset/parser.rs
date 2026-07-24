use std::collections::BTreeMap;

use crate::changeset::types::{BumpLevel, Changeset};
use crate::errors::ChangesetterError;

pub fn parse(input: &str, filename: Option<String>) -> Result<Changeset, ChangesetterError> {
    let input = input.replace("\r\n", "\n");
    let err_path = || filename.clone().unwrap_or_default().into();

    if !input.starts_with("---\n") {
        return Err(ChangesetterError::InvalidFrontmatter {
            path: err_path(),
            reason: "missing opening ---".to_string(),
        });
    }

    let after_start = 4;
    let Some(end) = input[after_start..].find("\n---") else {
        return Err(ChangesetterError::InvalidFrontmatter {
            path: err_path(),
            reason: "missing closing ---".to_string(),
        });
    };

    let frontmatter = &input[after_start..after_start + end];

    let packages: BTreeMap<String, BumpLevel> =
        serde_yaml::from_str(frontmatter).map_err(|e| ChangesetterError::InvalidFrontmatter {
            path: err_path(),
            reason: e.to_string(),
        })?;

    if packages.is_empty() {
        return Err(ChangesetterError::InvalidFrontmatter {
            path: err_path(),
            reason: "frontmatter contains no package entries".to_string(),
        });
    }

    // end is offset within input[after_start..] pointing at the \n before closing ---
    // So the closing --- starts at after_start + end + 1, and is 3 chars long
    let after_closing = after_start + end + 1 + 3; // skip past ---
    let body = if after_closing < input.len() {
        let raw = &input[after_closing..];
        // Strip up to two newlines right after --- (the \n after --- plus the blank line before body)
        let raw = raw.strip_prefix('\n').unwrap_or(raw);
        let raw = raw.strip_prefix('\n').unwrap_or(raw);
        raw.trim_end().to_string()
    } else {
        String::new()
    };

    Ok(Changeset {
        packages,
        body,
        filename,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_changeset() {
        let input = "---\nmylib: patch\n---\n\n#### Fixed a bug\n\nSome details.\n";
        let cs = parse(input, Some("cool-dogs-dance".to_string())).unwrap();
        assert_eq!(cs.packages.len(), 1);
        assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
        assert_eq!(cs.body, "#### Fixed a bug\n\nSome details.");
        assert_eq!(cs.filename.as_deref(), Some("cool-dogs-dance"));
    }

    #[test]
    fn parse_multiple_packages() {
        let input = "---\nmylib: patch\nmy-api: minor\n---\n\n#### Changes\n";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.packages.len(), 2);
        assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
        assert_eq!(cs.packages["my-api"], BumpLevel::Minor);
    }

    #[test]
    fn parse_none_bump() {
        let input = "---\nmylib: none\n---\n\n#### Updated CI\n";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.packages["mylib"], BumpLevel::None);
    }

    #[test]
    fn parse_major_bump() {
        let input = "---\nmylib: major\n---\n\n#### Breaking change\n";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.packages["mylib"], BumpLevel::Major);
    }

    #[test]
    fn parse_default_package() {
        let input = "---\ndefault: none\n---\n\n#### Docs change\n";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.packages["default"], BumpLevel::None);
    }

    #[test]
    fn parse_scoped_npm_package() {
        let input = "---\n\"@myorg/utils\": patch\n\"@myorg/core\": minor\n---\n\n#### Fix\n";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.packages["@myorg/utils"], BumpLevel::Patch);
        assert_eq!(cs.packages["@myorg/core"], BumpLevel::Minor);
    }

    #[test]
    fn parse_windows_line_endings() {
        let input = "---\r\nmylib: patch\r\n---\r\n\r\n#### Fix\r\n";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
        assert_eq!(cs.body, "#### Fix");
    }

    #[test]
    fn parse_dashes_in_body() {
        let input = "---\nmylib: patch\n---\n\n#### Fix\n\nSome text with ---\nin the body.\n";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
        assert!(cs.body.contains("---"));
    }

    #[test]
    fn parse_empty_body() {
        let input = "---\nmylib: patch\n---\n";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
        assert_eq!(cs.body, "");
    }

    #[test]
    fn parse_body_no_trailing_newline() {
        let input = "---\nmylib: patch\n---\n\n#### Fix";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.body, "#### Fix");
    }

    #[test]
    fn parse_missing_opening_dashes() {
        let result = parse("mylib: patch\n---\n", None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing opening ---"), "got: {err}");
    }

    #[test]
    fn parse_missing_closing_dashes() {
        let result = parse("---\nmylib: patch\n", None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing closing ---"), "got: {err}");
    }

    #[test]
    fn parse_unknown_bump_level() {
        let result = parse("---\nmylib: breaking\n---\n", None);
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_frontmatter() {
        let result = parse("---\n---\n", None);
        assert!(result.is_err());
    }

    #[test]
    fn parse_whitespace_body() {
        let input = "---\nmylib: patch\n---\n\n   \n\n";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.body, "");
    }

    #[test]
    fn parse_multiline_body() {
        let input = "---\nmylib: patch\n---\n\n#### Title\n\nParagraph one.\n\nParagraph two.\n";
        let cs = parse(input, None).unwrap();
        assert_eq!(cs.body, "#### Title\n\nParagraph one.\n\nParagraph two.");
    }
}
