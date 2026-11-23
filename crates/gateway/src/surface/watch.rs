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
//! score, a power state, a count, and another seat's build progress or sphere.
//! `no_view_ever_carries_a_...` asserts it as a key-set allow-list over the
//! rendered JSON, for every viewer and under every fog policy, rather than as
//! a promise in a comment. What a listed entity does carry beyond its kind and
//! place is S2's plan's decision 5 (decisions-log item 144 (11)): in live
//! vision, its hit points with their maximum and a unit's heading, for the
//! viewer's own assets and another seat's alike; from S3's `probw`, its name
//! and a structure's look for every asset, and a beacon's sphere and a
//! structure going up for the viewer's own (every asset, for a no-fog
//! spectator; the owner's "Own seat only", decisions-log item 168 (11)). Walls,
//! which are voxels, are listed as structures ([`visible_walls`]).

use std::collections::BTreeSet;

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::{Json, base64};
use pharmakos_proto::said;
use pharmakos_sim::sight::SensorKind;
use pharmakos_sim::tables::{BeaconId, NOT_RUINED, SeatId, StructureKind, UnitKind};
use pharmakos_sim::world::World;

use crate::error::Error;
use crate::feed::ViewCursor;
use crate::fog::{Audience, FogFilter, FogPolicy, Viewer, Vision};
use crate::rpc::Request;
use crate::strings;
use crate::surface::Surface;
use crate::view;
use crate::viewfeed::{ShellKey, Sight, ViewFeed, ViewPage, WallKey};

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
    /// PLACEHOLDER: the cost of recomputing every modified chunk whenever a sensor moves — owner, at S7, measured in S3 by `cert3` and built at S7 as decision 14 rules (item 152 (1) 11; decision 14 ruled by item 154 (4)).
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
///
/// From S3's `probw`, a structure's look on every structure and wall, and two
/// facts told only where [`private_to`] holds -- the viewer's own asset, or
/// any asset for a spectator with `spectate.nofog` -- a beacon's sphere and a
/// structure going up (S2's plan's decision 5; the owner's "Own seat only",
/// decisions-log item 168 (11)).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Condition {
    /// Hit points now and at full condition, whole HP, or `None` for a thing
    /// that shows none.
    hp: Option<(u32, u32)>,
    /// A unit's heading in the sim's own units, 65 536 to the turn.
    heading: Option<u32>,
    /// True on a structure going up, where the viewer may be told it.
    building: bool,
    /// How a structure or a wall is drawn.
    look: Option<Look>,
}

/// One `gp.api.v1.ViewEntity.Look`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Look {
    shape: view::Shape,
    /// Whole voxels, `[x, y, z]`.
    size: [u32; 3],
}

/// Whether this viewer is told an asset's private facts -- a beacon's sphere,
/// a structure going up -- which S2's plan's decision 5 shows only on a seat's
/// own assets (the owner's "Own seat only", decisions-log item 168 (11)): its
/// own, for a seat, whatever the match's fog policy (the policy decides which
/// entities are listed, not what each carries); every asset, for a spectator
/// with `spectate.nofog`; nothing, for anybody else.
const fn private_to(viewer: Viewer, owner: Option<u8>) -> bool {
    match (viewer, owner) {
        (Viewer::Seat(seat), Some(owner)) => seat.raw() == owner,
        (Viewer::Seat(_), None) | (Viewer::Admin, _) => false,
        (Viewer::Spectator { nofog }, _) => nofog,
    }
}

/// The view's names for structure kinds: plan-core's blueprint titles, the
/// words the vocabulary shows a player, read once per listing.
struct Titles {
    blueprints: Vec<pharmakos_plan_core::vocabulary::Blueprint>,
}

impl Titles {
    fn new() -> Titles {
        Titles {
            blueprints: pharmakos_plan_core::vocabulary::blueprints_by_slot(),
        }
    }

