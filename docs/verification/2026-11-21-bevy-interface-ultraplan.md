<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->
# IDApTIK Bevy interface ultraplan

Date: 2026-11-21. Baseline: `96d04339` (main after #240).
Status: proposed work programme. It is not an implementation claim and not a
validation certificate.

## 1. Objective and non-negotiable distinctions

Close the 2 critical items in `IDApTIK_chora.deed` (`:critical`):

1. Build the Bevy product shell: launch, session and seat choice, Ghost Lobby
   play, debrief, and restart.
2. Validate the Bevy frontend in a real window, and set performance and
   accessibility baselines.

The README milestone adds 2 more items: first art, and documented mechanics.

These rules are fixed. ADR-0008 and AGENTS.md set them, and this plan does not
change them:

- `idaptik-core` owns game truth. Bevy translates input into typed `Command`s
  and presents events and snapshots. No game rule moves into Bevy.
- Render-derived and interpolated values never feed simulation state. The
  parity suites (`tests/parity.rs`, `tests/interp_parity.rs`) are load-bearing.
- The headless, TUI, FFI and multiplayer paths stay usable without Bevy.
- The session layer stays in `metadatastician/burble`. No Elixir, Python, Ruby
  or JavaScript enters this repository. Actions stay SHA-pinned.

These are 4 different events. Do not report one as another:

1. A headless Bevy test passes (`MinimalPlugins`, no window, no GPU).
2. A frame renders in a real window on a named platform and adapter.
3. A person plays a full run from launch to restart.
4. A measured baseline exists for frame time, tick time or accessibility.

## 2. Baseline evidence

Inventory at `96d04339` (physical lines, including comments and blanks):

| Inventory | Files | Lines |
|---|---:|---:|
| `crates/idaptik-bevy/src` | 25 | 10,345 |
| `crates/idaptik-bevy/tests` | 5 | 766 |
| `crates/idaptik-bevy/examples` | 1 | 194 |
| `#[test]` functions in the crate | | 78 |
| Tracked binary assets (`crates/idaptik-bevy/assets`) | 92 | 345,425 bytes |

Stack: Bevy 0.20.0 with only the `2d` and `ui` features. `bevy_winit` is a
vendored patch (issue #105), gated by the `vendor-parity` CI job. The Evil Weevil
guard brain is the optional `weevil` feature.

What exists already (read from the source, not run in this session):

- Fixed 60 Hz step in `driver.rs`, and render interpolation.
- Both seats in one window (`seats.rs`): the infiltrator floor and the hacker
  van, with the deck as a layer.
- `AppMode` has 2 states, `GhostLobby` and `NetView` (`net_view.rs:14`).
- A netplay role selector (`netplay/ui/role_select.rs`).
- An end-of-run overlay with grade, score and the core `debrief()`, and
  `R` restarts from the same seed (`hud.rs:311-341`).
- A native Windows client from WSL2 (#215, evidence note
  `docs/dev-notes/wsl-windows-client-launch-2026-11-01.md`).

CI: the `bevy-frontend` job runs `cargo test -p idaptik-bevy` and clippy.
`vendor-parity` passes on main. `Creusot proof` fails on main. #245 fixes it.

Not executed in this session: a Bevy build, the Bevy tests, or a real-window run.
This is a statement of scope, not a green check.

Stale facts found while this plan was written:

- `crates/idaptik-bevy/src/lib.rs:1` says "evaluation, see ADR-0003". ADR-0008
  selected Bevy on 2026-07-25.
- `DR-0003` says the repository tracks zero binary assets and the question is
  "not yet live". 92 binary files are tracked since `cd7c71f` (2026-10-14). By
  its own text, the question is now live.

## 3. Workstream A: entry conditions (blocking)

### A0. Land the pre-Bevy maintenance

Merge or close #241 (Evil Weevil CI job, Zig pin, Rust ABI mirror test), #242
(repository links), #244 (Rust runtime smoke test, zero Python) and #245
(Creusot 0.15 pin). Done when main is green on every required check, so that a
Bevy PR is not judged against an inherited red.

### A1. Core review findings on the Bevy path

The max review of `idaptik-core` (21 Nov) found 3 defects that the shell
reaches directly. The owner ruled (21 Nov) that they are fixed before B0, in
1 PR: #247.

| Finding | Where | Why the shell reaches it |
|---|---|---|
| Restore does not check sub-states against the definition | `scenario/sim/mod.rs:175` | Netplay resync calls restore. A corrupt or hostile snapshot panics the client. |
| More than 16 doors panics | `interp.rs:274` (`MAX_DOORS`) | Interpolation is render-side. Validation puts no cap on doors. |
| Out-of-range difficulty presets pass validation | `scenario/definition.rs:560` | A difficulty choice in the shell exposes them. |

The other 10 findings are in the review report and are out of this plan.

### A2. Owner decisions

| # | Decision | Blocks |
|---|---|---|
| 1 | DR-0003: ruled 21 Nov, recorded as ADR-0019 (#248): plain Git in this repository, no LFS, assets built from `source/`, 1 MiB per file | Closed |
| 2 | DR-0004: which checks are required, and if `bevy-frontend` is one | A0 exit, every B PR |
| 3 | DR-0001 (SonarCloud organisation), DR-0002 (security contact) | Nothing here |
| 4 | Schema `$id` and `contract_id` in `contracts/` after the move to `catachresis-colloquium` | Nothing here |
| 5 | README "power grids bite back": no shipped floor drives power yet (#192) | E1 (documented mechanics) |
| 6 | #245 question 1: update `creusot_checker` in `config/proof-ledger.ncl` to 0.15 after a CI receipt | Nothing here |
| 7 | #245 question 2: commit proof sessions in the 0.15 `why3find.proofs/` layout | Nothing here |
| 8 | #245 question 3: dependabot ignores or groups `creusot-std` | Nothing here |
| 9 | `docs/dev-notes/weevil-integration.md:90-92` says the adapter needs evil-weevil PR 11 until it merges. CI pins `6c7864e`. Is the note still true? | The `weevil` feature in C0 |

## 4. Workstream B: the product shell

### B0. Shell state machine

Add the shell states around the existing modes: launch, seat choice, play,
debrief, restart or quit. The states are presentation. Each transition sends a
typed `Command` or builds a fresh sim from `(definition, config, seed)`.

Done when a headless `App` test drives every transition, `parity.rs` and
`interp_parity.rs` still pass, and a restart gives the same state as a fresh
sim with the same inputs.

### B1. Seat and session choice

Local play: both seats, the infiltrator only, or the hacker only, with the
existing `seats.rs` layouts. Netplay: the existing role selector, reached from
the shell. The network path depends on #211 (P0: safe Burble-hosted
multiplayer, versioned 0.8 session consumer).

Done when each choice reaches play in a headless test, and the netplay choice
fails with a clear message when the relay is absent.

### B2. Debrief

Extend the end-of-run overlay into a debrief screen. It is a view over the
event stream and `debrief()`, and adds no new truth. The power `OffReason` shows
only after a shipped floor drives power (#192).

### B3. Cognitive state in Bevy (WORKPLAN "Later")

Connect the supervision state to the Bevy frontend, and add player-visible
patrol consequences without omniscient state. The developer view (truth,
readouts, knowledge) goes in the TUI first, as handover section 6 M4 says. The
Bevy pane follows it.

## 5. Workstream C: real window and baselines

### C0. Real-window evidence

Record a run on each platform the owner names: Linux Wayland, Linux X11, and
native Windows (#215). Record the adapter Bevy selects, the platform, the
commit, and the result for each. A failure is evidence, not a skip.

### C1. Performance baseline

Measure frame time and sim tick time on the Ghost Lobby floor. Set budgets only
after the first measurement. The harness and runner are an owner decision; use
the `proven-tests-and-benchmarks` options the owner named, not a new tool.

### C2. Accessibility baseline

The contrast gate exists (`crates/idaptik-brand/tests/legibility.rs` with the
kernel contrast law). Every new text pair goes through it. Add keyboard-only
play checks for every shell screen, and a check that state is never shown by
colour alone. `bevy_a11y` is in the dependency tree; its use is a B0 design
choice.

## 6. Workstream D: art and assets

- D0. ADR-0019 (#248) rules on storage: plain Git, no LFS, 1 MiB per file,
  enforced by `scripts/asset_size_check.sh`. The loadable assets are built
  from `source/` at package time. The 92 hand-committed files move to
  `source/` with a package-time build before new art lands.
- D1. Every asset has a row in `crates/idaptik-bevy/assets/CREDITS.md` with its
  source and licence. Game content is CC-BY-SA-4.0.
- D2. Colour, type and spacing come only from `config/brand-tokens.ncl` through
  `just brand-regen` and `just brand-check`.

## 7. Workstream E: documentation and proof alignment

- E1. Documented mechanics: a player-facing page for each mechanic the shell
  shows, with claims checked by `scripts/doc_claims_check.sh`.
- E2. Fix the 2 stale facts in section 2.
- E3. Proof scope follows the Creusot ultraplan section C9. Pure view
  derivations (interpolation, input to command mapping, HUD choices) are
  candidates. Every new `.rs` file needs a row in
  `crates/CREUSOT-CLASSIFICATION.tsv`, or `creusot_ledger_check.sh` and the Pons
  job fail.

## 8. Integration gates

Every Bevy PR contains:

- One shell slice, and no unrelated cleanup.
- `cargo test -p idaptik-bevy`, clippy `-D warnings` and `fmt --check`.
- `parity.rs` and `interp_parity.rs` unchanged in intent and passing.
- The core determinism suites unchanged: `replay_determinism`,
  `snapshot_equivalence`, `rng_vectors`, `golden_reset`, `no_panic_fuzz`.
- `vendor-parity`, the Creusot ledger check, and `doc_claims_check.sh`.
- For a visible change: real-window evidence from C0 on at least 1 platform.

## 9. Delivery order and completion

No calendar promise before A0 and A2 decisions 2 and 9.

1. A0 and A2 (owner).
2. E2 and the A1 fixes the owner selects.
3. B0, then B1 and B2.
4. C0, then C1 and C2.
5. D0, then new art.
6. B3 and E1.

The deed component `idaptik-bevy` stays `selected-partial` until both deed
critical items have evidence of kinds 2, 3 and 4 from section 1. Stop a slice
on parity drift or on any render value that reaches the sim. Revert the slice
and do not weaken the parity test.
