// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scenario file, read.
//!
//! **`scenarios/README.md` is the specification and is the file to read first.**
//! This module is the runner's half of it, and `xtask/src/scenario.rs` is the
//! harness's: `cargo xtask ci`'s `scenario` step validates every committed file
//! against the format *before* shelling this binary, so by the time the runner
//! sees a file it has already been told it is well formed.
//!
//! # Why the rules are checked twice, on purpose
//!
//! The obvious tidy-up is for one of the two to trust the other. Neither can:
//! `xtask` is dev-only, std-only and has no dependencies at all (AGENTS.md §3),
//! so it cannot call into this crate; and this binary is run by hand, by a
//! nightly workflow and by T20's demo set, none of which go through `xtask`.
//! A runner that trusted its input would answer a malformed scenario with a
//! panic or, worse, with a pass — and a scenario that passes by not running is
//! the single failure the whole harness exists to prevent
//! (`tests/golden/README.md` rule 1).
//!
//! So both read the same prose and both refuse the same files, and
//! `tests/scenarios.rs` pins the agreement where it matters: every committed
//! scenario parses here, and the doctored copies it writes are refused here for
//! the reason `xtask` would give.
//!
//! # Every problem, with a JSON Pointer at it
//!
//! A scenario is written by hand, and five round trips to fix five typos is
//! five round trips. Every problem in a file is reported, each with an RFC 6901
//! pointer — the same convention the verifier's diagnostic catalogue uses, so a
//! scenario failure reads like every other failure in the project.

pub mod run;

use std::path::Path;

use pharmakos_proto::json::Json;

use crate::exit::Failure;
use crate::strings;

/// The value of the mandatory `format` key. A new format is a new string, so an
/// old runner refuses a new file rather than half-understanding it.
pub const FORMAT: &str = "pharmakos.scenario.v1";

/// The suffix every scenario file carries.
pub const SUFFIX: &str = ".scenario.jsonc";

/// The rules table a scenario runs against when it does not name one.
pub const DEFAULT_RULES: &str = "rules/rules.v1.json";

/// The assertion vocabulary, in full.
///
/// PLACEHOLDER — and this one is a decision, not an omission. Skeleton plan §7
/// decision 16 (recommended, not yet logged) schedules **one** extension of
/// this list for T15, "when the runner meets real events", naming
/// `event_count_in_range`, `state_hash_at_tick` and `terminal_hash`. T15 met
/// the real events and did not take it: `hash_chain_equals` already pins every
/// tick's hash, so `terminal_hash` and `state_hash_at_tick` assert a subset of
/// what is already asserted, and no scenario this task writes wants a count
/// range that `event_fired` does not cover. Taking it anyway would move the
/// vocabulary in `xtask/src/scenario.rs` and `scenarios/README.md`, both
/// contract paths (AGENTS.md §5), to buy assertions nothing needs. **The owner
/// decides** whether to leave the three names reserved or to spend the contract
/// change; the pull request puts the question with the measurement beside it.
/// Until then the three stay reserved and naming one is an error that says so.
pub const ASSERTIONS: &[&str] = &["event_fired", "hash_chain_equals"];

/// Names held for decision 16's extension.
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
    "rules",
    "seats",
    "segments",
    "assertions",
];
const MAP_KEYS: &[&str] = &["seed", "generator"];
const SEAT_KEYS: &[&str] = &["seat", "kind", "playbook"];
const SEGMENT_KEYS: &[&str] = &["index", "length_ms", "note"];
const EVENT_FIRED_KEYS: &[&str] = &["assert", "event", "seat", "by_tick", "note"];
const HASH_CHAIN_KEYS: &[&str] = &["assert", "golden", "note"];

/// At most three seats share a local match (spec §3; AGENTS.md §1).
const MAX_SEATS: usize = 3;

/// What a seat plays.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SeatKind {
    /// It seals the named file, as a path from the repository root.
    Playbook(String),
    /// It files the safe playbook — the one the operator files on a timeout.
    Safe,
    /// It plays the built-in operator. **T18**; this build has none.
    Builtin,
}

/// One seat of the match.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Seat {
    /// The seat id, which is also its index in the list.
    pub seat: u8,
    /// What it plays.
    pub kind: SeatKind,
}

