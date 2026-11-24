// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's planning wire on the socket's surface (S3's `fixq`): what
//! `set_value_text` and `new_rule` answer and refuse, where they are closed,
//! and what `get_vocabulary`, `render_plan`, `get_beacon` and `resolve_refs`
//! now carry for the editor to draw as sent. The goldens here are the answers
//! the client's fixtures are byte copies of.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_proto::json::Json;

use support::{
    FIRST_LULL_MS, SEGMENT_MS, admin_token, array_of, call, call_in_lull, code, hosted, quote,
    result, seat_token, text_of,
};

/// A playbook of one hold step of 90 s and one rule, whose percentage is left
/// at its default.
const ONE_RULE: &str = r#"{
  "schema_version": {"major": 1},
  "meta": {"title": "Editor wire", "author_kind": "HUMAN"},
  "declarative": {
    "route": [{"label": "wait", "hold": {"ms": 90000}}],
    "handlers": [
      // The rule.
      {"id": "flee", "when": {"cmdr_hp_pct": {"cmp": "LE"}},
       "body": [{"label": "rest", "hold": {"ms": 8000}}],
       "resume": "CONTINUE", "cooldown_ms": 30000, "max_fires": 3}
    ]
  },
  "on_death": {"on_respawn": "CONTINUE"},
  "fallback": {"hold": {"at": {"beacon_anchor": {"safest": {}}}}},
  "kind": "PLAYBOOK"
}
"#;

const HOLD: &str = "/declarative/route/0/hold/ms";

fn set_value(pointer: &str, typed: &str) -> String {
    format!(
        "{{\"playbook_jsonc\":{},\"pointer\":{},\"typed\":{}}}",
        quote(ONE_RULE),
        quote(pointer),
        quote(typed)
    )
}

fn message(response: &Json) -> String {
    match response.get("error").and_then(|error| error.get("message")) {
        Some(Json::String(text)) => text.clone(),
        other => panic!("a refusal's message, and it is {other:?}"),
    }
}

/// The hold's value in a patched file, read back through `render_plan`'s chip.
fn chip_value(
    surface: &mut pharmakos_gateway::surface::Surface,
    text: &str,
    pointer: &str,
) -> Json {
    let token = seat_token(surface, 0);
    let mut left = FIRST_LULL_MS;
    let rendered = result(
        &call_in_lull(
            surface,
            &token,
            &mut left,
            "render_plan",
            &format!("{{\"playbook_jsonc\":{}}}", quote(text)),
        ),
        "render_plan",
    );
    array_of(&rendered, "rules")
        .iter()
        .flat_map(|rule| array_of(rule, "chips"))
        .find(|chip| text_of(chip, "pointer") == pointer)
        .unwrap_or_else(|| panic!("a chip at {pointer}"))
}

