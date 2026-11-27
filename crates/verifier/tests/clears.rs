// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The verifier and the sim agree on every list's `clear_*` flag
//! (decisions-log item 184 (3)): over every place a `MandateSettings` can
//! stand in a playbook, the four lists and every way a row can write a list,
//! the verifier raises `E0515` exactly when the sim's compile refuses the
//! playbook with `PlanError::ClearRefused`, and nothing that qualifies fails
//! to compile. The places are read from the schema, so a new one fails
//! `the_carriers_are_the_schemas` until this file covers it.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::collections::BTreeSet;
use std::path::Path;

use pharmakos_proto::descriptor::{ScalarKind, schema};
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;
use pharmakos_sim::interpreter::{Plan, PlanError};
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{Input, Scope, verify};

/// An area inside the map (it is 64 voxels high).
const AREA: &str = r#"{"min":{"x":90,"y":10,"z":60},"max":{"x":94,"y":14,"z":62}}"#;

/// The four lists: the writ, the list's name, one entry of it.
const LISTS: [(&str, &str, &str); 4] = [
    ("build", "protected_areas", AREA),
    ("survey", "probe_areas", AREA),
    ("defend", "protect", r#"{"my_commander":{}}"#),
    ("attack", "avoid_areas", AREA),
];

/// The messages that hold a playbook's steps, rows or settings, and the
/// field that holds them: every (message, field) of `gp.v1` whose type is one
/// of these. A carrier added to the schema moves this set.
const HOLDERS: [&str; 5] = [
    "gp.v1.Step",
    "gp.v1.InterfaceRow",
    "gp.v1.InitialSettings",
    "gp.v1.MandateSettings",
    "gp.v1.PlaceBeaconStep",
];

/// Where those stand today, each covered by [`carriers`].
const CARRIED_AT: [&str; 7] = [
    "gp.v1.Declarative.route",
    "gp.v1.Handler.body",
    "gp.v1.InitialSettings.mandate",
    "gp.v1.InterfaceRow.set_mandate",
    "gp.v1.InterfaceRow.set_mandate_settings",
    "gp.v1.InterfaceStep.rows",
    "gp.v1.PlaceBeaconStep.initial",
];

#[test]
fn the_carriers_are_the_schemas() {
    let mut found = BTreeSet::new();
    for message in schema().messages() {
        if !message.full_name.starts_with("gp.v1.") {
            continue;
        }
        for field in &message.fields {
            let holds = matches!(field.kind, ScalarKind::Message)
                && (HOLDERS.contains(&field.type_name.as_str())
                    || field.type_name == "gp.v1.InterfaceStep");
            if holds {
                found.insert(format!("{}.{}", message.full_name, field.name));
            }
        }
    }
    let carried: BTreeSet<String> = CARRIED_AT
        .iter()
        .map(|at| (*at).to_owned())
        .chain([
            "gp.v1.Step.interface".to_owned(),
            "gp.v1.Step.place_beacon".to_owned(),
        ])
        .collect();
    assert_eq!(
        found, carried,
        "a place settings can stand moved in the schema: cover it in `carriers`"
    );
}

fn playbook(route: &str, handlers: &str) -> String {
    format!(
        concat!(
            r#"{{"schema_version":{{"major":1}},"meta":{{"title":"case","author_kind":"HUMAN"}},"#,
            r#""kind":"PLAYBOOK","declarative":{{"route":[{route}]{handlers}}},"#,
            r#""on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"#,
            r#""fallback":{{"hold":{{"at":{{"beacon_anchor":{{"safest":{{}}}}}}}}}}}}"#
        ),
        route = route,
        handlers = handlers
    )
}

fn visit(rows: &str) -> String {
    format!(
        r#"{{"label":"s1","timeout_ms":120000,"interface":{{"beacon":{{"beacon_id":"b_02"}},"rows":[{rows}]}}}}"#
    )
}

fn place(settings: &str) -> String {
    format!(
        r#"{{"label":"s1","place_beacon":{{"at":{{"voxel":{{"x":96,"y":11,"z":62}}}},"initial":{{"mandate":{settings}}}}}}}"#
    )
}

const HOLD: &str = r#"{"label":"s0","hold":{"ms":1000}}"#;

fn handler(body: &str) -> String {
    format!(
        concat!(
            r#","handlers":[{{"id":"h00","when":{{"cmdr_hp_pct":{{"cmp":"LE","pct":40}}}},"#,
            r#""body":[{body}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":2}}]"#
        ),
        body = body
    )
}

/// One settings block at every carrier: an edit row, a switch row and a
/// deploy's initial settings, each in the route and in a rule's body.
fn carriers(name: &str, settings: &str) -> Vec<(String, String)> {
    let edit = format!(r#"{{"set_mandate_settings":{settings}}}"#);
    let switch = format!(r#"{{"set_mandate":{settings}}}"#);
    vec![
        (format!("{name}/route-edit"), playbook(&visit(&edit), "")),
        (
            format!("{name}/route-switch"),
            playbook(&visit(&switch), ""),
        ),
        (
            format!("{name}/route-place"),
            playbook(&place(settings), ""),
        ),
        (
            format!("{name}/handler-edit"),
            playbook(HOLD, &handler(&visit(&edit))),
        ),
        (
            format!("{name}/handler-switch"),
            playbook(HOLD, &handler(&visit(&switch))),
        ),
        (
            format!("{name}/handler-place"),
            playbook(HOLD, &handler(&place(settings))),
        ),
    ]
}

/// Every case: each list in every way a row writes it, at every carrier;
/// two rows in one step in both orders; a switch then a clear.
fn cases() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (arm, list, entry) in LISTS {
        for (state, members) in [
            ("omitted", String::new()),
            ("clear", format!(r#""clear_{list}":true"#)),
            ("clear-false", format!(r#""clear_{list}":false"#)),
            (
                "clear-beside-entries",
                format!(r#""clear_{list}":true,"{list}":[{entry}]"#),
            ),
            (
                "clear-beside-empty",
                format!(r#""clear_{list}":true,"{list}":[]"#),
            ),
            ("entries", format!(r#""{list}":[{entry}]"#)),
        ] {
            let settings = format!(r#"{{"roe":"HOLD_FIRE","{arm}":{{{members}}}}}"#);
            out.extend(carriers(&format!("{arm}/{state}"), &settings));
        }
        let clear = format!(
            r#"{{"set_mandate_settings":{{"roe":"HOLD_FIRE","{arm}":{{"clear_{list}":true}}}}}}"#
        );
        let fill = format!(
            r#"{{"set_mandate_settings":{{"roe":"HOLD_FIRE","{arm}":{{"{list}":[{entry}]}}}}}}"#
        );
        let switch = format!(r#"{{"set_mandate":{{"roe":"HOLD_FIRE","{arm}":{{}}}}}}"#);
        for (order, rows) in [
            ("clear-then-fill", format!("{clear},{fill}")),
            ("fill-then-clear", format!("{fill},{clear}")),
            ("switch-then-clear", format!("{switch},{clear}")),
        ] {
            out.push((format!("{arm}/{order}/route"), playbook(&visit(&rows), "")));
            out.push((
                format!("{arm}/{order}/handler"),
                playbook(HOLD, &handler(&visit(&rows))),
            ));
        }
    }
    out
}

#[test]
fn e0515_stands_exactly_where_the_compile_refuses_a_clear() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let rules =
        RulesTable::load(&root.join("rules").join("rules.v1.json")).expect("the committed rules");
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
    let mut wrong = Vec::new();
    let mut refused = 0_u32;
    let all = cases();
    for (name, text) in &all {
        let decoded: Playbook = json::decode(text).expect("every case decodes");
        let compiled = Plan::compile(&decoded, &rules);
        let sim_refuses = matches!(compiled, Err(PlanError::ClearRefused { .. }));
        let input = Input::new(text.as_bytes(), &[], &scope, &rules).expect("an input");
        let report = verify(&input, Depth::Full);
        let e0515 = report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0515");
        if sim_refuses != e0515 {
            wrong.push(format!(
                "{name}: the sim {:?}, the verifier's E0515 {e0515}",
                compiled.as_ref().err()
            ));
        }
        if report.qualifies && compiled.is_err() {
            wrong.push(format!(
                "{name}: qualifies and the compile refuses it: {:?}",
                compiled.err()
            ));
        }
        refused += u32::from(sim_refuses);
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // The refused cases, per list: a clear beside entries at the two edit
    // carriers, and a clear (alone, beside entries or beside `[]`) at the
    // four carriers that start from nothing.
    assert_eq!(refused, 4 * (2 + 3 * 4), "of {} cases", all.len());
}
