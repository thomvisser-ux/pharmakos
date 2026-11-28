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
//! so it cannot call into this crate; and this binary is run by hand and by a
//! nightly workflow, and `tests/scenarios.rs` plays every committed file (the
//! demo, `scenarios/skeleton/against-easy-three-rounds.scenario.jsonc`, among
//! them) through this library, none of which go through `xtask`.
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
//! One asymmetry, deliberate and in one direction only: this reader also
//! resolves an `event_fired` assertion's name against
//! `pharmakos_sim::events::EventKind`, which `xtask` cannot do because it has
//! no dependencies. Every file `xtask` refuses is refused here; a file with a
//! misspelled event name is refused here and not there. Strictly stricter is
//! the safe direction — the runner is the thing that decides whether a
//! scenario passed, and `xtask` shells the runner.
//!
//! # Every problem, with a JSON Pointer at it
//!
//! A scenario is written by hand, and five round trips to fix five typos is
//! five round trips. Every problem in a file is reported, each with an RFC 6901
//! pointer — the same convention the verifier's diagnostic catalogue uses, so a
//! scenario failure reads like every other failure in the project.

pub mod nightly;
pub mod run;
pub mod seeds;

use std::path::Path;

use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::MS_PER_TICK;

use crate::exit::Failure;
use crate::strings;

/// The value of the mandatory `format` key. A new format is a new string, so an
/// old runner refuses a new file rather than half-understanding it.
pub const FORMAT: &str = "pharmakos.scenario.v1";

/// The suffix every scenario file carries.
pub const SUFFIX: &str = ".scenario.jsonc";

/// The rules table a scenario runs against when it does not name one.
pub const DEFAULT_RULES: &str = "rules/rules.v1.json";

/// Where the nightly adversarial arms live, as path components from the
/// repository root (S2's plan, E20).
///
/// An arm is an ordinary scenario file with two differences, both read from
/// where it lives rather than from a key, because the format gains no key for
/// the nightlies: it carries **no `hash_chain_equals`**, since a nightly run
/// replaces its map seed and no arm keeps a chain of its own; and it seats
/// exactly one `playbook` seat, the arm, and one `builtin` seat, Easy, which a
/// run over a seed file alternates between the two seats ([`nightly`]).
/// `xtask/src/scenario.rs` holds the same rule, and its `collect_played` leaves
/// these files out of every replay against a golden.
pub const NIGHTLY_DIR: [&str; 2] = ["scenarios", "nightly"];

/// The assertion vocabulary, in full.
///
/// **Ruled: none of item 97's three is taken now** (the register's M-05,
/// ruled by S1's plan's decision 9, decisions-log item 128). The history,
/// kept because it is the reason: **decisions-log item 97** (which is plan §7
/// decision 16, taken on the recommendation on 2026-09-14 — an earlier draft
/// of this comment called it "recommended, not yet logged", and a review was
/// right that it is logged) authorises T15 to extend this list **once**,
/// "when `scenario run` meets real
/// events", by *at most* `event_count_in_range`, `state_hash_at_tick` and
/// `terminal_hash`. "At most" is the word that matters: taking none of the
/// three is inside the decision, and that is what T15 did.
///
/// Why none. `hash_chain_equals` already pins every tick's hash, so
/// `terminal_hash` and `state_hash_at_tick` assert a subset of what is already
/// asserted. A count range is the one of the three that asserts something new,
/// and neither committed scenario needs it: `deploy-and-visit`'s claim that all
/// four route steps complete is carried by `visit_ended` on the last step,
/// every step having `on_fail: ABORT_ROUTE` — so a step that failed would have
/// stopped the route before that event. Taking the extension anyway would move
/// the vocabulary in `xtask/src/scenario.rs` and `scenarios/README.md`, both
/// contract paths (AGENTS.md §5), to buy assertions nothing needs.
///
/// The owner confirmed that taking none is the right reading of item 97's "by
/// at most" (item 128). The three stay reserved, and naming one is an error
/// that says so; a later stage that needs one spends a format change on it.
pub const ASSERTIONS: &[&str] = &["event_fired", "hash_chain_equals"];

/// Names held for decisions-log item 97's extension.
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
    "round_limit",
    "segment_lengths_ms",
    "seats",
    "segments",
    "assertions",
];
const MAP_KEYS: &[&str] = &["seed", "generator"];

