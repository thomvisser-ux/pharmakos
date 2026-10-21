// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `get_view`: the one read a camera needs.
//!
//! The module is `watch.rs` rather than `view.rs` because [`crate::view`]
//! already exists and is a different thing -- it is where the world's types
//! become the wire's -- and two modules whose bare file names collide are two
//! modules a confinement scan cannot tell apart.
//!
//! # What this handler does, in four steps
//!
//! 1. **Decide whether this viewer watches the world at all.** A seat does; a
//!    spectator with `spectate.nofog` does; `admin` and a spectator without
//!    the scope do not, and both get an empty view. That is not a special case
//!    invented here -- it is [`crate::fog`]'s own table said once more:
//!    `admin` "can never read another seat's knowledge", and a spectator
//!    without the scope has no seat and therefore no fogged view to compute.
//! 2. **Refresh what this viewer is entitled to see** of the chunks the world
//!    has written ([`crate::viewfeed`]). This is the one place a [`Vision`]
//!    exists in the view's life.
//! 3. **Page the chunks**, cut on encoded bytes.
//! 4. **List the entities**, on the completing page and on no other, **by
//!    inclusion**: what a viewer may not see is absent, and nothing says how
//!    much was left out.
//!
//! # Never on the wire
//!
//! A destination, a route, a step or a rule index, a mandate, a treasury, a
//! score, a power state, a build's progress, a count. `no_view_ever_carries_a_...`
//! asserts it as a key-set allow-list over the rendered JSON, for every viewer
//! and under both fog policies, rather than as a promise in a comment. What a
//! listed entity does carry beyond its kind and place is S2's plan's decision
//! 5 (decisions-log item 144 (11)): in live vision, its hit points with their
//! maximum and a unit's heading, for the viewer's own assets and another
//! seat's alike.

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::{Json, base64};
use pharmakos_sim::sight::SensorKind;
use pharmakos_sim::tables::{BeaconId, NOT_RUINED, SeatId, StructureKind, UnitKind};
use pharmakos_sim::world::World;

use crate::error::Error;
use crate::feed::ViewCursor;
use crate::fog::{Audience, FogFilter, FogPolicy, Viewer, Vision};
use crate::rpc::Request;
use crate::surface::Surface;
use crate::view;
use crate::viewfeed::{ShellKey, Sight, ViewFeed, ViewPage};

/// The full name of the entity-kind enum, for its lower-case wire spelling
/// (decisions-log item 80).
const KIND_ENUM: &str = "gp.api.v1.ViewEntity.Kind";

impl Surface {
    /// `get_view`: terrain and entities, as this viewer may see them.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when no match is hosted,
    /// [`crate::error::Code::InvalidArgument`] for a cursor this gateway did
    /// not issue, and [`crate::error::Code::StaleSnapshot`] for one issued
    /// against a view this gateway no longer has -- to which the client's
    /// answer is to ask for a keyframe.
    pub(super) fn get_view<V: Vision>(
        &mut self,
        subject: crate::token::Subject,
        held: crate::scopes::ScopeSet,
        request: &Request,
        vision: &V,
    ) -> Result<Json, Error> {
        let viewer = self.viewer_of(subject, held);
        // An absent cursor and an empty one are the same request, a
        // keyframe: proto3 JSON writes no default string at all.
        let asked = request.string_param("cursor")?.map(str::to_owned);
        if !self.views.is_open() {
            return Err(Error::internal(
                "this gateway is not hosting a match, so there is nothing to look at",
            ));
        }
        let at_ms = self.segment_elapsed_ms().raw();
        let view_id = self.views.view_id();

        if !watches_the_world(viewer) {
            // Built by inclusion: an empty view rather than a refusal, because
            // there is nothing wrong with the call -- there is simply nothing
            // in the world this token may be told about.
            let caught_up = ViewCursor {
                view_id,
                from_seq: self.views.seq(),
                to_seq: self.views.seq(),
                index: 0,
            };
            return Ok(render(
                at_ms,
                &ViewPage {
                    chunks: Vec::new(),
                    complete: true,
                    next: caught_up,
                },
                &[],
            ));
        }

        let cursor = match asked.as_deref() {
            None | Some("") => None,
            Some(text) => Some(ViewCursor::parse(text, view_id)?),
        };

        let unfogged = match viewer {
            Viewer::Seat(seat) => self.fog_policy().unfogged(seat),
            Viewer::Spectator { nofog } => nofog,
            Viewer::Admin => false,
        };
        let key = self.sight_key(viewer)?;

        // Three disjoint fields, borrowed field by field: the feed is written,
        // the world and the policy are read, and no accessor of `Surface`
        // could hold all three at once.
        let Surface {
            views, host, fog, ..
        } = self;
        let host = host
            .as_ref()
            .ok_or_else(|| Error::internal("this gateway is not hosting a match"))?;
        let world = host.world();
        let voxels = world.voxels();
        views.refresh(viewer, Sight { unfogged, key }, voxels, vision)?;
        let page = views.page(viewer, cursor, voxels)?;
        let entities = if page.complete {
            visible_entities(views, world, fog, vision, viewer)?
        } else {
            Vec::new()
        };
        Ok(render(at_ms, &page, &entities))
    }