    /// `kind`'s name in the player's words.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for a kind plan-core's list does not
    /// hold, which is the two crates disagreeing about the sim's kinds.
    fn structure(&self, kind: StructureKind) -> Result<&'static str, Error> {
        self.blueprints
            .iter()
            .find(|blueprint| blueprint.kind == kind)
            .map(|blueprint| blueprint.title)
            .ok_or_else(|| {
                Error::internal(said!(
                    "plan-core lists no blueprint for structure kind {}",
                    kind.id()
                ))
            })
    }
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
    let titles = Titles::new();
    let mut out: Vec<Json> = Vec::new();
    visible_beacons(views, world, &filter, viewer, &mut out)?;
    visible_units(views, world, &filter, viewer, &mut out)?;
    visible_structures(views, world, &filter, viewer, &titles, &mut out)?;
    visible_walls(views, world, &filter, viewer, &titles, &mut out)?;
    visible_wrecks(views, world, &filter, viewer, &mut out)?;
    visible_shells(views, world, &filter, viewer, &mut out)?;
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
        // The sim's one radius, as `list_beacons` reads it, on the viewer's own
        // beacons only (S3's `probw`, item 1).
        let sphere_radius = if private_to(viewer, Some(owner)) {
            let radius = world.sphere_radius_voxels_of(BeaconId::new(id));
            Some(u32::try_from(radius).map_err(|_| {
                Error::internal(said!("a beacon's sphere radius is {} voxels", radius))
            })?)
        } else {
            None
        };
        let condition = Condition {
            hp: Some((view::hp_wire(hp)?, view::hp_wire(max)?)),
            ..Condition::default()
        };
        // The viewer's own name for it: a seat's own `b_NN`, anybody else's
        // `e_NN` (decisions-log item 127 (13)).
        let name = views.beacon_name(viewer, world, row)?;
        let mut listed = entity(
            &name,
            "BEACON",
            ("", strings::VIEW_BEACON),
            Some(owner),
            at,
            condition,
        );
        // Written here, the one reader of the sphere in this module, so the
        // sim's census of sphere readers (`crates/sim/tests/spire.rs`) names
        // one function of the view: before the title, in field order.
        if let (Some(radius), Json::Object(entries)) = (sphere_radius, &mut listed) {
            let before_title = entries
                .len()
                .checked_sub(1)
                .ok_or_else(|| Error::internal("a view entity was written with no title"))?;
            entries.insert(
                before_title,
                (
                    String::from("sphere_radius_voxels"),
                    Json::Number(radius.to_string()),
                ),
            );
        }
        out.push(listed);
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
            Error::internal(said!(
                "a unit row holds kind {}, which this build does not name",
                raw_kind
            ))
        })?;
        let heading = column(units.headings().get(row), "unit", "heading")?;
        let condition = Condition {
            hp: Some((
                view::hp_wire(hp)?,
                view::hp_wire(world.rules().unit(kind).hp)?,
            )),
            heading: Some(u32::from(heading.raw())),
            ..Condition::default()
        };
        let handle = views.unit_handle(viewer, id)?;
        out.push(entity(
            &handle,
            "UNIT",
            (view::unit_subtype(kind), strings::unit_title(kind)),
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
    titles: &Titles,
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
            Error::internal(said!(
                "a structure row holds kind {}, which this build does not name",
                raw_kind
            ))
        })?;
        let hp = match world.structure_max_hp(kind) {
            Some(max) if owner.is_some() => Some((view::hp_wire(hp)?, view::hp_wire(max)?)),
            // A wall has no hit points of its own, and a ruin shows none.
            _ => None,
        };
        // A wall is voxels, never a row of this table (`visible_walls` lists
        // walls), so a row of the wall's kind is the sim and the gateway
        // disagreeing about what a structure row is.
        if kind == StructureKind::Wall {
            return Err(Error::internal(said!(
                "structure row {} is of the wall's kind, and a wall is never a structure row",
                row
            )));
        }
        let handle = views.structure_handle(viewer, id)?;
        // A ruin is listed as one (`ViewEntity.Kind.RUIN`), keeping the
        // subtype of the structure it was and the handle it had. The sim
        // marks a ruin twice -- its tick in `ruined_at`, and a no-seat owner --
        // and the two must agree.
        let ruined =
            column(structures.ruined_at().get(row), "structure", "ruined-at")? != NOT_RUINED;
        if ruined == owner.is_some() {
            return Err(Error::internal(said!(
                "structure row {} is {} by its ruined-at tick and {} by its seat",
                row,
                if ruined { "a ruin" } else { "standing" },
                if owner.is_some() {
                    "a seat's"
                } else {
                    "nobody's"
                }
            )));
        }
        // Going up: the sim's own column, told where the viewer may be told it
        // (S3's `probw`, item 2), and never on a ruin, which is nobody's.
        let building = !ruined
            && private_to(viewer, owner)
            && column(structures.building().get(row), "structure", "building")?;
        let name = titles.structure(kind)?;
        let title = if ruined {
            strings::ruined(name)
        } else if building {
            strings::going_up(name)
        } else {
            String::from(name)
        };
        let condition = Condition {
            hp,
            building,
            look: Some(Look {
                shape: view::structure_shape(kind),
                size: view::STRUCTURE_SIZE,
            }),
            ..Condition::default()
        };
        let listed = if ruined { "RUIN" } else { "STRUCTURE" };
        out.push(entity(
            &handle,
            listed,
            (view::structure_subtype(kind), &title),
            owner,
            at,
            condition,
        ));
    }
    Ok(())
}

