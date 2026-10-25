# IDApTIK gate verification runbook (offline, copy-paste)

- Last revised: 2026-10-25, for branch `arena/1287ad93-idaptik`
- Purpose: let one operator, on one machine, reproduce **every required
  CI gate** from a clean checkout, record each verdict, and report back.
- House doctrine (AGENTS.md, ADR-0013): a gate that cannot find its
  toolchain **fails, never skips**. If a command prints "tool missing",
  that is a FAIL result for the gate, not a reason to tick it.

Every block below is copy-pasteable. Nothing here opens a socket to a
non-loopback address; the only network use is fetching pinned toolchains
and the two pinned external repositories (burble, standards).

## 1. Pinned toolchains (exact versions)

| Tool | Pinned version | Authority file | Install |
| --- | --- | --- | --- |
| Rust (cargo/rustc, rustfmt, clippy, rust-src, rust-analyzer; target `wasm32-unknown-unknown`) | **1.97.1** | `rust-toolchain.toml` | rustup (below); the file auto-activates |
| just | **1.46.0** | `mise.toml`, CI `cargo install` | `cargo install just --version 1.46.0 --locked` |
| Nickel (config gate) | **1.16.0** | `mise.toml` | `cargo install nickel-lang-cli --version 1.16.0 --locked` |
| Erlang/OTP | **28.3.1** | `mise.toml` | mise, or asdf/kerl; CI uses `erlef/setup-beam` otp 28.3.1 |
| Elixir | **1.19.5 (OTP 28)** | `mise.toml` (`1.19.5-otp-28`) | alongside OTP; CI uses elixir 1.19.5 |
| Zig (dev pin) | **0.14.0** | `mise.toml` | mise, or ziglang.org tarball |
| cargo-deny | **0.20.2** | `ci.yml` supply-chain job | `cargo install cargo-deny --version 0.20.2 --locked` |
| trufflehog | **3.96.0** | `mise.toml` | mise (`aqua:trufflesecurity/trufflehog`) |
| Node.js | any maintained LTS (no repo pin) | required by `scripts/machine_readable_check.sh` | system package / nvm |
| Idris 2 | **v0.8.0** on Chez Scheme **v10.4.1** | `scripts/install-idris2.sh`, `ci.yml` abi-model | `just install-idris2` |
| Creusot | **v0.13.0**, ref `318615be3b8bbc60d1f6d52469ba5c0bdebed4f1` | `creusot-proof.yml` | CI-practicable only (opam/why3); local ledger half needs nothing |
| burble (sibling repo) | **`ef6756cf1591406d434c5c04c3c018e935836e4c`** | `ci.yml` session-loopback `BURBLE_REF` | git, see §3 |
| standards repo | **`8e48b1caade08a552646adeab66558ac43b4360d`** | `scripts/machine_readable_check.sh` | auto-fetched, or pre-clone via `IDAPTIK_STANDARDS_DIR` |

Two version subtleties, recorded honestly:

1. **Two Nickel versions coexist on purpose.** The config gate uses the
   mise-pinned Nickel 1.16.0 on your `PATH`. The machine-readable gate
   ignores `PATH` and downloads its own Nickel **1.18.0**, verifying
   sha256 `9cba4dd65ae9915ec61f73033aafcff307a377665a83fd8f530df086763318cb`
   into a temp directory. Do not "reconcile" them.
2. **Two Zig versions coexist today.** `mise.toml` pins 0.14.0 for the
   development environment; the `abi-conformance` workflow provisions
   **0.16.0** (`ziglang.org/download/0.16.0/...`) and that is the version
   the required conformance gate actually runs. To reproduce CI exactly,
   use 0.16.0 for G11. This drift is noted for a future toolchain-parity
   decision; do not edit either pin as part of verification.

## 2. Machine preparation

Debian/Ubuntu Linux is the reference (CI is `ubuntu-latest`). macOS works
for every Rust/Nickel gate; the two BEAM gates assume a POSIX shell and
loopback.

