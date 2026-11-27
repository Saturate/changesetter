//! The version PR's description: what merging it releases, written as the
//! changelog will read, for the person approving the release.

use crate::release::plan::{PlannedRelease, ReleasePlan};

/// Azure DevOps rejects a pull request description over 4000 characters.
pub const AZURE_LIMIT: usize = 4000;
/// GitHub's limit on a pull request body.
pub const GITHUB_LIMIT: usize = 65536;

const INTRO: &str = "This PR was opened by [changesetter](https://github.com/Saturate/changesetter). Merging it releases the packages below; each section is what goes into that package's changelog.";

/// Entries changesetter writes itself when a dependency bump cascades, or a
/// fixed group pulls a package along, rather than from a changeset.
fn is_carried_along(release: &PlannedRelease) -> bool {
    !release.entries.is_empty()
        && release
            .entries
            .iter()
            .all(|e| e.changeset == "dependency-cascade" || e.changeset == "fixed-group")
}

fn label(release: &PlannedRelease) -> String {
    let name = if release.name.is_empty() {
        String::new()
    } else {
        format!("{} ", release.name)
    };
    format!("{name}{} → {}", release.previous_version, release.version)
}

/// Renders the description and cuts it to `limit` characters. Packages with
/// their own changesets get a section each; packages released only because
/// a dependency or fixed group moved are one line each, since their entry
/// says nothing a reviewer needs to read. When it doesn't fit, whole
/// sections are dropped from the end and a note names what was left out.
pub fn render(plan: &ReleasePlan, limit: usize) -> String {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut carried = Vec::new();
    for r in &plan.releases {
        if is_carried_along(r) {
            carried.push(format!("- {}", label(r)));
        } else {
            let changelog = r.changelog.trim();
            let body = if changelog.is_empty() {
                "_No changelog entry._".to_string()
            } else {
                changelog.to_string()
            };
            sections.push((r.name.clone(), format!("## {}\n\n{body}", label(r))));
        }
    }
    if !carried.is_empty() {
        sections.push((
            "dependency updates".to_string(),
            format!("## Dependency updates only\n\n{}", carried.join("\n")),
        ));
    }
    let notes: Vec<String> = plan
        .none_entries
        .iter()
        .filter(|n| !n.title.trim().is_empty())
        .map(|n| format!("- {}", n.title.trim()))
        .collect();
    if !notes.is_empty() {
        sections.push((
            "unversioned notes".to_string(),
            format!("## Not versioned\n\n{}", notes.join("\n")),
        ));
    }
    if sections.is_empty() {
        return format!(
            "{INTRO}\n\nNothing is released; merging only removes changesets that bump nothing."
        );
    }

    let full = std::iter::once(INTRO.to_string())
        .chain(sections.iter().map(|(_, s)| s.clone()))
        .collect::<Vec<_>>()
        .join("\n\n");
    if full.chars().count() <= limit {
        return full;
    }

    let mut out = INTRO.to_string();
    let mut shown = 0;
    for (i, (_, section)) in sections.iter().enumerate() {
        let left_out = &sections[i + 1..];
        let note = truncation_note(left_out.iter().map(|(n, _)| n.as_str()));
        let candidate = format!("{out}\n\n{section}");
        if candidate.chars().count() + 2 + note.chars().count() > limit {
            break;
        }
        out = candidate;
        shown += 1;
    }
    let note = truncation_note(sections[shown..].iter().map(|(n, _)| n.as_str()));
    let mut out = format!("{out}\n\n{note}");
    // A limit too small for the intro and the note still gets a valid body.
    if out.chars().count() > limit {
        out = out.chars().take(limit).collect();
    }
    out
}

fn truncation_note<'a>(left_out: impl Iterator<Item = &'a str>) -> String {
    let names: Vec<&str> = left_out.collect();
    if names.is_empty() {
        return String::new();
    }
    let list = names.join(", ");
    format!(
        "_Cut to fit the description limit. Not shown: {list}. The changelog files in this PR have every entry._"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changeset::types::BumpLevel;
    use crate::package::types::Version;
    use crate::release::plan::NoneEntry;

    fn release(name: &str, from: &str, to: &str, changelog: &str, ids: &[&str]) -> PlannedRelease {
        PlannedRelease {
            name: name.to_string(),
            path: std::path::PathBuf::from("."),
            version: Version::parse(to).unwrap(),
            previous_version: Version::parse(from).unwrap(),
            bump: BumpLevel::Patch,
            changelog: changelog.to_string(),
            changesets: ids.iter().map(|s| s.to_string()).collect(),
            entries: ids
                .iter()
                .map(|id| crate::release::plan::ReleaseEntry::for_test(id))
                .collect(),
        }
    }

    fn plan(releases: Vec<PlannedRelease>, none: &[&str]) -> ReleasePlan {
        ReleasePlan {
            releases,
            none_entries: none
                .iter()
                .map(|t| NoneEntry {
                    title: t.to_string(),
                    ..Default::default()
                })
                .collect(),
        }
    }

    #[test]
    fn groups_per_package_and_collapses_cascades() {
        let p = plan(
            vec![
                release(
                    "@d/ui",
                    "0.1.0",
                    "0.2.0",
                    "#### Added a button\n\nDetails.",
                    &["brave-fox"],
                ),
                release(
                    "@d/web",
                    "1.0.0",
                    "1.0.1",
                    "#### Updated dependencies\n\n- @d/ui@0.2.0",
                    &["dependency-cascade"],
                ),
                release(
                    "@d/admin",
                    "2.0.0",
                    "2.1.0",
                    "#### Login fix",
                    &["calm-cat", "dependency-cascade"],
                ),
            ],
            &["Moved CI to a new pool"],
        );
        let body = render(&p, GITHUB_LIMIT);
        assert_eq!(
            body,
            format!(
                "{INTRO}\n\n\
                 ## @d/ui 0.1.0 → 0.2.0\n\n#### Added a button\n\nDetails.\n\n\
                 ## @d/admin 2.0.0 → 2.1.0\n\n#### Login fix\n\n\
                 ## Dependency updates only\n\n- @d/web 1.0.0 → 1.0.1\n\n\
                 ## Not versioned\n\n- Moved CI to a new pool"
            )
        );
    }

    #[test]
    fn cuts_whole_sections_and_names_what_was_left_out() {
        let long = "x".repeat(1500);
        let p = plan(
            vec![
                release("@d/a", "1.0.0", "1.0.1", &long, &["a"]),
                release("@d/b", "1.0.0", "1.0.1", &long, &["b"]),
                release("@d/c", "1.0.0", "1.0.1", &long, &["c"]),
            ],
            &[],
        );
        let body = render(&p, AZURE_LIMIT);
        assert!(body.chars().count() <= AZURE_LIMIT);
        assert!(body.contains("## @d/a") && body.contains("## @d/b"));
        assert!(!body.contains("## @d/c"));
        assert!(
            body.ends_with("Not shown: @d/c. The changelog files in this PR have every entry._"),
            "{body}"
        );
    }

    #[test]
    fn a_single_oversized_section_still_fits_the_limit() {
        let p = plan(
            vec![release("@d/a", "1.0.0", "1.0.1", &"é".repeat(5000), &["a"])],
            &[],
        );
        let body = render(&p, AZURE_LIMIT);
        assert!(body.chars().count() <= AZURE_LIMIT);
        assert!(body.contains("Not shown: @d/a"));
    }

    #[test]
    fn nothing_released() {
        let body = render(&plan(vec![], &[]), AZURE_LIMIT);
        assert!(body.contains("Nothing is released"));
    }
}