/// One segment of the match.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Segment {
    /// Its index, counting from 0.
    pub index: usize,
    /// Its length in game milliseconds (decisions-log item 46).
    pub length_ms: i32,
}

/// One thing the scenario claims about the run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Assertion {
    /// That event was on the bus at or before that tick.
    EventFired {
        /// The event kind's name, as the sim's own bus spells it.
        event: String,
        /// Whose it must be, when it must be anybody's.
        seat: Option<u8>,
        /// The deadline. Required: an assertion with no deadline is satisfied
        /// by the end of the match, which is not an assertion.
        by_tick: u32,
    },
    /// The run's per-tick xxh3 chain equals the committed file, byte for byte.
    HashChainEquals {
        /// The committed chain, as a path from the repository root.
        golden: String,
    },
}

/// A headless match, written down.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scenario {
    /// The file it came from, as it was named on the command line.
    pub source: String,
    /// The `name` key, which is also its golden directory.
    pub name: String,
    /// The one-sentence `summary`, or empty.
    pub summary: String,
    /// The 64-bit map seed.
    pub seed: u64,
    /// The generator's name.
    pub generator: String,
    /// The rules table, as a path from the repository root.
    pub rules: String,
    /// One to three seats, in seat order.
    pub seats: Vec<Seat>,
    /// One or more segments, in order.
    pub segments: Vec<Segment>,
    /// At least one assertion, and at least one of each kind.
    pub assertions: Vec<Assertion>,
}

impl Scenario {
    /// How many ticks the whole scenario plays, at 20 Hz.
    #[must_use]
    pub fn ticks(&self) -> u32 {
        self.segments
            .iter()
            .map(|segment| ticks_of(segment.length_ms))
            .fold(0_u32, u32::saturating_add)
    }
}

/// Ticks in a segment of `length_ms` game milliseconds.
///
/// `checked_div` rather than `/`: `integer_division` is denied workspace-wide
/// and this crate is not a walled one, so the rounding is written down. A
/// length is validated positive before it gets here, and the tick length is a
/// sim constant that is never zero, so the `unwrap_or` is unreachable rather
/// than a default anybody relies on.
#[must_use]
pub fn ticks_of(length_ms: i32) -> u32 {
    let per_tick = pharmakos_sim::math::quantity::MS_PER_TICK;
    u32::try_from(length_ms.checked_div(per_tick).unwrap_or(0)).unwrap_or(0)
}

/// One problem with one place in the file.
struct Problem {
    pointer: String,
    message: String,
}

/// Collects problems, so a file is reported on once rather than typo by typo.
struct Problems(Vec<Problem>);

impl Problems {
    fn at(&mut self, pointer: &str, message: impl Into<String>) {
        self.0.push(Problem {
            pointer: pointer.to_owned(),
            message: message.into(),
        });
    }
}

/// Read and validate one scenario file.
///
/// `root` is the repository root every path inside the file is resolved
/// against; `source` is the path as it was named on the command line, used in
/// messages.
///
/// # Errors
///
/// [`crate::exit::Exit::Input`] when the file cannot be read or is not a valid
/// scenario. A malformed scenario is an input error rather than a failed
/// assertion: nothing was played, so nothing held or did not hold.
pub fn load(root: &Path, source: &Path) -> Result<Scenario, Failure> {
    let display = crate::display(source);
    let bytes = std::fs::read(root.join(source)).map_err(|error| {
        Failure::input(strings::unreadable(
            "scenario",
            &display,
            &error.to_string(),
        ))
    })?;

    let mut problems = Problems(Vec::new());
    check_bytes(&bytes, &mut problems);

    let text = String::from_utf8(bytes).map_err(|error| {
        Failure::input(strings::unreadable(
            "scenario",
            &display,
            &error.to_string(),
        ))
    })?;
    // The comments come off through `plan-core`'s JSONC reader, which is the
    // same one a playbook goes through: one implementation of "what is a
    // comment" in the tree, not two.
    let json = pharmakos_plan_core::Document::parse(&text)
        .map(|document| document.to_json())
        .map_err(|error| {
            Failure::input(strings::scenario_invalid(
                &display,
                &[(
                    error.pointer.clone(),
                    format!("not valid JSONC: {}", error.message),
                )],
            ))
        })?;

    let scenario = read(root, &display, &json, &mut problems);
    if problems.0.is_empty() {
        return scenario.ok_or_else(|| {
            Failure::internal("a scenario with no problems did not come back read")
        });
    }
    let listed: Vec<(String, String)> = problems
        .0
        .into_iter()
        .map(|problem| (problem.pointer, problem.message))
        .collect();
    Err(Failure::input(strings::scenario_invalid(&display, &listed)))
}