```bash
# Bevy's native build requirements (G7); harmless elsewhere
sudo apt-get update
sudo apt-get install -y pkg-config libasound2-dev libudev-dev \
     libwayland-dev libxkbcommon-dev git curl jq nodejs

# Rust 1.97.1 via rustup (rust-toolchain.toml then auto-selects it and
# installs rustfmt/clippy/rust-src/rust-analyzer + the wasm32 target)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"

# Pinned Cargo-side CLIs
cargo install just --version 1.46.0 --locked
cargo install nickel-lang-cli --version 1.16.0 --locked
cargo install cargo-deny --version 0.20.2 --locked

# OTP 28.3.1 + Elixir 1.19.5 and Zig 0.14.0, if you use mise:
#   mise install   (reads mise.toml in the repo root)
# Otherwise install Erlang/Elixir through asdf/kerl/Homebrew at the
# versions in the table above.
```

Sanity check the environment before running anything:

```bash
just doctor          # flags anything missing
just versions        # cats mise.toml + rust-toolchain.toml for eyeball check
rustc --version      # must say 1.97.1
```

## 3. Checkouts

```bash
# The system under test
git clone https://github.com/catachresis-colloquium/IDApTIK.git
cd IDApTIK
git checkout arena/1287ad93-idaptik      # or the PR branch under test

# Burble must be a SIBLING directory named ../burble — both relay gates
# resolve ${IDAPTIK_BURBLE_DIR:-../burble} and hard-fail if it is absent.
git clone https://github.com/metadatastician/burble.git ../burble
git -C ../burble checkout --detach ef6756cf1591406d434c5c04c3c018e935836e4c
```

Burble's test endpoint is hard-bound to `127.0.0.1:4002`
(`server/config/test.exs` ignores `PORT`). Nothing here is reachable off
the machine; do not run G13's two gates concurrently with anything else
bound to 4002 (including a second copy of the same gate).

## 4. The gates, in recommended order

Run them cheapest first. Each section gives the exact command, what a
PASS looks like, and what the gate actually proves. Record verdicts in
the ledger in §6.

### G1 — Formatting (part of `test-ghost`, also its own CI job)

```bash
cargo fmt --all -- --check
```

PASS: no output, exit 0. Any diff is a FAIL; fix with `cargo fmt --all`.

### G2 — Ghost Lobby Rust gate: tests + clippy + fmt

```bash
just test-ghost
```

Runs `cargo test` for core/tui/ffi/net/evil-weevil, then
`cargo clippy --all-targets -- -D warnings` for the same set minus ffi,
then G1. PASS: every suite ends `test result: ok`, clippy prints no
warnings, fmt exits 0. This is the gate that covers the EW-06
infrastructure-order perception contract and the ADR-0015
`state_contract` table (`crates/idaptik-net/tests/deployment_contract.rs`,
30 topology/class cells).

### G3 — Evil Weevil offline adversarial corpus (ADR-0013)

```bash
just evil-weevil-check
```

No sockets, no Elixir. Runs the corpus tests, clippy with `-D warnings`,
two report-generating runs at seed `777001`, a secret-shape scan of the
artifact, and a `cmp` determinism check. PASS ends with:

```
PASS: Evil Weevil offline gate — artifacts in <tmp>/artifacts
```

### G4 — Shell fixture suites (the `fixtures-and-must` CI job)

```bash
just fixture-tests
```

Eleven offline suites in one recipe (runtime doctor, launcher fixtures,
vendor winit gate, ABI authority pins, CI security config, pages
`.well-known`, actions lock sync, toolchain parity self-test that plants
eight defects and requires them caught, coordination K9,
machine-readable migration, and Must probes). PASS: the recipe exits 0;
each suite prints its own ok line. A FAIL names the suite and the
assertion. The ABI pin suite here is the offline stubbed-curl structural
check; the real network digest re-derivation is the separate
authority-pin job described in G11.

### G5 — Nickel config + negative-fixture gate

```bash
just config-check          # full export + scenario gate
# or the CI job's exact command:
just config-scenario-check
```

Exports the three typed configs, round-trips `ghost_lobby_scenario.ncl`
through the Rust validator via the `--ignored` test
`nickel_scenario_round_trips_and_validates`, then asserts every
`config/scenario-fixtures/bad_*.ncl` is **rejected** by the Nickel
contract. PASS ends `config: ok (schema applied)` /
`scenario config: ok`. A bad fixture that exports is a hard FAIL.

### G6 — Vendored bevy_winit patch gate

```bash
just vendor-winit-check
```

Two checks: `parity` (the vendored crate differs from the published
bevy_winit only by the documented Wayland CSD feature split) and `graph`
(the resolved dependency graph keeps font-parser out while the
title-free CSD feature is active). Either check failing prints the
offending diff.

