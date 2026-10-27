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
//! last Push (decision 35); capabilities have no method until S4, so there is
//! nothing to read for them.
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
    owner
        .strip_prefix("seat.")
        .ok_or_else(unread)?
        .parse::<u8>()
        .map_err(|_| unread())
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
    /// Some commander died where this seat could see, and the lines naming
    /// whose were not all reached within [`PAGES_MAX`] pages. Easy then adds
    /// no guard, and its "why" says the feed was not read to its end.
    Unread,
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
    /// The match seed, as `get_map_summary` spells it.
    pub(crate) match_seed: String,
    /// Every beacon the seat is shown, ascending by id.
    pub(crate) beacons: Vec<Beacon>,
    /// The seat's own economy.
    pub(crate) economy: Economy,
    /// Every vent and seam, in the gateway's feature id order.
    pub(crate) features: Vec<Feature>,
    /// Where the seat's commander stands, if the view shows it.
    pub(crate) commander: Option<[i32; 3]>,
    /// Every asset of another seat that the view shows, sorted.
    pub(crate) enemies: Vec<Enemy>,
    /// Whether the seat's commander died in the last Push.
    pub(crate) last_push: LastPush,
    /// The template ids the library lists, ascending.
    pub(crate) templates: Vec<String>,
    /// False when `get_view` did not reach its complete page within
    /// [`PAGES_MAX`] pages: the entity list was then never read.
    pub(crate) view_complete: bool,
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

    /// Make the fixed reads.
    ///
    /// # Errors
    ///
    /// The first refusal, and `MALFORMED` for an answer whose member is not
    /// the shape the schema gives it, a feature row or an entity that does not
    /// read, or a `commander_died` line that does not say whose: a round the
    /// operator cannot read is a round it does not plan.
    pub(crate) fn read(wire: &mut Wire<'_, '_>, seat: u8) -> Result<Situation, Refused> {
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
        let features = array_of(&map, "features")
            .map_err(at("get_map_summary"))?
            .iter()
            .map(feature_of)
            .collect::<Result<Vec<Feature>, Refused>>()?;

        let seen = read_view(wire, &own)?;
        let last_push = read_last_push(wire, seat)?;
        let templates = read_templates(wire)?;

        Ok(Situation {
            seat,
            round,
            segment_ms,
            match_seed,
            beacons,
            economy,
            features,
            commander: seen.commander,
            enemies: seen.enemies,
            last_push,
            templates,
            view_complete: seen.complete,
        })
    }
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
    let reachable = bool_of(row, "reachable").map_err(misread)?;
    let travel_ms = int_of(row, "travel_ms").map_err(misread)?;
    Ok(Feature {
        id: id.to_owned(),
        kind,
        grade,
        anchor: [axis("x")?, axis("y")?],
        live: bool_of(row, "live").map_err(misread)?,
        covered: bool_of(row, "covered").map_err(misread)?,
        // `travel_ms` "is then 0 and means nothing" (`gateway.proto`): read as
        // absent, never as a free walk.
        travel_ms: reachable.then_some(travel_ms),
    })
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

/// What `get_view` shows of who stands where.
struct Seen {
    complete: bool,
    commander: Option<[i32; 3]>,
    enemies: Vec<Enemy>,
}

