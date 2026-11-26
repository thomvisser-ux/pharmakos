// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The verifier's per-code cap (S3's `vcap`; decisions-log item 183 (4)) as the
//! crates that host and read a report meet it.
//!
//! * The operator holds no copy of the cap: its tap probe reads each report's
//!   typed `left_out` field (S3's `leftout`). Its call budget assumes a report
//!   lists at least `TAP_PROBE_LISTED_FLOOR` of a code; this crate links both
//!   and holds that floor, and the budget it sizes, to the verifier's own cap.
//! * Easy's tap probe through the real gateway and verifier: seat 0's nearest
//!   vent is ringed, in the lobby, by twelve of its own Mine beacons and one
//!   Build beacon, so more pairs are off the Build mandate than a report lists
//!   `E0503`s. A probe that read an `E0503` the cap left out as absent would
//!   weigh a tap at a Mine beacon; Easy weighs one at the Build beacon alone,
//!   and asks again from the first unanswered pair, on the report's word, to
//!   know it. (The set-aside hosted case of PR #179's review, `67897578`,
//!   re-fitted to the probe; it lives here, not in `operator.rs`, which
//!   another lane holds.)

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pharmakos_gamectl::host::LIBRARY_PATH;
use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, Settings, SphereVision};
use pharmakos_gateway::limit::IN_PROCESS_LIMITS;
use pharmakos_gateway::rpc::Request;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::{InProcess, Surface};
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_operator::Easy;
use pharmakos_operator::easy::{
    EASY_CALL_BUDGET, EASY_CANDIDATES, TAP_PROBE_LISTED_FLOOR, TAP_PROBE_VERIFIES,
};
use pharmakos_proto::json::Json;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;

const SEED: u64 = pharmakos_sim::DETERMINISM_MATCH_SEED;
const SEGMENT_MS: i32 = 80_000;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/gamectl sits two levels below the root")
        .to_path_buf()
}

fn rules_json() -> String {
    std::fs::read_to_string(root().join("rules").join("rules.v1.json")).expect("the rules text")
}

/// The probe's budget is enough at the verifier's own cap: a report lists at
/// least the floor the budget was sized at, so every one of Easy's candidate
/// pairs is answered within its verifies. Fails the day the cap drops below
/// the floor, which is when Easy's budget must be sized again. Held at
/// compile time: this test target does not build past a cap under the floor.
const _: () = assert!(
    pharmakos_verifier::MAX_PER_CODE.get() >= TAP_PROBE_LISTED_FLOOR,
    "the verifier lists fewer of a code than Easy's probe budget is sized at"
);
const _: () = assert!(
    EASY_CANDIDATES.div_ceil(pharmakos_verifier::MAX_PER_CODE.get()) <= TAP_PROBE_VERIFIES,
    "Easy's probe cannot answer every candidate pair at the verifier's cap"
);

/// The squared ground distance between two columns.
fn ground(a: [i32; 2], b: [i32; 2]) -> i64 {
    let (dx, dy) = (i64::from(a[0] - b[0]), i64::from(a[1] - b[1]));
    dx * dx + dy * dy
}

