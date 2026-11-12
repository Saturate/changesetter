---
changesetter: minor
type: feat
---

Pull request, commit and issue links in changelog entries

`[changelog] links = true` ends each grouped changelog entry with the pull request and commit it came from, like `([#31](…/pull/31), [63c42f2](…/commit/…))`, and turns issue references such as `#12` into links. `[[changelog.issue_links]]` adds other trackers (`prefix = "GAME-"`, `url = "https://jira…/GAME-{id}"`). Links point at the `origin` remote, or `repo_url`, and understand GitHub, GitLab and Azure DevOps.

Where each changeset came from is read from local git, with no network calls: the commit that added the file, its author, and the pull request number from a squash-merge subject or the merge commit that brought it in. `release` and `status --output json` now include `commit`, `author` and `pr` for every entry.
