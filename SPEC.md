# Spec: changesetter

Polyglot changeset management CLI. Requires a changeset file on every PR, but the version bump is optional. Supports monorepos mixing Rust, Node, Python, .NET, and anything else with a version field in a manifest file.

## Objective

Replace knope and the Node-only changesets tool with a single Rust binary that:

- Works with any package ecosystem, not just Node or Rust
- Supports `none`-bump changesets (no version change, but the PR is still documented)
- Auto-detects packages from manifest files, with optional config overrides
- Ships GitHub Actions for CI integration out of the box

Target users: developers maintaining polyglot repos or monorepos who want lightweight, enforced change documentation without coupling to a single ecosystem.

Success looks like: `cargo install changesetter`, add two workflow files, and every PR gets a changeset check while releases are a single `changesetter release` command.

## Tech stack

- Language: Rust (MSRV 1.97)
- Serialization: serde + toml (config), serde + serde_yaml (changeset frontmatter)
- Markdown: pulldown-cmark or similar for changelog generation
- XML: quick-xml for .csproj adapter (v0.2, preserves existing structure/comments/conditionals)
- CLI framework: clap (derive)
- Git: shells out to `git` CLI (required on PATH). No `git2` dependency; keeps the binary small and avoids libgit2 build complexity.
- Distribution: crates.io, GitHub Releases (pre-built binaries)
- No HTTP client in the CLI itself; GitHub API calls live in the actions

## Commands

