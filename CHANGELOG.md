# Changelog

## 0.7.0 - 2026-11-12

### changesetter

Pull request, commit and issue links in changelog entries

`[changelog] links = true` ends each grouped changelog entry with the pull request and commit it came from, like `([#31](…/pull/31), [63c42f2](…/commit/…))`, and turns issue references such as `#12` into links. `[[changelog.issue_links]]` adds other trackers (`prefix = "GAME-"`, `url = "https://jira…/GAME-{id}"`). Links point at the `origin` remote, or `repo_url`, and understand GitHub, GitLab and Azure DevOps.

Where each changeset came from is read from local git, with no network calls: the commit that added the file, its author, and the pull request number from a squash-merge subject or the merge commit that brought it in. `release` and `status --output json` now include `commit`, `author` and `pr` for every entry.

On Windows, changesets in subdirectories are now named with `/` (`team/fix`) in `status --output json` and everywhere else, as on other platforms.

## 0.6.0 - 2026-11-12

### changesetter

Grouped changelogs and typed changesets

Changesets can carry a conventional `type:` and `scope:` in their front matter (`changesetter add --type fix --scope ui`), or a prefix on the summary like `fix(ui): ...`. Without either, the type follows the bump. Set `group = true` under `[changelog]`, or list your own `[[changelog.groups]]` with `types`, a `match` glob and `skip` (which never hides a breaking entry), to list each new version's entries under a heading per group. The default stays a flat list. The bump alone still decides the version, and a `breaking` type on a non-major bump warns.

`release --output json` and `status --output json` now list every entry's id, bump, type, scope, group and summary, for releases and none-bump entries alike. The `--dry-run` summary now goes to stderr, so with `--output json` only JSON is on stdout, and a run with nothing to release prints JSON with empty lists instead of nothing.

`changesetter add -m "text"` now writes the message as plain text instead of a `####` heading.

Docs: type and scope, grouped changelogs, the JSON entries, plain `vX.Y.Z` tags for a single configured package, and a recipe for building before committing with `release --no-commit`.

### Internal

Fixed docs links that pointed at `/docs/...` and returned 404, since the site serves pages from the root. Removed the GitHub Pages workflow, which failed on every docs change because the docs are hosted on Vercel.

## 0.5.2 - 2026-11-09

### changesetter

#### `check --base` passes when nothing changed

`changesetter check --base <ref>` failed with "no changeset files found" even when the branch had no changes since `<ref>`, for example a local gate running on `main` against `origin/main`. With no commits, staged, unstaged or untracked files since the base, there's nothing to describe, so the check now passes and says so. Any change still needs a changeset, including one outside `.changeset/`.

## 0.5.1 - 2026-11-09

### changesetter

#### `check --base` counts changesets you haven't committed yet

`changesetter check --base <ref>` used to compare commits only, so a pre-commit hook or local gate couldn't find the changeset it was about to commit. It now counts changesets added since the branch left `<ref>` whether they are committed, staged or untracked. Files matched by `.gitignore` don't count, and neither do changesets deleted from the working tree. CI checkouts are clean, so results there are unchanged.

The repo now ships a `LICENSE` file with the MIT terms that `Cargo.toml` already declared, and the published crate includes it.

## 0.5.0 - 2026-11-09

### changesetter

#### Godot projects

changesetter now versions Godot projects. It finds `project.godot` and reads the name from `config/name` and the version from `config/version`, both under `[application]`. A release replaces only the `config/version` line and leaves the rest of the file byte for byte, line endings included. Godot leaves the key out until a version is set. Auto-detection skips projects without it, so a demo project inside a library repo doesn't turn into a package. To start versioning one, list it under `[[package]]` with `type = "godot"`: it reads as `0.0.0`, and the first release inserts the line right after `config/name`. A version that isn't `x.y.z`, such as `"1.0"`, is an error naming the file. If your repo has a versioned `project.godot` you don't release, add its name to `ignore`.

`name` and `type` under `[[package]]` now do what the docs say. Before, both were ignored. `name` replaces the name read from the manifest, so a game called `"Slopocalypse"` can be released as `slopocalypse: minor`, and the auto-detected entry under the old name goes away. `type` picks the adapter for the path, which matters when a directory has more than one manifest, and an unknown `type` is now an error. If your config sets a `name` that differs from the manifest, changesets must now use the configured name.

##### Wildcards in `ignore`

`ignore` entries can now use `*` and `?`, so `ignore = ["*_core", "*_gd"]` covers every crate with those suffixes, including ones added later. Entries without wildcards still match the whole name exactly. A nameless package, such as a Cargo workspace root, is ignored with `""`.

## 0.4.4 - 2026-11-04

### changesetter

#### None-bump changesets stay out of the release notes

A `none` changeset for a package that also gets a real bump was folded into that release's changelog entry, tag message and GitHub release notes. It now goes under the none-bump heading ("Internal" by default), or nowhere with `none_bump = "omit"`, as the docs describe. The version PR's changelog preview also shows the changelog on a first release, when CHANGELOG.md is a new file.

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
