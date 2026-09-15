// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixed-point scalars, squared distances and headings.
//!
//! Three types and one lookup table, and between them they are the reason the
//! sim can promise a bit-identical replay on Windows, Linux and macOS:
//!
//! * [`Fx`] — Q16.16 position, length or speed, backed by `i32`.
//! * [`Sq`] — Q32.32 squared distance, backed by `i64`. Range checks compare
//!   squared distances; there is no square root anywhere in this crate.
//! * [`Angle`] — a `u16` turn (65 536 units to the full turn) with a
//!   4 096-entry sine table built by an integer Taylor series, so the table is
//!   the same bytes on x86-64 and on aarch64.
//!
//! # Rules this module is held to
//!
//! * No `f32`/`f64`, anywhere, including in the table generator (AGENTS.md
//!   §4.2).
//! * No `as` casts (AGENTS.md §4.3). The handful of widening conversions that
//!   a `const fn` cannot express as `From` carry a local `#[allow]` and a
//!   comment naming why they cannot truncate; they are audited by
//!   `tests/confinement.rs` and are the only `as_conversions` allowances in
//!   the crate.
//! * Every shift on a signed value is an arithmetic shift, which **floors
//!   toward negative infinity**. That is the documented rounding of
//!   [`Fx::saturating_mul_fx`], and it is what the goldens pin.
//! * No arithmetic that can panic reaches a tick phase: the checked forms
//!   return `Option`, the saturating forms name the cap as a decision.

use core::cmp::Ordering;
use std::sync::LazyLock;

/// Fractional bits in [`Fx`].
pub const FRAC_BITS: u32 = 16;

/// Entries in the sine table. The index is `angle >> 4`.
pub const SIN_TABLE_LEN: usize = 4096;

/// One quarter turn in [`Angle`] units.
pub const QUARTER_TURN: u16 = 16_384;

/// `round(2 * pi * 2^32)`, computed once by hand and pinned.
///
/// The table generated from it is a determinism contract, so the constant that
/// generates it is one too: moving it moves every heading in the project.
const TWO_PI_Q32: i64 = 26_986_075_409;

/// Q16.16 scalar: a position along one axis, a length, a speed per tick.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Fx(i32);

impl Fx {
    /// Zero.
    pub const ZERO: Fx = Fx(0);
    /// One whole voxel.
    pub const ONE: Fx = Fx(1 << FRAC_BITS);

    /// Whole voxels to Q16.16.
    ///
    /// `i16 -> i32` cannot truncate, and a `const fn` cannot call
    /// `i32::from`, which is why this is one of the audited widening casts of
    /// AGENTS.md §4.3.
    #[must_use]
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "audited widening cast (AGENTS.md §4.3): i16 -> i32 cannot truncate, and a const fn cannot call i32::from"
    )]
    pub const fn from_voxels(v: i16) -> Fx {
        Fx((v as i32) << FRAC_BITS)
    }

    /// Build from an already-fixed-point raw value.
    ///
    /// The only way a raw `i32` becomes an [`Fx`], so grepping for
    /// `Fx::from_raw` finds every place a bare number claims a unit.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Fx {
        Fx(raw)
    }

    /// The raw Q16.16 value, for the canonical encoder and the snapshot.
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Whole voxels, rounding toward negative infinity (an arithmetic shift).
    #[must_use]
    pub const fn floor_voxels(self) -> i32 {
        self.0 >> FRAC_BITS
    }

    /// Addition that reports overflow rather than wrapping or panicking.
    #[must_use]
    pub const fn checked_add(self, rhs: Fx) -> Option<Fx> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Some(Fx(v)),
            None => None,
        }
    }

    /// Subtraction that reports overflow.
    #[must_use]
    pub const fn checked_sub(self, rhs: Fx) -> Option<Fx> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Some(Fx(v)),
            None => None,
        }
    }

    /// Addition that saturates.
    ///
    /// Use it only where the cap is a real game rule — a position clamped to
    /// the map, a meter that cannot go past full — never to make an overflow
    /// go away quietly.
    #[must_use]
    pub const fn saturating_add(self, rhs: Fx) -> Fx {
        Fx(self.0.saturating_add(rhs.0))
    }

    /// Subtraction that saturates. Same rule as [`Fx::saturating_add`].
    #[must_use]
    pub const fn saturating_sub(self, rhs: Fx) -> Fx {
        Fx(self.0.saturating_sub(rhs.0))
    }

    /// Negation that saturates at [`i32::MIN`].
    #[must_use]
    pub const fn saturating_neg(self) -> Fx {
        Fx(self.0.saturating_neg())
    }

    /// Absolute value, saturating at [`i32::MIN`].
    #[must_use]
    pub const fn saturating_abs(self) -> Fx {
        Fx(self.0.saturating_abs())
    }

    /// Q16.16 × Q16.16 with an `i64` intermediate, saturating on the way back.
    ///
    /// The shift is arithmetic, so the result **floors toward negative
    /// infinity** rather than truncating toward zero. That choice is
    /// deterministic on every target and is what the goldens pin.
    #[must_use]
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "audited widening cast (AGENTS.md §4.3): i32 -> i64 cannot truncate, and a const fn cannot call i64::from"
    )]
    pub const fn saturating_mul_fx(self, rhs: Fx) -> Fx {
        let product: i64 = (self.0 as i64).wrapping_mul(rhs.0 as i64);
        Fx(saturate_i64_to_i32(product >> FRAC_BITS))
    }

    /// Multiply by a whole number, saturating.
    #[must_use]
    pub const fn saturating_scale(self, k: i32) -> Fx {
        Fx(self.0.saturating_mul(k))
    }

    /// Clamp into `[lo, hi]`. Total, so it may appear inside a sort key.
    ///
    /// Returns `lo` when the bounds are inverted, which cannot happen for a
    /// map extent read from the rules table and is the deterministic answer if
    /// it ever does.
    #[must_use]
    pub const fn clamp_to(self, lo: Fx, hi: Fx) -> Fx {
        if lo.0 > hi.0 {
            return lo;
        }
        if self.0 < lo.0 {
            lo
        } else if self.0 > hi.0 {
            hi
        } else {
            self
        }
    }
}

