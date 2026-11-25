// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where a settings clause sits in the middle of a list, and what its words are
//! (S3's `edlists`, from `zerotitle`'s second review of `render.rs`).
//!
//! * **The mid-list rule.** A settings clause (": firing freely, ...") is itself a
//!   list joined by commas and "and", so a list item after it reads as one more
//!   setting. Wherever a clause can sit before more of its list, the list's items
//!   are set off by semicolons, the last by "; and": the rows of one visit when any
//!   of them carries a clause, and a new beacon's Quartermaster priority after its
//!   mandate's clause. A list whose items carry no clause keeps its commas.
//! * **The roe's words.** A clause is a list of what the beacon is doing ("-ing"
//!   phrases), so the rules of engagement are said as participles there ("firing
//!   freely"), while a menu offers the same value as the order a player gives
//!   ("fire freely"): two facts, two string-table entries.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "clippy.toml sets allow-expect-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_plan_core::strings as s;

/// `route`, a route's JSON array text, rendered as prose with the committed rules
/// table and a one-seat snapshot.
fn prose(route: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let rules = pharmakos_sim::rules::RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads");
    let snapshot = pharmakos_sim::snapshot::Snapshot {
        seat_id: vec![0],
        seat_treasury: vec![200],
        seat_supply: vec![10],
        seat_draw: vec![2],
        ..pharmakos_sim::snapshot::Snapshot::default()
    };
    let context = pharmakos_plan_core::context::PlanContext::from_snapshot(&snapshot, 0)
        .expect("seat 0 is in the snapshot");
    let text = format!(
        r#"{{"schema_version":{{"major":1}},"kind":"PLAYBOOK",
        "meta":{{"title":"clauses","author_kind":"HUMAN"}},
        "declarative":{{"route":{route}}},
        "on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":3}},
        "fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
    );
    let canonical =
        pharmakos_plan_core::canonical::canonicalise_text(&text).expect("the playbook decodes");
    pharmakos_plan_core::render::render(&canonical.playbook, &context, &rules)
        .expect("the playbook renders")
        .prose
}

/// One visit to `b_00` with `rows`, a JSON array text of interface rows.
fn visit(rows: &str) -> String {
    format!(r#"[{{"label":"v","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":{rows}}}}}]"#)
}

/// The review's case: an edit, a switch whose settings carry a clause, and a Build
/// target after it. The target must not read as a Defend setting.
#[test]
fn rows_are_set_off_by_semicolons_when_one_carries_a_clause() {
    let said = prose(&visit(
        r#"[{"set_mandate_settings":{"attack":{"breach_allowed":false}}},
            {"set_mandate":{"defend":{}}},
            {"add_build_target":{"target":{"blueprint_id":"generator","anchor":{"voxel":{"x":4,"y":5,"z":6}}}}}]"#,
    ));
    let expected = format!(
        "{}: {}; {} a Defend mandate: {} {}; and {} ",
        s::ROW_SET_SETTINGS,
        s::BREACH_REFUSED,
        s::ROW_SET_MANDATE,
        s::ROE_FREE_FIRE,
        s::ROE_BY_DEFAULT,
        s::ROW_ADD_TARGET,
    );
    assert!(said.contains(&expected), "{said:?} says {expected:?}");
}

/// A clause on the last row sits at the end of the list, but the list still
/// holds a clause, so its rows are set off the same way: one rule for the list,
/// not one for each row's place in it.
#[test]
fn a_clause_on_the_last_row_sets_the_whole_list_off_too() {
    let said = prose(&visit(
        r#"[{"set_priority":"HIGH"},{"set_mandate":{"survey":{"scout_count":0}}}]"#,
    ));
    let expected = format!("; and {} a Survey mandate: ", s::ROW_SET_MANDATE);
    assert!(said.contains(&expected), "{said:?} says {expected:?}");
    assert!(!said.contains(", and "), "{said:?}");
}

