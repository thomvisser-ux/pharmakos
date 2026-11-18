// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! S3's predicates and `toward`: the verifier's checks and the sim's compile,
//! held to each other (S3's plan, tasks `vfy6` and `pred`; item 157; S2's
//! E8 test).
//!
//! S3's `con6` put eight predicates and the site `toward` on the wire, and
//! `E0020` to `E0028` refused each one whole until S3's `pred` gave it an
//! effect. `vfy6` built every check on what each one holds behind those
//! codes, and `pred` lifted the sim's refusals and retired the codes, so that
//! no playbook is called qualifying that the sim's compile then refuses (risk
//! R4 the other way round). Two tables hold that agreement:
//!
//! * **[`malformed`]**: a construct written wrong, each with the error the
//!   verifier raises at its pointer, so it never qualifies; and the sim's
//!   compile refuses it for what is wrong with it, except where the error is a
//!   question of the seat's view the compile cannot ask ([`SCOPE_CODES`]): the
//!   run answers those as `no_target`.
//! * **[`well_formed`]**: each construct written right, which the verifier
//!   qualifies and the sim compiles, the extremes each field's type holds
//!   included.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::gp::v1::beacon_filter::MandateKind;
use pharmakos_proto::gp::v1::by_richness::Richness;
use pharmakos_proto::json;
use pharmakos_sim::interpreter::{Plan, Refusal};
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{
    FeatureKind, Input, KnownBeacon, KnownFeature, Ownership, Scope, code_for_construct, verify,
};

fn rules() -> RulesTable {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/verifier sits two levels below the workspace root");
    RulesTable::load(&root.join("rules").join("rules.v1.json")).expect("the committed rules table")
}

/// Seat 0's core `b_01`, an enemy beacon `e_01` and one vent the seat knows.
fn scope() -> Scope {
    let beacon = |id: &str, side: Ownership, at: (i32, i32, i32)| KnownBeacon {
        beacon_id: id.to_owned(),
        owner: SeatId::new(u8::from(side != Ownership::Own)),
        side,
        mandate: if side == Ownership::Own {
            MandateKind::Build
        } else {
            MandateKind::Unspecified
        },
        tags: Vec::new(),
        at: Voxel {
            x: at.0,
            y: at.1,
            z: at.2,
        },
        is_core: side == Ownership::Own,
    };
    Scope::new(SeatId::new(0), SeatEconomy::default())
        .with_beacon(beacon("b_01", Ownership::Own, (80, 11, 55)))
        .with_beacon(beacon("e_01", Ownership::EnemyKnown, (300, 300, 40)))
        .with_feature(KnownFeature {
            feature_id: "vent_150_25".to_owned(),
            kind: FeatureKind::Vent,
            grade: Richness::Standard,
            x: 150,
            y: 25,
            live: true,
            covered_by: None,
        })
}

/// A playbook whose route is `step`, with `handlers` and a plain tail.
fn playbook(step: &str, handlers: &str) -> String {
    format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"case","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{step}],"handlers":[{handlers}]}},"on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
    )
}

/// A route that holds, and one rule watching `when`.
fn ruled(when: &str) -> String {
    playbook(
        r#"{"label":"s","hold":{"ms":1000}}"#,
        &format!(
            r#"{{"id":"h","when":{when},"body":[{{"label":"b","hold":{{"ms":1000}}}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":1}}"#
        ),
    )
}

