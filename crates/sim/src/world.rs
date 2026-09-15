// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The world, the named tick phases and the per-tick state hash.
//!
//! # What this world is today, said plainly
//!
//! T2 owns the determinism core, not the game. The world below carries the
//! smallest set of hashed tables that can exercise the canonical encoder, the
//! `SoA` tables, the CSR broadphase, the split RNG streams and the hash over a
//! real 1 200-tick chain: seats, and units that walk to a destination and draw
//! a new one when they arrive.
//!
//! * T5 brings the chunk store, the real world tables and the seeded map.
//! * T7 replaces straight-line walking with path following.
//! * T10 brings the Lull / Push / recap runner around this loop.
//! * T11 drives destinations from the playbook interpreter.
//! * T14 fills the economy, the Build mandate, Survey-lite and the programs.
//!
//! Every one of those pull requests **moves the hash chain**, and each one owes
//! the explanation (AGENTS.md §4.8, §10 item 2). The values marked
//! `PLACEHOLDER (harness)` below are scaffolding those tasks delete, not tuning
//! values: they are not in the rules table because they are not rules.
//!
//! # The phase order is fixed
//!
//! [`PHASE_ORDER`] is the tick, in the order spec section 15 sets out, and
//! [`World::step`] runs exactly it. Most phases are empty and name the task
//! that fills them. The order is determinism code: reordering it is a contract
//! change (AGENTS.md §5).
//!
//! # The hash is full, every tick
//!
//! Item 66: re-hash every table every tick, in declared order, into a **reused**
//! buffer, with the chunk store entering as per-chunk digests. The incremental
//! alternative was measured and rejected — 7.641 of 8 tables are dirty on
//! essentially every tick, so it saves 0.02 % of the gate tick and buys that
//! with a dirty flag on every write site in the sim, forever, on the
//! determinism artefact itself.

use crate::chunks::ChunkDigests;
use crate::encoding::Enc;
use crate::math::fixed::{Angle, Fx, Sq, cos, sin};
use crate::math::quantity::{Hp, Kw, Money, Tick};
use crate::math::random::{Stream, StreamRng};
use crate::rules::RulesTable;
use crate::seams::WorkCounter;
use crate::tables::{Csr, MovementColumns, SeatId, SeatTable, UnitId, UnitTable};

/// The eleven named phases of a tick, in the order they run.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Phase {
    /// Rebuild the CSR uniform grid from this tick's positions (T2).
    Broadphase,
    /// Run units' and buildings' built-in programs (T14).
    Programs,
    /// Advance every mover (T2 in its harness form; T7 replaces it with path
    /// following).
    Movement,
    /// Resolve fire and damage (S2).
    Combat,
    /// Apportion per-seat kill credit by largest remainder, ties to the lowest
    /// seat id (T14).
    KillCredit,
    /// Settle supply, draw, the brownout order and dormancy (T14).
    Power,
    /// Hold fabricator orders and fill spend requests by the urgency ladder
    /// (T14).
    Quartermaster,
    /// The playbook interpreter's decision tick, every 250 ms of game time
    /// (T11).
    Decision,
    /// Serve the per-tick repath cap and repair the abstract graph (T7).
    Pathing,
    /// Apply voxel edits and refresh the chunks they touched (T5).
    Voxels,
    /// Encode the world in declared table order and digest it (T2).
    Hash,
}

/// The tick, in order. Determinism code: see the module docs.
pub const PHASE_ORDER: [Phase; 11] = [
    Phase::Broadphase,
    Phase::Programs,
    Phase::Movement,
    Phase::Combat,
    Phase::KillCredit,
    Phase::Power,
    Phase::Quartermaster,
    Phase::Decision,
    Phase::Pathing,
    Phase::Voxels,
    Phase::Hash,
];

/// PLACEHOLDER (harness): walking speed, Q16.16 voxels per tick.
/// `0.5 voxels/tick` is `10 voxels/s`. Deleted when T14 gives the commander and
/// the drones their real speeds as rules-table rows (decision 14).
const HARNESS_SPEED: Fx = Fx::from_raw(32_768);

