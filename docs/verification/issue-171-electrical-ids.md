<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->
# Issue #171: electrical identity and integration integrity

Status: source repair already on main; regression and merge-policy hardening
in this change. Administrative ruleset activation remains outstanding.

## Causal record

The linked [comment](https://github.com/catachresis-colloquium/IDApTIK/issues/171#issuecomment-6391562283)
is a Linear backlink. The actual [issue](https://github.com/catachresis-colloquium/IDApTIK/issues/171)
reports unresolved imports of `PowerSourceId`, `FeedId`, `BreakerId`, `LoadId`
and `HazardId`.

| Evidence in Git history | Consequence |
| --- | --- |
| `a73f433` / #162 adds electrical IDs to `scenario/ids.rs` | Canonical definitions originally existed. |
| `7ef2d06` / #167 replaces their declarations while adding water/gas/airflow IDs | Electrical consumers retain imports; the integrated tree cannot compile. |
| `b6a1a24` / #176, `fbbc6d3` / #175 and `7076154` / #181 restore declarations independently | The final tree has three copies of each electrical type. |
| `ae6125e` / #190 removes two redundant blocks | Exactly one definition remains. |
| `02c1bb5313fce31d3e6d116ac6328bd0f130f372` / #195 | Audited starting main; Rust and all other reported post-merge checks succeed. |

The old comment saying the types were never defined was incorrect; it is
replaced by the verified history. Open PR #186 also includes an older duplicate
repair alongside persistence changes. Its description is not evidence that
current main still needs those declarations restored. Reconcile its persistence
work separately against current main; do not reapply an obsolete ID repair.

The source defect was an integration failure, not a missing electrical solver.
Ordinary Rust compilation detects both the missing and duplicate declarations.
The larger process defect is allowing integration before the relevant checks
have succeeded against the current base.

## Contract and evidence boundary

1. `scenario::ids` defines each content ID exactly once. Power, building and
   infrastructure code import that authority.
2. Electrical sources, feeds, breakers, loads and hazards are distinct nominal
   Rust types, also distinct from building circuits, signal devices,
   infrastructure equipment and bare strings. No aliases may collapse them.
3. On the existing v1 JSON boundary these IDs remain strings. Changing both
   serializer and deserializer to another representation is a wire break even
   if a round-trip still succeeds.
4. Constructing an ID does not prove it exists, is unique, is reachable or is
   authorized. Definition validation and command admission retain those duties.
5. A successful integration check must refer to the latest integration base.
   Formatting, a scanner result, an old branch's tests or an empty set of
   failures does not establish compilation of the proposed integrated tree.

`power_network.rs` tests pairwise type identity and property-tests the v1
representation over arbitrary Rust strings, including rejection of non-string
JSON values. The module documentation has a compiling real `PowerCommand`
example and three compile-fail examples of domain substitution. Existing power
tests continue to cover validation, permissions, arbitration, snapshots and
replay. These are compiler and test guarantees, **not an SMT/Creusot proof of
the electrical simulation or a physical circuit model**. Creusot's existing
kernel proof scope and proof-debt ledger are unchanged.

`just test-ghost` now selects the whole workspace except Bevy, with `--locked`,
for tests and all-target Clippy. This includes the previously omitted kernel
and persistence test targets, plus FFI linting, and admits future headless
consumers automatically. Bevy retains its existing native-dependency CI gate.
The established check-run name is retained for compatibility.

## Source-controlled merge protection

Mergify requires positive results for the Rust gate, policy fixtures and lock
sync, plus `#commits-behind = 0`. Optional third-party pending checks do not
block it indefinitely. `tests/ci_security_config_test.sh` derives required
names from the workflows and tests omitted conditions, renamed jobs,
substring substitutions, and an unquoted hash that would become a YAML comment.

The ruleset payload requires those same checks from the observed GitHub Actions
app (`15368`), on the default branch, with strict current-base validation and
no bypass actors. Offline fixtures reject disabled/evaluate enforcement,
non-strict validation, empty required checks, a wrong app, bypass actors and
excluding main. These tests validate the payload, not the live GitHub settings.

## Administrative activation still required

Read on 25 October 2026, including parent rulesets:

| Active ruleset | ID | Relevant contents |
| --- | --- | --- |
| Require-Linear-History | 24258419 | Linear history; no build requirement. |
| Secret-Scan-Floor | 24310239 | Only `scan / gitleaks`, non-strict; repository-admin bypass. |
| Immutable-Tags | 22941894 | Tag protection, not a Rust integration gate. |

The connected GitHub API exposes ruleset reads but no administrative write
operation. Committing the payload does not enable it. With an administrator's
GitHub CLI, inspect the list first to avoid creating duplicates, then create
the missing rule or update the existing rule by its returned ID:

```sh
gh api repos/catachresis-colloquium/IDApTIK/rulesets
gh api --method POST repos/catachresis-colloquium/IDApTIK/rulesets \
  --input .github/rulesets/rust-integration.json
# If Rust-Integration-Floor already exists, use PUT .../rulesets/ITS_ID instead.
gh api repos/catachresis-colloquium/IDApTIK/rules/branches/main
```

Preserve the existing rulesets. Activate the new floor after its named checks
have succeeded on the proposed change. Confirm readback includes all three
checks, strict validation and no bypass list on the new ruleset. Exercise a
throwaway PR with a deliberate compile error and confirm merging is blocked;
then exercise a stale-base PR. Do not merge either negative fixture. This is
the remaining acceptance boundary for claiming prevention across merge paths.

References: [GitHub rules API](https://docs.github.com/en/rest/repos/rules),
[required-check semantics](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets),
and [Mergify conditions](https://docs.mergify.com/configuration/conditions/).

## Verification record

See the pull request for the exact tested head, executed commands and hosted
checks. The initial local build exhausted disk; that attempt is not a passing
baseline. Local retries use the pinned Rust 1.97.1 and locked dependencies, an
external target directory, two build jobs, no incremental compilation and no
debug symbols. Test assertions, optimization levels and floating-point semantics
are unchanged. No golden files are regenerated.
