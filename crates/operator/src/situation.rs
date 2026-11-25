// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the operator knows at the start of a round: the fixed reads.
//!
//! Spec section 14's first step, "read the briefing, beacons, known enemies,
//! economy and capabilities", over the methods an ordinary client has
//! (decisions-log item 111, section A3): `get_status`, `get_briefing`,
//! `list_beacons`, `get_economy_forecast`, `get_map_summary`, `get_view`,
//! `get_segment_feed` and `list_templates`. Known enemies are what `get_view`
//! shows of another seat's assets, each by what it is, so the threat model
//! can weigh it by its weapon's row (S2's plan, decision 12); the segment
//! feed the Lull still holds says whether the seat's commander died in the
//! last Push (decision 35); and capabilities are the briefing's
//! `licences_held` since S3 (decisions-log item 153 (3)), whose one licence,
//! the Resonance Spire's, is read with the core's sphere radius and the
//! seat's own Spire in the view ([`crate::compose::spire`]).
//!
//! Every member is read **typed** ([`crate::wire`]'s readers): absent is
//! proto3's default, and a member of the wrong shape refuses the round's read
//! as `MALFORMED`, never as a default (decisions-log item 136 (5)).
//!
//! # The features, by name
//!
//! Since S1's targeting (decisions-log item 127 (12); `docs/design/
//! targeting.md`) every vent and seam has a **name**, `vent_<x>_<y>` or
//! `seam_<x>_<y>`, and `get_map_summary.features` lists each one with its
//! grade, whether it is still there, whether the seat's own spheres cover it
//! and the travel to it from the commander, by the estimator "nearest" ranks
//! with. That list is what the operator plans from: it no longer decodes the
//! view's voxels to find a patch of vent or seam, and it derives no id and no
//! site itself. A site comes from `estimate_route`'s `covering` waypoint
//! ([`crate::candidates`]), which answers the column the sim's own `cover`
//! would choose.
//!
//! # Read and never used, on purpose
//!
//! * the briefing's notebook -- "It ignores the notebook" (spec section 14);
//! * the phase timer in `get_status`'s `status` and in every `_status`
//!   footer, which the host clock moves ([`crate::wire`]);
//! * a unit's or a structure's `id`, which is a handle minted per viewer in
//!   the order it first saw the thing (decisions-log item 107 (5)). The
//!   operator keys nothing on it; only a beacon's `b_NN` and a feature's
//!   name, which are what a playbook names, are ever used as keys;
//! * the view's chunks: the ground is the gateway's to read now, through the
//!   feature list and the estimates.

use pharmakos_proto::json::Json;

use crate::easy::PAGES_MAX;
use crate::tuning::Richness;
use crate::wire::{
    COMMANDER_DIED, Misread, Refused, Wire, array_of, bool_of, int_of, location_voxel, malformed,
    object, string, text_of, voxel, whose_commander_died,
};

/// One beacon as `list_beacons` answers it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Beacon {
    /// `b_NN`.
    pub(crate) id: String,
    /// Where it stands.
    pub(crate) at: [i32; 3],
    /// True when it is this seat's.
    pub(crate) own: bool,
    /// The seat that owns it.
    pub(crate) seat: u8,
    /// Own beacons only: the seat's core.
    pub(crate) core: bool,
    /// Own beacons only: false while it is browned out.
    pub(crate) powered: bool,
    /// Own beacons only: its Quartermaster priority, `None` for another
    /// seat's beacon, which carries none on the wire, and for a value that
    /// does not read.
    pub(crate) priority: Option<Priority>,
    /// Another seat's beacon only: game milliseconds since the seat last saw
    /// it (`BeaconSummary.sighting_age_ms`, 0 in live vision); `None` on an
    /// own beacon, which the seat never sights.
    pub(crate) sighting_age_ms: Option<i64>,
    /// Own beacons only: its sphere's radius now, whole voxels, as the sim's
    /// one radius function answers it (`BeaconSummary.sphere_radius_voxels`,
    /// S3's plan, E3): the rules table's base, grown while a finished
    /// Resonance Spire stands on the beacon. `None` for another seat's
    /// beacon, whose radius is on no wire.
    pub(crate) sphere_radius_voxels: Option<i64>,
}

/// A Quartermaster priority, in the brownout order's sense: the lowest sheds
/// first (spec section 7; `crates/sim/src/power.rs`'s module doc).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Priority {
    /// `LOW`: shed first.
    Low,
    /// `NORMAL`.
    Normal,
    /// `HIGH`: shed last of the knob's three.
    High,
}

impl Priority {
    /// The proto name, as [`Wire::enum_name`] translates it.
    fn of(name: &str) -> Option<Priority> {
        match name {
            "LOW" => Some(Priority::Low),
            "NORMAL" => Some(Priority::Normal),
            "HIGH" => Some(Priority::High),
            _ => None,
        }
    }
}

/// The seat's own economy as the world stands (`get_economy_forecast`'s
/// present-state fields, `gateway.proto`'s fields 1 to 4).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Economy {
    /// Whole $.
    pub(crate) treasury: i64,
    /// Supply, whole kW.
    pub(crate) supply_kw: i64,
    /// Draw, whole kW: what the seat's lit beacons draw.
    pub(crate) draw_kw: i64,
}

