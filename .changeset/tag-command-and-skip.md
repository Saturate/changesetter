---
changesetter: minor
type: feat
---

`changesetter tag` creates the missing tags for the current package versions, and `[tag] skip` keeps matching packages versioned but untagged

`tag --since <rev>` only tags packages whose version changed since `<rev>` and fails on a version collision; `--push` checks the remote and pushes the new tags atomically. `[tag] skip` takes globs over package names and paths, and applies to `release`, `tag` and the `tag` field of `status --output json`.