/// The longest segment a scenario may play, in game milliseconds: spec §3's
/// "the world then runs for up to 8 minutes". The format's cap (the register's
/// X-01, taken with S1-41 in S1's first contract pull request), and
/// `xtask/src/scenario.rs`'s `MAX_SEGMENT_MS` holds the same number, so the two
/// readers refuse the same files.
pub const MAX_SEGMENT_MS: i32 = 480_000;
const SEAT_KEYS: &[&str] = &["seat", "kind", "playbook", "sealed"];
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
    /// It seals what a match sealed, round by round: the named folder, as a
    /// path from the repository root, holds `<round>.jsonc` for every round
    /// the scenario plays, the names and the bytes a match's own
    /// `seats/<seat>/sealed/` folder holds ([`sealed_file`]). Each goes
    /// through `submit_plan`, the door a seat's own playbook is sealed
    /// through (a filed safe playbook goes through it here for the first
    /// time), and a refusal fails the run: the harness door is never taken
    /// for it, because a
    /// replay that seals what no seat could have submitted replays nothing
    /// (decisions-log item 232).
    Sealed(String),
    /// It seals nothing, so the gateway files a safe playbook for it at the
    /// end of the Lull (spec §14). A scenario run advises no seat, so what is
    /// filed is the gateway's fallback — the library's Safe Playbook with
    /// nothing raised — and never a safe playbook the operator made for it.
    ///
    /// PLACEHOLDER: a `safe` seat files the gateway's fallback — owner, S5, with
    /// the `operator` scenario key, rather than an operator-made safe playbook.
    Safe,
    /// It plays the built-in operator: Easy, seated by the runner through
    /// `serve::InProcessSeats` as `gamectl host` seats it, which plans, submits
    /// and says ready in every Lull (T18b; decisions-log item 113 (8)).
    ///
    /// PLACEHOLDER: a `builtin` seat plays Easy — owner, S5, with a second
    /// difficulty. Easy is the one difficulty the skeleton ships, and the
    /// scenario format has no `operator` key to name another; the key waits
    /// for one.
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
    /// The round the match ends on: the `round_limit` key, or the sim's
    /// `DEFAULT_ROUND_LIMIT`. Never fewer than the segments.
    pub round_limit: u32,
    /// The match's segment length list, the lobby's setting
    /// (`MatchSettings::segment_lengths_ms`), never empty: the
    /// `segment_lengths_ms` key, or for a file that names none the lengths of
    /// its own segments, as every match took before the key existed; the
    /// reader resolves which. The list is hashed with the match, so a file
    /// that replays a match played somewhere else (a `sealed` seat's) states
    /// that match's list, which may be longer than the rounds the file plays.
    /// Every segment's length is its round's in this list: its own entry, and
    /// past the end the last one.
    pub segment_lengths_ms: Vec<i32>,
    /// One to three seats, in seat order.
    pub seats: Vec<Seat>,
    /// One or more segments, in order.
    pub segments: Vec<Segment>,
    /// At least one assertion, and at least one of each kind -- except for a
    /// nightly arm, which asserts on events alone ([`NIGHTLY_DIR`]).
    pub assertions: Vec<Assertion>,
    /// Whether the file is a nightly arm, under [`NIGHTLY_DIR`].
    pub nightly: bool,
}

impl Scenario {
    /// How many ticks the whole scenario plays, at 20 Hz.
    ///
    /// # Errors
    ///
    /// [`crate::exit::Exit::Input`] for a file whose segments add up to more
    /// ticks than a `u32` counts: each is capped at [`MAX_SEGMENT_MS`], and
    /// the format does not cap how many a file lists, so the sum is refused
    /// rather than saturated.
    pub fn ticks(&self) -> Result<u32, Failure> {
        self.segments.iter().try_fold(0_u32, |total, segment| {
            total
                .checked_add(ticks_of(segment.length_ms))
                .ok_or_else(|| Failure::input(strings::scenario_too_long(&self.source)))
        })
    }
}

/// Ticks in a segment of `length_ms` game milliseconds, rounded down.
///
/// The sim's own conversion ([`Ms::to_ticks_floor`]), so the rounding is
/// written once in the tree rather than a second time here: an earlier
/// version divided with `checked_div` and fell back to zero twice over, a
/// silent fallback S2's sweep (E7) removed. A length is validated positive
/// before it gets here.
///
/// [`Ms::to_ticks_floor`]: pharmakos_sim::math::quantity::Ms::to_ticks_floor
#[must_use]
pub fn ticks_of(length_ms: i32) -> u32 {
    pharmakos_sim::math::quantity::Ms::new(length_ms).to_ticks_floor()
}

