# Plan: changesetter v0.1 MVP

Scope: everything in the v0.1 release phase from SPEC.md.

## Dependency graph

```
Core types (BumpLevel, Changeset, Package, Version, Config)
    ├── Changeset parser (YAML frontmatter + markdown body)
    │       ├── Changeset reader (scan .changeset/ directory)
    │       │       ├── check command
    │       │       ├── status command
    │       │       └── Release plan assembler
    │       │               ├── version command
    │       │               └── release command
    │       └── add command (creates changeset files)
    ├── Package adapters (Cargo, npm)
    │       ├── Package detector (walk repo, find manifests)
    │       │       ├── status command
    │       │       └── Release plan assembler
    │       └── Version writer (update manifest files)
    │               ├── version command
    │               └── release command
    ├── Config loader (changesetter.toml)
    │       └── Everything that needs package list or settings
    ├── Changelog generator
    │       ├── version command
    │       └── release command
    └── init command (standalone, no deps on other modules)
```

Build order: types -> parser -> adapters -> config -> reader -> detector -> init -> add -> check -> status -> release plan -> changelog -> version -> release -> actions -> distribution.

---

## Task 1: Project scaffold and core types

**Description:** Initialize the Rust project with Cargo.toml, clap CLI skeleton, and the core domain types that everything else depends on: `BumpLevel`, `Version`, `Changeset`, `Package`, and error types.

**Acceptance criteria:**
- [ ] `cargo build` compiles a binary that prints help with subcommand stubs (init, add, check, status, release, version)
- [ ] `BumpLevel` enum with None/Patch/Minor/Major, ordered (None < Patch < Minor < Major), serde-deserializable
- [ ] `Changeset` struct with packages map and markdown body
- [ ] `Package` struct with name, path, manifest type, current version
- [ ] Error types using `thiserror` for library errors, `anyhow` at CLI boundary
- [ ] `cargo fmt --check && cargo clippy --all-targets -- -D warnings` passes
- [ ] Unit tests for `BumpLevel` ordering and serialization

**Verification:**
- [ ] `cargo test` passes
- [ ] `cargo run -- --help` shows all subcommands
- [ ] `cargo run -- init` prints "not yet implemented" (stub)

**Dependencies:** None

**Files likely touched:**
- `Cargo.toml`
- `src/main.rs`
- `src/cli/mod.rs`, `src/cli/*.rs` (one per subcommand, stubs)
- `src/changeset/mod.rs`, `src/changeset/types.rs`
- `src/package/mod.rs`, `src/package/types.rs`
- `src/errors.rs`

**Estimated scope:** Medium (8+ files but all small; type definitions and stubs)

---

## Task 2: Changeset parser

**Description:** Parse changeset markdown files: extract YAML frontmatter (package-to-bump-level map) and the markdown body. Handle edge cases from the changesets compat spec: Windows line endings, `---` appearing in the body, empty body, quoted YAML keys for scoped npm packages.

**Acceptance criteria:**
- [ ] Parse standard changeset: extracts package names, bump levels, and body
- [ ] `none` bump level parses correctly
- [ ] Scoped npm packages with quoted keys (`"@myorg/utils": patch`) parse correctly
- [ ] `default` key works for single-package repos
- [ ] Windows line endings (`\r\n`) handled
- [ ] `---` in the markdown body (after frontmatter) does not break parsing
- [ ] Empty body (frontmatter only) returns empty string body
- [ ] Malformed frontmatter returns a clear error (not a panic)
- [ ] Unknown bump level returns a validation error

**Verification:**
- [ ] `cargo test` passes with at least 10 parser unit tests
- [ ] `cargo clippy` clean

**Dependencies:** Task 1

**Files likely touched:**
- `src/changeset/parser.rs`
- `src/changeset/mod.rs`
- Test fixtures in `tests/fixtures/changesets-compat/parse/`

**Estimated scope:** Medium (2-3 source files + fixtures)

---

## Task 3: Package adapters (Cargo + npm)

**Description:** Implement the adapter trait and two concrete adapters: Cargo (Cargo.toml, including workspace detection) and npm (package.json). Each adapter can detect a manifest, read the current version, and write an updated version.

