# The proof ledger: every proof obligation this repository speaks about, in
# the estate's closed status vocabulary. Contracts: proof-ledger-schema.ncl.
# Rendered by `just proof-ledger-regen` into docs/status/proof-ledger.json
# (I-JSON/JCS) and docs/status/PROOF-STATUS.adoc; `just proof-ledger-check`
# fails when either has drifted from this file.
#
# A receipt names the CI job that checks the row and the run and commit it was
# last observed passing on. It is evidence that the check ran, not a promise
# that it still passes at a later commit: the job re-checks on every push.
#
# Every row here is an engineering obligation (it names a function, type or
# file of this repository), so no Proof Witness Grade applies (PROOF-POLICY
# §8.1). There are no theory obligations in this repository today.

let S = import "proof-ledger-schema.ncl" in

# The commit every receipt was observed on: c6b519d (PR #222), the first head
# with this ledger, on which every job named below passed (CI run
# 42629156912, Creusot run 42629156882).
let observed = { sha = "c6b519d5e8e75dc8b5b754f119589abb99f2814e", ci_run = 42629156912 } in
let creusot_receipt = {
  workflow = ".github/workflows/creusot-proof.yml",
  job = "Creusot proof",
  run = 42629156882,
  commit = observed.sha,
} in
let ci = fun name => {
  workflow = ".github/workflows/ci.yml",
  job = name,
  run = observed.ci_run,
  commit = observed.sha,
} in
let creusot_checker = "Creusot 0.14.0 (65349025c67e69fcd7479f1f420fa5904f69bc27), Pearlite contracts, Why3 with alt-ergo, cvc4, cvc5 and z3 as provisioned by creusot-install; proof sessions in verif/" in
let creusot_boundary = "Safe Rust in crates/idaptik-kernel only; the Rust compiler, Creusot's translation and the SMT provers are trusted; callers outside the kernel are not verified." in
let idris_checker = "Idris2 0.8.0 (Chez backend), %default total, escape scan, proof mutants; scripts/abi_model_check.sh" in
let idris_boundary = "A model of the C ABI, not the Rust implementation in idaptik-ffi; see BR-ABI-MODEL-TO-RUST." in

