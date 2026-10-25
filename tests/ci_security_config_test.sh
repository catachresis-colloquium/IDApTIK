#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Offline regression coverage for this repository's CI/CD security configuration.
#
# The production inputs are declarative YAML, so this suite checks their
# security-relevant structure directly and then mutates temporary copies to
# prove each assertion can fail. It needs no network, no action runner, and no
# project toolchain — Bash, mikefarah/yq v4, jq and standard text utilities.
# YAML values are obtained only through yq (standards Y-1), including KYAML.
#
# Originally added by PR #143. Rewritten here for three measured reasons:
#
# 1. IT WAS NEVER INVOKED. PR #143 committed 267 lines at mode 100755 and wired
#    them into nothing — no justfile recipe, no workflow step. That is the
#    estate's recurring vacuous-gate shape and it is strictly worse than no
#    test, because a reader assumes the CI configuration is guarded. Issue #141
#    is the same defect in tests/abi_authority_pin_test.sh; both are wired in
#    the same change. `just ci-security-config-test` and the `fixtures-and-must`
#    job in .github/workflows/ci.yml now call this file.
#
# 2. IT FAILED ON A CLEAN TREE. Its own "init and analyze cannot drift to
#    different revisions" mutant did not fire, because the mutant used the regex
#    interval `/@[0-9a-f]{40}/` and mawk — /usr/bin/awk on a default
#    Debian/Ubuntu, and the awk in this repository's development containers —
#    does not implement `{n}` intervals. The substitution silently did nothing,
#    the mutant equalled the original, and the suite reported one FAIL out of
#    eleven. Nothing caught that, because nothing ran it. Every regex here is
#    now POSIX: no `{n}` intervals, no gawk three-argument match().
#
# 3. ITS CODEQL POLICY WAS WRONG, AND BEING WRONG KILLED THE WORKFLOW. It
#    required `github/codeql-action` refs to be bare 40-hex commit SHAs. But
#    .github/workflows/actions.lock is authoritative for what runs, and GitHub
#    enforces it per workflow path at STARTUP: a ref the lockfile does not
#    record under that path makes the run unstartable — zero jobs, no log, only
#    "This run likely failed because of a workflow file issue." PR #143 obeyed
#    this suite, re-pinned both refs from `@v4.38.0` to a SHA, and every CodeQL
#    run from 2026-09-23T10:38Z onward rendered no verdict at all — while
#    `code_scanning` is a required rule on main. The assertion below now checks
#    what actually pins: the ref must be recorded in the lockfile under this
#    workflow's own path, and that record must resolve to a sha1-<40hex>
#    commit. A bare SHA in the YAML satisfies it only if the lockfile records
#    that same SHA.
#
# Checks are grouped and every group prints its denominator, because a check
# that examined nothing and reported "ok" is the defect this file was written
# to prevent.

set -euo pipefail
command -v yq >/dev/null || { printf 'yq is required (mise install)\n' >&2; exit 1; }
yq --version | grep -q 'mikefarah/yq/.*version v4\.' || { printf 'mikefarah/yq v4 is required\n' >&2; exit 1; }
command -v jq >/dev/null || { printf 'jq is required\n' >&2; exit 1; }

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEPENDABOT="$REPO_DIR/.github/dependabot.yml"
CODEQL="$REPO_DIR/.github/workflows/codeql.yml"
CI="$REPO_DIR/.github/workflows/ci.yml"
LOCK="$REPO_DIR/.github/workflows/actions.lock"
FIXTURE="$(mktemp -d)"
failures=0
checks=0

trap 'rm -rf "$FIXTURE"' EXIT

# ── EMIT GITHUB ANNOTATIONS AS WELL AS CONSOLE LINES ────────────────────────
#
# The Actions log endpoint has been unreachable from this repository's
# development environment twice in one day: once for the Pages failure that took
# the website dark, and once for this suite. Both times the only evidence
# obtainable was an annotation reading "Process completed with exit code 1."
# against a step number -- the log blob lives on a host this environment cannot
# reach, and the rendered job page requires sign-in even for a public repo.
#
# A gate whose failure reason is trapped in an unreadable log is a much weaker
# gate than it looks. Annotations are readable through the check-runs API
# without the log blob, so a failure that annotates is a failure that can still
# be diagnosed when the log cannot be fetched. This is not CI decoration; it is
# the difference between "step 6 failed" and "the sonar prose arm failed".
#
# Only active under GITHUB_ACTIONS=true, so local runs are unaffected.
annotate() { # $1 level, $2 message
  [ "${GITHUB_ACTIONS:-}" = "true" ] || return 0
  local m
  # Workflow commands are single-line and treat % as an escape introducer, so %
  # must be escaped FIRST, then the line terminators folded. Escaping in the
  # other order turns every folded newline into literal %250A text.
  m="$(printf '%s' "$2" | sed -e 's/%/%25/g' -e ':a' -e 'N' -e '$!ba' -e 's/\n/ | /g')"
  printf '::%s::%s\n' "$1" "$m"
}

note() { checks=$((checks + 1)); printf '  ok: %s\n' "$1"; }

fail() {
  printf '  FAIL: %s\n' "$1" >&2
  annotate error "ci-security-config: $1"
  failures=$((failures + 1))
}

# The failure text a human needs is on STDERR, and on this repository's runners
# stderr lands in a log blob that is not always reachable (see the annotation
# note above). So the evidence -- not just the assertion name -- goes into the
# annotation too. `%0A`-free: annotate() already folds newlines into " | ".
capture() { # rest = command; prints its combined output, never fails
  "$@" 2>&1 || true
}

# stdout is discarded but STDERR IS NOT. Every extractor below prints its own
# denominator to stderr, and a printed denominator that nobody can see is worth
# exactly as much as no denominator at all: the console transcript is the
# evidence that the check examined 19 tuples rather than zero and reported ok
# either way. This is why the passing runs below are noisy on purpose.
expect_pass() { # $1 description, rest = command
  local desc="$1"
  shift
  if "$@" >/dev/null; then
    note "$desc"
  else
    # `$1` here -- after the shift -- is the command's first ARGUMENT, a path,
    # not the assertion name. The original diagnostic printed that, so a failure
    # read as "expected success: /home/runner/work/..." and named nothing.
    # `$desc` is captured before the shift for exactly this reason.
    printf '  FAIL: expected success: %s\n' "$desc" >&2
    annotate error "ci-security-config: expected success: $desc -- $(capture "$@" | tail -c 600)"
    "$@" 2>&1 | sed 's/^/        /' >&2 || true
    failures=$((failures + 1))
  fi
}

expect_fail() { # $1 description, $2 expected output, rest = command
  local desc="$1" want="$2" out
  shift 2
  if out="$("$@" 2>&1)"; then
    fail "expected failure: $desc (a mutant that no longer mutates reads as a pass)"
  elif ! grep -qF "$want" <<< "$out"; then
    printf '  FAIL: %s failed for the wrong reason (wanted %s):\n%s\n' "$desc" "$want" "$out" >&2
    annotate error "ci-security-config: $desc failed for the wrong reason (wanted: $want) -- got: $(printf '%s' "$out" | tail -c 600)"
    failures=$((failures + 1))
  else
    note "$desc"
  fi
}

# Every extraction below guards its own emptiness. Two empty sets compare equal,
# so a grep or awk that silently matches nothing is the commonest way a gate
# like this goes vacuous — the exact defect docs/abi/ABI-AUTHORITY.md's gate had
# to be hardened against (issue #119).
#
# A COUNT of zero is the same failure as an empty string, and this guard treats
# them alike. It did not originally: `[ -z "0" ]` is false, so a counter that
# came back 0 because its parser was broken read as "one thing examined". The
# script-mode check below was written that way and its own firing fixture caught
# it on the first run — a check that reports ok while examining nothing.
require_nonempty() { # $1 label, $2 value-or-count
  if [ -z "$2" ] || [ "$2" = 0 ]; then
    printf 'DENOMINATOR ZERO: extracted nothing for %s; refusing to compare\n' "$1" >&2
    return 1
  fi
}

# ── dependabot: the grouped-actions pull-request cap ─────────────────────────
#
# Grouped `patterns: ["*"]` means one pull request carries every action bump at
# once. Uncapped, a busy week produces a queue of them, and because Dependabot
# cannot update actions.lock, EVERY one of those pull requests desynchronises
# the lockfile (see scripts/check-lock-sync.sh). The cap is what keeps the
# blast radius to one merge.
check_dependabot_limit() { # $1 configuration
  local blocks
  blocks="$(yq -p yaml -o=json -I=0 '.' "$1" |
    jq -c '[.updates[]? | select(."package-ecosystem" == "github-actions")]')" || return 1
  if ! jq -e 'length == 1' <<< "$blocks" >/dev/null; then
    printf 'expected exactly one github-actions update block\n' >&2; return 1
  fi
  if ! jq -e '.[0] | has("open-pull-requests-limit")' <<< "$blocks" >/dev/null; then
    printf 'expected exactly one github-actions open-pull-requests-limit\n' >&2; return 1
  fi
  if ! jq -e '.[0]."open-pull-requests-limit" == 2' <<< "$blocks" >/dev/null; then
    printf 'github-actions open-pull-requests-limit must be integer 2\n' >&2; return 1
  fi
}

# ── dependabot: no ecosystem may point at a directory that does not exist ─────
#
# A live defect, not a hypothetical. Issue #74 retired this repository's own
# `server/` relay in favour of the external metadatastician/burble fabric, and
# the Elixir/Mix ecosystem block was left pointing at `/server`. Dependabot has
# failed its weekly `hex in /server` update ever since — run 36117293992 on
# 2026-09-25 concluded `failure` with "Dependabot encountered an error
# performing the update". A red run nobody reads trains everyone to ignore red
# runs, and it hides the ecosystem that WOULD matter if the relay ever came
# home. `.gitignore` and `.gitattributes` carried the same corpse.
check_dependabot_directories() { # $1 configuration, $2 repository root
  local root="$2" dir dirs count=0
  dirs="$(yq -p yaml -o=json -I=0 '.' "$1" |
    jq -er '.updates[] | (.directory // .directories[]) | select(type == "string")')" || return 1
  while IFS= read -r dir; do
    count=$((count + 1))
    if [ ! -d "$root$dir" ]; then
      printf 'dependabot: %s does not exist under %s\n' "$dir" "$root" >&2
      return 1
    fi
  done <<< "$dirs"
  require_nonempty "dependabot directory declarations" "$count"
}