    /// A digest of everything besides the fog policy that decides what this
    /// viewer may see: every one of its own sensors, as the sim's live view
    /// draws them ([`pharmakos_sim::world::World::sight`]) -- each one's kind,
    /// centre and radius, in the sim's walk order.
    ///
    /// When the viewer's answer to this changes, the feed recomputes **every**
    /// modified chunk for it rather than only the ones the world wrote --
    /// which is how a beacon placed, a beacon lost, a unit walking, a
    /// structure finished and an elimination all reach the camera without a
    /// special case each.
    ///
    /// PLACEHOLDER: the cost of recomputing every modified chunk whenever a sensor moves — owner, at S2's G3′-real, with `view`'s per-frame measurement.
    /// Since units see, the key moves on every tick one of the seat's units
    /// walks, so "recompute everything on a change" runs on most calls of a
    /// Push. Its cost is the number of modified chunks times a chunk's 32 768
    /// voxels compared with the generated bytes, the vision asked only where
    /// they differ; the moment that misses the client's frame budget is the
    /// moment the sim reports *what* changed about a seat's sight rather
    /// than the gateway deriving it, and the key is where that answer
    /// arrives.
    ///
    /// # Errors
    ///
    /// As [`Surface::host`].
    fn sight_key(&self, viewer: Viewer) -> Result<u64, Error> {
        let Viewer::Seat(seat) = viewer else {
            return Ok(0);
        };
        let sight = self.host()?.world().sight();
        let mut bytes: Vec<u8> = Vec::new();
        for sensor in sight.sensors().iter().filter(|sensor| sensor.seat == seat) {
            bytes.push(sensor_tag(sensor.kind));
            for axis in sensor.centre {
                bytes.extend_from_slice(&axis.raw().to_le_bytes());
            }
            bytes.extend_from_slice(&sensor.radius.raw().to_le_bytes());
        }
        Ok(pharmakos_sim::digest(&bytes))
    }
}

/// One byte per kind of sensor, so that a sphere and a unit's vision of the
/// same radius at the same place still digest apart.
const fn sensor_tag(kind: SensorKind) -> u8 {
    match kind {
        SensorKind::Sphere => 0,
        SensorKind::Unit(_) => 1,
        SensorKind::Structure(_) => 2,
    }
}

/// True for a viewer the world is shown to at all.
///
/// [`crate::fog`]'s table in one line: a seat sees its own world, a spectator
/// sees it only with `spectate.nofog`, and `admin` sees announcements and
/// nothing else.
const fn watches_the_world(viewer: Viewer) -> bool {
    match viewer {
        Viewer::Seat(_) => true,
        Viewer::Spectator { nofog } => nofog,
        Viewer::Admin => false,
    }
}

