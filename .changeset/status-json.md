---
changesetter: minor
---

#### `status --output json`

`changesetter status --output json` prints pending changesets, each
package's current version and tag (and whether that tag exists), and the
releases that would be made. CI integrations use it instead of globbing
`.changeset/` or building tag names by hand.