/// Read the view to its complete page, which is the one that carries the
/// entity list. Its chunks are not decoded: the ground is the gateway's.
///
/// # Errors
///
/// The refusal, and `MALFORMED` for an entity that does not read.
fn read_view(wire: &mut Wire<'_, '_>, own: &str) -> Result<Seen, Refused> {
    let misread = |misread: Misread| misread.refused("get_view");
    let mut seen = Seen {
        complete: false,
        commander: None,
        enemies: Vec::new(),
    };
    let mut cursor = String::new();
    for _ in 0..PAGES_MAX {
        let page = wire.call("get_view", object(vec![("cursor", string(&cursor))]))?;
        if bool_of(&page, "complete").map_err(misread)? {
            // Read by what each thing is and where it stands, and never by its
            // id.
            for entity in array_of(&page, "entities").map_err(misread)? {
                let at = entity
                    .get("at")
                    .ok_or_else(|| malformed("get_view", "an entity with no position"))?;
                let at = location_voxel(at).map_err(misread)?;
                let kind = Wire::enum_name("gp.api.v1.ViewEntity.Kind", entity.get("kind"));
                let subtype = text_of(entity, "subtype").map_err(misread)?;
                let owner = text_of(entity, "owner").map_err(misread)?;
                if owner == own {
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
            seen.complete = true;
            break;
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
    seen.enemies.sort_unstable();
    Ok(seen)
}

/// Whether this seat's commander died in the last Push, from the segment
/// feed the Lull still holds: the Push's own (the gateway starts a new feed
/// when the next Push begins).
///
/// The feed's digests count every kind over the whole segment, so the first
/// page says whether any commander died where this seat could see; only then
/// are the lines read, at most [`PAGES_MAX`] pages, for the one that names
/// this seat. A line says whose commander it was in its words alone, since a
/// feed event carries no seat field until its per-kind payload (`gateway.
/// proto`'s `Event`, held to after S4), so the words are read strictly: the
/// gateway's one string table writes "The commander of seat N died." and a
/// line that does not read so is refused, never taken for another seat's.
///
/// A Lull resumed from a save opens with a fresh, empty feed (the save holds
/// no recap events, as the gateway resumes one), so there it reads as
/// [`LastPush::Survived`] and the guard a death would add is not added.
///
/// PLACEHOLDER: a resumed Lull forgets the last Push's death, which no seat-facing field carries — owner, S3, with `get_reports` or the feed's per-kind payload
///
/// # Errors
///
/// The refusal, and `MALFORMED` for an answer that does not read.
fn read_last_push(wire: &mut Wire<'_, '_>, seat: u8) -> Result<LastPush, Refused> {
    let misread = |misread: Misread| misread.refused("get_segment_feed");
    // The fullest page the detail ladder serves (256 events), so the bound
    // reaches as far into a busy Push as the page count allows.
    let feed = |cursor: &str| object(vec![("cursor", string(cursor)), ("detail", string("full"))]);
    let mut page = wire.call("get_segment_feed", feed(""))?;
    let deaths = deaths_counted(&page).map_err(misread)?;
    if deaths == 0 {
        return Ok(LastPush::Survived);
    }
    let mut seen: u64 = 0;
    for read in 1..=PAGES_MAX {
        for event in array_of(&page, "events").map_err(misread)? {
            if text_of(event, "kind").map_err(misread)? != COMMANDER_DIED {
                continue;
            }
            if whose_commander_died(text_of(event, "text").map_err(misread)?)? == seat {
                return Ok(LastPush::Died);
            }
            seen = seen.checked_add(1).ok_or_else(|| {
                malformed("get_segment_feed", "a count of lines that does not fit")
            })?;
        }
        if seen >= deaths {
            return Ok(LastPush::Survived);
        }
        let cursor = text_of(&page, "next_cursor").map_err(misread)?.to_owned();
        if cursor.is_empty() || read == PAGES_MAX {
            break;
        }
        page = wire.call("get_segment_feed", feed(&cursor))?;
    }
    Ok(LastPush::Unread)
}

/// How many `commander_died` lines the feed's digests count over the whole
/// segment.
///
/// # Errors
///
/// [`Misread`] for a digest that does not read, or a sum that does not fit.
fn deaths_counted(page: &Json) -> Result<u64, Misread> {
    let mut deaths: u64 = 0;
    for digest in array_of(page, "digests")? {
        for count in array_of(digest, "counts")? {
            if text_of(count, "kind")? == COMMANDER_DIED {
                let counted = u64::try_from(int_of(count, "count")?).map_err(|_| Misread {
                    key: String::from("count"),
                    wanted: "a count of zero or more",
                })?;
                deaths = deaths.checked_add(counted).ok_or_else(|| Misread {
                    key: String::from("count"),
                    wanted: "a count whose sum fits",
                })?;
            }
        }
    }
    Ok(deaths)
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
