# Plan: changesetter v0.2

Scope: everything in the v0.2 release phase from SPEC.md.

## Dependency graph

```
New adapters (Python, .NET, Helm)          [independent]
    └── Detector: register new manifest filenames

Pre-release mode (pre.json, pre command)   [independent]
    └── Release plan: wrap versions with pre tag
        └── Executor: read/write pre.json, increment counters

Fixed/linked groups                        [independent]
    └── Release plan: apply group logic before bump

Internal dependency cascading              [depends on adapters]
    └── Dependency graph reader (per-ecosystem)
        └── Release plan: cascade bumps to dependents

Version-PR mode                            [depends on release working]
    └── actions/release/action.yml rewrite

Documentation site                         [independent, last]
```

Build order: adapters -> groups -> pre-release -> dependency cascading -> version-PR -> docs site.

---

## Task 1: Python adapter (pyproject.toml)

**Description:** Add a Python package adapter that reads and writes versions in `pyproject.toml`. Must handle both PEP 621 (`project.version`) and Poetry (`tool.poetry.version`) layouts. Uses `toml_edit` (already a dependency) for structure-preserving edits.

**Acceptance criteria:**
- [ ] Detects `pyproject.toml`, extracts package name and version
- [ ] Reads from `project.version` (PEP 621) or `tool.poetry.version` (Poetry)
- [ ] Writes version back without clobbering other fields
- [ ] Package name from `project.name` or `tool.poetry.name`
- [ ] Registered in detector's `is_manifest` and adapter list

**Verification:**
- [ ] Unit tests: detect, read, write round-trip for both PEP 621 and Poetry layouts
- [ ] Fixture files in `tests/fixtures/adapters/python/`
- [ ] `cargo test` passes

**Dependencies:** None

**Files likely touched:**
- `src/package/python.rs` (new)
- `src/package/mod.rs`
- `src/package/types.rs` (add `Python` to `PackageType`)
- `src/package/detector.rs` (register `pyproject.toml`, add `PythonAdapter`)
- `src/release/executor.rs` (match new type in `apply_version_bump`)

**Estimated scope:** Medium

---

## Task 2: .NET adapter (.csproj)

**Description:** Add a .NET adapter that reads and writes `<Version>` in `.csproj` files. Uses `quick-xml` for XML parsing. Must preserve existing structure, comments, and conditional `PropertyGroup` elements. Detects by globbing `*.csproj` files.

**Acceptance criteria:**
- [ ] Detects `.csproj` files, extracts assembly name and version
- [ ] Reads `<Version>` from any `<PropertyGroup>` (uses first match)
- [ ] Writes version back preserving XML structure and comments
- [ ] Package name from `<AssemblyName>` or filename stem
- [ ] Handles missing `<Version>` element gracefully (skips, no crash)

**Verification:**
- [ ] Unit tests: detect, read, write round-trip
- [ ] Fixture `.csproj` files with comments, conditions, multiple PropertyGroups
- [ ] `cargo test` passes

**Dependencies:** None

**Files likely touched:**
- `src/package/dotnet.rs` (new)
- `src/package/mod.rs`
- `src/package/types.rs` (add `Dotnet` to `PackageType`)
- `src/package/detector.rs` (register `*.csproj`, add `DotnetAdapter`)
- `src/release/executor.rs` (match new type)
- `Cargo.toml` (add `quick-xml` dependency)
- `tests/fixtures/adapters/dotnet/`

**Estimated scope:** Medium

---

## Task 3: Helm adapter (Chart.yaml)

**Description:** Add a Helm adapter that reads and writes `version` in `Chart.yaml`. Simple YAML field, uses `serde_yaml` (already a dependency) but writes back with `toml_edit`-style preservation via raw string manipulation to avoid reordering fields.

**Acceptance criteria:**
- [ ] Detects `Chart.yaml`, extracts chart name and version
- [ ] Reads `version` field
- [ ] Writes version back without reordering other YAML fields
- [ ] Package name from `name` field
- [ ] Registered in detector

**Verification:**
- [ ] Unit tests: detect, read, write round-trip
- [ ] Fixture `Chart.yaml` files
- [ ] `cargo test` passes

**Dependencies:** None

**Files likely touched:**
- `src/package/helm.rs` (new)
- `src/package/mod.rs`
- `src/package/types.rs` (add `Helm` to `PackageType`)
- `src/package/detector.rs` (register `Chart.yaml`, add `HelmAdapter`)
- `src/release/executor.rs` (match new type)

**Estimated scope:** Small

---

## Checkpoint: After Tasks 1-3

