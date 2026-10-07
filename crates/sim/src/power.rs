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
//! (5)). The meter shows the net figures and says so beside itself (S1's plan,
//! decision 12, ruled by item 128: the register's S1-31).
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
//! fits. Dark beacons are tried in the revival order, so of two seats the
//! lower is settled first and of two equal beacons the lower id swaps first.
//! A beacon whose load fits without shedding anything is held by the revival
//! margin alone, which the swap does not override.
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
        }
    }
}

impl std::error::Error for PowerRulesError {}

/// Why a power read could not answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerError {
    /// The rules table is refused ([`PowerRules::read`]).
    Rules(PowerRulesError),
    /// The seat is not seated in this match.
    NoSuchSeat(SeatId),
    /// A `kW` sum does not fit a signed 32-bit `kW`.
    Overflow,
}

impl fmt::Display for PowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PowerError::Rules(error) => write!(f, "{error}"),
            PowerError::NoSuchSeat(seat) => write!(f, "seat {} is not seated", seat.raw()),
            PowerError::Overflow => write!(f, "a kW sum does not fit a signed 32-bit kW"),
        }
    }
}

impl std::error::Error for PowerError {}

impl From<PowerRulesError> for PowerError {
    fn from(error: PowerRulesError) -> PowerError {
        PowerError::Rules(error)
    }
}

/// The `kW` rows the power phase reads, taken once per tick.
///
/// A flat copy rather than a borrow of the rules table, because the phase
/// writes the tables the same table is reached through, and because reading
/// ten rows once a tick is cheaper than reading them once a beacon.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
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
    /// Read every `kW` row the phase needs, refusing a table that lacks one.
    ///
    /// # Errors
    ///
    /// [`PowerRulesError::MissingRow`] naming the first absent block or row,
    /// in the order the struct lists them, or
    /// [`PowerRulesError::OutOfRange`] for a row above `i32::MAX`.
    pub fn read(rules: &RulesTable) -> Result<PowerRules, PowerRulesError> {
        let message = rules.message();
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
        Ok(PowerRules {
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
        })
    }

    /// Every `kW` row the phase needs, for a caller with no error to return.
    ///
    /// This is [`PowerRules::read`] for the callers that cannot yet carry its
    /// error: the power phase itself, the Quartermaster's headroom
    /// (`crate::world`), the Build mandate's price (`crate::mandate`) and the
    /// verifier's projection. A table [`PowerRules::read`] refuses reads as
    /// every row zero here, which is the default this read exists to retire:
    /// the refusal belongs where the table is loaded, once, so that no world
    /// can hold a table the phase refuses (raised in the `grid` lane's pull
    /// request; those callers are outside its named place). The committed
    /// table passes [`PowerRules::read`], and a test says so.
    #[must_use]
    pub fn of(rules: &RulesTable) -> PowerRules {
        PowerRules::read(rules).unwrap_or_default()
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
/// [`PowerError::Rules`] when the world's rules table is refused by
/// [`PowerRules::read`], [`PowerError::NoSuchSeat`] for a seat that is not
/// seated, and [`PowerError::Overflow`] when a difference does not fit.
pub fn dark_load(world: &World, seat: SeatId) -> Result<DarkLoad, PowerError> {
    let rules = PowerRules::read(world.rules())?;
    if world.seat_row(seat).is_none() {
        return Err(PowerError::NoSuchSeat(seat));
    }
    let draw_lit = draw_under(world, seat, rules, Lighting::AllLit);
    let draw_now = draw_under(world, seat, rules, Lighting::AsIs);
    let supply_lit = supply_under(world, seat, rules, Lighting::AllLit);
    let supply_now = supply_under(world, seat, rules, Lighting::AsIs);
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
/// (G3′ §9.17): it holds the brownout order of one seat's beacons at a time,
/// and for the swap ([`swap_in`]) one seat's dark beacons followed by the lit
/// ones below one of them, which together are never more than the seat's
/// beacons.
pub(crate) fn settle(world: &mut World, order: &mut Vec<u32>) {
    let rules = PowerRules::of(world.rules());
    let seats = usize::try_from(world.seats().len()).unwrap_or(0);
    let mut index: usize = 0;
    while index < seats {
        let raw = world.seats().seats().get(index).copied().unwrap_or(0);
        let seat = SeatId::new(raw);
        if world.seats().is_alive(index) {
            brown_out(world, seat, rules, order);
            revive(world, seat, rules, order);
            swap_in(world, seat, rules, order);
        }
        let supply = supply_of(world, seat, rules);
        let draw = draw_of(world, seat, rules);
        {
            let columns = world.seats_mut().power_columns();
            if let Some(slot) = columns.supply.get_mut(index) {
                *slot = Kw::new(supply);
            }
            if let Some(slot) = columns.draw.get_mut(index) {
                *slot = Kw::new(draw);
            }
        }
        index = index.saturating_add(1);
    }
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
#[must_use]
pub(crate) fn supply_of(world: &World, seat: SeatId, rules: PowerRules) -> i32 {
    supply_under(world, seat, rules, Lighting::AsIs)
}

/// [`supply_of`] under a [`Lighting`].
fn supply_under(world: &World, seat: SeatId, rules: PowerRules, lighting: Lighting) -> i32 {
    let mut total: i32 = 0;
    let core = core_of(world, seat);
    if let Some(core) = core
        && beacon_counts(world, core, lighting)
    {
        total = total.saturating_add(rules.core_surplus);
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
                total = total.saturating_add(rules.generator_output(grade));
            }
        }
        row = row.saturating_add(1);
    }
    total
}

/// The seat's draw: every unit and every capability structure homed to a live
/// beacon.
///
/// **No beacon's base is in it**, the core's included: a live beacon's own
/// key-core supplies exactly its base ([`PowerRules::key_core_output`]), so the
/// beacon is net zero and the column shows the net (see the module docs). A
/// beacon with nothing homed to it adds nothing here.
#[must_use]
pub(crate) fn draw_of(world: &World, seat: SeatId, rules: PowerRules) -> i32 {
    draw_under(world, seat, rules, Lighting::AsIs)
}

/// [`draw_of`] under a [`Lighting`].
fn draw_under(world: &World, seat: SeatId, rules: PowerRules, lighting: Lighting) -> i32 {
    let mut total: i32 = unit_draw_of(world, seat, rules, lighting);
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
                total = total.saturating_add(rules.structure_draw(kind));
            }
        }
        row = row.saturating_add(1);
    }
    total
}

