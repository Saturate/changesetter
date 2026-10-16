---
changesetter: patch
---

Reading the changeset directory no longer follows symlinked subdirectories, so
a link like `.changeset/loop -> ..` can't crash changesetter with a stack
overflow. Symlinked `.md` files are still read. `changeset_dir` in
`changesetter.toml` must now be a relative path inside the repository:
absolute paths and `..` are rejected at load with a clear error, and a
trailing slash (`.changeset/`) is accepted and normalised.
