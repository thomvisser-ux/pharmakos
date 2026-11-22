// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a viewer may see, against a live gateway hosting a real match.
//!
//! The view is the one channel built to be fog-filtered, so the suite is
//! mostly about what does **not** come back. Two properties are worth naming
//! before the tests, because every one of them is a case of one or the other:
//!
//! * **Built by inclusion.** A chunk's bytes start from the generated map --
//!   which every viewer is entitled to whole (decisions-log item 107 (1)) --
//!   and a voxel is replaced by the world's current material only where the
//!   viewer may see it. Nothing is redacted out of a current byte.
//! * **Nothing counts what was left out.** No total, no count, no "hidden"
//!   marker, and no delta whose mere presence says "something happened over
//!   there".
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::{FogPolicy, Viewer, Vision};
use pharmakos_gateway::host::LiveVision;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Token;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;
use pharmakos_sim::sight::SensorKind;
use pharmakos_sim::tables::{SeatId, UnitKind};
use pharmakos_sim::voxels::Material;
use std::collections::BTreeSet;
use std::fs;

use support::{
    FIRST_LULL_MS, MATCH, SEED, SEGMENT_MS, admin_token, array_of, call, call_in_lull, chunks_of,
    code, core_beacon, erase, fell, hosted, hosted_as, hosted_casual, result, seat_token,
    seen_and_unseen_in_one_chunk, spectator_token, sphere_radius, step, target_dir, text_of, view,
    view_pages, workspace_root,
};

// ---------------------------------------------------------------------------
// Reading a view
// ---------------------------------------------------------------------------

/// Every object key of a view response, gathered recursively.
fn keys(value: &Json, into: &mut BTreeSet<String>) {
    match value {
        Json::Object(entries) => {
            for (key, held) in entries {
                into.insert(key.clone());
                keys(held, into);
            }
        }
        Json::Array(items) => {
            for item in items {
                keys(item, into);
            }
        }
        Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => {}
    }
}

/// The material a viewer's own bytes carry at one voxel, or `None` when the
/// chunk it is in never arrived.
fn material_at(pages: &[Json], at: [i32; 3]) -> Option<Material> {
    let origin = (
        at[0].div_euclid(32).saturating_mul(32),
        at[1].div_euclid(32).saturating_mul(32),
        at[2].div_euclid(32).saturating_mul(32),
    );
    let (_, voxels) = chunks_of(pages)
        .into_iter()
        .find(|(held, _)| *held == origin)?;
    let index = usize::try_from(
        at[0].saturating_sub(origin.0)
            + at[1].saturating_sub(origin.1).saturating_mul(32)
            + at[2].saturating_sub(origin.2).saturating_mul(1024),
    )
    .ok()?;
    voxels.get(index).copied().map(Material::from_raw)
}

/// Every entity id a completing page lists.
fn entity_ids(page: &Json) -> Vec<String> {
    array_of(page, "entities")
        .iter()
        .map(|entity| text_of(entity, "id"))
        .collect()
}

/// A two-seat match in its opening Lull, with a Push already started and one
/// tick of it run -- which is the earliest moment a voxel edit can land.
fn pushed() -> Surface {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    assert!(surface.begin_push().expect("a hosted match"), "the Push");
    surface
}

// ---------------------------------------------------------------------------
// The fog rule
// ---------------------------------------------------------------------------

/// An edit one voxel outside the seat's spheres, inside a chunk it half sees,
/// produces **no chunk at all** -- not an empty one and not a smaller one.
#[test]
fn an_edit_a_seat_cannot_see_never_reaches_it_as_a_delta() {
    let mut surface = pushed();
    let token = seat_token(&mut surface, 0);
    let (_, unseen) = seen_and_unseen_in_one_chunk(&surface, 0);

    // A quiet tick first, so the delta this test looks at can be compared with
    // one that had nothing in it.
    step(&mut surface, 1);
    let (_, quiet_cursor) = view(&mut surface, &token, "");
    step(&mut surface, 1);
    let (quiet, after_quiet) = view(&mut surface, &token, &quiet_cursor);
    assert!(array_of(&quiet, "chunks").is_empty(), "a quiet tick");

    erase(&mut surface, unseen);
    step(&mut surface, 1);
    let (page, _) = view(&mut surface, &token, &after_quiet);
    assert!(
        array_of(&page, "chunks").is_empty(),
        "a delta whose presence said `something happened over there` is a fog leak by \
         arithmetic, and the voxel is one outside this seat's own sphere"
    );
    assert!(
        matches!(page.get("complete"), Some(Json::Bool(true))),
        "and it completes exactly as the quiet tick did"
    );

    // The edit really happened: the world's own byte moved.
    let world = surface.host().expect("a match").world();
    assert_eq!(
        world.voxels().get(unseen),
        Some(Material::AIR),
        "the seam filed the edit and the tick applied it"
    );
}

/// The same edit, one voxel the other side of the sphere's boundary, arrives
/// with the world's current byte.
#[test]
fn an_edit_a_seat_can_see_arrives_with_its_current_bytes() {
    let mut surface = pushed();
    let token = seat_token(&mut surface, 0);
    let (seen, _) = seen_and_unseen_in_one_chunk(&surface, 0);

    step(&mut surface, 1);
    let (_, cursor) = view(&mut surface, &token, "");
    erase(&mut surface, seen);
    step(&mut surface, 1);

    let pages = view_pages(&mut surface, &token, &cursor);
    let page = pages.last().expect("a page");
    assert_eq!(
        array_of(page, "chunks").len(),
        1,
        "one chunk changed for this viewer, and it is the one it can see into"
    );
    assert_eq!(
        material_at(&pages, seen),
        Some(Material::AIR),
        "the seat watched it happen"
    );
}

/// The live view reads every sensor the sim's one interface walks (S2's plan,
/// E10; S1-34's gateway half), and the boundary is pinned with the answer
/// written out: the core's own sphere is one of the seat's sensors at the
/// beacon's place and `beacon.sphere_radius_voxels`, and every sensor in the
/// vision is the sim's, the commander's own vision among them.
#[test]
fn the_view_reads_every_sensor_the_sim_walks() {
    let surface = hosted(2, SEGMENT_MS, 3);
    let radius = sphere_radius(&surface);
    assert_eq!(radius, 24, "rules.beacon.sphere_radius_voxels, item 90");

    let world = surface.host().expect("a match").world();
    let (_, centre) = core_beacon(&surface, 0);
    let sight = LiveVision::of(world);
    assert_eq!(
        sight.sensors(),
        world.sight().sensors(),
        "the vision holds the sim's own sensors, every one, in the sim's order"
    );
    let own = |kind: SensorKind| {
        sight
            .sensors()
            .iter()
            .any(|sensor| sensor.seat == SeatId::new(0) && sensor.kind == kind)
    };
    assert!(own(SensorKind::Sphere), "the core's sphere");
    assert!(
        own(SensorKind::Unit(UnitKind::Commander)),
        "the commander's own vision"
    );
    let sphere = sight
        .sensors()
        .iter()
        .find(|sensor| sensor.seat == SeatId::new(0) && sensor.kind == SensorKind::Sphere)
        .expect("a sphere");
    assert_eq!(
        sphere.radius.floor_voxels(),
        radius,
        "a sphere's radius is the row's"
    );
    assert_eq!(
        pharmakos_gateway::view::voxel_of(sphere.centre),
        Voxel {
            x: centre[0],
            y: centre[1],
            z: centre[2],
        },
        "and it stands where the core does"
    );

    // The boundary, pinned with the answer written out and checked by an
    // independent squared distance (no square root, AGENTS.md section 4.2):
    // 24 voxels out along an axis is inside the core's sphere and 25 is not.
    let point = |dx: i16| {
        let [x, y, z] = sphere.centre;
        [
            x.checked_add(pharmakos_sim::math::fixed::Fx::from_voxels(dx))
                .expect("on the map"),
            y,
            z,
        ]
    };
    let inside = |dx: i32| dx.saturating_mul(dx) <= radius.saturating_mul(radius);
    assert!(inside(24) && !inside(25), "24² ≤ 24² < 25²");
    assert!(
        sphere.sees(&pharmakos_sim::sight::RadiusOnly, point(24)),
        "the boundary voxel is seen"
    );
    assert!(
        !sphere.sees(&pharmakos_sim::sight::RadiusOnly, point(25)),
        "one voxel past it is not"
    );
}

/// The view draws with the sim's own sensor rule and no copy of it (T17,
/// decisions-log items 107 (6) and 110 (5), and S2's E10): the production
/// vision and `Sight::sees` -- the interpreter's `within`, per sensor --
/// agree on every voxel of a square around the core's sphere boundary, for
/// the seat that owns it and for the seat that does not, and inside the
/// sphere the owner sees.
#[test]
fn the_view_uses_the_sims_own_sensor_rule() {
    let surface = hosted(2, SEGMENT_MS, 3);
    let world = surface.host().expect("a match").world();
    let radius = sphere_radius(&surface);
    let (_, centre) = core_beacon(&surface, 0);
    let sight = LiveVision::of(world);
    let sim = world.sight();

    // Inside the sphere along an axis, the sim and the view both see it.
    let along = |dx: i32| [centre[0].saturating_add(dx), centre[1], centre[2]];
    assert!(sim.sees(SeatId::new(0), along(radius)));
    assert!(world.in_own_sphere(SeatId::new(0), along(radius)));

    let mut compared = 0_u32;
    for dx in -(radius + 2)..=(radius + 2) {
        for dy in [-(radius + 1), -radius, -3, 0, 3, radius, radius + 1] {
            for dz in [-2_i32, 0, 2] {
                let at = [
                    centre[0].saturating_add(dx),
                    centre[1].saturating_add(dy),
                    centre[2].saturating_add(dz),
                ];
                let voxel = Voxel {
                    x: at[0],
                    y: at[1],
                    z: at[2],
                };
                for seat in [SeatId::new(0), SeatId::new(1)] {
                    assert_eq!(
                        sight.sees(seat, &voxel),
                        sim.sees(seat, at),
                        "seat {} at {at:?}",
                        seat.raw()
                    );
                    if world.in_own_sphere(seat, at) {
                        assert!(
                            sight.sees(seat, &voxel),
                            "a seat sees inside its own spheres, seat {} at {at:?}",
                            seat.raw()
                        );
                    }
                    compared = compared.saturating_add(1);
                }
            }
        }
    }
    assert!(compared > 1_000, "a comparison over nothing proves nothing");
}
// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

