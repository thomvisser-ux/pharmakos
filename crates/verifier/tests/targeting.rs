// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Targeting's sites, checked where they stand (S1's plan, task `tgtv`;
//! docs/design/targeting.md, "Sites" and "Surfaces").
//!
//! S1's targeting proto (task `con2`) refused `on` and `covering` with `E0003`
//! wherever they were written, until the rules landed; this file replaces the
//! assertions it shipped (`targeting_refused.rs`). What holds now:
//!
//! * `covering` is legal only in a `place_beacon`'s `at` (`E0407` elsewhere);
//!   `on` only in a Build target's anchor, and as a `feature_id` in
//!   `remove_build_target` (`E0408` elsewhere, `E0410` for a description
//!   there); `covered {}` only under `on` in a Build target inside the
//!   `initial` of a `covering` deploy (`E0409` elsewhere);
//! * an unset rank, an unset choice and, under `covering`, an unset coverage
//!   are `E0111`; a coverage under `on` is `E0411`;
//! * a name the view does not hold is `E0412`, and a seam under `on` `E0413`;
//! * the three lints, `W0704` to `W0706`, at FULL only, never refusing;
//! * no condition can name a feature at all in S1, so "feature NEAREST in a
//!   condition" is refused by the schema itself, which this file pins.
//!
//! Inline asserts and no golden: the report goldens are the committed cases'
//! (`tests/verifier.rs`), one per code.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report."
)]

use std::path::{Path, PathBuf};

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::gp::v1::beacon_filter::MandateKind;
use pharmakos_proto::gp::v1::by_richness::Richness;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{FeatureKind, Input, KnownBeacon, KnownFeature, Ownership, Scope, verify};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/verifier sits two levels below the workspace root"))
        .to_path_buf()
}

fn rules() -> RulesTable {
    RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
        .unwrap_or_else(|error| panic!("reading the rules table: {error}"))
}

fn snapshot() -> Vec<u8> {
    Snapshot {
        version: SNAPSHOT_VERSION,
        match_seed: 0x0102_0304_0506_0708,
        tick: 3_600,
        ..Snapshot::default()
    }
    .to_bytes()
    .unwrap_or_else(|error| panic!("encoding the snapshot: {error}"))
}

fn vent(id: &str, at: (i32, i32), live: bool, covered_by: Option<&str>) -> KnownFeature {
    KnownFeature {
        feature_id: id.to_owned(),
        kind: FeatureKind::Vent,
        grade: Richness::Standard,
        x: at.0,
        y: at.1,
        live,
        covered_by: covered_by.map(str::to_owned),
    }
}

fn seam(id: &str, at: (i32, i32)) -> KnownFeature {
    KnownFeature {
        kind: FeatureKind::Seam,
        ..vent(id, at, true, None)
    }
}

/// Seat 0 with its core `b_01` on BUILD at (80, 11, 55), one uncovered vent,
/// one vent its core covers, and one seam: the same shape as the case
/// fixture's map, smaller.
fn scope() -> Scope {
    Scope::new(
        SeatId::new(0),
        SeatEconomy {
            treasury: Money::new(200),
            supply: Kw::new(10),
            draw: Kw::new(4),
        },
    )
    .with_beacon(KnownBeacon {
        beacon_id: "b_01".to_owned(),
        owner: SeatId::new(0),
        side: Ownership::Own,
        mandate: MandateKind::Build,
        tags: Vec::new(),
        at: Voxel {
            x: 80,
            y: 11,
            z: 55,
        },
        is_core: true,
    })
    .with_feature(vent("vent_150_25", (150, 25), true, None))
    .with_feature(vent("vent_90_20", (90, 20), true, Some("b_01")))
    .with_feature(seam("seam_200_60", (200, 60)))
    .with_commander(Voxel {
        x: 82,
        y: 13,
        z: 55,
    })
}

/// A whole playbook around a route and a fallback, as canonical `gp.v1` JSON.
fn playbook(route: &str, fallback: &str) -> String {
    format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"targeting\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{route}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":2}},",
            "\"fallback\":{fallback},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        route = route,
        fallback = fallback
    )
}

