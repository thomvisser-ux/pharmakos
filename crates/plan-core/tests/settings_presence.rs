// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decisions-log item 184 (10): `scout_count`, `dig_max_depth` and
//! `pillar_spacing` carry presence, and four lists have a `clear_*` flag.
//!
//! A written 0 and a set clear survive the canonical form and the JSONC round
//! trip with their comments, and move the plan fingerprint, because each is a
//! different playbook from one that leaves the field out; a `clear_*: false`
//! is the same playbook as one that leaves it out, and the canonical form
//! never writes it. `render_plan` says a written 0 and a set clear with a
//! chip at the pointer, and `set_value_text` writes them where they are
//! unwritten. A seeded property test holds presence in to presence out over
//! edit rows, through the codec, the canonical form and the sim's compile.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, but that configuration only recognises \
              #[test] functions and #[cfg(test)] modules, not an integration test's helper \
              functions. A panic is this file's failure report."
)]

use std::path::Path;

use pharmakos_plan_core::edit::set_value_text;
use pharmakos_plan_core::{
    PlanContext, Rendering, canonicalise_text, decode_jsonc, patch_text, render,
};
use pharmakos_proto::gp::v1::{
    InterfaceRow, MandateSettings, Playbook, interface_row, mandate_settings, step,
};
use pharmakos_sim::hash::plan_fingerprint_of;
use pharmakos_sim::interpreter::{Action, Plan, Row};
use pharmakos_sim::math::random::{Stream, StreamRng};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::Snapshot;

