// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `kW`: supply, draw, the brownout order and dormancy (spec sections 5 and 7;
//! decisions log items 10, 90 and 127 (5) to (8)).
//!
//! # The beacon is the unit of power
//!
//! Item 10, in one paragraph. A dormant beacon powers down **everything homed
//! to it** — its fabricator, its structures, and its bound units, which park
//! where they stand at 0 kW. The seal, the sphere, interfacing and recycling
//! stay lit off the key-core, so a browned-out beacon can still be walked to
//! and changed; that is what keeps a brownout a setback rather than a lockout.
//! The core autocannon runs off the deep bore, **outside the grid**, so it
//! neither draws nor is ever shed. The commander is the one unit dormancy does
//! not park: it walks through a blackout, drawing nothing while its home is
//! dark (item 127 (5); [`crate::programs::program_for`]).
//!
//! Supply is the core's deep-bore surplus plus one Generator per vent at the
//! vent's richness — the vent's heat is the limit, not the tap, so a second
//! Generator on a vent that is already tapped adds nothing ([`supply_of`]).
//! Draw is per fielded item: every unit `power.kw_per_unit`, every capability
//! structure its own row.
//!
//! # A beacon is net zero through its own key-core
//!
//! A live beacon draws its base (`power.beacon_base_draw_kw`), and its own
//! key-core supplies exactly that base (spec section 7's Power supply row;
//! decisions log section 2.3's key-core decision and item 90): the phase reads
//! the row ([`PowerRules::beacon_base_draw`]) and the key-core's output is that
//! row and nothing else ([`PowerRules::key_core_output`]), so the two cancel
//! by construction and **neither column carries either**: [`draw_of`] counts
//! no beacon's base, the core's included, and [`revive_cost`] does not start
//! from one. That is the net the tick-0 columns map generation writes already
//! use (`mapgen`'s `starting_draw`), so a seat's meter does not step at the
//! first settle, and placing a beacon costs the grid nothing: a beacon with
//! nothing homed to it is never shed by its own draw (decisions log item 113
//! (5)). The meter is to show the net figures and say so beside itself (S1's
//! plan, decision 12, ruled by item 128: the register's S1-31; the client's
//! words are `ui`'s, and no client text says so yet).
//!
//! # The order is fixed, and it is not the `$` order
//!
//! When draw outruns supply the Quartermaster **holds fabricator orders first**
//! ([`crate::economy::SingleTreasury`] refuses a request the headroom cannot
//! run), and only then does the grid brown out. Beacons go dormant in one
//! order and one order only:
//!
//! 1. **lowest priority** — the per-beacon low / normal / high knob, which is
//!    the one player lever and is `kW`-only;
//! 2. then **furthest from the core** (or from its former site, which is where
//!    the core's row still stands even when it is dead);
//! 3. then **the core last**, because a key-core always powers its own beacon
//!    and only the load homed to it can brown it out;
//! 4. ties by **ascending beacon id**, item 62's convention, so the key is
//!    total.
//!
//! **A beacon whose shed relieves nothing is skipped and stays lit** (item 127
//! (8), the register's S1-33, amending spec section 5's Dormant row): shedding
//! a beacon with nothing homed to it relieves 0 kW, and shedding one whose
//! Generators supply more than its load draws makes the deficit worse, so
//! neither is shed. Idle expansions stay lit in a deficit.
//!
//! A dormant beacon revives only once supply exceeds draw by
//! `power.revive_margin_kw` **with that beacon's own load added back**, so a
//! grid on the edge does not oscillate and domes do not flicker. Reviving walks
//! the same order backwards: the core first, then highest priority, nearest the
//! core, lowest id. A shed core brings its deep-bore surplus back with it, so
//! its revival is weighed with that surplus credited, and a core shed by the
//! load homed to it comes back once that load leaves the margin spare; the
//! surplus is lost for good only when the core is destroyed (spec section 5,
//! the Core row).
//!
//! # A priority raise re-applies the order
//!
//! Item 127 (7), the register's S1-22: raising a dark beacon's priority
//! relights it **at once** when shedding lit beacons of lower priority covers
//! it, with draw no higher than supply after the swap, so the commander's walk
//! there pays off. The check runs in every settle, after the revival
//! ([`swap_in`]), and holds no state of its own: a dark beacon swaps with the
//! lit beacons that rank below it in the brownout order's first two terms (the
//! core above every other beacon, then the knob), shedding them in the
//! brownout order, skipping any whose shed relieves nothing, until its load
//! fits. Dark beacons are tried highest rank first and, within a rank, lowest
//! id first, so of two seats the lower is settled first and of two equal
//! beacons the lower id swaps first, wherever they stand (S1's plan, `grid`:
//! "ties to the lowest seat and then the lowest beacon id").
//!
//! The swap asks for draw no higher than supply, **not** for the revival
//! margin: a beacon whose load fits only once a lower beacon is shed relights
//! at a headroom below the margin, down to 0 kW, because the ruling's bar is
//! "draw no higher than supply after the swap". A beacon whose load fits
//! without shedding anything is held by the revival margin alone, which the
//! swap does not override. So a revival earlier in the same settle can be
//! undone by the swap (a lower beacon revived on the margin, then shed for a
//! higher one); the feed reports each seat's **net** change once per settle,
//! so a beacon lit and shed in one settle reports nothing ([`settle`]).
//!
//! Nothing in this module reads a clock and nothing in it divides: every figure
//! is a sum of integer `kW` rows.

use crate::events::{Emission, EventKind};
use crate::knowledge::{AssetId, Position};
use crate::math::fixed::{Fx, Sq};
use crate::math::quantity::Kw;
use crate::rules::RulesTable;
use crate::tables::{BeaconId, SeatId, StructureKind};
use crate::voxels::Richness;
use crate::world::World;
use core::fmt;
use pharmakos_proto::gp;

/// One `kW` row of the rules table the power phase reads.
///
/// Named so that a refused table says which row it is missing rather than
/// running on a zero ([`PowerRulesError`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerRow {
    /// The `power` block.
    Power,
    /// `power.core_surplus_kw`.
    CoreSurplus,
    /// `power.generator_output_kw`.
    GeneratorOutput,
    /// `power.generator_output_kw.lean`.
    GeneratorLean,
    /// `power.generator_output_kw.standard`.
    GeneratorStandard,
    /// `power.generator_output_kw.rich`.
    GeneratorRich,
    /// `power.kw_per_unit`.
    PerUnit,
    /// `power.revive_margin_kw`.
    ReviveMargin,
    /// `power.beacon_base_draw_kw`.
    BeaconBaseDraw,
    /// The `structures` block.
    Structures,
    /// `structures.autocannon`.
    Autocannon,
    /// `structures.mortar`.
    Mortar,
    /// `structures.survey_post`.
    SurveyPost,
    /// `structures.resonance_spire`.
    ResonanceSpire,
    /// The `units` block, whose starting force the draw bound counts.
    Units,
}

impl PowerRow {
    /// The row's path in `gp.v1.RulesTable`, as the rules file spells it.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            PowerRow::Power => "power",
            PowerRow::CoreSurplus => "power.core_surplus_kw",
            PowerRow::GeneratorOutput => "power.generator_output_kw",
            PowerRow::GeneratorLean => "power.generator_output_kw.lean",
            PowerRow::GeneratorStandard => "power.generator_output_kw.standard",
            PowerRow::GeneratorRich => "power.generator_output_kw.rich",
            PowerRow::PerUnit => "power.kw_per_unit",
            PowerRow::ReviveMargin => "power.revive_margin_kw",
            PowerRow::BeaconBaseDraw => "power.beacon_base_draw_kw",
            PowerRow::Structures => "structures",
            PowerRow::Autocannon => "structures.autocannon.draw_kw",
            PowerRow::Mortar => "structures.mortar.draw_kw",
            PowerRow::SurveyPost => "structures.survey_post.draw_kw",
            PowerRow::ResonanceSpire => "structures.resonance_spire.draw_kw",
            PowerRow::Units => "units",
        }
    }
}

