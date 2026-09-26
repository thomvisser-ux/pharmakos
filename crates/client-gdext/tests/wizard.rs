// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The template wizard and the rule list, with no engine and no host (skeleton plan T19,
//! pull request 2; `docs/design/skeleton-plan-w6-notes.md` section A4, as decisions-log
//! item 112 amends it).
//!
//! * `the_wizard_fixture_is_the_gateways_golden` — Godot cannot load a file outside
//!   `res://`, so `godot/fixtures/instantiate_suggested.json` is a byte-identical copy of
//!   the gateway's `tests/golden/gateway/instantiate_suggested/expected.response.json`, the
//!   render-only wizard scene draws it, and this test keeps it a copy. **The lane that
//!   changes `library/` moves that golden, and so owns the copy too**: re-copy it in the same
//!   pull request, and this file's page pins with it.
//! * the pages built from that fixture: order, labels, raw values, marks and the why, each
//!   exactly as the gateway wrote them;
//! * an edited value's request: sent exactly as typed, as the one explicit value, with
//!   `suggested` still true;
//! * a refusal shown as it came, and nothing retried;
//! * the gateway's playbook put into the editor byte for byte, as one edit Undo takes back;
//! * `the_rule_list_is_render_plans_lines`, over plan-core's own prose golden, read by path;
//! * `the_client_names_no_template`: no template id, pointer or label in the client's code
//!   outside its tests and fixtures, so a `library/` change needs no client change.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::fs;
use std::path::{Path, PathBuf};