/// The walls of [`visible_entities`] (S3's `probw`, item 5, as its review's
/// fix pass 1 redesigned it).
///
/// A wall is voxels, not a row: the material of a wall voxel names the seat
/// that built it. Two readings, by who is asking:
///
/// * **Its own seat, and a no-fog spectator** ([`private_to`]): each wall
///   Build target with a voxel of its seat standing in its box is one entity,
///   at the box's lowest corner at its lowest standing voxel, a `SLAB` its
///   box's size up to its top; and every standing wall voxel of the seat that
///   no listed target's box holds -- a wall whose target was dropped, or whose
///   beacon is gone, still stands -- is grouped as below. Two targets with one
///   box and one top are one wall.
/// * **Any other viewer**: a wall is derived from the wall voxels **this
///   viewer sees** and nothing else -- never the owner's targets, boxes, tops
///   or beacons, which are its Build settings and its build progress (S2's
///   plan's decision 5; the owner's "Own seat only", decisions-log item 168
///   (11)). The seen voxels of each seat are grouped by face contact into
///   walls ([`wall_groups`]), each listed once, as the box around its voxels,
///   its handle carried over, within that seat's material, from the
///   lowest-numbered group this viewer was last shown that shares a voxel
///   with it ([`ViewFeed::wall_group_handles`]: a join keeps the lower
///   handle, a split keeps it on the part holding the least voxel). So
///   nothing in
///   the answer changes when the owner changes its settings and the voxels
///   the viewer sees do not. What the viewer can still infer is what its eyes
///   give it: a wall it sees crumble, grow or split, and how many separate
///   walls it sees.
///
/// Listed as `structure`, subtype `wall`, with no hit points, in seat order,
/// each seat's targets before its groups. Read-only: nothing here writes the
/// world.
///
/// # Errors
///
/// As [`visible_entities`], and [`crate::error::Code::Internal`] for a wall
/// target whose beacon names no live row (a beacon's targets go with it) or
/// whose size does not fit the wire's `uint32`.
fn visible_walls<V: Vision>(
    views: &mut ViewFeed,
    world: &World,
    filter: &FogFilter<'_, V>,
    viewer: Viewer,
    titles: &Titles,
    out: &mut Vec<Json>,
) -> Result<(), Error> {
    let title = titles.structure(StructureKind::Wall)?;
    let standing = views.standing_walls(world)?;
    let mut targets_of: Vec<(u8, Vec<WallTarget>)> = Vec::new();
    let mut groups: Vec<(u8, BTreeSet<[i32; 3]>)> = Vec::new();
    for seat in seats_of(&standing) {
        let theirs = of_seat(&standing, seat).into_iter();
        if private_to(viewer, Some(seat)) {
            let mine: BTreeSet<[i32; 3]> = theirs.collect();
            let (held, orphans) = own_walls(wall_targets(world, seat)?, &mine);
            targets_of.push((seat, held));
            groups.extend(wall_groups(&orphans).into_iter().map(|group| (seat, group)));
        } else {
            let seen: BTreeSet<[i32; 3]> = theirs
                .filter(|[x, y, z]| {
                    filter.visible(
                        viewer,
                        &world_at(
                            Some(seat),
                            Voxel {
                                x: *x,
                                y: *y,
                                z: *z,
                            },
                        ),
                    )
                })
                .collect();
            groups.extend(wall_groups(&seen).into_iter().map(|group| (seat, group)));
        }
    }
    let mut group_handles = views.wall_group_handles(viewer, &groups)?.into_iter();
    let mut pending = groups.into_iter().peekable();
    for seat in seats_of(&standing) {
        if let Some((_, held)) = targets_of.iter().find(|(owner, _)| *owner == seat) {
            for (key, low, high) in held {
                let handle = views.wall_handle(viewer, *key)?;
                out.push(wall_entity(&handle, title, seat, *low, *high)?);
            }
        }
        while let Some((_, group)) = pending.next_if(|(owner, _)| *owner == seat) {
            let handle = group_handles.next().ok_or_else(|| {
                Error::internal("the view feed named fewer walls than it was given")
            })?;
            let (low, high) =
                bounds(&group).ok_or_else(|| Error::internal("a wall group holds no voxel"))?;
            out.push(wall_entity(&handle, title, seat, low, high)?);
        }
    }
    Ok(())
}

