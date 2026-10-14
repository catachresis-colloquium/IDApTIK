//! Render-time interpolation: double-buffered visual state, sampled with a
//! fractional alpha so a fixed-rate simulation renders smoothly on a display
//! that does not share its rate.
//!
//! # Why this exists
//!
//! The simulation advances in whole ticks at [`TICK_DT`](crate::scenario::TICK_DT)
//! (60 Hz). A frontend renders whenever the display asks it to. If a renderer
//! reads the live simulation state every frame, a 144 Hz display shows the same
//! position for a varying number of frames and motion visibly judders. The fix
//! is to keep the last two simulated states and draw somewhere between them.
//!
//! # The invariant
//!
//! **Interpolation is render-only. No value produced here may ever flow back
//! into simulation state.** Everything in this module is a pure function of two
//! already-simulated states; nothing in it is reachable from
//! [`GhostLobbySim::tick`](crate::scenario::GhostLobbySim::tick). Determinism
//! (ADR-0004) depends on that separation, and the `replay_determinism` and
//! `snapshot_equivalence` suites are what hold it.
//!
//! # Zero allocation
//!
//! [`DoubleBuffer`] is two inline fixed-size arrays. `commit` is a copy. There
//! is no `Vec`, no `Box` and no allocation on any path here, so nothing in the
//! per-frame or per-tick path can stall on the allocator.
//!
//! # Extraction note
//!
//! This module is deliberately at the crate root rather than under `scenario/`:
//! [`Pose`], [`Blend`] and [`DoubleBuffer`] know nothing about IDApTIK and are
//! intended to lift out wholesale. Only [`VisualSlot`], [`poses_of`] and
//! [`door_opens_of`] are game-specific. See `ENGINE_EXTRACTION_NOTES.md`.

use crate::scenario::state::RuntimeState;

/// A spatially-continuous visual pose — the part of an actor's state that is
/// meaningful to draw between two ticks.
///
/// Deliberately *not* the actor's full state: velocities, timers, meters and
/// every discrete flag stay out, because interpolating them is either
/// meaningless or actively wrong.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    /// Facing sign. Blended by [`Blending::Snap`], never lerped — see [`Blend`].
    pub facing: f64,
}

/// How a channel crosses a tick boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blending {
    /// Linear interpolation between the two ticks.
    Lerp,
    /// Take the current tick's value unchanged.
    Snap,
}

/// One row of the interpolation contract.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelSpec {
    pub name: &'static str,
    pub blending: Blending,
}

/// The interpolation contract, as inspectable data rather than as branching
/// buried in a render function.
///
/// This is the whole rule set: anything not listed is not interpolated. Reading
/// this table tells you what smoothing does to the picture without reading any
/// of the code that performs it, and it is the artefact that transfers when
/// this subsystem is generalised.
pub const CHANNELS: &[ChannelSpec] = &[
    ChannelSpec {
        name: "pose.x",
        blending: Blending::Lerp,
    },
    ChannelSpec {
        name: "pose.y",
        blending: Blending::Lerp,
    },
    // Facing is a sign, not a position. Lerping it passes through zero, which
    // renders as the actor briefly facing neither way mid-turn.
    ChannelSpec {
        name: "pose.facing",
        blending: Blending::Snap,
    },
    ChannelSpec {
        name: "door.open",
        blending: Blending::Lerp,
    },
];

/// A value that can be blended across a tick boundary.
pub trait Blend: Copy {
    /// Blend `prev` toward `curr` by `alpha`.
    ///
    /// Implementations must be **exact at the endpoints**: `alpha <= 0.0`
    /// returns `prev` bit-for-bit and `alpha >= 1.0` returns `curr`
    /// bit-for-bit. Anything approximate produces a visible pop at every tick
    /// boundary, which is the artefact interpolation exists to remove.
    fn blend(prev: Self, curr: Self, alpha: f64) -> Self;
}