/// The file a `sealed` seat seals in `round` (counted from 1), as a path from
/// the repository root: `<folder>/<round>.jsonc`, the name the gateway's
/// match cache gives a seat's sealed playbook of that round.
#[must_use]
pub fn sealed_file(folder: &str, round: u32) -> String {
    format!("{folder}/{round}.jsonc")
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
                    error.pointer().to_owned(),
                    format!("not valid JSONC: {}", error.message()),
                )],
            ))
        })?;

    let nightly = is_nightly(root, source);
    let scenario = read(root, &display, nightly, &json, &mut problems);
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

/// Whether `source` names a file under [`NIGHTLY_DIR`], read from the
/// repository root. A path outside the root (a test's scratch copy) is not an
/// arm.
fn is_nightly(root: &Path, source: &Path) -> bool {
    let relative = if source.is_absolute() {
        match source.strip_prefix(root) {
            Ok(relative) => relative,
            Err(_) => return false,
        }
    } else {
        source
    };
    let mut parts = relative
        .components()
        .filter(|part| !matches!(part, std::path::Component::CurDir));
    NIGHTLY_DIR
        .iter()
        .all(|wanted| parts.next().is_some_and(|part| part.as_os_str() == *wanted))
}

fn read(
    root: &Path,
    source: &str,
    nightly: bool,
    json: &Json,
    problems: &mut Problems,
) -> Option<Scenario> {
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
                "must be exactly `{FORMAT}`, and is `{}`: a new format is a new string, so \
                 an old runner refuses a new file rather than half-understanding it",
                pharmakos_proto::json::grammar::escaped(&found)
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
    if nightly {
        check_arm_seats(&seats, problems);
    }
    let segments = read_segments(json, problems);
    check_sealed_rounds(root, &seats, segments.len(), problems);
    let round_limit = read_match_settings(json, segments.len(), problems);
    let segment_lengths_ms = read_ladder(json, &segments, problems);
    let assertions = read_assertions(root, json, nightly, problems);

    Some(Scenario {
        source: source.to_owned(),
        name,
        summary,
        seed: seed?,
        generator: generator?,
        rules,
        round_limit,
        segment_lengths_ms: segment_lengths_ms?,
        seats,
        segments,
        assertions,
        nightly,
    })
}

/// The match's segment length list, or `None` with the problem said.
///
/// The optional `segment_lengths_ms` key holds one to
/// [`pharmakos_sim::rules::MAX_SEGMENT_LENGTHS`] lengths, each a segment's
/// length by the rule a segment's own is held to, and every segment the file
/// plays the length its round takes from the list: its own entry, and past
/// the end the last one, the sim's rule
/// (`MatchSettings::segment_lengths_ms`). A file that names no list plays a
/// match whose list is the lengths of its own segments.
fn read_ladder(json: &Json, segments: &[Segment], problems: &mut Problems) -> Option<Vec<i32>> {
    let Some(value) = json.get("segment_lengths_ms") else {
        return Some(segments.iter().map(|segment| segment.length_ms).collect());
    };
    let most = pharmakos_sim::rules::MAX_SEGMENT_LENGTHS;
    let count = |problems: &mut Problems, held: usize| {
        problems.at(
            "/segment_lengths_ms",
            format!(
                "a match's segment length list holds 1 to {most} lengths; this one holds {held}"
            ),
        );
    };
    let Json::Array(entries) = value else {
        problems.at(
            "/segment_lengths_ms",
            format!(
                "optional, but when present it is the match's segment length list: an array of \
                 1 to {most} lengths in game milliseconds"
            ),
        );
        return None;
    };
    if entries.len() > most {
        count(problems, entries.len());
        return None;
    }
    let mut ladder: Vec<i32> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        match integer_as::<i32>(Some(entry)) {
            Some(Ok(length)) if (MS_PER_TICK..=MAX_SEGMENT_MS).contains(&length) => {
                ladder.push(length);
            }
            _ => problems.at(
                &format!("/segment_lengths_ms/{index}"),
                format!(
                    "a segment length is a positive count of game milliseconds, at least one \
                     tick ({MS_PER_TICK}) and at most {MAX_SEGMENT_MS}, spec §3's eight minutes"
                ),
            ),
        }
    }
    if ladder.len() != entries.len() {
        return None;
    }
    // An empty list has no last length for the rounds past its end.
    let Some(last) = ladder.last().copied() else {
        count(problems, 0);
        return None;
    };
    for segment in segments {
        let wanted = match ladder.get(segment.index) {
            Some(own) => *own,
            None => last,
        };
        if wanted != segment.length_ms {
            problems.at(
                &format!("/segments/{}/length_ms", segment.index),
                format!(
                    "`segment_lengths_ms` gives this round {wanted} game milliseconds, and the \
                     segment says {}; a segment plays the length its round has in the match",
                    segment.length_ms
                ),
            );
        }
    }
    Some(ladder)
}

