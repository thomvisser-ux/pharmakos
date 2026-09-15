// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Split seeded RNG streams (decisions log item 50).
//!
//! Every draw is **counter-based**: the value is a pure function of
//! `(match_seed, stream, tick, seat, sub, counter)`. There is no shared mutable
//! state anywhere, so adding a draw in one stream can never shift a draw in
//! another, and no draw depends on the order in which streams are touched.
//!
//! The construction, written out because it is a contract (AGENTS.md §5):
//!
//! ```text
//! GOLDEN = 0x9E3779B97F4A7C15
//! mix64(z) = z ^= z >> 30; z *= 0xBF58476D1CE4E5B9;
//!            z ^= z >> 27; z *= 0x94D049BB133111EB;
//!            z ^  z >> 31                                (SplitMix64 finaliser)
//!
//! k0  = mix64(match_seed ^ (stream_id * GOLDEN))
//! k1  = mix64(k0 ^ (tick * GOLDEN))
//! key = mix64(k1 ^ (seat << 32) ^ sub)
//!
//! draw(n) = mix64(key ^ (n * GOLDEN))
//! ```
//!
//! Every multiplication is wrapping **by intent**: with `overflow-checks = true`
//! in every profile a plain `*` would panic, so `wrapping_mul` is spelled out
//! at every site.
//!
//! # Adding a stream
//!
//! Adding a variant with a fresh id is additive and safe. **Reordering the
//! enum, renumbering an id or reusing a stream for a second purpose is a
//! determinism change** and needs owner approval (AGENTS.md §4.7, §5). The ids
//! are written out in [`Stream::id`] rather than taken from the discriminant,
//! so a reordering cannot change one by accident.

/// The golden-ratio odd constant.
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// One stream per concern.
///
/// Ids are explicit constants, additive only, never renumbered (item 50).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Stream {
    /// Combat rolls: spread, critical effects, damage jitter. Used from S2.
    Combat,
    /// Spawning and placement: initial unit positions, respawn sites.
    Spawn,
    /// Map generation, used by the seeded generator (T5).
    Map,
}

impl Stream {
    /// The wire id of the stream. Additive only: never reuse, never renumber.
    #[must_use]
    pub const fn id(self) -> u64 {
        match self {
            Stream::Combat => 1,
            Stream::Spawn => 2,
            Stream::Map => 3,
        }
    }

    /// Every stream, in id order. The confinement test walks it to prove the
    /// ids are distinct and dense.
    pub const ALL: [Stream; 3] = [Stream::Combat, Stream::Spawn, Stream::Map];
}

/// The `SplitMix64` finaliser.
#[must_use]
pub const fn mix64(z0: u64) -> u64 {
    let mut z = z0;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A counter-based generator positioned at one point in the sim.
///
/// `sub` disambiguates draws that share a `(tick, seat)` — a unit id, a beacon
/// id. Two callers that pass the same tuple get the same numbers, and that is
/// the point: the position in the sim *is* the state, so nothing has to be
/// carried between ticks or hashed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StreamRng {
    key: u64,
    counter: u64,
}

impl StreamRng {
    /// Position a generator at `(match_seed, stream, tick, seat, sub)`.
    #[must_use]
    #[allow(
        clippy::as_conversions,
        reason = "audited widening casts (AGENTS.md §4.3): u32 -> u64 and u8 -> u64 cannot truncate, and a const fn cannot call u64::from"
    )]
    pub const fn new(match_seed: u64, stream: Stream, tick: u32, seat: u8, sub: u32) -> StreamRng {
        let k0 = mix64(match_seed ^ stream.id().wrapping_mul(GOLDEN));
        let k1 = mix64(k0 ^ (tick as u64).wrapping_mul(GOLDEN));
        let key = mix64(k1 ^ ((seat as u64) << 32) ^ (sub as u64));
        StreamRng { key, counter: 0 }
    }

    /// The next 64 bits from this position.
    pub const fn next_u64(&mut self) -> u64 {
        let v = mix64(self.key ^ self.counter.wrapping_mul(GOLDEN));
        self.counter = self.counter.wrapping_add(1);
        v
    }

    /// A value in `lo..=hi` by the multiply-shift method.
    ///
    /// The residual modulo bias is part of the frozen behaviour, not an
    /// implementation detail: it is *identical on every target*, which a
    /// rejection loop would also be, but this is branch-free and cheaper.
    ///
    /// Inverted bounds return `lo`, which is the deterministic answer to a
    /// caller bug rather than a panic inside a tick.
    #[must_use]
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "audited widening casts (AGENTS.md §4.3): i32/u64 -> i128 and the >> 64 narrowing back into a value provably below `span` cannot truncate; try_from in a hot path would cost a branch per draw"
    )]
    pub const fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        if lo >= hi {
            return lo;
        }
        // `hi - lo + 1` is at most 2^32, which is why the span is u64 and the
        // product is u128.
        let span: u64 = ((hi as i64) - (lo as i64) + 1) as u64;
        let draw = self.next_u64();
        let offset: u64 = (((draw as u128) * (span as u128)) >> 64) as u64;
        // `offset < span <= 2^32`, so `lo + offset` is inside i32 by
        // construction.
        ((lo as i64) + (offset as i64)) as i32
    }
}
