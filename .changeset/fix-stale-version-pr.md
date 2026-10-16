---
changesetter: patch
---

Fix version PR not being created after the first release. The check-merged
step matched historically merged version PRs instead of checking whether HEAD
is the actual merge commit. The lookup now asks GitHub which PR HEAD belongs
to, and a failed lookup (bad token, rate limit) fails the job instead of
silently skipping the release. The version-PR docs now use
`cancel-in-progress: false`, since cancelling the merge commit's run skips
that release.
