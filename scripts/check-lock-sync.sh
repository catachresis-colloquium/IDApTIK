#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# check-lock-sync.sh — prove `.github/workflows/actions.lock` agrees with the
# workflow YAML, in both directions, before GitHub proves it by refusing to
# start the run.
#
# ── THE FAILURE THIS EXISTS TO PREVENT ──────────────────────────────────────
#
# GitHub enforces the Actions lockfile at STARTUP, keyed BY WORKFLOW PATH. A
# step-level `uses:` ref that the lockfile does not record under that same
# workflow's own path makes the run unstartable: ZERO jobs are created, there
# is no log, no annotation, and no red X on any job — only
# "This run likely failed because of a workflow file issue." A required check
# that never posts is worse than a red one, because a branch ruleset waiting on
# `code_scanning` is satisfied by nothing and nobody sees the hole.
#
# This repository is a measured case, not a hypothetical. On 2026-09-23 PR #143
# rewrote codeql.yml's two refs from `@v4.38.0` to a bare commit SHA. The
# lockfile still recorded `github/codeql-action@v4.38.0`. Every CodeQL run from
# 2026-09-23T10:38Z onward concluded `startup_failure` with `jobs: 0`, across
# eight runs and four branches, while CI, Pages, Creusot and the secret scanner
# all stayed green — so the tree looked healthy. Dependabot then bumped the SHA
# twice more (#145, #146), each time re-breaking a file that was already broken.
#
# Dependabot rewrites refs in workflow YAML and cannot touch the lockfile, so
# every grouped `github-actions` update desynchronises a repository that has
# not got a gate. Regenerating the lockfile once buys exactly one week. Hence
# this script, and `.github/workflows/lock-sync-gate.yml`, which carries no
# `uses:` of its own and so is structurally immune to the failure it detects.
#
# ── HOW THIS DIFFERS FROM metadatastician/burble's scripts/check-lock-sync.sh ─
#
# Same job, two deliberate deviations, both measured rather than preferred:
#
# 1. POSIX awk only. burble's script uses gawk's three-argument match() and
#    `{n}` regex intervals. Neither is POSIX, and neither exists in mawk, which
#    is /usr/bin/awk on a default Debian/Ubuntu and in this repository's own
#    development containers. AGENTS.md's rule is that a check must be runnable
#    where the work is done; a gate that only runs on a CI image is a gate whose
#    mutants you cannot exercise locally. This repository already paid that
#    price twice: tests/abi_authority_pin_test.sh died with an awk syntax error
#    under mawk, and tests/ci_security_config_test.sh had a mutant that
#    silently did not fire because mawk ignores `{40}`. Both are fixed in the
#    same change as this script.
#
# 2. An unonboarded workflow is judged by its REFS, not condemned outright.
#    burble's script fails any workflow with no lockfile entry. In this
#    repository that would fail two workflows that are measurably healthy:
#    abi-conformance.yml and creusot-proof.yml have never appeared in the
#    lockfile's `workflows:` map, both carry step-level `uses:`, and both run —
#    including creusot-proof.yml straight through PR #145, which rewrote its
#    `actions/upload-artifact` SHA. The difference between them and codeql.yml
#    is that every one of their refs is an immutable 40-hex commit. So:
#      * unonboarded + all refs immutable 40-hex  -> NOTICE, with the count
#      * unonboarded + any movable ref (tag/branch) -> FAIL
#    The FAIL arm is the shape that killed burble's joinery-native.yml
#    (`actions/checkout@v7.0.1` with no lockfile entry). That attribution is an
#    INFERENCE from burble#223's write-up, not a measurement made here, and it
#    is labelled as such in the output.
#
# 3. Every step-level ref ANY workflow uses must have a `dependencies:` entry,
#    onboarded or not. This arm exists because the previous two let a real
#    failure through: on 2026-10-24 this gate reported "actions.lock is in
#    sync" for a tree whose `scan / gitleaks` job died at SET UP with
#    "Unable to resolve action `actions/checkout@3d3c42e5…`: invalid lockfile
#    pin", and metadatastician/evil-weevil hit the same wall on its Dependabot
#    branch (run 39113476510). Both callers use a job-level reusable from
#    another repository. That reusable does its own `actions/checkout`, and a
#    called reusable resolves its actions from the CALLER's `dependencies:`
#    map — so a ref we use ourselves but never declare is a hole that makes a
#    reusable-calling job unstartable. `actions/checkout`, `actions/cache` and
#    `actions/upload-artifact` were all used by creusot-proof.yml and
#    abi-conformance.yml and all three were absent from `dependencies:`, while
#    arms 1–2 passed them (unonboarded, immutable refs) and arm 5 never saw
#    them (it only walks refs the lockfile already records). The arm is
#    offline-checkable, which is why it belongs here and not in a network
#    probe of the reusable.
#
# Exit 0 only when every check passes. There is no warn-only mode for a FAIL
# arm: a desync means GitHub will refuse to start the run.
#
# Usage:
#   scripts/check-lock-sync.sh                # check this repository
#   scripts/check-lock-sync.sh DIR            # check another workflows dir
#   scripts/check-lock-sync.sh --self-test    # 13 controls, 12 of them mutants

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ─────────────────────────────────────────────────────────────────────────────
# The checker. One awk program, two passes: the lockfile, then the YAML.
# POSIX awk only — no match(s,r,arr), no gensub, no {n} intervals, no asort.
# ─────────────────────────────────────────────────────────────────────────────
run_check() { # $1 = workflows dir
  local wf_dir="$1" lock="$1/actions.lock"
  if [ ! -f "$lock" ]; then
    printf 'check-lock-sync: FATAL: no lockfile at %s\n' "$lock" >&2
    printf 'check-lock-sync: 0 workflows examined (denominator zero = no check performed)\n' >&2
    return 1
  fi
  local files=()
  local f
  for f in "$wf_dir"/*.yml "$wf_dir"/*.yaml; do
    [ -e "$f" ] || continue
    case "$(basename "$f")" in actions.lock) continue ;; esac
    files+=("$f")
  done
  if [ "${#files[@]}" -eq 0 ]; then
    printf 'check-lock-sync: FATAL: no workflow files under %s\n' "$wf_dir" >&2
    printf 'check-lock-sync: 0 workflows examined (denominator zero = no check performed)\n' >&2
    return 1
  fi
  awk -v lockfile="$lock" '
  # ── helpers ───────────────────────────────────────────────────────────────
  # owner/repo[/subpath...]@ref  ->  owner/repo@ref   ("" if not an external ref)
  function norm(r,    at, n, path, ref, parts) {
    at = 0
    for (n = length(r); n > 0; n--) if (substr(r, n, 1) == "@") { at = n; break }
    if (at == 0) return ""
    path = substr(r, 1, at - 1); ref = substr(r, at + 1)
    if (path == "" || ref == "") return ""
    if (substr(path, 1, 2) == "./" || substr(path, 1, 2) == "$/") return ""
    if (split(path, parts, "/") < 2) return ""
    return parts[1] "/" parts[2] "@" ref
  }
  function immutable(ref,    at, n) {            # 40 lowercase hex after the last @
    at = 0
    for (n = length(ref); n > 0; n--) if (substr(ref, n, 1) == "@") { at = n; break }
    if (at == 0) return 0
    ref = substr(ref, at + 1)
    if (length(ref) != 40) return 0
    return (ref ~ /^[0123456789abcdef]+$/)
  }
  function strip_quotes(s) { sub(/^["'"'"']/, "", s); sub(/["'"'"']$/, "", s); return s }
  function unquote_first(s) {                    # leading-space + '\''text'\'' -> text
    sub(/^[[:space:]]*'"'"'/, "", s); sub(/'"'"'.*$/, "", s); return s
  }
  function lead_spaces(s,    n) { n = 0; while (substr(s, n + 1, 1) == " ") n++; return n }
  function keypath(file,    b) { b = file; sub(/.*\//, "", b); return ".github/workflows/" b }

  # ── pass 1: the lockfile ──────────────────────────────────────────────────
  FILENAME == lockfile {
    if ($0 ~ /^workflows:[[:space:]]*$/)    { section = "wf";   next }
    if ($0 ~ /^dependencies:[[:space:]]*$/) { section = "deps"; next }
    if ($0 ~ /^[A-Za-z_]+:/)                { section = "";     next }

    if (section == "wf") {
      if ($0 ~ /^    '"'"'/) {
        cur = unquote_first($0)
        onboarded[cur] = 1; nwf++
        if ($0 ~ /\[[[:space:]]*\][[:space:]]*$/) lockn[cur] = 0
        next
      }
      if ($0 ~ /^        - '"'"'/ && cur != "") {
        r = unquote_first(substr($0, 11))
        lock[cur, r] = 1; lockn[cur]++; lockrefs = lockrefs " " cur "|" r; nlockrefs++
        next
      }
      next
    }
    if (section == "deps") {
      if ($0 ~ /^    '"'"'/) { dep = unquote_first($0); depseen[dep] = 1; ndeps++; next }
      if ($0 ~ /^        commit: / && dep != "") {
        c = $0; sub(/^[[:space:]]*commit:[[:space:]]*'"'"'/, "", c); sub(/'"'"'.*$/, "", c)
        depcommit[dep] = c
        next
      }
      next
    }
    next
  }

  # ── pass 2: the workflow YAML ─────────────────────────────────────────────
  FNR == 1 { wf = FILENAME; k = keypath(wf); scanned[k] = 1; nscanned++ }
  {
    line = $0
    if (line ~ /^[[:space:]]*#/) next                 # whole-line comment
    sub(/[[:space:]]+#.*$/, "", line)                 # trailing comment
    if (line !~ /^[[:space:]]*-?[[:space:]]*uses:[[:space:]]*/) next

    indent = lead_spaces(line)
    body = substr(line, indent + 1)
    if (substr(body, 1, 2) == "- ") { body = substr(body, 3); indent += 2 }
    sub(/^uses:[[:space:]]*/, "", body)
    raw = strip_quotes(body)
    sub(/[[:space:]]*$/, "", raw)
    if (raw == "") next

    if (raw ~ /^\$\//) { dollars[k] = dollars[k] " " raw; next }   # tool corruption
    n = norm(raw)
    if (n == "") next                                   # local ./ or $/ action

    if (indent <= 4) {                                  # job-level reusable call
      jobrefs[k] = jobrefs[k] " " n; njob++
    } else {                                            # step-level action
      if (!((k SUBSEP n) in stepseen)) { stepseen[k, n] = 1; steps[k] = steps[k] " " n; nstep++ }
      if (!(n in usedany)) { usedany[n] = k; nusedany++ }
    }
  }

  END {
    bad = 0
    printf "check-lock-sync: examined %d workflow file(s), %d step-level ref(s), %d job-level ref(s), %d lockfile workflow entr(ies), %d lockfile dependenc(ies)\n",
           nscanned, nstep, njob, nwf, ndeps
    if (nscanned == 0) { print "DENOMINATOR ZERO: nothing was examined; that is not a pass"; exit 1 }

    # -- 0. the `$/` local-action rewrite that gh actions-lock fix mode has caused
    for (k in dollars) {
      printf "FAIL %s\n     invalid local-action rewrite (uses: $/...):%s\n", k, dollars[k]
      bad = 1
    }

    # -- 1. onboarded workflows: exact set equality, both directions
    nchecked = 0
    for (k in scanned) {
      if (!(k in onboarded)) continue
      nchecked++
      missing = ""; nu = split(steps[k], u, " ")
      for (j = 1; j <= nu; j++) {
        if (u[j] == "") continue
        if (!((k SUBSEP u[j]) in lock)) missing = missing " " u[j]
      }
      if (missing != "") {
        printf "FAIL %s\n     step-level refs missing from the lockfile under this path:%s\n", k, missing
        printf "     GitHub will refuse to START this run: zero jobs, no log, no red X.\n"
        bad = 1
      }
      orphan = ""
      for (lr in lock) {
        split(lr, kp, SUBSEP)
        if (kp[1] != k) continue
        if (!((k SUBSEP kp[2]) in stepseen)) orphan = orphan " " kp[2]
      }
      if (orphan != "") {
        printf "FAIL %s\n     stale lockfile entries no uses: references:%s\n", k, orphan
        bad = 1
      }
    }

    # -- 2. unonboarded workflows: judged by ref shape, not condemned outright
    nnotice = 0
    for (k in scanned) {
      if (k in onboarded) continue
      movable = ""; nu = split(steps[k], u, " ")
      if (nu == 0) continue
      for (j = 1; j <= nu; j++) {
        if (u[j] == "" ) continue
        if (!immutable(u[j])) movable = movable " " u[j]
      }
      if (movable != "") {
        printf "FAIL %s\n     not onboarded AND carrying a movable (tag/branch) ref:%s\n", k, movable
        printf "     Either onboard it (record the refs under this path in actions.lock)\n"
        printf "     or pin the ref to an immutable 40-hex commit.\n"
        printf "     NOTE: the startup-death attribution for this exact shape is an\n"
        printf "     INFERENCE from metadatastician/burble#223 (joinery-native.yml),\n"
        printf "     not a measurement made in this repository.\n"
        bad = 1
      } else {
        nnotice++
        printf "notice %s\n     unonboarded, but all %d step-level ref(s) are immutable 40-hex commits\n", k, nu
      }
    }

    # -- 3. job-level reusable refs: stale recorded entry is fatal, absent is not
    for (k in scanned) {
      nj = split(jobrefs[k], jr, " ")
      for (j = 1; j <= nj; j++) {
        if (jr[j] == "") continue
        recorded = 0
        for (lr in lock) { split(lr, kp, SUBSEP); if (kp[1] == k && kp[2] == jr[j]) recorded = 1 }
        if (recorded) continue
        # is a DIFFERENT ref of the same action recorded under this path? then it is stale
        stale = ""
        for (lr in lock) {
          split(lr, kp, SUBSEP)
          if (kp[1] != k) continue
          split(kp[2], a, "@"); split(jr[j], b, "@")
          if (a[1] == b[1] && kp[2] != jr[j]) stale = stale " " kp[2]
        }
        if (stale != "") {
          printf "FAIL %s\n     job-level reusable ref %s disagrees with the recorded%s\n", k, jr[j], stale
          printf "     burble measured this shape killing mirror.yml at startup even though\n"
          printf "     gh actions-lock v0.1.6 reported the lockfile complete.\n"
          bad = 1
        } else {
          printf "notice %s\n     job-level reusable ref %s is not recorded in the lockfile\n", k, jr[j]
          printf "       gh actions-lock v0.1.6 cannot see job-level refs (standards#969);\n"
          printf "       a MISSING job-level ref does not cause startup death (standards#969,\n"
          printf "       12/12 sampled repos created jobs). Left unrecorded deliberately.\n"
        }
      }
    }

    # -- 4. a lockfile entry that pins nothing is not a pin
    npinned = 0
    for (d in depseen) {
      c = depcommit[d]
      if (c !~ /^sha1-[0123456789abcdef]+$/ || length(c) != 45) {
        printf "FAIL dependencies: %s has no resolvable sha1-<40hex> commit (got: %s)\n", d, (c == "" ? "<absent>" : c)
        bad = 1
      } else npinned++
    }
    if (ndeps > 0 && npinned == 0) { print "DENOMINATOR ZERO: no dependency resolved to a commit"; bad = 1 }

    # -- 5. every ref any workflow uses must have a dependency entry
    nresolvable = 0
    m = split(lockrefs, allrefs, " ")
    for (j = 1; j <= m; j++) {
      if (allrefs[j] == "") continue
      split(allrefs[j], pp, "|")
      if (!(pp[2] in depseen)) {
        printf "FAIL %s\n     lockfile records %s but dependencies: has no entry for it\n", pp[1], pp[2]
        bad = 1
      } else nresolvable++
    }

    # -- 5b. every step-level ref ANY workflow uses must be declared in
    #        dependencies:, whether or not its own workflow is onboarded.
    #        A called reusable resolves its own actions from this same map,
    #        so an undeclared ref here is what made `scan / gitleaks` die at
    #        set up on 2026-10-24 while this script reported "in sync".
    for (r in usedany) {
      if (r in depseen) { nuseddeclared++; continue }
      printf "FAIL %s\n     uses %s, but no dependencies: entry declares it\n", usedany[r], r
      printf "     A called reusable resolves its OWN actions from this same\n"
      printf "     dependencies: map. A ref used here but declared nowhere is the\n"
      printf "     hole that makes a reusable-calling job unstartable at set up.\n"
      bad = 1
    }

    # -- 6. lockfile entries for workflow files that no longer exist
    for (p in onboarded) {
      if (!(p in scanned)) {
        printf "FAIL %s\n     lockfile entry for a workflow file that does not exist\n", p
        bad = 1
      }
    }

    printf "check-lock-sync: %d onboarded workflow(s) checked both directions, %d unonboarded with immutable refs (notice), %d/%d dependencies resolved to a commit, %d/%d recorded refs resolvable, %d/%d used refs declared in dependencies:\n",
           nchecked, nnotice, npinned, ndeps, nresolvable, nlockrefs, nuseddeclared, nusedany
    if (nchecked == 0 && nnotice == 0) {
      print "DENOMINATOR ZERO: no workflow was checked in either direction; that is not a pass"
      bad = 1
    }

    if (bad) {
      print ""
      print "actions.lock is OUT OF SYNC with the workflow YAML."
      print "GitHub refuses such a run at startup: zero jobs are created and the run"
      print "reports only \"This run likely failed because of a workflow file issue.\""
      print ""
      print "Fix: agree the two files by hand, or run"
      print "       gh actions-lock --no-migrate-local-actions --no-narrow"
      print "     then review the diff. The released tool does not see job-level"
      print "     reusable refs and has de-pinned bare SHAs to floating tags, so its"
      print "     output is a draft, not an answer. Re-run this script afterwards."
      exit 1
    }
    print "actions.lock is in sync: every step-level ref in an onboarded workflow is"
    print "recorded under its own path, every recorded ref is used, every dependency"
    print "resolves to a commit, no unonboarded workflow carries a movable ref, and"
    print "every step-level ref any workflow uses is declared in dependencies:."
  }
  ' "$lock" "${files[@]}"
}