/// Vent or seam: what a feature is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Kind {
    /// A heat vent: a Generator stands on it.
    Vent,
    /// A scrap seam: a Mine beacon's drones dig it.
    Seam,
}

impl Kind {
    /// The lower-case name, for the "why" note.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Kind::Vent => "heat vent",
            Kind::Seam => "scrap seam",
        }
    }
}

/// One vent or seam as `get_map_summary.features` answers it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Feature {
    /// Its name, `vent_<x>_<y>` or `seam_<x>_<y>`, as a playbook writes it.
    pub(crate) id: String,
    /// Vent or seam.
    pub(crate) kind: Kind,
    /// Its grade.
    pub(crate) grade: Richness,
    /// The generation anchor column, with no z.
    pub(crate) anchor: [i32; 2],
    /// False once it is lost.
    pub(crate) live: bool,
    /// True when one of the seat's own living beacons' spheres holds it.
    pub(crate) covered: bool,
    /// Travel from the commander, game milliseconds; `None` when no route
    /// reaches it.
    pub(crate) travel_ms: Option<i64>,
}

/// A unit kind, as `gp.api.v1.ViewEntity.subtype` spells it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum UnitKind {
    /// `commander`: no weapon (spec section 4; a self-defence weapon is
    /// roadmap).
    Commander,
    /// `build_drone`.
    BuildDrone,
    /// `mining_drone`.
    MiningDrone,
    /// `repair_drone`.
    RepairDrone,
    /// `raider`.
    Raider,
    /// `scout`.
    Scout,
}

/// A structure kind, as `gp.api.v1.ViewEntity.subtype` spells it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum StructureKind {
    /// `generator`.
    Generator,
    /// `autocannon`.
    Autocannon,
    /// `mortar`.
    Mortar,
    /// `survey_post`.
    SurveyPost,
    /// `resonance_spire`.
    ResonanceSpire,
    /// `wall`.
    Wall,
}

/// What an asset of another seat is, as the view names it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Asset {
    /// A unit.
    Unit(UnitKind),
    /// A structure.
    Structure(StructureKind),
    /// A beacon. Whether it is that seat's core, the one armed beacon, is
    /// that seat's business and on no wire (`gateway.proto`'s
    /// `BeaconSummary.core`), so the threat model reads none in it
    /// ([`crate::tuning::Tuning::threat`]).
    Beacon,
}

impl Asset {
    /// The view entity's kind and subtype, read; `None` for a kind that is no
    /// asset (a shell, a wreck, a ruin), which the threat model does not
    /// count.
    ///
    /// # Errors
    ///
    /// `MALFORMED` for an asset kind with a subtype the operator does not
    /// read: the gateway's catalogue is closed (`crates/gateway/src/view.rs`'
    /// `unit_subtype` and `structure_subtype`), so an unknown one is an answer
    /// the operator cannot plan around, and it says so rather than reading the
    /// thing as unarmed.
    pub(crate) fn of(kind: Option<&str>, subtype: &str) -> Result<Option<Asset>, Refused> {
        let unknown = || {
            malformed(
                "get_view",
                &format!("an entity whose subtype `{subtype}` the operator does not read"),
            )
        };
        Ok(Some(match kind {
            Some("UNIT") => Asset::Unit(match subtype {
                "commander" => UnitKind::Commander,
                "build_drone" => UnitKind::BuildDrone,
                "mining_drone" => UnitKind::MiningDrone,
                "repair_drone" => UnitKind::RepairDrone,
                "raider" => UnitKind::Raider,
                "scout" => UnitKind::Scout,
                _ => return Err(unknown()),
            }),
            Some("STRUCTURE") => Asset::Structure(match subtype {
                "generator" => StructureKind::Generator,
                "autocannon" => StructureKind::Autocannon,
                "mortar" => StructureKind::Mortar,
                "survey_post" => StructureKind::SurveyPost,
                "resonance_spire" => StructureKind::ResonanceSpire,
                "wall" => StructureKind::Wall,
                _ => return Err(unknown()),
            }),
            Some("BEACON") => Asset::Beacon,
            Some("SHELL" | "WRECK" | "RUIN") => return Ok(None),
            _ => {
                return Err(malformed(
                    "get_view",
                    "an entity of a kind the operator does not read",
                ));
            }
        }))
    }
}

/// One asset of another seat that the view shows.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Enemy {
    /// Where it stands.
    pub(crate) at: [i32; 3],
    /// What it is.
    pub(crate) asset: Asset,
    /// The seat that owns it.
    pub(crate) seat: u8,
}

/// The seat a wire owner names: `seat.N` is N.
///
/// # Errors
///
/// `MALFORMED`, from `method`, for an owner that is not spelt so: an owner
/// the operator cannot read is not one it takes for some seat.
pub(crate) fn seat_of(method: &str, owner: &str) -> Result<u8, Refused> {
    let unread = || malformed(method, &format!("an owner `{owner}` that names no seat"));
    let digits = owner.strip_prefix("seat.").ok_or_else(unread)?;
    // Decimal digits only, and no leading zero: the one spelling the gateway
    // writes, so `seat.+1` or `seat.01` is not read as seat 1.
    if digits.is_empty()
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
    {
        return Err(unread());
    }
    digits.parse::<u8>().map_err(|_| unread())
}

