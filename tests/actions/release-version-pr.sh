#!/usr/bin/env bash
# Runs the version-PR steps of actions/release/action.yml against real git
# repos (a bare origin and a clone per scenario) with a fake `gh`, since the
# flow can't be exercised by PR CI without merging a real version PR.
# Usage: tests/actions/release-version-pr.sh <changesetter binary>
#
# The check expressions are single-quoted on purpose: `check` evals them later.
# shellcheck disable=SC2016
set -uo pipefail

REPO=$(cd "$(dirname "$0")/../.." && pwd)
CLI=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
ROOT=$(mktemp -d "${TMPDIR:-/tmp}/cs-harness.XXXXXX")
STEPS="$ROOT/steps"
mkdir -p "$STEPS"
ruby -ryaml -e '
  steps = YAML.load_file(ARGV[0])["runs"]["steps"]
  %w[check-merged merged-release check-pending version-pr].each do |id|
    File.write(File.join(ARGV[1], "#{id}.sh"), steps.find { |s| s["id"] == id }.fetch("run"))
  end' "$REPO/actions/release/action.yml" "$STEPS"
BIN="$ROOT/bin"
mkdir -p "$BIN"
cp "$CLI" "$BIN/changesetter"
export PATH="$BIN:$PATH"
export GIT_CONFIG_COUNT=4
export GIT_CONFIG_KEY_0=user.name GIT_CONFIG_VALUE_0=t
export GIT_CONFIG_KEY_1=user.email GIT_CONFIG_VALUE_1=t@t
export GIT_CONFIG_KEY_2=commit.gpgsign GIT_CONFIG_VALUE_2=false
export GIT_CONFIG_KEY_3=tag.gpgsign GIT_CONFIG_VALUE_3=false

cat > "$BIN/gh" <<'GH'
#!/usr/bin/env bash
F="$GH_FIXTURES"
echo "gh $*" >> "$F/calls.log"
case "$1" in
  api)
    [ -n "${GH_FAIL_API:-}" ] && { echo "gh: HTTP 401" >&2; exit 1; }
    ep=$2; shift 2; jqf=.
    while [ $# -gt 0 ]; do case "$1" in --jq) jqf=$2; shift 2 ;; *) shift ;; esac; done
    case "$ep" in
      *"/commits/"*"/pulls") sha=${ep#*commits/}; sha=${sha%/pulls}; f="$F/commit-$sha.json" ;;
      *"/pulls?"*) f="$F/pulls.json" ;;
      *) echo "unexpected endpoint $ep" >&2; exit 1 ;;
    esac
    [ -f "$f" ] || f="$F/empty.json"
    jq -r "$jqf" "$f" ;;
  pr)
    # Keep the body the version PR would get.
    while [ $# -gt 0 ]; do case "$1" in --body) printf '%s\n' "$2" > "$F/pr-body.md"; shift 2 ;; *) shift ;; esac; done ;;
  release)
    case "$2" in
      view) grep -qx "$3" "$F/releases.txt" 2>/dev/null ;;
      create) echo "$3" >> "$F/releases.txt"; echo "$*" >> "$F/created.log" ;;
    esac ;;
  *) ;;
esac
GH
chmod +x "$BIN/gh"

PASS=0; FAIL=0
check() { if eval "$2"; then echo "  ok   $1"; PASS=$((PASS+1)); else echo "  FAIL $1"; FAIL=$((FAIL+1)); fi; }

# new_repo <name>: bare origin + clone in $W, single Cargo package at 1.0.0, tagged v1.0.0
new_repo() {
  O="$ROOT/$1-origin.git"; W="$ROOT/$1-work"; FX="$ROOT/$1-fx"
  git init -q --bare -b main "$O"; git clone -q "$O" "$W" 2>/dev/null; mkdir -p "$FX"
  echo '[]' > "$FX/empty.json"; echo '[]' > "$FX/pulls.json"
  cd "$W" || exit 1
  printf '[package]\nname = "pkg"\nversion = "1.0.0"\n' > Cargo.toml
  printf '# Changelog\n\n## 1.0.0 - 2026-01-01\n\n- First\n' > CHANGELOG.md
  git add -A; git commit -qm init; git tag -a v1.0.0 -m v1.0.0; git push -q origin main v1.0.0
}

# version_pr_merge <new version> [extra file]: merges a version PR bump with --no-ff, prints merge sha
version_pr_merge() {
  git checkout -qb changesetter/version-packages
  sed -i.bak "s/^version = .*/version = \"$1\"/" Cargo.toml && rm Cargo.toml.bak
  printf '# Changelog\n\n## %s - 2026-02-01\n\n- Notes for %s\n\n## 1.0.0 - 2026-01-01\n\n- First\n' "$1" "$1" > CHANGELOG.md
  git commit -qam "chore: version packages"
  git checkout -q main
  if [ -n "${2:-}" ]; then mkdir -p .changeset; printf -- '---\npkg: patch\n---\n\nLate change.\n' > ".changeset/$2.md"; git add -A; git commit -qm "late changeset"; fi
  git merge -q --no-ff changesetter/version-packages -m "Merge version PR"
  git push -q origin main
  git rev-parse HEAD
}