# ─────────────────────────────────────────────────────────────────────────────
# Self-test. A gate that cannot fail is worse than no gate, so every arm above
# gets a mutant that must kill it, plus a clean control that must pass. Each
# mutant must fail FOR THE NAMED REASON: a mutant that trips a parse error
# proves nothing about the check it was built for.
# ─────────────────────────────────────────────────────────────────────────────
self_test() {
  local root fixture failures=0
  root="$(mktemp -d)"
  trap 'rm -rf "$root"' RETURN

  note() { printf '  ok: %s\n' "$1"; }
  fail() { printf '  FAIL: %s\n' "$1" >&2; failures=$((failures + 1)); }

  expect_pass() { # $1 desc, $2 dir
    if run_check "$2" >/dev/null 2>&1; then note "$1"
    else printf '  FAIL: expected success: %s\n' "$1" >&2; run_check "$2" 2>&1 | sed 's/^/      /' >&2; failures=$((failures + 1)); fi
  }
  expect_fail() { # $1 desc, $2 wanted-substring, $3 dir
    local out
    if out="$(run_check "$3" 2>&1)"; then
      printf '  FAIL: expected failure: %s\n' "$1" >&2; failures=$((failures + 1))
    elif ! printf '%s' "$out" | grep -qF "$2"; then
      printf '  FAIL: %s failed for the wrong reason (wanted %q):\n%s\n' "$1" "$2" "$out" >&2
      failures=$((failures + 1))
    else note "$1"; fi
  }

  # Build a clean tree, then mutate copies of it.
  new_case() { # $1 name -> prints the case dir
    local d="$root/$1"
    mkdir -p "$d/.github/workflows"
    cat > "$d/.github/workflows/ci.yml" <<'YML'
name: CI
on: [push]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Beam
        uses: erlef/setup-beam@v1.24.1
YML
    cat > "$d/.github/workflows/pinned.yml" <<'YML'
name: Pinned
on: [push]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
YML
    cat > "$d/.github/workflows/reusable.yml" <<'YML'
name: Reusable
on: [push]
jobs:
  scan:
    permissions:
      contents: read
    uses: hyperpolymath/standards/.github/workflows/secret-scanner-reusable.yml@092dedada188f56c5915f74a5fd40aac093742c3
YML
    cat > "$d/.github/workflows/actions.lock" <<'LOCK'
version: 'v0.0.2'
workflows:
    '.github/workflows/ci.yml':
        - 'erlef/setup-beam@v1.24.1'
    '.github/workflows/reusable.yml': []
dependencies:
    'actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1':
        ref: 'v7.0.1'
        commit: 'sha1-3d3c42e5aac5ba805825da76410c181273ba90b1'
        owner_id: 44036562
        repo_id: 197814629
    'erlef/setup-beam@v1.24.1':
        ref: 'v1.24.1'
        commit: 'sha1-54075bcc5e249e4758d363f27d099f55d843f124'
        owner_id: 47606891
        repo_id: 331103973
LOCK
    printf '%s\n' "$d/.github/workflows"
  }

  printf 'clean control:\n'
  fixture="$(new_case clean)"
  expect_pass 'the clean tree passes and prints a non-zero denominator' "$fixture"
  if run_check "$fixture" 2>&1 | grep -q 'examined 3 workflow file(s), 2 step-level ref(s), 1 job-level ref(s)'; then
    note 'the denominator names every file and ref it examined'
  else
    printf '  FAIL: denominator line wrong or absent\n' >&2; failures=$((failures + 1))
  fi

  printf 'mutants that must kill the gate:\n'

  fixture="$(new_case bump-without-lock)"
  sed -i 's/erlef\/setup-beam@v1\.24\.1/erlef\/setup-beam@v1.25.0/' "$fixture/ci.yml"
  expect_fail 'a Dependabot-style ref bump with no lockfile update is rejected' \
    'step-level refs missing from the lockfile' "$fixture"

  fixture="$(new_case sha-for-tag)"
  # The exact PR #143 mutation: readable tag in the lock, bare SHA in the YAML.
  sed -i 's|erlef/setup-beam@v1.24.1|erlef/setup-beam@54075bcc5e249e4758d363f27d099f55d843f124 # v1.24.1|' \
    "$fixture/ci.yml"
  expect_fail 're-pinning a lock-recorded tag to a bare SHA in the YAML is rejected (PR #143)' \
    'step-level refs missing from the lockfile' "$fixture"

  fixture="$(new_case stale-entry)"
  sed -i "s/'erlef\/setup-beam@v1.24.1'/'erlef\/setup-beam@v9.9.9'/" \
    "$fixture/actions.lock"
  # The YAML still says v1.24.1, so BOTH arms must fire: missing and stale.
  expect_fail 'a lockfile entry nothing references is rejected' 'stale lockfile entries' "$fixture"

  fixture="$(new_case movable-unonboarded)"
  sed -i 's|actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1|actions/checkout@v7.0.1|' \
    "$fixture/pinned.yml"
  expect_fail 'an unonboarded workflow carrying a movable tag ref is rejected' \
    'not onboarded AND carrying a movable' "$fixture"

  fixture="$(new_case dollar-rewrite)"
  sed -i 's|actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1|$/checkout|' "$fixture/pinned.yml"
  expect_fail 'the gh actions-lock fix-mode $/ local-action rewrite is rejected' \
    'invalid local-action rewrite' "$fixture"

  fixture="$(new_case no-commit)"
  sed -i "s/commit: 'sha1-54075bcc5e249e4758d363f27d099f55d843f124'/commit: 'sha1-nope'/" \
    "$fixture/actions.lock"
  expect_fail 'a lockfile dependency that resolves to no commit is rejected' \
    'has no resolvable sha1-' "$fixture"

  fixture="$(new_case missing-dep)"
  sed -i "/^    'erlef\/setup-beam@v1.24.1':$/,/^        repo_id:/d" "$fixture/actions.lock"
  expect_fail 'a recorded ref with no dependencies: entry is rejected' \
    'but dependencies: has no entry for it' "$fixture"

  fixture="$(new_case undeclared-step-ref)"
  # The 2026-10-24 shape: a ref used by an UNONBOARDED workflow, with no
  # dependencies: entry. Arms 1-2 pass it (no lockfile path, immutable ref) and
  # arm 5 never walks it, so before arm 5b the gate called this tree "in sync"
  # while GitHub refused to start a job that called a reusable using the same
  # ref. Deleting the checkout block is exactly that tree.
  sed -i "/^    'actions\/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1':$/,/^        repo_id:/d" \
    "$fixture/actions.lock"
  expect_fail 'a ref used by an unonboarded workflow with no dependencies: entry is rejected' \
    'no dependencies: entry declares it' "$fixture"

  fixture="$(new_case stale-jobref)"
  sed -i "s/'.github\/workflows\/reusable.yml': \[\]/'.github\/workflows\/reusable.yml':\n        - 'hyperpolymath\/standards@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'/" \
    "$fixture/actions.lock"
  expect_fail 'a recorded job-level ref that disagrees with the YAML is rejected (burble mirror.yml)' \
    'job-level reusable ref' "$fixture"

  fixture="$(new_case deleted-workflow)"
  rm "$fixture/ci.yml"
  expect_fail 'a lockfile entry for a deleted workflow file is rejected' \
    'workflow file that does not exist' "$fixture"

  printf 'emptiness guards (two empty sets compare equal):\n'

  fixture="$(new_case no-lockfile)"
  rm "$fixture/actions.lock"
  expect_fail 'a missing lockfile is a failure, not a skip' 'no lockfile at' "$fixture"

  fixture="$(new_case no-workflows)"
  rm -f "$fixture"/*.yml
  expect_fail 'a workflows directory with no workflows is a failure, not a skip' \
    'no workflow files under' "$fixture"

  printf 'portability control:\n'
  # The reason this script is POSIX awk: the gawk-only forms this replaces were
  # measured dying under mawk in two sibling test suites. Prove the interpreter
  # in use is named, so a future reader knows which one was exercised.
  if awk 'BEGIN { if (match("abc", /b/)) exit 0; exit 1 }' 2>/dev/null; then
    note "awk is $(awk -W version 2>&1 | head -1 || awk --version 2>&1 | head -1)"
  else
    printf '  FAIL: awk could not run a trivial match()\n' >&2; failures=$((failures + 1))
  fi

  if [ "$failures" -ne 0 ]; then
    printf 'check-lock-sync self-test: %d FAILURE(S)\n' "$failures" >&2
    return 1
  fi
  printf 'check-lock-sync self-test: all clean and firing fixtures behaved\n'
}

case "${1:-}" in
  --self-test) self_test ;;
  "")          run_check "$REPO_DIR/.github/workflows" ;;
  *)           run_check "$1" ;;
esac