/// Whether the seat's commander died in the Push before this Lull: what
/// decision 35 reads "recently" as (S2's plan; decisions-log item 144 (11)).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LastPush {
    /// No `commander_died` line names this seat's commander: the feed was
    /// read to its end, or there was no Push before this Lull.
    Survived,
    /// A `commander_died` line names this seat's commander.
    Died,
    /// Not known, for this reason. Easy then adds no guard for a death, and
    /// its "why" says why it could not tell.
    Unread(PushUnread),
}

/// Why the last Push's lines are not all known to a Lull (fix pass 4, items
/// 2 and 3): one state for a death and for a failed step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PushUnread {
    /// The Lull's feed does not show the last Push at all: a Lull resumed
    /// from a save made before the save carried the last Push (format
    /// version 3, which still loads) opens with a feed that shows none. A
    /// save of this build carries it (lane `opsave`; the owner's approval of
    /// 2026-11-23), and its resumed Lull reads the Push as the live one did.
    Resumed,
    /// The lines were not all reached within [`PAGES_MAX`] pages.
    PastPages,
}

/// The route steps of the seat's own that failed in the last Push, by index
/// from 0, read from its own feed lines (a rule body's steps are numbered in
/// their body and are left out).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum FailedSteps {
    /// Not asked for: an advisor does not read them.
    NotRead,
    /// Read to the end: these, none when none failed or there was no Push.
    Known(Vec<usize>),
    /// Not all known: these were seen, and the rest are not known, for this
    /// reason.
    Unread {
        /// The failed steps that were seen.
        seen: Vec<usize>,
        /// Why the rest are not known.
        why: PushUnread,
    },
}

/// Everything the fixed reads told the operator.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Situation {
    /// The seat the operator plays or advises.
    pub(crate) seat: u8,
    /// The round the Lull opens, 1-based.
    pub(crate) round: u32,
    /// The coming segment's length, game milliseconds.
    pub(crate) segment_ms: i64,
    /// The match seed, as `get_map_summary` spells it: empty for a seat's
    /// token, which is told none from S3 (S3's plan, E18).
    pub(crate) match_seed: String,
    /// The map's extent in voxels on each axis, `get_map_summary.size`: what
    /// a Survey post's one probe area spans ([`crate::compose`]).
    pub(crate) map_size: [i32; 3],
    /// Every beacon the seat is shown, ascending by id.
    pub(crate) beacons: Vec<Beacon>,
    /// The seat's own economy.
    pub(crate) economy: Economy,
    /// Every vent and seam, in the gateway's feature id order.
    pub(crate) features: Vec<Feature>,
    /// What the view shows of who stands where: `None` when `get_view` did not
    /// reach its complete page within [`PAGES_MAX`] pages, so no part of a list
    /// is ever read as the whole (S3's `outcap`, fix pass 1).
    view: Option<Seen>,
    /// Whether the seat's commander died in the last Push.
    pub(crate) last_push: LastPush,
    /// The route steps of the seat's own that failed in the last Push
    /// ([`read_last_push`]).
    pub(crate) failed_steps: FailedSteps,
    /// The template ids the library lists, ascending.
    pub(crate) templates: Vec<String>,
    /// True when the briefing's `licences_held` holds the Resonance Spire's
    /// licence (decisions-log item 153 (3); S3's plan, E2).
    pub(crate) spire_licensed: bool,
}

impl Situation {
    /// The seat's own beacons, ascending by id.
    pub(crate) fn own_beacons(&self) -> impl Iterator<Item = &Beacon> {
        self.beacons.iter().filter(|beacon| beacon.own)
    }

    /// The seat's core, if the seat is shown one.
    pub(crate) fn core(&self) -> Option<&Beacon> {
        self.own_beacons().find(|beacon| beacon.core)
    }

    /// Whether `get_view` reached its complete page within [`PAGES_MAX`]
    /// pages: the view is read whole or not at all.
    pub(crate) const fn view_complete(&self) -> bool {
        self.view.is_some()
    }

    /// Where the seat's commander stands, if a complete view shows it.
    pub(crate) fn commander(&self) -> Option<[i32; 3]> {
        self.view.as_ref().and_then(|view| view.commander)
    }

    /// Every asset of another seat that a complete view shows, sorted; none
    /// when the view did not complete.
    pub(crate) fn enemies(&self) -> &[Enemy] {
        match &self.view {
            Some(view) => view.enemies.as_slice(),
            None => &[],
        }
    }

    /// True when a complete view shows a Resonance Spire of the seat's own,
    /// finished or still going up: a queued Spire stands as a structure from
    /// the moment it is queued (`crates/sim/src/world.rs`' `queue_capability`).
    pub(crate) fn own_spire_seen(&self) -> bool {
        self.view.as_ref().is_some_and(|view| view.own_spire)
    }

