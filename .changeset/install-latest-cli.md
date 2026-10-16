---
changesetter: none
---

The install action now only picks CLI releases that have binaries when
resolving `latest`, authenticates its API call to avoid shared rate limits,
and reports a clear error when a version has no binary.
