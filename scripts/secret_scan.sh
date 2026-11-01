#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# secret_scan.sh -- the repository's secret gate, scanning HISTORY once per
# distinct blob instead of the whole tree on every push.
#
# WHY THIS EXISTS
# ---------------
# The gate this replaces was a call to `metadatastician/standards`' reusable
# workflow. That caller was added on 2026-10-31 and never started once: a
# cross-owner reusable rejected before its first job produces no check run at
# all, so `gh pr checks` shows nothing and the REQUIRED context `scan / gitleaks`
# never arrives. Every pull request waits forever. A required gate whose
# availability depends on another repository's policy is not a gate, it is a
# dependency on someone else's settings page.
#
# WHAT IT SCANS, AND WHY THAT IS MORE THAN THE OLD GATE
# -----------------------------------------------------
# The old workflow ran `gitleaks detect --source .` -- a WORKING-TREE scan. A
# secret that was committed in March and deleted in April is invisible to it,
# and that is the most common real leak: the rotation is forgotten exactly
# because the string is no longer on disk. This gate enumerates every blob
# reachable from any fetched ref (`git rev-list --objects --all`) and scans each
# distinct (blob, path) pair. A secret stays findable after it is deleted,
# because deleting a file does not delete the blob.
#
# WHY IT IS FAST ANYWAY
# ---------------------
# A blob is a content hash. Its scan verdict depends only on its bytes, its
# path, and the scanner (binary + config). So the ledger records
# `blob-sha <TAB> path` once it has been proven clean under a given scanner
# epoch, and the next run only materialises and scans pairs that are new. On a
# typical push that is a handful of files; on the first run it is all of
# history. The epoch covers the scanner binary digest and the rules digest, so
# editing `.gitleaks.toml` correctly invalidates everything.
#
# Fail-closed everywhere: a missing scanner, a missing config, an unreadable
# report or a cache of the wrong shape all end in either a full rescan or a
# non-zero exit. Nothing here can turn "we did not look" into a pass.
#
# USAGE
#   secret_scan.sh [options]
#     --repo DIR          repository to scan (default: the current one)
#     --config FILE       scanner config (default: <repo>/.gitleaks.toml)
#     --scanner PATH      scanner binary (default: $IDAPTIK_SECRET_SCANNER, then
#                         `gitleaks` on PATH)
#     --cache DIR         ledger directory (default: a temp dir, i.e. no reuse)
#     --max-bytes N       refuse to materialise a single blob larger than N
#                         bytes (default 8388608); refusals are reported and
#                         fail the run rather than being skipped
#     --self-test         prove the gate can fail, then exit
#     --quiet             only the verdict line
#
# EXIT
#   0 clean, 1 findings (or a self-test failure), 2 the gate could not run.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib/content-cache.sh
. "$here/lib/content-cache.sh"

repo="."
config=""
scanner=""
cache_dir=""
max_bytes=8388608
self_test=0
quiet=0

die() {
    printf 'secret-scan: %s\n' "$*" >&2
    exit 2
}

while [ $# -gt 0 ]; do
    case "$1" in
    --repo)
        repo="${2:?--repo needs a directory}"
        shift 2
        ;;
    --config)
        config="${2:?--config needs a file}"
        shift 2
        ;;
    --scanner)
        scanner="${2:?--scanner needs a path}"
        shift 2
        ;;
    --cache)
        cache_dir="${2:?--cache needs a directory}"
        shift 2
        ;;
    --max-bytes)
        max_bytes="${2:?--max-bytes needs a number}"
        shift 2
        ;;
    --self-test)
        self_test=1
        shift
        ;;
    --quiet)
        quiet=1
        shift
        ;;
    -h | --help)
        sed -n '3,60p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        die "unknown argument: $1"
        ;;
    esac
done

# --- resolve the inputs, refusing to guess -----------------------------------

[ -d "$repo" ] || die "no such repository: $repo"
repo="$(cd "$repo" && pwd)"
[ -n "$config" ] || config="$repo/.gitleaks.toml"
[ -f "$config" ] || die "no scanner config at $config -- a scan without the allowlist is not this gate"

if [ -z "$scanner" ]; then
    scanner="${IDAPTIK_SECRET_SCANNER:-}"
fi
if [ -z "$scanner" ]; then
    scanner="$(command -v gitleaks || true)"
fi
[ -n "$scanner" ] || die "no scanner binary: install gitleaks, or set IDAPTIK_SECRET_SCANNER"
[ -x "$scanner" ] || die "scanner is not executable: $scanner"
scanner="$(cd "$(dirname "$scanner")" && pwd)/$(basename "$scanner")"

