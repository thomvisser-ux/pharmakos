// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the schema and the codec promise, asserted.
//!
//! Four of these tests write a golden `actual.*` file into
//! `<target>/golden/proto/`, where `cargo xtask ci`'s `golden` step compares it
//! byte for byte with `tests/golden/proto/expected.*` at the workspace root.
//! `cargo xtask golden --bless` accepts a move, and a blessed golden has to be
//! explained in the PR that moved it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use pharmakos_proto::descriptor::{Message, ScalarKind, schema};
use pharmakos_proto::gp::api::v1::{Method, Scope};
use pharmakos_proto::gp::v1::by_richness::Richness;
use pharmakos_proto::gp::v1::{ByRichness, Playbook, RulesTable, playbook, rules_table};
use pharmakos_proto::json::{self, Json};
use pharmakos_proto::{fingerprint, scope};

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// The workspace root: this crate is `<root>/crates/proto`.
fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/proto sits two levels below the workspace root"))
        .to_path_buf()
}

/// The cargo target directory. `CARGO_TARGET_TMPDIR` is `<target>/tmp`, and it
/// is the only way a test can find the target directory that also honours
/// `CARGO_TARGET_DIR` — which every agent worktree sets to its own lane.
fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap_or_else(|| panic!("CARGO_TARGET_TMPDIR always has a parent"))
        .to_path_buf()
}

/// Writes one `actual.*` file where `cargo xtask ci`'s golden step looks.
fn write_actual(name: &str, contents: &str) {
    let directory = target_dir().join("golden").join("proto");
    fs::create_dir_all(&directory)
        .unwrap_or_else(|error| panic!("creating {}: {error}", directory.display()));
    fs::write(directory.join(format!("actual.{name}")), contents)
        .unwrap_or_else(|error| panic!("writing actual.{name}: {error}"));
}

/// Reads `tests/golden/proto/expected.*` from the workspace root.
fn read_expected(name: &str) -> String {
    let path = workspace_root()
        .join("tests")
        .join("golden")
        .join("proto")
        .join(format!("expected.{name}"));
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("reading {}: {error}", path.display());
    })
}

/// A readable first difference, so a failure names a byte rather than dumping
/// two files at the reader.
fn assert_same(name: &str, expected: &str, actual: &str) {
    if expected == actual {
        return;
    }
    let mut index = 0_usize;
    let expected_bytes = expected.as_bytes();
    let actual_bytes = actual.as_bytes();
    while expected_bytes.get(index) == actual_bytes.get(index)
        && expected_bytes.get(index).is_some()
    {
        index = index.saturating_add(1);
    }
    let line = expected
        .get(..index)
        .map_or(1, |head| head.lines().count().max(1));
    panic!(
        "{name} differs from the committed golden at byte {index} (line {line}).\n\
         expected: {:?}\n  actual: {:?}\n\
         Run `cargo xtask golden --bless` only if the move is intended, and explain it in the PR.",
        expected.get(index..index.saturating_add(60)),
        actual.get(index..index.saturating_add(60)),
    );
}

// ---------------------------------------------------------------------------
// A minimal JSONC comment stripper, for this test only
// ---------------------------------------------------------------------------

/// Strips `//` line comments. The real JSONC layer — comments in every
/// position, preserved byte for byte through a round trip — is `plan-core`'s
/// (decisions-log item 74, skeleton-plan T8). This is only enough to feed the
/// committed example to the codec.
fn strip_line_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut in_string = false;
        let mut escaped = false;
        let mut cut = line.len();
        let bytes = line.as_bytes();
        let mut index = 0_usize;
        while index < bytes.len() {
            let byte = bytes.get(index).copied().unwrap_or(0);
            if escaped {
                escaped = false;
            } else if byte == b'\\' && in_string {
                escaped = true;
            } else if byte == b'"' {
                in_string = !in_string;
            } else if !in_string
                && byte == b'/'
                && bytes.get(index.saturating_add(1)) == Some(&b'/')
            {
                cut = index;
                break;
            }
            index = index.saturating_add(1);
        }
        out.push_str(line.get(..cut).unwrap_or(""));
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Round-trip: every message
// ---------------------------------------------------------------------------