/// What one listed entity carries beyond where it is and what it is: S2's
/// plan, decision 5, ruled by decisions-log item 144 (11) (the register's
/// S2-11). Everything the list holds is live vision, so every field is a
/// live one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Condition {
    /// Hit points now and at full condition, whole HP, or `None` for a thing
    /// that shows none.
    hp: Option<(u32, u32)>,
    /// A unit's heading in the sim's own units, 65 536 to the turn.
    heading: Option<u32>,
}

/// The complete list of entities this viewer may see.
///
/// Row by row through the existing [`FogFilter`], in one fixed order --
/// beacons, then units, then structures, then wrecks, each ascending by table
/// row, which is ascending by id, then shells in firing order. The first three
/// are
/// ascending by id. Built by inclusion: a row the filter refuses is simply not
/// here, and nothing counts what was refused.
///
/// A destroyed asset is not listed: a thing with no hit points left is gone.
/// A listed asset carries its kind, its place, its hit points with their
/// maximum, and a unit its heading, whoever owns it, because the list is live
/// vision and decision 5 shows exactly that in live vision -- exact HP on
/// everything that has them, a structure still going up included, as
/// `gateway.proto`'s `ViewEntity.hp` says (the main session's ruling R2 on
/// the reviews of #128). A ruin is nobody's and shows none; a wall has none of
/// its own.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when a table row lacks a column the
/// table's own constructor gives every row, or a hit-point figure does not fit
/// the wire's `uint32`.
fn visible_entities<V: Vision>(
    views: &mut ViewFeed,
    world: &World,
    fog: &FogPolicy,
    vision: &V,
    viewer: Viewer,
) -> Result<Vec<Json>, Error> {
    let filter = FogFilter::new(fog, vision);
    let mut out: Vec<Json> = Vec::new();
    visible_beacons(views, world, &filter, viewer, &mut out)?;
    visible_units(views, world, &filter, viewer, &mut out)?;
    visible_structures(views, world, &filter, viewer, &mut out)?;
    visible_wrecks(views, world, &filter, viewer, &mut out)?;
    visible_shells(views, world, fog, viewer, &mut out)?;
    Ok(out)
}

/// The beacons of [`visible_entities`], in table order.
///
/// # Errors
///
/// As [`visible_entities`].
fn visible_beacons<V: Vision>(
    views: &mut ViewFeed,
    world: &World,
    filter: &FogFilter<'_, V>,
    viewer: Viewer,
    out: &mut Vec<Json>,
) -> Result<(), Error> {
    let beacons = world.beacons();
    for row in 0..beacons.ids().len() {
        let hp = column(beacons.hit_points().get(row), "beacon", "hit-point")?;
        if column(beacons.frees().get(row), "beacon", "free")? || !hp.is_alive() {
            continue;
        }
        let owner = column(beacons.seats().get(row), "beacon", "seat")?;
        let at = view::voxel_of(column(beacons.positions().get(row), "beacon", "position")?);
        if !filter.visible(viewer, &world_at(Some(owner), at)) {
            continue;
        }
        let id = column(beacons.ids().get(row), "beacon", "id")?;
        let max = world.beacon_max_hp(BeaconId::new(id));
        let condition = Condition {
            hp: Some((view::hp_wire(hp)?, view::hp_wire(max)?)),
            heading: None,
        };
        // The viewer's own name for it: a seat's own `b_NN`, anybody else's
        // `e_NN` (decisions-log item 127 (13)).
        let name = views.beacon_name(viewer, world, row)?;
        out.push(entity(&name, "BEACON", "", Some(owner), at, condition));
    }
    Ok(())
}

