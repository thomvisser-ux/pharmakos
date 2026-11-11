// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sim's refusal table and this crate's codes for it, held to each other
//! in both directions (S2's plan, E8).
//!
//! The sim refuses at the seal every construct whose effect needs a later stage
//! (`pharmakos_sim::interpreter::REFUSALS`); this crate refuses each of them by
//! a code of its own ([`pharmakos_verifier::REFUSED_CONSTRUCTS`]), so the editor
//! shows before `submit_plan` what the compile would refuse. A lane that lifts a
//! refusal lifts both rows in one pull request, and this file fails until it
//! does.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;
use pharmakos_sim::interpreter::{Plan, REFUSALS, Refusal};
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{Input, REFUSED_CONSTRUCTS, Scope, code_for_construct, verify};

#[test]
fn every_construct_the_sim_refuses_has_an_e_code() {
    let missing: Vec<&str> = REFUSALS
        .iter()
        .filter(|refusal| code_for_construct(refusal.construct).is_none())
        .map(|refusal| refusal.name)
        .collect();
    assert!(
        missing.is_empty(),
        "the sim refuses these constructs and the verifier has no code for them, so a playbook \
         could pass the editor and be refused at submit: {missing:?}"
    );
}

#[test]
fn every_code_names_a_construct_the_sim_refuses() {
    let stale: Vec<&str> = REFUSED_CONSTRUCTS
        .iter()
        .filter(|(construct, _)| {
            !REFUSALS
                .iter()
                .any(|refusal| refusal.construct == *construct)
        })
        .map(|(construct, _)| *construct)
        .collect();
    assert!(
        stale.is_empty(),
        "the verifier refuses these constructs and the sim no longer does, so the editor would \
         refuse a playbook the sim seals: {stale:?}"
    );
    assert_eq!(
        REFUSED_CONSTRUCTS.len(),
        REFUSALS.len(),
        "one code per refused construct, and one refused construct per code"
    );
}

/// A playbook holding `step` and the handlers in `handlers`, as canonical JSON.
fn playbook(step: &str, handlers: &str) -> String {
    format!(
        r#"{{
  "schema_version": {{"major": 1}},
  "meta": {{"title": "case", "author_kind": "HUMAN"}},
  "kind": "PLAYBOOK",
  "declarative": {{"route": [{step}], "handlers": [{handlers}]}},
  "on_death": {{"on_respawn": "CONTINUE", "max_deaths_before_fallback": 1}},
  "fallback": {{"hold": {{"at": {{"beacon_anchor": {{"safest": {{}}}}}}}}}}
}}"#
    )
}

fn rules() -> RulesTable {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/verifier sits two levels below the workspace root");
    RulesTable::load(&root.join("rules").join("rules.v1.json")).expect("the committed rules table")
}

/// The codes a QUICK check of `text` raises, each with its pointer.
fn codes(text: &str, rules: &RulesTable) -> Vec<(String, String)> {
    let snapshot = Snapshot {
        version: SNAPSHOT_VERSION,
        ..Snapshot::default()
    }
    .to_bytes()
    .expect("an empty snapshot encodes");
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
    let input = Input::new(text.as_bytes(), &snapshot, &scope, rules).expect("an input");
    verify(&input, Depth::Quick)
        .diagnostics
        .into_iter()
        .map(|found| (found.code, found.path))
        .collect()
}

