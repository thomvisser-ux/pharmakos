// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mandate layer: what a beacon's writ asks to be paid for (spec section
//! 6; decisions log item 22).
//!
//! # Two layers, and this is the upper one
//!
//! Spec section 6: "Each beacon has two layers. The mandate says **what** to do
//! and carries its settings; the mandate's built-in program decides **how**,
//! every tick. Units and buildings run built-in programs of their own, a
//! separate layer working under the mandate's orders, and the player edits
//! none of them." [`Mandate`] is the upper layer and [`crate::programs`] is the
//! lower one.
//!
//! What a mandate does here is narrow on purpose: it offers the Quartermaster
//! **at most one spend request per tick**. The round robin is what stops a
//! beacon hogging the treasury (spec section 7), and a beacon allowed to file
//! five requests would walk straight around it.
//!
//! # Fabrication follows the work in hand
//!
//! Item 22: "A beacon's fabricator produces a drone only while its mandate has
//! outstanding work its current drones cannot keep up with — unbuilt or damaged
//! targets, reachable ore — and stops when the work is covered. No counts, no
//! cap; headroom is the normal state." The threshold is
//! `economy.backlog_threshold_per_drone`, and the comparison is *work items per
//! available drone*, which is why a beacon with no drone and any work at all
//! asks for one and a beacon with a drone and one item does not.
//!
//! # The script arm stays shut
//!
//! [`crate::seams::BeaconMandate`] carries a `program_id`, which is the
//! `mandate {builtin|script}` seam. Only the built-in arm exists and nothing
//! reads the seam (AGENTS.md §2).

use crate::economy::{Purchase, SpendRequest, Urgency};
use crate::math::fixed::{Fx, Sq};
use crate::math::quantity::{Kw, Money};
use crate::seams::MandateKind;
use crate::tables::{BeaconId, SeatId, StructureKind, TargetKind, UnitKind};
use crate::world::World;

/// The mandate seam: one writ, one question.
///
/// Five mandates exist in the schema; **three run at the skeleton** — Build,
/// Mine and Survey — and Defend and Attack arrive with combat at S2
/// ([`mandate_for`] answers `None` for them rather than pretending). A mandate
/// that wants nothing this tick answers `None`, which is the normal state:
/// "headroom is the normal state" (item 22).
pub trait Mandate {
    /// Which writ this is.
    fn kind(&self) -> MandateKind;

    /// What this beacon would like paid for, this tick, or `None`.
    ///
    /// Read-only: a mandate never spends. It asks, the Quartermaster decides,
    /// and the Quartermaster phase commits — which is what keeps "all spending
    /// draws from the treasury through the Quartermaster" (spec section 7) true
    /// by construction rather than by agreement.
    fn request(&self, world: &World, beacon: BeaconId) -> Option<SpendRequest>;
}

/// The built-in Build mandate: the Generator blueprint and a target list.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Build;

/// The built-in Mine mandate: a mining drone while the seam it holds has work
/// left ([`crate::mining`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mine;

/// The built-in Survey mandate: scouts up to the setting's count.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Survey;

/// The built-in mandate a writ runs, or `None` for one this build does not run.
///
/// Zero-sized statics rather than values on the beacon table, because a
/// built-in mandate carries no state of its own: everything it reads is the
/// world and the beacon's own settings columns.
#[must_use]
pub fn mandate_for(kind: MandateKind) -> Option<&'static dyn Mandate> {
    match kind {
        MandateKind::Build => Some(&Build),
        MandateKind::Mine => Some(&Mine),
        MandateKind::Survey => Some(&Survey),
        // Defend and Attack are S2's, with destruction and combat. `None` is
        // the honest answer: a beacon on one of those writs asks for nothing
        // and its units idle at it under the common settings, which is exactly
        // spec section 6's "a unit the mandate has no job for".
        MandateKind::Defend | MandateKind::Attack | MandateKind::None => None,
    }
}

impl Mandate for Build {
    fn kind(&self) -> MandateKind {
        MandateKind::Build
    }

