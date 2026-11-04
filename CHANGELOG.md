# Changelog

## 0.4.3 - 2026-10-22

### changesetter

#### Releases with no name read cleanly

A package with no name, such as a Cargo workspace root versioned through `[workspace.package]`, broke several release strings. The release action's version PR "Changelog preview" came out as `### 0.1.0@CHANGELOG.md` with an empty body, because the empty name was dropped when the release fields were split. The preview now reads the right section and heads it with the bare version. The GitHub Release is titled `v0.1.0` instead of ` v0.1.0`, the release commit says `chore: release 0.1.0` instead of `chore: release @0.1.0`, and `changesetter status` no longer leaves a stray space before the version.

## 0.4.2 - 2026-10-22

### changesetter

#### None-bump entries without a release go under Unreleased

A release run with only `none` changesets wrote an `### Internal` section with no version heading above it, so it ended up inside the next version's section and its release notes. With `per_package = true`, `none` entries for packages that weren't being released (including a repo-wide `default: none`) were dropped. These entries now go under an `## Unreleased` heading at the top of the right changelog (every package's changelog for `default: none` in per-package mode, but not a private workspace root), and the next release written to that changelog folds them into its version section, tag message and GitHub release notes. A none-only run commits as `chore: add unreleased changelog notes` and creates no tags. New sections always go at the top, even in a changelog without a title, Keep a Changelog's `## [Unreleased]` spelling and CRLF line endings are kept, and release notes stop at Unreleased headings, `## [1.0.0]` or `## v1.0.0` headings and version link references. Headings in changeset bodies now render one level below the entry heading: a `## Title` on the first line becomes `####`, and later `#`, `##`, `###` or `####` headings (Setext ones too) become `#####`, so a heading in a body can't cut a version's notes short. Version link references in a body (`[1.0.0]: https://...`) get one space of indent so they don't end the entry either. Indented headings keep their indent, and `===`/`---` only counts as an underline below a plain paragraph. Code blocks are left as written, and one a body never closes is closed at the end of that body. A line starting with `## ` or `### ` inside a code block in a changeset body still ends the entry's section, so `changesetter add` and `status` now warn about it. The version PR's changelog preview now shows each release's full section, absorbed notes included. `none_bump = "omit"` still writes nothing.

## 0.4.1 - 2026-10-21

### changesetter

#### None-bump changesets stay out of the release notes

A `none` changeset for a package that also gets a real bump was folded into that release's changelog entry, tag message and GitHub release notes. It now goes under the none-bump heading ("Internal" by default), or nowhere with `none_bump = "omit"`, as the docs describe. The version PR's changelog preview also shows the changelog on a first release, when CHANGELOG.md is a new file.

### Internal

The release action's version-PR mode now tags every package a version PR
bumped, in the configured tag format, finishes a release a cancelled run left
halfway, and finds pending changesets through the CLI. Direct mode now pushes
its release commit and fails when `changesetter release` fails.

Revert two commits that were pushed to main by accident.


## 0.4.0 - 2026-10-16

### changesetter

The install action now only picks CLI releases that have binaries when
resolving `latest`, authenticates its API call to avoid shared rate limits,
and reports a clear error when a version has no binary.

#### `status --output json`

`changesetter status --output json` prints pending changesets, each
package's current version and tag (and whether that tag exists), and the
releases that would be made. CI integrations use it instead of globbing
`.changeset/` or building tag names by hand.


## 0.3.3 - 2026-10-16

### changesetter

Reading the changeset directory no longer follows symlinked subdirectories, so
a link like `.changeset/loop -> ..` can't crash changesetter with a stack
overflow. Symlinked `.md` files are still read. `changeset_dir` in
`changesetter.toml` must now be a relative path inside the repository:
absolute paths and `..` are rejected at load with a clear error, and a
trailing slash (`.changeset/`) is accepted and normalised.

CI now fails a release run when `changesetter release` fails instead of
reporting success, rejects a stale Cargo.lock, and lints workflows and
action scripts.


## 0.3.2 - 2026-10-16

### changesetter

Fix version PR not being created after the first release. The check-merged
step matched historically merged version PRs instead of checking whether HEAD
is the actual merge commit. The lookup now asks GitHub which PR HEAD belongs
to, and a failed lookup (bad token, rate limit) fails the job instead of
silently skipping the release. The version-PR docs now use
`cancel-in-progress: false`, since cancelling the merge commit's run skips
that release.

Release binaries are built for every CLI release again. Tags pushed by the
release workflow never triggered the binary build, so `changesetter@v0.3.1`
shipped without binaries and `actions/install` failed for everyone.


## 0.3.1 - 2026-10-16

### changesetter

Fix the changeset check workflow reporting "Missing" on every PR and failing
when it posts its comment.

Fix the release action's notes and error handling. Release notes kept only
part of a version's section when it was the last one in CHANGELOG.md (every
first release), and the version could match a longer one (`1.0.1` in
`1.0.10`). Every version in a shared monorepo heading now matches, not only
the first, and a changeset heading like `## 2FA support` no longer ends the
notes early. A failing `changesetter version`, such as a `post_bump` hook, no
longer opens a half-bumped version PR; the job now fails instead, which
includes a malformed changeset file that used to give an empty version PR.


## 0.3.0 - 2026-08-10

### changesetter

#### Publish to crates.io on version tags

#### Support custom changeset directory and recursive reading


## 0.2.0 - 2026-08-03

### changesetter

#### Add Python, .NET, and Helm adapters, pre-release mode, fixed/linked package groups, dependency cascading, version-PR action, and documentation site


## 0.1.1 - 2026-07-31

#### Initial release

Polyglot changeset management CLI with support for Cargo and npm
ecosystems. Includes init, add, check, status, version, and release
commands. Ships GitHub Actions for CI integration.

#### Improve release pipeline, fix git tag annotation stripping, fix action script injection

#### Add comprehensive test suites: compat read, integration, and insta snapshots (162 total tests)
