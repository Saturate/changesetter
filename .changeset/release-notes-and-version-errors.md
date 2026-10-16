---
changesetter: patch
---

Fix the release action's notes and error handling. Release notes kept only
part of a version's section when it was the last one in CHANGELOG.md (every
first release), and the version could match a longer one (`1.0.1` in
`1.0.10`). A failing `changesetter version`, such as a `post_bump` hook, no
longer opens a half-bumped version PR; the job now fails instead, which
includes a malformed changeset file that used to give an empty version PR.