fn check_bytes(bytes: &[u8], problems: &mut Problems) {
    if bytes.contains(&b'\r') {
        problems.at(
            "",
            "the file contains a carriage return; scenario files and their goldens are \
             byte-compared across Windows, Linux and macOS, so endings are LF",
        );
    }
    if bytes.contains(&b'\t') {
        problems.at(
            "",
            "the file contains a tab; indentation is two spaces so a diff lines up in every \
             viewer",
        );
    }
    if bytes.last() != Some(&b'\n') {
        problems.at("", "the file must end with a newline");
    }
}

fn read(root: &Path, source: &str, json: &Json, problems: &mut Problems) -> Option<Scenario> {
    let Json::Object(members) = json else {
        problems.at("", "a scenario file is a JSON object");
        return None;
    };
    unknown_keys("", members, TOP_LEVEL_KEYS, problems);

    match string_at(json, "format", "/format", problems) {
        Some(found) if found == FORMAT => {}
        Some(found) => problems.at(
            "/format",
            format!(
                "must be exactly `{FORMAT}`, and is `{found}`: a new format is a new string, so \
                 an old runner refuses a new file rather than half-understanding it"
            ),
        ),
        None => {}
    }
    let name = string_at(json, "name", "/name", problems).unwrap_or_default();
    let summary = match json.get("summary") {
        Some(Json::String(text)) => text.clone(),
        Some(_) => {
            problems.at("/summary", "must be a string");
            String::new()
        }
        None => String::new(),
    };
    let (seed, generator) = read_map(json, problems);
    let rules = read_rules(root, json, problems);
    let seats = read_seats(root, json, problems);
    let segments = read_segments(json, problems);
    let assertions = read_assertions(root, json, problems);

    Some(Scenario {
        source: source.to_owned(),
        name,
        summary,
        seed: seed?,
        generator: generator?,
        rules,
        seats,
        segments,
        assertions,
    })
}

fn read_map(json: &Json, problems: &mut Problems) -> (Option<u64>, Option<String>) {
    let Some(map) = json.get("map") else {
        problems.at("/map", "required: the map the scenario runs on");
        return (None, None);
    };
    let Json::Object(members) = map else {
        problems.at("/map", "must be an object");
        return (None, None);
    };
    unknown_keys("/map", members, MAP_KEYS, problems);

    let seed = match map.get("seed") {
        Some(Json::String(text)) => parse_seed(text),
        _ => None,
    };
    if seed.is_none() {
        problems.at(
            "/map/seed",
            "must be `0x` followed by exactly 16 lowercase hex digits — the 64-bit map seed is a \
             quoted string because a JSON number loses its low bits in a double-backed parser, \
             and a map that differs between two readers of the same file is not a scenario",
        );
    }
    let generator = string_at(map, "generator", "/map/generator", problems);
    (seed, generator)
}

