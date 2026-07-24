---
mylib: patch
my-api: minor
---

#### Fixed null handling in response parser

The API was returning null for optional fields. Now defaults to empty
values instead of crashing the deserializer.
