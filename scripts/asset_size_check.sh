#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# ADR-0019 guard: no committed file is larger than 1 MiB.
#
# Binary assets live in this repository as plain Git blobs (no LFS), so every
# revision stays in the history for ever. The limit is enforced here, not by
# memory. A larger file needs a new owner ruling, not a larger number here.
#
# Usage: asset_size_check.sh [--self-test] [repository root, default .]
set -euo pipefail

LIMIT=$((1024 * 1024))

# Print "size path" for every file in the index of $1 that is over LIMIT.
oversize() {
  git -C "$1" ls-files -s -z |
    tr '\0' '\n' |
    sed -E 's/^[0-7]+ ([0-9a-f]+) [0-9]+\t/\1 /' |
    git -C "$1" cat-file --batch-check='%(objectsize) %(rest)' |
    awk -v limit="$LIMIT" '$1 > limit'
}

check() {
  local over
  over="$(oversize "$1")"
  if [ -n "$over" ]; then
    echo "asset-size: FAIL: files over $LIMIT bytes (ADR-0019):"
    echo "$over"
    return 1
  fi
  echo "asset-size: ok (every committed file is at most $LIMIT bytes)"
}

self_test() {
  local dir rc
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' RETURN
  git -C "$dir" init -q
  head -c "$LIMIT" /dev/zero > "$dir/at limit.png"
  git -C "$dir" add -A
  check "$dir" > /dev/null || { echo "asset-size self-test: FAIL: a file of exactly $LIMIT bytes was refused"; return 1; }
  head -c $((LIMIT + 1)) /dev/zero > "$dir/over limit.png"
  git -C "$dir" add -A
  rc=0
  check "$dir" > /dev/null || rc=$?
  if [ "$rc" -ne 1 ]; then
    echo "asset-size self-test: FAIL: a file of $((LIMIT + 1)) bytes was accepted"
    return 1
  fi
  echo "asset-size self-test: ok (limit accepted, limit + 1 refused)"
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
else
  check "${1:-.}"
fi