    fn request(&self, world: &World, beacon: BeaconId) -> Option<SpendRequest> {
        let seat = seat_of(world, beacon)?;
        // The highest-order affordable target that is unbuilt (spec section 6,
        // the Build row): the lowest `order`, ties by list position, among the
        // targets the treasury covers ([`next_unpaid_target`]).
        if let Some(row) = next_unpaid_target(world, beacon) {
            let blueprint = world
                .targets()
                .blueprints()
                .get(row)
                .copied()
                .and_then(StructureKind::from_id)?;
            let (cost, draw) = structure_price(world, blueprint);
            return Some(SpendRequest {
                seat,
                beacon,
                urgency: Urgency::Build,
                cost,
                draw,
                buys: Purchase::Structure(u32::try_from(row).ok()?),
            });
        }
        // Otherwise the fabricator, and only while the work outruns the drones.
        let work = building_work(world, beacon);
        let drones = drones_homed(world, beacon, UnitKind::BuildDrone);
        backlogged(world, work, drones)
            .then(|| unit_request(world, seat, beacon, UnitKind::BuildDrone, Urgency::Units))
    }
}

impl Mandate for Mine {
    fn kind(&self) -> MandateKind {
        MandateKind::Mine
    }

    fn request(&self, world: &World, beacon: BeaconId) -> Option<SpendRequest> {
        let seat = seat_of(world, beacon)?;
        // "Reachable ore" is the work in hand (item 22): the seam the beacon
        // holds, while it has a voxel left that the beacon's settings allow
        // and a drone can safely take (`crate::mining`). One item rather than
        // a voxel count, because a seam is a single job: the beacon holds it
        // until it is spent, and counting the voxels would ask the fabricator
        // to field a drone per two voxels of scrap. A spent seam is no work,
        // so the fabricator stops.
        let work = u32::from(world.next_dig(beacon).is_some());
        let drones = drones_homed(world, beacon, UnitKind::MiningDrone);
        // Filed in the Mine band, last on spec section 7's ladder ("defend
        // under attack, then repair, units, build, mine"): the drone is what
        // this mandate spends on, so it is the mandate's own spend.
        backlogged(world, work, drones)
            .then(|| unit_request(world, seat, beacon, UnitKind::MiningDrone, Urgency::Mine))
    }
}

impl Mandate for Survey {
    fn kind(&self) -> MandateKind {
        MandateKind::Survey
    }

    fn request(&self, world: &World, beacon: BeaconId) -> Option<SpendRequest> {
        let seat = seat_of(world, beacon)?;
        let row = usize::try_from(beacon.raw()).ok()?;
        let wanted = u32::from(world.beacons().scout_counts().get(row).copied()?);
        let have = drones_homed(world, beacon, UnitKind::Scout);
        // A count rather than a backlog: spec section 6's Survey row names
        // "scout count" as a setting, so this is the one mandate the player
        // gives a number to and the fabricator fills it exactly.
        (have < wanted).then(|| unit_request(world, seat, beacon, UnitKind::Scout, Urgency::Units))
    }
}

/// The seat a beacon belongs to, or `None` when it is not a live beacon of a
/// seat still in the match.
fn seat_of(world: &World, beacon: BeaconId) -> Option<SeatId> {
    let row = usize::try_from(beacon.raw()).ok()?;
    let raw = world.beacons().seats().get(row).copied()?;
    let seat = SeatId::new(raw);
    seat.is_some().then_some(seat)
}

/// One structure kind's price and draw.
fn structure_price(world: &World, kind: StructureKind) -> (Money, Kw) {
    let rules = crate::power::PowerRules::of(world.rules());
    let cost = world.structure_cost(kind);
    (cost, Kw::new(rules.structure_draw(kind)))
}

/// A fabrication request for one unit kind, in the urgency band it is filed
/// under.
fn unit_request(
    world: &World,
    seat: SeatId,
    beacon: BeaconId,
    kind: UnitKind,
    urgency: Urgency,
) -> SpendRequest {
    SpendRequest {
        seat,
        beacon,
        urgency,
        cost: world.unit_cost(kind),
        draw: Kw::new(crate::power::PowerRules::of(world.rules()).per_unit),
        buys: Purchase::Unit(kind),
    }
}

/// Item 22's threshold: work items per available drone, above which the
/// fabricator produces.
///
/// A beacon with **no** drone and any work at all is backlogged whatever the
/// threshold says, because "work its current drones cannot keep up with" is
/// trivially true of no drones.
fn backlogged(world: &World, work: u32, drones: u32) -> bool {
    if work == 0 {
        return false;
    }
    if drones == 0 {
        return true;
    }
    let threshold = world
        .rules()
        .message()
        .economy
        .as_ref()
        .map_or(0, |economy| economy.backlog_threshold_per_drone);
    work > drones.saturating_mul(threshold)
}

