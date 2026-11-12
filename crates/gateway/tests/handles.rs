// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A seat's `e_NN` handles across a resume (decisions-log item 152 (1) 4; S3's
//! plan, E1).
//!
//! A seat names another seat's beacon by a handle of its own, `e_01`, `e_02`,
//! ..., minted where a Lull opens ([`Surface::open_lull`]'s
//! `name_sightings`), and an Attack target written with one is pinned at the
//! seal to the beacon the handle names then. A handle, once minted, names its
//! beacon for the rest of the match. So a match resumed from a save must name
//! every handle as the match that never stopped does -- including a handle
//! minted at an **earlier** Lull than the one the save was taken at, and the
//! handle an Attack written in an earlier round is pinned with when its
//! playbook is sealed again.
//!
//! The recipe, on the golden seed: in round 1 seat 0's Survey scout finds
//! seat 1's outpost (beacon 3) and not seat 1's core (beacon 1), so round 2's
//! Lull names the outpost `e_01`; in round 2 a second Survey scout of seat 0's
//! finds seat 1's core, so round 3's Lull names it `e_02`. The save is taken
//! at round 3's Lull and resumed through the file's own text, as
//! `tests/save.rs` does. The sighting table is ordered by asset id, so a
//! resume that rebuilds the names from it alone names the core `e_01`.
//!
//! The Attack half is asserted through the handle it would be pinned with
//! ([`Surface`]'s `plans_for_round` pins an `e_NN` target through
//! `ViewFeed::foreign_beacon_id`, the reader this test reads): an Attack aimed
//! at `e_01` cannot be submitted yet, because the verifier's view holds no
//! enemy beacon until S3's `gwr` fills it (the register's S3-04), and
//! `submit_plan` refuses it with `E0401`.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::{FogPolicy, Viewer};
use pharmakos_gateway::host::{Host, Settings};
use pharmakos_gateway::save::{Continuation, Save, SavedMatch, Stamp};
use pharmakos_gateway::serve::Config;
use pharmakos_gateway::surface::Surface;
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::json::Json;
use pharmakos_sim::tables::SeatId;

use support::{
    FIRST_LULL_MS, SEED, call_in_lull, hosted_with, quote, result, rules, rules_json, seat_token,
    workspace_root,
};

/// Five minutes a round: long enough for a Survey beacon to be placed, field
/// its scout, and for the scout to walk the length of the map and look.
const SEGMENT_MS: i32 = 300_000;

/// Three rounds; the save is taken at round 3's Lull.
const ROUNDS: u32 = 3;

/// Both seats of a two-seat match.
const SEATS: [SeatId; 2] = [SeatId::new(0), SeatId::new(1)];

/// The most handles a seat is asked about: far more than this match mints.
const MOST_HANDLES: u32 = 32;

/// Round 1, seat 0: a Survey beacon whose probe area sits beside seat 1's
/// outpost and just outside the reach of seat 1's core
/// (`survey-finds-the-enemy`'s).
const ROUND_1_SEAT_0: &str = "scenarios/s2/survey-finds-the-enemy.playbook.jsonc";

/// Round 1, seat 1: an outpost beacon inside its core's sphere.
const ROUND_1_SEAT_1: &str = "scenarios/s2/survey-finds-the-enemy-seat1.playbook.jsonc";

/// Round 2, seat 0: a second Survey beacon whose probe area sits within its
/// scout's sight of seat 1's core.
const ROUND_2_SEAT_0: &str = r#"// Look at the enemy core.
{"schema_version": {"major": 1, "minor": 0},
 "meta": {"title": "Look", "author_kind": "HUMAN"},
 "kind": "PLAYBOOK",
 "declarative": {"route": [
   {"label": "look", "timeout_ms": 60000,
    "place_beacon": {"at": {"voxel": {"x": 350, "y": 30, "z": 36}},
      "initial": {"priority": "NORMAL", "mandate": {"survey": {
        "probe_areas": [{"min": {"x": 357, "y": 340, "z": 0}, "max": {"x": 357, "y": 342, "z": 63}}],
        "scout_count": 1}}}},
    "on_fail": {"action": "ABORT_ROUTE"}}
 ], "handlers": []},
 "on_death": {"on_respawn": "CONTINUE", "max_deaths_before_fallback": 2},
 "fallback": {"hold": {"at": {"safest": {}}}}}
