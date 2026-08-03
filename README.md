# changesetter

Changeset management for polyglot repos. A single Rust binary that handles versioning and changelogs for Cargo, npm, and any other ecosystem with a version in a manifest file.

## Why not changesets?

[changesets](https://github.com/changesets/changesets) is excellent if your repo is JavaScript. Its v3 direction doubles down on npm/pnpm/yarn publishing workflows, formatter integrations, and `changeset pack` for JS artifacts. No plans for native support of other ecosystems.

If your repo has a `Cargo.toml`, a `pyproject.toml`, or a `.csproj` next to a `package.json`, changesets asks you to wrap everything in fake `package.json` files and wire up your own workflows. changesetter reads the manifests directly.

The changeset file format is compatible. A `.changeset/*.md` file that works with `@changesets/cli` works with `changesetter`, and vice versa.

## Install

```bash
# From source
cargo install changesetter

# Or download a prebuilt binary (linux, macOS, windows)
curl -sL https://github.com/saturate/changesetter/releases/latest/download/changesetter-x86_64-unknown-linux-gnu.tar.gz | tar xz
```

## Quick start

```bash
changesetter init                    # creates .changeset/ directory
changesetter add                     # interactive: pick packages, bump level, write description
changesetter check                   # CI: fails if no changeset on the branch
changesetter status                  # preview: what would release look like?
changesetter release                 # bump versions, update CHANGELOG.md, commit, tag
changesetter pre enter rc            # enter pre-release mode
changesetter pre exit                # exit pre-release mode
```

Non-interactive mode for CI scripts:

```bash
changesetter add --package mylib --bump patch --message "Fixed null handling"
changesetter add --package mylib --no-bump --message "Updated CI config"
```

## Changeset file format

Files live in `.changeset/` with random human-readable names like `cool-dogs-dance.md`:

```markdown
---
mylib: patch
my-api: minor
---

#### Fixed null handling in response parser

The API was returning null for optional fields. Now defaults to empty
values instead of crashing the deserializer.
```

Bump levels: `none`, `patch`, `minor`, `major`. The `none` level documents a change without bumping the version; useful for CI changes, docs, or internal tooling.

For scoped npm packages, quote the key:

```markdown
---
"@myorg/utils": patch
---
```

## Package detection

changesetter walks the repo (via `git ls-files`) and detects packages from manifest files:

| Manifest | Ecosystem | Version field |
|---|---|---|
| `Cargo.toml` | Rust | `package.version` or `workspace.package.version` |
| `package.json` | Node | `version` |
| `pyproject.toml` | Python | `project.version` or `tool.poetry.version` |
| `*.csproj` | .NET | `<Version>` in `<PropertyGroup>` |
| `Chart.yaml` | Helm | `version` |

No config needed for single-package repos. For monorepos, auto-detection finds all packages. Override with `changesetter.toml` if needed:

```toml
[[package]]
name = "mylib"
path = "crates/mylib"
type = "cargo"

ignore = ["examples", "internal-tools"]

[changelog]
per_package = true
none_bump_heading = "Internal"
```

## Pre-release mode

Enter pre-release mode to publish release candidates before a stable release:

```bash
changesetter pre enter rc            # enter pre mode with "rc" tag
# add changesets as normal
changesetter release                 # produces 1.0.0-rc.0, 1.0.0-rc.1, etc.
changesetter pre exit                # next release will be stable
changesetter release                 # produces 1.0.0
```

State is tracked in `.changeset/pre.json`. Changesets use normal bump levels (`patch`, `minor`, `major`); the pre mode wraps the version output. Snapshot releases (`--snapshot`) ignore pre mode.

## Package groups

For monorepos, packages can be grouped to coordinate their releases:

```toml
# Fixed: always bump together, even if only one has a changeset
[groups.core]
fixed = ["core-lib", "core-macros"]

# Linked: share version numbers, but only bump when individually changed
[groups.utils]
linked = ["util-a", "util-b"]
```

With `update_internal_dependencies = "patch"` in config, bumping a package cascades at least a patch bump to packages that depend on it.

## GitHub Actions

### Changeset check on PRs

```yaml
# .github/workflows/changeset-check.yml
name: Changeset Check
on:
  pull_request:
    types: [opened, synchronize, reopened]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: saturate/changesetter/actions/check@v1
        with:
          comment: true
```

### Release on merge to main

```yaml
# .github/workflows/release.yml
name: Release
on:
  push:
    branches: [main]

jobs:
  release:
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: saturate/changesetter/actions/release@v1
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

The release action runs `changesetter release --output json`, pushes the release commit and tags, and creates GitHub Releases with the changelog as the body.

## Configuration

`changesetter.toml` at the repo root. Entirely optional.

```toml
# Package groups
[groups.core]
fixed = ["core-lib", "core-macros"]

[groups.utils]
linked = ["util-a", "util-b"]

# Dependency cascading
update_internal_dependencies = "patch"  # "patch" | "minor" | "none"

# Changelog
[changelog]
file = "CHANGELOG.md"
per_package = true          # each package gets its own CHANGELOG.md
none_bump = "section"       # "section" | "omit"
none_bump_heading = "Internal"

# Tag format
[tag]
format = "v{version}"            # single-package (default)
# format = "{name}@v{version}"   # monorepo (default when >1 package)

# Release
[release]
commit_message = "chore: release {versions}"
tag_annotated = true

# Post-bump hooks
[hooks]
post_bump = ["cargo check", "cargo fmt"]
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success (including "nothing to do" for release with no changesets) |
| 1 | Check failed, validation error (bad frontmatter, unknown package) |
| 2 | Environment error (dirty working tree, git not found, base ref unavailable) |

## Roadmap

**v0.1**: `init`, `add`, `check`, `status`, `version`, `release`. Cargo + npm adapters. Changelog generation. GitHub Actions. Changesets format compatibility.

**v0.2** (current): Pre-release mode (`pre enter`/`exit`). Snapshot releases. Fixed and linked package groups. Internal dependency cascading. Version-PR mode in release action. Python, .NET, and Helm adapters. Documentation site.

**Later**: Additional adapters on request. GitHub App for richer PR integration. Config migration tool (`changesets config.json` -> `changesetter.toml`).

## License

MIT