/// How many living units of `kind` are homed to `beacon`.
fn drones_homed(world: &World, beacon: BeaconId, kind: UnitKind) -> u32 {
    let units = world.units();
    let count = usize::try_from(units.len()).unwrap_or(0);
    let mut total: u32 = 0;
    let mut row: usize = 0;
    while row < count {
        let homed = units.homes().get(row).copied() == Some(beacon.raw());
        let right = units.kinds().get(row).copied() == Some(kind.id());
        let alive = units.hit_points().get(row).is_some_and(|hp| hp.is_alive());
        if homed && right && alive {
            total = total.saturating_add(1);
        }
        row = row.saturating_add(1);
    }
    total
}

/// The Build target of `beacon` the mandate pays for next: **the highest-order
/// affordable target that is unbuilt** (spec section 6, the Build row; S1's
/// plan, decision 5), as a row of the target table.
///
/// "Highest-order" is the lowest `BuildTarget.order`, ties by list position
/// (`playbook.proto`), which is the row's position among its beacon's targets:
/// a settings edit writes its list in order and an added target goes after the
/// rest. "Affordable" is a cost the seat's treasury covers now. An unaffordable
/// target **waits**: it is passed over for the highest-order one the treasury
/// covers, and asked for again once it is covered. When no unbuilt target is
/// affordable the highest-order one is still the one asked for, so the
/// Quartermaster holds it -- an order the treasury cannot cover is not a spend
/// and fails nothing (spec section 7; only the deploy fails a step for want of
/// money, decision 7) -- and the fabricator waits behind it, as before S1.
///
/// Passed over as unbuildable ([`buildable`]):
///
/// * a target whose column a live structure of **any** seat already stands
///   on -- one structure per voxel, so the first seat to build wins and the
///   other's target fails, which here means it is never paid for;
/// * a target bound to a feature that is **lost** -- a name idles when its
///   target is lost, and a held description that found nothing to read again
///   idles with it ([`World`]'s re-read runs first, on the same tick);
/// * a target inside one of the beacon's own **protected areas**, ground the
///   mandate will not build on ([`protected`]).
#[must_use]
pub(crate) fn next_unpaid_target(world: &World, beacon: BeaconId) -> Option<usize> {
    let treasury = seat_of(world, beacon)
        .and_then(|seat| world.seat_row(seat))
        .and_then(|index| world.seats().treasuries().get(index).copied())?;
    let targets = world.targets();
    let mut first: Option<(u32, usize)> = None;
    let mut affordable: Option<(u32, usize)> = None;
    let rows = targets
        .beacons()
        .iter()
        .zip(targets.kinds())
        .zip(targets.built().iter().zip(targets.orders()));
    for (row, ((owner, kind), (built, order))) in rows.enumerate() {
        if *owner != beacon.raw()
            || *kind != TargetKind::Build.id()
            || *built != BeaconId::NONE.raw()
            || !buildable(world, beacon, row)
        {
            continue;
        }
        let Some(cost) = target_cost(world, row) else {
            continue;
        };
        // item 62: the row is unique, so the key is a total order.
        let key = (*order, row);
        if first.is_none_or(|held| key < held) {
            first = Some(key);
        }
        if cost.raw() <= treasury.raw() && affordable.is_none_or(|held| key < held) {
            affordable = Some(key);
        }
    }
    affordable.or(first).map(|(_, row)| row)
}

/// What the Build target at `row` costs: its blueprint's build cost. `None` for
/// a row whose blueprint this build does not name, which cannot be built.
fn target_cost(world: &World, row: usize) -> Option<Money> {
    let kind = world
        .targets()
        .blueprints()
        .get(row)
        .copied()
        .and_then(StructureKind::from_id)?;
    Some(world.structure_cost(kind))
}

/// Whether the Build target at `row` of `beacon` can still be built: nothing
/// stands on its column, the feature it is bound to, if any, is not lost, and
/// it lies in none of the beacon's protected areas.
fn buildable(world: &World, beacon: BeaconId, row: usize) -> bool {
    let targets = world.targets();
    let (Some(anchor), Some(feature)) = (
        targets.anchors().get(row).copied(),
        targets.features().get(row).copied(),
    ) else {
        return false;
    };
    let [x, y] = crate::targeting::column_of(anchor);
    if world.ground().structure_on(x, y) || protected(world, beacon, anchor) {
        return false;
    }
    feature == crate::targeting::NO_FEATURE
        || usize::try_from(feature).is_ok_and(|index| world.feature_is_live(index))
}

