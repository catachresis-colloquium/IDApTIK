#!/usr/bin/env bash
#
# Evil Weevil kernel build helper.
#
# The kernel is a separate checkout (catachresis-colloquium/evil-weevil). These
# subcommands locate it, build its native library and run its own gates. The
# justfile recipes are thin wrappers around this so the resolution logic lives
# in one place that can be read and tested on its own.
#
# Usage:
#   scripts/weevil_kernel.sh locate   print the resolved checkout path
#   scripts/weevil_kernel.sh build    build libevil_weevil.a
#   scripts/weevil_kernel.sh test     run the kernel's Zig test suite
#   scripts/weevil_kernel.sh host     compile and run the C null host
#   scripts/weevil_kernel.sh check    build, test and host, in order
#   scripts/weevil_kernel.sh doctor   report every prerequisite, fail if any is missing
#
# Every subcommand FAILS when a prerequisite is missing. Nothing here skips:
# a guard brain that quietly fell back to the classic FSM would make the
# comparison it exists for meaningless, so a missing kernel is an error.

set -euo pipefail

# The kernel needs Zig >= 0.16. See `weevil_zig` for why 0.14 is not enough.
REQUIRED_ZIG_MAJOR=0
REQUIRED_ZIG_MINOR=16

die() {
    echo "weevil-kernel: $*" >&2
    exit 1
}

# ── Locate the checkout ──────────────────────────────────────────────────────
#
# Order: $EVIL_WEEVIL_ROOT, then the sibling and home locations the two
# repositories are normally cloned into. Verified by the two files that make a
# checkout usable, not by the directory name — a directory called evil-weevil
# that lacks the header is not the checkout.

weevil_root() {
    local candidates=()
    if [ -n "${EVIL_WEEVIL_ROOT:-}" ]; then
        candidates+=("$EVIL_WEEVIL_ROOT")
    fi
    candidates+=(
        "$PWD/../evil-weevil"
        "$PWD/../../evil-weevil"
        "${HOME}/evil-weevil"
    )
    local c
    for c in "${candidates[@]}"; do
        if [ -f "$c/include/evil_weevil/ee.h" ] \
            && [ -f "$c/src/interface/ffi/build.zig" ]; then
            (cd "$c" && pwd)
            return 0
        fi
    done
    return 1
}

require_root() {
    local root
    if ! root="$(weevil_root)"; then
        die "no evil-weevil checkout found. Looked for include/evil_weevil/ee.h under:"
        die "  \$EVIL_WEEVIL_ROOT, $PWD/../evil-weevil, $PWD/../../evil-weevil, $HOME/evil-weevil"
        die "Clone it with: git clone https://github.com/catachresis-colloquium/evil-weevil.git"
    fi
    printf '%s\n' "$root"
}

# ── Locate a Zig that can build the kernel ───────────────────────────────────
#
# Zig 0.14.0 is NOT enough, and this is measured rather than assumed. 0.14.0
# compiles libevil_weevil.a, but the archive carries an undefined
# `__zig_probe_stack` reference, so no C host — and therefore no Rust host
# either — can link it; and its test binaries fail to spawn, so the kernel's own
# suite cannot run. Zig 0.16.0, which evil-weevil's CI pins, has neither
# problem: the archive has zero `__zig_probe_stack` references and links clean.
#
# IDApTIK's own mise.toml pins Zig 0.14.0 for IDApTIK's unrelated Zig adapter
# (crates/idaptik-ffi/zig-adapter/), so `mise install` puts a 0.14.0 first on
# PATH. The kernel build must therefore not reach for whatever `zig` happens to
# be there: it takes $EVIL_WEEVIL_ZIG, then PATH, and refuses a stale version
# with the exact command that fixes it.

