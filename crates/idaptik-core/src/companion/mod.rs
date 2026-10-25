//! Moletaire — the robotic mole companion, ported deterministically from the
//! archived ReScript game (`src/app/companions/Moletaire*.res`).
//!
//! The canonical semantics is the **pure ReScript** (not the wasm
//! "coprocessor bridge" mirrors — those carry a deliberately replicated
//! `can_carry_fragile` bug; the rule here is `level >= Enhanced`).
//!
//! # Shape
//!
//! * [`CompanionDefinition`] — declarative content (the full tuning table,
//!   hunger model, equipment items, coprocessor effect ladders, chiptune
//!   pattern data). Pure serde data; round-trips through JSON
//!   ([`MOLETAIRE_JSON`] is the committed golden) and self-validates via
//!   [`CompanionDefinition::validate`] / [`CompanionDefinition::ok`].
//! * [`moletaire`] — builds the canonical definition from the archive's
//!   constants.
//! * [`MoletaireSim`] — the deterministic, dt-stepped simulation: typed
//!   [`MoleCommand`]s in, typed [`MoleEvent`]s out, one seeded
//!   [`crate::scenario::rng::Mulberry32`] RNG stream for the delivery eat
//!   roll and the starving wander (no `Math.random`, no wall clock).
//!   Snapshots via [`MoletaireSnapshot`] (format tag
//!   `idaptik-moletaire-runtime-v1`).
//! * [`hunger`] — the gravity-based hunger model (inverse-square pull,
//!   behaviour bands, per-level configs).
//! * [`CoprocessorBay`] — the five-slot computational augment bay with the
//!   Stock/MK-I/MK-II/MK-III ladders.
//! * [`Equipment`] — the seven single-slot items plus the save-string codec
//!   with the legacy aliases (`miniglider`, `flash`, `camera`).
//! * [`music`] — the training-ground chiptune loop as pure pattern data plus
//!   [`music::schedule_notes`], the pure look-ahead step scheduler.
//!
//! # Integration seam — hosted by a floor that declares a mole
//!
//! The companion stays a standalone, self-contained module: a floor that
//! hosts the mole drives it through the typed boundary only ([`MoleCommand`]
//! in, [`MoleEvent`] out) and folds the events into its own world. The host
//! for scenario floors lives in `crate::scenario::sim` (ADR-0014); nothing in
//! this module reaches into scenario state, so the companion remains reusable
//! and deterministic on its own. Per the archive `GameLoop`:
//!
//! * [`MoleEvent::SynthSoundPlayed`] — spawn the diversion at the mole's
//!   position; security dogs within **250 px** investigate it;
//! * [`MoleEvent::VibrationDetected`] — feeds the hacker's (Q's) view; the
//!   overseer polls [`MoletaireSim::read_vibrations`], no world mutation;
//! * [`MoleEvent::UndergroundScanComplete`] — drives the mole's own scan
//!   overlay.
//!
//! Rendering is not ported: frontends draw from [`MoletaireSim::view`]
//! (state/facing ordinals, `visual_y = y + depth * 60`, the archive body
//! palette) and collide with [`MoletaireSim::body_rect`].

pub mod coprocessors;
pub mod definition;
pub mod equipment;
pub mod hunger;
pub mod music;
pub mod sim;

pub use coprocessors::{
    ALL_COPROCESSOR_TYPES, CoprocessorBay, CoprocessorType, Level, VibrationReading,
};
pub use definition::{
    COMPANION_FORMAT, COMPANION_ID, CompanionDefinition, CompanionValidationError,
    CoprocessorLadders, EquipmentDef, HungerDef, MOLETAIRE_JSON, MOLETAIRE_SNAPSHOT_FORMAT,
    MoleTuning, MusicDef, moletaire,
};
pub use equipment::{ALL_EQUIPMENT, Equipment};
pub use hunger::{EdibleObject, HungerBehaviour, HungerConfig};
pub use sim::{
    BodyRect, Facing, MoleCommand, MoleEvent, MoleParams, MoleRuntimeState, MoleState,
    MoleViewState, MoletaireSim, MoletaireSnapshot,
};
