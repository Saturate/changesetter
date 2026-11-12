//! Links in changelog entries: the pull request and commit an entry came
//! from, and issue references like `#123` or `PROJ-42` turned into links.

use crate::changeset::types::Origin;
use crate::config::ChangelogConfig;

/// A hosted repository's web URL and how it spells its pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub url: String,
    pub forge: Forge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    GitHub,
    GitLab,
    AzureDevOps,
}

impl Repo {
    /// The repo for a web URL, or a remote URL as `git remote get-url` gives
    /// it (`git@host:o/r.git`, `ssh://git@host/o/r`, `https://user@host/o/r`).
    /// Credentials and `.git` are dropped.
    pub fn parse(remote: &str) -> Option<Self> {
        let remote = remote.trim();
        let (host, path) = if let Some(rest) = remote
            .strip_prefix("https://")
            .or_else(|| remote.strip_prefix("http://"))
            .or_else(|| remote.strip_prefix("ssh://"))
        {
            let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
            let (host, path) = rest.split_once('/')?;
            // ssh://git@host:22/o/r
            (
                host.split(':').next().unwrap_or(host).to_string(),
                path.to_string(),
            )
        } else {
            let rest = remote.rsplit_once('@').map_or(remote, |(_, r)| r);
            let (host, path) = rest.split_once(':')?;
            (host.to_string(), path.to_string())
        };
        let mut path = path.trim_matches('/').trim_end_matches(".git").to_string();
        if host.is_empty() || path.is_empty() {
            return None;
        }
        let host_l = host.to_lowercase();
        let forge = if host_l.contains("gitlab") {
            Forge::GitLab
        } else if host_l.ends_with("dev.azure.com") || host_l.ends_with("visualstudio.com") {
            // ssh.dev.azure.com:v3/org/project/repo -> dev.azure.com/org/project/_git/repo
            if let Some(rest) = path.strip_prefix("v3/") {
                let parts: Vec<&str> = rest.splitn(3, '/').collect();
                if let [org, project, repo] = parts[..] {
                    return Some(Self {
                        url: format!("https://dev.azure.com/{org}/{project}/_git/{repo}"),
                        forge: Forge::AzureDevOps,
                    });
                }
            }
            Forge::AzureDevOps
        } else {
            Forge::GitHub
        };
        if forge == Forge::AzureDevOps {
            path = path.trim_end_matches('/').to_string();
        }
        Some(Self {
            url: format!("https://{host}/{path}"),
            forge,
        })
    }

    pub fn pr_url(&self, n: u64) -> String {
        match self.forge {
            Forge::GitHub => format!("{}/pull/{n}", self.url),
            Forge::GitLab => format!("{}/-/merge_requests/{n}", self.url),
            Forge::AzureDevOps => format!("{}/pullrequest/{n}", self.url),
        }
    }

    pub fn commit_url(&self, sha: &str) -> String {
        match self.forge {
            Forge::GitHub | Forge::AzureDevOps => format!("{}/commit/{sha}", self.url),
            Forge::GitLab => format!("{}/-/commit/{sha}", self.url),
        }
    }

    /// The issue URL template for `#123`, where the forge has one.
    fn issue_url(&self) -> Option<String> {
        match self.forge {
            Forge::GitHub => Some(format!("{}/issues/{{id}}", self.url)),
            Forge::GitLab => Some(format!("{}/-/issues/{{id}}", self.url)),
            Forge::AzureDevOps => None,
        }
    }
}

/// How a changelog writes links, from `[changelog]`.
#[derive(Debug, Clone, Default)]
pub struct Linker {
    repo: Option<Repo>,
    /// (prefix, url template with `{id}`), longest prefix first.
    rules: Vec<(String, String)>,
}