/// One wall's `gp.api.v1.ViewEntity`: a `SLAB` from `low` to `high`, both
/// included, standing at `low`.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a span no `uint32` holds.
fn wall_entity(
    handle: &str,
    title: &str,
    seat: u8,
    low: [i32; 3],
    high: [i32; 3],
) -> Result<Json, Error> {
    let span = |low: i32, high: i32| {
        high.checked_sub(low)
            .and_then(|span| span.checked_add(1))
            .and_then(|span| u32::try_from(span).ok())
            .ok_or_else(|| {
                Error::internal(said!(
                    "a wall spans {} to {}, which no size holds",
                    low,
                    high
                ))
            })
    };
    let [x0, y0, z0] = low;
    let [x1, y1, z1] = high;
    let condition = Condition {
        look: Some(Look {
            shape: view::structure_shape(StructureKind::Wall),
            size: [span(x0, x1)?, span(y0, y1)?, span(z0, z1)?],
        }),
        ..Condition::default()
    };
    Ok(entity(
        handle,
        "STRUCTURE",
        (view::structure_subtype(StructureKind::Wall), title),
        Some(seat),
        Voxel {
            x: x0,
            y: y0,
            z: z0,
        },
        condition,
    ))
}

/// The wall voxels of `seat`'s material in `standing`: walls are grouped one
/// seat at a time, so one seat's voxels and another's that touch are never one
/// wall, and a handle never crosses between them.
fn of_seat(standing: &[(u8, [i32; 3])], seat: u8) -> BTreeSet<[i32; 3]> {
    standing
        .iter()
        .filter(|(owner, _)| *owner == seat)
        .map(|(_, at)| *at)
        .collect()
}

