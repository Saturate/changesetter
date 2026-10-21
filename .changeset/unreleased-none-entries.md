---
changesetter: patch
---

#### None-bump entries without a release go under Unreleased

A release run with only `none` changesets wrote an `### Internal` section with no version heading above it, so it ended up inside the next version's section and its release notes. With `per_package = true`, `none` entries for packages that weren't being released (including a repo-wide `default: none`) were dropped. These entries now go under an `## Unreleased` heading at the top of the right changelog (every package's changelog for `default: none` in per-package mode), and the next release written to that changelog folds them into its version section. Release notes never treat `## Unreleased` as a version, and `none_bump = "omit"` still writes nothing.