/// A nightly arm seats one `playbook` seat and one `builtin` seat, in either
/// order (E20; decision 29).
fn check_arm_seats(seats: &[Seat], problems: &mut Problems) {
    let playbooks = seats
        .iter()
        .filter(|seat| matches!(seat.kind, SeatKind::Playbook(_)))
        .count();
    let builtin = seats
        .iter()
        .filter(|seat| seat.kind == SeatKind::Builtin)
        .count();
    if seats.len() != 2 || playbooks != 1 || builtin != 1 {
        problems.at(
            "/seats",
            "a nightly arm seats exactly two seats: one `playbook` seat, the arm, and one \
             `builtin` seat, Easy, which a run over a seed file alternates between seat 0 and \
             seat 1 (S2's plan, decision 29)",
        );
    }
}

/// A `sealed` seat's folder holds a file for every round the scenario plays.
///
/// The first round without one is named, once a seat: the format does not cap
/// how many segments a file lists, and one line says what is missing as well
/// as a thousand do.
fn check_sealed_rounds(root: &Path, seats: &[Seat], segments: usize, problems: &mut Problems) {
    for seat in seats {
        let SeatKind::Sealed(folder) = &seat.kind else {
            continue;
        };
        if leaves_the_repository(folder) || !root.join(folder).is_dir() {
            // Already said, at the same pointer, by `read_kind`, and a path
            // that leaves the tree is never probed.
            continue;
        }
        let missing = (1..=segments)
            .filter_map(|round| u32::try_from(round).ok())
            .map(|round| (round, sealed_file(folder, round)))
            .find(|(_, file)| !root.join(file).is_file());
        if let Some((round, file)) = missing {
            problems.at(
                &format!("/seats/{}/sealed", seat.seat),
                format!(
                    "`{}` is not a file in this repository: a `sealed` seat seals \
                     `<round>.jsonc` in every round, and this scenario plays round {round}",
                    pharmakos_proto::json::grammar::escaped(&file)
                ),
            );
        }
    }
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
        match integer(seat.get("seat")) {
            Some(Ok(number)) if usize::try_from(number).is_ok_and(|number| number == index) => {}
            Some(Ok(number)) => problems.at(
                &format!("{base}/seat"),
                format!(
                    "seat ids run from 0 in array order so that ties break to the lowest seat id \
                     the same way everywhere; expected {index}, found {number}"
                ),
            ),
            Some(Err(())) => problems.at(
                &format!("{base}/seat"),
                "a seat id is a small integer, and this number is not one any integer type here \
                 can hold",
            ),
            None => problems.at(&format!("{base}/seat"), "required: the seat id, an integer"),
        }

        let Some(kind) = read_kind(root, &base, seat, problems) else {
            continue;
        };
        // A file of more seats than a `u8` numbers has already been told it
        // has too many; the seat past the range is not read at all rather
        // than read as seat 0.
        let Ok(seat) = u8::try_from(index) else {
            continue;
        };
        read.push(Seat { seat, kind });
    }
    read
}

