// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scenario file format, and the checks `cargo xtask ci`'s `scenario` step
//! can make before a runner exists.
//!
//! A scenario is a headless match written down: **a map seed, one playbook per
//! seat, the segment list, and assertions on events *and* on the hash chain**
//! (AGENTS.md section 9 item 10, section 10 item 4). `gamectl scenario run`
//! executes one; it arrives at T15. This module is the format, frozen first, so
//! that every later task delivers *into* a format rather than inventing one —
//! which is the whole reason harness part 2's formats are pulled into wave 1
//! (decisions-log item 75).
//!
//! `scenarios/README.md` is the prose specification and is the file to read
//! first. The rules enforced here are its machine-checkable half; where the two
//! disagree, the README is wrong and this is right, and a pull request should
//! say so.
//!
//! # Why JSONC, and why every rule below exists
//!
//! The file is canonical JSON with `//` and `/* */` comments, exactly like a
//! playbook (spec section 10). A scenario is read by a person far more often
//! than it is written by one: when a nightly run goes red, the first question
//! is "what was this asserting and why", and the answer has to be in the file
//! rather than in a commit message. Hence comments, hence one assertion per
//! line, hence a fixed key order.
//!
//! * **LF endings and a trailing newline, never a tab.** Scenario files and
//!   their hash-chain goldens are byte-compared across Windows, Linux and
//!   macOS; a stray `\r` makes a file that is identical in every way that
//!   matters compare unequal.
//! * **Unknown keys are rejected, never ignored.** The same rule the verifier's
//!   Load applies to playbooks (AGENTS.md section 11): a construct outside the
//!   vocabulary is refused with a code and a JSON Pointer rather than silently
//!   stripped, because a silently stripped assertion is an assertion that
//!   passes by not running.
//! * **Every diagnostic carries a JSON Pointer.** Same convention as the
//!   verifier's diagnostic catalogue, so a scenario failure reads like every
//!   other failure in the project.
//! * **The seed is a quoted `0x`-prefixed 16-digit string, not a number.** JSON
//!   numbers are doubles in half the world's parsers; a 64-bit map seed that
//!   round-trips through one loses its low bits, and the map would then differ
//!   between two readers of the same file.
//! * **Durations are game milliseconds as bare integers** (decisions-log item
//!   46); a negative one is rejected here with a pointer, exactly as the
//!   verifier rejects one in a playbook.

use std::path::{Path, PathBuf};

use crate::Json;

/// The suffix every scenario file carries. Also how the step finds them.
pub(crate) const SUFFIX: &str = ".scenario.jsonc";

/// The value of the mandatory `format` key. A new format is a new string, so an
/// old runner refuses a new file rather than half-understanding it.
pub(crate) const FORMAT: &str = "pharmakos.scenario.v1";

/// The assertion vocabulary, in full.
///
/// PLACEHOLDER: decisions-log item 16 (plan section 7) extends this **once**,
/// at T15, when the runner meets real events — the named candidates are
/// `event_count_in_range`, `state_hash_at_tick` and `terminal_hash`. Owner
/// decides at T15. The vocabulary is data inside the format, not the format, so
/// adding to it is not a format break; removing one would be.
pub(crate) const ASSERTIONS: &[&str] = &["event_fired", "hash_chain_equals"];

/// Names held for item 16's extension. Naming one today is an error that says
/// which task adds it, rather than "unknown assertion".
const RESERVED_ASSERTIONS: &[&str] = &[
    "event_count_in_range",
    "state_hash_at_tick",
    "terminal_hash",
];

const TOP_LEVEL_KEYS: &[&str] = &[
    "format",
    "name",
    "summary",
    "map",
    "seats",
    "segments",
    "assertions",
];
const MAP_KEYS: &[&str] = &["seed", "generator"];
const SEAT_KEYS: &[&str] = &["seat", "kind", "playbook", "operator"];
const SEGMENT_KEYS: &[&str] = &["index", "length_ms", "note"];
const SEAT_KINDS: &[&str] = &["playbook", "safe", "builtin"];

/// At most three seats share a local match: one human and up to two built-in
/// operators (spec section 3; AGENTS.md section 1).
const MAX_SEATS: usize = 3;

/// What a valid scenario file told us, for the step's summary line and for the
/// runner's command line once T15 exists.
#[derive(Debug)]
pub(crate) struct Scenario {
    /// The file it came from, relative to the workspace root.
    pub(crate) relative: PathBuf,
    /// The `name` key.
    pub(crate) name: String,
    /// Seat count.
    pub(crate) seats: usize,
    /// Assertion count.
    pub(crate) assertions: usize,
}

