---
changesetter: minor
type: feat
---

Grouped changelogs and typed changesets

Changesets can carry a conventional `type:` and `scope:` in their front matter (`changesetter add --type fix --scope ui`), or a prefix on the summary like `fix(ui): ...`. Without either, the type follows the bump. Set `group = true` under `[changelog]`, or list your own `[[changelog.groups]]` with `types`, a `match` glob and `skip`, to list each new version's entries under a heading per group. The default stays a flat list. The bump alone still decides the version, and a `breaking` type on a non-major bump warns.

`release --output json` and `status --output json` now list every entry's id, bump, type, scope, group and summary, for releases and none-bump entries alike. The `--dry-run` summary now goes to stderr, so with `--output json` only JSON is on stdout, and a run with nothing to release prints JSON with empty lists instead of nothing.

`changesetter add -m "text"` now writes the message as plain text instead of a `####` heading.

Docs: type and scope, grouped changelogs, the JSON entries, plain `vX.Y.Z` tags for a single configured package, and a recipe for building before committing with `release --no-commit`.
