// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixed-point arithmetic for the toy sim.
//!
//! * [`Fx`] — Q16.16 position / length, backed by `i32`.
//! * [`Sq`] — Q32.32 squared distance, backed by `i64`. Range checks compare
//!   squared distances; there is no square root anywhere in the spike.
//! * [`Angle`] — a `u16` turn (65_536 units per full turn) plus a 4_096-entry
//!   sine table with linear interpolation over the low four bits.
//!
//! Rules honoured here and asserted in `tests/vectors.rs`:
//!
//! * no `f32`/`f64` — the sine table is built by an integer Taylor series in
//!   Q32.32 with `i128` intermediates, so it is bit-identical on every target;
//! * no `as` casts — every conversion is `From`/`TryFrom` with an explicit
//!   failure mode;
//! * every shift on a signed value is an arithmetic shift, which floors toward
//!   negative infinity; that is the documented rounding of `Fx::mul_fx`.

use std::sync::LazyLock;

/// Number of fractional bits in [`Fx`].
pub const FRAC_BITS: u32 = 16;

/// Entries in the sine table. Index = `angle >> 4`.
pub const SIN_TABLE_LEN: usize = 4096;

/// `round(2 * pi * 2^32)`. Computed once, by hand, and pinned: the table is a
/// contract, so the constant that generates it is one too.
const TWO_PI_Q32: i64 = 26_986_075_409;

/// Q16.16 scalar: position along one axis, a length, a speed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub struct Fx(pub i32);

impl Fx {
    pub const ZERO: Fx = Fx(0);
    pub const ONE: Fx = Fx(1 << FRAC_BITS);

    /// Whole voxels -> Q16.16. `i16 -> i32` cannot truncate.
    #[must_use]
    pub fn from_voxels(v: i16) -> Fx {
        Fx(i32::from(v) << FRAC_BITS)
    }

    #[must_use]
    pub fn raw(self) -> i32 {
        self.0
    }

    /// Checked add; overflow-checks are on in every profile so `+` would panic
    /// anyway, but a named method documents that the caller decided.
    #[must_use]
    pub fn plus(self, rhs: Fx) -> Fx {
        Fx(self.0.checked_add(rhs.0).expect("Fx::plus overflow"))
    }

    #[must_use]
    pub fn minus(self, rhs: Fx) -> Fx {
        Fx(self.0.checked_sub(rhs.0).expect("Fx::minus overflow"))
    }

    #[must_use]
    pub fn negate(self) -> Fx {
        Fx(self.0.checked_neg().expect("Fx::negate overflow"))
    }

    /// Q16.16 * Q16.16. The intermediate is Q32.32 in `i64`; the result is
    /// shifted right arithmetically, so it **rounds toward negative infinity**
    /// (`Fx(-1).mul_fx(Fx::ONE >> 1)` floors, it does not truncate toward zero).
    /// That choice is deterministic and is the one the tests pin.
    #[must_use]
    pub fn mul_fx(self, rhs: Fx) -> Fx {
        let p: i64 = i64::from(self.0) * i64::from(rhs.0);
        Fx(i32::try_from(p >> FRAC_BITS).expect("Fx::mul_fx out of range"))
    }

    /// Q16.16 / Q16.16. Integer division truncates toward zero (Rust-defined,
    /// identical on every target). Panics on a zero divisor by construction.
    #[must_use]
    pub fn div_fx(self, rhs: Fx) -> Fx {
        assert!(rhs.0 != 0, "Fx::div_fx by zero");
        let n: i64 = i64::from(self.0) << FRAC_BITS;
        Fx(i32::try_from(n / i64::from(rhs.0)).expect("Fx::div_fx out of range"))
    }

    /// Multiply by a whole number.
    #[must_use]
    pub fn scale(self, k: i32) -> Fx {
        Fx(self.0.checked_mul(k).expect("Fx::scale overflow"))
    }

    #[must_use]
    pub fn abs(self) -> Fx {
        Fx(self.0.checked_abs().expect("Fx::abs overflow"))
    }

    /// Clamp into `[lo, hi]`. Total, so it can appear in a sort key path.
    #[must_use]
    pub fn clamp_to(self, lo: Fx, hi: Fx) -> Fx {
        assert!(lo.0 <= hi.0, "Fx::clamp_to inverted bounds");
        if self.0 < lo.0 {
            lo
        } else if self.0 > hi.0 {
            hi
        } else {
            self
        }
    }
}

/// Q32.32 squared distance.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub struct Sq(pub i64);

impl Sq {
    pub const ZERO: Sq = Sq(0);

    /// Squared distance between two 3-vectors of Q16.16 coordinates.
    ///
    /// Bound: the toy map clamps every coordinate to +/- 1_024 voxels, so each
    /// delta is at most 2^27 raw, each square at most 2^54, and the sum of three
    /// at most 2^55.6 — comfortably inside `i64` with overflow checks on.
    #[must_use]
    pub fn between(a: [Fx; 3], b: [Fx; 3]) -> Sq {
        let mut acc: i64 = 0;
        let mut k = 0usize;
        while k < 3 {
            let d: i64 = i64::from(a[k].0) - i64::from(b[k].0);
            acc = acc.checked_add(d * d).expect("Sq::between overflow");
            k += 1;
        }
        Sq(acc)
    }

