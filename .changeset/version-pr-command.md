---
changesetter: minor
type: feat
---

`changesetter version-pr --provider azure|github` runs the version-PR loop: it tags what a merged version PR released, then creates or updates the next version PR

Pipelines that call the binary directly no longer have to script the PR API calls. The PR description is a changelog preview per package, with cascade-only bumps on one line, cut to Azure DevOps' 4000-character limit when needed.
