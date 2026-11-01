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
# Two behaviours are reproduced because the engine depends on them and a fixture
# that is tidier than the original hides bugs (both have already happened):
#
#   * report paths are ABSOLUTE, as `--no-git` produces -- not relativised to
#     make the test's life easier;
#   * whole-value anchored allowlist entries in the config are honoured, so the
#     config file's own allowlist strings are not reported.
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
config=""
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
    --config)
        config="$2"
        shift 2
        ;;
    --report-format | --exit-code)
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

# Anchored allowlist values from the config, one per line.
#
# The estate config's design rule requires every entry to be anchored to the
# whole value, so `'''^literal$'''` is the only shape that exists. Only whole
# string equality is implemented here; that is intentional, because a fixture
# that approximated a regex engine would be a second thing to get wrong.
allowed=""
if [ -f "$config" ]; then
    allowed="$(sed -n "s/^[[:space:]]*'''\\^\\(.*\\)\\$'''[[:space:]]*,*[[:space:]]*$/\\1/p" "$config" |
        LC_ALL=C sort -u)"
fi

tmp="$(mktemp)"
: >"$tmp"
while IFS= read -r file; do
    # Deliberately NOT relative: gitleaks --no-git reports the path it was
    # given, so the fixture must too.
    rel="$file"
    line_no=0
    while IFS= read -r line; do
        line_no=$((line_no + 1))
        value="$(printf '%s' "$line" | grep -oE 'AKIA[0-9A-Z]{16}' | head -n 1 || true)"
        if [ -n "$value" ]; then
            if [ -n "$allowed" ] && printf '%s\n' "$allowed" | grep -Fxq -- "$value"; then
                continue
            fi
            jq -n --arg f "$rel" --argjson n "$line_no" \
                '{RuleID: "aws-access-token", File: $f, StartLine: $n, Secret: "REDACTED", Match: "REDACTED"}'
        fi
    done <"$file"
done < <(find "$source_dir" -type f | LC_ALL=C sort) >"$tmp"

jq -s '.' "$tmp" >"$report_path"
rm -f "$tmp"
exit 0