    /// The square of a radius, for range checks.
    #[must_use]
    pub fn of_radius(r: Fx) -> Sq {
        let d = i64::from(r.0);
        Sq(d * d)
    }
}

/// A heading. 65_536 units to the full turn; 0 is +x, 16_384 is +y.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub struct Angle(pub u16);

/// One quarter turn in [`Angle`] units.
pub const QUARTER_TURN: u16 = 16_384;

/// `sin(2*pi*i/4096)` in Q16.16, for `i` in `0..4096`.
static SIN_TABLE: LazyLock<[i32; SIN_TABLE_LEN]> = LazyLock::new(build_sin_table);

/// `sin(x)` for `x` in Q32.32 radians over `[0, pi/2]`, result in Q32.32.
///
/// Taylor series to the `x^13/13!` term; at `x = pi/2` the first dropped term is
/// below `1e-7`, far under one Q16.16 ulp. Every operation is integer, so the
/// table is the same on x86-64 and aarch64.
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
    i64::try_from(acc).expect("sin_q32 out of range")
}

fn build_sin_table() -> [i32; SIN_TABLE_LEN] {
    // Quarter wave, inclusive of both endpoints.
    let mut quarter = [0i32; 1025];
    let mut i: usize = 0;
    while i <= 1024 {
        let idx = i64::try_from(i).expect("quarter index");
        let x = TWO_PI_Q32 * idx / 4096;
        let v = sin_q32(x);
        // Q32.32 -> Q16.16, rounding half up. `>> 16` is arithmetic.
        quarter[i] = i32::try_from((v + (1 << 15)) >> 16).expect("sin table out of range");
        i += 1;
    }

    let mut t = [0i32; SIN_TABLE_LEN];
    let mut j: usize = 0;
    while j < SIN_TABLE_LEN {
        t[j] = if j <= 1024 {
            quarter[j]
        } else if j <= 2048 {
            quarter[2048 - j]
        } else if j <= 3072 {
            -quarter[j - 2048]
        } else {
            -quarter[4096 - j]
        };
        j += 1;
    }
    t
}

/// The raw table, for the binary search in [`Angle::from_delta`] and for tests.
#[must_use]
pub fn sin_table() -> &'static [i32; SIN_TABLE_LEN] {
    &SIN_TABLE
}

/// `sin(angle)` in Q16.16, linearly interpolated over the low four bits.
#[must_use]
pub fn sin(a: Angle) -> Fx {
    let t = sin_table();
    let idx = usize::from(a.0 >> 4);
    let frac = i32::from(a.0 & 0xF);
    let a0 = t[idx];
    let a1 = t[(idx + 1) & (SIN_TABLE_LEN - 1)];
    // |a1 - a0| <= 101, so the product cannot overflow i32.
    Fx(a0 + (((a1 - a0) * frac) >> 4))
}

/// `cos(angle)` in Q16.16.
#[must_use]
pub fn cos(a: Angle) -> Fx {
    sin(Angle(a.0.wrapping_add(QUARTER_TURN)))
}

impl Angle {
    /// Rotate at most `max_step` units toward `target`, the short way round.
    ///
    /// Total by construction: at exactly half a turn the positive direction
    /// wins, so two machines never disagree about which way a unit turns.
    #[must_use]
    pub fn turn_toward(self, target: Angle, max_step: u16) -> Angle {
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

    /// `atan2(dy, dx)` as an [`Angle`], computed **through the sine table**: an
    /// octant reduction followed by a monotone binary search over table indices.
    /// No trigonometry, no division, no floats.
    ///
    /// Resolution is one table step (16 [`Angle`] units); that is deliberate —
    /// the desired heading is quantised, the unit's own heading is not.
    #[must_use]
    pub fn from_delta(dx: Fx, dy: Fx) -> Angle {
        let x = i64::from(dx.0);
        let y = i64::from(dy.0);
        if x == 0 && y == 0 {
            return Angle(0);
        }
        let ax = x.checked_abs().expect("from_delta abs");
        let ay = y.checked_abs().expect("from_delta abs");
        // Reduce to the first octant: 0 <= v <= u.
        let (u, v, swapped) = if ay <= ax { (ax, ay, false) } else { (ay, ax, true) };

        // Largest table index k in [0, 512] with sin(k)*u <= cos(k)*v.
        // sin increases and cos decreases across that range, so the predicate is
        // monotone and the search is total.
        //
        // Bound: |table| <= 65_536 (2^16) and u <= 2^27, so the product fits i64.
        let t = sin_table();
        let mut lo: usize = 0;
        let mut hi: usize = 512;
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let s = i64::from(t[mid]) * u;
            let c = i64::from(t[(mid + 1024) & (SIN_TABLE_LEN - 1)]) * v;
            if s <= c {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let base = u32::try_from(lo).expect("octant index") << 4; // -> Angle units, 0..=8192
        let quadrant = if swapped { 16_384 - base } else { base };
        let full: u32 = if x >= 0 && y >= 0 {
            quadrant
        } else if x < 0 && y >= 0 {
            32_768 - quadrant
        } else if x < 0 {
            32_768 + quadrant
        } else {
            65_536 - quadrant
        };
        Angle(u16::try_from(full & 0xFFFF).expect("angle wrap"))
    }
}
