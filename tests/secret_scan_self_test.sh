#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Control for scripts/secret_scan.sh: prove the gate can fail, that the cache
# cannot make it stop failing, and that history is really covered.
#
# A gate that reports "clean" on a repository nobody has scanned is worse than
# no gate; so is a cache that turns "we scanned this" into "we assumed this".
# Every case below is a mutation that must change the verdict.
#
# Runs offline against tests/fixtures/fake-gitleaks.sh, or against a real
# `gitleaks` with --scanner / IDAPTIK_SECRET_SCANNER. The planted value is
# `AKIA` + 16 upper-case alphanumerics, which both the fixture and gitleaks'
# `aws-access-token` rule match; it is not in gitleaks' stopword list and does
# not match any anchored allowlist entry in .gitleaks.toml.
#
#   secret_scan_self_test.sh [--scanner PATH] [--config FILE] [--script FILE]

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
script="$root/scripts/secret_scan.sh"
scanner="${IDAPTIK_SECRET_SCANNER:-$root/tests/fixtures/fake-gitleaks.sh}"
config="$root/.gitleaks.toml"

while [ $# -gt 0 ]; do
    case "$1" in
    --scanner)
        scanner="$2"
        shift 2
        ;;
    --config)
        config="$2"
        shift 2
        ;;
    --script)
        script="$2"
        shift 2
        ;;
    *)
        echo "self-test: unknown argument: $1" >&2
        exit 2
        ;;
    esac
done