/// Which of a seat's two `kW` sums a bound is over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerSum {
    /// The seat's supply ([`supply_of`]).
    Supply,
    /// The seat's draw ([`draw_of`]).
    Draw,
}

impl PowerSum {
    /// The sum's name, as a refusal says it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            PowerSum::Supply => "supply",
            PowerSum::Draw => "draw",
        }
    }
}

/// Why the power phase refuses a rules table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerRulesError {
    /// A block or row the phase reads is absent. Proto3 cannot tell an absent
    /// message from an empty one, so the read refuses rather than running the
    /// grid on zeroes.
    MissingRow(PowerRow),
    /// A `uint32` row does not fit the sim's signed `kW`.
    OutOfRange {
        /// The row.
        row: PowerRow,
        /// What the table said.
        value: u32,
    },
    /// The rows let a seat's supply or draw leave a signed 32-bit `kW`: the
    /// largest the sum can reach -- each `kW` row times the table room it
    /// counts over, summed ([`PowerRules::read`]) -- does not fit.
    SumOutOfRange {
        /// Which sum.
        sum: PowerSum,
        /// The largest the rows let it reach, in `kW`.
        most: i128,
    },
}

impl fmt::Display for PowerRulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PowerRulesError::MissingRow(row) => {
                write!(f, "the rules table has no `{}` row", row.path())
            }
            PowerRulesError::OutOfRange { row, value } => write!(
                f,
                "`{}` is {value}, which does not fit a signed 32-bit kW",
                row.path()
            ),
            PowerRulesError::SumOutOfRange { sum, most } => write!(
                f,
                "under these rows a seat's {} can reach {most} kW, which does not fit a signed \
                 32-bit kW",
                sum.name()
            ),
        }
    }
}

impl std::error::Error for PowerRulesError {}

/// Why a power read could not answer.
///
/// No arm for the rules table: [`crate::rules::RulesTable::from_message`]
/// refuses a table [`PowerRules::read`] refuses, so every world's table is one
/// the phase reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerError {
    /// The seat is not seated in this match.
    NoSuchSeat(SeatId),
    /// A beacon id the phase was handed has no row in the beacon table.
    NoSuchBeacon(u32),
    /// A `kW` sum does not fit a signed 32-bit `kW`.
    Overflow,
}

impl fmt::Display for PowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PowerError::NoSuchSeat(seat) => write!(f, "seat {} is not seated", seat.raw()),
            PowerError::NoSuchBeacon(beacon) => write!(f, "beacon {beacon} has no row"),
            PowerError::Overflow => write!(f, "a kW sum does not fit a signed 32-bit kW"),
        }
    }
}

impl std::error::Error for PowerError {}

/// The `kW` rows the power phase reads, taken once per tick.
///
/// A flat copy rather than a borrow of the rules table, because the phase
/// writes the tables the same table is reached through, and because reading
/// ten rows once a tick is cheaper than reading them once a beacon.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerRules {
    /// `power.core_surplus_kw`.
    pub core_surplus: i32,
    /// `power.generator_output_kw` for a lean vent.
    pub generator_lean: i32,
    /// `power.generator_output_kw` for a standard vent.
    pub generator_standard: i32,
    /// `power.generator_output_kw` for a rich vent.
    pub generator_rich: i32,
    /// `power.kw_per_unit`.
    pub per_unit: i32,
    /// `power.revive_margin_kw`.
    pub revive_margin: i32,
    /// `power.beacon_base_draw_kw`: what a live beacon draws, and what its own
    /// key-core supplies ([`PowerRules::key_core_output`]), so that neither
    /// column carries it (see the module docs).
    pub beacon_base_draw: i32,
    /// `structures.autocannon.draw_kw`, read only so the code can say out loud
    /// that it is never charged.
    pub autocannon_draw: i32,
    /// `structures.mortar.draw_kw`.
    pub mortar_draw: i32,
    /// `structures.survey_post.draw_kw`.
    pub survey_post_draw: i32,
    /// `structures.resonance_spire.draw_kw`.
    pub spire_draw: i32,
}

impl PowerRules {
    /// Read every `kW` row the phase needs, refusing a table that lacks one or
    /// under which a seat's sums could leave a signed 32-bit `kW`.
    ///
    /// [`crate::rules::RulesTable::from_message`] calls this, once, and keeps
    /// what it reads, so every world's table is one the phase can sum
    /// (decisions-log item 135 (2) (b)).
    ///
    /// **The sum bound.** A seat's largest supply is its core's deep-bore
    /// surplus plus the richest Generator output on every row of the structure
    /// table (`STRUCTURE_TABLE_ROOM` in `crate::world`); its largest draw is
    /// `power.kw_per_unit` on every unit it can field -- its starting force
    /// (the commander, `units.starting_build_drones` and
    /// `units.starting_mining_drones`) and the whole unit room
    /// (`UNIT_TABLE_ROOM`) -- plus the heaviest capability structure's draw on
    /// every row of the structure table. Each is an upper bound (a seat holds
    /// only its share of each room), so a table both fit is one under which
    /// [`supply_of`] and [`draw_of`] never leave an `i32`, and they add
    /// without saturating.
    ///
    /// # Errors
    ///
    /// [`PowerRulesError::MissingRow`] naming the first absent block or row,
    /// in the order the struct lists them (then the `units` block the bound
    /// reads), [`PowerRulesError::OutOfRange`] for a row above `i32::MAX`, and
    /// [`PowerRulesError::SumOutOfRange`] when a seat's largest supply or draw
    /// does not fit a signed 32-bit `kW`.
    pub fn read(message: &gp::v1::RulesTable) -> Result<PowerRules, PowerRulesError> {
        let power = message
            .power
            .as_ref()
            .ok_or(PowerRulesError::MissingRow(PowerRow::Power))?;
        let by_richness = power
            .generator_output_kw
            .as_ref()
            .ok_or(PowerRulesError::MissingRow(PowerRow::GeneratorOutput))?;
        let structures = message
            .structures
            .as_ref()
            .ok_or(PowerRulesError::MissingRow(PowerRow::Structures))?;
        let draw = |row: Option<&gp::v1::rules_table::StructureKind>, name: PowerRow| {
            row.ok_or(PowerRulesError::MissingRow(name))
                .and_then(|row| kw(row.draw_kw, name))
        };
        let rules = PowerRules {
            core_surplus: kw(power.core_surplus_kw, PowerRow::CoreSurplus)?,
            generator_lean: kw(by_richness.lean, PowerRow::GeneratorLean)?,
            generator_standard: kw(by_richness.standard, PowerRow::GeneratorStandard)?,
            generator_rich: kw(by_richness.rich, PowerRow::GeneratorRich)?,
            per_unit: kw(power.kw_per_unit, PowerRow::PerUnit)?,
            revive_margin: kw(power.revive_margin_kw, PowerRow::ReviveMargin)?,
            beacon_base_draw: kw(power.beacon_base_draw_kw, PowerRow::BeaconBaseDraw)?,
            autocannon_draw: draw(structures.autocannon.as_ref(), PowerRow::Autocannon)?,
            mortar_draw: draw(structures.mortar.as_ref(), PowerRow::Mortar)?,
            survey_post_draw: draw(structures.survey_post.as_ref(), PowerRow::SurveyPost)?,
            spire_draw: draw(
                structures.resonance_spire.as_ref(),
                PowerRow::ResonanceSpire,
            )?,
        };
        let units = message
            .units
            .as_ref()
            .ok_or(PowerRulesError::MissingRow(PowerRow::Units))?;
        rules.bound_sums(units)?;
        Ok(rules)
    }

    /// Every `kW` row the phase needs: the rules table's own, read and bounded
    /// once at load ([`crate::rules::RulesTable::power`]).
    #[must_use]
    pub const fn of(rules: &RulesTable) -> PowerRules {
        rules.power()
    }