use pharmakos_client_gdext::editor::{Editor, Verdict};
use pharmakos_client_gdext::wizard::{Instance, Wizard, instance_of_text, prose_rows};
use pharmakos_proto::json::{self, Json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn gateway_golden() -> PathBuf {
    root()
        .join("tests")
        .join("golden")
        .join("gateway")
        .join("instantiate_suggested")
        .join("expected.response.json")
}

fn godot_copy() -> PathBuf {
    root()
        .join("godot")
        .join("fixtures")
        .join("instantiate_suggested.json")
}

fn fixture_text() -> String {
    fs::read_to_string(godot_copy()).expect("godot/fixtures/instantiate_suggested.json")
}

/// The fixture's `result`, as the wire carries it.
fn fixture_result() -> Json {
    json::read(&fixture_text())
        .expect("the fixture is JSON")
        .get("result")
        .cloned()
        .expect("the fixture is a whole JSON-RPC answer with a result")
}

fn text_of(value: Option<&Json>) -> String {
    match value {
        Some(Json::String(text)) => text.clone(),
        other => panic!("a string, not {other:?}"),
    }
}

#[test]
fn the_wizard_fixture_is_the_gateways_golden() {
    let golden = fs::read(gateway_golden()).expect("the gateway's instantiate_suggested golden");
    let copy = fs::read(godot_copy()).expect("godot/fixtures/instantiate_suggested.json");
    assert!(
        golden == copy,
        "godot/fixtures/instantiate_suggested.json is not a byte-identical copy of \
         tests/golden/gateway/instantiate_suggested/expected.response.json. The gateway is \
         the producer, and the golden embeds a library/ template: re-copy the golden over the \
         fixture, bring this file's page pins up to date with it, and say in the pull request \
         which of the golden README's four things moved. The lane that changes library/ owns \
         this copy too."
    );
}

#[test]
fn the_pages_are_the_fixtures_parameters_as_they_came() {
    let instance: Instance = instance_of_text(&fixture_text()).expect("the fixture decodes");
    let result = fixture_result();
    let Some(Json::Array(parameters)) = result.get("parameters") else {
        panic!("the fixture lists its parameters");
    };
    assert_eq!(
        instance.pages.len(),
        parameters.len(),
        "one page per declared parameter"
    );
    for (page, parameter) in instance.pages.iter().zip(parameters) {
        assert_eq!(page.pointer, text_of(parameter.get("pointer")), "in order");
        assert_eq!(
            page.label,
            text_of(parameter.get("label")),
            "the label as it came"
        );
        assert_eq!(
            page.value,
            text_of(parameter.get("value")),
            "the value as raw JSON text, unconverted"
        );
        assert_eq!(
            Some(&Json::Bool(page.suggested)),
            parameter.get("suggested"),
            "the mark exactly where the gateway set it"
        );
    }
    assert_eq!(
        instance.why,
        text_of(result.get("why")),
        "the why as it came"
    );
    assert_eq!(
        instance.playbook_jsonc,
        text_of(result.get("playbook_jsonc")),
        "the playbook byte for byte"
    );

    // The pins: what this fixture is today. They move when the golden does (the lane that
    // changes library/ or the operator re-copies the fixture and updates them).
    let marks: Vec<bool> = instance.pages.iter().map(|page| page.suggested).collect();
    assert_eq!(
        marks,
        [true, false],
        "the anchor is the operator's, the hold the template's"
    );
    let values: Vec<&str> = instance
        .pages
        .iter()
        .map(|page| page.value.as_str())
        .collect();
    assert_eq!(values, [r#"{"x":150,"y":13,"z":118}"#, "30000"]);
    assert_eq!(
        instance.pages.first().map(|page| page.label.as_str()),
        Some("Where the Generator goes: a heat vent inside the beacon's sphere")
    );
    assert_eq!(
        instance.pages.get(1).map(|page| page.label.as_str()),
        Some("How long to hold at the beacon, in game milliseconds"),
        "a duration stays game milliseconds: unit display is S6's"
    );
    assert_eq!(
        instance.why,
        "The heat vent nearest your core is inside its sphere."
    );
}

/// An editor whose wizard is open and showing the fixture's answer.
fn wizard_on_the_fixture() -> Editor {
    let mut editor = Editor::new("seat.0");
    editor.wizard_open("a_template");
    let call = editor.next_call(false).expect("the instantiation");
    assert_eq!(call.method, "instantiate_template");
    let params = json::write(&call.params);
    assert!(params.contains(r#""suggested": true"#), "{params}");
    assert!(
        params.contains(r#""parameters": []"#),
        "nothing explicit before anything is typed: {params}"
    );
    editor.answered(Ok(&fixture_result())).expect("reads");
    assert!(editor.wizard().is_some_and(Wizard::current));
    editor
}

#[test]
fn an_edited_value_is_sent_exactly_as_typed_and_alone() {
    let mut editor = wizard_on_the_fixture();
    let pages = editor
        .wizard()
        .and_then(|wizard| wizard.instance.as_ref())
        .map(|instance| instance.pages.clone())
        .expect("pages");
    let hold = pages.get(1).expect("a second page");
    // Typed with spaces and in a form the client does not understand: it is not parsed,
    // checked or converted, only sent.
    let typed = " 45 000ms ";
    assert!(editor.wizard_set(&hold.pointer, typed));
    assert!(
        !editor.wizard().is_some_and(Wizard::current),
        "the pages on screen no longer answer the newest ask"
    );
    assert!(
        !editor.wizard_use(),
        "nothing is used until the gateway answers"
    );
    let call = editor.next_call(false).expect("the instantiation again");
    assert_eq!(call.method, "instantiate_template");
    let parameters = call.params.get("parameters").expect("parameters");
    let Json::Array(items) = parameters else {
        panic!("an array, not {parameters:?}");
    };
    assert_eq!(items.len(), 1, "only the edited page is explicit");
    let item = items.first().expect("one");
    assert_eq!(item.get("name"), Some(&Json::String(hold.pointer.clone())));
    assert_eq!(item.get("value"), Some(&Json::String(typed.to_owned())));
    assert_eq!(call.params.get("suggested"), Some(&Json::Bool(true)));
    assert_eq!(
        call.params.get("template_id"),
        Some(&Json::String("a_template".to_owned()))
    );
}

#[test]
fn a_refusal_is_shown_as_the_gateway_wrote_it() {
    let mut editor = wizard_on_the_fixture();
    let pointer = editor
        .wizard()
        .and_then(|wizard| wizard.instance.as_ref())
        .and_then(|instance| instance.pages.first())
        .map(|page| page.pointer.clone())
        .expect("a page");
    assert!(editor.wizard_set(&pointer, "not json"));
    let _ = editor.next_call(false).expect("the instantiation");
    let message = "`not json` is not a JSON value: expected a value at byte 0";
    editor
        .answered(Err(("INVALID_ARGUMENT", message)))
        .expect("settled");
    let wizard = editor.wizard().expect("still open").clone();
    assert_eq!(
        wizard.refusal.as_deref(),
        Some(format!("INVALID_ARGUMENT: {message}").as_str()),
        "as the gateway wrote it"
    );
    assert!(!wizard.current(), "a refused ask cannot be used");
    assert_eq!(editor.status().key, "wizard_refused");
    assert!(!editor.wizard_use());
    assert!(
        editor.next_call(false).is_none(),
        "nothing is retried with another value"
    );
    assert_eq!(
        wizard.explicit,
        vec![(pointer, "not json".to_owned())],
        "the value stays as typed, for the player to change"
    );
}

#[test]
fn using_the_wizard_puts_the_gateways_text_in_byte_for_byte_and_undo_takes_it_back() {
    let mut editor = wizard_on_the_fixture();
    let text = text_of(fixture_result().get("playbook_jsonc"));
    assert!(editor.wizard_use());
    // Applied in turn, with no call of its own; QUICK follows.
    let call = editor.next_call(false).expect("QUICK for the new text");
    assert_eq!(call.method, "verify_plan");
    assert_eq!(
        editor.bytes(),
        text.as_bytes(),
        "the gateway's playbook_jsonc"
    );
    assert_eq!(editor.undo_depth(), 1, "one edit");
    editor
        .answered(Ok(&json::read(
            r#"{"report":{"qualifies":true,"depth":"quick","diagnostics":[]}}"#,
        )
        .expect("json")))
        .expect("reads");
    assert_eq!(editor.verdict(), Verdict::Quick);
    let render = editor.next_call(false).expect("the rule list");
    assert_eq!(render.method, "render_plan");
    editor
        .answered(Ok(
            &json::read(r#"{"prose":"Hold & Build\n"}"#).expect("json")
        ))
        .expect("reads");
    assert!(editor.prose_current());

    // Undo: back to no text at all, with no call.
    assert!(editor.undo());
    assert!(
        editor.next_call(false).is_none(),
        "an Undo of the wizard asks nothing"
    );
    assert!(!editor.has_text());
    assert_eq!(editor.undo_depth(), 0);
    assert!(editor.prose().is_empty());
}

#[test]
fn the_rule_list_is_render_plans_lines() {
    let path = root()
        .join("tests")
        .join("golden")
        .join("plan-core")
        .join("expand_east")
        .join("expected.prose.txt");
    let prose =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut editor = Editor::new("seat.0");
    assert!(editor.load(b"{}"));
    let _ = editor.next_call(false).expect("the load check");
    editor
        .answered(Ok(&json::read(
            r#"{"report":{"qualifies":true,"depth":"quick"}}"#,
        )
        .expect("json")))
        .expect("reads");
    let call = editor.next_call(false).expect("the rule list");
    assert_eq!(call.method, "render_plan");
    let answer = Json::Object(vec![
        ("prose".to_owned(), Json::String(prose.clone())),
        (
            "_status".to_owned(),
            json::read(r#"{"phase":"lull","round":1}"#).expect("json"),
        ),
    ]);
    editor.answered(Ok(&answer)).expect("reads");
    let expected: Vec<&str> = prose.split_terminator('\n').collect();
    assert_eq!(
        editor.prose(),
        expected.as_slice(),
        "one row per line of the gateway's prose, each exactly as it came"
    );
    assert_eq!(prose_rows(&prose), expected);
    assert!(editor.prose_current());
    assert!(
        editor.prose().iter().any(|line| line.starts_with("  ")),
        "indents are kept: they are what says a line is a step or a rule"
    );
}

/// Every `.rs` under the crate's `src/` and every `.gd` under `godot/scripts/`, but the
/// checks: `watch_check.gd` drives a real host and names Hold & Build to pin it.
fn client_sources() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    for (dir, extension) in [
        (Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), "rs"),
        (root().join("godot").join("scripts"), "gd"),
    ] {
        let entries = fs::read_dir(&dir).expect("a source folder");
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some(extension)
                && path.file_name().and_then(|name| name.to_str()) != Some("watch_check.gd")
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn the_client_names_no_template() {
    // Every template id, declared pointer and label in library/, read from the files.
    let library = root().join("library");
    let mut needles: Vec<String> = Vec::new();
    let mut templates = fs::read_dir(&library)
        .expect("library/")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("jsonc"))
        .collect::<Vec<_>>();
    templates.sort();
    assert!(!templates.is_empty(), "library/ holds templates");
    for path in &templates {
        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            needles.push(stem.to_owned());
        }
        let text = fs::read_to_string(path).expect("a template");
        // A declaration is a `{"pointer": .., "label": ..}` line in `meta.parameters`; a
        // route step's `label` is not a declaration's.
        for line in text.lines().filter(|line| line.contains("\"pointer\": \"")) {
            for key in ["\"pointer\": \"", "\"label\": \""] {
                if let Some(rest) = line.split(key).nth(1) {
                    if let Some(value) = rest.split('"').next() {
                        needles.push(value.to_owned());
                    }
                }
            }
        }
    }
    // The safe template's one parameter is the whole route, `/declarative/route`: the
    // pointer the map route surface appends to (editor.rs, `ROUTE_POINTER`), which is the
    // playbook's own shape and not something a template declared.
    needles.retain(|needle| needle != "/declarative/route");
    assert!(
        needles.len() >= templates.len().saturating_mul(2),
        "the scan read no declarations out of library/: {needles:?}"
    );
    let mut found: Vec<String> = Vec::new();
    for path in client_sources() {
        let text = fs::read_to_string(&path).expect("a source file");
        let code: String = if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            // A crate's unit tests are tests, not the client's code.
            text.split("#[cfg(test)]").next().unwrap_or("").to_owned()
        } else {
            text
        };
        for needle in &needles {
            if code.contains(needle.as_str()) {
                found.push(format!("{} names `{needle}`", path.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "the wizard is generic over what each template declares: a library/ change needs no \
         client change.\n{}",
        found.join("\n")
    );
}

/// One line of source with its comment and its string literals removed, except a literal
/// that is one identifier, which is kept as that word (GDScript reads a dictionary through
/// such a literal: `page.get("value", "")`). `comment` is the language's line comment.
fn code_without_prose(line: &str, comment: &str) -> String {
    let before_comment = if comment == "#" {
        // A GDScript comment; this scan reads no Rust attribute through this branch.
        line.split('#').next().unwrap_or("")
    } else {
        line.split(comment).next().unwrap_or("")
    };
    let mut out = String::new();
    let mut literal = String::new();
    let mut in_string = false;
    let mut characters = before_comment.chars();
    while let Some(character) = characters.next() {
        if in_string {
            if character == '\\' {
                characters.next();
            } else if character == '"' {
                in_string = false;
                let identifier = literal
                    .chars()
                    .next()
                    .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
                    && literal
                        .chars()
                        .all(|each| each.is_ascii_alphanumeric() || each == '_');
                if identifier {
                    out.push(' ');
                    out.push_str(&literal);
                    out.push(' ');
                }
            } else {
                literal.push(character);
            }
        } else if character == '"' {
            in_string = true;
            literal.clear();
        } else {
            out.push(character);
        }
    }
    out
}

/// Whether a line names a wizard value: an identifier one of whose `_`-separated parts is
/// `value` or `values`.
fn names_a_value(code: &str) -> bool {
    code.to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|word| {
            word.split('_')
                .any(|part| part == "value" || part == "values")
        })
}

/// What converting or computing with a value would take: an arithmetic operator, or a
/// parse or number conversion.
fn converts(code: &str) -> Option<&'static str> {
    const CONVERSIONS: &[&str] = &[
        "int(",
        "float(",
        ".parse",
        "str_to_var",
        "from_str",
        "to_int",
        "to_float",
        "json.parse",
        "parse_string",
        "as_i64",
        "as_u64",
        "as_f64",
    ];
    let lowered = code.to_ascii_lowercase();
    if let Some(found) = CONVERSIONS.iter().find(|each| lowered.contains(**each)) {
        return Some(found);
    }
    let bytes = lowered.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        let next = bytes.get(index.saturating_add(1));
        match byte {
            b'+' | b'*' | b'/' | b'%' => return Some("an arithmetic operator"),
            b'-' if next != Some(&b'>') => return Some("an arithmetic operator"),
            _ => {}
        }
    }
    None
}

/// **The client never parses, checks or converts a wizard value** (w6 notes A4 item 2): a
/// value travels under the name `value`, which is not a quantity word, so the crate's
/// arithmetic scanner (`tests/no_arithmetic.rs`) cannot see a unit conversion of a
/// duration page. This scan can: in `godot/scripts/wizard.gd` and in `src/wizard.rs` outside
/// its unit tests, no line that names a value carries an arithmetic operator or a parse or
/// number conversion. A value is shown and sent as the text it is.
#[test]
fn the_wizard_never_converts_a_value() {
    let mut found: Vec<String> = Vec::new();
    let mut lines_seen = 0_usize;
    for (path, comment) in [
        (
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("wizard.rs"),
            "//",
        ),
        (root().join("godot").join("scripts").join("wizard.gd"), "#"),
    ] {
        let text = fs::read_to_string(&path).expect("a wizard source");
        let code = if comment == "//" {
            text.split("#[cfg(test)]").next().unwrap_or("").to_owned()
        } else {
            text
        };
        for (index, line) in code.lines().enumerate() {
            let stripped = code_without_prose(line, comment);
            if !names_a_value(&stripped) {
                continue;
            }
            lines_seen = lines_seen.saturating_add(1);
            if let Some(what) = converts(&stripped) {
                found.push(format!(
                    "{}:{}: {what}: {}",
                    path.display(),
                    index.saturating_add(1),
                    line.trim()
                ));
            }
        }
    }
    assert!(
        lines_seen >= 4,
        "the scan found almost no line naming a value; has the wizard renamed it?"
    );
    assert!(
        found.is_empty(),
        "the wizard converts or computes with a value; it shows and sends the text as it is:\n{}",
        found.join("\n")
    );
}

/// The scan above catches what it is for, and passes a line that only moves a value.
#[test]
fn the_value_scan_catches_a_conversion() {
    for planted in [
        "\treturn int(page.get(\"value\", \"0\")) / 1000",
        "        p.value.parse::<i64>().ok()",
        "\tvar shown := page.value - 3600",
        "\tvar value := str_to_var(field.text)",
    ] {
        let comment = if planted.contains("::") { "//" } else { "#" };
        let code = code_without_prose(planted, comment);
        assert!(
            names_a_value(&code) && converts(&code).is_some(),
            "{planted}"
        );
    }
    let clean = code_without_prose("\tfield.text = String(page.get(\"value\", \"\"))", "#");
    assert!(names_a_value(&clean) && converts(&clean).is_none());
    // A value named only in a comment is prose, not code.
    let prose = code_without_prose("\tvar shown := 1 - 2 # the value", "#");
    assert!(!names_a_value(&prose));
}