"#;

/// The config line the match was hosted with, as a save carries it.
fn config_of(match_id: &str) -> Config {
    Config {
        match_id: match_id.to_owned(),
        seed: SEED,
        seats: 2,
        human_seat: None,
        segment_lengths_ms: vec![SEGMENT_MS],
        round_limit: ROUNDS,
        preset: pharmakos_gateway::probation::Preset::Custom,
    }
}

/// A saved match as its file would carry it: rendered, read back and checked
/// against the config line a resume would repeat (`tests/save.rs`'s).
fn through_the_file(match_id: &str, saved: &SavedMatch) -> SavedMatch {
    let config = config_of(match_id);
    let hash = rules().rules_hash();
    let text = Save {
        stamp: Stamp::current(hash),
        config: config.clone(),
        state: saved.clone(),
    }
    .render();
    let save = Save::parse(&text).expect("a save reads back");
    save.check(&config, hash)
        .expect("the same rules, the same verifier, the same match");
    assert_eq!(&save.state, saved, "the file carries all of it");
    save.state
}

/// A fresh host from the saved config line: the pristine world a resume
/// regenerates.
fn pristine() -> Host {
    Host::open_from(
        &rules_json(),
        SEED,
        2,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: ROUNDS,
        },
        None,
    )
    .expect("a match")
}

/// A generation the host loop could pass, and no tick floor (`tests/save.rs`'s).
const CONTINUATION: Continuation = Continuation {
    generation: 1,
    last_tick: 0,
};

/// Resume a saved match on a new surface.
fn resume(match_id: &str, saved: &SavedMatch) -> Surface {
    let state = through_the_file(match_id, saved);
    Surface::resume(
        match_id,
        SEED,
        FogPolicy::fogged(),
        &SEATS,
        pristine(),
        &state,
        CONTINUATION,
    )
    .expect("a Lull resumes")
}

