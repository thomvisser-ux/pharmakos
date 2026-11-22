// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Easy prices a step as the verifier does (S3's `opr3`, fix pass 1, item 3).
//!
//! `pharmakos-operator` depends on `pharmakos-proto` alone (AGENTS.md section
//! 3 rule 3), so it restates spec section 5's interface arithmetic
//! (`crates/operator/src/price.rs`) rather than call the verifier's
//! (`pharmakos_verifier::interface`). This crate links both, so here every
//! step kind Easy can compose goes through both and the two are held equal:
//! the steps of every seal Easy makes in hosted matches on the committed
//! seeds (Hold & Build's and Expand & Mine's placements, stones, Survey posts,
//! the core's dig depth and the safe playbook's raises), each library
//! template's own place step (the guard post's, the raid post's), and the
//! operator's own step builders (a tap, the Spire, a stone).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pharmakos_gamectl::host::{EasyOperators, LIBRARY_PATH};
use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, Settings};
use pharmakos_gateway::serve::InProcessSeats;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Subject;
use pharmakos_operator::Easy;
use pharmakos_proto::gp::v1::Step;
use pharmakos_proto::json::Json;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::interface::{Rates, step_ms};

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

/// A JSONC text with its whole-line and trailing `//` comments taken out
/// (the playbooks here carry no `//` inside a string but URLs, which none of
/// these do).
fn without_comments(text: &str) -> String {
    text.lines()
        .map(|line| match line.find("//") {
            Some(at) if !line[..at].contains('"') || line[..at].matches('"').count() % 2 == 0 => {
                &line[..at]
            }
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every route step of a playbook text.
fn route_steps(text: &str) -> Vec<Json> {
    let body = pharmakos_proto::json::read(&without_comments(text)).expect("a playbook reads");
    match body.get("declarative").and_then(|d| d.get("route")) {
        Some(Json::Array(steps)) => steps.clone(),
        _ => Vec::new(),
    }
}

/// A step's label, or its kind's name when it has none.
fn label(step: &Json) -> String {
    match step.get("label") {
        Some(Json::String(label)) => label.clone(),
        _ => String::from("?"),
    }
}

/// Every seal Easy makes on one seed in a hosted match of `rounds` rounds,
/// every seat Easy, on the rules table's own segment ladder.
fn hosted_seals(seed: u64, seats: u32, rounds: u32, rules: &str) -> Vec<String> {
    let host = Host::open_from(
        rules,
        seed,
        seats,
        &Settings {
            segment_lengths_ms: Vec::new(),
            round_limit: rounds,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let ids: Vec<SeatId> = (0..seats)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect();
    let mut surface = Surface::new(
        &format!("pricing-{seed:x}-{seats}-{}", std::process::id()),
        seed,
        table,
        FogPolicy::fogged(),
        &ids,
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    let mut factory = EasyOperators::new(rules).expect("the operator reads the rules text");
    let mut easy =
        InProcessSeats::open(&mut surface, &ids, None, &mut factory).expect("in-process seats");
    let mut texts: Vec<String> = Vec::new();
    for round in 1..=rounds {
        easy.plan(&mut surface);
        for seat in &ids {
            if let Some(sealed) = surface
                .seat_state(Subject::Seat(*seat), *seat)
                .expect("its own")
                .sealed
                .as_ref()
            {
                texts.push(sealed.playbook_jsonc.clone());
            }
        }
        assert!(surface.begin_push().expect("the Push opens"));
        while let Some(report) = surface.step().expect("a tick") {
            if report.segment_ended {
                break;
            }
        }
        surface.end_recap().expect("the recap closes");
        if round < rounds {
            surface.open_lull().expect("the next Lull");
        }
    }
    texts
}

#[test]
fn easy_prices_every_step_it_composes_as_the_verifier_does() {
    let rules = rules_json();
    let easy = Easy::new(&rules).expect("the operator reads the rules text");
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let rates = Rates::from_rules(&table).expect("the rate card");

    let mut steps: Vec<Json> = Vec::new();
    for (seed, seats) in [
        (pharmakos_sim::DETERMINISM_MATCH_SEED, 3),
        (0x0000_0000_ca5c_aded, 3),
        (20, 2),
    ] {
        for seal in hosted_seals(seed, seats, 3, &rules) {
            steps.extend(route_steps(&seal));
        }
    }
    for template in [
        "defend_the_core",
        "expand_and_mine",
        "hold_and_build",
        "raid_an_enemy_beacon",
        "safe_playbook",
    ] {
        let text =
            std::fs::read_to_string(root().join(LIBRARY_PATH).join(format!("{template}.jsonc")))
                .expect("the template");
        steps.extend(route_steps(&text));
    }
    steps.extend([
        pharmakos_operator::steps::tap("b_00", "vent_40_40"),
        pharmakos_operator::steps::spire("b_00"),
        pharmakos_operator::steps::stone("vent_40_40", 60_000),
        pharmakos_operator::steps::deepen_core("b_00"),
    ]);

    let mut seen: BTreeSet<String> = BTreeSet::new();
    for step in &steps {
        let decoded: Step = pharmakos_proto::json::decode_json(step).expect("a gp.v1.Step");
        let verifier = i64::try_from(step_ms(&decoded, &rates).raw()).expect("a duration");
        let operator = easy.on_site_ms(step).expect("Easy prices the step");
        assert_eq!(
            operator,
            verifier,
            "{}: {}",
            label(step),
            pharmakos_proto::json::write(step)
        );
        seen.insert(label(step));
    }
    eprintln!("steps priced alike: {seen:?}");
    for wanted in [
        "add_generator",
        "deepen_core",
        "queue_spire",
        "stone",
        "survey_post",
        "place_power",
        "place_mine",
        "place_guard",
        "place_raid",
    ] {
        assert!(
            seen.iter().any(|label| label.starts_with(wanted)),
            "{wanted} was priced: {seen:?}"
        );
    }
}
