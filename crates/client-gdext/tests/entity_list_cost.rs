// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! E28's client half (S2's plan; the register's S2-12): what a full entity list at the gate
//! load costs the client each time a view arrives, so the delta-mode question is decided by
//! a measurement and not a guess.
//!
//! `crates/gateway/tests/view.rs`' `the_entity_list_at_the_gate_load_is_measured_and_reported`
//! measures the gateway's half: about 65 KB of JSON for 310 entities on the golden seed, 211
//! bytes an entity. This test builds a list of the gate's world totals (300 units and 40
//! beacons, decisions-log item 63), every entity carrying the hit points, maximum and
//! heading decision 5 lets a seat see, and times the client's own path for one view: the
//! text read, the schema decode and the model's replacement of its list
//! (`view::decode_page_text`, then `ViewModel::apply`). The watch rig asks for a view once a
//! pacer period, 100 ms, six frames at 60 fps, so the per-frame share is a sixth of the
//! figure; the Godot half (the dictionaries the bridge builds and `vista.gd`'s `_show`)
//! runs inside the engine and is not measured here.
//!
//! An alarm, not a gate, like the mesher's (`crates/mesher/tests/perf_alarm.rs`): it is
//! `#[ignore]`d, prints `::notice::` lines, and asserts only that it measured something.
//! Run it with
//!
//! ```sh
//! cargo test --release -p pharmakos-client-gdext --test entity_list_cost -- --ignored --nocapture
//! ```
//!
//! Wall-clock time is legal here: this crate is walled (`WALLED_PACKAGES`), and nothing
//! timed here is read by the sim.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::time::Instant;

use pharmakos_client_gdext::view::{ViewModel, decode_page_text};
use pharmakos_mesher::{DrainBudget, LightParams};
use pharmakos_proto::chunk_rle;
use pharmakos_proto::gp::api::v1::{GetViewResponse, ViewChunk, ViewEntity, view_entity};
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json;

/// The gate's world totals (decisions-log item 63).
const UNITS: u32 = 300;
const BEACONS: u32 = 40;

/// How many views the sample is taken over, and how many times each is timed.
const VIEWS: usize = 200;
const REPEATS: usize = 5;

/// The frames one view covers: a pacer period of 100 ms at 60 fps.
const FRAMES_PER_VIEW: u128 = 6;

fn entity(id: String, kind: view_entity::Kind, owner: &str, at: [i32; 3]) -> ViewEntity {
    ViewEntity {
        id,
        kind: kind.into(),
        subtype: "raider".to_owned(),
        owner: owner.to_owned(),
        at: Some(Voxel {
            x: at[0],
            y: at[1],
            z: at[2],
        }),
        hp: Some(80),
        max_hp: Some(120),
        heading: Some(12_000),
    }
}

/// One completing view page whose list is the gate load, as text.
fn gate_load_page() -> String {
    let mut entities = Vec::new();
    for unit in 0..UNITS {
        let at = i32::try_from(unit).expect("small");
        let owner = if unit % 2 == 0 { "seat.0" } else { "seat.1" };
        entities.push(entity(
            format!("u_{unit}"),
            view_entity::Kind::Unit,
            owner,
            [at, at, 24],
        ));
    }
    for beacon in 0..BEACONS {
        let at = i32::try_from(beacon).expect("small");
        let owner = if beacon % 2 == 0 { "seat.0" } else { "seat.1" };
        entities.push(entity(
            format!("b_{beacon:02}"),
            view_entity::Kind::Beacon,
            owner,
            [at * 8, at * 8, 24],
        ));
    }
    let response = GetViewResponse {
        at_ms: 1_000,
        chunks: Vec::new(),
        entities,
        next_cursor: "c".to_owned(),
        complete: true,
    };
    json::write(&json::encode_json(&response).expect("encodes"))
}

fn percentile(sorted: &[u128], percent: usize) -> u128 {
    let rank = (sorted.len() * percent)
        .div_euclid(100)
        .min(sorted.len().saturating_sub(1));
    *sorted.get(rank).expect("a sample")
}

#[test]
#[ignore = "a performance alarm, not a gate: run it with --ignored, in release"]
fn a_full_entity_list_at_the_gate_load_costs_this_much_a_view() {
    let params = LightParams::new(15, 1).expect("the committed pair");
    let budget = DrainBudget {
        surfaces_per_frame: 4,
        bytes_per_frame: 524_288,
        age_frames: 2,
    };
    let mut model = ViewModel::new(params, budget, [384, 384, 64]);
    // A keyframe of one chunk first, so the pages below are views of an established map.
    let keyframe = GetViewResponse {
        at_ms: 0,
        chunks: vec![ViewChunk {
            origin: Some(Voxel { x: 0, y: 0, z: 0 }),
            voxels_rle: chunk_rle::encode(&vec![2_u8; 32_768]),
        }],
        entities: Vec::new(),
        next_cursor: "k".to_owned(),
        complete: true,
    };
    let text = json::write(&json::encode_json(&keyframe).expect("encodes"));
    model
        .apply(decode_page_text(&text).expect("decodes"))
        .expect("applies");

    let page = gate_load_page();
    let mut best: Vec<u128> = Vec::with_capacity(VIEWS);
    for _ in 0..VIEWS {
        let mut lowest = u128::MAX;
        for _ in 0..REPEATS {
            let started = Instant::now();
            let decoded = decode_page_text(&page).expect("the gate load decodes");
            model.apply(decoded).expect("applies");
            lowest = lowest.min(started.elapsed().as_micros());
        }
        best.push(lowest);
    }
    best.sort_unstable();
    let p50 = percentile(&best, 50);
    let p99 = percentile(&best, 99);
    println!(
        "::notice::full entity list at the gate load ({UNITS} units, {BEACONS} beacons, {} \
         bytes of JSON): decode and apply p50 {p50} us, p99 {p99} us a view, about {} us a \
         frame at a view every {FRAMES_PER_VIEW} frames",
        page.len(),
        p99.div_euclid(FRAMES_PER_VIEW),
    );
    assert_eq!(
        model.entities().len(),
        usize::try_from(UNITS + BEACONS).expect("small")
    );
}
