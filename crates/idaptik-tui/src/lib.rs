//! The reusable surface of `idaptik-tui`.
//!
//! This library exposes the pieces other crates consume — the script/replay
//! file format ([`script`]), the CLI → [`idaptik_core::RunConfig`] mapping
//! ([`config`]), and the headless runner ([`headless`]) whose `event_log` +
//! `debrief` + `final_snapshot` blob is the determinism artifact the whole
//! estate compares against.
//!
//! `crates/idaptik-net` (ADR-0006) reuses all three so a networked seat runs
//! *exactly* the reference pipeline: same script expansion, same sim
//! construction, same output shape — byte-identical when lockstep holds. Its
//! interactive netplay client also reuses the terminal face ([`render`]) and
//! the input pipeline ([`keymap`], [`input`]) so a networked seat *plays*
//! exactly like the single-player TUI. Cargo forbids the cycle
//! `idaptik-tui (bin) → idaptik-net → idaptik-tui (lib)`, which is why the
//! netplay frontend lives in `idaptik-net` and borrows these modules, not the
//! other way round.

pub mod config;
pub mod headless;
pub mod input;
pub mod keymap;
pub mod render;
pub mod script;
pub mod tutorial;