    /// Make the fixed reads.
    ///
    /// # Errors
    ///
    /// The first refusal, and `MALFORMED` for an answer whose member is not
    /// the shape the schema gives it, a feature row or an entity that does not
    /// read, or a `commander_died` line that does not say whose: a round the
    /// operator cannot read is a round it does not plan.
    pub(crate) fn read(
        wire: &mut Wire<'_, '_>,
        seat: u8,
        steps: Steps,
    ) -> Result<Situation, Refused> {
        let own = format!("seat.{seat}");

        let answer = wire.call("get_status", object(vec![]))?;
        let status = answer
            .get("status")
            .ok_or_else(|| malformed("get_status", "the answer carries no status"))?;
        let at = |method: &'static str| move |misread: Misread| misread.refused(method);
        // The round and the segment's length. The status also carries the
        // phase timer, which the host clock moves: never read.
        let round = u32::try_from(int_of(status, "round").map_err(at("get_status"))?)
            .map_err(|_| malformed("get_status", "the round is not a whole number of rounds"))?;

        // The notebook at the top of the briefing is never read (spec section
        // 14: the operator ignores it). The segment length is: the briefing's,
        // and the status's when the briefing's is proto3's zero.
        let briefing = wire.call("get_briefing", object(vec![]))?;
        let segment_ms = match int_of(&briefing, "segment_length_ms").map_err(at("get_briefing"))? {
            0 => int_of(status, "segment_length_ms").map_err(at("get_status"))?,
            found => found,
        };
        let spire_licensed = read_licences(&briefing)?;

        let beacons = read_beacons(wire, &own)?;

        let forecast = wire.call("get_economy_forecast", object(vec![]))?;
        let economy = Economy {
            treasury: int_of(&forecast, "treasury_now").map_err(at("get_economy_forecast"))?,
            supply_kw: int_of(&forecast, "supply_kw_now").map_err(at("get_economy_forecast"))?,
            draw_kw: int_of(&forecast, "draw_kw_now").map_err(at("get_economy_forecast"))?,
        };

        let map = wire.call("get_map_summary", object(vec![]))?;
        let match_seed = text_of(&map, "match_seed")
            .map_err(at("get_map_summary"))?
            .to_owned();
        let map_size = map_size_of(&map)?;
        let features = array_of(&map, "features")
            .map_err(at("get_map_summary"))?
            .iter()
            .map(feature_of)
            .collect::<Result<Vec<Feature>, Refused>>()?;

        let seen = read_view(wire, &own)?;
        let (last_push, failed_steps) = read_last_push(wire, seat, round, steps)?;
        let templates = read_templates(wire)?;

        Ok(Situation {
            seat,
            round,
            segment_ms,
            match_seed,
            map_size,
            beacons,
            economy,
            features,
            view: seen,
            last_push,
            failed_steps,
            templates,
            spire_licensed,
        })
    }
}

/// The map's extent, `get_map_summary.size`.
///
/// # Errors
///
/// `MALFORMED` for a size that does not read or has an axis of no voxels: a
/// map with no extent is not one the operator can lay a probe area over.
fn map_size_of(map: &Json) -> Result<[i32; 3], Refused> {
    let size = map
        .get("size")
        .ok_or_else(|| malformed("get_map_summary", "the answer carries no map size"))?;
    let size = voxel(size).map_err(|misread| misread.refused("get_map_summary"))?;
    if size.iter().any(|axis| *axis <= 0) {
        return Err(malformed(
            "get_map_summary",
            "a map size with an axis of no voxels",
        ));
    }
    Ok(size)
}

/// One `gp.api.v1.MapFeature`, read.
///
/// # Errors
///
/// `MALFORMED` when the row has no name, or a kind or a grade that is not
/// one of the wire's values: a feature the operator cannot read is not one it
/// can plan around, and it says so rather than guessing.
fn feature_of(row: &Json) -> Result<Feature, Refused> {
    let misread = |misread: Misread| misread.refused("get_map_summary");
    let id = text_of(row, "feature_id").map_err(misread)?;
    if id.is_empty() {
        return Err(malformed("get_map_summary", "a feature with no name"));
    }
    let kind = match Wire::enum_name("gp.api.v1.MapFeature.Kind", row.get("kind")).as_deref() {
        Some("VENT") => Kind::Vent,
        Some("SEAM") => Kind::Seam,
        _ => {
            return Err(malformed(
                "get_map_summary",
                &format!("feature `{id}` is neither a vent nor a seam"),
            ));
        }
    };
    let grade = Wire::enum_name("gp.v1.ByRichness.Richness", row.get("grade"))
        .as_deref()
        .and_then(Richness::of)
        .ok_or_else(|| {
            malformed(
                "get_map_summary",
                &format!("feature `{id}` has no grade the operator reads"),
            )
        })?;
    let axis = |name: &str| {
        i32::try_from(int_of(row, name).map_err(misread)?).map_err(|_| {
            malformed(
                "get_map_summary",
                &format!("feature `{id}`'s {name} is not a column"),
            )
        })
    };
    // Both members are the gateway's answer in a Lull, the one phase the
    // operator plans in, and `travel_ms` stands only beside `reachable:
    // true` (`gateway.proto`'s `MapFeature`). Outside a Lull both are left
    // out (lane `mapw`): an absent `reachable` is withheld, not unreachable,
    // and an absent figure beside `reachable: true` is no walk of 0 ms, so
    // either is a row refused rather than read as a default.
    if row.get("reachable").is_none() {
        return Err(malformed(
            "get_map_summary",
            &format!("feature `{id}` carries no `reachable`: its travel is withheld"),
        ));
    }
    let reachable = bool_of(row, "reachable").map_err(misread)?;
    if reachable != row.get("travel_ms").is_some() {
        return Err(malformed(
            "get_map_summary",
            &format!(
                "feature `{id}`'s `travel_ms` does not stand exactly beside `reachable: true`"
            ),
        ));
    }
    let travel_ms = int_of(row, "travel_ms").map_err(misread)?;
    Ok(Feature {
        id: id.to_owned(),
        kind,
        grade,
        anchor: [axis("x")?, axis("y")?],
        live: bool_of(row, "live").map_err(misread)?,
        covered: bool_of(row, "covered").map_err(misread)?,
        travel_ms: reachable.then_some(travel_ms),
    })
}

