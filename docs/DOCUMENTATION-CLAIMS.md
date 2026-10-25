<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

# Documentation claims — intent vs. achieved state

This repository was bitten three times in a single review by documents that
were more confident than their implementations, all in one file
(`docs/abi/ABI-AUTHORITY.md`, issue [#119]):

1. A recorded digest that was simply **false**, carrying the claim "unchanged
   by the rename" when the file *had* changed.
2. A CI step **named** "diff against `docs/abi/ABI-AUTHORITY.md`" whose script
   never opened that file. The digests were hardcoded, so the document and the
   gate could drift apart with nothing to notice.
3. Prose saying CI "rejects drift", which invites the reading that the gate
   notices upstream movement. It cannot: every check fetches at a *fixed*
   revision, and bytes at a fixed commit are immutable.

Each was caught by a human reading carefully. That does not scale, so the
mechanical parts are now gated by `scripts/doc_claims_check.sh`, and the parts
that cannot be mechanised are written down here as a convention.

The distinction that matters is between a **claim** (this is true of the
repository as it stands) and an **intent** (this is what we mean to do). Both
are legitimate. Only one of them has to be true today.

## The rules

### 1. A digest, count or status is generated or checked — never both hand-written and load-bearing

Any 64-hex digest written into `docs/` must also appear in a workflow or a
script, so that some gate knows the value exists. **Checked** by
`doc_claims_check.sh` (check 1).

The check is set inclusion — every digest under `docs/` is also somewhere
under `.github/workflows/` or `scripts/`. It deliberately does *not* try to
prove that a gate re-derives the value, because that is not decidable from the
text; re-derivation is the gate's own job, and `abi-conformance.yml` does it
with `sha256sum` against a pinned revision. What this check forbids is the
case that actually bit us: a hash recorded as fact in a document and known to
nothing else.

### 2. A step name is a claim

If a workflow step's name says it does something to a named file, the step
must reference that file. **Checked** by `doc_claims_check.sh` (check 2).

The rule is scoped by grammar, and the scoping was measured before it was
written. The naive form — "a step name containing a filename must reference
it" — flags 12 steps on this repository, of which **11 are legitimate**:
`rustup show` genuinely reads `rust-toolchain.toml` without naming it. A gate
that is 91% false positives gets switched off in a week.

The discriminator is the **parenthesis**, because it marks a different speech
act:

| Form | Reading | Example | Gated? |
|---|---|---|---|
| Filename in the main clause | a **claim** about what this step does to the file | `Re-derive pinned digests and diff against docs/abi/ABI-AUTHORITY.md` | yes — the body must reference it |
| Filename in parentheses | a **provenance** note: where the values came from | `Provision pinned Rust toolchain (rust-toolchain.toml)` | no |

So if a step merely takes its inputs *from* a file without operating on it,
put the filename in parentheses and it reads as provenance.

A verb allowlist (`diff|check|verify|…`) was considered and rejected: in the
main clause every name is already a claim, so the verbs never fire; and in the
parenthesised branch they create false positives — `Check formatting
(rustfmt.toml)` would match on "check" and fail an honest step.

### 3. An indicative claim about CI names the thing that runs

Write "CI rejects X" only when some job, step or script actually rejects X,
and **name it**, so a reader can go and look:

> Determinism is enforced by tests: `replay_determinism`, `snapshot_equivalence` …
> — `docs/ARCHITECTURE.md`

> The local half of this boundary is enforced by the `no-authoring-dependency`
> clause in `.machine_readable/contractiles/must/must.k9.ncl`, executed by
> `scripts/must_check.sh`.
> — `docs/adr/0009-enaction-kernel-dependency-direction.md`

Both are good: the claim is indicative, and it carries its own address. A bare
"this is verified in CI" is not, because nothing can be checked against it and
nothing fails when it stops being true.

Be equally explicit about what a gate does **not** do. `ABI-AUTHORITY.md` has
a section headed "What the digest gate does and does not verify" for exactly
this reason — it is the cure for defect 3 above.

### 4. Intent is marked as intent

A document may describe a shape that does not exist yet, provided a reader
cannot mistake it for a description of the tree. Mark it inline:

```
**(intended)** `crates/idaptik-fyrox` — the Fyrox frontend, not yet scaffolded.
```

Use `**(intended)**` for a thing that is planned, and `**(proposed)**` for an
option still under discussion. A paths-exist check is *not* currently enforced
(see "Deliberately not gated" below), so this marker is for the reader — but
it is also what makes such a check possible later, which is why the form is
fixed now rather than invented then.

Referring to a path in **another repository**, always prefix the repository:
`idaptik-ums/scripts/check-architecture-boundaries.sh`, not the bare path. A
bare `scripts/…` reads as a promise about *this* tree.

## Running the gate

```sh
scripts/doc_claims_check.sh              # check this repository
scripts/doc_claims_check.sh --self-test  # 13 controls, 7 of them mutants
```

Both run in CI on every push and pull request, in the `fixtures-and-must` job.
The self-test runs *first*: a gate that cannot fail is worse than no gate, so
each check has a mutant that must kill it, and every extraction has an
emptiness guard, because two empty sets compare equal. Each check prints its
denominator — how many digests, how many step names, how many were exempt as
provenance — so a reader can see what was examined rather than trusting a bare
"ok".

## Deliberately not gated

**Repo-local paths in documents are not checked for existence.** This was
measured rather than assumed: of 101 backticked path-like tokens under
`docs/`, 43 resolve to this repository and 4 of those do not exist. On
inspection two are legitimate intent (`crates/idaptik-fyrox`, which ADR-0003
already marks "scaffolded in a later change") and two are artefacts of the
extraction (a cross-repository path whose first segment happens to exist here,
and an abbreviated `tests/parity.rs` for the real
`crates/idaptik-bevy/tests/parity.rs`). At roughly a 50% false-positive rate
on its own findings the check is not ready to be mandatory. Rules 4's markers
and the repository-prefix convention are the groundwork; the check is tracked
separately rather than shipped noisy.

[#119]: https://github.com/metadatastician/IDApTIK/issues/119