label_merge() { echo "[{\"number\": 5, \"merge_commit_sha\": \"$1\", \"labels\": [{\"name\": \"changesetter:version\"}]}]" > "$FX/commit-$1.json"; }

run_steps() {
  OUT="$FX/out-$1"; : > "$OUT"
  export GH_FIXTURES="$FX" GITHUB_OUTPUT="$OUT" GH_TOKEN=x GITHUB_ACTION_PATH="$REPO/actions/release" RUNNER_TEMP="$FX/tmp-$1" GITHUB_RELEASE=true DRAFT=false
  mkdir -p "$RUNNER_TEMP"
  bash -eo pipefail "$STEPS/check-merged.sh" > "$FX/log-$1" 2>&1 || { echo "check-merged failed" >> "$FX/log-$1"; return 1; }
  if grep -q '^merged=true' "$OUT"; then
    RELEASE_SHA=$(sed -n 's/^release-sha=//p' "$OUT") bash -eo pipefail "$STEPS/merged-release.sh" >> "$FX/log-$1" 2>&1 || { echo "merged-release failed" >> "$FX/log-$1"; return 1; }
  fi
  bash -eo pipefail "$STEPS/check-pending.sh" >> "$FX/log-$1" 2>&1
}
out() { sed -n "s/^$2=//p" "$FX/out-$1" | tail -1; }
remote_tag_at() { git -C "$W" ls-remote "$O" "refs/tags/$1^{}" | cut -f1; }

echo "A: HEAD is the version PR merge"
new_repo a; M=$(version_pr_merge 1.1.0); label_merge "$M"
run_steps 1; check "tag v1.1.0 on origin at the merge" '[ "$(remote_tag_at v1.1.0)" = "$M" ]'
check "released=true" '[ "$(out 1 released)" = true ]'
check "GitHub Release created with title" 'grep -q "v1.1.0 --verify-tag --title pkg v1.1.0" "$FX/created.log"'
check "notes are the 1.1.0 section" 'git -C "$W" fetch -q --tags origin; git -C "$W" tag -l --format="%(contents)" v1.1.0 | grep -qx "\- Notes for 1.1.0"'
check "no pending" '[ "$(out 1 pending)" = false ]'

echo "B: re-run of the same merge"
run_steps 2; check "released=false (nothing new)" '[ "$(out 2 released)" = false ]'
check "no second GitHub Release" '[ "$(wc -l < "$FX/created.log")" -eq 1 ]'

echo "C: run for the merge was cancelled, next push recovers"
new_repo c; M=$(version_pr_merge 1.1.0)
echo "[{\"number\": 5, \"merged_at\": \"2026-02-01T00:00:00Z\", \"merge_commit_sha\": \"$M\", \"labels\": [{\"name\": \"changesetter:version\"}]}]" > "$FX/pulls.json"
echo x > other.txt; git add -A; git commit -qm "unrelated"; git push -q origin main
run_steps 1; check "tag v1.1.0 created at the old merge" '[ "$(remote_tag_at v1.1.0)" = "$M" ]'
check "released=true" '[ "$(out 1 released)" = true ]'
echo "D: later push after a complete release"
echo y > other.txt; git commit -qam "unrelated 2"; git push -q origin main
run_steps 2; check "released=false" '[ "$(out 2 released)" = false ]'
check "still one GitHub Release" '[ "$(wc -l < "$FX/created.log")" -eq 1 ]'

echo "E: version PR merge that also brings in a late changeset"
new_repo e; M=$(version_pr_merge 1.1.0 late); label_merge "$M"
run_steps 1; check "tag v1.1.0 created" '[ "$(remote_tag_at v1.1.0)" = "$M" ]'
check "pending=true afterwards" '[ "$(out 1 pending)" = true ]'

echo "F: pre-release version"
new_repo f; M=$(version_pr_merge 2.0.0-rc.0); label_merge "$M"
run_steps 1; check "tag v2.0.0-rc.0" '[ "$(remote_tag_at v2.0.0-rc.0)" = "$M" ]'
check "created as --prerelease" 'grep -q -- "--prerelease" "$FX/created.log"'

echo "G: API failure fails the step"
new_repo g; M=$(version_pr_merge 1.1.0); label_merge "$M"
GH_FAIL_API=1 run_steps 1; check "step failed and nothing tagged" 'grep -q "check-merged failed" "$FX/log-1" && [ -z "$(remote_tag_at v1.1.0)" ]'