/// The units of [`visible_entities`], in table order.
///
/// # Errors
///
/// As [`visible_entities`].
fn visible_units<V: Vision>(
    views: &mut ViewFeed,
    world: &World,
    filter: &FogFilter<'_, V>,
    viewer: Viewer,
    out: &mut Vec<Json>,
) -> Result<(), Error> {
    let units = world.units();
    for row in 0..units.ids().len() {
        let hp = column(units.hit_points().get(row), "unit", "hit-point")?;
        if column(units.frees().get(row), "unit", "free")? || !hp.is_alive() {
            continue;
        }
        let owner = column(units.seats().get(row), "unit", "seat")?;
        let at = view::voxel_of(column(units.positions().get(row), "unit", "position")?);
        if !filter.visible(viewer, &world_at(Some(owner), at)) {
            continue;
        }
        let id = column(units.ids().get(row), "unit", "id")?;
        let raw_kind = column(units.kinds().get(row), "unit", "kind")?;
        let kind = UnitKind::from_id(raw_kind).ok_or_else(|| {
            Error::internal(format!(
                "a unit row holds kind {raw_kind}, which this build does not name"
            ))
        })?;
        let heading = column(units.headings().get(row), "unit", "heading")?;
        let condition = Condition {
            hp: Some((
                view::hp_wire(hp)?,
                view::hp_wire(world.rules().unit(kind).hp)?,
            )),
            heading: Some(u32::from(heading.raw())),
        };
        let handle = views.unit_handle(viewer, id)?;
        out.push(entity(
            &handle,
            "UNIT",
            view::unit_subtype(kind),
            Some(owner),
            at,
            condition,
        ));
    }
    Ok(())
}

/// The structures of [`visible_entities`], in table order.
///
/// # Errors
///
/// As [`visible_entities`].
fn visible_structures<V: Vision>(
    views: &mut ViewFeed,
    world: &World,
    filter: &FogFilter<'_, V>,
    viewer: Viewer,
    out: &mut Vec<Json>,
) -> Result<(), Error> {
    let structures = world.structures();
    for row in 0..structures.ids().len() {
        let hp = column(structures.hit_points().get(row), "structure", "hit-point")?;
        if column(structures.frees().get(row), "structure", "free")? || !hp.is_alive() {
            continue;
        }
        let raw_owner = column(structures.seats().get(row), "structure", "seat")?;
        // A ruin is nobody's (its seat is the sim's no-seat value), and it is
        // filtered and drawn as nobody's rather than as a seat's.
        let owner = SeatId::new(raw_owner).is_some().then_some(raw_owner);
        let at = view::voxel_of(column(
            structures.positions().get(row),
            "structure",
            "position",
        )?);
        if !filter.visible(viewer, &world_at(owner, at)) {
            continue;
        }
        let id = column(structures.ids().get(row), "structure", "id")?;
        let raw_kind = column(structures.kinds().get(row), "structure", "kind")?;
        let kind = StructureKind::from_id(raw_kind).ok_or_else(|| {
            Error::internal(format!(
                "a structure row holds kind {raw_kind}, which this build does not name"
            ))
        })?;
        let hp = match world.structure_max_hp(kind) {
            Some(max) if owner.is_some() => Some((view::hp_wire(hp)?, view::hp_wire(max)?)),
            // A wall has no hit points of its own, and a ruin shows none.
            _ => None,
        };
        let condition = Condition { hp, heading: None };
        let handle = views.structure_handle(viewer, id)?;
        // A ruin is listed as one (`ViewEntity.Kind.RUIN`), keeping the
        // subtype of the structure it was and the handle it had. The sim
        // marks a ruin twice -- its tick in `ruined_at`, and a no-seat owner --
        // and the two must agree.
        let ruined =
            column(structures.ruined_at().get(row), "structure", "ruined-at")? != NOT_RUINED;
        if ruined == owner.is_some() {
            return Err(Error::internal(format!(
                "structure row {row} is {} by its ruined-at tick and {} by its seat",
                if ruined { "a ruin" } else { "standing" },
                if owner.is_some() {
                    "a seat's"
                } else {
                    "nobody's"
                },
            )));
        }
        let listed = if ruined { "RUIN" } else { "STRUCTURE" };
        out.push(entity(
            &handle,
            listed,
            view::structure_subtype(kind),
            owner,
            at,
            condition,
        ));
    }
    Ok(())
}

