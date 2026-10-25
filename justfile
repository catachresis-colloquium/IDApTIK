# IDApTIK task runner. Run `just` (no args) to list recipes.
#
# Toolchains are pinned in mise.toml (+ rust-toolchain.toml for Rust).
# Recipes that build application code activate as the crates/server land; the
# environment recipes (setup/doctor/versions) work today.

set shell := ["bash", "-euo", "pipefail", "-c"]

# List available recipes.
default:
    @just --list

# --- Environment ---------------------------------------------------------------

# On a machine without `just` yet, run `./install.sh` directly. Flags pass
# through, e.g. `just install --no-build` or `just install --full`.
# One-shot fresh-clone install (Debian/Ubuntu/WSL2): rustup, Rust, mise tools, Bevy libs (sudo), build, doctor.
install *ARGS:
    bash install.sh {{ARGS}}

# Install the pinned toolchains (mise) + Rust targets. Erlang/Elixir build from
# source on first run and take a while; subsequent runs are cached.
setup:
    mise trust
    mise install
    rustup target add wasm32-unknown-unknown
    bash scripts/install-git-hooks.sh
    @echo "Base toolchains and Git hooks ready. On Linux also run 'just bevy-linux-deps'; for Idris2 'just install-idris2'."

# Activate the tracked, fail-closed pre-push secret scan for an existing clone.
install-git-hooks:
    bash scripts/install-git-hooks.sh

# Scan the complete local Git history for verified or unverifiable secrets.
secret-scan:
    trufflehog git --no-update --fail --results=verified,unknown file://.

# Install Bevy's Linux system libraries (Debian/Ubuntu). Bevy needs these to
# build/run on Linux (audio, input, windowing); not needed on macOS/Windows.
bevy-linux-deps:
    sudo apt-get update
    sudo apt-get install -y pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev

# WSL2 fallback client, used by launcher.sh when WSLg Copy Mode is broken: the
# Windows Rust target plus the MinGW cross-linker (Debian/Ubuntu).
windows-client-deps:
    rustup target add x86_64-pc-windows-gnu
    sudo apt-get update
    sudo apt-get install -y gcc-mingw-w64-x86-64

# Fast bootstrap for ephemeral/web sessions: mise + prebuilt tools only
# (skips the slow Erlang/Elixir source builds). Called by the SessionStart hook.
bootstrap:
    bash scripts/bootstrap.sh

# Provision Idris2 (+ Chez Scheme) via idris2-pack. Separate because it compiles
# a Scheme backend and is not needed for day-to-day gameplay work.
install-idris2:
    bash scripts/install-idris2.sh

# Print the version of every toolchain we depend on; flags anything missing.
doctor:
    @bash scripts/doctor.sh

# Diagnose only what players need to launch; unlike `just doctor`, this does
# not require optional FFI/config development toolchains.
runtime-doctor:
    @bash scripts/runtime-doctor.sh report all

# Install pinned repository dependencies and guide host/network recovery.
repair:
    @bash scripts/runtime-doctor.sh repair all

# Deterministic fixture coverage for fatal/warning runtime diagnosis.
runtime-doctor-test:
    @bash tests/runtime_doctor_test.sh

# Launcher/multiplayer-runtime fixture coverage: host/join URL formation,
# spaces in checkout paths, environment overrides, readiness timeout, stale
# PID, relay early exit, and cleanup (issue #102).
launcher-fixture-test:
    bash tests/launcher_fixture_test.sh

# Fixture coverage for the bevy_winit vendor gate (issue #105).
vendor-winit-test:
    bash tests/vendor_winit_check_test.sh

# Offline fixture coverage for the ABI authority pins and the wire table
# (issues #139, #141). Runs the workflow's own pin block against stubbed
# curl/sha256sum, then checks the document independently. Was committed by
# PR #140 at mode 100755 and invoked by nothing for five days; this recipe and
# the `fixtures-and-must` CI job are what make it a gate rather than a file.
abi-authority-pin-test:
    bash tests/abi_authority_pin_test.sh

# Offline fixture coverage for the CI/CD security configuration: the dependabot
# pull-request cap, that no ecosystem points at a directory that does not exist,
# the CodeQL entry points and their actions.lock pin, issue #142's concurrency
# scoping, per-job timeouts, and that no suite under tests/ is invoked by
# nothing. Added by PR #143 and, like the above, wired by nothing until now.
ci-security-config-test:
    bash tests/ci_security_config_test.sh

# Prove .github/workflows/actions.lock agrees with the workflow YAML before
# GitHub proves it by refusing to start the run. Self-test first: a gate that
# cannot fail is worse than no gate.
lock-sync-check:
    ./scripts/check-lock-sync.sh --self-test
    ./scripts/check-lock-sync.sh

# Offline fixture coverage for the .well-known/ publish step in pages.yml.
# Extracts the real step text from the real workflow and runs it under dash with
# a PATH containing only mkdir, cp and cat -- the three external commands the
# pre-#147 version of that step already proved exist in the idris2-pack
# container. PR #147 widened the step and reached for basename(1) and head(1);
# it failed on main with exit code 1 and no retrievable log, taking the deployed
# website with it. This recipe is why the next revision cannot do that quietly.
pages-wellknown-test:
    bash tests/pages_wellknown_test.sh

# Run every offline shell fixture suite (no Rust toolchain needed).
fixture-tests: runtime-doctor-test launcher-fixture-test vendor-winit-test abi-authority-pin-test ci-security-config-test pages-wellknown-test lock-sync-check coordination-k9-test machine-readable-migration-test

# Validate DEED, K9 components, Nickel runners, and the coordination protocol
# against the pinned current standards; the K9 semantic lane requires Nickel.
machine-readable-check:
    bash scripts/machine_readable_check.sh

