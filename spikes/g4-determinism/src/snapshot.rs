// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Save / restore, in both candidate formats, behind features.
//!
//! [`Snapshot`] is a flat projection of [`World`] whose **fields** are all
//! fixed-width. No `usize` or `isize` field, no pointer, no `Option`, no enum.
//!
//! The one thing that is *not* a field of ours is the sequence length each
//! `Vec<T>` hands the serialiser, and it is worth being exact about it rather
//! than claiming the snapshot contains no `usize` at all — it does, once per
//! vector, supplied by the format:
//!
//! * **rkyv** writes lengths at the pinned 32-bit pointer width
//!   (`pointer_width_32` in `Cargo.toml`), so they are the same bytes on a
//!   32- and a 64-bit host.
//! * **postcard** encodes a sequence length as a canonical LEB128 varint of the
//!   host's `usize`, with no width padding — so the bytes agree between 32- and
//!   64-bit hosts for every length below 2^32, which covers every snapshot this
//!   toy can produce by many orders of magnitude. It is a *value* identity, not
//!   a type-level one; the 32-bit web build on the roadmap stays inside it, but
//!   the caveat belongs in the snapshot-format decision rather than being
//!   waved away here.
//!
//! `Option<u32>` in the world becomes [`NO_TARGET`] in the snapshot, so the
//! encoding has no tag byte whose layout could differ.

use crate::fixed::Fx;
use crate::{Beacons, Credits, Seat, Units, World};
use imbl::OrdMap;

// The two formats are additive cargo features, which means `--all-features`
// would quietly enable both and — since rkyv wins the `cfg` race below — measure
// rkyv twice while reporting that postcard was covered. Step 7 of the plan needs
// both formats measured separately, so make the ambiguity a compile error rather
// than a habit of always invoking the two feature builds by hand.
#[cfg(all(feature = "snap-rkyv", feature = "snap-postcard"))]
compile_error!(
    "pick exactly one snapshot format: --features snap-rkyv OR --features snap-postcard. \
     They are measured separately (G4 plan step 7); enabling both would test one twice."
);

/// Sentinel for "no target" in the flat encoding.
pub const NO_TARGET: u32 = u32::MAX;

/// Snapshot format version; part of the bytes, checked on restore.
pub const SNAPSHOT_VERSION: u32 = 1;

/// Flat, fixed-width projection of the world.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "snap-postcard", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "snap-rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Snapshot {
    pub version: u32,
    pub match_seed: u64,
    pub tick: u32,
    pub deaths: u32,

    pub u_id: Vec<u32>,
    pub u_seat: Vec<u8>,
    /// Three raw Q16.16 coordinates per unit, x, y, z.
    pub u_pos: Vec<i32>,
    pub u_heading: Vec<u16>,
    pub u_hp: Vec<i32>,
    pub u_target: Vec<u32>,
    pub u_cooldown: Vec<u16>,

    pub b_id: Vec<u32>,
    pub b_seat: Vec<u8>,
    pub b_pos: Vec<i32>,
    pub b_hp: Vec<i32>,
    pub b_treasury: Vec<i64>,
    pub b_kw_draw: Vec<i32>,

    pub s_seat: Vec<u8>,
    pub s_cash: Vec<i64>,
    pub s_kw: Vec<i32>,
    pub s_kills: Vec<u32>,

    /// Kill-credit table, flattened: one `c_asset`/`c_n` pair per asset in key
    /// order, and `c_n` consecutive `(c_seat, c_dmg)` pairs in the run vectors.
    pub c_asset: Vec<u32>,
    pub c_n: Vec<u8>,
    pub c_seat: Vec<u8>,
    pub c_dmg: Vec<u32>,
}