weevil_zig() {
    local z="${EVIL_WEEVIL_ZIG:-}"
    if [ -z "$z" ]; then
        z="$(command -v zig 2>/dev/null || true)"
    fi
    if [ -z "$z" ] || [ ! -x "$z" ]; then
        echo "weevil-kernel: no Zig found (set EVIL_WEEVIL_ZIG)" >&2
        echo "weevil-kernel: python3 -m pip install --target /opt/zig16 ziglang==0.16.0" >&2
        return 1
    fi

    local v major minor ok=0
    v="$("$z" version 2>/dev/null | head -1)" || {
        echo "weevil-kernel: '$z version' failed" >&2
        return 1
    }
    major="${v%%.*}"
    minor="${v#*.}"
    minor="${minor%%.*}"
    case "$major" in
        '' | *[!0-9]*)
            echo "weevil-kernel: cannot parse Zig version '$v' from '$z'" >&2
            return 1
            ;;
    esac
    case "$minor" in
        '' | *[!0-9]*) minor=0 ;;
    esac
    if [ "$major" -gt "$REQUIRED_ZIG_MAJOR" ]; then
        ok=1
    elif [ "$major" -eq "$REQUIRED_ZIG_MAJOR" ] && [ "$minor" -ge "$REQUIRED_ZIG_MINOR" ]; then
        ok=1
    fi
    if [ "$ok" -ne 1 ]; then
        echo "weevil-kernel: $z is Zig $v, but the kernel needs >= $REQUIRED_ZIG_MAJOR.$REQUIRED_ZIG_MINOR" >&2
        echo "weevil-kernel: 0.14.0 builds the library yet leaves __zig_probe_stack undefined," >&2
        echo "weevil-kernel: so no host can link it, and its test binaries fail to spawn." >&2
        echo "weevil-kernel: Pin the kernel build, e.g." >&2
        echo "weevil-kernel:   EVIL_WEEVIL_ZIG=/opt/zig16/ziglang/zig just weevil-kernel-check" >&2
        echo "weevil-kernel: or install one:" >&2
        echo "weevil-kernel:   python3 -m pip install --target /opt/zig16 ziglang==0.16.0" >&2
        return 1
    fi
    printf '%s\n' "$z"
}

# ── Subcommands ─────────────────────────────────────────────────────────────

cmd_locate() {
    require_root
}

cmd_build() {
    local root zig
    root="$(require_root)"
    zig="$(weevil_zig)" || exit 1
    echo "==> building libevil_weevil.a ($("$zig" version)) in $root"
    (
        cd "$root"
        "$zig" build --build-file src/interface/ffi/build.zig
    )
    ls -la "$root/src/interface/ffi/zig-out/lib/libevil_weevil.a"
}

cmd_test() {
    local root zig
    root="$(require_root)"
    zig="$(weevil_zig)" || exit 1
    echo "==> kernel test suite ($("$zig" version))"
    (
        cd "$root"
        "$zig" build test --build-file src/interface/ffi/build.zig --summary all
    )
}

cmd_host() {
    local root
    root="$(require_root)"
    command -v gcc >/dev/null 2>&1 || die "gcc is required for the C null host"
    echo "==> C null host in $root"
    (
        cd "$root"
        mkdir -p _build
        gcc -std=c11 -Wall -Wextra -Werror -Iinclude \
            tests/host/null_host.c src/interface/ffi/zig-out/lib/libevil_weevil.a \
            -o _build/null-host
        ./_build/null-host
    )
}

cmd_check() {
    cmd_build
    cmd_test
    cmd_host
    echo ""
    echo "weevil-kernel: kernel build, test suite and C null host all pass"
}

# Report the state of every prerequisite the kernel brain needs. Unlike the
# build subcommands this does not stop at the first failure: it prints all four
# lines and exits non-zero if any of them is missing, so a fresh machine gets the
# whole picture in one run.
cmd_doctor() {
    local ok=1 root zig
    if root="$(weevil_root)"; then
        echo "evil-weevil : $root"
    else
        echo "evil-weevil : NOT FOUND (set EVIL_WEEVIL_ROOT, or clone it beside IDApTIK)"
        ok=0
    fi
    if zig="$(weevil_zig 2>/dev/null)"; then
        echo "zig         : $zig ($("$zig" version))"
    elif command -v zig >/dev/null 2>&1; then
        echo "zig         : $(command -v zig) is too old (need >= $REQUIRED_ZIG_MAJOR.$REQUIRED_ZIG_MINOR)"
        echo "              0.14.0 builds the library but cannot be linked or tested"
        echo "              set EVIL_WEEVIL_ZIG to a 0.16.0 build"
        ok=0
    else
        echo "zig         : NOT FOUND (need >= $REQUIRED_ZIG_MAJOR.$REQUIRED_ZIG_MINOR)"
        echo "              set EVIL_WEEVIL_ZIG, or: python3 -m pip install --target /opt/zig16 ziglang==0.16.0"
        ok=0
    fi
    if command -v cargo >/dev/null 2>&1; then
        echo "cargo       : $(cargo --version)"
    else
        echo "cargo       : NOT FOUND (rust-toolchain.toml pins the channel)"
        ok=0
    fi
    if [ -f crates/idaptik-weevil/src/abi.rs ] && [ -f tools/verify_abi_mirror.py ]; then
        echo "adapter     : present"
    else
        echo "adapter     : MISSING"
        ok=0
    fi
    [ "$ok" -eq 1 ] || exit 1
}

case "${1:-}" in
    locate) cmd_locate ;;
    build) cmd_build ;;
    test) cmd_test ;;
    host) cmd_host ;;
    check) cmd_check ;;
    doctor) cmd_doctor ;;
    '' | -h | --help | help)
        sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        ;;
    *)
        die "unknown subcommand '$1' (expected: locate build test host check)"
        ;;
esac
