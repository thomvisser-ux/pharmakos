// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's planning wire, plan-core's half (S3's `fixq`): a value typed
//! as text and converted by plan-core (`edit::set_value_text`, item 7), an
//! empty rule added (`edit::new_rule`, item 8), a chip at a proto3 default
//! that an edit adds (item 11), a chip that says it lies outside its shape
//! (item 14), and a file that writes its members under their camelCase JSON
//! names, whose chips and slots edit through `patch_plan` (item 10).

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a panic is this file's failure report; clippy.toml's allow-expect-in-tests does \
              not reach an integration test's helpers"
)]

use std::path::{Path, PathBuf};

use pharmakos_plan_core::edit::{NewRuleError, SetValueError, new_rule, set_value_text};
use pharmakos_plan_core::shape::{Shape, Typed};
use pharmakos_plan_core::{Chip, PickOp, PlanContext, decode_jsonc, patch_text, render};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::Snapshot;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn rules() -> RulesTable {
    RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table")
}

fn context() -> PlanContext {
    let snapshot = Snapshot {
        seat_id: vec![0],
        seat_treasury: vec![200],
        seat_supply: vec![10],
        seat_draw: vec![2],
        ..Snapshot::default()
    };
    PlanContext::from_snapshot(&snapshot, 0).expect("seat 0")
}