### G7 — Bevy frontend tests + clippy

```bash
cargo test -p idaptik-bevy
cargo clippy -p idaptik-bevy --all-targets -- -D warnings
```

Needs the apt libraries from §2. This is the longest pure-Rust gate on a
cold build (Bevy); CI allows 40 minutes. Both commands must exit 0.

### G8 — Machine-readable conformance (DEED / K9 / Nickel runners)

```bash
bash scripts/machine_readable_check.sh
```

Needs `node`, `git`, `curl`, `sha256sum`, and outbound HTTPS to
github.com (or a pre-cloned standards checkout at the pinned commit,
exported as `IDAPTIK_STANDARDS_DIR`). It fetches the standards repo at
`8e48b1ca…`, runs the pinned DEED linter (with `--self-test` first as a
positive control), the K9 validator, and the checksum-pinned Nickel
1.18.0 runner. Missing node/git is exit 2 — a FAIL, per doctrine.

### G9 — Supply chain

```bash
just supply-chain        # cargo deny check --hide-inclusion-graph
```

Advisories, licenses, sources and the dependency graph against
`deny.toml`, with cargo-deny 0.20.2. Any advisory or banned crate is a
FAIL with the crate and rule named.

### G10 — Idris 2 ABI model gate

```bash
just install-idris2      # one-time: Chez v10.4.1 + Idris2 v0.8.0 bootstrap
just abi-model-check     # bash scripts/abi_model_check.sh
```

The bootstrap is a 8–14 minute native build (CI caps the job at 45).
Set `IDRIS2_BOOTSTRAP_BIN` exactly as `scripts/abi_model_check.sh`'s
toolchain-resolution list expects; `just install-idris2` follows the
same recipe documented in `crates/idaptik-ffi/abi/README.md`. The gate
runs the proof-escape scan (no `postulate`/`believe_me` outside the
waived ledger rows), total typecheck, runtime smoke, and header/
snapshot-format parity.

### G11 — Zig ↔ Rust ABI conformance (reproduce CI with Zig 0.16.0)

```bash
# Provision Zig 0.16.0 exactly as abi-conformance.yml does:
curl -sL https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz -o /tmp/zig.tar.xz
sudo tar -xJf /tmp/zig.tar.xz -C /usr/local
export PATH="/usr/local/zig-x86_64-linux-0.16.0:$PATH"
zig version    # 0.16.0

bash scripts/zig_adapter_surface_check.sh
cargo build -p idaptik-ffi
(cd crates/idaptik-ffi/zig-adapter && zig build test)
(cd crates/idaptik-ffi/zig-adapter && zig build conformance \
    -Dlibdir="$PWD/../../target/debug")
(cd crates/idaptik-ffi/zig-adapter && zig build fuzz)
bash scripts/fuzz_double_free_probe.sh
```

The surface check enforces header ↔ Idris2 ↔ Zig surface lockstep; the
conformance vectors must pass and the planted failure vectors must be
caught (a test that cannot go red is itself a FAIL here).

The workflow's second job, `authority-pin`, re-derives the sha256
digests pinned in `docs/abi/ABI-AUTHORITY.md` against the live
`hyperpolymath/hypatia` refs named in `.github/workflows/abi-conformance.yml`.
It carries its own negative control (a deliberately nonexistent path
must be refused) and a vacuity control (empty digest sets must fail).
It is an inline workflow step rather than a repo script, so local
reproduction means running the `authority-pin` job on a branch or
replaying its `check()` block verbatim; the offline structural twin is
the `abi_authority_pin_test.sh` suite already run inside G4.

### G12 — Creusot proof ledgers (cheap local half) and full proof (heavy)

```bash
# Ledger half: no toolchain, seconds. Bidirectional check that every
# first-party .rs file is classified and every proof escape has a row,
# plus mutant controls that must kill the check.
bash scripts/creusot_ledger_check.sh --self-test
bash scripts/creusot_ledger_check.sh all
```

The full proof job additionally builds Creusot v0.13.0
(ref `318615be…`) through opam, discharges the kernel's proof
obligations, and requires a planted mutant to go red. It is realistic
only on CI or a dedicated proof host; on a verification machine the two
ledger commands above are the local evidence, and the `Creusot Proof /
proof` check run on the PR is the remote evidence. Record both.