/// S3's `con6`: wherever the compile reads a predicate or `toward` the sim
/// refuses it by its table's entry, and the verifier by that entry's code at
/// the construct's own pointer, so neither can be half-used.
#[test]
fn the_verifier_and_the_sim_refuse_s3s_constructs_in_the_same_places() {
    let hold = r#"{"label": "s", "hold": {"ms": 1000}}"#;
    let toward = r#"{"toward": {"feature": {"feature_id": "vent_1_2"}}}"#;
    let supply = r#"{"kw_supply": {"kw": {"op": "GE", "value": 1}}}"#;
    let interface = |row: String| {
        format!(
            r#"{{"label": "q", "interface": {{"beacon": {{"beacon_id": "b_01"}}, "rows": [{row}]}}, "timeout_ms": 30000}}"#
        )
    };
    let cases: Vec<(String, String, &str, &str)> = vec![
        (
            format!(r#"{{"label": "m", "move": {{"to": {toward}, "pace": "DIRECT"}}}}"#),
            String::new(),
            "E0028",
            "/declarative/route/0/move/to/toward",
        ),
        (
            format!(r#"{{"label": "p", "place_beacon": {{"at": {toward}}}}}"#),
            String::new(),
            "E0028",
            "/declarative/route/0/place_beacon/at/toward",
        ),
        (
            interface(format!(
                r#"{{"add_build_target": {{"target": {{"blueprint_id": "generator", "anchor": {toward}}}}}}}"#
            )),
            String::new(),
            "E0028",
            "/declarative/route/0/interface/rows/0/add_build_target/target/anchor/toward",
        ),
        (
            interface(format!(
                r#"{{"remove_build_target": {{"anchor": {toward}}}}}"#
            )),
            String::new(),
            "E0028",
            "/declarative/route/0/interface/rows/0/remove_build_target/anchor/toward",
        ),
        (
            format!(r#"{{"label": "s", "hold": {{"ms": 1000}}, "skip_if": {supply}}}"#),
            String::new(),
            "E0026",
            "/declarative/route/0/skip_if/kw_supply",
        ),
        (
            hold.to_owned(),
            format!(
                r#"{{"id": "h", "when": {{"not": {{"item": {supply}}}}}, "body": [{hold}], "resume": "CONTINUE", "cooldown_ms": 5000, "max_fires": 1}}"#
            ),
            "E0026",
            "/declarative/handlers/0/when/not/item/kw_supply",
        ),
    ];
    let rules = rules();
    for (step, handlers, code, at) in cases {
        let text = playbook(&step, &handlers);
        let found = codes(&text, &rules);
        assert!(
            found.contains(&(code.to_owned(), at.to_owned())),
            "{code} at {at}, among {found:?}"
        );
        let decoded: Playbook = json::decode(&text).expect("the case decodes");
        let error = Plan::compile(&decoded, &rules).expect_err("the sim refuses it too");
        let refused = Refusal::of(&error).expect("as a refusal of its table");
        assert_eq!(code_for_construct(refused.construct), Some(code));
    }
}

/// The sim never compiles a `broadcast`'s place, so `toward` there is not judged:
/// "exactly where the sim refuses, and nowhere else".
#[test]
fn toward_is_not_judged_where_the_sim_compiles_no_place() {
    let toward = r#"{"toward": {"feature": {"feature_id": "vent_1_2"}}}"#;
    let step = format!(r#"{{"label": "b", "broadcast": {{"rally": {{"at": {toward}}}}}}}"#);
    let found = codes(&playbook(&step, ""), &rules());
    assert!(
        !found.iter().any(|(code, _)| code == "E0028"),
        "no E0028 under a broadcast: {found:?}"
    );
}

/// An enum read raw while its construct is refused, as S2's settings were, so
/// that the day S3's `pred` reads each one an unknown number already is not.
#[test]
fn a_number_no_enum_names_in_s3s_constructs_is_e0018() {
    let rule = |predicate: &str| {
        format!(
            r#"{{"id": "h", "when": {predicate}, "body": [{{"label": "b", "hold": {{"ms": 1000}}}}], "resume": "CONTINUE", "cooldown_ms": 5000, "max_fires": 1}}"#
        )
    };
    let hold = r#"{"label": "s", "hold": {"ms": 1000}}"#;
    let when = "/declarative/handlers/0/when";
    for (predicate, at) in [
        (
            r#"{"beacon_units_by_role": {"beacon": {"safest": {}}, "role": 99, "count": {"op": "GE", "value": 1}}}"#,
            "/beacon_units_by_role/role",
        ),
        (
            r#"{"beacon_units_by_role": {"beacon": {"safest": {}}, "role": "SCOUT", "count": {"op": 99, "value": 1}}}"#,
            "/beacon_units_by_role/count/op",
        ),
        (
            r#"{"beacon_mandate": {"beacon": {"safest": {}}, "mandate": 99}}"#,
            "/beacon_mandate/mandate",
        ),
        (
            r#"{"enemy_units_near": {"count": {"op": 99, "value": 1}, "freshness": {"kind": "VISIBLE_NOW"}}}"#,
            "/enemy_units_near/count/op",
        ),
        (
            r#"{"enemy_units_near": {"count": {"op": "GE", "value": 1}, "freshness": {"kind": 99}}}"#,
            "/enemy_units_near/freshness/kind",
        ),
        (
            r#"{"enemy_beacon_known": {"freshness": {"kind": 99}}}"#,
            "/enemy_beacon_known/freshness/kind",
        ),
        (
            r#"{"enemy_structure_near": {"freshness": {"kind": 99}}}"#,
            "/enemy_structure_near/freshness/kind",
        ),
        (
            r#"{"kw_supply": {"kw": {"op": 99, "value": 1}}}"#,
            "/kw_supply/kw/op",
        ),
        (
            r#"{"kw_draw": {"kw": {"op": 99, "value": 1}}}"#,
            "/kw_draw/kw/op",
        ),
    ] {
        let found = codes(&playbook(hold, &rule(predicate)), &rules());
        let wanted = ("E0018".to_owned(), format!("{when}{at}"));
        assert!(found.contains(&wanted), "{wanted:?} among {found:?}");
    }
    let step = r#"{"label": "p", "place_beacon": {"at": {"toward": {"feature": {"vent": {"rank": 99, "coverage": "UNCOVERED"}}}}}}"#;
    let found = codes(&playbook(step, ""), &rules());
    let wanted = (
        "E0018".to_owned(),
        "/declarative/route/0/place_beacon/at/toward/feature/vent/rank".to_owned(),
    );
    assert!(found.contains(&wanted), "{wanted:?} among {found:?}");
}