/// A playbook of one hold step and one rule: the rule waits on a commander
/// hit-point test whose percentage the file leaves at its default, and a
/// cooldown of 30 s.
const ONE_RULE: &str = r#"{
  "schema_version": {"major": 1},
  "meta": {"title": "Edits", "author_kind": "HUMAN"},
  "declarative": {
    "route": [{"label": "wait", "hold": {"ms": 90000}}],
    "handlers": [
      // The rule the tests edit.
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

fn chips(text: &str) -> Vec<Chip> {
    let canonical = decode_jsonc(text).expect("a canonical form");
    render(canonical.decoded().message(), &context(), &rules())
        .expect("a rendering")
        .rules
        .into_iter()
        .flat_map(|rule| rule.chips)
        .collect()
}

fn chip_at(text: &str, pointer: &str) -> Chip {
    chips(text)
        .into_iter()
        .find(|chip| chip.pointer == pointer)
        .unwrap_or_else(|| panic!("a chip at {pointer}"))
}

fn typed(text: &str, pointer: &str, value: &str) -> Result<(String, String), SetValueError> {
    set_value_text(text, pointer, value, &context(), &rules())
}

/// A duration chip says its value in seconds and is shown in them; the
/// typed seconds land as milliseconds, exactly, and the undo gives the file
/// back byte for byte.
#[test]
fn a_duration_is_typed_in_seconds_and_lands_in_milliseconds() {
    let pointer = "/declarative/route/0/hold/ms";
    let chip = chip_at(ONE_RULE, pointer);
    assert_eq!(chip.value_json, "90000");
    assert_eq!(chip.value_text, "90");
    assert_eq!(chip.shape.display_unit(), Some("s"));
    assert_eq!(chip.pick, PickOp::Replace);
    assert!(!chip.outside);

    for (typed_text, ms) in [
        ("3", 3000),
        ("1.5", 1500),
        (" 45.250 ", 45250),
        ("12.5000", 12500),
    ] {
        let (patched, inverse) = typed(ONE_RULE, pointer, typed_text).expect("typed");
        let read = chip_at(&patched, pointer);
        assert_eq!(read.value_json, ms.to_string(), "{typed_text}");
        let (undone, _) = patch_text(&patched, &inverse).expect("the undo applies");
        assert_eq!(
            undone, ONE_RULE,
            "{typed_text}: the undo gives the file back"
        );
    }
    // The comment above the rule survives the edit.
    let (patched, _) = typed(ONE_RULE, pointer, "3").expect("typed");
    assert!(patched.contains("// The rule the tests edit."), "{patched}");
}

/// Every refusal is typed and says what was typed; none writes anything.
#[test]
fn a_typed_value_is_refused_by_name_never_rounded_or_clamped() {
    let cooldown = "/declarative/handlers/0/cooldown_ms";
    let refused = |pointer: &str, value: &str| typed(ONE_RULE, pointer, value).expect_err(value);
    assert_eq!(
        refused(cooldown, "abc"),
        SetValueError::Typed(Typed::NotANumber)
    );
    assert_eq!(refused(cooldown, ""), SetValueError::Typed(Typed::Empty));
    for odd in ["+3", "1e3", "1,5", "1.", ".5", "- 3", "3s", "0x10"] {
        assert_eq!(
            refused(cooldown, odd),
            SetValueError::Typed(Typed::NotANumber),
            "{odd}"
        );
    }
    assert_eq!(
        refused(cooldown, "1.0005"),
        SetValueError::Typed(Typed::FinerThanAMillisecond)
    );
    // Below the rules table's floor, said in seconds.
    let floor = pharmakos_verifier::Limits::from_rules(&rules())
        .expect("limits")
        .handler_cooldown_min_ms();
    let Err(SetValueError::Typed(Typed::BelowLeast(least, unit))) = typed(ONE_RULE, cooldown, "0")
    else {
        panic!("below the floor");
    };
    assert_eq!(
        least,
        pharmakos_plan_core::shape::shown_text("ms", i64::from(floor))
    );
    assert_eq!(unit, "s");
    // A percentage takes whole numbers only, at most 100.
    let fires = "/declarative/handlers/0/max_fires";
    assert_eq!(refused(fires, "1.5"), SetValueError::Typed(Typed::NotWhole));
    assert!(matches!(
        refused("/declarative/handlers/0/when/cmdr_hp_pct/pct", "140"),
        SetValueError::Typed(Typed::AboveMost(most, "%")) if most == "100"
    ));
    // Past every integer: refused, never wrapped.
    assert_eq!(
        refused(cooldown, "99999999999999999999"),
        SetValueError::Typed(Typed::TooLarge)
    );
    // Within the shape (no ceiling) but past the field's `int32`.
    assert_eq!(
        refused(cooldown, "2147484"),
        SetValueError::DoesNotFit,
        "2147484 s is 2147484000 ms, past an int32"
    );
    // A pointer no number stands at, and a choice chip.
    assert_eq!(
        refused("/declarative/handlers/0/id", "3"),
        SetValueError::NoNumberHere
    );
    assert_eq!(
        refused("/declarative/handlers/0/when/cmdr_hp_pct/cmp", "3"),
        SetValueError::NoNumberHere
    );
}

/// A value the file leaves at its proto3 default has a chip whose op is
/// `add`, which writes it; one outside its shape says so (items 11 and 14).
#[test]
fn a_default_value_has_a_chip_that_adds_it() {
    let pointer = "/declarative/handlers/0/when/cmdr_hp_pct/pct";
    let chip = chip_at(ONE_RULE, pointer);
    assert_eq!(chip.value_json, "0");
    assert_eq!(chip.pick, PickOp::Add);
    assert!(!chip.outside, "0 % is inside 0 to 100");
    let (patched, inverse) = typed(ONE_RULE, pointer, "40").expect("typed");
    assert!(
        patched.contains("\"pct\": 40") || patched.contains("\"pct\":40"),
        "{patched}"
    );
    assert_eq!(chip_at(&patched, pointer).pick, PickOp::Replace);
    let (undone, _) = patch_text(&patched, &inverse).expect("the undo applies");
    assert_eq!(undone, ONE_RULE);

    // A hold of zero is outside the verifier's least, 1 ms, and says so.
    let zero = ONE_RULE.replace("\"hold\": {\"ms\": 90000}", "\"hold\": {}");
    let hold = chip_at(&zero, "/declarative/route/0/hold/ms");
    assert_eq!(hold.pick, PickOp::Add);
    assert!(hold.outside);
    assert_eq!(hold.value_text, "0");

    // An unspecified comparison is a choice's value no menu offers.
    let unset = ONE_RULE.replace("{\"cmp\": \"LE\"}", "{}");
    let cmp = chip_at(&unset, "/declarative/handlers/0/when/cmdr_hp_pct/cmp");
    assert_eq!(cmp.pick, PickOp::Add);
    assert!(cmp.outside);
    let Shape::Choice { options } = &cmp.shape else {
        panic!("a choice");
    };
    assert!(options.iter().all(|option| !option.title.contains('"')));
    assert!(
        options.iter().all(|option| option.json != cmp.value_json),
        "the zero is not offered"
    );
}

/// Item 10: a file that writes its members under their camelCase JSON names
/// has the same chips and slots, at the canonical pointers, and an edit at
/// one lands on the member the file writes, never beside it.
#[test]
fn a_camel_case_file_edits_through_its_chips_and_slots() {
    let camel = ONE_RULE
        .replace("\"cooldown_ms\"", "\"cooldownMs\"")
        .replace("\"max_fires\"", "\"maxFires\"")
        .replace("\"cmdr_hp_pct\"", "\"cmdrHpPct\"")
        .replace("\"on_death\"", "\"onDeath\"")
        .replace("\"on_respawn\"", "\"onRespawn\"");
    let snake = chips(ONE_RULE);
    let camel_chips = chips(&camel);
    assert_eq!(
        snake.iter().map(|chip| &chip.pointer).collect::<Vec<_>>(),
        camel_chips
            .iter()
            .map(|chip| &chip.pointer)
            .collect::<Vec<_>>()
    );
    let (patched, _) = typed(&camel, "/declarative/handlers/0/cooldown_ms", "40").expect("typed");
    assert!(patched.contains("\"cooldownMs\": 40000"), "{patched}");
    assert!(!patched.contains("cooldown_ms"), "{patched}");
    let (patched, _) =
        typed(&camel, "/declarative/handlers/0/when/cmdr_hp_pct/pct", "30").expect("typed");
    assert!(
        patched.contains("\"cmdrHpPct\"") && !patched.contains("cmdr_hp_pct"),
        "{patched}"
    );
    assert!(patched.contains("30"), "{patched}");
    decode_jsonc(&patched).expect("still one field per name");

    // A slot pick at the canonical pointer replaces the camelCase member.
    let pick = r#"[{"op":"replace","path":"/declarative/handlers/0/when","value":{"cmdr_deaths":{"deaths":{"op":"GE","value":1}}}}]"#;
    let (picked, _) = patch_text(&camel, pick).expect("the pick lands");
    let read = decode_jsonc(&picked).expect("a canonical form");
    assert!(
        read.decoded()
            .message()
            .declarative
            .as_ref()
            .is_some_and(|body| body
                .handlers
                .first()
                .and_then(|rule| rule.when.as_ref())
                .is_some_and(|when| matches!(
                    when.node,
                    Some(pharmakos_proto::gp::v1::condition::Node::CmdrDeaths(_))
                ))),
        "{picked}"
    );
}

/// Item 8: a new rule is an `add` at the end of the rules, with the least
/// free id, the rules table's least cooldown and an empty condition slot; its
/// undo takes it out again; a full playbook is refused.
#[test]
fn a_new_rule_is_added_at_the_end_with_a_fresh_id() {
    let made = new_rule(ONE_RULE, &rules()).expect("a rule");
    assert_eq!(made.pointer, "/declarative/handlers/1");
    assert_eq!(made.id, "rule_2");
    let canonical = decode_jsonc(&made.playbook_jsonc).expect("a canonical form");
    let body = canonical
        .decoded()
        .message()
        .declarative
        .clone()
        .expect("declarative");
    let rule = body.handlers.get(1).expect("the new rule");
    let floor = pharmakos_verifier::Limits::from_rules(&rules())
        .expect("limits")
        .handler_cooldown_min_ms();
    assert_eq!(rule.id, "rule_2");
    assert_eq!(rule.cooldown_ms, floor);
    assert_eq!(rule.max_fires, 1);
    assert!(rule.when.is_none() && rule.body.is_empty());
    let rendered = render(canonical.decoded().message(), &context(), &rules()).expect("rendered");
    let slot = rendered
        .rules
        .iter()
        .find(|row| row.pointer == made.pointer)
        .and_then(|row| row.slots.first())
        .expect("the new rule's slot");
    assert_eq!(slot.pointer, "/declarative/handlers/1/when");
    assert_eq!(slot.pick, Some(PickOp::Add));
    let (undone, _) = patch_text(&made.playbook_jsonc, &made.inverse_json_patch).expect("undo");
    assert_eq!(undone, ONE_RULE);

    // An id a rule already holds is skipped.
    let taken = ONE_RULE.replace("\"id\": \"flee\"", "\"id\": \"rule_2\"");
    assert_eq!(new_rule(&taken, &rules()).expect("a rule").id, "rule_3");

    // A file with no rules, and one with no declarative block, take one.
    let none = ONE_RULE.replace(
        &ONE_RULE[ONE_RULE.find(",\n    \"handlers\"").expect("handlers")
            ..ONE_RULE.find("\n  },\n  \"on_death\"").expect("end")],
        "",
    );
    let made = new_rule(&none, &rules()).expect("a rule");
    assert_eq!(made.pointer, "/declarative/handlers/0");
    assert_eq!(made.id, "rule_1");
    decode_jsonc(&made.playbook_jsonc).expect("a canonical form");

    // The sim's ceiling, which the verifier's E0113 reads.
    let mut full = String::from(
        "{\"schema_version\":{\"major\":1},\"meta\":{\"title\":\"Full\"},\"declarative\":{\"handlers\":[",
    );
    for index in 0..pharmakos_sim::interpreter::MAX_HANDLERS {
        if index > 0 {
            full.push(',');
        }
        full.push_str("{\"id\":\"r");
        full.push_str(&index.to_string());
        full.push_str("\"}");
    }
    full.push_str("]},\"kind\":\"PLAYBOOK\"}");
    assert_eq!(
        new_rule(&full, &rules()),
        Err(NewRuleError::Full(pharmakos_sim::interpreter::MAX_HANDLERS))
    );
}

/// Every patch these edits compose is one the strict reader takes: what
/// `patch_text` applied came through `Patch::from_text`, so a successful
/// edit is the proof; a broken file is refused with its own error.
#[test]
fn a_file_with_no_canonical_form_is_refused() {
    assert!(matches!(
        typed("{", "/declarative/handlers/0/cooldown_ms", "3"),
        Err(SetValueError::NoCanonicalForm(_))
    ));
    assert!(matches!(
        new_rule("{", &rules()),
        Err(NewRuleError::NoCanonicalForm(_))
    ));
}

/// Review fix pass 1, item 2: a pointer that names no schema field takes no
/// typed number (it is not "past what this value can hold"), and a bound in
/// a unit with no name is said with no trailing space.
#[test]
fn a_pointer_naming_no_field_takes_no_number_and_a_bare_count_has_no_trailing_space() {
    let nowhere = "/nowhere/set_mandate_settings/retreat_hp_pct";
    assert_eq!(
        typed(ONE_RULE, nowhere, "40"),
        Err(SetValueError::NoNumberHere)
    );
    let fires = "/declarative/handlers/0/max_fires";
    let error = typed(ONE_RULE, fires, "0").expect_err("below the least");
    let words = pharmakos_plan_core::edit::set_value_words(&error, "0");
    assert_eq!(
        words.as_str(),
        "`0` is below the least this takes, 1",
        "no trailing space after a bound with no unit"
    );
    let error = typed(ONE_RULE, "/declarative/route/0/hold/ms", "0").expect_err("below 1 ms");
    assert_eq!(
        pharmakos_plan_core::edit::set_value_words(&error, "0").as_str(),
        "`0` is below the least this takes, 0.001 s"
    );
}

/// Review fix pass 1, item 3: a `uint32` field takes a value past `i32::MAX`
/// and up to `u32::MAX`, and refuses one past that (the reviewer's mutant
/// M07 read a `uint32` as an `int32`).
#[test]
fn a_uint32_field_takes_its_whole_range() {
    let near = ONE_RULE.replace(
        r#"{"cmdr_hp_pct": {"cmp": "LE"}}"#,
        r#"{"enemy_units_near": {"near": {"safest": {}}, "radius_voxels": 24, "count": {"op": "GE", "value": 1}, "freshness": {"kind": "VISIBLE_NOW"}}}"#,
    );
    let radius = "/declarative/handlers/0/when/enemy_units_near/radius_voxels";
    let (patched, _) = typed(&near, radius, "4294967295").expect("a uint32's most");
    assert!(patched.contains("4294967295"), "{patched}");
    let (patched, _) = typed(&near, radius, "3000000000").expect("past an int32");
    assert!(patched.contains("3000000000"), "{patched}");
    assert_eq!(
        typed(&near, radius, "4294967296"),
        Err(SetValueError::DoesNotFit)
    );
}

/// Review fix pass 1, item 4: a `.` is read only where the display unit has
/// a fraction (a duration). In a whole-number unit "40.0" is refused, typed,
/// never read as 40.
#[test]
fn a_point_is_read_only_in_a_unit_with_a_fraction() {
    let pct = "/declarative/handlers/0/when/cmdr_hp_pct/pct";
    for odd in ["40.0", "40.", "40.00", "40.5"] {
        assert_eq!(
            typed(ONE_RULE, pct, odd),
            Err(SetValueError::Typed(Typed::NotWhole)),
            "{odd}"
        );
    }
    let (patched, _) = typed(ONE_RULE, "/declarative/route/0/hold/ms", "2.0").expect("seconds");
    assert!(patched.contains("\"ms\": 2000"), "{patched}");
}