    /// Refuse the rows when a seat's largest supply or draw would not fit a
    /// signed 32-bit `kW` ([`PowerRules::read`]'s sum bound). Every figure is
    /// a non-negative `i32` row times a `u32` count, so `i128` holds each
    /// product and the sum of a handful of them exactly.
    fn bound_sums(self, units: &gp::v1::rules_table::Units) -> Result<(), PowerRulesError> {
        let structure_room = i128::from(crate::world::STRUCTURE_TABLE_ROOM);
        let fielded = i128::from(crate::world::UNIT_TABLE_ROOM)
            + 1
            + i128::from(units.starting_build_drones)
            + i128::from(units.starting_mining_drones);
        let richest = self
            .generator_lean
            .max(self.generator_standard)
            .max(self.generator_rich);
        // The heaviest draw any one structure row can carry: every kind's,
        // folded from the first kind's rather than from a made-up zero.
        let heaviest = StructureKind::ALL
            .into_iter()
            .map(|kind| self.structure_draw(kind))
            .fold(self.structure_draw(StructureKind::Generator), i32::max);
        let supply = i128::from(self.core_surplus) + i128::from(richest) * structure_room;
        let draw = i128::from(self.per_unit) * fielded + i128::from(heaviest) * structure_room;
        for (sum, most) in [(PowerSum::Supply, supply), (PowerSum::Draw, draw)] {
            if most > i128::from(i32::MAX) {
                return Err(PowerRulesError::SumOutOfRange { sum, most });
            }
        }
        Ok(())
    }

    /// What a live beacon's own key-core supplies: exactly that beacon's base
    /// draw, `power.beacon_base_draw_kw` (spec section 7's Power supply row).
    ///
    /// The reason [`draw_of`] carries no beacon's base and [`supply_of`] no
    /// key-core's output: the two are one row read twice, so they cancel by
    /// construction (S1's plan, decision 12).
    #[must_use]
    pub const fn key_core_output(&self) -> i32 {
        self.beacon_base_draw
    }

    /// A Generator's output on a vent of this grade.
    #[must_use]
    pub const fn generator_output(&self, richness: Richness) -> i32 {
        match richness {
            Richness::Lean => self.generator_lean,
            Richness::Standard => self.generator_standard,
            Richness::Rich => self.generator_rich,
        }
    }

    /// One structure kind's draw.
    ///
    /// **A Generator draws nothing**: it is the thing that supplies. **An
    /// autocannon draws nothing either**, and that is the load-bearing line —
    /// it runs off the core's deep bore, outside the grid, which is the same
    /// sentence that makes it the one thing a brownout never sheds (spec
    /// section 5, the Core row). A wall has no draw because it has no row: its
    /// hit points are by material and it is priced per voxel.
    #[must_use]
    pub const fn structure_draw(&self, kind: StructureKind) -> i32 {
        match kind {
            StructureKind::Generator | StructureKind::Autocannon | StructureKind::Wall => 0,
            StructureKind::Mortar => self.mortar_draw,
            StructureKind::SurveyPost => self.survey_post_draw,
            StructureKind::ResonanceSpire => self.spire_draw,
        }
    }
}

/// A `uint32` row as the sim's signed `kW`, refused when it does not fit.
fn kw(value: u32, row: PowerRow) -> Result<i32, PowerRulesError> {
    i32::try_from(value).map_err(|_| PowerRulesError::OutOfRange { row, value })
}

/// Which beacons a supply or draw sum counts as lit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lighting {
    /// The world as it stands: a dormant beacon is dark.
    AsIs,
    /// Every living beacon of the seat lit, dormant or not: what the seat's
    /// grid would hold with nothing shed ([`dark_load`]).
    AllLit,
}

/// What a seat's dark beacons hold off its grid, in `kW`.
///
/// The read `econ`'s shortfall line takes its number from (`gp.api.v1`
/// `Shortfall.kw`; decisions log item 134 (2) (c)). Pure: it reads the world
/// and writes nothing, and it applies exactly the rules the power phase
/// applies, because it is the phase's own [`supply_of`] and [`draw_of`] taken
/// twice — once as the world stands and once with every living beacon lit.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DarkLoad {
    /// The draw homed to the seat's dark beacons: every unit and capability
    /// structure that went dark with its home. The **dark load**.
    pub draw: Kw,
    /// The supply those beacons took off the grid with them: a dark core's
    /// deep-bore surplus, and every Generator homed to a dark beacon that would
    /// tap a vent no lit Generator taps.
    pub supply: Kw,
}

impl DarkLoad {
    /// The **shed kW**: what the brownout took off the grid net, the dark load
    /// less the supply that went dark with it. Positive while the dark beacons
    /// relieve the grid, which is every beacon the brownout order sheds
    /// (a shed that relieves nothing is skipped).
    ///
    /// # Errors
    ///
    /// [`PowerError::Overflow`] when the difference does not fit.
    pub fn shed(self) -> Result<Kw, PowerError> {
        self.draw
            .checked_sub(self.supply)
            .ok_or(PowerError::Overflow)
    }
}

/// A seat's dark load and the supply that went dark with it ([`DarkLoad`]).
///
/// # Errors
///
/// [`PowerError::NoSuchSeat`] for a seat that is not seated, and
/// [`PowerError::Overflow`] when a sum or a difference does not fit a signed
/// 32-bit `kW`, which the load-time bound of [`PowerRules::read`] rules out.
pub fn dark_load(world: &World, seat: SeatId) -> Result<DarkLoad, PowerError> {
    let rules = PowerRules::of(world.rules());
    if world.seat_row(seat).is_none() {
        return Err(PowerError::NoSuchSeat(seat));
    }
    let sum = |total: Option<i32>| total.ok_or(PowerError::Overflow);
    let draw_lit = sum(draw_under(world, seat, rules, Lighting::AllLit))?;
    let draw_now = sum(draw_under(world, seat, rules, Lighting::AsIs))?;
    let supply_lit = sum(supply_under(world, seat, rules, Lighting::AllLit))?;
    let supply_now = sum(supply_under(world, seat, rules, Lighting::AsIs))?;
    Ok(DarkLoad {
        draw: Kw::new(draw_lit.checked_sub(draw_now).ok_or(PowerError::Overflow)?),
        supply: Kw::new(
            supply_lit
                .checked_sub(supply_now)
                .ok_or(PowerError::Overflow)?,
        ),
    })
}

/// Settle every seat's grid for this tick.
///
/// `order` is the caller's scratch buffer, so the phase allocates nothing
/// (G3′ §9.17). For one seat at a time it holds, from index 0, the seat's
/// dark beacons as the settle found them (`base` of them, by ascending id),
/// and after them the working list of whichever step runs: the brownout
/// order, the revival order, or for the swap ([`swap_in`]) the dark beacons
/// followed by the lit ones below one of them. Together those are never more
/// than twice the seat's beacons, so the buffer stops growing after the first
/// settles.
///
/// **The feed reports each seat's net change, once per settle**
/// ([`report_changes`]): the three steps flip dormancy flags without reporting,
/// and the beacons whose flag differs from the one the settle found are
/// reported at the end. A beacon revived on the margin and then shed by the
/// swap in the same settle was dark before and is dark after, so it reports
/// nothing; the feed never carries a revival and a shed of one beacon on one
/// tick.
///
/// **Every sum is exact.** [`supply_of`] and [`draw_of`] add without
/// saturating, and the load-time bound of [`PowerRules::read`] is what makes
/// that safe: no seat's sum under a table that loaded can leave a signed
/// 32-bit `kW`. A seat whose settle nevertheless reports an overflow is
/// therefore a table and a world that disagree about the rooms; its columns
/// then keep the values its last settle wrote, rather than a number nobody
/// summed.
pub(crate) fn settle(world: &mut World, order: &mut Vec<u32>) {
    let rules = PowerRules::of(world.rules());
    let mut index: usize = 0;
    while let Some(raw) = world.seats().seats().get(index).copied() {
        let seat = SeatId::new(raw);
        let settled = settle_seat(world, seat, index, rules, order);
        order.clear();
        if let Ok((supply, draw)) = settled {
            let columns = world.seats_mut().power_columns();
            if let Some(slot) = columns.supply.get_mut(index) {
                *slot = supply;
            }
            if let Some(slot) = columns.draw.get_mut(index) {
                *slot = draw;
            }
        }
        index = index.saturating_add(1);
    }
}

