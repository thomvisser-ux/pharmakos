// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A written `retreat_hp_pct` of 0 survives the canonical form and the JSONC
//! round trip, comments included, and moves the plan fingerprint, because it
//! is a different playbook from one that leaves the field out (decisions-log
//! item 177 (3): the field carries presence, and a written 0 is retreat off).
//! `render_plan` says a written 0 as retreat off, with a chip at its pointer,
//! and `set_value_text` of "0" writes the present 0.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, but that configuration only recognises \
              #[test] functions and #[cfg(test)] modules, not an integration test's helper \
              functions. A panic is this file's failure report."
)]

use std::path::Path;

use pharmakos_plan_core::edit::set_value_text;
use pharmakos_plan_core::{
    PickOp, PlanContext, Rendering, canonicalise_text, decode_jsonc, render,
};
use pharmakos_proto::gp::v1::{InterfaceRow, Playbook, interface_row, step};
use pharmakos_sim::hash::plan_fingerprint_of;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::Snapshot;

/// The row's `retreat_hp_pct`, as a pointer into the playbook.
const RETREAT: &str = "/declarative/route/0/interface/rows/0/set_mandate_settings/retreat_hp_pct";

fn rules() -> RulesTable {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    RulesTable::load(&root.join("rules").join("rules.v1.json")).expect("the committed rules table")
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

fn rendered(text: &str) -> Rendering {
    let canonical = decode_jsonc(text).expect("a canonical form");
    render(canonical.decoded().message(), &context(), &rules()).expect("a rendering")
}

/// A playbook whose one step visits the core with one `set_mandate_settings`
/// row holding `settings`, a comment on the row's first member.
fn playbook(settings: &str) -> String {
    format!(
        r#"{{
  "schema_version": {{"major": 1}},
  "meta": {{"title": "retreat", "author_kind": "HUMAN"}},
  "kind": "PLAYBOOK",
  "declarative": {{
    "route": [
      {{"label": "visit", "interface": {{"beacon": {{"beacon_id": "b_00"}}, "rows": [
        {{"set_mandate_settings": {{
          // Retreat off: no unit of this beacon walks away from a fight.
          {settings}
        }}}}
      ]}}, "timeout_ms": 120000}}
    ]
  }},
  "on_death": {{"on_respawn": "CONTINUE", "max_deaths_before_fallback": 3}},
  "fallback": {{"hold": {{"at": {{"safest": {{}}}}}}}}
}}
"#
    )
}

/// The first row of the first step.
fn first_row(playbook: &Playbook) -> &InterfaceRow {
    let step = playbook
        .declarative
        .as_ref()
        .and_then(|declarative| declarative.route.first())
        .expect("a step");
    match step.kind.as_ref() {
        Some(step::Kind::Interface(visit)) => visit.rows.first().expect("a row"),
        other => panic!("an interface step: {other:?}"),
    }
}

fn retreat_of(playbook: &Playbook) -> Option<u32> {
    match first_row(playbook).row.as_ref() {
        Some(interface_row::Row::SetMandateSettings(settings)) => settings.retreat_hp_pct,
        other => panic!("a settings row: {other:?}"),
    }
}

#[test]
fn a_written_zero_survives_the_canonical_form_with_its_comment() {
    let text = playbook(r#""retreat_hp_pct": 0, "roe": "HOLD_FIRE""#);
    let canonical = canonicalise_text(&text).expect("canonicalises");
    assert_eq!(retreat_of(&canonical.playbook), Some(0));
    assert!(
        canonical.json.contains("\"retreat_hp_pct\": 0"),
        "the canonical JSON dropped a written 0:\n{}",
        canonical.json
    );
    assert!(
        canonical.text.contains("\"retreat_hp_pct\""),
        "the canonical JSONC dropped a written 0:\n{}",
        canonical.text
    );
    assert!(
        canonical.text.contains("// Retreat off"),
        "the comment did not survive:\n{}",
        canonical.text
    );
    // The canonical form is a fixed point: a second pass changes nothing.
    let again = canonicalise_text(&canonical.text).expect("the canonical text canonicalises");
    assert_eq!(again.text, canonical.text);
    assert_eq!(again.json, canonical.json);
    // And the one JSONC decode the gateway seals through reads it the same.
    let decoded = decode_jsonc(&canonical.text).expect("decodes");
    assert_eq!(retreat_of(decoded.decoded().message()), Some(0));
}

#[test]
fn a_written_zero_and_an_omitted_field_are_two_playbooks() {
    let written = canonicalise_text(&playbook(r#""retreat_hp_pct": 0, "roe": "HOLD_FIRE""#))
        .expect("canonicalises");
    let omitted = canonicalise_text(&playbook(r#""roe": "HOLD_FIRE""#)).expect("canonicalises");
    assert_eq!(retreat_of(&omitted.playbook), None);
    assert_ne!(written.json, omitted.json);
    assert_ne!(
        plan_fingerprint_of(decode_jsonc(&written.text).expect("decodes").decoded())
            .expect("a fingerprint"),
        plan_fingerprint_of(decode_jsonc(&omitted.text).expect("decodes").decoded())
            .expect("a fingerprint"),
        "a written 0 is a different plan from one that leaves the field out"
    );
}

#[test]
fn render_plan_says_a_written_zero_as_retreat_off_with_its_chip() {
    let off = rendered(&playbook(r#""retreat_hp_pct": 0"#));
    assert!(
        off.prose.contains("retreat off (0%)"),
        "a written 0 is said as retreat off:
{}",
        off.prose
    );
    let chip = off
        .rules
        .iter()
        .flat_map(|rule| &rule.chips)
        .find(|chip| chip.pointer == RETREAT)
        .expect("a chip at the written 0");
    assert_eq!(chip.value_json, "0");
    assert_eq!(chip.value_text, "0");
    assert_eq!(chip.pick, PickOp::Replace);

    let thirty = rendered(&playbook(r#""retreat_hp_pct": 30"#));
    assert!(
        thirty
            .prose
            .contains("units retreating below 30% of their hit points"),
        "{}",
        thirty.prose
    );

    let omitted = rendered(&playbook(r#""roe": "HOLD_FIRE""#));
    assert!(
        !omitted.prose.contains("retreat off") && !omitted.prose.contains("retreating"),
        "{}",
        omitted.prose
    );
    assert!(
        omitted
            .rules
            .iter()
            .flat_map(|rule| &rule.chips)
            .all(|chip| chip.pointer != RETREAT),
        "an omitted threshold has no chip"
    );
}

#[test]
fn a_typed_zero_writes_the_present_zero() {
    // Unwritten: the vocabulary's shape, and an `add`.
    let (patched, _) = set_value_text(
        &playbook(r#""roe": "HOLD_FIRE""#),
        RETREAT,
        "0",
        &context(),
        &rules(),
    )
    .expect("a typed 0 is taken");
    let decoded = decode_jsonc(&patched).expect("decodes");
    assert_eq!(
        retreat_of(decoded.decoded().message()),
        Some(0),
        "{patched}"
    );
    // Written: the chip's `replace`.
    let (patched, _) = set_value_text(
        &playbook(r#""retreat_hp_pct": 30"#),
        RETREAT,
        "0",
        &context(),
        &rules(),
    )
    .expect("a typed 0 is taken");
    let decoded = decode_jsonc(&patched).expect("decodes");
    assert_eq!(
        retreat_of(decoded.decoded().message()),
        Some(0),
        "{patched}"
    );
}
