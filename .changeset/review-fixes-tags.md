---
changesetter: patch
type: fix
---

Annotated tags end their message with a newline, so a signed tag (`tag.gpgSign`) verifies instead of having its signature run into the message
