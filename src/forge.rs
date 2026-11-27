//! The pull request APIs `version-pr` talks to: finding the version PR a
//! commit merged, and creating or updating the open one.

use anyhow::Context;

pub const VERSION_BRANCH: &str = "changesetter/version-packages";
pub const VERSION_LABEL: &str = "changesetter:version";

/// A completed version PR whose merge commit is the one being released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedPr {
    pub id: u64,
    /// The PR's merge commit, which gets the tags.
    pub commit: String,
    /// The commit the PR merged into, when the API says. Versions that differ
    /// between it and the merge were released by the PR.
    pub target_commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPr {
    pub id: u64,
    pub url: Option<String>,
}

pub trait Forge {
    /// The version PR merged as `commit`. A failed lookup is an error, never
    /// `None`: reading it as "not a version PR merge" would skip a release
    /// with a green build.
    fn merged_version_pr(&self, commit: &str) -> anyhow::Result<Option<MergedPr>>;
    fn open_version_pr(&self) -> anyhow::Result<Option<OpenPr>>;
    fn create_version_pr(&self, title: &str, body: &str) -> anyhow::Result<OpenPr>;
    fn update_version_pr(&self, pr: &OpenPr, body: &str) -> anyhow::Result<()>;
    /// The newest merged version PR whose merge `commit` contains, for when
    /// `commit` itself isn't one: the run for that merge may have been
    /// cancelled, or failed after tagging part of it. Re-releasing it only
    /// creates what's missing, so offering it again is safe.
    fn recover_merged_version_pr(&self, _commit: &str) -> anyhow::Result<Option<MergedPr>> {
        Ok(None)
    }
    /// Creates a release for an existing tag unless the forge already has
    /// one. Returns its URL when this call created it.
    fn publish_release(&self, _release: &ReleaseInfo) -> anyhow::Result<Option<String>> {
        anyhow::bail!("this provider has no releases to publish")
    }
    /// The longest description the forge accepts, in characters.
    fn body_limit(&self) -> usize;
    /// The commit the pipeline was triggered for, from the CI environment.
    fn ci_commit(&self) -> Option<String>;
}

/// A release page for a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag: String,
    pub title: String,
    pub notes: String,
    pub prerelease: bool,
    pub draft: bool,
}

fn env(name: &str) -> anyhow::Result<String> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Ok(v.trim().to_string()),
        _ => anyhow::bail!("{name} is not set"),
    }
}