/// Endpoint-exact, clamped linear interpolation.
///
/// Both halves of this are load-bearing, and both were arrived at by measuring
/// rather than by reasoning:
///
/// * **The endpoints are clamped, not computed.** `prev + (curr - prev) * 1.0`
///   is not exact in general — with `prev = -5.5, curr = 3.3` it yields
///   `3.3000000000000007`, and with `prev = 1e16, curr = 1.0` it yields `0.0`.
///   An inexact endpoint pops once per tick, which is the artefact this module
///   exists to remove.
/// * **The interior uses the one-term form.** The symmetric-looking
///   `prev * (1 - alpha) + curr * alpha` is *worse* here: with
///   `prev == curr == 42.0` and `alpha = 0.1` it returns `42.00000000000001`,
///   so a stationary object jitters in the last ulp every frame. The one-term
///   form collapses to `prev + 0.0 * alpha` and is exactly stationary.
///
/// A `NaN` alpha resolves to `curr`. It means something upstream is broken, and
/// the current tick is the safe thing to draw — propagating `NaN` into a
/// transform makes the sprite vanish instead.
#[inline]
#[must_use]
pub fn lerp(prev: f64, curr: f64, alpha: f64) -> f64 {
    if alpha.is_nan() || alpha >= 1.0 {
        curr
    } else if alpha <= 0.0 {
        prev
    } else {
        prev + (curr - prev) * alpha
    }
}

impl Blend for f64 {
    #[inline]
    fn blend(prev: Self, curr: Self, alpha: f64) -> Self {
        lerp(prev, curr, alpha)
    }
}

impl Blend for Pose {
    #[inline]
    fn blend(prev: Self, curr: Self, alpha: f64) -> Self {
        Self {
            x: lerp(prev.x, curr.x, alpha),
            y: lerp(prev.y, curr.y, alpha),
            // Snap, per CHANNELS. See the comment there.
            facing: curr.facing,
        }
    }
}

/// Two ticks of visual state, held in fixed-size inline storage.
///
/// `prev` is tick *N*, `curr` is tick *N+1*, and [`sample`](Self::sample) draws
/// between them. Nothing here allocates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DoubleBuffer<T: Blend, const N: usize> {
    prev: [T; N],
    curr: [T; N],
}

impl<T: Blend + Default, const N: usize> Default for DoubleBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Blend + Default, const N: usize> DoubleBuffer<T, N> {
    /// A buffer with both ticks at `T::default()`, so the first frame drawn
    /// before any tick has landed does not interpolate away from the origin.
    #[must_use]
    pub fn new() -> Self {
        Self {
            prev: [T::default(); N],
            curr: [T::default(); N],
        }
    }

    /// Seed both ticks with the same state.
    ///
    /// Call this once the real starting state is known, so the first rendered
    /// interval is stationary rather than a slide from `default()`.
    pub fn prime(&mut self, state: &[T; N]) {
        self.curr = *state;
        self.prev = *state;
    }
}

impl<T: Blend, const N: usize> DoubleBuffer<T, N> {
    /// Advance one tick: `prev` takes the old `curr`, `curr` takes `fresh`.
    pub fn commit(&mut self, fresh: &[T; N]) {
        self.prev = self.curr;
        self.curr = *fresh;
    }

    /// Collapse history so there is nothing left to interpolate across.
    ///
    /// Call this **after** [`commit`](Self::commit), never instead of it. On a
    /// discontinuity — a restart, a teleport — the fresh state still has to
    /// enter the buffer; `snap` then removes the stale `prev` that would
    /// otherwise be drawn as a slide across the jump. Using `snap` alone leaves
    /// the new state out of the buffer entirely and renders the *old* position
    /// for a further interval.
    pub fn snap(&mut self) {
        self.prev = self.curr;
    }

