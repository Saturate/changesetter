---
changesetter-check-task: patch
type: fix
---

The check task reports a changeset that names an unknown package as such, instead of "No changeset found", and shows the uncovered-package warning in the log and the PR comment

It also passes the `base` input to the CLI as one argument instead of through a shell.
