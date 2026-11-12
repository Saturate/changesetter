//! Grouped changelogs: each entry gets a conventional type ("feat", "fix",
//! ...) and is listed under the first `[[changelog.groups]]` entry that takes
//! it. The type only decides where an entry is listed; the bump alone decides
//! the version.

use crate::changeset::types::BumpLevel;
use crate::config::{ChangelogConfig, ChangelogGroup, glob_match};

/// Types a summary prefix like "fix: ..." or "feat(world)!: ..." is read as,
/// besides the types the configured groups name. A prefix outside this set
/// ("Note: ...") is left as part of the summary.
const KNOWN_TYPES: &[&str] = &[
    "breaking", "feat", "fix", "perf", "docs", "refactor", "test", "ci", "chore", "revert",
    "build", "style", "deps",
];

/// The type of the entries changesetter writes itself: a package bumped with
/// its fixed group or for a dependency.
pub const DEPS_TYPE: &str = "deps";

/// Where an entry goes when no group takes it.
pub const FALLBACK_TITLE: &str = "Other changes";

/// An entry's type, scope and one-line summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub kind: String,
    pub scope: Option<String>,
    pub summary: String,
}

/// The type an entry gets from its bump when nothing else names one.
pub fn kind_for_bump(bump: BumpLevel) -> &'static str {
    match bump {
        BumpLevel::Major => "breaking",
        BumpLevel::Minor => "feat",
        BumpLevel::Patch => "fix",
        BumpLevel::None => "chore",
    }
}

/// The first non-blank line of a body, without heading hashes.
pub fn first_line(body: &str) -> &str {
    body.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .trim_start_matches('#')
        .trim()
}

/// Reads a conventional prefix: `type`, optional `(scope)`, optional `!`,
/// then ": " and the rest. Returns the type, scope, whether it had `!`, and
/// the rest.
fn split_prefix<'a>(
    line: &'a str,
    extra_types: &[&str],
) -> Option<(&'a str, Option<&'a str>, bool, &'a str)> {
    let (head, rest) = line.split_once(": ")?;
    let (head, bang) = match head.strip_suffix('!') {
        Some(h) => (h, true),
        None => (head, false),
    };
    let (kind, scope) = match head.split_once('(') {
        Some((kind, scope)) => (kind, Some(scope.strip_suffix(')')?)),
        None => (head, None),
    };
    let known = KNOWN_TYPES.contains(&kind) || extra_types.contains(&kind);
    let scope_ok = scope.is_none_or(|s| !s.trim().is_empty() && !s.contains(['(', ')']));
    let rest = rest.trim();
    (known && scope_ok && !rest.is_empty()).then(|| (kind, scope.map(str::trim), bang, rest))
}

/// Works out an entry's type, scope and summary. The type comes from the
/// front matter's `type:`, else a conventional prefix on the summary (`!`
/// makes it `breaking`), else the bump. A recognised prefix is stripped from
/// the summary either way.
pub fn classify(
    body: &str,
    kind: Option<&str>,
    scope: Option<&str>,
    bump: BumpLevel,
    config: &ChangelogConfig,
) -> Classified {
    let extra: Vec<&str> = config
        .groups
        .iter()
        .flat_map(|g| g.types.iter().map(String::as_str))
        .collect();
    let line = first_line(body);
    let prefix = split_prefix(line, &extra);
    let summary = prefix.map_or(line, |(_, _, _, rest)| rest).to_string();
    let prefix_kind = prefix.map(|(k, _, bang, _)| if bang { "breaking" } else { k });
    Classified {
        kind: kind
            .or(prefix_kind)
            .unwrap_or_else(|| kind_for_bump(bump))
            .to_string(),
        scope: scope
            .or(prefix.and_then(|(_, s, _, _)| s))
            .map(str::to_string),
        summary,
    }
}

fn group(title: &str, types: &[&str]) -> ChangelogGroup {
    ChangelogGroup {
        title: title.to_string(),
        types: types.iter().map(|t| t.to_string()).collect(),
        pattern: None,
        skip: false,
    }
}