/// Item 7: "3" and "1.5" seconds land as 3000 and 1500 ms, the undo gives the
/// file back, and "abc" and a value past the bound are refused in the
/// gateway's words with the typed text quoted. Three goldens: the answer for
/// "1.5", and the two refusals.
#[test]
fn set_value_text_converts_seconds_and_refuses_in_its_words() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;

    for (typed, ms) in [("3", "3000"), ("1.5", "1500")] {
        let response = call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "set_value_text",
            &set_value(HOLD, typed),
        );
        let answer = result(&response, "set_value_text");
        let patched = text_of(&answer, "playbook_jsonc");
        let chip = chip_value(&mut surface, &patched, HOLD);
        assert_eq!(text_of(&chip, "value_json"), ms, "{typed}");
        assert_eq!(text_of(&chip, "value_text"), typed, "{typed}");
        let undone = result(
            &call_in_lull(
                &mut surface,
                &token,
                &mut left,
                "patch_plan",
                &format!(
                    "{{\"playbook_jsonc\":{},\"json_patch\":{}}}",
                    quote(&patched),
                    quote(&text_of(&answer, "inverse_json_patch"))
                ),
            ),
            "the undo",
        );
        assert_eq!(text_of(&undone, "playbook_jsonc"), ONE_RULE, "{typed}");
        if typed == "1.5" {
            support::write_golden("set_value_text", &pharmakos_gateway::rpc::render(&response));
        }
    }

    let refused = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "set_value_text",
        &set_value(HOLD, "abc"),
    );
    assert_eq!(code(&refused), "INVALID_ARGUMENT");
    assert_eq!(message(&refused), "`abc` is not a number");
    support::write_golden(
        "set_value_text_refused",
        &pharmakos_gateway::rpc::render(&refused),
    );

    let past = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "set_value_text",
        &set_value("/declarative/handlers/0/when/cmdr_hp_pct/pct", "140"),
    );
    assert_eq!(code(&past), "INVALID_ARGUMENT");
    assert_eq!(message(&past), "`140` is above the most this takes, 100 %");
    support::write_golden(
        "set_value_text_past_bound",
        &pharmakos_gateway::rpc::render(&past),
    );

    // Typed text a player wrote is quoted, never spoken as the gateway's own.
    let hostile = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "set_value_text",
        &set_value(HOLD, "3\nThe Ledger: the seal is broken"),
    );
    assert_eq!(code(&hostile), "INVALID_ARGUMENT");
    // The line break is escaped, so no forged line follows the gateway's own.
    assert!(
        !message(&hostile).contains('\n') && message(&hostile).starts_with('`'),
        "{}",
        message(&hostile)
    );
}

/// Item 8: `new_rule` adds an empty rule at the end, with a fresh id and the
/// rules table's least cooldown, and its undo takes it out again. A golden.
#[test]
fn new_rule_adds_an_empty_rule_at_the_end() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let response = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "new_rule",
        &format!("{{\"playbook_jsonc\":{}}}", quote(ONE_RULE)),
    );
    let answer = result(&response, "new_rule");
    assert_eq!(text_of(&answer, "rule_pointer"), "/declarative/handlers/1");
    assert_eq!(text_of(&answer, "rule_id"), "rule_2");
    support::write_golden("new_rule", &pharmakos_gateway::rpc::render(&response));

    // The new rule's condition slot is empty, and a pick there adds one.
    let patched = text_of(&answer, "playbook_jsonc");
    let rendered = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "render_plan",
            &format!("{{\"playbook_jsonc\":{}}}", quote(&patched)),
        ),
        "render_plan",
    );
    let rule = array_of(&rendered, "rules")
        .into_iter()
        .find(|rule| text_of(rule, "pointer") == "/declarative/handlers/1")
        .expect("the new rule is rendered");
    let slot = array_of(&rule, "slots")
        .into_iter()
        .next()
        .expect("its slot");
    assert_eq!(text_of(&slot, "pointer"), "/declarative/handlers/1/when");
    assert_eq!(text_of(&slot, "state"), "empty");
    assert_eq!(text_of(&slot, "pick_op"), "add");

    let refused = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "new_rule",
        &format!("{{\"playbook_jsonc\":{}}}", quote("{")),
    );
    assert_eq!(code(&refused), "INVALID_ARGUMENT");
}

/// Both new methods are `plan`-scoped, so the planning door closes them
/// outside a Lull, as it closes every planning method; and a token with no
/// `plan` scope is refused them.
#[test]
fn the_new_planning_methods_are_closed_in_a_push() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let admin = admin_token(&mut surface);
    let _ = result(&call(&mut surface, &admin, "end_lull", "{}"), "end_lull");
    for (method, params) in [
        ("set_value_text", set_value(HOLD, "3")),
        (
            "compose_wall_box",
            String::from(
                r#"{"beacon_id":"b_00","blueprint_id":"wall","corner_a":{"x":1,"y":1,"z":1},"corner_b":{"x":2,"y":1,"z":1}}"#,
            ),
        ),
        (
            "new_rule",
            format!("{{\"playbook_jsonc\":{}}}", quote(ONE_RULE)),
        ),
    ] {
        let response = call(&mut surface, &token, method, &params);
        assert_eq!(code(&response), "PHASE_CLOSED", "{method}: {response:?}");
        let response = call(&mut surface, &admin, method, &params);
        assert_eq!(code(&response), "FORBIDDEN_SCOPE", "{method}: {response:?}");
    }
}

