#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# A stand-in for `gitleaks` used ONLY by tests/secret_scan_*.sh.
#
# It exists so the cache, the git-blob enumeration, the report parsing and the
# failure paths can be exercised offline. It is deliberately NOT a scanner: it
# recognises one shape, `AKIA` + 16 upper-case alphanumerics, which is the
# pattern of gitleaks' own `aws-access-token` rule. Nothing in the gate itself
# depends on which binary it is given -- that is the point of the design -- so a
# fixture that speaks the same CLI and report format tests the real code path.
#
# It is not installed, not referenced by any workflow, and not a dependency.
#
#   fake-gitleaks detect --no-git --source DIR --config CFG \
#       --report-format json --report-path OUT --exit-code 0

set -euo pipefail

[ "${1:-}" = "detect" ] || { echo "fake-gitleaks: only 'detect' is implemented" >&2; exit 2; }
shift

source_dir=""
report_path=""
while [ $# -gt 0 ]; do
    case "$1" in
    --source)
        source_dir="$2"
        shift 2
        ;;
    --report-path)
        report_path="$2"
        shift 2
        ;;
    --config | --report-format | --exit-code)
        shift 2
        ;;
    --no-git | --no-banner | --redact)
        shift
        ;;
    *)
        echo "fake-gitleaks: unknown argument: $1" >&2
        exit 2
        ;;
    esac
done

[ -n "$source_dir" ] || { echo "fake-gitleaks: --source is required" >&2; exit 2; }
[ -n "$report_path" ] || { echo "fake-gitleaks: --report-path is required" >&2; exit 2; }
[ -d "$source_dir" ] || { echo "fake-gitleaks: not a directory: $source_dir" >&2; exit 2; }

tmp="$(mktemp)"
: >"$tmp"
while IFS= read -r file; do
    rel="${file#"$source_dir"/}"
    line_no=0
    while IFS= read -r line; do
        line_no=$((line_no + 1))
        if printf '%s' "$line" | grep -qE 'AKIA[0-9A-Z]{16}'; then
            jq -n --arg f "$rel" --argjson n "$line_no" \
                '{RuleID: "aws-access-token", File: $f, StartLine: $n, Secret: "REDACTED", Match: "REDACTED"}'
        fi
    done <"$file"
done < <(find "$source_dir" -type f | LC_ALL=C sort) >"$tmp"

jq -s '.' "$tmp" >"$report_path"
rm -f "$tmp"
exit 0