const HOLD_SAFEST: &str = r#"{"hold":{"at":{"safest":{}}}}"#;

fn report_with(route: &str, fallback: &str, scope: &Scope, depth: Depth) -> VerifyReport {
    let bytes = playbook(route, fallback);
    let snapshot = snapshot();
    let rules = rules();
    let input = Input::new(bytes.as_bytes(), &snapshot, scope, &rules)
        .unwrap_or_else(|error| panic!("the rules table carries what the checks read: {error}"));
    verify(&input, depth)
}

fn report(route: &str, depth: Depth) -> VerifyReport {
    report_with(route, HOLD_SAFEST, &scope(), depth)
}

/// Every (code, path) a report carries, notes and warnings included.
fn found(report: &VerifyReport) -> Vec<(String, String)> {
    report
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.clone(), diagnostic.path.clone()))
        .collect()
}

/// Both depths report exactly these (code, path) pairs from the QUICK stages.
fn assert_quick(route: &str, expected: &[(&str, &str)]) {
    for depth in [Depth::Quick, Depth::Full] {
        let report = report(route, depth);
        let quick: Vec<(String, String)> = found(&report)
            .into_iter()
            .filter(|(code, _)| !code.starts_with("W07"))
            .collect();
        let want: Vec<(String, String)> = expected
            .iter()
            .map(|(code, path)| ((*code).to_owned(), (*path).to_owned()))
            .collect();
        assert_eq!(quick, want, "{depth:?}: {report:?}");
        let errors = report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity() == Severity::Error);
        assert_eq!(report.qualifies, !errors, "{depth:?}");
    }
}

const NEAREST_UNCOVERED_VENT: &str = r#"{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}"#;

