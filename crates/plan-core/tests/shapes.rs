// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The verifier's agreement with every shape this crate sends (S3's `gwr`
//! review, items 3, 5 and 8): each vocabulary parameter's and each chip's
//! bounds, choices and arms are the ones the verifier enforces at that
//! pointer, each open skeleton verifies clean inside a playbook, and every
//! schema arm a picker could offer is listed or excluded with its reason.
//!
//! Each check writes a value into a minimal playbook and asks the verifier at
//! QUICK, so this crate and the verifier cannot drift apart unseen.

use std::fmt::Write as _;
use std::path::Path;

use pharmakos_plan_core::canonical::canonicalise_text;
use pharmakos_plan_core::error::split_pointer;
use pharmakos_plan_core::patch::patch_text;
use pharmakos_plan_core::shape::Shape;
use pharmakos_plan_core::verify::verify_jsonc;
use pharmakos_plan_core::vocabulary::{Entry, Parameter, vocabulary};
use pharmakos_proto::descriptor::schema;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;
use pharmakos_sim::rules::RulesTable;
use pharmakos_verifier::{
    FeatureKind, KnownBeacon, KnownFeature, Ownership, Scope, Sighted, Sighting,
};

fn rules() -> RulesTable {
    RulesTable::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("rules")
            .join("rules.v1.json"),
    )
    .unwrap_or_else(|error| panic!("reading the committed rules table: {error}"))
}

const CORE: Voxel = Voxel {
    x: 96,
    y: 11,
    z: 62,
};

const ENEMY: Voxel = Voxel {
    x: 120,
    y: 30,
    z: 40,
};

/// The vent the scope knows, by name.
const VENT: &str = "vent_96_11";

/// A seat with its core `b_00`, a sighted enemy beacon `e_01`, one vent, and
/// the money to pay for what a probe writes.
fn scope() -> Scope {
    let own = KnownBeacon {
        beacon_id: String::from("b_00"),
        owner: pharmakos_sim::tables::SeatId::new(0),
        side: Ownership::Own,
        mandate: pharmakos_proto::gp::v1::beacon_filter::MandateKind::Unspecified,
        tags: Vec::new(),
        at: CORE,
        is_core: true,
    };
    let enemy = KnownBeacon {
        beacon_id: String::from("e_01"),
        owner: pharmakos_sim::tables::SeatId::new(1),
        side: Ownership::EnemyKnown,
        mandate: pharmakos_proto::gp::v1::beacon_filter::MandateKind::Unspecified,
        tags: Vec::new(),
        at: ENEMY,
        is_core: false,
    };
    Scope::new(
        pharmakos_sim::tables::SeatId::new(0),
        pharmakos_sim::knowledge::SeatEconomy {
            treasury: pharmakos_sim::math::quantity::Money::new(100_000),
            supply: pharmakos_sim::math::quantity::Kw::new(1_000),
            draw: pharmakos_sim::math::quantity::Kw::new(0),
        },
    )
    .with_beacon(own)
    .with_beacon(enemy)
    .with_sighting(Sighting {
        what: Sighted::Beacon(String::from("e_01")),
        at: ENEMY,
        age_ms: 0,
    })
    .with_feature(KnownFeature {
        feature_id: String::from(VENT),
        kind: FeatureKind::Vent,
        grade: pharmakos_proto::gp::v1::by_richness::Richness::Standard,
        x: 96,
        y: 11,
        live: true,
        covered_by: Some(String::from("b_00")),
    })
    .with_commander(CORE)
}

/// The verifier at QUICK over `text`, as `(code, path)` pairs.
fn diagnostics(text: &str) -> Vec<(String, String)> {
    verify_jsonc(text, &[], &scope(), &rules(), Depth::Quick)
        .unwrap_or_else(|error| panic!("the committed rules table is complete: {error:?}"))
        .diagnostics
        .into_iter()
        .map(|diagnostic| (diagnostic.code, diagnostic.path))
        .collect()
}

/// Whether `path` is `pointer` or under it.
fn under(path: &str, pointer: &str) -> bool {
    path == pointer || path.starts_with(&format!("{pointer}/"))
}

/// The diagnostics at or under `pointer`.
fn at_or_under(text: &str, pointer: &str) -> Vec<(String, String)> {
    diagnostics(text)
        .into_iter()
        .filter(|(_, path)| under(path, pointer))
        .collect()
}

