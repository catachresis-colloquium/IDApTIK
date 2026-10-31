<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->
# IDApTIK Rust/Creusot ultraplan

Date: 2026-10-31. Baseline: `dfe0f7b4591c1fc4f96bdaa762f39995ee3c9b57`.
Status: proposed work programme, not a verification certificate or implementation claim.

## 1. Objective and non-negotiable distinctions

First independently reproduce the existing kernel evidence. Then migrate the
remaining first-party game Rust to an architecture in which gameplay decisions
have machine-checked contracts, progressively composing those contracts into a
whole-tick invariant. Use pons-asinorum to prioritise contradictions, dead work
and missing failure paths, not as a substitute for a theorem prover.

Creusot is a verifier for Rust, not a replacement implementation language.
Adding attributes, moving a file into the kernel, translating it to Why3, passing
a scanner, and discharging a functional contract are five different events.

The goal is **all first-party game code accounted for**, with functional proofs
for game truth and explicit, minimised boundaries for framework calls, floats,
serialization, unsafe FFI and I/O. Full implementation verification of every
renderer/database/network interaction is not currently feasible with the pinned
Creusot stack. Do not achieve a nominal 100% by trusting bodies or shrinking the
denominator. If literal whole-program Creusot verification is required, keep it
an unmet research objective until tool support and boundary proofs exist.

Third-party libraries, `vendor/`, the external Burble session service, and old
game incarnations are outside the migration. First-party crates named libraries
are inside it: that is where this game's implementation lives. Zig and Idris
remain boundary technologies; this plan does not rewrite them into Rust.

## 2. Baseline evidence and reproducibility

Physical lines, including comments, blanks and inline tests:

| Inventory | Files | Lines |
|---|---:|---:|
| First-party Rust under `crates/*/src` | 175 | 63,049 |
| First-party Rust including separate tests/examples | 226 | 78,280 |
| Existing `idaptik-kernel/src` | 4 | 611 |

611 / 63,049 = 0.97% is a **proof-boundary size**, not verified-line coverage.
The kernel includes UI contrast code, tests, and unverified bodies. Count
production gameplay, presentation/support, and tests separately in future reports.

Executed in this session:

- `./scripts/creusot_ledger_check.sh --self-test`: 12 passed, 0 failed.
- `./scripts/creusot_ledger_check.sh all`: exact classification bijection,
  226 files / 226 rows; exact proof-debt bijection, six escapes / six rows.

Not executed: compilation, game tests, Creusot translation/proof, or pons scans.
`cargo`, `rustc`, `just`, `mise`, and `pons` were not found on PATH. Internet
access here cannot provision the usual Rust distribution host. No pons releases
were returned; the recent artifact listing contained reports, not a scanner
binary. This is an environment blocker, not a green check.

Existing evidence inputs:

- `.github/workflows/creusot-proof.yml`: Creusot source commit
  `318615be3b8bbc60d1f6d52469ba5c0bdebed4f1`, version 0.13.0,
  proof nightly `nightly-2026-06-22`.
- Normal project toolchain: Rust 1.97.1. Preserve separate toolchains; do not
  assume the proof nightly can compile the entire Bevy dependency graph.
- `crates/CREUSOT-CLASSIFICATION.tsv`, `crates/CREUSOT-PROOF-DEBT.tsv`,
  `verif/idaptik_kernel_rlib/`, and the kernel source.
- `docs/PROJECT-ASSURANCE-PROFILE.adoc` contains older statements about no
  discharged proofs and absent adapters. Reconcile through a dedicated evidence
  review; do not silently upgrade its assurance labels as part of refactoring.

## 3. Workstream A — verify what already claims Creusot coverage

### A0. Reproducible proof environment — blocking

Use a provisioned runner with the existing workflow's exact compiler, Creusot,
Why3 and prover setup. Record source SHA, dirty status, dependency lock hash,
compiler versions, prover versions, command lines and runner platform. Make the
first replay against the baseline; rerun on each proposed integration head.

