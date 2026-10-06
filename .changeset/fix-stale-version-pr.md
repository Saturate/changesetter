---
changesetter: patch
---

Fix version PR not being created after the first release. The check-merged
step matched historically merged version PRs instead of checking whether HEAD
is the actual merge commit.