# ── codeql: entry points, step names, agreement, and the lockfile pin ────────
check_codeql_pins() { # $1 workflow, $2 lockfile
  local records
  records="$(
    yq -p yaml -o=json -I=0 '.' "$2" | jq -r '
      (.workflows[".github/workflows/codeql.yml"][]? | ["L", .] | @tsv),
      (.dependencies | to_entries[] | ["D", .key, (.value.commit // "")] | @tsv)' || exit 1
    yq -p yaml -o=json -I=0 '.' "$1" | jq -r '
      .jobs[].steps[]? | select((.uses // "") | startswith("github/codeql-action/")) |
      ["C", (.name // ""), .uses] | @tsv' || exit 1
  )" || return 1
  awk -F '\t' '
    # owner/repo[/subpath...]@ref -> owner/repo@ref
    function action_key(r,   at, n, path, ref, parts) {
      at = 0
      for (n = length(r); n > 0; n--) if (substr(r, n, 1) == "@") { at = n; break }
      if (at == 0) return ""
      path = substr(r, 1, at - 1); ref = substr(r, at + 1)
      if (path == "" || ref == "") return ""
      if (split(path, parts, "/") < 2) return ""
      return parts[1] "/" parts[2] "@" ref
    }
    function resolved_commit(key,   c) {
      c = depcommit[key]
      if (c ~ /^sha1-[0123456789abcdef]+$/ && length(c) == 45) return substr(c, 6)
      return ""
    }

    $1 == "L" { locked[$2] = 1; nlocked++; next }
    $1 == "D" { depcommit[$2] = $3; next }
    $1 == "C" {
      step = $2; ref = $3
      nrefs++

      if (split(ref, parts, "@") != 2) {
        printf "malformed CodeQL action reference: %s\n", ref > "/dev/stderr"
        failed = 1; next
      }
      action = parts[1]; pin = parts[2]
      key = action_key(ref)

      if (action == "github/codeql-action/init") {
        init_count++; init_pin = pin; init_key = key
        if (step != "Initialize CodeQL") {
          printf "CodeQL init action is attached to unexpected step: %s\n", step > "/dev/stderr"
          failed = 1
        }
      } else if (action == "github/codeql-action/analyze") {
        analyze_count++; analyze_pin = pin; analyze_key = key
        if (step != "Perform CodeQL Analysis") {
          printf "CodeQL analyze action is attached to unexpected step: %s\n", step > "/dev/stderr"
          failed = 1
        }
      } else {
        printf "unexpected CodeQL action entry point: %s\n", action > "/dev/stderr"
        failed = 1
      }

      # THE PIN. Not "is it a 40-hex SHA" — that policy is what killed this
      # workflow (PR #143). The lockfile is authoritative for what runs, so the
      # ref must be recorded under this workflow'"'"'s own path AND resolve to a
      # commit. A bare SHA that the lockfile does not record is not a pin; it is
      # a startup failure.
      if (!(key in locked)) {
        printf "CodeQL ref %s is not recorded in actions.lock under .github/workflows/codeql.yml\n", key > "/dev/stderr"
        printf "  GitHub enforces that lockfile per workflow path at startup: an unrecorded\n" > "/dev/stderr"
        printf "  ref makes the run unstartable (zero jobs, no verdict). Record the ref in\n" > "/dev/stderr"
        printf "  the lockfile, or restore the readable ref the lockfile already records.\n" > "/dev/stderr"
        failed = 1
      } else if (resolved_commit(key) == "") {
        printf "CodeQL ref %s is recorded but resolves to no sha1-<40hex> commit\n", key > "/dev/stderr"
        failed = 1
      }
    }

    END {
      printf "codeql: %d action ref(s) in the workflow, %d ref(s) recorded for this path in the lockfile\n",
             nrefs, nlocked > "/dev/stderr"
      if (nrefs == 0) { printf "DENOMINATOR ZERO: found no CodeQL action refs to check\n"; exit 1 }
      if (nlocked == 0) { printf "DENOMINATOR ZERO: actions.lock records nothing for codeql.yml\n"; exit 1 }
      if (init_count != 1) {
        printf "expected exactly one CodeQL init action, found %d\n", init_count > "/dev/stderr"
        failed = 1
      }
      if (analyze_count != 1) {
        printf "expected exactly one CodeQL analyze action, found %d\n", analyze_count > "/dev/stderr"
        failed = 1
      }
      if (init_count == 1 && analyze_count == 1 && init_key != analyze_key) {
        printf "CodeQL init and analyze actions must resolve to the same action revision\n" > "/dev/stderr"
        printf "  init=%s analyze=%s\n", init_key, analyze_key > "/dev/stderr"
        failed = 1
      }
      exit failed
    }
  ' <<< "$records"
}

# ── ci.yml: a push on main must never be superseded (issue #142) ─────────────
#
# `cancel-in-progress: true` under a ref-keyed group meant a second merge inside
# the abi-model job's 8-14 minute runtime cancelled the first run. `abi-model`
# then reported `cancelled` — no verdict at all — and almost everything that
# consumes check results reads `cancelled` as "not red". Measured: 3 of the last
# 13 completed abi-model outcomes on main were cancellations, clustering exactly
# where merges landed inside its runtime.
check_ci_concurrency() { # $1 ci workflow
  local data group cancel
  data="$(yq -p yaml -o=json -I=0 '.concurrency' "$1")" || return 1
  group="$(jq -r '.group // ""' <<< "$data")" || return 1
  cancel="$(jq -r '."cancel-in-progress" // ""' <<< "$data")" || return 1
  if [[ "$group" != *github.run_id* ]]; then
    printf 'concurrency group does not isolate push/dispatch runs by github.run_id\n' >&2; return 1
  fi
  if [ "$cancel" != "\${{ github.event_name == 'pull_request' }}" ]; then
    printf 'cancel-in-progress is not scoped to pull_request events\n' >&2; return 1
  fi
}

# ── ci.yml: every long job must be able to time out ──────────────────────────
#
# Issue #142 records that ci.yml had no `timeout-minutes` anywhere, so the ~10
# minutes elapsed on the cancelled abi-model run was the gap to the next push
# rather than a timeout — a misleading coincidence. Now that a push run cannot
# be cancelled at all, a hung job would hold a runner for the six-hour default.
check_ci_timeouts() { # $1 ci workflow
  local jobs
  jobs="$(yq -p yaml -o=json -I=0 '.jobs' "$1")" || return 1
  if ! jq -e 'type == "object" and length > 0' <<< "$jobs" >/dev/null; then
    printf 'DENOMINATOR ZERO: found no jobs in ci.yml\n' >&2; return 1
  fi
  if ! jq -e 'all(.[]; (."timeout-minutes" | type == "number" and . > 0))' <<< "$jobs" >/dev/null; then
    printf 'job(s) with no timeout-minutes\n' >&2; return 1
  fi
}

# ── every test script in tests/ must be invoked by something ─────────────────
#
# Issues #141 and its sibling: two suites in tests/ were committed executable
# and wired into nothing. This is the check that stops the shape recurring. It
# counts its own denominator and fails on zero, so it cannot pass by examining
# nothing.
check_no_dead_test_scripts() { # $1 repository root
  local root="$1" f base hits workflow runs="" total=0 dead=""
  for workflow in "$root"/.github/workflows/*.yml "$root"/.github/workflows/*.yaml; do
    [ -f "$workflow" ] || continue
    runs+="$(yq -p yaml -o=json -I=0 '.' "$workflow" | jq -r '.jobs[]?.steps[]?.run // empty')" || return 1
    runs+=$'\n'
  done
  for f in "$root"/tests/*.sh; do
    [ -e "$f" ] || continue
    base="$(basename "$f")"
    total=$((total + 1))
    # Invoked from the justfile, or from any workflow, or from another test.
    hits=$(grep -rl --exclude-dir=.git -F "$base" \
      "$root/justfile" "$root/scripts" 2>/dev/null | wc -l | tr -d ' ')
    if grep -qF "$base" <<< "$runs"; then hits=$((hits + 1)); fi
    if [ "$hits" -eq 0 ]; then dead="$dead $base"; fi
  done
  if [ "$total" -eq 0 ]; then
    printf 'DENOMINATOR ZERO: no test scripts found under %s/tests\n' "$root" >&2
    return 1
  fi
  printf 'tests/: %d shell suite(s) examined, %d wired into justfile/workflows/scripts\n' \
    "$total" "$((total - $(printf '%s' "$dead" | wc -w)))" >&2
  if [ -n "$dead" ]; then
    printf 'unreferenced test suite(s) invoked by nothing — a gate nothing calls protects nothing:%s\n' "$dead" >&2
    return 1
  fi
}

# ── a script with a shebang must carry the executable bit IN GIT ─────────────
#
# Cosmetic until it is not. Two of the seventeen scripts here
# (scripts/abi_model_check.sh, scripts/zig_adapter_surface_check.sh) were
# committed at mode 100644 while every sibling was 100755. Both are always
# invoked as `bash script.sh`, so nothing broke -- which is exactly why nobody
# noticed. The cost is downstream: `./scripts/foo.sh` fails with a confusing
# "Permission denied" for anyone following a README that writes it that way, and
# a hook or wrapper that execs the path silently does not run the gate at all.
#
# The git INDEX mode is checked, not the filesystem mode, because the index is
# what a fresh clone reproduces. A local `chmod +x` that was never staged reads
# as fixed here and is not.
check_script_modes() { # $1 repository root
  local root="$1" bad="" total=0
  if ! git -C "$root" rev-parse --git-dir >/dev/null 2>&1; then
    printf 'not a git checkout; index modes unverifiable, skipping\n' >&2
    return 0
  fi
  # `git ls-files -s` prints "<mode> <sha> <stage>\t<path>". A plain
  # `read -r mode path` puts the sha and the stage into `path`, so every
  # `$root/$path` lookup failed and the loop examined nothing while still
  # reporting success. Fields are taken positionally instead.
  while read -r mode path; do
    [ -n "$path" ] || continue
    # Only files whose first line is a shebang are meant to be executed.
    head -n1 "$root/$path" 2>/dev/null | grep -q '^#!' || continue
    total=$((total + 1))
    [ "$mode" = 100755 ] || bad="$bad $path($mode)"
  done < <(git -C "$root" ls-files -s -- 'scripts/*.sh' 'tests/*.sh' | awk '{ print $1, $4 }')
  require_nonempty "shebang-bearing scripts under scripts/ and tests/" "$total" || return 1
  printf 'script modes: %d shebang-bearing script(s) in the git index, %d at 100755\n' \
    "$total" "$((total - $(printf '%s' "$bad" | wc -w)))" >&2
  if [ -n "$bad" ]; then
    printf 'script(s) with a shebang but no executable bit in the git index:%s\n' "$bad" >&2
    printf '  git update-index --chmod=+x <path>   (a local chmod that is never staged does not count)\n' >&2
    return 1
  fi
}

# ── the four files that name a security contact must name the SAME ones ──────
#
# A live contradiction, not a style preference. .well-known/security.txt said
# `mailto:jonathan.jewell@gmail.com` while SECURITY.md and CODE_OF_CONDUCT.md
# both said `developer@joshuajewell.dev`. Neither mentioned the other. A
# researcher arriving via RFC 9116 discovery and one arriving via the GitHub
# policy link were routed to different inboxes, and there was no way to tell
# from inside the repository which one was monitored.
#
# The check compares SETS, not order or count, and it prints both sides. It
# fails on an empty set: a document that stopped naming any address at all is
# not "consistent with" one that names two.
contact_addresses() { # $1 file
  [ -f "$1" ] || return 0
  # COMMENT LINES ARE STRIPPED FIRST, and that is load-bearing rather than
  # cosmetic. security.txt's own header records the address it USED to carry so
  # the change stays auditable; without this filter that provenance note reads
  # as a second live contact and the check disagrees with itself on a clean
  # tree. In RFC 9116 and humanstxt.org a `#` line is a comment. In Markdown a
  # `#` line is a heading, which never carries a mailto, so dropping it is
  # harmless there too.
  local body
  body="$(grep -v '^[[:space:]]*#' "$1" 2>/dev/null || true)"
  # Recognised shapes only: a mailto: link (Markdown and RFC 9116) or a bare
  # `Contact: <addr>` line (humanstxt.org). Nothing else, because an address in
  # running prose is too easy to hit by accident -- a changelog entry, a worked
  # example, a bot's noreply mail.
  { printf '%s\n' "$body" | grep -o 'mailto:[A-Za-z0-9._%+-]\+@[A-Za-z0-9.-]\+' || true
    printf '%s\n' "$body" | grep -o '^Contact:[[:space:]]*mailto:[^[:space:]]\+' || true
    printf '%s\n' "$body" | grep -o '^Contact:[[:space:]]*[A-Za-z0-9._%+-]\+@[A-Za-z0-9.-]\+' || true
  } | sed -e 's/^Contact:[[:space:]]*//' -e 's/^mailto://' | sort -u
}

CONTACT_FILES=".well-known/security.txt .well-known/humans.txt SECURITY.md CODE_OF_CONDUCT.md"

check_contact_consistency() { # $1 repository root
  local root="$1" f addrs="" present=0 reference=""
  for f in $CONTACT_FILES; do
    [ -f "$root/$f" ] || { printf 'contact check: %s is absent, skipped\n' "$f" >&2; continue; }
    present=$((present + 1))
    local these
    these="$(contact_addresses "$root/$f")"
    if [ -z "$these" ]; then
      printf 'contact check: %s names no contact address at all\n' "$f" >&2
      return 1
    fi
    printf 'contact check: %s -> %s\n' "$f" "$(printf '%s' "$these" | tr '\n' ' ')" >&2
    if [ -z "$reference" ]; then reference="$these"; addrs="$f"; continue; fi
    if [ "$reference" != "$these" ]; then
      printf 'CONTACT SETS DISAGREE\n' >&2
      printf '  %s names: %s\n' "$addrs" "$(printf '%s' "$reference" | tr '\n' ' ')" >&2
      printf '  %s names: %s\n' "$f" "$(printf '%s' "$these" | tr '\n' ' ')" >&2
      printf '  A researcher following RFC 9116 discovery and one following the policy\n' >&2
      printf '  link reach different inboxes. Change every file in one commit.\n' >&2
      return 1
    fi
  done
  require_nonempty "files naming a security contact" "$present" || return 1
  printf 'contact check: %d file(s) examined, all name the same %d address(es)\n' \
    "$present" "$(printf '%s' "$reference" | grep -c . || true)" >&2
}

# ── sonar-project.properties must agree with itself ─────────────────────────
#
# A live contradiction. The two effective lines said
#     sonar.organization=metadatastician
#     sonar.projectKey=metadatastician_IDApTIK
# and the prose directly above them said "Organisation is `hyperpolymath`,
# decided in PR #42 and closing issue #38", implying a projectKey of
# hyperpolymath_IDApTIK. Both cannot be true, and the prose was the more
# confident of the two, so a reader following it would "correct" working values
# and break the scan. The failure is silent in the dangerous direction: as the
# file's own old comment warned, with SONAR_TOKEN present and a wrong
# projectKey the scan reports into a DIFFERENT project rather than failing.
#
# sonarcloud.io is unreachable from the development sandbox, so which project
# actually receives data could not be queried. That question is recorded as an
# owner ruling rather than guessed at; what IS checkable offline is internal
# consistency, and that is what this asserts.
#
# Lines between the explicit BEGIN/END RETIRED-VALUES QUOTE markers are skipped.
# Those markers exist so the file can quote the retired claim verbatim for
# auditability without the quote being read as a second live configuration --
# the same shape as a security contact named only inside a comment.
check_sonar_consistency() { # $1 properties file, $2 repository root
  local props="$1" root="$2"
  [ -f "$props" ] || { printf 'no sonar-project.properties; nothing to check\n' >&2; return 0; }

  local repo
  repo="$(git -C "$root" remote get-url origin 2>/dev/null | sed -e 's#.*/##' -e 's#\.git$##')"
  if [ -z "$repo" ]; then
    repo="$(grep -o 'overview?id=[A-Za-z0-9_-]*' "$props" | head -1 | sed 's/.*_//')"
  fi
  require_nonempty "repository name for the sonar projectKey check" "$repo" || return 1

  awk -v repo="$repo" '
    function strip_value(line,   v) {
      v = line
      sub(/^[^=]*=[[:space:]]*/, "", v)
      sub(/[[:space:]]+$/, "", v)
      sub(/^"/, "", v); sub(/"$/, "", v)
      return v
    }
    # Skip the quoted retired configuration, but count it so a marker pair that
    # was opened and never closed cannot silently swallow the whole file.
    /^#[[:space:]]*BEGIN RETIRED-VALUES QUOTE/ { quoted = 1; markers++; next }
    /^#[[:space:]]*END RETIRED-VALUES QUOTE/   { quoted = 0; markers++; next }
    quoted { next }

    /^#/ {
      # Any <something>_<Repo> token in prose is an organisation claim. Collect
      # them; the END block compares the set against the effective projectKey.
      # This branch runs BEFORE the effective-line rules deliberately: a comment
      # can never set configuration, so consuming it here loses nothing, and the
      # header overview URL lives on a comment line. An earlier draft had a
      # separate `/^# Project:/` rule below to read that URL, which could never
      # fire, because this branch had already taken the line.
      line = $0
      while (match(line, "[A-Za-z0-9][A-Za-z0-9-]*_" repo)) {
        tok = substr(line, RSTART, RLENGTH)
        prose[tok] = 1
        nprose++
        line = substr(line, RSTART + RLENGTH)
      }
      if (match($0, /overview\?id=[A-Za-z0-9_-]+/)) {
        url = substr($0, RSTART, RLENGTH)
        sub(/^.*id=/, "", url)
      }
      next
    }
    /^sonar\.organization[[:space:]]*=/ { norg++; org = strip_value($0); next }
    /^sonar\.projectKey[[:space:]]*=/   { nkey++; key = strip_value($0); next }

    END {
      printf "sonar: %d organization line(s), %d projectKey line(s), %d organisation claim(s) in prose, %d quote marker(s)\n",
             norg, nkey, nprose, markers > "/dev/stderr"
      bad = 0
      if (markers % 2 != 0) {
        printf "unbalanced RETIRED-VALUES QUOTE markers (%d): an unclosed quote swallows the rest of the file\n", markers > "/dev/stderr"
        bad = 1
      }
      if (norg != 1) { printf "expected exactly one sonar.organization, found %d\n", norg > "/dev/stderr"; bad = 1 }
      if (nkey != 1) { printf "expected exactly one sonar.projectKey, found %d\n", nkey > "/dev/stderr"; bad = 1 }
      if (norg == 0 || nkey == 0) { printf "DENOMINATOR ZERO: no effective sonar configuration to compare\n"; exit 1 }

      expected = org "_" repo
      if (key != expected) {
        printf "sonar.projectKey is %s but sonar.organization=%s in repository %s requires %s\n",
               key, org, repo, expected > "/dev/stderr"
        bad = 1
      }
      if (url != "" && url != key) {
        printf "the header overview URL names %s but sonar.projectKey is %s\n", url, key > "/dev/stderr"
        bad = 1
      }
      for (t in prose) {
        if (t != key) {
          printf "PROSE CONTRADICTS CONFIGURATION: a comment names %s while sonar.projectKey is %s\n", t, key > "/dev/stderr"
          printf "  this is the exact defect that let two readings of this file coexist.\n" > "/dev/stderr"
          printf "  Change organization, projectKey and the overview URL together, or move the\n" > "/dev/stderr"
          printf "  retired value inside a BEGIN/END RETIRED-VALUES QUOTE block.\n" > "/dev/stderr"
          bad = 1
        }
      }
      exit bad
    }
  ' "$props"
}

# ── Mergify must not auto-merge a pull request with a failing check ─────────
#
# PR #148 merged to main with three checks red, seven still running, and one
# that never started, because the whole of `.mergify.yml`'s
# `auto_merge_conditions` was a single line: `author = dependabot[bot]`. The
# comment above it claimed auto-merge happened "after GitHub's protected-branch
# checks have passed". Nothing implemented that claim.
#
# The consequence was concrete. PR #148 bumped `github/codeql-action` to
# v4.38.2 in codeql.yml without touching actions.lock -- which Dependabot
# cannot do -- and the lock-sync gate added three minutes earlier by PR #147
# correctly reported `failure`. The merge proceeded regardless, and main went
# back to rendering no CodeQL verdict at all. A gate that nothing is required
# to wait for is a suggestion.
ci_job_name() { # $1 workflow, $2 job id; Y-1: parse YAML, including KYAML
  JOB_ID="$2" yq -p yaml -o=yaml -r '.jobs[strenv(JOB_ID)].name // ""' "$1"
}

check_mergify_conditions() { # $1 config, $2 lock-sync workflow, $3 optional CI fixture
  local cfg="$1" gatewf="$2"
  local ciwf="${3:-$CI}"
  if [ ! -f "$cfg" ]; then
    printf 'no .mergify.yml; nothing auto-merges, nothing to check\n' >&2
    return 0
  fi
  # The job name the lock-sync gate reports under. Read from the workflow rather
  # than hardcoded: a `check-success = <name>` condition whose name no longer
  # matches anything is silently satisfied by nothing and blocks everything, or
  # worse, is one of several conditions and the others carry the merge. Either
  # way a rename must not be able to disable the condition unnoticed.
  local gatename conditions
  gatename="$(ci_job_name "$gatewf" lock-sync)" || return 1
  conditions="$(yq -p yaml -o=json -I=0 '.merge_protections_settings.auto_merge_conditions' "$cfg" |
    jq -er 'if type == "array" and all(.[]; type == "string") then .[] else error("auto_merge_conditions must be strings") end')" || return 1
  require_nonempty "lock-sync gate job name read from $gatewf" "$gatename" || return 1
  local ghostname fixturesname
  ghostname="$(ci_job_name "$ciwf" test-ghost)"
  fixturesname="$(ci_job_name "$ciwf" fixtures-and-must)"
  require_nonempty "test-ghost job name" "$ghostname" || return 1
  require_nonempty "fixtures-and-must job name" "$fixturesname" || return 1

  awk -v gatename="$gatename" -v ghostname="$ghostname" -v fixturesname="$fixturesname" -v GATEWF="$gatewf" '
    {
      c = $0
      nconds++
      if (c ~ /^author[[:space:]]*=[[:space:]]*dependabot/) nauthor++
      if (c ~ /^head[[:space:]]*~=/) { nhead++; headre = c }
      if (c == "#check-failure = 0") nfailzero++
      if (c == "#check-neutral = 0") nneutralzero++
      if (c == "#commits-behind = 0") ncurrentbase++
      if (c ~ /^check-success[[:space:]]*=/) { nchecksuccess++; success_conds[++ns] = c }
      next
    }
    END {
      printf "mergify: %d auto_merge condition(s) -- %d author, %d head-pattern, %d check-success, %d #check-failure=0, %d #check-neutral=0\n",
             nconds, nauthor, nhead, nchecksuccess, nfailzero, nneutralzero > "/dev/stderr"
      if (nconds == 0) {
        printf "DENOMINATOR ZERO: no auto_merge_conditions found\n"
        exit 1
      }
      bad = 0
      if (ncurrentbase != 1) {
        printf "expected #commits-behind = 0: stale head checks do not validate current main\n" > "/dev/stderr"
        bad = 1
      }
      if (nauthor == 0) {
        printf "no author condition: this would auto-merge human-authored work too\n" > "/dev/stderr"
        bad = 1
      }
      # An author-only condition is the exact defect that shipped.
      if (nconds == nauthor) {
        printf "auto_merge_conditions is author-only: it merges regardless of check status\n" > "/dev/stderr"
        printf "  PR #148 merged to main with 3 checks failing and 7 in progress on this\n" > "/dev/stderr"
        printf "  exact configuration, re-breaking CodeQL three minutes after it was fixed.\n" > "/dev/stderr"
        bad = 1
      }
      if (nfailzero != 1) {
        printf "expected `\"#check-failure = 0\"`, found %d\n", nfailzero > "/dev/stderr"
        bad = 1
      }
      if (nneutralzero != 1) {
        printf "expected `\"#check-neutral = 0\"`, found %d (a neutral conclusion is not a pass)\n", nneutralzero > "/dev/stderr"
        bad = 1
      }
      if (nchecksuccess == 0) {
        printf "no check-success condition\n" > "/dev/stderr"
        printf "  `#check-failure = 0` is satisfied by a check that NEVER RAN. When GitHub\n" > "/dev/stderr"
        printf "  refuses a workflow at startup it creates zero jobs and contributes no check\n" > "/dev/stderr"
        printf "  to any count, so a startup_failure is invisible to a nothing-failed test.\n" > "/dev/stderr"
        printf "  A positive check-success is the only form that catches it.\n" > "/dev/stderr"
        bad = 1
      }
      # The lock-sync gate specifically must be named, and must match the
      # workflow'"'"'s current job name.
      found = 0
      for (i = 1; i <= ns; i++) {
        if (success_conds[i] == "check-success = " gatename) found = 1
        if (success_conds[i] == "check-success = " ghostname) found_ghost = 1
        if (success_conds[i] == "check-success = " fixturesname) found_fixtures = 1
      }
      if (!found) {
        printf "no check-success condition names the lock-sync gate job \"%s\"\n", gatename > "/dev/stderr"
        printf "  that name was read from %s just now, so this is not a stale hardcode\n", GATEWF > "/dev/stderr"
        bad = 1
      }
      if (!found_ghost || !found_fixtures) {
        printf "missing exact positive Rust/fixture check: %s; %s\n", ghostname, fixturesname > "/dev/stderr"
        bad = 1
      }
      # A github_actions bump can never satisfy the lockfile condition, so it
      # must not be admitted to auto-merge at all.
      if (nhead == 0) {
        printf "no head-pattern condition: github_actions bumps are admitted to auto-merge\n" > "/dev/stderr"
        printf "  Dependabot cannot regenerate actions.lock, so every such bump merges a\n" > "/dev/stderr"
        printf "  desynchronised lockfile and breaks CI with no log to read.\n" > "/dev/stderr"
        bad = 1
      } else if (headre ~ /github_actions/) {
        printf "the head pattern admits github_actions branches: %s\n", headre > "/dev/stderr"
        bad = 1
      } else if (headre !~ /cargo/) {
        # A bare `^dependabot/` admits every ecosystem including github_actions,
        # so requiring the pattern to NAME an allowed ecosystem is what makes the
        # default closed. Matching on the absence of a bad name is not enough.
        printf "the head pattern does not name an allowed ecosystem: %s\n", headre > "/dev/stderr"
        printf "  a bare ^dependabot/ admits github_actions bumps, which can never satisfy\n" > "/dev/stderr"
        printf "  the lockfile condition because Dependabot cannot regenerate actions.lock.\n" > "/dev/stderr"
        bad = 1
      }
      exit bad
    }
  ' <<< "$conditions"
}

# This validates the reviewed import payload, not GitHub's live settings.
# Only an administrator can apply it; never describe this local test as proof
# that manual merges are protected.
check_rust_ruleset() { # $1 ruleset JSON, $2 CI workflow, $3 lock-sync workflow
  local ghost fixtures lockname
  ghost="$(ci_job_name "$2" test-ghost)"
  fixtures="$(ci_job_name "$2" fixtures-and-must)"
  lockname="$(ci_job_name "$3" lock-sync)"
  require_nonempty "ruleset Rust job name" "$ghost" || return 1
  require_nonempty "ruleset fixture job name" "$fixtures" || return 1
  require_nonempty "ruleset lock-sync job name" "$lockname" || return 1
  jq -e --arg ghost "$ghost" --arg fixtures "$fixtures" --arg lock "$lockname" '
    .name == "Rust-Integration-Floor" and
    .target == "branch" and .enforcement == "active" and
    .bypass_actors == [] and
    .conditions.ref_name == {include: ["~DEFAULT_BRANCH"], exclude: []} and
    (.rules | length == 1) and
    .rules[0].type == "required_status_checks" and
    (.rules[0].parameters |
      .strict_required_status_checks_policy == true and
      .do_not_enforce_on_create == false and
      (.required_status_checks | sort_by(.context)) ==
      ([$ghost, $fixtures, $lock] | map({context: ., integration_id: 15368}) | sort_by(.context)))
  ' "$1" >/dev/null || {
    printf 'Rust integration ruleset must require current-base checks from GitHub Actions with no bypass\n' >&2
    return 1
  }
}

# ── every shell script here must at least parse ─────────────────────────────
#
# `bash -n` is free and catches the failure mode that no fixture can: a script
# with a syntax error still runs every line ABOVE the error and reports their
# results before aborting, so a truncated suite can print forty passing
# assertions and an exit code that only a reader of the last line would notice.
# This file shipped one -- a literal two-character `\n` inserted between the
# last fixture and the summary block by a patch script -- and it presented as
# "49 ok" followed by exit 2.
check_shell_syntax() { # $1 repository root
  local root="$1" f total=0 bad=""
  for f in "$root"/scripts/*.sh "$root"/tests/*.sh; do
    [ -e "$f" ] || continue
    total=$((total + 1))
    if ! bash -n "$f" 2>/dev/null; then
      bad="$bad ${f##*/}"
    fi
  done
  require_nonempty "shell scripts to parse" "$total" || return 1
  printf 'shell syntax: %d script(s) parsed with bash -n, %d clean\n' \
    "$total" "$((total - $(printf '%s' "$bad" | wc -w)))" >&2
  if [ -n "$bad" ]; then
    printf 'script(s) that do not parse:%s\n' "$bad" >&2
    for f in $bad; do bash -n "$root/scripts/$f" 2>&1 || bash -n "$root/tests/$f" 2>&1 || true; done >&2
    return 1
  fi
}

printf 'production configuration:\n'
expect_pass "GitHub Actions updates are capped at two open pull requests" \
  check_dependabot_limit "$DEPENDABOT"
expect_pass "every dependabot ecosystem directory exists in the tree" \
  check_dependabot_directories "$DEPENDABOT" "$REPO_DIR"
expect_pass "CodeQL init and analyze agree and are pinned by actions.lock" \
  check_codeql_pins "$CODEQL" "$LOCK"
expect_pass "a push on main cannot be superseded mid-run (issue #142)" \
  check_ci_concurrency "$CI"
expect_pass "every ci.yml job can time out" \
  check_ci_timeouts "$CI"
expect_pass "no test suite under tests/ is invoked by nothing (issue #141)" \
  check_no_dead_test_scripts "$REPO_DIR"
expect_pass "every shebang-bearing script is executable in the git index" \
  check_script_modes "$REPO_DIR"
expect_pass "every file naming a security contact names the same one" \
  check_contact_consistency "$REPO_DIR"
expect_pass "sonar-project.properties agrees with itself and with its prose" \
  check_sonar_consistency "$REPO_DIR/sonar-project.properties" "$REPO_DIR"
expect_pass "Mergify will not auto-merge over a failing or absent check" \
  check_mergify_conditions "$REPO_DIR/.mergify.yml" "$REPO_DIR/.github/workflows/lock-sync-gate.yml"
expect_pass "the Rust ruleset import matches the live workflow names" \
  check_rust_ruleset "$REPO_DIR/.github/rulesets/rust-integration.json" "$CI" "$REPO_DIR/.github/workflows/lock-sync-gate.yml"
expect_pass "every shell script under scripts/ and tests/ parses" \
  check_shell_syntax "$REPO_DIR"

printf 'Dependabot firing fixtures:\n'
awk '
  /^  - package-ecosystem:[[:space:]]*"github-actions"/ { in_actions = 1; print; next }
  in_actions && /^    open-pull-requests-limit:/ { next }
  in_actions && /^  - package-ecosystem:/ { in_actions = 0 }
  { print }
' "$DEPENDABOT" > "$FIXTURE/dependabot-missing.yml"
expect_fail "a missing GitHub Actions limit is rejected" \
  "expected exactly one github-actions open-pull-requests-limit" \
  check_dependabot_limit "$FIXTURE/dependabot-missing.yml"

awk '
  /^  - package-ecosystem:[[:space:]]*"github-actions"/ { in_actions = 1 }
  in_actions && /^    open-pull-requests-limit:/ {
    print "    open-pull-requests-limit: 3"; in_actions = 0; next
  }
  { print }
' "$DEPENDABOT" > "$FIXTURE/dependabot-too-high.yml"
expect_fail "a limit above the requested boundary is rejected" \
  "github-actions open-pull-requests-limit must be integer 2" \
  check_dependabot_limit "$FIXTURE/dependabot-too-high.yml"

awk '
  /^  - package-ecosystem:[[:space:]]*"github-actions"/ { in_actions = 1 }
  in_actions && /^    open-pull-requests-limit:/ {
    print "    open-pull-requests-limit: \"2\""; in_actions = 0; next
  }
  { print }
' "$DEPENDABOT" > "$FIXTURE/dependabot-string-limit.yml"
expect_fail "a string-valued limit is rejected" \
  "github-actions open-pull-requests-limit must be integer 2" \
  check_dependabot_limit "$FIXTURE/dependabot-string-limit.yml"

awk '
  /^  - package-ecosystem:[[:space:]]*"github-actions"/ { in_actions = 1 }
  in_actions && /^    open-pull-requests-limit:/ { held = $0; next }
  in_actions && /^  - package-ecosystem:[[:space:]]*"cargo"/ {
    in_actions = 0; print; print held; next
  }
  { print }
' "$DEPENDABOT" > "$FIXTURE/dependabot-wrong-block.yml"
expect_fail "a limit placed on another ecosystem does not satisfy the policy" \
  "expected exactly one github-actions open-pull-requests-limit" \
  check_dependabot_limit "$FIXTURE/dependabot-wrong-block.yml"

awk '
  { print }
  /^  - package-ecosystem:[[:space:]]*"github-actions"/ { print }
' "$DEPENDABOT" > "$FIXTURE/dependabot-duplicate-block.yml"
expect_fail "duplicate GitHub Actions update blocks are rejected" \
  "expected exactly one github-actions update block" \
  check_dependabot_limit "$FIXTURE/dependabot-duplicate-block.yml"

# The retired-relay defect, reproduced: point an ecosystem at a directory that
# does not exist. This is `/server` as it stood until this change.
sed 's|directory: "/"|directory: "/server-retired"|' "$DEPENDABOT" \
  > "$FIXTURE/dependabot-missing-dir.yml"
expect_fail "an ecosystem pointing at a directory that does not exist is rejected" \
  "does not exist under" \
  check_dependabot_directories "$FIXTURE/dependabot-missing-dir.yml" "$REPO_DIR"

printf 'CodeQL firing fixtures:\n'
# Movable tag with no lockfile record. NOTE: no `{n}` regex intervals anywhere in
# this file — mawk does not implement them, and a mutant that silently does not
# fire is worse than no mutant. That is exactly how the original divergent-pin
# fixture below came to pass for five days.
awk '
  /uses:[[:space:]]*github\/codeql-action\/analyze@/ {
    sub(/@[^[:space:]]+/, "@v0.0.1-movable")
  }
  { print }
' "$CODEQL" > "$FIXTURE/codeql-divergent.yml"
expect_fail "init and analyze cannot drift to different revisions" \
  "must resolve to the same action revision" \
  check_codeql_pins "$FIXTURE/codeql-divergent.yml" "$LOCK"

awk '
  /uses:[[:space:]]*github\/codeql-action\// { sub(/@[^[:space:]]+/, "@v9.99.99") }
  { print }
' "$CODEQL" > "$FIXTURE/codeql-unrecorded.yml"
expect_fail "a ref the lockfile does not record is rejected (the PR #143 defect)" \
  "is not recorded in actions.lock" \
  check_codeql_pins "$FIXTURE/codeql-unrecorded.yml" "$LOCK"

# The ref and the commit are DERIVED, never hardcoded. The first draft of this
# suite spelled `v4.38.1` and the matching sha1 literally, and within a day
# Dependabot had bumped the workflow to v4.38.2: both `sed` expressions stopped
# matching, both mutants became byte-identical to the original, and two firing
# fixtures quietly turned into passes. A mutant that no longer mutates is worse
# than no mutant, because the suite still reports a number.
CODEQL_REF="$(yq -p yaml -o=json -I=0 '.' "$CODEQL" |
  jq -r '[.jobs[].steps[]? | .uses // "" | select(startswith("github/codeql-action/init@"))][0] | split("@")[1]')"
require_nonempty "the CodeQL ref currently used in codeql.yml" "$CODEQL_REF" || exit 1
CODEQL_COMMIT="$(yq -p yaml -o=json -I=0 '.' "$LOCK" |
  jq -r --arg key "github/codeql-action@$CODEQL_REF" '.dependencies[$key].commit // ""')"
