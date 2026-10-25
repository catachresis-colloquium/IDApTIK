# Dev notes

Working documents for planning and coordination between the developers.
Unlike `docs/adr/` (decisions, immutable once accepted) and
`docs/level-design.md` (synthesised design canon), files here are
**living** — edit them freely, argue in them, strike things through.

## Programme and planning

- [roadmap-2026-07.md](roadmap-2026-07.md) — the programme of work
  distilled from the 2026-07-20 planning discussion, organised into
  workstreams with status, sequencing, and a gap list.
- [working-agreement-2026-09-27.md](working-agreement-2026-09-27.md) — the
  two-author trunk/frontier working agreement. A proposal: unratified, not
  policy.
- [mistral-handover-triage-2026-08-10.md](mistral-handover-triage-2026-08-10.md)
  — triage of an external "ultimate handover" pack: what was adopted, what was
  rejected (with reasons), and the owner rulings it still needs.
- [database-candidates.md](database-candidates.md) — what the game actually
  stores and which store holds it. Tier 1 shipped as
  `crates/idaptik-persist`; Tiers 2–3 are analysis, with measured numbers.

## Cognitive model

- [cognitive-model-hypothesis-2026-07-26.md](cognitive-model-hypothesis-2026-07-26.md)
  — provisional affective–cognitive–conative loop and Kautz terminology.
- [vsm-relevance-hypothesis-2026-07-26.md](vsm-relevance-hypothesis-2026-07-26.md)
  — provisional VSM mapping for regulation, autonomy, recursion, and viability.

## Toolchain

- [toolchain-authority.md](toolchain-authority.md) — the one-provider-per-tool
  policy: which file owns which toolchain, and why the four that disagreed were
  retired.
- [toolchain-setup.md](toolchain-setup.md) — putting every toolchain on a
  machine system-wide, the permanent mise fix, and `mise trust`.

## Evil Weevil guard brain

- [weevil-integration.md](weevil-integration.md) — developer instructions:
  the seam between game and kernel, how to build it, and the invariant that keeps
  the boundary intact.
- [weevil-local-setup.md](weevil-local-setup.md) — the local build and setup
  guide for the vertical slice, marking which steps were actually verified.
- [weevil-play.md](weevil-play.md) — how to play it on the kernel brain and
  tell it apart from the classic FSM.

## Request handovers

- [tamper-testbed-handover-2026-10-25.md](tamper-testbed-handover-2026-10-25.md)
  — **closed.** Handover for the physical-tampering testbed request: a
  source-backed audit of what exists, the gap table, a staged M0–M6 plan, a first
  device/action/observation matrix, and the owner rulings still needed. Read §0a,
  which reconciles the audit with the tree after the same day's merges. Nothing
  in the request was implemented.