```bash
cargo build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Project structure

```
changesetter/
├── Cargo.toml
├── changesetter.toml          # optional, for config overrides
├── src/
│   ├── main.rs
│   ├── cli/                   # clap subcommands
│   ├── changeset/             # changeset file parsing, creation, validation
│   ├── package/               # package detection and version adapters
│   ├── release/               # release pipeline (bump, changelog, tag)
│   └── changelog/             # changelog generation and formatting
├── actions/
│   ├── check/
│   │   └── action.yml         # composite: download CLI, run `changesetter check`
│   ├── install/
│   │   └── action.yml         # composite: download and cache the CLI binary
│   └── release/
│       └── action.yml         # composite: run `changesetter release`, create GitHub Release
├── docs/                          # fumadocs site
│   ├── package.json
│   ├── next.config.mjs
│   ├── content/
│   │   └── docs/
│   │       ├── index.mdx          # getting started
│   │       ├── cli.mdx            # CLI reference
│   │       ├── configuration.mdx  # changesetter.toml reference
│   │       ├── changeset-format.mdx
│   │       ├── github-actions.mdx
│   │       ├── adapters.mdx       # supported ecosystems
│   │       └── recipes/
│   │           ├── monorepo.mdx
│   │           ├── pre-releases.mdx
│   │           └── version-pr.mdx
│   └── ...
├── tests/
│   ├── fixtures/
│   └── integration/
└── .changeset/                # changesetter eats its own dogfood
```

## CLI interface

### `changesetter init`

Initialize a repo for changesetter. Creates `.changeset/` directory and optionally `changesetter.toml`.

### `changesetter add`

Interactive prompt to create a changeset file. Asks for affected packages, bump level, and description. Generates a markdown file in `.changeset/` with a random name (like changesets does).

When stdin is not a TTY (headless CI), requires `--package`, `--bump`, and `--message` flags. Errors with guidance if called interactively without a terminal.

```
changesetter add
changesetter add --package mylib --bump patch --message "Fixed null handling"
changesetter add --no-bump  # none-bump, opens editor for description
```

To retract a changeset, delete the file from `.changeset/` manually. No dedicated command needed.

### `changesetter check`

Verify that at least one changeset file exists in `.changeset/`. Used by CI. Exits 0 if valid, 1 if missing. Validates frontmatter format.

The check is file-presence only; it does not know which packages a PR touches. Even PRs that only affect ignored packages need a changeset (use `--no-bump` for those). This keeps the rule simple and enforceable without analyzing git diffs against package boundaries.

```
changesetter check
changesetter check --base main  # only check if changeset files were added vs base branch
```

When `--base` is provided, runs `git diff --name-only <base>...HEAD -- .changeset/` to check if any changeset files were added in the current branch. Requires the base ref to be available locally (not a shallow clone). Without `--base`, checks for any `.md` files in `.changeset/` regardless of git history.

### `changesetter status`

Show pending changesets and what would happen on release: which packages bump, to what version.

### `changesetter release`

The release pipeline. If there are no pending changesets, exits 0 with a message ("No pending changesets, nothing to release.") and no other side effects.

Before modifying any files, checks `git status --porcelain`. If the working tree is dirty, errors: "Working tree has uncommitted changes. Commit or stash them before running release." (Exit code 2.) Same check applies to `version` when `--no-commit` is not passed.

When changesets exist:

1. Collect all changeset files from `.changeset/`
2. Compute the highest bump per package (none < patch < minor < major)
3. Update version in each package's manifest file
4. Run post-bump hooks if configured (e.g. `cargo check`, `npm install`)
5. Generate/update CHANGELOG.md
6. Remove consumed changeset files
7. Commit changes with message `chore: release {versions}` (e.g. `chore: release mylib@0.2.0, my-api@1.0.0`)
8. Create annotated git tag(s) with the changelog excerpt as the tag message

Commit message format and tag annotation are configurable in `changesetter.toml`:

```toml
[release]
commit_message = "chore: release {versions}"  # default
tag_annotated = true                           # default; false for lightweight tags
```

GitHub Releases are handled by the `actions/release` action, not the CLI. The CLI outputs a structured release plan that the action consumes.

When run with `--output json`, `release` writes a JSON summary to stdout after completing:

```json
{
  "releases": [
    {
      "name": "mylib",
      "version": "0.2.0",
      "previous_version": "0.1.5",
      "bump": "minor",
      "tag": "v0.2.0",
      "changelog": "#### Added endpoint retry logic\n\nRequests now retry up to 3 times...",
      "changesets": ["cool-dogs-dance", "red-lions-run"]
    }
  ],
  "none_entries": [
    {
      "title": "Updated CI configuration",
      "body": "Switched from ubuntu-20.04 to ubuntu-24.04 runners."
    }
  ]
}
```

The action parses this to create GitHub Releases with the `changelog` field as the release body.

```
changesetter release
changesetter release --dry-run    # show what would happen
changesetter release --no-commit  # make changes but don't commit/tag
```

### `changesetter version`

Just the version bump + changelog step, without tagging. Useful when you want to separate versioning from releasing, or when a CI action handles the tagging/release creation.

```
changesetter version
changesetter version --snapshot canary  # temporary version for CI/preview deploys
changesetter version --no-commit        # update files but don't git commit
changesetter version --dry-run          # show what would change without writing
```

`--snapshot` ignores pre-release mode entirely. Snapshots always produce `0.0.0-{tag}-{timestamp}`, never the pre-release version scheme. Running `--snapshot` while in pre mode is valid and doesn't interact with the pre state.

### `changesetter pre`

Enter or exit pre-release mode. See [Pre-release mode](#pre-release-mode) for details.

```
changesetter pre enter rc      # enter pre mode with "rc" tag
changesetter pre enter beta    # enter pre mode with "beta" tag
changesetter pre exit          # exit pre mode, next release is stable
changesetter pre status        # show current pre mode state
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success (including "nothing to do" for `release` with no changesets) |
| 1 | Check failed (no changeset found), validation error (bad frontmatter, unknown package) |
| 2 | Environment error (dirty working tree, git not found, not a git repo, base ref unavailable) |

## Changeset file format

Files live in `.changeset/` with random kebab-case names and `.md` extension. Following the changesets convention.

Filenames are generated from a small embedded word list (~500 words, adjective-noun-verb pattern, e.g. `cool-dogs-dance.md`). Human-readable names make PR diffs easier to scan than hex strings.

```markdown
---
mylib: patch
my-api: minor
---

#### Fixed null handling in response parser

The API was returning null for optional fields. Now defaults to empty
values instead of crashing the deserializer.
```

For a none-bump changeset (no version change, but documented):

