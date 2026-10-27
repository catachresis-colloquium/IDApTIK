//! The proof kernel: the part of IDApTIK that a theorem prover can reach.
//!
//! # Why this crate exists
//!
//! Creusot 0.13 can only prove a crate that satisfies three conditions, each
//! measured on this codebase rather than assumed (issue #121):
//!
//! 1. **No derived `PartialEq`.** `creusot-std` gives `core::cmp::PartialEq` an
//!    `extern_spec` postcondition, so every impl owes a refinement proof. A
//!    derived impl carries no contract of its own, which reduces that
//!    obligation to `forall result: bool. result = (a.deep_model() =
//!    b.deep_model())` -- universally quantified over `result` with no premise,
//!    and therefore false. Measured: `Goal Coma.refines: X`.
//! 2. **No derived `Serialize`.** It translates, but does not prove
//!    (`vc_serialize: X (24/28)`).
//! 3. **No float in any compared type.** A float field *can* be given a
//!    `DeepModel` -- `type DeepModelTy = (f64, bool)` type-checks -- but the
//!    body obligation is then unprovable because the contract is false: IEEE
//!    `==` is false for `NaN` vs `NaN` while logical `=` on the model is true.
//!    That is a correctness wall, not a tooling gap, and `#[trusted]` there
//!    would be *unsound* rather than merely unproven.
//!
//! `idaptik-core` violates all three in quantity -- roughly 64 float-carrying
//! `PartialEq` derivers and `Serialize` throughout -- so whole-crate proof is
//! unreachable, not merely expensive. Rather than carry a proof harness that
//! can never run (the vacuous gate this estate keeps re-learning about), the
//! provable part lives here and `idaptik-core` carries no Creusot machinery at
//! all.
//!
//! # What that costs, stated plainly
//!
//! `PartialEq` is hand-written here instead of derived. This is not a
//! `cfg(creusot)` twin of the shipped impl -- it *is* the shipped impl, so the
//! proof is about the code that runs. The serde derives are the opposite: they
//! are compiled out under `cfg(creusot)`, so serialization sits outside the
//! proof boundary. Rows PD-03 to PD-05 in `crates/CREUSOT-PROOF-DEBT.tsv`
//! record that class -- one per derive name, which is what makes the
//! ledger gate an exact bijection with what is on disk.

#![forbid(unsafe_code)]

#[cfg(creusot)]
extern crate creusot_std;

pub mod contrast;
pub mod rng;
pub mod trace;

pub use rng::Mulberry32;
pub use trace::{Alert, Trace};