**Acceptance criteria:**
- [ ] Adapter trait with `detect`, `read_version`, `write_version`, `post_bump_hook` methods
- [ ] Cargo adapter reads `package.version` from Cargo.toml
- [ ] Cargo adapter reads `workspace.package.version` from workspace Cargo.toml
- [ ] Cargo adapter writes version back without clobbering other fields (uses `toml_edit` or equivalent)
- [ ] npm adapter reads `version` from package.json
- [ ] npm adapter writes version back preserving formatting (trailing newline, indent style)
- [ ] Post-bump hook: Cargo returns `cargo check`, npm detects lockfile and returns appropriate install command
- [ ] Unit tests with real fixture files for each adapter

**Verification:**
- [ ] `cargo test` passes
- [ ] Round-trip test: read version, write new version, read again, verify it changed
- [ ] Fixture files in `tests/fixtures/adapters/cargo/` and `tests/fixtures/adapters/npm/`

**Dependencies:** Task 1

**Files likely touched:**
- `src/package/adapter.rs` (trait)
- `src/package/cargo.rs`
- `src/package/npm.rs`
- `src/package/mod.rs`
- `tests/fixtures/adapters/cargo/Cargo.toml` (various fixtures)
- `tests/fixtures/adapters/npm/package.json` (various fixtures)
- `Cargo.toml` (add `toml_edit`, `serde_json` deps)

**Estimated scope:** Medium (5-6 source files + fixtures)

---

## Task 4: Config loader

**Description:** Parse `changesetter.toml` configuration file. Support all v0.1-relevant fields: package overrides, ignore list, changelog settings, tag format, release settings, and post-bump hooks. When no config file exists, return sensible defaults.

**Acceptance criteria:**
- [ ] Deserializes full `changesetter.toml` structure with serde
- [ ] Missing config file returns default config (no error)
- [ ] Validates: error on unknown packages in ignore list (validated later when packages are known)
- [ ] `changelog` section: `file`, `per_package`, `none_bump`, `none_bump_file`, `none_bump_heading`
- [ ] `tag` section: `format` with `{version}` and `{name}` placeholders
- [ ] `release` section: `commit_message`, `tag_annotated`
- [ ] `hooks` section: `post_bump` command list
- [ ] Unit tests for parsing valid config, defaults, and error cases

**Verification:**
- [ ] `cargo test` passes
- [ ] Test with a full example config, a minimal config, and no config

**Dependencies:** Task 1

**Files likely touched:**
- `src/config.rs`
- Test fixtures (inline or small .toml files)

**Estimated scope:** Small (1-2 source files)

---

## Task 5: Package detector

**Description:** Walk the repo to auto-detect packages from manifest files. Uses `git ls-files` to avoid scanning ignored directories. Falls back to filesystem walk with hardcoded excludes when not in a git repo. Merges auto-detected packages with config overrides.

**Acceptance criteria:**
- [ ] Finds Cargo.toml and package.json files via `git ls-files`
- [ ] Falls back to filesystem walk excluding `node_modules`, `target`, `.git`, `vendor`, `dist`, `build`
- [ ] Runs each found manifest through the appropriate adapter's `detect`
- [ ] Config-defined packages override auto-detected ones (by name)
- [ ] Ignored packages (from config) are filtered out
- [ ] Cargo workspace: detected as single package by default (workspace name, workspace version)
- [ ] Single-package repo with no config uses `default` as the package name? No: uses the detected name from the manifest. `default` is only for changeset frontmatter when no package is specified.
- [ ] Returns a `Vec<Package>` sorted by name for deterministic output

**Verification:**
- [ ] `cargo test` with temp directory fixtures (using `tempfile` crate)
- [ ] Test: repo with just a Cargo.toml detects one Cargo package
- [ ] Test: repo with Cargo.toml + package.json detects both
- [ ] Test: config ignore list filters out packages

**Dependencies:** Tasks 3, 4

**Files likely touched:**
- `src/package/detector.rs`
- `src/package/mod.rs`
- `Cargo.toml` (add `tempfile` dev-dependency)

**Estimated scope:** Medium (2-3 source files)

---

## Task 6: Changeset reader

**Description:** Read all changeset files from the `.changeset/` directory. Filter out non-changeset files (config.json, pre.json, README, non-.md files). Parse each valid file using the parser from Task 2. Validate that referenced packages exist.

