---
changesetter: patch
---

#### None-bump changesets stay out of the release notes

A `none` changeset for a package that also gets a real bump was folded into that release's changelog entry, tag message and GitHub release notes. It now goes under the none-bump heading ("Internal" by default), or nowhere with `none_bump = "omit"`, as the docs describe. The version PR's changelog preview also shows the changelog on a first release, when CHANGELOG.md is a new file.