/// The seats `standing` holds a voxel of, ascending, each once.
fn seats_of(standing: &[(u8, [i32; 3])]) -> Vec<u8> {
    let mut seats: Vec<u8> = standing.iter().map(|(seat, _)| *seat).collect();
    seats.dedup();
    seats
}

/// One wall target as the view reads it: its key, its lowest corner and its
/// highest corner.
type WallTarget = (WallKey, [i32; 3], [i32; 3]);

/// `seat`'s wall Build targets, in table order: each one's key, its box's
/// lowest corner (z unset, 0) and its highest corner at its top.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a wall target whose beacon names no
/// live row, or a table row short of a column.
fn wall_targets(world: &World, seat: u8) -> Result<Vec<WallTarget>, Error> {
    let targets = world.targets();
    let build = pharmakos_sim::tables::TargetKind::Build.id();
    let mut out = Vec::new();
    for row in 0..targets.row_count() {
        let top = column(targets.wall_tops().get(row), "target", "wall top")?;
        if column(targets.kinds().get(row), "target", "kind")? != build
            || top == pharmakos_sim::tables::NO_WALL
        {
            continue;
        }
        let beacon = column(targets.beacons().get(row), "target", "beacon")?;
        let at_row = world.beacons().row_of(beacon).map_err(|_| {
            Error::internal(said!(
                "wall target row {} belongs to a beacon the table does not hold",
                row
            ))
        })?;
        if column(
            world.beacons().seats().get(at_row.index()),
            "beacon",
            "seat",
        )? != seat
        {
            continue;
        }
        let area = column(targets.areas().get(row), "target", "area")?;
        let [lx, ly] = area.min_column();
        let [hx, hy] = area.max_column();
        out.push((
            WallKey {
                seat,
                min: [lx, ly],
                max: [hx, hy],
                top,
            },
            [i32::from(lx), i32::from(ly), 0],
            [i32::from(hx), i32::from(hy), i32::from(top)],
        ));
    }
    Ok(out)
}

/// The owner's reading of its own walls (or a no-fog spectator's of any
/// seat's): of `targets`, each one with a voxel of `mine` standing in its box,
/// at its lowest standing voxel, two targets with one key listed once -- the
/// key is the seat, the box and the top, not the beacon, so two of the seat's
/// beacons writing one box are one wall and dropping either target leaves its
/// key as it was -- and every voxel of `mine` no listed box holds, to be
/// grouped. Pure: a function of the targets and the voxels.
fn own_walls(
    targets: Vec<WallTarget>,
    mine: &BTreeSet<[i32; 3]>,
) -> (Vec<WallTarget>, BTreeSet<[i32; 3]>) {
    let mut held: Vec<WallTarget> = Vec::new();
    let mut covered: BTreeSet<[i32; 3]> = BTreeSet::new();
    for (key, low, high) in targets {
        let ([lx, ly, _], [hx, hy, top]) = (low, high);
        let inside: Vec<[i32; 3]> = mine
            .iter()
            .filter(|[x, y, z]| (lx..=hx).contains(x) && (ly..=hy).contains(y) && *z <= top)
            .copied()
            .collect();
        covered.extend(inside.iter().copied());
        let Some(lowest) = inside.iter().map(|[_, _, z]| *z).min() else {
            continue;
        };
        if held.iter().any(|(listed, _, _)| *listed == key) {
            continue;
        }
        held.push((key, [lx, ly, lowest], high));
    }
    let orphans = mine.difference(&covered).copied().collect();
    (held, orphans)
}