/// Builds a JSON value that sets every field of a message to a non-default
/// value, so nothing can round-trip by being dropped.
///
/// A message type already on the stack — `Condition` contains `All` contains
/// `Condition` — becomes an empty object, which still round-trips and keeps
/// the walk finite.
fn populate(message: &Message, visiting: &mut BTreeSet<String>) -> Json {
    if !visiting.insert(message.full_name.clone()) {
        return Json::Object(Vec::new());
    }
    let schema = schema();
    let mut entries: Vec<(String, Json)> = Vec::new();
    let mut oneofs_filled: BTreeSet<i32> = BTreeSet::new();

    for field in &message.fields {
        if let Some(index) = field.oneof_index {
            if !oneofs_filled.insert(index) {
                continue;
            }
        }
        let value = match field.kind {
            ScalarKind::Double | ScalarKind::Float => {
                panic!(
                    "{} is floating point, which this schema forbids",
                    field.name
                )
            }
            ScalarKind::Bool => Json::Bool(true),
            ScalarKind::Int32 | ScalarKind::Sint32 | ScalarKind::Sfixed32 => {
                Json::Number("-7".to_owned())
            }
            ScalarKind::Uint32 | ScalarKind::Fixed32 => Json::Number("7".to_owned()),
            ScalarKind::Int64 | ScalarKind::Sint64 | ScalarKind::Sfixed64 => {
                Json::String("-7".to_owned())
            }
            ScalarKind::Uint64 | ScalarKind::Fixed64 => Json::String("7".to_owned()),
            ScalarKind::String => Json::String("x".to_owned()),
            ScalarKind::Bytes => Json::String("AQID".to_owned()),
            ScalarKind::Enum => {
                let declared = schema
                    .enumeration(&field.type_name)
                    .unwrap_or_else(|| panic!("{} is not in the schema", field.type_name));
                // The zero value is the proto3 default and would not survive a
                // round trip through the wire, so a non-zero one is chosen.
                match declared.values.iter().find(|value| value.number != 0) {
                    Some(value) => Json::String(value.name.clone()),
                    None => continue,
                }
            }
            ScalarKind::Message => {
                let nested = schema
                    .message(&field.type_name)
                    .unwrap_or_else(|| panic!("{} is not in the schema", field.type_name));
                populate(nested, visiting)
            }
        };
        let value = if field.repeated {
            Json::Array(vec![value])
        } else {
            value
        };
        entries.push((field.name.clone(), value));
    }

    visiting.remove(&message.full_name);
    Json::Object(entries)
}

#[test]
fn every_message_round_trips_through_canonical_json() {
    let schema = schema();
    let mut checked = 0_usize;
    for message in schema.messages() {
        // Only our own packages: the descriptor set also carries
        // google/protobuf/descriptor.proto, which the gateway imports for its
        // scope annotation and which nothing here encodes.
        if !message.full_name.starts_with("gp.") {
            continue;
        }
        let populated = populate(message, &mut BTreeSet::new());
        let bytes = json::json_to_wire(&message.full_name, &populated)
            .unwrap_or_else(|error| panic!("{}: {error}", message.full_name));
        let back = json::wire_to_json(&message.full_name, &bytes)
            .unwrap_or_else(|error| panic!("{}: {error}", message.full_name));
        assert_eq!(
            populated, back,
            "{} did not survive JSON -> wire -> JSON",
            message.full_name
        );

        // And the text form round-trips too, which is what a golden file and a
        // hand-edited playbook actually exercise.
        let text = json::write(&back);
        let reread =
            json::read(&text).unwrap_or_else(|error| panic!("{}: {error}", message.full_name));
        assert_eq!(back, reread, "{} did not survive text", message.full_name);
        checked = checked.saturating_add(1);
    }
    assert!(
        checked > 60,
        "only {checked} messages were checked; the schema should carry far more"
    );
}

#[test]
fn the_typed_round_trip_goes_through_prost() {
    // The descriptor-driven test above covers every message but never names a
    // Rust type. This one closes the loop for the two that matter most: the
    // playbook envelope and the rules table.
    let text = strip_line_comments(
        &fs::read_to_string(
            workspace_root()
                .join("examples")
                .join("playbooks")
                .join("expand_east.jsonc"),
        )
        .expect("reading the example playbook"),
    );
    let playbook: Playbook = json::decode(&text).expect("the example decodes");
    let encoded = json::encode(&playbook).expect("and re-encodes");
    let again: Playbook = json::decode(&encoded).expect("and decodes again");
    assert_eq!(playbook, again);

    let rules_text = fs::read_to_string(workspace_root().join("rules").join("rules.v1.json"))
        .expect("reading the rules table");
    let rules: RulesTable = json::decode(&rules_text).expect("the rules table decodes");
    let again: RulesTable =
        json::decode(&json::encode(&rules).expect("re-encodes")).expect("and decodes again");
    assert_eq!(rules, again);
}

// ---------------------------------------------------------------------------
// The worked example
// ---------------------------------------------------------------------------

#[test]
fn the_example_playbook_decodes_with_zero_unknown_fields() {
    let raw = fs::read_to_string(
        workspace_root()
            .join("examples")
            .join("playbooks")
            .join("expand_east.jsonc"),
    )
    .expect("reading the example playbook");
    let text = strip_line_comments(&raw);

    // Decoding is the assertion: an unknown field is an error, never ignored.
    let playbook: Playbook = json::decode(&text)
        .unwrap_or_else(|error| panic!("the spec's own worked example did not load: {error}"));

    assert_eq!(playbook.kind(), playbook::Kind::Playbook);
    let canonical = json::encode(&playbook).expect("re-encoding the example");
    write_actual("expand_east.json", &canonical);
    assert_same(
        "expand_east.json",
        &read_expected("expand_east.json"),
        &canonical,
    );
}