/// The row's settings, as a pointer into the playbook.
const ROW: &str = "/declarative/route/0/interface/rows/0/set_mandate_settings";

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
  "meta": {{"title": "presence", "author_kind": "HUMAN"}},
  "kind": "PLAYBOOK",
  "declarative": {{
    "route": [
      {{"label": "visit", "interface": {{"beacon": {{"beacon_id": "b_00"}}, "rows": [
        {{"set_mandate_settings": {{
          // Written on purpose.
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

fn settings_of(playbook: &Playbook) -> &MandateSettings {
    match first_row(playbook).row.as_ref() {
        Some(interface_row::Row::SetMandateSettings(settings)) => settings,
        other => panic!("a settings row: {other:?}"),
    }
}

/// Each of the seven as a written 0 or a set clear, with the arm it lives in
/// and its member's name.
const WRITTEN: [(&str, &str, &str); 7] = [
    ("survey", "scout_count", "0"),
    ("mine", "dig_max_depth", "0"),
    ("mine", "pillar_spacing", "0"),
    ("build", "clear_protected_areas", "true"),
    ("defend", "clear_protect", "true"),
    ("attack", "clear_avoid_areas", "true"),
    ("survey", "clear_probe_areas", "true"),
];

#[test]
fn a_written_zero_or_a_clear_survives_the_canonical_form_and_moves_the_fingerprint() {
    for (arm, member, value) in WRITTEN {
        let written = playbook(&format!(
            r#""roe": "HOLD_FIRE", "{arm}": {{"{member}": {value}}}"#
        ));
        let canonical = canonicalise_text(&written).expect("canonicalises");
        assert!(
            canonical.json.contains(&format!("\"{member}\": {value}")),
            "the canonical JSON dropped {member}:\n{}",
            canonical.json
        );
        assert!(
            canonical.text.contains(&format!("\"{member}\""))
                && canonical.text.contains("// Written on purpose."),
            "the canonical JSONC dropped {member} or its comment:\n{}",
            canonical.text
        );
        let again = canonicalise_text(&canonical.text).expect("the canonical text canonicalises");
        assert_eq!(again.text, canonical.text, "{member}: a fixed point");

        let omitted =
            canonicalise_text(&playbook(&format!(r#""roe": "HOLD_FIRE", "{arm}": {{}}"#)))
                .expect("canonicalises");
        assert_ne!(
            plan_fingerprint_of(decode_jsonc(&canonical.text).expect("decodes").decoded())
                .expect("a fingerprint"),
            plan_fingerprint_of(decode_jsonc(&omitted.text).expect("decodes").decoded())
                .expect("a fingerprint"),
            "{member}: written and omitted are two plans"
        );
    }
}

#[test]
fn a_clear_written_false_is_the_playbook_that_leaves_it_out() {
    for (arm, member, _) in WRITTEN
        .iter()
        .filter(|(_, member, _)| member.starts_with("clear_"))
    {
        let written = canonicalise_text(&playbook(&format!(
            r#""roe": "HOLD_FIRE", "{arm}": {{"{member}": false}}"#
        )))
        .expect("canonicalises");
        let omitted =
            canonicalise_text(&playbook(&format!(r#""roe": "HOLD_FIRE", "{arm}": {{}}"#)))
                .expect("canonicalises");
        assert_eq!(
            written.json, omitted.json,
            "{member}: false is never written"
        );
    }
}

#[test]
fn render_plan_says_a_written_zero_and_a_clear_with_their_chips() {
    for ((arm, member, value), words) in WRITTEN.iter().zip([
        "no scouts (0)",
        "digging the exposed rim only (0)",
        "leaving no pillars (0)",
        "emptying its protected areas",
        "emptying its protect list",
        "emptying its avoid areas",
        "emptying its probe areas",
    ]) {
        let said = rendered(&playbook(&format!(r#""{arm}": {{"{member}": {value}}}"#)));
        assert!(said.prose.contains(words), "{member}: {}", said.prose);
        let pointer = format!("{ROW}/{arm}/{member}");
        let chip = said
            .rules
            .iter()
            .flat_map(|rule| &rule.chips)
            .find(|chip| chip.pointer == pointer)
            .unwrap_or_else(|| panic!("a chip at {pointer}"));
        assert_eq!(chip.value_json, *value, "{member}");

        let silent = rendered(&playbook(&format!(r#""{arm}": {{}}"#)));
        assert!(!silent.prose.contains(words), "{member}: {}", silent.prose);
        assert!(
            silent
                .rules
                .iter()
                .flat_map(|rule| &rule.chips)
                .all(|chip| chip.pointer != pointer),
            "{member}: an omitted one has no chip"
        );
    }
}

#[test]
fn a_typed_zero_or_a_composed_clear_writes_the_present_value() {
    for (arm, member, value) in WRITTEN {
        let pointer = format!("{ROW}/{arm}/{member}");
        let unwritten = playbook(&format!(r#""roe": "HOLD_FIRE", "{arm}": {{}}"#));
        // A number is typed through `set_value_text`, which reads the
        // vocabulary's shape where no chip stands; a clear is a boolean, whose
        // `add` the client composes from the vocabulary's shape and
        // `patch_plan` applies.
        let (patched, _) = if member.starts_with("clear_") {
            patch_text(
                &unwritten,
                &format!(r#"[{{"op":"add","path":"{pointer}","value":true}}]"#),
            )
            .unwrap_or_else(|error| panic!("{member}: the add applies: {error:?}"))
        } else {
            set_value_text(&unwritten, &pointer, value, &context(), &rules())
                .unwrap_or_else(|error| panic!("{member}: a typed {value} is taken: {error:?}"))
        };
        let canonical = canonicalise_text(&patched).expect("canonicalises");
        assert!(
            canonical.json.contains(&format!("\"{member}\": {value}")),
            "{member}: {}",
            canonical.json
        );
    }
}

/// What the sim's compile reads from one settings row: the three numbers and
/// whether each list is left, replaced or emptied.
#[derive(Debug, PartialEq, Eq)]
struct Read {
    scouts: Option<u8>,
    depth: Option<u32>,
    pillars: Option<u32>,
    lists: [Option<usize>; 4],
}

fn compiled(text: &str) -> Read {
    let decoded = decode_jsonc(text).expect("decodes");
    let plan = Plan::compile(decoded.decoded().message(), &rules()).expect("compiles");
    let Some(Action::Interface { rows, .. }) = plan.route().first().map(|step| &step.action) else {
        panic!("an interface step");
    };
    match rows.first() {
        Some(Row::Settings {
            protected,
            probes,
            scouts,
            mine,
            defend,
            attack,
            ..
        }) => Read {
            scouts: *scouts,
            depth: mine.as_ref().and_then(|edit| edit.dig_max_depth),
            pillars: mine.as_ref().and_then(|edit| edit.pillar_spacing),
            lists: [
                protected.as_ref().map(Vec::len),
                probes.as_ref().map(Vec::len),
                defend
                    .as_ref()
                    .and_then(|edit| edit.protect.as_ref().map(Vec::len)),
                attack
                    .as_ref()
                    .and_then(|edit| edit.avoid.as_ref().map(Vec::len)),
            ],
        },
        other => panic!("a settings row: {other:?}"),
    }
}

/// A seeded generator of `set_mandate_settings` rows and what the compile
/// must read from each.
struct Rows {
    rng: StreamRng,
}

impl Rows {
    /// 0 to `most`, inclusive.
    fn draw(&mut self, most: u32) -> u32 {
        let most = i32::try_from(most).expect("small");
        u32::try_from(self.rng.range_i32(0, most)).expect("non-negative")
    }

    /// A number with presence: left out, a written 0 or a written 1 to 10.
    fn number(&mut self, members: &mut Vec<String>, member: &str) -> Option<u32> {
        let value = match self.draw(2) {
            0 => return None,
            1 => 0,
            _ => self.draw(9) + 1,
        };
        members.push(format!(r#""{member}": {value}"#));
        Some(value)
    }

    /// A list: left out, cleared, or written with one or two entries; the
    /// count the compile must read (`Some(0)` for a clear).
    fn list(&mut self, members: &mut Vec<String>, name: &str) -> Option<usize> {
        let entry = if name == "protect" {
            r#"{"my_commander":{}}"#
        } else {
            r#"{"min":{"x":1,"y":1,"z":1},"max":{"x":2,"y":2,"z":2}}"#
        };
        match self.draw(3) {
            0 => None,
            1 => {
                members.push(format!(r#""clear_{name}": true"#));
                Some(0)
            }
            count => {
                let entries = vec![entry; usize::try_from(count - 1).expect("small")];
                members.push(format!(r#""{name}": [{}]"#, entries.join(",")));
                Some(entries.len())
            }
        }
    }

    /// One row's text and what the compile must read from it.
    fn next(&mut self) -> (String, Read) {
        let mut members: Vec<String> = Vec::new();
        let mut expected = Read {
            scouts: None,
            depth: None,
            pillars: None,
            lists: [None; 4],
        };
        let name = match self.draw(4) {
            0 => {
                expected.scouts = self
                    .number(&mut members, "scout_count")
                    .map(|n| u8::try_from(n).expect("small"));
                expected.lists[1] = self.list(&mut members, "probe_areas");
                "survey"
            }
            1 => {
                expected.depth = self.number(&mut members, "dig_max_depth");
                expected.pillars = self.number(&mut members, "pillar_spacing");
                "mine"
            }
            2 => {
                expected.lists[0] = self.list(&mut members, "protected_areas");
                "build"
            }
            3 => {
                expected.lists[2] = self.list(&mut members, "protect");
                "defend"
            }
            _ => {
                expected.lists[3] = self.list(&mut members, "avoid_areas");
                "attack"
            }
        };
        let text = playbook(&format!(
            r#""roe": "HOLD_FIRE", "{name}": {{{}}}"#,
            members.join(", ")
        ));
        (text, expected)
    }
}

/// The three numbers as the decoded message holds them.
fn numbers_of(playbook: &Playbook) -> (Option<u32>, Option<u32>, Option<u32>) {
    match settings_of(playbook).mandate.as_ref() {
        Some(mandate_settings::Mandate::Survey(survey)) => (survey.scout_count, None, None),
        Some(mandate_settings::Mandate::Mine(mine)) => {
            (None, mine.dig_max_depth, mine.pillar_spacing)
        }
        _ => (None, None, None),
    }
}

/// The property: over seeded edit rows, a field present in the text is
/// present, with its value, after the codec, the canonical form and the
/// sim's compile; a field left out is absent at every stage; a set clear is
/// an emptied list.
#[test]
fn presence_in_is_presence_out_through_the_codec_the_canonical_form_and_the_compile() {
    let mut rows = Rows {
        rng: StreamRng::new(0x0184_0010, Stream::Map, 0, 0, 0),
    };
    for case in 0..400 {
        let (text, expected) = rows.next();
        let canonical = canonicalise_text(&text).expect("canonicalises");
        assert_eq!(
            numbers_of(&canonical.playbook),
            (
                expected.scouts.map(u32::from),
                expected.depth,
                expected.pillars
            ),
            "case {case}: {text}"
        );
        assert_eq!(
            compiled(&canonical.text),
            expected,
            "case {case}: {}",
            canonical.text
        );
        assert_eq!(compiled(&text), expected, "case {case}: {text}");
    }
}