```markdown
---
mylib: none
---

#### Updated CI configuration

Switched from ubuntu-20.04 to ubuntu-24.04 runners.
```

For a repo-wide none-bump (no specific package):

```markdown
---
default: none
---

#### Added CONTRIBUTING.md

Documentation-only change, no code modified.
```

For scoped npm packages, quote the key (standard YAML):

```markdown
---
"@myorg/utils": patch
"@myorg/core": minor
---

#### Fixed scoped package resolution
```

### Frontmatter rules

- Keys are package names (as defined in config or auto-detected from manifest)
- Values: `none`, `patch`, `minor`, `major`
- Keys with special characters (`@`, `/`) must be YAML-quoted
- Multiple packages allowed in one changeset
- `default` is the implicit package name for single-package repos
- Body is markdown; the first heading (if any) becomes the changelog entry title

## Package detection and adapters

### Auto-detection

Walk the repo for known manifest files and register each as a package:

| Manifest | Ecosystem | Version field |
|---|---|---|
| `Cargo.toml` | Rust | `package.version` or `workspace.package.version` |
| `package.json` | Node | `version` |
| `pyproject.toml` | Python | `project.version` or `tool.poetry.version` |
| `*.csproj` | .NET | `PropertyGroup > Version` (XML; requires `quick-xml`, must preserve structure/comments/conditionals) |
| `Chart.yaml` | Helm | `version` |

Package name is derived from the manifest (crate name, npm name, etc.).

Auto-detection uses `git ls-files` to only scan tracked files, avoiding `node_modules/`, `target/`, `vendor/`, and other ignored directories. Falls back to a filesystem walk with a hardcoded exclude list (`node_modules`, `target`, `.git`, `vendor`, `dist`, `build`) if not in a git repo.

#### Cargo workspace handling

Cargo workspaces get special treatment:

- A workspace with `workspace.package.version` can be treated as a single package (all members bump together) or split into individual members
- Default: single package using the workspace name. All members sharing `workspace.package.version` bump as one unit.
- Config override: list individual members to bump them independently

```toml
# Bump all workspace members together (default for workspace.package.version)
[[package]]
name = "my-workspace"
path = "."
type = "cargo-workspace"
members = "all"

# Or pick specific members to bump independently
[[package]]
name = "core-lib"
path = "crates/core"
type = "cargo"

[[package]]
name = "cli"
path = "crates/cli"
type = "cargo"
```

Same pattern applies to npm workspaces, Python monorepos, or any ecosystem with workspace-level versioning.

### Adapter interface

Each adapter implements:

- `detect(path) -> Option<Package>` - check if this manifest exists and extract name + version
- `read_version(path) -> Version` - read current version
- `write_version(path, version)` - update version in the manifest
- `post_bump_hook(path) -> Option<Command>` - optional command to run after bumping (e.g. `cargo check` to update Cargo.lock)

### Lockfile handling

After bumping, some ecosystems need lockfile updates:

- Rust: `cargo check` or `cargo generate-lockfile`
- Node: `npm install` / `pnpm install` / `yarn install` (detect from lockfile present)
- Python: depends on tooling, configurable

## Configuration

`changesetter.toml` at repo root. Entirely optional for single-package repos.