**Acceptance criteria:**
- [ ] Reads all `.md` files from `.changeset/`
- [ ] Ignores `config.json`, `pre.json`, `README.md`, and non-`.md` files
- [ ] Parses each file and returns `Vec<Changeset>` with filename
- [ ] Validates package names against known packages (error on unknown, unless `default`)
- [ ] Validates bump levels
- [ ] Handles empty `.changeset/` directory (returns empty vec, no error)
- [ ] Handles missing `.changeset/` directory (returns empty vec, no error)

**Verification:**
- [ ] `cargo test` with temp directories containing various changeset files
- [ ] Test: directory with mix of valid changesets, config.json, and non-md files

**Dependencies:** Tasks 2, 5

**Files likely touched:**
- `src/changeset/reader.rs`
- `src/changeset/mod.rs`

**Estimated scope:** Small (1-2 source files)

---

## Checkpoint: After Tasks 1-6

- [ ] All tests pass (`cargo test`)
- [ ] `cargo fmt --check && cargo clippy --all-targets -- -D warnings` clean
- [ ] Core domain is solid: types, parsing, adapters, config, detection, reading
- [ ] Review with human before building CLI commands

---

## Task 7: `init` command

**Description:** Create `.changeset/` directory and optionally generate a starter `changesetter.toml`. If `.changeset/` already exists, print a message and exit successfully.

**Acceptance criteria:**
- [ ] Creates `.changeset/` directory
- [ ] Idempotent: succeeds if directory already exists
- [ ] Prints confirmation message to stderr
- [ ] Optional `--config` flag generates a commented `changesetter.toml` template
- [ ] Works from any subdirectory (finds repo root via git or walks up to find `.git`)

**Verification:**
- [ ] `cargo test` with temp directory integration test
- [ ] `cargo run -- init` in a temp repo creates `.changeset/`
- [ ] Running it twice does not error

**Dependencies:** Task 4

**Files likely touched:**
- `src/cli/init.rs`

**Estimated scope:** Small (1 file)

---

## Task 8: `add` command

**Description:** Interactive and non-interactive changeset creation. Interactive mode prompts for packages, bump level, and opens editor for description. Non-interactive mode uses `--package`, `--bump`, `--message` flags. Generates a random kebab-case filename.

**Acceptance criteria:**
- [ ] Non-interactive: `changesetter add --package foo --bump patch --message "fix"` creates a changeset file
- [ ] `--no-bump` flag creates a `none` bump changeset
- [ ] Generated filename uses adjective-noun-verb pattern from embedded word list
- [ ] Generated file has correct YAML frontmatter and markdown body
- [ ] Interactive mode: detects TTY, prompts for package selection, bump level, opens `$EDITOR` for body
- [ ] Non-TTY without required flags: exits with error and guidance
- [ ] Multi-package: `--package a --package b` or comma-separated
- [ ] Created file round-trips through the parser from Task 2

**Verification:**
- [ ] `cargo test` including integration test that creates a file and re-parses it
- [ ] Manual: `cargo run -- add --package default --bump patch --message "test"` in a temp repo

**Dependencies:** Tasks 2, 5, 6, 7

**Files likely touched:**
- `src/cli/add.rs`
- `src/changeset/writer.rs` (file generation)
- `src/changeset/words.rs` (word list for filenames)

**Estimated scope:** Medium (3-4 files)

---

## Task 9: `check` command

**Description:** Verify that at least one changeset file exists. With `--base`, check that changeset files were added relative to a base branch using `git diff`. Exit 0 on success, 1 on failure.

**Acceptance criteria:**
- [ ] Without `--base`: exits 0 if any `.md` files exist in `.changeset/`, exits 1 otherwise
- [ ] With `--base main`: runs `git diff --name-only main...HEAD -- .changeset/` and checks for added files
- [ ] Validates frontmatter of found changesets (exits 1 with details on malformed files)
- [ ] Prints human-readable summary: which packages, which bump levels
- [ ] Exit code 1 when no changesets found, with helpful message suggesting `changesetter add`
- [ ] Exit code 2 when `--base` ref is not available (e.g. shallow clone)

**Verification:**
- [ ] Integration tests with temp git repos
- [ ] Test: repo with changeset -> exit 0
- [ ] Test: empty .changeset/ -> exit 1
- [ ] Test: --base with changeset on branch -> exit 0

**Dependencies:** Task 6

**Files likely touched:**
- `src/cli/check.rs`

**Estimated scope:** Small (1-2 files)

---

## Task 10: `status` command

