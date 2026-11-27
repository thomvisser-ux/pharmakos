// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decisions-log item 184 (3): `scout_count`, `dig_max_depth` and
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
use pharmakos_proto::gp::v1::{MandateSettings, Playbook, interface_row, mandate_settings, step};
use pharmakos_sim::hash::plan_fingerprint_of;
use pharmakos_sim::interpreter::{Action, Plan, PlanError, Row};
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
        // The JSONC the player keeps drops it too, and the plan is the same.
        assert!(
            !written.text.contains(member),
            "{member}: the canonical JSONC kept a false:\n{}",
            written.text
        );
        assert_eq!(
            plan_fingerprint_of(decode_jsonc(&written.text).expect("decodes").decoded())
                .expect("a fingerprint"),
            plan_fingerprint_of(decode_jsonc(&omitted.text).expect("decodes").decoded())
                .expect("a fingerprint"),
            "{member}: false and left out are one plan"
        );
        // And the codec's own canonical form, which the verifier's size meter
        // and the gateway read, writes no false either.
        let message = match *arm {
            "build" => "gp.v1.BuildSettings",
            "defend" => "gp.v1.DefendSettings",
            "attack" => "gp.v1.AttackSettings",
            _ => "gp.v1.SurveySettings",
        };
        let codec =
            pharmakos_proto::json::canonicalise(message, &format!(r#"{{"{member}":false}}"#))
                .expect("canonicalises");
        assert!(!codec.contains(member), "{member}: {codec}");
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
#[derive(Debug, PartialEq, Eq, Default)]
struct Read {
    scouts: Option<u8>,
    depth: Option<u32>,
    pillars: Option<u32>,
    lists: [Option<usize>; 4],
}

/// The first step of the route, or of the first rule's body when the route
/// is only a hold.
fn first_step(plan: &Plan) -> &Action {
    match plan.route().first().map(|step| &step.action) {
        Some(action @ Action::Interface { .. }) => action,
        _ => plan
            .handlers()
            .first()
            .and_then(|rule| rule.body.first())
            .map(|step| &step.action)
            .expect("a rule's step"),
    }
}

/// What the compile reads from the case's one row, or its refusal.
fn compiled(text: &str) -> Result<Read, PlanError> {
    let decoded = decode_jsonc(text).expect("decodes");
    let plan = Plan::compile(decoded.decoded().message(), &rules())?;
    let Action::Interface { rows, .. } = first_step(&plan) else {
        panic!("an interface step");
    };
    let row = match rows.first() {
        Some(Row::Mandate { settings, .. }) => match settings.as_deref() {
            Some(row) => row,
            // A switch whose settings write nothing reads nothing.
            None => return Ok(Read::default()),
        },
        Some(row) => row,
        None => panic!("a row"),
    };
    match row {
        Row::Settings {
            protected,
            probes,
            scouts,
            mine,
            defend,
            attack,
            ..
        } => Ok(Read {
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
        }),
        other => panic!("a settings row: {other:?}"),
    }
}

/// How a generated row writes a list.
enum Written {
    Left,
    Clear,
    ClearBesideEntries,
    Entries(usize),
}

/// A seeded generator of settings rows, edits and switches, in the route or
/// in a rule's body, and what the compile must read from each or refuse.
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

    /// A list: left out, cleared, cleared beside entries, or written with one
    /// or two entries.
    fn list(&mut self, members: &mut Vec<String>, name: &str) -> Written {
        let entry = if name == "protect" {
            r#"{"my_commander":{}}"#
        } else {
            r#"{"min":{"x":1,"y":1,"z":1},"max":{"x":2,"y":2,"z":2}}"#
        };
        match self.draw(4) {
            0 => Written::Left,
            1 => {
                members.push(format!(r#""clear_{name}": true"#));
                Written::Clear
            }
            2 => {
                members.push(format!(r#""clear_{name}": true, "{name}": [{entry}]"#));
                Written::ClearBesideEntries
            }
            count => {
                let entries = vec![entry; usize::try_from(count - 2).expect("small")];
                members.push(format!(r#""{name}": [{}]"#, entries.join(",")));
                Written::Entries(entries.len())
            }
        }
    }

    /// One case's text and what the compile must answer: what it reads, or
    /// the list a refused clear names.
    fn next(&mut self) -> (String, Result<Read, &'static str>) {
        let switch = self.draw(1) == 1;
        let in_rule = self.draw(1) == 1;
        let mut members: Vec<String> = Vec::new();
        let mut expected = Read::default();
        let mut refused: Option<&'static str> = None;
        let mut take =
            |written: Written, list: &'static str, slot: &mut Option<usize>| match written {
                Written::Left => {}
                Written::Entries(count) => *slot = Some(count),
                Written::Clear if !switch => *slot = Some(0),
                Written::Clear | Written::ClearBesideEntries => refused = Some(list),
            };
        let name = match self.draw(4) {
            0 => {
                expected.scouts = self
                    .number(&mut members, "scout_count")
                    .map(|n| u8::try_from(n).expect("small"));
                let written = self.list(&mut members, "probe_areas");
                take(written, "probe_areas", &mut expected.lists[1]);
                "survey"
            }
            1 => {
                expected.depth = self.number(&mut members, "dig_max_depth");
                expected.pillars = self.number(&mut members, "pillar_spacing");
                "mine"
            }
            2 => {
                let written = self.list(&mut members, "protected_areas");
                take(written, "protected_areas", &mut expected.lists[0]);
                "build"
            }
            3 => {
                let written = self.list(&mut members, "protect");
                take(written, "protect", &mut expected.lists[2]);
                "defend"
            }
            _ => {
                let written = self.list(&mut members, "avoid_areas");
                take(written, "avoid_areas", &mut expected.lists[3]);
                "attack"
            }
        };
        let row = if switch {
            "set_mandate"
        } else {
            "set_mandate_settings"
        };
        let settings = format!(
            r#"{{"roe": "HOLD_FIRE", "{name}": {{{}}}}}"#,
            members.join(", ")
        );
        let answer = match refused {
            Some(list) => Err(list),
            None => Ok(expected),
        };
        (case_playbook(row, &settings, in_rule), answer)
    }
}

/// A playbook whose one visit to the core writes `settings` in a `row`, in
/// the route or in a rule's body.
fn case_playbook(row: &str, settings: &str, in_rule: bool) -> String {
    let visit = format!(
        r#"{{"label": "visit", "interface": {{"beacon": {{"beacon_id": "b_00"}}, "rows": [{{"{row}": {settings}}}]}}, "timeout_ms": 120000}}"#
    );
    let (route, handlers) = if in_rule {
        (
            r#"{"label": "wait", "hold": {"ms": 1000}}"#.to_owned(),
            format!(
                r#", "handlers": [{{"id": "h0", "when": {{"cmdr_hp_pct": {{"cmp": "LE", "pct": 40}}}}, "body": [{visit}], "resume": "CONTINUE", "cooldown_ms": 5000, "max_fires": 2}}]"#
            ),
        )
    } else {
        (visit, String::new())
    };
    format!(
        r#"{{
  "schema_version": {{"major": 1}},
  "meta": {{"title": "presence", "author_kind": "HUMAN"}},
  "kind": "PLAYBOOK",
  "declarative": {{"route": [{route}]{handlers}}},
  "on_death": {{"on_respawn": "CONTINUE", "max_deaths_before_fallback": 3}},
  "fallback": {{"hold": {{"at": {{"safest": {{}}}}}}}}
}}
"#
    )
}

/// The three numbers as the decoded message holds them, wherever the case's
/// one row stands.
fn numbers_of(playbook: &Playbook) -> (Option<u32>, Option<u32>, Option<u32>) {
    let declarative = playbook.declarative.as_ref().expect("a declarative body");
    let step = declarative
        .route
        .iter()
        .chain(
            declarative
                .handlers
                .iter()
                .flat_map(|rule| rule.body.iter()),
        )
        .find(|step| matches!(step.kind, Some(step::Kind::Interface(_))))
        .expect("an interface step");
    let Some(step::Kind::Interface(visit)) = step.kind.as_ref() else {
        panic!("an interface step");
    };
    let settings: &MandateSettings = match visit.rows.first().and_then(|row| row.row.as_ref()) {
        Some(
            interface_row::Row::SetMandateSettings(settings)
            | interface_row::Row::SetMandate(settings),
        ) => settings,
        other => panic!("a settings row: {other:?}"),
    };
    match settings.mandate.as_ref() {
        Some(mandate_settings::Mandate::Survey(survey)) => (survey.scout_count, None, None),
        Some(mandate_settings::Mandate::Mine(mine)) => {
            (None, mine.dig_max_depth, mine.pillar_spacing)
        }
        _ => (None, None, None),
    }
}

/// The property: over seeded rows (edits and switches, in the route and in a
/// rule), a field present in the text is present, with its value, after the
/// codec, the canonical form and the sim's compile; a field left out is
/// absent at every stage; a set clear is an emptied list in an edit; and a
/// clear the rules refuse (beside entries, or in a switch) is refused by the
/// compile as `ClearRefused` naming its list, before and after the canonical
/// form.
#[test]
fn presence_in_is_presence_out_through_the_codec_the_canonical_form_and_the_compile() {
    let mut rows = Rows {
        rng: StreamRng::new(0x0184_0010, Stream::Map, 0, 0, 0),
    };
    let mut refusals = 0_u32;
    for case in 0..600 {
        let (text, expected) = rows.next();
        let canonical = canonicalise_text(&text).expect("canonicalises");
        let answer = |text: &str| match compiled(text) {
            Ok(read) => Ok(read),
            Err(PlanError::ClearRefused { list, .. }) => Err(list),
            Err(other) => panic!("case {case}: an unexpected refusal {other:?}\n{text}"),
        };
        match &expected {
            Ok(read) => assert_eq!(
                numbers_of(&canonical.playbook),
                (read.scouts.map(u32::from), read.depth, read.pillars),
                "case {case}: {text}"
            ),
            Err(_) => refusals += 1,
        }
        assert_eq!(
            answer(&canonical.text),
            expected,
            "case {case}: {}",
            canonical.text
        );
        assert_eq!(answer(&text), expected, "case {case}: {text}");
    }
    assert!(
        refusals > 50,
        "the generator draws refused clears: {refusals}"
    );
}

/// One rendering case: the arm's JSON, the words it must say, and the
/// chips (pointer under the row, value) it must carry.
type Said = (String, Vec<&'static str>, Vec<(&'static str, &'static str)>);

/// Every field a Build, Survey or Mine arm writes is said, each value with a
/// chip holding it, and a field the row leaves out is not said and has no
/// chip, as the Defend and Attack arms say only what they write
/// (decisions-log item 184 (3), fix pass 1).
#[test]
fn render_plan_says_every_written_setting_of_the_build_survey_and_mine_arms() {
    let area = r#"{"min":{"x":1,"y":1,"z":1},"max":{"x":2,"y":2,"z":2}}"#;
    let target = r#"{"blueprint_id":"generator","anchor":{"voxel":{"x":3,"y":3,"z":3}},"order":0,"rotation_quarter_turns":0}"#;
    let cases: Vec<Said> = vec![
        (
            r#""survey": {"scout_count": 3}"#.to_owned(),
            vec!["fielding 3 scouts"],
            vec![("survey/scout_count", "3")],
        ),
        (
            format!(r#""survey": {{"probe_areas": [{area}, {area}]}}"#),
            vec!["probing 2 areas"],
            vec![],
        ),
        (
            r#""mine": {"dig_max_depth": 4, "flee_on_threat": false, "seam_choice": "RICHEST", "pillar_spacing": 2}"#.to_owned(),
            vec![
                "digging down to 4 voxels below the seam's top",
                "not fleeing threats",
                "choosing the richest seam",
                "leaving a pillar every 2 columns",
            ],
            vec![
                ("mine/dig_max_depth", "4"),
                ("mine/flee_on_threat", "false"),
                ("mine/seam_choice", "\"RICHEST\""),
                ("mine/pillar_spacing", "2"),
            ],
        ),
        (
            format!(
                r#""build": {{"targets": [{target}], "repair_threshold_pct": 40, "rebuild_destroyed": true, "terraform": "FILL", "protected_areas": [{area}]}}"#
            ),
            vec![
                "building toward 1 target",
                "repairing structures below 40% of their hit points",
                "rebuilding destroyed targets",
                "filling ground to build",
                "keeping clear of 1 area",
            ],
            vec![
                ("build/repair_threshold_pct", "40"),
                ("build/rebuild_destroyed", "true"),
                ("build/terraform", "\"FILL\""),
            ],
        ),
        (
            r#""build": {"repair_threshold_pct": 0}"#.to_owned(),
            vec!["never repairing structures, beacons or walls (0%)"],
            vec![("build/repair_threshold_pct", "0")],
        ),
    ];
    for (arm, words, chips) in &cases {
        let said = rendered(&playbook(arm));
        for expected in words {
            assert!(said.prose.contains(expected), "{arm}: {}", said.prose);
        }
        for (pointer, value) in chips {
            let pointer = format!("{ROW}/{pointer}");
            let chip = said
                .rules
                .iter()
                .flat_map(|rule| &rule.chips)
                .find(|chip| chip.pointer == pointer)
                .unwrap_or_else(|| panic!("{arm}: a chip at {pointer}"));
            assert_eq!(chip.value_json, *value, "{arm}");
        }
    }
    // A row that leaves every field out says none and has no chip under the
    // arm.
    for arm in ["build", "survey", "mine"] {
        let said = rendered(&playbook(&format!(r#""{arm}": {{}}"#)));
        let under = format!("{ROW}/{arm}/");
        assert!(
            said.rules
                .iter()
                .flat_map(|rule| &rule.chips)
                .all(|chip| !chip.pointer.starts_with(&under)),
            "{arm}: an omitted field is not said as the player's choice"
        );
        for words in [
            "fielding",
            "no scouts",
            "digging",
            "pillar",
            "fleeing",
            "choosing",
            "repairing",
            "rebuilding",
            "ground",
            "probing",
            "building toward",
            "keeping clear",
        ] {
            assert!(!said.prose.contains(words), "{arm}: {}", said.prose);
        }
    }
}