require_nonempty "the commit actions.lock resolves the CodeQL ref to" "$CODEQL_COMMIT" || exit 1
printf 'derived from the live tree: codeql ref %s, lockfile commit %s\n' \
  "$CODEQL_REF" "$CODEQL_COMMIT" >&2

# A bare SHA in the YAML: superficially "harder pinned", actually a startup
# failure, because the lockfile records the readable tag instead.
# Synthetic on purpose. Any 40-hex string the lockfile does not record makes the
# point; using a REAL commit sha would be a fixture that starts passing the day
# somebody records it, and a 40-hex constant that looks like a real pin invites
# exactly that mistake.
SHA=deadbeefdeadbeefdeadbeefdeadbeefdeadbeef
sed "s|github/codeql-action/init@${CODEQL_REF}|github/codeql-action/init@${SHA}|;
     s|github/codeql-action/analyze@${CODEQL_REF}|github/codeql-action/analyze@${SHA}|" \
  "$CODEQL" > "$FIXTURE/codeql-bare-sha.yml"
if cmp -s "$CODEQL" "$FIXTURE/codeql-bare-sha.yml"; then
  fail "the bare-SHA mutant did not change the workflow; the derived ref no longer matches"
fi
expect_fail "a bare SHA the lockfile does not record is rejected, not rewarded" \
  "is not recorded in actions.lock" \
  check_codeql_pins "$FIXTURE/codeql-bare-sha.yml" "$LOCK"