impl Linker {
    /// `None` when `links` is off.
    pub fn new(config: &ChangelogConfig) -> Option<Self> {
        if !config.links {
            return None;
        }
        let repo = config.repo_url.as_deref().and_then(Repo::parse);
        let mut rules: Vec<(String, String)> = config
            .issue_links
            .iter()
            .map(|l| (l.prefix.clone(), l.url.clone()))
            .collect();
        if !rules.iter().any(|(p, _)| p == "#") {
            if let Some(url) = repo.as_ref().and_then(Repo::issue_url) {
                rules.push(("#".to_string(), url));
            }
        }
        rules.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        Some(Self { repo, rules })
    }

    /// " ([#24](…/pull/24), [a1b2c3d](…/commit/…))" for an entry's origin,
    /// or the same without links when there's no repo URL. Empty without an
    /// origin.
    pub fn origin_suffix(&self, origin: Option<&Origin>) -> String {
        let Some(origin) = origin else {
            return String::default();
        };
        let short = &origin.commit[..origin.commit.len().min(7)];
        let mut parts = Vec::new();
        match (&self.repo, origin.pr) {
            (Some(repo), Some(n)) => parts.push(format!("[#{n}]({})", repo.pr_url(n))),
            (None, Some(n)) => parts.push(format!("#{n}")),
            _ => {}
        }
        match &self.repo {
            Some(repo) => parts.push(format!("[{short}]({})", repo.commit_url(&origin.commit))),
            None => parts.push(short.to_string()),
        }
        format!(" ({})", parts.join(", "))
    }

    /// Turns issue references into links: a rule's prefix followed by
    /// digits, standing alone (not inside a word, path or URL). Code (fenced
    /// or in backticks) and existing links are left alone.
    pub fn linkify(&self, text: &str) -> String {
        if self.rules.is_empty() {
            return text.to_string();
        }
        let code = crate::changelog::markdown::fenced_lines(text);
        let mut out = String::with_capacity(text.len());
        for (line, is_code) in text.split_inclusive('\n').zip(code) {
            if is_code {
                out.push_str(line);
            } else {
                out.push_str(&self.linkify_line(line));
            }
        }
        out
    }

    fn linkify_line(&self, line: &str) -> String {
        let bytes = line.as_bytes();
        let mut out = String::with_capacity(line.len());
        let mut i = 0;
        let mut in_code = false;
        let mut bracket = 0usize;
        let mut in_url = false;
        let mut in_angle = false;
        while i < line.len() {
            let c = line[i..].chars().next().unwrap_or(' ');
            let len = c.len_utf8();
            match c {
                '`' => in_code = !in_code,
                '[' if !in_code => bracket += 1,
                ']' if !in_code => {
                    bracket = bracket.saturating_sub(1);
                    if line[i + 1..].starts_with('(') {
                        in_url = true;
                    }
                }
                ')' if in_url => in_url = false,
                '<' if !in_code && line[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic()) => {
                    in_angle = true;
                }
                '>' if in_angle => in_angle = false,
                _ => {}
            }
            let free = !in_code && bracket == 0 && !in_url && !in_angle;
            if free && starts_token(bytes, i) {
                if let Some((prefix, url, id_len)) = self.rule_at(&line[i..]) {
                    let id = &line[i + prefix.len()..i + prefix.len() + id_len];
                    let text = &line[i..i + prefix.len() + id_len];
                    out.push_str(&format!("[{text}]({})", url.replace("{id}", id)));
                    i += prefix.len() + id_len;
                    continue;
                }
            }
            out.push(c);
            i += len;
        }
        out
    }

    fn rule_at<'a>(&'a self, rest: &str) -> Option<(&'a str, &'a str, usize)> {
        self.rules.iter().find_map(|(prefix, url)| {
            let after = rest.strip_prefix(prefix.as_str())?;
            let digits = after.bytes().take_while(u8::is_ascii_digit).count();
            let next = after.as_bytes().get(digits).copied();
            (digits > 0 && !next.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_'))
                .then_some((prefix.as_str(), url.as_str(), digits))
        })
    }
}

