#!/usr/bin/env bash
# Prints the body of one version's section in a changelog: everything after
# its "## <version>" heading up to the next version's heading, without leading
# blank lines. Usage: release-notes.sh <version> [changelog]
set -euo pipefail

version="$1"
changelog="${2:-CHANGELOG.md}"
[ -f "$changelog" ] || exit 0

# A version heading is "## <versions> - <date>", where a monorepo release
# lists several versions comma-separated. Each is compared as plain text
# rather than a regex: the dots would match any character, and 1.0.1 must not
# match a 1.0.10 heading. Changeset bodies are copied in verbatim and may have
# "## " headings of their own ("## 2FA support"), so only a semver-looking
# heading counts as a version heading. Other headings end a section too, so
# notes never run to the end of the file: "## Unreleased" (none-bump entries
# no release has picked up yet, never a version), versions in other tools'
# spellings ("## [1.0.0]", "## v1.0.0", "## [v1.0.0]") and Keep a Changelog's trailing
# "[1.0.0]: https://..." link references. Only version and Unreleased labels
# count, since changeset bodies may use reference links of their own. Same
# rule as the Rust side.
awk -v version="$version" '
  { sub(/[ \t\r]+$/, "") }
  # Fenced code (``` or ~~~) is never a heading, so a "## " line in a code
  # sample stays in the notes. A fence closes with at least as many of the
  # same character and nothing after them.
  match($0, /^ *(````*|~~~~*)/) {
    run = substr($0, RSTART, RLENGTH); sub(/^ */, "", run)
    if (fence == "") fence = run
    else if (substr(run, 1, 1) == substr(fence, 1, 1) && length(run) >= length(fence) && RLENGTH == length($0)) fence = ""
    if (found && (started || NF)) { started = 1; print }
    next
  }
  fence != "" { if (found && (started || NF)) { started = 1; print }; next }
  /^## \[?Unreleased\]?$/ || /^## (\[[vV]?|[vV])[0-9]+\.[0-9]+\.[0-9]+/ || /^\[(Unreleased|[vV]?[0-9]+\.[0-9]+\.[0-9]+[^]]*)\]: / {
    if (found) exit
    next
  }
  /^## [0-9]+\.[0-9]+\.[0-9]+/ {
    # The section ends at the next version heading, even a duplicate of ours.
    if (found) exit
    list = substr($0, 4)
    sub(/ - .*$/, "", list)
    n = split(list, versions, ", ")
    for (i = 1; i <= n; i++) if (versions[i] == version) found = 1
    next
  }
  found && (started || NF) { started = 1; print }
' "$changelog"