/// PLACEHOLDER (harness): turn rate in [`Angle`] units per tick.
/// `512/65 536` of a turn is about 2.8° per tick.
const HARNESS_TURN_RATE: u16 = 512;

/// PLACEHOLDER (harness): the map footprint the broadphase covers, in voxels.
/// About 384 × 384 is the skeleton's provisional map (skeleton plan §1.1);
/// T5's generator replaces it, and the dimensions are Tuning (decision 14).
const HARNESS_MAP_EXTENT_VOXELS: [i32; 2] = [384, 384];

/// PLACEHOLDER (harness): the largest radius a broadphase query asks for, in
/// cells. Item 67 sizes the cell edge to the largest query radius so a query
/// touches at most 3 × 3 cells; until S2 has a real query radius, one cell is
/// what that means.
pub const BROADPHASE_QUERY_RADIUS_CELLS: u32 = 1;

/// PLACEHOLDER (harness): what a harness unit is built with, in hit points.
/// Nothing damages it — combat is S2 — so the number only has to be alive and
/// in the hash. T14 replaces it with a rules-table row per unit kind
/// (decision 14).
const HARNESS_UNIT_HP: Hp = Hp::new(150);

/// PLACEHOLDER (harness): the per-tick work budget the seam starts with. The
/// number becomes meaningful at S5, when the operator's budget is measured in
/// evaluation units (owner).
const HARNESS_WORK_BUDGET: u32 = 1_000_000;

/// How to build a world.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WorldConfig {
    /// The match seed. The sim is a pure function of this, the playbooks and
    /// the rules hash.
    pub match_seed: u64,
    /// How many seats share the match. At most three in v1; the determinism
    /// harness runs more because a wider table is a wider hash.
    pub seats: u32,
    /// How many units each seat fields.
    pub units_per_seat: u32,
    /// How many chunks the map's digest block covers.
    pub chunk_count: u32,
    /// The rules table, an **input** to the sim and never hashed state.
    pub rules: RulesTable,
}

/// The authoritative world: the hashed tables, the derived indexes and the
/// tick loop.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct World {
    // --- hashed state, in declared encoding order ---
    match_seed: u64,
    tick: Tick,
    seats: SeatTable,
    units: UnitTable,
    chunks: ChunkDigests,

    // --- inputs and derived state: never encoded, never hashed ---
    rules: RulesTable,
    broadphase: Csr,
    work: WorkCounter,
    /// Scratch for broadphase queries, so a query allocates nothing.
    candidates: Vec<u32>,
}

impl World {
    /// Build a world and place its units from [`Stream::Spawn`].
    ///
    /// Returns `None` when the rules table cannot describe a grid — a
    /// non-positive cell size, or a footprint that does not fit — which is a
    /// rules-table mistake caught once, here, rather than every tick.
    #[must_use]
    pub fn new(config: &WorldConfig) -> Option<World> {
        let unit_count = config.seats.checked_mul(config.units_per_seat)?;
        let mut seats = SeatTable::with_capacity(config.seats);
        let mut units = UnitTable::with_capacity(unit_count);

        let extent = HARNESS_MAP_EXTENT_VOXELS;
        let max_x = i16::try_from(*extent.first()?).unwrap_or(i16::MAX);
        let max_y = i16::try_from(*extent.get(1)?).unwrap_or(i16::MAX);

        let mut next_id: u32 = 0;
        let mut seat_index: u32 = 0;
        while seat_index < config.seats {
            let seat = SeatId::new(u8::try_from(seat_index).unwrap_or(u8::MAX));
            seats.push(seat, Money::ZERO, Kw::ZERO, Kw::ZERO);
            let mut n: u32 = 0;
            while n < config.units_per_seat {
                let mut rng = StreamRng::new(
                    config.match_seed,
                    Stream::Spawn,
                    Tick::ZERO.raw(),
                    seat.raw(),
                    next_id,
                );
                let pos = draw_point(&mut rng, max_x, max_y);
                let dest = draw_point(&mut rng, max_x, max_y);
                units.push(UnitId::new(next_id), seat, pos, dest, HARNESS_UNIT_HP);
                next_id = next_id.checked_add(1)?;
                n = n.checked_add(1)?;
            }
            seat_index = seat_index.checked_add(1)?;
        }

        let broadphase = Csr::new(
            [0, 0],
            extent,
            config.rules.csr_cell_size_voxels,
            unit_count,
        )?;

        Some(World {
            match_seed: config.match_seed,
            tick: Tick::ZERO,
            seats,
            units,
            chunks: ChunkDigests::new(config.chunk_count),
            rules: config.rules.clone(),
            broadphase,
            work: WorkCounter::with_budget(HARNESS_WORK_BUDGET),
            candidates: Vec::with_capacity(usize::try_from(unit_count).unwrap_or(0)),
        })
    }

