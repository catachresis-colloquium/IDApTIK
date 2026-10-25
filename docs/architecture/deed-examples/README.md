# Historical DEED design example for the building-infrastructure proposal

This directory contains illustrative DEED material from the 2026-10-24
building-infrastructure design proposal. It is **not active repository
metadata or policy**. IDApTIK's machine-readable migration completed on
2026-10-25, and the active sources now have distinct roles:

- `IDApTIK_chora.deed` — descriptive project and component state;
- `.machine_readable/contractiles/<verb>/` — six K9 policy components with
  paired Nickel runner descriptors;
- root `coordination.k9` — agent coordination instructions;
- `0-AI-MANIFEST.deed` — pointer-only startup entry point.

The K9 components and current DEED are the sources of truth. The praxis DEED
below is retained only to document the older design discussion; do not use it
as a policy source or copy its clauses into active metadata without translating
them to the current K9 contract.

## What is here

- `idaptik-building-authoring_praxis.deed` — a historical `praxis-deed`
  illustration of building-authoring and validation rules. It is not part of
  the active policy contract.
- A historical repo-deed snippet remains in
  [the building-infrastructure design document](../building-infrastructure-design.md).
  The live project deed takes precedence over that sketch.

## Conformance

The pinned standards revision is recorded in
[`docs/standards-conformance.md`](../../standards-conformance.md). To lint this
historical example with the current DEED grammar:

```
node <pinned-standards>/1-formats/deed/tools/deed_lint.js \
  idaptik-building-authoring_praxis.deed
```