/// Each row kind that carries a clause sets the list off on its own: an edit's
/// row before a row with none, and a switch before one.
#[test]
fn each_row_kind_with_a_clause_sets_the_list_off() {
    for first in [
        r#"{"set_mandate_settings":{"attack":{"breach_allowed":false}}}"#,
        r#"{"set_mandate":{"mine":{}}}"#,
    ] {
        let said = prose(&visit(&format!(r#"[{first},{{"set_priority":"HIGH"}}]"#)));
        assert!(said.contains("; and "), "{first}: {said:?}");
    }
}

/// Rows with no clause keep the plain list: commas, then "and".
#[test]
fn rows_with_no_clause_keep_their_commas() {
    let said = prose(&visit(
        r#"[{"set_priority":"HIGH"},
            {"add_build_target":{"target":{"blueprint_id":"generator","anchor":{"voxel":{"x":4,"y":5,"z":6}}}}},
            {"set_mandate":{"build":{}}}]"#,
    ));
    // A Build switch with nothing written still says its roe's default: so this
    // list does carry a clause. The plain list is one with no switch at all.
    assert!(said.contains("; and "), "{said:?}");
    let plain = prose(&visit(
        r#"[{"set_priority":"HIGH"},
            {"add_build_target":{"target":{"blueprint_id":"generator","anchor":{"voxel":{"x":4,"y":5,"z":6}}}}},
            {"queue_structure":{"blueprint_id":"generator"}}]"#,
    ));
    assert!(!plain.contains(';'), "{plain:?}");
    assert!(plain.contains(", "), "{plain:?}");
}

/// A new beacon's priority after its mandate's clause is set off by a
/// semicolon, so it does not read as a setting of the mandate.
#[test]
fn a_new_beacons_priority_after_its_clause_is_set_off() {
    let said = prose(
        r#"[{"label":"p","place_beacon":{"at":{"voxel":{"x":1,"y":1,"z":0}},
            "initial":{"mandate":{"mine":{"dig_max_depth":0}},"priority":"HIGH"}}}]"#,
    );
    assert!(said.contains("; at "), "{said:?}");
    assert!(
        said.contains(&format!("{} {}", s::ROE_RETURN_FIRE, s::ROE_BY_DEFAULT)),
        "{said:?}"
    );
}

/// A written roe is said as a participle, each value its own entry, and none is
/// the menu's imperative.
#[test]
fn a_written_roe_is_said_as_what_the_beacon_is_doing() {
    for (value, words, menu) in [
        ("HOLD_FIRE", s::ROE_HOLD_FIRE, s::CHOICE_HOLD_FIRE),
        ("RETURN_FIRE", s::ROE_RETURN_FIRE, s::CHOICE_RETURN_FIRE),
        ("FREE_FIRE", s::ROE_FREE_FIRE, s::CHOICE_FREE_FIRE),
    ] {
        let said = prose(&visit(&format!(
            r#"[{{"set_mandate_settings":{{"roe":"{value}"}}}}]"#
        )));
        assert!(
            said.contains(&format!("{}: {words}", s::ROW_SET_SETTINGS)),
            "{value}: {said:?}"
        );
        assert_ne!(words, menu, "{value}: the prose's words are not the menu's");
        assert!(
            words.ends_with("ing fire") || words.starts_with("firing"),
            "{words}"
        );
    }
}

/// A writ's list is said last in its clause, so its own "and" ends the clause
/// and no setting follows it (fix pass 1, P1): a Defend arm's protect list after
/// its picket, an Attack arm's avoid areas after its waves, a Survey arm's probe
/// areas after its scout count.
#[test]
fn a_writs_list_is_said_last_in_its_clause() {
    for (arm, list_words, other_words) in [
        (
            r#"{"defend":{"protect":[{"beacon":{"beacon_id":"b_00"}},{"my_commander":{}}],"picket_scouts":2}}"#,
            format!("{} ", s::DEFEND_PROTECTING),
            format!("{} 2 ", s::DEFEND_PICKET),
        ),
        (
            r#"{"attack":{"avoid_areas":[{"min":{"x":1,"y":1,"z":0},"max":{"x":2,"y":2,"z":1}}],"waves":2}}"#,
            format!("{} 1 ", s::ATTACK_AVOIDING),
            format!(" {} 2 ", s::WAVES_IN),
        ),
        (
            r#"{"survey":{"probe_areas":[{"min":{"x":1,"y":1,"z":0},"max":{"x":2,"y":2,"z":1}}],"scout_count":3}}"#,
            format!("{} 1 ", s::SURVEY_PROBING),
            format!("{} 3 ", s::SCOUTS_FIELDING),
        ),
    ] {
        let said = prose(&visit(&format!(r#"[{{"set_mandate_settings":{arm}}}]"#)));
        let list = said
            .find(&list_words)
            .unwrap_or_else(|| panic!("{arm}: {said:?}"));
        let other = said
            .find(&other_words)
            .unwrap_or_else(|| panic!("{arm}: {said:?}"));
        assert!(other < list, "{arm}: the list is last: {said:?}");
    }
}

/// Fix pass 1, T4: a new beacon given only a priority, no mandate, keeps the
/// comma: no clause precedes it (the review's R12, the priority always set off
/// by a semicolon, is red here).
#[test]
fn a_priority_with_no_mandate_keeps_its_comma() {
    let said = prose(
        r#"[{"label":"p","place_beacon":{"at":{"voxel":{"x":1,"y":1,"z":0}},
            "initial":{"priority":"HIGH"}}}]"#,
    );
    assert!(
        said.contains(", at high Quartermaster priority"),
        "{said:?}"
    );
    assert!(!said.contains(';'), "{said:?}");
}