{
  schema_version = "1",
  policy = {
    repository = "metadatastician/standards",
    path = "3-practice/PROOF-POLICY.adoc",
    commit = "9bdf864cc7f2c8e24b4df84bf00d04158eee3ad5",
  },
  rows = [
    # --- Creusot: the proof kernel (crates/idaptik-kernel) -------------------
    {
      id = "CR-RNG-NEW",
      obligation = "Mulberry32::new starts from exactly the seed (its deep model is the seed).",
      kind = "engineering", verifier = "creusot", status = "proven",
      where = "crates/idaptik-kernel/src/rng.rs:Mulberry32::new",
      checker = creusot_checker, trust_boundary = creusot_boundary, receipt = creusot_receipt,
    },
    {
      id = "CR-RNG-NEXT",
      obligation = "Mulberry32::next_u32 is free of panics and overflow for all 2^32 states. No functional contract, by design (PD-06).",
      kind = "engineering", verifier = "creusot", status = "proven",
      where = "crates/idaptik-kernel/src/rng.rs:Mulberry32::next_u32",
      checker = creusot_checker,
      trust_boundary = "Absence of run-time errors only; the output sequence is pinned by the unit test seed_123456_u32_vector, not proved. %{creusot_boundary}",
      receipt = creusot_receipt,
    },
    {
      id = "CR-RNG-EQ",
      obligation = "Mulberry32's hand-written PartialEq is equality of deep models.",
      kind = "engineering", verifier = "creusot", status = "proven",
      where = "crates/idaptik-kernel/src/rng.rs:PartialEq for Mulberry32",
      checker = creusot_checker, trust_boundary = creusot_boundary, receipt = creusot_receipt,
    },
    {
      id = "CR-TRACE-NEW",
      obligation = "Trace::new starts at zero progress with the given threshold.",
      kind = "engineering", verifier = "creusot", status = "proven",
      where = "crates/idaptik-kernel/src/trace.rs:Trace::new",
      checker = creusot_checker, trust_boundary = creusot_boundary, receipt = creusot_receipt,
    },
    {
      id = "CR-TRACE-ADVANCE",
      obligation = "Trace::advance never moves the threshold, never passes it, and an active intrusion short of the threshold always gains ground (the anti-stall guarantee).",
      kind = "engineering", verifier = "creusot", status = "proven",
      where = "crates/idaptik-kernel/src/trace.rs:Trace::advance",
      checker = creusot_checker,
      trust_boundary = "The CI job's mutant control (deleting the .max(1)) must fail on Coma.vc_advance. %{creusot_boundary}",
      receipt = creusot_receipt,
    },
    {
      id = "CR-TRACE-ACCESSORS",
      obligation = "Trace::traced, progress and threshold return exactly what they name.",
      kind = "engineering", verifier = "creusot", status = "proven",
      where = "crates/idaptik-kernel/src/trace.rs:Trace::{traced, progress, threshold}",
      checker = creusot_checker, trust_boundary = creusot_boundary, receipt = creusot_receipt,
    },
    {
      id = "CR-TRACE-EQ",
      obligation = "The hand-written PartialEq for Trace and for Alert is equality of deep models.",
      kind = "engineering", verifier = "creusot", status = "proven",
      where = "crates/idaptik-kernel/src/trace.rs:PartialEq for Trace, Alert",
      checker = creusot_checker, trust_boundary = creusot_boundary, receipt = creusot_receipt,
    },
    {
      id = "CR-KERNEL-DERIVES",
      obligation = "The derived Clone, Debug and Eq impls in the kernel are free of panics.",
      kind = "engineering", verifier = "creusot", status = "proven",
      where = "verif/idaptik_kernel_rlib/{rng,trace}/impl_{Clone,Debug,Eq}_*",
      checker = creusot_checker, trust_boundary = creusot_boundary, receipt = creusot_receipt,
    },
    {
      id = "CR-CONTRAST-LUMINANCE",
      obligation = "contrast::luminance is exactly 2126 R + 7152 G + 722 B for channels at most 10^6, and at most 10^10.",
      kind = "engineering", verifier = "creusot", status = "proven",
      where = "crates/idaptik-kernel/src/contrast.rs:luminance",
      checker = creusot_checker,
      trust_boundary = "The CI job's mutant control (one weight off by one) must fail on Coma.vc_luminance. %{creusot_boundary}",
      receipt = creusot_receipt,
    },
    {
      id = "CR-CONTRAST-MEETS",
      obligation = "contrast::meets returns exactly (lighter + 5*10^8) * den >= (darker + 5*10^8) * num, in either argument order, without overflow, for luminances at most 10^10 and 0 < den, num <= 10^6.",
      kind = "engineering", verifier = "creusot", status = "proven",
      where = "crates/idaptik-kernel/src/contrast.rs:meets",
      checker = creusot_checker,
      trust_boundary = "The CI job's mutant control (the offset at 1/21) must fail on Coma.vc_meets. That this integer test is WCAG's ratio test is BR-CONTRAST-RATIO. %{creusot_boundary}",
      receipt = creusot_receipt,
    },

    # --- Verus: the planned second verifier (PROOF-POLICY §1.2) ---------------
    {
      id = "VE-KERNEL-RNG-TRACE",
      obligation = "Verus leg for the rng and trace contracts.",
      kind = "engineering", verifier = "verus", status = "open",
      where = "crates/idaptik-kernel/src/{rng,trace}.rs",
      checker = "Verus (not installed)",
      trust_boundary = "Nothing is claimed until a Verus run is recorded. Verus would share SMT trust with Creusot, which is recorded, not claimed as independence.",
      covers = ["CR-RNG-NEW", "CR-RNG-NEXT", "CR-RNG-EQ", "CR-TRACE-NEW", "CR-TRACE-ADVANCE", "CR-TRACE-ACCESSORS", "CR-TRACE-EQ", "CR-KERNEL-DERIVES"],
      next_action = "Provision a pinned Verus release and the Z3 it requires (4.16.0 at verus-lang/verus main on 2026-11-09), through an environment setup script or a pinned CI job with a PROOF-POLICY §6 deviation record; then state the same postconditions in a verus! harness over the shipped code and record how the statements correspond. Blocked on 2026-11-09: the agent session's permission classifier refused to run the toolchain's install script.",
    },
    {
      id = "VE-CONTRAST",
      obligation = "Verus leg for contrast::luminance and contrast::meets.",
      kind = "engineering", verifier = "verus", status = "open",
      where = "crates/idaptik-kernel/src/contrast.rs",
      checker = "Verus (not installed)",
      trust_boundary = "Nothing is claimed until a Verus run is recorded. A separately written Verus model would not prove the shipped functions; the harness must verify them or be recorded as a model.",
      covers = ["CR-CONTRAST-LUMINANCE", "CR-CONTRAST-MEETS"],
      next_action = "As VE-KERNEL-RNG-TRACE. The owner asked for a Verus spike on 2026-11-09; it is pending the toolchain.",
    },

    # --- Proof debt: what the kernel deliberately does not prove -------------
    {
      id = "PD-TRACE-FRACTION",
      obligation = "Trace::fraction (f32) has no proof: #[trusted] with an empty contract (CREUSOT-PROOF-DEBT PD-01).",
      kind = "engineering", verifier = "creusot", status = "open",
      where = "crates/idaptik-kernel/src/trace.rs:Trace::fraction",
      checker = creusot_checker,
      trust_boundary = "Creusot 0.14 has no logical model of IEEE-754. Display-only: nothing in the determinism kernel reads it.",
      next_action = "Prove it once Creusot ships a float theory, or move it behind an integer-valued API (PD-01).",
    },
    {
      id = "PD-RNG-NEXT-F64",
      obligation = "Mulberry32::next_f64 has no proof: #[trusted] with an empty contract (PD-02). The integer step it wraps is CR-RNG-NEXT.",
      kind = "engineering", verifier = "creusot", status = "open",
      where = "crates/idaptik-kernel/src/rng.rs:Mulberry32::next_f64",
      checker = creusot_checker,
      trust_boundary = "No float model in Creusot; byte-exactness of the division rests on TE-DETERMINISM and the libm pin (ADR-0004).",
      next_action = "As PD-TRACE-FRACTION (PD-02).",
    },
    {
      id = "PD-KERNEL-SERDE-HASH",
      obligation = "The kernel's Serialize, Deserialize and Hash impls are compiled out of the proof (PD-03, PD-04, PD-05).",
      kind = "engineering", verifier = "creusot", status = "open",
      where = "crates/idaptik-kernel",
      checker = creusot_checker,
      trust_boundary = "Outside the proof boundary by decision: determinism is about values, not their wire form, and Hash is in no invariant.",
      next_action = "None planned: recorded so the absence is a decision (crates/CREUSOT-PROOF-DEBT.tsv).",
    },

    # --- Bridges: model or integer law to the thing a reader cares about ----
    {
      id = "BR-CONTRAST-RATIO",
      obligation = "meets's integer test is WCAG 2's (L1 + 0.05) / (L2 + 0.05) >= num / den at luminance scale 10^10: for positive den and positive (L2 + 0.05), x / y >= n / d iff x * d >= y * n, and 5*10^8 is 0.05 at that scale.",
      kind = "engineering", verifier = "tests", status = "argued",
      where = "crates/idaptik-kernel/src/contrast.rs (module docs and meets's docs)",
      checker = "-",
      trust_boundary = "Elementary arithmetic, not machine-checked. The sweep test integer_verdict_agrees_with_the_float_formula checks it on 4000+ pairs.",
    },
    {
      id = "BR-ABI-MODEL-TO-RUST",
      obligation = "The Rust C ABI in crates/idaptik-ffi behaves as the Idris2 model describes.",
      kind = "engineering", verifier = "tests", status = "argued",
      where = "crates/idaptik-ffi/abi/README.md; zig-adapter conformance (abi-conformance.yml)",
      checker = "-",
      trust_boundary = "Header-to-model is gated (ID-HEADER-PARITY); model-to-Rust is argued and exercised by the conformance fixtures, not proved.",
    },

    # --- Idris2: the C ABI model (crates/idaptik-ffi/abi) ---------------------
    {
      id = "ID-ABI-OWNERSHIP",
      obligation = "O1: no operation takes a freed handle back to live, and every complete session frees its handle exactly once (noResurrection, completeFreesExactlyOnce).",
      kind = "engineering", verifier = "idris2", status = "proven",
      where = "crates/idaptik-ffi/abi/src/Idaptik/Abi/Protocol.idr",
      checker = idris_checker, trust_boundary = idris_boundary, receipt = ci "abi-model (Idris2 gate)",
    },
    {
      id = "ID-ABI-STRINGS",
      obligation = "O2: at every prefix of a session, strings handed out balance strings freed; a complete session frees exactly as many as it was given (sessionBalance, completeBalancesStrings).",
      kind = "engineering", verifier = "idris2", status = "proven",
      where = "crates/idaptik-ffi/abi/src/Idaptik/Abi/Protocol.idr",
      checker = idris_checker, trust_boundary = idris_boundary, receipt = ci "abi-model (Idris2 gate)",
    },
    {
      id = "ID-ABI-WIRE",
      obligation = "O3: rendered results close their delimiters, a tick result's first character identifies its shape, and a rendered snapshot holds no interior NUL (snapshotOfIsNulFree).",
      kind = "engineering", verifier = "idris2", status = "proven",
      where = "crates/idaptik-ffi/abi/src/Idaptik/Abi/Json.idr",
      checker = idris_checker, trust_boundary = idris_boundary, receipt = ci "abi-model (Idris2 gate)",
    },
    {
      id = "ID-ABI-RUN",
      obligation = "O4 and snapshots: the frame clock advances by the frame count, restore refuses a foreign format, and a snapshot restores to exactly itself (snapshotRoundTrip).",
      kind = "engineering", verifier = "idris2", status = "proven",
      where = "crates/idaptik-ffi/abi/src/Idaptik/Abi/Run.idr",
      checker = idris_checker, trust_boundary = idris_boundary, receipt = ci "abi-model (Idris2 gate)",
    },
    {
      id = "ID-HEADER-PARITY",
      obligation = "The model's exported functions are one-for-one with include/idaptik.h, and its snapshot format tag equals the Rust SNAPSHOT_FORMAT.",
      kind = "engineering", verifier = "tests", status = "tested-only",
      where = "scripts/abi_model_check.sh (steps 4 and 5)",
      checker = "Text diff in CI, on every push",
      trust_boundary = "A syntactic diff of names and one constant, not of behaviour.",
      receipt = ci "abi-model (Idris2 gate)",
    },

    # --- Tests standing where the repository says "enforced" or "proves" ------
    {
      id = "TE-DETERMINISM",
      obligation = "A run is a pure function of (definition, config, seed, command stream) (ADR-0004).",
      kind = "engineering", verifier = "tests", status = "tested-only",
      where = "crates/idaptik-core/tests: replay_determinism, snapshot_equivalence, rng_vectors, golden_reset, no_panic_fuzz",
      checker = "cargo test in CI",
      trust_boundary = "Checked on the fixed scenarios, seeds and fuzz budget those suites run, not for all inputs.",
      receipt = ci "test-ghost (core/tui/ffi + clippy + fmt)",
    },
    {
      id = "TE-NETPLAY-LOOPBACK",
      obligation = "Two seats over the loopback relay stay byte-identical.",
      kind = "engineering", verifier = "tests", status = "tested-only",
      where = "just loopback-check",
      checker = "CI two-seat gate",
      trust_boundary = "The scripted sessions the gate runs.",
      receipt = ci "session-loopback (Burble relay + two-seat determinism gate)",
    },
    {
      id = "TE-FRONTEND-PARITY",
      obligation = "The TUI and Bevy frontends stay in step over the Command/Event API.",
      kind = "engineering", verifier = "tests", status = "tested-only",
      where = "crates/idaptik-bevy/tests/parity.rs",
      checker = "cargo test in CI",
      trust_boundary = "The command scripts the test drives.",
      receipt = ci "bevy-frontend (tests + clippy)",
    },

    # --- The brand (docs/design/brand) ----------------------------------------
    {
      id = "TE-SRGB-LINEAR",
      obligation = "srgb::linear is WCAG's channel linearisation scaled to 10^6, and every one of its 256 outputs is inside luminance's precondition.",
      kind = "engineering", verifier = "tests", status = "tested-only",
      where = "crates/idaptik-brand/src/srgb.rs",
      checker = "cargo test (just brand-check) in CI",
      trust_boundary = "The precondition bound is checked over the whole u8 domain; the transfer values are checked at the endpoints, the knee and published pairs. f64 powf is the platform's.",
      receipt = ci "config-scenario-check (Nickel negative fixtures + round-trip)",
    },
    {
      id = "TE-BRAND-VERDICTS",
      obligation = "Every colour pair the frontend draws has the WCAG verdict the brand book records, and none is a live failure.",
      kind = "engineering", verifier = "tests", status = "tested-only",
      where = "crates/idaptik-brand/tests/legibility.rs; crates/idaptik-bevy/src/{palette,hud}.rs tests",
      checker = "cargo test in CI",
      trust_boundary = "The verdict itself is CR-CONTRAST-MEETS; the channels are TE-SRGB-LINEAR; the meaning is BR-CONTRAST-RATIO. Pairs within 1e-3 of a threshold are refused. Which pairs are drawn is read from the drawing code, not proved complete.",
      receipt = ci "config-scenario-check (Nickel negative fixtures + round-trip)",
      depends_on = ["CR-CONTRAST-MEETS", "CR-CONTRAST-LUMINANCE", "TE-SRGB-LINEAR", "BR-CONTRAST-RATIO"],
    },
    {
      id = "TE-BRAND-TOKENS",
      obligation = "The brand tokens satisfy brand-schema.ncl, which rejects each of the eight bad fixtures, and the committed tokens.json and generated.rs equal a fresh export.",
      kind = "engineering", verifier = "nickel", status = "tested-only",
      where = "config/brand-schema.ncl, config/brand-fixtures/, just brand-check",
      checker = "Nickel 1.16 contracts and cargo test in CI",
      trust_boundary = "Contracts are checked at export time on the committed tokens and fixtures.",
      receipt = ci "config-scenario-check (Nickel negative fixtures + round-trip)",
    },
    {
      id = "TE-PROOF-LEDGER",
      obligation = "This ledger satisfies proof-ledger-schema.ncl, which rejects each bad fixture, and the committed JSON and PROOF-STATUS.adoc equal a fresh rendering.",
      kind = "engineering", verifier = "nickel", status = "tested-only",
      where = "config/proof-ledger-schema.ncl, config/proof-ledger-fixtures/, just proof-ledger-check",
      checker = "Nickel 1.16 contracts and cargo test in CI",
      trust_boundary = "The schema checks the ledger's shape and its stated dependencies; whether a row's claim is true is the receipt's business.",
      receipt = ci "config-scenario-check (Nickel negative fixtures + round-trip)",
    },
  ],
} | S.Ledger
