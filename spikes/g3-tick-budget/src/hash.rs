// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Canonical byte encoding, and the per-tick state hash in **both** forms the
//! plan asks for.
//!
//! The encoder half is copied from the G4 spike (`spikes/README.md` rule 2):
//! same [`STATE_HASH_SEED`], same [`ENCODING_VERSION`], same fixed-width
//! little-endian discipline, same rule that lengths are checked into `u32` and
//! that pointer width never enters the hash. Keeping it identical is what makes
//! this spike's hash stream usable as a second determinism trace for G4.
//!
//! The new half is [`StateHasher`], which is what G3' is for. Plan §2:
//!
//! > The spike measures it in both forms — a full re-hash, and an
//! > incremental/merkle-ish scheme where each table keeps a dirty flag and only
//! > changed tables are re-hashed with the rest folded in from cached
//! > sub-hashes — and reports the cost of each.
//!
//! Both forms produce **the same 64-bit value**, by construction: the per-tick
//! hash is always
//!
//! ```text
//! fold = xxh3(ENCODING_VERSION || sub[0] || sub[1] || ... || sub[N-1], SEED)
//! ```
//!
//! over the sub-hashes in [`Table::ALL`] order, and the only difference between
//! the modes is *which* sub-hashes were recomputed this tick rather than reused
//! from the cache. [`HashMode::Full`] recomputes all of them; [`HashMode::Incremental`]
//! recomputes the ones a phase marked dirty. That identity is the thing the
//! tests pin, because it is what makes the choice a performance decision with a
//! contract consequence rather than two different hashes.
//!
//! Note what is *not* on the two sides of that comparison: the 32^3 chunk store
//! keeps a per-chunk digest that it refreshes when a chunk is written, in both
//! modes, and the voxel table's canonical encoding is the sequence of those
//! digests. Neither mode ever re-hashes 9.4 MB of voxel bytes, so calling the
//! first one "full" would overclaim — and the difference is not small.
//! `tickbench` measures one whole-store rehash outside the tick and publishes
//! it as `hash.whole_voxel_store_rehash_ms` precisely so the exclusion is a
//! number rather than an assertion: on the developer machine it is about
//! **0.76 ms**, against about **0.016 ms** for the whole eight-table hash as
//! built here. Hashing voxels rather than digests would therefore cost roughly
//! 45x the hash phase and about three times the entire rest of the tick outside
//! `pathing` — affordable in the sense that it fits, but never worth it.

use xxhash_rust::xxh3::xxh3_64_with_seed;

/// The one compiled-in hash seed. Same value as the G4 spike's, on purpose:
/// the ASCII bytes `PHARMKOS`, big-endian in the literal so the constant reads
/// as the word.
pub const STATE_HASH_SEED: u64 = 0x5048_4152_4D4B_4F53;

/// Version byte of the canonical encoding. Bump only with the owner's approval;
/// a bump moves every hash in the project.
pub const ENCODING_VERSION: u8 = 1;

/// Append-only canonical encoder. Every construction path pushes
/// [`ENCODING_VERSION`] first, and `Default` is written by hand for exactly
/// that reason.
#[derive(Clone, Debug)]
pub struct Enc {
    buf: Vec<u8>,
}

impl Default for Enc {
    fn default() -> Enc {
        Enc::with_capacity(0)
    }
}

impl Enc {
    #[must_use]
    pub fn with_capacity(n: usize) -> Enc {
        let mut e = Enc {
            buf: Vec::with_capacity(n),
        };
        e.buf.push(ENCODING_VERSION);
        e
    }

    /// Reset to "just the version byte" **without** giving the capacity back.
    /// The hot loop calls this once per table per tick; plan §5's last-but-one
    /// row ("hash cost hidden by the allocator") is only answerable because
    /// this does not allocate.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.buf.push(ENCODING_VERSION);
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// `Option<u32>`: a one-byte tag then four bytes. The absent arm still
    /// writes four zero bytes, so the encoding is fixed-stride and a field can
    /// never be confused with its neighbour.
    pub fn opt_u32(&mut self, v: Option<u32>) {
        match v {
            Some(x) => {
                self.buf.push(1);
                self.buf.extend_from_slice(&x.to_le_bytes());
            }
            None => {
                self.buf.push(0);
                self.buf.extend_from_slice(&0u32.to_le_bytes());
            }
        }
    }

    /// A collection length. `usize` is converted, checked, and hashed as `u32`.
    pub fn len(&mut self, n: usize) {
        let v = u32::try_from(n).expect("length exceeds u32 in canonical encoding");
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.buf.len()
    }

    /// Finish: xxh3-64 over the canonical bytes at the project seed.
    #[must_use]
    pub fn finish(&self) -> u64 {
        xxh3_64_with_seed(&self.buf, STATE_HASH_SEED)
    }
}

/// xxh3-64 of an arbitrary byte slice at the project seed.
#[must_use]
pub fn digest(bytes: &[u8]) -> u64 {
    xxh3_64_with_seed(bytes, STATE_HASH_SEED)
}

/// Lowercase, zero-padded, 16 hex digits. The only hash rendering in the spike.
#[must_use]
pub fn hex(h: u64) -> String {
    format!("{h:016x}")
}

// ---------------------------------------------------------------------------
// The tables, in declared order.
// ---------------------------------------------------------------------------