/// Whether a reference may start at byte `i`: at the start of the line, or
/// after a character that can't be part of a word, path, URL or entity.
fn starts_token(bytes: &[u8], i: usize) -> bool {
    i == 0 || {
        let prev = bytes[i - 1];
        !(prev.is_ascii_alphanumeric() || b"_/-&#.:@=".contains(&prev) || prev >= 0x80)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::IssueLink;

    #[test]
    fn repo_from_remotes() {
        let gh = Repo::parse("git@github.com:Saturate/changesetter.git").unwrap();
        assert_eq!(gh.url, "https://github.com/Saturate/changesetter");
        assert_eq!(gh.forge, Forge::GitHub);
        assert_eq!(
            Repo::parse("https://x-access-token:secret@github.com/o/r.git")
                .unwrap()
                .url,
            "https://github.com/o/r"
        );
        assert_eq!(
            Repo::parse("ssh://git@gitlab.example.com:2222/g/sub/r.git").unwrap(),
            Repo {
                url: "https://gitlab.example.com/g/sub/r".into(),
                forge: Forge::GitLab
            }
        );
        assert_eq!(
            Repo::parse("git@ssh.dev.azure.com:v3/org/proj/repo").unwrap(),
            Repo {
                url: "https://dev.azure.com/org/proj/_git/repo".into(),
                forge: Forge::AzureDevOps
            }
        );
        assert_eq!(
            Repo::parse("https://org@dev.azure.com/org/proj/_git/repo")
                .unwrap()
                .url,
            "https://dev.azure.com/org/proj/_git/repo"
        );
        assert_eq!(Repo::parse("/local/path"), None);
    }

    fn linker(repo: Option<&str>, extra: &[(&str, &str)]) -> Linker {
        let config = ChangelogConfig {
            links: true,
            repo_url: repo.map(str::to_string),
            issue_links: extra
                .iter()
                .map(|(p, u)| IssueLink {
                    prefix: p.to_string(),
                    url: u.to_string(),
                })
                .collect(),
            ..Default::default()
        };
        Linker::new(&config).unwrap()
    }

    #[test]
    fn origin_suffix_with_and_without_repo() {
        let origin = Origin {
            commit: "a1b2c3d4e5f6".into(),
            author: "Ada".into(),
            pr: Some(24),
        };
        assert_eq!(
            linker(Some("https://github.com/o/r"), &[]).origin_suffix(Some(&origin)),
            " ([#24](https://github.com/o/r/pull/24), [a1b2c3d](https://github.com/o/r/commit/a1b2c3d4e5f6))"
        );
        assert_eq!(
            linker(None, &[]).origin_suffix(Some(&origin)),
            " (#24, a1b2c3d)"
        );
        let no_pr = Origin { pr: None, ..origin };
        assert_eq!(linker(None, &[]).origin_suffix(Some(&no_pr)), " (a1b2c3d)");
        assert_eq!(linker(None, &[]).origin_suffix(None), "");
    }

    #[test]
    fn linkify_issue_references() {
        let l = linker(
            Some("https://github.com/o/r"),
            &[("PROJ-", "https://jira.example.com/browse/PROJ-{id}")],
        );
        assert_eq!(
            l.linkify("Fix #12 and PROJ-7, not #x or a#3."),
            "Fix [#12](https://github.com/o/r/issues/12) and [PROJ-7](https://jira.example.com/browse/PROJ-7), not #x or a#3."
        );
        // Code, existing links, URLs and entities stay as they are.
        let left = "`#1` [#2](u) <https://x/#3> https://x/y#4 &#5; ##6 #7a\n```\n#8\n```\n";
        assert_eq!(l.linkify(left), left);
        assert_eq!(l.linkify("(#9)"), "([#9](https://github.com/o/r/issues/9))");
    }

    #[test]
    fn no_default_issue_rule_without_repo() {
        assert_eq!(linker(None, &[]).linkify("Fix #12"), "Fix #12");
    }
}
