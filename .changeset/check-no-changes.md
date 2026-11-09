---
changesetter: patch
---

#### `check --base` passes when nothing changed

`changesetter check --base <ref>` failed with "no changeset files found" even when the branch had no changes since `<ref>`, for example a local gate running on `main` against `origin/main`. With no commits, staged, unstaged or untracked files since the base, there's nothing to describe, so the check now passes and says so. Any change still needs a changeset, including one outside `.changeset/`.
