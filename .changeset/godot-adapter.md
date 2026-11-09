---
changesetter: minor
---

#### Godot projects

changesetter now versions Godot projects. It finds `project.godot` and reads the name from `config/name` and the version from `config/version`, both under `[application]`. A release replaces only the `config/version` line and leaves the rest of the file byte for byte, line endings included. Godot leaves the key out until a version is set. Auto-detection skips projects without it, so a demo project inside a library repo doesn't turn into a package. To start versioning one, list it under `[[package]]` with `type = "godot"`: it reads as `0.0.0`, and the first release inserts the line right after `config/name`. A version that isn't `x.y.z`, such as `"1.0"`, is an error naming the file. If your repo has a versioned `project.godot` you don't release, add its name to `ignore`.

`name` and `type` under `[[package]]` now do what the docs say. Before, both were ignored. `name` replaces the name read from the manifest, so a game called `"Slopocalypse"` can be released as `slopocalypse: minor`, and the auto-detected entry under the old name goes away. `type` picks the adapter for the path, which matters when a directory has more than one manifest, and an unknown `type` is now an error. If your config sets a `name` that differs from the manifest, changesets must now use the configured name.

#### Wildcards in `ignore`

`ignore` entries can now use `*` and `?`, so `ignore = ["*_core", "*_gd"]` covers every crate with those suffixes, including ones added later. Entries without wildcards still match the whole name exactly. A nameless package, such as a Cargo workspace root, is ignored with `""`.
