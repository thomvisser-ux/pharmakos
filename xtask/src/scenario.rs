// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scenario file format, and the checks `cargo xtask ci`'s `scenario` step
//! makes before it plays a file.
//!
//! A scenario is a headless match written down: **a map seed, the rules table,
//! each seat's playbooks, the segment list, and assertions on events *and* on
//! the hash chain** (AGENTS.md section 9 item 10, section 10 item 4). The first
//! three are exactly AGENTS.md section 4's triple — "the sim is a pure function
//! of (map seed, playbooks, rules hash)" — which is what makes a committed hash
//! chain a claim about the sim rather than about whatever was on disk that
//! afternoon.
//!
//! `gamectl scenario run` executes one (T15), and `cargo xtask ci`'s
//! `scenario` step validates every file here before it plays each with that
//! runner. This module is the format, frozen first, so that every later task
//! delivered *into* a format rather than inventing one — which is the whole
//! reason harness part 2's formats were pulled into wave 1 (decisions-log item
//! 75). gamectl's own validator (`crates/gamectl/src/scenario.rs`) reads the
//! same format and is stricter in two places `scenarios/README.md` names.
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

/// Where the nightly adversarial arms live, as path components from the
/// workspace root (S2's plan, task `night`, and its E20).
///
/// An arm is an ordinary scenario file read by two rules of its own, taken
/// from where it lives because the format gains no key for the nightlies: it
/// carries **no `hash_chain_equals`** (a nightly run replaces its map seed, so
/// a chain for the file's own seed would pin a match the nightly never plays),
/// and it seats exactly one `playbook` seat and one `builtin` seat. The
/// `scenario` step validates an arm and does not play it, and the package's
/// replay never replays one: both take [`collect_played`]. `gamectl`'s reader
/// holds the same two rules (`crates/gamectl/src/scenario.rs`, `NIGHTLY_DIR`).
pub(crate) const NIGHTLY_DIR: [&str; 2] = ["scenarios", "nightly"];

/// Where the arena's match-ups live, as path components from the workspace
/// root (decisions-log item 233). A match-up is a scenario file whose seats
/// `gamectl arena` plays against each other over a seed file: like an arm it
/// carries **no `hash_chain_equals`**, and it seats two or three, none of
/// them `sealed`. It is validated here and played against no golden. The
/// runner holds the same rule (`crates/gamectl/src/scenario.rs`,
/// `ARENA_DIR`).
pub(crate) const ARENA_DIR: [&str; 2] = ["scenarios", "arena"];

/// What a scenario file is, read from where it lives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Home {
    /// A committed scenario, played against its golden.
    Scenario,
    /// A nightly arm, under [`NIGHTLY_DIR`].
    Arm,
    /// An arena match-up, under [`ARENA_DIR`].
    MatchUp,
}

/// The value of the mandatory `format` key. A new format is a new string, so an
/// old runner refuses a new file rather than half-understanding it.
pub(crate) const FORMAT: &str = "pharmakos.scenario.v1";

/// The assertion vocabulary, in full.
///
/// Decisions-log item 97 (skeleton-plan section 7 decision 16, taken
/// 2026-09-14) authorised one extension "when `scenario run` meets real
/// events", by *at most* `event_count_in_range`, `state_hash_at_tick` and
/// `terminal_hash`; T15 took none of the three, and the owner confirmed that
/// none is taken now (the register's M-05, ruled by S1's plan's decision 9,
/// decisions-log item 128; `crates/gamectl/src/scenario.rs` says why). The
/// vocabulary is data inside the format, not the format, so adding to it is
/// not a format break; removing one would be.
pub(crate) const ASSERTIONS: &[&str] = &["event_fired", "hash_chain_equals"];

/// Names held for decisions-log item 97's extension. Naming one today is an
/// error that names the decision holding it, rather than "unknown assertion",
/// in the words gamectl's own validator uses.
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

/// The most lengths a match's segment length list holds: the sim's
/// `MAX_SEGMENT_LENGTHS` (`crates/sim/src/rules.rs`). xtask is std-only and
/// cannot name it, so `crates/gamectl/tests/parity.rs` pins this copy to it,
/// and `gamectl`'s reader, which names the sim's, refuses the same files.
pub(crate) const MAX_SEGMENT_LENGTHS: usize = 8;

/// The shortest segment a match opens on: one tick, the sim's `MS_PER_TICK`
/// (`crates/sim/src/math/quantity.rs`). The gateway opens no match on a
/// shorter length, so both readers refuse one in the file, with its pointer,
/// rather than leave the run to fail on a match that would not open.
/// `crates/gamectl/tests/parity.rs` pins this copy to the sim's as well.
pub(crate) const MS_PER_TICK: i64 = 50;

/// The longest segment a scenario may play, in game milliseconds: spec
/// section 3's "the world then runs for up to 8 minutes", the top of the 3 / 5
/// / 8 ladder.
///
/// The cap is the format's rather than the runner's (the register's X-01,
/// taken with S1-41 in S1's first contract pull request): before it, a segment
/// was any positive `int32`, so two segments of `i32::MAX` asked the runner
/// for gigabytes of hash chain before the first tick, and the runner capped
/// its reservation instead of the file refusing the length with a pointer.
/// `gamectl`'s reader holds the same number, so the two readers keep refusing
/// the same files.
pub(crate) const MAX_SEGMENT_MS: i64 = 480_000;

/// The round a match ends on when a scenario names none: the sim's
/// `DEFAULT_ROUND_LIMIT` (spec section 3's six rounds). xtask is std-only and
/// cannot read the sim's constant, so `crates/gamectl/tests/parity.rs` pins
/// this copy to it.
pub(crate) const DEFAULT_ROUND_LIMIT: i64 = 6;