/// A fogged seat is told about its own assets wherever they are, and about
/// nobody else's it cannot see.
#[test]
fn a_fogged_seat_never_receives_an_unseen_entity() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let (page, _) = view(&mut surface, &token, "");
    let ids = entity_ids(&page);
    let owners: Vec<String> = array_of(&page, "entities")
        .iter()
        .map(|entity| text_of(entity, "owner"))
        .collect();

    // A seat's own core is its `b_00`, and anybody else's beacon would be an
    // `e_NN` minted for it (decisions-log item 127 (13)).
    assert!(ids.contains(&String::from("b_00")), "its own core: {ids:?}");
    assert!(
        !ids.iter().any(|id| id.starts_with("e_")),
        "and not the other seat's, which is a map away: {ids:?}"
    );
    assert!(
        owners.iter().all(|owner| owner == "seat.0"),
        "nothing of anybody else's reached it: {owners:?}"
    );
    assert!(
        !ids.is_empty(),
        "a seat that saw nothing at all would make every assertion above vacuous"
    );
}

/// S2's plan, decision 5 (decisions-log item 144 (11)), on the view: an enemy
/// asset a seat cannot see is absent, and one in its live vision carries its
/// kind, position, exact hit points with their maximum and, for a unit, its
/// heading -- and nothing else. Fogged, seat 1 is a map away and nothing of it
/// is listed; in a casual match it is in sight, and every field is the sim's
/// own figure.
#[test]
fn the_view_shows_what_decision_5_rules() {
    // Fogged: the other seat is a map away, and none of it is in the list.
    let mut fogged = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut fogged, 0);
    let (page, _) = view(&mut fogged, &token, "");
    assert!(
        array_of(&page, "entities")
            .iter()
            .all(|entity| text_of(entity, "owner") == "seat.0"),
        "a fogged enemy is absent: {page:?}"
    );

    // Visible: every enemy asset in sight carries decision 5's fields.
    let mut visible = hosted_casual(2, SEGMENT_MS, 3);
    let token = seat_token(&mut visible, 0);
    let (page, _) = view(&mut visible, &token, "");
    let entities = array_of(&page, "entities");
    let theirs: Vec<&Json> = entities
        .iter()
        .filter(|entity| text_of(entity, "owner") == "seat.1")
        .collect();
    assert!(!theirs.is_empty(), "seat 1 is in sight in a casual match");
    let world = visible.host().expect("a match").world();
    let mut units = 0_u32;
    for entity in theirs {
        let mut found: BTreeSet<String> = BTreeSet::new();
        keys(entity, &mut found);
        let number = |key: &str| match entity.get(key) {
            Some(Json::Number(text)) => text.parse::<i64>().expect("a whole number"),
            other => panic!("{key}: {other:?} on {entity:?}"),
        };
        assert!(number("hp") > 0 && number("hp") <= number("max_hp"));
        match text_of(entity, "kind").as_str() {
            "unit" => {
                units = units.saturating_add(1);
                let expected: BTreeSet<String> = [
                    "id", "kind", "subtype", "owner", "at", "x", "y", "z", "hp", "max_hp",
                    "heading", "title",
                ]
                .iter()
                .map(|key| (*key).to_owned())
                .collect();
                assert_eq!(found, expected, "a unit in sight: {entity:?}");
                assert!((0..65_536).contains(&number("heading")));
            }
            "beacon" => {
                let expected: BTreeSet<String> = [
                    "id", "kind", "subtype", "owner", "at", "x", "y", "z", "hp", "max_hp", "title",
                ]
                .iter()
                .map(|key| (*key).to_owned())
                .collect();
                assert_eq!(
                    found, expected,
                    "a beacon in sight: never its mandate, power or sphere: {entity:?}"
                );
                let core = world
                    .beacons()
                    .ids()
                    .iter()
                    .zip(world.beacons().seats())
                    .find(|(_, seat)| **seat == 1)
                    .map(|(id, _)| *id)
                    .expect("seat 1's core");
                assert_eq!(
                    number("max_hp"),
                    i64::from(
                        world
                            .beacon_max_hp(pharmakos_sim::tables::BeaconId::new(core))
                            .raw()
                    ),
                    "the core's maximum is the sim's own"
                );
            }
            other => panic!("seat 1 holds a `{other}` at the opening: {entity:?}"),
        }
    }
    assert!(units > 0, "seat 1's commander and drones are in sight");
}

/// An id is a handle this viewer was given, in the order it first saw the
/// thing -- never a row of a table, which would be a count of everything the
/// match has ever made.
#[test]
fn an_entity_id_reveals_nothing_about_the_tables_size() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let first = seat_token(&mut surface, 0);
    let second = seat_token(&mut surface, 1);

    let handles = |page: &Json| -> Vec<String> {
        entity_ids(page)
            .into_iter()
            .filter(|id| !id.starts_with("b_"))
            .collect()
    };

    let (page, _) = view(&mut surface, &first, "");
    let own = handles(&page);
    let (page, _) = view(&mut surface, &second, "");
    let other = handles(&page);

    assert!(!own.is_empty(), "each seat has a commander to see");
    assert_eq!(
        own, other,
        "two seats looking at their own one unit both get `u_1`: the numbering is each \
         viewer's own and says nothing about how many units exist"
    );
    for (index, handle) in own.iter().enumerate() {
        assert_eq!(
            *handle,
            format!("u_{}", index.saturating_add(1)),
            "handles count what this viewer has seen, from one"
        );
    }

    // And the world really does hold more units than either seat can name.
    let total = surface.host().expect("a match").world().units().ids().len();
    assert!(total >= own.len(), "{total} units in the world");
}

// ---------------------------------------------------------------------------
// What never reaches the wire
// ---------------------------------------------------------------------------

/// A key-set allow-list over the rendered JSON, for every viewer and under
/// both fog policies.
///
/// An allow-list rather than a ban-list, because the thing to prevent is a
/// field nobody thought about: a ban-list only catches what somebody already
/// worried over, and "never on the wire" is a claim about everything.
#[test]
fn no_view_ever_carries_a_destination_a_step_a_mandate_or_a_power_state() {
    const ALLOWED: &[&str] = &[
        // The response.
        "at_ms",
        "chunks",
        "entities",
        "next_cursor",
        "complete",
        // A chunk.
        "origin",
        "voxels_rle",
        // An entity.
        "id",
        "kind",
        "subtype",
        "owner",
        "at",
        // What decision 5 shows of an entity in live vision (S2's plan;
        // decisions-log item 144 (11)): its hit points with their maximum,
        // and a unit's heading.
        "hp",
        "max_hp",
        "heading",
        // S3's `probw`: every entity's name in the player's words and a
        // structure's look; and, on the viewer's own assets only (or every
        // asset for a no-fog spectator), a beacon's sphere and a structure going
        // up -- the one fact of build progress the sim keeps, which the owner
        // ruled own-seat only (decisions-log item 168 (11)).
        "title",
        "look",
        "shape",
        "sphere_radius_voxels",
        "building",
        // A voxel.
        "x",
        "y",
        "z",
        // The footer every result carries.
        "_status",
        "phase",
        "phase_remaining_ms",
        "segment_length_ms",
        "round",
        "untimed",
    ];

    // All three policies a match can be under, and they are three: the
    // default, the one a CASUAL match is made with at `Surface::new`, and the
    // one a match that has ended reaches through the unlock. The second and
    // third answer `unfogged` the same way and arrive by different roads, so
    // the allow-list is run over both roads.
    for policy in ["fogged", "casual", "ended"] {
        let mut surface = match policy {
            "casual" => hosted_casual(2, SEGMENT_MS, 3),
            "ended" => {
                let mut surface = hosted(2, SEGMENT_MS, 3);
                surface.fog().end_match();
                surface
            }
            _ => hosted(2, SEGMENT_MS, 3),
        };
        let seat = seat_token(&mut surface, 0);
        let spectator = spectator_token(&mut surface, true);
        let admin = admin_token(&mut surface);
        for token in [&seat, &spectator, &admin] {
            let pages = view_pages(&mut surface, token, "");
            let mut found: BTreeSet<String> = BTreeSet::new();
            for page in &pages {
                keys(page, &mut found);
            }
            for key in &found {
                assert!(
                    ALLOWED.contains(&key.as_str()),
                    "a view under the `{policy}` policy carried `{key}`, which is not one of \
                     the fields `get_view` answers. \
                     Never on the wire: a destination, a route, a step or rule index, a mandate, \
                     a treasury, a score, a power state, another seat's build progress, a count."
                );
            }
            assert!(found.contains("at_ms"), "the scan read a real answer");
        }
    }
}

/// Nothing says how much was withheld -- not a total, not a count, not a
/// marker.
#[test]
fn a_view_response_reports_no_count_of_what_it_withheld() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let seat = seat_token(&mut surface, 0);
    let spectator = spectator_token(&mut surface, true);

    let (fogged, _) = view(&mut surface, &seat, "");
    let (open, _) = view(&mut surface, &spectator, "");
    let fogged_entities = array_of(&fogged, "entities").len();
    let open_entities = array_of(&open, "entities").len();
    assert!(
        open_entities > fogged_entities,
        "the spectator sees more ({open_entities}) than the fogged seat ({fogged_entities}), \
         which is what makes the next assertion mean anything"
    );

    let mut found: BTreeSet<String> = BTreeSet::new();
    keys(&fogged, &mut found);
    for banned in ["total", "count", "hidden", "withheld", "of", "unseen"] {
        assert!(
            !found.contains(banned),
            "a view named `{banned}`: a count of what a seat cannot see is a fog leak by \
             arithmetic (decisions-log item 26)"
        );
    }
}

// ---------------------------------------------------------------------------
// Who gets a view at all
// ---------------------------------------------------------------------------