/// One problem with one place in the file.
struct Problem {
    pointer: String,
    message: String,
}

impl Problem {
    fn new(pointer: &str, message: impl Into<String>) -> Problem {
        Problem {
            pointer: pointer.to_owned(),
            message: message.into(),
        }
    }
}

/// Every scenario file under `scenarios/`, in sorted order.
pub(crate) fn collect(root: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = root.join("scenarios");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = Vec::new();
    crate::walk(&dir, &mut files).map_err(|error| format!("reading {}: {error}", dir.display()))?;
    files.retain(|path| crate::file_name(path).ends_with(SUFFIX));
    Ok(files)
}

/// Reads and validates one scenario file. Every problem in the file is
/// reported, not just the first: a scenario is usually written by hand, and
/// five round trips to fix five typos is five round trips.
pub(crate) fn validate(root: &Path, path: &Path) -> Result<Scenario, String> {
    let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    let bytes =
        std::fs::read(path).map_err(|error| format!("reading {}: {error}", path.display()))?;

    let mut problems: Vec<Problem> = Vec::new();
    check_bytes(&bytes, &mut problems);

    let text = String::from_utf8(bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", relative.display()))?;
    let stripped = strip_comments(&text);
    let json = Json::parse(&stripped).map_err(|error| {
        format!(
            "{}: not valid JSON once comments are removed: {error}",
            relative.display()
        )
    })?;

    let mut name = String::new();
    let mut seats = 0;
    let mut assertions = 0;

    if let Json::Object(members) = &json {
        unknown_keys("", members, TOP_LEVEL_KEYS, &mut problems);
        require_exact_string(&json, "/format", "format", FORMAT, &mut problems);
        name = require_string(&json, "/name", "name", &mut problems);
        check_map(&json, &mut problems);
        seats = check_seats(root, &json, &mut problems);
        check_segments(&json, &mut problems);
        assertions = check_assertions(&json, &mut problems);
    } else {
        problems.push(Problem::new("", "a scenario file is a JSON object"));
    }

    if problems.is_empty() {
        return Ok(Scenario {
            relative,
            name,
            seats,
            assertions,
        });
    }

    let mut report = format!("{} is not a valid scenario file:\n", relative.display());
    for problem in &problems {
        report.push_str("        ");
        if problem.pointer.is_empty() {
            report.push_str("(document)");
        } else {
            report.push_str(&problem.pointer);
        }
        report.push_str(" — ");
        report.push_str(&problem.message);
        report.push('\n');
    }
    report.push_str("      scenarios/README.md documents the format.");
    Err(report)
}

// ---------------------------------------------------------------------------
// The individual rules
// ---------------------------------------------------------------------------

fn check_bytes(bytes: &[u8], problems: &mut Vec<Problem>) {
    if bytes.contains(&b'\r') {
        problems.push(Problem::new(
            "",
            "the file contains a carriage return; scenario files and their goldens are \
             byte-compared across Windows, Linux and macOS, so endings are LF",
        ));
    }
    if bytes.contains(&b'\t') {
        problems.push(Problem::new(
            "",
            "the file contains a tab; indentation is two spaces so a diff lines up in every viewer",
        ));
    }
    if bytes.last() != Some(&b'\n') {
        problems.push(Problem::new("", "the file must end with a newline"));
    }
}

fn check_map(json: &Json, problems: &mut Vec<Problem>) {
    let Some(map) = json.get("map") else {
        problems.push(Problem::new(
            "/map",
            "required: the map the scenario runs on",
        ));
        return;
    };
    let Json::Object(members) = map else {
        problems.push(Problem::new("/map", "must be an object"));
        return;
    };
    unknown_keys("/map", members, MAP_KEYS, problems);

    match map.get("seed").and_then(Json::as_str) {
        Some(seed) => {
            let digits = seed.strip_prefix("0x").unwrap_or("");
            let well_formed = digits.len() == 16
                && digits
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
            if !well_formed {
                problems.push(Problem::new(
                    "/map/seed",
                    "must be `0x` followed by exactly 16 lowercase hex digits — the 64-bit map \
                     seed is a quoted string because a JSON number loses its low bits in a \
                     double-backed parser, and a map that differs between two readers of the same \
                     file is not a scenario",
                ));
            }
        }
        None => problems.push(Problem::new(
            "/map/seed",
            "required: the 64-bit map seed, as a quoted 0x-prefixed 16-digit string",
        )),
    }
    if map.get("generator").and_then(Json::as_str).is_none() {
        problems.push(Problem::new(
            "/map/generator",
            "required: the map generator's name, so a scenario written against one generation \
             rule is not silently replayed against another",
        ));
    }
}

fn check_seats(root: &Path, json: &Json, problems: &mut Vec<Problem>) -> usize {
    let Some(Json::Array(seats)) = json.get("seats") else {
        problems.push(Problem::new(
            "/seats",
            "required: an array of one to three seats",
        ));
        return 0;
    };
    if seats.is_empty() || seats.len() > MAX_SEATS {
        problems.push(Problem::new(
            "/seats",
            format!(
                "a match has one to {MAX_SEATS} seats (spec section 3); this file has {}",
                seats.len()
            ),
        ));
    }
    for (index, seat) in seats.iter().enumerate() {
        let base = format!("/seats/{index}");
        let Json::Object(members) = seat else {
            problems.push(Problem::new(&base, "must be an object"));
            continue;
        };
        unknown_keys(&base, members, SEAT_KEYS, problems);

        match integer(seat.get("seat")) {
            Some(number) if number == i64::try_from(index).unwrap_or(-1) => {}
            Some(number) => problems.push(Problem::new(
                &format!("{base}/seat"),
                format!(
                    "seat ids run from 0 in array order so that ties break to the lowest seat id \
                     the same way everywhere; expected {index}, found {number}"
                ),
            )),
            None => problems.push(Problem::new(
                &format!("{base}/seat"),
                "required: the seat id, an integer",
            )),
        }

        let kind = seat.get("kind").and_then(Json::as_str).unwrap_or("");
        if !SEAT_KINDS.contains(&kind) {
            problems.push(Problem::new(
                &format!("{base}/kind"),
                format!("must be one of {}", SEAT_KINDS.join(", ")),
            ));
            continue;
        }
        if kind == "playbook" {
            match seat.get("playbook").and_then(Json::as_str) {
                Some(relative) => check_repository_path(
                    root,
                    &format!("{base}/playbook"),
                    relative,
                    true,
                    problems,
                ),
                None => problems.push(Problem::new(
                    &format!("{base}/playbook"),
                    "a `playbook` seat names the playbook it seals, as a path from the repository \
                     root",
                )),
            }
        } else if seat.get("playbook").is_some() {
            problems.push(Problem::new(
                &format!("{base}/playbook"),
                format!("a `{kind}` seat does not seal a file, so it takes no `playbook`"),
            ));
        }
    }
    seats.len()
}

fn check_segments(json: &Json, problems: &mut Vec<Problem>) {
    let Some(Json::Array(segments)) = json.get("segments") else {
        problems.push(Problem::new(
            "/segments",
            "required: an array of one or more segments",
        ));
        return;
    };
    if segments.is_empty() {
        problems.push(Problem::new(
            "/segments",
            "a scenario runs at least one segment",
        ));
    }
    for (index, segment) in segments.iter().enumerate() {
        let base = format!("/segments/{index}");
        let Json::Object(members) = segment else {
            problems.push(Problem::new(&base, "must be an object"));
            continue;
        };
        unknown_keys(&base, members, SEGMENT_KEYS, problems);

        match integer(segment.get("index")) {
            Some(number) if number == i64::try_from(index).unwrap_or(-1) => {}
            Some(number) => problems.push(Problem::new(
                &format!("{base}/index"),
                format!("segments run in order from 0; expected {index}, found {number}"),
            )),
            None => problems.push(Problem::new(
                &format!("{base}/index"),
                "required: the segment index, an integer counting from 0",
            )),
        }

        match integer(segment.get("length_ms")) {
            Some(number) if number > 0 && number <= i64::from(i32::MAX) => {}
            Some(number) => problems.push(Problem::new(
                &format!("{base}/length_ms"),
                format!(
                    "a segment length is a positive int32 count of game milliseconds \
                     (decisions-log item 46); found {number}"
                ),
            )),
            None => problems.push(Problem::new(
                &format!("{base}/length_ms"),
                "required: the segment's length in game milliseconds, a bare integer",
            )),
        }
    }
}

fn check_assertions(json: &Json, problems: &mut Vec<Problem>) -> usize {
    let Some(Json::Array(assertions)) = json.get("assertions") else {
        problems.push(Problem::new(
            "/assertions",
            "required: an array of assertions — a scenario that asserts nothing passes by not \
             checking, which is the failure the scenario runner exists to prevent",
        ));
        return 0;
    };
    if assertions.is_empty() {
        problems.push(Problem::new(
            "/assertions",
            "at least one assertion, on an event or on the hash chain",
        ));
    }

    let mut on_events = false;
    let mut on_hashes = false;

    for (index, assertion) in assertions.iter().enumerate() {
        let base = format!("/assertions/{index}");
        let Json::Object(_) = assertion else {
            problems.push(Problem::new(&base, "must be an object"));
            continue;
        };
        let kind = assertion.get("assert").and_then(Json::as_str).unwrap_or("");
        if RESERVED_ASSERTIONS.contains(&kind) {
            problems.push(Problem::new(
                &format!("{base}/assert"),
                format!(
                    "`{kind}` is reserved for the vocabulary extension decisions-log item 16 \
                     schedules for T15, when the runner meets real events; it is not in \
                     {FORMAT} yet"
                ),
            ));
            continue;
        }
        match kind {
            "event_fired" => {
                on_events = true;
                check_event_fired(assertion, &base, problems);
            }
            "hash_chain_equals" => {
                on_hashes = true;
                check_hash_chain_equals(assertion, &base, problems);
            }
            other => problems.push(Problem::new(
                &format!("{base}/assert"),
                format!(
                    "`{other}` is not in the assertion vocabulary; {FORMAT} has exactly {}",
                    ASSERTIONS.join(" and ")
                ),
            )),
        }
    }

    if !assertions.is_empty() && (!on_events || !on_hashes) {
        problems.push(Problem::new(
            "/assertions",
            "a scenario asserts on events AND on the hash chain (AGENTS.md section 10 item 4): \
             events alone prove the match did something, hashes alone prove it did the same thing \
             twice, and only the pair proves it did the right thing reproducibly",
        ));
    }
    assertions.len()
}

fn check_event_fired(assertion: &Json, base: &str, problems: &mut Vec<Problem>) {
    const KEYS: &[&str] = &["assert", "event", "seat", "by_tick", "note"];
    if let Json::Object(members) = assertion {
        unknown_keys(base, members, KEYS, problems);
    }
    if assertion.get("event").and_then(Json::as_str).is_none() {
        problems.push(Problem::new(
            &format!("{base}/event"),
            "required: the event name the runner waits for",
        ));
    }
    if let Some(seat) = assertion.get("seat") {
        if integer(Some(seat)).is_none_or(|number| number < 0) {
            problems.push(Problem::new(
                &format!("{base}/seat"),
                "optional, but when present it is a seat id: an integer of 0 or more",
            ));
        }
    }
    match assertion.get("by_tick").map(|value| integer(Some(value))) {
        None => problems.push(Problem::new(
            &format!("{base}/by_tick"),
            "required: the tick the event must have fired by. An assertion with no deadline is \
             satisfied by the end of the match, which is not an assertion",
        )),
        Some(Some(number)) if number >= 0 => {}
        Some(_) => problems.push(Problem::new(
            &format!("{base}/by_tick"),
            "must be a tick number of 0 or more",
        )),
    }
}

fn check_hash_chain_equals(assertion: &Json, base: &str, problems: &mut Vec<Problem>) {
    const KEYS: &[&str] = &["assert", "golden", "note"];
    if let Json::Object(members) = assertion {
        unknown_keys(base, members, KEYS, problems);
    }
    match assertion.get("golden").and_then(Json::as_str) {
        Some(path) => {
            if !path.starts_with("tests/golden/") {
                problems.push(Problem::new(
                    &format!("{base}/golden"),
                    "the committed hash chain lives under `tests/golden/`, where the `golden` \
                     step and `cargo xtask golden --bless` can see it",
                ));
            }
            if !path.ends_with(".hashes.txt") {
                problems.push(Problem::new(
                    &format!("{base}/golden"),
                    "a hash chain is named `expected.hashes.txt`: one `<tick>\\t<16 lowercase hex \
                     digits>` line per tick, LF endings, trailing newline",
                ));
            }
        }
        None => problems.push(Problem::new(
            &format!("{base}/golden"),
            "required: the committed hash chain this run must reproduce",
        )),
    }
}

/// A path written inside a scenario file: repository-relative, forward slashes,
/// never absolute and never escaping the tree. `must_exist` is false for
/// artefacts a later task produces.
fn check_repository_path(
    root: &Path,
    pointer: &str,
    value: &str,
    must_exist: bool,
    problems: &mut Vec<Problem>,
) {
    if value.starts_with('/') || value.contains('\\') || value.contains("..") {
        problems.push(Problem::new(
            pointer,
            "paths are relative to the repository root, use forward slashes, and never contain \
             `..` — a scenario names files in the tree and nothing outside it",
        ));
        return;
    }
    if must_exist && !root.join(value).is_file() {
        problems.push(Problem::new(
            pointer,
            format!("`{value}` does not exist in the repository"),
        ));
    }
}

fn unknown_keys(
    base: &str,
    members: &[(String, Json)],
    allowed: &[&str],
    problems: &mut Vec<Problem>,
) {
    for (key, _) in members {
        if !allowed.contains(&key.as_str()) {
            problems.push(Problem::new(
                &format!("{base}/{key}"),
                format!(
                    "unknown key. The format rejects what it does not recognise rather than \
                     stripping it — a silently dropped assertion is one that passes by not \
                     running. Known keys here: {}",
                    allowed.join(", ")
                ),
            ));
        }
    }
}

fn require_string(json: &Json, pointer: &str, key: &str, problems: &mut Vec<Problem>) -> String {
    match json.get(key).and_then(Json::as_str) {
        Some(value) if !value.is_empty() => value.to_owned(),
        _ => {
            problems.push(Problem::new(pointer, "required: a non-empty string"));
            String::new()
        }
    }
}

fn require_exact_string(
    json: &Json,
    pointer: &str,
    key: &str,
    expected: &str,
    problems: &mut Vec<Problem>,
) {
    match json.get(key).and_then(Json::as_str) {
        Some(value) if value == expected => {}
        Some(value) => problems.push(Problem::new(
            pointer,
            format!(
                "this runner reads `{expected}`; the file says `{value}`. A new format is a new \
                 string, so an old runner refuses a new file rather than half-understanding it"
            ),
        )),
        None => problems.push(Problem::new(
            pointer,
            format!("required: `{expected}`, the format tag, as the first key in the file"),
        )),
    }
}

/// A JSON number read as an integer. Numbers keep their source text in xtask's
/// reader precisely so that no float ever appears here.
fn integer(value: Option<&Json>) -> Option<i64> {
    match value {
        Some(Json::Num(text)) => text.parse::<i64>().ok(),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// JSONC
// ---------------------------------------------------------------------------

/// Replaces `//` and `/* */` comments with spaces, leaving string literals
/// alone. Spaces rather than deletion so that byte offsets in a JSON parse
/// error still point at the right place in the original file.
fn strip_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = *bytes.get(index).unwrap_or(&b' ');
        if in_string {
            out.push(char::from(byte));
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            out.push('"');
            index += 1;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while index < bytes.len() && bytes.get(index) != Some(&b'\n') {
                out.push(' ');
                index += 1;
            }
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let mut closed = false;
            while index < bytes.len() {
                let current = *bytes.get(index).unwrap_or(&b' ');
                if current == b'\n' {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
                if closed {
                    index += 1;
                    break;
                }
                if current == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    closed = true;
                }
                index += 1;
            }
            continue;
        }
        // Bytes at or above 0x80 are the middle of a multi-byte UTF-8 sequence
        // and never collide with the delimiters scanned for here, but they
        // cannot go through `char::from`, which would mean Latin-1. Copy the
        // whole character.
        if byte < 0x80 {
            out.push(char::from(byte));
            index += 1;
        } else {
            let rest = text.get(index..).unwrap_or("");
            match rest.chars().next() {
                Some(character) => {
                    out.push(character);
                    index += character.len_utf8();
                }
                None => index += 1,
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("pharmakos-xtask-tests")
            .join("scenario")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("scenarios")).expect("temp dir");
        fs::create_dir_all(dir.join("examples").join("playbooks")).expect("temp dir");
        fs::write(
            dir.join("examples").join("playbooks").join("p.jsonc"),
            "{}\n",
        )
        .expect("playbook fixture");
        dir
    }

    const GOOD: &str = r#"{
  "format": "pharmakos.scenario.v1",
  "name": "smoke",
  "map": { "seed": "0x0102030405060708", "generator": "skeleton" },
  "seats": [
    // one sealed playbook, one built-in operator
    { "seat": 0, "kind": "playbook", "playbook": "examples/playbooks/p.jsonc" },
    { "seat": 1, "kind": "safe" }
  ],
  "segments": [ { "index": 0, "length_ms": 180000 } ],
  "assertions": [
    { "assert": "event_fired", "event": "beacon_placed", "seat": 0, "by_tick": 2400 },
    { "assert": "hash_chain_equals", "golden": "tests/golden/scenarios/smoke/expected.hashes.txt" }
  ]
}
"#;

    fn write(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("scenarios").join("case.scenario.jsonc");
        fs::write(&path, body).expect("write scenario");
        path
    }

    #[test]
    fn a_well_formed_scenario_validates() {
        let dir = scratch("good");
        let path = write(&dir, GOOD);
        let scenario = validate(&dir, &path).expect("valid");
        assert_eq!(scenario.name, "smoke");
        assert_eq!(scenario.seats, 2);
        assert_eq!(scenario.assertions, 2);
    }

    #[test]
    fn comments_survive_the_parse() {
        let dir = scratch("comments");
        let body = GOOD.replace(
            "\"name\": \"smoke\",",
            "/* a block comment with a \"quoted\" word */ \"name\": \"smoke\", // and a line one\n",
        );
        let path = write(&dir, &body);
        validate(&dir, &path).expect("comments are stripped, not parsed");
    }

    #[test]
    fn a_comment_delimiter_inside_a_string_is_left_alone() {
        assert_eq!(
            strip_comments(r#"{"a":"http://x"} // gone"#),
            r#"{"a":"http://x"}        "#
        );
        assert_eq!(strip_comments(r#"{"a":"b\"// c"}"#), r#"{"a":"b\"// c"}"#);
    }

    #[test]
    fn an_unknown_key_is_rejected_rather_than_ignored() {
        let dir = scratch("unknown");
        let path = write(&dir, &GOOD.replace("\"name\":", "\"nmae\":"));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/nmae"), "{report}");
        assert!(report.contains("unknown key"), "{report}");
    }

    #[test]
    fn a_seed_that_a_double_would_round_is_rejected() {
        let dir = scratch("seed");
        let path = write(
            &dir,
            &GOOD.replace("\"0x0102030405060708\"", "72623859790382856"),
        );
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/map/seed"), "{report}");
    }

    #[test]
    fn a_negative_duration_is_rejected_with_a_pointer() {
        let dir = scratch("duration");
        let path = write(&dir, &GOOD.replace("180000", "-1"));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/segments/0/length_ms"), "{report}");
        assert!(report.contains("item 46"), "{report}");
    }

    #[test]
    fn a_scenario_must_assert_on_events_and_on_hashes() {
        let dir = scratch("events-only");
        let body = GOOD.replace(
            "    { \"assert\": \"hash_chain_equals\", \"golden\": \"tests/golden/scenarios/smoke/expected.hashes.txt\" }\n",
            "",
        );
        let body = body.replace("\"by_tick\": 2400 },", "\"by_tick\": 2400 }");
        let path = write(&dir, &body);
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("events AND on the hash chain"), "{report}");
    }

    #[test]
    fn a_reserved_assertion_names_the_task_that_adds_it() {
        let dir = scratch("reserved");
        let path = write(&dir, &GOOD.replace("\"event_fired\"", "\"terminal_hash\""));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("item 16"), "{report}");
        assert!(report.contains("T15"), "{report}");
    }

    #[test]
    fn a_missing_playbook_is_named() {
        let dir = scratch("missing-playbook");
        let path = write(&dir, &GOOD.replace("p.jsonc", "nowhere.jsonc"));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/seats/0/playbook"), "{report}");
        assert!(report.contains("does not exist"), "{report}");
    }

    #[test]
    fn carriage_returns_are_rejected() {
        let dir = scratch("crlf");
        let path = write(&dir, &GOOD.replace('\n', "\r\n"));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("carriage return"), "{report}");
    }

    #[test]
    fn out_of_order_seats_are_rejected() {
        let dir = scratch("seat-order");
        let path = write(&dir, &GOOD.replace("\"seat\": 0,", "\"seat\": 2,"));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/seats/0/seat"), "{report}");
        assert!(report.contains("lowest seat id"), "{report}");
    }

    #[test]
    fn the_committed_example_is_valid() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask sits under the workspace root")
            .to_path_buf();
        let files = collect(&root).expect("scenarios/ is readable");
        assert!(
            !files.is_empty(),
            "scenarios/ carries at least the committed example"
        );
        for file in &files {
            validate(&root, file).unwrap_or_else(|report| {
                panic!("the committed scenario does not satisfy its own format:\n{report}")
            });
        }
    }
}
