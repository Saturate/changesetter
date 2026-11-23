//! Where each changeset came from: the commit that added it, its author, and
//! the pull request number when the history names one. Everything is read
//! from local git, so it needs the history (`fetch-depth: 0` in CI); a
//! changeset with no commit yet, or a shallow clone, simply has no origin.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use crate::changeset::types::{Changeset, Origin};

const FIELD: char = '\u{1f}';
const END: char = '\u{1d}';

/// A pull request number in a commit message, in the forms merges and squash
/// merges write: GitHub's "Merge pull request #12 from ..." and "Title
/// (#12)", GitLab's "See merge request group/project!12" and Azure DevOps'
/// "Merged PR 12: Title".
pub fn pr_number(subject: &str, body: &str) -> Option<u64> {
    let digits = |s: &str| -> Option<u64> {
        let end = s.bytes().take_while(u8::is_ascii_digit).count();
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

/// The commits a shallow clone's history stops at. `git log` shows every
/// file as added in them, so they say nothing about where a file came from.
fn shallow_boundary(repo_root: &Path) -> anyhow::Result<HashSet<String>> {
    let shallow = crate::git::run_git(repo_root, &["rev-parse", "--is-shallow-repository"])?;
    if shallow.as_deref().map(str::trim) != Some("true") {
        return Ok(HashSet::new());
    }
    let Some(path) = crate::git::run_git(repo_root, &["rev-parse", "--git-path", "shallow"])?
    else {
        return Ok(HashSet::new());
    };
    let path = repo_root.join(path.trim());
    Ok(std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// The files under `dir` that HEAD tracks. A changeset that isn't committed
/// has no origin, even when an older file once had its name.
fn tracked(repo_root: &Path, dir: &str) -> anyhow::Result<HashSet<String>> {
    let pathspec = format!("{dir}/");
    let out = crate::git::run_git(
        repo_root,
        &[
            "--literal-pathspecs",
            "ls-tree",
            "-r",
            "-z",
            "--name-only",
            "HEAD",
            "--",
            &pathspec,
        ],
    )?
    .unwrap_or_default();
    Ok(out
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .collect())
}

/// The commit that added the file at `path` as HEAD has it. A per-file log
/// follows the side of each merge the file came from, so a name reused after
/// a release deleted the old file gets the commit that added this one.
fn added_commit(
    repo_root: &Path,
    path: &str,
    boundary: &HashSet<String>,
) -> anyhow::Result<Option<Added>> {
    let format = format!("--format=%H{FIELD}%an{FIELD}%s{FIELD}%b{END}");
    let Some(out) = crate::git::run_git(
        repo_root,
        &[
            "--literal-pathspecs",
            "log",
            "-1",
            "--no-show-signature",
            "--diff-filter=A",
            "--no-renames",
            &format,
            "--",
            path,
        ],
    )?
    else {
        return Ok(None);
    };
    let header = out.split_once(END).map_or(out.as_str(), |(h, _)| h);
    let mut fields = header.splitn(4, FIELD);
    let (Some(commit), Some(author), Some(subject), Some(body)) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return Ok(None);
    };
    let commit = commit.trim();
    if commit.is_empty() || boundary.contains(commit) {
        return Ok(None);
    }
    Ok(Some(Added {
        commit: commit.to_string(),
        author: author.to_string(),
        subject: subject.to_string(),
        body: body.to_string(),
    }))
}

/// How deep `merged_by` follows merges of merges (a feature merged into
/// develop, develop into main).
const MERGE_DEPTH: usize = 3;

/// History lookups for `merged_by`, cached across changesets.
struct History<'a> {
    repo_root: &'a Path,
    lines: HashMap<String, Vec<String>>,
}

impl History<'_> {
    /// The first-parent line from `tip`, newest first.
    fn first_parent_line(&mut self, tip: &str) -> anyhow::Result<&[String]> {
        if !self.lines.contains_key(tip) {
            let out = crate::git::run_git(self.repo_root, &["rev-list", "--first-parent", tip])?
                .unwrap_or_default();
            let line = out.lines().map(|l| l.trim().to_string()).collect();
            self.lines.insert(tip.to_string(), line);
        }
        Ok(&self.lines[tip])
    }

    fn git(&self, args: &[&str]) -> anyhow::Result<Option<String>> {
        crate::git::run_git(self.repo_root, args)
    }

    /// The pull request that brought `commit` onto `tip`'s first-parent line.
    /// That's the oldest commit on the line with `commit` in its history (a
    /// merge), unless `commit` is on the line itself. The merge's own branch
    /// is searched first, so a feature merged into develop and develop into
    /// main gets the feature's pull request, not the release's.
    fn merged_by(
        &mut self,
        tip: &str,
        commit: &str,
        descendants: &HashSet<String>,
        depth: usize,
    ) -> anyhow::Result<Option<u64>> {
        let line = self.first_parent_line(tip)?;
        if line.iter().any(|c| c == commit) {
            return Ok(None);
        }
        let Some(merge) = line
            .iter()
            .rev()
            .find(|c| descendants.contains(*c))
            .cloned()
        else {
            return Ok(None);
        };
        if depth > 0 {
            if let Some(branch) =
                self.git(&["rev-parse", "--verify", "-q", &format!("{merge}^2")])?
            {
                let inner = self.merged_by(branch.trim(), commit, descendants, depth - 1)?;
                if inner.is_some() {
                    return Ok(inner);
                }
            }
        }
        let format = format!("--format=%s{FIELD}%b");
        let message = self
            .git(&["log", "-1", "--no-show-signature", &format, &merge])?
            .unwrap_or_default();
        let (subject, body) = message.split_once(FIELD).unwrap_or((&message, ""));
        Ok(pr_number(subject, body))
    }

    /// Every commit between `commit` and HEAD that has `commit` in its history.
    fn descendants(&self, commit: &str) -> anyhow::Result<HashSet<String>> {
        let range = format!("{commit}..HEAD");
        let out = self
            .git(&["rev-list", "--ancestry-path", &range])?
            .unwrap_or_default();
        Ok(out.lines().map(|l| l.trim().to_string()).collect())
    }
}

/// Fills in each changeset's `origin`. `dir` is the changeset directory
/// relative to the repo root, with `/` separators.
pub fn annotate(repo_root: &Path, dir: &str, changesets: &mut [Changeset]) -> anyhow::Result<()> {
    let tracked = tracked(repo_root, dir)?;
    if tracked.is_empty() {
        return Ok(());
    }
    let boundary = shallow_boundary(repo_root)?;
    let mut history = History {
        repo_root,
        lines: HashMap::new(),
    };
    let mut merges: BTreeMap<String, Option<u64>> = BTreeMap::new();
    for cs in changesets.iter_mut() {
        let Some(name) = &cs.filename else {
            continue;
        };
        let path = format!("{dir}/{name}.md");
        if !tracked.contains(&path) {
            continue;
        }
        let Some(a) = added_commit(repo_root, &path, &boundary)? else {
            continue;
        };
        let pr = match pr_number(&a.subject, &a.body) {
            Some(n) => Some(n),
            None => match merges.get(&a.commit) {
                Some(n) => *n,
                None => {
                    let descendants = history.descendants(&a.commit)?;
                    let n = history.merged_by("HEAD", &a.commit, &descendants, MERGE_DEPTH)?;
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

    fn pr_of(dir: &Path, name: &str) -> Option<u64> {
        let mut changesets = vec![changeset(name)];
        annotate(dir, ".changeset", &mut changesets).unwrap();
        changesets[0].origin.as_ref().and_then(|o| o.pr)
    }

    fn commit_file(dir: &Path, file: &str, message: &str) {
        std::fs::write(dir.join(file), message).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-qm", message]);
    }

    #[test]
    fn branch_with_more_commits_after_the_changeset() {
        let dir = repo();
        let d = dir.path();
        git(d, &["checkout", "-qb", "feat"]);
        add(d, "a", "add changeset");
        commit_file(d, "y", "review fixes");
        git(d, &["checkout", "-q", "main"]);
        commit_file(d, "z", "unrelated on main");
        git(
            d,
            &[
                "merge",
                "-q",
                "--no-ff",
                "feat",
                "-m",
                "Merge pull request #9 from o/feat",
            ],
        );
        commit_file(d, "w", "later on main");
        assert_eq!(pr_of(d, "a"), Some(9));
    }

    #[test]
    fn feature_through_develop_gets_the_feature_pr() {
        let dir = repo();
        let d = dir.path();
        git(d, &["checkout", "-qb", "develop"]);
        git(d, &["checkout", "-qb", "feat"]);
        add(d, "a", "add changeset");
        commit_file(d, "y", "more");
        git(d, &["checkout", "-q", "develop"]);
        git(
            d,
            &[
                "merge",
                "-q",
                "--no-ff",
                "feat",
                "-m",
                "Merge pull request #9 from o/feat",
            ],
        );
        commit_file(d, "v", "direct on develop");
        add(d, "b", "changeset straight on develop");
        git(d, &["checkout", "-q", "main"]);
        git(
            d,
            &[
                "merge",
                "-q",
                "--no-ff",
                "develop",
                "-m",
                "Merge pull request #50 from o/develop",
            ],
        );
        assert_eq!(pr_of(d, "a"), Some(9));
        // Committed on develop, so the release merge is what brought it in.
        assert_eq!(pr_of(d, "b"), Some(50));
    }

    #[test]
    fn reused_name_takes_the_newest_add() {
        let dir = repo();
        let d = dir.path();
        add(d, "fix", "old (#3)");
        std::fs::remove_file(d.join(".changeset/fix.md")).unwrap();
        git(d, &["add", "-A"]);
        git(d, &["commit", "-qm", "release"]);
        add(d, "fix", "new (#40)");
        assert_eq!(pr_of(d, "fix"), Some(40));
    }

    #[test]
    fn non_ascii_names() {
        let dir = repo();
        add(dir.path(), "føø", "add (#5)");
        assert_eq!(pr_of(dir.path(), "føø"), Some(5));
    }

    #[test]
    fn shallow_clone_boundary_is_not_an_origin() {
        let dir = repo();
        let d = dir.path();
        add(d, "a", "a (#1)");
        add(d, "b", "b (#2)");
        commit_file(d, "y", "unrelated (#99)");
        let clone = tempfile::tempdir().unwrap();
        let url = format!("file://{}", d.display().to_string().replace('\\', "/"));
        git(clone.path(), &["clone", "-q", "--depth", "1", &url, "."]);
        let mut changesets = vec![changeset("a"), changeset("b")];
        annotate(clone.path(), ".changeset", &mut changesets).unwrap();
        assert_eq!(changesets[0].origin, None);
        assert_eq!(changesets[1].origin, None);
    }

    #[test]
    fn uncommitted_file_with_a_reused_name_has_no_origin() {
        let dir = repo();
        let d = dir.path();
        add(d, "fix", "old fix (#3)");
        git(d, &["rm", "-q", ".changeset/fix.md"]);
        git(d, &["commit", "-qm", "release"]);
        std::fs::create_dir_all(d.join(".changeset")).unwrap();
        std::fs::write(d.join(".changeset/fix.md"), "---\nx: patch\n---\n").unwrap();
        let mut changesets = vec![changeset("fix")];
        annotate(d, ".changeset", &mut changesets).unwrap();
        assert_eq!(changesets[0].origin, None);
    }

    #[test]
    fn reused_name_from_an_older_branch_commit() {
        let dir = repo();
        let d = dir.path();
        git(d, &["checkout", "-qb", "f"]);
        add(d, "fix", "new fix on feature");
        git(d, &["checkout", "-q", "main"]);
        add(d, "fix", "old fix (#3)");
        add(d, "other", "other (#4)");
        git(d, &["rm", "-q", ".changeset/fix.md"]);
        git(d, &["commit", "-qm", "release"]);
        git(
            d,
            &[
                "merge",
                "-q",
                "--no-ff",
                "f",
                "-m",
                "Merge pull request #10 from o/f",
            ],
        );
        assert_eq!(pr_of(d, "fix"), Some(10));
        assert_eq!(pr_of(d, "other"), Some(4));
    }

    #[test]
    fn names_with_glob_characters() {
        let dir = repo();
        add(dir.path(), "a[1]", "add (#6)");
        add(dir.path(), "a1", "other (#7)");
        assert_eq!(pr_of(dir.path(), "a[1]"), Some(6));
    }
}