/// A one-seat match in its opening Lull whose seat 0 has its nearest vent
/// ringed by thirteen of its own beacons, laid out in the lobby: the ring's
/// columns, and the one Build beacon's.
fn ringed_vent(rules: &str) -> (Surface, Vec<[i32; 2]>, [i32; 2]) {
    use pharmakos_sim::features::FeatureKind;
    use pharmakos_sim::runner::{Layout, LayoutBeacon};
    use pharmakos_sim::seams::MandateKind;

    let mut host = Host::open_from(
        rules,
        SEED,
        1,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: 2,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let core = {
        let beacons = host.world().beacons();
        let row = beacons
            .ids()
            .iter()
            .zip(beacons.seats())
            .enumerate()
            .filter(|(_, (_, seat))| **seat == 0)
            .min_by_key(|(_, (id, _))| **id)
            .map(|(row, _)| row)
            .expect("seat 0 has a core");
        let at = beacons.positions().get(row).copied().expect("a position");
        [at[0].floor_voxels(), at[1].floor_voxels()]
    };
    let vent = host
        .world()
        .features()
        .features()
        .iter()
        .filter(|feature| feature.kind == FeatureKind::Vent)
        .min_by_key(|feature| (ground(feature.anchor, core), feature.anchor))
        .cloned()
        .expect("a vent");
    // Thirteen columns within 14 of the vent's anchor, off its footprint by
    // two, three apart: inside every one's sphere, and leaving its columns
    // free for a Generator.
    let [min_x, min_y, max_x, max_y] = vent.bounds;
    let mut columns: Vec<[i32; 2]> = Vec::new();
    for dy in -14..=14 {
        for dx in -14..=14 {
            let column = [vent.anchor[0] + dx, vent.anchor[1] + dy];
            let off_footprint = column[0] < min_x - 2
                || column[0] > max_x + 2
                || column[1] < min_y - 2
                || column[1] > max_y + 2;
            if columns.len() < 13
                && off_footprint
                && ground(column, vent.anchor) <= 14 * 14
                && columns.iter().all(|held| ground(*held, column) >= 9)
            {
                columns.push(column);
            }
        }
    }
    assert_eq!(columns.len(), 13, "room for the ring");
    let build_column = *columns.last().expect("thirteen");
    host.seed_layout(&Layout {
        beacons: columns
            .iter()
            .map(|column| LayoutBeacon {
                seat: SeatId::new(0),
                column: *column,
                mandate: if *column == build_column {
                    MandateKind::Build
                } else {
                    MandateKind::Mine
                },
                units: Vec::new(),
            })
            .collect(),
    })
    .expect("the ring is laid out");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let mut surface = Surface::new(
        &format!("vcap-tap-cap-{}", std::process::id()),
        SEED,
        table,
        FogPolicy::fogged(),
        &[SeatId::new(0)],
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    (surface, columns, build_column)
}

/// A seat's own token, registered as an in-process one.
fn in_process_token(surface: &mut Surface, seat: u8) -> Token {
    let tick = surface.time().tick;
    let held = surface.match_id().to_owned();
    let (token, handle) = surface
        .tokens()
        .mint(
            Subject::Seat(SeatId::new(seat)),
            &held,
            ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs]),
            tick,
        )
        .expect("minted");
    surface.register_in_process(
        handle,
        InProcess {
            limits: IN_PROCESS_LIMITS,
            scratch_view: true,
        },
    );
    token
}

/// One call through a token, with the production vision.
fn call(surface: &mut Surface, token: &Token, method: &str, params: Json) -> Json {
    let vision = surface.host().map_or_else(
        |_| SphereVision::default(),
        |host| SphereVision::of(host.world()),
    );
    surface.call(
        Some(token),
        &Request {
            id: Json::Number(String::from("1")),
            method: method.to_owned(),
            params,
        },
        &vision,
    )
}

fn number(value: &Json, key: &str) -> i64 {
    match value.get(key) {
        Some(Json::Number(lexeme)) => lexeme.parse().expect("a whole number"),
        other => panic!("{key} is not a whole number: {other:?}"),
    }
}

/// The beacon an `estimate_route` call ends on, if it ends on one.
fn beacon_target(params: &Json) -> Option<String> {
    let Some(Json::Array(waypoints)) = params.get("waypoints") else {
        return None;
    };
    match waypoints.last()?.get("beacon_anchor")?.get("beacon_id")? {
        Json::String(id) => Some(id.clone()),
        _ => None,
    }
}

