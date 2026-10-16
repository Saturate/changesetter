---
changesetter: none
---

Release binaries are built for every CLI release again. Tags pushed by the
release workflow never triggered the binary build, so `changesetter@v0.3.1`
shipped without binaries and `actions/install` failed for everyone.
