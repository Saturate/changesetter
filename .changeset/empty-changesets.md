---
changesetter: minor
type: feat
---

Empty changesets (`---` followed directly by `---`) are valid, as in `@changesets/cli`, and `add --empty` writes one

`check` counts an empty changeset, and `version` and `release` delete it without bumping anything or writing its body to a changelog. A changeset with only `type:` or `scope:` and no package is still an error.
