// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rules table and `rules_hash` (decisions log item 78).
//!
//! **Tuning values are data** (AGENTS.md §12): K and B, 10/14/4, the move cost
//! per tick, the repath cap, the segment ladder and the CSR cell size sit in
//! one reviewable file — `rules/rules.v1.json`, the canonical JSON of one
//! [`gp::v1::RulesTable`] — rather than as constants sprinkled through the
//! code. The sim decodes that file with `pharmakos-proto`'s canonical JSON
//! codec and hashes it with the canonical encoder of item 48, so `rules_hash`
//! is one of the verifier's five `report_hash` inputs **by construction**
//! rather than by agreement.
//!
//! The sim is a pure function of `(map seed, playbooks, rules hash)`. That
//! sentence is only true if the rules table is an *input*: nothing in this
//! module enters the state hash, and `tests/determinism.rs` asserts it twice —
//! once over the canonical encoding and once over the chain itself.
//!
//! # `rules_hash` covers the whole table, not the rows the sim reads
//!
//! [`RulesTable::encode`] hashes the **canonical JSON of the whole decoded
//! message**: every row, including the ones this crate never looks at, plus
//! `revision` and `note`.
//!
//! It is deliberately not a walk over the sim's own fields, because the rows
//! the sim does not read are exactly the ones the *verifier* and *plan-core*
//! will: `interface_times.*` is T10's and T11's interface arithmetic,
//! `locomotion.fog_cost_*` and `hpa_cluster_voxels` are T7's estimator (item
//! 61), `match.lull_ms` is the host's. A hash over nine hand-picked fields
//! would let a tuning pull request change
//! `interface_times.place_beacon_deploy_ms` — changing what the verifier
//! reports — while leaving `report_hash`, and every report cached under it,
//! exactly where it was. Hashing the message closes that by construction, and
//! it cannot be reopened by forgetting to mirror a new row here.
//!
//! Two consequences, both intended:
//!
//! * Editing `note` moves `rules_hash`. The hash identifies the *table*, and
//!   `revision` is meant to move whenever a value does, so an edit that moves
//!   neither is an edit that changed nothing.
//! * The canonical form is the codec's, so a reformatted file hashes the same
//!   as the file it was reformatted from. `crates/proto`'s
//!   `the_rules_table_is_in_canonical_form` keeps the committed file identical
//!   to that form anyway.
//!
//! # The two shapes, and why there are two
//!
//! [`gp::v1::RulesTable`] is the **contract shape**: nested blocks, every row
//! the project will need, guarded by `buf breaking` (item 78). [`RulesTable`]
//! here is the **sim's view**: the flat subset the tick actually reads, derived
//! by [`RulesTable::from_message`] and by nothing else, so a row cannot reach
//! the sim without passing through the schema. The view is read-only accessors
//! rather than public fields, because a field that could be mutated after
//! construction could disagree with the message the hash is over — which is
//! the same bug in a smaller box.
//!
//! | Accessor | `gp.v1.RulesTable` |
//! |---|---|
//! | [`mesher_drain_surfaces`](RulesTable::mesher_drain_surfaces) | `mesher.surfaces_per_frame` |
//! | [`mesher_drain_bytes`](RulesTable::mesher_drain_bytes) | `mesher.bytes_per_frame` |
//! | [`step_cardinal`](RulesTable::step_cardinal) | `locomotion.step_cost_cardinal` |
//! | [`step_diagonal`](RulesTable::step_diagonal) | `locomotion.step_cost_diagonal` |
//! | [`climb_surcharge`](RulesTable::climb_surcharge) | `locomotion.climb_surcharge` |
//! | [`move_cost_per_tick`](RulesTable::move_cost_per_tick) | `locomotion.move_cost_per_tick` (superseded by item 90's per-kind rows; unread) |
//! | [`repath_cap_per_tick`](RulesTable::repath_cap_per_tick) | `locomotion.repath_cap_per_tick` |
//! | [`csr_cell_size_voxels`](RulesTable::csr_cell_size_voxels) | `broadphase.cell_size_voxels` |
//! | [`segment_lengths_ms`](RulesTable::segment_lengths_ms) | `match.segment_lengths_ms` |
//! | [`map_size_voxels`](RulesTable::map_size_voxels) | `map.size_x` / `size_y` / `size_z` |
//! | [`commander_cost_per_second`](RulesTable::commander_cost_per_second) | `commander.cost_per_second` |
//! | [`hpa_cluster_voxels`](RulesTable::hpa_cluster_voxels) | `locomotion.hpa_cluster_voxels` |
//! | [`fog_cost_numerator`](RulesTable::fog_cost_numerator), [`fog_cost_denominator`](RulesTable::fog_cost_denominator) | `locomotion.fog_cost_numerator` / `_denominator` |
//! | [`unit_hp`](RulesTable::unit_hp), [`unit_draw_kw`](RulesTable::unit_draw_kw) | `units.<kind>.hp` / `.draw_kw` |
//! | [`cost_per_second`](RulesTable::cost_per_second) | `commander.cost_per_second`, `units.<kind>.cost_per_second` |
//! | [`mining_carry_voxels`](RulesTable::mining_carry_voxels) | `economy.mining_carry_voxels` |
//! | [`sphere_radius_voxels`](RulesTable::sphere_radius_voxels) | `beacon.sphere_radius_voxels` |
//! | [`arrive_radius_voxels`](RulesTable::arrive_radius_voxels), [`interface_range_voxels`](RulesTable::interface_range_voxels) | `commander.arrive_radius_voxels` / `.interface_range_voxels` |
//! | [`build_hp_per_tick`](RulesTable::build_hp_per_tick) | `structures.build_hp_per_second` |
//! | [`power`](RulesTable::power) | the `power` block and the `structures.*.draw_kw` rows ([`crate::power::PowerRules`]) |
//!
//! The map generator ([`crate::mapgen`]) reads a dozen more rows — the whole of
//! `map`, `beacon`, `power`, `economy` and `units` — and reads them from
//! [`RulesTable::message`] rather than through accessors here. That is
//! deliberate: those rows are read **once, at generation**, by one module that
//! already has to report a rules-table mistake as a typed error, and mirroring
//! them into this flat view would be a second copy of numbers the view's own
//! rule (read-only accessors, derived from the message and nothing else) exists
//! to prevent. The rows the *tick* reads are the ones that earn an accessor.
//!
//! A row the sim starts reading joins that table and gains an accessor in the
//! task that first reads it. That is an ordinary change now: it does not move
//! `rules_hash`, because the hash already covered the row.