impl Snapshot {
    /// Project a world into the flat form. Lossless.
    #[must_use]
    pub fn capture(w: &World) -> Snapshot {
        let mut s = Snapshot {
            version: SNAPSHOT_VERSION,
            match_seed: w.match_seed,
            tick: w.tick,
            deaths: w.deaths,
            ..Snapshot::default()
        };
        for i in 0..w.units.len() {
            s.u_id.push(w.units.id[i]);
            s.u_seat.push(w.units.seat[i]);
            s.u_pos.push(w.units.pos[i][0].raw());
            s.u_pos.push(w.units.pos[i][1].raw());
            s.u_pos.push(w.units.pos[i][2].raw());
            s.u_heading.push(w.units.heading[i]);
            s.u_hp.push(w.units.hp[i]);
            s.u_target.push(w.units.target[i].unwrap_or(NO_TARGET));
            s.u_cooldown.push(w.units.cooldown[i]);
        }
        for i in 0..w.beacons.len() {
            s.b_id.push(w.beacons.id[i]);
            s.b_seat.push(w.beacons.seat[i]);
            s.b_pos.push(w.beacons.pos[i][0].raw());
            s.b_pos.push(w.beacons.pos[i][1].raw());
            s.b_pos.push(w.beacons.pos[i][2].raw());
            s.b_hp.push(w.beacons.hp[i]);
            s.b_treasury.push(w.beacons.treasury[i]);
            s.b_kw_draw.push(w.beacons.kw_draw[i]);
        }
        for seat in &w.seats {
            s.s_seat.push(seat.seat);
            s.s_cash.push(seat.cash);
            s.s_kw.push(seat.kw);
            s.s_kills.push(seat.kills);
        }
        for (asset, c) in w.credits.iter() {
            s.c_asset.push(*asset);
            s.c_n
                .push(u8::try_from(c.entries.len()).expect("credit count"));
            for &(seat, dmg) in &c.entries {
                s.c_seat.push(seat);
                s.c_dmg.push(dmg);
            }
        }
        s
    }

    /// Rebuild a world. Panics on a malformed snapshot — this is a spike, and a
    /// malformed snapshot is a failed round trip, which is the result we want.
    #[must_use]
    pub fn restore(&self) -> World {
        assert_eq!(self.version, SNAPSHOT_VERSION, "snapshot version mismatch");

        // Every parallel vector is checked by name before anything is indexed.
        // G4-b runs `restore` in a child process, and an index-out-of-bounds
        // panic there surfaces to the parent as `restore child failed:` plus a
        // raw backtrace — a named assertion says which field of a truncated or
        // hand-edited snapshot was short.
        let n = self.u_id.len();
        assert_eq!(self.u_pos.len(), n * 3, "unit position stride (u_pos)");
        assert_eq!(self.u_seat.len(), n, "unit field length (u_seat)");
        assert_eq!(self.u_heading.len(), n, "unit field length (u_heading)");
        assert_eq!(self.u_hp.len(), n, "unit field length (u_hp)");
        assert_eq!(self.u_target.len(), n, "unit field length (u_target)");
        assert_eq!(self.u_cooldown.len(), n, "unit field length (u_cooldown)");

        let mut units = Units::default();
        for i in 0..n {
            units.id.push(self.u_id[i]);
            units.seat.push(self.u_seat[i]);
            units.pos.push([
                Fx(self.u_pos[i * 3]),
                Fx(self.u_pos[i * 3 + 1]),
                Fx(self.u_pos[i * 3 + 2]),
            ]);
            units.heading.push(self.u_heading[i]);
            units.hp.push(self.u_hp[i]);
            units.target.push(if self.u_target[i] == NO_TARGET {
                None
            } else {
                Some(self.u_target[i])
            });
            units.cooldown.push(self.u_cooldown[i]);
        }

        let m = self.b_id.len();
        assert_eq!(self.b_pos.len(), m * 3, "beacon position stride (b_pos)");
        assert_eq!(self.b_seat.len(), m, "beacon field length (b_seat)");
        assert_eq!(self.b_hp.len(), m, "beacon field length (b_hp)");
        assert_eq!(self.b_treasury.len(), m, "beacon field length (b_treasury)");
        assert_eq!(self.b_kw_draw.len(), m, "beacon field length (b_kw_draw)");
        let mut beacons = Beacons::default();
        for i in 0..m {
            beacons.id.push(self.b_id[i]);
            beacons.seat.push(self.b_seat[i]);
            beacons.pos.push([
                Fx(self.b_pos[i * 3]),
                Fx(self.b_pos[i * 3 + 1]),
                Fx(self.b_pos[i * 3 + 2]),
            ]);
            beacons.hp.push(self.b_hp[i]);
            beacons.treasury.push(self.b_treasury[i]);
            beacons.kw_draw.push(self.b_kw_draw[i]);
        }

        let k = self.s_seat.len();
        assert_eq!(self.s_cash.len(), k, "seat field length (s_cash)");
        assert_eq!(self.s_kw.len(), k, "seat field length (s_kw)");
        assert_eq!(self.s_kills.len(), k, "seat field length (s_kills)");
        let mut seats = Vec::with_capacity(self.s_seat.len());
        for i in 0..self.s_seat.len() {
            seats.push(Seat {
                seat: self.s_seat[i],
                cash: self.s_cash[i],
                kw: self.s_kw[i],
                kills: self.s_kills[i],
            });
        }

        assert_eq!(
            self.c_n.len(),
            self.c_asset.len(),
            "credit run length (c_n) does not match the asset list (c_asset)"
        );
        let runs: usize = self.c_n.iter().map(|&x| usize::from(x)).sum();
        assert_eq!(
            self.c_seat.len(),
            runs,
            "credit run vector (c_seat) does not match the sum of c_n"
        );
        assert_eq!(
            self.c_dmg.len(),
            runs,
            "credit run vector (c_dmg) does not match the sum of c_n"
        );

        let mut credits: OrdMap<u32, Credits> = OrdMap::new();
        let mut cursor = 0usize;
        for i in 0..self.c_asset.len() {
            let n = usize::from(self.c_n[i]);
            let mut c = Credits::default();
            for k in 0..n {
                c.entries.push((self.c_seat[cursor + k], self.c_dmg[cursor + k]));
            }
            cursor += n;
            credits.insert(self.c_asset[i], c);
        }

        World {
            match_seed: self.match_seed,
            tick: self.tick,
            units,
            beacons,
            seats,
            credits,
            deaths: self.deaths,
        }
    }
}

