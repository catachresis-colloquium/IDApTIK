# ADR-0009: Enaction kernel dependency direction

- Status: Accepted
- Date: 2026-07-27
- Relates to: ADR-0003 (engine-agnostic core), ADR-0007 (versioned UMS package
  boundary), `docs/ENGINE_EXTRACTION_NOTES.md`

## Context

Four repositories now hold pieces of one platform: IDApTIK, Chronicles of
Slavia, Universal Modding Studio, and the Enaction Engine. Until now the
relationships between them existed only as prose, in four documents that agreed
with each other and bound nothing.

`docs/architecture/esm-contract-and-repo-boundaries.md` §5 assigns kernel event
and state semantics to the Enaction Engine. `docs/ENGINE_EXTRACTION_NOTES.md`
records the interpolation layer as extraction provenance rather than a
dependency. Neither states the conditions under which IDApTIK would actually
link the kernel, so the absence of a dependency reads as an oversight rather
than a decision.

The estate has now settled a two-layer shape: the Enaction Engine is the
runtime kernel a shipped game links; Universal Modding Studio is the authoring
studio that emits packages. They are separate repositories because a released
game must build and run with neither UMS source nor UMS binaries present.

## Decision

IDApTIK is a **proving ground** for the Enaction kernel. The dependency
direction is one-way and conditional.

IDApTIK may take a build dependency on an `enaction-*` crate when that crate:

1. satisfies the Enaction determinism contract, including byte-identical
   canonical encoding across the targets IDApTIK ships;
2. ships conformance fixtures with expected outputs, which IDApTIK vendors with
   a recorded digest; and
3. has been extracted under the Enaction extraction trigger, not merely
   published.

IDApTIK never depends on `idaptik-ums` or on `slavia-core`, in any
configuration, including dev-dependencies and feature-gated paths. UMS consumes
`contracts/idaptik/v1/*.json` as **data**, by path; that
`ums_profiles::compile_idaptik()` takes an `--idaptik-root` argument rather than
a Cargo dependency is the architecture, not an implementation detail.

The Elixir session relay orders and relays envelopes. It never becomes semantic
authority: canonical ordering is carried in the event envelope and reduced by
the authoritative runtime.

IDApTIK publishes `idaptik/esm/v1` as a profile over the shared kernel. A
profile supplies propositions, actor roles and transition policy; it does not
redefine kernel field meaning.

The local half of this boundary is enforced by the `no-authoring-dependency`
invariant in `.machine_readable/contractiles/must/must.k9.ncl`, executed by
`scripts/must_check.sh` against this repository's own manifests so the gate
runs without a sibling repository present. The cross-repository half lives in
`idaptik-ums/scripts/check-architecture-boundaries.sh`.

## Consequences

- The absence of an Enaction dependency becomes a recorded state with stated
  exit conditions, rather than an unexplained gap.
- Conditions 1–3 are currently unmet by every `enaction-*` crate, so no
  dependency is added by this ADR.
- Vendoring fixtures with digests means a kernel change that alters behaviour
  fails IDApTIK's build rather than drifting silently.
- Anyone tempted to convert `--idaptik-root` into a Cargo dependency to "clean
  up" the compiler must supersede this ADR first.

**Real-game status:** IDApTIK links no `enaction-*` crate today. The Enaction
Engine's workspace does not currently build. This ADR fixes direction; it does
not report an integration.