# The lockfile records the ref but resolves it to nothing.
sed "s|commit: '${CODEQL_COMMIT}'|commit: 'sha1-notahex'|" \
  "$LOCK" > "$FIXTURE/actions-nocommit.lock"
if cmp -s "$LOCK" "$FIXTURE/actions-nocommit.lock"; then
  fail "the no-commit mutant did not change the lockfile; the derived commit no longer matches"
fi
expect_fail "a recorded ref that resolves to no commit is rejected" \
  "resolves to no sha1-" \
  check_codeql_pins "$CODEQL" "$FIXTURE/actions-nocommit.lock"

# A lockfile that records nothing for this workflow at all: the emptiness guard.
sed "/'.github\/workflows\/codeql.yml':/,+1d" "$LOCK" > "$FIXTURE/actions-noentry.lock"
expect_fail "a lockfile with no codeql.yml entry fails on a zero denominator" \
  "DENOMINATOR ZERO" \
  check_codeql_pins "$CODEQL" "$FIXTURE/actions-noentry.lock"

sed '0,/github\/codeql-action\/init/s//github\/codeql-action\/upload-sarif/' \
  "$CODEQL" > "$FIXTURE/codeql-wrong-entry-point.yml"
expect_fail "an unexpected CodeQL action entry point is rejected" \
  "unexpected CodeQL action entry point" \
  check_codeql_pins "$FIXTURE/codeql-wrong-entry-point.yml" "$LOCK"

