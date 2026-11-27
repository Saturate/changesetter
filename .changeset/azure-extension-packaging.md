---
changesetter-azure-pipelines: patch
changesetter-check-task: patch
changesetter-release-task: patch
type: fix
---

The extension package includes the installer both tasks load and their npm dependencies, which `npm run build` didn't put in each task folder
