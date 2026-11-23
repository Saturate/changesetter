---
changesetter-release-task: patch
changesetter-azure-pipelines: patch
type: fix
---

Release pushes are atomic, so a rejected branch push no longer leaves its tags behind on origin