    /// The match seed.
    #[must_use]
    pub const fn match_seed(&self) -> u64 {
        self.match_seed
    }

    /// The current tick.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The seat table.
    #[must_use]
    pub const fn seats(&self) -> &SeatTable {
        &self.seats
    }

    /// The unit table.
    #[must_use]
    pub const fn units(&self) -> &UnitTable {
        &self.units
    }

    /// The chunk store's digests.
    #[must_use]
    pub const fn chunks(&self) -> &ChunkDigests {
        &self.chunks
    }

    /// The chunk store's digests, for the voxel phase to refresh.
    pub const fn chunks_mut(&mut self) -> &mut ChunkDigests {
        &mut self.chunks
    }

    /// The rules table this world runs under. An input, never hashed state.
    #[must_use]
    pub const fn rules(&self) -> &RulesTable {
        &self.rules
    }

    /// The broadphase index. Derived, never hashed.
    #[must_use]
    pub const fn broadphase(&self) -> &Csr {
        &self.broadphase
    }

    /// This tick's work counter. Not hashed today; see [`WorkCounter`].
    #[must_use]
    pub const fn work(&self) -> WorkCounter {
        self.work
    }

    /// This tick's work counter, to charge against.
    ///
    /// Nothing in the tick charges it yet — the phases that will are T11's and
    /// S5's. It is reachable now so that
    /// `tests/determinism.rs`'s `the_work_counter_is_not_in_the_state_encoding`
    /// can prove the counter stays out of the hash.
    pub const fn work_mut(&mut self) -> &mut WorkCounter {
        &mut self.work
    }

    /// Advance one tick and return the tick's state hash.
    ///
    /// `enc` is the caller's buffer, reused across ticks: that is what makes a
    /// tick allocation-free (G3′ §9.17), and `tests/allocations.rs` asserts it.
    pub fn step(&mut self, enc: &mut Enc) -> u64 {
        self.tick = self.tick.next().unwrap_or(self.tick);
        self.work.reset();
        let mut hash = 0;
        for phase in PHASE_ORDER {
            match phase {
                Phase::Broadphase => self.phase_broadphase(),
                Phase::Programs => self.phase_programs(),
                Phase::Movement => self.phase_movement(),
                Phase::Combat => self.phase_combat(),
                Phase::KillCredit => self.phase_kill_credit(),
                Phase::Power => self.phase_power(),
                Phase::Quartermaster => self.phase_quartermaster(),
                Phase::Decision => self.phase_decision(),
                Phase::Pathing => self.phase_pathing(),
                Phase::Voxels => self.phase_voxels(),
                Phase::Hash => hash = self.phase_hash(enc),
            }
        }
        hash
    }