/// One seat's settle: for a seat still in the match, the brownout, the
/// revival and the swap, with the net change reported; then the seat's supply
/// and draw as the columns will hold them.
fn settle_seat(
    world: &mut World,
    seat: SeatId,
    index: usize,
    rules: PowerRules,
    order: &mut Vec<u32>,
) -> Result<(Kw, Kw), PowerError> {
    if world.seats().is_alive(index) {
        order.clear();
        found_dark(world, seat, order);
        let base = order.len();
        brown_out(world, seat, rules, order, base)?;
        revive(world, seat, rules, order, base)?;
        swap_in(world, seat, rules, order, base)?;
        report_changes(world, seat, order, base);
    }
    Ok((supply_of(world, seat, rules)?, draw_of(world, seat, rules)?))
}

/// Whether two standing points are taps on **one** heat vent.
///
/// Both stand on vent material, and on footprint columns of the same vent in
/// the map's feature table ([`crate::features`]): the generator records every
/// column it stamps a vent into and refuses a map where two features share a
/// column, so a vent's identity is true by construction rather than inferred
/// from how far apart two taps stand (register S1-32, discharged by S1's
/// targeting; `docs/design/targeting.md`, "Names").
fn one_vent(world: &World, a: [Fx; 3], b: [Fx; 3]) -> bool {
    if vent_under(world, a).is_none() || vent_under(world, b).is_none() {
        return false;
    }
    match (vent_feature_under(world, a), vent_feature_under(world, b)) {
        (Some(first), Some(second)) => first == second,
        _ => false,
    }
}

/// The vent of the map's feature table whose footprint holds the column a
/// standing point floors onto, or `None`.
fn vent_feature_under(world: &World, at: [Fx; 3]) -> Option<usize> {
    let x = at.first()?.floor_voxels();
    let y = at.get(1)?.floor_voxels();
    let index = world.features().at_column(x, y)?;
    (world.features().get(index)?.kind == crate::features::FeatureKind::Vent).then_some(index)
}

/// Whether an earlier counted Generator of `seat` already taps the vent under
/// `at`.
///
/// "Earlier" is the lower structure id, which is a total order (item 62), so
/// which tap counts does not depend on the order the table is walked in.
fn vent_already_tapped(
    world: &World,
    seat: SeatId,
    before: usize,
    at: [Fx; 3],
    lighting: Lighting,
) -> bool {
    let structures = world.structures();
    let mut row: usize = 0;
    while row < before {
        if structure_counts(world, seat, row, lighting)
            && structures.kinds().get(row).copied() == Some(StructureKind::Generator.id())
            && let Some(other) = structures.positions().get(row).copied()
            && one_vent(world, at, other)
        {
            return true;
        }
        row = row.saturating_add(1);
    }
    false
}

/// The seat's supply: the core's deep-bore surplus plus every live Generator,
/// **one to a vent**.
///
/// A Generator that is still going up supplies nothing — "construction time is
/// hit points and spectacle" (item 23) — and one homed to a dormant beacon
/// supplies nothing either, because dormancy powers down everything homed to a
/// beacon and a tap that is powered down is not a tap.
///
/// A second Generator standing on a vent that is already tapped supplies
/// nothing either, and that is the whole of "one Generator per vent, output
/// set by the vent's richness, **because the vent's heat is the limit, not the
/// tap**" (spec section 5). The rule is enforced here, where the heat is
/// counted, rather than only by refusing the second building: a seat that
/// writes two fixed-voxel targets on two columns of one vent may stand two
/// Generators there and waste the `$`, and the grid is not one kilowatt richer
/// for it. Since S1 the other routes are shut where they are written: a Build
/// target `on` a vent refuses a vent a live Generator of any seat already
/// stands on (`crate::targeting::on_vent`), the interface row refuses an anchor
/// another of the seat's targets has claimed ([`World::anchor_is_claimed`]),
/// and construction is refused where any live structure stands (one structure
/// per voxel, `docs/design/targeting.md`, "Sites").
///
/// No key-core output is in it: a key-core supplies exactly its own beacon's
/// base, which [`draw_of`] leaves out for the same reason.
///
/// The sum is exact and never saturates: [`PowerRules::read`] refuses at load
/// any table under which a seat's supply could leave a signed 32-bit `kW`
/// (decisions-log item 135 (2) (b)).
///
/// # Errors
///
/// [`PowerError::Overflow`] when the sum does not fit, which that bound rules
/// out for a world built from a table that loaded.
pub(crate) fn supply_of(world: &World, seat: SeatId, rules: PowerRules) -> Result<Kw, PowerError> {
    supply_under(world, seat, rules, Lighting::AsIs)
        .map(Kw::new)
        .ok_or(PowerError::Overflow)
}

/// [`supply_of`] under a [`Lighting`], `None` when the sum does not fit.
fn supply_under(world: &World, seat: SeatId, rules: PowerRules, lighting: Lighting) -> Option<i32> {
    let mut total: i32 = 0;
    let core = core_of(world, seat);
    if let Some(core) = core
        && beacon_counts(world, core, lighting)
    {
        total = total.checked_add(rules.core_surplus)?;
    }
    let structures = world.structures();
    let count = usize::try_from(structures.len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
        if structure_counts(world, seat, row, lighting)
            && structures.kinds().get(row).copied() == Some(StructureKind::Generator.id())
        {
            let at = structures.positions().get(row).copied();
            let richness = at.and_then(|point| vent_under(world, point));
            if let (Some(grade), Some(point)) = (richness, at)
                && !vent_already_tapped(world, seat, row, point, lighting)
            {
                total = total.checked_add(rules.generator_output(grade))?;
            }
        }
        row = row.saturating_add(1);
    }
    Some(total)
}

/// The seat's draw: every unit and every capability structure homed to a live
/// beacon.
///
/// **No beacon's base is in it**, the core's included: a live beacon's own
/// key-core supplies exactly its base ([`PowerRules::key_core_output`]), so the
/// beacon is net zero and the column shows the net (see the module docs). A
/// beacon with nothing homed to it adds nothing here.
///
/// Exact, never saturating, for the reason [`supply_of`] gives.
///
/// # Errors
///
/// [`PowerError::Overflow`] when the sum does not fit, which the load-time
/// bound rules out.
pub(crate) fn draw_of(world: &World, seat: SeatId, rules: PowerRules) -> Result<Kw, PowerError> {
    draw_under(world, seat, rules, Lighting::AsIs)
        .map(Kw::new)
        .ok_or(PowerError::Overflow)
}

/// The seat's headroom: supply less draw, as the next settle reads them.
///
/// # Errors
///
/// [`PowerError::Overflow`] as [`supply_of`] and [`draw_of`] give it. Both
/// are non-negative `i32`s, so their difference always fits.
pub(crate) fn headroom_of(
    world: &World,
    seat: SeatId,
    rules: PowerRules,
) -> Result<Kw, PowerError> {
    supply_of(world, seat, rules)?
        .checked_sub(draw_of(world, seat, rules)?)
        .ok_or(PowerError::Overflow)
}

/// [`draw_of`] under a [`Lighting`].
fn draw_under(world: &World, seat: SeatId, rules: PowerRules, lighting: Lighting) -> Option<i32> {
    let mut total: i32 = unit_draw_of(world, seat, rules, lighting)?;
    let structures = world.structures();
    let count = usize::try_from(structures.len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
        if structure_counts(world, seat, row, lighting) {
            let kind = structures
                .kinds()
                .get(row)
                .copied()
                .and_then(StructureKind::from_id);
            if let Some(kind) = kind {
                total = total.checked_add(rules.structure_draw(kind))?;
            }
        }
        row = row.saturating_add(1);
    }
    Some(total)
}

