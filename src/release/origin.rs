//! Where each changeset came from: the commit that added it, its author, and
//! the pull request number when the history names one. Everything is read
//! from local git, so it needs the history (`fetch-depth: 0` in CI); a
//! changeset with no commit yet, or a shallow clone, simply has no origin.

use std::collections::BTreeMap;
use std::path::Path;

use crate::changeset::types::{Changeset, Origin};

const RECORD: char = '\u{1e}';
const FIELD: char = '\u{1f}';
const END: char = '\u{1d}';

/// A pull request number in a commit message, in the forms merges and squash
/// merges write: GitHub's "Merge pull request #12 from ..." and "Title
/// (#12)", GitLab's "See merge request group/project!12" and Azure DevOps'
/// "Merged PR 12: Title".
pub fn pr_number(subject: &str, body: &str) -> Option<u64> {
    let digits = |s: &str| -> Option<u64> {
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        s[..end].parse().ok()
    };
    if let Some(rest) = subject.strip_prefix("Merge pull request #") {
        return digits(rest);
    }
    if let Some(rest) = subject.strip_prefix("Merged PR ") {
        return digits(rest);
    }
    if let Some(open) = subject
        .trim_end()
        .strip_suffix(')')
        .and_then(|s| s.rfind("(#").map(|i| &s[i + 2..]))
    {
        if !open.is_empty() && open.chars().all(|c| c.is_ascii_digit()) {
            return open.parse().ok();
        }
    }
    body.lines()
        .filter_map(|l| l.trim().strip_prefix("See merge request "))
        .find_map(|r| r.rsplit_once('!').and_then(|(_, n)| digits(n)))
}

struct Added {
    commit: String,
    author: String,
    subject: String,
    body: String,
}