- [ ] All tests pass
- [ ] `cargo clippy` and `cargo fmt --check` clean
- [ ] Polyglot detection works: repo with Cargo.toml + package.json + pyproject.toml + Chart.yaml detects all four
- [ ] Integration test: version bump round-trip for each new adapter
- [ ] Review with human before proceeding

---

## Task 4: Fixed package groups

**Description:** Implement fixed group logic in the release plan assembler. Packages in a `fixed` group always bump together to the same version. If `core-lib` gets a `minor` changeset but `core-macros` has no changeset, both bump to `minor`. The group's version is the highest current version among members, bumped by the highest bump in the group.

**Acceptance criteria:**
- [ ] Config `[groups.X] fixed = ["a", "b"]` parsed (already deserializes, needs logic)
- [ ] If any member has a changeset, all members appear in the release plan
- [ ] All members bump to the same version (highest current + highest bump)
- [ ] Members without changesets get an auto-generated changelog entry
- [ ] Config validation: error if a package appears in multiple groups

**Verification:**
- [ ] Unit tests in `release/plan.rs` for fixed group scenarios
- [ ] Test: one member has minor, other has no changeset -> both get minor
- [ ] Test: two members, one patch one minor -> both get minor
- [ ] Test: package in two groups -> config validation error
- [ ] `cargo test` passes

**Dependencies:** None

**Files likely touched:**
- `src/release/plan.rs` (modify `assemble` to accept config, apply fixed groups)
- `src/config.rs` (add validation method)
- callers of `assemble` (pass config)

**Estimated scope:** Medium

---

## Task 5: Linked package groups

**Description:** Implement linked group logic. Packages in a `linked` group share version numbers but only bump when individually changed. When multiple members bump in the same release, they coordinate to the highest bump level. Unlike fixed, members without changesets don't bump.

**Acceptance criteria:**
- [ ] Config `[groups.X] linked = ["a", "b"]` activates linked behavior
- [ ] Only members with changesets appear in the release plan
- [ ] When multiple linked members bump, they all use the highest bump level in the group
- [ ] Linked members converge to the same version number over time
- [ ] Works correctly with fixed groups in the same config (different group names)

**Verification:**
- [ ] Unit tests: only-one-bumps, both-bump-to-highest, mixed with fixed
- [ ] `cargo test` passes

**Dependencies:** Task 4 (shared group infrastructure)

**Files likely touched:**
- `src/release/plan.rs`

**Estimated scope:** Small

---

## Task 6: Internal dependency cascading

**Description:** When package A bumps and package B depends on A, optionally cascade a bump to B. Requires reading dependency information from manifest files (ecosystem-aware). The `update_internal_dependencies` config controls behavior: `"patch"` always cascades, `"minor"` only when the range breaks, `"none"` disables.

**Acceptance criteria:**
- [ ] Each adapter can extract internal dependencies (Cargo: `[dependencies]`, npm: `dependencies`/`devDependencies`, Python: `project.dependencies`, .NET: `<PackageReference>`, Helm: `dependencies` in Chart.yaml)
- [ ] Build a dependency graph of detected packages
- [ ] After computing bumps, cascade to dependents based on config
- [ ] `"patch"` mode: any bumped package cascades at least a patch to dependents
- [ ] `"minor"` mode: only cascade if the version change breaks the dependent's range
- [ ] `"none"` mode: no cascading
- [ ] Cascaded bumps include an auto-generated changelog entry

**Verification:**
- [ ] Unit tests with mock dependency graphs
- [ ] Integration test: two Cargo crates where A depends on B
- [ ] Test each cascade mode
- [ ] `cargo test` passes

**Dependencies:** Tasks 1-3 (adapters must exist), Task 4 (groups)

**Files likely touched:**
- `src/package/adapter.rs` (add `dependencies` method to trait)
- `src/package/cargo.rs`, `npm.rs`, `python.rs`, `dotnet.rs`, `helm.rs` (implement)
- `src/release/plan.rs` (cascade logic after group resolution)
- `src/release/deps.rs` (new, dependency graph builder)
- `src/release/mod.rs`

**Estimated scope:** Large (split across many files but each change is small)

---

## Checkpoint: After Tasks 4-6

- [ ] All tests pass
- [ ] Fixed groups: two crates, one changeset, both bump
- [ ] Linked groups: two crates, one changeset, only one bumps; two changesets, both converge
- [ ] Cascading: A depends on B, B bumps, A gets a patch cascade
- [ ] Manual test in a temp monorepo
- [ ] Review with human before proceeding

---

## Task 7: Pre-release mode - `pre` command and state

