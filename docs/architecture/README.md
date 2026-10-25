# Architecture documents

IDApTIK documents describe stable interfaces, ownership, determinism, and
integration boundaries.

- [**Estate status**](ESTATE-STATUS.md) — start here: where IDApTIK sits in the
  four-repository estate, what is enforced rather than written down, and what is open
- [ESM contract and repository boundaries](esm-contract-and-repo-boundaries.md)
- [IDApTIK cognitive-model working architecture](idaptik-cognitive-model.adoc)
- [Observer-relative evidence ledger](evidence-ledger-design.adoc)
- [UMS package contract ownership](ums-package-contract.md)
- [Infrastructure implementation status](infrastructure-implementation-status.md)
  — **start here**: what the engine does not do, identifier by identifier
- [Infrastructure interactions](infrastructure-interactions.md) — **not
  implemented**: the rule layer, capability set and precedence ladder the
  shipped `scenario/infrastructure/` module does not have (ADR-0018)
- [Infrastructure rule catalogue](infrastructure-rule-catalogue.md) — the eleven
  specified properties for every electricity/water/gas/tunnel/control interaction
- [Repository architecture](../ARCHITECTURE.md)

## Where cognitive-model work is documented

Cognitive-model internals are documented here, in the open, alongside the code
that implements them.

An earlier rule sent detailed internals to a private canonical UMS repository
and kept only the consumable contract here. That rule is withdrawn: it split
one design across two repositories, left the most detailed specification in the
estate untracked in the less-maintained of the two, and made the boundary
harder to check rather than easier. See ADR-0011.

The engine is AGPL-3.0-or-later and the project is built to stay open. A design
that cannot be published is a design that cannot be reviewed.