/// `i64 -> i32`, saturating. Written out because a `const fn` cannot call
/// `i32::try_from` and `as` would truncate silently.
#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "audited narrowing cast (AGENTS.md §4.3): the range test above it is what makes the cast exact"
)]
const fn saturate_i64_to_i32(v: i64) -> i32 {
    if v > i32::MAX as i64 {
        i32::MAX
    } else if v < i32::MIN as i64 {
        i32::MIN
    } else {
        v as i32
    }
}

/// Q32.32 squared distance. Range checks compare `r²`; nothing takes a root.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Sq(i64);

impl Sq {
    /// Zero.
    pub const ZERO: Sq = Sq(0);

    /// The raw Q32.32 value, for the canonical encoder.
    #[must_use]
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// Squared distance between two three-vectors of Q16.16 coordinates.
    ///
    /// The intermediate is `i128` and the result saturates into `i64`: a
    /// single delta can be as wide as `2^32` raw, whose square already fills
    /// `i64`, so a map wider than the rules table allows saturates rather than
    /// wrapping. Saturation keeps the comparison monotone, which is all a
    /// range check needs.
    #[must_use]
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::indexing_slicing,
        reason = "audited widening cast (AGENTS.md §4.3): i32 -> i128 cannot truncate, and a const fn cannot call i128::from; the index is below 3 by the loop condition over a [Fx; 3]"
    )]
    pub const fn between(a: [Fx; 3], b: [Fx; 3]) -> Sq {
        let mut acc: i128 = 0;
        let mut k: usize = 0;
        while k < 3 {
            let d: i128 = (a[k].0 as i128) - (b[k].0 as i128);
            acc += d * d;
            k += 1;
        }
        Sq(saturate_i128_to_i64(acc))
    }

    /// The square of a radius, for a range check.
    #[must_use]
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "audited widening cast (AGENTS.md §4.3): i32 -> i128 cannot truncate, and a const fn cannot call i128::from"
    )]
    pub const fn of_radius(r: Fx) -> Sq {
        let d = r.0 as i128;
        Sq(saturate_i128_to_i64(d * d))
    }
}

/// `i128 -> i64`, saturating, for the same reason as [`saturate_i64_to_i32`].
#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "audited narrowing cast (AGENTS.md §4.3): the range test above it is what makes the cast exact"
)]
const fn saturate_i128_to_i64(v: i128) -> i64 {
    if v > i64::MAX as i128 {
        i64::MAX
    } else if v < i64::MIN as i128 {
        i64::MIN
    } else {
        v as i64
    }
}

/// A heading. 65 536 units to the full turn; 0 is `+x`, [`QUARTER_TURN`] is `+y`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Angle(u16);

/// `sin(2*pi*i/4096)` in Q16.16, for `i` in `0..4096`.
static SIN_TABLE: LazyLock<[i32; SIN_TABLE_LEN]> = LazyLock::new(build_sin_table);

/// `sin(x)` for `x` in Q32.32 radians over `[0, pi/2]`, result in Q32.32.
///
/// Taylor series to the `x^13/13!` term; at `x = pi/2` the first dropped term
/// is far below one Q16.16 ulp. Every operation is integer, so the table is
/// the same on every target.
#[allow(
    clippy::integer_division,
    reason = "exact-by-construction series division; the rounding is truncation toward zero and is part of the pinned table"
)]
fn sin_q32(x: i64) -> i64 {
    let xx: i128 = (i128::from(x) * i128::from(x)) >> 32;
    let mut term: i128 = i128::from(x);
    let mut acc: i128 = term;
    let mut k: i128 = 1;
    while k <= 6 {
        let d: i128 = (2 * k) * (2 * k + 1);
        term = -((term * xx) >> 32) / d;
        acc += term;
        k += 1;
    }
    saturate_i128_to_i64(acc)
}