    /// Rebuild the CSR uniform grid by counting sort (item 67).
    ///
    /// [`Csr::rebuild`] answers `false` when the unit table is larger than the
    /// grid it was built for, and leaves the grid **empty** — so swallowing
    /// that answer buys a world that hashes correctly and then queries an empty
    /// index for the rest of the match. [`World::new`] and
    /// [`World::restore_tables`] are the only two ways to get a grid, and both
    /// size it to the unit table, so a `false` here is a construction bug and
    /// the assertion is how it surfaces at the tick that caused it.
    fn phase_broadphase(&mut self) {
        let rebuilt = self.broadphase.rebuild(self.units.positions());
        debug_assert!(
            rebuilt,
            "the unit table outgrew the broadphase grid; every query from here on would come \
             back empty"
        );
    }

    /// Units' and buildings' built-in programs. **Empty: T14 fills it** (move,
    /// build, mine and scout).
    #[allow(
        clippy::unused_self,
        reason = "an empty phase stub keeps the tick's shape visible; T14 fills the body"
    )]
    const fn phase_programs(&mut self) {}

    /// Advance every mover.
    ///
    /// The harness rule, in full: turn at most [`HARNESS_TURN_RATE`] toward the
    /// destination, step [`HARNESS_SPEED`] along the new heading, and on
    /// arrival draw a fresh destination from [`Stream::Spawn`] at
    /// `(tick, seat, unit id)`. Arrival is a **squared** comparison; nothing in
    /// this crate takes a square root (AGENTS.md §4.2).
    ///
    /// T7 replaces this with path following over HPA\*'s route.
    fn phase_movement(&mut self) {
        let match_seed = self.match_seed;
        let tick = self.tick.raw();
        let extent = HARNESS_MAP_EXTENT_VOXELS;
        let max_x = i16::try_from(extent.first().copied().unwrap_or(0)).unwrap_or(i16::MAX);
        let max_y = i16::try_from(extent.get(1).copied().unwrap_or(0)).unwrap_or(i16::MAX);
        let lo = Fx::ZERO;
        let hi_x = Fx::from_voxels(max_x);
        let hi_y = Fx::from_voxels(max_y);
        // Arrival inside two steps, so a heading quantised to one table entry
        // cannot orbit a destination forever.
        let arrival = Sq::of_radius(HARNESS_SPEED.saturating_scale(2));

        let MovementColumns {
            ids,
            seats,
            hit_points,
            positions,
            destinations,
            headings,
        } = self.units.movement_columns();

        for (index, position) in positions.iter_mut().enumerate() {
            let Some(hp) = hit_points.get(index) else {
                continue;
            };
            if !hp.is_alive() {
                continue;
            }
            let Some(destination) = destinations.get_mut(index) else {
                continue;
            };
            let Some(heading) = headings.get_mut(index) else {
                continue;
            };

            if Sq::between(*position, *destination) <= arrival {
                let seat = seats.get(index).copied().unwrap_or(0);
                let id = ids.get(index).copied().unwrap_or(0);
                let mut rng = StreamRng::new(match_seed, Stream::Spawn, tick, seat, id);
                *destination = draw_point(&mut rng, max_x, max_y);
            }

            let dx = destination
                .first()
                .copied()
                .unwrap_or(Fx::ZERO)
                .saturating_sub(position.first().copied().unwrap_or(Fx::ZERO));
            let dy = destination
                .get(1)
                .copied()
                .unwrap_or(Fx::ZERO)
                .saturating_sub(position.get(1).copied().unwrap_or(Fx::ZERO));
            let want = Angle::from_delta(dx, dy);
            *heading = heading.turn_toward(want, HARNESS_TURN_RATE);

            let step_x = cos(*heading).saturating_mul_fx(HARNESS_SPEED);
            let step_y = sin(*heading).saturating_mul_fx(HARNESS_SPEED);
            if let Some(x) = position.first_mut() {
                *x = x.saturating_add(step_x).clamp_to(lo, hi_x);
            }
            if let Some(y) = position.get_mut(1) {
                *y = y.saturating_add(step_y).clamp_to(lo, hi_y);
            }
        }
    }

    /// Fire and damage. **Empty: S2 fills it.** `Stream::Combat` is reserved
    /// for it and is drawn from nowhere else.
    #[allow(
        clippy::unused_self,
        reason = "an empty phase stub keeps the tick's shape visible; S2 fills the body"
    )]
    const fn phase_combat(&mut self) {}

    /// Per-seat kill-credit counters, at most three per asset, apportioned by
    /// largest remainder with ties to the lowest seat id. **Empty: T14 fills
    /// it**, and hashes the counters from the day they exist even though combat
    /// is S2 — a field that affects behaviour and is not hashed is a latent
    /// desync.
    #[allow(
        clippy::unused_self,
        reason = "an empty phase stub keeps the tick's shape visible; T14 fills the body"
    )]
    const fn phase_kill_credit(&mut self) {}

    /// Supply, draw, brownout order, dormancy, the revive margin. **Empty: T14
    /// fills it.**
    #[allow(
        clippy::unused_self,
        reason = "an empty phase stub keeps the tick's shape visible; T14 fills the body"
    )]
    const fn phase_power(&mut self) {}

    /// Hold fabricator orders, then the brownout order; fill spend requests by
    /// the urgency ladder, round-robin within a band. **Empty: T14 fills it**,
    /// behind the `Quartermaster` trait that is also T14's.
    #[allow(
        clippy::unused_self,
        reason = "an empty phase stub keeps the tick's shape visible; T14 fills the body"
    )]
    const fn phase_quartermaster(&mut self) {}

    /// The interpreter's decision tick, one per 250 ms of game time. **Empty:
    /// T11 fills it.**
    #[allow(
        clippy::unused_self,
        reason = "an empty phase stub keeps the tick's shape visible; T11 fills the body"
    )]
    const fn phase_decision(&mut self) {}

    /// Serve the repath cap round-robin by `(seat, beacon, unit)` and repair
    /// the abstract graph. **Empty: T7 fills it**, with the connectivity oracle
    /// from day one and "sealed in" as a designed state.
    #[allow(
        clippy::unused_self,
        reason = "an empty phase stub keeps the tick's shape visible; T7 fills the body"
    )]
    const fn phase_pathing(&mut self) {}

    /// Apply voxel edits and refresh the digests of the chunks they touched.
    /// **Empty: T5 fills it**, through [`ChunkDigests::refresh`].
    #[allow(
        clippy::unused_self,
        reason = "an empty phase stub keeps the tick's shape visible; T5 fills the body"
    )]
    const fn phase_voxels(&mut self) {}

    /// Encode the world and digest it.
    fn phase_hash(&self, enc: &mut Enc) -> u64 {
        self.encode(enc);
        enc.finish()
    }

    /// Append the world to the canonical encoding, in **declared table order**.
    ///
    /// The order below is the contract. Adding a table means appending it here,
    /// adding it to the snapshot round-trip and re-blessing the goldens, all in
    /// the same pull request (AGENTS.md §4.8).
    ///
    /// 1. header — match seed, tick
    /// 2. seats
    /// 3. units
    /// 4. chunk digests
    ///
    /// The rules table, the broadphase and the work counter are **not** here:
    /// they are inputs and derived state, and `tests/determinism.rs` asserts
    /// that changing a performance knob leaves the chain byte-identical.
    pub fn encode(&self, enc: &mut Enc) {
        enc.clear();
        enc.u64(self.match_seed);
        enc.u32(self.tick.raw());

        enc.len(self.seats.len());
        for (index, seat) in self.seats.seats().iter().enumerate() {
            enc.u8(*seat);
            enc.i64(
                self.seats
                    .treasuries()
                    .get(index)
                    .copied()
                    .unwrap_or(Money::ZERO)
                    .raw(),
            );
            enc.i32(
                self.seats
                    .supplies()
                    .get(index)
                    .copied()
                    .unwrap_or(Kw::ZERO)
                    .raw(),
            );
            enc.i32(
                self.seats
                    .draws()
                    .get(index)
                    .copied()
                    .unwrap_or(Kw::ZERO)
                    .raw(),
            );
        }

        enc.len(self.units.len());
        let positions = self.units.positions();
        let destinations = self.units.destinations();
        let headings = self.units.headings();
        let hit_points = self.units.hit_points();
        for (index, id) in self.units.ids().iter().enumerate() {
            enc.u32(*id);
            enc.u8(self.units.seats().get(index).copied().unwrap_or(0));
            encode_point(enc, positions.get(index));
            encode_point(enc, destinations.get(index));
            enc.u16(headings.get(index).copied().unwrap_or(Angle::ZERO).raw());
            enc.i32(hit_points.get(index).copied().unwrap_or(Hp::ZERO).raw());
        }

        self.chunks.encode(enc);
    }

    /// The per-tick state hash, allocating its own encoder.
    ///
    /// The tick loop uses [`World::step`]'s reused buffer instead; this form is
    /// for tests, snapshots and one-off checks.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        let mut enc = Enc::with_capacity(16 * 1024);
        self.encode(&mut enc);
        enc.finish()
    }

    /// Candidate ids near `centre`, sorted ascending, into the world's own
    /// scratch buffer.
    ///
    /// Allocation-free after construction, and independent of the broadphase's
    /// cell size — see [`Csr::collect_in_radius`].
    pub fn candidates_near(&mut self, centre: [Fx; 3], radius_cells: u32) -> &[u32] {
        let mut scratch = core::mem::take(&mut self.candidates);
        self.broadphase
            .collect_in_radius(centre, radius_cells, &mut scratch);
        self.candidates = scratch;
        &self.candidates
    }

    /// Replace the hashed tables from a restored snapshot, and **resize the
    /// derived index to match them**.
    ///
    /// The broadphase is sized for a unit count at construction. A snapshot
    /// holding more units than the receiving world was built for would
    /// otherwise restore cleanly, hash correctly, and leave every later
    /// broadphase query empty — see [`World::phase_broadphase`]. So the grid is
    /// rebuilt here, for the restored count, before anything is written.
    ///
    /// Returns `false` and changes **nothing** when the grid cannot be built
    /// for the restored world, the same answer [`World::new`] gives for the
    /// same reason. A half-applied restore is not a state this sim has.
    ///
    /// Crate-internal: [`crate::snapshot`] is the only caller, so a restore
    /// cannot skip the version check.
    pub(crate) fn restore_tables(
        &mut self,
        match_seed: u64,
        tick: Tick,
        seats: SeatTable,
        units: UnitTable,
        chunks: ChunkDigests,
    ) -> bool {
        let unit_count = units.len();
        let Some(broadphase) = Csr::new(
            [0, 0],
            HARNESS_MAP_EXTENT_VOXELS,
            self.rules.csr_cell_size_voxels,
            unit_count,
        ) else {
            return false;
        };

        self.match_seed = match_seed;
        self.tick = tick;
        self.seats = seats;
        self.units = units;
        self.chunks = chunks;
        self.broadphase = broadphase;
        // The query scratch is sized the same way, and for the same reason: a
        // query that has to grow its buffer is an allocation inside a tick.
        self.candidates = Vec::with_capacity(usize::try_from(unit_count).unwrap_or(0));
        true
    }
}

fn encode_point(enc: &mut Enc, point: Option<&[Fx; 3]>) {
    let p = point.copied().unwrap_or([Fx::ZERO; 3]);
    for axis in p {
        enc.i32(axis.raw());
    }
}

/// A point drawn uniformly over the harness footprint.
fn draw_point(rng: &mut StreamRng, max_x: i16, max_y: i16) -> [Fx; 3] {
    let x = rng.range_i32(0, i32::from(max_x).saturating_sub(1));
    let y = rng.range_i32(0, i32::from(max_y).saturating_sub(1));
    [
        Fx::from_voxels(i16::try_from(x).unwrap_or(0)),
        Fx::from_voxels(i16::try_from(y).unwrap_or(0)),
        Fx::ZERO,
    ]
}
