#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Offline structural checks for the DEED/K9/coordination migration.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
failures=0
checks=0

check() { # $1 label; rest = test command
  local label="$1"
  shift
  checks=$((checks + 1))
  if "$@"; then
    printf '  ok: %s\n' "$label"
  else
    printf '  FAIL: %s\n' "$label" >&2
    failures=$((failures + 1))
  fi
}

check 'single project deed is present' test -r "$ROOT/IDApTIK_chora.deed"
check 'pointer-only startup deed is present' test -r "$ROOT/0-AI-MANIFEST.deed"
check 'root coordination protocol is present' test -r "$ROOT/coordination.k9"
check 'canonical contractile directory is present' test -d "$ROOT/.machine_readable/contractiles"
check 'legacy split descriptiles directory is absent' test ! -e "$ROOT/.machine_readable/descriptiles"
check 'legacy k9iser wrapper is absent' test ! -e "$ROOT/k9iser.toml"

for verb in adjust bust dust intend must trust; do
  check "$verb K9 component exists" test -r "$ROOT/.machine_readable/contractiles/$verb/$verb.k9.ncl"
  check "$verb Nickel runner exists" test -r "$ROOT/.machine_readable/contractiles/$verb/$verb.ncl"
done

legacy_files="$(find "$ROOT" -type f -name '*.a2ml' -not -path "$ROOT/.git/*" -print -quit)"
if [ -z "$legacy_files" ]; then
  checks=$((checks + 1))
  printf '  ok: no legacy A2ML files remain in the working tree\n'
else
  printf '  FAIL: legacy A2ML file remains: %s\n' "$legacy_files" >&2
  failures=$((failures + 1))
fi

for f in \
  "$ROOT/AGENTS.md" \
  "$ROOT/.well-known/ai.txt" \
  "$ROOT/justfile" \
  "$ROOT/.github/workflows/ci.yml" \
  "$ROOT/.machine_readable/contractiles/Justfile" \
  "$ROOT/launcher.sh" \
  "$ROOT/docs/PROJECT-GOVERNANCE-BINDING.adoc"; do
  if grep -qiE 'a2ml|k9iser|Mustfile\.a2ml' "$f"; then
    printf '  FAIL: active legacy-format reference remains in %s\n' "${f#$ROOT/}" >&2
    failures=$((failures + 1))
  else
    printf '  ok: no active legacy-format reference in %s\n' "${f#$ROOT/}"
  fi
  checks=$((checks + 1))
done

if grep -qiE 'Mustfile\.a2ml|k9iser[[:space:]]+(validate|generate|build)|validate-a2ml' "$ROOT/scripts/must_check.sh"; then
  printf '  FAIL: the Must runner still reads or invokes a legacy parser\n' >&2
  failures=$((failures + 1))
else
  printf '  ok: the Must runner executes K9 probe IDs through the host adapter\n'
fi
checks=$((checks + 1))

printf 'machine-readable-migration-test: %d checks, %d failure(s)\n' "$checks" "$failures"
[ "$failures" -eq 0 ]