/// The groups `group = true` uses when none are configured.
pub fn default_groups() -> Vec<ChangelogGroup> {
    vec![
        group("Breaking changes", &["breaking"]),
        group("Features", &["feat"]),
        group("Fixes", &["fix"]),
        group("Performance", &["perf"]),
        group("Reverts", &["revert"]),
        group(
            "Under the hood",
            &[
                "docs", "refactor", "test", "ci", "chore", "build", "style", DEPS_TYPE,
            ],
        ),
    ]
}

/// The groups in effect, in order: the configured ones, or the defaults.
pub fn groups(config: &ChangelogConfig) -> Vec<ChangelogGroup> {
    if config.groups.is_empty() {
        default_groups()
    } else {
        config.groups.clone()
    }
}

/// The first group that takes an entry, or `None` when none does.
pub fn find_group<'a>(
    groups: &'a [ChangelogGroup],
    kind: &str,
    summary: &str,
) -> Option<&'a ChangelogGroup> {
    let summary = summary.to_lowercase();
    groups.iter().find(|g| {
        (g.types.is_empty() || g.types.iter().any(|t| t == kind))
            && g.pattern
                .as_ref()
                .is_none_or(|p| glob_match(&p.to_lowercase(), &summary))
    })
}

/// Renders an entry as a list item: "- **scope:** summary", then the rest of
/// its body indented under it. `demoted` is the body as
/// `markdown::demote_headings` left it.
pub fn render_item(demoted: &str, summary: &str, scope: Option<&str>) -> String {
    let mut item = match scope {
        Some(scope) => format!("- **{scope}:** {summary}"),
        None => format!("- {summary}"),
    };
    let mut lines = demoted.lines().skip_while(|l| l.trim().is_empty());
    lines.next();
    let rest: Vec<&str> = lines.collect();
    let rest = rest
        .iter()
        .skip_while(|l| l.trim().is_empty())
        .collect::<Vec<_>>();
    let end = rest
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(0, |i| i + 1);
    if end > 0 {
        item.push_str("\n\n");
        let body: Vec<String> = rest[..end]
            .iter()
            .map(|l| {
                if l.trim().is_empty() {
                    String::new()
                } else {
                    format!("  {l}")
                }
            })
            .collect();
        item.push_str(&body.join("\n"));
    }
    item
}

/// Renders a version's entries grouped: a heading per group with entries, in
/// the groups' order, then the fallback group. `items` pairs each entry's
/// group title (`None` for the fallback) with its rendered item.
pub fn render_sections(
    groups: &[ChangelogGroup],
    items: &[(Option<String>, String)],
    level: usize,
) -> String {
    let hashes = "#".repeat(level);
    let mut titles: Vec<Option<&str>> = Vec::new();
    for g in groups {
        if !titles.contains(&Some(g.title.as_str())) {
            titles.push(Some(&g.title));
        }
    }
    titles.push(None);
    let mut sections = Vec::new();
    for title in titles {
        let list: Vec<&str> = items
            .iter()
            .filter(|(t, _)| t.as_deref() == title)
            .map(|(_, item)| item.as_str())
            .collect();
        if list.is_empty() {
            continue;
        }
        let separator = if list.iter().any(|i| i.contains('\n')) {
            "\n\n"
        } else {
            "\n"
        };
        sections.push(format!(
            "{hashes} {}\n\n{}",
            title.unwrap_or(FALLBACK_TITLE),
            list.join(separator)
        ));
    }
    sections.join("\n\n")
}