/// What the seat's units draw.
///
/// A unit draws while its home counts as lit, and only then, whatever it is
/// doing: so the commander, which a blackout never parks, draws nothing while
/// its home is dark and walks on (item 127 (5)).
fn unit_draw_of(world: &World, seat: SeatId, rules: PowerRules, lighting: Lighting) -> Option<i32> {
    let mut total: i32 = 0;
    let units = world.units();
    let count = usize::try_from(units.len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
        let mine = units.seats().get(row).copied() == Some(seat.raw());
        let alive = units.hit_points().get(row).is_some_and(|hp| hp.is_alive());
        let home = units
            .homes()
            .get(row)
            .copied()
            .unwrap_or(BeaconId::NONE.raw());
        if mine && alive && beacon_counts(world, BeaconId::new(home), lighting) {
            total = total.checked_add(rules.per_unit)?;
        }
        row = row.saturating_add(1);
    }
    Some(total)
}

/// The seat's core: its lowest-id beacon row, alive or not.
///
/// "Brownout distance is measured from the core **or its former site**" (spec
/// section 5), so a dead core still anchors the order — which is exactly what
/// keeping its row gives for free.
#[must_use]
pub(crate) fn core_of(world: &World, seat: SeatId) -> Option<BeaconId> {
    let beacons = world.beacons();
    let count = usize::try_from(beacons.len()).unwrap_or(0);
    let mut best: Option<u32> = None;
    let mut row: usize = 0;
    while row < count {
        if beacons.seats().get(row).copied() == Some(seat.raw()) {
            let id = beacons.ids().get(row).copied().unwrap_or(0);
            if best.is_none_or(|held| id < held) {
                best = Some(id);
            }
        }
        row = row.saturating_add(1);
    }
    best.map(BeaconId::new)
}

/// Whether a beacon is alive and awake.
#[must_use]
pub(crate) fn beacon_is_live(world: &World, beacon: BeaconId) -> bool {
    beacon_counts(world, beacon, Lighting::AsIs)
}

/// Whether a beacon counts as lit under a [`Lighting`]: alive, and awake
/// unless every living beacon is counted lit.
fn beacon_counts(world: &World, beacon: BeaconId, lighting: Lighting) -> bool {
    if !beacon.is_some() {
        return false;
    }
    let Ok(row) = usize::try_from(beacon.raw()) else {
        return false;
    };
    let beacons = world.beacons();
    let alive = beacons
        .hit_points()
        .get(row)
        .is_some_and(|hp| hp.is_alive());
    let awake = match lighting {
        Lighting::AsIs => beacons.dormant().get(row).copied() == Some(false),
        Lighting::AllLit => beacons.dormant().get(row).is_some(),
    };
    alive && awake
}

/// Whether the beacon at `row` is alive and awake.
fn beacon_row_is_live(world: &World, row: usize) -> bool {
    let beacons = world.beacons();
    beacons
        .hit_points()
        .get(row)
        .is_some_and(|hp| hp.is_alive())
        && beacons.dormant().get(row).copied() == Some(false)
}

/// Whether the structure at `row` is `seat`'s, standing, finished, and homed to
/// a beacon that counts as lit.
fn structure_counts(world: &World, seat: SeatId, row: usize, lighting: Lighting) -> bool {
    let structures = world.structures();
    if structures.seats().get(row).copied() != Some(seat.raw()) {
        return false;
    }
    if !structures
        .hit_points()
        .get(row)
        .is_some_and(|hp| hp.is_alive())
    {
        return false;
    }
    if structures.building().get(row).copied() != Some(false) {
        return false;
    }
    let home = structures
        .homes()
        .get(row)
        .copied()
        .unwrap_or(BeaconId::NONE.raw());
    beacon_counts(world, BeaconId::new(home), lighting)
}

/// The grade of the heat vent a Generator stands on, or `None` when it stands
/// on no vent at all.
///
/// A structure's position is the standing point — one voxel above the column's
/// top solid voxel, the same convention a walker uses — so the vent is the
/// voxel underneath it. The voxel *at* the position is checked too, so that a
/// Generator placed by a test directly on the vent reads the same grade as one
/// the Build mandate raised.
#[must_use]
pub(crate) fn vent_under(world: &World, at: [Fx; 3]) -> Option<Richness> {
    let x = at.first()?.floor_voxels();
    let y = at.get(1)?.floor_voxels();
    let z = at.get(2)?.floor_voxels();
    let below = world.voxels().get([x, y, z.saturating_sub(1)]);
    below
        .and_then(crate::voxels::Material::vent_richness)
        .or_else(|| {
            world
                .voxels()
                .get([x, y, z])
                .and_then(crate::voxels::Material::vent_richness)
        })
}

/// The seat's headroom in whole `kW` ([`headroom_of`]), for the phase's own
/// comparisons.
fn headroom(world: &World, seat: SeatId, rules: PowerRules) -> Result<i32, PowerError> {
    headroom_of(world, seat, rules).map(Kw::raw)
}

/// Write one beacon's dormancy flag. `false` when the id names no row.
fn set_dark(world: &mut World, beacon: u32, dark: bool) -> bool {
    let Ok(row) = usize::try_from(beacon) else {
        return false;
    };
    match world.beacons_mut().dormant_mut().get_mut(row) {
        Some(slot) => {
            *slot = dark;
            true
        }
        None => false,
    }
}

/// Whether one beacon is dormant now.
fn is_dark(world: &World, beacon: u32) -> bool {
    usize::try_from(beacon)
        .ok()
        .and_then(|row| world.beacons().dormant().get(row).copied())
        == Some(true)
}

/// Shed `beacon` if, and only if, its shed relieves something: the seat's
/// headroom after the shed is greater than before it.
///
/// The flag is flipped, the headroom read with the same [`supply_of`] and
/// [`draw_of`] the next settle reads, and the flag put back when the shed
/// relieved nothing. Nothing observes the world between the two flips. `true`
/// when the beacon is now shed; on an error the flag is put back first.
fn shed_if_it_relieves(
    world: &mut World,
    seat: SeatId,
    rules: PowerRules,
    beacon: u32,
) -> Result<bool, PowerError> {
    if is_dark(world, beacon) {
        return Ok(false);
    }
    let before = headroom(world, seat, rules)?;
    if !set_dark(world, beacon, true) {
        return Err(PowerError::NoSuchBeacon(beacon));
    }
    match headroom(world, seat, rules) {
        Ok(after) if after > before => Ok(true),
        Ok(_) => {
            set_dark(world, beacon, false);
            Ok(false)
        }
        Err(error) => {
            set_dark(world, beacon, false);
            Err(error)
        }
    }
}

/// Report a beacon's change of state on the bus.
fn report(world: &mut World, seat: SeatId, beacon: u32, kind: EventKind) {
    let tick = world.tick();
    let place = usize::try_from(beacon)
        .ok()
        .and_then(|row| world.beacons().positions().get(row).copied());
    let mut emission = Emission::of(kind)
        .seat(seat)
        .subject(AssetId::of_beacon(BeaconId::new(beacon)));
    if let Some(point) = place {
        emission = emission.at(Position::from_array(point));
    }
    world.emit(tick, emission);
}

/// Append the seat's living dark beacons, as the settle finds them, to
/// `order`, by ascending id: the "before" [`report_changes`] compares with.
fn found_dark(world: &World, seat: SeatId, order: &mut Vec<u32>) {
    let start = order.len();
    let beacons = world.beacons();
    for (row, owner) in beacons.seats().iter().enumerate() {
        if *owner == seat.raw()
            && beacon_row_is_dark(world, row)
            && let Some(id) = beacons.ids().get(row).copied()
        {
            order.push(id);
        }
    }
    if let Some(found) = order.get_mut(start..) {
        // item 62: the key is the unique beacon id itself.
        found.sort_unstable();
    }
}