Do not update tools while establishing the baseline. If existing pins fail,
retain that failure and isolate toolchain repair into its own reviewed change.
The workflow remains the authority for installation commands.

### A1. Replay positive and negative controls

1. Run both ledger commands above.
2. Run `cargo creusot -p idaptik-kernel --only coma --span-mode relative`.
3. Run `cargo creusot --only prove`; require a nonzero obligation/file
   denominator and record every discharged, failed and timed-out obligation.
4. Reproduce the workflow's `Trace::advance` anti-stall mutant: remove the
   final `.max(1)`. It must still translate, then fail the named advance proof.
   A compilation failure is not the required negative control.
5. Restore the exact source; translate and prove again.
6. Run `cargo test -p idaptik-kernel --lib` and applicable integration tests.
7. Retain logs and proof sessions as CI artifacts, with small durable summaries
   in the repository. Never interpret an old committed proof session as a new run.

Exit gate: clean baseline proves; mutant fails for the intended obligation;
restored baseline proves; kernel tests pass. Until then, no expansion claims.

### A2. Review specifications and six debt entries

For each public operation, identify the actual caller preconditions, termination,
panic/overflow freedom, functional result, state frame, and any assumptions.
Check satisfiability and construct reachable witnesses; contradictory preconditions
must not make the proof vacuous. Track generated trait obligations separately.

- `Trace`: threshold/progress bounds, completion, anti-stall, unchanged threshold.
- PRNG: seed/state relation, all-u32-domain safety. Explore an exact bitvector or
  mathematical wrapping-step contract for `next_u32`; retain PD-06 until that
  contract really proves. Tests alone do not retire it.
- Contrast: bounds and weighted-integer/ratio predicates; report separately from
  game-truth coverage so UI arithmetic cannot inflate gameplay assurance.
- PD-01/02 float helpers: quarantine callers and investigate exact conversion
  semantics; neither trusting the body nor assuming real-number arithmetic closes
  these debts.
- PD-03/04/05 serde/hash: trace the runtime dependency boundary; prove relevant
  value-level relations before claiming wire-level reproducibility.

Each invariant family gets a falsifier: wrong threshold comparison, omitted
clamp, wrong seed update, or changed contrast coefficient. Require a translated
mutant to fail its intended functional obligation, not just a random later goal.

Deliverable: reviewed contract matrix and baseline proof report, with unchanged
or explicitly retired debt rows supported by evidence.

## 4. Workstream B — pons scan and refactor triage

### B0. Actual scanner execution — blocking for scan-derived claims

Reviewed upstream at commit `84f58f910de7020b79450f2c2643599fe7760f73`
(`hyperpolymath/pons-asinorum`). Its live registry contains 13 rules: ten T0
and three T1. The README explicitly says source-derived typestate is planned;
`pons trace` checks supplied event traces, not arbitrary Rust executions.
The older manual report, `docs/reviews/2026-10-25-pons-asinorum-manual-scan.md`,
is historical evidence, not an executable scan of the current checkout.

On a Rust-equipped runner, build/test `pons-cli` from that exact upstream commit
in a separate tool checkout, outside this repository's tracked source. Record
`pons --version`, `pons catalogue`, binary hash, and scanner test results.
Verify each selected rule's Rust applicability from the current catalogue/source;
Python/PHP dataflow support does not establish Rust dataflow coverage.

Build an explicit staging inventory of tracked first-party `.rs` files preserving
relative paths. Assert it matches all 226 classification rows; do not scan a
mixed assets tree and then hide skipped inputs. Scanner metadata and evidence
must retain originals' paths and hashes. Keep production and tests/examples as
separate partitions, and combine their inventories for completeness.

For each partition (substitute its actual directory for `$RUST_SCAN_ROOT`):

```sh
pons scan "$RUST_SCAN_ROOT" --format json --no-speculative \
  --require-language rust --fail-on-skips > pons-report.json
pons validate-report pons-report.json
pons scan "$RUST_SCAN_ROOT" --format sarif --no-speculative \
  --require-language rust --fail-on-skips > pons-report.sarif
```