/// `voxels` grouped into walls by face contact (two voxels sharing a face are
/// one wall), each group in the order of its least voxel, so the grouping is a
/// pure function of the set.
fn wall_groups(voxels: &BTreeSet<[i32; 3]>) -> Vec<BTreeSet<[i32; 3]>> {
    let mut left: BTreeSet<[i32; 3]> = voxels.clone();
    let mut groups: Vec<BTreeSet<[i32; 3]>> = Vec::new();
    while let Some(start) = left.pop_first() {
        let mut group: BTreeSet<[i32; 3]> = BTreeSet::new();
        let mut frontier: Vec<[i32; 3]> = vec![start];
        while let Some(at) = frontier.pop() {
            for next in face_neighbours(at) {
                if left.remove(&next) {
                    frontier.push(next);
                }
            }
            group.insert(at);
        }
        groups.push(group);
    }
    groups
}

/// The six voxels sharing a face with `at` (fewer only past `i32`'s range,
/// where no voxel of a map lies).
fn face_neighbours([x, y, z]: [i32; 3]) -> impl Iterator<Item = [i32; 3]> {
    [
        x.checked_sub(1).map(|x| [x, y, z]),
        x.checked_add(1).map(|x| [x, y, z]),
        y.checked_sub(1).map(|y| [x, y, z]),
        y.checked_add(1).map(|y| [x, y, z]),
        z.checked_sub(1).map(|z| [x, y, z]),
        z.checked_add(1).map(|z| [x, y, z]),
    ]
    .into_iter()
    .flatten()
}

/// The box around `voxels`: its lowest and highest corner, or `None` for an
/// empty set.
fn bounds(voxels: &BTreeSet<[i32; 3]>) -> Option<([i32; 3], [i32; 3])> {
    let mut iter = voxels.iter();
    let first = *iter.next()?;
    Some(
        iter.fold((first, first), |([ax, ay, az], [bx, by, bz]), [x, y, z]| {
            (
                [ax.min(*x), ay.min(*y), az.min(*z)],
                [bx.max(*x), by.max(*y), bz.max(*z)],
            )
        }),
    )
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
        out.push(entity(
            &handle,
            "WRECK",
            ("", strings::VIEW_WRECK),
            None,
            at,
            Condition::default(),
        ));
    }
    Ok(())
}