Changesetter ignores `.changeset/config.json` (the original changesets tool's config file). Both files can coexist safely during migration. A migration recipe in the docs covers converting `config.json` to `changesetter.toml`.

Config is validated at load time. Errors on:
- A package appearing in more than one group (fixed or linked)
- Unknown package names in groups, ignore list, or changeset frontmatter
- A real package named `default` colliding with the implicit single-package name (the real name wins; use the detected crate/npm name in changesets, not `default`)

```toml
# Optional: override auto-detected packages
[[package]]
name = "mylib"
path = "crates/mylib"
type = "cargo"

[[package]]
name = "my-frontend"
path = "apps/web"
type = "npm"

# Package groups
[groups.core]
fixed = ["core-lib", "core-macros"]  # always bump together, even if only one has a changeset

[groups.utils]
linked = ["util-a", "util-b"]        # share version numbers, but only bump when individually changed

# Ignore packages (auto-detected but never versioned/released)
ignore = ["examples", "internal-tools"]

# Internal dependency cascading
# When package A bumps and package B depends on A, bump B's dependency range
# "patch" = always bump dependents, "minor" = only when range breaks, "none" = don't cascade
update_internal_dependencies = "patch"

# Changelog settings
[changelog]
file = "CHANGELOG.md"            # filename to use (default: "CHANGELOG.md")
                                 # when per_package = true: filename used in each package dir
                                 # when per_package = false: path relative to repo root
per_package = true               # monorepo: each package gets its own CHANGELOG.md in its dir
                                 # false: single root CHANGELOG.md with entries grouped by package
none_bump = "section"            # "section" | "file" | "omit"
none_bump_file = "CHANGELOG-internal.md"  # only if none_bump = "file"
none_bump_heading = "Internal"   # heading for none-bump section, default "Internal"

# Tag format
[tag]
format = "v{version}"             # single-package default
# format = "{name}@v{version}"    # monorepo default

# Post-bump hooks (override auto-detected ones)
[hooks]
post_bump = ["cargo check", "cargo fmt"]
```

When no config file exists, changesetter auto-detects packages and uses defaults for everything.

## Changelog generation

Generated CHANGELOG.md follows [Keep a Changelog](https://keepachangelog.com/) conventions:

```markdown
# Changelog

## 0.2.0 - 2026-07-25

#### Added endpoint retry logic

Requests now retry up to 3 times on 5xx responses with exponential backoff.

#### Fixed null handling in response parser

The API was returning null for optional fields.

### Internal

#### Updated CI configuration

Switched from ubuntu-20.04 to ubuntu-24.04 runners.

## 0.1.0 - 2026-07-01

Initial release.
```

- Each changeset's markdown body becomes an entry
- Entries grouped under the version heading
- `none`-bump entries go under the configured heading (default: "Internal")
- Newest version at the top
- Unreleased changes can optionally go under an `## Unreleased` heading

### Monorepo changelogs

When `changelog.per_package = true` (default for monorepos), each package gets its own `CHANGELOG.md` in its package directory.

A changeset that references multiple packages (e.g. `mylib: patch` and `my-api: minor` in one file) has its full markdown body written to every referenced package's changelog. This matches changesets' behavior. If a user wants different descriptions per package, they write separate changeset files.

When `changelog.per_package = false`, a single root `CHANGELOG.md` groups entries by package:

```markdown
## 0.2.0 - 2026-07-25

### mylib

#### Added endpoint retry logic

### my-frontend

#### Updated dashboard layout
```

Single-package repos always use a single root `CHANGELOG.md` regardless of this setting.

## GitHub Actions

All GitHub-specific logic (API calls, PR comments, GitHub Releases) lives in the actions, not the CLI. The CLI is pure filesystem + git.

### `actions/install/action.yml`

Downloads and caches the changesetter binary. Thin composite action.

Inputs:
- `version`: version to install (default: `latest`)

### `actions/check/action.yml`

Runs `changesetter check` on PRs. Composite action that uses the install action, then runs the check command.

Inputs:
- `base`: base branch to compare against (default: auto-detect from PR)
- `comment`: whether to leave a PR comment summarizing the changeset (default: `true`)

When `comment` is enabled, posts/updates a comment showing which packages are affected and their bump levels. Updates the same comment on subsequent pushes rather than creating new ones.

### `actions/release/action.yml`

Runs `changesetter release` and optionally creates GitHub Releases from the output.

Inputs:
- `github-release`: whether to create a GitHub Release (default: `true`)
- `version-pr`: whether to create/update a "Version Packages" PR instead of releasing directly (default: `false`)
- `draft`: create GitHub Releases as drafts (default: `false`)

When `version-pr` is disabled (default), `changesetter release --output json` runs directly on push to main. The action parses the JSON output, creates git tags, and creates GitHub Releases.

When `version-pr` is enabled, the action operates as a state machine:

#### Version-PR state machine

The action runs on every push to main and follows this flow:

```
Push to main
  │
  ├── Are there pending changesets in .changeset/?
  │     │
  │     ├── No  → Is this push the merge of a version PR?
  │     │          │
  │     │          ├── Yes → Run `changesetter release`, create tags + GitHub Releases
  │     │          │         (changesets were already consumed by the version PR)
  │     │          │
  │     │          └── No  → Nothing to do. Exit.
  │     │
  │     └── Yes → Run `changesetter version --no-commit` (dry run to compute changes)
  │               │
  │               ├── Does a version PR already exist? (found by label: `changesetter:version`)
  │               │     │
  │               │     ├── Yes → Force-push updated version branch, update PR body
  │               │     │
  │               │     └── No  → Create branch `changesetter/version-packages`,
  │               │               run `changesetter version`, commit, push, open PR
  │               │
  │               └── PR body lists: packages to bump, new versions, changelog preview
```

**Branch**: `changesetter/version-packages` (fixed name, force-pushed on updates)

**PR label**: `changesetter:version` (used to find the existing PR)

**PR title**: "Version Packages" (configurable via `version-pr-title` input)

**Merge detection**: the action queries the GitHub API for a PR labeled `changesetter:version` whose `merged_at` matches the current push. Branch name in the merge commit is unreliable (squash and rebase merges don't preserve it). If a matching merged PR is found, the action skips version computation and goes straight to tagging + GitHub Release creation.

**Conflict handling**: if the version branch has merge conflicts with main (e.g. someone manually edited CHANGELOG.md), the action force-pushes a fresh version branch computed from current main. The PR body notes that it was regenerated.

**Concurrency**: the workflow should use `concurrency: { group: changesetter-release, cancel-in-progress: true }` to prevent races when multiple pushes arrive in quick succession.

Outputs:
- `released`: whether a release was created (`true`/`false`)
- `releases`: JSON array of `{name, version, tag, changelog}` objects
- `version-pr`: URL of the version PR if one was created/updated, empty otherwise

Usage:

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
          fetch-depth: 0  # needed for --base diff
      - uses: saturate/changesetter/actions/check@v1
        with:
          comment: true  # post/update PR comment with changeset summary
```

```yaml
# .github/workflows/release.yml
# Option A: direct release on merge to main
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

```yaml
# Option B: version PR pattern (like changesets)
name: Release
on:
  push:
    branches: [main]

jobs:
  release:
    runs-on: ubuntu-latest
    permissions:
      contents: write
      pull-requests: write
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: saturate/changesetter/actions/release@v1
        with:
          version-pr: true
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

## Tag format

- Single-package: `v{version}` (e.g. `v0.2.0`)
- Monorepo: `{name}@v{version}` (e.g. `mylib@v0.2.0`)
- Configurable via `[tag]` section

Follows the convention knope uses for monorepos, which aligns with the npm/changesets convention.

## Package groups and dependency cascading

Borrowed from changesets, adapted to work across ecosystems.

### Fixed groups

Packages in a `fixed` group always bump and release together. If `core-lib` gets a minor changeset but `core-macros` has no changeset, both bump to the same minor version. Use for tightly coupled packages where mixed versions don't make sense.

### Linked groups

Packages in a `linked` group share version numbers but only bump when they individually have a changeset. If only `util-a` has a changeset, `util-b` stays put. But when both have changesets in the same release, they coordinate to the highest bump level in the group. Use for packages that should stay in sync but can skip releases independently.

### Internal dependency cascading

When package B depends on package A (detected from manifest files), and A bumps its version:

- `update_internal_dependencies = "patch"`: always bump B (at least patch) and update its dependency on A
- `update_internal_dependencies = "minor"`: only bump B if A's version change would break B's dependency range
- `update_internal_dependencies = "none"`: don't cascade; the user manages it

Dependency detection is ecosystem-aware: reads `[dependencies]` from Cargo.toml, `dependencies` from package.json, etc.

### Ignored packages

Packages listed in `ignore` are auto-detected but never versioned, tagged, or included in changelogs. Useful for example crates, internal tooling, or test fixtures that happen to have a manifest file.

## Snapshot releases

Temporary versions for CI, preview deploys, or testing packages before a real release.

```
changesetter version --snapshot canary
```

Produces versions like `0.0.0-canary-20260724T143022` (tag + timestamp). Snapshot versions:

- Don't consume changesets (they stay for the real release)
- Don't update CHANGELOG.md
- Don't create git tags
- Do update manifest files (so you can build and publish a preview)

Useful in CI to publish canary packages from feature branches for integration testing.

## Documentation site

A [fumadocs](https://fumadocs.vercel.app/) site lives in `docs/`. This is a Next.js app with MDX content, deployed to GitHub Pages (or Vercel).

Content structure:
- **Getting started**: install, init, first changeset, first release
- **CLI reference**: all commands, flags, exit codes
- **Configuration**: `changesetter.toml` reference with all options
- **Changeset format**: frontmatter rules, bump levels, `none` bumps, examples
- **GitHub Actions**: setup guides for check, release, version PR pattern
- **Adapters**: supported ecosystems, how detection works, how to request new ones
- **Recipes**: monorepo setup, pre-releases, migrating from changesets/knope

The docs site is a separate package in the repo with its own `package.json` but does not get versioned or released by changesetter. It just deploys on push to main.

## Code style

```rust
use clap::Parser;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Changeset {
    packages: BTreeMap<String, BumpLevel>,
    body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BumpLevel {
    None,
    Patch,
    Minor,
    Major,
}
```

Standard Rust conventions. Derive what you can. Error handling with `thiserror` for library errors, `anyhow` at the CLI boundary.

## Testing strategy

### Unit tests

Inline `#[cfg(test)]` modules for parsing, version comparison, adapter logic.

### Integration tests

`tests/` directory using tempdir repos with real manifest files. No mocking of the filesystem; use real temp directories (`tempfile` crate).

### Snapshot tests

Changelog generation output validated with `insta` snapshots.

### Changesets compatibility suite

Port the core test cases from the original [changesets](https://github.com/changesets/changesets) project (MIT licensed) to validate format compatibility. The original tests use inline markdown strings as test data, which we extract into `tests/fixtures/changesets-compat/`.

Three test areas to port:

| Original package | What it tests | Port approach |
|---|---|---|
| `@changesets/parse` | YAML frontmatter extraction, edge cases (Windows line endings, `---` in body, empty files, malformed frontmatter) | Extract ~20 inline markdown strings into fixture files, assert our parser produces the same structure |
| `@changesets/read` | Reading `.changeset/` directory, filtering non-changeset files, handling `config.json` | Create temp `.changeset/` dirs with `tempfile`, assert same read behavior |
| `@changesets/assemble-release-plan` | Bump precedence, fixed/linked groups, snapshot versions, dependency cascading, `none` type handling | Pure data-in/data-out tests. Port the `FakeFullState` builder pattern to Rust. This is the richest suite and covers the hardest logic. |

Not ported (npm-specific): `apply-release-plan` (package.json updates), `get-dependents-graph` (npm dependency resolution), git integration tests.

### Extension tests

Separate test suite for changesetter-specific features that go beyond changesets:

- `none` bump level (changesets has `none` internally but doesn't expose it in the file format the same way)
- `default` package name for single-package repos
- Pre-release mode (enter/exit/status)
- Polyglot adapter tests (Cargo.toml, pyproject.toml, .csproj version read/write)
- Snapshot release with configurable tags
- Ignore list behavior
- `changesetter.toml` config parsing (vs changesets' JSON config)

### Fixture structure

```
tests/
├── fixtures/
│   ├── changesets-compat/       # ported from original changesets (MIT)
│   │   ├── parse/               # changeset markdown files with expected parse output
│   │   ├── read/                # directory structures to test reading
│   │   └── release-plan/        # input state + expected release plan output
│   ├── adapters/                # manifest files for each ecosystem
│   │   ├── cargo/
│   │   ├── npm/
│   │   ├── python/
│   │   └── dotnet/
│   └── changelogs/              # expected changelog output (insta snapshots)
└── integration/
    ├── init_test.rs
    ├── add_test.rs
    ├── check_test.rs
    ├── release_test.rs
    └── snapshot_test.rs
```

### Compliance validation

CI runs the compat suite on every PR. If a change breaks compatibility with the original changesets format, the test fails. Extensions (new bump levels, new config options) must never break parsing of valid changesets-format files. A file that the original `@changesets/parse` accepts must also be accepted by changesetter, with the same semantic result.

## Boundaries

**Always:**
- Run `cargo fmt --check && cargo clippy && cargo test` before commits
- Validate changeset frontmatter strictly (reject unknown bump levels, unknown packages)
- Handle missing/malformed manifest files gracefully with clear error messages

**Ask first:**
- Adding new package ecosystem adapters
- Changing the changeset file format
- Adding new CLI subcommands

**Never:**
- Execute arbitrary commands outside of configured hooks. Post-bump hooks in `changesetter.toml` are trusted because the config file is committed to the repo and reviewed in PRs, same trust model as CI config. Hooks inherit the CLI's environment and run with the same permissions. No sandboxing.
- Modify files outside the repo root
- Push commits or create PRs (the CLI prepares changes; the user or CI pushes)

## Success criteria

- `changesetter init && changesetter add --no-bump && changesetter check` works in any repo with zero config
- A Cargo.toml-only repo needs no changesetter.toml to auto-detect and bump
- A repo with Cargo.toml + package.json auto-detects both and bumps independently
- `none`-bump changesets pass the check but don't change any version
- Changelog includes `none`-bump entries under a configurable heading
- GitHub Actions work with `uses: saturate/changesetter/actions/check@v1`
- Round-trip test: create changeset, run release, verify version bumped, changelog updated, tag created

## Pre-release mode

Pre-releases use a mode-based workflow (like changesets), not a bump level. The repo enters and exits pre-release mode explicitly.

### Entering pre mode

```
changesetter pre enter rc
```

Creates `.changeset/pre.json`:

```json
{
  "mode": "pre",
  "tag": "rc",
  "packages_released": {}
}
```

While in pre mode:
- `changesetter release` produces pre-release versions: `1.0.0-rc.0`, `1.0.0-rc.1`, etc.
- Changesets are consumed normally (deleted after release)
- Each release increments the pre-release counter for that package
- The `packages_released` field tracks how many pre-releases each package has had

### Adding changesets in pre mode

Changesets use normal bump levels (`patch`, `minor`, `major`). The pre mode wraps them:

```markdown
---
mylib: minor
---

#### New API endpoint for batch operations
```

If `mylib` is at `0.5.0`, this produces `0.6.0-rc.0` (not `0.6.0`).

### Exiting pre mode

```
changesetter pre exit
```

The next `changesetter release` strips the pre-release suffix and publishes the stable version. `0.6.0-rc.3` becomes `0.6.0`.

### Multiple pre-release stages

To go from `rc` to `beta` or vice versa:

```
changesetter pre exit
changesetter pre enter beta
```

## Release phases

Not everything ships at once. The spec describes the full vision; here's the cut line.

### v0.1 (MVP)

- CLI: `init`, `add`, `check`, `status`, `release`, `version`
- Adapters: Cargo (single crate + workspace), npm
- Changeset format: full compatibility with original changesets
- `none` bump level
- Single-package and basic monorepo support
- Changelog generation (single file and per-package)
- GitHub Actions: `install`, `check` (with PR comments), `release` (direct mode)
- Changesets compatibility test suite (parse + read + release-plan)
- Binary distribution: linux-x64, linux-arm64, darwin-x64, darwin-arm64, windows-x64

### v0.2

- Pre-release mode (`pre enter`/`exit`)
- Snapshot releases
- Fixed and linked package groups
- Internal dependency cascading
- Version-PR mode in release action
- Adapters: Python (pyproject.toml), .NET (.csproj), Helm (Chart.yaml)
- Documentation site (fumadocs)

### Later

- Additional adapters on request
- GitHub App (richer PR integration than Actions)
- Config migration tool (`changesets config.json` -> `changesetter.toml`)

## Open questions

1. **Binary distribution**: should the install action download from GitHub Releases, or also support `cargo-binstall`?
2. **Changelog template customization**: should users be able to provide a custom template for changelog entries, or is the default format sufficient for v1?
