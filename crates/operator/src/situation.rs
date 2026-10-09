// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the operator knows at the start of a round: the fixed reads.
//!
//! Spec section 14's first step, "read the briefing, beacons, known enemies,
//! economy and capabilities", over the methods an ordinary client has
//! (decisions-log item 111, section A3): `get_status`, `get_briefing`,
//! `list_beacons`, `get_economy_forecast`, `get_map_summary`, `get_view` and
//! `list_templates`. Known enemies are what `get_view` shows of another seat;
//! capabilities have no method until S4, so there is nothing to read for them.
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
    Refused, Wire, array_of, bool_of, int_of, location_voxel, object, string, text_of, voxel,
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
    /// Own beacons only: the seat's core.
    pub(crate) core: bool,
    /// Own beacons only: false while it is browned out.
    pub(crate) powered: bool,
    /// Own beacons only: its Quartermaster priority, `None` for another
    /// seat's beacon, which carries none on the wire, and for a value that
    /// does not read.
    pub(crate) priority: Option<Priority>,
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
    /// An ore seam: a Mine beacon's drones dig it.
    Seam,
}

impl Kind {
    /// The lower-case name, for the "why" note.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Kind::Vent => "heat vent",
            Kind::Seam => "ore seam",
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
    /// Where each thing of another seat that the view shows stands, sorted.
    pub(crate) enemies: Vec<[i32; 3]>,
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
    /// The first refusal, and `MALFORMED` for a feature row that does not
    /// read: a round the operator cannot read is a round it does not plan.
    pub(crate) fn read(wire: &mut Wire<'_, '_>, seat: u8) -> Result<Situation, Refused> {
        let own = format!("seat.{seat}");

        let status = wire.call("get_status", object(vec![]))?;
        let status = status.get("status").cloned().unwrap_or(Json::Null);
        // The round and the segment's length. The status also carries the
        // phase timer, which the host clock moves: never read.
        let round = u32::try_from(int_of(&status, "round"))
            .map_err(|_| malformed("get_status", "the round is not a whole number of rounds"))?;

        // The notebook at the top of the briefing is never read (spec section
        // 14: the operator ignores it). The segment length is.
        let briefing = wire.call("get_briefing", object(vec![]))?;
        let segment_ms = match int_of(&briefing, "segment_length_ms") {
            0 => int_of(&status, "segment_length_ms"),
            found => found,
        };

        let beacons = read_beacons(wire, &own)?;

        let forecast = wire.call("get_economy_forecast", object(vec![]))?;
        let economy = Economy {
            treasury: int_of(&forecast, "treasury_now"),
            supply_kw: int_of(&forecast, "supply_kw_now"),
            draw_kw: int_of(&forecast, "draw_kw_now"),
        };

        let map = wire.call("get_map_summary", object(vec![]))?;
        let match_seed = text_of(&map, "match_seed").to_owned();
        let features = array_of(&map, "features")
            .iter()
            .map(feature_of)
            .collect::<Result<Vec<Feature>, Refused>>()?;

        let seen = read_view(wire, &own)?;
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
            templates,
            view_complete: seen.complete,
        })
    }
}

/// A refusal the operator writes itself: the answer came and did not read.
fn malformed(method: &str, message: &str) -> Refused {
    Refused {
        method: method.to_owned(),
        code: String::from("MALFORMED"),
        message: message.to_owned(),
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
    let id = text_of(row, "feature_id");
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
        i32::try_from(int_of(row, name)).map_err(|_| {
            malformed(
                "get_map_summary",
                &format!("feature `{id}`'s {name} is not a column"),
            )
        })
    };
    let reachable = bool_of(row, "reachable");
    Ok(Feature {
        id: id.to_owned(),
        kind,
        grade,
        anchor: [axis("x")?, axis("y")?],
        live: bool_of(row, "live"),
        covered: bool_of(row, "covered"),
        // `travel_ms` "is then 0 and means nothing" (`gateway.proto`): read as
        // absent, never as a free walk.
        travel_ms: reachable.then(|| int_of(row, "travel_ms")),
    })
}

/// Every beacon `list_beacons` shows, ascending by id, at most
/// [`PAGES_MAX`] pages.
fn read_beacons(wire: &mut Wire<'_, '_>, own: &str) -> Result<Vec<Beacon>, Refused> {
    let mut beacons: Vec<Beacon> = Vec::new();
    let mut cursor = String::new();
    for _ in 0..PAGES_MAX {
        let page = wire.call("list_beacons", object(vec![("cursor", string(&cursor))]))?;
        for row in array_of(&page, "beacons") {
            let Some(at) = row.get("at").and_then(voxel) else {
                continue;
            };
            let mine = text_of(row, "owner") == own;
            let priority = Wire::enum_name(
                "gp.v1.InterfaceRow.QuartermasterPriority",
                row.get("priority"),
            )
            .as_deref()
            .and_then(Priority::of);
            beacons.push(Beacon {
                id: text_of(row, "beacon_id").to_owned(),
                at,
                own: mine,
                core: mine && bool_of(row, "core"),
                powered: mine && bool_of(row, "powered"),
                priority: if mine { priority } else { None },
            });
        }
        text_of(&page, "next_cursor").clone_into(&mut cursor);
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
    enemies: Vec<[i32; 3]>,
}

/// Read the view to its complete page, which is the one that carries the
/// entity list. Its chunks are not decoded: the ground is the gateway's.
fn read_view(wire: &mut Wire<'_, '_>, own: &str) -> Result<Seen, Refused> {
    let mut seen = Seen {
        complete: false,
        commander: None,
        enemies: Vec::new(),
    };
    let mut cursor = String::new();
    for _ in 0..PAGES_MAX {
        let page = wire.call("get_view", object(vec![("cursor", string(&cursor))]))?;
        if bool_of(&page, "complete") {
            // Read by what each thing is and where it stands, and never by its
            // id.
            for entity in array_of(&page, "entities") {
                let Some(at) = entity.get("at").and_then(location_voxel) else {
                    continue;
                };
                let kind = Wire::enum_name("gp.api.v1.ViewEntity.Kind", entity.get("kind"));
                let subtype = text_of(entity, "subtype");
                let owner = text_of(entity, "owner");
                if owner == own {
                    if kind.as_deref() == Some("UNIT") && subtype == "commander" {
                        seen.commander = Some(seen.commander.map_or(at, |held| held.min(at)));
                    }
                } else if !owner.is_empty() {
                    seen.enemies.push(at);
                }
            }
            seen.complete = true;
            break;
        }
        text_of(&page, "next_cursor").clone_into(&mut cursor);
        if cursor.is_empty() {
            // Not complete and no page after it: asking again from the empty
            // cursor would read the first page twice.
            break;
        }
    }
    seen.enemies.sort_unstable();
    Ok(seen)
}

/// The template ids the library lists, ascending.
fn read_templates(wire: &mut Wire<'_, '_>) -> Result<Vec<String>, Refused> {
    let mut templates: Vec<String> = Vec::new();
    let mut cursor = String::new();
    for _ in 0..PAGES_MAX {
        let page = wire.call("list_templates", object(vec![("cursor", string(&cursor))]))?;
        for row in array_of(&page, "templates") {
            templates.push(text_of(row, "template_id").to_owned());
        }
        text_of(&page, "next_cursor").clone_into(&mut cursor);
        if cursor.is_empty() {
            break;
        }
    }
    templates.sort_unstable();
    templates.dedup();
    Ok(templates)
}