**Description:** Show pending changesets and what would happen on release: which packages would bump, from what version to what version. Pure read-only, no side effects.

**Acceptance criteria:**
- [ ] Lists all pending changesets with their affected packages and bump levels
- [ ] Shows computed highest bump per package (when multiple changesets affect same package)
- [ ] Shows current version -> next version for each affected package
- [ ] Handles `none` bumps: shows them separately (no version change)
- [ ] When no changesets pending, prints "No pending changesets"
- [ ] Human-readable table output to stdout

**Verification:**
- [ ] Integration test with temp repo, multiple changesets
- [ ] Manual: create changesets, run `cargo run -- status`, verify output

**Dependencies:** Tasks 5, 6

**Files likely touched:**
- `src/cli/status.rs`
- `src/release/plan.rs` (release plan computation, reused by release/version commands)

**Estimated scope:** Medium (2-3 files)

---

## Task 11: Release plan assembler

**Description:** Core logic that computes the release plan from pending changesets and detected packages. Determines the highest bump per package, computes new versions, handles `none` bumps. This is the shared engine used by `status`, `version`, and `release`.

**Acceptance criteria:**
- [ ] Collects all changesets, computes highest bump per package
- [ ] Bumps version correctly: patch increments patch, minor increments minor and resets patch, major increments major and resets minor+patch
- [ ] `none` bump: package appears in plan but version unchanged
- [ ] Multiple changesets for same package: highest bump wins
- [ ] Returns structured `ReleasePlan` with list of releases and none-entries
- [ ] Single-package repos: `default` in frontmatter maps to the detected package
- [ ] Monorepo: each package bumped independently

**Verification:**
- [ ] Unit tests covering bump precedence, multi-changeset, none handling
- [ ] Test: patch + minor for same package = minor
- [ ] Test: none + patch for same package = patch
- [ ] Test: default mapping for single-package repo

**Dependencies:** Tasks 5, 6

**Files likely touched:**
- `src/release/plan.rs`
- `src/release/mod.rs`

**Estimated scope:** Medium (2-3 files)

---

## Task 12: Changelog generator

**Description:** Generate or update CHANGELOG.md from a release plan. Supports single root changelog and per-package changelogs. Handles `none`-bump entries under a configurable heading. Prepends new entries below the `# Changelog` header, above existing entries.

**Acceptance criteria:**
- [ ] Generates changelog entry with version heading, date, and changeset bodies
- [ ] Prepends to existing CHANGELOG.md (newest version on top)
- [ ] Creates CHANGELOG.md if it doesn't exist, with `# Changelog` header
- [ ] `none`-bump entries under configurable heading (default: "Internal")
- [ ] Per-package mode: writes to each package's directory
- [ ] Single-file mode: groups entries by package name under version heading
- [ ] Entries are the raw markdown body from changesets (not re-formatted)
- [ ] Snapshot tests with `insta` for output format

**Verification:**
- [ ] `cargo test` with insta snapshots
- [ ] Test: fresh changelog creation
- [ ] Test: prepend to existing changelog
- [ ] Test: per-package mode
- [ ] Test: none-bump entries in separate section

**Dependencies:** Task 11

**Files likely touched:**
- `src/changelog/mod.rs`
- `src/changelog/generator.rs`
- `Cargo.toml` (add `insta` dev-dependency)

**Estimated scope:** Medium (2-3 files + snapshots)

---

## Checkpoint: After Tasks 7-12

- [ ] All tests pass
- [ ] `init`, `add`, `check`, `status` work end-to-end manually
- [ ] Release plan computation is solid and tested
- [ ] Changelog generation produces correct output
- [ ] Review with human before building the release/version commands (they modify files and git state)

---

## Task 13: `version` command

**Description:** Bump versions in manifest files and update changelogs, without tagging. Optionally commits the changes. Supports `--dry-run`, `--no-commit`, and `--snapshot`.

**Acceptance criteria:**
- [ ] Computes release plan and applies version bumps to manifest files via adapters
- [ ] Updates/creates CHANGELOG.md
- [ ] Removes consumed changeset files from `.changeset/`
- [ ] `--no-commit`: makes changes but doesn't git commit
- [ ] `--dry-run`: prints what would happen, no file modifications
- [ ] `--snapshot <tag>`: sets version to `0.0.0-{tag}-{timestamp}`, doesn't consume changesets, doesn't update changelog, doesn't tag
- [ ] Without `--no-commit`: commits with configurable message, checks for dirty working tree first (exit 2 if dirty)
- [ ] Runs post-bump hooks after version update
- [ ] Exit 0 with message if no pending changesets