/// Whether the briefing's `licences_held` holds the Resonance Spire's
/// licence. Each value is a `gp.v1.Licence` in the wire's lower case,
/// translated by [`Wire::enum_name`]; the member is left out while the seat
/// holds none, which reads as proto3's empty list.
///
/// # Errors
///
/// `MALFORMED` for a member that is not a list of licence names the schema
/// knows: a licence the operator cannot read is not one it may guess at.
fn read_licences(briefing: &Json) -> Result<bool, Refused> {
    let mut spire = false;
    for held in
        array_of(briefing, "licences_held").map_err(|misread| misread.refused("get_briefing"))?
    {
        let name = Wire::enum_name("gp.v1.Licence", Some(held)).ok_or_else(|| {
            malformed(
                "get_briefing",
                "a held licence that is no licence the schema names",
            )
        })?;
        if name == pharmakos_proto::gp::v1::Licence::ResonanceSpire.as_str_name() {
            spire = true;
        }
    }
    Ok(spire)
}

/// Every beacon `list_beacons` shows, ascending by id, at most
/// [`PAGES_MAX`] pages.
///
/// # Errors
///
/// The refusal, and `MALFORMED` for a row that does not read.
fn read_beacons(wire: &mut Wire<'_, '_>, own: &str) -> Result<Vec<Beacon>, Refused> {
    let misread = |misread: Misread| misread.refused("list_beacons");
    let mut beacons: Vec<Beacon> = Vec::new();
    let mut cursor = String::new();
    for _ in 0..PAGES_MAX {
        let page = wire.call("list_beacons", object(vec![("cursor", string(&cursor))]))?;
        for row in array_of(&page, "beacons").map_err(misread)? {
            let at = row
                .get("at")
                .ok_or_else(|| malformed("list_beacons", "a beacon with no position"))?;
            let at = voxel(at).map_err(misread)?;
            let owner = text_of(row, "owner").map_err(misread)?;
            let mine = owner == own;
            let seat = seat_of("list_beacons", owner)?;
            // Another seat's beacon carries its sighting's age, and one that
            // does not is an answer the operator cannot tell fresh from
            // stale: refused rather than read as seen now.
            let sighting_age_ms = if mine {
                None
            } else {
                if row.get("sighting_age_ms").is_none() {
                    return Err(malformed(
                        "list_beacons",
                        "another seat's beacon with no sighting age",
                    ));
                }
                let age = int_of(row, "sighting_age_ms").map_err(misread)?;
                if age < 0 {
                    return Err(malformed("list_beacons", "a sighting age below zero"));
                }
                Some(age)
            };
            // An own beacon's sphere radius, which the gateway always writes:
            // a radius of zero or less is no sphere, and an answer the
            // operator cannot plan a reach around.
            let sphere_radius_voxels = if mine {
                let radius = int_of(row, "sphere_radius_voxels").map_err(misread)?;
                if radius <= 0 {
                    return Err(malformed(
                        "list_beacons",
                        "an own beacon with no sphere radius",
                    ));
                }
                Some(radius)
            } else {
                None
            };
            let priority = Wire::enum_name(
                "gp.v1.InterfaceRow.QuartermasterPriority",
                row.get("priority"),
            )
            .as_deref()
            .and_then(Priority::of);
            beacons.push(Beacon {
                id: text_of(row, "beacon_id").map_err(misread)?.to_owned(),
                at,
                own: mine,
                seat,
                core: mine && bool_of(row, "core").map_err(misread)?,
                powered: mine && bool_of(row, "powered").map_err(misread)?,
                priority: if mine { priority } else { None },
                sighting_age_ms,
                sphere_radius_voxels,
            });
        }
        text_of(&page, "next_cursor")
            .map_err(misread)?
            .clone_into(&mut cursor);
        if cursor.is_empty() {
            break;
        }
    }
    beacons.sort_by(|a, b| a.id.cmp(&b.id));
    beacons.dedup_by(|a, b| a.id == b.id);
    Ok(beacons)
}

