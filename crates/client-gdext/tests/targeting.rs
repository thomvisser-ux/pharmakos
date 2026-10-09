// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **S1's surfaces read off the gateway's own goldens** (S1's plan, task `ui`): the client
//! decodes exactly what the gateway answers, with no engine and no host.
//!
//! * `get_map_summary`'s features, the editor's vent click's source
//!   (`tests/golden/gateway/map_summary`);
//! * `resolve_refs`' references, the chips (`tests/golden/gateway/resolve_refs`);
//! * `get_recap`'s prose, drawn as it came (`tests/golden/gateway/recap`);
//! * `get_economy_forecast`'s next BMI and committed spend, present as they came
//!   (`tests/golden/gateway/economy_forecast`);
//! * `get_briefing`, read every Lull (`tests/golden/gateway/briefing`);
//! * and the words the client finds the Lull's "this round" sentence by, which are the
//!   gateway's own (`crates/gateway/src/strings.rs`), read by path because the client may
//!   not depend on the gateway (`CLIENT_WALL`).
//!
//! The goldens are read by path, not copied: a gateway lane that moves one moves what this
//! test reads, and a shape the client cannot read fails here rather than in a played match.

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

use pharmakos_client_gdext::editor::Editor;
use pharmakos_client_gdext::rig::{ADMIN, Outgoing, Phase, Rig, SEAT};
use pharmakos_client_gdext::targeting::{
    ChipArm, FeatureKind, THIS_ROUND_LEAD, chips_of, features_of,
};
use pharmakos_proto::json::{self, Json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The gateway golden `case`'s whole JSON-RPC answer.
fn golden(case: &str) -> Json {
    let path = root()
        .join("tests")
        .join("golden")
        .join("gateway")
        .join(case)
        .join("expected.response.json");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    json::read(&text).expect("a golden is JSON")
}

/// The golden `case`'s `result`.
fn result_of(case: &str) -> Json {
    golden(case).get("result").cloned().expect("a result")
}

/// The golden `case`'s answer, renumbered to answer `frame`.
fn answering(case: &str, frame: &Outgoing) -> String {
    let id = json::read(&frame.text)
        .expect("a request")
        .get("id")
        .cloned()
        .expect("an id");
    let Json::Object(entries) = golden(case) else {
        panic!("a golden answer is an object");
    };
    json::write(&Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| {
                if key == "id" {
                    (key, id.clone())
                } else {
                    (key, value)
                }
            })
            .collect(),
    ))
}

fn method_of(frame: &Outgoing) -> String {
    match json::read(&frame.text).expect("a request").get("method") {
        Some(Json::String(method)) => method.clone(),
        other => panic!("a method, not {other:?}"),
    }
}

/// An answer to `frame` with `result` and a footer in `phase` of round 1.
fn plain(frame: &Outgoing, result: &str, phase: &str) -> String {
    let id = json::read(&frame.text)
        .expect("a request")
        .get("id")
        .map(json::write)
        .expect("an id");
    let body = result.strip_suffix('}').expect("an object");
    let sep = if body.len() > 1 { "," } else { "" };
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"result":{body}{sep}"_status":{{"phase":"{phase}","round":1,"untimed":true}}}}}}"#
    )
}

#[test]
fn the_map_summarys_features_are_the_vent_clicks_source() {
    let features = features_of(&result_of("map_summary")).expect("the golden decodes");
    assert!(!features.is_empty(), "the golden map lists features");
    assert!(
        features
            .iter()
            .any(|feature| feature.kind == FeatureKind::Vent),
        "and vents among them"
    );
    for feature in &features {
        let prefix = format!("{}_{}_{}", feature.kind.name(), feature.x, feature.y);
        assert_eq!(
            feature.id, prefix,
            "a feature's name is its kind and anchor column"
        );
    }
}

#[test]
fn resolve_refs_answer_is_one_chip_per_reference() {
    let result = result_of("resolve_refs");
    let Some(Json::Array(refs)) = result.get("refs") else {
        panic!("the golden lists references");
    };
    let chips = chips_of(&result, "").expect("the golden decodes");
    assert_eq!(chips.len(), refs.len());
    let first = chips.first().expect("a chip");
    assert_eq!(first.arm, ChipArm::Covering);
    assert_eq!(first.step, Some(1));
    let now = first.now.as_ref().expect("it reads a feature");
    let reference = refs.first().expect("a reference");
    assert_eq!(
        Some(&Json::String(now.id.clone())),
        reference.get("feature_id")
    );
    assert_eq!(
        Some(&Json::Number(now.travel_ms.to_string())),
        reference.get("travel_ms"),
        "the travel time as the gateway answered it"
    );
    assert!(
        first.next.is_some(),
        "the golden's first reference has more candidates"
    );
}