awk '/uses:[[:space:]]*github\/codeql-action\// { next } { print }' "$CODEQL" \
  > "$FIXTURE/codeql-norefs.yml"
expect_fail "a CodeQL workflow with no action refs fails on a zero denominator" \
  "DENOMINATOR ZERO" \
  check_codeql_pins "$FIXTURE/codeql-norefs.yml" "$LOCK"

printf 'ci.yml concurrency firing fixtures (issue #142):\n'
sed 's|^  group: ci-.*|  group: ci-${{ github.ref }}|' "$CI" > "$FIXTURE/ci-refgroup.yml"
expect_fail "a ref-keyed concurrency group is rejected — it cancels a merged commit's verdict" \
  "does not isolate push/dispatch runs by github.run_id" \
  check_ci_concurrency "$FIXTURE/ci-refgroup.yml"

sed 's|^  cancel-in-progress: .*|  cancel-in-progress: true|' "$CI" > "$FIXTURE/ci-cancelall.yml"
expect_fail "unconditional cancel-in-progress is rejected" \
  "cancel-in-progress is not scoped to pull_request events" \
  check_ci_concurrency "$FIXTURE/ci-cancelall.yml"

printf 'ci.yml timeout firing fixture:\n'
awk '/^    timeout-minutes:/ && !done1 { done1 = 1; next } { print }' "$CI" \
  > "$FIXTURE/ci-notimeout.yml"