use crate::encoding::Enc;
use crate::math::quantity::{Hp, Kw, Money, TICK_HZ};
use crate::power::{PowerRules, PowerRulesError};
use crate::tables::{StructureKind, UnitKind};
use pharmakos_proto::gp;
use std::fmt;
use std::num::NonZeroU32;

/// The disk location of the canonical rules table, relative to the repository
/// root.
pub const RULES_PATH: &str = "rules/rules.v1.json";

/// The message the file on disk holds exactly one of.
pub const RULES_MESSAGE: &str = "gp.v1.RulesTable";

/// The rules *encoding's* version, part of the hashed bytes below.
///
/// Not the table's `revision`, which counts value changes and is the file's to
/// carry: this counts changes to [`RulesTable::encode`], and moving it is a
/// determinism change (AGENTS.md §5).
pub const RULES_VERSION: u32 = 1;

/// How many segment lengths the ladder may carry. Fixed so the table's
/// encoding has a bound; the skeleton's ladder is 3 / 5 / 8 minutes (item 68).
pub const MAX_SEGMENT_LENGTHS: usize = 8;

/// One rules table: the decoded message, its canonical bytes, and the flat
/// view the tick reads.
///
/// Built only by [`RulesTable::from_message`] and the two loaders above it, so
/// the view and the bytes can never describe two different tables.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RulesTable {
    /// The whole decoded table, kept so `rules_hash` can cover all of it.
    message: gp::v1::RulesTable,
    /// `message` rendered back through the canonical JSON codec, once, at
    /// construction. These are the bytes [`RulesTable::encode`] hashes.
    canonical: String,
    /// Surfaces the mesher may drain per frame. `K = 4` (item 54). From
    /// `mesher.surfaces_per_frame`.
    ///
    /// PLACEHOLDER: K, the surfaces the mesher drains per frame — owner, at S6's art pass, as `rules.proto` says (item 124 (5) (m)).
    /// No measured frame-time reason separates K = 4 from K = 8 on the spike
    /// machine (item 54).
    mesher_drain_surfaces: u32,
    /// Bytes the mesher may drain per frame. `B = 512 KiB` (item 54). From
    /// `mesher.bytes_per_frame`.
    ///
    /// PLACEHOLDER: B, as above — owner, at S6's art pass.
    mesher_drain_bytes: u32,
    /// Cost of a cardinal step. `10` (item 59). From
    /// `locomotion.step_cost_cardinal`.
    step_cardinal: i32,
    /// Cost of a diagonal step. `14` (item 59). From
    /// `locomotion.step_cost_diagonal`.
    step_diagonal: i32,
    /// Surcharge for a one-voxel climb. `4` (item 59). From
    /// `locomotion.climb_surcharge`.
    ///
    /// PLACEHOLDER: `no gameplay evidence behind it` (item 59) — owner, at S3.
    climb_surcharge: i32,
    /// Path cost a unit covers per tick. `3` (item 59). From
    /// `locomotion.move_cost_per_tick`.
    ///
    /// **SUPERSEDED** by item 90's per-kind `locomotion.*_cost_per_second`
    /// rows, which express the same model in cost units per second: T7
    /// integrates those with an accumulator (add `cost_per_second` per tick,
    /// spend one cost unit per 20 accumulated) and estimates with
    /// `ceil(cost * 20 / cost_per_second)` ticks, which is where item 59's
    /// **ceiling** rule now lives. Nothing reads this row; it stays in the
    /// schema because deleting a field is a `buf breaking` failure in
    /// `WIRE_JSON` mode.
    move_cost_per_tick: i32,
    /// Repaths served per tick, round-robin by `(seat, beacon, unit)`. `16`
    /// (item 69). From `locomotion.repath_cap_per_tick`.
    ///
    /// PLACEHOLDER: the repath cap — owner, at S2's exit, once the burst frequency is a measurement (item 69).
    repath_cap_per_tick: u32,
    /// The broadphase's cell edge, in whole voxels. From
    /// `broadphase.cell_size_voxels`.
    ///
    /// PLACEHOLDER: the cell edge, tied to unit density — owner, at S2's exit (item 67's caveat).
    /// A performance knob, so it must never reach hashed state; the
    /// broadphase takes its query radius in voxels and filters it exactly,
    /// precisely so that it cannot.
    csr_cell_size_voxels: i32,
    /// The per-round segment ladder, in game milliseconds (item 68: 3 / 5 / 8
    /// minutes). The runner reads the coming segment's length **from the frozen
    /// snapshot**, not from this row (T10). From `match.segment_lengths_ms`.
    segment_lengths_ms: Vec<i32>,
    /// The map extent in voxels, `[x, y, z]`. From `map.size_*`.
    ///
    /// PLACEHOLDER: the map's size — owner, at S4, with the symmetric map (item 90; the skeleton's demo kept it, and ruling 6 of S1's stage demo, decisions-log item 139 (1), kept it for S1).
    map_size_voxels: [i32; 3],
    /// [`RulesTable::map_last_voxel`].
    map_last_voxel: [i16; 3],
    /// The commander's walking speed in cost units per second. `10` (item 90).
    /// From `commander.cost_per_second`.
    ///
    /// PLACEHOLDER: the commander's speed — owner, at S2's demo, with `tune2`'s report of the spawn travel distance (item 90; the skeleton's demo kept it, and ruling 6 of S1's stage demo, decisions-log item 139 (1), kept it for S1).
    commander_cost_per_second: i32,
    /// The HPA\* cluster edge in voxels. `32` — one chunk footprint, a single
    /// abstract level (item 58). From `locomotion.hpa_cluster_voxels`.
    hpa_cluster_voxels: i32,
    /// The fog multiplier's numerator. `3` (item 61). From
    /// `locomotion.fog_cost_numerator`.
    fog_cost_numerator: i32,
    /// The fog multiplier's denominator. `2` (item 61). From
    /// `locomotion.fog_cost_denominator`.
    fog_cost_denominator: i32,
    /// The voxels a mining drone digs before it carries its load home. `16`
    /// (register S1-25). From `economy.mining_carry_voxels`. Refused at zero,
    /// which would send every drone home with empty hands on every pass.
    mining_carry_voxels: NonZeroU32,
    /// A beacon's sphere of authority in whole voxels, `24` in the committed
    /// table. From `beacon.sphere_radius_voxels`. Refused at load when the
    /// `beacon` block is missing or the radius does not fit a voxel
    /// coordinate, so the resolver's sphere test reads it as it is and never
    /// substitutes a radius (decisions-log item 134, `fog`).
    sphere_radius_voxels: i16,
    /// How close a walk has to get to count as arrived, in whole voxels. From
    /// `commander.arrive_radius_voxels`; refused at load when it is not a voxel
    /// length, so every range test reads it as it is (S1's `build` lane: the
    /// interpreter's range test used to clamp it).
    arrive_radius_voxels: i16,
    /// How close the commander stands to interface or deploy, in whole voxels.
    /// From `commander.interface_range_voxels`, refused at load as above.
    interface_range_voxels: i16,
    /// Hit points a working build drone adds per tick:
    /// `structures.build_hp_per_second` over the tick rate (the register's
    /// S1-24, read from its row since S1's `build` lane). Refused at load
    /// unless the row is a positive multiple of [`TICK_HZ`], so construction
    /// needs no accumulator and no rounding rule.
    build_hp_per_tick: Hp,
    /// Every `kW` row the power phase reads, refused at load by
    /// [`PowerRules::read`] (decisions-log item 135 (2) (b)), so that no world
    /// can hold a table the phase refuses.
    power: PowerRules,
    /// Every unit kind's and structure kind's price, hit points and walking
    /// speed, read typed at load ([`KindRows::read`]), so that no reader
    /// answers zero for a missing row (decisions-log item 136 (5)): a table
    /// missing one is refused here, once.
    kinds: KindRows,
}