/// One hashed table. **The order of [`Table::ALL`] is the canonical order and
/// is a contract**: the fold reads the sub-hashes in it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Table {
    /// Match seed, tick, per-match scalars.
    Header,
    Units,
    Beacons,
    Structures,
    Projectiles,
    /// The per-seat kill-credit counters, at most three per asset.
    Credits,
    Seats,
    /// The chunk store, as its per-chunk digests.
    Voxels,
}

/// How many tables the fold covers.
pub const TABLE_COUNT: usize = 8;

impl Table {
    /// Declared order. Additive only; reordering moves every hash.
    pub const ALL: [Table; TABLE_COUNT] = [
        Table::Header,
        Table::Units,
        Table::Beacons,
        Table::Structures,
        Table::Projectiles,
        Table::Credits,
        Table::Seats,
        Table::Voxels,
    ];

    /// Slot in the sub-hash array. An explicit match, never a discriminant
    /// cast: reordering the enum must not be able to move a slot by accident.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Table::Header => 0,
            Table::Units => 1,
            Table::Beacons => 2,
            Table::Structures => 3,
            Table::Projectiles => 4,
            Table::Credits => 5,
            Table::Seats => 6,
            Table::Voxels => 7,
        }
    }

    /// The tag byte written at the head of the table's own encoding, so two
    /// tables that happened to encode to the same bytes still hash apart.
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Table::Header => 0x10,
            Table::Units => 0x11,
            Table::Beacons => 0x12,
            Table::Structures => 0x13,
            Table::Projectiles => 0x14,
            Table::Credits => 0x15,
            Table::Seats => 0x16,
            Table::Voxels => 0x17,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Table::Header => "header",
            Table::Units => "units",
            Table::Beacons => "beacons",
            Table::Structures => "structures",
            Table::Projectiles => "projectiles",
            Table::Credits => "credits",
            Table::Seats => "seats",
            Table::Voxels => "voxels",
        }
    }
}

/// Which of the two schemes the `hash` phase runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HashMode {
    /// Re-encode and re-hash every table, every tick.
    #[default]
    Full,
    /// Re-encode only the tables a phase marked dirty; fold the rest in from
    /// their cached sub-hashes.
    Incremental,
}

impl HashMode {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            HashMode::Full => "full",
            HashMode::Incremental => "incremental",
        }
    }
}

/// Cached sub-hashes plus dirty flags, and the fold that turns them into the
/// per-tick value.
///
/// The fold buffer is allocated once and reused; the caller owns the [`Enc`]
/// used for table bodies, for the same reason.
#[derive(Clone, Debug)]
pub struct StateHasher {
    sub: [u64; TABLE_COUNT],
    dirty: [bool; TABLE_COUNT],
    fold_buf: Vec<u8>,
    /// Count of sub-hash recomputations, for the cost attribution in the JSON.
    pub rehashes: u64,
}

impl Default for StateHasher {
    fn default() -> StateHasher {
        StateHasher::new()
    }
}

impl StateHasher {
    #[must_use]
    pub fn new() -> StateHasher {
        StateHasher {
            sub: [0; TABLE_COUNT],
            // Everything starts dirty: a cached sub-hash that was never
            // computed must never be folded in.
            dirty: [true; TABLE_COUNT],
            fold_buf: Vec::with_capacity(1 + 8 * TABLE_COUNT),
            rehashes: 0,
        }
    }

    pub fn mark(&mut self, t: Table) {
        self.dirty[t.index()] = true;
    }

    pub fn mark_all(&mut self) {
        self.dirty = [true; TABLE_COUNT];
    }

    #[must_use]
    pub fn is_dirty(&self, t: Table) -> bool {
        self.dirty[t.index()]
    }

    /// Store a freshly computed sub-hash and clear the table's dirty flag.
    pub fn set_sub(&mut self, t: Table, h: u64) {
        self.sub[t.index()] = h;
        self.dirty[t.index()] = false;
        self.rehashes += 1;
    }

    #[must_use]
    pub fn sub(&self, t: Table) -> u64 {
        self.sub[t.index()]
    }

    #[must_use]
    pub fn sub_hashes(&self) -> [u64; TABLE_COUNT] {
        self.sub
    }

    /// The per-tick value: xxh3 over the version byte and the sub-hashes in
    /// [`Table::ALL`] order. Identical in both [`HashMode`]s by construction.
    pub fn fold(&mut self) -> u64 {
        self.fold_buf.clear();
        self.fold_buf.push(ENCODING_VERSION);
        for t in Table::ALL {
            self.fold_buf
                .extend_from_slice(&self.sub[t.index()].to_le_bytes());
        }
        xxh3_64_with_seed(&self.fold_buf, STATE_HASH_SEED)
    }

    /// The same fold, computed from a caller-supplied array. `tests/` uses it
    /// to check the incremental value against a from-scratch fold of the same
    /// sub-hashes without going through the cache at all.
    #[must_use]
    pub fn fold_of(subs: &[u64; TABLE_COUNT]) -> u64 {
        let mut buf: Vec<u8> = Vec::with_capacity(1 + 8 * TABLE_COUNT);
        buf.push(ENCODING_VERSION);
        for s in subs {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        xxh3_64_with_seed(&buf, STATE_HASH_SEED)
    }
}