Capture the scanner exit code before report validation; a valid report can still
contain coverage failures or findings. Treat zero files, parse failures, missing
required coverage and unexpected skips as incomplete evidence. Generated Rust
must be identified separately, not silently removed. Review JSON/SARIF parity.
Run a known Rust positive and negative fixture to show the selected checks fire.
Retain raw reports as artifacts, not large generated files in Git.

No fresh findings are claimed here. The current blockers must be resolved before
publishing a finding count. In particular, do not repeat the old report's zero
findings as if they described this run.

### B1. Finding handling

For every finding record: rule and evidence tier; path/span and source hash;
applicability; minimal reproducer; true/false positive rationale; intended
behaviour; proposed contract; regression test/mutant; owner and disposition.

Priorities: contradictions that violate an invariant, missing failure/escape
paths that permit bad transitions, then measurable dead work. Heuristic findings
are review prompts. No automated semantic rewrite or blanket suppression.
Promote a hypothesis only after source review and a reproducer; retain rejected
hypotheses to avoid repeatedly rediscovering false positives.

### B2. Source-review opportunities to test, not pons findings

| Candidate and source anchors | Refactor/proof opportunity | Required caution and acceptance evidence |
|---|---|---|
| `scenario/sim/mod.rs` (2,331 lines), `sim/systems.rs` (1,340) | Extract named, narrow transition functions and explicit pre/post-tick readouts; compose a tick invariant and frame conditions | Preserve actual ordering and RNG/event consumption exactly; differential replay and swapped-phase negative control |
| `scenario/signal/sim.rs` (884), `power/sim.rs` (1,733) | Separate immutable start-of-tick observations from next-state writes; typed phase inputs; prove one-hop signal delay and interlock semantics | Existing snapshots of previous values are semantic, not redundant work; never remove them because of a superficial clone/collect match |
| Power `step` around lines 1194–1404 | Isolate arbitration into total decision functions with an explicit first-failing `OffReason`; model bounded backup/warm-up counters | Prove exact precedence, permission checks and one-event-per-transition semantics, including denied-command frame conditions |
| `scenario/infrastructure/sim.rs` (2,194), `water.rs`, `air.rs` | Extract bounded-transfer primitive and ledger updates; prove capacity and conservation with sources/sinks | Conservation needs explicit losses, combustion, external supply and sinks; saturating arithmetic can conceal loss rather than prove conservation |
| Signal `Access` and power `PowerAccess` | Share only a proven membership/seat-policy primitive where semantics genuinely match | Signal typed IDs and power's cross-element string space differ; keep domain separation and public wire strings |
| `scenario/definition.rs`, `ids.rs`, state/snapshot modules | Validated-definition constructors and typed internal indices; prove reference validity and restore reconstruction | Keep one ID authority; preserve domain separation and rejection behaviour; no replacement of checked external IDs by unchecked array indices |
| `scenario/mathf.rs` | Isolate authoritative float semantics from discrete decisions; unit-bearing bounded numeric types where justified | JS rounding, repeated time accumulation and libm are intentional; fixed point is a semantic migration, not a mechanical cleanup |
| `scenario/infra.rs`, `sim/infra.rs`, `infrastructure/sim.rs` | Map host wiring versus independent infrastructure runtime before composing proofs | Similar names do not establish duplicate functionality; verify actual tick reachability and shipped-floor activation |
| `idaptik-net`, `idaptik-persist`, `idaptik-ffi` | Pure protocol/storage/handle state machines behind thin effects adapters | Proving the state machine does not prove transport, disk durability or raw-pointer safety |
| Bevy/TUI and interpolation modules | Pure view-model derivation and command mapping; narrow effectful render shell | No presentation result may mutate authoritative simulation state; retain frontend parity |

File sizes above include inline tests/comments; splitting files by itself adds
no proof coverage. Clones, saturating operations and collection allocations are
review locations, not confirmed defects or performance findings. Benchmark
before claiming speedups. Do not invent a generic mega-rule-engine to unify
subsystems with distinct tick semantics.

## 5. Workstream C — migration waves

