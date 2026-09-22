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
//! score, a hit point, a power state, a count. `no_view_ever_carries_a_...`
//! asserts it as a key-set allow-list over the rendered JSON, for every viewer
//! and under both fog policies, rather than as a promise in a comment.

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::{Json, base64};
use pharmakos_sim::math::quantity::Hp;
use pharmakos_sim::tables::{BeaconId, SeatId, StructureKind, UnitKind};
use pharmakos_sim::world::World;

use crate::error::Error;
use crate::feed::ViewCursor;
use crate::fog::{Audience, FogFilter, FogPolicy, Viewer, Vision};
use crate::rpc::Request;
use crate::surface::Surface;
use crate::view;
use crate::viewfeed::{Sight, ViewFeed, ViewPage};

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
        let asked = request
            .string_param("cursor")?
            .unwrap_or_default()
            .to_owned();
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

        let cursor = if asked.is_empty() {
            None
        } else {
            Some(ViewCursor::parse(&asked, view_id)?)
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
        views.refresh(viewer, Sight { unfogged, key }, voxels, vision);
        let page = views.page(viewer, cursor, voxels);
        let entities = if page.complete {
            visible_entities(views, world, fog, vision, viewer)
        } else {
            Vec::new()
        };
        Ok(render(at_ms, &page, &entities))
    }

    /// A digest of everything besides the fog policy that decides what this
    /// viewer may see.
    ///
    /// Today that is the centres of its own living beacons, because
    /// [`crate::host::SphereVision`] is the production sight rule and spheres
    /// are all of it. When the viewer's answer to this changes, the feed
    /// recomputes **every** modified chunk for it rather than only the ones
    /// the world wrote -- which is how a beacon placed, a beacon lost and an
    /// elimination all reach the camera without a special case each.
    ///
    /// PLACEHOLDER: when units gain a sight radius (**OWNER**, S1 at the
    /// latest) this key moves every tick, and "recompute everything on a
    /// change" stops being cheap. That is the moment the sim has to report
    /// *what* changed about a seat's sight rather than the gateway deriving
    /// it; the key is where that answer arrives.
    ///
    /// # Errors
    ///
    /// As [`Surface::host`].
    fn sight_key(&self, viewer: Viewer) -> Result<u64, Error> {
        let Viewer::Seat(seat) = viewer else {
            return Ok(0);
        };
        let beacons = self.host()?.world().beacons();
        let mut bytes: Vec<u8> = Vec::new();
        for row in 0..beacons.ids().len() {
            if beacons.seats().get(row).copied() != Some(seat.raw()) {
                continue;
            }
            if !beacons
                .hit_points()
                .get(row)
                .copied()
                .is_some_and(Hp::is_alive)
            {
                continue;
            }
            let Some(centre) = beacons.positions().get(row) else {
                continue;
            };
            for axis in centre {
                bytes.extend_from_slice(&axis.raw().to_le_bytes());
            }
        }
        Ok(pharmakos_sim::digest(&bytes))
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

/// The complete list of entities this viewer may see.
///
/// Row by row through the existing [`FogFilter`], in one fixed order --
/// beacons, then units, then structures, each ascending by table row, which is
/// ascending by id. Built by inclusion: a row the filter refuses is simply not
/// here, and nothing counts what was refused.
///
/// A destroyed asset is not listed: a thing with no hit points left is gone,
/// and listing it would be the one place in this answer where hit points
/// decided something a client could read.
fn visible_entities<V: Vision>(
    views: &mut ViewFeed,
    world: &World,
    fog: &FogPolicy,
    vision: &V,
    viewer: Viewer,
) -> Vec<Json> {
    let filter = FogFilter::new(fog, vision);
    let mut out: Vec<Json> = Vec::new();

    let beacons = world.beacons();
    for row in 0..beacons.ids().len() {
        if !beacons
            .hit_points()
            .get(row)
            .copied()
            .is_some_and(Hp::is_alive)
        {
            continue;
        }
        let owner = beacons.seats().get(row).copied();
        let at = beacons
            .positions()
            .get(row)
            .copied()
            .map(view::voxel_of)
            .unwrap_or_default();
        if !filter.visible(viewer, &world_at(owner, at)) {
            continue;
        }
        let id = beacons.ids().get(row).copied().unwrap_or_default();
        out.push(entity(
            &view::beacon_id(BeaconId::new(id)),
            "BEACON",
            "",
            owner,
            at,
        ));
    }

    let units = world.units();
    for row in 0..units.ids().len() {
        if !units
            .hit_points()
            .get(row)
            .copied()
            .is_some_and(Hp::is_alive)
        {
            continue;
        }
        let owner = units.seats().get(row).copied();
        let at = units
            .positions()
            .get(row)
            .copied()
            .map(view::voxel_of)
            .unwrap_or_default();
        if !filter.visible(viewer, &world_at(owner, at)) {
            continue;
        }
        let id = units.ids().get(row).copied().unwrap_or_default();
        let subtype = UnitKind::from_id(units.kinds().get(row).copied().unwrap_or(0))
            .map_or("", view::unit_subtype);
        let handle = views.unit_handle(viewer, id);
        out.push(entity(&handle, "UNIT", subtype, owner, at));
    }

    let structures = world.structures();
    for row in 0..structures.ids().len() {
        if !structures
            .hit_points()
            .get(row)
            .copied()
            .is_some_and(Hp::is_alive)
        {
            continue;
        }
        let owner = structures.seats().get(row).copied();
        let at = structures
            .positions()
            .get(row)
            .copied()
            .map(view::voxel_of)
            .unwrap_or_default();
        if !filter.visible(viewer, &world_at(owner, at)) {
            continue;
        }
        let id = structures.ids().get(row).copied().unwrap_or_default();
        let subtype = StructureKind::from_id(structures.kinds().get(row).copied().unwrap_or(0))
            .map_or("", view::structure_subtype);
        let handle = views.structure_handle(viewer, id);
        out.push(entity(&handle, "STRUCTURE", subtype, owner, at));
    }

    out
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
/// comment in `gateway.proto` promises. Nothing in v1 makes one; the promise
/// is kept here rather than in a comment so that the first thing that does is
/// not silently attributed to seat 0.
fn entity(id: &str, kind: &str, subtype: &str, owner: Option<u8>, at: Voxel) -> Json {
    Json::Object(vec![
        (String::from("id"), Json::String(id.to_owned())),
        (
            String::from("kind"),
            Json::String(pharmakos_proto::scope::wire_name(KIND_ENUM, kind)),
        ),
        (String::from("subtype"), Json::String(subtype.to_owned())),
        (
            String::from("owner"),
            Json::String(
                owner.map_or_else(String::new, |seat| view::owner_text(SeatId::new(seat))),
            ),
        ),
        (String::from("at"), voxel(at)),
    ])
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
