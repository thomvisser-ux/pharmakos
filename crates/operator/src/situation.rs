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
//! Three things are read **and never used** on purpose:
//!
//! * the briefing's notebook -- "It ignores the notebook" (spec section 14);
//! * the phase timer in `get_status`'s `status` and in every `_status`
//!   footer, which the host clock moves ([`crate::wire`]);
//! * a unit's or a structure's `id`, which is a handle minted per viewer in
//!   the order it first saw the thing (decisions-log item 107 (5)). The
//!   operator keys nothing on it; only a beacon's `b_NN`, which is public and
//!   is what a playbook names, is ever used as a key.

use pharmakos_proto::json::Json;

use crate::easy::{MAP_COLUMNS_MAX, PAGES_MAX};
use crate::terrain::Terrain;
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
}

/// The seat's own economy as the world stands (`get_economy_forecast`'s
/// present-state fields).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Economy {
    /// Whole $.
    pub(crate) treasury: i64,
    /// Supply minus draw, whole kW.
    pub(crate) headroom_kw: i64,
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
    /// Where the seat's commander stands, if the view shows it.
    pub(crate) commander: Option<[i32; 3]>,
    /// Where each thing of another seat that the view shows stands, sorted.
    pub(crate) enemies: Vec<[i32; 3]>,
    /// Where the seat's own Generators stand, sorted.
    pub(crate) own_generators: Vec<[i32; 3]>,
    /// The ground.
    pub(crate) terrain: Terrain,
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

    /// The seat's core, if it still has one.
    pub(crate) fn core(&self) -> Option<&Beacon> {
        self.own_beacons().find(|beacon| beacon.core)
    }

    /// Make the fixed reads.
    ///
    /// # Errors
    ///
    /// The first refusal: a round the operator cannot read is a round it does
    /// not plan.
    pub(crate) fn read(wire: &mut Wire<'_, '_>, seat: u8) -> Result<Situation, Refused> {
        let own = format!("seat.{seat}");

        let status = wire.call("get_status", object(vec![]))?;
        let status = status.get("status").cloned().unwrap_or(Json::Null);
        // The round and the segment's length. The status also carries the
        // phase timer, which the host clock moves: never read.
        let round = u32::try_from(int_of(&status, "round")).unwrap_or(0);

        // The notebook at the top of the briefing is never read (spec section
        // 14: the operator ignores it). The segment length is.
        let briefing = wire.call("get_briefing", object(vec![]))?;
        let segment_ms = match int_of(&briefing, "segment_length_ms") {
            0 => int_of(&status, "segment_length_ms"),
            found => found,
        };

        let mut beacons: Vec<Beacon> = Vec::new();
        let mut cursor = String::new();
        for _ in 0..PAGES_MAX {
            let page = wire.call("list_beacons", object(vec![("cursor", string(&cursor))]))?;
            for row in array_of(&page, "beacons") {
                let Some(at) = row.get("at").and_then(voxel) else {
                    continue;
                };
                let mine = text_of(row, "owner") == own;
                beacons.push(Beacon {
                    id: text_of(row, "beacon_id").to_owned(),
                    at,
                    own: mine,
                    core: mine && bool_of(row, "core"),
                    powered: mine && bool_of(row, "powered"),
                });
            }
            text_of(&page, "next_cursor").clone_into(&mut cursor);
            if cursor.is_empty() {
                break;
            }
        }
        beacons.sort_by(|a, b| a.id.cmp(&b.id));
        beacons.dedup_by(|a, b| a.id == b.id);

        let forecast = wire.call("get_economy_forecast", object(vec![]))?;
        let economy = Economy {
            treasury: int_of(&forecast, "treasury_now"),
            headroom_kw: int_of(&forecast, "headroom_kw_now"),
        };

        let map = wire.call("get_map_summary", object(vec![]))?;
        let size = map.get("size").and_then(voxel).unwrap_or([0, 0, 0]);
        let match_seed = text_of(&map, "match_seed").to_owned();
        // A size the operator cannot hold is a round it cannot read, said
        // before anything is allocated for it.
        let terrain = Terrain::new(size[0], size[1]).ok_or_else(|| Refused {
            method: String::from("get_map_summary"),
            code: String::from("MALFORMED"),
            message: format!(
                "a map of {} x {} columns is more than the operator holds ({MAP_COLUMNS_MAX})",
                size[0], size[1]
            ),
        })?;

        let seen = read_view(wire, &own, terrain)?;
        let templates = read_templates(wire)?;

        Ok(Situation {
            seat,
            round,
            segment_ms,
            match_seed,
            beacons,
            economy,
            commander: seen.commander,
            enemies: seen.enemies,
            own_generators: seen.own_generators,
            terrain: seen.terrain,
            templates,
            view_complete: seen.complete,
        })
    }
}

/// What `get_view` shows.
struct Seen {
    terrain: Terrain,
    complete: bool,
    commander: Option<[i32; 3]>,
    enemies: Vec<[i32; 3]>,
    own_generators: Vec<[i32; 3]>,
}

/// Read the view to its complete page: the ground from every chunk, and the
/// entity list from the page that carries it.
fn read_view(wire: &mut Wire<'_, '_>, own: &str, terrain: Terrain) -> Result<Seen, Refused> {
    let mut seen = Seen {
        terrain,
        complete: false,
        commander: None,
        enemies: Vec::new(),
        own_generators: Vec::new(),
    };
    let mut cursor = String::new();
    for _ in 0..PAGES_MAX {
        let page = wire.call("get_view", object(vec![("cursor", string(&cursor))]))?;
        for chunk in array_of(&page, "chunks") {
            let origin = chunk.get("origin").and_then(voxel);
            let decoded = pharmakos_proto::json::base64::decode(text_of(chunk, "voxels_rle"))
                .and_then(|bytes| pharmakos_proto::chunk_rle::decode(&bytes).ok());
            if let (Some(origin), Some(voxels)) = (origin, decoded) {
                seen.terrain.add_chunk(origin, &voxels);
            }
        }
        if bool_of(&page, "complete") {
            // The complete page is the one that carries the entity list. Read
            // by what each thing is and where it stands, and never by its id.
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
                    } else if kind.as_deref() == Some("STRUCTURE") && subtype == "generator" {
                        seen.own_generators.push(at);
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
            // cursor would fold the first page's chunks in twice.
            break;
        }
    }
    seen.enemies.sort_unstable();
    seen.own_generators.sort_unstable();
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
