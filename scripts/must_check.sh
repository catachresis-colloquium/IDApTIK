#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Execute the blocking repository probes declared in
# .machine_readable/contractiles/must/must.k9.ncl.
#
# K9 clauses carry stable probe IDs, never arbitrary shell. This adapter maps
# each ID to a reviewed command below. Critical failures block; warning failures
# are reported without failing. Missing tools are failures, never silent skips.

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPONENT="$REPO_DIR/.machine_readable/contractiles/must/must.k9.ncl"
PROBES="$(mktemp)"
DETAIL="$(mktemp)"
trap 'rm -f "$PROBES" "$DETAIL"' EXIT

if [ ! -r "$COMPONENT" ]; then
  printf 'must: %s is missing\n' "$COMPONENT" >&2
  exit 2
fi

# Parse the deliberately one-line clause records. Commands are selected by the
# host-side case statement below; no contract text is evaluated as shell.
if ! awk '
  # Return the quoted value after the first key = " marker, or an empty
  # string if the marker or closing quote is missing. Escapes are not decoded.
  function field(line, key,    pos, rest, end) {
    pos = index(line, key " = \"")
    if (!pos) return ""
    rest = substr(line, pos + length(key " = \""))
    end = index(rest, "\"")
    if (!end) return ""
    return substr(rest, 1, end - 1)
  }
  /^[[:space:]]*\{ id = "/ {
    id = field($0, "id")
    severity = field($0, "severity")
    probe = field($0, "probe")
    if (id == "" || severity == "" || probe == "") {
      printf "must: malformed K9 clause at line %d\n", FNR > "/dev/stderr"
      bad = 1
      next
    }
    if (id in seen) {
      printf "must: duplicate K9 clause ID %s\n", id > "/dev/stderr"
      bad = 1
      next
    }
    seen[id] = 1
    printf "%s\t%s\t%s\n", id, severity, probe
    count++
  }
  END {
    if (count == 0) {
      print "must: no K9 clauses were parsed" > "/dev/stderr"
      bad = 1
    }
    exit bad
  }
' "$COMPONENT" > "$PROBES"; then
  exit 2
fi