/// `admin` covers lobby and match control and reads no seat's knowledge.
#[test]
fn admin_gets_an_empty_view() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = admin_token(&mut surface);
    let (page, cursor) = view(&mut surface, &token, "");
    assert!(array_of(&page, "chunks").is_empty(), "no terrain");
    assert!(array_of(&page, "entities").is_empty(), "no entities");
    assert!(
        matches!(page.get("complete"), Some(Json::Bool(true))),
        "an empty view completes at once: there is nothing wrong with the call"
    );
    assert!(!cursor.is_empty(), "and it is still a cursor");

    // A spectator without the scope is the same answer for the other reason
    // fog.rs gives: it has no seat, so there is no fogged view to compute.
    let plain = spectator_token(&mut surface, false);
    let (page, _) = view(&mut surface, &plain, "");
    assert!(array_of(&page, "chunks").is_empty());
    assert!(array_of(&page, "entities").is_empty());
}

/// A spectator with `spectate.nofog` sees every edit and every entity -- and
/// still never a playbook, a draft or a notebook, because that scope lifts
/// **fog** and not **secrecy**.
#[test]
fn a_nofog_spectator_sees_every_edit_and_entity_and_never_a_playbook_draft_or_notebook() {
    let mut surface = pushed();
    let spectator = spectator_token(&mut surface, true);
    let (_, unseen) = seen_and_unseen_in_one_chunk(&surface, 0);

    step(&mut surface, 1);
    let (_, cursor) = view(&mut surface, &spectator, "");
    erase(&mut surface, unseen);
    step(&mut surface, 1);
    let pages = view_pages(&mut surface, &spectator, &cursor);
    assert_eq!(
        material_at(&pages, unseen),
        Some(Material::AIR),
        "the edit no seat could see still reaches a no-fog spectator"
    );

    // Every entity of both seats.
    let (page, _) = view(&mut surface, &spectator, "");
    let owners: BTreeSet<String> = array_of(&page, "entities")
        .iter()
        .map(|entity| text_of(entity, "owner"))
        .collect();
    assert!(owners.contains("seat.0"), "{owners:?}");
    assert!(owners.contains("seat.1"), "{owners:?}");

    // And nothing of a seat's own.
    for (method, params) in [
        ("list_drafts", "{}"),
        ("get_briefing", "{}"),
        ("get_safe_plan", "{}"),
        ("save_notes", r#"{"notes":"x"}"#),
    ] {
        let response = call(&mut surface, &spectator, method, params);
        assert_eq!(
            code(&response),
            "FORBIDDEN_SCOPE",
            "`{method}` reached a spectator: spectate.nofog lifts fog, never secrecy"
        );
    }
}

// ---------------------------------------------------------------------------
// Cursors
// ---------------------------------------------------------------------------

/// The `view_seq` regression: a cursor taken while the match was in its Lull
/// still delivers what the Push went on to reveal.
///
/// A feed stamped with the **sim's** tick would answer this wrongly in the
/// other direction too -- a Lull and a recap consume no tick, so every
/// visibility change in one would compare equal and be lost.
#[test]
fn a_cursor_taken_in_a_lull_still_delivers_what_the_push_reveals() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let (seen, _) = seen_and_unseen_in_one_chunk(&surface, 0);

    let mut remaining = FIRST_LULL_MS;
    let response = call_in_lull(
        &mut surface,
        &token,
        &mut remaining,
        "get_view",
        r#"{"cursor":""}"#,
    );
    let page = result(&response, "get_view");
    assert_eq!(
        page.get("at_ms"),
        Some(&Json::Number(String::from("0"))),
        "a Lull spends no game time"
    );
    let cursor = text_of(&page, "next_cursor");
    // The first page came back through `call_in_lull` so that the Lull's own
    // clock moved; the rest of the keyframe follows the cursor as any client
    // does, and the last one is the position this test keeps.
    let (_, cursor) = view(&mut surface, &token, &cursor);

    assert!(surface.begin_push().expect("a match"));
    erase(&mut surface, seen);
    step(&mut surface, 1);

    let pages = view_pages(&mut surface, &token, &cursor);
    assert_eq!(
        material_at(&pages, seen),
        Some(Material::AIR),
        "the cursor was issued in the Lull and the Push's edit still reached it"
    );
}

/// A cursor from a view this gateway no longer has is `STALE_SNAPSHOT`, and
/// the client's answer is to ask for a keyframe.
#[test]
fn a_cursor_from_another_view_is_stale_rather_than_a_silent_restart() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let (_, cursor) = view(&mut surface, &token, "");

    // A second match is a second view. The cursor is a match's, so the one
    // from the first is refused rather than read as a place in the second --
    // the same seed and the same map, and a different view all the same,
    // because the view's id is minted from the match this gateway attached
    // and not from the map it attached.
    let mut other = hosted_as("m-0002", FogPolicy::fogged(), 2, SEGMENT_MS, 3);
    let held = seat_token(&mut other, 0);
    let response = call(
        &mut other,
        &held,
        "get_view",
        &format!(r#"{{"cursor":"{cursor}"}}"#),
    );
    assert_eq!(
        code(&response),
        "STALE_SNAPSHOT",
        "a cursor of another match is never read as a place in this one"
    );

    let response = call(
        &mut surface,
        &token,
        "get_view",
        r#"{"cursor":"not a cursor"}"#,
    );
    assert_eq!(code(&response), "INVALID_ARGUMENT");
}

// ---------------------------------------------------------------------------
// The unlock
// ---------------------------------------------------------------------------

/// The full map opens up because the **policy** changed, and for no other
/// reason: the same token, before and after, under the default `Limits`.
///
/// Two policy changes, both of which had to work: a seat eliminated
/// ([`pharmakos_gateway::fog::FogPolicy::eliminate`] had no caller at all
/// before this lane) and the match ending.
#[test]
fn the_full_map_unlock_comes_from_a_server_side_policy_change() {
    // Three seats, so that eliminating one does not end the match and the two
    // halves of this test stay separable.
    let mut surface = hosted(3, SEGMENT_MS, 1);
    let watcher = seat_token(&mut surface, 2);
    let bystander = seat_token(&mut surface, 1);
    let (hidden, _) = seen_and_unseen_in_one_chunk(&surface, 0);
    let pristine = surface
        .host()
        .expect("a match")
        .world()
        .voxels()
        .get(hidden)
        .expect("a voxel on the map");

    assert!(surface.begin_push().expect("a match"));
    let (_, cursor) = view(&mut surface, &watcher, "");
    erase(&mut surface, hidden);
    step(&mut surface, 1);

    let (page, cursor) = view(&mut surface, &watcher, &cursor);
    assert!(
        array_of(&page, "chunks").is_empty(),
        "seat 2 is a map away from seat 0's sphere"
    );

    // Elimination. Nothing about the world's terrain changes; the policy does.
    fell(&mut surface, 2);
    step(&mut surface, 1);
    assert!(
        surface
            .fog()
            .eliminated()
            .contains(&pharmakos_sim::tables::SeatId::new(2)),
        "the surface wired FogPolicy::eliminate from the sim's own standing rule"
    );
    let pages = view_pages(&mut surface, &watcher, &cursor);
    assert_eq!(
        material_at(&pages, hidden),
        Some(Material::AIR),
        "an eliminated seat sees the whole map, on the token it already had"
    );

    // And seat 1, still standing, still does not.
    let (page, standing_cursor) = view(&mut surface, &bystander, "");
    assert_eq!(
        material_at(&[page], hidden),
        Some(pristine),
        "the generated byte, because a seat still in the match sees the map it was given"
    );

    // Match end. This match plays one round, so its segment's last tick is the
    // match's final tick: since S1's `fixs` lane the ROUND LIMIT is decided
    // there, by the final audit, so that tick reports `match_ended` and
    // `Surface::step` lifts the fog; the recap then plays and closes.
    step(&mut surface, 200);
    assert!(
        surface.fog().ended(),
        "the last round's final tick ends the match, and the fog with it"
    );
    assert!(surface.end_recap().expect("a match"), "the recap closes");
    assert!(surface.fog().ended(), "and the match stays ended");
    // On the cursor it was already holding, and not on a fresh keyframe: a
    // camera that is caught up asks for a delta, so the unlock has to reach
    // the delta path or it does not reach the camera at all.
    let pages = view_pages(&mut surface, &bystander, &standing_cursor);
    assert_eq!(
        material_at(&pages, hidden),
        Some(Material::AIR),
        "at match end every seat sees everything, on the cursor and the token it already had"
    );
    let pages = view_pages(&mut surface, &bystander, "");
    assert_eq!(
        material_at(&pages, hidden),
        Some(Material::AIR),
        "and a fresh keyframe says the same"
    );
}

// ---------------------------------------------------------------------------
// The payload's two frozen facts
// ---------------------------------------------------------------------------

