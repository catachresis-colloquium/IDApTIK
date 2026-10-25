# ADR-0010: The CAC kernel is grown in `idaptik-core`, not extracted yet

- Status: Accepted
- Date: 2026-07-27
- Relates to: ADR-0004 (deterministic event-sourced sim), ADR-0009 (kernel
  dependency direction), `docs/ENGINE_EXTRACTION_NOTES.md`

## Context

The evidence ledger, hypothesis ledger, patrol attention allocation and the
Ghost Lobby supervisor adapter live in `crates/idaptik-core/src/scenario/vsm.rs`
and `src/trace.rs`. The cognition/affect/conation trace (`CacTrace`,
`GuardTraceStage`) lives in `crates/idaptik-core/src/package.rs` and runs in the
package round trip.

`docs/architecture/esm-contract-and-repo-boundaries.md` §5 assigns kernel
semantics to the Enaction Engine. Read literally and immediately, that says this
code is in the wrong repository.

It is not. `vsm.rs` has one caller, is three weeks old, and its own doc comment
still describes it as an experiment. The Enaction Engine has no evidence ledger
and its workspace does not build. Moving unproven code into an empty repository
would produce a kernel designed from a single example, in a repository with no
game to be wrong against.

`docs/ENGINE_EXTRACTION_NOTES.md` set the precedent for the interpolation layer:
extract when a second host wants the same buffers. That rule works for
`DoubleBuffer` because its contract is falsifiable — it is correct or the
picture judders, and the judder does not care who wrote the second caller. An
epistemic kernel's contract is semantic. Two callers written by the same author
can both be satisfied by a boundary that quietly encodes Ghost Lobby's
ontology. **Same-author second callers do not supply independence.**

## Decision

The evidence ledger, hypothesis ledger, VSM supervisor and CAC trace **stay in
`crates/idaptik-core/`** until all three parts of the Enaction extraction
trigger fire:

- **T1 — a second caller, whose want was recorded first.** A second consumer
  must want the seam, and the want must be written down in that consumer's own
  repository *before* the extraction is designed. Retrofitted justification does
  not count.
- **T2 — bidirectional conformance fixtures.** The seam ships with fixtures that
  run in IDApTIK and in the kernel and produce identical canonical bytes.
- **T3 — a recorded surprise.** At least one documented case where the naive
  boundary turned out to be wrong. `docs/ENGINE_EXTRACTION_NOTES.md` is exactly
  this kind of log; the CAC section extends it.

Until then this code is IDApTIK's, and improving it here is the correct place to
spend effort.

Because it is being grown for eventual extraction, it is held to the kernel's
fixed-point rule now rather than converted later: the
`cac-kernel-is-fixed-point` clause in
`.machine_readable/contractiles/must/must.k9.ncl` is executed by
`scripts/must_check.sh` and fails the build if `vsm.rs` acquires an `f32` or
`f64`.

Superseding this ADR is the mechanism by which extraction happens. It is not
paperwork to be skipped: a supersession must state which of T1, T2 and T3 fired
and cite the evidence.

## Consequences

- A deliberate non-decision becomes a recorded one. Non-extractions are the
  decisions that get silently reversed by whoever next reads the ownership table
  and takes it as an instruction.
- The ownership table in `esm-contract-and-repo-boundaries.md` §5 is a
  destination, not a description of the present tense. That document is amended
  to say so.
- `actor/belief.rs` (6 float occurrences), `trace.rs` (3) and `agents.rs` (1)
  are outside the invariant today. Each must become fixed-point before it feeds
  the ledger. Widen the check as they convert; do not widen it first.
- Effort spent hardening `vsm.rs` in place is not wasted work that will be
  thrown away on extraction — under T2 the fixtures written here become the
  conformance suite.

**Real-game status:** the evidence ledger runs in the Ghost Lobby scenario
behind an opt-in supervisor adapter, on this branch. It has one caller, no
second consumer, and no cross-repository fixture. None of T1, T2 or T3 has
fired.
