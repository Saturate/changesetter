---
changesetter: patch
---

#### `check --base` counts changesets you haven't committed yet

`changesetter check --base <ref>` used to compare commits only, so a pre-commit hook or local gate couldn't find the changeset it was about to commit. It now counts changesets added since the branch left `<ref>` whether they are committed, staged or untracked. Files matched by `.gitignore` don't count, and neither do changesets deleted from the working tree. CI checkouts are clean, so results there are unchanged.

The repo now ships a `LICENSE` file with the MIT terms that `Cargo.toml` already declared, and the published crate includes it.
