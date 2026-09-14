// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Save / restore, in both candidate formats, behind features.
//!
//! [`Snapshot`] is a flat projection of [`World`] built from **fixed-width types
//! only**. No `usize`, no `isize`, no pointers, no `Option`, no enums: the two
//! formats then have nothing target-dependent left to disagree about except the
//! sequence lengths they write themselves, and both write those in a
//! width-independent way (rkyv at the pinned 32-bit pointer width, postcard as
//! LEB128 of the same numeric value).
//!
//! `Option<u32>` in the world becomes [`NO_TARGET`] in the snapshot, so the
//! encoding has no tag byte whose layout could differ.

use crate::fixed::Fx;
use crate::{Beacons, Credits, Seat, Units, World};
use imbl::OrdMap;

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
        let n = self.u_id.len();
        assert_eq!(self.u_pos.len(), n * 3, "unit position stride");
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
        assert_eq!(self.b_pos.len(), m * 3, "beacon position stride");
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

        let mut seats = Vec::with_capacity(self.s_seat.len());
        for i in 0..self.s_seat.len() {
            seats.push(Seat {
                seat: self.s_seat[i],
                cash: self.s_cash[i],
                kw: self.s_kw[i],
                kills: self.s_kills[i],
            });
        }

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