/// Whether the beacon at `row` is alive and dormant.
fn beacon_row_is_dark(world: &World, row: usize) -> bool {
    let beacons = world.beacons();
    beacons
        .hit_points()
        .get(row)
        .is_some_and(|hp| hp.is_alive())
        && beacons.dormant().get(row).copied() == Some(true)
}

/// Write into `order[base..]` the seat's living beacons whose dormancy now
/// differs from what the settle found (`order[..base]`, ascending), keeping
/// only those now dark when `now_dark` is set and only those now lit when it
/// is not.
fn changed(world: &World, seat: SeatId, order: &mut Vec<u32>, base: usize, now_dark: bool) {
    order.truncate(base);
    let beacons = world.beacons();
    for (row, owner) in beacons.seats().iter().enumerate() {
        let alive = beacons
            .hit_points()
            .get(row)
            .is_some_and(|hp| hp.is_alive());
        if *owner != seat.raw() || !alive {
            continue;
        }
        let Some(id) = beacons.ids().get(row).copied() else {
            continue;
        };
        let dark = beacons.dormant().get(row).copied() == Some(true);
        let was = order
            .get(..base)
            .is_some_and(|found| found.binary_search(&id).is_ok());
        if dark == now_dark && dark != was {
            order.push(id);
        }
    }
}

/// Report the seat's net change for this settle on the bus: every beacon now
/// dark that the settle found lit, in the brownout order, and then every
/// beacon now lit that it found dark, in the revival order. A beacon whose
/// flag ends where it started reports nothing (see [`settle`]).
fn report_changes(world: &mut World, seat: SeatId, order: &mut Vec<u32>, base: usize) {
    let core = core_of(world, seat);
    let core_at = core_site(world, core);
    for (now_dark, kind) in [
        (true, EventKind::BeaconBrownedOut),
        (false, EventKind::BeaconRevived),
    ] {
        changed(world, seat, order, base, now_dark);
        if let Some(list) = order.get_mut(base..) {
            // item 62: both keys end in the unique beacon id, so either order
            // is total.
            if now_dark {
                list.sort_unstable_by_key(|id| shed_key(world, core, core_at, *id));
            } else {
                // item 62: `revive_key` ends in the unique beacon id.
                list.sort_unstable_by_key(|id| revive_key(world, core, core_at, *id));
            }
        }
        let mut at = base;
        while let Some(beacon) = order.get(at).copied() {
            report(world, seat, beacon, kind);
            at = at.saturating_add(1);
        }
    }
    order.truncate(base);
}

/// Shed beacons, in the fixed order, until draw no longer outruns supply.
///
/// **A beacon whose shed relieves nothing is skipped** and stays lit (item 127
/// (8), the register's S1-33): with a beacon's base netted out by its
/// key-core, shedding a beacon with nothing homed to it relieves 0 kW, and
/// shedding one whose Generators out-supply its load makes the deficit worse.
/// Each candidate's relief is measured, not modelled
/// ([`shed_if_it_relieves`]), so every rule [`supply_of`] and [`draw_of`]
/// apply is weighed in one place.
///
/// The candidate list is built **once** and walked in order rather than
/// re-picked after every shed, which is what bounds the loop, and the order a
/// reader sees is the order the spec writes down. It works in `order[base..]`
/// and leaves `order[..base]` as it found it (see [`settle`]).
fn brown_out(
    world: &mut World,
    seat: SeatId,
    rules: PowerRules,
    order: &mut Vec<u32>,
    base: usize,
) -> Result<(), PowerError> {
    if headroom(world, seat, rules)? >= 0 {
        return Ok(());
    }
    shed_order(world, seat, order, base);
    let mut at = base;
    while let Some(beacon) = order.get(at).copied() {
        if headroom(world, seat, rules)? >= 0 {
            break;
        }
        at = at.saturating_add(1);
        shed_if_it_relieves(world, seat, rules, beacon)?;
    }
    order.truncate(base);
    Ok(())
}

/// The brownout order's key for one beacon: the core last, then lowest
/// priority, then furthest from the core, then ascending id.
///
/// item 62: the key ends in the beacon id, which is unique, so the order is
/// total. `is_core` sorts last as a plain `bool`, which is the core's own rule;
/// the distance is negated by `Reverse` so the furthest sheds first.
///
/// `None` when the id has no beacon row, or the seat no core site to measure
/// from: such a beacon has no place in the order, and the lists the order
/// sorts admit only beacons whose key is `Some`, rather than reading a missing
/// priority as the lowest or a missing distance as zero.
fn shed_key(
    world: &World,
    core: Option<BeaconId>,
    core_at: Option<[Fx; 3]>,
    id: u32,
) -> Option<(bool, u8, core::cmp::Reverse<Sq>, u32)> {
    let beacons = world.beacons();
    let row = usize::try_from(id).ok()?;
    let priority = beacons.priorities().get(row).copied()?;
    let distance = Sq::between(beacons.positions().get(row).copied()?, core_at?);
    let is_core = core.is_some_and(|core| core.raw() == id);
    Some((is_core, priority, core::cmp::Reverse(distance), id))
}

/// Where the seat's core stands, or stood.
fn core_site(world: &World, core: Option<BeaconId>) -> Option<[Fx; 3]> {
    core.and_then(|id| usize::try_from(id.raw()).ok())
        .and_then(|row| world.beacons().positions().get(row).copied())
}

/// The brownout order: lowest priority, then furthest from the core, then the
/// core last, ties by ascending id.
///
/// Written into `order` as beacon ids. Only this seat's living, awake beacons
/// are candidates: a dead one is already off the grid and a dormant one is
/// already shed.
fn shed_order(world: &World, seat: SeatId, order: &mut Vec<u32>, base: usize) {
    order.truncate(base);
    let core = core_of(world, seat);
    let core_at = core_site(world, core);
    let beacons = world.beacons();
    for (row, (owner, id)) in beacons.seats().iter().zip(beacons.ids()).enumerate() {
        if *owner == seat.raw()
            && beacon_row_is_live(world, row)
            && shed_key(world, core, core_at, *id).is_some()
        {
            order.push(*id);
        }
    }
    // item 62: `shed_key` ends in the unique beacon id, so the order is total.
    if let Some(list) = order.get_mut(base..) {
        list.sort_unstable_by_key(|id| shed_key(world, core, core_at, *id));
    }
}

/// Bring dormant beacons back, best first, while the margin allows it.
///
/// The margin is spec section 5's: a beacon revives "only once supply exceeds
/// draw by a margin", and the margin is measured **with that beacon's own load
/// added back**, so a beacon cannot revive into a deficit it immediately causes
/// and then be shed again next tick. That is the whole anti-flicker rule, and
/// it holds only because [`revive_cost`] measures that load with the same
/// [`supply_of`] and [`draw_of`] the next settle sheds by.
fn revive(
    world: &mut World,
    seat: SeatId,
    rules: PowerRules,
    order: &mut Vec<u32>,
    base: usize,
) -> Result<(), PowerError> {
    let mut guard: u32 = 0;
    let limit = world.beacons().len();
    while guard <= limit {
        guard = guard.saturating_add(1);
        revive_order(world, seat, order, base);
        let mut woke = false;
        let mut at = base;
        while at < order.len() {
            let Some(beacon) = order.get(at).copied() else {
                break;
            };
            at = at.saturating_add(1);
            let cost = revive_cost(world, seat, rules, BeaconId::new(beacon))?;
            let after = headroom(world, seat, rules)?
                .checked_sub(cost)
                .ok_or(PowerError::Overflow)?;
            if after < rules.revive_margin {
                continue;
            }
            set_dark(world, beacon, false);
            woke = true;
            break;
        }
        if !woke {
            break;
        }
    }
    order.truncate(base);
    Ok(())
}

