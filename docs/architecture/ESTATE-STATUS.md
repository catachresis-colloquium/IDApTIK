# Estate status

Last assessed: **2026-07-28**.

> **Currency note (2026-10-25):** this document describes IDApTIK's place in the
> four-repository estate, and it has not been re-assessed since July. For state
> *inside* this repository — components, crates, ADR register, build and test
> gates — the canonical source is now `IDApTIK_chora.deed`, which is updated
> with each change. Read this document for the estate picture and that one for
> the current tree.

Where IDApTIK sits in the four-repository estate, what is enforced rather than
merely written down, and what is still open. Written to be readable by someone
arriving with no history.

For the cognitive-architecture material specifically, start at
`enaction-engine/docs/architecture/CAC-DOCUMENT-MAP.adoc`, which indexes every
relevant document across all six repositories.

---

## The shape

```
        enaction-engine  (runtime kernel)
              ▲
   ┌──────────┴──────────┐
IDApTIK           chronicles-of-slavia        Ovine / cac-engine
   ▲                     ▲                    (donor — nothing depends on it)
   └──────────┬──────────┘
        idaptik-ums  (authoring studio)
```

Two shared layers, in separate repositories, with separate release artefacts.
The Enaction Engine is the runtime kernel a shipped game links; Universal
Modding Studio is the authoring studio that emits packages.

They are separate for one hard reason: **a released game must build and run
with neither UMS source nor UMS binaries present.** UMS reads
`contracts/idaptik/v1/*.json` as data, by path. That
`ums_profiles::compile_idaptik()` takes an `--idaptik-root` argument rather
than a Cargo dependency *is* the architecture, not an implementation detail.

Accepted in: IDApTIK ADR-0009, Enaction ADR-0013, UMS ADR-0016.

---

## What is enforced, not just documented

The estate's recurring failure has been boundaries stated in prose with no
failing test behind them. These now have one. Every check below runs with **no
sibling repository present** — a check that skips when a sibling is absent is
not a check.

| Invariant | What it prevents |
|---|---|
| `no-authoring-dependency` | IDApTIK acquiring a dependency on UMS or Slavia |
| `cac-kernel-is-fixed-point` | `scenario/vsm.rs` acquiring an `f32`/`f64` |
| `package-contract-published` | the v1 contract artefacts going missing |
| `crates-workspace`, `adr-records` | the manifest drifting from the tree |

The cross-repository half is
`idaptik-ums/scripts/check-architecture-boundaries.sh`, which hard-exits on a
missing sibling and runs weekly rather than per-PR — a PR gate that fails on
unrelated sibling drift teaches you to ignore it.

**Scope note on `cac-kernel-is-fixed-point`.** It pins `scenario/vsm.rs` only,
which is float-free today. The rest of `scenario/` uses `f64` for continuous
world quantities under ADR-0004 and that stays permitted.
`actor/belief.rs` (6), `trace.rs` (3) and `agents.rs` (1) still hold floats and
must convert before they feed the ledger. Widen the check as they convert; do
not widen it first.

---

## The UMS round trip: tested

`just roundtrip-idaptik` satisfies every clause of UMS ADR-0014 as of
2026-07-28. It compiles the profile source, hands the exact artefact to this
repository's real loader, executes, snapshots, restores, replays, and compares.

Two things it now proves that it did not before:

**Deterministic repeat output.** The source is compiled twice and the artefacts
must be byte-identical.

**Comparison with the post-edit model.** `RoundTripResult.accepted` carries the
loader's own parsed view of the authored envelope — `scenario_id`, `seed`,
`run_ticks`, `snapshot_tick`, `taxonomy`, `actors`, `commands`, `guarantees`.
UMS diffs it against the profile source and fails on any difference.

Before this, the gate proved the game *accepted* the artefact and *replayed it
deterministically*. Both real, and neither is faithfulness: a compiler that
dropped a taxonomy term or retimed a command would have produced a package this
game accepts, replaying identically **to itself**, with every assertion passing.

