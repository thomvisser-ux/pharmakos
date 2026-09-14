// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Split seeded RNG streams.
//!
//! Every draw is **counter-based**: the value is a pure function of
//! `(match_seed, stream, tick, seat, sub, counter)`. There is no shared mutable
//! state anywhere, so adding a draw in one stream can never shift a draw in
//! another, and no draw depends on the order in which streams are touched.
//!
//! The construction, written out so harness part 1 can turn it into a contract:
//!
//! ```text
//! GOLDEN = 0x9E3779B97F4A7C15
//! mix64(z) = z ^= z >> 30; z *= 0xBF58476D1CE4E5B9;
//!            z ^= z >> 27; z *= 0x94D049BB133111EB;
//!            z ^  z >> 31                                (SplitMix64 finaliser)
//!
//! k0 = mix64(match_seed ^ (stream_id * GOLDEN))
//! k1 = mix64(k0 ^ (tick * GOLDEN))
//! key = mix64(k1 ^ (seat << 32) ^ sub)
//!
//! draw(n) = mix64(key ^ (n * GOLDEN))
//! ```
//!
//! All multiplications are wrapping by intent — with `overflow-checks = true` a
//! plain `*` would panic, so `wrapping_mul` is spelled out at every site.

/// Golden-ratio odd constant.
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// One stream per concern. Ids are explicit, never a discriminant cast:
/// reordering this enum must not be able to change a stream id by accident.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Stream {
    Combat,
    Spawn,
    Map,
}

impl Stream {
    /// The wire id of the stream. Additive only: never reuse or renumber.
    #[must_use]
    pub const fn id(self) -> u64 {
        match self {
            Stream::Combat => 1,
            Stream::Spawn => 2,
            Stream::Map => 3,
        }
    }
}

/// SplitMix64 finaliser.
#[must_use]
pub const fn mix64(z0: u64) -> u64 {
    let mut z = z0;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A counter-based generator positioned at one point in the sim.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StreamRng {
    key: u64,
    counter: u64,
}

impl StreamRng {
    /// `sub` disambiguates draws that share a `(tick, seat)` — the unit id, the
    /// beacon id. Two callers that pass the same tuple get the same numbers,
    /// which is the point: the position in the sim *is* the state.
    #[must_use]
    pub fn new(match_seed: u64, stream: Stream, tick: u32, seat: u8, sub: u32) -> Self {
        let k0 = mix64(match_seed ^ stream.id().wrapping_mul(GOLDEN));
        let k1 = mix64(k0 ^ u64::from(tick).wrapping_mul(GOLDEN));
        let key = mix64(k1 ^ (u64::from(seat) << 32) ^ u64::from(sub));
        StreamRng { key, counter: 0 }
    }

    #[must_use]
    pub fn next_u64(&mut self) -> u64 {
        let v = mix64(self.key ^ self.counter.wrapping_mul(GOLDEN));
        self.counter = self.counter.wrapping_add(1);
        v
    }

    /// Uniform-ish value in `lo..=hi` by the multiply-shift method.
    ///
    /// The residual modulo bias is irrelevant here and, more importantly, is
    /// *identical on every target* — a rejection loop would be too, but this is
    /// branch-free and cheaper.
    #[must_use]
    pub fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        assert!(lo <= hi, "StreamRng::range_i32 inverted bounds");
        let span: u64 = u64::try_from(i64::from(hi) - i64::from(lo) + 1).expect("range span");
        let r: u128 = (u128::from(self.next_u64()) * u128::from(span)) >> 64;
        let off = i64::try_from(r).expect("range offset");
        i32::try_from(i64::from(lo) + off).expect("range result")
    }
}