/// What a COMPLETE `get_view` shows of who stands where: a view that did
/// not complete is no `Seen` at all ([`read_view`]), so nothing of a part
/// of a list reaches a reader.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Seen {
    commander: Option<[i32; 3]>,
    enemies: Vec<Enemy>,
    own_spire: bool,
}

/// Read the view to its complete page, taking the entities of every page on
/// the way: a list past one page is paged (S3's `outcap`). Its chunks are
/// not decoded: the ground is the gateway's.
///
/// # Errors
///
/// The refusal, and `MALFORMED` for an entity that does not read.
fn read_view(wire: &mut Wire<'_, '_>, own: &str) -> Result<Option<Seen>, Refused> {
    let misread = |misread: Misread| misread.refused("get_view");
    let mut seen = Seen {
        commander: None,
        enemies: Vec::new(),
        own_spire: false,
    };
    let mut cursor = String::new();
    for _ in 0..PAGES_MAX {
        let page = wire.call("get_view", object(vec![("cursor", string(&cursor))]))?;
        // Read by what each thing is and where it stands, and never by its
        // id, from every page: a list past one page goes on over several,
        // each cut from the one list its listing took (S3's `outcap`).
        for entity in array_of(&page, "entities").map_err(misread)? {
            let at = entity
                .get("at")
                .ok_or_else(|| malformed("get_view", "an entity with no position"))?;
            let at = location_voxel(at).map_err(misread)?;
            let kind = Wire::enum_name("gp.api.v1.ViewEntity.Kind", entity.get("kind"));
            let subtype = text_of(entity, "subtype").map_err(misread)?;
            let owner = text_of(entity, "owner").map_err(misread)?;
            if owner == own {
                if kind.as_deref() == Some("STRUCTURE") && subtype == "resonance_spire" {
                    seen.own_spire = true;
                }
                if kind.as_deref() == Some("UNIT") && subtype == "commander" {
                    // The least position when the view shows two, so the
                    // answer does not hang on the order of the list.
                    seen.commander = Some(match seen.commander {
                        Some(held) => held.min(at),
                        None => at,
                    });
                }
            } else if !owner.is_empty()
                && let Some(asset) = Asset::of(kind.as_deref(), subtype)?
            {
                seen.enemies.push(Enemy {
                    at,
                    asset,
                    seat: seat_of("get_view", owner)?,
                });
            }
        }
        if bool_of(&page, "complete").map_err(misread)? {
            seen.enemies.sort_unstable();
            return Ok(Some(seen));
        }
        text_of(&page, "next_cursor")
            .map_err(misread)?
            .clone_into(&mut cursor);
        if cursor.is_empty() {
            // Not complete and no page after it: asking again from the empty
            // cursor would read the first page twice.
            break;
        }
    }
    // Not complete within `PAGES_MAX` pages: no view, and nothing of the part read.
    Ok(None)
}

/// Whether [`Situation::read`] reads the seat's own failed route steps from
/// the last Push's feed: a built-in seat does, to notice a stone that failed
/// ([`crate::easy`]); an advisor does not, since it composes no stone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Steps {
    /// Read them.
    Read,
    /// Do not ask: [`FailedSteps::NotRead`].
    Skip,
}

/// One seat's own step lines, read in the feed's order: the route step
/// running, and whether a rule's body is (its steps are numbered in its body).
#[derive(Clone, Copy, Default)]
struct Running {
    step: Option<usize>,
    in_rule: bool,
}

/// `Step N started.`, the gateway's own words: the step's index from 0.
///
/// # Errors
///
/// `MALFORMED` for a line that does not read so: a step the operator cannot
/// number is not taken for another one.
fn started_index(text: &str) -> Result<usize, Refused> {
    let unread = || {
        malformed(
            "get_segment_feed",
            "a `step_started` line that names no step",
        )
    };
    let number = text
        .strip_prefix("Step ")
        .and_then(|rest| rest.strip_suffix(" started."))
        .ok_or_else(unread)?;
    number
        .parse::<usize>()
        .map_err(|_| unread())?
        .checked_sub(1)
        .ok_or_else(unread)
}