/// Whether `at` lies inside one of `beacon`'s own **protected areas**: ground
/// its Build mandate will not build on (`playbook.proto`'s `protected_areas`;
/// spec section 6's Build row; S1's plan, decision 5).
///
/// The test is on the **column**: `at`'s column is inside when it lies in the
/// area's box, edges included, whatever its height
/// ([`crate::tables::ColumnBox::contains`]).
/// The box is the one the author drew, so a column one step outside it is
/// outside the area.
#[must_use]
pub(crate) fn protected(world: &World, beacon: BeaconId, at: [Fx; 3]) -> bool {
    let [x, y] = crate::targeting::column_of(at);
    let targets = world.targets();
    targets
        .beacons()
        .iter()
        .zip(targets.kinds())
        .zip(targets.areas())
        .any(|((owner, kind), area)| {
            *owner == beacon.raw() && *kind == TargetKind::Protected.id() && area.contains(x, y)
        })
}

/// The first Build target of `beacon` that has a structure and is not finished.
///
/// What a build drone walks to: either something still going up, or something
/// of the seat's that has taken damage inside the beacon's sphere. The second
/// half is `repair_threshold_pct`'s, which is S2's with damage — at the
/// skeleton only construction can leave a structure short of full hit points.
///
/// A target inside one of the beacon's protected areas is passed over: a
/// protected area written after its structure was paid for keeps the ground
/// clear of construction, so the drones leave the structure as it stands.
///
/// PLACEHOLDER: a paid structure under a later protected area stays unfinished — owner, S3
///
/// That is: a structure already paid for and still going up when a later edit
/// protects its ground is left as it stands, for as long as the area holds --
/// nothing finishes, refunds or ruins it, and the spec does not say which
/// (S1's `build` lane, from its review).
#[must_use]
pub(crate) fn next_unfinished_target(world: &World, beacon: BeaconId) -> Option<u32> {
    let targets = world.targets();
    let structures = world.structures();
    let rows = targets
        .beacons()
        .iter()
        .zip(targets.kinds())
        .zip(targets.anchors().iter().zip(targets.built()));
    for ((owner, kind), (at, built)) in rows {
        if *owner != beacon.raw()
            || *kind != TargetKind::Build.id()
            || *built == BeaconId::NONE.raw()
            || protected(world, beacon, *at)
        {
            continue;
        }
        // The link is a structure row the Quartermaster wrote when it paid;
        // one that names no row is not a structure going up.
        let Ok(index) = usize::try_from(*built) else {
            continue;
        };
        let unfinished = structures.building().get(index).copied() == Some(true);
        let alive = structures
            .hit_points()
            .get(index)
            .is_some_and(|hp| hp.is_alive());
        if unfinished && alive {
            return Some(*built);
        }
    }
    None
}

/// How many Build targets of `beacon` are outstanding: unpaid, or paid for and
/// still going up.
fn building_work(world: &World, beacon: BeaconId) -> u32 {
    let targets = world.targets();
    let rows = targets
        .beacons()
        .iter()
        .zip(targets.kinds())
        .zip(targets.anchors().iter().zip(targets.built()));
    let mut work: u32 = 0;
    for (row, ((owner, kind), (at, built))) in rows.enumerate() {
        if *owner != beacon.raw() || *kind != TargetKind::Build.id() {
            continue;
        }
        let outstanding = if *built == BeaconId::NONE.raw() {
            buildable(world, beacon, row)
        } else {
            !protected(world, beacon, *at)
                && usize::try_from(*built).is_ok_and(|index| {
                    world.structures().building().get(index).copied() == Some(true)
                })
        };
        if outstanding {
            work = work.saturating_add(1);
        }
    }
    work
}

/// Whether `at` lies inside `beacon`'s sphere of authority.
#[must_use]
pub(crate) fn inside_sphere(world: &World, beacon: BeaconId, at: [Fx; 3]) -> bool {
    let Ok(row) = usize::try_from(beacon.raw()) else {
        return false;
    };
    let Some(centre) = world.beacons().positions().get(row).copied() else {
        return false;
    };
    let radius = world.sphere_radius();
    Sq::between(at, centre) <= Sq::of_radius(radius)
}
