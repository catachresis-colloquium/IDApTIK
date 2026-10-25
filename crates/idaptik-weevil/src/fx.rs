//! Q16.16 fixed point, the kernel's only arithmetic (ADR-0006).
//!
//! The kernel does integer maths exclusively so a replay is bit-exact across
//! platforms, compilers and injection modes. This module is the host's side of
//! that contract: convert a world value into the ABI's `ee_fx` and back, with
//! saturating conversion so a host can never produce an out-of-range value that
//! would make the kernel's comparisons meaningless.
//!
//! Range: an `i32` Q16.16 value spans ±32768.0 with a resolution of 1/65536.
//! `FX_ONE` (65536) is exactly 1.0 and fits comfortably.

use crate::abi::FX_ONE;
use serde::{Deserialize, Serialize};

/// A two-component fixed-point vector.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fx2 {
    pub x: i32,
    pub y: i32,
}

impl Fx2 {
    /// The zero vector.
    pub const ZERO: Fx2 = Fx2 { x: 0, y: 0 };

    /// Magnitude squared, in fx² units (no square root: the kernel never needs
    /// one, and avoiding `libm` keeps the host float-free too).
    pub fn mag2(self) -> i64 {
        let x = self.x as i64;
        let y = self.y as i64;
        x * x + y * y
    }

    /// The component with the larger absolute value, for diagnostics.
    pub fn dominant_axis(self) -> Axis {
        if self.x.abs() >= self.y.abs() {
            Axis::X
        } else {
            Axis::Y
        }
    }
}

/// Which axis a direction is dominated by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// Convert a finite `f64` to Q16.16, saturating at the `i32` bounds.
///
/// `f64::NAN` maps to 0 rather than poisoning the snapshot: a host that produced
/// a NaN has a bug, but the kernel must still receive a well-formed number, and
/// silently dropping the observation is the host's job higher up.
pub fn to_fx(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    let scaled = v * FX_ONE as f64;
    if scaled >= i32::MAX as f64 {
        i32::MAX
    } else if scaled <= i32::MIN as f64 {
        i32::MIN
    } else {
        scaled as i32
    }
}

/// Convert a Q16.16 `i32` back to `f64`.
pub fn from_fx(v: i32) -> f64 {
    v as f64 / FX_ONE as f64
}

/// Clamp a Q16.16 value to `[lo, hi]`. Total: `hi < lo` returns `hi`.
pub fn clamp_fx(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// The unit vector toward `target` from `origin`, in Q16.16.
///
/// A degenerate (zero-length) direction returns the zero vector rather than
/// dividing by zero: the kernel treats a zero bearing as "no direction", which
/// is the honest answer when there is nothing to point at.
pub fn bearing_to(origin: Fx2, target: Fx2) -> Fx2 {
    let dx = target.x - origin.x;
    let dy = target.y - origin.y;
    let mag2 = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
    if mag2 == 0 {
        return Fx2::ZERO;
    }
    // Normalise with an integer square root on the squared magnitude. The
    // kernel never needs a true unit vector — it only compares and copies — so
    // a rounded normalisation is exact enough and keeps the host integer-only.
    let mag = isqrt(mag2);
    if mag == 0 {
        return Fx2::ZERO;
    }
    Fx2 {
        x: ((dx as i64 * FX_ONE as i64) / mag) as i32,
        y: ((dy as i64 * FX_ONE as i64) / mag) as i32,
    }
}

/// Integer square root (floor). Bit-twiddling, no `libm`.
pub fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fx_round_trips_small_values() {
        for v in [-12.5, -1.0, 0.0, 0.25, 1.0, 3.75, 1200.0] {
            assert_eq!(from_fx(to_fx(v)), v, "v={v}");
        }
    }

    #[test]
    fn fx_saturates_rather_than_wrapping() {
        assert_eq!(to_fx(1e12), i32::MAX);
        assert_eq!(to_fx(-1e12), i32::MIN);
        assert_eq!(to_fx(f64::NAN), 0);
        assert_eq!(to_fx(f64::INFINITY), i32::MAX);
    }

    #[test]
    fn bearing_is_unit_length() {
        let b = bearing_to(Fx2 { x: 0, y: 0 }, Fx2 { x: 3, y: 4 });
        // (3,4)/5 scaled to Q16.16.
        assert_eq!(b.x, 39321); // 0.6 * 65536
        assert_eq!(b.y, 52428); // 0.8 * 65536
    }

    #[test]
    fn degenerate_bearing_is_zero() {
        let b = bearing_to(Fx2 { x: 7, y: 7 }, Fx2 { x: 7, y: 7 });
        assert_eq!(b, Fx2::ZERO);
    }
}