/// The revival order's key for one beacon: every term of the shed key,
/// reversed — the core first, then the highest priority, then the nearest.
///
/// item 62: the key still ends in the unique beacon id, ascending, so it is
/// total.
///
/// `None` exactly where [`shed_key`] is.
fn revive_key(
    world: &World,
    core: Option<BeaconId>,
    core_at: Option<[Fx; 3]>,
    id: u32,
) -> Option<(bool, core::cmp::Reverse<u8>, Sq, u32)> {
    let (is_core, priority, core::cmp::Reverse(distance), id) = shed_key(world, core, core_at, id)?;
    Some((!is_core, core::cmp::Reverse(priority), distance, id))
}

/// The order beacons come back in: the brownout order, backwards.
fn revive_order(world: &World, seat: SeatId, order: &mut Vec<u32>, base: usize) {
    order.truncate(base);
    let core = core_of(world, seat);
    let core_at = core_site(world, core);
    let beacons = world.beacons();
    for (row, (owner, id)) in beacons.seats().iter().zip(beacons.ids()).enumerate() {
        if *owner == seat.raw()
            && beacon_row_is_dark(world, row)
            && revive_key(world, core, core_at, *id).is_some()
        {
            order.push(*id);
        }
    }
    // item 62: `revive_key` ends in the unique beacon id, so the order is total.
    if let Some(list) = order.get_mut(base..) {
        list.sort_unstable_by_key(|id| revive_key(world, core, core_at, *id));
    }
}

/// What reviving `beacon` would add to the seat's net draw, **measured rather
/// than modelled**: the seat's headroom now, less its headroom with `beacon`
/// awake.
///
/// The beacon is woken, [`supply_of`] and [`draw_of`] are read, and its flag
/// is put back before anything else runs, so every rule those two apply is
/// weighed once and in one place: the units and structures homed to it, the
/// Generators it would bring back **only where their vent is not already
/// tapped** (a Generator on a vent an earlier live one taps adds nothing, and
/// waking the earlier one moves the tap rather than adding one), and the
/// core's deep-bore surplus when `beacon` is the seat's core. A cost summed by
/// hand from the homed rows credited a Generator on a tapped vent with output
/// it would not add, so a beacon revived into a deficit and was shed again at
/// the next settle, every tick.
///
/// The beacon's own base is not in it: its key-core nets it out, as in
/// [`draw_of`]. The core's surplus counts for the same reason the Generators
/// do: [`supply_of`] counts it only while the core is live, so reviving the
/// core brings it back. Without that a shed core could never revive, since a
/// total blackout's headroom is 0 and the margin is positive, and spec section
/// 5 loses the surplus for good only when the core is **destroyed** (decisions
/// log item 113 (5)).
///
/// Nothing observes the world between the flip and the flip back: no event is
/// emitted, no hash is taken and no other seat's grid is read, so the
/// measurement leaves the world exactly as it found it.
///
/// # Errors
///
/// [`PowerError::NoSuchBeacon`] for an id with no dormancy row, and
/// [`PowerError::Overflow`] as [`headroom_of`] gives it or when the difference
/// does not fit.
fn revive_cost(
    world: &mut World,
    seat: SeatId,
    rules: PowerRules,
    beacon: BeaconId,
) -> Result<i32, PowerError> {
    let now = headroom(world, seat, rules)?;
    let was = usize::try_from(beacon.raw())
        .ok()
        .and_then(|row| world.beacons().dormant().get(row).copied())
        .ok_or(PowerError::NoSuchBeacon(beacon.raw()))?;
    set_dark(world, beacon.raw(), false);
    let awake = headroom(world, seat, rules);
    set_dark(world, beacon.raw(), was);
    now.checked_sub(awake?).ok_or(PowerError::Overflow)
}

/// The rank a swap compares (S1-22): the brownout order's first two terms, the
/// core above every other beacon and then the priority knob. A dark beacon
/// swaps only with lit beacons of a strictly lower rank, so two beacons of one
/// priority never swap on distance or id.
///
/// The core's term is the brownout order's own (the core is shed last,
/// whatever its knob), so a dark core outranks every lit expansion. `None`
/// for an id with no priority row: such a beacon takes no part in a swap,
/// rather than being read as the lowest priority.
fn swap_rank(world: &World, core: Option<BeaconId>, id: u32) -> Option<(bool, u8)> {
    let row = usize::try_from(id).ok()?;
    let priority = world.beacons().priorities().get(row).copied()?;
    Some((core.is_some_and(|core| core.raw() == id), priority))
}

/// Re-apply the brownout order after a priority change (item 127 (7), the
/// register's S1-22): relight a dark beacon at once when shedding lit beacons
/// of a lower rank covers its load.
///
/// The seat's dark beacons are tried highest [`swap_rank`] first and, within
/// a rank, **lowest id first**, so the seat walk in [`settle`] and this order
/// settle every tie as S1's plan words it: the lowest seat, then the lowest
/// beacon id, wherever the two stand. For each, the lit beacons of a strictly
/// lower rank are appended after the dark list in `order`, sorted in the
/// brownout order, and [`relight`] tries the swap. It works in
/// `order[base..]` and leaves `order[..base]` as it found it.
///
/// No state is kept: the check reads the priorities and dormancy the tables
/// already hold, every settle, so a raise committed on site is seen at the next
/// settle and nothing else is.
fn swap_in(
    world: &mut World,
    seat: SeatId,
    rules: PowerRules,
    order: &mut Vec<u32>,
    base: usize,
) -> Result<(), PowerError> {
    let core = core_of(world, seat);
    let core_at = core_site(world, core);
    order.truncate(base);
    {
        let beacons = world.beacons();
        for (row, owner) in beacons.seats().iter().enumerate() {
            if *owner == seat.raw()
                && beacon_row_is_dark(world, row)
                && let Some(id) = beacons.ids().get(row).copied()
                && swap_rank(world, core, id).is_some()
            {
                order.push(id);
            }
        }
    }
    if let Some(dark) = order.get_mut(base..) {
        // item 62: the key ends in the unique beacon id, so the order is total.
        dark.sort_unstable_by_key(|id| (core::cmp::Reverse(swap_rank(world, core, *id)), *id));
    }
    let end = order.len();
    let mut at = base;
    while at < end {
        let Some(beacon) = order.get(at).copied() else {
            break;
        };
        at = at.saturating_add(1);
        if !is_dark(world, beacon) {
            continue;
        }
        let Some(rank) = swap_rank(world, core, beacon) else {
            continue;
        };
        order.truncate(end);
        {
            let beacons = world.beacons();
            for (row, owner) in beacons.seats().iter().enumerate() {
                if *owner == seat.raw()
                    && beacon_row_is_live(world, row)
                    && let Some(id) = beacons.ids().get(row).copied()
                    && swap_rank(world, core, id).is_some_and(|lower| lower < rank)
                    && shed_key(world, core, core_at, id).is_some()
                {
                    order.push(id);
                }
            }
        }
        if let Some(lower) = order.get_mut(end..) {
            // item 62: `shed_key` ends in the unique beacon id, so the order is
            // total.
            lower.sort_unstable_by_key(|id| shed_key(world, core, core_at, *id));
        }
        relight(world, seat, rules, beacon, order, end)?;
    }
    order.truncate(base);
    Ok(())
}

/// Try one swap: light `beacon`, then shed the lit beacons at `order[from..]`
/// in that order, skipping any whose shed relieves nothing, until draw is no
/// higher than supply. Committed only when at least one beacon was shed and
/// the load then fits; otherwise every flag is put back as it was. `true` when
/// the swap was committed. It reports nothing: [`settle`] reports the net
/// change once the seat is settled.
///
/// A beacon whose load fits without shedding anything is not relit here: what
/// holds it dark is the revival margin, which the swap does not override. A
/// committed swap asks for draw no higher than supply and not for the margin,
/// so it can leave the seat's headroom anywhere from 0 kW up.
///
/// An error puts every flag back as it was, as a refused swap does, before it
/// is returned.
fn relight(
    world: &mut World,
    seat: SeatId,
    rules: PowerRules,
    beacon: u32,
    order: &[u32],
    from: usize,
) -> Result<bool, PowerError> {
    let Some(lower) = order.get(from..) else {
        return Ok(false);
    };
    if lower.is_empty() || !set_dark(world, beacon, false) {
        return Ok(false);
    }
    let attempt = try_swap(world, seat, rules, lower);
    if let Ok(true) = attempt {
        return Ok(true);
    }
    // Every beacon in `lower` was lit when the list was made, so each one dark
    // now was shed by this attempt.
    for candidate in lower {
        if is_dark(world, *candidate) {
            set_dark(world, *candidate, false);
        }
    }
    set_dark(world, beacon, true);
    attempt.map(|_| false)
}