#[test]
fn durations_are_bare_numbers_in_the_canonical_form() {
    let raw = fs::read_to_string(
        workspace_root()
            .join("examples")
            .join("playbooks")
            .join("expand_east.jsonc"),
    )
    .expect("reading the example playbook");
    let playbook: Playbook = json::decode(&strip_line_comments(&raw)).expect("the example decodes");
    let canonical = json::encode(&playbook).expect("re-encoding the example");

    // Decisions-log item 46: int32, so the mapping emits a bare number and the
    // spec's example loads as written. A quoted duration means somebody
    // widened a field to int64.
    assert!(
        canonical.contains("\"timeout_ms\": 120000"),
        "timeout_ms should be a bare number:\n{canonical}"
    );
    assert!(
        !canonical.contains("\"timeout_ms\": \""),
        "timeout_ms is quoted, so some duration field is 64-bit"
    );
    assert!(canonical.contains("\"cooldown_ms\": 30000"));
}

// ---------------------------------------------------------------------------
// Strictness
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_field_is_rejected_not_ignored() {
    let text = r#"{"schema_version":{"major":1,"minor":0},"pace_of_advance":"BRISK"}"#;
    let error = json::decode::<Playbook>(text).expect_err("an unknown field must be rejected");
    assert_eq!(error.pointer, "/pace_of_advance");
    assert!(
        error.message.contains("not a field"),
        "the message should say what is wrong: {}",
        error.message
    );

    // And nested, with a pointer that names the place.
    let nested = r#"{"declarative":{"route":[{"label":"a","dash":{}}]}}"#;
    let error = json::decode::<Playbook>(nested).expect_err("rejected");
    assert_eq!(error.pointer, "/declarative/route/0/dash");
}

#[test]
fn two_members_of_one_oneof_are_rejected() {
    let text = r#"{"declarative":{"route":[{"label":"a","hold":{"ms":1},"move":{}}]}}"#;
    let error = json::decode::<Playbook>(text).expect_err("a double-set oneof must be rejected");
    assert!(
        error.message.contains("oneof"),
        "the message should say so: {}",
        error.message
    );
}

#[test]
fn an_unknown_enum_value_is_rejected() {
    let text = r#"{"meta":{"author_kind":"ROBOT"}}"#;
    let error = json::decode::<Playbook>(text).expect_err("an unknown enum value is rejected");
    assert_eq!(error.pointer, "/meta/author_kind");
}

#[test]
fn a_fractional_number_is_rejected() {
    // There is no floating-point field in this schema, so there is nothing a
    // fractional literal could mean.
    let text = r#"{"declarative":{"route":[{"label":"a","timeout_ms":1.5}]}}"#;
    let error = json::decode::<Playbook>(text).expect_err("a fractional duration is rejected");
    assert_eq!(error.pointer, "/declarative/route/0/timeout_ms");
}

// ---------------------------------------------------------------------------
// Schema shape
// ---------------------------------------------------------------------------