/// What the seat at `base` plays, or `None` with the problem said.
fn read_kind(root: &Path, base: &str, seat: &Json, problems: &mut Problems) -> Option<SeatKind> {
    const KINDS: &[&str] = &["playbook", "sealed", "safe", "builtin"];
    let kind = match seat.get("kind") {
        Some(Json::String(kind)) if KINDS.contains(&kind.as_str()) => kind.as_str(),
        Some(Json::String(other)) => {
            problems.at(
                &format!("{base}/kind"),
                format!(
                    "`{}` must be one of playbook, sealed, safe, builtin",
                    pharmakos_proto::json::grammar::escaped(other)
                ),
            );
            return None;
        }
        _ => {
            problems.at(
                &format!("{base}/kind"),
                "must be one of playbook, sealed, safe, builtin",
            );
            return None;
        }
    };
    // The key that names what a kind seals belongs to that kind alone.
    for key in ["playbook", "sealed"] {
        if kind == key || seat.get(key).is_none() {
            continue;
        }
        let why = match kind {
            "playbook" => "seals the one file its `playbook` names",
            "sealed" => "seals the files in the folder its `sealed` names",
            _ => "does not seal a file",
        };
        problems.at(
            &format!("{base}/{key}"),
            format!("a `{kind}` seat {why}, so it takes no `{key}`"),
        );
    }
    match kind {
        "sealed" => {
            let Some(Json::String(relative)) = seat.get("sealed") else {
                problems.at(
                    &format!("{base}/sealed"),
                    "a `sealed` seat names the folder of playbooks a match sealed, \
                     `<round>.jsonc` for each round, as a path from the repository root",
                );
                return None;
            };
            check_folder(root, &format!("{base}/sealed"), relative, problems);
            Some(SeatKind::Sealed(relative.clone()))
        }
        "playbook" => {
            let Some(Json::String(relative)) = seat.get("playbook") else {
                problems.at(
                    &format!("{base}/playbook"),
                    "a `playbook` seat names the playbook it seals, as a path from the \
                     repository root",
                );
                return None;
            };
            check_path(root, &format!("{base}/playbook"), relative, true, problems);
            Some(SeatKind::Playbook(relative.clone()))
        }
        "safe" => Some(SeatKind::Safe),
        _ => Some(SeatKind::Builtin),
    }
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
        match integer(segment.get("index")) {
            Some(Ok(number)) if usize::try_from(number).is_ok_and(|number| number == index) => {}
            Some(Ok(number)) => problems.at(
                &format!("{base}/index"),
                format!("segments run in order from 0; expected {index}, found {number}"),
            ),
            Some(Err(())) => problems.at(
                &format!("{base}/index"),
                "a segment index counts from 0, and this number is not one any integer type here \
                 can hold",
            ),
            None => problems.at(
                &format!("{base}/index"),
                "required: the segment index, an integer counting from 0",
            ),
        }
        match integer_as::<i32>(segment.get("length_ms")) {
            Some(Ok(length)) if (MS_PER_TICK..=MAX_SEGMENT_MS).contains(&length) => {
                read.push(Segment {
                    index,
                    length_ms: length,
                });
            }
            Some(Ok(length)) if length > MAX_SEGMENT_MS => problems.at(
                &format!("{base}/length_ms"),
                format!(
                    "a segment plays at most {MAX_SEGMENT_MS} game milliseconds, spec §3's eight \
                     minutes; found {length}"
                ),
            ),
            // The gateway opens no match on a segment under a tick
            // (`Host::check_settings`), so the file is refused here, with its
            // pointer, rather than the run on a match that would not open.
            Some(Ok(length)) if length > 0 => problems.at(
                &format!("{base}/length_ms"),
                format!(
                    "a segment plays at least one tick, {MS_PER_TICK} game milliseconds: no \
                     match opens on a shorter one; found {length}"
                ),
            ),
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

/// `round_limit`, one of the two match settings a scenario may name (the
/// register's S1-41, taken in S1's first contract pull request; the other is
/// `segment_lengths_ms`, [`read_ladder`]): optional, and absent what every
/// chain committed before the key existed was produced with, so adding it moved
/// no chain. Its sibling `units_per_seat`, the harness walkers, went with them
/// (S2-16, S2's plan decision 23): a file that still names it is refused as an
/// unknown key.
fn read_match_settings(json: &Json, segments: usize, problems: &mut Problems) -> u32 {
    let default_limit = pharmakos_sim::runner::DEFAULT_ROUND_LIMIT;
    let round_limit = match json.get("round_limit") {
        None => Some(default_limit),
        Some(value) => match integer_as::<u32>(Some(value)) {
            Some(Ok(limit)) if limit >= 1 => Some(limit),
            _ => {
                problems.at(
                    "/round_limit",
                    "optional, but when present it is the round the match ends on: an integer of \
                     1 or more",
                );
                None
            }
        },
    };
    // The limit the match plays to, named or the default, never falls below
    // the file's segment count: a scenario plays no round past its limit.
    if let Some(limit) =
        round_limit.filter(|limit| usize::try_from(*limit).is_ok_and(|limit| limit < segments))
    {
        problems.at(
            "/round_limit",
            format!(
                "the match ends on round {limit}, and the file plays {segments} segments; a \
                 scenario plays no round past its limit (absent, the limit is the sim's default \
                 of {default_limit})"
            ),
        );
    }
    round_limit.unwrap_or(default_limit)
}

fn read_assertions(
    root: &Path,
    json: &Json,
    nightly: bool,
    problems: &mut Problems,
) -> Vec<Assertion> {
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
                    "`{}` is reserved for the vocabulary extension decisions-log item 97 \
                     authorises (plan §7 decision 16); T15 took none of the three, and the owner \
                     confirmed none is taken now (decisions-log item 128). It is not in {FORMAT}.",
                    pharmakos_proto::json::grammar::escaped(kind)
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
            "hash_chain_equals" if nightly => problems.at(
                &format!("{base}/assert"),
                "a nightly arm carries no hash chain: a run over a seed file replaces its map \
                 seed, so a chain for the file's own seed would pin a match the nightly never \
                 plays (S2's plan, E20)",
            ),
            "hash_chain_equals" => {
                unknown_keys(&base, members, HASH_CHAIN_KEYS, problems);
                on_hashes = true;
                if let Some(chain) = read_hash_chain(root, assertion, &base, problems) {
                    read.push(chain);
                }
            }
            other => problems.at(
                &format!("{base}/assert"),
                format!(
                    "`{}` is not in the assertion vocabulary; {FORMAT} has exactly {}",
                    pharmakos_proto::json::grammar::escaped(other),
                    ASSERTIONS.join(" and ")
                ),
            ),
        }
    }

    if nightly && !assertions.is_empty() && !on_events {
        problems.at(
            "/assertions",
            "a nightly arm asserts on events: its run on its own seed checks them, and it carries \
             no hash chain (S2's plan, E20)",
        );
    }
    if !nightly && !assertions.is_empty() && (!on_events || !on_hashes) {
        problems.at(
            "/assertions",
            "a scenario asserts on events AND on the hash chain (AGENTS.md §10 item 4): events \
             alone prove the match did something, hashes alone prove it did the same thing twice, \
             and only the pair proves it did the right thing reproducibly",
        );
    }
    read
}