/// A warning for an entry typed `breaking` that doesn't bump major.
pub fn breaking_warning(file: &str, kind: &str, package: &str, bump: BumpLevel) -> Option<String> {
    (kind == "breaking" && bump != BumpLevel::Major).then(|| {
        format!(
            "warning: {file}: typed breaking but bumps {package} {bump}; the bump decides the version, so this won't be a major release"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ChangelogConfig {
        ChangelogConfig::default()
    }

    #[test]
    fn kind_from_bump() {
        let c = classify("Added a thing", None, None, BumpLevel::Minor, &cfg());
        assert_eq!(c.kind, "feat");
        assert_eq!(c.summary, "Added a thing");
        assert_eq!(c.scope, None);
        assert_eq!(
            classify("x", None, None, BumpLevel::Major, &cfg()).kind,
            "breaking"
        );
        assert_eq!(
            classify("x", None, None, BumpLevel::Patch, &cfg()).kind,
            "fix"
        );
        assert_eq!(
            classify("x", None, None, BumpLevel::None, &cfg()).kind,
            "chore"
        );
    }

    #[test]
    fn prefix_sets_kind_and_scope_and_is_stripped() {
        let c = classify(
            "#### feat(world): New biome\n\nDetails",
            None,
            None,
            BumpLevel::Patch,
            &cfg(),
        );
        assert_eq!(c.kind, "feat");
        assert_eq!(c.scope.as_deref(), Some("world"));
        assert_eq!(c.summary, "New biome");
    }

    #[test]
    fn bang_prefix_is_breaking() {
        let c = classify(
            "refactor!: Drop old saves",
            None,
            None,
            BumpLevel::Major,
            &cfg(),
        );
        assert_eq!(c.kind, "breaking");
        assert_eq!(c.summary, "Drop old saves");
    }

    #[test]
    fn front_matter_wins_over_prefix() {
        let c = classify(
            "fix(ui): Button",
            Some("perf"),
            Some("core"),
            BumpLevel::Patch,
            &cfg(),
        );
        assert_eq!(c.kind, "perf");
        assert_eq!(c.scope.as_deref(), Some("core"));
        assert_eq!(c.summary, "Button");
    }

    #[test]
    fn unknown_prefix_stays_in_summary() {
        let c = classify("Note: be careful", None, None, BumpLevel::Patch, &cfg());
        assert_eq!(c.kind, "fix");
        assert_eq!(c.summary, "Note: be careful");
        let c = classify("http://x: y", None, None, BumpLevel::Patch, &cfg());
        assert_eq!(c.summary, "http://x: y");
    }

    #[test]
    fn configured_types_count_as_prefixes() {
        let mut config = cfg();
        config.groups = vec![group("Balance", &["balance"])];
        let c = classify(
            "balance: Cheaper turrets",
            None,
            None,
            BumpLevel::Patch,
            &config,
        );
        assert_eq!(c.kind, "balance");
        assert_eq!(c.summary, "Cheaper turrets");
    }

    #[test]
    fn first_matching_group_wins() {
        let mut groups = vec![ChangelogGroup {
            title: "Security".into(),
            types: vec![],
            pattern: Some("*SECURITY*".into()),
            skip: false,
        }];
        groups.extend(default_groups());
        assert_eq!(
            find_group(&groups, "fix", "Security: patch the login").map(|g| g.title.as_str()),
            Some("Security")
        );
        assert_eq!(
            find_group(&groups, "fix", "Patch the login").map(|g| g.title.as_str()),
            Some("Fixes")
        );
        assert_eq!(find_group(&groups, "wat", "x"), None);
    }

    #[test]
    fn item_with_scope_and_details() {
        let item = render_item(
            "#### New biome\n\nWith trees.\n\n```\ncode\n```\n",
            "New biome",
            Some("world"),
        );
        assert_eq!(
            item,
            "- **world:** New biome\n\n  With trees.\n\n  ```\n  code\n  ```"
        );
        assert_eq!(
            render_item("Just one line", "Just one line", None),
            "- Just one line"
        );
    }

    #[test]
    fn sections_in_group_order_with_fallback_last() {
        let groups = default_groups();
        let items = vec![
            (Some("Fixes".to_string()), "- b".to_string()),
            (None, "- z".to_string()),
            (Some("Features".to_string()), "- a".to_string()),
            (Some("Fixes".to_string()), "- c".to_string()),
        ];
        assert_eq!(
            render_sections(&groups, &items, 3),
            "### Features\n\n- a\n\n### Fixes\n\n- b\n- c\n\n### Other changes\n\n- z"
        );
    }

    #[test]
    fn breaking_warning_only_below_major() {
        assert!(breaking_warning("a.md", "breaking", "game", BumpLevel::Minor).is_some());
        assert!(breaking_warning("a.md", "breaking", "game", BumpLevel::Major).is_none());
        assert!(breaking_warning("a.md", "feat", "game", BumpLevel::Minor).is_none());
    }
}