/// A route whose one step places a beacon at `at`.
fn placed(at: &str) -> String {
    playbook(
        &format!(r#"{{"label":"p","place_beacon":{{"at":{at}}}}}"#),
        "",
    )
}

/// Where a rule's condition stands.
const WHEN: &str = "/declarative/handlers/0/when";

/// Where a placement's site stands.
const SITE: &str = "/declarative/route/0/place_beacon/at";

/// The errors a QUICK check of `text` raises, each with its pointer.
fn errors(text: &str, rules: &RulesTable) -> Vec<(String, String)> {
    let snapshot = Snapshot {
        version: SNAPSHOT_VERSION,
        ..Snapshot::default()
    }
    .to_bytes()
    .expect("an empty snapshot encodes");
    let scope = scope();
    let input = Input::new(text.as_bytes(), &snapshot, &scope, rules).expect("an input");
    verify(&input, Depth::Quick)
        .diagnostics
        .into_iter()
        .filter(|found| found.severity == i32::from(Severity::Error))
        .map(|found| (found.code, found.path))
        .collect()
}

/// The errors that are questions of the seat's view, which the verifier
/// answers from its scope and the sim's compile, holding no view, cannot ask:
/// a `b_NN` the seat does not hold (`E0401`) and a feature it cannot name
/// (`E0412`). The sim compiles such a reference and the run answers it as
/// `no_target`, so the verifier is the stricter side, the safe direction.
const SCOPE_CODES: [&str; 2] = ["E0401", "E0412"];

/// One malformed case: the playbook, and the error it must raise besides its
/// `E002x` code.
struct Malformed {
    text: String,
    code: &'static str,
    at: String,
}

fn when(predicate: &str, code: &'static str, at: &str) -> Malformed {
    Malformed {
        text: ruled(predicate),
        code,
        at: format!("{WHEN}{at}"),
    }
}

fn site(at_json: &str, code: &'static str, at: &str) -> Malformed {
    Malformed {
        text: placed(at_json),
        code,
        at: format!("{SITE}{at}"),
    }
}

/// Every construct written wrong, with the error it is refused by.
#[allow(
    clippy::too_many_lines,
    reason = "one table, one case a line or three; split, a construct's cases would part"
)]
fn malformed() -> Vec<Malformed> {
    let near = r#""near":{"beacon_anchor":{"beacon_id":"b_01"}},"radius_voxels":24"#;
    let count = r#""count":{"op":"GE","value":1}"#;
    let visible = r#""freshness":{"kind":"VISIBLE_NOW"}"#;
    vec![
        // The plan's own examples: an empty comparison, and an unset role.
        when(r#"{"kw_supply":{}}"#, "E0101", "/kw_supply/kw"),
        when(
            r#"{"kw_draw":{"kw":{"value":3}}}"#,
            "E0201",
            "/kw_draw/kw/op",
        ),
        when(
            r#"{"beacon_units_by_role":{"beacon":{"safest":{}},"role":0,"count":{"op":"GE","value":1}}}"#,
            "E0111",
            "/beacon_units_by_role/role",
        ),
        when(
            r#"{"beacon_units_by_role":{"role":"SCOUT","count":{"op":"GE","value":1}}}"#,
            "E0101",
            "/beacon_units_by_role/beacon",
        ),
        when(
            r#"{"beacon_units_by_role":{"beacon":{},"role":"SCOUT","count":{"op":"GE","value":1}}}"#,
            "E0111",
            "/beacon_units_by_role/beacon",
        ),
        when(
            r#"{"beacon_units_by_role":{"beacon":{"safest":{}},"role":"SCOUT"}}"#,
            "E0101",
            "/beacon_units_by_role/count",
        ),
        when(
            r#"{"beacon_mandate":{"beacon":{"beacon_id":"b_01"}}}"#,
            "E0111",
            "/beacon_mandate/mandate",
        ),
        when(
            r#"{"beacon_mandate":{"beacon":{"beacon_id":"b_77"},"mandate":"BUILD"}}"#,
            "E0401",
            "/beacon_mandate/beacon/beacon_id",
        ),
        when(
            r#"{"beacon_has_structure":{"beacon":{"safest":{}}}}"#,
            "E0101",
            "/beacon_has_structure/blueprint_id",
        ),
        when(
            r#"{"beacon_has_structure":{"beacon":{"safest":{}},"blueprint_id":"death_ray"}}"#,
            "E0506",
            "/beacon_has_structure/blueprint_id",
        ),
        when(
            &format!(r#"{{"enemy_units_near":{{"radius_voxels":24,{count},{visible}}}}}"#),
            "E0101",
            "/enemy_units_near/near",
        ),
        when(
            &format!(
                r#"{{"enemy_units_near":{{"near":{{}},"radius_voxels":24,{count},{visible}}}}}"#
            ),
            "E0111",
            "/enemy_units_near/near",
        ),
        when(
            &format!(r#"{{"enemy_units_near":{{"near":{{"safest":{{}}}},{count},{visible}}}}}"#),
            "E0101",
            "/enemy_units_near/radius_voxels",
        ),
        when(
            &format!(
                r#"{{"enemy_units_near":{{"near":{{"voxel":{{"x":-4,"y":2,"z":2}}}},"radius_voxels":8,{count},{visible}}}}}"#
            ),
            "E0402",
            "/enemy_units_near/near/voxel",
        ),
        when(
            &format!(r#"{{"enemy_units_near":{{{near},{visible}}}}}"#),
            "E0101",
            "/enemy_units_near/count",
        ),
        when(
            &format!(r#"{{"enemy_units_near":{{{near},{count}}}}}"#),
            "E0209",
            "/enemy_units_near/freshness",
        ),
        when(
            &format!(r#"{{"enemy_units_near":{{{near},{count},"freshness":{{}}}}}}"#),
            "E0209",
            "/enemy_units_near/freshness/kind",
        ),
        when(
            &format!(
                r#"{{"enemy_units_near":{{{near},{count},"freshness":{{"kind":"KNOWN_WITHIN"}}}}}}"#
            ),
            "E0209",
            "/enemy_units_near/freshness/known_within_ms",
        ),
        when(
            &format!(
                r#"{{"enemy_units_near":{{{near},{count},"freshness":{{"kind":"KNOWN_WITHIN","known_within_ms":-5}}}}}}"#
            ),
            "E0109",
            "/enemy_units_near/freshness/known_within_ms",
        ),
        when(
            &format!(
                r#"{{"enemy_units_near":{{{near},{count},"freshness":{{"kind":"VISIBLE_NOW","known_within_ms":5000}}}}}}"#
            ),
            "E0210",
            "/enemy_units_near/freshness/known_within_ms",
        ),
        when(
            r#"{"enemy_beacon_known":{}}"#,
            "E0209",
            "/enemy_beacon_known/freshness",
        ),
        when(
            &format!(r#"{{"enemy_structure_near":{{{near},{visible}}}}}"#),
            "E0111",
            "/enemy_structure_near",
        ),
        when(
            &format!(r#"{{"enemy_structure_near":{{{near},"blueprint_id":"nope",{visible}}}}}"#),
            "E0506",
            "/enemy_structure_near/blueprint_id",
        ),
        when(
            &format!(r#"{{"enemy_structure_near":{{{near},"blueprint_id":"",{visible}}}}}"#),
            "E0101",
            "/enemy_structure_near/blueprint_id",
        ),
        when(
            &format!(
                r#"{{"enemy_structure_near":{{"near":{{"beacon_anchor":{{}}}},"radius_voxels":24,"any_kind":{{}},{visible}}}}}"#
            ),
            "E0111",
            "/enemy_structure_near/near/beacon_anchor",
        ),
        // The rule applied to the predicates that already had their effect.
        when(
            r#"{"beacon_hp_pct":{"beacon":{},"pct":{"op":"LT","value":50}}}"#,
            "E0111",
            "/beacon_hp_pct/beacon",
        ),
        // `toward`: only in a placement's site, and its feature as a
        // `covering`'s.
        Malformed {
            text: playbook(
                r#"{"label":"m","move":{"to":{"toward":{"feature":{"feature_id":"vent_150_25"}}},"pace":"DIRECT"}}"#,
                "",
            ),
            code: "E0420",
            at: "/declarative/route/0/move/to/toward".to_owned(),
        },
        Malformed {
            text: playbook(r#"{"label":"s","hold":{"ms":1000}}"#, "").replace(
                r#""fallback":{"hold":{"at":{"safest":{}}}}"#,
                r#""fallback":{"hold":{"at":{"toward":{"feature":{"feature_id":"vent_150_25"}}}}}"#,
            ),
            code: "E0420",
            at: "/fallback/hold/at/toward".to_owned(),
        },
        site(r#"{"toward":{}}"#, "E0101", "/toward/feature"),
        site(r#"{"toward":{"feature":{}}}"#, "E0111", "/toward/feature"),
        site(
            r#"{"toward":{"feature":{"vent":{"coverage":"UNCOVERED"}}}}"#,
            "E0111",
            "/toward/feature/vent/rank",
        ),
        site(
            r#"{"toward":{"feature":{"vent":{"rank":"NEAREST"}}}}"#,
            "E0111",
            "/toward/feature/vent/coverage",
        ),
        site(
            r#"{"toward":{"feature":{"covered":{}}}}"#,
            "E0409",
            "/toward/feature/covered",
        ),
        site(
            r#"{"toward":{"feature":{"feature_id":"vent_9_9"}}}"#,
            "E0412",
            "/toward/feature/feature_id",
        ),
    ]
}

/// Every construct written right.
fn well_formed() -> Vec<String> {
    let near = r#""near":{"beacon_anchor":{"beacon_id":"b_01"}},"radius_voxels":24"#;
    let count = r#""count":{"op":"GE","value":1}"#;
    vec![
        ruled(
            r#"{"beacon_units_by_role":{"beacon":{"beacon_id":"b_01"},"role":"SCOUT","count":{"op":"GE","value":1}}}"#,
        ),
        ruled(r#"{"beacon_mandate":{"beacon":{"safest":{}},"mandate":"DEFEND"}}"#),
        ruled(
            r#"{"beacon_has_structure":{"beacon":{"beacon_id":"b_01"},"blueprint_id":"resonance_spire"}}"#,
        ),
        ruled(&format!(
            r#"{{"enemy_units_near":{{{near},{count},"freshness":{{"kind":"KNOWN_WITHIN","known_within_ms":30000}}}}}}"#
        )),
        ruled(r#"{"enemy_beacon_known":{"freshness":{"kind":"VISIBLE_NOW"}}}"#),
        ruled(
            r#"{"enemy_structure_near":{"near":{"voxel":{"x":90,"y":20,"z":50}},"radius_voxels":16,"blueprint_id":"autocannon","freshness":{"kind":"VISIBLE_NOW"}}}"#,
        ),
        ruled(&format!(
            r#"{{"enemy_structure_near":{{{near},"any_kind":{{}},"freshness":{{"kind":"VISIBLE_NOW"}}}}}}"#
        )),
        ruled(r#"{"kw_supply":{"kw":{"op":"LT","value":20}}}"#),
        ruled(r#"{"kw_draw":{"kw":{"op":"GT","value":4}}}"#),
        placed(r#"{"toward":{"feature":{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}}}"#),
        placed(r#"{"toward":{"feature":{"seam":{"rank":"NEAREST","coverage":"ANY"}}}}"#),
        // The extremes each field's type holds (the review of PR #179, item 10):
        // a radius at `u32::MAX` and a window at `i32::MAX` ms, each a legal
        // value the sim reads exactly (S3's `pred`: squared distances in
        // `i128`, ages in whole ticks).
        ruled(&format!(
            r#"{{"enemy_units_near":{{"near":{{"beacon_anchor":{{"beacon_id":"b_01"}}}},"radius_voxels":4294967295,{count},"freshness":{{"kind":"KNOWN_WITHIN","known_within_ms":2147483647}}}}}}"#
        )),
        ruled(
            r#"{"enemy_structure_near":{"near":{"voxel":{"x":90,"y":20,"z":50}},"radius_voxels":4294967295,"any_kind":{},"freshness":{"kind":"KNOWN_WITHIN","known_within_ms":2147483647}}}"#,
        ),
        placed(r#"{"toward":{"feature":{"feature_id":"vent_150_25"}}}"#),
    ]
}

#[test]
fn a_malformed_construct_fails_and_the_sim_refuses_it() {
    let rules = rules();
    for case in malformed() {
        let found = errors(&case.text, &rules);
        let wanted = (case.code.to_owned(), case.at.clone());
        assert!(
            found.contains(&wanted),
            "{wanted:?} among {found:?}\n{}",
            case.text
        );
        let decoded: Playbook = json::decode(&case.text).expect("the case decodes");
        let compiled = Plan::compile(&decoded, &rules);
        if found
            .iter()
            .all(|(code, _)| SCOPE_CODES.contains(&code.as_str()))
        {
            assert!(
                compiled.is_ok(),
                "{}: a question of the seat's view only, which the compile cannot ask: {:?}",
                case.at,
                compiled.err()
            );
        } else {
            assert!(
                compiled.is_err(),
                "{}: the sim's compile refuses it too",
                case.at
            );
        }
    }
}

#[test]
fn a_well_formed_construct_qualifies_and_the_sim_compiles_it() {
    let rules = rules();
    for text in well_formed() {
        let found = errors(&text, &rules);
        let decoded: Playbook = json::decode(&text).expect("the case decodes");
        match Plan::compile(&decoded, &rules) {
            Ok(_) => assert!(
                found.is_empty(),
                "the sim compiles it, so the verifier qualifies it: {found:?}\n{text}"
            ),
            // A construct still refused for its stage is refused on both
            // sides by one table entry and its code alone.
            Err(error) => {
                let refused = Refusal::of(&error)
                    .unwrap_or_else(|| panic!("a table entry, not {error}\n{text}"));
                let code = code_for_construct(refused.construct).expect("a code for the entry");
                assert!(
                    !found.is_empty() && found.iter().all(|(found_code, _)| *found_code == code),
                    "{code} is the one code: {found:?}"
                );
            }
        }
    }
}

/// S3's plan, decision 8: an absent `on_fail` reads as SKIP (the register's
/// S3-08) and a `max_deaths_before_fallback` of 0 as no limit (S3-09), each
/// said where it is omitted, as an `I0003` note and no more.
#[test]
fn decision_eights_readings_are_noted_where_they_are_omitted() {
    let rules = rules();
    let notes = |text: &str| -> Vec<(String, String, String)> {
        let snapshot = Snapshot {
            version: SNAPSHOT_VERSION,
            ..Snapshot::default()
        }
        .to_bytes()
        .expect("an empty snapshot encodes");
        let scope = scope();
        let input = Input::new(text.as_bytes(), &snapshot, &scope, &rules).expect("an input");
        let report = verify(&input, Depth::Quick);
        assert!(report.qualifies, "a note never refuses: {report:?}");
        report
            .diagnostics
            .into_iter()
            .filter(|found| found.code == "I0003")
            .map(|found| {
                let patch = found
                    .suggestions
                    .first()
                    .map(|fix| fix.json_patch.clone())
                    .unwrap_or_default();
                (found.path, found.message, patch)
            })
            .collect()
    };
    let step = |on_fail: &str| {
        playbook(
            &format!(r#"{{"label":"s",{on_fail}"hold":{{"ms":1000}}}}"#),
            "",
        )
    };
    assert_eq!(
        notes(&step("")),
        [(
            "/declarative/route/0/on_fail".to_owned(),
            "`on_fail` is omitted and reads as SKIP.".to_owned(),
            r#"[{"op":"add","path":"/declarative/route/0/on_fail","value":{"action":"SKIP"}}]"#
                .to_owned(),
        )]
    );
    assert_eq!(
        notes(&step(r#""on_fail":{},"#)),
        [(
            "/declarative/route/0/on_fail/action".to_owned(),
            "`action` is omitted and reads as SKIP.".to_owned(),
            r#"[{"op":"add","path":"/declarative/route/0/on_fail/action","value":"SKIP"}]"#
                .to_owned(),
        )]
    );
    for written in ["SKIP", "ABORT_ROUTE"] {
        assert!(
            notes(&step(&format!(r#""on_fail":{{"action":"{written}"}},"#))).is_empty(),
            "{written} is written"
        );
    }
    let unlimited =
        step(r#""on_fail":{"action":"SKIP"},"#).replace(r#","max_deaths_before_fallback":1"#, "");
    assert_eq!(
        notes(&unlimited),
        [(
            "/on_death/max_deaths_before_fallback".to_owned(),
            "`max_deaths_before_fallback` is omitted and reads as 0, no limit.".to_owned(),
            String::new(),
        )]
    );
}