fn place(at: &str, initial: &str) -> String {
    if initial.is_empty() {
        format!(r#"{{"label":"place","place_beacon":{{"at":{at}}}}}"#)
    } else {
        format!(r#"{{"label":"place","place_beacon":{{"at":{at},"initial":{initial}}}}}"#)
    }
}

fn generator_on(on: &str) -> String {
    format!(r#"{{"blueprint_id":"generator","anchor":{{"on":{on}}},"order":1}}"#)
}

fn build_initial(target: &str) -> String {
    format!(r#"{{"mandate":{{"build":{{"targets":[{target}],"terraform":"NONE"}}}}}}"#)
}

fn visit_core(row: &str) -> String {
    format!(r#"{{"label":"visit","interface":{{"beacon":{{"beacon_id":"b_01"}},"rows":[{row}]}}}}"#)
}

// ---------------------------------------------------------------------------
// Which arm is legal where
// ---------------------------------------------------------------------------

#[test]
fn the_adopted_spelling_qualifies_clean() {
    // "Place a beacon covering the nearest vent you can cover, and build a
    // Generator on it" (targeting.md, "Sites"): every arm in its legal place.
    let route = place(
        &format!(r#"{{"covering":{NEAREST_UNCOVERED_VENT}}}"#),
        &build_initial(&generator_on(r#"{"covered":{}}"#)),
    );
    for depth in [Depth::Quick, Depth::Full] {
        let report = report(&route, depth);
        assert!(report.qualifies, "{depth:?}: {report:?}");
        assert!(report.diagnostics.is_empty(), "{depth:?}: {report:?}");
    }
}

#[test]
fn neither_arm_is_e0003_any_more() {
    // `on` and `covering` are v1 vocabulary, so `E0003` — "held back to a later
    // version" — is decode's again, for the reserved words alone (decisions-log
    // item 130 (4)).
    for route in [
        place(&format!(r#"{{"covering":{NEAREST_UNCOVERED_VENT}}}"#), ""),
        place(r#"{"on":{"feature_id":"vent_150_25"}}"#, ""),
        visit_core(&format!(
            r#"{{"add_build_target":{{"target":{}}}}}"#,
            generator_on(r#"{"vent":{"rank":"NEAREST"}}"#)
        )),
    ] {
        let report = report(&route, Depth::Full);
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "E0003"),
            "{report:?}"
        );
    }
}

#[test]
fn covering_stands_only_in_a_place_step() {
    let covering = format!(r#"{{"covering":{NEAREST_UNCOVERED_VENT}}}"#);
    assert_quick(
        &format!(r#"{{"label":"walk","move":{{"to":{covering},"pace":"DIRECT"}}}}"#),
        &[("E0407", "/declarative/route/0/move/to/covering")],
    );
    assert_quick(
        &visit_core(&format!(
            r#"{{"add_build_target":{{"target":{{"blueprint_id":"generator","anchor":{covering},"order":1}}}}}}"#
        )),
        &[(
            "E0407",
            "/declarative/route/0/interface/rows/0/add_build_target/target/anchor/covering",
        )],
    );
    // The fallback, where neither arm has a meaning.
    let report = report_with(
        r#"{"label":"wait","hold":{"ms":1000}}"#,
        &format!(r#"{{"hold":{{"at":{covering}}}}}"#),
        &scope(),
        Depth::Quick,
    );
    assert_eq!(
        found(&report),
        [("E0407".to_owned(), "/fallback/hold/at/covering".to_owned())]
    );
}

#[test]
fn on_stands_only_in_a_build_targets_anchor() {
    assert_quick(
        &place(r#"{"on":{"feature_id":"vent_150_25"}}"#, ""),
        &[("E0408", "/declarative/route/0/place_beacon/at/on")],
    );
    assert_quick(
        r#"{"label":"walk","move":{"to":{"on":{"feature_id":"vent_150_25"}},"pace":"DIRECT"}}"#,
        &[("E0408", "/declarative/route/0/move/to/on")],
    );
    // Legal in each of a Build target's three homes.
    let target = generator_on(r#"{"vent":{"rank":"NEAREST"}}"#);
    for route in [
        visit_core(&format!(r#"{{"add_build_target":{{"target":{target}}}}}"#)),
        visit_core(&format!(
            r#"{{"set_mandate_settings":{{"build":{{"targets":[{target}],"terraform":"NONE"}}}}}}"#
        )),
        place(
            r#"{"voxel":{"x":85,"y":15,"z":55}}"#,
            &build_initial(&target),
        ),
    ] {
        assert_quick(&route, &[]);
    }
}

#[test]
fn an_edit_row_carries_no_default_note() {
    // S1-39's defaults (decision 11) are what an omitted field reads as where a
    // beacon's settings start from nothing. A `set_mandate_settings` row edits
    // the current mandate instead: a field it leaves out is one it does not
    // write, and the sim prices the row by the fields it does write, so a
    // "write the default out" Fix there would lengthen the row and could
    // overwrite a live setting.
    let notes = |route: &str| -> Vec<String> {
        found(&report(route, Depth::Full))
            .into_iter()
            .filter(|(code, _)| code == "I0003")
            .map(|(_, path)| path)
            .collect()
    };
    for row in [
        r#"{"set_mandate_settings":{"mine":{"dig_max_depth":3}}}"#,
        r#"{"set_mandate_settings":{"build":{"repair_threshold_pct":50}}}"#,
    ] {
        assert_eq!(notes(&visit_core(row)), Vec::<String>::new(), "{row}");
    }
    // The same block in a switch, whose settings start from nothing, is noted.
    assert_eq!(
        notes(&visit_core(
            r#"{"set_mandate":{"mine":{"dig_max_depth":3}}}"#
        )),
        [
            "/declarative/route/0/interface/rows/0/set_mandate/mine/seam_choice",
            "/declarative/route/0/interface/rows/0/set_mandate/mine/pillar_spacing",
        ]
    );
}

#[test]
fn covered_means_something_only_under_a_covering_deploy() {
    let covered_target = generator_on(r#"{"covered":{}}"#);
    // In a row, there is no deploy for it to mean.
    assert_quick(
        &visit_core(&format!(
            r#"{{"add_build_target":{{"target":{covered_target}}}}}"#
        )),
        &[(
            "E0409",
            "/declarative/route/0/interface/rows/0/add_build_target/target/anchor/on/covered",
        )],
    );
    // Under a deploy at a fixed voxel, the beacon covers nothing in particular.
    assert_quick(
        &place(
            r#"{"voxel":{"x":85,"y":15,"z":55}}"#,
            &build_initial(&covered_target),
        ),
        &[(
            "E0409",
            "/declarative/route/0/place_beacon/initial/mandate/build/targets/0/anchor/on/covered",
        )],
    );
    // As the site itself, it is the feature it would be defined by.
    assert_quick(
        &place(r#"{"covering":{"covered":{}}}"#, ""),
        &[(
            "E0409",
            "/declarative/route/0/place_beacon/at/covering/covered",
        )],
    );
}

#[test]
fn removing_a_target_takes_a_name_and_nothing_else() {
    let remove = |on: &str| {
        visit_core(&format!(
            r#"{{"remove_build_target":{{"anchor":{{"on":{on}}}}}}}"#
        ))
    };
    assert_quick(&remove(r#"{"feature_id":"vent_90_20"}"#), &[]);
    let base = "/declarative/route/0/interface/rows/0/remove_build_target/anchor/on";
    assert_quick(
        &remove(r#"{"vent":{"rank":"NEAREST"}}"#),
        &[("E0410", &format!("{base}/vent"))],
    );
    assert_quick(
        &remove(r#"{"covered":{}}"#),
        &[("E0410", &format!("{base}/covered"))],
    );
    assert_quick(&remove("{}"), &[("E0111", base)]);
    // A seam description is refused once, as a description; a seam's name is
    // a name, and no target stands on a seam.
    assert_quick(
        &remove(r#"{"seam":{"rank":"NEAREST"}}"#),
        &[("E0410", &format!("{base}/seam"))],
    );
    assert_quick(
        &remove(r#"{"feature_id":"seam_200_60"}"#),
        &[("E0413", &format!("{base}/feature_id"))],
    );
}

#[test]
fn a_pick_carries_its_rank_and_the_coverage_its_arm_asks_for() {
    let at = "/declarative/route/0/place_beacon/at/covering";
    assert_quick(
        &place(r#"{"covering":{"vent":{"coverage":"ANY"}}}"#, ""),
        &[("E0111", &format!("{at}/vent/rank"))],
    );
    assert_quick(
        &place(r#"{"covering":{"seam":{"rank":"NEAREST"}}}"#, ""),
        &[("E0111", &format!("{at}/seam/coverage"))],
    );
    assert_quick(&place(r#"{"covering":{}}"#, ""), &[("E0111", at)]);
    // Under `on`, the coverage must be left out, and its Fix does just that.
    let route = visit_core(&format!(
        r#"{{"add_build_target":{{"target":{}}}}}"#,
        generator_on(r#"{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}"#)
    ));
    let path =
        "/declarative/route/0/interface/rows/0/add_build_target/target/anchor/on/vent/coverage";
    assert_quick(&route, &[("E0411", path)]);
    let report = report(&route, Depth::Quick);
    let fix = report
        .diagnostics
        .first()
        .and_then(|diagnostic| diagnostic.suggestions.first())
        .expect("E0411 offers a Fix");
    assert_eq!(
        fix.json_patch,
        format!(r#"[{{"op":"remove","path":"{path}"}}]"#)
    );
}

#[test]
fn a_name_must_be_one_the_view_holds() {
    assert_quick(
        &place(r#"{"covering":{"feature_id":"vent_150_25"}}"#, ""),
        &[],
    );
    assert_quick(
        &place(r#"{"covering":{"feature_id":"vent_1_1"}}"#, ""),
        &[(
            "E0412",
            "/declarative/route/0/place_beacon/at/covering/feature_id",
        )],
    );
    // A view with no features — every gateway's until S1's task `tgtw` fills
    // it — holds no name at all.
    let empty = Scope::new(SeatId::new(0), SeatEconomy::default());
    let report = report_with(
        &place(r#"{"covering":{"feature_id":"vent_150_25"}}"#, ""),
        HOLD_SAFEST,
        &empty,
        Depth::Quick,
    );
    assert!(
        found(&report).iter().any(|(code, _)| code == "E0412"),
        "{report:?}"
    );
}

#[test]
fn on_stands_a_structure_on_a_vent_and_never_on_a_seam() {
    let add = |on: &str| {
        visit_core(&format!(
            r#"{{"add_build_target":{{"target":{}}}}}"#,
            generator_on(on)
        ))
    };
    let base = "/declarative/route/0/interface/rows/0/add_build_target/target/anchor/on";
    assert_quick(
        &add(r#"{"seam":{"rank":"NEAREST"}}"#),
        &[("E0413", &format!("{base}/seam"))],
    );
    assert_quick(
        &add(r#"{"feature_id":"seam_200_60"}"#),
        &[("E0413", &format!("{base}/feature_id"))],
    );
    assert_quick(&add(r#"{"feature_id":"vent_150_25"}"#), &[]);
    // `covered` under a deploy that covers a seam names that seam.
    assert_quick(
        &place(
            r#"{"covering":{"seam":{"rank":"NEAREST","coverage":"ANY"}}}"#,
            &build_initial(&generator_on(r#"{"covered":{}}"#)),
        ),
        &[(
            "E0413",
            "/declarative/route/0/place_beacon/initial/mandate/build/targets/0/anchor/on/covered",
        )],
    );
}

// ---------------------------------------------------------------------------
// The three lints
// ---------------------------------------------------------------------------

/// The lint codes a FULL report carries, with their paths.
fn lints(report: &VerifyReport) -> Vec<(String, String)> {
    found(report)
        .into_iter()
        .filter(|(code, _)| code.starts_with("W07"))
        .collect()
}

#[test]
fn a_lint_is_full_only_and_never_refuses() {
    let route = place(r#"{"covering":{"feature_id":"vent_150_25"}}"#, "");
    let mut lost = scope();
    lost.push_feature(vent("vent_150_25", (150, 25), false, None));
    let quick = report_with(&route, HOLD_SAFEST, &lost, Depth::Quick);
    let full = report_with(&route, HOLD_SAFEST, &lost, Depth::Full);
    assert!(lints(&quick).is_empty(), "{quick:?}");
    assert_eq!(
        lints(&full),
        [(
            "W0704".to_owned(),
            "/declarative/route/0/place_beacon/at/covering/feature_id".to_owned()
        )]
    );
    assert!(quick.qualifies && full.qualifies);
}

#[test]
fn a_pick_matches_nothing_when_its_filters_leave_nothing() {
    let route = place(&format!(r#"{{"covering":{NEAREST_UNCOVERED_VENT}}}"#), "");
    let path = "/declarative/route/0/place_beacon/at/covering/vent";
    // One uncovered vent: something matches.
    assert!(lints(&report(&route, Depth::Full)).is_empty());
    // Every live vent covered.
    let mut all_covered = scope();
    all_covered.push_feature(vent("vent_150_25", (150, 25), true, Some("b_01")));
    let report = report_with(&route, HOLD_SAFEST, &all_covered, Depth::Full);
    assert_eq!(lints(&report), [("W0704".to_owned(), path.to_owned())]);
    let message = &report.diagnostics.first().expect("the lint").message;
    assert!(
        message.contains("the nearest vent you do not cover")
            && message.contains("every live one is inside one of your spheres"),
        "{message}"
    );
    // The same pick under ANY still matches a covered vent.
    let any = place(
        r#"{"covering":{"vent":{"rank":"NEAREST","coverage":"ANY"}}}"#,
        "",
    );
    assert!(lints(&report_with(&any, HOLD_SAFEST, &all_covered, Depth::Full)).is_empty());
    // No live vent at all.
    let none = Scope::new(SeatId::new(0), SeatEconomy::default()).with_feature(vent(
        "vent_150_25",
        (150, 25),
        false,
        None,
    ));
    let report = report_with(&any, HOLD_SAFEST, &none, Depth::Full);
    assert_eq!(lints(&report).len(), 1, "{report:?}");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("none of that kind is live")),
        "{report:?}"
    );
}

#[test]
fn an_earlier_placement_anywhere_that_may_run_first_is_named() {
    let pick = place(&format!(r#"{{"covering":{NEAREST_UNCOVERED_VENT}}}"#), "");
    // The first step reads before anything is placed.
    assert!(lints(&report(&pick, Depth::Full)).is_empty());
    // A handler that places can fire before any route step.
    let bytes = playbook(&pick, HOLD_SAFEST).replace(
        r#""declarative":{"route":["#,
        r#""declarative":{"handlers":[{"id":"h","when":{"cmdr_deaths":{"deaths":{"op":"GE","value":1}}},"body":[{"label":"b","place_beacon":{"at":{"voxel":{"x":85,"y":15,"z":55}}}}],"resume":"CONTINUE","cooldown_ms":30000,"max_fires":1}],"route":["#,
    );
    let snapshot = snapshot();
    let rules = rules();
    let scope = scope();
    let input = Input::new(bytes.as_bytes(), &snapshot, &scope, &rules).expect("input");
    let report = verify(&input, Depth::Full);
    let earlier = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "W0705")
        .unwrap_or_else(|| panic!("a handler's placement counts: {report:?}"));
    assert_eq!(
        earlier.related_paths,
        ["/declarative/handlers/0/body/0".to_owned()]
    );
}

#[test]
fn a_fixed_site_is_flagged_only_when_every_vent_in_reach_is_covered() {
    // (95, 25) reaches vent_90_20, which b_01 covers, and nothing else.
    let fixed = place(r#"{"voxel":{"x":95,"y":25,"z":56}}"#, "");
    let flagged = report(&fixed, Depth::Full);
    assert_eq!(
        lints(&flagged),
        [(
            "W0706".to_owned(),
            "/declarative/route/0/place_beacon/at/voxel".to_owned()
        )]
    );
    let diagnostic = flagged.diagnostics.first().expect("the lint");
    assert!(
        diagnostic.message.contains("Heat vent (90, 20)") && diagnostic.message.contains("b_01"),
        "{}",
        diagnostic.message
    );
    // Its Fix swaps in the description, at the site.
    let fix = diagnostic.suggestions.first().expect("a Fix");
    assert_eq!(
        fix.json_patch,
        r#"[{"op":"replace","path":"/declarative/route/0/place_beacon/at","value":{"covering":{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}}}]"#
    );
    // An uncovered vent in reach as well: the site covers something new.
    let mut with_new = scope();
    with_new.push_feature(vent("vent_100_30", (100, 30), true, None));
    assert!(lints(&report_with(&fixed, HOLD_SAFEST, &with_new, Depth::Full)).is_empty());
    // No vent in reach at all: nothing to say.
    let far = place(r#"{"voxel":{"x":70,"y":5,"z":55}}"#, "");
    assert!(lints(&report(&far, Depth::Full)).is_empty());
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

#[test]
fn no_condition_can_name_a_feature_in_s1() {
    // "Feature NEAREST in a condition" is refused (targeting.md, "Nearest": a
    // condition reads every 250 ms and re-ranking every evaluation thrashes).
    // In S1 the schema itself refuses it: no message a condition can reach
    // carries a `FeatureRef` or a `Location`, so there is nothing for the
    // verifier to check — and the walk, which reaches only `BeaconRef`s inside
    // a condition, would not see one if a later schema added it. This test is
    // the tripwire: it goes red in the pull request that gives a predicate a
    // place or a feature, which is where the verifier's refusal has to land.
    let schema = pharmakos_proto::descriptor::schema();
    let mut seen: Vec<String> = Vec::new();
    let mut stack: Vec<String> = vec!["gp.v1.Condition".to_owned()];
    while let Some(name) = stack.pop() {
        if seen.contains(&name) {
            continue;
        }
        let message = schema
            .message(&name)
            .unwrap_or_else(|| panic!("{name} is in the schema"));
        for field in &message.fields {
            if schema.message(&field.type_name).is_some() {
                stack.push(field.type_name.clone());
            }
        }
        seen.push(name);
    }
    assert!(
        seen.len() > 5,
        "the walk reached only {seen:?}; it is not following the condition's fields"
    );
    for forbidden in ["gp.v1.FeatureRef", "gp.v1.Location", "gp.v1.VentPick"] {
        assert!(
            !seen.iter().any(|name| name == forbidden),
            "{forbidden} is reachable from gp.v1.Condition: a condition can now name a place \
             or a feature, so the verifier must refuse feature NEAREST there"
        );
    }
}
