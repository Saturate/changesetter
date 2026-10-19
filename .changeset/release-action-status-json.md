---
changesetter: none
---

The release action's version-PR mode now tags every package a version PR
bumped, in the configured tag format, finishes a release a cancelled run left
halfway, and finds pending changesets through the CLI. Direct mode now pushes
its release commit and fails when `changesetter release` fails.