/// What the seat's units draw.
///
/// A unit draws while its home counts as lit, and only then, whatever it is
/// doing: so the commander, which a blackout never parks, draws nothing while
/// its home is dark and walks on (item 127 (5)).
fn unit_draw_of(world: &World, seat: SeatId, rules: PowerRules, lighting: Lighting) -> i32 {
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
            total = total.saturating_add(rules.per_unit);
        }
        row = row.saturating_add(1);
    }
    total
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

/// The seat's headroom: supply less draw, as the next settle reads them.
fn headroom(world: &World, seat: SeatId, rules: PowerRules) -> i32 {
    supply_of(world, seat, rules).saturating_sub(draw_of(world, seat, rules))
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
/// when the beacon is now shed.
fn shed_if_it_relieves(world: &mut World, seat: SeatId, rules: PowerRules, beacon: u32) -> bool {
    if is_dark(world, beacon) {
        return false;
    }
    let before = headroom(world, seat, rules);
    if !set_dark(world, beacon, true) {
        return false;
    }
    if headroom(world, seat, rules) > before {
        return true;
    }
    set_dark(world, beacon, false);
    false
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
/// reader sees is the order the spec writes down.
fn brown_out(world: &mut World, seat: SeatId, rules: PowerRules, order: &mut Vec<u32>) {
    if headroom(world, seat, rules) >= 0 {
        return;
    }
    shed_order(world, seat, order);
    let mut at: usize = 0;
    while at < order.len() {
        if headroom(world, seat, rules) >= 0 {
            break;
        }
        let Some(beacon) = order.get(at).copied() else {
            break;
        };
        at = at.saturating_add(1);
        if shed_if_it_relieves(world, seat, rules, beacon) {
            report(world, seat, beacon, EventKind::BeaconBrownedOut);
        }
    }
    order.clear();
}

/// The brownout order's key for one beacon: the core last, then lowest
/// priority, then furthest from the core, then ascending id.
///
/// item 62: the key ends in the beacon id, which is unique, so the order is
/// total. `is_core` sorts last as a plain `bool`, which is the core's own rule;
/// the distance is negated by `Reverse` so the furthest sheds first.
fn shed_key(
    world: &World,
    core: Option<BeaconId>,
    core_at: Option<[Fx; 3]>,
    id: u32,
) -> (bool, u8, core::cmp::Reverse<Sq>, u32) {
    let beacons = world.beacons();
    let row = usize::try_from(id).unwrap_or(usize::MAX);
    let priority = beacons.priorities().get(row).copied().unwrap_or(0);
    let at = beacons.positions().get(row).copied();
    let distance = match (at, core_at) {
        (Some(here), Some(there)) => Sq::between(here, there),
        _ => Sq::ZERO,
    };
    let is_core = core.is_some_and(|core| core.raw() == id);
    (is_core, priority, core::cmp::Reverse(distance), id)
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
fn shed_order(world: &World, seat: SeatId, order: &mut Vec<u32>) {
    order.clear();
    let core = core_of(world, seat);
    let core_at = core_site(world, core);
    let beacons = world.beacons();
    let count = usize::try_from(beacons.len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
        if beacons.seats().get(row).copied() == Some(seat.raw()) && beacon_row_is_live(world, row) {
            order.push(beacons.ids().get(row).copied().unwrap_or(0));
        }
        row = row.saturating_add(1);
    }
    // item 62: `shed_key` ends in the unique beacon id, so the order is total.
    order.sort_unstable_by_key(|id| shed_key(world, core, core_at, *id));
}

/// Bring dormant beacons back, best first, while the margin allows it.
///
/// The margin is spec section 5's: a beacon revives "only once supply exceeds
/// draw by a margin", and the margin is measured **with that beacon's own load
/// added back**, so a beacon cannot revive into a deficit it immediately causes
/// and then be shed again next tick. That is the whole anti-flicker rule, and
/// it holds only because [`revive_cost`] measures that load with the same
/// [`supply_of`] and [`draw_of`] the next settle sheds by.
fn revive(world: &mut World, seat: SeatId, rules: PowerRules, order: &mut Vec<u32>) {
    let mut guard: u32 = 0;
    let limit = world.beacons().len();
    while guard <= limit {
        guard = guard.saturating_add(1);
        revive_order(world, seat, order);
        let mut woke = false;
        let mut at: usize = 0;
        while at < order.len() {
            let Some(beacon) = order.get(at).copied() else {
                break;
            };
            at = at.saturating_add(1);
            let cost = revive_cost(world, seat, rules, BeaconId::new(beacon));
            if headroom(world, seat, rules).saturating_sub(cost) < rules.revive_margin {
                continue;
            }
            set_dark(world, beacon, false);
            report(world, seat, beacon, EventKind::BeaconRevived);
            woke = true;
            break;
        }
        if !woke {
            break;
        }
    }
    order.clear();
}

/// The revival order's key for one beacon: every term of the shed key,
/// reversed — the core first, then the highest priority, then the nearest.
///
/// item 62: the key still ends in the unique beacon id, ascending, so it is
/// total.
fn revive_key(
    world: &World,
    core: Option<BeaconId>,
    core_at: Option<[Fx; 3]>,
    id: u32,
) -> (bool, core::cmp::Reverse<u8>, Sq, u32) {
    let (is_core, priority, core::cmp::Reverse(distance), id) = shed_key(world, core, core_at, id);
    (!is_core, core::cmp::Reverse(priority), distance, id)
}

/// The order beacons come back in: the brownout order, backwards.
fn revive_order(world: &World, seat: SeatId, order: &mut Vec<u32>) {
    order.clear();
    let core = core_of(world, seat);
    let core_at = core_site(world, core);
    let beacons = world.beacons();
    let count = usize::try_from(beacons.len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
        let mine = beacons.seats().get(row).copied() == Some(seat.raw());
        let alive = beacons
            .hit_points()
            .get(row)
            .is_some_and(|hp| hp.is_alive());
        let dormant = beacons.dormant().get(row).copied() == Some(true);
        if mine && alive && dormant {
            order.push(beacons.ids().get(row).copied().unwrap_or(0));
        }
        row = row.saturating_add(1);
    }
    // item 62: `revive_key` ends in the unique beacon id, so the order is total.
    order.sort_unstable_by_key(|id| revive_key(world, core, core_at, *id));
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
fn revive_cost(world: &mut World, seat: SeatId, rules: PowerRules, beacon: BeaconId) -> i32 {
    let now = headroom(world, seat, rules);
    let Ok(row) = usize::try_from(beacon.raw()) else {
        return 0;
    };
    let Some(was) = world.beacons().dormant().get(row).copied() else {
        return 0;
    };
    set_dark(world, beacon.raw(), false);
    let awake = headroom(world, seat, rules);
    set_dark(world, beacon.raw(), was);
    now.saturating_sub(awake)
}

/// The rank a swap compares (S1-22): the brownout order's first two terms, the
/// core above every other beacon and then the priority knob. A dark beacon
/// swaps only with lit beacons of a strictly lower rank, so two beacons of one
/// priority never swap on distance or id.
fn swap_rank(world: &World, core: Option<BeaconId>, id: u32) -> (bool, u8) {
    let row = usize::try_from(id).unwrap_or(usize::MAX);
    let priority = world.beacons().priorities().get(row).copied().unwrap_or(0);
    (core.is_some_and(|core| core.raw() == id), priority)
}

/// Re-apply the brownout order after a priority change (item 127 (7), the
/// register's S1-22): relight a dark beacon at once when shedding lit beacons
/// of a lower rank covers its load.
///
/// Dark beacons are tried in the revival order (the core first, then the
/// highest priority, the nearest, the lowest id), so the seat walk in
/// [`settle`] and that order settle every tie: the lowest seat, then the
/// lowest beacon id. For each, the lit beacons of a strictly lower
/// [`swap_rank`] are appended after the dark list in `order`, sorted in the
/// brownout order, and [`relight`] tries the swap.
///
/// No state is kept: the check reads the priorities and dormancy the tables
/// already hold, every settle, so a raise committed on site is seen at the next
/// settle and nothing else is.
fn swap_in(world: &mut World, seat: SeatId, rules: PowerRules, order: &mut Vec<u32>) {
    revive_order(world, seat, order);
    let dark = order.len();
    let core = core_of(world, seat);
    let core_at = core_site(world, core);
    let mut at: usize = 0;
    while at < dark {
        let Some(beacon) = order.get(at).copied() else {
            break;
        };
        at = at.saturating_add(1);
        if !is_dark(world, beacon) {
            continue;
        }
        let rank = swap_rank(world, core, beacon);
        order.truncate(dark);
        {
            let beacons = world.beacons();
            let count = usize::try_from(beacons.len()).unwrap_or(0);
            let mut row: usize = 0;
            while row < count {
                if beacons.seats().get(row).copied() == Some(seat.raw())
                    && beacon_row_is_live(world, row)
                    && let Some(id) = beacons.ids().get(row).copied()
                    && swap_rank(world, core, id) < rank
                {
                    order.push(id);
                }
                row = row.saturating_add(1);
            }
        }
        if let Some(lower) = order.get_mut(dark..) {
            // item 62: `shed_key` ends in the unique beacon id, so the order is
            // total.
            lower.sort_unstable_by_key(|id| shed_key(world, core, core_at, *id));
        }
        relight(world, seat, rules, beacon, order, dark);
    }
    order.clear();
}

/// Try one swap: light `beacon`, then shed the lit beacons at `order[from..]`
/// in that order, skipping any whose shed relieves nothing, until draw is no
/// higher than supply. Committed, with its events, only when at least one
/// beacon was shed and the load then fits; otherwise every flag is put back as
/// it was and nothing is reported. `true` when the swap was committed.
///
/// A beacon whose load fits without shedding anything is not relit here: what
/// holds it dark is the revival margin, which the swap does not override.
fn relight(
    world: &mut World,
    seat: SeatId,
    rules: PowerRules,
    beacon: u32,
    order: &[u32],
    from: usize,
) -> bool {
    let lower = order.get(from..).unwrap_or(&[]);
    if lower.is_empty() || !set_dark(world, beacon, false) {
        return false;
    }
    if headroom(world, seat, rules) >= 0 {
        set_dark(world, beacon, true);
        return false;
    }
    let mut shed_any = false;
    for candidate in lower {
        if headroom(world, seat, rules) >= 0 {
            break;
        }
        shed_any |= shed_if_it_relieves(world, seat, rules, *candidate);
    }
    if shed_any && headroom(world, seat, rules) >= 0 {
        for candidate in lower {
            if is_dark(world, *candidate) {
                report(world, seat, *candidate, EventKind::BeaconBrownedOut);
            }
        }
        report(world, seat, beacon, EventKind::BeaconRevived);
        return true;
    }
    for candidate in lower {
        if is_dark(world, *candidate) {
            set_dark(world, *candidate, false);
        }
    }
    set_dark(world, beacon, true);
    false
}

#[cfg(test)]
mod tests {
    use super::{PowerRow, PowerRules, PowerRulesError};
    use crate::rules::RulesTable;

    fn committed() -> RulesTable {
        let path = crate::default_rules_path()
            .unwrap_or_else(|| std::path::PathBuf::from("../../rules/rules.v1.json"));
        RulesTable::load(&path).unwrap_or_else(|error| panic!("the committed table: {error}"))
    }

    fn without(edit: impl FnOnce(&mut pharmakos_proto::gp::v1::RulesTable)) -> RulesTable {
        let mut message = committed().message().clone();
        edit(&mut message);
        RulesTable::from_message(&message).unwrap_or_else(|error| panic!("a table: {error}"))
    }

    #[test]
    fn the_committed_table_passes_the_typed_read() {
        let read = PowerRules::read(&committed());
        assert!(read.is_ok(), "{read:?}");
        assert_eq!(read.ok(), Some(PowerRules::of(&committed())));
    }

    #[test]
    fn a_missing_row_is_refused_by_name_rather_than_read_as_zero() {
        let cases: [(PowerRow, RulesTable); 7] = [
            (PowerRow::Power, without(|m| m.power = None)),
            (
                PowerRow::GeneratorOutput,
                without(|m| {
                    if let Some(power) = m.power.as_mut() {
                        power.generator_output_kw = None;
                    }
                }),
            ),
            (PowerRow::Structures, without(|m| m.structures = None)),
            (
                PowerRow::Autocannon,
                without(|m| {
                    if let Some(block) = m.structures.as_mut() {
                        block.autocannon = None;
                    }
                }),
            ),
            (
                PowerRow::Mortar,
                without(|m| {
                    if let Some(block) = m.structures.as_mut() {
                        block.mortar = None;
                    }
                }),
            ),
            (
                PowerRow::SurveyPost,
                without(|m| {
                    if let Some(block) = m.structures.as_mut() {
                        block.survey_post = None;
                    }
                }),
            ),
            (
                PowerRow::ResonanceSpire,
                without(|m| {
                    if let Some(block) = m.structures.as_mut() {
                        block.resonance_spire = None;
                    }
                }),
            ),
        ];
        for (row, table) in cases {
            assert_eq!(
                PowerRules::read(&table),
                Err(PowerRulesError::MissingRow(row)),
                "{}",
                row.path()
            );
        }
    }

    #[test]
    fn a_row_above_a_signed_kw_is_refused_rather_than_saturated() {
        let table = without(|m| {
            if let Some(power) = m.power.as_mut() {
                power.revive_margin_kw = u32::MAX;
            }
        });
        assert_eq!(
            PowerRules::read(&table),
            Err(PowerRulesError::OutOfRange {
                row: PowerRow::ReviveMargin,
                value: u32::MAX,
            })
        );
    }
}