/// The seed, or `None` when it is not spelled exactly as the format says.
fn parse_seed(text: &str) -> Option<u64> {
    let digits = text.strip_prefix("0x")?;
    if digits.len() != 16
        || !digits
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

fn read_rules(root: &Path, json: &Json, problems: &mut Problems) -> String {
    let Some(value) = json.get("rules") else {
        check_path(root, "/rules", DEFAULT_RULES, true, problems);
        return DEFAULT_RULES.to_owned();
    };
    let Json::String(relative) = value else {
        problems.at(
            "/rules",
            "must be a string: the repository-relative path of the rules table this scenario \
             runs against",
        );
        return DEFAULT_RULES.to_owned();
    };
    check_path(root, "/rules", relative, true, problems);
    relative.clone()
}

fn read_seats(root: &Path, json: &Json, problems: &mut Problems) -> Vec<Seat> {
    let Some(Json::Array(seats)) = json.get("seats") else {
        problems.at("/seats", "required: an array of one to three seats");
        return Vec::new();
    };
    if seats.is_empty() || seats.len() > MAX_SEATS {
        problems.at(
            "/seats",
            format!(
                "a match has one to {MAX_SEATS} seats (spec §3); this file has {}",
                seats.len()
            ),
        );
    }
    let mut read: Vec<Seat> = Vec::with_capacity(seats.len());
    for (index, seat) in seats.iter().enumerate() {
        let base = format!("/seats/{index}");
        let Json::Object(members) = seat else {
            problems.at(&base, "must be an object");
            continue;
        };
        unknown_keys(&base, members, SEAT_KEYS, problems);
        let wanted = i64::try_from(index).unwrap_or(-1);
        match integer(seat.get("seat")) {
            Some(number) if number == wanted => {}
            Some(number) => problems.at(
                &format!("{base}/seat"),
                format!(
                    "seat ids run from 0 in array order so that ties break to the lowest seat id \
                     the same way everywhere; expected {index}, found {number}"
                ),
            ),
            None => problems.at(&format!("{base}/seat"), "required: the seat id, an integer"),
        }

        let Some(Json::String(kind)) = seat.get("kind") else {
            problems.at(
                &format!("{base}/kind"),
                "must be one of playbook, safe, builtin",
            );
            continue;
        };
        let kind = match kind.as_str() {
            "playbook" => {
                let Some(Json::String(relative)) = seat.get("playbook") else {
                    problems.at(
                        &format!("{base}/playbook"),
                        "a `playbook` seat names the playbook it seals, as a path from the \
                         repository root",
                    );
                    continue;
                };
                check_path(root, &format!("{base}/playbook"), relative, true, problems);
                SeatKind::Playbook(relative.clone())
            }
            other @ ("safe" | "builtin") => {
                if seat.get("playbook").is_some() {
                    problems.at(
                        &format!("{base}/playbook"),
                        format!("a `{other}` seat does not seal a file, so it takes no `playbook`"),
                    );
                }
                if other == "safe" {
                    SeatKind::Safe
                } else {
                    SeatKind::Builtin
                }
            }
            other => {
                problems.at(
                    &format!("{base}/kind"),
                    format!("`{other}` must be one of playbook, safe, builtin"),
                );
                continue;
            }
        };
        read.push(Seat {
            seat: u8::try_from(index).unwrap_or(0),
            kind,
        });
    }
    read
}

fn read_segments(json: &Json, problems: &mut Problems) -> Vec<Segment> {
    let Some(Json::Array(segments)) = json.get("segments") else {
        problems.at("/segments", "required: an array of one or more segments");
        return Vec::new();
    };
    if segments.is_empty() {
        problems.at("/segments", "a scenario runs at least one segment");
    }
    let mut read: Vec<Segment> = Vec::with_capacity(segments.len());
    for (index, segment) in segments.iter().enumerate() {
        let base = format!("/segments/{index}");
        let Json::Object(members) = segment else {
            problems.at(&base, "must be an object");
            continue;
        };
        unknown_keys(&base, members, SEGMENT_KEYS, problems);
        let wanted = i64::try_from(index).unwrap_or(-1);
        match integer(segment.get("index")) {
            Some(number) if number == wanted => {}
            Some(number) => problems.at(
                &format!("{base}/index"),
                format!("segments run in order from 0; expected {index}, found {number}"),
            ),
            None => problems.at(
                &format!("{base}/index"),
                "required: the segment index, an integer counting from 0",
            ),
        }
        match integer(segment.get("length_ms")).map(i32::try_from) {
            Some(Ok(length)) if length > 0 => read.push(Segment {
                index,
                length_ms: length,
            }),
            Some(_) => problems.at(
                &format!("{base}/length_ms"),
                "a segment length is a positive int32 count of game milliseconds \
                 (decisions-log item 46)",
            ),
            None => problems.at(
                &format!("{base}/length_ms"),
                "required: the segment's length in game milliseconds, a bare integer",
            ),
        }
    }
    read
}

fn read_assertions(root: &Path, json: &Json, problems: &mut Problems) -> Vec<Assertion> {
    let Some(Json::Array(assertions)) = json.get("assertions") else {
        problems.at(
            "/assertions",
            "required: an array of assertions — a scenario that asserts nothing passes by not \
             checking, which is the failure the scenario runner exists to prevent",
        );
        return Vec::new();
    };
    if assertions.is_empty() {
        problems.at(
            "/assertions",
            "at least one assertion, on an event or on the hash chain",
        );
    }

    let mut read: Vec<Assertion> = Vec::with_capacity(assertions.len());
    let mut on_events = false;
    let mut on_hashes = false;
    for (index, assertion) in assertions.iter().enumerate() {
        let base = format!("/assertions/{index}");
        let Json::Object(members) = assertion else {
            problems.at(&base, "must be an object");
            continue;
        };
        let Some(Json::String(kind)) = assertion.get("assert") else {
            problems.at(
                &format!("{base}/assert"),
                format!("required: one of {}", ASSERTIONS.join(" and ")),
            );
            continue;
        };
        if RESERVED_ASSERTIONS.contains(&kind.as_str()) {
            problems.at(
                &format!("{base}/assert"),
                format!(
                    "`{kind}` is reserved for the vocabulary extension skeleton-plan §7 decision \
                     16 schedules; T15 did not take it and the owner has the question (see \
                     crates/gamectl/src/scenario.rs). It is not in {FORMAT}."
                ),
            );
            continue;
        }
        match kind.as_str() {
            "event_fired" => {
                unknown_keys(&base, members, EVENT_FIRED_KEYS, problems);
                on_events = true;
                if let Some(fired) = read_event_fired(assertion, &base, problems) {
                    read.push(fired);
                }
            }
            "hash_chain_equals" => {
                unknown_keys(&base, members, HASH_CHAIN_KEYS, problems);
                on_hashes = true;
                if let Some(Json::String(golden)) = assertion.get("golden") {
                    // The chain need not exist yet: the task that first
                    // drives the run commits it. Everything else about the
                    // path is checked, because
                    // `tests/golden/../../../etc/x.hashes.txt` satisfies
                    // both the prefix and the suffix.
                    check_path(root, &format!("{base}/golden"), golden, false, problems);
                    if !golden.starts_with("tests/golden/") {
                        problems.at(
                            &format!("{base}/golden"),
                            "the committed hash chain lives under `tests/golden/`, where the \
                                 `golden` step and `cargo xtask golden --bless` can see it",
                        );
                    }
                    if !golden.ends_with(".hashes.txt") {
                        problems.at(
                            &format!("{base}/golden"),
                            "a hash chain is named `expected.hashes.txt`, so that the format \
                                 is readable from the name",
                        );
                    }
                    read.push(Assertion::HashChainEquals {
                        golden: golden.clone(),
                    });
                } else {
                    problems.at(
                        &format!("{base}/golden"),
                        "required: the committed chain, as a path from the repository root",
                    );
                }
            }
            other => problems.at(
                &format!("{base}/assert"),
                format!(
                    "`{other}` is not in the assertion vocabulary; {FORMAT} has exactly {}",
                    ASSERTIONS.join(" and ")
                ),
            ),
        }
    }

    if !assertions.is_empty() && (!on_events || !on_hashes) {
        problems.at(
            "/assertions",
            "a scenario asserts on events AND on the hash chain (AGENTS.md §10 item 4): events \
             alone prove the match did something, hashes alone prove it did the same thing twice, \
             and only the pair proves it did the right thing reproducibly",
        );
    }
    read
}

fn read_event_fired(assertion: &Json, base: &str, problems: &mut Problems) -> Option<Assertion> {
    let Some(Json::String(event)) = assertion.get("event") else {
        problems.at(
            &format!("{base}/event"),
            "required: the event name the runner waits for",
        );
        return None;
    };
    let event = event.clone();
    let seat = if let Some(value) = assertion.get("seat") {
        let Some(Ok(seat)) = integer(Some(value)).map(u8::try_from) else {
            problems.at(
                &format!("{base}/seat"),
                "optional, but when present it is a seat id: an integer of 0 or more",
            );
            return None;
        };
        Some(seat)
    } else {
        None
    };
    let by_tick = match integer(assertion.get("by_tick")).map(u32::try_from) {
        Some(Ok(tick)) => tick,
        Some(Err(_)) => {
            problems.at(
                &format!("{base}/by_tick"),
                "must be a tick number of 0 or more",
            );
            return None;
        }
        None => {
            problems.at(
                &format!("{base}/by_tick"),
                "required: the tick the event must have fired by. An assertion with no deadline \
                 is satisfied by the end of the match, which is not an assertion",
            );
            return None;
        }
    };
    Some(Assertion::EventFired {
        event,
        seat,
        by_tick,
    })
}

// ---------------------------------------------------------------------------
// Shared little rules
// ---------------------------------------------------------------------------

fn unknown_keys(base: &str, members: &[(String, Json)], known: &[&str], problems: &mut Problems) {
    for (key, _) in members {
        if !known.contains(&key.as_str()) {
            problems.at(
                &format!("{base}/{key}"),
                format!(
                    "unknown key. The format rejects one rather than ignoring it, exactly as Load \
                     rejects an out-of-vocabulary construct in a playbook: a silently stripped \
                     assertion is an assertion that passes by not running. Known here: {}",
                    known.join(", ")
                ),
            );
        }
    }
}

fn string_at(json: &Json, key: &str, pointer: &str, problems: &mut Problems) -> Option<String> {
    match json.get(key) {
        Some(Json::String(text)) if !text.is_empty() => Some(text.clone()),
        _ => {
            problems.at(pointer, format!("required: `{key}`, a non-empty string"));
            None
        }
    }
}

/// A JSON number as an integer, or `None` when it is not one.
fn integer(value: Option<&Json>) -> Option<i64> {
    match value {
        Some(Json::Number(lexeme)) => lexeme.parse::<i64>().ok(),
        _ => None,
    }
}

/// The path rule every path in a scenario follows: from the repository root,
/// forward slashes, no `..`, and no way out of the tree.
fn check_path(
    root: &Path,
    pointer: &str,
    relative: &str,
    must_exist: bool,
    problems: &mut Problems,
) {
    let bad = relative.is_empty()
        || relative.contains('\\')
        || relative.starts_with('/')
        || relative.split('/').any(|part| part == ".." || part.is_empty())
        // A Windows drive letter is an absolute path that does not start with
        // a slash, so the check above does not see it.
        || relative.as_bytes().get(1) == Some(&b':');
    if bad {
        problems.at(pointer, strings::path_escapes(pointer, relative));
        return;
    }
    if must_exist && !root.join(relative).is_file() {
        problems.at(
            pointer,
            format!("`{relative}` is not a file in this repository"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{ASSERTIONS, RESERVED_ASSERTIONS, parse_seed, ticks_of};

    #[test]
    fn the_seed_is_sixteen_lowercase_hex_digits_and_nothing_else() {
        assert_eq!(
            parse_seed("0x00000000ca5caded"),
            Some(0x0000_0000_ca5c_aded)
        );
        for wrong in [
            "0x00000000CA5CADED", // uppercase
            "0xca5caded",         // fifteen too few
            "00000000ca5caded",   // no prefix
            "0x00000000ca5cadedd",
            "0xzzzzzzzzzzzzzzzz",
        ] {
            assert_eq!(parse_seed(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn a_segment_is_a_whole_number_of_ticks() {
        assert_eq!(ticks_of(180_000), 3_600, "20 Hz x 180 s");
        assert_eq!(ticks_of(1_000), 20);
        // Rounding is written down rather than left to `/`: a length that is
        // not a whole number of ticks plays the ticks that fit.
        assert_eq!(ticks_of(1_049), 20);
    }

    #[test]
    fn the_reserved_names_are_not_in_the_vocabulary() {
        for reserved in RESERVED_ASSERTIONS {
            assert!(
                !ASSERTIONS.contains(reserved),
                "`{reserved}` is both reserved and in the vocabulary"
            );
        }
    }
}
