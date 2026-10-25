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

## Data formats and gate migration

Owner direction (25 October 2026) sets `metadatastician/kyq` as the intended
single data-format gate interface: preserve TOML and Dhall, migrate YAML/YML
content to KYAML, and migrate JSON and its variants (including JSONL/NDJSON and
JSON-LD) to the `hyperpolymath/ijson-jcs` representation through kyq. A filename
extension alone establishes none of these properties. JSONL retains record
framing and JSON-LD retains its context and graph semantics; canonical JSON
bytes alone do not establish RDF dataset canonicalisation.

This target does not turn `.deed`, Nickel `.k9.ncl`, or the coordination `.k9`
protocol into generic configuration: their authorities remain the grammars
and contracts above. Their consumers must keep validating those contracts.

**Deployment prerequisite:** kyq main `4370c76` still reports build/test as
unimplemented, and `src/Kyaml/Ijson.affine` and `src/Kyaml/Jcs.affine` contain
unconditional-success validation stubs. It cannot yet serve as a blocking
format gate. Do not alias yq to kyq, accept a placeholder validation result,
or claim that the repository has completed the kyq migration. First obtain a
pinned executable with tested TOML, Dhall, KYAML and I-JSON/JCS support,
including negative tests, then replace the transitional parser calls.

Until that prerequisite clears, the affected CI and lock-coverage checks read
YAML through pinned mikefarah/yq, as required by the currently published
`3-practice/YAML-POLICY.adoc` Y-1. They compare parsed values, so KYAML flow
syntax is supported. This is transitional enforcement, not the final kyq
interface. The rewritten `.mergify.yml` uses KYAML while retaining Mergify's
required filename. Workflow conversion awaits the required lock-coverage
context specified by D280. Existing unconverted files remain explicit
migration backlog.

kyq is limited to those four format families. Wider format projection belongs
to the planned **kyX** route (renamed from kcX by D334), not to new kyq formats.
Standards describes kyX as a projection compiler for DEED/K9/coordination and
other representations; no available kyX repository or executable was verified
in this review. Do not invent a repository URL or treat the plan as deployed.

The new Rust ruleset payload is validated and canonicalised with
`hyperpolymath/ijson-jcs` revision
`3a9a4ee326a968a32b2f152b4faec243a2d00765` (`fix`, then `check`): strict
I-JSON, JCS bytes, one trailing LF. The versioned simulation wire format is
not silently rewritten by this configuration change.

Launcher authority is `launcher/launcher-standard_praxis.deed` (standard
0.6.0) and its paired prose at `docs/UX-standards/launcher-standard.adoc` in
the pinned standards revision. Metadata migration does not establish launcher
conformance. Existing player-flow and launcher fixtures continue to apply;
the process-liveness fixture needs a host with a functioning `ps`.

## Retired metadata

The migration completed on 2026-10-25. The former A2ML manifests, state files, contractile files, and embedded launcher metadata were classified rather than suffix-renamed. Descriptive facts were consolidated into the project deed; policy clauses moved to the six K9 components; agent instructions moved to root `coordination.k9`. The old A2ML files, k9iser wrapper, parser/generation recipes, and active discovery instructions are removed. The prior WSLg Copy Mode recovery material remains in the existing troubleshooting, player-flow, and launcher-manual sources.

When the standards repository publishes a newer normative grammar, contract, validator, or coordination parser, update this pin, this document, and the conformance checks together.
