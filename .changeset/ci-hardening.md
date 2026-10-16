---
changesetter: none
---

CI now fails a release run when `changesetter release` fails instead of
reporting success, rejects a stale Cargo.lock, and lints workflows and
action scripts.
