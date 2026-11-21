# IDApTIK workplan

Status: **organised first draft, 2026-07-26; refreshed 2026-11-21** (the
work queue is the cognitive-model line; the building systems that landed
alongside it are summarised in *Where things stand* below)

This is the work queue. Detailed cognitive-model design is documented here in
the open, alongside the code that implements it, under `docs/architecture/` and
`docs/dev-notes/` — see ADR-0011, which withdrew the earlier rule sending it to
a private repository.

## Now: deterministic foundations

- [x] Clean `main`, Pages deployment, domain, HTTPS, security baseline.
- [x] Publish the shared ESM and repository-boundary contract.
- [x] Add the first VSM-shaped deterministic supervisor prototype.
- [x] Add bounded, provenance-bearing scalar hypothesis tests.
- [x] Add a deterministic DST-style evidence ledger using fixed-point masses.
- [x] Add USB/fridge-note and front-door/vent conformance fixtures.
- [x] Preserve conflict, ignorance, fixed-point replay, and focal-set queries.
- [x] Connect evidence plausibility to bounded patrol attention allocation.
- [x] Translate selected Ghost Lobby events into observer-relative evidence.
- [x] Add an opt-in supervised headless runner path.

## Next: first playable proving slice

- [x] Model the first NPC operator-team type and attention trace event.
- [x] Connect the team allocation to actual Ghost Lobby world coverage.
      (Supervision is deterministic runtime state; `RunConfig::supervised`
      bends Billy's Assess patrol band toward the evidence-selected coverage
      target. The interactive TUI is always supervised.)
- [x] Add a bounded adaptive intervention with an explicit policy check.
      (`SupervisionState::director`; sabotage folds into tactic counts, the
      director proposes, the policy envelope accepts or rejects. Tests:
      `repeated_confident_tactic_proposes_bounded_intervention`,
      `policy_rejects_stale_or_over_limit_proposals`,
      `repeated_sabotage_escalates_through_the_bounded_proposal_path`.)
- [ ] Expose developer telemetry in TUI/replay output.
      (Still open: the supervision state is not rendered. A truth / readouts /
      beliefs view is the outstanding half — see the tamper handover §6 M4.)
- [x] Verify that the player can create a rational but false NPC hypothesis.
      (`ledger_can_hold_a_rational_but_wrong_usb_hypothesis`,
      `evidence_fixture_supports_usb_then_note_revision`.)

## Later: presentation and wider integration

- [ ] Connect the same state to the Bevy frontend.
- [ ] Add player-visible patrol consequences without exposing omniscient state.
- [ ] Export/import versioned UMS profile packages.
- [ ] Add typed/proof conformance checks in the Enaction Engine and formal
      repositories.
- [ ] Generalise the profile for Chronicles of Slavia's cognitive, affective,
      and conative scope.

## Boundary rule

IDApTIK owns gameplay meaning and runtime validation. UMS owns authoring and
profile compilation. The Enaction Engine owns shared semantics. Multiplayer
relays events but does not become semantic authority. Private AI details remain
in the canonical private UMS repository unless a public interface requires
their disclosure.

## Where things stand (2026-11-21)

The work queue above is the cognitive-model line, and it has not been the only
line. Since this plan was drafted, the building itself grew four systems and the
simulation grew a companion host, a storage crate and an adversarial test
corpus. All of it is in `idaptik-core`, and none of it changes the boundary rule
below:

- **Signals** (`scenario/signal/`, ADR-0012) — control wiring, per-seat access
  and pricing, hardwired installer runs.
- **Electrical power** (`scenario/power/`, ADR-0016) — sources, feeds, loads,
  breakers, and `OffReason` naming the first failed rung. **Not yet driven by any
  shipped floor** (issue #192).
- **Water, gas and air** (`scenario/infrastructure/`, ADR-0013), with
  consequences-as-observations in `scenario/infra.rs` (ADR-0014). **Called from
  every game tick but inert on the canonical floor:** `system_infra()` runs at
  `scenario/sim/mod.rs:727`, and `ghost_lobby.rs:603` sets `infra: None`, so it
  returns early unless a synth diversion is live. No shipped floor defines water,
  gas or air yet.
- **Moletaire is hosted** (`Command::Mole`, `sim/mole.rs`), so the companion is
  no longer a seam waiting for a host.
- **`crates/idaptik-persist`** holds the command log, checkpoints and
  replay-safe resume; **`crates/idaptik-evil-weevil`** exercises the acceptance
  rules offline, and **`crates/idaptik-weevil`** is the guard brain's host
  adapter.
- **November (2026-11-02 to 2026-11-20).** The weevil adapter moved from B1
  stages 1 and 2 (#225, #227) to C2 (#230): it mirrors kernel ABI 1.2, the
  corridor observes, and contacts remember (#232). Since #234 it emits Enaction
  kernel contract 2.0 (`enaction-runner-output/2`); IDApTIK's side is done and
  what remains is the UMS side emitting the new tag (the deed's open decisions
  hold the detail). The proof ledger became machine data
  (`config/proof-ledger.ncl`, #222) beside a brand contrast gate decided by the
  kernel's integer contrast law, and the Creusot migration plan landed
  (`docs/verification/2026-10-31-creusot-ultraplan.md`, #212). The legacy test
  relay is confined to local-only sessions (#214, #219); `idaptik-persist`
  resume replays the first unapplied command and builds on redb 4 (#213); and
  `launcher.sh --windows-client` offers a native Windows client under WSL2
  (#215).

Two things are still outstanding in the cognitive-model line, both below:
**developer telemetry** (truth / readouts / beliefs) is the next visible item and
is not rendered anywhere; and the **Billy-with-a-closed-door limit cycle** is
still canonical behaviour that nobody has ruled on.

## Current stopping point of the cognitive-model line

The supervised-coverage slice (2026-08-03) moved the supervisor inside the
deterministic runtime state: in a supervised run the sim folds its own event
stream into `RuntimeState::supervision` (snapshot format v3), the evidence
ledger picks the most plausible coverage target (`usb` vs `fridge_note`,
declaration-order tie-break), and Billy's Assess patrol band bends toward that
target in proportion to team attention. The allocation is visible in the
canonical log as announce-once `TeamAttentionAllocated` / `CoverageRetargeted`
events — the seed of the "developer telemetry" checkbox. The interactive TUI
always runs supervised; headless scripts and the CLI opt in via
`"supervised": true` / `--supervised`, and unsupervised runs are byte-identical
to before the slice.

Two facts the next session should know:

- Billy cannot badge through a closed door: the constraint bounce-back plus
  `door_wait` resetting on every unblocked tick form a limit cycle, so the
  badge timer never accumulates at any Billy speed. Every golden embeds this,
  so it is canonical behaviour for now — but it silently confines patrol
  coverage to one door region, and deserves its own decision (bug or feature).
- ~~The next unchecked item is the bounded adaptive intervention; the
  `VsmDirector::observe`/`apply` policy envelope already exists and is tested,
  it just is not driven from sim state yet.~~ **Done on 2026-10-25:**
  `SupervisionState` owns the director and drives `observe` → policy → `apply`
  from the sim's own event fold. The next unchecked item is developer telemetry.