**Description:** Add the `changesetter pre` subcommand with `enter`, `exit`, and `status` subcommands. Manages `.changeset/pre.json` which tracks mode, tag, and per-package release counters.

**Acceptance criteria:**
- [ ] `pre enter rc` creates `.changeset/pre.json` with `{"mode":"pre","tag":"rc","packages_released":{}}`
- [ ] `pre exit` sets mode to `"exit"` (consumed on next release)
- [ ] `pre status` prints current pre-release state
- [ ] Error if `pre enter` called while already in pre mode
- [ ] Error if `pre exit` called while not in pre mode
- [ ] `pre.json` is committed by the user (not auto-committed)

**Verification:**
- [ ] Unit tests for enter/exit/status state transitions
- [ ] `cargo test` passes
- [ ] `cargo run -- pre enter rc` creates valid JSON

**Dependencies:** None

**Files likely touched:**
- `src/cli/pre.rs` (new)
- `src/cli/mod.rs` (add `Pre` command)
- `src/main.rs` (route `Pre` command)
- `src/release/pre.rs` (new, pre.json read/write/types)
- `src/release/mod.rs`

**Estimated scope:** Medium

---

## Task 8: Pre-release version computation

**Description:** Modify the release plan assembler and executor to produce pre-release versions when pre mode is active. A `minor` bump on `0.5.0` in `rc` mode produces `0.6.0-rc.0`. Subsequent releases increment the counter: `0.6.0-rc.1`. Exiting pre mode and releasing produces the stable version `0.6.0`.

**Acceptance criteria:**
- [ ] In pre mode: versions get `-{tag}.{counter}` suffix
- [ ] Counter increments per package per release cycle (tracked in `pre.json`)
- [ ] `pre.json` updated after release with new counter values
- [ ] On exit: next release strips the pre suffix, produces stable version
- [ ] Snapshot releases (`--snapshot`) ignore pre mode entirely
- [ ] Changelog entries still generated normally

**Verification:**
- [ ] Unit tests: pre-release version computation, counter increment, exit-to-stable
- [ ] Integration test: enter rc, add changeset, release -> `1.0.0-rc.0`, add another, release -> `1.0.0-rc.1`, exit, release -> `1.0.0`
- [ ] `cargo test` passes

**Dependencies:** Task 7

**Files likely touched:**
- `src/release/plan.rs` (accept pre state, modify version computation)
- `src/release/executor.rs` (read/write pre.json, pass to plan)
- `src/release/pre.rs` (counter logic)

**Estimated scope:** Medium

---

## Checkpoint: After Tasks 7-8

- [ ] All tests pass
- [ ] Pre-release round-trip works: enter -> add -> release -> versions have `-rc.0` suffix
- [ ] Exit -> release -> stable versions
- [ ] `changesetter status` shows pre-release versions when in pre mode
- [ ] Review with human before proceeding

---

## Task 9: Version-PR mode in release action

**Description:** Rewrite `actions/release/action.yml` to support the version-PR state machine. When `version-pr: true`, instead of releasing directly, the action creates/updates a "Version Packages" PR with the computed version changes. Merging that PR triggers the actual release.

**Acceptance criteria:**
- [ ] `version-pr` input (default: `false`) enables version-PR mode
- [ ] Pending changesets: creates/updates branch `changesetter/version-packages` and PR with label `changesetter:version`
- [ ] No pending changesets + merged version PR detected: runs release + creates GitHub Releases
- [ ] No pending changesets + no merged version PR: no-op
- [ ] PR body shows: packages to bump, new versions, changelog preview
- [ ] Force-pushes version branch on updates (idempotent)
- [ ] `version-pr-title` input for custom PR title
- [ ] Outputs: `released`, `releases`, `version-pr`

**Verification:**
- [ ] YAML is valid composite action syntax
- [ ] Manual review of state machine logic against spec
- [ ] Action inputs/outputs documented

**Dependencies:** Tasks 1-8 (release pipeline must be complete)

**Files likely touched:**
- `actions/release/action.yml` (major rewrite)

**Estimated scope:** Medium (one file but complex logic)

---

## Task 10: Documentation site scaffold

**Description:** Create a fumadocs site in `docs/` with the initial content pages. Fumadocs is a Next.js documentation framework using MDX. The site covers getting started, CLI reference, configuration, changeset format, GitHub Actions, adapters, and recipes.

**Acceptance criteria:**
- [ ] `docs/` contains a working fumadocs Next.js app
- [ ] `npm run dev` in `docs/` starts the dev server
- [ ] Getting started page: install, init, add, check, release flow
- [ ] CLI reference page: all commands with flags and exit codes
- [ ] Configuration page: full `changesetter.toml` reference
- [ ] Changeset format page: frontmatter rules, bump levels, examples