/// The material byte values on the wire are the sim's own, and the proto
/// comment that a client reads says the same numbers.
///
/// A renumbering in `crates/sim` therefore goes red **here**, in the crate
/// that produces the bytes, rather than in a client that draws the wrong
/// thing.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one row per material of the sim's catalogue, which is the table this test holds the wire to"
)]
fn the_wire_material_table_is_the_sims() {
    const TABLE: &[(u8, &str, Material)] = &[
        (0, "air", Material::AIR),
        (1, "dirt", Material::DIRT),
        (2, "stone", Material::STONE),
        (3, "scrap seam, lean", Material::ORE_LEAN),
        (4, "scrap seam, standard", Material::ORE_STANDARD),
        (5, "scrap seam, rich", Material::ORE_RICH),
        (6, "heat vent, lean", Material::VENT_LEAN),
        (7, "heat vent, standard", Material::VENT_STANDARD),
        (8, "heat vent, rich", Material::VENT_RICH),
        (9, "wall, seat 0, whole", Material::WALL_SEAT_0),
        (10, "wall, seat 0, cracked", Material::WALL_SEAT_0_CRACKED),
        (11, "wall, seat 1, whole", Material::WALL_SEAT_1),
        (12, "wall, seat 1, cracked", Material::WALL_SEAT_1_CRACKED),
        (13, "wall, seat 2, whole", Material::WALL_SEAT_2),
        (14, "wall, seat 2, cracked", Material::WALL_SEAT_2_CRACKED),
        (15, "wall, seat 3, whole", Material::WALL_SEAT_3),
        (16, "wall, seat 3, cracked", Material::WALL_SEAT_3_CRACKED),
        (17, "dirt, cracked once", Material::DIRT_CRACKED_ONCE),
        (18, "dirt, cracked twice", Material::DIRT_CRACKED_TWICE),
        (19, "stone, cracked once", Material::STONE_CRACKED_ONCE),
        (20, "stone, cracked twice", Material::STONE_CRACKED_TWICE),
        (
            21,
            "scrap seam, lean, cracked once",
            Material::ORE_LEAN_CRACKED_ONCE,
        ),
        (
            22,
            "scrap seam, lean, cracked twice",
            Material::ORE_LEAN_CRACKED_TWICE,
        ),
        (
            23,
            "scrap seam, standard, cracked once",
            Material::ORE_STANDARD_CRACKED_ONCE,
        ),
        (
            24,
            "scrap seam, standard, cracked twice",
            Material::ORE_STANDARD_CRACKED_TWICE,
        ),
        (
            25,
            "scrap seam, rich, cracked once",
            Material::ORE_RICH_CRACKED_ONCE,
        ),
        (
            26,
            "scrap seam, rich, cracked twice",
            Material::ORE_RICH_CRACKED_TWICE,
        ),
        (
            27,
            "heat vent, lean, cracked once",
            Material::VENT_LEAN_CRACKED_ONCE,
        ),
        (
            28,
            "heat vent, lean, cracked twice",
            Material::VENT_LEAN_CRACKED_TWICE,
        ),
        (
            29,
            "heat vent, standard, cracked once",
            Material::VENT_STANDARD_CRACKED_ONCE,
        ),
        (
            30,
            "heat vent, standard, cracked twice",
            Material::VENT_STANDARD_CRACKED_TWICE,
        ),
        (
            31,
            "heat vent, rich, cracked once",
            Material::VENT_RICH_CRACKED_ONCE,
        ),
        (
            32,
            "heat vent, rich, cracked twice",
            Material::VENT_RICH_CRACKED_TWICE,
        ),
        (33, "bedrock", Material::BEDROCK),
    ];
    assert_eq!(TABLE.len(), Material::ALL.len(), "every material v1 has");

    let proto = fs::read_to_string(
        workspace_root()
            .join("proto")
            .join("gp")
            .join("api")
            .join("v1")
            .join("gateway.proto"),
    )
    .expect("the schema is where it always was");
    for (value, name, material) in TABLE {
        assert_eq!(
            material.raw(),
            *value,
            "the sim spells `{name}` {} and the wire says {value}",
            material.raw()
        );
        assert!(
            proto.contains(&format!("{value}  {name}")),
            "`ViewChunk`'s comment does not list `{value}  {name}`, so a client reading the \
             schema would not know how to draw it"
        );
    }
}

/// Every chunk a view serves decodes to a whole chunk, and a payload that is
/// not one is refused rather than drawn short.
#[test]
fn a_view_chunk_decodes_to_a_whole_chunk_and_a_short_run_list_is_refused() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let pages = view_pages(&mut surface, &token, "");
    let chunks = chunks_of(&pages);
    assert!(
        chunks.len() > 100,
        "the whole generated map: {}",
        chunks.len()
    );
    for (origin, voxels) in &chunks {
        assert_eq!(
            voxels.len(),
            pharmakos_proto::chunk_rle::CHUNK_VOXELS,
            "chunk at {origin:?}"
        );
    }
    // The decoder's own refusals are `crates/proto`'s tests; this is the one
    // that matters at the door: a truncated payload is never half a chunk.
    assert!(pharmakos_proto::chunk_rle::decode(&[2, 0, 0]).is_err());
}

// ---------------------------------------------------------------------------
// Goldens
// ---------------------------------------------------------------------------

/// One JSON value on one line.
///
/// `pharmakos_proto::json::write` is the canonical writer and puts one entry
/// on a line, which is what every other golden in this project wants and what
/// a `.jsonl` fixture cannot have. Collapsing its output is safe because the
/// writer escapes every control character inside a string, so the only
/// newlines in its text are the ones it put between entries.
fn one_line(value: &Json) -> String {
    pharmakos_proto::json::write(value)
        .lines()
        .map(str::trim_start)
        .collect::<Vec<&str>>()
        .concat()
}

/// The one line ending every golden in this tree uses.
fn row(out: &mut String, record: &str) {
    out.push_str(record);
    out.push('\n');
}

/// Write the fresh output and compare it with the committed golden.
fn check(case: &str, extension: &str, contents: &str) {
    assert!(!contents.contains('\r'), "{case}: LF endings only");
    assert!(contents.ends_with('\n'), "{case}: a trailing newline");

    let fresh = target_dir().join("golden").join("gateway").join(case);
    fs::create_dir_all(&fresh)
        .unwrap_or_else(|error| panic!("creating {}: {error}", fresh.display()));
    fs::write(fresh.join(format!("actual.{extension}")), contents)
        .unwrap_or_else(|error| panic!("writing {case}'s fresh output: {error}"));

    let golden = workspace_root()
        .join("tests")
        .join("golden")
        .join("gateway")
        .join(case)
        .join(format!("expected.{extension}"));
    let Ok(expected) = fs::read_to_string(&golden) else {
        panic!(
            "no committed golden at {}. Run `cargo xtask golden --bless` once the fresh output \
             is right.",
            golden.display()
        );
    };
    assert_eq!(
        expected, contents,
        "the {case} golden moved. tests/golden/gateway/README.md says what a diff here means; \
         `cargo xtask golden --bless` accepts it, and the pull request has to say which \
         behaviour changed."
    );
}

/// One tick, two edits, five viewers, two policies.
///
/// The table is what a reviewer reads instead of running anything: for each
/// viewer, how many chunks its keyframe carried, what its own bytes say at the
/// voxel inside seat 0's sphere and at the one outside it, and which entities
/// it was told about.
#[test]
fn the_view_one_tick_golden() {
    let mut out = String::from(
        "# One tick of a hosted match through every viewer's `get_view` (item 107 (5)).\n\
         # Two voxels of ONE chunk were turned to air in the same tick: `seen` is inside\n\
         # seat 0's core sphere and `unseen` is outside it, so the chunk is half seen.\n\
         # The material column is what THAT viewer's own bytes say at that voxel.\n\
         # policy\tviewer\tchunks\tseen\tunseen\tentities\n",
    );
    for policy in ["fogged", "match-ended"] {
        let mut surface = hosted(2, SEGMENT_MS, 3);
        let seat0 = seat_token(&mut surface, 0);
        let seat1 = seat_token(&mut surface, 1);
        let plain = spectator_token(&mut surface, false);
        let nofog = spectator_token(&mut surface, true);
        let admin = admin_token(&mut surface);
        let (seen, unseen) = seen_and_unseen_in_one_chunk(&surface, 0);

        assert!(surface.begin_push().expect("a match"));
        erase(&mut surface, seen);
        erase(&mut surface, unseen);
        step(&mut surface, 1);
        if policy == "match-ended" {
            surface.fog().end_match();
        }

        for (name, token) in [
            ("seat.0", &seat0),
            ("seat.1", &seat1),
            ("spectator", &plain),
            ("spectator.nofog", &nofog),
            ("admin", &admin),
        ] {
            let pages = view_pages(&mut surface, token, "");
            let last = pages.last().cloned().expect("a page");
            let chunks = chunks_of(&pages).len();
            let at = |place: [i32; 3]| {
                material_at(&pages, place)
                    .map_or_else(|| String::from("-"), |m| m.name().to_owned())
            };
            let mut ids = entity_ids(&last);
            ids.sort();
            let entities = if ids.is_empty() {
                String::from("-")
            } else {
                ids.join(" ")
            };
            row(
                &mut out,
                &format!(
                    "{policy}\t{name}\t{chunks}\t{}\t{}\t{entities}",
                    at(seen),
                    at(unseen)
                ),
            );
        }
    }
    check("view_one_tick", "view.txt", &out);
}

/// Seat 0's keyframe at the opening Lull of the golden seed: one served
/// `GetViewResponse` per line, in page order.
///
/// **The fixture T16's hostless screenshot and `client-gdext`'s tests load.**
/// It is produced here, by the gateway, because the gateway is the producer:
/// a fixture made by the consumer tests nothing but the consumer.
#[test]
fn the_view_keyframe_golden() {
    // The footer this fixture carries reports the whole first Lull,
    // `first_lull_ms` (600 000 ms), as the hosted match opens it.
    // `godot/fixtures/view_keyframe.jsonl` is a byte-for-byte copy of it
    // (`crates/client-gdext/tests/vista_fixture.rs`), re-copied in the same
    // commit that dropped the 300 000 ms pin the `econ` lane kept (decisions-
    // log item 135 (2) (h)).
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let pages = view_pages(&mut surface, &token, "");

    let mut jsonl = String::new();
    for page in &pages {
        let line = one_line(page);
        assert!(!line.contains('\n'), "one response to a line");
        assert_eq!(
            pharmakos_proto::json::read(&line).expect("valid JSON"),
            *page,
            "the fixture a client decodes is the answer the gateway gave"
        );
        row(&mut jsonl, &line);
    }

    // Two sizes, because they are two different numbers and the one a reader
    // takes away should not be ambiguous: the runs are what the feed budgets
    // and pages on, the JSON is what actually goes down the socket once
    // `bytes` has become base64 and the entities have been written out.
    let run_bytes: usize = chunks_of(&pages)
        .iter()
        .map(|(_, voxels)| pharmakos_proto::chunk_rle::encode(voxels).len())
        .sum();
    let mut readable = format!(
        "# Seat 0's get_view keyframe at the opening Lull of seed 0x{SEED:016x}.\n\
         # The whole generated map: what is fogged is what changes it and what stands on\n\
         # it, and at the opening Lull nothing has (item 107 (1)).\n\
         # pages {}, chunks {}, run bytes {run_bytes}, json bytes {}\n\
         # chunk\torigin\truns\tdigest\n",
        pages.len(),
        chunks_of(&pages).len(),
        jsonl.len(),
    );
    for (index, (origin, voxels)) in chunks_of(&pages).iter().enumerate() {
        let encoded = pharmakos_proto::chunk_rle::encode(voxels);
        let runs = encoded.len().checked_div(3).unwrap_or(0);
        row(
            &mut readable,
            &format!(
                "{index}\t({},{},{})\t{runs}\t{:016x}",
                origin.0,
                origin.1,
                origin.2,
                pharmakos_sim::digest(voxels)
            ),
        );
    }
    let mut entities = entity_ids(pages.last().expect("a page"));
    entities.sort();
    row(
        &mut readable,
        &format!("# entities\t{}", entities.join(" ")),
    );

    check("view_keyframe", "keyframe.jsonl", &jsonl);
    check("view_keyframe", "keyframe.txt", &readable);
}