[ -x "$scanner" ] || { echo "self-test: no executable scanner at $scanner" >&2; exit 2; }
[ -f "$config" ] || { echo "self-test: no config at $config" >&2; exit 2; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

pass=0
fail=0
ok() {
    pass=$((pass + 1))
    printf '  ok    %s\n' "$1"
}
no() {
    fail=$((fail + 1))
    printf '  FAIL  %s\n' "$1"
}

# A throwaway repository, so a planted secret can never touch a real one.
repo="$work/repo"
mkdir -p "$repo"
git -C "$repo" init -q
git -C "$repo" config user.email selftest@example.invalid
git -C "$repo" config user.name selftest
git -C "$repo" config commit.gpgsign false

# The scanner is told which config to use; the fixture ignores it, gitleaks does
# not. Copying it in keeps both honest about "this is the ruleset in force".
cp "$config" "$repo/.gitleaks.toml"

cache="$work/cache"

run() {
    # Echoes the exit status; the caller decides what each status means.
    set +e
    "$script" --repo "$repo" --config "$repo/.gitleaks.toml" \
        --scanner "$scanner" --cache "$cache" "$@" >"$work/out" 2>"$work/err"
    local rc=$?
    set -e
    printf '%s' "$rc"
}

printf 'secret-scan self-test: %s\n' "$(basename "$scanner")"

# 1. A clean tree passes -- otherwise every later red is meaningless.
printf 'hello\n' >"$repo/README.md"
git -C "$repo" add -A
git -C "$repo" commit -q -m "clean"
rc="$(run)"
if [ "$rc" = 0 ]; then ok "a clean history passes"; else no "a clean history should pass, got rc=$rc"; cat "$work/err" >&2; fi

# 2. The denominator is not zero: "ok" with no count is how a vacuous gate reads.
blobs="$(sed -n 's/.*ok -- no secrets in \([0-9]\+\) blob.*/\1/p' "$work/out")"
if [ -n "$blobs" ] && [ "$blobs" -gt 0 ]; then
    ok "the pass reports a non-zero denominator ($blobs blobs)"
else
    no "the pass does not report its denominator: $(cat "$work/out")"
fi

# 3. A planted secret is caught -- the gate can fail.
secret_file="$repo/credentials.md"
printf 'aws_key = "AKIAZ7Q2XK9MPL4RTUVW"\n' >"$secret_file"
git -C "$repo" add -A
git -C "$repo" commit -q -m "planted"
rc="$(run)"
if [ "$rc" = 1 ] && grep -q '1 finding' "$work/err"; then
    ok "a planted secret fails the run"
else
    no "a planted secret should fail with one finding, got rc=$rc"; cat "$work/err" >&2
fi

# 4. Deleting the file does NOT launder it: the blob is still in history.
git -C "$repo" rm -q credentials.md
git -C "$repo" commit -q -m "removed, not rotated"
rc="$(run)"
if [ "$rc" = 1 ]; then
    ok "a secret deleted from the tree is still caught in history"
else
    no "history coverage lost: the deleted secret passed (rc=$rc)"
fi

# 5. The cache is really being used -- and a finding is never cached as clean.
#    The one blob still carries the secret, so exactly one blob is rescanned and
#    the run stays red. Caching a finding would let a red run go green by itself.
scanned="$(sed -n 's/.* \([0-9]\+\) scanned now.*/\1/p' "$work/out")"
if [ "$scanned" = 1 ]; then
    ok "a warm cache rescans only the blob that still fails"
else
    no "warm cache rescanned $scanned blobs, expected exactly the 1 failing blob"
fi

# 6. A NEW blob is scanned even with a warm cache, and is still caught.
#    (This is the case a whole-repository fingerprint would get wrong: the
#    repository changed, so everything would rescan, and the interesting
#    question -- "was the ONE new file looked at?" -- would be unanswerable.)
printf 'more\n' >"$repo/notes.md"
git -C "$repo" add -A
git -C "$repo" commit -q -m "notes"
rc="$(run)"
scanned_after="$(sed -n 's/.* \([0-9]\+\) scanned now.*/\1/p' "$work/out")"
if [ "$scanned_after" = "$((scanned + 1))" ]; then
    ok "a warm cache scans exactly one extra blob when one file is added"
else
    no "adding one file scanned $scanned_after blobs, expected $((scanned + 1))"
fi

# 7. Changing the rules invalidates the cache: the epoch is the checker.
printf '\n# epoch bump\n' >>"$repo/.gitleaks.toml"
rc="$(run)"
rescan="$(sed -n 's/.* \([0-9]\+\) scanned now.*/\1/p' "$work/out")"
total_after="$(sed -n 's/secret-scan: \([0-9]\+\) blob(s) in history.*/\1/p' "$work/out")"
if [ -n "$rescan" ] && [ "$rescan" = "$total_after" ]; then
    ok "a rules change rescans every blob ($rescan of $total_after)"
else
    no "a rules change did not invalidate the cache: $rescan of $total_after"
fi

# 8. A corrupt cache is a rescan, never a pass.
printf 'garbage\tnot-a-key\n' >"$cache/seen.tsv"
printf 'not-the-epoch\n' >"$cache/epoch"
rc="$(run)"
if [ "$rc" = 1 ]; then
    ok "a corrupt cache still fails on the planted secret"
else
    no "a corrupt cache produced rc=$rc instead of a rescan-and-fail"
fi

# 9. A missing scanner is a failure, never a skip.
set +e
"$script" --repo "$repo" --config "$repo/.gitleaks.toml" \
    --scanner "$work/no-such-scanner" --cache "$cache" --quiet >"$work/out" 2>"$work/err"
rc=$?
set -e
if [ "$rc" = 2 ] && grep -q 'not executable' "$work/err"; then
    ok "a missing scanner is a hard failure"
else
    no "a missing scanner gave rc=$rc, expected 2"
fi

# 10. A missing config is a failure, not a scan with default rules.
set +e
"$script" --repo "$repo" --config "$work/no-such-config.toml" \
    --scanner "$scanner" --cache "$cache" --quiet >"$work/out" 2>"$work/err"
rc=$?
set -e
if [ "$rc" = 2 ] && grep -q 'no scanner config' "$work/err"; then
    ok "a missing ruleset is a hard failure"
else
    no "a missing ruleset gave rc=$rc, expected 2"
fi

# 11. Removing the secret from history makes it green again -- the ordinary
#     remediation path (rotate, then rewrite the branch) must work.
git -C "$repo" checkout -q --orphan clean
git -C "$repo" rm -q -rf --cached .
rm -f "$repo/credentials.md"
printf 'hello\n' >"$repo/README.md"
git -C "$repo" add -A
git -C "$repo" commit -q -m "clean history"
git -C "$repo" branch -q -D main 2>/dev/null || git -C "$repo" branch -q -D master 2>/dev/null || true
git -C "$repo" update-ref -d refs/heads/main 2>/dev/null || true
git -C "$repo" update-ref -d refs/heads/master 2>/dev/null || true
git -C "$repo" reflog expire --expire=now --all
git -C "$repo" gc --prune=now -q
rm -rf "$cache"
rc="$(run)"
if [ "$rc" = 0 ]; then
    ok "rewriting the secret out of history restores a pass"
else
    no "after rewrite the run still fails (rc=$rc): $(cat "$work/err")"
fi

printf 'secret-scan self-test: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
