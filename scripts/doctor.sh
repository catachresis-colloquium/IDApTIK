#!/usr/bin/env bash
# Verify the presence and pinned version of every IDApTIK toolchain.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

failures=0

echo "IDApTIK player/runtime status:"
if ! bash scripts/runtime-doctor.sh report all; then
  failures=$((failures + 1))
fi
echo

check_contains() {
  local name="$1" expected="$2"
  shift 2

  local output
  if output="$("$@" 2>&1)" && [[ "$output" == *"$expected"* ]]; then
    printf '  \033[1;32m✓\033[0m %-10s %s\n' "$name" "$(printf '%s\n' "$output" | tail -n1)"
  else
    printf '  \033[1;31m✗\033[0m %-10s expected %s; got %s\n' \
      "$name" "$expected" "${output:-not installed}"
    failures=$((failures + 1))
  fi
}

echo "IDApTIK development toolchain status:"
echo "Rust (gameplay truth):"
check_contains rustc  "rustc 1.97.1" rustc --version
check_contains cargo  "cargo 1.97.1" cargo --version
check_contains clippy "clippy 0.1.97" cargo-clippy --version

echo "Systems / FFI / config / tasks:"
check_contains zig    "0.14.0" mise exec zig@0.14.0 -- zig version
check_contains just   "1.46.0" mise exec just@1.46.0 -- just --version
check_contains nickel "1.16.0" mise exec aqua:nickel-lang/nickel@1.16.0 -- nickel --version
check_contains idris2 "Idris 2" idris2 --version

echo "BEAM (multiplayer / session):"
check_contains erlang "28" mise exec erlang@28.3.1 -- erl \
  -eval 'io:format("~s",[erlang:system_info(otp_release)]), halt().' -noshell
check_contains elixir "Elixir 1.19.5" mise exec elixir@1.19.5-otp-28 -- elixir --version
check_contains mix    "Mix 1.19.5" mise exec elixir@1.19.5-otp-28 -- mix --version

echo "Manager:"
check_contains mise "2026." env MISE_OFFLINE=1 mise --version

if (( failures > 0 )); then
  printf '\n%d required toolchain check(s) failed.\n' "$failures" >&2
  exit 1
fi