echo "H: npm monorepo with a custom tag format, only one package bumped"
O="$ROOT/h-origin.git"; W="$ROOT/h-work"; FX="$ROOT/h-fx"
git init -q --bare -b main "$O"; git clone -q "$O" "$W" 2>/dev/null; mkdir -p "$FX"; echo '[]' > "$FX/empty.json"; echo '[]' > "$FX/pulls.json"
cd "$W" || exit 1
printf '{"name":"root","private":true,"workspaces":["packages/*"]}' > package.json
mkdir -p packages/a packages/b
printf '{"name":"@s/a","version":"1.0.0"}' > packages/a/package.json
printf '{"name":"@s/b","version":"2.0.0"}' > packages/b/package.json
printf '[tag]\nformat = "{name}-{version}"\n\n[changelog]\nper_package = true\n' > changesetter.toml
printf '# a\n\n## 1.0.0 - d\n\n- a1\n' > packages/a/CHANGELOG.md
git add -A; git commit -qm init; git push -q origin main
git checkout -qb changesetter/version-packages
printf '{"name":"@s/a","version":"1.1.0"}' > packages/a/package.json
printf '# a\n\n## 1.1.0 - d\n\n- a notes\n\n## 1.0.0 - d\n\n- a1\n' > packages/a/CHANGELOG.md
git commit -qam "chore: version packages"; git checkout -q main; git merge -q --no-ff changesetter/version-packages -m merge; git push -q origin main
M=$(git rev-parse HEAD); label_merge "$M"
run_steps 1; check "only @s/a tagged, custom format" '[ "$(remote_tag_at @s/a-1.1.0)" = "$M" ] && [ -z "$(git ls-remote "$O" "refs/tags/@s/b*")" ]'
check "notes from the per-package changelog" 'grep -q "a notes" <(git -C "$W" fetch -q --tags origin; git -C "$W" tag -l --format="%(contents)" "@s/a-1.1.0")'

echo "I: [release] tag_annotated = false gives a lightweight tag"
new_repo i; printf '[release]\ntag_annotated = false\n' > changesetter.toml; git add -A; git commit -qm config; git push -q origin main
M=$(version_pr_merge 1.1.0); label_merge "$M"
run_steps 1; check "v1.1.0 is lightweight at the merge" '[ "$(git -C "$W" ls-remote "$O" refs/tags/v1.1.0 | cut -f1)" = "$M" ] && [ -z "$(remote_tag_at v1.1.0)" ]'

# open_version_pr <n>: runs the version-PR step for the pending changesets
open_version_pr() {
  OUT="$FX/out-pr-$1"; : > "$OUT"
  GH_FIXTURES="$FX" GITHUB_OUTPUT="$OUT" GH_TOKEN=x GITHUB_ACTION_PATH="$REPO/actions/release" \
    GITHUB_REF_NAME=main PR_TITLE="chore: version packages" \
    bash -eo pipefail "$STEPS/version-pr.sh" > "$FX/log-pr-$1" 2>&1 || echo "version-pr failed" >> "$FX/log-pr-$1"
}

echo "J: version PR preview shows notes folded in from Unreleased"
new_repo j
mkdir -p .changeset
printf -- '---\npkg: none\n---\n\n#### CI tweak\n' > .changeset/ci.md
git add -A; git commit -qm ci; changesetter version --no-commit > /dev/null 2>&1; git add -A; git commit -qm "unreleased notes"
printf -- '---\npkg: patch\n---\n\n#### Fix\n' > .changeset/fix.md
git add -A; git commit -qm fix; git push -q origin main
open_version_pr 1
check "step ran" '! grep -q "version-pr failed" "$FX/log-pr-1"'
check "preview has the release and the folded note" 'grep -qx "### pkg@1.0.1" "$FX/pr-body.md" && grep -qx "#### Fix" "$FX/pr-body.md" && grep -qx "#### CI tweak" "$FX/pr-body.md"'
check "preview leaves out older versions" '! grep -qx -- "- First" "$FX/pr-body.md"'

echo "K: version PR preview on a first release"
new_repo k; git rm -q CHANGELOG.md; git commit -qm "no changelog"
mkdir -p .changeset; printf -- '---\npkg: minor\n---\n\n#### First feature\n' > .changeset/feat.md
git add -A; git commit -qm feat; git push -q origin main
open_version_pr 1
check "preview has the new changelog's section" 'grep -qx "### pkg@1.1.0" "$FX/pr-body.md" && grep -qx "#### First feature" "$FX/pr-body.md"'

echo "L: version PR preview with only none-bump notes"
new_repo l; mkdir -p .changeset; printf -- '---\npkg: none\n---\n\n#### Docs\n' > .changeset/docs.md
git add -A; git commit -qm docs; git push -q origin main
open_version_pr 1
check "preview has the Unreleased lines" 'grep -qx "## Unreleased" "$FX/pr-body.md" && grep -qx "#### Docs" "$FX/pr-body.md"'

echo
echo "passed $PASS, failed $FAIL (logs in $ROOT)"
[ "$FAIL" -eq 0 ]
