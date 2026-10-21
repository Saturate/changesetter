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
# spellings ("## [1.0.0]", "## v1.0.0", "## [v1.0.0]") and Keep a
# Changelog's trailing "[1.0.0]: https://..." link references. Only version and Unreleased labels
# count, since changeset bodies may use reference links of their own. Same
# rule as the Rust side.
awk -v version="$version" '
  # Fence rules match src/changelog/markdown.rs: up to 3 spaces of indent,
  # then 3+ backticks or tildes, and the info string of a backtick fence has
  # no backtick. Sets fc (character), fl (run length) and fb (nothing after it).
  function fence(s,   run, info) {
    if (!match(s, /^(   |  | )?(````*|~~~~*)/)) return 0
    run = substr(s, RSTART, RLENGTH)
    sub(/^ */, "", run)
    info = substr(s, RSTART + RLENGTH)
    fc = substr(run, 1, 1)
    if (fc == "`" && index(info, "`")) return 0
    fl = length(run)
    fb = (info ~ /^[ \t]*$/)
    return 1
  }
  # Marks fenced code (fence lines and what is between them) in code[] and
  # returns the line of a fence still open at the end, or 0.
  function scan(   i, op, oc, ol) {
    op = 0
    for (i = 1; i <= n; i++) {
      code[i] = 0
      if (op) {
        code[i] = 1
        if (fence(line[i]) && fc == oc && fl >= ol && fb) op = 0
      } else if (!(i in skip) && fence(line[i])) {
        op = i; oc = fc; ol = fl; code[i] = 1
      }
    }
    return op
  }
  function emit(s) { if (found && (started || s != "")) { started = 1; print s } }
  { sub(/[ \t\r]+$/, ""); line[++n] = $0 }
  END {
    # A fence never closed would hide every heading below it: it is a body
    # written before bodies got their fences closed, so its opening line is
    # read as text, the same as the Rust side does.
    while ((op = scan()) > 0) skip[op] = 1
    for (i = 1; i <= n; i++) {
      s = line[i]
      # Fenced code is never a heading, so a "## " line in a sample stays.
      if (code[i]) { emit(s); continue }
      if (s ~ /^## \[?Unreleased\]?$/ || s ~ /^## (\[[vV]?|[vV])[0-9]+\.[0-9]+\.[0-9]+/ || s ~ /^\[(Unreleased|[vV]?[0-9]+\.[0-9]+\.[0-9]+[^]]*)\]: /) {
        if (found) exit
        continue
      }
      if (s ~ /^## [0-9]+\.[0-9]+\.[0-9]+/) {
        # The section ends at the next version heading, even a duplicate of ours.
        if (found) exit
        list = substr(s, 4)
        sub(/ - .*$/, "", list)
        k = split(list, versions, ", ")
        for (j = 1; j <= k; j++) if (versions[j] == version) found = 1
        continue
      }
      emit(s)
    }
  }
' "$changelog"
