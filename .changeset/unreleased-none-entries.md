---
changesetter: patch
---

#### None-bump entries without a release go under Unreleased

A release run with only `none` changesets wrote an `### Internal` section with no version heading above it, so it ended up inside the next version's section and its release notes. With `per_package = true`, `none` entries for packages that weren't being released (including a repo-wide `default: none`) were dropped. These entries now go under an `## Unreleased` heading at the top of the right changelog (every package's changelog for `default: none` in per-package mode, but not a private workspace root), and the next release written to that changelog folds them into its version section, tag message and GitHub release notes. A none-only run commits as `chore: add unreleased changelog notes` and creates no tags. New sections always go at the top, even in a changelog without a title, Keep a Changelog's `## [Unreleased]` spelling and CRLF line endings are kept, and release notes stop at Unreleased headings, `## [1.0.0]` or `## v1.0.0` headings and version link references. `none_bump = "omit"` still writes nothing.