    /// The value to draw for `slot` at `alpha` through the current interval.
    ///
    /// `alpha` is clamped: `0.0` is exactly `prev`, `1.0` is exactly `curr`.
    ///
    /// # Panics
    ///
    /// If `slot >= N`.
    #[must_use]
    pub fn sample(&self, slot: usize, alpha: f64) -> T {
        T::blend(self.prev[slot], self.curr[slot], alpha)
    }

    /// The previous tick's raw value, unblended.
    #[must_use]
    pub fn prev(&self, slot: usize) -> T {
        self.prev[slot]
    }

    /// The current tick's raw value, unblended. This is what non-interpolated
    /// (discrete) reads should use.
    #[must_use]
    pub fn curr(&self, slot: usize) -> T {
        self.curr[slot]
    }
}

// ── IDApTIK-specific mapping ────────────────────────────────────────────────
//
// Everything above is engine-generic. Everything below knows about Ghost Lobby
// and is the part that does *not* lift out unchanged.

/// The interpolated actors and props, in buffer-slot order.
///
/// Kept as an explicit enum rather than an index so adding a slot cannot
/// silently shift an existing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum VisualSlot {
    Player = 0,
    Billy = 1,
    Note = 2,
    Usb = 3,
    Vacuum = 4,
}

impl VisualSlot {
    /// This slot's index into a [`DoubleBuffer`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Number of interpolated pose slots — see [`VisualSlot`].
pub const N_VISUAL_SLOTS: usize = 5;

/// Maximum doors an interpolated scenario may declare.
///
/// Ghost Lobby has 4. The buffer is fixed-size to keep the zero-allocation
/// guarantee, so this is a real ceiling rather than a hint; [`door_opens_of`]
/// asserts against it in debug builds and truncates in release rather than
/// panicking mid-render.
pub const MAX_DOORS: usize = 16;

/// The pose buffer type for this game.
pub type PoseBuffer = DoubleBuffer<Pose, N_VISUAL_SLOTS>;
/// The door-openness buffer type for this game.
pub type DoorBuffer = DoubleBuffer<f64, MAX_DOORS>;

/// Extract the interpolatable poses from a simulation state.
///
/// Only continuous, position-like quantities are read. Discrete state
/// (`hidden`, `crouching`, `BillyMode`, visibility, colour) is deliberately
/// absent: a renderer reads those live from the current tick.
#[must_use]
pub fn poses_of(state: &RuntimeState) -> [Pose; N_VISUAL_SLOTS] {
    let mut out = [Pose::default(); N_VISUAL_SLOTS];
    out[VisualSlot::Player.index()] = Pose {
        x: state.player.x,
        y: state.player.y,
        facing: state.player.facing,
    };
    out[VisualSlot::Billy.index()] = Pose {
        x: state.billy.x,
        y: state.billy.y,
        facing: state.billy.facing,
    };
    out[VisualSlot::Note.index()] = Pose {
        x: state.note.x,
        y: state.note.y,
        facing: 0.0,
    };
    out[VisualSlot::Usb.index()] = Pose {
        x: state.usb.x,
        y: state.usb.y,
        facing: 0.0,
    };
    out[VisualSlot::Vacuum.index()] = Pose {
        x: state.vacuum.x,
        y: state.vacuum.y,
        facing: 0.0,
    };
    out
}

/// Extract each door's openness, indexed to match `state.doors`.
///
/// Slots beyond the scenario's door count stay at `0.0` and are never read by
/// the renderer, which iterates real doors.
#[must_use]
pub fn door_opens_of(state: &RuntimeState) -> [f64; MAX_DOORS] {
    debug_assert!(
        state.doors.len() <= MAX_DOORS,
        "scenario declares {} doors, but the interpolation buffer holds {MAX_DOORS}; \
         raise MAX_DOORS",
        state.doors.len()
    );
    let mut out = [0.0; MAX_DOORS];
    for (slot, door) in state.doors.iter().take(MAX_DOORS).enumerate() {
        // A picked lock leaves its door standing open.
        out[slot] = if door.unlocked { 1.0 } else { door.open };
    }
    out
}