/// One unit kind's row as the tick reads it: `units.<kind>.*`, or the
/// `commander` block for the commander.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnitRow {
    /// The build cost, `cost_dollars`. Zero for the commander, which is never
    /// bought: it has no `units` row and no price (spec section 4).
    pub cost: Money,
    /// The hit points at full condition. Refused at load at zero, which would
    /// be a unit dead on the tick it was made.
    pub hp: Hp,
    /// The power it draws, `draw_kw`. Zero for the commander, whose draw is
    /// none of the power phase's (`crate::power`).
    pub draw: Kw,
    /// Its walking speed in cost units per second (item 90).
    pub cost_per_second: i32,
}

/// One structure kind's row as the tick reads it: `structures.<kind>`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StructureRow {
    /// The build cost, `cost_dollars`.
    pub cost: Money,
    /// The hit points at full condition, refused at load at zero.
    pub hp: Hp,
}

/// Every kind row the tick reads, typed at load.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct KindRows {
    commander: UnitRow,
    build_drone: UnitRow,
    mining_drone: UnitRow,
    repair_drone: UnitRow,
    raider: UnitRow,
    scout: UnitRow,
    generator: StructureRow,
    autocannon: StructureRow,
    mortar: StructureRow,
    survey_post: StructureRow,
    resonance_spire: StructureRow,
    /// A placed beacon's row, `structures.beacon`.
    beacon: StructureRow,
    /// A seat's core's hit points, `beacon.core_hp`; its price is the placed
    /// beacon's ([`crate::world::World::beacon_cost`]).
    core_hp: Hp,
    /// One wall voxel's price, `structures.wall_cost_per_voxel`: a wall has no
    /// structure row by design (`rules.proto`), and is priced by the voxel.
    wall_cost_per_voxel: Money,
    /// Item 22's work items per drone, `economy.backlog_threshold_per_drone`.
    backlog_threshold_per_drone: u32,
    /// A mining drone's time per voxel, `economy.mining_ms_per_voxel`.
    mining_ms_per_voxel: i32,
    /// A recycled beacon's refund, `economy.recycle_refund_percent`.
    recycle_refund_percent: u32,
    /// The decision cadence, `match.decision_tick_ms`.
    decision_tick_ms: i32,
    /// The commander's respawn curve, `commander.respawn_base_ms` and
    /// `commander.respawn_growth_ms`, each refused at load when negative.
    respawn_base_ms: i32,
    respawn_growth_ms: i32,
}

