---
changesetter-release-task: minor
changesetter-azure-pipelines: minor
type: feat
---

Version PR mode runs `changesetter version-pr`, so the version PR description is a per-package changelog preview, and the new `versionPrId` output names the PR

It needs changesetter 0.9.0 or newer.

Output `releases[].name` is now the package name instead of the repository folder, and tags are annotated with the package's changelog section (lightweight with `[release] tag_annotated = false`). `post_bump` hooks run in a temporary worktree of HEAD, without `SYSTEM_ACCESSTOKEN`.
