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

use pharmakos_proto::gp::api::v1::diagnostic::Severity;
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
    playbook_with(
        step,
        handlers,
        r#"{"hold": {"at": {"beacon_anchor": {"safest": {}}}}}"#,
    )
}

/// [`playbook`], with `fallback` written out.
fn playbook_with(step: &str, handlers: &str, fallback: &str) -> String {
    format!(
        r#"{{
  "schema_version": {{"major": 1}},
  "meta": {{"title": "case", "author_kind": "HUMAN"}},
  "kind": "PLAYBOOK",
  "declarative": {{"route": [{step}], "handlers": [{handlers}]}},
  "on_death": {{"on_respawn": "CONTINUE", "max_deaths_before_fallback": 1}},
  "fallback": {fallback}
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

/// S3's `con6`, held on both sides by one table: every construct `con6` put on
/// the wire, written in every place it can stand, is refused by the sim's
/// compile with its own table entry and by the verifier with that entry's code
/// at the construct's own pointer. So none of them can be half-used, wherever it
/// is written, until S3's `pred` lifts it.
#[test]
fn every_s3_construct_is_refused_in_every_place_it_can_stand() {
    let mut cases = predicate_cases();
    cases.append(&mut toward_cases());
    assert_eq!(cases.len(), 8 * 5 + 9, "every construct in every place");

    let rules = rules();
    for (step, handlers, fallback, at, code) in cases {
        let text = playbook_with(&step, &handlers, &fallback);
        let found = codes(&text, &rules);
        assert!(
            found.contains(&(code.to_owned(), at.clone())),
            "{code} at {at}, among {found:?}"
        );
        assert!(
            found
                .iter()
                .all(|(other, _)| !other.starts_with("E002") || other == code),
            "only {code} of con6's codes at {at}: {found:?}"
        );
        let decoded: Playbook = json::decode(&text).expect("the case decodes");
        let error = Plan::compile(&decoded, &rules).expect_err("the sim refuses it too");
        let refused = Refusal::of(&error)
            .unwrap_or_else(|| panic!("{at}: refused as a table entry, not as {error}"));
        assert_eq!(code_for_construct(refused.construct), Some(code), "{at}");
    }
}

/// One place case: the step, the handlers, the fallback, the construct's
/// pointer and the code it is refused by.
type PlaceCase = (String, String, String, String, &'static str);

/// The fallback every case not about the fallback carries.
const SAFEST: &str = r#"{"hold": {"at": {"safest": {}}}}"#;

/// A step that does nothing, for the cases whose construct is elsewhere.
const HOLD: &str = r#"{"label": "s", "hold": {"ms": 1000}}"#;

/// Each predicate in each place a condition is read.
fn predicate_cases() -> Vec<PlaceCase> {
    let hold = HOLD;
    let safest = SAFEST;
    let rule = |when: &str, body: &str| {
        format!(
            r#"{{"id": "h", "when": {when}, "body": [{body}], "resume": "CONTINUE", "cooldown_ms": 5000, "max_fires": 1}}"#
        )
    };
    let predicates: [(&str, &str, &str); 8] = [
        (
            "beacon_units_by_role",
            "E0020",
            r#"{"beacon": {"safest": {}}, "role": "SCOUT", "count": {"op": "GE", "value": 1}}"#,
        ),
        (
            "beacon_mandate",
            "E0021",
            r#"{"beacon": {"safest": {}}, "mandate": "BUILD"}"#,
        ),
        (
            "beacon_has_structure",
            "E0022",
            r#"{"beacon": {"safest": {}}, "blueprint_id": "generator"}"#,
        ),
        (
            "enemy_units_near",
            "E0023",
            r#"{"near": {"safest": {}}, "radius_voxels": 24, "count": {"op": "GE", "value": 1}, "freshness": {"kind": "VISIBLE_NOW"}}"#,
        ),
        (
            "enemy_beacon_known",
            "E0024",
            r#"{"freshness": {"kind": "VISIBLE_NOW"}}"#,
        ),
        (
            "enemy_structure_near",
            "E0025",
            r#"{"near": {"safest": {}}, "radius_voxels": 24, "any_kind": {}, "freshness": {"kind": "VISIBLE_NOW"}}"#,
        ),
        ("kw_supply", "E0026", r#"{"kw": {"op": "GE", "value": 1}}"#),
        ("kw_draw", "E0027", r#"{"kw": {"op": "GE", "value": 1}}"#),
    ];
    // (step, handlers, fallback, pointer, code)
    let mut cases: Vec<PlaceCase> = Vec::new();
    for (name, code, body) in predicates {
        let predicate = format!(r#"{{"{name}": {body}}}"#);
        let places: [(String, String, String); 5] = [
            (
                format!(r#"{{"label": "s", "hold": {{"ms": 1000}}, "skip_if": {predicate}}}"#),
                String::new(),
                "/declarative/route/0/skip_if".to_owned(),
            ),
            (
                format!(
                    r#"{{"label": "w", "wait_until": {{"condition": {predicate}}}, "timeout_ms": 1000}}"#
                ),
                String::new(),
                "/declarative/route/0/wait_until/condition".to_owned(),
            ),
            (
                hold.to_owned(),
                rule(&predicate, hold),
                "/declarative/handlers/0/when".to_owned(),
            ),
            (
                hold.to_owned(),
                rule(
                    &format!(r#"{{"not": {{"item": {{"any": {{"items": [{predicate}]}}}}}}}}"#),
                    hold,
                ),
                "/declarative/handlers/0/when/not/item/any/items/0".to_owned(),
            ),
            (
                hold.to_owned(),
                rule(
                    r#"{"kw_headroom": {"kw": {"op": "LT", "value": 0}}}"#,
                    &format!(r#"{{"label": "b", "hold": {{"ms": 1000}}, "skip_if": {predicate}}}"#),
                ),
                "/declarative/handlers/0/body/0/skip_if".to_owned(),
            ),
        ];
        for (step, handlers, at) in places {
            cases.push((
                step,
                handlers,
                safest.to_owned(),
                format!("{at}/{name}"),
                code,
            ));
        }
    }
    cases
}

/// `toward` in each place a `Location` is compiled.
fn toward_cases() -> Vec<PlaceCase> {
    let hold = HOLD;
    let safest = SAFEST;
    let toward = r#"{"toward": {"feature": {"feature_id": "vent_1_2"}}}"#;
    let target = format!(r#"{{"blueprint_id": "generator", "anchor": {toward}}}"#);
    let interface = |row: String| {
        format!(
            r#"{{"label": "q", "interface": {{"beacon": {{"safest": {{}}}}, "rows": [{row}]}}, "timeout_ms": 30000}}"#
        )
    };
    let rows = "/declarative/route/0/interface/rows/0";
    let sites: [(String, String, String); 9] = [
        (
            format!(r#"{{"label": "m", "move": {{"to": {toward}, "pace": "DIRECT"}}}}"#),
            safest.to_owned(),
            "/declarative/route/0/move/to".to_owned(),
        ),
        (
            format!(r#"{{"label": "p", "place_beacon": {{"at": {toward}}}}}"#),
            safest.to_owned(),
            "/declarative/route/0/place_beacon/at".to_owned(),
        ),
        (
            format!(
                r#"{{"label": "p", "place_beacon": {{"at": {{"voxel": {{"x": 80, "y": 11, "z": 55}}}}, "initial": {{"mandate": {{"build": {{"targets": [{target}]}}}}}}}}}}"#
            ),
            safest.to_owned(),
            "/declarative/route/0/place_beacon/initial/mandate/build/targets/0/anchor".to_owned(),
        ),
        (
            interface(format!(r#"{{"add_build_target": {{"target": {target}}}}}"#)),
            safest.to_owned(),
            format!("{rows}/add_build_target/target/anchor"),
        ),
        (
            interface(format!(
                r#"{{"remove_build_target": {{"anchor": {toward}}}}}"#
            )),
            safest.to_owned(),
            format!("{rows}/remove_build_target/anchor"),
        ),
        (
            interface(format!(
                r#"{{"set_mandate": {{"build": {{"targets": [{target}]}}}}}}"#
            )),
            safest.to_owned(),
            format!("{rows}/set_mandate/build/targets/0/anchor"),
        ),
        (
            interface(format!(
                r#"{{"set_mandate_settings": {{"build": {{"targets": [{target}]}}}}}}"#
            )),
            safest.to_owned(),
            format!("{rows}/set_mandate_settings/build/targets/0/anchor"),
        ),
        (
            hold.to_owned(),
            format!(r#"{{"hold": {{"at": {toward}}}}}"#),
            "/fallback/hold/at".to_owned(),
        ),
        (
            hold.to_owned(),
            format!(r#"{{"patrol": {{"waypoints": [{{"safest": {{}}}}, {toward}]}}}}"#),
            "/fallback/patrol/waypoints/1".to_owned(),
        ),
    ];
    sites
        .into_iter()
        .map(|(step, fallback, at)| {
            (
                step,
                String::new(),
                fallback,
                format!("{at}/toward"),
                "E0028",
            )
        })
        .collect()
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

/// S3's `vfy5` builds the Spire's own checks (`E0507`, `E0513`) behind
/// `E0009`, which still refuses every `queue_structure` row (S3's plan, E4
/// and R4): a Spire queued by a seat that holds its licence, where none
/// stands, is refused by the sim's compile with the table entry `E0009`
/// answers, and by the verifier with `E0009` at the row, so the verdict never
/// runs ahead of the seal. S3's `lic` narrows both sides together.
#[test]
fn a_licensed_spire_is_refused_on_both_sides_until_the_sim_builds_it() {
    let rules = rules();
    let row = r#"{"queue_structure": {"blueprint_id": "resonance_spire"}}"#;
    let step = format!(
        r#"{{"label": "s", "interface": {{"beacon": {{"beacon_id": "b_00"}}, "rows": [{row}]}}}}"#
    );
    let text = playbook(&step, "");
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default())
        .with_beacon(pharmakos_verifier::KnownBeacon {
            beacon_id: "b_00".to_owned(),
            owner: SeatId::new(0),
            side: pharmakos_verifier::Ownership::Own,
            mandate: pharmakos_proto::gp::v1::beacon_filter::MandateKind::Build,
            tags: Vec::new(),
            at: pharmakos_proto::gp::v1::Voxel {
                x: 10,
                y: 10,
                z: 10,
            },
            is_core: true,
        })
        .with_licence(pharmakos_proto::gp::v1::Licence::ResonanceSpire)
        .expect("the licence joins the view");
    let snapshot = Snapshot {
        version: SNAPSHOT_VERSION,
        ..Snapshot::default()
    }
    .to_bytes()
    .expect("an empty snapshot encodes");
    let input = Input::new(text.as_bytes(), &snapshot, &scope, &rules).expect("an input");
    // QUICK: the seat's empty economy makes FULL's projection add `E0601`
    // for the Spire's draw, which is about the economy and not the queue.
    let report = verify(&input, Depth::Quick);
    assert!(!report.qualifies, "{:?}", report.diagnostics);
    let found: Vec<(&str, &str)> = report
        .diagnostics
        .iter()
        .filter(|found| found.severity == i32::from(Severity::Error))
        .map(|found| (found.code.as_str(), found.path.as_str()))
        .collect();
    assert_eq!(
        found,
        [(
            "E0009",
            "/declarative/route/0/interface/rows/0/queue_structure"
        )],
        "a licensed Spire where none stands is refused by E0009 alone"
    );
    let decoded: Playbook = json::decode(&text).expect("the case decodes");
    let error = Plan::compile(&decoded, &rules).expect_err("the sim refuses it too");
    let refused =
        Refusal::of(&error).unwrap_or_else(|| panic!("refused as a table entry, not as {error}"));
    assert_eq!(code_for_construct(refused.construct), Some("E0009"));
}
