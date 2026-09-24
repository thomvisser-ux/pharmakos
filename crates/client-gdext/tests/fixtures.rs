// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The committed `gp.api.v1` fixture responses the bridge is unit-tested against.
//!
//! Skeleton plan T12, Needs: "The bridge is unit-tested against committed `gp.api.v1`
//! fixture responses, which is marshalling and needs no live server; it meets the real
//! gateway at T16."
//!
//! # Generated, not typed
//!
//! Every fixture under `tests/fixtures/` is produced by **building the message and
//! encoding it with `pharmakos-proto`'s canonical codec**, never by hand. A hand-typed
//! fixture is a second opinion about the wire format, and the whole point of a fixture is
//! that it is the first one. [`generate`] below builds them; `--bless` — here, the
//! environment variable `PHARMAKOS_BLESS_FIXTURES` — rewrites them, and the default run
//! compares what is committed against a fresh generation and fails on a difference.
//!
//! So a fixture moving means the schema or the codec moved, and the pull request says
//! which. That is the same contract a golden carries, applied to a crate's own test data.
//!
//! These are **not** `tests/golden/` files: they are this crate's fixtures, they are read
//! only by this crate, and `cargo xtask ci`'s `golden` step does not compare them. What
//! it would add is a second place to keep in step; what this file gives instead is the
//! same guarantee, enforced where the data lives.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::fs;
use std::path::{Path, PathBuf};

use pharmakos_client_gdext::api;
use pharmakos_proto::gp::api::v1::{
    BeaconSummary, Digest, Event, GetBriefingResponse, GetSegmentFeedResponse, GetStatusResponse,
    KindCount, ListBeaconsResponse, Standing, Status, status,
};
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json;

/// Set this to rewrite the committed fixtures from a fresh generation.
const BLESS: &str = "PHARMAKOS_BLESS_FIXTURES";

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// The fixtures, as `(method wire name, file name, canonical JSON)`.
///
/// Four, and each is there for a shape rather than for a method:
///
/// * `get_status` — the `_status` footer's own message: a scalar enum, three integers;
/// * `get_briefing` — a nested message and a string, the notebook at the top as spec
///   section 12 asks;
/// * `list_beacons` — a repeated message with a nested `gp.v1` type inside it, and an
///   empty `next_cursor`, which canonical JSON omits rather than writing as `""`;
/// * `get_segment_feed` — the live event list's source (T16), whose rows the event-list
///   golden pins.
fn generate() -> Vec<(&'static str, &'static str, String)> {
    let status = GetStatusResponse {
        status: Some(Status {
            phase: status::Phase::Lull.into(),
            phase_remaining_ms: 90_000,
            segment_length_ms: 180_000,
            round: 1,
        }),
    };
    let briefing = GetBriefingResponse {
        notes: "vent to the east; two drones idle at the core".to_owned(),
        standing: Some(Standing {
            rank: 1,
            score: 1_450,
            living_seats: 2,
        }),
        segment_length_ms: 180_000,
        prose: "Round 1. You hold the core and one Generator.".to_owned(),
    };
    // `gp.api.v1.BeaconSummary` gains fields in wave 6 (decisions-log item 111), and
    // canonical JSON omits a field at its default, so this fixture's bytes do not move
    // when they land; the base keeps the literals compiling across that additive change.
    #[allow(
        clippy::needless_update,
        reason = "the base is for the fields gp.api.v1.BeaconSummary gains in wave 6"
    )]
    let beacons = ListBeaconsResponse {
        beacons: vec![
            BeaconSummary {
                beacon_id: "core".to_owned(),
                at: Some(Voxel {
                    x: 96,
                    y: 96,
                    z: 24,
                }),
                ..BeaconSummary::default()
            },
            BeaconSummary {
                beacon_id: "vent-east".to_owned(),
                at: Some(Voxel {
                    x: 128,
                    y: 90,
                    z: 22,
                }),
                ..BeaconSummary::default()
            },
        ],
        next_cursor: String::new(),
    };

    vec![
        (
            "get_status",
            "get_status.json",
            json::encode(&status).expect("GetStatusResponse encodes"),
        ),
        (
            "get_briefing",
            "get_briefing.json",
            json::encode(&briefing).expect("GetBriefingResponse encodes"),
        ),
        (
            "list_beacons",
            "list_beacons.json",
            json::encode(&beacons).expect("ListBeaconsResponse encodes"),
        ),
        (
            "get_segment_feed",
            "get_segment_feed.json",
            json::encode(&feed_fixture()).expect("GetSegmentFeedResponse encodes"),
        ),
    ]
}