expect_fail "a job with no timeout-minutes is rejected" \
  "job(s) with no timeout-minutes" \
  check_ci_timeouts "$FIXTURE/ci-notimeout.yml"

printf 'dead-gate firing fixture (issue #141):\n'
dead_root="$FIXTURE/dead-tree"
mkdir -p "$dead_root/tests" "$dead_root/.github/workflows"
: > "$dead_root/justfile"
printf '#!/usr/bin/env bash\nexit 0\n' > "$dead_root/tests/wired_test.sh"
printf '#!/usr/bin/env bash\nexit 0\n' > "$dead_root/tests/orphan_test.sh"
printf 'name: CI\non: [push]\njobs:\n  x:\n    steps:\n      - run: bash tests/wired_test.sh\n' \
  > "$dead_root/.github/workflows/ci.yml"
expect_fail "a test suite nothing invokes is rejected" \
  "invoked by nothing" \
  check_no_dead_test_scripts "$dead_root"
# Positive control for the same arm: wiring the orphan into the justfile must
# turn it green, so the check above is proven to be about wiring and not about
# the mere presence of a second file.
printf 'orphan-test:\n    bash tests/orphan_test.sh\n' >> "$dead_root/justfile"
expect_pass "the same tree passes once the orphan suite is wired into the justfile" \
  check_no_dead_test_scripts "$dead_root"
# And the denominator guard: an empty tests/ must not read as "all wired".
empty_root="$FIXTURE/empty-tree"
mkdir -p "$empty_root/tests"
expect_fail "a tests/ directory with no suites fails on a zero denominator" \
  "DENOMINATOR ZERO" \
  check_no_dead_test_scripts "$empty_root"

printf 'script-mode firing fixture:\n'
mode_root="$FIXTURE/mode-tree"
mkdir -p "$mode_root/scripts"
printf '#!/usr/bin/env bash\nexit 0\n' > "$mode_root/scripts/executable_gate.sh"
printf '#!/usr/bin/env bash\nexit 0\n' > "$mode_root/scripts/unexecutable_gate.sh"
chmod 755 "$mode_root/scripts/executable_gate.sh"
chmod 644 "$mode_root/scripts/unexecutable_gate.sh"
( cd "$mode_root" && git init -q . && git add -A scripts && \
  git -c user.email=t@t -c user.name=t commit -qm fixture >/dev/null 2>&1 )
expect_fail "a staged shebang script with no executable bit is rejected" \
  "no executable bit in the git index" \
  check_script_modes "$mode_root"
chmod 755 "$mode_root/scripts/unexecutable_gate.sh"
( cd "$mode_root" && git add -A scripts && \
  git -c user.email=t@t -c user.name=t commit -qm fixture2 >/dev/null 2>&1 )
expect_pass "the same tree passes once the bit is staged, not merely chmod-ed" \
  check_script_modes "$mode_root"

printf 'contact-consistency firing fixtures:\n'
contact_root="$FIXTURE/contact-tree"
mkdir -p "$contact_root/.well-known"
printf 'Contact: mailto:role@example.org\nContact: https://example.org/advisories\n' \
  > "$contact_root/.well-known/security.txt"
printf 'Contact: role@example.org\n' > "$contact_root/.well-known/humans.txt"
printf '# Policy\n\nemail [role@example.org](mailto:role@example.org) us.\n' \
  > "$contact_root/SECURITY.md"
printf '# Conduct\n\nContact [role@example.org](mailto:role@example.org).\n' \
  > "$contact_root/CODE_OF_CONDUCT.md"
expect_pass "four files naming one address agree" \
  check_contact_consistency "$contact_root"
# The exact defect that was live: security.txt alone names a different inbox.
printf 'Contact: mailto:personal@gmail.example\n' > "$contact_root/.well-known/security.txt"
expect_fail "one file naming a different inbox is rejected" \
  "CONTACT SETS DISAGREE" \
  check_contact_consistency "$contact_root"
# A second address added everywhere is consistent, so it must pass: the check is
# about agreement, not about how many channels a project offers.
printf 'Contact: mailto:role@example.org\nContact: mailto:security@example.org\n' \
  > "$contact_root/.well-known/security.txt"
printf 'Contact: role@example.org\nContact: security@example.org\n' \
  > "$contact_root/.well-known/humans.txt"
printf '# Policy\n\nemail [role@example.org](mailto:role@example.org) or\n[security@example.org](mailto:security@example.org).\n' \
  > "$contact_root/SECURITY.md"
printf '# Conduct\n\nContact [role@example.org](mailto:role@example.org) or\n[security@example.org](mailto:security@example.org).\n' \
  > "$contact_root/CODE_OF_CONDUCT.md"
expect_pass "the same two addresses named in all four files still agree" \
  check_contact_consistency "$contact_root"
# And a file that stopped naming anyone is not "consistent" by omission.
# The provenance-comment arm. A `#` line naming the address this file USED to
# carry must not count as a live contact. This is the real shape of
# .well-known/security.txt's header, which records the retired gmail address so
# the 2026-09-27 reconciliation stays auditable.
printf '%s\n' \
  '# This file said `mailto:old-inbox@gmail.example` until 2026-09-27.' \
  '# It now names the same address as SECURITY.md and CODE_OF_CONDUCT.md.' \
  'Contact: mailto:role@example.org' \
  'Contact: mailto:security@example.org' \
  > "$contact_root/.well-known/security.txt"
expect_pass "an address named only inside a provenance comment is not a contact" \
  check_contact_consistency "$contact_root"

printf '# Policy\n\nUse the advisory form.\n' > "$contact_root/SECURITY.md"
expect_fail "a file that names no address at all is rejected" \
  "names no contact address at all" \
  check_contact_consistency "$contact_root"


printf 'sonar consistency firing fixtures:\n'
sonar_root="$FIXTURE/sonar-tree"
mkdir -p "$sonar_root"
( cd "$sonar_root" && git init -q . && \
  git remote add origin "https://github.com/example-org/IDApTIK.git" )
cat > "$sonar_root/sonar-project.properties" <<'PROPS'
# Project: https://sonarcloud.io/project/overview?id=example-org_IDApTIK
# No organisation is claimed anywhere in this prose.
sonar.organization=example-org
sonar.projectKey=example-org_IDApTIK
PROPS
expect_pass "a self-consistent sonar configuration passes" \
  check_sonar_consistency "$sonar_root/sonar-project.properties" "$sonar_root"

# The exact live defect: prose names a different organisation than the values.
# The clean fixture above deliberately does NOT contain a second `*_IDApTIK`
# token anywhere -- the first draft of it did, in a comment describing the
# retired claim, so the fixture contradicted itself and the positive control
# failed. That is worth recording because it is the same trap the real file
# fell into: describing a wrong value in prose makes the prose a wrong value.
sed 's/^# No organisation is claimed anywhere in this prose./# Organisation is `other-org`, so the key is other-org_IDApTIK./' \
  "$sonar_root/sonar-project.properties" > "$sonar_root/contradictory.properties"
expect_fail "prose naming a different organisation than the values is rejected" \
  "PROSE CONTRADICTS CONFIGURATION" \
  check_sonar_consistency "$sonar_root/contradictory.properties" "$sonar_root"

# ...unless the retired value is inside an explicit quote block, which is how
# the real file records its own history without tripping the check.
awk '
  /^# No organisation is claimed anywhere in this prose\./ {
    print "# BEGIN RETIRED-VALUES QUOTE"
    print "# Organisation is `other-org`, so the key is other-org_IDApTIK."
    print "# END RETIRED-VALUES QUOTE"
    next
  }
  { print }
' "$sonar_root/sonar-project.properties" > "$sonar_root/quoted.properties"
expect_pass "a retired value inside a marked quote block is not a live claim" \
  check_sonar_consistency "$sonar_root/quoted.properties" "$sonar_root"

# An unclosed marker would swallow the effective configuration entirely, which
# must not read as "consistent".
sed 's/^# END RETIRED-VALUES QUOTE$//' "$sonar_root/quoted.properties" > "$sonar_root/unclosed.properties"
expect_fail "an unclosed quote marker is rejected" \
  "unbalanced RETIRED-VALUES QUOTE markers" \
  check_sonar_consistency "$sonar_root/unclosed.properties" "$sonar_root"

# projectKey that does not follow <organization>_<repository>.
sed 's/^sonar.projectKey=.*/sonar.projectKey=example-org_wrong-name/' \
  "$sonar_root/sonar-project.properties" > "$sonar_root/badkey.properties"
expect_fail "a projectKey that is not organization_repository is rejected" \
  "requires example-org_IDApTIK" \
  check_sonar_consistency "$sonar_root/badkey.properties" "$sonar_root"