Each wave follows the same loop: inventory -> specify -> feasibility spike ->
behaviour-preserving extraction -> prove -> falsify -> differential tests ->
review -> integrate. Prove the shipped implementation, never a proof-only twin.

### C0. Architecture spike and contracts before mass moves

Build a dependency/call graph and a mutable-state/read-write map. Identify which
functions are actually called by each shipped floor. Classify every production
function as functional-proof, safety-proof-only, trusted, unsupported, or pending;
allow additional orthogonal tested/reviewed labels, not a misleading single green
status. Introduce an exhaustiveness check for this inventory in a later PR.

Spike three representative operations: integer transfer, signal state transition,
and bounded collection lookup. Test pinned Creusot support for chosen containers,
traits and modular contracts. Determine whether to extend `idaptik-kernel` or add
small proof-capable domain crates. Do not move the whole serde/float-heavy core
into the proof build and expect annotations to fix it.

Exit gate: real shipped call sites invoke the extracted functions; finite input
bounds have validated constructors; missing contracts fail CI; proofs reproduce
under the proof toolchain and normal builds still work under Rust 1.97.1.

### C1. Discrete arithmetic and validated data

Migrate bounded counters, cooldowns, costs, budgets, transfer limits, ID/index
lookups and finite decision tables. Prove exact saturation policy, no underflow,
valid indices, constructor invariants and denied-command nonmutation. Separate
wire DTOs from domain types only when the conversion is checked and specified.
The DTO boundary is still pending until serialization obligations are addressed.

### C2. Signal network

Migrate access checks, port compatibility, gate truth tables, rising-edge state,
wire mutation/builtin protections, budget charging and one-hop transitions.
Prove that denied operations preserve state and budget; successful commands
charge exactly the intended cost; feedback uses the declared prior state.
A two-link chain must not propagate twice in one tick. Keep event order explicit.

### C3. Power network

Migrate sources -> breakers -> feeds -> loads -> hazards in actual runtime order.
Prove equipment arbitration, backup countdown bounds, warm-up/restart policy,
prior-tick interlocks and transition-event uniqueness. Keep float body/hazard
geometry outside the initial proof boundary, explicitly, until C6 resolves it.
Test host integration separately: subsystem proofs cannot establish activation
on a shipped floor.

### C4. Water, air and infrastructure

Prove source/destination/capacity-bounded transfers first, then conservation
ledgers and hazard tiers, then composed step order. Express equations in
mathematical integers with machine-range refinements. Model explicit external
supplies, drains, filters and combustion terms; do not assert closed-system
conservation for an open system. Verify snapshot/accounting restoration.
No claim that the gameplay tick drives this runtime until a call-site and
integration test establish that fact.

### C5. Companion, network gameplay and actors

Inventory and migrate the hosted Moletaire transitions, network traversal/access,
guard decisions, alert/trace interactions, score/outcome rules and other live
first-party gameplay not already covered above. Traverse `idaptik-core/src`
beyond `scenario/` (including `companion` and `netsim`), plus the first-party
`idaptik-weevil` adapter; do not equate core coverage with Ghost Lobby alone.

Prove legal-state transitions, bounded resources and capability/seat checks.
Treat external AI/kernel proposals as untrusted input: acceptance must preserve
host invariants, rejection must not partially apply. Inventory all finite enum
cases, terminal states and optional-subsystem absence paths.

### C6. Authoritative floats — explicit decision gate

The pinned kernel documents no usable floating-point logical model for its
current proof needs. Whole-crate derived equality over float-containing types is
also problematic. Pick a route per numeric domain, not once for the entire game:

1. Keep exact IEEE/JS/libm behaviour: pursue an adequate float/bitvector theory
   and justified library specifications; until then label these functions as
   unverified boundaries, with bit-exact regression evidence only.
2. Move authoritative values to bounded integer/fixed-point units: specify range,
   scale, rounding, overflow and boundary conversions, then prove them. This
   requires an ADR and explicit replay/snapshot compatibility decision.
3. Keep floats exclusively in presentation where a proved discrete authority can
   replace them without changing intended rules; prove the authority separation.

