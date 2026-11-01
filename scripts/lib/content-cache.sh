#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# content-cache -- "these exact bytes were already checked, under these exact
# rules" -- as a shell library.
#
# WHY
# ---
# Every gate in this repository re-derives its whole answer on every run: the
# secret scan re-walks history, the doc-claims gate re-reads every document, the
# fixture suites re-run every case. The expensive part is almost never the
# checking; it is doing it again to bytes nothing has touched. `panic-attack`
# fingerprints repositories by BLAKE3 and skips unchanged ones; `docudactyl`
# keys work on content. Same idea here, at the granularity a gate actually
# needs: one entry per unit of work, not one per repository.
#
# THE ONE INVARIANT
# -----------------
# A cache hit means "these exact bytes were checked, and the checker was
# identical". Both halves are load-bearing:
#
#   * the KEY identifies the bytes (a content digest, never a path, mtime or
#     size on its own);
#   * the EPOCH identifies the checker (digest of the scanner binary, its
#     configuration and this library's own behaviour).
#
# If the epoch does not match, the cache is not consulted at all. A cache that
# survives a change of checker turns "we already proved this" into "we already
# assumed this", which is the failure this whole repository is built to avoid.
#
# A missing, empty, unreadable or corrupt cache is never an error: it degrades
# to a full rescan. The rule is skip-on-proof, rescan-on-doubt.
#
# PORTABILITY
# -----------
# bash 3.2 (stock macOS) has no associative arrays, so membership tests are done
# in bulk with `comm` over sorted files rather than per-key lookups. Needs only
# coreutils, awk and git.
#
# TYPICAL USE
# -----------
#   . scripts/lib/content-cache.sh
#   epoch="$(cc_epoch "$SCANNER_BIN" "$CONFIG")"
#   cc_load "$CACHE_DIR" "$epoch"
#   cc_partition all-keys.txt "$CC_SEEN" todo.txt hit.txt
#   ... do the work for todo.txt ...
#   cc_keep hit.txt
#   cc_record clean-keys.txt
#   cc_save "$CACHE_DIR" "$epoch"

# A per-run scratch directory's contents must never leak between callers.
cc__tmp_suffix="$$.$RANDOM"

# cc_sha256_file PATH -> hex digest of the file's content.
# Fails loudly: a gate that cannot hash its input cannot decide anything.
cc_sha256_file() {
    [ -f "$1" ] || { echo "content-cache: no such file: $1" >&2; return 1; }
    # `sha256sum --` guards against a path that looks like an option.
    sha256sum -- "$1" | cut -d' ' -f1
}

# cc_epoch COMPONENT... -> one digest identifying the checker.
#
# Each component is either a file (hashed by content) or a literal token
# (prefixed `literal:`). Order matters and is preserved, so callers can encode
# "scanner, then config" without ambiguity about which changed.
cc_epoch() {
    local out="" part digest
    for part in "$@"; do
        case "$part" in
        literal:*) out="$out"$'\n'"$part" ;;
        *)
            digest="$(cc_sha256_file "$part")" || return 1
            out="$out"$'\n'"file:$part=$digest"
            ;;
        esac
    done
    printf '%s' "$out" | sha256sum | cut -d' ' -f1
}

# cc_load CACHE_DIR EPOCH -> sets CC_SEEN to a file of known values (possibly
# empty) and CC_HITS_FILE to a disposable path.
#
# Never fails. A cache from a different epoch, or one that cannot be read, is
# simply not used.
cc_load() {
    local dir="$1" epoch="$2"
    CC_DIR="$dir"
    mkdir -p "$CC_DIR" 2>/dev/null || true
    CC_SEEN="$CC_DIR/seen.${cc__tmp_suffix}.tsv"
    CC_KEPT="$CC_DIR/kept.${cc__tmp_suffix}.tsv"
    : >"$CC_SEEN"
    : >"$CC_KEPT"

    local stored_epoch=""
    if [ -f "$CC_DIR/epoch" ]; then
        stored_epoch="$(head -n 1 "$CC_DIR/epoch" 2>/dev/null || true)"
    fi
    if [ -n "$stored_epoch" ] && [ "$stored_epoch" = "$epoch" ] && [ -f "$CC_DIR/seen.tsv" ]; then
        # Sorted, because every membership test below is `comm`.
        LC_ALL=C sort -u "$CC_DIR/seen.tsv" >"$CC_SEEN" 2>/dev/null || : >"$CC_SEEN"
    fi
}

# cc_partition ALL_FILE SEEN_FILE TODO_OUT HIT_OUT
#
# Both outputs are sorted files. ALL_FILE must be sorted as well; callers
# produce it with `LC_ALL=C sort -u`. A value in ALL that already appears in
# SEEN is a hit, everything else is work.
cc_partition() {
    local all="$1" seen="$2" todo="$3" hit="$4"
    LC_ALL=C sort -u "$all" >"$all.sorted.$cc__tmp_suffix"
    LC_ALL=C sort -u "$seen" >"$seen.sorted.$cc__tmp_suffix"
    LC_ALL=C comm -23 "$all.sorted.$cc__tmp_suffix" "$seen.sorted.$cc__tmp_suffix" >"$todo"
    LC_ALL=C comm -12 "$all.sorted.$cc__tmp_suffix" "$seen.sorted.$cc__tmp_suffix" >"$hit"
    rm -f "$all.sorted.$cc__tmp_suffix" "$seen.sorted.$cc__tmp_suffix"
}

# cc_keep FILE  -- values carried over from the previous cache.
cc_keep() {
    [ -s "$1" ] && cat "$1" >>"$CC_KEPT"
    return 0
}

# cc_record FILE -- values proven clean in THIS run.
cc_record() {
    [ -s "$1" ] && cat "$1" >>"$CC_KEPT"
    return 0
}

# cc_save EPOCH -- write the ledger atomically, or leave the old one alone.
#
# The ledger is REPLACED, never appended to: `CC_KEPT` must already contain both
# this run's verdicts (`cc_record`) and the hits carried forward (`cc_keep`). If
# a caller forgets `cc_keep`, the cache shrinks and the next run redoes work --
# slow, still correct. The inverse (appending to a stale ledger) would claim
# work was done under a checker that never looked at it.
#
# Written via a temporary file in the same directory and `mv`, so an interrupted
# run can leave a stale cache but never a half-written one.
cc_save() {
    local epoch="$1"
    [ -n "${CC_DIR:-}" ] || return 0
    if ! LC_ALL=C sort -u "$CC_KEPT" >"$CC_DIR/seen.tsv.tmp.$cc__tmp_suffix" 2>/dev/null; then
        rm -f "$CC_DIR/seen.tsv.tmp.$cc__tmp_suffix"
        return 0
    fi
    mv -f "$CC_DIR/seen.tsv.tmp.$cc__tmp_suffix" "$CC_DIR/seen.tsv"
    printf '%s\n' "$epoch" >"$CC_DIR/epoch.tmp.$cc__tmp_suffix"
    mv -f "$CC_DIR/epoch.tmp.$cc__tmp_suffix" "$CC_DIR/epoch"
}

cc_cleanup() {
    [ -n "${CC_SEEN:-}" ] && rm -f "$CC_SEEN"
    [ -n "${CC_KEPT:-}" ] && rm -f "$CC_KEPT"
    return 0
}