# Header URL that names a different project than the effective key.
sed 's#overview?id=example-org_IDApTIK#overview?id=other-org_IDApTIK#' \
  "$sonar_root/sonar-project.properties" > "$sonar_root/badurl.properties"
expect_fail "a header URL naming another project is rejected" \
  "the header overview URL names" \
  check_sonar_consistency "$sonar_root/badurl.properties" "$sonar_root"


printf 'mergify firing fixtures:\n'
mergify_root="$FIXTURE/mergify-tree"
mkdir -p "$mergify_root/.github/workflows"
printf 'name: Lock Sync Gate\njobs:\n  lock-sync:\n    name: actions.lock is in sync with the workflow YAML\n' \
  > "$mergify_root/.github/workflows/lock-sync-gate.yml"
cat > "$mergify_root/good.yml" <<'MERGIFY'
merge_protections_settings:
  auto_merge_conditions:
    - author = dependabot[bot]
    - head ~= ^dependabot/cargo/
    - "#check-failure = 0"
    - "#check-neutral = 0"
    - "#commits-behind = 0"
    - check-success = actions.lock is in sync with the workflow YAML
    - check-success = test-ghost (core/tui/ffi + clippy + fmt)
    - check-success = fixtures-and-must (shell suites + Mustfile probes)
MERGIFY
expect_pass "a fully conditioned auto-merge policy passes" \
  check_mergify_conditions "$mergify_root/good.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"

grep -v 'test-ghost' "$mergify_root/good.yml" > "$mergify_root/no-rust.yml"
expect_fail "a lockfile success cannot stand in for the Rust gate (#171)" \
  "missing exact positive Rust/fixture check" \
  check_mergify_conditions "$mergify_root/no-rust.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"
grep -v 'fixtures-and-must' "$mergify_root/good.yml" > "$mergify_root/no-fixtures.yml"
expect_fail "auto-merge must wait for the merge-policy fixtures" \
  "missing exact positive Rust/fixture check" \
  check_mergify_conditions "$mergify_root/no-fixtures.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"
grep -v 'commits-behind' "$mergify_root/good.yml" > "$mergify_root/stale-base.yml"
expect_fail "success on an obsolete base does not permit auto-merge" \
  "expected #commits-behind = 0" \
  check_mergify_conditions "$mergify_root/stale-base.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"
sed 's/"#commits-behind = 0"/#commits-behind = 0/' "$mergify_root/good.yml" > "$mergify_root/comment-base.yml"
expect_fail "an unquoted hash is a YAML comment, not a current-base condition" \
  "auto_merge_conditions must be strings" \
  check_mergify_conditions "$mergify_root/comment-base.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"
sed 's/name: test-ghost (/name: renamed-ghost (/' "$CI" > "$mergify_root/renamed-ci.yml"
expect_fail "renaming the Rust job invalidates the old merge condition" \
  "missing exact positive Rust/fixture check" \
  check_mergify_conditions "$mergify_root/good.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml" "$mergify_root/renamed-ci.yml"
sed 's/check-success = test-ghost (/check-success = old-test-ghost (/' "$mergify_root/good.yml" > "$mergify_root/substring.yml"
expect_fail "a substring of the Rust job name cannot satisfy its condition" \
  "missing exact positive Rust/fixture check" \
  check_mergify_conditions "$mergify_root/substring.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"

# The exact configuration that shipped and merged PR #148 over three red checks.
cat > "$mergify_root/author-only.yml" <<'MERGIFY'
merge_protections_settings:
  auto_merge_conditions:
    - author = dependabot[bot]
MERGIFY
expect_fail "an author-only auto-merge condition is rejected (the PR #148 defect)" \
  "author-only" \
  check_mergify_conditions "$mergify_root/author-only.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"

# `#check-failure = 0` alone is not enough: a workflow refused at startup
# creates zero jobs and therefore contributes no failing check.
cat > "$mergify_root/no-positive.yml" <<'MERGIFY'
merge_protections_settings:
  auto_merge_conditions:
    - author = dependabot[bot]
    - head ~= ^dependabot/cargo/
    - "#check-failure = 0"
    - "#check-neutral = 0"
MERGIFY
expect_fail "a policy with no positive check-success is rejected" \
  "no check-success condition" \
  check_mergify_conditions "$mergify_root/no-positive.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"

# A check-success that names a job the workflow no longer has is satisfied by
# nothing. The expected name is read from the workflow at check time, so
# renaming the job must fail the policy rather than silently disable it.
cat > "$mergify_root/stale-name.yml" <<'MERGIFY'
merge_protections_settings:
  auto_merge_conditions:
    - author = dependabot[bot]
    - head ~= ^dependabot/cargo/
    - "#check-failure = 0"
    - "#check-neutral = 0"
    - check-success = the old gate name
MERGIFY
expect_fail "a check-success naming a job that no longer exists is rejected" \
  "no check-success condition names the lock-sync gate job" \
  check_mergify_conditions "$mergify_root/stale-name.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"

# Admitting github_actions branches to auto-merge guarantees merging a
# desynchronised lockfile, because Dependabot cannot regenerate it.
cat > "$mergify_root/actions-admitted.yml" <<'MERGIFY'
merge_protections_settings:
  auto_merge_conditions:
    - author = dependabot[bot]
    - head ~= ^dependabot/
    - "#check-failure = 0"
    - "#check-neutral = 0"
    - check-success = actions.lock is in sync with the workflow YAML
MERGIFY
expect_fail "a bare ^dependabot/ head pattern is rejected: the default must be closed" \
  "does not name an allowed ecosystem" \
  check_mergify_conditions "$mergify_root/actions-admitted.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"
cat > "$mergify_root/actions-explicit.yml" <<'MERGIFY'
merge_protections_settings:
  auto_merge_conditions:
    - author = dependabot[bot]
    - head ~= ^dependabot/(cargo|github_actions)/
    - "#check-failure = 0"
    - "#check-neutral = 0"
    - check-success = actions.lock is in sync with the workflow YAML
MERGIFY
expect_fail "a head pattern that explicitly admits github_actions is rejected" \
  "admits github_actions branches" \
  check_mergify_conditions "$mergify_root/actions-explicit.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"

# Neutral is not a pass. Dropping the #check-neutral arm must fail.
grep -v 'check-neutral' "$mergify_root/good.yml" > "$mergify_root/no-neutral.yml"
expect_fail "dropping #check-neutral = 0 is rejected" \
  "a neutral conclusion is not a pass" \
  check_mergify_conditions "$mergify_root/no-neutral.yml" "$mergify_root/.github/workflows/lock-sync-gate.yml"

printf 'Rust ruleset firing fixtures:\n'
ruleset="$REPO_DIR/.github/rulesets/rust-integration.json"
for mutation in \
  '.enforcement = "evaluate"' \
  '.rules[0].parameters.strict_required_status_checks_policy = false' \
  '.rules[0].parameters.required_status_checks = []' \
  '.rules[0].parameters.required_status_checks[0].integration_id = null' \
  '.bypass_actors = [{actor_id: 5, actor_type: "RepositoryRole", bypass_mode: "always"}]' \
  '.conditions.ref_name.exclude = ["refs/heads/main"]'; do
  jq "$mutation" "$ruleset" > "$mergify_root/bad-ruleset.json"
  expect_fail "ruleset rejects $mutation" "must require current-base checks" \
    check_rust_ruleset "$mergify_root/bad-ruleset.json" "$CI" "$REPO_DIR/.github/workflows/lock-sync-gate.yml"
done
expect_fail "a renamed Rust job invalidates the ruleset payload too" \
  "must require current-base checks" \
  check_rust_ruleset "$ruleset" "$mergify_root/renamed-ci.yml" "$REPO_DIR/.github/workflows/lock-sync-gate.yml"

printf 'shell-syntax firing fixture:\n'
syntax_root="$FIXTURE/syntax-tree"
mkdir -p "$syntax_root/scripts" "$syntax_root/tests"
printf '#!/usr/bin/env bash\nif [ 1 -eq 1 ]; then echo ok\n' > "$syntax_root/scripts/broken.sh"
printf '#!/usr/bin/env bash\nexit 0\n' > "$syntax_root/tests/fine.sh"
expect_fail "a script that does not parse is rejected" \
  "do not parse" \
  check_shell_syntax "$syntax_root"
printf '#!/usr/bin/env bash\nexit 0\n' > "$syntax_root/scripts/broken.sh"
expect_pass "the same tree parses once the script is fixed" \
  check_shell_syntax "$syntax_root"

# PR #165 extends the checker to require a dependency for every step-level
# external ref, including refs in workflows absent from the workflows: map.
# Exercise its public CLI against independent fixtures; copying the awk here
# would test a second implementation. Keep these in this already wired suite.
printf 'lock-sync dependency fixtures (PR #165):\n'

lock_sync_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
lock_sync_other_sha=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
lock_sync_ref="fixture/action@$lock_sync_sha"
lock_sync_other_ref="fixture/action@$lock_sync_other_sha"

write_lock_sync_workflow() { # $1 path, remaining args are uses: values
  local file="$1" ref
  shift
  cat > "$file" <<'YML'
name: Fixture
on: [push]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
YML
  for ref in "$@"; do
    printf '      - uses: %s\n' "$ref" >> "$file"
  done
}

write_lock_sync_dependency() { # $1 lockfile, $2 key, $3 commit SHA
  cat >> "$1" <<LOCK
    '$2':
        ref: 'v1'
        commit: 'sha1-$3'
LOCK
}

new_lock_sync_case() { # $1 case name; prints an isolated workflows directory
  local dir="$FIXTURE/lock sync/$1"
  mkdir -p "$dir"
  # An unrecorded job-level reusable still needs no dependency of its own.
  # Its onboarded empty step set also gives local-action fixtures a real
  # denominator without requiring unrelated external step refs.
  cat > "$dir/reusable.yml" <<'YML'
name: Reusable
on: [push]
jobs:
  scan:
    uses: fixture/shared/.github/workflows/scan.yml@v1
YML
  cat > "$dir/actions.lock" <<'LOCK'
version: 'v0.0.2'
workflows:
    '.github/workflows/reusable.yml': []
dependencies:
LOCK
  write_lock_sync_workflow "$dir/pinned.yml" "$lock_sync_ref"
  write_lock_sync_dependency "$dir/actions.lock" "$lock_sync_ref" "$lock_sync_sha"
  printf '%s\n' "$dir"
}