/// The rules table a scenario runs against when it does not name one.
///
/// `rules/rules.v1.json` is the canonical JSON of one `gp.v1.RulesTable`
/// (decisions-log item 78), loaded by `pharmakos-sim` and hashed into
/// `rules_hash`. Its *shape* is a contract file and its *values* are data that
/// ordinary tuning PRs move during S1 and S2 — which is precisely why a
/// scenario has to name it. A tuning PR moves every committed hash chain, and
/// the chain has to be able to say which table produced it.
pub(crate) const DEFAULT_RULES: &str = "rules/rules.v1.json";
const MAP_KEYS: &[&str] = &["seed", "generator"];
/// No `operator` key. A `builtin` seat will one day name the difficulty it runs
/// at, but nothing validates that today and `scenarios/README.md`'s key table
/// does not document it — and a key that is accepted, undocumented and unchecked
/// is the same failure as a silently stripped one in miniature: a scenario could
/// carry `"operator": "whatever"` and have it neither honoured nor refused.
/// A `builtin` seat plays Easy, and the key that would name another difficulty
/// waits for one: owner, at S5 (decisions-log item 113 (8)). Adding a key is
/// additive, so that is not a format break.
const SEAT_KEYS: &[&str] = &["seat", "kind", "playbook", "sealed"];
const SEGMENT_KEYS: &[&str] = &["index", "length_ms", "note"];
const SEAT_KINDS: &[&str] = &["playbook", "sealed", "safe", "builtin"];

/// At most three seats share a local match: one human and up to two built-in
/// operators (spec section 3; AGENTS.md section 1).
const MAX_SEATS: usize = 3;

/// What a valid scenario file told us, for the step's summary line and for the
/// runner's command line.
#[derive(Debug)]
pub(crate) struct Scenario {
    /// The file it came from, relative to the workspace root.
    pub(crate) relative: PathBuf,
    /// The `name` key.
    pub(crate) name: String,
    /// The rules table this scenario runs against, relative to the workspace
    /// root: the `rules` key when it names one, [`DEFAULT_RULES`] otherwise.
    /// The runner reads the same key from the file; the step names it in its
    /// summary line when it is not the default.
    pub(crate) rules: String,
    /// Seat count.
    pub(crate) seats: usize,
    /// Assertion count.
    pub(crate) assertions: usize,
    /// What it is. A nightly arm and an arena match-up are validated and
    /// never played against a golden.
    pub(crate) home: Home,
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
    files.retain(|path| crate::file_name(path).is_some_and(|name| name.ends_with(SUFFIX)));
    Ok(files)
}

/// Every scenario file a run plays against its committed golden: [`collect`]
/// less the nightly arms under [`NIGHTLY_DIR`] and the arena's match-ups
/// under [`ARENA_DIR`], which carry no chain (E20; decisions-log item 233).
/// The `scenario` step plays these, and `cargo xtask package`'s replay
/// replays these, so neither plays an arm or a match-up as though it were a
/// committed scenario.
pub(crate) fn collect_played(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = collect(root)?;
    files.retain(|path| match path.strip_prefix(root) {
        Ok(relative) => home_of(relative) == Home::Scenario,
        Err(_) => true,
    });
    Ok(files)
}