`accepted` deliberately **excludes `scenario`**. The compiler copies this
repository's own fixture into the package verbatim, so diffing it back would
compare the fixture against itself and prove only that serde round-trips JSON.
If the compiler ever *synthesises* scenario content rather than copying it, that
decision needs revisiting.

---

## The CAC kernel: grown here, not extracted

The evidence ledger, hypothesis ledger, VSM supervisor and `CacTrace` live in
`crates/idaptik-core/` and stay there until the Enaction extraction trigger
fires (ADR-0010). All three parts are required:

- **T1** a second caller whose want was recorded *in its own repository first*;
- **T2** bidirectional conformance fixtures;
- **T3** a recorded surprise — a documented case where the naive boundary was
  wrong.

T3 exists because the interpolation precedent does not transfer. `DoubleBuffer`
has a *falsifiable* contract: it is correct or the picture judders, and the
judder does not care who wrote the second caller. An epistemic kernel's contract
is *semantic*, and **same-author second callers do not supply independence**.

This was tested on 2026-07-28 against `enaction-relation`, the most plausible
extraction candidate, and it **failed T1** — no want was recorded anywhere, and
`slavia-core/src/esm/kanren.rs` says in its own module docs *"do not pay that
until a profile asks."* The defect that prompted it was fixed in place instead.

---

## Where the estate's cognitive work actually lives

Three games have each implemented a different third of one architecture, and
none of them knows about the others:

| | Chronicles of Slavia | Ovine / cac-engine | IDApTIK |
|---|---|---|---|
| **Cognition** | **strong** — microKanren, belief as baseline plus ordered deltas, confidence *is* the solution count | weak — a mock solver with the intended query in a comment | evidence and hypothesis ledgers |
| **Affect** | specified, unbuilt | **strong** — six cross-coupled emotions, PAD summary | six constants in a fixture |
| **Conation** | absent | **strong** — homeostatic drives, utility arbitration, commitment hysteresis | VSM supervisor, patrol attention |
| **Determinism** | enforced | **broken** — variable timestep | enforced, byte-parity under lockstep |

The synthesis, accepted in Enaction ADR-0014: **appraisal is a process,
inference is an act.** They join at an event-sourced ledger, never at a shared
update function, and exactly two values cross — a quantised affect token, and a
contradiction acting as an algedonic signal. **Affect buys attention, never
belief.**

---

## Open problems

Recorded so they are not silently assumed. Full statements in
`esm-contract-and-repo-boundaries.md` §9.

- **The proposition language.** Can a Dempster–Shafer focal set contain a
  non-ground proposition? miniKanren's value is variables; DS frames are
  enumerated sets. If irreconcilable, "one kernel" weakens to "one trace, two
  reasoners". This blocks a single shared ESM crate.
- **Theory-of-mind depth.** No policy exists. Any cap must be counted in
  *deterministic units* — inferences, unifications, stream nodes — never
  milliseconds, or byte-parity breaks between machines.
- **Affect's clock**, which sets the scale ceiling.
- **Whether affect is state or annotation.** The contract permits only the
  latter; a continuous appraisal layer is the former.
- **What "verified" means** in the conative path — and that a veto must be an
  *event*, not a silent substitution.
- **The 1:1 domain/stage correspondence** holds for exactly one scenario.
  Either a deep result or a frozen accident.
- **Nothing has been benchmarked.** No measurement exists for a solver query, a
  ledger combine, or a tick of affect.

---

## Reading order

1. `docs/architecture/esm-contract-and-repo-boundaries.md` §5–§7 — ownership and
   the package hand-off, which this repository owns.
2. `enaction-engine/docs/architecture/CAC-KERNEL.adoc` — the kernel contract.
3. ADR-0009, ADR-0010, ADR-0011 here — dependency direction, why the kernel
   stays put, and why the work is documented in the open.
4. `idaptik-ums/docs/ROUNDTRIP-STATUS.adoc` — what the round trip does and
   does not prove.