/// The wrecks of [`visible_entities`], in table order: nobody's, with no hit
/// points, shown where the viewer's live vision reaches them. A wreck the
/// viewer only remembers is not listed: `get_view` is live vision only, and
/// the sim's sightings of wrecks are read by no gateway surface yet.
///
/// # Errors
///
/// As [`visible_entities`].
fn visible_wrecks<V: Vision>(
    views: &mut ViewFeed,
    world: &World,
    filter: &FogFilter<'_, V>,
    viewer: Viewer,
    out: &mut Vec<Json>,
) -> Result<(), Error> {
    let wrecks = world.wrecks();
    for row in 0..wrecks.ids().len() {
        if column(wrecks.frees().get(row), "wreck", "free")? {
            continue;
        }
        let at = view::voxel_of(column(wrecks.positions().get(row), "wreck", "position")?);
        if !filter.visible(viewer, &world_at(None, at)) {
            continue;
        }
        let id = column(wrecks.ids().get(row), "wreck", "id")?;
        let handle = views.wreck_handle(viewer, id)?;
        out.push(entity(&handle, "WRECK", "", None, at, Condition::default()));
    }
    Ok(())
}

/// The shells in flight of [`visible_entities`], in the table's firing order:
/// the firer's seat as the owner, no hit points.
///
/// **The sim keeps no position for a shell in flight**: a row holds the point
/// it was aimed at and the tick it lands on (S2's plan, E13; decision 3, no
/// lead), and nothing in between. So a shell is drawn at its aim point, where
/// it lands (the main session's call), and **only its firing seat is sent
/// it**, as it is sent every asset of its own, besides a viewer the fog hides
/// nothing from. Another seat is sent no shell in flight, whatever it sees: a shell listed at its aim point for its whole
/// flight would tell the seat it falls on how long that flight is, and so
/// the range to a mortar it has never seen (#133's fog review, F1). The
/// landing is a world event, `shell_landed`, which the fog filter tells any
/// seat that sees the place.
///
/// PLACEHOLDER: another seat is sent a shell only once the sim gives its in-flight point (`ShellTable::position_at`, lane `site`), filtered there — owner, at S2, wgw's follow-up after `site`.
///
/// # Errors
///
/// As [`visible_entities`], and [`crate::error::Code::Internal`] for a shell
/// whose firer's seat is the sim's no-seat value: a shell is always some
/// seat's (`ViewEntity.owner`).
fn visible_shells(
    views: &mut ViewFeed,
    world: &World,
    fog: &FogPolicy,
    viewer: Viewer,
    out: &mut Vec<Json>,
) -> Result<(), Error> {
    // A viewer the fog hides nothing from -- a seat the policy unfogs (a
    // casual match, its elimination, the match's end) or a spectator holding
    // `spectate.nofog` -- sees every mortar, so a shell tells it nothing it
    // cannot see, and it is sent every shell.
    let sees_all = match viewer {
        Viewer::Seat(seat) => fog.unfogged(seat),
        Viewer::Spectator { nofog } => nofog,
        Viewer::Admin => false,
    };
    let shells = world.shells();
    let mut live: Vec<ShellKey> = Vec::new();
    for row in 0..shells.len() {
        let raw_owner = column(shells.seats().get(row), "shell", "seat")?;
        if !SeatId::new(raw_owner).is_some() {
            return Err(Error::internal(format!(
                "shell row {row} was fired by the no-seat value, and a shell is always a seat's"
            )));
        }
        let firer = column(shells.firers().get(row), "shell", "firer")?;
        let lands_at = column(shells.lands_at().get(row), "shell", "landing")?;
        let aim = column(shells.aims().get(row), "shell", "aim")?;
        let key = ShellKey {
            firer,
            lands_at,
            aim: aim.map(pharmakos_sim::math::fixed::Fx::raw),
        };
        live.push(key);
        if !sees_all && viewer != Viewer::Seat(SeatId::new(raw_owner)) {
            continue;
        }
        let handle = views.shell_handle(viewer, key)?;
        out.push(entity(
            &handle,
            "SHELL",
            "",
            Some(raw_owner),
            view::voxel_of(aim),
            Condition::default(),
        ));
    }
    views.forget_landed_shells(viewer, &live);
    Ok(())
}