Prototype movement/collision, time and camera-related domains separately. Test
negative halves, signed zero, NaN/infinity rejection/handling, extreme values,
long-run accumulation and supported targets. Real arithmetic is not IEEE proof.
Do not change accumulated `t += 1/60` into `tick/60` under an equivalence claim.
If behaviour changes, version rulesets and snapshot/replay formats and retain an
old replay path or explicitly reject old versions. Never silently rebaseline
all golden files. Exit gate is either semantic equivalence evidence/proof or an
approved documented break, plus proved new invariants.

### C7. State, commands, snapshots and the complete tick

With subsystem contracts in place, compose command dispatch, phase ordering,
state-frame conditions, deterministic event ordering and RNG-state transitions.
State the whole-tick invariant over validated definition/config/state/input.
Prove preservation and termination under concrete validated size bounds; keep
resource exhaustion and allocator assumptions explicit.

Prove reset/restore reconstruct derived indices and optional hosts consistently.
Prove a value-level snapshot round-trip first; separately address encoding,
format migration and rejection of malformed data. Conditional determinism of a
pure transition is not yet byte-identical cross-platform serialization.

### C8. First-party net, persistence and FFI

Net: prove seat ownership, sequence monotonicity, duplicate/stale handling,
lockstep transitions and resync state-machine properties. Retain Burble-backed
two-seat/recovery gates; the external service remains out of scope.

Persistence: prove log/checkpoint prefix and replay-resume decision logic under
explicit atomic-write/durability assumptions. Use fault-injection to test those
assumptions, not to relabel filesystem implementation as verified.

FFI/weevil: extract checked lengths, command validation and handle lifecycle
logic into safe proof-capable Rust. Connect implementation contracts to the
existing Idris ABI model and Zig conformance harness. Keep pointer provenance,
unsafe dereferences and foreign callers explicitly outside Creusot's supported
safe subset unless independently verified. Do not claim a model proves its C ABI
implementation without a refinement argument.

### C9. Frontends, brand and remaining support code

Migrate pure interpolation/view derivation, HUD choices, input-to-command mapping,
canonicalisation/validation decision logic and other first-party helpers where
supported. Frame the Bevy/TUI effect shells; external rendering frameworks remain
trusted dependencies, never proof denominator padding. Generated brand constants
must come from `config/brand-tokens.ncl` and the normal regeneration/check path.

Audit every remaining production function against the inventory. Tests, examples,
Evil Weevil corpus generation and build utilities stay classified as evidence
producers; do not count them as functionally proved gameplay. Apply pons and normal
quality gates to them. Any unsupported first-party body stays visibly pending or
boundary debt; no unaccounted remainder at project closure.

## 6. Integration gates and proof economics

Every migration PR contains:

- One invariant family and a small implementation slice; no unrelated cleanup.
- Before/after call graph and boundary/debt delta, including the exact shipped
  caller. No copied proof-only implementation.
- Contracts, satisfiability witnesses, termination arguments and frame conditions.
- Prover logs with obligation totals, timings, source/toolchain hashes, failures
  and timeouts; proof reproduction after source restoration.
- A translated negative mutant for each newly claimed invariant family.
- Relevant regression/property/fuzz tests, differential state/event/RNG/snapshot
  replay, normal compilation, formatting and clippy.
- Current-base integration evidence; never rely on an obsolete branch head.
- Classification/debt checks and updated function inventory; no silent new trust.
- Fresh scoped pons results and reviewed dispositions, not just a green exit code.

Use fast ledger/unit gates per PR, domain proofs per PR once required, and broader
cross-target replay plus expensive fuzz/proof suites on a scheduled runner as well
as before releases. Required proof contexts must not disappear through path
filters. Cache by tool/source versions, never as a substitute for a current verdict.
Use the existing `just test-ghost`, config/scenario gates, parity suites,
`just loopback-check`, Evil Weevil and ABI checks as applicable. Missing tools
fail the evidence run; a local environment inability is reported separately.