fn read_hash_chain(
    root: &Path,
    assertion: &Json,
    base: &str,
    problems: &mut Problems,
) -> Option<Assertion> {
    if let Some(Json::String(golden)) = assertion.get("golden") {
        // The chain need not exist yet: the task that first
        // drives the run commits it. Everything else about the
        // path is checked, because
        // `tests/golden/../../../etc/x.hashes.txt` satisfies
        // both the prefix and the suffix.
        check_path(root, &format!("{base}/golden"), golden, false, problems);
        // `tests/golden/scenarios/` and not merely `tests/golden/`,
        // which is where a review found this. The runner writes the
        // run's fresh chain to `<target>/golden/<area>/actual.…`
        // (`crate::actual_for`), so a scenario naming
        // `tests/golden/determinism/expected.hashes.txt` would
        // overwrite the determinism area's fresh output in the
        // shared target directory — the file
        // `crates/sim/tests/determinism.rs` writes and compares in
        // the same `cargo test` run. Nothing committed does it; a
        // scenario written next year would.
        //
        // Stricter than `xtask/src/scenario.rs`'s reader, in the
        // same direction as the `event_fired` name resolution above
        // and for the same reason: this is the thing that decides
        // whether a scenario passed, and `xtask` shells it.
        if !golden.starts_with("tests/golden/scenarios/") {
            problems.at(
                &format!("{base}/golden"),
                "a scenario's committed chain lives under \
                     `tests/golden/scenarios/<name>/`, where the `golden` step and \
                     `cargo xtask golden --bless` can see it and where it cannot \
                     collide with another area's fresh output",
            );
        }
        if !golden.ends_with(".hashes.txt") {
            problems.at(
                &format!("{base}/golden"),
                "a hash chain is named `expected.hashes.txt`, so that the format \
                     is readable from the name",
            );
        }
        Some(Assertion::HashChainEquals {
            golden: golden.clone(),
        })
    } else {
        problems.at(
            &format!("{base}/golden"),
            "required: the committed chain, as a path from the repository root",
        );
        None
    }
}