impl KindRows {
    /// Read every kind row, refusing a missing block or row, a value past the
    /// sim's type for it, and zero hit points.
    fn read(message: &gp::v1::RulesTable) -> Result<KindRows, RulesError> {
        let commander = message
            .commander
            .as_ref()
            .ok_or(RulesError::MissingBlock("commander"))?;
        let units = message
            .units
            .as_ref()
            .ok_or(RulesError::MissingBlock("units"))?;
        let structures = message
            .structures
            .as_ref()
            .ok_or(RulesError::MissingBlock("structures"))?;
        let beacon = message
            .beacon
            .as_ref()
            .ok_or(RulesError::MissingBlock("beacon"))?;
        let economy = message
            .economy
            .as_ref()
            .ok_or(RulesError::MissingBlock("economy"))?;
        let unit = |name: &'static str,
                    row: Option<gp::v1::rules_table::UnitKind>|
         -> Result<UnitRow, RulesError> {
            let row = row.ok_or(RulesError::MissingBlock(name))?;
            Ok(UnitRow {
                cost: Money::new(i64::from(row.cost_dollars)),
                hp: hit_points(name, row.hp)?,
                draw: Kw::new(signed(name, row.draw_kw)?),
                cost_per_second: signed(name, row.cost_per_second)?,
            })
        };
        let structure = |name: &'static str,
                         row: Option<gp::v1::rules_table::StructureKind>|
         -> Result<StructureRow, RulesError> {
            let row = row.ok_or(RulesError::MissingBlock(name))?;
            Ok(StructureRow {
                cost: Money::new(i64::from(row.cost_dollars)),
                hp: hit_points(name, row.hp)?,
            })
        };
        let respawn = |field: &'static str, value: i32| -> Result<i32, RulesError> {
            if value < 0 {
                return Err(RulesError::OutOfRange {
                    field: field.to_owned(),
                    value: format!("{value}; a respawn delay is not negative"),
                });
            }
            Ok(value)
        };
        Ok(KindRows {
            commander: UnitRow {
                cost: Money::ZERO,
                hp: hit_points("commander.hp", commander.hp)?,
                draw: Kw::ZERO,
                cost_per_second: signed("commander.cost_per_second", commander.cost_per_second)?,
            },
            build_drone: unit("units.build_drone", units.build_drone)?,
            mining_drone: unit("units.mining_drone", units.mining_drone)?,
            repair_drone: unit("units.repair_drone", units.repair_drone)?,
            raider: unit("units.raider", units.raider)?,
            scout: unit("units.scout", units.scout)?,
            generator: structure("structures.generator", structures.generator)?,
            autocannon: structure("structures.autocannon", structures.autocannon)?,
            mortar: structure("structures.mortar", structures.mortar)?,
            survey_post: structure("structures.survey_post", structures.survey_post)?,
            resonance_spire: structure("structures.resonance_spire", structures.resonance_spire)?,
            beacon: structure("structures.beacon", structures.beacon)?,
            core_hp: hit_points("beacon.core_hp", beacon.core_hp)?,
            wall_cost_per_voxel: Money::new(i64::from(structures.wall_cost_per_voxel)),
            backlog_threshold_per_drone: economy.backlog_threshold_per_drone,
            mining_ms_per_voxel: economy.mining_ms_per_voxel,
            recycle_refund_percent: economy.recycle_refund_percent,
            decision_tick_ms: message
                .r#match
                .as_ref()
                .ok_or(RulesError::MissingBlock("match"))?
                .decision_tick_ms,
            respawn_base_ms: respawn("commander.respawn_base_ms", commander.respawn_base_ms)?,
            respawn_growth_ms: respawn("commander.respawn_growth_ms", commander.respawn_growth_ms)?,
        })
    }
}

/// A row's hit points as the tick reads them: a positive signed 32-bit count.
fn hit_points(field: &'static str, value: u32) -> Result<Hp, RulesError> {
    if value == 0 {
        return Err(RulesError::OutOfRange {
            field: field.to_owned(),
            value: "0; an asset with no hit points is dead on the tick it is made".to_owned(),
        });
    }
    signed(field, value).map(Hp::new)
}