#[test]
fn no_duration_field_in_gp_v1_is_int64() {
    // Decisions-log item 46. A 64-bit field is a QUOTED STRING in proto3 JSON,
    // and the spec's worked example writes bare numbers.
    let mut offenders: Vec<String> = Vec::new();
    for message in schema().messages() {
        if !message.full_name.starts_with("gp.") {
            continue;
        }
        for field in &message.fields {
            let is_duration = field.name.ends_with("_ms")
                || field.name.ends_with("_ms_list")
                || field.name.contains("_ms_");
            let is_64_bit = matches!(
                field.kind,
                ScalarKind::Int64
                    | ScalarKind::Uint64
                    | ScalarKind::Fixed64
                    | ScalarKind::Sfixed64
                    | ScalarKind::Sint64
            );
            if is_duration && is_64_bit {
                offenders.push(format!("{}.{}", message.full_name, field.name));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these duration fields are 64-bit and would be quoted in JSON: {offenders:?}"
    );
}

#[test]
fn no_field_in_either_package_is_floating_point() {
    let mut offenders: Vec<String> = Vec::new();
    for message in schema().messages() {
        if !message.full_name.starts_with("gp.") {
            continue;
        }
        for field in &message.fields {
            if matches!(field.kind, ScalarKind::Double | ScalarKind::Float) {
                offenders.push(format!("{}.{}", message.full_name, field.name));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "the sim is integer-only (AGENTS.md section 4.2); these fields are not: {offenders:?}"
    );
}

#[test]
fn the_envelope_kind_tag_is_field_seven() {
    // Item 47 held the number; item 76 defines the enum. If this ever moves,
    // every hand-edited playbook on disk is wrong.
    let playbook_message = schema()
        .message("gp.v1.Playbook")
        .expect("gp.v1.Playbook is in the schema");
    let kind = playbook_message
        .field_by_name("kind")
        .expect("Playbook.kind exists");
    assert_eq!(kind.number, 7);
    assert_eq!(kind.kind, ScalarKind::Enum);

    let declared = schema()
        .enumeration("gp.v1.Playbook.Kind")
        .expect("the Kind enum is nested in Playbook");
    let spelled: Vec<(&str, i32)> = declared
        .values
        .iter()
        .map(|value| (value.name.as_str(), value.number))
        .collect();
    assert_eq!(
        spelled,
        vec![
            ("KIND_UNSPECIFIED", 0),
            ("PLAYBOOK", 1),
            ("TEMPLATE", 2),
            ("SAMPLE", 3),
        ]
    );
}

#[test]
fn every_reserved_number_is_still_reserved() {
    // A table test, as the acceptance asks: the whole reservation set, written
    // out as a golden, so a reservation cannot be dropped without the diff
    // saying so. Un-reserving is how a held field number turns into a silent
    // format break.
    let mut out = String::new();
    out.push_str("# Reserved field numbers and names in gp.v1 and gp.api.v1.\n");
    out.push_str("# Generated from crates/proto/src/generated/descriptor.binpb.\n");
    out.push_str("# Ranges are inclusive at both ends. A reservation is a promise:\n");
    out.push_str("# dropping one from this file is a WIRE_JSON break (AGENTS.md section 5).\n");

    let mut render = |full_name: &str, ranges: &[(i32, i32)], names: &[String]| {
        if ranges.is_empty() && names.is_empty() {
            return;
        }
        let _ignored = writeln!(out, "\n{full_name}");
        if !ranges.is_empty() {
            let spelled: Vec<String> = ranges
                .iter()
                .map(|(start, end)| {
                    if start == end {
                        start.to_string()
                    } else {
                        format!("{start}-{end}")
                    }
                })
                .collect();
            let _ignored = writeln!(out, "  numbers: {}", spelled.join(", "));
        }
        if !names.is_empty() {
            let _ignored = writeln!(out, "  names:   {}", names.join(", "));
        }
    };

    for message in schema().messages() {
        if message.full_name.starts_with("gp.") {
            render(
                &message.full_name,
                &message.reserved_ranges,
                &message.reserved_names,
            );
        }
    }
    for declared in schema().enums() {
        if declared.full_name.starts_with("gp.") {
            render(
                &declared.full_name,
                &declared.reserved_ranges,
                &declared.reserved_names,
            );
        }
    }

    write_actual("reserved.txt", &out);
    assert_same("reserved.txt", &read_expected("reserved.txt"), &out);
}

// ---------------------------------------------------------------------------
// The gateway's scope table and wire casing
// ---------------------------------------------------------------------------

#[test]
fn every_method_declares_a_scope() {
    let declared = schema()
        .enumeration("gp.api.v1.Method")
        .expect("the Method enum is in the schema");
    let mut rows: Vec<(String, String)> = Vec::new();
    for value in &declared.values {
        if value.number == 0 {
            continue;
        }
        let method = Method::try_from(value.number).expect("a generated Method value");
        let found = scope::required_scope(method).unwrap_or_else(|| {
            panic!(
                "{} carries no [(required_scope)] annotation in gateway.proto",
                value.name
            )
        });
        rows.push((
            scope::wire_name("gp.api.v1.Method", &value.name),
            scope::wire_name("gp.api.v1.Scope", found.as_str_name()),
        ));
    }

    let mut out = String::new();
    out.push_str("# Seat Gateway methods and the scope each one needs.\n");
    out.push_str("# Generated from the [(required_scope)] annotations in\n");
    out.push_str("# proto/gp/api/v1/gateway.proto: this file is a view of the schema,\n");
    out.push_str("# never a second table to keep in step.\n");
    out.push_str("# Both columns are the JSON-RPC wire spelling (decisions-log item 80).\n\n");
    for (method, needed) in &rows {
        let _ignored = writeln!(out, "{method:<24} {needed}");
    }

    write_actual("method-scopes.txt", &out);
    assert_same(
        "method-scopes.txt",
        &read_expected("method-scopes.txt"),
        &out,
    );
}

#[test]
fn a_seat_scope_is_never_spectate_nofog() {
    // Spec section 12's token invariant, as far as the schema can carry it: no
    // method is gated on spectate.nofog, because fog is a per-match
    // server-side policy and not something a seat asks for.
    let declared = schema()
        .enumeration("gp.api.v1.Method")
        .expect("the Method enum is in the schema");
    for value in &declared.values {
        if value.number == 0 {
            continue;
        }
        let method = Method::try_from(value.number).expect("a generated Method value");
        assert_ne!(
            scope::required_scope(method),
            Some(Scope::SpectateNofog),
            "{} is gated on spectate.nofog, which no seat token can ever hold",
            value.name
        );
    }
}

#[test]
fn enum_params_travel_as_lower_case_strings() {
    // Decisions-log item 80, and spec section 12's own worked example:
    // verify_plan{depth:"quick"}.
    assert_eq!(
        scope::wire_name("gp.api.v1.VerifyPlan.Depth", "QUICK"),
        "quick"
    );
    assert_eq!(
        scope::wire_name("gp.api.v1.VerifyPlan.Depth", "FULL"),
        "full"
    );
    assert_eq!(
        scope::wire_name("gp.api.v1.ReadOptions.Detail", "STANDARD"),
        "standard"
    );
    // The scope names are the one place a `_` becomes a `.`, because spec
    // section 12 spells them plan.submit and spectate.nofog.
    assert_eq!(
        scope::wire_name("gp.api.v1.Scope", "SCOPE_PLAN_SUBMIT"),
        "plan.submit"
    );
    assert_eq!(
        scope::wire_name("gp.api.v1.Scope", "SCOPE_SPECTATE_NOFOG"),
        "spectate.nofog"
    );

    // And back again, for every value of every enum the gateway exposes.
    for name in [
        "gp.api.v1.Scope",
        "gp.api.v1.VerifyPlan.Depth",
        "gp.api.v1.ReadOptions.Detail",
        "gp.api.v1.Method",
    ] {
        let declared = schema().enumeration(name).expect("in the schema");
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for value in &declared.values {
            let wire = scope::wire_name(name, &value.name);
            assert_eq!(
                scope::from_wire_name(name, &wire).as_deref(),
                Some(value.name.as_str()),
                "{name}: `{wire}` did not translate back"
            );
            assert!(
                seen.insert(wire.clone(), value.name.clone()).is_none(),
                "{name}: two values share the wire spelling `{wire}`"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The rules table
// ---------------------------------------------------------------------------

#[test]
fn the_rules_table_is_in_canonical_form() {
    let path = workspace_root().join("rules").join("rules.v1.json");
    let text = fs::read_to_string(&path).expect("reading rules/rules.v1.json");
    let canonical = json::canonicalise("gp.v1.RulesTable", &text).expect("the rules table decodes");
    write_actual("rules.v1.json", &canonical);
    assert_same("rules.v1.json", &read_expected("rules.v1.json"), &canonical);
    assert_same("rules/rules.v1.json on disk", &text, &canonical);
}

/// The committed table, decoded. A plain helper rather than an inline read in
/// each test, because six tests below read the same file.
fn committed_rules_table() -> RulesTable {
    let path = workspace_root().join("rules").join("rules.v1.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    json::decode(&text).unwrap_or_else(|error| panic!("the rules table decodes: {error:?}"))
}

#[test]
fn the_rules_table_carries_the_numbers_the_decisions_log_fixed() {
    let table = committed_rules_table();

    let locomotion = table.locomotion.expect("the locomotion block");
    assert_eq!(locomotion.step_cost_cardinal, 10, "item 59");
    assert_eq!(locomotion.step_cost_diagonal, 14, "item 59");
    assert_eq!(locomotion.climb_surcharge, 4, "item 59");
    assert_eq!(locomotion.move_cost_per_tick, 3, "item 59");
    assert_eq!(locomotion.repath_cap_per_tick, 16, "item 69");
    assert_eq!(locomotion.hpa_cluster_voxels, 32, "item 58");
    assert_eq!(
        (
            locomotion.fog_cost_numerator,
            locomotion.fog_cost_denominator
        ),
        (3, 2),
        "item 61"
    );

    let matched = table.r#match.expect("the match block");
    assert_eq!(
        matched.segment_lengths_ms,
        vec![180_000, 300_000, 480_000],
        "item 68: the 3 / 5 / 8 ladder"
    );

    let mesher = table.mesher.expect("the mesher block");
    assert_eq!(mesher.surfaces_per_frame, 4, "item 54: K");
    assert_eq!(mesher.bytes_per_frame, 512 * 1024, "item 54: B");

    let interface = table.interface_times.expect("the interface times");
    assert_eq!(interface.visit_handshake_ms, 1_500, "spec section 5");
    assert_eq!(interface.switch_mandate_ms, 8_000, "spec section 5");
    assert_eq!(interface.recycle_ms, 10_000, "spec section 5");
    assert_eq!(interface.place_beacon_deploy_ms, 12_000, "spec section 5");
}

/// One unit kind's four numbers, in the order item 90 writes them:
/// `$` cost / HP / draw kW / cost per second.
fn unit(kind: Option<rules_table::UnitKind>, name: &str) -> (u32, u32, u32, u32) {
    let kind = kind.unwrap_or_else(|| panic!("the {name} row"));
    (
        kind.cost_dollars,
        kind.hp,
        kind.draw_kw,
        kind.cost_per_second,
    )
}

/// One structure kind's three numbers: `$` cost / HP / draw kW.
fn structure(kind: Option<rules_table::StructureKind>, name: &str) -> (u32, u32, u32) {
    let kind = kind.unwrap_or_else(|| panic!("the {name} row"));
    (kind.cost_dollars, kind.hp, kind.draw_kw)
}

/// A `ByRichness` row as lean / standard / rich.
fn by_richness(row: Option<ByRichness>, name: &str) -> (u32, u32, u32) {
    let row = row.unwrap_or_else(|| panic!("the {name} row"));
    (row.lean, row.standard, row.rich)
}

// Item 90's whole point is that "the numbers below are the numbers, so no task
// writes its own". Neither of the other two guards can tell a typo from a
// decision: `tests/golden/proto/expected.rules.v1.json` is blessed FROM
// `rules/rules.v1.json`, and `rules_hash` re-pins whatever that file says. The
// three tests below are what tie revision 3's rows to the log entries that
// fixed them, so a value that moves without a decision behind it goes red here
// first. They are split by block only to stay under clippy's line limit.

#[test]
fn the_rules_table_carries_item_90s_timing_and_economy_numbers() {
    let table = committed_rules_table();

    let matched = table.r#match.expect("the match block");
    assert_eq!(
        matched.decision_tick_ms, 250,
        "item 90: five sim ticks at 20 Hz"
    );

    let locomotion = table.locomotion.expect("the locomotion block");
    assert_eq!(locomotion.commander_cost_per_second, 10, "item 90");
    assert_eq!(locomotion.drone_cost_per_second, 10, "item 90");
    assert_eq!(locomotion.raider_cost_per_second, 12, "item 90");
    assert_eq!(locomotion.scout_cost_per_second, 20, "item 90");

    let economy = table.economy.expect("the economy block");
    assert_eq!(economy.bmi_dollars, 100, "item 90");
    assert_eq!(economy.starting_bmi_multiplier, 2, "item 90");
    assert_eq!(economy.scaling_last_place_bonus_percent, 10, "item 90");
    assert_eq!(economy.scaling_leader_malus_percent, 5, "item 90");
    assert_eq!(economy.award_fund_percent_of_bmi, 50, "item 90");
    assert_eq!(economy.recycle_refund_percent, 50, "item 90");
    assert_eq!(economy.salvage_percent, 25, "item 90");
    assert_eq!(economy.backlog_threshold_per_drone, 2, "item 90");
    assert_eq!(
        by_richness(economy.ore_yield_per_voxel_dollars, "ore yield"),
        (2, 4, 8),
        "item 90"
    );
    assert_eq!(economy.seam_voxels, 150, "item 90");
    assert_eq!(economy.mining_ms_per_voxel, 2_000, "item 90");

    let power = table.power.expect("the power block");
    assert_eq!(power.core_surplus_kw, 10, "item 90");
    assert_eq!(
        by_richness(power.generator_output_kw, "generator output"),
        (20, 30, 40),
        "item 90"
    );
    assert_eq!(power.kw_per_unit, 1, "item 91");
    assert_eq!(power.reserve_percent, 40, "item 91");
    assert_eq!(power.map_ceiling_kw, 190, "item 91");
    assert_eq!(power.revive_margin_kw, 2, "item 90");
    assert_eq!(power.beacon_base_draw_kw, 2, "item 90");
}

#[test]
fn the_rules_table_carries_item_90s_commander_beacon_and_map_numbers() {
    let table = committed_rules_table();

    let commander = table.commander.expect("the commander block");
    assert_eq!(commander.hp, 300, "item 90");
    assert_eq!(commander.cost_per_second, 10, "item 90: 1.0 voxels/s");
    assert_eq!(commander.respawn_base_ms, 30_000, "item 90");
    assert_eq!(commander.respawn_growth_ms, 15_000, "item 90");
    assert_eq!(commander.arrive_radius_voxels, 2, "item 90");
    assert_eq!(commander.placement_range_voxels, 12, "item 90");
    assert_eq!(commander.interface_range_voxels, 4, "item 90, item 11");

    let beacon = table.beacon.expect("the beacon block");
    assert_eq!(beacon.sphere_radius_voxels, 24, "item 90, item 11");
    assert_eq!(beacon.core_hp, 3_000, "item 90");

    let map = table.map.expect("the map block");
    assert_eq!(
        (map.size_x, map.size_y, map.size_z),
        (384, 384, 64),
        "item 90: twelve chunks square, two high"
    );
    assert_eq!(map.spawn_zones, 3, "item 90");
    assert_eq!(map.spawn_zone_radius_voxels, 24, "item 90");
    assert_eq!(
        map.start_vent_richness,
        i32::from(Richness::Lean),
        "item 90"
    );
    assert_eq!(
        (map.vent_min_distance_voxels, map.vent_max_distance_voxels),
        (28, 44),
        "item 90"
    );
    assert_eq!(
        map.start_seam_richness,
        i32::from(Richness::Standard),
        "item 90"
    );
    assert_eq!(
        (map.seam_min_distance_voxels, map.seam_max_distance_voxels),
        (8, 20),
        "item 90"
    );
    assert_eq!(map.contested_standard_vents, 2, "item 90");
    assert_eq!(map.contested_rich_vents, 1, "item 90");
    assert_eq!(map.contested_rich_seams, 3, "item 90");
    assert_eq!(
        (
            map.spawn_separation_pushes_numerator,
            map.spawn_separation_pushes_denominator
        ),
        (3, 2),
        "item 90: 3/2 early Pushes of raider travel"
    );
}

#[test]
fn the_rules_table_carries_item_90s_unit_structure_and_verifier_numbers() {
    let table = committed_rules_table();

    let units = table.units.expect("the units block");
    assert_eq!(
        unit(units.build_drone, "build drone"),
        (20, 100, 1, 10),
        "item 90"
    );
    assert_eq!(
        unit(units.mining_drone, "mining drone"),
        (20, 100, 1, 10),
        "item 90"
    );
    assert_eq!(
        unit(units.repair_drone, "repair-reclaim drone"),
        (25, 100, 1, 10),
        "item 90"
    );
    assert_eq!(unit(units.raider, "raider"), (30, 120, 1, 12), "item 90");
    assert_eq!(unit(units.scout, "scout"), (10, 60, 1, 20), "item 90");
    assert_eq!(
        units.starting_build_drones, 2,
        "item 95: two starting build drones"
    );
    assert_eq!(
        units.starting_mining_drones, 1,
        "item 95: one starting mining drone"
    );

    let structures = table.structures.expect("the structures block");
    assert_eq!(
        structure(structures.beacon, "beacon"),
        (60, 800, 2),
        "item 90"
    );
    assert_eq!(
        structure(structures.generator, "generator"),
        (80, 600, 0),
        "item 90: the Generator supplies, so it draws nothing"
    );
    assert_eq!(
        structure(structures.autocannon, "autocannon"),
        (60, 500, 2),
        "item 90"
    );
    assert_eq!(
        structure(structures.mortar, "mortar"),
        (90, 400, 3),
        "item 90"
    );
    assert_eq!(
        structure(structures.survey_post, "survey post"),
        (30, 200, 1),
        "item 90"
    );
    assert_eq!(
        structure(structures.resonance_spire, "Resonance Spire"),
        (120, 500, 3),
        "item 90"
    );
    assert_eq!(structures.wall_cost_per_voxel, 1, "item 90: per voxel");
    assert_eq!(structures.demolition_charge_cost_dollars, 15, "item 90");

    let verifier = table.verifier.expect("the verifier block");
    assert_eq!(verifier.size_budget_units, 128, "item 94");
    assert_eq!(verifier.handler_cooldown_min_ms, 5_000, "item 90");
    assert_eq!(verifier.max_fires_max, 8, "item 90");
    assert_eq!(verifier.notebook_max_chars, 4_000, "item 90");
    assert_eq!(verifier.reach_memory_ms, 180_000, "item 90: S2/S3");
}

#[test]
fn the_map_supply_adds_up_to_the_ceiling() {
    // Item 90 and item 91 state the sum out loud — three seats supply
    // 3 x 10 + 3 x 20 + 2 x 30 + 40 = 190 kW, exactly `map_ceiling_kw` — and
    // the map generator is sized against it. The sum is a PROPERTY of six
    // rows fixed in two different log entries, so a tuning PR that moves one
    // of them without the others goes red here with the arithmetic in hand.
    let table = committed_rules_table();
    let power = table.power.expect("the power block");
    let map = table.map.expect("the map block");
    let generator = power.generator_output_kw.expect("the generator output row");

    let cores = map.spawn_zones * power.core_surplus_kw;
    let start_vents = map.spawn_zones * generator.lean;
    let contested = map.contested_standard_vents * generator.standard
        + map.contested_rich_vents * generator.rich;

    assert_eq!(
        cores + start_vents + contested,
        power.map_ceiling_kw,
        "items 90 and 91: total supply at three seats is exactly the ceiling"
    );
}

#[test]
fn the_restated_rows_agree() {
    // Eleven numbers are stated in two places on purpose, because a reader of
    // the `units` or `structures` block should see a whole unit or a whole
    // building without cross-referencing `power` and `locomotion`. Nothing in
    // the schema makes a pair move together, and the two halves of a pair are
    // re-derived at DIFFERENT stages (item 90 moves the drone speed at S1 and
    // the raider's at S2; item 91 moves `kw_per_unit` at S2's exit), so a
    // tuner editing one side at its own stage is the expected case rather
    // than a hypothetical. This test is the only thing that catches it.
    //
    // rules/README.md names the same three families under "Where the numbers
    // come from"; keep the two in step.
    let table = committed_rules_table();
    let locomotion = table.locomotion.expect("the locomotion block");
    let power = table.power.expect("the power block");
    let commander = table.commander.expect("the commander block");
    let units = table.units.expect("the units block");
    let structures = table.structures.expect("the structures block");

    // 1. Draw per fielded unit: `power.kw_per_unit` is every kind's `draw_kw`.
    for (name, kind) in [
        ("build_drone", units.build_drone),
        ("mining_drone", units.mining_drone),
        ("repair_drone", units.repair_drone),
        ("raider", units.raider),
        ("scout", units.scout),
    ] {
        let kind = kind.unwrap_or_else(|| panic!("the {name} row"));
        assert_eq!(
            kind.draw_kw, power.kw_per_unit,
            "units.{name}.draw_kw restates power.kw_per_unit (item 91): move both or neither"
        );
    }

    // 2. A beacon's own draw, in `power` and in `structures`.
    let beacon = structures.beacon.expect("the beacon structure row");
    assert_eq!(
        beacon.draw_kw, power.beacon_base_draw_kw,
        "structures.beacon.draw_kw restates power.beacon_base_draw_kw (item 90)"
    );

    // 3. Walking speed, in `locomotion` and per kind. The three drone kinds
    //    share `drone_cost_per_second`; the commander is not a unit kind, so
    //    its row is in `commander` rather than in `units`.
    for (name, kind) in [
        ("build_drone", units.build_drone),
        ("mining_drone", units.mining_drone),
        ("repair_drone", units.repair_drone),
    ] {
        let kind = kind.unwrap_or_else(|| panic!("the {name} row"));
        assert_eq!(
            kind.cost_per_second, locomotion.drone_cost_per_second,
            "units.{name}.cost_per_second restates locomotion.drone_cost_per_second (item 90)"
        );
    }
    assert_eq!(
        units.raider.expect("the raider row").cost_per_second,
        locomotion.raider_cost_per_second,
        "units.raider.cost_per_second restates locomotion.raider_cost_per_second (item 90)"
    );
    assert_eq!(
        units.scout.expect("the scout row").cost_per_second,
        locomotion.scout_cost_per_second,
        "units.scout.cost_per_second restates locomotion.scout_cost_per_second (item 90)"
    );
    assert_eq!(
        commander.cost_per_second, locomotion.commander_cost_per_second,
        "commander.cost_per_second restates locomotion.commander_cost_per_second (item 90)"
    );
}

// ---------------------------------------------------------------------------
// The plan fingerprint
// ---------------------------------------------------------------------------

#[test]
fn the_fingerprint_is_taken_over_the_playbook_without_its_own_fingerprint() {
    let raw = fs::read_to_string(
        workspace_root()
            .join("examples")
            .join("playbooks")
            .join("expand_east.jsonc"),
    )
    .expect("reading the example playbook");
    let mut playbook: Playbook =
        json::decode(&strip_line_comments(&raw)).expect("the example decodes");

    let before = fingerprint::canonical_input(&playbook).expect("canonical input");
    if let Some(meta) = playbook.meta.as_mut() {
        meta.fingerprint = fingerprint::to_field(0x0123_4567_89ab_cdef);
    }
    let after = fingerprint::canonical_input(&playbook).expect("canonical input");
    assert_eq!(
        before, after,
        "a fingerprint already in the file must not change the bytes the next one is taken over"
    );

    // Eight bytes, big-endian, both ways.
    let field = fingerprint::to_field(0x0123_4567_89ab_cdef);
    assert_eq!(field.len(), 8);
    assert_eq!(field.first(), Some(&0x01));
    assert_eq!(fingerprint::from_field(&field), Some(0x0123_4567_89ab_cdef));
    assert_eq!(fingerprint::from_field(&[]), None);
    assert_eq!(fingerprint::from_field(&[0, 1, 2]), None);
}

// ---------------------------------------------------------------------------
// The generated tree, as source text
// ---------------------------------------------------------------------------

#[test]
fn the_generated_source_names_no_determinism_hazard() {
    // AGENTS.md section 4.9: an `#[allow]` on a determinism lint outside a
    // walled crate is a workaround wearing a disguise. crates/proto allows
    // `missing_docs` and `clippy::pedantic` over the generated tree; this test
    // is what makes that safe, because it checks the source text rather than
    // trusting the lint that was switched off.
    //
    // The spike lesson G2 and G3' both wrote down: copy the source-text test,
    // do not rely on the lint alone.
    let generated = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("generated");
    let files = [
        generated.join("gp").join("v1").join("gp.v1.rs"),
        generated
            .join("gp")
            .join("api")
            .join("v1")
            .join("gp.api.v1.rs"),
    ];
    let hazards = [
        "f32",
        "f64",
        "HashMap",
        "HashSet",
        "Instant",
        "SystemTime",
        "RandomState",
    ];
    for path in &files {
        let source = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        for line in source.lines() {
            let code = line.trim_start();
            // Doc comments carry the .proto prose, which is allowed to contain
            // the word "as" and anything else.
            if code.starts_with("//") {
                continue;
            }
            for hazard in hazards {
                assert!(
                    !names_token(code, hazard),
                    "{} names `{hazard}`: {code}",
                    path.display()
                );
            }
            assert!(
                !code.contains(" as "),
                "{} contains an `as` cast: {code}",
                path.display()
            );
        }
    }
}

/// Whether a line names an identifier, rather than merely containing its
/// letters. `InstantiateTemplateRequest` contains "Instant" and is not a
/// wall-clock read; a test that cannot tell the difference is a test nobody
/// will trust the second time it fires.
fn names_token(line: &str, token: &str) -> bool {
    let mut from = 0_usize;
    while let Some(offset) = line.get(from..).and_then(|rest| rest.find(token)) {
        let start = from.saturating_add(offset);
        let end = start.saturating_add(token.len());
        let before = line.get(..start).and_then(|head| head.chars().next_back());
        let after = line.get(end..).and_then(|tail| tail.chars().next());
        let boundary = |character: Option<char>| {
            character.is_none_or(|found| !found.is_alphanumeric() && found != '_')
        };
        if boundary(before) && boundary(after) {
            return true;
        }
        from = end;
    }
    false
}