/// The oldest commit that added each file under `dir`, by repo-relative path.
fn added_commits(repo_root: &Path, dir: &str) -> anyhow::Result<BTreeMap<String, Added>> {
    let format = format!("--format={RECORD}%H{FIELD}%an{FIELD}%s{FIELD}%b{END}");
    let pathspec = format!("{dir}/");
    let Some(out) = crate::git::run_git(
        repo_root,
        &[
            "log",
            "--diff-filter=A",
            "--no-renames",
            "--name-only",
            &format,
            "--",
            &pathspec,
        ],
    )?
    else {
        return Ok(BTreeMap::new());
    };
    let mut added = BTreeMap::new();
    // Newest first, so a later (older) record for the same path wins.
    for record in out.split(RECORD).filter(|r| !r.trim().is_empty()) {
        let Some((header, files)) = record.split_once(END) else {
            continue;
        };
        let mut fields = header.splitn(4, FIELD);
        let (Some(commit), Some(author), Some(subject), Some(body)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        for file in files.lines().map(str::trim).filter(|f| !f.is_empty()) {
            added.insert(
                file.to_string(),
                Added {
                    commit: commit.to_string(),
                    author: author.to_string(),
                    subject: subject.to_string(),
                    body: body.to_string(),
                },
            );
        }
    }
    Ok(added)
}

/// The pull request that merged `commit` into HEAD's branch, when the merge
/// commit names one. That merge is the oldest commit on HEAD's first-parent
/// line that has `commit` in its history. When that commit's first parent is
/// `commit` itself, `commit` was made on the branch directly: no merge.
fn merged_by(repo_root: &Path, commit: &str) -> anyhow::Result<Option<u64>> {
    let range = format!("{commit}..HEAD");
    let Some(chain) = crate::git::run_git(
        repo_root,
        &[
            "rev-list",
            "--first-parent",
            "--ancestry-path",
            "--reverse",
            &range,
        ],
    )?
    else {
        return Ok(None);
    };
    let Some(merge) = chain
        .lines()
        .next()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    else {
        return Ok(None);
    };
    let parent = crate::git::run_git(repo_root, &["rev-parse", &format!("{merge}^1")])?;
    if parent.as_deref().map(str::trim) == Some(commit) {
        return Ok(None);
    }
    let format = format!("--format=%s{FIELD}%b");
    let Some(message) = crate::git::run_git(repo_root, &["log", "-1", &format, merge])? else {
        return Ok(None);
    };
    let (subject, body) = message.split_once(FIELD).unwrap_or((&message, ""));
    Ok(pr_number(subject, body))
}

/// Fills in each changeset's `origin`. `dir` is the changeset directory
/// relative to the repo root, with `/` separators.
pub fn annotate(repo_root: &Path, dir: &str, changesets: &mut [Changeset]) -> anyhow::Result<()> {
    let added = added_commits(repo_root, dir)?;
    let mut merges: BTreeMap<String, Option<u64>> = BTreeMap::new();
    for cs in changesets.iter_mut() {
        let Some(name) = &cs.filename else {
            continue;
        };
        let Some(a) = added.get(&format!("{dir}/{name}.md")) else {
            continue;
        };
        let pr = match pr_number(&a.subject, &a.body) {
            Some(n) => Some(n),
            None => match merges.get(&a.commit) {
                Some(n) => *n,
                None => {
                    let n = merged_by(repo_root, &a.commit)?;
                    merges.insert(a.commit.clone(), n);
                    n
                }
            },
        };
        cs.origin = Some(Origin {
            commit: a.commit.clone(),
            author: a.author.clone(),
            pr,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn pr_numbers_from_merge_messages() {
        assert_eq!(pr_number("Merge pull request #24 from a/b", ""), Some(24));
        assert_eq!(pr_number("feat: groups (#24)", ""), Some(24));
        assert_eq!(pr_number("Merged PR 812: Fix", ""), Some(812));
        assert_eq!(
            pr_number("Fix", "Body\n\nSee merge request group/proj!31\n"),
            Some(31)
        );
        assert_eq!(pr_number("Fix (see #24 for context)", ""), None);
        assert_eq!(pr_number("Fix the thing", ""), None);
        assert_eq!(pr_number("Fix (#)", ""), None);
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "a@b.c"]);
        git(dir.path(), &["config", "user.name", "Ada"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("x"), "x").unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-qm", "init"]);
        dir
    }

    fn changeset(name: &str) -> Changeset {
        crate::changeset::parser::parse("---\nx: patch\n---\n\nFix\n", Some(name.to_string()))
            .unwrap()
    }

    fn add(dir: &Path, name: &str, message: &str) {
        let path = dir.join(".changeset").join(format!("{name}.md"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "---\nx: patch\n---\n\nFix\n").unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-qm", message]);
    }

    #[test]
    fn origin_from_squash_and_merge_commits() {
        let dir = repo();
        let d = dir.path();
        add(d, "squashed", "fix: crash (#7)");
        add(d, "direct", "add changeset on main");
        git(d, &["checkout", "-qb", "feature"]);
        add(d, "team/merged", "add changeset");
        git(d, &["checkout", "-q", "main"]);
        git(
            d,
            &[
                "merge",
                "-q",
                "--no-ff",
                "feature",
                "-m",
                "Merge pull request #9 from o/feature",
            ],
        );
        std::fs::write(d.join(".changeset/local.md"), "---\nx: patch\n---\n").unwrap();

        let mut changesets = vec![
            changeset("squashed"),
            changeset("team/merged"),
            changeset("local"),
            changeset("direct"),
        ];
        annotate(d, ".changeset", &mut changesets).unwrap();

        let o = changesets[0].origin.as_ref().unwrap();
        assert_eq!(o.pr, Some(7));
        assert_eq!(o.author, "Ada");
        assert_eq!(o.commit.len(), 40);
        assert_eq!(changesets[1].origin.as_ref().unwrap().pr, Some(9));
        assert_eq!(changesets[2].origin, None);
        // Made on main before the merge, so the merge's PR isn't its own.
        assert_eq!(changesets[3].origin.as_ref().unwrap().pr, None);
    }

    #[test]
    fn no_pr_without_a_merge() {
        let dir = repo();
        add(dir.path(), "plain", "add changeset");
        let mut changesets = vec![changeset("plain")];
        annotate(dir.path(), ".changeset", &mut changesets).unwrap();
        assert_eq!(changesets[0].origin.as_ref().unwrap().pr, None);
    }
}