/// What the file at a path from the workspace root is.
pub(crate) fn home_of(relative: &Path) -> Home {
    let under = |folder: [&str; 2]| {
        let mut parts = relative
            .components()
            .filter(|part| !matches!(part, std::path::Component::CurDir));
        folder
            .iter()
            .all(|wanted| parts.next().is_some_and(|part| part.as_os_str() == *wanted))
    };
    if under(NIGHTLY_DIR) {
        Home::Arm
    } else if under(ARENA_DIR) {
        Home::MatchUp
    } else {
        Home::Scenario
    }
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

    let home = home_of(&relative);
    let mut name = String::new();
    let mut rules = DEFAULT_RULES.to_owned();
    let mut seats = 0;
    let mut assertions = 0;

    if let Json::Object(members) = &json {
        unknown_keys("", members, TOP_LEVEL_KEYS, &mut problems);
        require_exact_string(&json, "/format", "format", FORMAT, &mut problems);
        name = require_string(&json, "/name", "name", &mut problems);
        check_map(&json, &mut problems);
        rules = check_rules(root, &json, &mut problems);
        seats = check_seats(root, &json, &mut problems);
        match home {
            Home::Scenario => {}
            Home::Arm => check_arm_seats(&json, &mut problems),
            Home::MatchUp => check_match_up_seats(&json, &mut problems),
        }
        let segments = check_segments(&json, &mut problems);
        check_sealed_rounds(root, &json, segments, &mut problems);
        check_match_settings(&json, segments, &mut problems);
        check_ladder(&json, &mut problems);
        assertions = check_assertions(root, &json, home, &mut problems);
    } else {
        problems.push(Problem::new("", "a scenario file is a JSON object"));
    }

    if problems.is_empty() {
        return Ok(Scenario {
            relative,
            name,
            rules,
            seats,
            assertions,
            home,
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

/// The third leg of the determinism triple.
///
/// AGENTS.md section 4 says the sim is a pure function of (map seed, playbooks,
/// rules hash). `map.seed` and `seats[].playbook` name the first two; without
/// this key the third is whatever happens to be checked out, so a hash chain
/// committed in one PR and compared in another would be comparing two different
/// claims and calling the difference a desync.
///
/// Optional, defaulting to [`DEFAULT_RULES`] — almost every scenario runs
/// against the shipped table, and making every file repeat the same path would
/// be noise that stops being read. The default is checked for existence too: a
/// scenario that silently ran against a table that is not there is the quiet
/// pass this harness exists to prevent.
fn check_rules(root: &Path, json: &Json, problems: &mut Vec<Problem>) -> String {
    let Some(value) = json.get("rules") else {
        if !root.join(DEFAULT_RULES).is_file() {
            problems.push(Problem::new(
                "/rules",
                format!(
                    "no `rules` key, and the default `{DEFAULT_RULES}` is not in the repository \
                     — name the table this scenario runs against"
                ),
            ));
        }
        return DEFAULT_RULES.to_owned();
    };
    let Some(relative) = value.as_str() else {
        problems.push(Problem::new(
            "/rules",
            "must be a string: the repository-relative path of the rules table this scenario \
             runs against",
        ));
        return DEFAULT_RULES.to_owned();
    };
    check_repository_path(root, "/rules", relative, true, problems);
    relative.to_owned()
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
            let well_formed = seed.strip_prefix("0x").is_some_and(|digits| {
                digits.len() == 16
                    && digits
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            });
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
            Some(number) if usize::try_from(number) == Ok(index) => {}
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

        // A missing or non-string kind is reported at `/kind` like an unknown
        // one.
        let Some(kind) = seat
            .get("kind")
            .and_then(Json::as_str)
            .filter(|kind| SEAT_KINDS.contains(kind))
        else {
            problems.push(Problem::new(
                &format!("{base}/kind"),
                format!("must be one of {}", SEAT_KINDS.join(", ")),
            ));
            continue;
        };
        // The key that names what a kind seals belongs to that kind alone.
        for (key, owner) in [("playbook", "playbook"), ("sealed", "sealed")] {
            if kind == owner || seat.get(key).is_none() {
                continue;
            }
            let why = match kind {
                "playbook" => "seals the one file its `playbook` names",
                "sealed" => "seals the files in the folder its `sealed` names",
                _ => "does not seal a file",
            };
            problems.push(Problem::new(
                &format!("{base}/{key}"),
                format!("a `{kind}` seat {why}, so it takes no `{key}`"),
            ));
        }
        if kind == "sealed" {
            match seat.get("sealed").and_then(Json::as_str) {
                Some(relative) => {
                    check_repository_folder(root, &format!("{base}/sealed"), relative, problems);
                }
                None => problems.push(Problem::new(
                    &format!("{base}/sealed"),
                    "a `sealed` seat names the folder of playbooks a match sealed, \
                     `<round>.jsonc` for each round, as a path from the repository root",
                )),
            }
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
        }
    }
    seats.len()
}

/// A `sealed` seat's folder holds `<round>.jsonc` for every round the
/// scenario plays; the first round without one is named, once a seat
/// (`crates/gamectl/src/scenario.rs` holds the same rule).
fn check_sealed_rounds(root: &Path, json: &Json, segments: usize, problems: &mut Vec<Problem>) {
    let Some(Json::Array(seats)) = json.get("seats") else {
        return;
    };
    for (index, seat) in seats.iter().enumerate() {
        if seat.get("kind").and_then(Json::as_str) != Some("sealed") {
            continue;
        }
        let Some(folder) = seat.get("sealed").and_then(Json::as_str) else {
            continue;
        };
        if leaves_the_repository(folder) || !root.join(folder).is_dir() {
            // Already said, at the same pointer, by `check_seats`.
            continue;
        }
        let missing = (1..=segments)
            .map(|round| (round, format!("{folder}/{round}.jsonc")))
            .find(|(_, file)| !root.join(file).is_file());
        if let Some((round, file)) = missing {
            problems.push(Problem::new(
                &format!("/seats/{index}/sealed"),
                format!(
                    "`{file}` does not exist in the repository: a `sealed` seat seals \
                     `<round>.jsonc` in every round, and this scenario plays round {round}"
                ),
            ));
        }
    }
}

/// Returns how many segments the file lists, for the round limit's check.
fn check_segments(json: &Json, problems: &mut Vec<Problem>) -> usize {
    let Some(Json::Array(segments)) = json.get("segments") else {
        problems.push(Problem::new(
            "/segments",
            "required: an array of one or more segments",
        ));
        return 0;
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
            Some(number) if usize::try_from(number) == Ok(index) => {}
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
            Some(number) if (MS_PER_TICK..=MAX_SEGMENT_MS).contains(&number) => {}
            Some(number) if number > MAX_SEGMENT_MS => problems.push(Problem::new(
                &format!("{base}/length_ms"),
                format!(
                    "a segment plays at most {MAX_SEGMENT_MS} game milliseconds, spec section \
                     3's eight minutes; found {number}"
                ),
            )),
            Some(number) if number > 0 => problems.push(Problem::new(
                &format!("{base}/length_ms"),
                format!(
                    "a segment plays at least one tick, {MS_PER_TICK} game milliseconds: no \
                     match opens on a shorter one; found {number}"
                ),
            )),
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
    segments.len()
}

/// `round_limit`, one of the two match settings a scenario may name (the
/// register's S1-41, taken in S1's first contract pull request; the other is
/// `segment_lengths_ms`, [`check_ladder`]): the round the match ends on,
/// optional, and absent what every committed chain was produced with, the
/// sim's `DEFAULT_ROUND_LIMIT`. Its sibling `units_per_seat`, the harness
/// walkers, went with them (S2-16, S2's plan decision 23): a file that still
/// names it is refused as an unknown key.
fn check_match_settings(json: &Json, segments: usize, problems: &mut Vec<Problem>) {
    // The limit the match plays to, named or the default, never falls below
    // the file's segment count: a scenario plays no round past its limit.
    let limit = match json.get("round_limit") {
        None => Some(DEFAULT_ROUND_LIMIT),
        Some(value) => match integer(Some(value)) {
            Some(number) if number >= 1 && u32::try_from(number).is_ok() => Some(number),
            _ => {
                problems.push(Problem::new(
                    "/round_limit",
                    "optional, but when present it is the round the match ends on: an integer of \
                     1 or more",
                ));
                None
            }
        },
    };
    if let Some(number) =
        limit.filter(|number| usize::try_from(*number).is_ok_and(|limit| limit < segments))
    {
        problems.push(Problem::new(
            "/round_limit",
            format!(
                "the match ends on round {number}, and the file plays {segments} segments; a \
                 scenario plays no round past its limit (absent, the limit is the sim's default \
                 of {DEFAULT_ROUND_LIMIT})"
            ),
        ));
    }
}

/// The optional `segment_lengths_ms` key, the match's segment length list:
/// one to [`MAX_SEGMENT_LENGTHS`] lengths, each a segment's length by the
/// rule a segment's own is held to, and every segment the file plays the
/// length its round takes from the list (its own entry, and past the end the
/// last one). Absent, the match takes the lengths of the file's segments, as
/// every match did before the key existed, so adding it moved no chain.
fn check_ladder(json: &Json, problems: &mut Vec<Problem>) {
    let Some(value) = json.get("segment_lengths_ms") else {
        return;
    };
    let Json::Array(entries) = value else {
        problems.push(Problem::new(
            "/segment_lengths_ms",
            format!(
                "optional, but when present it is the match's segment length list: an array of \
                 1 to {MAX_SEGMENT_LENGTHS} lengths in game milliseconds"
            ),
        ));
        return;
    };
    if entries.is_empty() || entries.len() > MAX_SEGMENT_LENGTHS {
        problems.push(Problem::new(
            "/segment_lengths_ms",
            format!(
                "a match's segment length list holds 1 to {MAX_SEGMENT_LENGTHS} lengths; this \
                 one holds {}",
                entries.len()
            ),
        ));
        return;
    }
    let mut ladder: Vec<i64> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        match integer(Some(entry)) {
            Some(number) if (MS_PER_TICK..=MAX_SEGMENT_MS).contains(&number) => {
                ladder.push(number);
            }
            _ => problems.push(Problem::new(
                &format!("/segment_lengths_ms/{index}"),
                format!(
                    "a segment length is a positive count of game milliseconds, at least one \
                     tick ({MS_PER_TICK}) and at most {MAX_SEGMENT_MS}, spec section 3's eight \
                     minutes"
                ),
            )),
        }
    }
    if ladder.len() != entries.len() {
        return;
    }
    let Some(Json::Array(segments)) = json.get("segments") else {
        return;
    };
    for (index, segment) in segments.iter().enumerate() {
        // A segment with no usable length has been told so already.
        let Some(length) = integer(segment.get("length_ms")) else {
            continue;
        };
        let wanted = ladder.get(index).or(ladder.last()).copied();
        if wanted != Some(length) {
            problems.push(Problem::new(
                &format!("/segments/{index}/length_ms"),
                format!(
                    "`segment_lengths_ms` gives this round {} game milliseconds, and the segment \
                     says {length}; a segment plays the length its round has in the match",
                    wanted.map_or_else(|| String::from("no"), |wanted| wanted.to_string()),
                ),
            ));
        }
    }
}

/// A nightly arm seats one `playbook` seat and one `builtin` seat, in either
/// order (E20; S2's plan, decision 29).
fn check_arm_seats(json: &Json, problems: &mut Vec<Problem>) {
    let Some(Json::Array(seats)) = json.get("seats") else {
        return;
    };
    let kind_count = |wanted: &str| {
        seats
            .iter()
            .filter(|seat| seat.get("kind").and_then(Json::as_str) == Some(wanted))
            .count()
    };
    if seats.len() != 2 || kind_count("playbook") != 1 || kind_count("builtin") != 1 {
        problems.push(Problem::new(
            "/seats",
            "a nightly arm seats exactly two seats: one `playbook` seat, the arm, and one \
             `builtin` seat, Easy, which a run over a seed file alternates between seat 0 and \
             seat 1 (S2's plan, decision 29)",
        ));
    }
}

/// An arena match-up seats two or three contestants, none of them `sealed`
/// (decisions-log item 233).
fn check_match_up_seats(json: &Json, problems: &mut Vec<Problem>) {
    let Some(Json::Array(seats)) = json.get("seats") else {
        return;
    };
    let sealed = seats
        .iter()
        .any(|seat| seat.get("kind").and_then(Json::as_str) == Some("sealed"));
    if seats.len() < 2 || sealed {
        problems.push(Problem::new(
            "/seats",
            "an arena match-up seats two or three contestants, each a `playbook`, a \
             `builtin` or a `safe` seat: a `sealed` seat's files are one match's, and belong to \
             that match's seed (decisions-log item 233)",
        ));
    }
}

fn check_assertions(root: &Path, json: &Json, home: Home, problems: &mut Vec<Problem>) -> usize {
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
        let Some(kind) = assertion.get("assert").and_then(Json::as_str) else {
            problems.push(Problem::new(
                &format!("{base}/assert"),
                format!(
                    "required: the assertion's kind, a string; {FORMAT} has exactly {}",
                    ASSERTIONS.join(" and ")
                ),
            ));
            continue;
        };
        if RESERVED_ASSERTIONS.contains(&kind) {
            problems.push(Problem::new(
                &format!("{base}/assert"),
                format!(
                    "`{kind}` is reserved for the vocabulary extension decisions-log item 97 \
                     authorises (plan §7 decision 16); T15 took none of the three, and the owner \
                     confirmed none is taken now (decisions-log item 128). It is not in {FORMAT}."
                ),
            ));
            continue;
        }
        match kind {
            "event_fired" => {
                on_events = true;
                check_event_fired(assertion, &base, problems);
            }
            "hash_chain_equals" if home == Home::Arm => problems.push(Problem::new(
                &format!("{base}/assert"),
                "a nightly arm carries no hash chain: a run over a seed file replaces its map \
                 seed, so a chain for the file's own seed would pin a match the nightly never \
                 plays (S2's plan, E20)",
            )),
            "hash_chain_equals" if home == Home::MatchUp => problems.push(Problem::new(
                &format!("{base}/assert"),
                "an arena match-up carries no hash chain: the arena replaces its map seed, so a \
                 chain for the file's own seed would pin a match the arena never plays \
                 (decisions-log item 233)",
            )),
            "hash_chain_equals" => {
                on_hashes = true;
                check_hash_chain_equals(root, assertion, &base, problems);
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

    if home == Home::Arm && !assertions.is_empty() && !on_events {
        problems.push(Problem::new(
            "/assertions",
            "a nightly arm asserts on events: its run on its own seed checks them, and it carries \
             no hash chain (S2's plan, E20)",
        ));
    }
    if home == Home::MatchUp && !assertions.is_empty() && !on_events {
        problems.push(Problem::new(
            "/assertions",
            "an arena match-up asserts on events: `scenario run` on its own seed checks them, \
             and it carries no hash chain (decisions-log item 233)",
        ));
    }
    if home == Home::Scenario && !assertions.is_empty() && (!on_events || !on_hashes) {
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
    if let Some(seat) = assertion.get("seat")
        && integer(Some(seat)).is_none_or(|number| number < 0)
    {
        problems.push(Problem::new(
            &format!("{base}/seat"),
            "optional, but when present it is a seat id: an integer of 0 or more",
        ));
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

fn check_hash_chain_equals(root: &Path, assertion: &Json, base: &str, problems: &mut Vec<Problem>) {
    const KEYS: &[&str] = &["assert", "golden", "note"];
    if let Json::Object(members) = assertion {
        unknown_keys(base, members, KEYS, problems);
    }
    match assertion.get("golden").and_then(Json::as_str) {
        Some(path) => {
            // Every other path in the format goes through this, and this one
            // must too: without it `tests/golden/../../../etc/x.hashes.txt`
            // satisfies the prefix and the suffix and names a file outside the
            // tree. `must_exist` is false because the chain is committed by the
            // task that produces it, which may not have run yet.
            check_repository_path(root, &format!("{base}/golden"), path, false, problems);
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
    if leaves_the_repository(value) {
        problems.push(Problem::new(pointer, PATH_RULE));
        return;
    }
    if must_exist && !root.join(value).is_file() {
        problems.push(Problem::new(
            pointer,
            format!("`{value}` does not exist in the repository"),
        ));
    }
}

/// What a path in a scenario file must be.
const PATH_RULE: &str = "paths are relative to the repository root, use forward slashes, have \
                         no empty or `.` part and no `:`, and never contain `..` — a scenario \
                         names files in the tree and nothing outside it";

/// Whether `value` is anything but a path from the repository root that
/// stays inside it, spelled the one way the format takes.
///
/// An empty part is a leading slash, a trailing one or a doubled one, and the
/// empty path; a `:` is a Windows drive, which `Path::join` would follow out
/// of the tree, or a stream name. `crates/gamectl/src/scenario.rs` holds the
/// same function, and both crates' tests run their copy over
/// `crates/gamectl/tests/corpus/scenario-paths.txt`.
fn leaves_the_repository(value: &str) -> bool {
    value.contains('\\')
        || value.contains(':')
        || value.contains("..")
        || value.split('/').any(|part| part.is_empty() || part == ".")
}

/// [`check_repository_path`] for a folder that must exist.
fn check_repository_folder(root: &Path, pointer: &str, value: &str, problems: &mut Vec<Problem>) {
    if leaves_the_repository(value) {
        problems.push(Problem::new(pointer, PATH_RULE));
        return;
    }
    if !root.join(value).is_dir() {
        problems.push(Problem::new(
            pointer,
            format!("`{value}` is not a folder in the repository"),
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
///
/// A comment is what plan-core's reader takes for one
/// (`crates/plan-core/src/jsonc.rs`, `comment_end`): a line comment runs to
/// the first line feed or carriage return; a block comment to the first `*/`
/// after its `/*`, and one that never closes is no comment, its text left for
/// the parser to refuse.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    // Read a character at a time, so a byte at or above 0x80 is always part
    // of a whole character copied as itself (never `char::from` of one byte,
    // which would mean Latin-1), and a blanked character becomes as many
    // spaces as it has bytes: the stripped text keeps the original's byte
    // offsets.
    while let Some(character) = chars.next() {
        if in_string {
            out.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        let next = chars.peek().copied();
        match (character, next) {
            ('"', _) => {
                in_string = true;
                out.push('"');
            }
            ('/', Some('/')) => {
                blank(&mut out, character);
                while let Some(next) = chars.next_if(|next| !matches!(next, '\n' | '\r')) {
                    blank(&mut out, next);
                }
            }
            ('/', Some('*')) if block_closes(chars.clone()) => {
                blank(&mut out, character);
                // The opener's own `*` cannot also close it: `/*/` is open.
                if let Some(star) = chars.next() {
                    blank(&mut out, star);
                }
                let mut previous_star = false;
                for next in chars.by_ref() {
                    blank(&mut out, next);
                    if previous_star && next == '/' {
                        break;
                    }
                    previous_star = next == '*';
                }
            }
            _ => out.push(character),
        }
    }
    out
}

/// Whether the block comment whose `/` was just read closes: `rest` starts at
/// its `*`, and the comment ends at the first `*/` after that `*`. One that
/// never closes is no comment, as plan-core's reader has it.
fn block_closes(mut rest: std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    rest.next();
    let mut previous_star = false;
    rest.any(|next| {
        let closes = previous_star && next == '/';
        previous_star = next == '*';
        closes
    })
}

/// A commented-out character as spaces, one per byte, except a line feed or
/// a carriage return, which stays so that line numbers in a parse error
/// still match the file.
fn blank(out: &mut String, character: char) {
    if matches!(character, '\n' | '\r') {
        out.push(character);
    } else {
        for _ in 0..character.len_utf8() {
            out.push(' ');
        }
    }
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
        // The default rules table. Every scenario runs against one, named or
        // not, so the scratch tree carries it as the real tree does.
        fs::create_dir_all(dir.join("rules")).expect("temp dir");
        fs::write(dir.join("rules").join("rules.v1.json"), "{}\n").expect("rules fixture");
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
    fn block_comments_keep_offsets_and_lines_and_close_only_on_their_own_star() {
        assert_eq!(
            strip_comments("1/* a\n\u{e9} */2"),
            format!("1{}\n{}2", " ".repeat(4), " ".repeat(5))
        );
        // The opener's `*` does not close it: everything after `/*/` up to a
        // real `*/` is comment.
        assert_eq!(strip_comments("1/*/ x */2"), format!("1{}2", " ".repeat(8)));
        // A block comment that never closes is no comment, as plan-core's
        // reader has it (`jsonc::comment_end`): its text is left for the
        // parser to refuse. A line comment runs to its line's end.
        assert_eq!(strip_comments("1/* x"), "1/* x");
        assert_eq!(strip_comments("1/* x */ /* y"), "1        /* y");
        // A line comment ends at a carriage return as at a line feed, so a
        // file with CR line endings does not lose its later lines.
        assert_eq!(strip_comments("1// x\r2"), "1    \r2");
        assert_eq!(strip_comments("1// x\r\n2"), "1    \r\n2");
        assert_eq!(strip_comments("1/* a\r\nb */2"), "1    \r\n    2");
        assert_eq!(
            strip_comments("1// \u{2014}\n2"),
            format!("1{}\n2", " ".repeat(6))
        );
        for source in [
            "1/* a\n\u{e9} */2",
            "1/*/ x */2",
            "1// \u{2014}\n2",
            "\"\u{2014}\"/**/",
        ] {
            assert_eq!(strip_comments(source).len(), source.len(), "{source}");
        }
    }

    #[test]
    fn a_negative_seat_id_or_segment_index_is_a_problem_not_a_magic_value() {
        let dir = scratch("negative-index");
        let path = write(
            &dir,
            &GOOD
                .replace("\"seat\": 0,", "\"seat\": -1,")
                .replace("\"index\": 0,", "\"index\": -1,"),
        );
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/seats/0/seat"), "{report}");
        assert!(report.contains("expected 0, found -1"), "{report}");
        assert!(report.contains("/segments/0/index"), "{report}");
    }

    #[test]
    fn an_assertion_without_a_kind_is_named_as_missing() {
        let dir = scratch("assert-missing");
        let path = write(
            &dir,
            &GOOD.replace("\"assert\": \"event_fired\",", "\"assert\": 7,"),
        );
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/assertions/0/assert"), "{report}");
        assert!(
            report.contains("required: the assertion's kind"),
            "{report}"
        );
    }

    #[test]
    fn non_ascii_text_survives_the_comment_stripper() {
        // The committed example's `summary` carries an em dash, and a `note` or
        // a `name` may carry anything. Re-encoding a UTF-8 continuation byte as
        // Latin-1 turns it into mojibake in the step's summary line and in every
        // diagnostic that quotes the file.
        let source = "{\"s\":\"east \u{2014} ash\"} // gone";
        let stripped = strip_comments(source);
        assert_eq!(stripped, "{\"s\":\"east \u{2014} ash\"}        ");
        assert_eq!(
            stripped.len(),
            source.len(),
            "comments become spaces rather than vanishing so that a parse error's byte offset \
             still points at the right place"
        );

        let dir = scratch("non-ascii");
        let path = write(
            &dir,
            &GOOD.replace("\"smoke\"", "\"smoke \u{2014} \u{e9}tape\""),
        );
        let scenario = validate(&dir, &path).expect("valid");
        assert_eq!(scenario.name, "smoke \u{2014} \u{e9}tape");
    }

    #[test]
    fn a_golden_path_that_escapes_the_tree_is_rejected() {
        // The prefix and the suffix alone would accept this: the `..` rule is
        // what keeps a scenario naming files in the tree and nothing outside it.
        let dir = scratch("golden-escape");
        let path = write(
            &dir,
            &GOOD.replace(
                "tests/golden/scenarios/smoke/expected.hashes.txt",
                "tests/golden/../../../etc/passwd.hashes.txt",
            ),
        );
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/assertions/1/golden"), "{report}");
        assert!(report.contains("`..`"), "{report}");
    }

    #[test]
    fn an_undocumented_seat_key_is_rejected() {
        let dir = scratch("operator-key");
        let path = write(
            &dir,
            &GOOD.replace(
                "{ \"seat\": 1, \"kind\": \"safe\" }",
                "{ \"seat\": 1, \"kind\": \"safe\", \"operator\": \"whatever\" }",
            ),
        );
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/seats/1/operator"), "{report}");
        assert!(report.contains("unknown key"), "{report}");
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
    fn a_segment_longer_than_the_spec_s_eight_minutes_is_rejected() {
        let dir = scratch("segment-cap");
        let path = write(&dir, &GOOD.replace("180000", "480000"));
        validate(&dir, &path).expect("eight minutes is the cap, and allowed");

        let path = write(&dir, &GOOD.replace("180000", "480001"));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/segments/0/length_ms"), "{report}");
        assert!(report.contains("at most 480000"), "{report}");

        // What the cap exists for: a length that asked the runner for gigabytes.
        let path = write(&dir, &GOOD.replace("180000", "2147483647"));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/segments/0/length_ms"), "{report}");
    }

    /// Inserts top-level keys into [`GOOD`] after its `name`.
    fn with_keys(keys: &str) -> String {
        GOOD.replace(
            "  \"name\": \"smoke\",\n",
            &format!("  \"name\": \"smoke\",\n{keys}"),
        )
    }

    #[test]
    fn round_limit_is_an_optional_match_setting_and_the_walkers_key_is_gone() {
        let dir = scratch("match-settings");
        let path = write(&dir, &with_keys("  \"round_limit\": 1,\n"));
        validate(&dir, &path).expect("the key, in range");
        let path = write(&dir, &with_keys("  \"round_limit\": 6,\n"));
        validate(&dir, &path).expect("the default round limit");

        // S2-16 (S2's plan, decision 23): `units_per_seat` left the format
        // with the walkers, so any value of it is an unknown key.
        for (keys, pointer) in [
            ("  \"units_per_seat\": 0,\n", "/units_per_seat"),
            ("  \"units_per_seat\": 50,\n", "/units_per_seat"),
            ("  \"round_limit\": 0,\n", "/round_limit"),
            ("  \"round_limit\": 1.5,\n", "/round_limit"),
        ] {
            let path = write(&dir, &with_keys(keys));
            let report = validate(&dir, &path).expect_err(keys);
            assert!(report.contains(pointer), "{keys}: {report}");
        }
    }

    #[test]
    fn a_round_limit_below_the_segment_count_is_rejected() {
        let dir = scratch("round-limit-short");
        let body = with_keys("  \"round_limit\": 1,\n").replace(
            "[ { \"index\": 0, \"length_ms\": 180000 } ]",
            "[ { \"index\": 0, \"length_ms\": 180000 }, { \"index\": 1, \"length_ms\": 300000 } ]",
        );
        let path = write(&dir, &body);
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/round_limit"), "{report}");
        assert!(report.contains("plays 2 segments"), "{report}");
    }

    /// The default limit holds a file to its segment count as a named one
    /// does: seven segments and no `round_limit` would play past round 6.
    #[test]
    fn the_default_round_limit_is_checked_against_the_segment_count() {
        let dir = scratch("round-limit-default");
        let segments: Vec<String> = (0..7)
            .map(|index| format!("{{ \"index\": {index}, \"length_ms\": 180000 }}"))
            .collect();
        let one = "[ { \"index\": 0, \"length_ms\": 180000 } ]";
        let path = write(
            &dir,
            &GOOD.replace(one, &format!("[ {} ]", segments.join(", "))),
        );
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/round_limit"), "{report}");
        assert!(report.contains("plays 7 segments"), "{report}");

        let six = segments.get(..6).expect("seven segments").join(", ");
        let path = write(&dir, &GOOD.replace(one, &format!("[ {six} ]")));
        validate(&dir, &path).expect("six segments fit the default limit");
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
    fn a_reserved_assertion_names_the_decision_that_holds_it() {
        let dir = scratch("reserved");
        let path = write(&dir, &GOOD.replace("\"event_fired\"", "\"terminal_hash\""));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("decisions-log item 97"), "{report}");
        assert!(report.contains("T15 took none"), "{report}");
        assert!(
            report.contains("owner confirmed none is taken now"),
            "the register's M-05, ruled: {report}"
        );
        assert!(!report.contains("not yet logged"), "{report}");
    }

    /// Inserts a `rules` key into [`GOOD`], which has none.
    fn with_rules(value: &str) -> String {
        GOOD.replace(
            "  \"name\": \"smoke\",\n",
            &format!("  \"name\": \"smoke\",\n  \"rules\": \"{value}\",\n"),
        )
    }

    #[test]
    fn a_scenario_that_names_no_rules_table_runs_against_the_default() {
        let dir = scratch("rules-default");
        let path = write(&dir, GOOD);
        let scenario = validate(&dir, &path).expect("valid");
        assert_eq!(scenario.rules, DEFAULT_RULES);
    }

    #[test]
    fn a_named_rules_table_is_carried_through_to_the_runner() {
        let dir = scratch("rules-named");
        fs::write(dir.join("rules").join("rules.tuning.json"), "{}\n").expect("rules fixture");
        let path = write(&dir, &with_rules("rules/rules.tuning.json"));
        let scenario = validate(&dir, &path).expect("valid");
        assert_eq!(scenario.rules, "rules/rules.tuning.json");
    }

    #[test]
    fn a_rules_table_that_is_not_in_the_repository_is_named() {
        let dir = scratch("rules-missing");
        let path = write(&dir, &with_rules("rules/nowhere.json"));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/rules"), "{report}");
        assert!(report.contains("does not exist"), "{report}");
    }

    /// A scenario that names no table still runs against one, so a tree without
    /// the default is a scenario asserting on a hash chain nothing produced.
    #[test]
    fn a_missing_default_rules_table_is_not_a_silent_pass() {
        let dir = scratch("rules-default-missing");
        fs::remove_file(dir.join("rules").join("rules.v1.json")).expect("rules fixture");
        let path = write(&dir, GOOD);
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/rules"), "{report}");
        assert!(report.contains(DEFAULT_RULES), "{report}");
    }

    #[test]
    fn a_rules_key_that_is_not_a_string_is_refused() {
        let dir = scratch("rules-not-a-string");
        let body = GOOD.replace(
            "  \"name\": \"smoke\",\n",
            "  \"name\": \"smoke\",\n  \"rules\": 1,\n",
        );
        let path = write(&dir, &body);
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/rules"), "{report}");
        assert!(report.contains("must be a string"), "{report}");
    }

    #[test]
    fn a_missing_playbook_is_named() {
        let dir = scratch("missing-playbook");
        let path = write(&dir, &GOOD.replace("p.jsonc", "nowhere.jsonc"));
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/seats/0/playbook"), "{report}");
        assert!(report.contains("does not exist"), "{report}");
    }

    /// `GOOD` with seat 0 a `sealed` seat over `sealed/seat-0`, in a scratch
    /// tree whose folder holds the rounds listed.
    fn sealed_case(name: &str, rounds: &[u32]) -> (PathBuf, String) {
        let dir = scratch(name);
        let folder = dir.join("sealed").join("seat-0");
        fs::create_dir_all(&folder).expect("sealed folder");
        for round in rounds {
            fs::write(folder.join(format!("{round}.jsonc")), "{}\n").expect("sealed file");
        }
        let body = GOOD.replace(
            r#""kind": "playbook", "playbook": "examples/playbooks/p.jsonc""#,
            r#""kind": "sealed", "sealed": "sealed/seat-0""#,
        );
        assert_ne!(body, GOOD, "the seat line was not found");
        (dir, body)
    }

    #[test]
    fn a_sealed_seat_names_a_folder_with_a_file_for_every_round() {
        let (dir, body) = sealed_case("sealed-good", &[1]);
        let path = write(&dir, &body);
        validate(&dir, &path).expect("a sealed seat with its round's file is valid");

        // Two rounds played, one file.
        let two = body.replace(
            r#"[ { "index": 0, "length_ms": 180000 } ]"#,
            r#"[ { "index": 0, "length_ms": 180000 }, { "index": 1, "length_ms": 180000 } ]"#,
        );
        let path = write(&dir, &two);
        let report = validate(&dir, &path).expect_err("rejected");
        assert!(report.contains("/seats/0/sealed"), "{report}");
        assert!(report.contains("sealed/seat-0/2.jsonc"), "{report}");
        assert!(report.contains("plays round 2"), "{report}");

        // No folder, no key, a path out of the tree, and the other kind's key.
        for (from, to, wanted) in [
            ("sealed/seat-0", "sealed/nowhere", "is not a folder"),
            (r#", "sealed": "sealed/seat-0""#, "", "names the folder"),
            ("sealed/seat-0", "../seat-0", "never contain `..`"),
            ("sealed/seat-0", "sealed/seat-0/", "no empty or `.` part"),
            ("sealed/seat-0", "sealed//seat-0", "no empty or `.` part"),
            ("sealed/seat-0", "C:/sealed/seat-0", "no `:`"),
            (
                r#""sealed": "sealed/seat-0""#,
                r#""sealed": 7"#,
                "names the folder",
            ),
        ] {
            let path = write(&dir, &body.replace(from, to));
            let report = validate(&dir, &path).expect_err("rejected");
            assert!(report.contains("/seats/0/sealed"), "{report}");
            assert!(report.contains(wanted), "{wanted}: {report}");
        }
        let both = body.replace(
            r#""sealed": "sealed/seat-0""#,
            r#""sealed": "sealed/seat-0", "playbook": "examples/playbooks/p.jsonc""#,
        );
        let report = validate(&dir, &write(&dir, &both)).expect_err("rejected");
        assert!(report.contains("/seats/0/playbook"), "{report}");
        let safe = body.replace(
            r#"{ "seat": 1, "kind": "safe" }"#,
            r#"{ "seat": 1, "kind": "safe", "sealed": "sealed/seat-0" }"#,
        );
        let report = validate(&dir, &write(&dir, &safe)).expect_err("rejected");
        assert!(report.contains("/seats/1/sealed"), "{report}");
        assert!(report.contains("does not seal a file"), "{report}");
    }

    #[test]
    fn a_segment_plays_the_length_its_round_has_in_the_list() {
        let dir = scratch("ladder");
        let with = |list: &str| {
            GOOD.replace(
                r#""segments": ["#,
                &format!(r#""segment_lengths_ms": {list}, "segments": ["#),
            )
        };
        // The match's list may be longer than the rounds the file plays.
        let path = write(&dir, &with("[180000, 300000, 480000]"));
        validate(&dir, &path).expect("a list whose first length is the segment's");

        for (list, pointer, wanted) in [
            (
                "[300000]",
                "/segments/0/length_ms",
                "gives this round 300000",
            ),
            (
                "180000",
                "/segment_lengths_ms",
                "an array of 1 to 8 lengths",
            ),
            ("[]", "/segment_lengths_ms", "this one holds 0"),
            (
                "[1, 1, 1, 1, 1, 1, 1, 1, 1]",
                "/segment_lengths_ms",
                "this one holds 9",
            ),
            ("[180000, 0]", "/segment_lengths_ms/1", "a positive count"),
            ("[480001]", "/segment_lengths_ms/0", "at most 480000"),
            // Under a tick: no match opens on it, whether or not the file
            // plays that round.
            (
                "[180000, 49]",
                "/segment_lengths_ms/1",
                "at least one tick (50)",
            ),
        ] {
            let report = validate(&dir, &write(&dir, &with(list))).expect_err("rejected");
            assert!(report.contains(pointer), "{list}: {report}");
            assert!(report.contains(wanted), "{list}: {report}");
        }

        // A segment's own length is held to the same floor.
        let short = GOOD.replace(r#""length_ms": 180000"#, r#""length_ms": 49"#);
        assert_ne!(short, GOOD, "the segment line was not found");
        let report = validate(&dir, &write(&dir, &short)).expect_err("rejected");
        assert!(report.contains("/segments/0/length_ms"), "{report}");
        assert!(report.contains("at least one tick, 50"), "{report}");
        let tick = GOOD.replace(r#""length_ms": 180000"#, r#""length_ms": 50"#);
        validate(&dir, &write(&dir, &tick)).expect("one tick is a segment");
    }

    /// The path rule's shared cases: `gamectl`'s reader runs the same file
    /// through its own copy of the rule, so the two cannot part unseen
    /// (decisions-log item 232 (7)).
    #[test]
    fn the_path_rule_answers_the_shared_cases() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask sits under the workspace root")
            .to_path_buf();
        let text = fs::read_to_string(root.join("crates/gamectl/tests/corpus/scenario-paths.txt"))
            .expect("the shared path cases");
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

    /// A file under `scenarios/nightly/` in the scratch tree.
    fn write_arm(dir: &Path, body: &str) -> PathBuf {
        let arms = dir.join("scenarios").join("nightly");
        fs::create_dir_all(&arms).expect("temp dir");
        let path = arms.join("arm.scenario.jsonc");
        fs::write(&path, body).expect("write arm");
        path
    }

    /// An arm: [`GOOD`] with its `safe` seat made `builtin` and its chain
    /// assertion dropped.
    fn arm_body() -> String {
        GOOD.replace("\"kind\": \"safe\"", "\"kind\": \"builtin\"")
            .replace(
                "    { \"assert\": \"hash_chain_equals\", \"golden\": \"tests/golden/scenarios/smoke/expected.hashes.txt\" }\n",
                "",
            )
            .replace("\"by_tick\": 2400 },", "\"by_tick\": 2400 }")
    }

    #[test]
    fn an_arm_validates_without_a_chain_and_is_not_played() {
        let dir = scratch("arm");
        let path = write_arm(&dir, &arm_body());
        let arm = validate(&dir, &path).expect("an arm needs no chain");
        assert_eq!(arm.home, Home::Arm);
        write(&dir, GOOD);
        let all = collect(&dir).expect("listed");
        let played = collect_played(&dir).expect("listed");
        assert_eq!(all.len(), 2, "{all:?}");
        assert_eq!(
            played,
            vec![dir.join("scenarios").join("case.scenario.jsonc")],
            "the arm is validated and never played against a golden"
        );
    }

    #[test]
    fn an_arm_with_a_chain_or_the_wrong_seats_is_refused() {
        let dir = scratch("arm-chained");
        let path = write_arm(
            &dir,
            &GOOD.replace("\"kind\": \"safe\"", "\"kind\": \"builtin\""),
        );
        let report = validate(&dir, &path).expect_err("an arm carries no chain");
        assert!(report.contains("/assertions/1/assert"), "{report}");
        assert!(report.contains("E20"), "{report}");

        let dir = scratch("arm-safe");
        let path = write_arm(
            &dir,
            &arm_body().replace("\"kind\": \"builtin\"", "\"kind\": \"safe\""),
        );
        let report = validate(&dir, &path).expect_err("an arm plays the built-in operator");
        assert!(report.contains("decision 29"), "{report}");
    }

    /// A file under `scenarios/arena/` in the scratch tree.
    fn write_match_up(dir: &Path, body: &str) -> PathBuf {
        let home = dir.join("scenarios").join("arena");
        fs::create_dir_all(&home).expect("temp dir");
        let path = home.join("pair.scenario.jsonc");
        fs::write(&path, body).expect("write match-up");
        path
    }

    /// A match-up: [`GOOD`], a playbook and a `safe` seat, with its chain
    /// assertion dropped.
    fn match_up_body() -> String {
        arm_body().replace("\"kind\": \"builtin\"", "\"kind\": \"safe\"")
    }

    #[test]
    fn a_match_up_validates_without_a_chain_and_is_not_played() {
        let dir = scratch("match-up");
        let path = write_match_up(&dir, &match_up_body());
        let pair = validate(&dir, &path).expect("a match-up needs no chain");
        assert_eq!(pair.home, Home::MatchUp);
        write(&dir, GOOD);
        let played = collect_played(&dir).expect("listed");
        assert_eq!(
            played,
            vec![dir.join("scenarios").join("case.scenario.jsonc")],
            "the match-up is validated and never played against a golden"
        );
    }

    #[test]
    fn a_match_up_with_a_chain_or_a_sealed_seat_is_refused() {
        let dir = scratch("match-up-chained");
        let path = write_match_up(&dir, GOOD);
        let report = validate(&dir, &path).expect_err("a match-up carries no chain");
        assert!(report.contains("/assertions/1/assert"), "{report}");
        assert!(
            report.contains("an arena match-up carries no hash chain"),
            "{report}"
        );

        let dir = scratch("match-up-sealed");
        let path = write_match_up(
            &dir,
            &match_up_body().replace(
                "\"kind\": \"safe\"",
                "\"kind\": \"sealed\", \"sealed\": \"scenarios/arena/pair/seat-1\"",
            ),
        );
        let report = validate(&dir, &path).expect_err("a sealed seat is no contestant");
        assert!(
            report.contains("an arena match-up seats two or three contestants"),
            "{report}"
        );
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
