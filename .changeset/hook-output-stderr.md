---
changesetter: patch
type: fix
---

`post_bump` hook output goes to stderr, so `release --output json` stays valid JSON when a hook prints, and hooks no longer see `SYSTEM_ACCESSTOKEN`, `GITHUB_TOKEN` or `GH_TOKEN`