/// One Push's worth of the segment feed, in the kinds and the words the gateway's own
/// string table writes (`pharmakos_gateway::strings::event_text`), with a 60-second digest
/// beside it. The event list's golden (`tests/golden/vista/expected.events.txt`) is
/// rendered from this fixture by `tests/event_list.rs`.
fn feed_fixture() -> GetSegmentFeedResponse {
    let event = |at_ms: i32, kind: &str, text: &str| Event {
        at_ms,
        kind: kind.to_owned(),
        text: text.to_owned(),
        at: None,
    };
    GetSegmentFeedResponse {
        events: vec![
            event(0, "push_started", "The Push began. This segment runs 3:00."),
            event(
                0,
                "plan_sealed",
                "The playbook of seat 0 was sealed: 3 route steps.",
            ),
            event(250, "step_started", "Step 0 started."),
            event(14_250, "step_completed", "Step 0 completed."),
            event(14_250, "step_started", "Step 1 started."),
            event(31_500, "beacon_placed", "A beacon of seat 0 was deployed."),
            event(
                47_750,
                "ore_delivered",
                "A mining drone of seat 0 delivered ore worth $ 40.",
            ),
            event(
                63_000,
                "structure_queued",
                "seat 0 paid for a structure; it is going up now.",
            ),
        ],
        digests: vec![Digest {
            from_ms: 0,
            to_ms: 60_000,
            text: "0:00-1:00: 7 events.".to_owned(),
            counts: vec![KindCount {
                kind: "step_started".to_owned(),
                count: 2,
            }],
        }],
        next_cursor: "a730c5c72d863a51".to_owned(),
    }
}

/// Fixture files are read on three operating systems, so they follow the same rule the
/// goldens do: LF endings, a trailing newline, no carriage return.
fn with_trailing_newline(text: &str) -> String {
    let mut out = text.replace("\r\n", "\n").replace('\r', "");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[test]
fn the_committed_fixtures_match_a_fresh_generation() {
    let dir = fixtures_dir();
    fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("creating {}: {error}", dir.display()));
    let bless = std::env::var_os(BLESS).is_some();

    for (_, file, fresh) in generate() {
        let path = dir.join(file);
        let fresh = with_trailing_newline(&fresh);
        if bless {
            fs::write(&path, fresh.as_bytes())
                .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
            continue;
        }
        let committed = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "{} is missing ({error}). Fixtures are generated through the codec, never typed: \
                 run the tests with {BLESS}=1 to write them.",
                path.display()
            )
        });
        assert_eq!(
            committed,
            fresh,
            "{} no longer matches what the codec produces. A fixture that moves means the \
             schema or the canonical form moved; say which in the pull request, then \
             re-generate with {BLESS}=1.",
            path.display()
        );
    }
}

#[test]
fn every_fixture_decodes_through_the_bridge_for_its_own_method() {
    for (method, file, _) in generate() {
        let path = fixtures_dir().join(file);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        let decoded = api::decode_result(method, &text)
            .unwrap_or_else(|error| panic!("{file} did not decode as {method}: {error}"));
        // Canonical in means canonical out: the bridge changes nothing on the way past.
        assert_eq!(
            with_trailing_newline(&json::write(&decoded)),
            text,
            "{file} did not survive the bridge unchanged"
        );
    }
}

#[test]
fn a_fixture_offered_for_the_wrong_method_is_refused() {
    // `get_briefing`'s body is not a `GetStatusResponse`, and the bridge finds that out
    // from the schema rather than from a shape it guessed at.
    let path = fixtures_dir().join("get_briefing.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    let error = api::decode_result("get_status", &text)
        .expect_err("a briefing is not a status, and the schema says so");
    assert!(
        format!("{error}").contains("notes") || format!("{error}").contains("standing"),
        "the error should name the field that does not belong: {error}"
    );
}

#[test]
fn the_fixtures_have_no_carriage_returns() {
    for (_, file, _) in generate() {
        let path = fixtures_dir().join(file);
        let bytes =
            fs::read(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        assert!(
            !bytes.contains(&b'\r'),
            "{} carries a carriage return; fixtures are read on three operating systems",
            path.display()
        );
        assert_eq!(
            bytes.last(),
            Some(&b'\n'),
            "{} must end with a newline",
            path.display()
        );
    }
}