### G13 — Burble loopback determinism + real-relay Evil Weevil leg

Requires §3's sibling burble checkout and OTP/Elixir from §1.

```bash
# ADR-0006 §4: throwaway local burble on 127.0.0.1:4002, two batch seats,
# two live delay-lockstep seats, mid-stream kills, snapshot resync.
just loopback-check

# ADR-0013 real-relay leg against the same throwaway server:
# guest dummy identity, bounded frames, loopback only.
just evil-weevil-relay-check
```

PASS for loopback: both batch artifacts and both live artifacts are
byte-identical to each other and to the `idaptik-tui --headless`
reference, the killed batch seat's peer ends `ended_peer_lost` with
exit 0, and the rejoined live seat reconstructs the full run. The relay
leg PASS means the in-process relay model matches the pinned burble
`GameChannel` behaviour (stale `last_seq` refused, unknown inbound
refused, bodies never parsed). A missing `mix` or missing burble
directory is a hard FAIL with an explicit `FAIL:` line — never a skip.

### Supporting checks worth running once

```bash
just secret-scan     # trufflehog git --no-update --fail (3.96.0)
just lock-sync-check # actions lockfile synchronisation, with self-test
bash scripts/toolchain-parity.sh --self-test   # also inside G4
```

CodeQL runs as a GitHub-hosted workflow with no local twin; record its
check-run verdict from the PR rather than reproducing it.

## 5. One-shot sequences

Offline set — no Elixir, no burble, no servers (everything except G13
and the heavy halves of G10/G12; roughly the practical local sweep):

```bash
set -euo pipefail
cargo fmt --all -- --check
just test-ghost
just evil-weevil-check
just fixture-tests
just config-check
just vendor-winit-check
cargo test -p idaptik-bevy
cargo clippy -p idaptik-bevy --all-targets -- -D warnings
bash scripts/machine_readable_check.sh
just supply-chain
bash scripts/creusot_ledger_check.sh --self-test
bash scripts/creusot_ledger_check.sh all
just secret-scan
just lock-sync-check
```

Full set adds, after the above: `just abi-model-check` (G10), the G11
Zig block, and the two G13 relay gates.

## 6. Verdict ledger (fill in and report back)

Record the commit SHA under test and the machine's `rustc --version`,
`elixir --version`, `nickel --version`, `zig version` first.

| Gate | Command | Verdict (PASS/FAIL) | Evidence (last line / failure excerpt) |
| --- | --- | --- | --- |
| G1 fmt | `cargo fmt --all -- --check` | | |
| G2 test-ghost | `just test-ghost` | | |
| G3 evil-weevil | `just evil-weevil-check` | | |
| G4 fixtures | `just fixture-tests` | | |
| G5 config | `just config-check` | | |
| G6 vendor winit | `just vendor-winit-check` | | |
| G7 bevy | test + clippy | | |
| G8 machine-readable | `bash scripts/machine_readable_check.sh` | | |
| G9 supply-chain | `just supply-chain` | | |
| G10 abi-model | `just abi-model-check` | | |
| G11 Zig conformance | `zig build …` block | | |
| G12 creusot ledgers | `creusot_ledger_check.sh all` | | |
| G12 creusot proof | PR check run (remote) | | |
| G13 loopback | `just loopback-check` | | |
| G13 relay leg | `just evil-weevil-relay-check` | | |
| secret-scan | `just secret-scan` | | |
| lock-sync | `just lock-sync-check` | | |
| CodeQL | PR check run (remote) | | |

## 7. Failure triage notes

- A compile error naming `scenario/ids.rs` duplicate newtypes means you
  are not on the fixed branch — the dedupe landed before this runbook.
- `deployment_contract.rs` failures name the exact cell in the panic
  (`"<StateClass> / <Topology>"`); ADR-0015 §4 is the authority, and P2P
  seats have `NotRequired` hot stores for derived/playout state.
- A Cargo.lock failure from G4/G6 is repaired by editing the lock to the
  already-resolved graph (see the lock-sync self-test), never by
  `cargo update`, which moves unrelated packages.
- G13 port-4002 collisions come from another burble/loopback process on
  the same host; stop it and rerun — the port is not configurable by
  design.
- Any "FAIL: … is required" line means install the pinned tool from §1
  and rerun that single gate; do not mark it skipped.