/// A rig with both connections open, its first admin answer in `phase`, and its keyframe
/// answered.
fn rig_in(phase: &str) -> Rig {
    let mut rig = Rig::new();
    rig.opened(ADMIN);
    rig.opened(SEAT);
    for frame in rig.poll(0) {
        let answer = match method_of(&frame).as_str() {
            "get_view" => plain(&frame, r#"{"next_cursor":"c1","complete":true}"#, phase),
            _ => plain(&frame, "{}", phase),
        };
        rig.receive(frame.link, &answer).expect("reads");
    }
    rig
}

/// Answers the rig's calls at `now`, `case`'s golden for `method` and something plain for
/// the rest, until it sends `method` and its answer has been read.
fn answer_until(rig: &mut Rig, method: &str, case: &str, phase: &str) {
    for step in 1..64_u64 {
        let mut done = false;
        for frame in rig.poll(step) {
            let answer = if method_of(&frame) == method {
                done = true;
                answering(case, &frame)
            } else {
                match method_of(&frame).as_str() {
                    "get_view" => plain(&frame, r#"{"next_cursor":"c2","complete":true}"#, phase),
                    "get_segment_feed" => plain(&frame, r#"{"events":[]}"#, phase),
                    "get_briefing" => plain(&frame, r#"{"notes":""}"#, phase),
                    "list_drafts" => plain(&frame, r#"{"drafts":[]}"#, phase),
                    "list_templates" => plain(&frame, r#"{"templates":[]}"#, phase),
                    "get_map_summary" => plain(&frame, r#"{"features":[]}"#, phase),
                    _ => plain(&frame, "{}", phase),
                }
            };
            rig.receive(frame.link, &answer).expect("reads");
        }
        if done {
            return;
        }
    }
    panic!("the rig never asked for {method}");
}

#[test]
fn the_recap_is_the_gateways_prose_as_it_came() {
    let mut rig = rig_in("recap");
    assert_eq!(rig.phase(), Phase::Recap);
    answer_until(&mut rig, "get_recap", "recap", "recap");
    let expected = result_of("recap");
    assert_eq!(
        Some(&Json::String(rig.recap().prose.clone())),
        expected.get("prose")
    );
    assert_eq!(rig.recap().round, 1);
}

#[test]
fn the_forecast_carries_the_next_bmi_and_the_committed_spend_as_they_came() {
    let mut rig = rig_in("lull");
    answer_until(&mut rig, "get_economy_forecast", "economy_forecast", "lull");
    let expected = result_of("economy_forecast");
    let number = |key: &str| match expected.get(key) {
        Some(Json::Number(value)) => value.parse::<i32>().ok(),
        _ => None,
    };
    assert_eq!(rig.meter().bmi_next_dollars, number("bmi_next_dollars"));
    assert_eq!(rig.meter().committed_dollars, number("committed_dollars"));
    assert!(rig.meter().bmi_next_dollars.is_some(), "the golden has one");
}

#[test]
fn the_briefing_golden_is_read_every_lull() {
    let mut editor = Editor::new("seat.0");
    editor.lull_opened(1);
    let mut read = false;
    for _ in 0..8 {
        let Some(call) = editor.next_call(false) else {
            break;
        };
        let answer = match call.method {
            "get_briefing" => {
                read = true;
                result_of("briefing")
            }
            "list_drafts" => json::read(r#"{"drafts":[]}"#).expect("json"),
            "list_templates" => json::read(r#"{"templates":[]}"#).expect("json"),
            "get_map_summary" => result_of("map_summary"),
            other => panic!("not this test's call: {other}"),
        };
        editor.answered(Ok(&answer)).expect("the golden decodes");
    }
    assert!(read);
    let prose = match result_of("briefing").get("prose") {
        Some(Json::String(prose)) => prose.clone(),
        other => panic!("a prose, not {other:?}"),
    };
    assert_eq!(
        editor.this_round().is_some(),
        prose.contains(THIS_ROUND_LEAD),
        "the sentence is found exactly when the gateway wrote one"
    );
    assert!(
        !editor.features().is_empty(),
        "the map's features are read with it"
    );
}

/// The words the client finds the "this round" sentence by are the gateway's own: if
/// `strings::this_round` changes its lead, this fails and the client follows.
#[test]
fn the_this_round_lead_is_the_gateways_own_words() {
    let path = root()
        .join("crates")
        .join("gateway")
        .join("src")
        .join("strings.rs");
    let source =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let lead = THIS_ROUND_LEAD.trim_end();
    let written = format!("Some(format!(\"{lead} {{}}.\", parts.join(\"; \")))");
    assert!(
        source.contains(&written),
        "crates/gateway/src/strings.rs no longer writes `{written}`; the client finds the \
         sentence by `{THIS_ROUND_LEAD}`"
    );
}
