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
use pharmakos_gateway::host::SphereVision;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Token;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::voxels::Material;
use std::collections::BTreeSet;
use std::fs;

use support::{
    LULL_MS, MATCH, SEED, SEGMENT_MS, admin_token, array_of, call, call_in_lull, chunks_of, code,
    core_beacon, erase, fell, hosted, hosted_as, hosted_casual, result, seat_token,
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

/// The boundary is pinned with the answer written out. Since T17 the
/// production vision is a call to the sim's own rule, `World::in_own_sphere`
/// (decisions-log item 107 (6)); `the_view_uses_the_sims_own_sphere_rule`
/// holds the two together, and this holds the numbers.
#[test]
fn the_boundary_voxel_of_a_sphere_is_seen_and_the_next_one_is_not() {
    let surface = hosted(2, SEGMENT_MS, 3);
    let radius = sphere_radius(&surface);
    assert_eq!(radius, 24, "rules.beacon.sphere_radius_voxels, item 90");

    let (_, centre) = core_beacon(&surface, 0);
    let sight = SphereVision::of(surface.host().expect("a match").world());
    assert!(sight.spheres() >= 2, "one core per seat");

    let at = |dx: i32| Voxel {
        x: centre[0].saturating_add(dx),
        y: centre[1],
        z: centre[2],
    };
    // Measured from the beacon's own voxel: 24 out is inside, 25 is not, and
    // the comparison is `<=` on squared distance with no square root anywhere
    // (AGENTS.md section 4.2).
    assert!(sight.sees(SeatId::new(0), &at(23)), "inside");
    assert!(sight.sees(SeatId::new(0), &at(24)), "the boundary voxel");
    assert!(!sight.sees(SeatId::new(0), &at(25)), "one voxel past it");
    assert!(
        !sight.sees(SeatId::new(1), &at(0)),
        "another seat's sphere is not this seat's"
    );
}

/// The view draws with the sim's own sphere rule and no copy of it (T17;
/// decisions-log items 107 (6) and 110 (5)): the production vision and
/// `World::in_own_sphere` -- the interpreter's `within` -- agree on every voxel
/// of a square around a beacon's sphere boundary, for the seat that owns it and
/// for the seat that does not, and the boundary voxel is where the sim says.
#[test]
fn the_view_uses_the_sims_own_sphere_rule() {
    let surface = hosted(2, SEGMENT_MS, 3);
    let world = surface.host().expect("a match").world();
    let radius = sphere_radius(&surface);
    let (_, centre) = core_beacon(&surface, 0);
    let sight = SphereVision::of(world);

    // The boundary, pinned through the sim: 24 out along an axis is inside
    // and 25 is not, measured from the beacon's own voxel.
    let along = |dx: i32| [centre[0].saturating_add(dx), centre[1], centre[2]];
    assert!(world.in_own_sphere(SeatId::new(0), along(radius)));
    assert!(!world.in_own_sphere(SeatId::new(0), along(radius.saturating_add(1))));

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
                        world.in_own_sphere(seat, at),
                        "seat {} at {at:?}",
                        seat.raw()
                    );
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
    let (own, _) = core_beacon(&surface, 0);
    let (other, _) = core_beacon(&surface, 1);

    let (page, _) = view(&mut surface, &token, "");
    let ids = entity_ids(&page);
    let owners: Vec<String> = array_of(&page, "entities")
        .iter()
        .map(|entity| text_of(entity, "owner"))
        .collect();

    assert!(
        ids.contains(&pharmakos_gateway::view::beacon_id(own)),
        "its own core: {ids:?}"
    );
    assert!(
        !ids.contains(&pharmakos_gateway::view::beacon_id(other)),
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
fn no_view_ever_carries_a_destination_a_step_a_mandate_or_a_hit_point() {
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
                     a treasury, a score, a hit point, a power state, a count."
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

    let mut remaining = LULL_MS;
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

    // Match end. The segment runs out, the recap plays, and closing it is what
    // ends a match the ROUND LIMIT decided -- `MatchState::close_recap` is
    // where `decide(RoundLimit)` happens, so the last tick reports
    // `match_ended: false` and only `Surface::end_recap` can lift the fog.
    step(&mut surface, 200);
    assert!(
        !surface.fog().ended(),
        "a segment ending is not a match ending"
    );
    assert!(surface.end_recap().expect("a match"), "the recap closes");
    assert!(surface.fog().ended(), "and closing it ended the match");
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
