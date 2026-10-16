#!/usr/bin/env bash
# Prints the body of one version's section in a changelog: everything after
# its "## <version>" heading up to the next version's heading, without leading
# blank lines. Usage: release-notes.sh <version> [changelog]
set -euo pipefail

version="$1"
changelog="${2:-CHANGELOG.md}"
[ -f "$changelog" ] || exit 0

# Compared as plain text rather than a regex: the dots in a version would
# match any character, and 1.0.1 must not match a 1.0.10 heading. Monorepo
# releases share one heading ("## 1.0.0, 2.0.0 - date"), hence the comma.
awk -v heading="## ${version}" '
  { sub(/\r$/, "") }
  $0 == heading || index($0, heading " ") == 1 || index($0, heading ",") == 1 { found = 1; next }
  # Only a version heading ends the section; changeset bodies are copied in
  # verbatim and may have "## " headings of their own.
  found && /^## [0-9]/ { exit }
  found && (started || NF) { started = 1; print }
' "$changelog"
