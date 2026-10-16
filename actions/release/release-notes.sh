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
# heading counts as a version heading.
awk -v version="$version" '
  { sub(/[ \t\r]+$/, "") }
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