fn build_sin_table() -> [i32; SIN_TABLE_LEN] {
    // The quarter wave, inclusive of both endpoints.
    let mut quarter = [0i32; 1025];
    for (i, slot) in quarter.iter_mut().enumerate() {
        let Ok(idx) = i64::try_from(i) else {
            continue;
        };
        // `>> 12` is `/ 4096` for a non-negative value, exactly.
        let x = (TWO_PI_Q32 * idx) >> 12;
        let v = sin_q32(x);
        // Q32.32 -> Q16.16, rounding half up.
        *slot = saturate_i64_to_i32((v + (1 << 15)) >> 16);
    }

    let mut t = [0i32; SIN_TABLE_LEN];
    for (j, slot) in t.iter_mut().enumerate() {
        let (index, negate) = if j <= 1024 {
            (j, false)
        } else if j <= 2048 {
            (2048 - j, false)
        } else if j <= 3072 {
            (j - 2048, true)
        } else {
            (4096 - j, true)
        };
        let value = quarter.get(index).copied().unwrap_or(0);
        *slot = if negate { -value } else { value };
    }
    t
}

/// The sine table, for [`Angle::from_delta`] and for the golden tests.
#[must_use]
pub fn sin_table() -> &'static [i32; SIN_TABLE_LEN] {
    &SIN_TABLE
}

/// One table entry, by index, wrapping at the end of the table.
fn table_at(index: usize) -> i32 {
    sin_table()
        .get(index & (SIN_TABLE_LEN - 1))
        .copied()
        .unwrap_or(0)
}

/// `sin(angle)` in Q16.16, linearly interpolated over the low four bits.
#[must_use]
pub fn sin(a: Angle) -> Fx {
    let idx = usize::from(a.0 >> 4);
    let frac = i32::from(a.0 & 0xF);
    let a0 = table_at(idx);
    let a1 = table_at(idx + 1);
    // `|a1 - a0| <= 101` and `frac <= 15`, so the product cannot overflow.
    Fx(a0 + (((a1 - a0) * frac) >> 4))
}

/// `cos(angle)` in Q16.16.
#[must_use]
pub fn cos(a: Angle) -> Fx {
    sin(Angle(a.0.wrapping_add(QUARTER_TURN)))
}

impl Angle {
    /// Zero: facing `+x`.
    pub const ZERO: Angle = Angle(0);

    /// Build from raw turn units.
    #[must_use]
    pub const fn from_raw(raw: u16) -> Angle {
        Angle(raw)
    }

    /// The raw turn units, for the canonical encoder and the snapshot.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }

    /// Rotate at most `max_step` units toward `target`, the short way round.
    ///
    /// Total by construction: at exactly half a turn the positive direction
    /// wins, so two machines never disagree about which way a unit turns.
    #[must_use]
    pub const fn turn_toward(self, target: Angle, max_step: u16) -> Angle {
        let diff = target.0.wrapping_sub(self.0);
        if diff == 0 {
            return self;
        }
        if diff <= 32_768 {
            if diff <= max_step {
                target
            } else {
                Angle(self.0.wrapping_add(max_step))
            }
        } else {
            let back = 0u16.wrapping_sub(diff);
            if back <= max_step {
                target
            } else {
                Angle(self.0.wrapping_sub(max_step))
            }
        }
    }

    /// `atan2(dy, dx)` as an [`Angle`], computed **through the sine table**:
    /// an octant reduction followed by a monotone binary search over table
    /// indices. No trigonometry, no division, no floats.
    ///
    /// The result is quantised to one table step (16 [`Angle`] units). That is
    /// deliberate: the *desired* heading is quantised, the unit's own heading
    /// is not.
    #[must_use]
    pub fn from_delta(dx: Fx, dy: Fx) -> Angle {
        let east = i64::from(dx.0);
        let north = i64::from(dy.0);
        if east == 0 && north == 0 {
            return Angle(0);
        }
        let abs_east = east.saturating_abs();
        let abs_north = north.saturating_abs();
        // Reduce to the first octant: `short <= long`.
        let (long, short, swapped) = match abs_north.cmp(&abs_east) {
            Ordering::Less | Ordering::Equal => (abs_east, abs_north, false),
            Ordering::Greater => (abs_north, abs_east, true),
        };

        // Largest table index `k` in `0..=512` with
        // `sin(k) * long <= cos(k) * short`. `sin` rises and `cos` falls across
        // that range, so the predicate is monotone and the search is total.
        // `|table| <= 2^16` and `long <= 2^32`, so each product fits in `i64`
        // with room to spare.
        let mut lo: usize = 0;
        let mut hi: usize = 512;
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let sine = i64::from(table_at(mid)).saturating_mul(long);
            let cosine = i64::from(table_at(mid + 1024)).saturating_mul(short);
            if sine <= cosine {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let base: u32 = u32::try_from(lo).unwrap_or(0) << 4; // -> 0..=8192
        let quadrant = if swapped { 16_384 - base } else { base };
        let full: u32 = if east >= 0 && north >= 0 {
            quadrant
        } else if east < 0 && north >= 0 {
            32_768 - quadrant
        } else if east < 0 {
            32_768 + quadrant
        } else {
            65_536 - quadrant
        };
        Angle(u16::try_from(full & 0xFFFF).unwrap_or(0))
    }
}
