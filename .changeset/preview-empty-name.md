---
changesetter: patch
---

#### Version PR preview works for a release with no name

In a repo whose only package has no name, such as a Cargo workspace root versioned through `[workspace.package]`, the release action's "Changelog preview" came out as `### 0.1.0@CHANGELOG.md` with an empty body. The empty name was dropped when the release fields were split, which shifted the version and changelog path into the wrong places. The preview now reads the right section and heads it with the bare version.