/// The shells in flight of [`visible_entities`], in the table's firing order:
/// the firer's seat as the owner, no hit points, drawn **where the shell is
/// now** -- the sim's own answer, [`pharmakos_sim::tables::ShellTable::position_at`]
/// at the world's tick, never its aim point and never a point the gateway
/// works out (S2's plan, `wgw`, amended by decisions-log item 148 (3) (h)).
///
/// The fog filter decides who is sent it, as for every other thing in the
/// world: the firing seat always (it is that seat's), a seat or spectator the
/// fog hides nothing from always, and any other seat only while its live
/// vision covers the shell's **current** point. So an enemy sees a shell
/// crossing its sight and is never handed its aim (the main session's ruling,
/// open to the owner's veto). The flight is a straight line at a steady
/// pace, so a seat that sees a shell on two ticks can extrapolate its aim and
/// its launch point: that is the cost of drawing it in flight at all, and it
/// is bounded by the seat's own vision. The landing is a world event,
/// `shell_landed`, which the fog filter tells any seat that sees the place.
///
/// # Errors
///
/// As [`visible_entities`], and [`crate::error::Code::Internal`] for a shell
/// whose firer's seat is the sim's no-seat value (a shell is always some
/// seat's, `ViewEntity.owner`), a held row whose landing tick has come, or a
/// held row the sim gives no position for.
fn visible_shells<V: Vision>(
    views: &mut ViewFeed,
    world: &World,
    filter: &FogFilter<'_, V>,
    viewer: Viewer,
    out: &mut Vec<Json>,
) -> Result<(), Error> {
    let tick = world.tick().raw();
    let shells = world.shells();
    let mut live: Vec<ShellKey> = Vec::new();
    for row in 0..shells.len() {
        let raw_owner = column(shells.seats().get(row), "shell", "seat")?;
        if !SeatId::new(raw_owner).is_some() {
            return Err(Error::internal(said!(
                "shell row {} was fired by the no-seat value, and a shell is always a seat's",
                row
            )));
        }
        let firer = column(shells.firers().get(row), "shell", "firer")?;
        let lands_at = column(shells.lands_at().get(row), "shell", "landing")?;
        let aim = column(shells.aims().get(row), "shell", "aim")?;
        let fired_at = column(shells.fired_at().get(row), "shell", "fire tick")?;
        let key = ShellKey {
            firer,
            fired_at,
            lands_at,
            aim: aim.map(pharmakos_sim::math::fixed::Fx::raw),
        };
        if tick >= lands_at {
            // The sim lands every shell on its landing tick, and a snapshot
            // holds none at or past it, so a held row here is the two
            // disagreeing; drawn, it would sit on its aim point.
            // The words name no row and no landing tick: the check runs
            // before the fog filter, so they may be about a shell this
            // viewer is never shown.
            return Err(Error::internal(said!(
                "a shell is held at tick {} past its landing tick",
                tick
            )));
        }
        live.push(key);
        let now = shells.position_at(row, tick).ok_or_else(|| {
            Error::internal(said!(
                "a shell is held and in flight, and the sim gives it no position at tick {}",
                tick
            ))
        })?;
        let at = view::voxel_of(now);
        if !filter.visible(viewer, &world_at(Some(raw_owner), at)) {
            continue;
        }
        let handle = views.shell_handle(viewer, key)?;
        out.push(entity(
            &handle,
            "SHELL",
            ("", strings::VIEW_SHELL),
            Some(raw_owner),
            at,
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
    value.copied().ok_or_else(|| {
        Error::internal(said!(
            "a {} row has no {} column",
            pharmakos_proto::json::grammar::quoted(table),
            pharmakos_proto::json::grammar::quoted(name)
        ))
    })
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
    (subtype, title): (&str, &str),
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
    if condition.building {
        entries.push((String::from("building"), Json::Bool(true)));
    }
    if let Some(look) = condition.look {
        let [x, y, z] = look.size;
        entries.push((
            String::from("look"),
            Json::Object(vec![
                (
                    String::from("shape"),
                    Json::String(pharmakos_proto::scope::wire_name(
                        view::SHAPE_ENUM,
                        look.shape.proto_name(),
                    )),
                ),
                (String::from("x"), Json::Number(x.to_string())),
                (String::from("y"), Json::Number(y.to_string())),
                (String::from("z"), Json::Number(z.to_string())),
            ]),
        ));
    }
    entries.push((String::from("title"), Json::String(title.to_owned())));
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

#[cfg(test)]
mod tests {
    use super::{Titles, private_to};
    use crate::fog::Viewer;
    use crate::view::{self, Shape};
    use pharmakos_sim::tables::{SeatId, StructureKind, UnitKind};

    /// S3's `probw`, items 3 and 4, as table tests over the sim's own kinds:
    /// every structure kind has a shape, a title and a wire spelling the schema
    /// declares, the autocannon's look differs from the mortar's, a wall is a
    /// slab, and every unit kind has a title of its own.
    #[test]
    fn every_kind_has_a_look_and_a_name() {
        let titles = Titles::new();
        let mut named: Vec<&str> = Vec::new();
        for kind in StructureKind::ALL {
            let title = titles.structure(kind).expect("plan-core names every kind");
            assert!(!title.is_empty(), "{kind:?}");
            named.push(title);
            let shape = view::structure_shape(kind);
            let wire = pharmakos_proto::scope::wire_name(view::SHAPE_ENUM, shape.proto_name());
            assert_eq!(
                pharmakos_proto::scope::from_wire_name(view::SHAPE_ENUM, &wire).as_deref(),
                Some(shape.proto_name()),
                "{kind:?}: the schema declares its shape"
            );
        }
        named.sort_unstable();
        named.dedup();
        assert_eq!(named.len(), StructureKind::ALL.len(), "one name a kind");
        assert_ne!(
            view::structure_shape(StructureKind::Autocannon),
            view::structure_shape(StructureKind::Mortar)
        );
        assert_eq!(view::structure_shape(StructureKind::Wall), Shape::Slab);
        assert_eq!(
            titles.structure(StructureKind::Wall).expect("a wall"),
            "Wall"
        );

        let mut units: Vec<&str> = UnitKind::ALL
            .iter()
            .map(|kind| crate::strings::unit_title(*kind))
            .collect();
        assert!(units.iter().all(|title| !title.is_empty()));
        units.sort_unstable();
        units.dedup();
        assert_eq!(units.len(), UnitKind::ALL.len(), "one name a unit kind");
        assert_eq!(
            crate::strings::going_up("Mortar"),
            "Mortar, going up",
            "the gateway's going-up words"
        );
    }

    /// S3's `probw`, its last pass (item 6): two of the owner's beacons whose
    /// wall targets give one box and one top are one wall, under a key that
    /// does not name a beacon, so dropping either target leaves the wall and
    /// its key exactly as they were; and the owner's own voxels no box holds
    /// are left to be grouped.
    #[test]
    fn two_beacons_writing_one_box_are_one_wall_with_a_stable_key() {
        use super::own_walls;
        use crate::viewfeed::WallKey;
        use std::collections::BTreeSet;
        let key = WallKey {
            seat: 0,
            min: [357, 32],
            max: [360, 32],
            top: 37,
        };
        let target = (key, [357, 32, 0], [360, 32, 37]);
        let mine: BTreeSet<[i32; 3]> = [[357, 32, 36], [358, 32, 36], [358, 32, 37], [400, 9, 30]]
            .into_iter()
            .collect();
        let (both, orphans) = own_walls(vec![target, target], &mine);
        assert_eq!(both, vec![(key, [357, 32, 36], [360, 32, 37])], "one wall");
        assert_eq!(
            orphans,
            [[400, 9, 30]].into_iter().collect(),
            "the rest is grouped"
        );
        let (either, _) = own_walls(vec![target], &mine);
        assert_eq!(
            either, both,
            "either beacon's target dropped: the same wall and key"
        );
        let (none, all) = own_walls(Vec::new(), &mine);
        assert!(none.is_empty());
        assert_eq!(all, mine, "with no target, every voxel is grouped");
    }

    /// S3's `probw`, its last pass (item 3): a viewer's own wall voxels and
    /// another seat's that touch them are two walls, one per seat's material.
    #[test]
    fn touching_walls_of_two_seats_are_two_walls() {
        use super::{of_seat, seats_of, wall_groups};
        let standing = [
            (0, [10, 0, 0]),
            (0, [11, 0, 0]),
            (1, [12, 0, 0]),
            (1, [13, 0, 0]),
        ];
        let groups: Vec<(u8, usize)> = seats_of(&standing)
            .into_iter()
            .flat_map(|seat| {
                wall_groups(&of_seat(&standing, seat))
                    .into_iter()
                    .map(move |group| (seat, group.len()))
            })
            .collect();
        assert_eq!(groups, [(0, 2), (1, 2)]);
    }

    /// Who is told an asset's private facts: its own seat, and a no-fog
    /// spectator; never another seat, a plain spectator or the lobby, and
    /// nobody for a thing nobody owns but the no-fog spectator.
    #[test]
    fn private_facts_go_to_the_owner_and_the_no_fog_spectator_only() {
        let zero = Viewer::Seat(SeatId::new(0));
        assert!(private_to(zero, Some(0)));
        assert!(!private_to(zero, Some(1)));
        assert!(!private_to(zero, None));
        assert!(private_to(Viewer::Spectator { nofog: true }, Some(1)));
        assert!(private_to(Viewer::Spectator { nofog: true }, None));
        assert!(!private_to(Viewer::Spectator { nofog: false }, Some(0)));
        assert!(!private_to(Viewer::Admin, Some(0)));
    }
}
