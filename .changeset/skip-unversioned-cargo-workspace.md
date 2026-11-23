---
changesetter: patch
type: fix
---

Skip Cargo workspace roots without a shared version

A root `Cargo.toml` with `[workspace.package]` but no `version` was detected as a nameless package at `0.0.0`, which `changesetter version` then failed to bump. That root is now skipped and its member crates are still detected. A virtual workspace with a single member crate now counts as a single-package repo, so its tags change from `member@v1.2.3` to `v1.2.3`.
