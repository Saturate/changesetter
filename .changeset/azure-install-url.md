---
changesetter-azure-pipelines: patch
changesetter-check-task: patch
changesetter-release-task: patch
type: fix
---

The tasks download the CLI from its `changesetter@v<version>` release instead of a `v<version>` URL that doesn't exist, and verify it against the release's `SHA256SUMS` when one is published