/// The encoded size of the whole-map keyframe, reported as a number.
///
/// Not a gate: the plan asks for the figure, and a threshold on it would be a
/// perf budget in a golden (`tests/golden/README.md`, "What is not here").
#[test]
fn the_whole_map_keyframe_is_measured_and_reported() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let pages = view_pages(&mut surface, &token, "");
    let bytes: usize = pages
        .iter()
        .map(|page| pharmakos_proto::json::write(page).len())
        .sum();
    let runs: usize = chunks_of(&pages)
        .iter()
        .map(|(_, voxels)| pharmakos_proto::chunk_rle::encode(voxels).len())
        .sum();
    println!(
        "::notice::whole-map keyframe for seed 0x{SEED:016x}: {} chunks, {runs} bytes of runs, \
         {bytes} bytes of JSON, {} pages at {} bytes a page",
        chunks_of(&pages).len(),
        pages.len(),
        pharmakos_gateway::viewfeed::VIEW_PAGE_BYTES,
    );
    assert!(runs > 0);
}

/// E28's measurement (S2's plan; the register's S2-12): the bytes of a full
/// entity list at the gate load of 300 units, reported as a number for the
/// pull request that records it.
///
/// Two seats each field 150 raiders on top of their starting force, laid out
/// in the lobby through `Host::seed_layout` (E30's seam, which replaced the
/// harness walkers when S2-16 removed them), and a casual match shows
/// every entity to every seat, so the seat's keyframe lists the whole world:
/// every unit with its hit points, maximum and heading (decision 5). Not a gate, as the keyframe's size is not: a delta
/// mode is decided by `view`'s per-frame cost against this figure (E28).
#[test]
fn the_entity_list_at_the_gate_load_is_measured_and_reported() {
    use pharmakos_gateway::host::Host;
    use pharmakos_sim::runner::MatchSettings;
    use pharmakos_sim::runner::{Layout, LayoutBeacon};
    use pharmakos_sim::seams::MandateKind;
    use pharmakos_sim::tables::UnitKind;
    use pharmakos_sim::world::WorldConfig;
    const UNITS_PER_SEAT: u32 = 150;
    const BEACONS_PER_SEAT: u32 = 5;
    let ids = [SeatId::new(0), SeatId::new(1)];
    let mut surface =
        Surface::new(MATCH, SEED, support::rules(), FogPolicy::casual(), &ids).expect("a match id");
    let mut host = Host::open(
        &WorldConfig {
            match_seed: SEED,
            seats: 2,
            rules: support::rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![SEGMENT_MS],
                round_limit: 3,
            },
        },
        None,
    )
    .expect("a match");
    let beacons = ids
        .iter()
        .flat_map(|seat| {
            (0..BEACONS_PER_SEAT).map(move |index| LayoutBeacon {
                seat: *seat,
                column: [
                    40_i32.saturating_add(
                        60_i32.saturating_mul(i32::try_from(index).expect("five beacons")),
                    ),
                    60_i32.saturating_add(200_i32.saturating_mul(i32::from(seat.raw()))),
                ],
                mandate: MandateKind::Build,
                units: vec![(
                    UnitKind::Raider,
                    UNITS_PER_SEAT
                        .checked_div(BEACONS_PER_SEAT)
                        .expect("five beacons"),
                )],
            })
        })
        .collect();
    host.seed_layout(&Layout { beacons })
        .expect("the gate load fits each seat's share");
    surface.attach(host).expect("attached");
    surface
        .set_phase_remaining_ms(pharmakos_sim::math::quantity::Ms::new(FIRST_LULL_MS))
        .expect("a custom match takes a countdown");
    surface.open_lull().expect("the opening Lull");
    let token = seat_token(&mut surface, 0);
    let pages = view_pages(&mut surface, &token, "");
    let last = pages.last().expect("a page");
    let Some(Json::Array(entities)) = last.get("entities") else {
        panic!("the completing page lists the entities: {last:?}");
    };
    let units = entities
        .iter()
        .filter(|entity| entity.get("kind") == Some(&Json::String(String::from("unit"))))
        .count();
    assert!(
        units >= 300,
        "the gate load is 300 units, and the list holds {units}"
    );
    let bytes = pharmakos_proto::json::write(&Json::Array(entities.clone())).len();
    let per_entity = bytes.checked_div(entities.len()).expect("a list of some");
    println!(
        "::notice::full entity list at the gate load (seed 0x{SEED:016x}, {UNITS_PER_SEAT} raiders \
         a seat, casual): {} entities, {units} of them units, {bytes} bytes of JSON, about \
         {per_entity} bytes an entity",
        entities.len(),
    );
    assert!(
        entities.iter().all(|entity| entity.get("hp").is_some()),
        "in a casual match every listed entity is in live vision and carries its hit points"
    );
}