**Verification:**
- [ ] Integration test: create changeset, run version, verify manifest updated, changelog written, changeset deleted
- [ ] Test: --dry-run doesn't modify any files
- [ ] Test: --snapshot produces correct version format
- [ ] Test: dirty working tree -> exit 2

**Dependencies:** Tasks 3, 11, 12

**Files likely touched:**
- `src/cli/version.rs`
- `src/release/executor.rs` (shared file-modification logic)

**Estimated scope:** Medium (2-3 files)

---

## Task 14: `release` command

**Description:** The full release pipeline: version bump + changelog + remove changesets + commit + tag. Extends the version command with git tagging and JSON output.

**Acceptance criteria:**
- [ ] Does everything `version` does (bump, changelog, remove changesets, commit)
- [ ] Creates annotated git tag(s) with changelog excerpt as tag message
- [ ] Tag format: `v{version}` for single-package, `{name}@v{version}` for monorepo (configurable)
- [ ] `--dry-run`: shows full plan including tags, no modifications
- [ ] `--no-commit`: modifies files but doesn't commit or tag
- [ ] `--output json`: writes JSON summary to stdout (for GitHub Action consumption)
- [ ] Dirty working tree check (exit 2)
- [ ] No pending changesets: exit 0 with message, no side effects
- [ ] Configurable: `tag_annotated` (true/false), `commit_message` template

**Verification:**
- [ ] Integration test: full round-trip in temp git repo (init, add changeset, release, verify tag exists)
- [ ] Test: JSON output matches spec format
- [ ] Test: monorepo with two packages produces two tags
- [ ] Test: --dry-run doesn't create any tags or commits

**Dependencies:** Task 13

**Files likely touched:**
- `src/cli/release.rs`
- `src/release/executor.rs` (extend with tagging)

**Estimated scope:** Medium (2-3 files)

---

## Checkpoint: After Tasks 13-14

- [ ] Full round-trip works: init -> add -> check -> status -> release
- [ ] All tests pass
- [ ] Manual test in a real temp repo with Cargo.toml
- [ ] Manual test in a temp repo with package.json
- [ ] Manual test in a polyglot temp repo (both)
- [ ] Review with human before building GitHub Actions and distribution

---

## Task 15: Changesets compatibility test suite

**Description:** Port core test cases from the original changesets project (MIT licensed) for format compatibility. Three areas: parse, read, and release-plan. Extract test data into fixture files.

**Acceptance criteria:**
- [ ] Parse tests: ~20 fixture files covering frontmatter edge cases (Windows line endings, `---` in body, empty files, malformed frontmatter, scoped packages)
- [ ] Read tests: temp `.changeset/` directories with mixed files, verify correct filtering and parsing
- [ ] Release-plan tests: bump precedence, multi-package, none handling, snapshot versions
- [ ] All ported tests pass
- [ ] Tests are clearly marked as compat tests (separate module or test file)

**Verification:**
- [ ] `cargo test` passes all compat tests
- [ ] Each compat test references the original changesets test it was ported from (comment with source file/line)

**Dependencies:** Tasks 2, 6, 11

**Files likely touched:**
- `tests/fixtures/changesets-compat/parse/*.md`
- `tests/fixtures/changesets-compat/read/` (directory structures)
- `tests/fixtures/changesets-compat/release-plan/` (JSON input/output)
- `tests/compat_parse.rs`
- `tests/compat_read.rs`
- `tests/compat_release_plan.rs`

**Estimated scope:** Large (many fixture files, 3 test files; but no production code changes)

---

## Task 16: GitHub Actions - install + check

**Description:** Create the composite GitHub Actions for installing the CLI binary and running the changeset check on PRs. The check action posts/updates a PR comment summarizing the changeset.

**Acceptance criteria:**
- [ ] `actions/install/action.yml`: downloads binary from GitHub Releases, caches it, adds to PATH
- [ ] `actions/install/action.yml`: `version` input with `latest` default
- [ ] `actions/check/action.yml`: uses install action, runs `changesetter check --base $base`
- [ ] `actions/check/action.yml`: `base` input (default: auto-detect from PR context)
- [ ] `actions/check/action.yml`: `comment` input (default: `true`)
- [ ] When `comment: true`, posts a PR comment with changeset summary using `gh` or the GitHub API
- [ ] Comment is updated on subsequent pushes (uses a marker comment to find existing one)
- [ ] Actions are valid YAML and use composite action format