# Offline structural regression suites for the coordination and migration layout.
coordination-k9-test:
    bash tests/coordination_k9_check_test.sh

machine-readable-migration-test:
    bash tests/machine_readable_migration_test.sh

# Execute the K9 Must component's host-side physical-state probes.
# Critical probes gate; a required tool missing fails, never skips.
must-check:
    bash scripts/must_check.sh

# Gate the Idris2 ABI model: escape scan, total typecheck, runtime smoke,
# header + snapshot-format parity (issue #103).
abi-model-check:
    bash scripts/abi_model_check.sh

# Show the pinned versions at a glance.
versions:
    @cat mise.toml rust-toolchain.toml

# --- Rust (gameplay truth) -----------------------------------------------------
# Activate once crates/ exists (see ADR-0003). Left as the intended commands.

build:
    cargo build --workspace

run-bevy:
    cargo run -p idaptik-bevy

# Run the Ghost Lobby terminal frontend (ratatui/crossterm, ADR-0004).
run-tui:
    cargo run -p idaptik-tui

# Run a headless script and print the event-log + debrief + snapshot JSON.
headless SCRIPT:
    cargo run -q -p idaptik-tui -- --headless --script {{SCRIPT}}

# Verify a script replays deterministically (PASS/FAIL + exit code).
replay FILE:
    cargo run -q -p idaptik-tui -- --replay {{FILE}}

test:
    cargo test --workspace

# The Ghost Lobby scenario gate: core + tui + ffi + net (does not build Bevy).
test-ghost:
    cargo test -p idaptik-core -p idaptik-tui -p idaptik-ffi -p idaptik-net
    cargo clippy -p idaptik-core -p idaptik-tui -p idaptik-net --all-targets -- -D warnings
    cargo fmt --all -- --check

# The ADR-0006 §4 loopback gate: throwaway burble server + seat processes; asserts
# byte-identical artifacts (vs each other AND the headless reference) for the
# batch seats AND the live delay-lockstep seats (pause window included), a
# clean PeerLost end when a batch seat is killed, and a full snapshot resync
# when a live seat dies and rejoins. Needs Elixir and burble repo (hard requirement —
# the gate fails, never skips, when mix is missing or burble is not present).
loopback-check SCRIPT="fixtures/session_relay/capture_script.json" LIVE="fixtures/session_relay/live_script.json":
    bash scripts/loopback_check.sh {{SCRIPT}} {{LIVE}}

# Regenerate the C header for the FFI crate (needs the cbindgen CLI:
# `cargo install cbindgen`).
ffi-header:
    cd crates/idaptik-ffi && cbindgen --output include/idaptik.h

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Audit the complete lock graph against deny.toml. A missing cargo-deny binary
# is a hard failure: install the pinned version used by CI (0.20.2).
supply-chain:
    cargo deny check --hide-inclusion-graph

# --- Vendored bevy_winit patch (issue #105) -------------------------------------

# Gate the temporary bevy_winit feature patch: the vendored copy must match
# the published crate except for the documented Wayland CSD feature split, and
# the resolved graph must keep the font-parser path out with the title-free
# CSD feature active. Run this on every Bevy upgrade (vendor/README.md).
vendor-winit-check:
    bash scripts/vendor_bevy_winit_check.sh parity
    bash scripts/vendor_bevy_winit_check.sh graph

# Watch published bevy_winit releases for the patch's retirement condition:
# independently selectable Wayland CSD features. Exits non-zero (with removal
# steps) once upstream makes the vendored patch unnecessary — then remove
# vendor/bevy_winit and the [patch.crates-io] section (issue #105).
vendor-winit-watch:
    bash scripts/vendor_bevy_winit_check.sh watch

# --- Config (Nickel) -----------------------------------------------------------

# Check the typed Nickel configuration. A missing nickel binary is a hard
# failure, not a skip: run `just setup` first.
config-check:
    @command -v nickel >/dev/null 2>&1 || { echo "nickel is not installed: run 'just setup'" >&2; exit 1; }
    nickel export config/default.ncl >/dev/null && nickel export config/grounded_slice.ncl >/dev/null && nickel export config/ghost_lobby_floor.ncl >/dev/null && just config-scenario-check && echo "config: ok (schema applied)"

# Check the Nickel-authored scenario against scenario-schema.ncl: the good
# fixture must export and round-trip through the Rust validator unchanged, and
# every bad fixture must be rejected by the contract (mirroring the Rust
# ValidationError variants).
config-scenario-check:
    nickel export config/ghost_lobby_scenario.ncl --format json > /tmp/ghost_lobby_scenario_raw.json
    cargo test -q -p idaptik-core nickel_scenario_round_trips_and_validates -- --ignored
    @for f in config/scenario-fixtures/bad_*.ncl; do \
        if nickel export "$f" >/dev/null 2>&1; then \
            echo "FAIL: $f exported but the contract should reject it"; exit 1; \
        fi; \
        echo "rejected as expected: $f"; \
    done
    @echo "scenario config: ok"

# Re-export the Nickel graphs and rewrite the committed goldens they embed. The
# exports are read at compile time, so each regeneration is its own cargo run:
# the derived floor graph is rebuilt last, from the freshly embedded backbone.
config-regen:
    nickel export config/grounded_slice.ncl --format json > /tmp/grounded_slice_raw.json
    nickel export config/ghost_lobby_floor.ncl --format json > /tmp/ghost_lobby_floor_raw.json
    cargo test -p idaptik-core regenerate_slice_json -- --ignored
    cargo test -p idaptik-core regenerate_ghost_lobby_floor_json -- --ignored
    cargo test -p idaptik-core regenerate_floor_graph_json -- --ignored
    @echo "config: goldens regenerated"
