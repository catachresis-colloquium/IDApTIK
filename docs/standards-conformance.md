# Machine-readable format conformance

IDApTIK uses three distinct machine-readable artefacts:

- `IDApTIK_chora.deed` is the single descriptive repo deed: identity, project state, architecture facts, and evidence pointers.
- `.machine_readable/contractiles/<verb>/<verb>.k9.ncl` holds a contractile policy component; its sibling `<verb>.ncl` is the Nickel runner descriptor.
- Root `coordination.k9` is the agent-coordination protocol. It is not a DEED and not a K9 component.

The pointer-only `0-AI-MANIFEST.deed` provides the DEED filename dispatch and startup links. It does not duplicate the project deed's descriptive clauses.

## Pinned standards

Conformance is pinned to `hyperpolymath/standards` commit
`8e48b1caade08a552646adeab66558ac43b4360d` (2026-10-25):

- DEED grammar: `1-formats/deed/spec/DEED-GRAMMAR-SPEC.adoc` and `1-formats/deed/spec/abnf/deed.abnf` (normative DEED ABNF v1.2.0).
- K9 components: `1-formats/k9/spec/K9-CONTRACT-SPEC.adoc` and `1-formats/k9/spec/contract/k9_contract.ncl` (Nickel component contract, not an ABNF).
- Coordination: `2-protocols/k9-coordination/spec/COORDINATION-K9-SPEC.adoc` and `2-protocols/k9-coordination/spec/abnf/coordination-k9-grammar_v1.2.abnf` (coordination protocol v1.2.0).

The published K9 contract defines component envelopes and Nickel semantics. The pinned standards revision does not publish a `k9.abnf`; IDApTIK does not create a substitute grammar. The current coordination ABNF applies only to root `coordination.k9`.

`scripts/machine_readable_check.sh` fetches the pinned standards revision, runs its DEED linter and K9 validator, installs Nickel 1.18.0, typechecks the paired Nickel runners, and invokes IDApTIK's structural coordination check. CI runs this script in a dedicated conformance job. The coordination format has no reusable standards validator in the pinned revision, so the local check covers its version, required sections, field ordering, indentation, header, and DEED reference without claiming to be a general ABNF parser.

## Retired metadata

The migration completed on 2026-10-25. The former A2ML manifests, state files, contractile files, and embedded launcher metadata were classified rather than suffix-renamed. Descriptive facts were consolidated into the project deed; policy clauses moved to the six K9 components; agent instructions moved to root `coordination.k9`. The old A2ML files, k9iser wrapper, parser/generation recipes, and active discovery instructions are removed. The prior WSLg Copy Mode recovery material remains in the existing troubleshooting, player-flow, and launcher-manual sources.

When the standards repository publishes a newer normative grammar, contract, validator, or coordination parser, update this pin, this document, and the conformance checks together.
