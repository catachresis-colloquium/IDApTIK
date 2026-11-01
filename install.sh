#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# One-shot user install for a fresh clone on Debian/Ubuntu (including WSL2).
# Idempotent: every step is skipped when already satisfied, so re-running is
# safe. It ends by running the solo-play runtime doctor, and exits non-zero
# unless that reports READY TO LAUNCH — an install that cannot launch the game
# is a failed install, not a skipped check.
#
# Steps, in order:
#   1. rustup             (user-space, ~/.cargo; downloaded, then run)
#   2. pinned Rust        (rust-toolchain.toml: 1.97.1 + components + targets)
#   3. mise + zig/just/nickel   (scripts/bootstrap.sh)
#   4. Bevy system libs   (`just bevy-linux-deps`; the only step needing sudo)
#   4b. WSL2 only, when WSLg Copy Mode is broken: Windows Rust target + MinGW
#      (`just windows-client-deps`; sudo), because the launcher then runs the
#      native Windows client instead of the Linux one
#   5. build idaptik-bevy (the client the launcher will pick; job count derived
#      from RAM so small machines finish)
#   6. runtime doctor     (`scripts/runtime-doctor.sh report solo`)
#
# Usage: ./install.sh [--no-build] [--full] [-h|--help]
#   --no-build  stop after the system libraries; skip the Bevy build
#   --full      also run `just setup`: every pinned toolchain incl. Erlang/Elixir
#               (long source build; only needed to host multiplayer) and the
#               tracked pre-push secret-scan hook
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

BUILD=1
FULL=0
for arg in "$@"; do
  case "$arg" in
    --no-build) BUILD=0 ;;
    --full) FULL=1 ;;
    -h|--help) sed -n '4,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//' ; exit 0 ;;
    *) printf 'install.sh: unknown option: %s (try --help)\n' "$arg" >&2; exit 2 ;;
  esac
done

log() { printf '\033[1;34m[install]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[install] ERROR:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Linux" ] || die "this installer targets Linux/WSL2; on other systems see README.md"

export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
# `mise exec` (used by scripts/runtime-doctor.sh) would otherwise auto-install
# every pinned tool, including the Erlang/Elixir source build reserved for --full.
export MISE_EXEC_AUTO_INSTALL=0

# 1. rustup -------------------------------------------------------------------
if command -v rustup >/dev/null 2>&1; then
  log "rustup present: $(rustup --version 2>/dev/null | head -1)"
else
  command -v curl >/dev/null 2>&1 || die "curl is required (sudo apt-get install -y curl ca-certificates)"
  log "installing rustup into ~/.cargo (no root needed)…"
  installer="$(mktemp)"
  trap 'rm -f "$installer"' EXIT
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$installer"
  # The pinned toolchain comes from rust-toolchain.toml in the next step.
  sh "$installer" -y --default-toolchain none --profile minimal
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

# 2. Pinned Rust --------------------------------------------------------------
# With no argument, `rustup toolchain install` reads rust-toolchain.toml, so the
# channel, components and targets stay defined in exactly one place.
log "ensuring the pinned Rust toolchain (rust-toolchain.toml)…"
rustup toolchain install
cargo --version >/dev/null || die "pinned cargo does not resolve"

# 3. mise + prebuilt tools ----------------------------------------------------
log "bootstrapping mise, zig, just and nickel…"
bash scripts/bootstrap.sh
command -v mise >/dev/null 2>&1 || die "mise is still not on PATH after bootstrap"
# `mise which` only resolves the pinned binary. Do not use `mise exec` here: it
# auto-installs every missing tool in mise.toml, i.e. the Erlang/Elixir source
# build that is deliberately reserved for --full.
just_bin="$(mise which just)" || die "pinned just is not installed"
run_just() { "$just_bin" "$@"; }

# 4. Bevy system libraries ----------------------------------------------------
# Same .pc names the Bevy build scripts probe; the package list itself lives
# only in the justfile (`bevy-linux-deps`).
have_bevy_libs() {
  command -v pkg-config >/dev/null 2>&1 &&
    pkg-config --exists alsa libudev wayland-client xkbcommon
}
if have_bevy_libs; then
  log "Bevy system libraries already present"
else
  command -v apt-get >/dev/null 2>&1 ||
    die "Bevy needs ALSA, udev, Wayland and xkbcommon dev libraries; install them with your package manager (see the bevy-linux-deps recipe in the justfile)"
  log "installing Bevy system libraries — sudo will ask for your password…"
  run_just bevy-linux-deps
  have_bevy_libs || die "Bevy system libraries are still missing after bevy-linux-deps"
fi

# 4b. Windows-native client (WSL2 only) ----------------------------------------
# Uses the same interop discovery as launcher.sh and the runtime doctor
# (scripts/windows-interop.sh), so the installer provisions exactly the client
# the launcher will pick. `command -v cmd.exe` is deliberately not used: WSL
# does not put C:\Windows\System32 on PATH, and machines with working interop
# were being told they had none.
# shellcheck source=scripts/windows-interop.sh
. scripts/windows-interop.sh
client=linux
if hp_is_wsl && hp_wslg_copy_mode_broken && hp_windows_interop; then
  client=windows
  export IDAPTIK_CLIENT_PLATFORM=windows
  log "WSLg Copy Mode is broken; the launcher will use the native Windows client"
  if rustup target list --installed | grep -qx 'x86_64-pc-windows-gnu' &&
    command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    log "Windows client prerequisites already present"
  else
    log "installing the Windows Rust target and MinGW linker — sudo will ask for your password…"
    run_just windows-client-deps
  fi
fi

# Optional: everything else `just setup` provisions.
if [ "$FULL" -eq 1 ]; then
  log "running 'just setup' (Erlang/Elixir build from source and take a long time)…"
  run_just setup
fi

# 5. Build the game -----------------------------------------------------------
if [ "$BUILD" -eq 1 ]; then
  if [ -z "${CARGO_BUILD_JOBS:-}" ]; then
    # Budget ~1.5 GiB of RAM per rustc job so a small WSL VM is not OOM-killed
    # while linking Bevy. Bounded to [1, nproc].
    mem_kib="$(awk '/^MemTotal:/ {print $2}' /proc/meminfo)"
    jobs=$(( mem_kib / 1572864 ))
    cpus="$(nproc)"
    [ "$jobs" -gt "$cpus" ] && jobs="$cpus"
    [ "$jobs" -lt 1 ] && jobs=1
    export CARGO_BUILD_JOBS="$jobs"
  fi
  log "building idaptik-bevy ($client client) with ${CARGO_BUILD_JOBS} parallel job(s) (first build is slow)…"
  if [ "$client" = "windows" ]; then
    cargo build -p idaptik-bevy --target x86_64-pc-windows-gnu
  else
    cargo build -p idaptik-bevy
  fi
fi

# 6. Verify -------------------------------------------------------------------
log "running the solo-play runtime doctor…"
if bash scripts/runtime-doctor.sh report solo; then
  log "done. Start the game with ./launcher.sh"
else
  printf '\n\033[1;31m[install] Installed, but the machine is not ready to launch.\033[0m\n' >&2
  printf 'Fix what the doctor reported above (on WSL: run `wsl --update` then `wsl --shutdown`\n' >&2
  printf 'from Windows PowerShell as Administrator, reopen your distribution) and re-run ./install.sh.\n' >&2
  exit 1
fi

case ":$PATH:" in *":$HOME/.local/share/mise/shims:"*) ;; *)
  log "tip: to use 'just' directly in this repo, add to ~/.bashrc:  eval \"\$(mise activate bash)\""
esac