check_lock_sync_result() { # $1 exit status, $2 workflows dir, rest = output fragments
  local expected="$1" dir="$2" out status=0 want
  shift 2
  out="$(bash "$REPO_DIR/scripts/check-lock-sync.sh" "$dir" 2>&1)" || status=$?
  if [ "$status" -ne "$expected" ]; then
    printf 'expected lock-sync exit %s, got %s:\n%s\n' "$expected" "$status" "$out" >&2
    return 1
  fi
  for want in "$@"; do
    if [[ "$out" != *"$want"* ]]; then
      printf 'missing lock-sync output %s:\n%s\n' "$want" "$out" >&2
      return 1
    fi
  done
  if [ "$expected" -ne 0 ] && [[ "$out" == *'actions.lock is in sync:'* ]]; then
    printf 'failed lock-sync check also reported success:\n%s\n' "$out" >&2
    return 1
  fi
}

lock_sync_dir="$(new_lock_sync_case declared)"
expect_pass "a declared immutable step ref passes without onboarding its workflow" \
  check_lock_sync_result 0 "$lock_sync_dir" \
  'unonboarded, but all 1 step-level ref(s) are immutable' \
  '1/1 used refs declared in dependencies:' 'actions.lock is in sync:'

lock_sync_dir="$(new_lock_sync_case missing)"
sed -i '/^dependencies:/,$d' "$lock_sync_dir/actions.lock"
printf 'dependencies:\n' >> "$lock_sync_dir/actions.lock"
expect_pass "an empty dependency map rejects an unonboarded immutable step ref" \
  check_lock_sync_result 1 "$lock_sync_dir" \
  'FAIL .github/workflows/pinned.yml' \
  "uses $lock_sync_ref, but no dependencies: entry declares it" \
  '0/1 used refs declared in dependencies:' 'actions.lock is OUT OF SYNC'

# Resolving the same commit under a different key is insufficient: matching
# uses the full owner/repo@ref, not commit: or the readable ref: metadata.
lock_sync_dir="$(new_lock_sync_case alias)"
sed -i "s|'$lock_sync_ref':|'fixture/action@v1':|" "$lock_sync_dir/actions.lock"
expect_pass "a tag dependency pointing at the same commit does not declare a SHA ref" \
  check_lock_sync_result 1 "$lock_sync_dir" \
  "uses $lock_sync_ref, but no dependencies: entry declares it" \
  '0/1 used refs declared in dependencies:'

lock_sync_dir="$(new_lock_sync_case other-revision)"
write_lock_sync_workflow "$lock_sync_dir/other.yaml" "$lock_sync_other_ref"
expect_pass "another revision of a declared repository still needs its own dependency" \
  check_lock_sync_result 1 "$lock_sync_dir" \
  'FAIL .github/workflows/other.yaml' \
  "uses $lock_sync_other_ref, but no dependencies: entry declares it" \
  '1/2 used refs declared in dependencies:'
write_lock_sync_dependency "$lock_sync_dir/actions.lock" "$lock_sync_other_ref" "$lock_sync_other_sha"
expect_pass "adding the exact missing revision repairs the same workflows" \
  check_lock_sync_result 0 "$lock_sync_dir" \
  '2/2 used refs declared in dependencies:' 'actions.lock is in sync:'

lock_sync_dir="$(new_lock_sync_case multiple-missing)"
write_lock_sync_workflow "$lock_sync_dir/other.yaml" "$lock_sync_other_ref"
sed -i '/^dependencies:/,$d' "$lock_sync_dir/actions.lock"
printf 'dependencies:\n' >> "$lock_sync_dir/actions.lock"
expect_pass "missing refs in both yml and yaml files are all diagnosed" \
  check_lock_sync_result 1 "$lock_sync_dir" \
  'FAIL .github/workflows/pinned.yml' 'FAIL .github/workflows/other.yaml' \
  "uses $lock_sync_ref, but no dependencies: entry declares it" \
  "uses $lock_sync_other_ref, but no dependencies: entry declares it" \
  '0/2 used refs declared in dependencies:'

lock_sync_dir="$(new_lock_sync_case shared)"
write_lock_sync_workflow "$lock_sync_dir/pinned.yml" "$lock_sync_ref" "$lock_sync_ref"
write_lock_sync_workflow "$lock_sync_dir/other.yaml" "$lock_sync_ref"
expect_pass "one dependency covers repeated steps and the same ref in multiple workflows" \
  check_lock_sync_result 0 "$lock_sync_dir" \
  'examined 3 workflow file(s), 2 step-level ref(s), 1 job-level ref(s)' \
  '1/1 used refs declared in dependencies:'
sed -i '/^dependencies:/,$d' "$lock_sync_dir/actions.lock"
printf 'dependencies:\n' >> "$lock_sync_dir/actions.lock"
expect_pass "a shared undeclared ref is counted once across steps and workflows" \
  check_lock_sync_result 1 "$lock_sync_dir" \
  "uses $lock_sync_ref, but no dependencies: entry declares it" \
  '0/1 used refs declared in dependencies:'

lock_sync_dir="$(new_lock_sync_case subpaths)"
write_lock_sync_workflow "$lock_sync_dir/pinned.yml" \
  "fixture/action/build@$lock_sync_sha" "fixture/action/test/nested@$lock_sync_sha"
expect_pass "subdirectory actions share their normalized repository dependency" \
  check_lock_sync_result 0 "$lock_sync_dir" '1/1 used refs declared in dependencies:'
sed -i '/^dependencies:/,$d' "$lock_sync_dir/actions.lock"
printf 'dependencies:\n' >> "$lock_sync_dir/actions.lock"
expect_pass "undeclared subdirectory actions report the normalized ref" \
  check_lock_sync_result 1 "$lock_sync_dir" \
  "uses $lock_sync_ref, but no dependencies: entry declares it" \
  '0/1 used refs declared in dependencies:'
write_lock_sync_dependency "$lock_sync_dir/actions.lock" \
  "fixture/action/build@$lock_sync_sha" "$lock_sync_sha"
expect_pass "a dependency keyed by a subdirectory does not declare the normalized repository" \
  check_lock_sync_result 1 "$lock_sync_dir" \
  "uses $lock_sync_ref, but no dependencies: entry declares it" \
  '0/1 used refs declared in dependencies:'

lock_sync_dir="$(new_lock_sync_case quotes-and-comments)"
write_lock_sync_workflow "$lock_sync_dir/pinned.yml" \
  "'$lock_sync_ref' # readable version" "\"$lock_sync_ref\" # readable version" \
  './local/action@v1'
printf '      # - uses: %s\n' "$lock_sync_other_ref" >> "$lock_sync_dir/pinned.yml"
expect_pass "quoted refs and comments preserve matching while local actions are excluded" \
  check_lock_sync_result 0 "$lock_sync_dir" '1/1 used refs declared in dependencies:'
sed -i '/^dependencies:/,$d' "$lock_sync_dir/actions.lock"
printf 'dependencies:\n' >> "$lock_sync_dir/actions.lock"
expect_pass "quoting and commenting a step ref cannot hide a missing dependency" \
  check_lock_sync_result 1 "$lock_sync_dir" \
  "uses $lock_sync_ref, but no dependencies: entry declares it" \
  '0/1 used refs declared in dependencies:'

lock_sync_dir="$(new_lock_sync_case local-only)"
write_lock_sync_workflow "$lock_sync_dir/pinned.yml" './local/action@v1' './another/action'
sed -i '/^dependencies:/,$d' "$lock_sync_dir/actions.lock"
printf 'dependencies:\n' >> "$lock_sync_dir/actions.lock"
expect_pass "local actions and unrecorded job-level reusables require no step dependencies" \
  check_lock_sync_result 0 "$lock_sync_dir" \
  '0/0 used refs declared in dependencies:' 'actions.lock is in sync:'

lock_sync_dir="$(new_lock_sync_case onboarded)"
write_lock_sync_workflow "$lock_sync_dir/pinned.yml" 'fixture/action@v1' "$lock_sync_ref"
sed -i "/^dependencies:/i\\    '.github/workflows/pinned.yml':\n        - 'fixture/action@v1'\n        - '$lock_sync_ref'" \
  "$lock_sync_dir/actions.lock"
write_lock_sync_dependency "$lock_sync_dir/actions.lock" 'fixture/action@v1' "$lock_sync_sha"
expect_pass "onboarded tags and SHA refs each have a matching dependency" \
  check_lock_sync_result 0 "$lock_sync_dir" '2/2 used refs declared in dependencies:'
sed -i "/^    'fixture\/action@v1':$/,/^        commit:/d" "$lock_sync_dir/actions.lock"
expect_pass "recording an onboarded tag in workflows does not supply its dependency" \
  check_lock_sync_result 1 "$lock_sync_dir" \
  'lockfile records fixture/action@v1 but dependencies: has no entry for it' \
  'uses fixture/action@v1, but no dependencies: entry declares it' \
  '1/2 used refs declared in dependencies:'

lock_sync_dir="$(new_lock_sync_case invalid-pin)"
sed -i "s/commit: 'sha1-$lock_sync_sha'/commit: 'sha1-nope'/" "$lock_sync_dir/actions.lock"
expect_pass "a declared dependency with an invalid commit still fails pin validation" \
  check_lock_sync_result 1 "$lock_sync_dir" \
  "FAIL dependencies: $lock_sync_ref has no resolvable sha1-" \
  '1/1 used refs declared in dependencies:' 'actions.lock is OUT OF SYNC'

if [ "$failures" -ne 0 ]; then
  printf '%d CI security configuration test(s) failed (%d passed)\n' "$failures" "$checks" >&2
  annotate error "ci-security-config: $failures assertion(s) FAILED, $checks passed -- see the per-assertion annotations above"
  exit 1
fi
printf 'CI security configuration fixtures: %d assertion(s) passed, 0 failed\n' "$checks"
