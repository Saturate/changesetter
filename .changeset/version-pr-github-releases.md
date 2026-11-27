---
changesetter: minor
type: feat
---

`version-pr --provider github` creates GitHub Releases and finishes a version PR merge whose run was cancelled, and `tag` annotates tags with the version's changelog section

The release action's version-PR mode now runs `changesetter version-pr` instead of its own scripts. `--no-github-release` and `--draft` match the action's inputs.