// ---------------------------------------------------------------------------
// Format selection. Additive features; if both are on, rkyv wins, so a build
// with `--all-features` still has one well-defined on-disk format.
// ---------------------------------------------------------------------------

/// Name of the format compiled into this binary.
#[must_use]
pub fn format_name() -> &'static str {
    #[cfg(feature = "snap-rkyv")]
    {
        "rkyv"
    }
    #[cfg(all(feature = "snap-postcard", not(feature = "snap-rkyv")))]
    {
        "postcard"
    }
    #[cfg(not(any(feature = "snap-rkyv", feature = "snap-postcard")))]
    {
        "none"
    }
}

#[cfg(feature = "snap-rkyv")]
mod imp {
    use super::Snapshot;

    pub fn encode(s: &Snapshot) -> Vec<u8> {
        let av = rkyv::to_bytes::<rkyv::rancor::Error>(s).expect("rkyv serialise");
        av.to_vec()
    }

    pub fn decode(bytes: &[u8]) -> Snapshot {
        // rkyv needs the buffer aligned; copy into rkyv's aligned vector first.
        let mut av = rkyv::util::AlignedVec::<16>::new();
        av.extend_from_slice(bytes);
        rkyv::from_bytes::<Snapshot, rkyv::rancor::Error>(&av).expect("rkyv deserialise")
    }
}

#[cfg(all(feature = "snap-postcard", not(feature = "snap-rkyv")))]
mod imp {
    use super::Snapshot;

    pub fn encode(s: &Snapshot) -> Vec<u8> {
        postcard::to_stdvec(s).expect("postcard serialise")
    }

    pub fn decode(bytes: &[u8]) -> Snapshot {
        postcard::from_bytes::<Snapshot>(bytes).expect("postcard deserialise")
    }
}

/// Serialise a snapshot with the compiled-in format.
///
/// # Panics
/// If neither snapshot feature is enabled.
#[must_use]
pub fn encode(s: &Snapshot) -> Vec<u8> {
    #[cfg(any(feature = "snap-rkyv", feature = "snap-postcard"))]
    {
        imp::encode(s)
    }
    #[cfg(not(any(feature = "snap-rkyv", feature = "snap-postcard")))]
    {
        let _ = s;
        panic!("no snapshot format compiled in: build with --features snap-rkyv or snap-postcard")
    }
}

/// Deserialise a snapshot with the compiled-in format.
///
/// # Panics
/// If neither snapshot feature is enabled.
#[must_use]
pub fn decode(bytes: &[u8]) -> Snapshot {
    #[cfg(any(feature = "snap-rkyv", feature = "snap-postcard"))]
    {
        imp::decode(bytes)
    }
    #[cfg(not(any(feature = "snap-rkyv", feature = "snap-postcard")))]
    {
        let _ = bytes;
        panic!("no snapshot format compiled in: build with --features snap-rkyv or snap-postcard")
    }
}
