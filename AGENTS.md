# Repository instructions

IDApTIK is an asymmetric two-player infiltration game: Rust/Crusoe owns gameplay
truth, Elixir/OTP owns multiplayer session life. The Rust half is this
repository; the Elixir half is `metadatastician/burble`, consumed as a pinned
Cargo git dependency, and **this repository contains no Elixir at all**. It is a
member repository of The Metadatastician estate and adopts that estate's
governance profile via `docs/PROJECT-GOVERNANCE-BINDING.adoc`.

Read `coordination.k9`, `IDApTIK_chora.deed`, `README.md`, `GOVERNANCE.md`,
`.github/MAINTAINERS`, and `docs/PROJECT-GOVERNANCE-BINDING.adoc` before
editing. The deed records descriptive project state; K9 components hold
contractile policy; `coordination.k9` is the root agent-instructions protocol.

Data-format routing: kyq is the estate gate interface for exactly TOML, Dhall,
KYAML and I-JSON/JCS. Preserve TOML/Dhall; migrate YAML/YML content to KYAML and
JSON variants to the estate I-JSON/JCS profile through kyq. Wider projection
belongs to kyX. See `docs/standards-conformance.md` for the current kyq build
blocker and explicit transitional enforcement; never count a placeholder as a
passing gate or describe Kubernetes as kyq's entire scope.

Invariants (see `coordination.k9` and the K9 contractiles):

- Licence layers are deliberate: engine/code is AGPL-3.0-or-later, game
  content is CC-BY-SA-4.0, names/marks (IDApTIK, Moletaire) are trademarked.
  Do not relicense the engine or flatten the layers.
- Gameplay truth lives in `crates/idaptik-core` and stays engine-agnostic and
  deterministic; `crates/idaptik-bevy` is the selected thin graphical
  frontend. Do not move game truth into Bevy.