/// Whether this seat's commander died in the last Push, and which of its own
/// route steps failed in it, from the segment feed the Lull still holds: the
/// Push's own (the gateway starts a new feed when the next Push begins).
///
/// The feed's digests count every kind over the whole segment, so the first
/// page says whether it shows the last Push at all (its `segment_ended`,
/// which every seat is shown), whether any commander died where this seat
/// could see and how many of the seat's own steps failed (a step's lines are
/// told to its seat alone); only then are the lines read, at most
/// [`PAGES_MAX`] pages. A line says whose commander it was in its words
/// alone, since a feed event carries no seat field until its per-kind
/// payload (`gateway.proto`'s `Event`, held to after S4), so the words are
/// read strictly: the gateway's one string table writes "The commander of
/// seat N died." and a line that does not read so is refused, never taken
/// for another seat's. A failed step is the one whose `step_started` line
/// came last outside a rule's body (`rule_fired` to `rule_ended`).
///
/// What is not known is said, never read as "nothing happened": in a Lull
/// after the first whose feed shows no Push (one resumed from a save made
/// before the save carried the last Push) both the death and the failed
/// steps are
/// [`PushUnread::Resumed`]; deaths or failures past the pages read are
/// [`PushUnread::PastPages`], with the failed steps that were seen kept.
///
/// # Errors
///
/// The refusal, and `MALFORMED` for an answer that does not read.
fn read_last_push(
    wire: &mut Wire<'_, '_>,
    seat: u8,
    round: u32,
    steps: Steps,
) -> Result<(LastPush, FailedSteps), Refused> {
    let misread = |misread: Misread| misread.refused("get_segment_feed");
    // The fullest page the detail ladder serves (256 events), so the bound
    // reaches as far into a busy Push as the page count allows.
    let feed = |cursor: &str| object(vec![("cursor", string(cursor)), ("detail", string("full"))]);
    let mut page = wire.call("get_segment_feed", feed(""))?;
    let shown = counted(&page, SEGMENT_ENDED).map_err(misread)? > 0;
    let failed_of = |seen: Vec<usize>, unread: Option<PushUnread>| match (steps, unread) {
        (Steps::Skip, _) => FailedSteps::NotRead,
        (Steps::Read, None) => FailedSteps::Known(seen),
        (Steps::Read, Some(why)) => FailedSteps::Unread { seen, why },
    };
    if !shown {
        // No Push is shown: before the first round's Push there was none,
        // and after it the feed was not kept (a Lull resumed from a save
        // made before the save carried it).
        return Ok(if round > 1 {
            (
                LastPush::Unread(PushUnread::Resumed),
                failed_of(Vec::new(), Some(PushUnread::Resumed)),
            )
        } else {
            (LastPush::Survived, failed_of(Vec::new(), None))
        });
    }
    let deaths = counted(&page, COMMANDER_DIED).map_err(misread)?;
    let failures = match steps {
        Steps::Read => counted(&page, STEP_FAILED).map_err(misread)?,
        Steps::Skip => 0,
    };
    let mut died = false;
    let mut failed: Vec<usize> = Vec::new();
    let (mut deaths_seen, mut failures_seen) = (0_u64, 0_u64);
    let more = |seen: u64, of: u64| seen < of;
    if deaths > 0 || failures > 0 {
        let mut running = Running::default();
        for read in 1..=PAGES_MAX {
            for event in array_of(&page, "events").map_err(misread)? {
                let kind = text_of(event, "kind").map_err(misread)?;
                let text = text_of(event, "text").map_err(misread)?;
                let one = |seen: u64| {
                    seen.checked_add(1).ok_or_else(|| {
                        malformed("get_segment_feed", "a count of lines that does not fit")
                    })
                };
                match kind {
                    COMMANDER_DIED => {
                        if whose_commander_died(text)? == seat {
                            died = true;
                        }
                        deaths_seen = one(deaths_seen)?;
                    }
                    "rule_fired" if steps == Steps::Read => running.in_rule = true,
                    "rule_ended" if steps == Steps::Read => running.in_rule = false,
                    "step_started" if steps == Steps::Read && !running.in_rule => {
                        running.step = Some(started_index(text)?);
                    }
                    STEP_FAILED if steps == Steps::Read => {
                        if !running.in_rule
                            && let Some(index) = running.step
                            && !failed.contains(&index)
                        {
                            failed.push(index);
                        }
                        failures_seen = one(failures_seen)?;
                    }
                    _ => {}
                }
            }
            let deaths_left = !died && more(deaths_seen, deaths);
            if !deaths_left && !more(failures_seen, failures) {
                break;
            }
            let cursor = text_of(&page, "next_cursor").map_err(misread)?.to_owned();
            if cursor.is_empty() || read == PAGES_MAX {
                break;
            }
            page = wire.call("get_segment_feed", feed(&cursor))?;
        }
    }
    let last_push = if died {
        LastPush::Died
    } else if more(deaths_seen, deaths) {
        LastPush::Unread(PushUnread::PastPages)
    } else {
        LastPush::Survived
    };
    failed.sort_unstable();
    let unread = more(failures_seen, failures).then_some(PushUnread::PastPages);
    Ok((last_push, failed_of(failed, unread)))
}

/// The feed's kind for a segment's end, the gateway's `segment_ended`: a
/// line every seat is shown, so a feed that counts none shows no Push.
const SEGMENT_ENDED: &str = "segment_ended";

/// The feed's kind for a step's failure, the gateway's `step_failed`.
const STEP_FAILED: &str = "step_failed";