#[test]
fn easy_taps_no_beacon_off_the_build_mandate_past_the_verifiers_cap() {
    let rules = rules_json();
    let (mut surface, columns, build_column) = ringed_vent(&rules);
    let token = in_process_token(&mut surface, 0);

    // The ring's ids, by where `list_beacons` says each stands.
    let listed = call(
        &mut surface,
        &token,
        "list_beacons",
        pharmakos_proto::json::read("{}").expect("params"),
    );
    let listed = listed
        .get("result")
        .cloned()
        .expect("list_beacons answered");
    let mut ring: Vec<(String, [i32; 2])> = Vec::new();
    if let Some(Json::Array(rows)) = listed.get("beacons") {
        for row in rows {
            let id = match row.get("beacon_id") {
                Some(Json::String(id)) => id.clone(),
                other => panic!("a beacon id: {other:?}"),
            };
            let at = row.get("at").expect("a place");
            let column = [
                i32::try_from(number(at, "x")).expect("an x"),
                i32::try_from(number(at, "y")).expect("a y"),
            ];
            if columns.contains(&column) {
                ring.push((id, column));
            }
        }
    }
    assert_eq!(ring.len(), 13, "the ring, listed: {listed:?}");
    let builds: BTreeSet<String> = ring
        .iter()
        .filter(|(_, column)| *column == build_column)
        .map(|(id, _)| id.clone())
        .collect();
    assert_eq!(builds.len(), 1);

    let mut taps: BTreeSet<String> = BTreeSet::new();
    let mut probes = 0_u32;
    // Each probe report's `E0503` rows and whether its `left_out` names it.
    let mut reads: Vec<(usize, bool)> = Vec::new();
    let played = {
        let mut bridged = |method: &str, params: Json| {
            if method == "estimate_route"
                && let Some(Json::Array(waypoints)) = params.get("waypoints")
                && waypoints.len() == 3
                && let Some(beacon) = beacon_target(&params)
            {
                taps.insert(beacon);
            }
            let probe = method == "verify_plan"
                && matches!(params.get("playbook_jsonc"), Some(Json::String(text)) if text.contains("Easy's tap probe"));
            let answer = call(&mut surface, &token, method, params);
            if probe {
                probes += 1;
                let report = answer.get("result").and_then(|result| result.get("report"));
                let e0503 = |key: &str| -> Vec<Json> {
                    match report.and_then(|report| report.get(key)) {
                        Some(Json::Array(rows)) => rows
                            .iter()
                            .filter(|row| matches!(row.get("code"), Some(Json::String(code)) if code == "E0503"))
                            .cloned()
                            .collect(),
                        _ => Vec::new(),
                    }
                };
                reads.push((e0503("diagnostics").len(), !e0503("left_out").is_empty()));
            }
            answer
        };
        Easy::new(&rules)
            .expect("the rules text")
            .play(0, &mut bridged)
    };
    assert!(played.calls <= EASY_CALL_BUDGET, "{played:#?}");
    let ring_ids: BTreeSet<String> = ring.iter().map(|(id, _)| id.clone()).collect();
    let tapped_in_ring: BTreeSet<String> = taps.intersection(&ring_ids).cloned().collect();
    assert_eq!(
        tapped_in_ring, builds,
        "a tap is weighed on the ring's Build beacon alone"
    );
    // The first report left `E0503`s out, on the field's word, and the probe
    // asked again; the last left none out; each that did listed the cap's
    // number, as the real verifier writes it.
    let cap = usize::try_from(pharmakos_verifier::MAX_PER_CODE.get()).expect("fits");
    assert!(
        probes >= 2,
        "the pairs were asked in {probes} verifies: {reads:?}"
    );
    assert!(probes <= pharmakos_operator::easy::TAP_PROBE_VERIFIES);
    assert!(
        reads.first().is_some_and(|(_, left_out)| *left_out),
        "the ring's first report left nothing out: {reads:?}"
    );
    assert!(
        reads.last().is_some_and(|(_, left_out)| !*left_out),
        "the probe stopped on a report that left an E0503 out: {reads:?}"
    );
    for (listed, left_out) in &reads {
        if *left_out {
            assert_eq!(*listed, cap, "{reads:?}");
        }
    }
}

/// **The probe says why it made no tap** (S3's `vcap`, fix pass 1): through
/// the real gateway, the ring's second probe part is asked with a depth the
/// gateway refuses, so the probe cannot say which taps are legal: Easy weighs
/// no tap, and `Played::taps_unread` names the refusal, never a silent "no
/// tap".
#[test]
fn a_refused_probe_part_is_kept_as_the_reason_no_tap_was_made() {
    let rules = rules_json();
    let (mut surface, _, _) = ringed_vent(&rules);
    let token = in_process_token(&mut surface, 0);
    let mut parts = 0_u32;
    let played = {
        let mut bridged = |method: &str, mut params: Json| {
            let probe = method == "verify_plan"
                && matches!(params.get("playbook_jsonc"), Some(Json::String(text)) if text.contains("Easy's tap probe"));
            if probe {
                parts += 1;
                if parts == 2
                    && let Json::Object(members) = &mut params
                {
                    for (key, value) in members.iter_mut() {
                        if key == "depth" {
                            *value = Json::String(String::from("sideways"));
                        }
                    }
                }
            }
            call(&mut surface, &token, method, params)
        };
        Easy::new(&rules)
            .expect("the rules text")
            .play(0, &mut bridged)
    };
    assert_eq!(parts, 2, "the probe stopped at the refused part");
    match &played.taps_unread {
        Some(pharmakos_operator::TapsUnread::Refused(refusal)) => {
            assert_eq!(refusal.method, "verify_plan", "{refusal:?}");
        }
        other => panic!("the refusal was not kept: {other:?}"),
    }
}
