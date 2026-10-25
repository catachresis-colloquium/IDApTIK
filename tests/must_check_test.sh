#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Offline fixtures run the real Must dispatcher in a minimal repository.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
FIXTURE="$TMP/repo"
COMPONENT="$FIXTURE/.machine_readable/contractiles/must/must.k9.ncl"
checks=0

reset_fixture() {
  rm -rf "$FIXTURE"
  mkdir -p "$FIXTURE/scripts" "$(dirname "$COMPONENT")" \
    "$FIXTURE/crates/core/src/scenario" "$FIXTURE/crates/idaptik-core/src/scenario"
  cp "$ROOT/scripts/must_check.sh" "$FIXTURE/scripts/"
  printf '[workspace.dependencies]\nserde = "1"\n' > "$FIXTURE/Cargo.toml"
  printf '[dependencies]\nserde = "1"\n' > "$FIXTURE/crates/core/Cargo.toml"
  printf 'pub struct Ledger { ticks: u64 }\n' > "$FIXTURE/crates/idaptik-core/src/scenario/vsm.rs"
  printf '[dependencies]\n' > "$FIXTURE/crates/idaptik-core/Cargo.toml"
}

clause() {
  printf '{ id = "fixture", severity = "%s", probe = "%s" }\n' \
    "${2:-critical}" "$1" > "$COMPONENT"
}

expect_status() {
  local name="$1" expected="$2" want="${3:-}" status=0
  bash "$FIXTURE/scripts/must_check.sh" > "$TMP/result" 2>&1 || status=$?
  if [ "$status" -ne "$expected" ] || { [ -n "$want" ] && ! grep -qF "$want" "$TMP/result"; }; then
    printf 'FAIL: %s (exit %s, expected %s; expected text: %s)\n' \
      "$name" "$status" "$expected" "$want" >&2
    cat "$TMP/result" >&2
    exit 1
  fi
  checks=$((checks + 1))
  printf '  ok: %s\n' "$name"
}

reset_fixture
clause no-authoring-dependency
expect_status 'accepts allowed dependencies' 0

for package in idaptik-ums ums-profiles slavia-core; do
  reset_fixture
  clause no-authoring-dependency
  printf '%s = "1"\n' "$package" >> "$FIXTURE/Cargo.toml"
  expect_status "rejects direct $package dependency" 1 'FAIL  fixture'

  reset_fixture
  clause no-authoring-dependency
  printf 'authoring = { package = "%s", version = "1", optional = true }\n' \
    "$package" >> "$FIXTURE/Cargo.toml"
  expect_status "rejects renamed $package dependency" 1 'FAIL  fixture'
done

reset_fixture
clause no-authoring-dependency
cat >> "$FIXTURE/crates/core/Cargo.toml" <<'EOF'
[target.'cfg(unix)'.dev-dependencies.authoring]
package = 'idaptik-ums'
version = '1'
EOF
expect_status 'rejects renamed target-specific dev dependency table' 1

reset_fixture
clause no-authoring-dependency
printf 'authoring.package = "idaptik-ums"\nauthoring.version = "1"\n' \
  >> "$FIXTURE/crates/core/Cargo.toml"
expect_status 'rejects renamed dependency using dotted keys' 1

reset_fixture
clause no-authoring-dependency
printf '\n[build-dependencies . "slavia-core"]\nversion = "1"\n' \
  >> "$FIXTURE/crates/core/Cargo.toml"
expect_status 'rejects prohibited dependency table name' 1

for missing in Cargo.toml crates; do
  reset_fixture
  clause no-authoring-dependency
  rm -rf "$FIXTURE/$missing"
  expect_status "rejects missing manifest input: $missing" 1
done

reset_fixture
clause no-authoring-dependency
rm "$FIXTURE/crates/core/Cargo.toml"
ln -s missing-manifest "$FIXTURE/crates/core/Cargo.toml"
expect_status 'rejects an unreadable crate manifest path' 1

reset_fixture
clause no-authoring-dependency
rm "$FIXTURE/Cargo.toml"
mkdir "$FIXTURE/Cargo.toml"
expect_status 'rejects a manifest path that is a directory' 1

reset_fixture
clause cac-kernel-is-fixed-point
expect_status 'accepts integer ledger' 0
for float in f32 f64; do
  printf 'pub struct Ledger { value: %s }\n' "$float" \
    > "$FIXTURE/crates/idaptik-core/src/scenario/vsm.rs"
  expect_status "rejects $float ledger" 1
done
rm "$FIXTURE/crates/idaptik-core/src/scenario/vsm.rs"
expect_status 'rejects missing ledger source' 1
mkdir "$FIXTURE/crates/idaptik-core/src/scenario/vsm.rs"
expect_status 'rejects a ledger path that is a directory' 1

for severity in critical warning; do
  reset_fixture
  clause unregistered-fixture-probe "$severity"
  expect_status "rejects unknown $severity probe" 1 'unregistered probe'
done

reset_fixture
clause readme-present warning
expect_status 'retains advisory behavior for a registered warning failure' 0 'WARN  fixture'

# A registered command returning 2 must not be mistaken for an unknown probe.
mkdir "$TMP/bin"
printf '#!/usr/bin/env bash\nexit 2\n' > "$TMP/bin/cargo"
chmod +x "$TMP/bin/cargo"
clause rust-formatted warning
PATH="$TMP/bin:$PATH" expect_status 'normalizes registered command errors' 0 'WARN  fixture'

printf 'must-check-test: %d checks passed\n' "$checks"