command -v jq >/dev/null || die "jq is required to read the scanner report"

work="$(mktemp -d)"
trap 'rm -rf "$work"; cc_cleanup' EXIT

if [ "$self_test" = 1 ]; then
    # The control runs the same script, on a throwaway repository, and requires
    # red-then-green. "It exited 0" is not evidence that it can fail.
    exec "$here/../tests/secret_scan_self_test.sh" \
        --scanner "$scanner" --config "$config" --script "$here/secret_scan.sh"
fi

# --- epoch: what counts as "the same checker" --------------------------------

epoch="$(cc_epoch "$scanner" "$config" "literal:secret-scan-v1")" ||
    die "cannot compute the scanner epoch"
if [ -z "$cache_dir" ]; then
    cache_dir="$work/cache" # no reuse unless the caller asks for it
fi
cc_load "$cache_dir" "$epoch"

# --- enumerate every (blob, path) pair reachable from any fetched ref -------

git -C "$repo" rev-parse --git-dir >/dev/null 2>&1 || die "not a git repository: $repo"

all_keys="$work/all.tsv"
: >"$all_keys"
rejected="$work/rejected.tsv"
: >"$rejected"

# `--objects` prints `<sha> <path>` for commits, trees AND blobs -- a tree
# carries the directory's path just as a blob does, and feeding a tree to
# `cat-file blob` is a hard error. So the type is resolved for every object in
# ONE `--batch-check` pass (one process, not one per object), and only blobs
# survive. The same pass yields sizes, so the materialisation loop needs no
# further git calls.
#
# The path is everything after the FIRST space, so spaces in filenames survive.
raw="$work/raw.tsv"
: >"$raw"
while IFS= read -r line; do
    case "$line" in
    *" "*) ;;
    *) continue ;;
    esac
    sha="${line%% *}"
    path="${line#* }"
    # Some git versions print `<sha> ` (trailing space, empty name) for a root
    # tree. That is not a file, and treating it as one materialises the tree
    # directory itself.
    if [ -z "$sha" ] || [ -z "$path" ]; then
        continue
    fi
    # Defensive: a path the ledger cannot represent would otherwise be dropped
    # silently. Refuse it and fail, instead of scanning four-fifths of a tree.
    case "$path" in
    *$'\t'* | *$'\n'* | /* | *../*)
        printf '%s\t%s\n' "$sha" "$path" >>"$rejected"
        continue
        ;;
    esac
    printf '%s\t%s\n' "$sha" "$path" >>"$raw"
done < <(git -C "$repo" rev-list --objects --all)

types="$work/types.tsv"
cut -f1 "$raw" | LC_ALL=C sort -u |
    git -C "$repo" cat-file --batch-check='%(objectname) %(objecttype) %(objectsize)' >"$types"

# `--batch-check` is SPACE-separated; the ledger is TAB-separated. Both fields
# are being mixed here on purpose, so the split class says so.
LC_ALL=C awk -F'[ \t]+' '
    NR == FNR { if ($2 == "blob") size[$1] = $3; next }
    ($1 in size) { printf "%s\t%s\t%s\n", $1, $2, size[$1] }
' "$types" "$raw" >"$work/blobs.tsv"

cut -f1,2 "$work/blobs.tsv" >"$all_keys"

total="$(LC_ALL=C sort -u "$all_keys" | wc -l | tr -d ' ')"
[ "$total" -gt 0 ] || die "no blobs found -- an empty denominator is not a pass"

todo="$work/todo.tsv"
hit="$work/hit.tsv"
cc_partition "$all_keys" "$CC_SEEN" "$todo" "$hit"
n_todo="$(wc -l <"$todo" | tr -d ' ')"
n_hit="$(wc -l <"$hit" | tr -d ' ')"

# --- materialise only the unseen pairs --------------------------------------

tree="$work/tree"
mkdir -p "$tree"
oversize="$work/oversize.tsv"
: >"$oversize"

if [ "$n_todo" -gt 0 ]; then
    # Join the todo list with the sizes resolved above; the union is ordered by
    # sha then path so the two files can be walked once.
    LC_ALL=C awk -F'\t' '
        NR == FNR { size[$1 SUBSEP $2] = $3; next }
        (($1 SUBSEP $2) in size) { printf "%s\t%s\t%s\n", $1, $2, size[$1 SUBSEP $2] }
    ' "$work/blobs.tsv" "$todo" >"$work/todo-sized.tsv"
    while IFS=$'\t' read -r sha path size; do
        [ -n "$sha" ] || continue
        if [ "$size" -gt "$max_bytes" ]; then
            printf '%s\t%s\t%s\n' "$sha" "$path" "$size" >>"$oversize"
            continue
        fi
        mkdir -p "$tree/$(dirname "$path")"
        # One `cat-file` process per blob is deliberate: `--batch` would need a
        # length-prefixed protocol to survive binary-safe paths, and a few
        # hundred to a few thousand processes is cheap next to the bug surface.
        git -C "$repo" cat-file blob "$sha" >"$tree/$path"
    done <"$work/todo-sized.tsv"
fi

# --- scan -------------------------------------------------------------------

findings=0
report="$work/report.json"
if [ "$n_todo" -gt 0 ]; then
    # `--no-git` keeps the scanner on the materialised tree; `--redact` keeps a
    # live credential out of CI logs and out of the JSON we parse below.
    set +e
    "$scanner" detect --no-git --no-banner --redact \
        --source "$tree" --config "$config" \
        --report-format json --report-path "$report" --exit-code 0 \
        >"$work/scanner.log" 2>&1
    scanner_rc=$?
    set -e
    # 0 is "ran and reported"; 1 is "ran and found"; anything else is the gate
    # being unable to run, which must never be reported as a clean history.
    case "$scanner_rc" in
    0 | 1) ;;
    *)
        tail -20 "$work/scanner.log" >&2 || true
        die "scanner failed with rc=$scanner_rc -- not a clean result"
        ;;
    esac
    [ -f "$report" ] || die "scanner produced no report at $report"
    findings="$(jq 'length' "$report" 2>/dev/null)" || die "cannot parse $report"
    case "$findings" in
    '' | *[!0-9]*) die "report is not a JSON array of findings" ;;
    esac
fi

# Paths with a finding are NOT recorded as clean, so they re-report on every run
# until the secret is rotated and the rules explain why it is allowed.
finding_paths="$work/finding-paths.tsv"
: >"$finding_paths"
if [ "$findings" -gt 0 ]; then
    jq -r '.[].File // empty' "$report" | LC_ALL=C sort -u >"$finding_paths"
fi

clean_keys="$work/clean.tsv"
if [ -s "$finding_paths" ] && [ "$n_todo" -gt 0 ]; then
    LC_ALL=C awk -F'\t' 'NR == FNR { bad[$0] = 1; next } !($2 in bad)' \
        "$finding_paths" "$todo" | LC_ALL=C sort -u >"$clean_keys"
else
    LC_ALL=C sort -u "$todo" >"$clean_keys"
fi

cc_keep "$hit"
cc_record "$clean_keys"
cc_save "$epoch"

# --- verdict ----------------------------------------------------------------

n_oversize="$(wc -l <"$oversize" | tr -d ' ')"
if [ "$quiet" = 0 ]; then
    printf 'secret-scan: %s blob(s) in history, %s scanned now, %s from cache\n' \
        "$total" "$n_todo" "$n_hit"
    if [ -s "$rejected" ]; then
        printf 'secret-scan: %s path(s) refused\n' "$(wc -l <"$rejected" | tr -d ' ')"
    fi
fi

status=0
if [ "$n_oversize" -gt 0 ]; then
    printf 'secret-scan: FAIL %s blob(s) exceed --max-bytes and were never scanned\n' "$n_oversize" >&2
    awk -F'\t' '{ printf "  %s  %s  (%s bytes)\n", $1, $2, $3 }' "$oversize" >&2
    status=1
fi
if [ -s "$rejected" ]; then
    printf 'secret-scan: FAIL %s path(s) cannot be represented in the ledger:\n' \
        "$(wc -l <"$rejected" | tr -d ' ')" >&2
    sed 's/^/  /' "$rejected" >&2
    status=1
fi
if [ "$findings" -gt 0 ]; then
    status=1
fi

if [ "$status" -ne 0 ]; then
    if [ "$findings" -gt 0 ]; then
        printf 'secret-scan: FAIL %s finding(s)\n' "$findings" >&2
        jq -r '.[] | "\(.RuleID)\t\(.File)\t\(.StartLine)"' "$report" 2>/dev/null |
            while IFS=$'\t' read -r rule file line; do
                printf '  %s  %s:%s\n' "$rule" "$file" "$line"
            done >&2
        printf 'secret-scan: rotate the credential, then either remove it from history or\n' >&2
        printf 'secret-scan: add an anchored, value-shaped allowlist entry to %s\n' "$config" >&2
    fi
    exit 1
fi

if [ "$quiet" = 0 ]; then
    printf 'secret-scan: ok -- no secrets in %s blob(s)\n' "$total"
fi
exit 0