- **The session layer is not in this repository.** It is Bandit + Phoenix
  Channels (not LiveView) and it lives in `metadatastician/burble`, reached as a
  Cargo git dependency pinned by rev (`burble-client`, see `Cargo.toml`). ADR-0005
  designed it here; ADR-0006 records the port out (burble PR #182) and issue #74
  retired the in-repo relay. There is no Elixir source, no `mix.lock`, and no
  `server/` tree in this repository — do not propose changes to them here, and
  do not add an Elixir toolchain, ecosystem, or build step. The dead
  `.gitignore`, `.gitattributes` and `dependabot.yml` entries that implied
  otherwise were removed on 2026-09-27; the dependabot one had been failing its
  weekly `hex in /server` update for a year because of it.
- Descriptive state belongs in the single root `IDApTIK_chora.deed`; keep policy in K9 contractiles and agent instructions in root `coordination.k9`.
- Contributions come in under DCO 1.1 — sign commits with `git commit -s`
  (see `CONTRIBUTING.md`).

Do not edit generated files directly. Two are declared (in `.gitattributes`):
`crates/idaptik-brand/tokens.json` and `crates/idaptik-brand/src/generated.rs`,
both rendered from `config/brand-tokens.ncl` by `just brand-regen` and checked
by `just brand-check`. Change the Nickel, regenerate, commit all three. Do not
change licences, upstream coined names, or evidence-status labels (in
`docs/PROJECT-ASSURANCE-PROFILE.adoc`) as a side effect of an unrelated change.

## The stack is Rust/Crusoe — attempts 1–3 are dead

`README.md` documents four incarnations. Agents routinely read that lineage
table as current and propose work against a codebase that no longer exists:

| # | Codebase | Stack | Status |
|---|----------|-------|--------|
| 1 | IDApTIK | TypeScript / Excalibur | dead |
| 2 | IDApixiTIK | AffineScript / PixiJS | dead |
| 3 | idaptik | AffineScript / PixiJS | dead |
| 4 | **IDApTIK** | **Rust/Crusoe** + Elixir/OTP session fabric | **live — this repo** |

Incarnation 4 is a two-repository system. **Only the Rust half is here.** The
Elixir/OTP half — Bandit, Phoenix Channels, the `game:` lane relay — lives in
`metadatastician/burble` and is reached through the pinned `burble-client` Cargo
dependency. Reading "Rust/Crusoe / Elixir" as "this repository has Elixir in it"
is the single most common misreading of this file, and it produces proposals to
edit a `server/` tree that was removed under issue #74 and ADR-0006.

There is no JavaScript runtime here: no browser, no Canvas, no
`requestAnimationFrame`, no npm, no PixiJS, no Excalibur. A proposal written
against any of those cannot be applied.

Elixir is likewise not an implementation language *of this repository*, though it
is of the system: there is no `.ex`, `.exs`, `mix.lock`, or `_build` here, and
`git ls-files` confirms it. Session-layer changes are pull requests against
`metadatastician/burble`, not against this tree.

Ruby and Python are also not implementation or project-tooling languages in
this repository. Do not introduce Ruby/Python source, runtime dependencies,
formatters, linters, package ecosystems, or workflow steps. A documentation
reference to a separate repository's implementation does not make that
language part of IDApTIK.

GitHub's hosted service actions are an infrastructure boundary, not part of
the project stack. Prefer direct commands when an action would add an avoidable
language runtime (checkout uses `git` directly). A hosted action whose service
protocol has no practical replacement, such as CodeQL analysis or Pages
deployment, may use its own internal runtime; keep it SHA-pinned and do not
extend that exception to repository code, scripts, dependencies, or general
workflow tooling.

Two further confusions worth pre-empting:

- **miniKanren is not in this repository.** Its Rust implementation lives in
  `metadatastician/universal-modding-studio` (UMS, the Universal Modding
  Studio). IDApTIK owns game truth and the package contract; UMS owns the
  authoring and solving surface.
- **AffineScript is not the current language.** It is a separate language of the
  maintainer's, used in dead attempts 2 and 3 and a possible future target. It
  is not what this repo is written in.

## Already built — do not re-specify

- **Fixed 60 Hz timestep.** `crates/idaptik-bevy/src/driver.rs` sets
  `Time::<Fixed>::from_hz(60.0)` and steps the sim in `FixedUpdate`;
  `idaptik-core` defines `TICK_DT = 1.0/60.0` (`scenario/mathf.rs`) and
  accumulates time rather than multiplying it.
- **Determinism.** A run is a pure function of
  `(definition, config, seed, command stream)`, enforced by `replay_determinism`,
  `snapshot_equivalence`, `rng_vectors`, `golden_reset` and `no_panic_fuzz`.
- **The `Command`/`Event` wire API**, shared by the TUI and Bevy frontends;
  `crates/idaptik-bevy/tests/parity.rs` proves they stay in step.
- **Snapshots** (`scenario/snapshot.rs`) for save/restore and resync.
- **Netplay.** `crates/idaptik-net` holds a sans-IO delay-lockstep core behind a
  two-seat byte-identical loopback gate.
- **The signal network** (`scenario/signal/`, ADR-0012): typed devices with fixed
  port tables, wires that carry booleans one hop per tick, relay gates, per-seat
  `SeatProfile` pricing, an `Access` set computed from vantage, and a `budget`.
  Installer wires are `builtin` and cutting one is refused with
  `SignalDenyReason::Hardwired`. Do not re-specify "wiring"; extend it.
- **Electrical power** (`scenario/power/`, ADR-0016): sources, feeds, loads,
  breakers and backup supply behind one rule —
  `operating = commanded AND energized AND intact AND not interlocked AND warmed
  up` — with `OffReason` naming the first failed rung rather than returning a
  bool. Not driven by any shipped floor yet (issue #192).
- **Water, gas and airflow** (`scenario/infrastructure/`, ADR-0013) and
  consequences-as-observations (`scenario/infra.rs`, ADR-0014): integer-only
  derivations, not a fluid solver; the infrastructure runtime is not yet called
  from the game tick.
- **The hosted companion.** Moletaire is no longer a seam: `Command::Mole`,
  `MoleHostState` on `RuntimeState`, `scenario/sim/mole.rs`.
- **Durable storage** (`crates/idaptik-persist`): a store *beside* the tick for
  command log, checkpoints and replay-safe resume. Nothing on the hot path.
- **Evil Weevil**, in two pieces: `crates/idaptik-evil-weevil` (offline
  adversarial corpus over the real validators, ADR-0017) and
  `crates/idaptik-weevil` (host adapter over the kernel's C ABI). See
  `docs/dev-notes/weevil-integration.md`.

There is no garbage collector — this is Rust/Crusoe. "Avoid GC pauses" is not a design
goal here; it is free. Bevy is the selected graphical frontend (ADR-0008).
Fyrox was retired because its evaluation crate remained a bring-up stub while
Bevy acquired the renderer, parity tests and hosted-client design.

Nothing render-side may feed back into simulation state. The determinism tests
are load-bearing, not decorative.

## Checks must be able to fail

A check that cannot fail is not a check. Do not add a test that reports success
without running anything, and do not add a gate that exits 0 when its tool is
missing — an absent toolchain is a failure, not a skip.

## Building

Run `mise trust` first. An untrusted mise config is *silently ignored*, so the
global toolchain resolves instead (e.g. zig 0.16 rather than the pinned 0.14)
and the resulting stdlib errors point nowhere near the real cause.

```
just test-ghost              # cargo test (core/tui/ffi/net/evil-weevil/persist) + clippy -D warnings + fmt --check
just config-check            # nickel typecheck
just config-scenario-check   # Nickel export → Rust validator → every bad_*.ncl rejected
just loopback-check          # two-seat netplay parity gate
just evil-weevil-check       # offline adversarial corpus over the real validators (no sockets)
just machine-readable-check  # DEED + K9 conformance and Nickel runner typechecks
```