/// [`relight`]'s attempt, with `beacon` already lit: `true` when shedding
/// from `lower` made the load fit and shed at least one beacon. A load that
/// fits with nothing shed is not a swap (the revival margin holds it).
fn try_swap(
    world: &mut World,
    seat: SeatId,
    rules: PowerRules,
    lower: &[u32],
) -> Result<bool, PowerError> {
    if headroom(world, seat, rules)? >= 0 {
        return Ok(false);
    }
    let mut shed_any = false;
    for candidate in lower {
        if headroom(world, seat, rules)? >= 0 {
            break;
        }
        shed_any |= shed_if_it_relieves(world, seat, rules, *candidate)?;
    }
    Ok(shed_any && headroom(world, seat, rules)? >= 0)
}

#[cfg(test)]
mod tests {
    use super::{PowerRow, PowerRules, PowerRulesError, PowerSum};
    use crate::rules::{RulesError, RulesTable};

    fn committed() -> RulesTable {
        let path = crate::default_rules_path()
            .unwrap_or_else(|| std::path::PathBuf::from("../../rules/rules.v1.json"));
        RulesTable::load(&path).unwrap_or_else(|error| panic!("the committed table: {error}"))
    }

    /// The committed table with one edit, as `RulesTable::from_message` takes
    /// it: refused or not.
    fn edited(
        edit: impl FnOnce(&mut pharmakos_proto::gp::v1::RulesTable),
    ) -> Result<RulesTable, RulesError> {
        let mut message = committed().message().clone();
        edit(&mut message);
        RulesTable::from_message(&message)
    }

    #[test]
    fn the_committed_table_passes_the_typed_read() {
        let read = PowerRules::read(committed().message());
        assert!(read.is_ok(), "{read:?}");
        assert_eq!(read.ok(), Some(PowerRules::of(&committed())));
    }

    /// Decisions-log item 135 (2) (b): the power phase's refusal is the load's,
    /// so no world holds a table the phase refuses and `PowerRules::of` has no
    /// fallback to take. A table without `structures.mortar` is the case the
    /// ruling names.
    #[test]
    fn a_table_without_a_kw_row_is_refused_at_load_by_name() {
        type Edit = fn(&mut pharmakos_proto::gp::v1::RulesTable);
        assert_eq!(
            edited(|m| {
                if let Some(block) = m.structures.as_mut() {
                    block.mortar = None;
                }
            }),
            Err(RulesError::Power(PowerRulesError::MissingRow(
                PowerRow::Mortar
            )))
        );
        let cases: [(PowerRow, Edit); 7] = [
            (PowerRow::Power, |m| m.power = None),
            (PowerRow::GeneratorOutput, |m| {
                if let Some(power) = m.power.as_mut() {
                    power.generator_output_kw = None;
                }
            }),
            (PowerRow::Autocannon, |m| {
                if let Some(block) = m.structures.as_mut() {
                    block.autocannon = None;
                }
            }),
            (PowerRow::SurveyPost, |m| {
                if let Some(block) = m.structures.as_mut() {
                    block.survey_post = None;
                }
            }),
            (PowerRow::ResonanceSpire, |m| {
                if let Some(block) = m.structures.as_mut() {
                    block.resonance_spire = None;
                }
            }),
            (PowerRow::Units, |m| m.units = None),
            (PowerRow::Mortar, |m| {
                if let Some(block) = m.structures.as_mut() {
                    block.mortar = None;
                }
            }),
        ];
        for (row, edit) in cases {
            assert_eq!(
                edited(edit),
                Err(RulesError::Power(PowerRulesError::MissingRow(row))),
                "{}",
                row.path()
            );
        }
    }

    #[test]
    fn a_row_above_a_signed_kw_is_refused_rather_than_saturated() {
        assert_eq!(
            edited(|m| {
                if let Some(power) = m.power.as_mut() {
                    power.revive_margin_kw = u32::MAX;
                }
            }),
            Err(RulesError::Power(PowerRulesError::OutOfRange {
                row: PowerRow::ReviveMargin,
                value: u32::MAX,
            }))
        );
    }

    /// Item 135 (2) (b)'s bound: each `kW` row times the table room it counts
    /// over, summed, must fit a signed 32-bit `kW`, for supply and for draw.
    /// The rows that fit one at a time are refused once a full table of them
    /// would not: a Generator output the structure room (120) multiplies past
    /// `i32::MAX`, and a unit draw the unit room (300) and the starting force
    /// multiply past it.
    #[test]
    fn a_table_whose_largest_supply_or_draw_leaves_a_signed_kw_is_refused() {
        let share = |parts: u32| {
            i32::MAX
                .unsigned_abs()
                .checked_div(parts)
                .unwrap_or_else(|| panic!("a share"))
        };
        let generator = share(100);
        let supply = edited(|m| {
            if let Some(by) = m
                .power
                .as_mut()
                .and_then(|p| p.generator_output_kw.as_mut())
            {
                by.rich = generator;
            }
        });
        assert!(
            matches!(
                supply,
                Err(RulesError::Power(PowerRulesError::SumOutOfRange {
                    sum: PowerSum::Supply,
                    ..
                }))
            ),
            "{supply:?}"
        );
        let unit = share(200);
        let draw = edited(|m| {
            if let Some(power) = m.power.as_mut() {
                power.kw_per_unit = unit;
            }
        });
        assert!(
            matches!(
                draw,
                Err(RulesError::Power(PowerRulesError::SumOutOfRange {
                    sum: PowerSum::Draw,
                    ..
                }))
            ),
            "{draw:?}"
        );
        let structure = share(100);
        let heavy = edited(|m| {
            if let Some(mortar) = m.structures.as_mut().and_then(|b| b.mortar.as_mut()) {
                mortar.draw_kw = structure;
            }
        });
        assert!(
            matches!(
                heavy,
                Err(RulesError::Power(PowerRulesError::SumOutOfRange {
                    sum: PowerSum::Draw,
                    ..
                }))
            ),
            "{heavy:?}"
        );
    }

    /// The bound is exact at its edge: the largest unit draw whose full table
    /// fits loads, and one `kW` more is refused.
    #[test]
    fn the_sum_bound_admits_exactly_the_tables_whose_sums_fit() {
        let rules = committed();
        let units = rules
            .message()
            .units
            .unwrap_or_else(|| panic!("a units block"));
        let power = PowerRules::of(&rules);
        let heaviest = i64::from(
            power
                .mortar_draw
                .max(power.survey_post_draw)
                .max(power.spire_draw),
        );
        let fielded = i64::from(crate::world::UNIT_TABLE_ROOM)
            + 1
            + i64::from(units.starting_build_drones)
            + i64::from(units.starting_mining_drones);
        let room = i64::from(crate::world::STRUCTURE_TABLE_ROOM);
        let largest = (i64::from(i32::MAX) - heaviest * room)
            .checked_div(fielded)
            .unwrap_or_else(|| panic!("a unit draw"));
        let at = |per_unit: i64| {
            edited(|m| {
                if let Some(power) = m.power.as_mut() {
                    power.kw_per_unit = u32::try_from(per_unit).unwrap_or_else(|_| panic!("a row"));
                }
            })
        };
        assert!(at(largest).is_ok(), "{:?}", at(largest));
        assert!(matches!(
            at(largest + 1),
            Err(RulesError::Power(PowerRulesError::SumOutOfRange {
                sum: PowerSum::Draw,
                ..
            }))
        ));
    }
}
