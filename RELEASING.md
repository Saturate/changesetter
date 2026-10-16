# Releasing

This repo ships two things on separate version lines: the `changesetter` CLI and the GitHub Actions under `actions/`.

## CLI

CLI releases are automatic. Merging a PR with a changeset to `main` runs `.github/workflows/release.yml`, which:

1. Runs `changesetter release`: bumps `Cargo.toml`, runs the `post_bump` hook from `changesetter.toml` (`cargo update --workspace`, so `Cargo.lock` matches), writes `CHANGELOG.md`, commits and tags `changesetter@v<version>`.
2. Creates the GitHub Release with `--latest=false`.
3. Calls `build-binaries.yml`, which builds the binaries, attaches them to the release and then marks it as latest. Publishing to crates.io runs alongside; a failed publish doesn't hold back the binaries, since `actions/install` only needs those.

`actions/install` doesn't use GitHub's "latest" flag. For `version: latest` it picks the highest stable `changesetter@vX.Y.Z` release that already has the runner's binary attached, so a release whose build is still running is skipped until its upload lands. The flag still matters for people browsing the releases page, which is why releases start as not-latest. The release workflow doesn't cancel in-progress runs, so a build is never cut off halfway.

To pull a bad release back from `actions/install`, mark it as a pre-release or delete its assets; install then falls back to the previous version. Changing which release is marked latest has no effect on install.

If a release ends up without binaries or isn't on crates.io, run **Build Binaries** manually (Actions tab, "Run workflow") with the tag, e.g. `changesetter@v0.3.2`. It replaces existing assets and skips crates.io if that version is already published. Tick "Mark the release as latest once binaries are attached" if it should be shown as the latest release on GitHub. If the GitHub Release itself is missing, create it first, since the build only uploads to an existing release: `gh release create <tag> --verify-tag --latest=false --notes-file <notes>`, with that version's section of `CHANGELOG.md` as the notes (`actions/release/release-notes.sh <version>` prints it).

Tags up to `changesetter@v0.3.1` were cut before the `post_bump` hook existed, so their `Cargo.lock` is stale and `cargo publish` from them fails on a dirty tree. 0.3.1 is not on crates.io for that reason; it only changed the GitHub Actions, so the next release covers it.

## GitHub Actions

The actions follow GitHub's recommended versioning: an immutable `v1.x.y` tag per release, plus a major tag `v1` that is moved to the newest compatible release. Users on `@v1` get fixes; users who pin `@v1.x.y` or a SHA keep that action code. `check` and `release` call `actions/install@v1` internally, though, so the install step follows `v1` for everyone. Keep `actions/install` backwards compatible within v1.

To release the actions from the commit on `main` you want to ship:

```sh
git tag -a v1.2.0 -m "v1.2.0 of the GitHub Actions"
git tag -fa v1 -m "v1: latest v1.x of the GitHub Actions (currently v1.2.0)"
git push origin v1.2.0
git push --force origin v1
gh release create v1.2.0 --verify-tag --latest=false --title "Actions v1.2.0" --notes "..."
```

- Bump the patch for fixes, the minor for new optional inputs or outputs. Changes outside `actions/` don't need an action release.
- A change that removes or renames an input or output, changes required permissions, or otherwise breaks an existing workflow is `v2.0.0`. Tag `v2` for it and leave `v1` where it was.
- Always pass `--latest=false`, so the releases page keeps showing the CLI as latest. (`actions/install` ignores the flag and only looks at `changesetter@v*` releases.)
- Plain `vX.Y.Z` tags don't trigger the binary build; only `changesetter@v*` tags do.
- In the release notes, call out behaviour changes that make a job fail where it used to pass.
