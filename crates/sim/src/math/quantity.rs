// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One newtype per quantity, so the compiler stops you mixing them.
//!
//! A bare `i32` "in some unit" is how Q16.16 gets added to whole voxels and
//! how a duration in milliseconds gets compared against a tick counter
//! (AGENTS.md §4.1). Every quantity the sim carries has a type here, every
//! conversion between them is a named method, and every arithmetic form says
//! what it does on overflow.
//!
//! Times in playbooks are **game milliseconds** ([`Ms`], `int32`, decisions log
//! item 46), never ticks. The conversion is explicit and names its rounding:
//! [`Ms::to_ticks_floor`] and [`Ms::to_ticks_ceil`] both exist so that no rule
//! inherits a rounding nobody chose.

/// Ticks per second. Fixed by the spec at 20 Hz; not a tuning value, so it is
/// not a rules-table row.
pub const TICK_HZ: u32 = 20;

/// Game milliseconds in one tick: `1_000 / TICK_HZ`.
///
/// Written as a literal rather than derived from [`TICK_HZ`] by a cast — a
/// `const` initialiser cannot call `i32::try_from`, and an `as` cast in the one
/// constant every duration in the project divides by is not worth the
/// convenience (AGENTS.md §4.3). `ms_per_tick_agrees_with_tick_hz` in
/// `tests/vectors.rs` asserts the identity rather than a comment claiming it.
pub const MS_PER_TICK: i32 = 50;

/// Hit points.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Hp(i32);

impl Hp {
    /// Zero — destroyed.
    pub const ZERO: Hp = Hp(0);

    /// Build from whole hit points.
    #[must_use]
    pub const fn new(v: i32) -> Hp {
        Hp(v)
    }

    /// The raw value, for the canonical encoder and the snapshot.
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Alive means strictly positive, everywhere in the sim.
    #[must_use]
    pub const fn is_alive(self) -> bool {
        self.0 > 0
    }

    /// Apply damage, clamped at zero. The clamp is a game rule: nothing is
    /// more destroyed than destroyed.
    #[must_use]
    pub const fn damaged_by(self, amount: Hp) -> Hp {
        let left = self.0.saturating_sub(amount.0);
        if left < 0 { Hp(0) } else { Hp(left) }
    }
}

/// Money, in `$`. `i64` because a match's treasury outgrows `i32`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Money(i64);

impl Money {
    /// Nothing.
    pub const ZERO: Money = Money(0);

    /// Build from whole `$`.
    #[must_use]
    pub const fn new(v: i64) -> Money {
        Money(v)
    }

    /// The raw value, for the canonical encoder and the snapshot.
    #[must_use]
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// Addition that reports overflow rather than wrapping.
    #[must_use]
    pub const fn checked_add(self, rhs: Money) -> Option<Money> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Some(Money(v)),
            None => None,
        }
    }

    /// Subtraction that reports overflow.
    #[must_use]
    pub const fn checked_sub(self, rhs: Money) -> Option<Money> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Some(Money(v)),
            None => None,
        }
    }
}

/// Electrical power, in `kW`. Supply, draw and headroom all use it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Kw(i32);

impl Kw {
    /// Nothing.
    pub const ZERO: Kw = Kw(0);

    /// Build from whole `kW`.
    #[must_use]
    pub const fn new(v: i32) -> Kw {
        Kw(v)
    }

    /// The raw value, for the canonical encoder and the snapshot.
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Addition that reports overflow.
    #[must_use]
    pub const fn checked_add(self, rhs: Kw) -> Option<Kw> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Some(Kw(v)),
            None => None,
        }
    }

    /// Subtraction that reports overflow.
    #[must_use]
    pub const fn checked_sub(self, rhs: Kw) -> Option<Kw> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Some(Kw(v)),
            None => None,
        }
    }
}

/// The sim's clock: a 20 Hz tick counter, and the only clock it has.
///
/// Nothing in this crate reads a wall clock (AGENTS.md §4.5). A duration
/// compared against a `Tick` is converted from [`Ms`] at the call site, by a
/// named method, so the rounding is visible.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Tick(u32);

impl Tick {
    /// The first tick of a segment.
    pub const ZERO: Tick = Tick(0);

    /// Build from a raw tick index.
    #[must_use]
    pub const fn new(v: u32) -> Tick {
        Tick(v)
    }

    /// The raw tick index, for the canonical encoder and the snapshot.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// The next tick, or `None` at the end of the counter's range.
    ///
    /// A match cannot reach `u32::MAX` ticks (that is over six years of game
    /// time), so `None` is a bug report rather than a game state.
    #[must_use]
    pub const fn next(self) -> Option<Tick> {
        match self.0.checked_add(1) {
            Some(v) => Some(Tick(v)),
            None => None,
        }
    }

    /// Ticks since `earlier`, saturating at zero.
    #[must_use]
    pub const fn since(self, earlier: Tick) -> u32 {
        self.0.saturating_sub(earlier.0)
    }
}

/// A duration in game milliseconds. `int32`, matching `gp.v1` (item 46).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Ms(i32);

impl Ms {
    /// Nothing.
    pub const ZERO: Ms = Ms(0);

    /// Build from whole game milliseconds.
    #[must_use]
    pub const fn new(v: i32) -> Ms {
        Ms(v)
    }

    /// The raw value, for the canonical encoder and the snapshot.
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Whole ticks, rounding **down**. Negative durations give zero; the
    /// verifier rejects them with a code and a JSON Pointer long before they
    /// reach here (item 46).
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "the rounding is the point of the method's name; MS_PER_TICK is a non-zero constant"
    )]
    pub fn to_ticks_floor(self) -> u32 {
        if self.0 <= 0 {
            return 0;
        }
        // `self.0 > 0` and `MS_PER_TICK > 0`, so the quotient is non-negative
        // and fits `u32` for every `i32` numerator.
        let ticks = self.0 / MS_PER_TICK;
        u32::try_from(ticks).unwrap_or(0)
    }

    /// Whole ticks, rounding **up** — the form a timeout wants, so a wait is
    /// never cut short by rounding.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "the rounding is the point of the method's name; MS_PER_TICK is a non-zero constant"
    )]
    pub fn to_ticks_ceil(self) -> u32 {
        if self.0 <= 0 {
            return 0;
        }
        let ticks = self.0.saturating_add(MS_PER_TICK - 1) / MS_PER_TICK;
        u32::try_from(ticks).unwrap_or(0)
    }

    /// The duration of `ticks` whole ticks, saturating at [`i32::MAX`].
    #[must_use]
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "audited widening casts (AGENTS.md §4.3): u32 -> i64 and i32 -> i64 cannot truncate, the narrowing back is guarded by the range test, and a const fn cannot call i64::from"
    )]
    pub const fn from_ticks(ticks: u32) -> Ms {
        let ms = (ticks as i64).saturating_mul(MS_PER_TICK as i64);
        if ms > i32::MAX as i64 {
            Ms(i32::MAX)
        } else {
            Ms(ms as i32)
        }
    }
}