/// A watch session, call by call: the shape T16's watch rig drives.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "a transcript is one call after another and reads as one; splitting it into halves would put the session's own order behind a function call"
)]
fn the_walkthrough_watch_golden() {
    let mut surface = hosted(2, SEGMENT_MS, 2);
    let seat = seat_token(&mut surface, 0);
    let admin = admin_token(&mut surface);
    let mut out = String::from(
        "# A watch session, as the lobby and the camera drive it together.\n\
         # The camera holds the seat's connection and the lobby holds the admin's; the\n\
         # gateway reads no clock, so the lobby's `report_host_clock` is what moves the\n\
         # gateway's tick in every phase that spends no sim tick.\n\
         # call\tmethod\tconnection\tanswer\n",
    );
    let mut line = 0_u32;
    let mut say = |out: &mut String, method: &str, who: &str, answer: &str| {
        line = line.saturating_add(1);
        row(out, &format!("{line}\t{method}\t{who}\t{answer}"));
    };

    // 1. The camera asks for a keyframe.
    let pages = view_pages(&mut surface, &seat, "");
    let mut cursor = text_of(pages.last().expect("a page"), "next_cursor");
    say(
        &mut out,
        "get_view",
        "seat",
        &format!(
            "keyframe: {} pages, {} chunks, {} entities",
            pages.len(),
            chunks_of(&pages).len(),
            array_of(pages.last().expect("a page"), "entities").len()
        ),
    );

    // 2. The lobby reports its clock; the answer says whether Ready may end
    //    the Lull.
    let response = call(
        &mut surface,
        &admin,
        "report_host_clock",
        r#"{"elapsed_ms":1000,"remaining_ms":179000}"#,
    );
    let answer = result(&response, "report_host_clock");
    say(
        &mut out,
        "report_host_clock",
        "admin",
        &format!("all_ready={:?}", answer.get("all_ready")),
    );

    // 3. The lobby ends the Lull.
    let response = call(&mut surface, &admin, "end_lull", "{}");
    let answer = result(&response, "end_lull");
    say(
        &mut out,
        "end_lull",
        "admin",
        &format!("phase={}", phase_of(&answer)),
    );

    // 4. Four speeds' worth of game time, in one call.
    let response = call(&mut surface, &admin, "advance_push", r#"{"ms":200}"#);
    let answer = result(&response, "advance_push");
    say(
        &mut out,
        "advance_push",
        "admin",
        &format!(
            "advanced_ms={:?}, phase={}",
            answer.get("advanced_ms"),
            phase_of(&answer)
        ),
    );

    // 5. The camera's delta.
    let (page, next) = view(&mut surface, &seat, &cursor);
    cursor = next;
    say(
        &mut out,
        "get_view",
        "seat",
        &format!(
            "delta: {} chunks, {} entities, at_ms={:?}",
            array_of(&page, "chunks").len(),
            array_of(&page, "entities").len(),
            page.get("at_ms")
        ),
    );

    // 6. A skip: the largest step, repeated, until the footer leaves the Push.
    let mut skips = 0_u32;
    loop {
        let response = call(&mut surface, &admin, "advance_push", r#"{"ms":60000}"#);
        if response.get("result").is_none() {
            break;
        }
        skips = skips.saturating_add(1);
        if phase_of(&result(&response, "advance_push")) != "push" {
            break;
        }
        assert!(skips < 20, "a skip that never ends");
    }
    say(
        &mut out,
        "advance_push",
        "admin",
        &format!("skip: {skips} call(s), phase=recap"),
    );

    // 7. The lobby's clock in the recap, which is where the tick would
    //    otherwise stand still.
    let before = surface.time().tick;
    let response = call(
        &mut surface,
        &admin,
        "report_host_clock",
        r#"{"elapsed_ms":2000,"remaining_ms":0}"#,
    );
    let _ = result(&response, "report_host_clock");
    say(
        &mut out,
        "report_host_clock",
        "admin",
        &format!("tick moved by {}", surface.time().tick.since(before)),
    );

    // 8. And on into the next Lull.
    let response = call(&mut surface, &admin, "end_recap", "{}");
    let answer = result(&response, "end_recap");
    say(
        &mut out,
        "end_recap",
        "admin",
        &format!("phase={}", phase_of(&answer)),
    );

    // 9. The camera picks up where it left off.
    let (page, _) = view(&mut surface, &seat, &cursor);
    say(
        &mut out,
        "get_view",
        "seat",
        &format!(
            "delta: {} chunks, {} entities",
            array_of(&page, "chunks").len(),
            array_of(&page, "entities").len()
        ),
    );

    check("walkthrough_watch", "watch.txt", &out);
    assert_eq!(MATCH, "m-0001");
}

/// The phase the `_status` footer of a result reports.
fn phase_of(answer: &Json) -> String {
    answer
        .get("_status")
        .and_then(|status| status.get("phase"))
        .map_or_else(
            || String::from("<no footer>"),
            |value| match value {
                Json::String(text) => text.clone(),
                other => format!("{other:?}"),
            },
        )
}

/// A seat, a spectator and the lobby are three different viewers of one feed,
/// and none of them can be mistaken for another.
#[test]
fn a_viewer_is_a_seat_a_spectator_or_the_lobby() {
    assert_ne!(
        Viewer::Seat(SeatId::new(0)),
        Viewer::Spectator { nofog: true }
    );
    assert_ne!(Viewer::Admin, Viewer::Spectator { nofog: false });
    let _: Option<&Token> = None;
}

/// The beacon names one `list_beacons` page answers, in its order.
fn listed<V: Vision>(surface: &mut Surface, token: &Token, vision: &V) -> Vec<String> {
    let response = surface.call(Some(token), &support::request("list_beacons", "{}"), vision);
    array_of(&result(&response, "list_beacons"), "beacons")
        .iter()
        .map(|row| text_of(row, "beacon_id"))
        .collect()
}

/// Decisions-log item 127 (13): a seat names its own beacons `b_NN` per seat
/// and everybody else's `e_NN`, a handle counting what that viewer knows,
/// never another seat's placements, with two viewers numbering the same
/// beacon independently (`docs/design/targeting.md`, "Names"). Since the
/// main session's ruling R1 on #128's reviews a seat's handles are minted
/// only where a Lull opens or a save resumes -- from its sightings in the
/// sim's table order, then its live vision by row -- and never by a listing,
/// so the vision a call is handed names nothing.
#[test]
fn another_seats_beacon_is_e_nn_minted_where_the_lull_opens() {
    // Fogged: at the opening Lull no seat sees another's core, so none is
    // named, and a listing handed a vision that sees everything mints none.
    let mut fogged = hosted(3, SEGMENT_MS, 3);
    let zero = seat_token(&mut fogged, 0);
    assert_eq!(listed(&mut fogged, &zero, &Everything), ["b_00"]);
    let response = fogged.call(
        Some(&zero),
        &support::request("get_beacon", r#"{"beacon_id":"e_01"}"#),
        &Everything,
    );
    assert_eq!(code(&response), "NOT_FOUND", "a listing minted no name");

    // Casual: every core is in live vision when the Lull opens, so each seat
    // holds a name for each other seat's, minted by row.
    let mut surface = hosted_casual(3, SEGMENT_MS, 3);
    let zero = seat_token(&mut surface, 0);
    let one = seat_token(&mut surface, 1);
    assert_eq!(
        listed(&mut surface, &zero, &Everything),
        ["b_00", "e_01", "e_02"]
    );
    let get = |surface: &mut Surface, token: &Token, name: &str| {
        let response = surface.call(
            Some(token),
            &support::request("get_beacon", &format!(r#"{{"beacon_id":"{name}"}}"#)),
            &Everything,
        );
        let summary = result(&response, "get_beacon")
            .get("summary")
            .cloned()
            .expect("a summary");
        (text_of(&summary, "beacon_id"), text_of(&summary, "owner"))
    };
    // On the golden seed seat 1's core comes before seat 2's in the table.
    assert_eq!(get(&mut surface, &zero, "e_01").1, "seat.1");
    assert_eq!(get(&mut surface, &zero, "e_02").1, "seat.2");
    assert_eq!(get(&mut surface, &zero, "b_00").1, "seat.0");
    let response = surface.call(
        Some(&zero),
        &support::request("get_beacon", r#"{"beacon_id":"e_03"}"#),
        &Everything,
    );
    assert_eq!(
        code(&response),
        "NOT_FOUND",
        "a handle never minted names nothing"
    );
    // Seat 1 numbers the world its own way.
    assert_eq!(
        listed(&mut surface, &one, &Everything),
        ["b_00", "e_01", "e_02"]
    );
    assert_eq!(get(&mut surface, &one, "e_01").1, "seat.0");
    assert_eq!(get(&mut surface, &one, "b_00").1, "seat.1");
}

// Sees everything.
struct Everything;

impl Vision for Everything {
    fn sees(&self, _seat: SeatId, _at: &Voxel) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// S3's `probw`: the ring, a structure going up, the look, the names and walls
// ---------------------------------------------------------------------------

/// The scenario playbook that raises an autocannon and fills a wall at seat
/// 0's core on the golden seed (`scenarios/s2/base-kit-at-the-core`).
const BASE_KIT: &str = "scenarios/s2/base-kit-at-the-core.playbook.jsonc";

/// Its segment: long enough for the autocannon and the wall.
const KIT_SEGMENT_MS: i32 = 60_000;

/// A two-seat match under `policy` whose Push has begun with seat 0 sealed on
/// [`BASE_KIT`], `rebuild_destroyed` as given, and seat 1 on the safe
/// playbook.
fn base_kit(policy: FogPolicy, rebuild: bool) -> Surface {
    let mut surface = hosted_as(MATCH, policy, 2, KIT_SEGMENT_MS, 1);
    let text = fs::read_to_string(workspace_root().join(BASE_KIT)).expect("the scenario playbook");
    let text = if rebuild {
        text
    } else {
        let off = text.replace(
            "\"rebuild_destroyed\": true",
            "\"rebuild_destroyed\": false",
        );
        assert_ne!(off, text, "the playbook names rebuild_destroyed");
        off
    };
    let token = seat_token(&mut surface, 0);
    let mut remaining = FIRST_LULL_MS;
    let answer = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut remaining,
            "submit_plan",
            &format!("{{\"playbook_jsonc\":{}}}", support::quote(&text)),
        ),
        "submit_plan",
    );
    assert_eq!(
        answer.get("accepted"),
        Some(&Json::Bool(true)),
        "{answer:?}"
    );
    assert!(surface.begin_push().expect("a hosted match"), "the Push");
    surface
}

/// A page decodes as a `gp.api.v1.GetViewResponse`, as a client reads it.
fn decodes(page: &Json) {
    let decoded: Result<pharmakos_proto::gp::api::v1::GetViewResponse, _> =
        pharmakos_proto::json::decode_json(&support::canonical_answer(
            "gp.api.v1.GetViewResponse",
            page,
        ));
    assert!(decoded.is_ok(), "not a GetViewResponse: {decoded:?}");
}

/// One viewer reading the view as a client does: a keyframe first, then the
/// cursor each completing page hands back.
struct Watcher {
    token: Token,
    cursor: String,
}

impl Watcher {
    fn new(token: Token) -> Watcher {
        Watcher {
            token,
            cursor: String::new(),
        }
    }

    /// The complete entity list of the next view, under `vision`.
    fn entities<V: Vision>(&mut self, surface: &mut Surface, vision: &V) -> Vec<Json> {
        loop {
            let response = surface.call(
                Some(&self.token),
                &support::request(
                    "get_view",
                    &format!("{{\"cursor\":{}}}", support::quote(&self.cursor)),
                ),
                vision,
            );
            let page = result(&response, "get_view");
            decodes(&page);
            self.cursor = text_of(&page, "next_cursor");
            if page.get("complete") == Some(&Json::Bool(true)) {
                return array_of(&page, "entities");
            }
        }
    }

    /// As [`Watcher::entities`], through the production vision.
    fn live(&mut self, surface: &mut Surface) -> Vec<Json> {
        let sight = support::vision(surface);
        self.entities(surface, &sight)
    }
}

/// The first entity of `entities` with this subtype and owner.
fn find<'a>(entities: &'a [Json], subtype: &str, owner: &str) -> Option<&'a Json> {
    entities
        .iter()
        .find(|entity| text_of(entity, "subtype") == subtype && text_of(entity, "owner") == owner)
}

/// One whole-number member.
fn number(value: &Json, key: &str) -> i32 {
    match value.get(key) {
        Some(Json::Number(text)) => text.parse::<i32>().expect("a whole number"),
        other => panic!("{key}: {other:?} on {value:?}"),
    }
}

/// Seat 0's live autocannon's `building` column, or `None` before it is
/// placed.
fn autocannon_building(surface: &Surface) -> Option<bool> {
    building(surface, pharmakos_sim::tables::StructureKind::Autocannon)
}

/// Where `the_view_built_golden` queues seat 0's mortar: a column inside its
/// core's sphere, clear of the scenario's autocannon and wall.
const MORTAR_SITE: [i32; 2] = [365, 30];

/// The `building` column of seat 0's first live structure of `kind`, or `None`
/// before one is placed.
fn building(surface: &Surface, of: pharmakos_sim::tables::StructureKind) -> Option<bool> {
    let world = surface.host().expect("a match").world();
    let structures = world.structures();
    (0..structures.ids().len()).find_map(|row| {
        let alive = structures.hit_points().get(row)?.is_alive();
        let free = *structures.frees().get(row)?;
        let ours = *structures.seats().get(row)? == 0;
        let kind = *structures.kinds().get(row)?;
        if alive && !free && ours && kind == of.id() {
            structures.building().get(row).copied()
        } else {
            None
        }
    })
}

/// S3's `probw`, items 2 and 4: seat 0's autocannon carries `building: true`
/// and the going-up title from its first page until the sim finishes it, then
/// neither, for seat 0 and for a no-fog spectator, read against the sim's own
/// column on every page; seat 1, which sees it in a casual match, is shown its
/// hit points and never that it is going up, by flag or by name.
#[test]
fn a_structure_going_up_is_told_to_its_own_seat_and_the_no_fog_spectator_only() {
    let mut surface = base_kit(FogPolicy::casual(), true);
    let mut own = Watcher::new(seat_token(&mut surface, 0));
    let mut theirs = Watcher::new(seat_token(&mut surface, 1));
    let mut nofog = Watcher::new(spectator_token(&mut surface, true));
    let mut going_up = 0_u32;
    let mut standing = 0_u32;
    for _ in 0..300 {
        step(&mut surface, 4);
        let Some(building) = autocannon_building(&surface) else {
            continue;
        };
        let title = if building {
            "Autocannon, going up"
        } else {
            "Autocannon"
        };
        for (who, watcher) in [("seat 0", &mut own), ("the no-fog spectator", &mut nofog)] {
            let entities = watcher.live(&mut surface);
            let cannon = find(&entities, "autocannon", "seat.0")
                .unwrap_or_else(|| panic!("{who} sees seat 0's autocannon"));
            assert_eq!(
                cannon.get("building"),
                building.then_some(&Json::Bool(true)),
                "{who}: {cannon:?}"
            );
            assert_eq!(text_of(cannon, "title"), title, "{who}");
            let look = cannon.get("look").expect("a structure's look");
            assert_eq!(text_of(look, "shape"), "tower");
            assert_eq!(
                [number(look, "x"), number(look, "y"), number(look, "z")],
                [1, 1, 1],
                "one column, as the sim stands a structure"
            );
        }
        let entities = theirs.live(&mut surface);
        let cannon = find(&entities, "autocannon", "seat.0").expect("seat 1 sees it, unfogged");
        assert_eq!(
            cannon.get("building"),
            None,
            "never another seat's: {cannon:?}"
        );
        assert_eq!(text_of(cannon, "title"), "Autocannon", "named as standing");
        assert!(cannon.get("hp").is_some(), "its hit points still show");
        if building {
            going_up = going_up.saturating_add(1);
        } else {
            assert!(going_up > 0, "it was first seen going up");
            standing = standing.saturating_add(1);
            if standing > 2 {
                break;
            }
        }
    }
    assert!(
        going_up > 0 && standing > 0,
        "{going_up} pages going up, {standing} standing"
    );
}

/// Seat 0's core with a finished Resonance Spire beside it, under `policy`,
/// in its opening Lull, and the Spire's id.
fn spire_at_the_core(policy: FogPolicy) -> (Surface, pharmakos_sim::tables::StructureId) {
    use pharmakos_gateway::host::Host;
    use pharmakos_sim::math::fixed::Fx;
    let config = pharmakos_sim::world::WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules: support::rules(),
        match_settings: pharmakos_sim::runner::MatchSettings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: 3,
        },
    };
    let mut world = pharmakos_sim::world::World::new(&config).expect("the world generates");
    let row = (0..world.beacons().ids().len())
        .find(|row| world.beacons().seats().get(*row) == Some(&0))
        .expect("seat 0's core");
    let core = pharmakos_sim::tables::BeaconId::new(
        world.beacons().ids().get(row).copied().expect("an id"),
    );
    let at = world
        .beacons()
        .positions()
        .get(row)
        .copied()
        .expect("a place");
    let beside = [at[0].saturating_add(Fx::from_voxels(1)), at[1], at[2]];
    let spire = world
        .raise_structure(
            SeatId::new(0),
            core,
            pharmakos_sim::tables::StructureKind::ResonanceSpire,
            beside,
        )
        .expect("room for a Spire");
    let bytes = pharmakos_sim::snapshot::Snapshot::capture(&world)
        .expect("a world between ticks")
        .to_bytes()
        .expect("it encodes");
    let mut host = Host::open(&config, Some(support::library_folder())).expect("a match");
    host.resume(&bytes).expect("the world resumes");
    let mut surface = Surface::new(
        MATCH,
        SEED,
        support::rules(),
        policy,
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a match id");
    surface.attach(host).expect("attached");
    surface
        .set_phase_remaining_ms(pharmakos_sim::math::quantity::Ms::new(FIRST_LULL_MS))
        .expect("a countdown");
    surface.open_lull().expect("the opening Lull");
    (surface, spire)
}

/// `list_beacons`' radius for each of `token`'s own beacons, by id.
fn listed_radii(surface: &mut Surface, token: &Token) -> Vec<(String, Json)> {
    let listed = result(&call(surface, token, "list_beacons", "{}"), "list_beacons");
    array_of(&listed, "beacons")
        .into_iter()
        .filter_map(|beacon| {
            let id = text_of(&beacon, "beacon_id");
            if !id.starts_with("b_") {
                return None;
            }
            let radius = beacon
                .get("sphere_radius_voxels")
                .cloned()
                .expect("an own beacon's radius");
            Some((id, radius))
        })
        .collect()
}

/// The radius `entities` carries on the entity `id`.
fn radius_of(entities: &[Json], id: &str) -> Option<Json> {
    entities
        .iter()
        .find(|entity| text_of(entity, "id") == id)
        .unwrap_or_else(|| panic!("{id} is listed"))
        .get("sphere_radius_voxels")
        .cloned()
}

/// S3's `probw`, item 1: a beacon entity carries the sim's radius on the
/// viewer's own beacons, equal to `list_beacons`' for each, grown beside a
/// finished Spire and the base elsewhere; never on another seat's beacon in
/// live vision; on every beacon for a no-fog spectator; on nothing that is not
/// a beacon; and a radius that changes in a Push changes the next page.
#[test]
fn the_ring_is_on_own_beacons_and_follows_the_spire() {
    let (mut surface, spire) = spire_at_the_core(FogPolicy::casual());
    let grown = Json::Number(support::rules().grown_sphere_radius_voxels().to_string());
    let base = Json::Number(sphere_radius(&surface).to_string());
    let zero = seat_token(&mut surface, 0);
    let one = seat_token(&mut surface, 1);
    let mut seat0 = Watcher::new(zero.clone());
    let mut seat1 = Watcher::new(one.clone());
    let mut nofog = Watcher::new(spectator_token(&mut surface, true));

    for (token, watcher, theirs) in [(&zero, &mut seat0, "seat.1"), (&one, &mut seat1, "seat.0")] {
        let entities = watcher.live(&mut surface);
        for (id, radius) in listed_radii(&mut surface, token) {
            assert_eq!(radius_of(&entities, &id), Some(radius), "{id}");
        }
        for entity in &entities {
            let beacon = text_of(entity, "kind") == "beacon";
            if !beacon || text_of(entity, "owner") == theirs {
                assert_eq!(
                    entity.get("sphere_radius_voxels"),
                    None,
                    "not an own beacon: {entity:?}"
                );
            }
        }
        assert!(
            entities
                .iter()
                .any(|entity| text_of(entity, "kind") == "beacon"
                    && text_of(entity, "owner") == theirs),
            "the other seat's core is in live vision"
        );
    }
    assert_eq!(
        radius_of(&seat0.live(&mut surface), "b_00"),
        Some(grown.clone())
    );
    assert_eq!(
        radius_of(&seat1.live(&mut surface), "b_00"),
        Some(base.clone())
    );

    let entities = nofog.live(&mut surface);
    let cores: Vec<Option<Json>> = entities
        .iter()
        .filter(|entity| text_of(entity, "kind") == "beacon")
        .map(|entity| entity.get("sphere_radius_voxels").cloned())
        .collect();
    assert_eq!(cores.len(), 2, "{entities:?}");
    assert!(cores.contains(&Some(grown)) && cores.contains(&Some(base.clone())));

    // The Spire falls in a Push: the very next page draws the base ring.
    assert!(surface.begin_push().expect("a match"));
    let host = surface.host_mut().expect("a match");
    assert!(
        host.file_damage(pharmakos_sim::world::DamageOrder {
            target: pharmakos_sim::world::DamageTarget::Structure(spire),
            amount: pharmakos_sim::math::quantity::Damage::new(i64::from(i32::MAX))
                .expect("a positive amount"),
            by: SeatId::NEUTRAL,
            source: None,
        })
        .is_ok()
    );
    step(&mut surface, 2);
    assert_eq!(
        radius_of(&seat0.live(&mut surface), "b_00"),
        Some(base.clone())
    );
    assert_eq!(
        listed_radii(&mut surface, &zero)
            .into_iter()
            .find(|(id, _)| id == "b_00")
            .map(|(_, radius)| radius),
        Some(base)
    );
}

/// The wall target of seat 0's core, once its row has committed: its box's
/// lowest and highest columns and its top.
fn wall_box(surface: &Surface) -> Option<([i16; 2], [i16; 2], i16)> {
    let world = surface.host().expect("a match").world();
    let targets = world.targets();
    (0..targets.row_count()).find_map(|row| {
        let top = *targets.wall_tops().get(row)?;
        let area = *targets.areas().get(row)?;
        (top != pharmakos_sim::tables::NO_WALL).then(|| (area.min_column(), area.max_column(), top))
    })
}

/// Every wall voxel of seat 0 standing in the box.
fn wall_voxels(surface: &Surface) -> Vec<[i32; 3]> {
    let Some((low, high, top)) = wall_box(surface) else {
        return Vec::new();
    };
    let voxels = surface.host().expect("a match").world().voxels();
    let mut out = Vec::new();
    for y in i32::from(low[1])..=i32::from(high[1]) {
        for x in i32::from(low[0])..=i32::from(high[0]) {
            for z in 0..=i32::from(top) {
                if voxels
                    .get([x, y, z])
                    .and_then(Material::wall_of)
                    .is_some_and(|(seat, _)| seat == SeatId::new(0))
                {
                    out.push([x, y, z]);
                }
            }
        }
    }
    out
}

/// Whether seat 0's wall has been filled whole once.
fn wall_done(surface: &Surface) -> bool {
    let world = surface.host().expect("a match").world();
    let targets = world.targets();
    (0..targets.row_count()).any(|row| {
        targets
            .wall_tops()
            .get(row)
            .is_some_and(|top| *top != pharmakos_sim::tables::NO_WALL)
            && targets.walls_done().get(row).copied() == Some(true)
    })
}

/// S3's `probw`, item 5: a wall is listed as a `structure`/`wall` entity from
/// its first standing voxel, under one handle on every page, at its box's
/// lowest corner and lowest standing voxel, with a slab look its box's size
/// and no hit points; another seat's wall only while one of its voxels is in
/// that seat's vision; a no-fog spectator sees it; reading it moves no hashed
/// state; and it goes when its last voxel does.
#[test]
fn a_wall_is_listed_while_a_voxel_of_it_stands() {
    let mut surface = base_kit(FogPolicy::fogged(), false);
    let mut own = Watcher::new(seat_token(&mut surface, 0));
    let mut nofog = Watcher::new(spectator_token(&mut surface, true));
    assert!(wall_voxels(&surface).is_empty());
    assert_eq!(
        find(&own.live(&mut surface), "wall", "seat.0"),
        None,
        "nothing stands yet"
    );

    let mut handle: Option<String> = None;
    for _ in 0..600 {
        step(&mut surface, 2);
        let standing = wall_voxels(&surface);
        let entities = own.live(&mut surface);
        let wall = find(&entities, "wall", "seat.0");
        if standing.is_empty() {
            assert_eq!(wall, None, "no voxel stands");
            continue;
        }
        let wall = wall.expect("a wall with a voxel standing is listed");
        let (low, high, top) = wall_box(&surface).expect("the playbook's wall target");
        let lowest = standing.iter().map(|at| at[2]).min().expect("one voxel");
        assert_eq!(text_of(wall, "kind"), "structure");
        assert_eq!(text_of(wall, "title"), "Wall");
        assert_eq!(wall.get("hp"), None, "a wall has no hit points of its own");
        assert_eq!(wall.get("building"), None);
        let at = wall.get("at").expect("a place");
        assert_eq!(
            [number(at, "x"), number(at, "y"), number(at, "z")],
            [i32::from(low[0]), i32::from(low[1]), lowest]
        );
        let look = wall.get("look").expect("a look");
        assert_eq!(text_of(look, "shape"), "slab");
        assert_eq!(
            [number(look, "x"), number(look, "y"), number(look, "z")],
            [
                i32::from(high[0]) - i32::from(low[0]) + 1,
                i32::from(high[1]) - i32::from(low[1]) + 1,
                i32::from(top) - lowest + 1
            ]
        );
        let id = text_of(wall, "id");
        assert!(id.starts_with("l_"), "{id}");
        assert_eq!(handle.get_or_insert_with(|| id.clone()), &id, "one handle");
        if wall_done(&surface) {
            break;
        }
    }
    assert!(wall_done(&surface), "the wall is filled");
    assert!(handle.is_some(), "the wall was listed");
    assert!(find(&nofog.live(&mut surface), "wall", "seat.0").is_some());

    // Seat 1, a map away: absent through its live vision and through a
    // vision that sees none of it, listed through one that sees one voxel.
    let mut theirs = Watcher::new(seat_token(&mut surface, 1));
    assert_eq!(find(&theirs.live(&mut surface), "wall", "seat.0"), None);
    let mut blind = pharmakos_gateway::fog::KnownVoxels::new();
    blind.see(SeatId::new(1), &Voxel { x: 0, y: 0, z: 0 });
    assert_eq!(
        find(&theirs.entities(&mut surface, &blind), "wall", "seat.0"),
        None
    );
    let voxel = *wall_voxels(&surface).last().expect("a voxel");
    let mut one = pharmakos_gateway::fog::KnownVoxels::new();
    one.see(
        SeatId::new(1),
        &Voxel {
            x: voxel[0],
            y: voxel[1],
            z: voxel[2],
        },
    );
    let before = surface.host().expect("a match").world().state_hash();
    let entities = theirs.entities(&mut surface, &one);
    assert_eq!(
        surface.host().expect("a match").world().state_hash(),
        before,
        "reading a wall writes nothing"
    );
    let seen = find(&entities, "wall", "seat.0").expect("one voxel in sight lists it");
    let ours = find(&own.live(&mut surface), "wall", "seat.0")
        .cloned()
        .expect("ours");
    assert_eq!(
        seen.get("at"),
        ours.get("at"),
        "its place is not what of it is seen"
    );
    assert_eq!(seen.get("look"), ours.get("look"));
    assert_eq!(text_of(seen, "id"), "l_1", "seat 1's own count of walls");

    // Its last voxel goes: so does the wall.
    for at in wall_voxels(&surface) {
        erase(&mut surface, at);
    }
    step(&mut surface, 2);
    assert!(wall_voxels(&surface).is_empty(), "nothing rebuilds it");
    assert_eq!(find(&own.live(&mut surface), "wall", "seat.0"), None);
}

/// The standing point of the column `(x, y)`: one voxel above its top solid
/// voxel, where the sim stands a structure.
fn standing_point(
    world: &pharmakos_sim::world::World,
    x: i32,
    y: i32,
) -> [pharmakos_sim::math::fixed::Fx; 3] {
    use pharmakos_sim::math::fixed::Fx;
    let z = world.voxels().top_solid_z(x, y).expect("solid ground");
    let voxel = |value: i32| Fx::from_voxels(i16::try_from(value).expect("on the map"));
    [voxel(x), voxel(y), voxel(z + 1)]
}

/// Seat 0 seals `text` in the current Lull, and the Push begins.
fn seal_and_push(surface: &mut Surface, token: &Token, remaining: i32, text: &str) {
    let mut remaining = remaining;
    let answer = result(
        &call_in_lull(
            surface,
            token,
            &mut remaining,
            "submit_plan",
            &format!("{{\"playbook_jsonc\":{}}}", support::quote(text)),
        ),
        "submit_plan",
    );
    assert_eq!(
        answer.get("accepted"),
        Some(&Json::Bool(true)),
        "{answer:?}"
    );
    assert!(surface.begin_push().expect("a hosted match"), "the Push");
}

/// The world of [`built_scene`] in its opening Lull: a finished Resonance
/// Spire beside seat 0's core and a Survey post of seat 1's inside seat 0's
/// sphere.
fn built_world() -> Surface {
    use pharmakos_gateway::host::Host;
    let config = pharmakos_sim::world::WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules: support::rules(),
        match_settings: pharmakos_sim::runner::MatchSettings {
            segment_lengths_ms: vec![KIT_SEGMENT_MS],
            round_limit: 2,
        },
    };
    let mut world = pharmakos_sim::world::World::new(&config).expect("the world generates");
    let core_of = |world: &pharmakos_sim::world::World, seat: u8| {
        let row = (0..world.beacons().ids().len())
            .find(|row| world.beacons().seats().get(*row) == Some(&seat))
            .expect("a core");
        let id = pharmakos_sim::tables::BeaconId::new(
            world.beacons().ids().get(row).copied().expect("an id"),
        );
        let at = world
            .beacons()
            .positions()
            .get(row)
            .copied()
            .expect("a place");
        (id, at[0].floor_voxels(), at[1].floor_voxels())
    };
    let (ours, x, y) = core_of(&world, 0);
    let (theirs, _, _) = core_of(&world, 1);
    let spire_at = standing_point(&world, x + 1, y);
    world
        .raise_structure(
            SeatId::new(0),
            ours,
            pharmakos_sim::tables::StructureKind::ResonanceSpire,
            spire_at,
        )
        .expect("room for a Spire");
    let post_at = standing_point(&world, x - 4, y);
    world
        .raise_structure(
            SeatId::new(1),
            theirs,
            pharmakos_sim::tables::StructureKind::SurveyPost,
            post_at,
        )
        .expect("room for a Survey post");
    let bytes = pharmakos_sim::snapshot::Snapshot::capture(&world)
        .expect("a world between ticks")
        .to_bytes()
        .expect("it encodes");
    let mut host = Host::open(&config, Some(support::library_folder())).expect("a match");
    host.resume(&bytes).expect("the world resumes");
    let mut surface = Surface::new(
        MATCH,
        SEED,
        support::rules(),
        FogPolicy::fogged(),
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a match id");
    surface.attach(host).expect("attached");
    surface
        .set_phase_remaining_ms(pharmakos_sim::math::quantity::Ms::new(FIRST_LULL_MS))
        .expect("a countdown");
    surface.open_lull().expect("the opening Lull");

    surface
}

/// The scene of `the_view_built_golden`, in round 2's Push, and seat 0's
/// token.
///
/// The world opens with a finished Resonance Spire beside seat 0's core (its
/// ring grown) and a Survey post of seat 1's inside seat 0's sphere. Round 1
/// plays `scenarios/s2/base-kit-at-the-core`: the autocannon is raised, then
/// the wall filled (a Build beacon raises its structures before it fills a
/// wall). Round 2 seals the same settings with a mortar added, so the mortar
/// goes up beside the wall that stands.
fn built_scene() -> (Surface, Token) {
    let mut surface = built_world();
    let token = seat_token(&mut surface, 0);
    let scenario =
        fs::read_to_string(workspace_root().join(BASE_KIT)).expect("the scenario playbook");
    seal_and_push(&mut surface, &token, FIRST_LULL_MS, &scenario);
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
    assert!(!wall_voxels(&surface).is_empty(), "round 1 filled the wall");
    assert!(surface.end_recap().expect("the recap ends"));
    surface
        .set_phase_remaining_ms(pharmakos_sim::math::quantity::Ms::new(support::LULL_MS))
        .expect("a countdown");
    surface.open_lull().expect("round 2's Lull");

    let ground = surface
        .host()
        .expect("a match")
        .world()
        .voxels()
        .standing_z(MORTAR_SITE[0], MORTAR_SITE[1]);
    let cannon =
        r#"{"blueprint_id": "autocannon", "anchor": {"voxel": {"x": 362, "y": 30, "z": 36}}},"#;
    let mortar = format!(
        r#"{{"blueprint_id": "mortar", "anchor": {{"voxel": {{"x": {}, "y": {}, "z": {ground}}}}}, "order": 2}},"#,
        MORTAR_SITE[0], MORTAR_SITE[1]
    );
    let text = scenario.replace(
        cannon,
        &format!(
            "{cannon}
              {mortar}"
        ),
    );
    assert_ne!(text, scenario, "the scenario names its autocannon");
    seal_and_push(&mut surface, &token, support::LULL_MS, &text);
    for _ in 0..1_200 {
        step(&mut surface, 1);
        if building(&surface, pharmakos_sim::tables::StructureKind::Mortar) == Some(true) {
            assert!(!wall_voxels(&surface).is_empty(), "the wall still stands");
            return (surface, token);
        }
    }
    panic!("the mortar never went up");
}

/// S3's `probw`, item 8's `view_built`: seat 0's keyframe, one served
/// `GetViewResponse` per line as `view_keyframe` is, on [`built_scene`]: a
/// grown ring, a standing Resonance Spire and autocannon, a mortar going up,
/// an own wall with voxels standing, and a Survey post of seat 1's in seat 0's
/// live vision. Every field `probw` added, on the scene `prob` and `fixq`
/// draw from.
#[test]
fn the_view_built_golden() {
    let (mut surface, token) = built_scene();
    let pages = view_pages(&mut surface, &token, "");
    let last = pages.last().expect("a page");
    let entities = array_of(last, "entities");
    let core = entities
        .iter()
        .find(|entity| text_of(entity, "id") == "b_00")
        .expect("the core");
    assert_eq!(
        core.get("sphere_radius_voxels"),
        Some(&Json::Number(
            support::rules().grown_sphere_radius_voxels().to_string()
        )),
        "the grown ring"
    );
    for (subtype, owner, title) in [
        ("resonance_spire", "seat.0", "Resonance Spire"),
        ("autocannon", "seat.0", "Autocannon"),
        ("mortar", "seat.0", "Mortar, going up"),
        ("wall", "seat.0", "Wall"),
        ("survey_post", "seat.1", "Survey post"),
    ] {
        let entity = find(&entities, subtype, owner)
            .unwrap_or_else(|| panic!("{owner}'s {subtype} is in the view"));
        assert_eq!(text_of(entity, "title"), title);
    }
    let mut out = String::new();
    for page in &pages {
        decodes(page);
        row(&mut out, &one_line(page));
    }
    check("view_built", "keyframe.jsonl", &out);
}
