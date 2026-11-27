---
changesetter: minor
type: feat
---

`check` fails when a changeset names a package that doesn't exist, listing the file and name in `unknown_packages`

A typo in a package name used to pass the PR check and then fail `version` on `main`. Checks that passed with such a changeset now exit 1.