fn agent() -> ureq::Agent {
    // The OS trust store, so agents behind a TLS-inspecting proxy with a
    // company CA work like every other tool on them.
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .tls_config(tls)
        .user_agent(concat!("changesetter/", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into()
}

/// Percent-encodes one URL path segment or query value.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

struct Http {
    agent: ureq::Agent,
    auth: String,
    extra: Vec<(&'static str, &'static str)>,
}

impl Http {
    /// Sends a request and returns the JSON body of a 2xx response. Azure
    /// DevOps answers a missing or under-scoped token with a 203 HTML sign-in
    /// page, so a 2xx that isn't JSON is a failure too.
    /// Sends a request and returns the status and the parsed JSON body, if
    /// the body is JSON.
    fn send(
        &self,
        what: &str,
        method: &str,
        url: &str,
        body: Option<&serde_json::Value>,
    ) -> anyhow::Result<(u16, Option<serde_json::Value>, String)> {
        let mut req = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("Authorization", &self.auth);
        if !self.extra.iter().any(|(k, _)| *k == "Accept") {
            req = req.header("Accept", "application/json");
        }
        for (k, v) in &self.extra {
            req = req.header(*k, *v);
        }
        let payload = match body {
            Some(b) => {
                req = req.header("Content-Type", "application/json");
                serde_json::to_vec(b)?
            }
            None => Vec::new(),
        };
        let req = req.body(payload)?;
        let mut res = self
            .agent
            .run(req)
            .with_context(|| format!("{what} failed"))?;
        let status = res.status().as_u16();
        let text = res.body_mut().read_to_string().unwrap_or_default();
        let parsed = serde_json::from_str(&text).ok();
        Ok((status, parsed, text))
    }

    /// The JSON body of a 2xx response. Azure DevOps answers a missing or
    /// under-scoped token with a 203 HTML sign-in page, so a 2xx that isn't
    /// JSON is a failure too.
    fn json(
        &self,
        what: &str,
        method: &str,
        url: &str,
        body: Option<&serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value> {
        let (status, parsed, text) = self.send(what, method, url, body)?;
        match parsed {
            Some(v) if (200..300).contains(&status) && (v.is_object() || v.is_array()) => Ok(v),
            _ => Err(http_error(what, status, &text)),
        }
    }
}

/// A full commit id (SHA-1 or SHA-256). Values from an API response reach
/// git as arguments, so anything else, such as one starting with `-`, is
/// refused rather than passed on.
fn sha(value: &str, what: &str) -> anyhow::Result<String> {
    let ok = matches!(value.len(), 40 | 64) && value.chars().all(|c| c.is_ascii_hexdigit());
    if !ok {
        anyhow::bail!("{what} is not a commit id: {:?}", snippet(value));
    }
    Ok(value.to_lowercase())
}

/// At most 300 characters of an API response, for error messages.
fn snippet(text: &str) -> String {
    text.chars().take(300).collect()
}

fn http_error(what: &str, status: u16, text: &str) -> anyhow::Error {
    anyhow::anyhow!("{what} failed: HTTP {status} {}", snippet(text))
}

fn id_of(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(serde_json::Value::as_u64)
}

// ---------- Azure DevOps ----------

pub struct Azure {
    http: Http,
    /// `{collection}{project}/_apis/git/repositories/{repo}/`
    api: String,
    web: String,
    target_branch: String,
}

impl Azure {
    /// Reads what an Azure Pipelines job provides: `SYSTEM_ACCESSTOKEN`
    /// (mapped in with `env:`), `SYSTEM_COLLECTIONURI`,
    /// `SYSTEM_TEAMPROJECT` and `BUILD_REPOSITORY_ID`.
    pub fn from_env(target_branch: &str) -> anyhow::Result<Self> {
        let token = env("SYSTEM_ACCESSTOKEN").context(
            "map the job token in with `env: SYSTEM_ACCESSTOKEN: $(System.AccessToken)`",
        )?;
        Self::new(
            &env("SYSTEM_COLLECTIONURI")?,
            &env("SYSTEM_TEAMPROJECT")?,
            &env("BUILD_REPOSITORY_ID")?,
            std::env::var("BUILD_REPOSITORY_NAME").ok().as_deref(),
            &token,
            target_branch,
        )
    }

    pub fn new(
        collection: &str,
        project: &str,
        repo_id: &str,
        repo_name: Option<&str>,
        token: &str,
        target_branch: &str,
    ) -> anyhow::Result<Self> {
        let collection = if collection.ends_with('/') {
            collection.to_string()
        } else {
            format!("{collection}/")
        };
        let project = encode(project);
        let api = format!(
            "{collection}{project}/_apis/git/repositories/{}/",
            encode(repo_id)
        );
        let web = format!(
            "{collection}{project}/_git/{}/pullrequest/",
            encode(repo_name.filter(|n| !n.is_empty()).unwrap_or(repo_id))
        );
        Ok(Self {
            http: Http {
                agent: agent(),
                auth: format!("Bearer {token}"),
                extra: Vec::new(),
            },
            api,
            web,
            target_branch: target_branch.to_string(),
        })
    }

    fn source_ref() -> String {
        format!("refs/heads/{VERSION_BRANCH}")
    }

    fn open(&self, id: u64) -> OpenPr {
        OpenPr {
            id,
            url: Some(format!("{}{id}", self.web)),
        }
    }
}

impl Forge for Azure {
    // Looks the commit up by merge commit instead of listing completed
    // version PRs: the newest completed one stays "merged" forever, so every
    // later push would look like a version PR merge.
    fn merged_version_pr(&self, commit: &str) -> anyhow::Result<Option<MergedPr>> {
        let body = serde_json::json!({
            "queries": [{ "type": "lastMergeCommit", "items": [commit] }],
        });
        let data = self.http.json(
            "Pull request query",
            "POST",
            &format!("{}pullrequestquery?api-version=7.1", self.api),
            Some(&body),
        )?;
        let Some(results) = data.get("results").and_then(|r| r.as_array()) else {
            anyhow::bail!(
                "Pull request query returned no results array: {}",
                snippet(&data.to_string())
            );
        };
        let source = Self::source_ref();
        let found = results
            .iter()
            .filter_map(|by_commit| by_commit.as_object())
            .flat_map(|m| m.iter())
            .filter(|(sha, _)| sha.eq_ignore_ascii_case(commit))
            .filter_map(|(_, list)| list.as_array())
            .flatten()
            .find(|pr| {
                pr["sourceRefName"] == source.as_str()
                    && pr["status"] == "completed"
                    && pr["lastMergeCommit"]["commitId"]
                        .as_str()
                        .is_some_and(|c| c.eq_ignore_ascii_case(commit))
            });
        let Some(pr) = found else { return Ok(None) };
        let Some(id) = id_of(pr, "pullRequestId") else {
            return Ok(None);
        };
        let target_commit = pr["lastMergeTargetCommit"]["commitId"]
            .as_str()
            .map(|c| sha(c, "lastMergeTargetCommit"))
            .transpose()?;
        Ok(Some(MergedPr {
            id,
            commit: commit.to_lowercase(),
            target_commit,
        }))
    }

    fn open_version_pr(&self) -> anyhow::Result<Option<OpenPr>> {
        let url = format!(
            "{}pullrequests?searchCriteria.status=active&searchCriteria.sourceRefName={}&$top=1&api-version=7.1",
            self.api,
            encode(&Self::source_ref())
        );
        let data = self
            .http
            .json("Open version PR lookup", "GET", &url, None)?;
        Ok(data["value"]
            .as_array()
            .and_then(|v| v.first())
            .and_then(|pr| id_of(pr, "pullRequestId"))
            .map(|id| self.open(id)))
    }

    fn create_version_pr(&self, title: &str, body: &str) -> anyhow::Result<OpenPr> {
        let payload = serde_json::json!({
            "sourceRefName": Self::source_ref(),
            "targetRefName": format!("refs/heads/{}", self.target_branch),
            "title": title,
            "description": body,
            "labels": [{ "name": VERSION_LABEL }],
        });
        let data = self.http.json(
            "Creating version PR",
            "POST",
            &format!("{}pullrequests?api-version=7.1", self.api),
            Some(&payload),
        )?;
        let id = id_of(&data, "pullRequestId").ok_or_else(|| {
            anyhow::anyhow!(
                "Creating version PR returned no id: {}",
                snippet(&data.to_string())
            )
        })?;
        Ok(self.open(id))
    }

    fn update_version_pr(&self, pr: &OpenPr, body: &str) -> anyhow::Result<()> {
        self.http.json(
            &format!("Updating version PR #{}", pr.id),
            "PATCH",
            &format!("{}pullrequests/{}?api-version=7.1", self.api, pr.id),
            Some(&serde_json::json!({ "description": body })),
        )?;
        Ok(())
    }

    fn body_limit(&self) -> usize {
        crate::release::pr_body::AZURE_LIMIT
    }

    fn ci_commit(&self) -> Option<String> {
        std::env::var("BUILD_SOURCEVERSION").ok()
    }
}

// ---------- GitHub ----------

pub struct GitHub {
    http: Http,
    /// `{api}/repos/{owner}/{repo}/`
    api: String,
    owner: String,
    target_branch: String,
}

impl GitHub {
    /// Reads `GITHUB_TOKEN` (or `GH_TOKEN`), `GITHUB_REPOSITORY` and
    /// `GITHUB_API_URL`, which GitHub Actions sets except for the token.
    pub fn from_env(target_branch: &str) -> anyhow::Result<Self> {
        let token = env("GITHUB_TOKEN")
            .or_else(|_| env("GH_TOKEN"))
            .context("set GITHUB_TOKEN (or GH_TOKEN) to a token that can write pull requests")?;
        let api = env("GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".to_string());
        Self::new(&api, &env("GITHUB_REPOSITORY")?, &token, target_branch)
    }

    pub fn new(
        api: &str,
        repository: &str,
        token: &str,
        target_branch: &str,
    ) -> anyhow::Result<Self> {
        let Some((owner, name)) = repository.split_once('/') else {
            anyhow::bail!("GITHUB_REPOSITORY must be owner/name, got {repository:?}");
        };
        Ok(Self {
            http: Http {
                agent: agent(),
                auth: format!("Bearer {token}"),
                extra: vec![
                    ("Accept", "application/vnd.github+json"),
                    ("X-GitHub-Api-Version", "2022-11-28"),
                ],
            },
            api: format!(
                "{}/repos/{}/{}/",
                api.trim_end_matches('/'),
                encode(owner),
                encode(name)
            ),
            owner: owner.to_string(),
            target_branch: target_branch.to_string(),
        })
    }

    /// Whether the release list has a release for `tag`, drafts included.
    /// Newest first, so a draft made by a recent run is on the first pages.
    fn draft_release_exists(&self, tag: &str) -> anyhow::Result<bool> {
        for page in 1..=10 {
            let data = self.http.json(
                "Listing GitHub Releases",
                "GET",
                &format!("{}releases?per_page=100&page={page}", self.api),
                None,
            )?;
            let Some(list) = data.as_array() else {
                return Ok(false);
            };
            if list.iter().any(|r| r["tag_name"] == tag) {
                return Ok(true);
            }
            if list.len() < 100 {
                return Ok(false);
            }
        }
        Ok(false)
    }

    fn open(pr: &serde_json::Value) -> Option<OpenPr> {
        Some(OpenPr {
            id: id_of(pr, "number")?,
            url: pr["html_url"].as_str().map(str::to_string),
        })
    }
}

fn has_version_label(pr: &serde_json::Value) -> bool {
    pr["labels"]
        .as_array()
        .is_some_and(|l| l.iter().any(|l| l["name"] == VERSION_LABEL))
}

impl Forge for GitHub {
    // The PRs associated with the commit rather than a search over merged
    // PRs: the search index can lag right after the merge.
    fn merged_version_pr(&self, commit: &str) -> anyhow::Result<Option<MergedPr>> {
        let data = self.http.json(
            "Pull request lookup",
            "GET",
            &format!("{}commits/{}/pulls", self.api, encode(commit)),
            None,
        )?;
        let Some(prs) = data.as_array() else {
            anyhow::bail!(
                "Pull request lookup returned no list: {}",
                snippet(&data.to_string())
            );
        };
        Ok(prs
            .iter()
            .find(|pr| {
                pr["merge_commit_sha"]
                    .as_str()
                    .is_some_and(|c| c.eq_ignore_ascii_case(commit))
                    && !pr["merged_at"].is_null()
                    && pr["head"]["ref"] == VERSION_BRANCH
                    && has_version_label(pr)
            })
            .and_then(|pr| {
                Some(MergedPr {
                    id: id_of(pr, "number")?,
                    commit: commit.to_lowercase(),
                    // GitHub's base.sha is the base tip when the PR was last
                    // synced, not the merge's parent; the caller uses ^1.
                    target_commit: None,
                })
            }))
    }

    fn open_version_pr(&self) -> anyhow::Result<Option<OpenPr>> {
        let url = format!(
            "{}pulls?state=open&head={}&per_page=1",
            self.api,
            encode(&format!("{}:{VERSION_BRANCH}", self.owner))
        );
        let data = self
            .http
            .json("Open version PR lookup", "GET", &url, None)?;
        Ok(data.as_array().and_then(|v| v.first()).and_then(Self::open))
    }

    fn create_version_pr(&self, title: &str, body: &str) -> anyhow::Result<OpenPr> {
        let data = self.http.json(
            "Creating version PR",
            "POST",
            &format!("{}pulls", self.api),
            Some(&serde_json::json!({
                "title": title,
                "head": VERSION_BRANCH,
                "base": self.target_branch,
                "body": body,
            })),
        )?;
        let pr = Self::open(&data).ok_or_else(|| {
            anyhow::anyhow!(
                "Creating version PR returned no number: {}",
                snippet(&data.to_string())
            )
        })?;
        // The label is how a later run recognises the merge. 422 is "already
        // exists".
        let what = "Creating the changesetter:version label";
        let (status, _, text) = self.http.send(
            what,
            "POST",
            &format!("{}labels", self.api),
            Some(&serde_json::json!({
                "name": VERSION_LABEL,
                "color": "6e40c9",
                "description": "Changesetter version PR",
            })),
        )?;
        if !(200..300).contains(&status) && status != 422 {
            return Err(http_error(what, status, &text));
        }
        self.http.json(
            &format!("Labelling version PR #{}", pr.id),
            "POST",
            &format!("{}issues/{}/labels", self.api, pr.id),
            Some(&serde_json::json!({ "labels": [VERSION_LABEL] })),
        )?;
        Ok(pr)
    }

    fn update_version_pr(&self, pr: &OpenPr, body: &str) -> anyhow::Result<()> {
        self.http.json(
            &format!("Updating version PR #{}", pr.id),
            "PATCH",
            &format!("{}pulls/{}", self.api, pr.id),
            Some(&serde_json::json!({ "body": body })),
        )?;
        self.http.json(
            &format!("Labelling version PR #{}", pr.id),
            "POST",
            &format!("{}issues/{}/labels", self.api, pr.id),
            Some(&serde_json::json!({ "labels": [VERSION_LABEL] })),
        )?;
        Ok(())
    }

    fn recover_merged_version_pr(&self, commit: &str) -> anyhow::Result<Option<MergedPr>> {
        let url = format!(
            "{}pulls?state=closed&head={}&sort=updated&direction=desc&per_page=20",
            self.api,
            encode(&format!("{}:{VERSION_BRANCH}", self.owner))
        );
        let data = self
            .http
            .json("Merged version PR lookup", "GET", &url, None)?;
        let last = data.as_array().and_then(|prs| {
            prs.iter()
                .filter(|pr| pr["merged_at"].is_string() && has_version_label(pr))
                // RFC 3339 in UTC, so the text order is the time order.
                .max_by_key(|pr| pr["merged_at"].as_str().unwrap_or_default().to_string())
        });
        let Some(pr) = last else { return Ok(None) };
        let (Some(id), Some(sha)) = (id_of(pr, "number"), pr["merge_commit_sha"].as_str()) else {
            return Ok(None);
        };
        // Asked of the API rather than local history, which a shallow
        // checkout doesn't have. "behind" or "diverged" means the merge isn't
        // in `commit`, as on a branch that was reset past it.
        // A 404 is GitHub's answer for commits with no common history, as
        // after a history rewrite: then the merge isn't in `commit` either.
        let merge = self::sha(sha, "merge_commit_sha")?;
        let what = "Version PR merge ancestry check";
        let url = format!("{}compare/{merge}...{}", self.api, encode(commit));
        let (status, compare, text) = self.http.send(what, "GET", &url, None)?;
        let contained = match status {
            200..=299 => matches!(
                compare.as_ref().and_then(|c| c["status"].as_str()),
                Some("ahead" | "identical")
            ),
            404 => false,
            _ => return Err(http_error(what, status, &text)),
        };
        Ok(contained.then_some(MergedPr {
            id,
            commit: merge,
            target_commit: None,
        }))
    }

    fn publish_release(&self, release: &ReleaseInfo) -> anyhow::Result<Option<String>> {
        let what = format!("Looking up GitHub Release {}", release.tag);
        let url = format!("{}releases/tags/{}", self.api, encode(&release.tag));
        let (status, _, text) = self.http.send(&what, "GET", &url, None)?;
        match status {
            200..=299 => return Ok(None),
            404 => {}
            _ => return Err(http_error(&what, status, &text)),
        }
        // releases/tags/{tag} only finds published releases; a draft for the
        // tag is only in the release list.
        if self.draft_release_exists(&release.tag)? {
            return Ok(None);
        }
        let data = self.http.json(
            &format!("Creating GitHub Release {}", release.tag),
            "POST",
            &format!("{}releases", self.api),
            Some(&serde_json::json!({
                "tag_name": release.tag,
                "name": release.title,
                "body": release.notes,
                "draft": release.draft,
                "prerelease": release.prerelease,
            })),
        )?;
        Ok(Some(
            data["html_url"].as_str().unwrap_or_default().to_string(),
        ))
    }

    fn body_limit(&self) -> usize {
        crate::release::pr_body::GITHUB_LIMIT
    }

    fn ci_commit(&self) -> Option<String> {
        std::env::var("GITHUB_SHA").ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone)]
    struct Seen {
        method: String,
        path: String,
        auth: String,
        body: serde_json::Value,
    }

    type Reply = fn(&Seen) -> (u16, String);

    /// A one-request-per-connection HTTP server on localhost.
    fn serve(reply: Reply) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                let (mut len, mut auth) = (0, String::new());
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    let (k, v) = h.split_once(':').unwrap();
                    match k.to_ascii_lowercase().as_str() {
                        "content-length" => len = v.trim().parse().unwrap(),
                        "authorization" => auth = v.trim().to_string(),
                        _ => {}
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let s = Seen {
                    method,
                    path,
                    auth,
                    body: serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
                };
                let (status, text) = reply(&s);
                log.lock().unwrap().push(s);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        (format!("http://{addr}/"), seen)
    }

    fn azure(url: &str) -> Azure {
        Azure::new(url, "My Project", "repo-id", Some("web"), "tok", "main").unwrap()
    }

    #[test]
    fn azure_finds_the_completed_version_pr_for_the_merge_commit() {
        let (url, seen) = serve(|_| {
            (
                200,
                serde_json::json!({"results": [{"abc123": [
                    {"pullRequestId": 5, "status": "active", "sourceRefName": "refs/heads/changesetter/version-packages", "lastMergeCommit": {"commitId": "abc123"}},
                    {"pullRequestId": 6, "status": "completed", "sourceRefName": "refs/heads/feature", "lastMergeCommit": {"commitId": "abc123"}},
                    {"pullRequestId": 7, "status": "completed", "sourceRefName": "refs/heads/changesetter/version-packages", "lastMergeCommit": {"commitId": "ABC123"}, "lastMergeTargetCommit": {"commitId": "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD"}},
                ]}]})
                .to_string(),
            )
        });
        let pr = azure(&url).merged_version_pr("abc123").unwrap();
        assert_eq!(
            pr,
            Some(MergedPr {
                id: 7,
                commit: "abc123".into(),
                target_commit: Some("dddddddddddddddddddddddddddddddddddddddd".into())
            })
        );
        let s = seen.lock().unwrap()[0].clone();
        assert_eq!(s.method, "POST");
        assert_eq!(
            s.path,
            "/My%20Project/_apis/git/repositories/repo-id/pullrequestquery?api-version=7.1"
        );
        assert_eq!(s.auth, "Bearer tok");
        assert_eq!(
            s.body,
            serde_json::json!({"queries": [{"type": "lastMergeCommit", "items": ["abc123"]}]})
        );
    }

    #[test]
    fn azure_errors_are_failures_not_misses() {
        let (url, _) = serve(|_| (203, "<html>Sign in</html>".into()));
        let err = azure(&url)
            .merged_version_pr("abc")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Pull request query failed: HTTP 203 <html>"),
            "{err}"
        );

        let (url, _) = serve(|_| (401, r#"{"message":"TF400813"}"#.into()));
        let err = azure(&url).open_version_pr().unwrap_err().to_string();
        assert!(err.contains("HTTP 401"), "{err}");

        let (url, _) = serve(|_| (200, r#"{"count":0}"#.into()));
        let err = azure(&url)
            .merged_version_pr("abc")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no results array"), "{err}");
    }

    #[test]
    fn azure_creates_and_updates_the_version_pr() {
        let (url, seen) = serve(|s| match s.method.as_str() {
            "GET" => (200, r#"{"value":[]}"#.into()),
            "POST" => (201, r#"{"pullRequestId":8}"#.into()),
            _ => (200, r#"{"pullRequestId":8}"#.into()),
        });
        let a = azure(&url);
        assert_eq!(a.open_version_pr().unwrap(), None);
        let pr = a.create_version_pr("Next Release", "body").unwrap();
        assert_eq!(
            pr.url.as_deref(),
            Some(format!("{url}My%20Project/_git/web/pullrequest/8").as_str())
        );
        a.update_version_pr(&pr, "new body").unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(
            seen[0].path,
            "/My%20Project/_apis/git/repositories/repo-id/pullrequests?searchCriteria.status=active&searchCriteria.sourceRefName=refs%2Fheads%2Fchangesetter%2Fversion-packages&$top=1&api-version=7.1"
        );
        assert_eq!(seen[1].body["targetRefName"], "refs/heads/main");
        assert_eq!(
            seen[1].body["labels"],
            serde_json::json!([{"name": "changesetter:version"}])
        );
        assert_eq!(seen[2].method, "PATCH");
        assert_eq!(
            seen[2].path,
            "/My%20Project/_apis/git/repositories/repo-id/pullrequests/8?api-version=7.1"
        );
        assert_eq!(seen[2].body, serde_json::json!({"description": "new body"}));
    }

    #[test]
    fn github_matches_merge_sha_branch_and_label() {
        let (url, seen) = serve(|_| {
            (
                200,
                serde_json::json!([
                    {"number": 3, "merge_commit_sha": "abc", "merged_at": "x", "head": {"ref": "feature"}, "labels": [{"name": "changesetter:version"}]},
                    {"number": 4, "merge_commit_sha": "abc", "merged_at": null, "head": {"ref": "changesetter/version-packages"}, "labels": [{"name": "changesetter:version"}]},
                    {"number": 5, "merge_commit_sha": "abc", "merged_at": "x", "head": {"ref": "changesetter/version-packages"}, "labels": [{"name": "changesetter:version"}]},
                ])
                .to_string(),
            )
        });
        let gh = GitHub::new(url.trim_end_matches('/'), "acme/repo", "tok", "main").unwrap();
        assert_eq!(
            gh.merged_version_pr("abc").unwrap(),
            Some(MergedPr {
                id: 5,
                commit: "abc".into(),
                target_commit: None
            })
        );
        assert_eq!(
            seen.lock().unwrap()[0].path,
            "/repos/acme/repo/commits/abc/pulls"
        );
    }

    #[test]
    fn github_creates_the_pr_and_labels_it() {
        let (url, seen) = serve(|s| match (s.method.as_str(), s.path.as_str()) {
            ("GET", _) => (200, "[]".into()),
            ("POST", "/repos/acme/repo/pulls") => {
                (201, r#"{"number":12,"html_url":"https://gh/pr/12"}"#.into())
            }
            ("POST", "/repos/acme/repo/labels") => (422, r#"{"message":"already_exists"}"#.into()),
            _ => (200, "[]".into()),
        });
        let gh = GitHub::new(&url, "acme/repo", "tok", "main").unwrap();
        assert_eq!(gh.open_version_pr().unwrap(), None);
        let pr = gh.create_version_pr("Version Packages", "b").unwrap();
        assert_eq!(
            pr,
            OpenPr {
                id: 12,
                url: Some("https://gh/pr/12".into())
            }
        );
        let seen = seen.lock().unwrap();
        assert_eq!(
            seen[0].path,
            "/repos/acme/repo/pulls?state=open&head=acme%3Achangesetter%2Fversion-packages&per_page=1"
        );
        assert_eq!(seen[1].body["head"], "changesetter/version-packages");
        assert_eq!(seen[1].body["base"], "main");
        assert_eq!(seen[2].path, "/repos/acme/repo/labels");
        assert_eq!(seen[2].body["name"], "changesetter:version");
        assert_eq!(seen[3].path, "/repos/acme/repo/issues/12/labels");
        assert_eq!(
            seen[3].body,
            serde_json::json!({"labels": ["changesetter:version"]})
        );
    }

    #[test]
    fn github_recovery_takes_the_newest_merged_version_pr_in_history() {
        let (url, seen) = serve(|s| {
            if s.path.contains("/compare/") {
                let status = if s
                    .path
                    .contains("/compare/abababababababababababababababababababab...")
                {
                    "ahead"
                } else {
                    "diverged"
                };
                return (200, serde_json::json!({"status": status}).to_string());
            }
            (
                200,
                serde_json::json!([
                    {"number": 1, "merged_at": "2026-01-01T00:00:00Z", "merge_commit_sha": "m1", "labels": [{"name": "changesetter:version"}]},
                    {"number": 2, "merged_at": "2026-02-01T00:00:00Z", "merge_commit_sha": "ABABABABABABABABABABABABABABABABABABABAB", "labels": [{"name": "changesetter:version"}]},
                    {"number": 3, "merged_at": null, "merge_commit_sha": "m3", "labels": [{"name": "changesetter:version"}]},
                    {"number": 4, "merged_at": "2026-03-01T00:00:00Z", "merge_commit_sha": "m4", "labels": []},
                ])
                .to_string(),
            )
        });
        let gh = GitHub::new(&url, "acme/repo", "tok", "main").unwrap();
        assert_eq!(
            gh.recover_merged_version_pr("head").unwrap(),
            Some(MergedPr {
                id: 2,
                commit: "abababababababababababababababababababab".into(),
                target_commit: None
            })
        );
        assert_eq!(
            seen.lock().unwrap()[1].path,
            "/repos/acme/repo/compare/abababababababababababababababababababab...head"
        );
    }

    #[test]
    fn github_recovery_skips_a_merge_outside_history() {
        let (url, _) = serve(|s| {
            if s.path.contains("/compare/") {
                return (200, r#"{"status":"behind"}"#.into());
            }
            (200, r#"[{"number":2,"merged_at":"2026-02-01T00:00:00Z","merge_commit_sha":"abababababababababababababababababababab","labels":[{"name":"changesetter:version"}]}]"#.into())
        });
        let gh = GitHub::new(&url, "acme/repo", "tok", "main").unwrap();
        assert_eq!(gh.recover_merged_version_pr("head").unwrap(), None);
    }

    fn release(tag: &str) -> ReleaseInfo {
        ReleaseInfo {
            tag: tag.into(),
            title: "pkg v2.0.0-rc.0".into(),
            notes: "- Notes".into(),
            prerelease: true,
            draft: false,
        }
    }

    #[test]
    fn github_publishes_a_release_only_when_missing() {
        let (url, seen) = serve(|s| match (s.method.as_str(), s.path.as_str()) {
            ("GET", "/repos/acme/repo/releases/tags/v1.0.0") => (200, "{}".into()),
            ("GET", "/repos/acme/repo/releases?per_page=100&page=1") => {
                (200, r#"[{"tag_name":"v0.9.0","draft":true}]"#.into())
            }
            ("GET", _) => (404, r#"{"message":"Not Found"}"#.into()),
            _ => (201, r#"{"html_url":"https://gh/rel/2"}"#.into()),
        });
        let gh = GitHub::new(&url, "acme/repo", "tok", "main").unwrap();
        assert_eq!(gh.publish_release(&release("v1.0.0")).unwrap(), None);
        assert_eq!(
            gh.publish_release(&release("@s/a@v2.0.0-rc.0"))
                .unwrap()
                .as_deref(),
            Some("https://gh/rel/2")
        );
        let seen = seen.lock().unwrap();
        assert_eq!(
            seen[1].path,
            "/repos/acme/repo/releases/tags/%40s%2Fa%40v2.0.0-rc.0"
        );
        assert_eq!(
            seen[2].path,
            "/repos/acme/repo/releases?per_page=100&page=1"
        );
        assert_eq!(
            seen[3].body,
            serde_json::json!({"tag_name": "@s/a@v2.0.0-rc.0", "name": "pkg v2.0.0-rc.0", "body": "- Notes", "draft": false, "prerelease": true})
        );
    }

    #[test]
    fn github_release_lookup_errors_fail() {
        let (url, _) = serve(|_| (500, "boom".into()));
        let gh = GitHub::new(&url, "acme/repo", "tok", "main").unwrap();
        let err = gh.publish_release(&release("v1")).unwrap_err().to_string();
        assert!(err.contains("HTTP 500"), "{err}");
    }

    #[test]
    fn github_finds_an_existing_draft_in_the_release_list() {
        let (url, seen) = serve(|s| match s.path.as_str() {
            "/repos/acme/repo/releases?per_page=100&page=1" => {
                (200, r#"[{"tag_name":"v0.9.0","draft":true}]"#.into())
            }
            _ => (404, "{}".into()),
        });
        let gh = GitHub::new(&url, "acme/repo", "tok", "main").unwrap();
        assert_eq!(gh.publish_release(&release("v0.9.0")).unwrap(), None);
        assert!(seen.lock().unwrap().iter().all(|s| s.method == "GET"));
    }

    #[test]
    fn commit_ids_from_the_api_are_validated() {
        assert!(sha("--upload-pack=x", "merge_commit_sha").is_err());
        assert!(sha("abc", "merge_commit_sha").is_err());
        assert_eq!(sha(&"A".repeat(40), "x").unwrap(), "a".repeat(40));
        assert!(sha(&"a".repeat(64), "x").is_ok());

        let (url, _) = serve(|_| {
            (
                200,
                serde_json::json!({"results": [{"abc": [
                    {"pullRequestId": 7, "status": "completed", "sourceRefName": "refs/heads/changesetter/version-packages", "lastMergeCommit": {"commitId": "abc"}, "lastMergeTargetCommit": {"commitId": "-x"}},
                ]}]})
                .to_string(),
            )
        });
        let err = azure(&url)
            .merged_version_pr("abc")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("lastMergeTargetCommit is not a commit id"),
            "{err}"
        );
    }

    #[test]
    fn github_recovery_treats_unrelated_history_as_not_contained() {
        let (url, _) = serve(|s| {
            if s.path.contains("/compare/") {
                return (404, r#"{"message":"No common ancestor"}"#.into());
            }
            (200, serde_json::json!([{"number": 2, "merged_at": "2026-02-01T00:00:00Z", "merge_commit_sha": "c".repeat(40), "labels": [{"name": "changesetter:version"}]}]).to_string())
        });
        let gh = GitHub::new(&url, "acme/repo", "tok", "main").unwrap();
        assert_eq!(gh.recover_merged_version_pr("head").unwrap(), None);
    }
}
