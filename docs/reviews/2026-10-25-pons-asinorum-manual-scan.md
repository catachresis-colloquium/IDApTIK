# pons-asinorum scan of IDApTIK — manual application, 2026-10-25

> **Note (2026-10-25, after this record was written):** the four ADRs that
> shared number 0013 were given distinct numbers on `main` — 0013
> (water/gas/airflow), 0016 (electrical), 0017 (Evil Weevil), 0018
> (interaction rules). Filenames and numbers below are as they stood when this
> was written.

**Branch:** `arena/65b9fd51-idaptik` @ `7076154` (#181)
**Tool:** `hyperpolymath/pons-asinorum` @ `main`
**Status:** ⚠️ **I could not execute the scanner.** What follows is a faithful
manual application of its rule set, not a tool run. Details in §1.

---

## 1. Why there is no tool output

`pons` is a Rust binary (`crates/pons-cli`). In this environment:

| Route | Result |
|---|---|
| `cargo` / `rustc` present | **no** — nothing on PATH, no `~/.cargo`, no `/usr/local/rustup` |
| `static.rust-lang.org` reachable | **no** — `SSL_ERROR_SYSCALL`, connection refused by the egress allowlist |
| GitHub Releases binary | **none** — the repo has zero releases |
| CI build artifact | **none** — only `scorecard-results` artifacts exist |
| PyPI / npm package | **404 / 404** |

So I read all ten rule sources (`crates/pons-rules/src/t0/*.rs` and the
`*.scm` matchers) and reimplemented each predicate against this tree. Treat
every result below as **T0 (heuristic)** by pons's own taxonomy. None of it is
a proof, and none of it is the tool's output.

## 2. What `pons` checks

Ten registered rules across four evidence tiers. Only findings from a real
dataflow or typestate analysis get T1/T2; a syntactic match is T0 and is
never counted toward `--fail-on`.

| # | Rule | Tier | Rust grammar? |
|---|---|---|---|
| 1 | `div-by-literal-zero` | T0 | yes |
| 2 | `constant-condition` | T0 | yes |
| 3 | `self-assignment` | T0 | yes |
| 4 | `while-true-no-break` | T0 | yes |
| 5 | `empty-effect-loop` | T0 | yes |
| 6 | `unreachable-after-jump` | T0 | yes |
| 7 | `swallowed-error` | T0 | **no** — multi-language only |
| 8 | `string-concat-in-loop` | T0 | **no** — multi-language only |
| 9 | `dead-store` | T1 | **no** — Python CFG only |
| 10 | `read-before-init` | T1 | **no** — Python CFG only |

Its three *categories* are the useful frame: **wasted work** (4, 5, 8, 9),
**contradiction** (1, 2, 3), **missing escape hatches** (6, 7).

## 3. Results — six applicable rules, zero findings

Scope: 197 Rust files excluding `vendor/`, plus the 10 under `vendor/` scanned
separately. Strings and comments masked before matching, because unmasked
regexing produced phantom hits on `"{}"` format strings, `// 0` comments and
`"idaptik-signal/0"` literals — none of which the AST rules would fire on.

| Rule | Findings | Note |
|---|---|---|
| `div-by-literal-zero` | **0** | masked scan; all 10 raw hits were comments or `"…/0"` strings |
| `constant-condition` | **0** | no `if true` / `while false` |
| `self-assignment` | **0** | 5 raw hits were all field writes (`node.display = display`) — the rule's left side is `(identifier)`, which excludes `field_expression` |
| `while-true-no-break` | **0** | all 15 `loop {` sites have a reachable `break`/`return` |
| `empty-effect-loop` | **0** | 2 raw hits are `impl std::error::Error for X {}` — empty impls, not loops |
| `unreachable-after-jump` | **0** | no statement follows `return`/`break`/`continue` in the same block |

`vendor/` (10 files): also 0.

**Interpretation.** This is not evidence that the code is clean; it is
evidence that pons's *designed* checks find nothing here, which was
predictable — `clippy` runs in CI and subsumes most of the T0 set
(`self_assignment`, `while_immutable_condition`, unreachable code). Four of
the ten rules cannot fire on this repo at all: two have no Rust grammar and
two are Python-only, and the repo contains no Python by policy.

## 4. The contradiction category, applied to the docs

Here pons's second category earns its keep. The tool scans code, but the
*shape* of its contradiction rule — "the program asserts two incompatible
things at once" — is exactly what a design document set does when it
describes a system that was not the one built.

### 4.1 The type with the same name has a different shape

`InfrastructureDefinition` exists in both. They are not the same type.

| Field in docs | Field in code |
|---|---|
| `format: InfraFormat` | `format: String` |
| `ruleset_version: u16` | — |
| `materials: Vec<MaterialDef>` | — |
| `gases: Vec<GasDef>` | — |
| `compartments` | `compartments` ✅ |
| `links: Vec<FluidLinkDef>` | `openings: Vec<OpeningDef>`, `water`, `air` |
| `entities: Vec<EntityDef>` | — |
| `controllers: Vec<ControllerDef>` | — |
| `rules: Vec<RuleId>` | — |
| `budgets: CascadeBudgets` | `tuning: InfrastructureTuning` |

Six of ten fields differ. Two are shared.

### 4.2 Eighteen of nineteen promised identifiers do not exist

| Identifier | docs | code |
|---|---|---|
| `full_depth_mm`, `aperture_cm2`, `weir` | yes | **0** |
| `RuleDef`, `RuleId`, `Capability`, `AccessReq` | yes | **0** |
| `precedence`, `spoof`, `LockOut` | yes | **0** |
| `Irreversible`, `RateLimit`, `ReadoutRef`, `CausalTag` | yes | **0** |
| `MaterialDef`, `GasDef`, `FluidLinkDef`, `InfraFormat` | yes | **0** |

The docs specify a rule engine, a capability model, an actuator-precedence
ladder and a geometry-derived depth model. The shipped module has none of
them; it has three fixed-order integer systems.

### 4.3 The cue vocabulary is disjoint

| | Count | Members |
|---|---|---|
| Catalogue cues | 20 | `WaterRunning`, `WaterSlosh`, `PumpCavitation`, `ArcCrackle`, `GasReek`, … |
| `CueKind` variants | 10 | `Puddle`, `Spray`, `Hiss`, `Haze`, `PressureWhine`, `FilterLoaded`, `Ignition`, `Gurgle`, `Scent`, `Noise` |
| **Shared** | **1** | `Hiss` |

### 4.4 Consequence: neither level can be built

- `infrastructure-level-1.md` references **11** cues — **0** exist in `CueKind`.
- `infrastructure-level-2.md` references **6** cues — **0** exist in `CueKind`.

The levels, as written, cannot run against the shipped code. Making them run
means adding 19 `CueKind` variants or rewriting both levels.

### 4.5 What did *not* turn out to be a defect

Checked and cleared, so it is not carried forward as a false finding:

- **Rule id citations.** I initially flagged `SA-4` as cited-but-missing in
  three documents. It is **my own bug**: the regex matched the SPDX string
  `CC-BY-SA-4.0`. All rule citations in the level docs resolve against the
  catalogue's 36 rules.
- **"Eleven properties".** The catalogue preamble says every rule fills
  eleven properties and then lists eleven; every rule table has ten rows.
  The eleventh is the merge of *causal events* and *regression tests* into
  one row. All eleven properties are present. Wording imprecision, not a
  contradiction.
- **Enums.** `GasClass` (`Haze`/`Hush`/`Ember`), `Tier`
  (`Clear`/`Trace`/`Warning`/`Critical`) and the `WaterLedger` fields all
  match ADR-0013 exactly.

## 5. ANSWER: what is at risk in the ~3 000 lines

The deletion target from the disposition memo, audited line by line:

| Document | Lines | Verdict after this scan |
|---|---|---|
| `infrastructure-interactions.md` | 1 238 | **Mixed.** §2.4 (the `RuleDef` container), the capability set and the precedence ladder are the only specification of 18 unimplemented identifiers. The rest restates the shipped module with a different type shape. |
| `infrastructure-rule-catalogue.md` | 906 | **Keep.** 36 rules, each with 10 fields, all still unimplemented. This is the only place the intended behaviour exists. Nothing in the code encodes it. |
| `infrastructure-m0-loader.md` | 349 | **Delete.** `load_infrastructure`, the validation report and the round-trip tests all shipped. |
| `infrastructure-m0-acceptance-tests.md` | 690 | **Delete mostly.** Written against the unbuilt model; the shipped test suite covers the same ground. Extract only assertions the shipped tests do not make. |
| `infrastructure-level-1.md` | 519 | **Keep as design intent, not as a spec.** Unbuildable today (0/11 cues). It is the only statement of the Level 1 teaching goal. |
| `infrastructure-level-2.md` | 589 | **Same.** Unbuildable today (0/6 cues). |
| `0013-infrastructure-interaction-rules.md` | 134 | **Renumber or fold.** Four files now claim ADR-0013 on `main`. |

**The honest bottom line.** Deleting the ~3 000 lines does not lose duplicate
documentation of shipped code — that is the part the memo called duplicated,
and it is duplicated. What it loses is the **specification of everything that
was never built**: 18 identifiers, 36 rules, 20 cues, and the geometry-derived
depth model. Those are not restatements of `sim.rs`; they are the list of
things `sim.rs` does not do.

Under pons's own framing that is **wasted work** in the precise sense: 3 000
lines labouring toward an implementation that a different design replaced. The
work is not wrong; it is unspent. Deleting it discards the backlog. Keeping it
unlabelled invites the next reader to believe the engine works that way.

**Recommendation:** do not silently delete. Re-label the surviving documents
as *unimplemented design* — a forward-looking backlog with an explicit
`not_simulated` header, which is the discipline `model_declaration()` already
applies in code — and delete only the two documents the code has superseded
(`m0-loader`, `m0-acceptance-tests`) plus the restatement half of
`interactions.md`.

## 6. Confidence

Every finding in §3 is **T0**. §4.1–4.3 are mechanical counts over the tree
and can be reproduced with `grep`; §4.4 follows from §4.3. §5 is judgement,
and it is the part a reviewer should argue with.