fn read_event_fired(assertion: &Json, base: &str, problems: &mut Problems) -> Option<Assertion> {
    let Some(Json::String(event)) = assertion.get("event") else {
        problems.at(
            &format!("{base}/event"),
            "required: the event name the runner waits for",
        );
        return None;
    };
    // Resolved against the sim's own vocabulary, here, rather than compared as
    // a string per event later. `EventKind::from_name` exists for exactly this
    // and its doc says so; a review found nothing calling it. Without this, a
    // typo such as `beacon_place` validates, plays a whole match and then exits
    // 3 with "never fired" — which is this module's own contract
    // ("every problem, with a JSON Pointer at it") not met for one field.
    if pharmakos_sim::events::EventKind::from_name(event).is_none() {
        let known: Vec<&str> = pharmakos_sim::events::EventKind::ALL
            .iter()
            .map(|kind| kind.name())
            .collect();
        problems.at(
            &format!("{base}/event"),
            format!(
                "`{}` is not an event this build's sim reports. A misspelled name would \
                 otherwise play the whole match and then fail for never having fired, which is \
                 a typo reported as a behaviour change. Known: {}",
                pharmakos_proto::json::grammar::escaped(event),
                known.join(", ")
            ),
        );
        return None;
    }
    let event = event.clone();
    let seat = if let Some(value) = assertion.get("seat") {
        let Some(Ok(seat)) = integer_as::<u8>(Some(value)) else {
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
    let by_tick = match integer_as::<u32>(assertion.get("by_tick")) {
        Some(Ok(tick)) => tick,
        Some(Err(())) => {
            problems.at(
                &format!("{base}/by_tick"),
                "must be a tick number of 0 or more, and small enough to be one: a number that \
                 does not fit is a deadline this match could not reach",
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

/// A JSON number as an integer.
///
/// `None` means "no number is there at all"; `Some(Err(…))` means "a number is
/// there and it is not one this format can use". A review found the two
/// conflated: `"by_tick": 99999999999999999999` is present and overflows
/// `i64`, and the reader answered "required", which is a pointer at the right
/// key under the wrong sentence in the module whose stated contract is every
/// problem with a pointer at it.
fn integer(value: Option<&Json>) -> Option<Result<i64, ()>> {
    match value {
        Some(Json::Number(lexeme)) => Some(lexeme.parse::<i64>().map_err(|_| ())),
        _ => None,
    }
}

/// [`integer`], narrowed to the type the field is written in.
///
/// The same three answers: `None` for "not there", `Some(Err(()))` for "there
/// and out of range", `Some(Ok(_))` for a usable number.
fn integer_as<T: TryFrom<i64>>(value: Option<&Json>) -> Option<Result<T, ()>> {
    integer(value).map(|number| number.and_then(|number| T::try_from(number).map_err(|_| ())))
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
    if leaves_the_repository(relative) {
        problems.at(pointer, strings::path_escapes(relative));
        return;
    }
    if must_exist && !root.join(relative).is_file() {
        problems.at(
            pointer,
            format!(
                "`{}` is not a file in this repository",
                pharmakos_proto::json::grammar::escaped(relative)
            ),
        );
    }
}

/// Whether `relative` is anything but a path from the repository root that
/// stays inside it, spelled the one way the format takes.
///
/// An empty part is a leading slash, a trailing one or a doubled one, and the
/// empty path; a `:` is a Windows drive, which `Path::join` would follow out
/// of the tree, or a stream name. `xtask/src/scenario.rs` holds the same
/// function, xtask being std-only, and both crates' tests run their copy over
/// `tests/corpus/scenario-paths.txt` (decisions-log item 232 (7)).
fn leaves_the_repository(relative: &str) -> bool {
    relative.contains('\\')
        || relative.contains(':')
        || relative.contains("..")
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == ".")
}

/// [`check_path`] for a folder that must exist.
fn check_folder(root: &Path, pointer: &str, relative: &str, problems: &mut Problems) {
    if leaves_the_repository(relative) {
        problems.at(pointer, strings::path_escapes(relative));
        return;
    }
    if !root.join(relative).is_dir() {
        problems.at(
            pointer,
            format!(
                "`{}` is not a folder in this repository",
                pharmakos_proto::json::grammar::escaped(relative)
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ASSERTIONS, Json, MAX_SEGMENT_MS, Problems, RESERVED_ASSERTIONS, TOP_LEVEL_KEYS,
        leaves_the_repository, parse_seed, read_match_settings, read_segments, ticks_of,
    };

    /// The path rule's shared cases: `xtask`'s reader runs the same file
    /// through its own copy of the rule, so the two cannot part unseen
    /// (decisions-log item 232 (7)).
    #[test]
    fn the_path_rule_answers_the_shared_cases() {
        let text = include_str!("../tests/corpus/scenario-paths.txt");
        let mut accepted = 0_u32;
        let mut refused = 0_u32;
        for line in text.lines().filter(|line| !line.starts_with('#')) {
            let (verdict, path) = line.split_once(' ').unwrap_or((line, ""));
            assert!(
                matches!(verdict, "accept" | "refuse"),
                "`{line}` is neither an `accept` nor a `refuse` case"
            );
            let refuse = verdict == "refuse";
            assert_eq!(leaves_the_repository(path), refuse, "`{path}`");
            if refuse {
                refused += 1;
            } else {
                accepted += 1;
            }
        }
        assert!(
            accepted >= 5 && refused >= 10,
            "{accepted} accepted, {refused} refused"
        );
    }

    fn object(text: &str) -> Json {
        pharmakos_proto::json::read(text).expect("test JSON")
    }

    /// E22: a file's segments that add up past a `u32` of ticks are an input
    /// error naming the file, never a saturated total.
    #[test]
    fn a_scenario_s_ticks_past_a_u32_are_refused() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let source = root.join("scenarios/skeleton/expand-east-segment.scenario.jsonc");
        let mut scenario = super::load(root, &source).expect("a committed scenario");
        assert!(scenario.ticks().is_ok_and(|ticks| ticks > 0));
        let longest = super::Segment {
            index: 0,
            length_ms: MAX_SEGMENT_MS,
        };
        let per = ticks_of(MAX_SEGMENT_MS);
        // One more segment than a u32 of ticks holds at this length.
        let past = u32::MAX
            .checked_div(per)
            .and_then(|whole| whole.checked_add(1))
            .and_then(|count| usize::try_from(count).ok())
            .expect("a count that fits a usize");
        scenario.segments = vec![longest; past];
        let refused = scenario.ticks().expect_err("past a u32");
        assert_eq!(refused.code, crate::exit::Exit::Input);
        assert!(
            refused.message.contains("expand-east-segment"),
            "{}",
            refused.message
        );
    }

    fn pointers(problems: &Problems) -> Vec<&str> {
        problems
            .0
            .iter()
            .map(|problem| problem.pointer.as_str())
            .collect()
    }

    /// The register's S1-41: the round limit is optional, and absent it is
    /// what every committed chain was produced with.
    #[test]
    fn the_match_settings_default_to_what_every_chain_was_produced_with() {
        let mut problems = Problems(Vec::new());
        assert_eq!(
            read_match_settings(&object("{}"), 3, &mut problems),
            pharmakos_sim::runner::DEFAULT_ROUND_LIMIT
        );
        assert!(problems.0.is_empty());

        let mut problems = Problems(Vec::new());
        assert_eq!(
            read_match_settings(&object(r#"{"round_limit": 3}"#), 3, &mut problems),
            3
        );
        assert!(problems.0.is_empty());
    }

    /// S2-16 (S2's plan, decision 23): the walkers' key left the format with
    /// the walkers, so a file that still names it is refused as an unknown
    /// key rather than read.
    #[test]
    fn the_walkers_key_is_no_longer_part_of_the_format() {
        assert!(!TOP_LEVEL_KEYS.contains(&"units_per_seat"));
    }

    #[test]
    fn a_bad_match_setting_is_refused_with_a_pointer() {
        for (text, pointer) in [
            (r#"{"round_limit": 0}"#, "/round_limit"),
            (r#"{"round_limit": "3"}"#, "/round_limit"),
            (r#"{"round_limit": 2}"#, "/round_limit"), // below the 3 segments
        ] {
            let mut problems = Problems(Vec::new());
            let _ = read_match_settings(&object(text), 3, &mut problems);
            assert_eq!(pointers(&problems), vec![pointer], "{text}");
        }
    }

    /// The default round limit holds a file to its segment count as a named
    /// one does: seven segments and no `round_limit` would play past round 6.
    #[test]
    fn the_default_round_limit_is_checked_against_the_segment_count() {
        let default = usize::try_from(pharmakos_sim::runner::DEFAULT_ROUND_LIMIT).unwrap();
        let mut problems = Problems(Vec::new());
        let _ = read_match_settings(&object("{}"), default, &mut problems);
        assert!(problems.0.is_empty());
        let _ = read_match_settings(&object("{}"), default + 1, &mut problems);
        assert_eq!(pointers(&problems), vec!["/round_limit"]);
    }

    /// The register's X-01: the format caps a segment at spec §3's eight
    /// minutes, where the runner used to cap only its reservation.
    #[test]
    fn a_segment_past_eight_minutes_is_refused_with_a_pointer() {
        assert_eq!(MAX_SEGMENT_MS, 480_000);
        let mut problems = Problems(Vec::new());
        let read = read_segments(
            &object(r#"{"segments": [{"index": 0, "length_ms": 480000}]}"#),
            &mut problems,
        );
        assert_eq!(read.len(), 1);
        assert!(problems.0.is_empty());

        let mut problems = Problems(Vec::new());
        let read = read_segments(
            &object(r#"{"segments": [{"index": 0, "length_ms": 480001}]}"#),
            &mut problems,
        );
        assert!(read.is_empty());
        assert_eq!(pointers(&problems), vec!["/segments/0/length_ms"]);
    }

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
