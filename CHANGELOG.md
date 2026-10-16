# Changelog






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