**Verification:**
- [ ] YAML linting passes
- [ ] Manual review of action.yml files against GitHub Actions composite action spec
- [ ] Action inputs/outputs documented in action.yml

**Dependencies:** Task 9 (check command must exist)

**Files likely touched:**
- `actions/install/action.yml`
- `actions/check/action.yml`

**Estimated scope:** Small (2 files)

---

## Task 17: GitHub Actions - release

**Description:** Create the release composite action that runs `changesetter release --output json` and creates GitHub Releases from the output. Direct mode only (version-PR is v0.2).

**Acceptance criteria:**
- [ ] `actions/release/action.yml`: uses install action, runs `changesetter release --output json`
- [ ] Parses JSON output to create GitHub Releases via `gh release create`
- [ ] Each release gets the `changelog` field as the release body
- [ ] Handles monorepo (multiple releases) and single-package (one release)
- [ ] `github-release` input (default: `true`) to control GitHub Release creation
- [ ] `draft` input (default: `false`) for draft releases
- [ ] Outputs: `released` (true/false), `releases` (JSON array)
- [ ] Pushes tags to remote before creating releases
- [ ] No-op when `changesetter release` reports no releases

**Verification:**
- [ ] YAML linting passes
- [ ] Manual review against spec
- [ ] Action inputs/outputs documented

**Dependencies:** Task 14 (release command must exist with --output json)

**Files likely touched:**
- `actions/release/action.yml`

**Estimated scope:** Small (1 file)

---

## Task 18: CI workflow and binary distribution

**Description:** Set up the project's own CI workflow (test, lint, clippy on PRs) and a release workflow that builds cross-platform binaries and publishes to GitHub Releases.

**Acceptance criteria:**
- [ ] `.github/workflows/ci.yml`: runs `cargo fmt --check`, `cargo clippy`, `cargo test` on PRs
- [ ] `.github/workflows/release.yml`: triggered by version tags (`v*`), builds binaries for 5 targets
- [ ] Targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`
- [ ] Binaries uploaded as GitHub Release assets
- [ ] Binary naming convention works with the install action from Task 16
- [ ] Uses `cross` or `cargo-zigbuild` for cross-compilation (or GitHub's matrix strategy with native runners)

**Verification:**
- [ ] CI workflow YAML is valid
- [ ] Release workflow YAML is valid
- [ ] Binary names match what `actions/install/action.yml` expects to download

**Dependencies:** Tasks 16, 17

**Files likely touched:**
- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`

**Estimated scope:** Small (2 files)

---

## Task 19: Dogfood - changesetter uses itself

**Description:** Set up changesetter to manage its own releases. Add changeset-check and release workflows, create the initial `.changeset/` directory, and add the first changeset for the v0.1 release.

**Acceptance criteria:**
- [ ] `.changeset/` directory exists in repo
- [ ] Changeset-check workflow runs on PRs
- [ ] Release workflow uses `actions/release` action
- [ ] Initial changeset for v0.1.0 release exists
- [ ] `changesetter check` passes in the repo

**Verification:**
- [ ] `cargo run -- check` exits 0 in the repo
- [ ] `cargo run -- status` shows the pending v0.1.0 changeset
- [ ] CI workflow files are valid YAML

**Dependencies:** Tasks 16, 17, 18

**Files likely touched:**
- `.changeset/*.md` (initial changeset)
- `.github/workflows/changeset-check.yml`
- `.github/workflows/release.yml` (update to use own actions)

**Estimated scope:** Small (3-4 files)

---

## Final checkpoint

- [ ] All tests pass: `cargo test`
- [ ] Lint clean: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
- [ ] Full round-trip works in a temp repo: init -> add -> check -> status -> version -> release
- [ ] Polyglot round-trip: Cargo.toml + package.json repo
- [ ] `none`-bump round-trip: add no-bump changeset, check passes, release includes it in changelog under "Internal"
- [ ] JSON output from release matches spec format
- [ ] GitHub Actions YAML files are valid
- [ ] Changesetter dogfoods itself
- [ ] Binary builds locally for at least one target
