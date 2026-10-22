---
changesetter: patch
---

#### Releases with no name read cleanly

A package with no name, such as a Cargo workspace root versioned through `[workspace.package]`, broke several release strings. The release action's version PR "Changelog preview" came out as `### 0.1.0@CHANGELOG.md` with an empty body, because the empty name was dropped when the release fields were split. The preview now reads the right section and heads it with the bare version. The GitHub Release is titled `v0.1.0` instead of ` v0.1.0`, the release commit says `chore: release 0.1.0` instead of `chore: release @0.1.0`, and `changesetter status` no longer leaves a stray space before the version.
