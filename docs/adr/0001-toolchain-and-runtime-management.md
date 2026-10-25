# ADR-0001: Toolchain and runtime management

- Status: Accepted
- Date: 2026-07-09

## Context

IDApTIK is polyglot from day one: Rust owns gameplay truth, Elixir/OTP owns
multiplayer and session life, and the FFI/config edges pull in several more
toolchains. We need every contributor — and every ephemeral Claude Code web
session and CI runner — to get the *same* toolchain versions with minimal fuss.

The tools in play:

| Tool | Role here |
|------|-----------|
| **Rust** (rustup) | gameplay truth, engine frontends, FFI surface |
| **Erlang/OTP + Elixir** | multiplayer / session layer (see ADR-0002) |
| **Zig** | C ABI + cross-compilation for FFI/APIs |
| **Idris2** | formally modelling the FFI/ABI contracts |
| **Nickel** | typed configuration (game / level / network config) |
| **Just** | task runner |

`bevy` is a Cargo crate, not a toolchain — it is pulled by Cargo, not by a
version manager (see ADR-0008). `hex` and `bandit` are likewise Hex
packages pulled by `mix`, not toolchains (see ADR-0002).

## Decision

1. **Use `mise` to pin and install the non-Rust toolchains** (Erlang, Elixir,
   Zig, Just, Nickel), via `mise.toml`. mise is chosen over asdf: same
   `.tool-versions` semantics and asdf-plugin compatibility, but a fast Rust
   implementation, PATH-based activation instead of shims, and registry backends
   that install Zig/Just/Nickel from upstream releases without bespoke plugins.
   Staying asdf-compatible also keeps us aligned with the wider estate's
   convention.
2. **Keep Rust on `rustup` via `rust-toolchain.toml`**, not mise. This keeps
   cargo subcommands, clippy, and rust-analyzer resolving identically in editors,
   CI, and web sessions, and avoids the cargo-subcommand conflicts that come from
   managing Rust through a generic version manager. Pinned to 1.97.1 (Bevy 0.20
   MSRV; was 1.95.0 for Bevy 0.19 until the 2026-10-25 re-vendor of
   bevy_winit 0.20.0); `wasm32-unknown-unknown` kept ready for browser builds.
3. **Provision Idris2 out-of-band via `idris2-pack`** (`just install-idris2`),
   not through mise. Idris2 bootstraps through Chez Scheme + GMP and has no clean
   prebuilt; forcing it into `mise install` would break bootstrap on machines
   without Chez. This mirrors how the estate's opsm handles it (via `guix.scm`).
4. **Single source of truth per layer.** Toolchain versions live in `mise.toml`
   (+ `rust-toolchain.toml`); we do not also keep a hand-maintained
   `.tool-versions`, to avoid the drift that would let the two disagree.
5. **Bootstrap is user-invoked**, via `just bootstrap` (fast: mise + prebuilt
   tools) or `just setup` (full, builds Erlang/Elixir from source). Wiring
   `scripts/bootstrap.sh` into a SessionStart hook for web sessions is left as an
   opt-in the maintainer can add deliberately.

### Is that the full set?

For the current milestone (Envelope), yes: `rust, erlang, elixir, zig, idris2,
just, nickel`. Deliberately **not** pinned yet, to be added when first needed:

- a fast linker (`mold`/`lld`) and Bevy's dynamic-linking feature for iteration
  speed — a per-machine `.cargo/config.toml` concern, not a pinned toolchain;
- `wasm-bindgen`/`trunk` if/when we ship a browser build;
- a wire-format toolchain (protobuf/flatbuffers) for the Rust↔Elixir boundary,
  once that protocol is designed.

### Relationship to opsm

opsm (the estate's universal package manager) is **not** used to provision these
toolchains today: it is a cross-registry *package* manager, it is itself an
Elixir escript that needs the BEAM present first, and its real-registry path is
still maturing. mise now; opsm later, for cross-ecosystem *dependency* pulls once
it is ready. IDApTIK is a natural dogfooding target at that point.

## Consequences

- `mise install` + `rustup` (honouring `rust-toolchain.toml`) reproduces the
  toolchain anywhere; `just doctor` reports what is present.
- Versions mirror the opsm estate where they overlap, so the projects agree.
- Idris2 remains a deliberate, documented extra step rather than a silent failure.
- Zig and Idris2 are the FFI/ABI boundary languages. The Idris2 ABI *model*
  now exists (`crates/idaptik-ffi/abi`, gated by `just abi-model-check` and
  CI), but the Zig *adapter* does not: the C ABI is still exported directly
  from Rust and consumed as such. The remaining boundary work — the Zig
  adapter and the serde-wire conformance suite — is tracked in
  [issue #103](https://github.com/metadatastician/IDApTIK/issues/103).