/// One column's entry for a row the table holds, or the typed refusal that
/// says which column of which table came up short.
///
/// # Errors
///
/// [`crate::error::Code::Internal`]: the tables give every row every column,
/// so a missing one is the sim and the gateway disagreeing.
fn column<T: Copy>(value: Option<&T>, table: &str, name: &str) -> Result<T, Error> {
    value
        .copied()
        .ok_or_else(|| Error::internal(format!("a {table} row has no {name} column")))
}

/// One row's audience: a thing of `owner`'s, standing at `at`.
///
/// `None` is a thing nobody owns, and it stays `None` all the way into the
/// filter. A missing seat defaulted to zero would be filtered as seat 0's own
/// asset -- visible to that seat wherever it stood -- which is the one kind of
/// mistake a fog filter must not make quietly.
fn world_at(owner: Option<u8>, at: Voxel) -> Audience {
    Audience::World {
        owner: owner.map(SeatId::new),
        at,
    }
}

/// One `gp.api.v1.ViewEntity`.
///
/// `owner` is empty for a thing nobody owns, which is what the field's own
/// comment in `gateway.proto` promises: a ruin, and from `fire` a wreck. An
/// unset optional field is left out, as proto3 JSON writes one, so a reader
/// can tell "no hit points shown" from a zero.
fn entity(
    id: &str,
    kind: &str,
    subtype: &str,
    owner: Option<u8>,
    at: Voxel,
    condition: Condition,
) -> Json {
    let mut entries = vec![
        (String::from("id"), Json::String(id.to_owned())),
        (
            String::from("kind"),
            Json::String(pharmakos_proto::scope::wire_name(KIND_ENUM, kind)),
        ),
        (String::from("subtype"), Json::String(subtype.to_owned())),
        (
            String::from("owner"),
            Json::String(match owner {
                Some(seat) => view::owner_text(SeatId::new(seat)),
                None => String::new(),
            }),
        ),
        (String::from("at"), voxel(at)),
    ];
    if let Some((hp, max_hp)) = condition.hp {
        entries.push((String::from("hp"), Json::Number(hp.to_string())));
        entries.push((String::from("max_hp"), Json::Number(max_hp.to_string())));
    }
    if let Some(heading) = condition.heading {
        entries.push((String::from("heading"), Json::Number(heading.to_string())));
    }
    Json::Object(entries)
}

/// One `gp.v1.Voxel`.
fn voxel(at: Voxel) -> Json {
    Json::Object(vec![
        (String::from("x"), Json::Number(at.x.to_string())),
        (String::from("y"), Json::Number(at.y.to_string())),
        (String::from("z"), Json::Number(at.z.to_string())),
    ])
}

/// One `gp.api.v1.GetViewResponse`.
///
/// `voxels_rle` is a `bytes` field, so it travels as standard base64 -- the
/// proto3 JSON mapping's own rule, through the one base64 this workspace has.
fn render(at_ms: i32, page: &ViewPage, entities: &[Json]) -> Json {
    let chunks: Vec<Json> = page
        .chunks
        .iter()
        .map(|(origin, bytes)| {
            Json::Object(vec![
                (String::from("origin"), voxel(*origin)),
                (
                    String::from("voxels_rle"),
                    Json::String(base64::encode(bytes)),
                ),
            ])
        })
        .collect();
    Json::Object(vec![
        (String::from("at_ms"), Json::Number(at_ms.to_string())),
        (String::from("chunks"), Json::Array(chunks)),
        (String::from("entities"), Json::Array(entities.to_vec())),
        (
            String::from("next_cursor"),
            Json::String(page.next.render()),
        ),
        (String::from("complete"), Json::Bool(page.complete)),
    ])
}