/// A committed playbook's text.
fn committed(path: &str) -> String {
    std::fs::read_to_string(workspace_root().join(path))
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Submit `playbook` for `seat` through `submit_plan`, like a client.
fn submit(surface: &mut Surface, seat: u8, playbook: &str) {
    let mut left = FIRST_LULL_MS;
    let token = seat_token(surface, seat);
    let answer = result(
        &call_in_lull(
            surface,
            &token,
            &mut left,
            "submit_plan",
            &format!("{{\"playbook_jsonc\":{}}}", quote(playbook)),
        ),
        "submit_plan",
    );
    assert_eq!(
        answer.get("accepted"),
        Some(&Json::Bool(true)),
        "seat {seat}'s playbook: {answer:?}"
    );
}

/// Begin the Push, play it to its end, close the recap and open the next
/// Lull.
///
/// Returns how many `sighting_refreshed` lines the segment's feed carried, and
/// holds the feed to item 155 (3) 6's thinning: at most one such line a seat a
/// tick, however many sightings its Survey refreshed in it.
fn play_round(surface: &mut Surface) -> usize {
    assert!(surface.begin_push().expect("the Push begins"));
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
    let mut refreshes: Vec<(i32, String)> = surface
        .feed()
        .events()
        .iter()
        .filter(|event| event.kind.name() == "sighting_refreshed")
        .map(|event| (event.at_ms.raw(), format!("{:?}", event.audience)))
        .collect();
    let lines = refreshes.len();
    refreshes.sort();
    refreshes.dedup();
    assert_eq!(
        refreshes.len(),
        lines,
        "one refresh line a seat a tick (item 155 (3) 6)"
    );
    assert!(surface.end_recap().expect("the recap ends"));
    assert_eq!(surface.time().phase, Phase::Lull, "the match goes on");
    surface.open_lull().expect("the next Lull");
    lines
}

/// Every handle `seat` holds, `e_01` up, with the beacon id each names.
fn handles(surface: &Surface, seat: SeatId) -> Vec<(String, u32)> {
    (1..=MOST_HANDLES)
        .map(pharmakos_sim::tables::foreign_beacon_name)
        .filter_map(|name| {
            surface
                .views()
                .foreign_beacon_id(Viewer::Seat(seat), &name)
                .map(|id| (name, id))
        })
        .collect()
}

/// `seat`'s core: its lowest beacon id, placed by the map generator.
fn core_of(surface: &Surface, seat: SeatId) -> u32 {
    let beacons = surface.host().expect("a hosted match").world().beacons();
    (0..beacons.ids().len())
        .filter(|row| beacons.seats().get(*row).copied() == Some(seat.raw()))
        .filter_map(|row| beacons.ids().get(row).copied())
        .min()
        .expect("every seat starts with its core")
}

/// A match resumed at round 3's Lull names every `e_NN` handle as the match
/// that never stopped does, the handle an Attack aimed in round 2 is pinned
/// with included.
#[test]
fn a_resumed_lull_names_every_handle_as_the_match_that_never_stopped() {
    const MATCH_ID: &str = "m-gwp-handles";
    let mut surface = hosted_with(MATCH_ID, FogPolicy::fogged(), 2, SEGMENT_MS, ROUNDS, None);
    let seat_0 = SeatId::new(0);
    let core_1 = core_of(&surface, SeatId::new(1));
    let mut refreshed = 0_usize;

    // Round 1: seat 0's scout finds seat 1's outpost, and only it.
    submit(&mut surface, 0, &committed(ROUND_1_SEAT_0));
    submit(&mut surface, 1, &committed(ROUND_1_SEAT_1));
    refreshed = refreshed.saturating_add(play_round(&mut surface));

    // Round 2's Lull: the outpost is `e_01`. Seat 1 submits nothing from here
    // on and plays its safe playbook.
    let round_2 = handles(&surface, seat_0);
    let Some((_, outpost)) = round_2.first().cloned() else {
        panic!("the recipe: seat 0's scout finds something in round 1, and it found nothing");
    };
    assert_eq!(
        round_2,
        vec![(String::from("e_01"), outpost)],
        "the recipe: round 2's Lull names one beacon of seat 1's"
    );
    assert_ne!(
        outpost, core_1,
        "the recipe: round 1 finds the outpost and not the core"
    );
    submit(&mut surface, 0, ROUND_2_SEAT_0);
    refreshed = refreshed.saturating_add(play_round(&mut surface));

    // Round 3's Lull: the core, sighted in round 2, has a handle of its own,
    // after the outpost's, and the outpost keeps `e_01`.
    let before: Vec<Vec<(String, u32)>> =
        SEATS.iter().map(|seat| handles(&surface, *seat)).collect();
    let seat_0_before = before.first().cloned().unwrap_or_default();
    assert_eq!(
        seat_0_before,
        vec![
            (String::from("e_01"), outpost),
            (String::from("e_02"), core_1)
        ],
        "the recipe: round 2 finds seat 1's core, and the match that never stopped renames nothing"
    );

    assert!(
        refreshed > 0,
        "the recipe: seat 0's scouts refresh sightings, so the thinning is exercised"
    );
    let saved = surface.lull_save().expect("a Lull saves");
    let resumed = resume(MATCH_ID, &saved);
    assert_eq!(resumed.time().phase, Phase::Lull);
    assert_eq!(resumed.time().round, 3);

    for (seat, held) in SEATS.iter().zip(&before) {
        let after = handles(&resumed, *seat);
        for (name, id) in held {
            let again = resumed.views().foreign_beacon_id(Viewer::Seat(*seat), name);
            assert_eq!(
                again,
                Some(*id),
                "seat {}'s {name} names beacon {id} in the match that never stopped and \
                 {again:?} in the one resumed at round 3's Lull ({held:?} before, {after:?} \
                 after)",
                seat.raw()
            );
        }
        assert_eq!(
            &after,
            held,
            "seat {} holds the same handles after the resume",
            seat.raw()
        );
    }

    // An Attack written against `e_01` in round 2's Lull, sealed again in
    // round 3's, is pinned through this reader (`plans_for_round`): it must
    // hit the outpost it was aimed at, not the core.
    assert_eq!(
        resumed
            .views()
            .foreign_beacon_id(Viewer::Seat(seat_0), "e_01"),
        Some(outpost),
        "an Attack aimed at seat 0's `e_01` in round 2 is pinned to beacon {outpost} in the match \
         that never stopped, and must be after a resume at round 3's Lull too"
    );
}
