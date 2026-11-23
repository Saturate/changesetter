---
changesetter: minor
type: feat
---

`changesetter check --output json` reports the changesets a branch adds, changes or deletes, with their bumps and text

`check --base` now also fails on a changeset it can't parse, instead of only counting files, refuses a changeset that is a symlink, and no longer skips changesets with non-ASCII characters or quotes in their name. Staged and untracked changesets still count. The check action uses the report to show each changeset's text and bumps in a collapsed section of the PR comment, and needs this release or newer.