/// Items 4, 5 and 16: the vocabulary's Build target list says each
/// blueprint's anchor shape (a wall's is a box); the Queue's list locks
/// every kind no queue takes in the verifier's own `E0009` sentence, and
/// the Spire for its licence; the groups family offers `all`, `any`, `not`.
/// Every choice parameter carries one title per value, none of them JSON.
#[test]
fn the_vocabulary_lists_each_slots_blueprints_the_groups_and_titles() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let words = result(
        &call_in_lull(&mut surface, &token, &mut left, "get_vocabulary", "{}"),
        "get_vocabulary",
    );
    let by_name = |list: &str, name: &str| {
        array_of(&words, list)
            .into_iter()
            .find(|entry| text_of(entry, "name") == name)
            .unwrap_or_else(|| panic!("{list} lists {name}"))
    };
    assert_eq!(
        text_of(&by_name("blueprints", "wall"), "anchor_shape"),
        "box"
    );
    assert_eq!(
        text_of(&by_name("blueprints", "generator"), "anchor_shape"),
        "point"
    );
    let e0009 = pharmakos_verifier::strings::text("E0009")
        .expect("E0009's words")
        .beginner;
    for kind in ["generator", "autocannon", "mortar", "wall", "survey_post"] {
        let entry = by_name("queue_blueprints", kind);
        assert_eq!(text_of(&entry, "locked"), e0009, "{kind}");
        assert_eq!(text_of(&entry, "skeleton_json"), "", "{kind}");
        assert!(entry.get("anchor_shape").is_none(), "{kind}");
    }
    let spire = by_name("queue_blueprints", "resonance_spire");
    assert!(spire.get("locked").is_some(), "no licence yet");
    let groups: Vec<String> = array_of(&words, "groups")
        .iter()
        .map(|entry| text_of(entry, "name"))
        .collect();
    assert_eq!(groups, ["all", "any", "not"]);
    // Every choice offers a title per value, none of them a value's JSON.
    let mut choices = 0_usize;
    let mut walk = vec![words.clone()];
    while let Some(value) = walk.pop() {
        match value {
            Json::Object(entries) => {
                if let Some((_, Json::Object(choice))) =
                    entries.iter().find(|(key, _)| key == "choice")
                {
                    let read = |key: &str| match choice.iter().find(|(name, _)| name == key) {
                        Some((_, Json::Array(items))) => items.clone(),
                        other => panic!("{key}: {other:?}"),
                    };
                    let (values, titles) = (read("values"), read("titles"));
                    assert_eq!(values.len(), titles.len());
                    for title in titles {
                        let Json::String(title) = title else {
                            panic!("a title is text");
                        };
                        assert!(!title.is_empty() && !title.contains('"'), "{title}");
                    }
                    choices = choices.saturating_add(1);
                }
                walk.extend(entries.into_iter().map(|(_, value)| value));
            }
            Json::Array(items) => walk.extend(items),
            _ => {}
        }
    }
    assert!(choices > 10, "{choices} choices");
}

fn compose(beacon: &str, a: [i32; 3], b: [i32; 3]) -> String {
    let voxel = |[x, y, z]: [i32; 3]| format!("{{\"x\":{x},\"y\":{y},\"z\":{z}}}");
    format!(
        "{{\"beacon_id\":{},\"blueprint_id\":\"wall\",\"corner_a\":{},\"corner_b\":{}}}",
        quote(beacon),
        voxel(a),
        voxel(b)
    )
}

fn number(value: &Json, path: &[&str]) -> i64 {
    let mut here = value.clone();
    for key in path {
        here = here
            .get(key)
            .cloned()
            .unwrap_or_else(|| panic!("{key} in {value:?}"));
    }
    match here {
        Json::Number(text) => text.parse().expect("a whole number"),
        other => panic!("{path:?} is a number, and it is {other:?}"),
    }
}