Set an initial per-domain proof budget after measuring baseline solver times.
A timeout is pending evidence, not proof. Refactor abstractions/invariants before
blindly increasing solver time. Review trusted external specs as carefully as
code: a wrong assumption can make a successful proof meaningless.

## 7. Dashboard and completion criteria

Report separately by gameplay, presentation, boundary and tests/support:

1. Inventoried production functions / total production functions.
2. Functionally proved functions / total, and safety-only / total.
3. Proved invariant families, stated preconditions and negative-control results.
4. Trusted bodies, assumed external specs, cfg-excluded bodies and unsupported
   functions, with owners and retirement conditions.
5. Authoritative float operations remaining and live caller reachability.
6. Proof times, reproducibility, timeouts and mutation rejection rate.
7. Pons scanned/parsed/skipped counts, applicable Rust rules and triage status.
8. Replay compatibility and integration-test status per supported target.

Physical line count is a secondary inventory metric only. Keep both the original
baseline and current denominator, so deletion, generated code or moving functions
cannot manufacture progress. One tiny proof per function is not functional
coverage if the specified property does not describe the function's behaviour.

Completion levels:

- L0: existing kernel independently reproduced and reviewed.
- L1: all first-party code inventoried; pons execution complete and triaged.
- L2: discrete subsystems proved at shipped call sites, float/boundary debt named.
- L3: authoritative gameplay/tick contracts composed, numeric policy resolved,
  compatibility and snapshot claims supported by their own evidence.
- L4: remaining first-party boundaries either verified with suitable techniques
  or explicitly accepted as residual risk. Do not call accepted risk a proof.

## 8. Delivery order and ownership

No calendar promise before A0/B0 and the C0 spikes establish feasibility.
Assign one accountable integration owner, a gameplay-domain reviewer per wave,
and an independent specification/proof reviewer where capacity allows. Record
unavailable review capacity rather than inventing independence.

Suggested PR sequence (split further whenever proof review becomes unwieldy):

1. Baseline reproduction report and evidence reconciliation (A0–A2).
2. Reproducible pons runner, complete inventory and scan triage (B0–B1).
3. Proof-capable dependency/representation spikes and function inventory (C0).
4. Bounded arithmetic/validated-ID pilot (C1).
5. Signal contracts and extracted transitions (C2).
6. Power arbitration and counters (C3).
7. Infrastructure transfers and accounting (C4).
8. Companion/network/actor discrete transitions (C5; several PRs).
9. Numeric-semantics ADR and isolated pilot (C6, research can start after C0).
10. Numeric migration or supported exact-float proofs (potentially many PRs).
11. State/snapshot and whole-tick composition (C7).
12. Net/persist/FFI state-machine and refinement work (C8; separate domains).
13. View-model/frontends/support remainder and closure audit (C9).

A and B may run in parallel, but no migration is called verified until A exits.
C2–C5 can overlap after C1, with a single owner for tick/state/ID authority.
C7 depends on the numeric decision and completed subsystem contracts. Stop a
wave on unexplained replay drift, vacuous contracts, increased unreviewed trust,
or unverifiable shipped-call linkage. Revert the extraction rather than weaken
the contract just to turn the gate green.

Immediate next action: obtain a provisioned proof/scanner runner and produce
PRs 1–2. This document deliberately does not pretend those executions occurred.

## 9. Cubical Agda decision

Defer adding Agda/Cubical to the implementation toolchain. Potential uses are
representation-equivalence proofs, algebraic laws for subsystem composition, or
a mathematically precise snapshot/refinement model. Cubical paths and univalence
may help with equivalences, but are not needed merely to express finite-state
invariants. First try the existing Rust/Creusot and Idris model infrastructure.

A separate Agda theorem does not prove the Rust implementation. Any future spike
must name a concrete obligation, explain why existing tools cannot discharge it,
pin Agda plus its compatible Cubical library, and define the refinement link to
shipped Rust (or explicitly label that link unverified). Do not introduce a
second handwritten implementation and count its proofs as Rust coverage. Keep
such a research job optional until its cost and implementation relevance are
measured. No Agda installation is needed for the first scanner/proof run.