/// A playbook with one route step and one rule: the frame every probe edits.
const FRAME: &str = r#"{"schema_version":{"major":1},"meta":{"title":"Probe","author_kind":"HUMAN"},"kind":"PLAYBOOK","declarative":{"route":[{"label":"step_1","move":{"to":{"safest":{}}}}],"handlers":[{"id":"rule_1","when":{"segment_elapsed":{"ms":{"op":"GE","value":1000}}},"body":[{"label":"body_1","hold":{"ms":1000}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":1}]},"on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":2},"fallback":{"hold":{"at":{"safest":{}}}}}"#;

/// `text` with `value` (JSON text) at `pointer`, every object on the way made.
fn with_value(text: &str, pointer: &str, value: &str) -> String {
    let document = pharmakos_proto::json::read(text)
        .unwrap_or_else(|error| panic!("the probe reads: {error:?}"));
    let tokens = split_pointer(pointer).unwrap_or_else(|error| panic!("{pointer}: {error:?}"));
    let mut operations = Vec::new();
    let mut here = Some(&document);
    let mut path = String::new();
    for (index, token) in tokens.iter().enumerate() {
        path.push('/');
        path.push_str(&token.replace('~', "~0").replace('/', "~1"));
        let next = here.and_then(|node| match node {
            Json::Array(items) => token.parse::<usize>().ok().and_then(|at| items.get(at)),
            other => other.get(token),
        });
        if index + 1 == tokens.len() {
            operations.push(format!(r#"{{"op":"add","path":"{path}","value":{value}}}"#));
        } else if next.is_none() {
            operations.push(format!(r#"{{"op":"add","path":"{path}","value":{{}}}}"#));
        }
        here = next;
    }
    patch_text(text, &format!("[{}]", operations.join(",")))
        .unwrap_or_else(|error| panic!("{pointer} takes {value}: {error}"))
        .0
}

/// `text` with the member at `pointer` removed, if it is there.
fn without(text: &str, pointer: &str) -> String {
    patch_text(text, &format!(r#"[{{"op":"remove","path":"{pointer}"}}]"#))
        .map_or_else(|_| text.to_owned(), |(patched, _)| patched)
}

/// Where an entry stands in the frame, and the frame holding its skeleton.
struct Placed {
    /// The entry's own object.
    at: String,
    /// The frame with the entry written in.
    text: String,
}

/// A predicate in the rule's condition.
fn place_predicate(name: &str, skeleton: &str) -> Placed {
    let text = without(FRAME, "/declarative/handlers/0/when/segment_elapsed");
    let at = format!("/declarative/handlers/0/when/{name}");
    Placed {
        text: with_value(&text, &at, skeleton),
        at,
    }
}

/// A step as the first route step.
fn place_step(name: &str, skeleton: &str) -> Placed {
    let text = without(FRAME, "/declarative/route/0/move");
    let at = format!("/declarative/route/0/{name}");
    Placed {
        text: with_value(&text, &at, skeleton),
        at,
    }
}

/// A writ's settings on a new beacon's `initial`, at a voxel.
fn place_mandate(name: &str, skeleton: &str) -> Placed {
    let text = without(FRAME, "/declarative/route/0/move");
    let step = "/declarative/route/0/place_beacon";
    let text = with_value(
        &text,
        step,
        &format!(
            r#"{{"at":{{"voxel":{{"x":{},"y":{},"z":{}}}}}}}"#,
            CORE.x + 4,
            CORE.y,
            CORE.z
        ),
    );
    let at = format!("{step}/initial/mandate/{name}");
    Placed {
        text: with_value(&text, &at, skeleton),
        at,
    }
}

/// The message the value at `pointer` (from the playbook's root) is, by the
/// schema: a field name steps into its type, an array index stays.
fn message_at(pointer: &str) -> String {
    let mut here = String::from("gp.v1.Playbook");
    for token in split_pointer(pointer).unwrap_or_else(|error| panic!("{pointer}: {error:?}")) {
        if token.parse::<usize>().is_ok() {
            continue;
        }
        let declared = schema()
            .message(&here)
            .unwrap_or_else(|| panic!("{here} is a declared message ({pointer})"));
        let field = declared
            .field_by_name(&token)
            .unwrap_or_else(|| panic!("{here} declares no `{token}` ({pointer})"));
        field
            .type_name
            .trim_start_matches('.')
            .clone_into(&mut here);
    }
    here
}

/// Every arm of the oneof `arms` belong to in `message`.
fn oneof_arms(message: &str, arms: &[&str]) -> Vec<String> {
    let declared = schema()
        .message(message)
        .unwrap_or_else(|| panic!("{message} is a declared message"));
    let first = arms
        .first()
        .and_then(|arm| declared.field_by_name(arm))
        .and_then(|field| field.oneof_index)
        .unwrap_or_else(|| panic!("{message}: {arms:?} are a oneof's arms"));
    declared
        .fields
        .iter()
        .filter(|field| field.oneof_index == Some(first))
        .map(|field| field.name.clone())
        .collect()
}

/// A value of `arm` (of `message`) that names only what the probe's scope
/// holds, so a refusal is the arm's and never a dangling name. An Attack
/// target names the enemy, with a selector's `ENEMY_KNOWN` filter, as one
/// must (`E0015`, `E0415`).
fn arm_example(text: &str, message: &str, arm: &str, pointer: &str) -> String {
    let attack = pointer.contains("/attack/target");
    // A wall target's anchor is a voxel inside its box (`E0417`).
    let wall_corner = pointer.strip_suffix("/anchor").and_then(|target| {
        let document = pharmakos_proto::json::read(text).ok()?;
        let mut here = &document;
        for token in split_pointer(&format!("{target}/wall_box/min")).ok()? {
            here = match here {
                Json::Array(items) => items.get(token.parse::<usize>().ok()?)?,
                other => other.get(&token)?,
            };
        }
        Some(pharmakos_proto::json::write(here))
    });
    let pick = r#"{"rank":"NEAREST","coverage":"UNCOVERED"}"#;
    let value = match (message, arm) {
        (_, "voxel") if wall_corner.is_some() => wall_corner.unwrap_or_default(),
        (_, "voxel") => format!(r#"{{"x":{},"y":{},"z":{}}}"#, CORE.x + 2, CORE.y, CORE.z),
        (_, "beacon_anchor") => String::from(r#"{"beacon_id":"b_00"}"#),
        ("gp.v1.BeaconRef", "nearest" | "weakest" | "most_threatened") if attack => {
            String::from(r#"{"filter":{"side":"ENEMY_KNOWN"}}"#)
        }
        (
            _,
            "safest" | "covered" | "enemy_commander" | "any_kind" | "nearest" | "weakest"
            | "most_threatened",
        ) => String::from("{}"),
        (_, "on") => format!(r#"{{"feature_id":"{VENT}"}}"#),
        (_, "covering") => format!(r#"{{"vent":{pick}}}"#),
        (_, "toward") => format!(r#"{{"feature":{{"vent":{pick}}}}}"#),
        ("gp.v1.BeaconRef", "beacon_id") if attack => String::from(r#""e_01""#),
        ("gp.v1.BeaconRef", "beacon_id") => String::from(r#""b_00""#),
        ("gp.v1.FeatureRef", "feature_id") => format!(r#""{VENT}""#),
        ("gp.v1.FeatureRef", "vent" | "seam") => String::from(pick),
        ("gp.v1.AttackTarget", "beacon") => String::from(r#"{"beacon_id":"e_01"}"#),
        other => panic!("no example for {other:?} at {pointer}"),
    };
    format!(r#"{{"{arm}":{value}}}"#)
}

/// The errors (`E` codes) at or under `pointer`: what the verifier refuses
/// there. Warnings and notes are no refusal.
fn refused(text: &str, pointer: &str) -> Vec<(String, String)> {
    at_or_under(text, pointer)
        .into_iter()
        .filter(|(code, _)| code.starts_with('E'))
        .collect()
}

/// Holds one shape to the verifier at `pointer` of `text`: an integer's
/// bounds (one step past each stated bound refused, the bound itself not,
/// an open side not refused far out), each choice accepted, and each listed
/// arm accepted and every other arm of its oneof refused. `what` names the
/// shape's owner in a failure.
fn agrees(what: &str, text: &str, pointer: &str, shape: &Shape) -> Vec<String> {
    let mut wrong = Vec::new();
    let refused_with =
        |value: &str| !refused(&with_value(text, pointer, value), pointer).is_empty();
    match shape {
        Shape::Integer { min, max, .. } => {
            match min {
                Some(least) => {
                    if !refused_with(&(i64::from(*least) - 1).to_string()) {
                        wrong.push(format!(
                            "{what}: {pointer} below its least {least} is not refused"
                        ));
                    }
                    if refused_with(&least.to_string()) {
                        wrong.push(format!("{what}: {pointer} at its least {least} is refused"));
                    }
                }
                None => {
                    if refused_with("-1000000") {
                        wrong.push(format!(
                            "{what}: {pointer} is refused far below and states no least"
                        ));
                    }
                }
            }
            match max {
                Some(most) => {
                    if !refused_with(&(i64::from(*most) + 1).to_string()) {
                        wrong.push(format!(
                            "{what}: {pointer} above its most {most} is not refused"
                        ));
                    }
                    if refused_with(&most.to_string()) {
                        wrong.push(format!("{what}: {pointer} at its most {most} is refused"));
                    }
                }
                None => {
                    if refused_with("1000000") {
                        wrong.push(format!(
                            "{what}: {pointer} is refused far above and states no most"
                        ));
                    }
                }
            }
        }
        Shape::Choice { values } => {
            for value in values {
                if refused_with(value) {
                    wrong.push(format!("{what}: {pointer} refuses the choice {value}"));
                }
            }
        }
        Shape::Reference { arms, .. } if !arms.is_empty() => {
            let message = message_at(pointer);
            for arm in oneof_arms(&message, arms) {
                let example = arm_example(text, &message, &arm, pointer);
                let listed = arms.contains(&arm.as_str());
                let refusals = refused(&with_value(text, pointer, &example), pointer);
                if listed && !refusals.is_empty() {
                    wrong.push(format!(
                        "{what}: {pointer} lists `{arm}`, which the verifier refuses: {refusals:?}"
                    ));
                }
                if !listed && refusals.is_empty() {
                    wrong.push(format!(
                        "{what}: {pointer} leaves out `{arm}`, which the verifier accepts"
                    ));
                }
            }
        }
        Shape::Reference { .. } => {}
    }
    wrong
}

/// A writ's arm with every field it requires written, so a parameter is held
/// where it is written beside the rest: a wholly empty arm (a writ's
/// skeleton) is legal at a deploy, and a value there that only empties it
/// again says nothing of its bounds.
fn full_arm(writ: &str) -> &'static str {
    match writ {
        "defend" => {
            r#"{"protect":[{"beacon":{"beacon_id":"b_00"}}],"engagement_radius_pct":50,"pursue":"NEVER","picket_scouts":1}"#
        }
        "attack" => {
            r#"{"target":{"beacon":{"beacon_id":"e_01"}},"retarget":"NONE","staging":{"x":100,"y":11,"z":62},"launch":{"force_size":2,"segment_ms":60000},"retreat":{"force_below_pct":30,"after_launch_ms":60000},"after_action":"RETURN_HOME","breach_allowed":false,"waves":1}"#
        }
        "survey" => r#"{"scout_count":1}"#,
        _ => "{}",
    }
}

/// Every open entry of the vocabulary, placed in the frame: a writ's as its
/// skeleton, or, when `full`, as its [`full_arm`].
fn placed_entries(full: bool) -> Vec<(String, Placed, Vec<Parameter>)> {
    let words = vocabulary(&[], &rules()).unwrap_or_else(|error| panic!("{error}"));
    let mut found = Vec::new();
    let open = |entry: &Entry| entry.locked.is_none() && !entry.name.is_empty();
    for entry in words.families.iter().flat_map(|family| &family.predicates) {
        if open(entry) {
            found.push((
                format!("predicate {}", entry.name),
                place_predicate(entry.name, &entry.skeleton_json),
                entry.parameters.clone(),
            ));
        }
    }
    for entry in &words.steps {
        if open(entry) {
            found.push((
                format!("step {}", entry.name),
                place_step(entry.name, &entry.skeleton_json),
                entry.parameters.clone(),
            ));
        }
    }
    for entry in &words.mandates {
        if open(entry) {
            found.push((
                format!("writ {}", entry.name),
                if full {
                    place_mandate(entry.name, full_arm(entry.name))
                } else {
                    place_mandate(entry.name, &entry.skeleton_json)
                },
                entry.parameters.clone(),
            ));
        }
    }
    let common = pharmakos_plan_core::vocabulary::common_parameters()
        .unwrap_or_else(|error| panic!("{error}"));
    let placed = place_mandate("build", "{}");
    let at = placed
        .at
        .strip_suffix("/build")
        .unwrap_or_else(|| panic!("a writ's settings"))
        .to_owned();
    found.push((
        String::from("the common settings"),
        Placed {
            at,
            text: placed.text,
        },
        common,
    ));
    found
}

/// S3's `gwr` review, item 8: each open entry's skeleton, written into a
/// playbook, draws no error at or under it, so a picker inserts nothing the
/// verifier refuses (a starting value the player then picks is a legal one).
#[test]
fn every_open_skeleton_verifies_clean() {
    let frame = canonicalise_text(FRAME).map(|canonical| canonical.text);
    assert!(frame.is_ok(), "{frame:?}");
    assert!(refused(FRAME, "").is_empty(), "{:?}", diagnostics(FRAME));
    let mut wrong = Vec::new();
    for (what, placed, _) in placed_entries(false) {
        let refusals = refused(&placed.text, &placed.at);
        if !refusals.is_empty() {
            wrong.push(format!("{what}: {refusals:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// S3's `gwr` review, item 3: every vocabulary parameter's shape is what the
/// verifier enforces at its pointer -- an integer's bounds, each choice, each
/// arm and every arm left out -- as `tests/plan_core.rs` holds each chip's
/// integer bounds. A parameter of a writ is held on a new beacon's settings;
/// a predicate's in a rule's condition; a step's in the route.
#[test]
fn every_vocabulary_parameter_is_the_verifiers() {
    let mut wrong = Vec::new();
    for (what, placed, parameters) in placed_entries(true) {
        let refusals = refused(&placed.text, &placed.at);
        assert!(refusals.is_empty(), "{what}'s full arm: {refusals:?}");
        for parameter in parameters {
            let pointer = format!("{}{}", placed.at, parameter.pointer);
            wrong.extend(agrees(&what, &placed.text, &pointer, &parameter.shape));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Every oneof arm of `message`, by the schema.
fn arms_of(message: &str) -> Vec<String> {
    schema()
        .message(message)
        .unwrap_or_else(|| panic!("{message} is a declared message"))
        .fields
        .iter()
        .filter(|field| field.oneof_index.is_some())
        .map(|field| field.name.clone())
        .collect()
}

/// The arms the vocabulary leaves out, each with why: a picker composes the
/// groups around predicates rather than inserting one.
const NOT_IN_THE_VOCABULARY: &[(&str, &str, &str)] = &[
    (
        "gp.v1.Condition",
        "all",
        "a group, which a picker composes around predicates",
    ),
    (
        "gp.v1.Condition",
        "any",
        "a group, which a picker composes around predicates",
    ),
    (
        "gp.v1.Condition",
        "not",
        "a group, which a picker composes around a predicate",
    ),
];

/// S3's `gwr` review, item 8, the converse: every arm the schema declares --
/// each `Condition` arm but the groups, each `Step` arm, each
/// `MandateSettings` arm -- and every structure the sim defines is in the
/// vocabulary or left out above with its reason, so a word the schema gains
/// fails here until it is offered or excluded.
#[test]
fn every_schema_arm_is_in_the_vocabulary_or_excluded_with_its_reason() {
    let words = vocabulary(&[], &rules()).unwrap_or_else(|error| panic!("{error}"));
    let predicates: Vec<&str> = words
        .families
        .iter()
        .flat_map(|family| &family.predicates)
        .map(|entry| entry.name)
        .collect();
    let steps: Vec<&str> = words.steps.iter().map(|entry| entry.name).collect();
    let mandates: Vec<&str> = words.mandates.iter().map(|entry| entry.name).collect();
    let mut missing = Vec::new();
    for (message, listed) in [
        ("gp.v1.Condition", &predicates),
        ("gp.v1.Step", &steps),
        ("gp.v1.MandateSettings", &mandates),
    ] {
        for arm in arms_of(message) {
            let excluded = NOT_IN_THE_VOCABULARY
                .iter()
                .any(|(from, name, _)| *from == message && *name == arm);
            if !listed.contains(&arm.as_str()) && !excluded {
                missing.push(format!("{message}.{arm}"));
            }
        }
    }
    for kind in pharmakos_sim::tables::StructureKind::ALL {
        let id = pharmakos_plan_core::vocabulary::blueprint_id(kind);
        if !words.blueprints.iter().any(|entry| entry.name == id) {
            missing.push(format!("blueprint {id}"));
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
    for (message, name, why) in NOT_IN_THE_VOCABULARY {
        assert!(
            arms_of(message).iter().any(|arm| arm == name),
            "{message}.{name} is excluded ({why}) and the schema no longer declares it"
        );
    }
}

/// S3's `gwr` review, item 4: the verifier's answer per slot. The base kit
/// and the Generator go in a Build target; the Resonance Spire only in a
/// queue, with its licence; the Survey post in neither yet, so the
/// vocabulary locks it in the verifier's own words.
#[test]
fn each_blueprint_is_offered_where_the_verifier_takes_it() {
    let kinds = pharmakos_plan_core::vocabulary::blueprints_by_slot(&rules())
        .unwrap_or_else(|error| panic!("{error}"));
    let slots: Vec<(&str, bool, bool)> = kinds
        .iter()
        .map(|kind| (kind.id, kind.build_target, kind.queued))
        .collect();
    assert_eq!(
        slots,
        [
            ("generator", true, false),
            ("autocannon", true, false),
            ("mortar", true, false),
            ("survey_post", false, false),
            ("resonance_spire", false, true),
            ("wall", true, false),
        ]
    );
    // Each answer is the verifier's: a Build target of each kind the probe
    // calls buildable draws no E0010, and every other kind draws one.
    for kind in &kinds {
        let placed = place_step(
            "interface",
            &format!(
                r#"{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"add_build_target":{{"target":{{"blueprint_id":"{}","anchor":{{"on":{{"feature_id":"{VENT}"}}}}}}}}}},{{"queue_structure":{{"blueprint_id":"{}"}}}}]}}"#,
                kind.id, kind.id
            ),
        );
        let codes: Vec<String> = refused(&placed.text, &placed.at)
            .into_iter()
            .map(|(code, _)| code)
            .collect();
        assert_eq!(
            codes.contains(&String::from("E0010")),
            !kind.build_target,
            "{}: {codes:?}",
            kind.id
        );
        assert_eq!(
            codes.contains(&String::from("E0009")),
            !kind.queued,
            "{}: {codes:?}",
            kind.id
        );
    }
    let words = vocabulary(&[], &rules()).unwrap_or_else(|error| panic!("{error}"));
    let locked = |id: &str| {
        words
            .blueprints
            .iter()
            .find(|entry| entry.name == id)
            .map_or_else(|| panic!("{id} is listed"), |entry| entry.locked.clone())
    };
    assert_eq!(
        locked("survey_post").as_deref(),
        pharmakos_verifier::strings::text("E0010").map(|text| text.beginner)
    );
    assert!(locked("resonance_spire").is_some());
    assert_eq!(locked("generator"), None);
}

/// A frozen snapshot with one seat in it, as `tests/plan_core.rs`' context.
fn context() -> pharmakos_plan_core::context::PlanContext {
    let snapshot = pharmakos_sim::snapshot::Snapshot {
        seat_id: vec![0],
        seat_treasury: vec![200],
        seat_supply: vec![10],
        seat_draw: vec![2],
        ..pharmakos_sim::snapshot::Snapshot::default()
    };
    pharmakos_plan_core::context::PlanContext::from_snapshot(&snapshot, 0)
        .unwrap_or_else(|error| panic!("seat 0 is in the snapshot: {error}"))
}

/// A playbook's canonical JSON and its rendering.
fn rendered(text: &str) -> (String, pharmakos_plan_core::render::Rendering) {
    let canonical =
        canonicalise_text(text).unwrap_or_else(|error| panic!("the case canonicalises: {error:?}"));
    let rendering = pharmakos_plan_core::render::render(&canonical.playbook, &context(), &rules())
        .unwrap_or_else(|error| panic!("a rendering: {error:?}"));
    (canonical.json, rendering)
}

/// Every case with a chips golden, by name, with its text.
fn rendered_cases() -> Vec<(&'static str, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |path: std::path::PathBuf| {
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
    };
    let case = |name: &str| {
        read(
            root.join("tests")
                .join("cases")
                .join(format!("{name}.jsonc")),
        )
    };
    vec![
        (
            "expand_east",
            read(
                root.join("..")
                    .join("..")
                    .join("examples")
                    .join("playbooks")
                    .join("expand_east.jsonc"),
            ),
        ),
        ("cover_the_nearest_vent", case("cover_the_nearest_vent")),
        ("defend_and_attack", case("defend_and_attack")),
        ("predicates_and_toward", case("predicates_and_toward")),
    ]
}

/// S3's `gwr` review, item 5: every chip's arms and choices are the
/// verifier's at its pointer -- each listed arm accepted, every arm of its
/// oneof left out refused, each choice accepted -- as every vocabulary
/// parameter's are. Its integer bounds are `tests/plan_core.rs`'.
#[test]
fn every_chips_arms_and_choices_are_the_verifiers() {
    let mut wrong = Vec::new();
    let mut held = 0;
    for (case, text) in rendered_cases() {
        let (canonical, rendering) = rendered(&text);
        for chip in rendering.rules.iter().flat_map(|rule| &rule.chips) {
            if matches!(chip.shape, Shape::Integer { .. }) {
                continue;
            }
            held += 1;
            wrong.extend(agrees(case, &canonical, &chip.pointer, &chip.shape));
        }
    }
    assert!(held > 20, "only {held} chips held");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `text` as a JSON string, quotes included, every control character
/// escaped as JSON spells one.
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            other if u32::from(other) < 0x20 || other.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(other));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// A playbook writing `name` everywhere the prose shows a string the player
/// wrote: its title, a step's label and tag, a rule's id, a step label and a
/// rule id a predicate names, a queued blueprint and a beacon's handle.
fn naming(name: &str) -> String {
    let name = json_string(name);
    format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":{name},"author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{{"label":{name},"place_beacon":{{"at":{{"safest":{{}}}},"tags":[{name}]}}}},{{"label":"queue","interface":{{"beacon":{{"beacon_id":{name}}},"rows":[{{"queue_structure":{{"blueprint_id":{name}}}}}]}}}}],"handlers":[{{"id":{name},"when":{{"any":{{"items":[{{"step_reached":{{"label":{name}}}}},{{"rule_fired":{{"handler_id":{name}}}}}]}}}},"body":[{{"label":"body","hold":{{"ms":1000}}}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":1}}]}},"on_death":{{"on_respawn":"CONTINUE"}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
    )
}

/// S3's `gwr` review, item 6: over every control character, and line
/// breaks and tabs among words, a string the player wrote never breaks the
/// prose: no line of it holds a control character, and every chip whose
/// value is a string shows that string on its own line, at its span, as the
/// prose shows player text ([`pharmakos_plan_core::render::shown_text`]).
#[test]
fn a_string_the_player_wrote_never_breaks_a_line_or_moves_a_chip() {
    let mut names: Vec<String> = (0_u32..0x20)
        .chain([0x7f, 0x85, 0x9f])
        .filter_map(char::from_u32)
        .map(|control| format!("a{control}b"))
        .collect();
    names.extend(
        [
            "line\nbreak",
            "cr\r\nlf",
            "\ttab",
            "end\n",
            "plain",
            "\u{2028}sep",
        ]
        .map(String::from),
    );
    for name in names {
        let (_, rendering) = rendered(&naming(&name));
        let lines: Vec<&str> = rendering.prose.lines().collect();
        for line in &lines {
            assert!(
                !line.chars().any(char::is_control),
                "{name:?}: a line holds a control character: {line:?}"
            );
        }
        let mut strings = 0;
        for chip in rendering.rules.iter().flat_map(|rule| &rule.chips) {
            let Ok(Json::String(value)) = pharmakos_proto::json::read(&chip.value_json) else {
                continue;
            };
            strings += 1;
            let line = lines
                .get(usize::try_from(chip.line).unwrap_or_else(|error| panic!("{error}")))
                .unwrap_or_else(|| panic!("{name:?}: {} is past the prose", chip.pointer));
            let span: String = line
                .chars()
                .skip(usize::try_from(chip.start).unwrap_or_else(|error| panic!("{error}")))
                .take(
                    usize::try_from(chip.end - chip.start)
                        .unwrap_or_else(|error| panic!("{error}")),
                )
                .collect();
            let shown = pharmakos_plan_core::render::shown_text(&value);
            assert_eq!(
                span.strip_prefix('"')
                    .and_then(|inner| inner.strip_suffix('"'))
                    .unwrap_or(&span),
                shown.as_ref(),
                "{name:?}: {}'s span is not its value",
                chip.pointer
            );
        }
        assert!(strings >= 3, "{name:?}: only {strings} string chips");
    }
}