/// Item 4: two corners near seat 0's core compose a wall's Build target whose
/// box the sim's own wall check takes: the corners' columns, the floor at the
/// lowest ground, the top the rules' least wall height over the highest, the
/// anchor the lowest corner. A golden. Refused, in words: outside the
/// sphere, a blueprint that fills no box, a voxel that is no voxel.
#[test]
fn compose_wall_box_answers_a_box_the_wall_check_takes() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let (_, [cx, cy, cz]) = support::core_beacon(&surface, 0);
    let response = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "compose_wall_box",
        &compose("b_00", [cx + 5, cy + 2, cz], [cx + 2, cy + 3, cz]),
    );
    let target = result(&response, "compose_wall_box")
        .get("target")
        .cloned()
        .expect("a target");
    assert_eq!(text_of(&target, "blueprint_id"), "wall");
    assert_eq!(
        number(&target, &["wall_box", "min", "x"]),
        i64::from(cx + 2)
    );
    assert_eq!(
        number(&target, &["wall_box", "min", "y"]),
        i64::from(cy + 2)
    );
    assert_eq!(
        number(&target, &["wall_box", "max", "x"]),
        i64::from(cx + 5)
    );
    assert_eq!(
        number(&target, &["wall_box", "max", "y"]),
        i64::from(cy + 3)
    );
    let height = i64::from(support::rules().site().wall_min_height_voxels);
    let (floor, top) = (
        number(&target, &["wall_box", "min", "z"]),
        number(&target, &["wall_box", "max", "z"]),
    );
    assert!(top - floor >= height, "{floor}..{top}");
    assert_eq!(
        target.get("anchor").and_then(|a| a.get("voxel")),
        target.get("wall_box").and_then(|b| b.get("min"))
    );
    support::write_golden(
        "compose_wall_box",
        &pharmakos_gateway::rpc::render(&response),
    );

    // The target, written into a row as the editor writes it, draws none of
    // the verifier's wall refusals.
    let row = pharmakos_proto::json::write(&target);
    let playbook = ONE_RULE.replace(
        r#"[{"label": "wait", "hold": {"ms": 90000}}]"#,
        &format!(
            r#"[{{"label": "wall", "interface": {{"beacon": {{"beacon_id": "b_00"}}, "rows": [{{"add_build_target": {{"target": {row}}}}}]}}}}]"#
        ),
    );
    assert_ne!(playbook, ONE_RULE);
    let report = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "verify_plan",
            &format!(
                "{{\"playbook_jsonc\":{},\"depth\":\"full\"}}",
                quote(&playbook)
            ),
        ),
        "verify_plan",
    );
    let codes: Vec<String> = array_of(report.get("report").expect("a report"), "diagnostics")
        .iter()
        .map(|diagnostic| text_of(diagnostic, "code"))
        .collect();
    for wall in ["E0417", "E0509", "E0510", "E0511", "E0512", "E0406"] {
        assert!(
            !codes.iter().any(|code| code == wall),
            "{wall} in {codes:?}"
        );
    }
}