impl RulesTable {
    /// Append the table to the canonical encoding: the encoding version, then
    /// the canonical JSON of the whole decoded message, length-prefixed.
    ///
    /// This is the function `rules_hash` is, and it is determinism code: a
    /// change to it moves every `report_hash` the verifier has ever produced.
    /// It covers every row of the table rather than the subset the sim reads —
    /// see this module's header for why that is the load-bearing part.
    pub fn encode(&self, enc: &mut Enc) {
        enc.u32(RULES_VERSION);
        let bytes = self.canonical.as_bytes();
        enc.len(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
        enc.bytes(bytes);
    }

    /// The rules hash: xxh3-64 over the canonical encoding at the project seed.
    ///
    /// One of the verifier's five `report_hash` inputs, and stamped into every
    /// save so a save made under different rules refuses to load (T17).
    #[must_use]
    pub fn rules_hash(&self) -> u64 {
        let mut enc = Enc::with_capacity(self.canonical.len().saturating_add(16));
        self.encode(&mut enc);
        enc.finish()
    }

    /// The whole decoded table, for a caller that needs a row the sim's own
    /// view does not carry.
    #[must_use]
    pub const fn message(&self) -> &gp::v1::RulesTable {
        &self.message
    }

    /// The canonical JSON `rules_hash` is taken over.
    #[must_use]
    pub fn canonical_json(&self) -> &str {
        &self.canonical
    }

    /// Surfaces the mesher may drain per frame (`mesher.surfaces_per_frame`).
    #[must_use]
    pub const fn mesher_drain_surfaces(&self) -> u32 {
        self.mesher_drain_surfaces
    }

    /// Bytes the mesher may drain per frame (`mesher.bytes_per_frame`).
    #[must_use]
    pub const fn mesher_drain_bytes(&self) -> u32 {
        self.mesher_drain_bytes
    }

    /// Cost of a cardinal step (`locomotion.step_cost_cardinal`).
    #[must_use]
    pub const fn step_cardinal(&self) -> i32 {
        self.step_cardinal
    }

    /// Cost of a diagonal step (`locomotion.step_cost_diagonal`).
    #[must_use]
    pub const fn step_diagonal(&self) -> i32 {
        self.step_diagonal
    }

    /// Surcharge for a one-voxel climb (`locomotion.climb_surcharge`).
    #[must_use]
    pub const fn climb_surcharge(&self) -> i32 {
        self.climb_surcharge
    }

    /// Path cost a unit covers per tick (`locomotion.move_cost_per_tick`).
    ///
    /// **Superseded** by item 90's per-kind `locomotion.*_cost_per_second`
    /// rows; see the field's documentation. Nothing reads this, and T7 should
    /// reach for the per-kind rows instead.
    #[must_use]
    pub const fn move_cost_per_tick(&self) -> i32 {
        self.move_cost_per_tick
    }

    /// Repaths served per tick (`locomotion.repath_cap_per_tick`).
    #[must_use]
    pub const fn repath_cap_per_tick(&self) -> u32 {
        self.repath_cap_per_tick
    }

    /// The broadphase's cell edge in voxels (`broadphase.cell_size_voxels`).
    #[must_use]
    pub const fn csr_cell_size_voxels(&self) -> i32 {
        self.csr_cell_size_voxels
    }

    /// The voxels a mining drone digs before it carries its load home
    /// (`economy.mining_carry_voxels`), never zero.
    #[must_use]
    pub const fn mining_carry_voxels(&self) -> NonZeroU32 {
        self.mining_carry_voxels
    }

    /// A beacon's sphere of authority in whole voxels
    /// (`beacon.sphere_radius_voxels`): never negative, and always a voxel
    /// coordinate, because a table whose radius is neither does not load.
    #[must_use]
    pub const fn sphere_radius_voxels(&self) -> i16 {
        self.sphere_radius_voxels
    }

    /// How close a walk has to get to count as arrived, in whole voxels
    /// (`commander.arrive_radius_voxels`): never negative, and always a voxel
    /// length, because a table whose radius is neither does not load.
    #[must_use]
    pub const fn arrive_radius_voxels(&self) -> i16 {
        self.arrive_radius_voxels
    }

    /// How close the commander stands to interface or deploy, in whole voxels
    /// (`commander.interface_range_voxels`), read as
    /// [`RulesTable::arrive_radius_voxels`] is.
    #[must_use]
    pub const fn interface_range_voxels(&self) -> i16 {
        self.interface_range_voxels
    }

    /// Hit points one working build drone adds per tick
    /// (`structures.build_hp_per_second` over [`TICK_HZ`]): positive, and
    /// exact, because a row that is not a positive multiple of the tick rate
    /// does not load.
    #[must_use]
    pub const fn build_hp_per_tick(&self) -> Hp {
        self.build_hp_per_tick
    }

    /// Every `kW` row the power phase reads ([`PowerRules`]). Infallible: a
    /// table [`PowerRules::read`] refuses, or under which a seat's supply or
    /// draw could leave a signed 32-bit `kW`, does not load.
    #[must_use]
    pub const fn power(&self) -> PowerRules {
        self.power
    }

    /// The per-round segment ladder (`match.segment_lengths_ms`).
    #[must_use]
    pub fn segment_lengths_ms(&self) -> &[i32] {
        &self.segment_lengths_ms
    }

    /// The map extent in voxels, `[x, y, z]` (`map.size_x` / `size_y` /
    /// `size_z`).
    #[must_use]
    pub const fn map_size_voxels(&self) -> [i32; 3] {
        self.map_size_voxels
    }

    /// The map's last voxel on each axis, `size - 1`, as the `i16` a voxel
    /// coordinate is: refused at load when an axis is empty or wider than a
    /// coordinate holds, so every voxel of the map is a coordinate.
    #[must_use]
    pub const fn map_last_voxel(&self) -> [i16; 3] {
        self.map_last_voxel
    }

    /// The commander's walking speed in cost units per second
    /// (`commander.cost_per_second`).
    #[must_use]
    pub const fn commander_cost_per_second(&self) -> i32 {
        self.commander_cost_per_second
    }

    /// The HPA\* cluster edge in voxels (`locomotion.hpa_cluster_voxels`).
    ///
    /// Item 58: 32, one chunk footprint, so a crater's dirty chunk list is its
    /// dirty cluster list with no translation.
    #[must_use]
    pub const fn hpa_cluster_voxels(&self) -> i32 {
        self.hpa_cluster_voxels
    }

    /// The fog multiplier's numerator (`locomotion.fog_cost_numerator`).
    #[must_use]
    pub const fn fog_cost_numerator(&self) -> i32 {
        self.fog_cost_numerator
    }

    /// The fog multiplier's denominator (`locomotion.fog_cost_denominator`).
    #[must_use]
    pub const fn fog_cost_denominator(&self) -> i32 {
        self.fog_cost_denominator
    }

    /// One kind's walking speed in cost units per second (item 90).
    ///
    /// The commander is not a unit kind in the schema — it has a block of its
    /// own — so its row is `commander.cost_per_second` and every other kind's
    /// is `units.<kind>.cost_per_second`. One accessor over both, because the
    /// caller has a [`UnitKind`] and should not have to know which block the
    /// number lives in.
    ///
    /// `None` when the `units` block or that kind's row is absent.
    #[must_use]
    pub const fn cost_per_second(&self, kind: UnitKind) -> i32 {
        self.unit(kind).cost_per_second
    }

    /// One unit kind's hit points at full condition: `units.<kind>.hp`, or
    /// `commander.hp` for the commander. Every row is refused at load when it
    /// is missing or out of range, so this has an answer for every kind.
    #[must_use]
    pub const fn unit_hp(&self, kind: UnitKind) -> Hp {
        self.unit(kind).hp
    }

    /// One unit kind's power draw, `units.<kind>.draw_kw` (zero for the
    /// commander).
    #[must_use]
    pub const fn unit_draw_kw(&self, kind: UnitKind) -> Kw {
        self.unit(kind).draw
    }

    /// One unit kind's whole row ([`UnitRow`]).
    #[must_use]
    pub const fn unit(&self, kind: UnitKind) -> UnitRow {
        match kind {
            UnitKind::Commander => self.kinds.commander,
            UnitKind::BuildDrone => self.kinds.build_drone,
            UnitKind::MiningDrone => self.kinds.mining_drone,
            UnitKind::RepairDrone => self.kinds.repair_drone,
            UnitKind::Raider => self.kinds.raider,
            UnitKind::Scout => self.kinds.scout,
        }
    }

    /// One structure kind's row, `structures.<kind>`, or `None` for the
    /// wall, which has **no** structure row by design: it is priced by the
    /// voxel ([`RulesTable::wall_cost_per_voxel`]) and no structure row ever
    /// holds one (`crate::world::World::raise_structure` and the snapshot's
    /// restore refuse it, and the compile's one blueprint is the Generator).
    #[must_use]
    pub const fn structure(&self, kind: StructureKind) -> Option<StructureRow> {
        match kind {
            StructureKind::Generator => Some(self.kinds.generator),
            StructureKind::Autocannon => Some(self.kinds.autocannon),
            StructureKind::Mortar => Some(self.kinds.mortar),
            StructureKind::SurveyPost => Some(self.kinds.survey_post),
            StructureKind::ResonanceSpire => Some(self.kinds.resonance_spire),
            StructureKind::Wall => None,
        }
    }

    /// A placed beacon's row, `structures.beacon`.
    #[must_use]
    pub const fn beacon_row(&self) -> StructureRow {
        self.kinds.beacon
    }

    /// A seat's core's hit points, `beacon.core_hp`.
    #[must_use]
    pub const fn core_hp(&self) -> Hp {
        self.kinds.core_hp
    }

    /// One wall voxel's price, `structures.wall_cost_per_voxel`.
    #[must_use]
    pub const fn wall_cost_per_voxel(&self) -> Money {
        self.kinds.wall_cost_per_voxel
    }

    /// Item 22's threshold, `economy.backlog_threshold_per_drone`: work items
    /// per available drone above which a fabricator produces.
    #[must_use]
    pub const fn backlog_threshold_per_drone(&self) -> u32 {
        self.kinds.backlog_threshold_per_drone
    }

    /// A recycled beacon's refund in whole percent of its remaining value,
    /// `economy.recycle_refund_percent`.
    #[must_use]
    pub const fn recycle_refund_percent(&self) -> u32 {
        self.kinds.recycle_refund_percent
    }

    /// The decision cadence in game milliseconds, `match.decision_tick_ms`.
    #[must_use]
    pub const fn decision_tick_ms(&self) -> i32 {
        self.kinds.decision_tick_ms
    }

    /// A mining drone's time per voxel in game milliseconds,
    /// `economy.mining_ms_per_voxel`.
    #[must_use]
    pub const fn mining_ms_per_voxel(&self) -> i32 {
        self.kinds.mining_ms_per_voxel
    }

    /// The commander's respawn curve in game milliseconds: the base delay and
    /// the growth per earlier death in the same Push (item 90).
    #[must_use]
    pub const fn respawn_curve(&self) -> (i32, i32) {
        (self.kinds.respawn_base_ms, self.kinds.respawn_growth_ms)
    }

    /// Read a rules table from canonical proto JSON.
    ///
    /// The decode is `pharmakos-proto`'s and nobody else's (item 74): one
    /// codec, driven by the checked-in descriptor set, so the sim cannot read a
    /// file the verifier would reject. An unknown field is therefore
    /// **rejected** with a JSON Pointer, never ignored and never stripped (spec
    /// section 10, "Load never strips").
    ///
    /// # Errors
    ///
    /// Returns [`RulesError::Json`] carrying the codec's own pointer, or
    /// whatever [`RulesTable::from_message`] finds wrong with the values.
    pub fn from_canonical_json(text: &str) -> Result<RulesTable, RulesError> {
        let message: gp::v1::RulesTable =
            pharmakos_proto::json::decode(text).map_err(RulesError::Json)?;
        RulesTable::from_message(&message)
    }

    /// Take the sim's view of a decoded [`gp::v1::RulesTable`].
    ///
    /// Every block the sim reads is **required**. Proto3 cannot tell an absent
    /// message from an empty one, so a missing `locomotion` would otherwise
    /// arrive as a table of zeroes and a unit that never moves — a silent wrong
    /// answer, which is the failure mode the rules table exists to prevent.
    ///
    /// # Errors
    ///
    /// Returns [`RulesError::MissingBlock`] when a block the sim reads is
    /// absent, [`RulesError::OutOfRange`] when a value does not fit the sim's
    /// own type for it, [`RulesError::Power`] when the power phase refuses the
    /// table ([`PowerRules::read`]: a missing or oversized `kW` row, or rows
    /// under which a seat's supply or draw could leave a signed 32-bit `kW`),
    /// or [`RulesError::Json`] when the message will not render back to
    /// canonical JSON — which is what `rules_hash` is over, so a table that
    /// cannot be rendered is a table that cannot be hashed and must not become
    /// a [`RulesTable`].
    pub fn from_message(message: &gp::v1::RulesTable) -> Result<RulesTable, RulesError> {
        let locomotion = message
            .locomotion
            .as_ref()
            .ok_or(RulesError::MissingBlock("locomotion"))?;
        let broadphase = message
            .broadphase
            .as_ref()
            .ok_or(RulesError::MissingBlock("broadphase"))?;
        let mesher = message
            .mesher
            .as_ref()
            .ok_or(RulesError::MissingBlock("mesher"))?;
        let matched = message
            .r#match
            .as_ref()
            .ok_or(RulesError::MissingBlock("match"))?;
        let map = message
            .map
            .as_ref()
            .ok_or(RulesError::MissingBlock("map"))?;
        let commander = message
            .commander
            .as_ref()
            .ok_or(RulesError::MissingBlock("commander"))?;
        let economy = message
            .economy
            .as_ref()
            .ok_or(RulesError::MissingBlock("economy"))?;
        band_rows_of(economy)?;
        let mining_carry_voxels =
            NonZeroU32::new(economy.mining_carry_voxels).ok_or_else(|| RulesError::OutOfRange {
                field: "economy.mining_carry_voxels".to_owned(),
                value: "0; a drone that carries nothing never delivers".to_owned(),
            })?;

        let sphere_radius_voxels = sphere_radius_of(message)?;
        let typed = TypedRows::read(message, commander)?;
        let kinds = KindRows::read(message)?;

        let segment_lengths_ms = segment_lengths_of(matched)?;

        let canonical = pharmakos_proto::json::encode(message).map_err(RulesError::Json)?;

        Ok(RulesTable {
            message: message.clone(),
            canonical,
            mesher_drain_surfaces: mesher.surfaces_per_frame,
            mesher_drain_bytes: mesher.bytes_per_frame,
            step_cardinal: signed(
                "locomotion.step_cost_cardinal",
                locomotion.step_cost_cardinal,
            )?,
            step_diagonal: signed(
                "locomotion.step_cost_diagonal",
                locomotion.step_cost_diagonal,
            )?,
            climb_surcharge: signed("locomotion.climb_surcharge", locomotion.climb_surcharge)?,
            move_cost_per_tick: signed(
                "locomotion.move_cost_per_tick",
                locomotion.move_cost_per_tick,
            )?,
            repath_cap_per_tick: locomotion.repath_cap_per_tick,
            csr_cell_size_voxels: signed(
                "broadphase.cell_size_voxels",
                broadphase.cell_size_voxels,
            )?,
            segment_lengths_ms,
            map_size_voxels: [
                signed("map.size_x", map.size_x)?,
                signed("map.size_y", map.size_y)?,
                signed("map.size_z", map.size_z)?,
            ],
            map_last_voxel: [
                last_voxel("map.size_x", map.size_x)?,
                last_voxel("map.size_y", map.size_y)?,
                last_voxel("map.size_z", map.size_z)?,
            ],
            commander_cost_per_second: signed(
                "commander.cost_per_second",
                commander.cost_per_second,
            )?,
            hpa_cluster_voxels: signed(
                "locomotion.hpa_cluster_voxels",
                locomotion.hpa_cluster_voxels,
            )?,
            fog_cost_numerator: signed(
                "locomotion.fog_cost_numerator",
                locomotion.fog_cost_numerator,
            )?,
            fog_cost_denominator: signed(
                "locomotion.fog_cost_denominator",
                locomotion.fog_cost_denominator,
            )?,
            mining_carry_voxels,
            sphere_radius_voxels,
            arrive_radius_voxels: typed.arrive_radius_voxels,
            interface_range_voxels: typed.interface_range_voxels,
            build_hp_per_tick: typed.build_hp_per_tick,
            power: typed.power,
            kinds,
        })
    }

    /// Read the rules table from a file.
    ///
    /// # Errors
    ///
    /// Returns [`RulesError::Io`] when the file cannot be read, or whatever
    /// [`RulesTable::from_canonical_json`] returns.
    pub fn load(path: &std::path::Path) -> Result<RulesTable, RulesError> {
        let text = std::fs::read_to_string(path).map_err(|error| RulesError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        RulesTable::from_canonical_json(&text)
    }
}

/// `beacon.sphere_radius_voxels` as the resolver reads it: present, and a
/// voxel length.
fn sphere_radius_of(message: &gp::v1::RulesTable) -> Result<i16, RulesError> {
    let beacon = message
        .beacon
        .as_ref()
        .ok_or(RulesError::MissingBlock("beacon"))?;
    i16::try_from(beacon.sphere_radius_voxels).map_err(|_| RulesError::OutOfRange {
        field: "beacon.sphere_radius_voxels".to_owned(),
        value: format!(
            "{}; a sphere's radius is a voxel length, at most {}",
            beacon.sphere_radius_voxels,
            i16::MAX
        ),
    })
}

/// The rows S1's `build` lane reads typed at load, so the tick reads each as
/// it is: the commander's two radii, the build rate and the power rows.
struct TypedRows {
    arrive_radius_voxels: i16,
    interface_range_voxels: i16,
    build_hp_per_tick: Hp,
    power: PowerRules,
}

impl TypedRows {
    fn read(
        message: &gp::v1::RulesTable,
        commander: &gp::v1::rules_table::Commander,
    ) -> Result<TypedRows, RulesError> {
        Ok(TypedRows {
            arrive_radius_voxels: voxel_length(
                "commander.arrive_radius_voxels",
                commander.arrive_radius_voxels,
            )?,
            interface_range_voxels: voxel_length(
                "commander.interface_range_voxels",
                commander.interface_range_voxels,
            )?,
            build_hp_per_tick: build_hp_per_tick_of(message)?,
            power: PowerRules::read(message).map_err(RulesError::Power)?,
        })
    }
}

/// `match.segment_lengths_ms` as the runner reads it: at least one segment,
/// and no more than the encoding bounds.
fn segment_lengths_of(matched: &gp::v1::rules_table::Match) -> Result<Vec<i32>, RulesError> {
    if matched.segment_lengths_ms.is_empty() {
        return Err(RulesError::OutOfRange {
            field: "match.segment_lengths_ms".to_owned(),
            value: "an empty ladder; a match plays at least one segment".to_owned(),
        });
    }
    if matched.segment_lengths_ms.len() > MAX_SEGMENT_LENGTHS {
        return Err(RulesError::OutOfRange {
            field: "match.segment_lengths_ms".to_owned(),
            value: format!(
                "{} entries; the encoding is bounded at {MAX_SEGMENT_LENGTHS}",
                matched.segment_lengths_ms.len()
            ),
        });
    }
    Ok(matched.segment_lengths_ms.clone())
}

/// The settlement band's two percents as `economy::band_percent` and
/// `economy::bmi_for` read them: each a signed 32-bit percent. The band's
/// percent for any place on any ladder lies between the leader's malus and the
/// last place's bonus, so a table whose two rows fit is one whose ladder is
/// always paid (S1's `build` lane, decisions-log item 134: `bmi_for`'s reads
/// made typed).
fn band_rows_of(economy: &gp::v1::rules_table::Economy) -> Result<(), RulesError> {
    for (field, value) in [
        (
            "economy.scaling_last_place_bonus_percent",
            economy.scaling_last_place_bonus_percent,
        ),
        (
            "economy.scaling_leader_malus_percent",
            economy.scaling_leader_malus_percent,
        ),
    ] {
        if i32::try_from(value).is_err() {
            return Err(RulesError::OutOfRange {
                field: field.to_owned(),
                value: format!("{value}; a band percent is a signed 32-bit percent"),
            });
        }
    }
    Ok(())
}

/// A commander radius as the range tests read it: a voxel length.
fn voxel_length(field: &'static str, value: u32) -> Result<i16, RulesError> {
    i16::try_from(value).map_err(|_| RulesError::OutOfRange {
        field: field.to_owned(),
        value: format!("{value}; a radius is a voxel length, at most {}", i16::MAX),
    })
}

/// `structures.build_hp_per_second` as hit points per tick: present, positive,
/// and a whole multiple of the tick rate, so construction needs no accumulator
/// and no rounding rule (the row's own note in `rules.proto`).
fn build_hp_per_tick_of(message: &gp::v1::RulesTable) -> Result<Hp, RulesError> {
    let structures = message
        .structures
        .as_ref()
        .ok_or(RulesError::MissingBlock("structures"))?;
    let per_second = structures.build_hp_per_second;
    let refused = |why: &str| RulesError::OutOfRange {
        field: "structures.build_hp_per_second".to_owned(),
        value: format!("{per_second}; {why}"),
    };
    if per_second == 0 {
        return Err(refused(
            "a drone that adds nothing never finishes a structure",
        ));
    }
    if per_second.checked_rem(TICK_HZ) != Some(0) {
        return Err(refused(
            "the rate must be whole hit points per tick at the tick rate, or construction would \
             need an accumulator and a rounding rule",
        ));
    }
    per_second
        .checked_div(TICK_HZ)
        .and_then(|per_tick| i32::try_from(per_tick).ok())
        .map(Hp::new)
        .ok_or_else(|| refused("past a hit-point count"))
}

/// A map axis's last voxel, `size - 1`, as a voxel coordinate: an empty axis
/// and one wider than a coordinate holds are refused.
fn last_voxel(field: &'static str, size: u32) -> Result<i16, RulesError> {
    size.checked_sub(1)
        .and_then(|last| i16::try_from(last).ok())
        .ok_or_else(|| RulesError::OutOfRange {
            field: field.to_owned(),
            value: format!(
                "{size}; a map axis holds 1 to {} voxels, each a voxel coordinate",
                i32::from(i16::MAX) + 1
            ),
        })
}

/// The schema spells the move costs `uint32`; the sim's path arithmetic is
/// signed, so the narrowing is a decision and gets one (AGENTS.md §4.3).
fn signed(field: &'static str, value: u32) -> Result<i32, RulesError> {
    i32::try_from(value).map_err(|_| RulesError::OutOfRange {
        field: field.to_owned(),
        value: value.to_string(),
    })
}

/// What went wrong reading a rules table.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RulesError {
    /// The file could not be read.
    Io {
        /// The path that was tried.
        path: String,
        /// The operating system's message.
        message: String,
    },
    /// The text is not canonical `gp.v1.RulesTable` JSON, or a decoded table
    /// will not render back to it. Carries the codec's own JSON Pointer, so a
    /// diagnostic can point at the byte that caused it.
    Json(pharmakos_proto::json::Error),
    /// A block the sim reads is absent. Proto3 cannot tell that from a block of
    /// zeroes, so the sim refuses rather than guesses.
    MissingBlock(&'static str),
    /// A value is out of the range the sim's own type for it allows.
    OutOfRange {
        /// The field's path in the message.
        field: String,
        /// What the table said.
        value: String,
    },
    /// The power phase refuses the table ([`PowerRules::read`]): a `kW` row
    /// is missing or out of range, or the rows would let a seat's supply or
    /// draw leave a signed 32-bit `kW`.
    Power(PowerRulesError),
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RulesError::Io { path, message } => write!(f, "reading {path}: {message}"),
            RulesError::Json(error) => write!(f, "{RULES_MESSAGE}: {error}"),
            RulesError::MissingBlock(name) => write!(
                f,
                "the `{name}` block is missing; the sim reads it and will not \
                 substitute zeroes for it"
            ),
            RulesError::OutOfRange { field, value } => {
                write!(f, "field `{field}`: `{value}` is out of range")
            }
            RulesError::Power(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RulesError {}