# Run the repository check selected by probe ID ($1). Return zero on success,
# a nonzero check status on failure, or 2 for an unknown ID.
run_probe() {
  local id="$1"
  case "$id" in
    license-present)
      test -f "$REPO_DIR/LICENSE"
      ;;
    license-texts-present)
      test -f "$REPO_DIR/LICENSES/AGPL-3.0-or-later.txt" \
        && test -f "$REPO_DIR/LICENSES/CC-BY-SA-4.0.txt"
      ;;
    readme-present)
      test -f "$REPO_DIR/README.md"
      ;;
    ai-front-doors-present)
      test -f "$REPO_DIR/0-AI-MANIFEST.deed" \
        && test -f "$REPO_DIR/IDApTIK_chora.deed" \
        && test -f "$REPO_DIR/coordination.k9"
      ;;
    governance-docs)
      test -f "$REPO_DIR/GOVERNANCE.md" \
        && test -f "$REPO_DIR/.github/MAINTAINERS" \
        && test -f "$REPO_DIR/CODE_OF_CONDUCT.md" \
        && test -f "$REPO_DIR/.github/CODEOWNERS"
      ;;
    mise-pins-present)
      test -f "$REPO_DIR/mise.toml" \
        && grep -q 'zig' "$REPO_DIR/mise.toml" \
        && grep -q 'elixir' "$REPO_DIR/mise.toml"
      ;;
    rust-toolchain-pinned)
      grep -q 'channel = "1.97.1"' "$REPO_DIR/rust-toolchain.toml" \
        && grep -q ':rust-toolchain "1.97.1"' "$REPO_DIR/IDApTIK_chora.deed"
      ;;
    engine-license-agpl)
      grep -q 'license = "AGPL-3.0-or-later"' "$REPO_DIR/Cargo.toml"
      ;;
    crates-workspace)
      test -d "$REPO_DIR/crates/idaptik-kernel" \
        && test -d "$REPO_DIR/crates/idaptik-core" \
        && test -d "$REPO_DIR/crates/idaptik-ffi" \
        && test -d "$REPO_DIR/crates/idaptik-tui" \
        && test -d "$REPO_DIR/crates/idaptik-net" \
        && test -d "$REPO_DIR/crates/idaptik-evil-weevil" \
        && test -d "$REPO_DIR/crates/idaptik-bevy"
      ;;
    server-and-config)
      test -d "$REPO_DIR/config" \
        && test -f "$REPO_DIR/docs/adr/0006-gossamer-host-and-burble-transport.md"
      ;;
    adr-records)
      test -f "$REPO_DIR/docs/adr/0001-toolchain-and-runtime-management.md" \
        && test -f "$REPO_DIR/docs/adr/0007-versioned-ums-package-boundary.md" \
        && test -f "$REPO_DIR/docs/adr/0008-select-bevy-and-retire-fyrox.md" \
        && test -f "$REPO_DIR/docs/adr/0015-deployment-topologies-and-state-ownership.md"
      ;;
    rust-formatted)
      (cd "$REPO_DIR" && cargo fmt --all -- --check)
      ;;
    no-authoring-dependency)
      local rc=0
      grep -rqE '^[[:space:]]*(ums-[a-z-]+|slavia-[a-z-]+)[[:space:]]*=' \
        "$REPO_DIR/Cargo.toml" "$REPO_DIR"/crates/*/Cargo.toml || rc=$?
      [ "$rc" -eq 1 ]
      ;;
    cac-kernel-is-fixed-point)
      local rc=0
      grep -qE '(^|[^[:alnum:]_])(f32|f64)([^[:alnum:]_]|$)' \
        "$REPO_DIR/crates/idaptik-core/src/scenario/vsm.rs" || rc=$?
      [ "$rc" -eq 1 ]
      ;;
    package-contract-published)
      test -f "$REPO_DIR/contracts/idaptik/v1/contract.json" \
        && test -f "$REPO_DIR/contracts/idaptik/v1/package.schema.json" \
        && test -f "$REPO_DIR/contracts/idaptik/v1/fixtures/ghost-lobby-scenario.json"
      ;;
    machine-readable-dir)
      test -d "$REPO_DIR/.machine_readable/contractiles"
      ;;
    machine-readable-layout)
      bash "$REPO_DIR/tests/machine_readable_migration_test.sh"
      ;;
    contractiles-present)
      local verb
      for verb in adjust bust dust intend must trust; do
        test -r "$REPO_DIR/.machine_readable/contractiles/$verb/$verb.k9.ncl" \
          && test -r "$REPO_DIR/.machine_readable/contractiles/$verb/$verb.ncl" || return 1
      done
      ;;
    ffi-language-claims-honest)
      local zig_count idr_count language_list
      zig_count="$(find "$REPO_DIR" -name '*.zig' -not -path '*/.git/*' -not -path '*/target/*' -not -path '*/vendor/*' -type f | wc -l | tr -d ' ')"
      idr_count="$(find "$REPO_DIR" -name '*.idr' -not -path '*/.git/*' -not -path '*/target/*' -not -path '*/vendor/*' -type f | wc -l | tr -d ' ')"
      language_list="$(sed -n 's/.*:languages (\([^)]*\)).*/\1/p' "$REPO_DIR/IDApTIK_chora.deed" | head -n 1)"
      if [ "$zig_count" -gt 0 ]; then
        printf '%s\n' "$language_list" | tr ' ' '\n' | grep -qx zig || return 1
      else
        ! printf '%s\n' "$language_list" | tr ' ' '\n' | grep -qx zig || return 1
      fi
      if [ "$idr_count" -gt 0 ]; then
        printf '%s\n' "$language_list" | tr ' ' '\n' | grep -qx idris2 \
          && grep -q 'abi_model_check.sh' "$REPO_DIR/.github/workflows/ci.yml"
      else
        ! printf '%s\n' "$language_list" | tr ' ' '\n' | grep -qx idris2
      fi
      ;;
    vendor-patch-gated)
      test -x "$REPO_DIR/scripts/vendor_bevy_winit_check.sh" \
        && test -x "$REPO_DIR/tests/vendor_winit_check_test.sh" \
        && grep -q 'wayland-csd-adwaita-notitle' "$REPO_DIR/Cargo.toml" \
        && grep -q '^\[patch.crates-io\]' "$REPO_DIR/Cargo.toml"
      ;;
    deployment-contract-recorded)
      test -f "$REPO_DIR/crates/idaptik-net/src/deployment.rs" \
        && test -f "$REPO_DIR/docs/adr/0015-deployment-topologies-and-state-ownership.md"
      ;;
    deployment-mode-cross-product)
      grep -q 'all_three_topologies_allow_gsa_present_or_absent_without_merging_axes' \
        "$REPO_DIR/crates/idaptik-net/src/deployment.rs" \
        && grep -q 'GsaIntegration::Absent' "$REPO_DIR/crates/idaptik-net/src/deployment.rs" \
        && grep -q 'GsaIntegration::Present' "$REPO_DIR/crates/idaptik-net/src/deployment.rs"
      ;;
    state-class-contract-present)
      grep -q 'pub const STATE_CLASSES' "$REPO_DIR/crates/idaptik-net/src/deployment.rs" \
        && grep -q 'every_state_class_has_explicit_owner_storage_cache_consistency_partition_and_recovery' \
          "$REPO_DIR/crates/idaptik-net/src/deployment.rs"
      ;;
    snif-boundary-recorded)
      grep -qi 'SNIF' "$REPO_DIR/docs/adr/0015-deployment-topologies-and-state-ownership.md" \
        && grep -qi 'buffer' "$REPO_DIR/docs/adr/0015-deployment-topologies-and-state-ownership.md" \
        && grep -qi 'database' "$REPO_DIR/docs/adr/0015-deployment-topologies-and-state-ownership.md"
      ;;
    coordination-k9-v1-2)
      bash "$REPO_DIR/scripts/coordination_k9_check.sh"
      ;;
    legacy-layout-retired)
      ! find "$REPO_DIR" -type f -name '*.a2ml' -not -path '*/.git/*' -print -quit | grep -q . \
        && test ! -e "$REPO_DIR/k9iser.toml" \
        && test ! -d "$REPO_DIR/.machine_readable/descriptiles"
      ;;
    validation-ci-wired)
      grep -q 'machine-readable-conformance' "$REPO_DIR/.github/workflows/ci.yml" \
        && grep -q 'scripts/machine_readable_check.sh' "$REPO_DIR/.github/workflows/ci.yml"
      ;;
    *)
      printf 'must: no host-side probe is registered for K9 clause %s\n' "$id" >&2
      return 2
      ;;
  esac
}

critical_failures=0
warnings_failed=0
total=0

while IFS=$'\t' read -r id severity probe; do
  [ -n "$id" ] || continue
  total=$((total + 1))
  if run_probe "$probe" >"$DETAIL" 2>&1; then
    printf '  ok    %s\n' "$id"
  else
    case "$severity" in
      warning)
        warnings_failed=$((warnings_failed + 1))
        printf '  WARN  %s\n' "$id" >&2
        ;;
      *)
        critical_failures=$((critical_failures + 1))
        printf '  FAIL  %s\n' "$id" >&2
        if [ -s "$DETAIL" ]; then sed 's/^/        /' "$DETAIL" >&2; fi
        ;;
    esac
  fi
done < "$PROBES"

printf 'must: %d K9 clauses, %d critical failure(s), %d warning failure(s)\n' \
  "$total" "$critical_failures" "$warnings_failed"
[ "$critical_failures" -eq 0 ]
