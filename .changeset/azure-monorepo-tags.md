---
changesetter-release-task: minor
changesetter-azure-pipelines: minor
type: feat
---

Version-PR releases tag every package the merge bumped, so monorepos without a root version release instead of failing

The task runs `changesetter tag --since <merge target> --push`, which follows `[tag] format` and `[tag] skip` and pushes the tags atomically. It needs changesetter 0.9.0 or newer.
