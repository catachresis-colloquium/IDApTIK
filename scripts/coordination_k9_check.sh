#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Focused structural guard for this repository's coordination.k9 (v1.2.0).
# The pinned standards revision publishes normative ABNF but no standalone
# coordination validator; this script checks the file's required envelope,
# section order, indentation, key fields, and DEED reference. It is not a
# general-purpose K9 coordination parser.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FILE="${1:-$REPO_DIR/coordination.k9}"
case "$FILE" in
  /*) ;;
  *) FILE="$REPO_DIR/$FILE" ;;
esac

# Report all arguments as one failure message on stderr and exit with status 1.
fail() { printf 'coordination-k9: FAIL: %s\n' "$*" >&2; exit 1; }
[ -r "$FILE" ] || fail "cannot read $FILE"

first_line="$(head -n 1 "$FILE" | tr -d '\r')"
[ "$first_line" = 'K9!' ] || fail 'line 1 must be exactly K9! (no BOM or leading whitespace)'
if LC_ALL=C grep -q $'\t' "$FILE"; then
  fail 'tabs are forbidden; indentation must use spaces'
fi
if LC_ALL=C grep -q $'\r' "$FILE"; then
  fail 'this repository requires LF line endings'
fi
if grep -q '^---[[:space:]]*$' "$FILE"; then
  fail 'YAML document separators are invalid in coordination.k9'
fi

awk '
  NR == 1 { next }
  NR == 2 && $0 !~ /^# SPDX-/ { bad = 1; msg = "SPDX header must immediately follow K9!" }
  in_header && /^# SPDX-/ { headers++; next }
  in_header && $0 == "" { in_header = 0; next }
  NR == 2 { in_header = 1; headers = 1; next }
  END {
    if (bad || headers < 1) {
      print "coordination-k9: FAIL: " (msg ? msg : "missing SPDX header") > "/dev/stderr"
      exit 1
    }
  }
' "$FILE" || exit 1

awk '
  BEGIN {
    n = split("metadata project build_commands invariants protected architecture innervation do_not_create terminology ecosystem coordination", names, " ")
    for (i = 1; i <= n; i++) rank[names[i]] = i
  }
  /^[a-z_]+:[[:space:]]*$/ {
    section = $1; sub(/:$/, "", section)
    if (!(section in rank)) {
      printf "coordination-k9: FAIL: unknown top-level section %s\n", section > "/dev/stderr"
      bad = 1
    } else if (rank[section] <= previous) {
      printf "coordination-k9: FAIL: section %s is repeated or out of v1.2.0 order\n", section > "/dev/stderr"
      bad = 1
    }
    previous = rank[section]
    seen[section] = 1
    next
  }
  {
    if ($0 == "K9!" || $0 == "" || $0 ~ /^# SPDX-/) next
    if ($0 !~ /^  /) {
      printf "coordination-k9: FAIL: body line %d is not a section or an indented field\n", FNR > "/dev/stderr"
      bad = 1
      next
    }
    prefix = $0
    sub(/[^ ].*$/, "", prefix)
    if (length(prefix) % 2 != 0) {
      printf "coordination-k9: FAIL: line %d has indentation not divisible by two\n", FNR > "/dev/stderr"
      bad = 1
    }
  }
  END {
    for (required in required_sections) { }
    if (!seen["metadata"] || !seen["project"] || !seen["invariants"]) {
      print "coordination-k9: FAIL: metadata, project, and invariants sections are required" > "/dev/stderr"
      bad = 1
    }
    exit bad
  }
' "$FILE" || exit 1

required_lines=(
  '  schema: k9-coordination'
  '  schema_version: 1.2.0'
  '  generated_by: manual'
  '  name: "IDApTIK"'
  '  deed_ref: "IDApTIK_chora.deed"'
)
for line in "${required_lines[@]}"; do
  grep -Fqx "$line" "$FILE" || fail "missing required v1.2.0 field: $line"
done

grep -Eq '^  last_updated: [0-9]{4}-[0-9]{2}-[0-9]{2}$' "$FILE" \
  || fail 'metadata.last_updated must be an ISO 8601 calendar date'
[ -f "$REPO_DIR/IDApTIK_chora.deed" ] || fail 'project.deed_ref target is missing'

# Check that every invariant has all three normative fields and a legal
# severity when one is supplied. This is deliberately narrower than ABNF.
awk '
  /^invariants:[[:space:]]*$/ { in_section = 1; next }
  in_section && /^[a-z_]+:/ { in_section = 0 }
  !in_section { next }
  /^  - id: [A-Za-z][A-Za-z0-9_.-]*$/ {
    if (id != "" && (!rule || !reason)) { printf "coordination-k9: FAIL: invariant %s lacks rule or reason\n", id > "/dev/stderr"; bad = 1 }
    id = $3; rule = 0; reason = 0; next
  }
  /^    rule: ".*"$/ { rule = 1; next }
  /^    reason: ".*"$/ { reason = 1; next }
  /^    severity: / && $0 !~ /^    severity: (critical|high|moderate)$/ { printf "coordination-k9: FAIL: illegal invariant severity on line %d\n", FNR > "/dev/stderr"; bad = 1 }
  END {
    if (id == "" || !rule || !reason) { print "coordination-k9: FAIL: missing or incomplete invariant" > "/dev/stderr"; bad = 1 }
    exit bad
  }
' "$FILE" || exit 1

printf 'coordination-k9: OK %s (coordination ABNF v1.2.0 structural guard)\n' "${FILE#$REPO_DIR/}"