/// How many lines of `kind` the feed's digests count over the whole segment.
///
/// # Errors
///
/// [`Misread`] for a digest that does not read, or a sum that does not fit.
fn counted(page: &Json, kind: &str) -> Result<u64, Misread> {
    let mut total: u64 = 0;
    for digest in array_of(page, "digests")? {
        for count in array_of(digest, "counts")? {
            if text_of(count, "kind")? == kind {
                let counted = u64::try_from(int_of(count, "count")?).map_err(|_| Misread {
                    key: String::from("count"),
                    wanted: "a count of zero or more",
                })?;
                total = total.checked_add(counted).ok_or_else(|| Misread {
                    key: String::from("count"),
                    wanted: "a count whose sum fits",
                })?;
            }
        }
    }
    Ok(total)
}

/// The template ids the library lists, ascending.
///
/// # Errors
///
/// The refusal, and `MALFORMED` for a listing that does not read.
fn read_templates(wire: &mut Wire<'_, '_>) -> Result<Vec<String>, Refused> {
    let misread = |misread: Misread| misread.refused("list_templates");
    let mut templates: Vec<String> = Vec::new();
    let mut cursor = String::new();
    for _ in 0..PAGES_MAX {
        let page = wire.call("list_templates", object(vec![("cursor", string(&cursor))]))?;
        for row in array_of(&page, "templates").map_err(misread)? {
            templates.push(text_of(row, "template_id").map_err(misread)?.to_owned());
        }
        text_of(&page, "next_cursor")
            .map_err(misread)?
            .clone_into(&mut cursor);
        if cursor.is_empty() {
            break;
        }
    }
    templates.sort_unstable();
    templates.dedup();
    Ok(templates)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Entities from every page** (S3's `outcap`): a list the gateway
    /// pages is read whole, the commander on the first page and an enemy on
    /// the completing one.
    #[test]
    fn the_view_is_read_from_every_page() {
        let page = |entities: &str, cursor: &str, complete: bool| {
            pharmakos_proto::json::read(&format!(
                r#"{{"result":{{"at_ms":0,"chunks":[],"entities":[{entities}],"next_cursor":"{cursor}","complete":{complete}}}}}"#
            ))
            .expect("a page")
        };
        let mut asked: Vec<String> = Vec::new();
        let mut call = |method: &str, params: Json| -> Json {
            assert_eq!(method, "get_view");
            let cursor = match params.get("cursor") {
                Some(Json::String(cursor)) => cursor.clone(),
                _ => String::new(),
            };
            asked.push(cursor.clone());
            match cursor.as_str() {
                "" => page(
                    r#"{"id":"u_1","kind":"unit","subtype":"commander","owner":"seat.0","at":{"x":1,"y":2,"z":3}}"#,
                    "next",
                    false,
                ),
                _ => page(
                    r#"{"id":"u_2","kind":"unit","subtype":"raider","owner":"seat.1","at":{"x":4,"y":5,"z":6}}"#,
                    "done",
                    true,
                ),
            }
        };
        let mut wire = Wire::new(&mut call);
        let seen = read_view(&mut wire, "seat.0")
            .expect("read")
            .expect("a complete view");
        assert_eq!(seen.commander, Some([1, 2, 3]), "from the first page");
        assert_eq!(seen.enemies.len(), 1, "from the completing page");
        assert_eq!(asked, vec![String::new(), String::from("next")]);
    }

    /// **A view that does not complete is no view** (S3's `outcap`, fix pass
    /// 1, item H): pages that never complete within `PAGES_MAX` give no
    /// `Seen` at all, so nothing of the entities read reaches a reader.
    #[test]
    fn a_view_that_never_completes_is_none() {
        let mut call = |_: &str, _: Json| -> Json {
            pharmakos_proto::json::read(
                r#"{"result":{"at_ms":0,"chunks":[],"entities":[{"id":"u_1","kind":"unit","subtype":"commander","owner":"seat.0","at":{"x":1,"y":2,"z":3}}],"next_cursor":"more","complete":false}}"#,
            )
            .expect("a page")
        };
        let mut wire = Wire::new(&mut call);
        assert!(
            read_view(&mut wire, "seat.0").expect("read").is_none(),
            "a part is not a view"
        );
        assert_eq!(wire.calls(), crate::easy::PAGES_MAX);
    }

    #[test]
    fn an_owner_is_read_in_the_gateways_one_spelling_only() {
        assert_eq!(seat_of("get_view", "seat.0"), Ok(0));
        assert_eq!(seat_of("get_view", "seat.12"), Ok(12));
        for owner in [
            "seat.x", "seat.+1", "seat.01", "seat.300", "seat.", "seat 1", "",
        ] {
            assert_eq!(
                seat_of("list_beacons", owner).map_err(|refused| refused.code),
                Err(String::from("MALFORMED")),
                "{owner}"
            );
        }
    }

    #[test]
    fn a_death_line_names_its_seat_and_any_other_line_is_refused() {
        assert_eq!(
            whose_commander_died("The commander of seat 1 died. That is death 2 in this Push."),
            Ok(1)
        );
        for line in [
            "The commander of seat one died.",
            "The commander of seat 1 came back after 3 ticks.",
            "A commander died.",
            "The commander of seat 300 died.",
        ] {
            assert_eq!(
                whose_commander_died(line).map_err(|refused| refused.code),
                Err(String::from("MALFORMED")),
                "{line}"
            );
        }
    }
}