**Verification:**
- [ ] `cd docs && npm install && npm run build` succeeds
- [ ] Pages render correctly in dev server
- [ ] No broken links between pages

**Dependencies:** None (can be built any time, but content accuracy requires features to exist)

**Files likely touched:**
- `docs/` (new directory tree)
- `docs/package.json`
- `docs/next.config.mjs`
- `docs/content/docs/*.mdx` (6+ content files)

**Estimated scope:** Large (many files, but all content/config, no Rust code)

---

## Task 11: Documentation content - Actions, adapters, recipes

**Description:** Complete the docs site with the remaining content pages: GitHub Actions setup, adapter ecosystem docs, and recipe pages for monorepo setup, pre-releases, and migration from changesets.

**Acceptance criteria:**
- [ ] GitHub Actions page: check + release setup, version-PR pattern, permissions
- [ ] Adapters page: supported ecosystems, detection rules, version field locations
- [ ] Recipe: monorepo setup with fixed/linked groups
- [ ] Recipe: pre-releases workflow
- [ ] Recipe: migrating from changesets (file format compat, config differences)

**Verification:**
- [ ] `npm run build` in `docs/` succeeds
- [ ] All recipe examples are accurate against current CLI behavior
- [ ] No placeholder content

**Dependencies:** Tasks 1-9 (all features must be implemented for accurate docs)

**Files likely touched:**
- `docs/content/docs/github-actions.mdx`
- `docs/content/docs/adapters.mdx`
- `docs/content/docs/recipes/monorepo.mdx`
- `docs/content/docs/recipes/pre-releases.mdx`
- `docs/content/docs/recipes/migration.mdx`

**Estimated scope:** Medium (content writing, no code)

---

## Task 12: Docs deployment workflow

**Description:** Add a GitHub Actions workflow to build and deploy the docs site to GitHub Pages on push to main.

**Acceptance criteria:**
- [ ] `.github/workflows/docs.yml` builds the fumadocs site and deploys to GitHub Pages
- [ ] Only triggers when `docs/` files change (path filter)
- [ ] Uses `actions/configure-pages`, `actions/upload-pages-artifact`, `actions/deploy-pages`

**Verification:**
- [ ] YAML is valid
- [ ] Path filter only matches `docs/**`

**Dependencies:** Task 10

**Files likely touched:**
- `.github/workflows/docs.yml` (new)

**Estimated scope:** Small

---

## Checkpoint: After Tasks 9-12

- [ ] All Rust tests pass
- [ ] Docs site builds cleanly
- [ ] Version-PR action is valid YAML with correct state machine
- [ ] Review full v0.2 feature set with human

---

## Task 13: v0.2 integration tests and release

**Description:** Add integration tests for all new v0.2 features and prepare the v0.2 release. Update the README with new features. Add a changeset for v0.2.

**Acceptance criteria:**
- [ ] Integration tests for: Python adapter round-trip, .NET adapter round-trip, Helm adapter round-trip
- [ ] Integration tests for: fixed group release, linked group release
- [ ] Integration tests for: pre-release enter/release/exit cycle
- [ ] Integration tests for: dependency cascading
- [ ] README updated with new adapters, pre-release docs, group config
- [ ] Changeset added for v0.2 release

**Verification:**
- [ ] `cargo test` passes (all existing + new tests)
- [ ] `cargo fmt --check && cargo clippy --all-targets -- -D warnings` clean
- [ ] `changesetter status` shows pending v0.2 changeset
- [ ] Manual round-trip in a polyglot temp repo (Cargo + npm + Python)

**Dependencies:** All previous tasks

**Files likely touched:**
- `tests/integration.rs` (extend)
- `tests/compat_release_plan.rs` (extend with group/cascade tests)
- `README.md`
- `.changeset/*.md`

**Estimated scope:** Medium

---

## Final checkpoint

- [ ] All tests pass: `cargo test`
- [ ] Lint clean: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
- [ ] Adapters: Cargo, npm, Python, .NET, Helm all detect and bump
- [ ] Groups: fixed and linked work in release plan
- [ ] Pre-release: enter -> release -> exit -> stable release cycle works
- [ ] Cascading: dependent packages bump when their dependency bumps
- [ ] Version-PR: action YAML is complete and matches spec state machine
- [ ] Docs: site builds, all pages have content, deployment workflow exists
- [ ] README updated for v0.2
- [ ] Changeset exists for v0.2 release