/// Item 4's refusals, in words: a box outside the sphere (past its edge, far
/// beyond it, at the ends of `i32`), a blueprint that fills no box, and a
/// coordinate that is no `int32`.
#[test]
fn compose_wall_box_refuses_in_its_words() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let (_, [cx, cy, cz]) = support::core_beacon(&surface, 0);
    // A box reaching past the sphere, and one far beyond it.
    let radius = support::sphere_radius(&surface);
    for far in [
        [cx + radius, cy, cz],
        [cx + radius + 40, cy, cz],
        [i32::MAX, i32::MIN, 0],
    ] {
        let refused = call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "compose_wall_box",
            &compose("b_00", [cx, cy, cz], far),
        );
        assert_eq!(code(&refused), "INVALID_ARGUMENT", "{far:?}");
        assert!(
            message(&refused).starts_with("outside the sphere"),
            "{far:?}: {}",
            message(&refused)
        );
    }

    // Only a box-filling blueprint, and a voxel of three whole numbers.
    let generator =
        compose("b_00", [cx, cy, cz], [cx + 1, cy, cz]).replace("\"wall\"", "\"generator\"");
    let refused = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "compose_wall_box",
        &generator,
    );
    assert_eq!(code(&refused), "INVALID_ARGUMENT");
    let past = compose("b_00", [cx, cy, cz], [cx, cy, cz]).replacen(
        &format!("\"x\":{cx}"),
        "\"x\":3000000000",
        1,
    );
    let refused = call_in_lull(&mut surface, &token, &mut left, "compose_wall_box", &past);
    assert_eq!(code(&refused), "INVALID_ARGUMENT", "{refused:?}");
    // A voxel is exactly `x`, `y` and `z` (review fix pass 1, item 3; the
    // reviewer's mutant M08): a fourth member, or one missing, is refused.
    let voxel = format!("{{\"x\":{cx},\"y\":{cy},\"z\":{cz}}}");
    for odd in [
        format!("{{\"x\":{cx},\"y\":{cy},\"z\":{cz},\"w\":0}}"),
        format!("{{\"x\":{cx},\"y\":{cy}}}"),
    ] {
        let params = compose("b_00", [cx, cy, cz], [cx + 1, cy, cz]).replacen(&voxel, &odd, 1);
        assert_ne!(params, compose("b_00", [cx, cy, cz], [cx + 1, cy, cz]));
        let refused = call_in_lull(&mut surface, &token, &mut left, "compose_wall_box", &params);
        assert_eq!(code(&refused), "INVALID_ARGUMENT", "{odd}: {refused:?}");
        assert!(message(&refused).contains("is a voxel"), "{odd}");
    }
}

/// Item 4: a beacon that is not the caller's own, a foreign handle and a name
/// that names nothing are one `NOT_FOUND`, worded alike; the other seat's own
/// `b_00` is its own beacon and composes for it alone.
#[test]
fn compose_wall_box_answers_only_about_the_callers_own_beacons() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let (_, [cx, cy, cz]) = support::core_beacon(&surface, 0);
    let (_, [ox, oy, oz]) = support::core_beacon(&surface, 1);
    // Seat 1's core, asked about by seat 0: as `b_00` it names seat 0's own
    // core, far from those corners; as a handle or an unknown name, nothing.
    let near_theirs = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "compose_wall_box",
        &compose("b_00", [ox + 1, oy, oz], [ox + 2, oy, oz]),
    );
    assert_eq!(
        code(&near_theirs),
        "INVALID_ARGUMENT",
        "seat 0's own sphere is not there"
    );
    let mut words = Vec::new();
    for name in ["e_01", "e_00", "b_09", "b_", "x"] {
        let refused = call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "compose_wall_box",
            &compose(name, [cx + 1, cy, cz], [cx + 2, cy, cz]),
        );
        assert_eq!(code(&refused), "NOT_FOUND", "{name}");
        words.push(message(&refused).replace(name, "NAME"));
    }
    words.dedup();
    assert_eq!(words.len(), 1, "one refusal for every name: {words:?}");
    let theirs = seat_token(&mut surface, 1);
    let own = call_in_lull(
        &mut surface,
        &theirs,
        &mut left,
        "compose_wall_box",
        &compose("b_00", [ox + 2, oy + 2, oz], [ox + 3, oy + 2, oz]),
    );
    let _ = result(&own, "seat 1's own core composes");
}

/// Review fix pass 1, item 5: a beacon of the seat's own that is dead while
/// the table still holds its row names no beacon to `compose_wall_box`,
/// exactly as to `get_beacon`: one `NOT_FOUND`, worded as for a name that
/// names nothing. A seat's core is the row the sim never frees (and any
/// beacon's row is held while kill credit is owed), so the probe kills seat
/// 0's core in a three-seat match and asks in the next Lull.
#[test]
fn compose_wall_box_answers_a_dead_beacon_as_get_beacon_does() {
    use pharmakos_sim::math::quantity::Damage;
    use pharmakos_sim::tables::SeatId;
    use pharmakos_sim::world::{DamageOrder, DamageTarget};

    let mut surface = hosted(3, SEGMENT_MS, 3);
    let (core, [cx, cy, cz]) = support::core_beacon(&surface, 0);
    assert!(surface.begin_push().expect("the Push begins"));
    surface
        .host_mut()
        .expect("hosted")
        .file_damage(DamageOrder {
            target: DamageTarget::Beacon(core),
            amount: Damage::new(i64::from(i32::MAX)).expect("a positive amount"),
            by: SeatId::new(1),
            source: None,
        })
        .expect("the damage queue takes the order");
    let _ = support::step(&mut surface, 1_000);
    while !surface.end_recap().expect("the recap closes") {
        let _ = support::step(&mut surface, 1);
    }
    let mut left = FIRST_LULL_MS;
    let token = seat_token(&mut surface, 0);
    let (alive, held) = {
        let beacons = surface.host().expect("hosted").world().beacons();
        let row = beacons.row_of_ordinal(SeatId::new(0), 0);
        (
            row.and_then(|row| beacons.hit_points().get(row).copied())
                .is_some_and(pharmakos_sim::math::quantity::Hp::is_alive),
            row.is_some(),
        )
    };
    assert!(!alive, "the core is dead");
    assert!(held, "its row is still held in the Lull");
    let mut answers = Vec::new();
    for name in ["b_00", "b_09"] {
        let built = call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "compose_wall_box",
            &compose(name, [cx + 2, cy, cz], [cx + 3, cy, cz]),
        );
        let seen = call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "get_beacon",
            &format!("{{\"beacon_id\":{}}}", quote(name)),
        );
        assert_eq!(code(&built), "NOT_FOUND", "{name}: {built:?}");
        assert_eq!(code(&seen), "NOT_FOUND", "{name}: {seen:?}");
        answers.push(message(&built).replace(name, "NAME"));
    }
    answers.dedup();
    assert_eq!(answers.len(), 1, "{answers:?}");
}

/// Decisions-log item 177 (3): `retreat_hp_pct` carries presence. A "0"
/// typed at a settings row's unwritten `retreat_hp_pct` writes the present 0,
/// and `render_plan` then says it as retreat off, with its chip at that
/// pointer holding 0.
#[test]
fn a_typed_zero_retreat_is_written_and_said_as_retreat_off() {
    let visit = ONE_RULE.replace(
        r#"{"label": "wait", "hold": {"ms": 90000}}"#,
        r#"{"label": "visit", "interface": {"beacon": {"beacon_id": "b_00"}, "rows": [{"set_mandate_settings": {"roe": "HOLD_FIRE"}}]}, "timeout_ms": 120000}"#,
    );
    assert_ne!(visit, ONE_RULE, "the visit replaced the hold");
    let pointer = "/declarative/route/0/interface/rows/0/set_mandate_settings/retreat_hp_pct";
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let answer = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "set_value_text",
            &format!(
                "{{\"playbook_jsonc\":{},\"pointer\":{},\"typed\":\"0\"}}",
                quote(&visit),
                quote(pointer)
            ),
        ),
        "set_value_text",
    );
    let patched = text_of(&answer, "playbook_jsonc");
    assert!(patched.contains("\"retreat_hp_pct\":0"), "{patched}");
    let chip = chip_value(&mut surface, &patched, pointer);
    assert_eq!(text_of(&chip, "value_json"), "0");
    assert_eq!(text_of(&chip, "value_text"), "0");
    let rendered = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "render_plan",
            &format!("{{\"playbook_jsonc\":{}}}", quote(&patched)),
        ),
        "render_plan",
    );
    let prose = text_of(&rendered, "prose");
    assert!(prose.contains("retreat off (0%)"), "{prose}");
}
