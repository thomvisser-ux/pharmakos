// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl arena <file> --seeds <file>`: the scenario's seats as
//! **contestants**, played against each other over a seed set
//! (decisions-log item 233).
//!
//! The nightly harness (`scenario run --seeds`, [`super::nightly`]) answers
//! one question, how often one hand-written arm beats the built-in operator,
//! and its result is the alarm band's. The arena answers the general one: any
//! two or three contestants, each a playbook, the built-in operator or the
//! safe playbook, and who wins how often from which seat. It is how a
//! playbook found by play from outside is measured against another. It has
//! no band and no alarm, the main session's call; its results are development
//! evidence only, never proof, and nothing is tuned on them, the main
//! session's reading of the owner's answers to point 72 (results of AI play
//! are development evidence) and point 35 (balance is paused until the new
//! core).
//!
//! # The file
//!
//! Any scenario file whose seats are contestants. A match-up written for the
//! arena lives under `scenarios/arena/` ([`crate::scenario::ARENA_DIR`]),
//! where the reader asks it for no hash chain, as it asks a nightly arm for
//! none: the arena replaces the file's seed.
//!
//! # The contestants
//!
//! A contestant is one seat of the scenario file, numbered by its place in
//! the file's `seats` list, from 0: a `playbook` seat's file, sealed through
//! `submit_plan` every round as the nightly's arm is (a refusal is a broken
//! contestant and fails the run, never the harness door), a `builtin` seat
//! (Easy), or a `safe` seat (the gateway's filed safe playbook, which places
//! no beacon: a passive baseline). A `sealed` seat is refused: it is one
//! match's own files, and they belong to that match's seed.
//!
//! # The matches
//!
//! Every seed is played once per **rotation**: with `n` contestants, rotation
//! `r` (0 to `n - 1`) seats contestant `i` on seat `(i + r) mod n`. So every
//! contestant plays every seat on every seed, and a seat's advantage on a map
//! is in both sides of the count. With three contestants that is three of
//! the six ways to seat them, each contestant on each seat once. Each match
//! is played **to its end**, each seed replacing the file's `map.seed`; the
//! file's assertions and golden are not read, as on a run over a seed file.
//!
//! # Counting
//!
//! A contestant **wins** a match as its sole winner, **shares** it as one of
//! several, and loses it otherwise; `win_rate_percent` is
//! `wins × 100 / matches`, an integer rounded down. `wins_by_seat` splits the
//! wins by the seat they were won from.
//!
//! # The result
//!
//! `--format text` (the default) is for a person; `--format json` is one
//! document, `pharmakos.arena-result.v1`, a match per line. Two numberings
//! run through it and each key says which it counts in: `contestant_by_seat`
//! holds, at each seat's index, the contestant that sat there;
//! `winning_contestants` and the standings count in contestants; `winners`,
//! `launches`, `hunts_unlaunched` and the audit's lines count in seats, as
//! the nightly result's do. A `playbook` contestant carries the plan
//! fingerprint of what it sealed, so a result names the text it measured and
//! not only a path whose file may change.

use std::fmt::Write as _;
use std::path::Path;

use pharmakos_proto::json::Json;

use crate::cli::{Format, Operator, RunOptions};
use crate::exit::Failure;
use crate::scenario::nightly::{
    compact, hunts_by_seat, launches_by_seat, number, read_seeds, text, write_result,
};
use crate::scenario::run::{Ending, ToEnd, play_to_end_for};
use crate::scenario::seeds::{self, Listed};
use crate::scenario::{Scenario, Seat, SeatKind};
use crate::strings;

/// The value of the result's `format` key. A new shape is a new string.
pub const RESULT_FORMAT: &str = "pharmakos.arena-result.v1";

/// What a contestant plays.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Contestant {
    /// The named playbook file, sealed through `submit_plan` every round.
    Playbook(String),
    /// The built-in operator.
    Builtin,
    /// Nothing: the gateway files its safe playbook.
    Safe,
}

impl Contestant {
    /// How a result spells the kind: the scenario format's own word.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Contestant::Playbook(_) => "playbook",
            Contestant::Builtin => "builtin",
            Contestant::Safe => "safe",
        }
    }

    fn seat_kind(&self) -> SeatKind {
        match self {
            Contestant::Playbook(path) => SeatKind::Playbook(path.clone()),
            Contestant::Builtin => SeatKind::Builtin,
            Contestant::Safe => SeatKind::Safe,
        }
    }
}

/// How one match went for one contestant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Its sole winner.
    Win,
    /// One of several winners.
    Shared,
    /// Not a winner.
    Loss,
}

impl Outcome {
    fn of(contestant: usize, winners: &[usize]) -> Outcome {
        match winners {
            [only] if *only == contestant => Outcome::Win,
            _ if winners.contains(&contestant) => Outcome::Shared,
            _ => Outcome::Loss,
        }
    }
}

/// One match: one seed under one rotation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MatchResult {
    /// The seed played.
    pub seed: u64,
    /// The rotation it was played under ([`seating`]).
    pub rotation: usize,
    /// Who sat where: the entry at a seat's index is its contestant.
    pub contestant_by_seat: Vec<usize>,
    /// How the match ended, in seats.
    pub ending: Ending,
    /// The winners as contestants, ascending.
    pub winning_contestants: Vec<usize>,
    /// How many times each seat's Attack beacons launched in each round: the
    /// outer index is the seat, the inner the round less one.
    pub launches: Vec<Vec<u32>>,
    /// How many hunts each seat filed and never launched, by seat.
    pub hunts_unlaunched: Vec<usize>,
}

/// One contestant's counts over every match.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Standing {
    /// Matches played: every seed under every rotation.
    pub matches: usize,
    /// Matches it was the sole winner of.
    pub wins: usize,
    /// Matches whose win it shared.
    pub shared: usize,
    /// Matches it was not a winner of.
    pub losses: usize,
    /// `wins × 100 / matches`, rounded down.
    pub win_rate_percent: usize,
    /// The wins by the seat they were won from, one entry a seat.
    pub wins_by_seat: Vec<usize>,
}

/// A whole run over a seed file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArenaResult {
    /// The scenario's `name`.
    pub scenario: String,
    /// The scenario file, as it was named.
    pub source: String,
    /// The seed file, as it was named.
    pub seed_file: String,
    /// The built-in operator a `builtin` contestant played.
    pub operator: Operator,
    /// The rules table's hash, sixteen hex digits.
    pub rules_hash: String,
    /// The contestants, in the file's seat order.
    pub contestants: Vec<Contestant>,
    /// The plan fingerprint of what each `playbook` contestant sealed, in the
    /// contestants' order; `None` for a contestant that seals no file of its
    /// own.
    pub fingerprints: Vec<Option<u64>>,
    /// Every match: seeds in file order, and each seed's rotations in order.
    pub matches: Vec<MatchResult>,
    /// Each contestant's counts, in the contestants' order.
    pub standings: Vec<Standing>,
}

/// Play `source`'s contestants over the seed file `options.seeds` names and
/// render, or write, the result.
///
/// # Errors
///
/// [`crate::exit::Exit::Input`] for a scenario whose seats are not
/// contestants, a seed file that is refused or `--operator` with no `builtin`
/// contestant, [`crate::exit::Exit::Failed`] for a playbook `submit_plan`
/// refused in a match, and [`crate::exit::Exit::Internal`] for a result that
/// could not be written.
pub fn run(root: &Path, source: &Path, options: &RunOptions) -> Result<String, Failure> {
    let Some(seed_path) = options.seeds.as_ref() else {
        return Err(Failure::internal(
            "the arena was reached without a seed file",
        ));
    };
    let scenario = crate::scenario::load(root, source)?;
    let contestants = contestants_of(&scenario)?;
    if options.operator.is_some() && !contestants.contains(&Contestant::Builtin) {
        return Err(Failure::input(strings::operator_unused(
            "arena",
            &scenario.source,
        )));
    }
    let seed_file = crate::display(seed_path);
    let listed = read_seeds(root, seed_path, &seed_file)?;
    let operator = options.operator.unwrap_or(Operator::Easy);
    let result = play_seeds(root, &scenario, &contestants, &listed, operator, &seed_file)?;
    let rendered = match options.format.unwrap_or(Format::Text) {
        Format::Text => render_text(&result),
        Format::Json => render_json(&result)?,
    };
    match options.out.as_ref() {
        None => Ok(rendered),
        Some(out) => {
            write_result(&root.join(out), &rendered)?;
            Ok(strings::arena_written(
                &crate::display(out),
                result.matches.len(),
            ))
        }
    }
}

/// The file's seats as contestants: two or more, none of them `sealed`.
///
/// # Errors
///
/// [`crate::exit::Exit::Input`], naming the file and the rule.
pub fn contestants_of(scenario: &Scenario) -> Result<Vec<Contestant>, Failure> {
    let refused = || Failure::input(strings::arena_layout(&scenario.source));
    let contestants = scenario
        .seats
        .iter()
        .map(|seat| match &seat.kind {
            SeatKind::Playbook(path) => Ok(Contestant::Playbook(path.clone())),
            SeatKind::Builtin => Ok(Contestant::Builtin),
            SeatKind::Safe => Ok(Contestant::Safe),
            SeatKind::Sealed(_) => Err(refused()),
        })
        .collect::<Result<Vec<Contestant>, Failure>>()?;
    match contestants.as_slice() {
        [] | [_] => Err(refused()),
        _ => Ok(contestants),
    }
}

/// The contestant each seat holds under `rotation`: contestant `i` sits on
/// seat `(i + rotation) mod n`.
///
/// # Errors
///
/// [`crate::exit::Exit::Internal`] for arithmetic past a `usize` or no
/// contestant at all, neither of which a scenario's seat list reaches.
pub fn seating(contestants: usize, rotation: usize) -> Result<Vec<usize>, Failure> {
    let wrong = || Failure::internal("an arena rotation over no contestant, or past a usize");
    if contestants == 0 {
        return Err(wrong());
    }
    let mut seats = vec![0_usize; contestants];
    for contestant in 0..contestants {
        let seat = contestant
            .checked_add(rotation)
            .and_then(|sum| sum.checked_rem(contestants))
            .ok_or_else(wrong)?;
        *seats.get_mut(seat).ok_or_else(wrong)? = contestant;
    }
    Ok(seats)
}

/// Play every seed under every rotation, in order, and count.
///
/// Public because `tests/arena.rs` plays through it.
///
/// # Errors
///
/// As [`run`], less the reading and the writing.
pub fn play_seeds(
    root: &Path,
    scenario: &Scenario,
    contestants: &[Contestant],
    listed: &[Listed],
    operator: Operator,
    seed_file: &str,
) -> Result<ArenaResult, Failure> {
    let mut matches: Vec<MatchResult> = Vec::new();
    let mut rules_hash = String::new();
    let mut fingerprints: Vec<Option<u64>> = vec![None; contestants.len()];
    for entry in listed {
        for rotation in 0..contestants.len() {
            let seated = seating(contestants.len(), rotation)?;
            // Which match a failure belongs to: the seed, the rotation and
            // who sat where.
            let placed = |mut failure: Failure| {
                failure.message = format!(
                    "seed {} (line {} of {seed_file}), rotation {rotation}, the seats holding \
                     contestants {}: {}",
                    seeds::spell(entry.seed),
                    entry.line,
                    listed_of(&seated),
                    failure.message
                );
                failure
            };
            let one = play_one(root, scenario, contestants, entry.seed, rotation, &seated)
                .map_err(placed)?;
            rules_hash = one.rules_hash;
            keep_fingerprints(&mut fingerprints, &seated, &one.fingerprints).map_err(placed)?;
            matches.push(one.result);
        }
    }
    for (index, contestant) in contestants.iter().enumerate() {
        let sealed = fingerprints.get(index).copied().flatten().is_some();
        if matches!(contestant, Contestant::Playbook(_)) != sealed {
            return Err(Failure::internal(format!(
                "contestant {index} is a {} and {} a playbook of its own",
                contestant.kind(),
                if sealed { "sealed" } else { "never sealed" }
            )));
        }
    }
    let standings = standings_of(contestants.len(), &matches)?;
    Ok(ArenaResult {
        scenario: scenario.name.clone(),
        source: scenario.source.clone(),
        seed_file: seed_file.to_owned(),
        operator,
        rules_hash,
        contestants: contestants.to_vec(),
        fingerprints,
        matches,
        standings,
    })
}

/// File each seat's fingerprint under the contestant that sat there. A
/// contestant seals one file in every match, so a match whose fingerprint
/// differs from an earlier match's is its file changing under the run; a
/// change between two rounds of one match is the runner's to refuse
/// (`run.rs`'s `read_fingerprints`).
fn keep_fingerprints(
    kept: &mut [Option<u64>],
    seated: &[usize],
    by_seat: &std::collections::BTreeMap<u8, u64>,
) -> Result<(), Failure> {
    let nobody = || Failure::internal("a seal by a seat no contestant sat on");
    for (seat, fingerprint) in by_seat {
        let contestant = seated.get(usize::from(*seat)).copied().ok_or_else(nobody)?;
        let slot = kept.get_mut(contestant).ok_or_else(nobody)?;
        match slot {
            None => *slot = Some(*fingerprint),
            Some(first) if first == fingerprint => {}
            Some(_) => {
                return Err(Failure::input(strings::arena_playbook_changed(contestant)));
            }
        }
    }
    Ok(())
}

/// One match played.
struct One {
    /// The rules hash it was played under, sixteen hex digits.
    rules_hash: String,
    /// The fingerprint of what each seat that seals a file sealed, by seat.
    fingerprints: std::collections::BTreeMap<u8, u64>,
    /// The match.
    result: MatchResult,
}

/// One match.
fn play_one(
    root: &Path,
    scenario: &Scenario,
    contestants: &[Contestant],
    seed: u64,
    rotation: usize,
    seated: &[usize],
) -> Result<One, Failure> {
    let lost = || Failure::internal("an arena seat with no contestant, or past a seat id");
    let seats = seated
        .iter()
        .enumerate()
        .map(|(seat, contestant)| {
            Ok(Seat {
                seat: u8::try_from(seat).map_err(|_| lost())?,
                kind: contestants.get(*contestant).ok_or_else(lost)?.seat_kind(),
            })
        })
        .collect::<Result<Vec<Seat>, Failure>>()?;
    let match_of_seed = Scenario {
        seed,
        seats,
        ..scenario.clone()
    };
    let (played, ending) = play_to_end_for(root, &match_of_seed, ToEnd::Arena)?;
    let winning_contestants = {
        let mut winners = ending
            .winners
            .iter()
            .map(|seat| seated.get(usize::from(*seat)).copied().ok_or_else(lost))
            .collect::<Result<Vec<usize>, Failure>>()?;
        winners.sort_unstable();
        winners
    };
    let launches = launches_by_seat(&played.events, ending.rounds, seated.len())?;
    let hunts_unlaunched = hunts_by_seat(&played.hunts, seated.len())?;
    Ok(One {
        rules_hash: crate::verify::hex(&played.rules_hash.to_be_bytes()),
        fingerprints: played.fingerprints,
        result: MatchResult {
            seed,
            rotation,
            contestant_by_seat: seated.to_vec(),
            ending,
            winning_contestants,
            launches,
            hunts_unlaunched,
        },
    })
}

/// Each contestant's counts.
///
/// # Errors
///
/// [`crate::exit::Exit::Internal`] for a run of no match, which the seed
/// decoder refuses first, a contestant that sat on no seat of a match, or a
/// count past what the arithmetic holds.
pub fn standings_of(contestants: usize, matches: &[MatchResult]) -> Result<Vec<Standing>, Failure> {
    let wrong = || Failure::internal("an arena count over no match, or past a usize");
    (0..contestants)
        .map(|contestant| {
            let mut standing = Standing {
                matches: matches.len(),
                wins: 0,
                shared: 0,
                losses: 0,
                win_rate_percent: 0,
                wins_by_seat: vec![0; contestants],
            };
            for played in matches {
                let slot = match Outcome::of(contestant, &played.winning_contestants) {
                    Outcome::Win => {
                        let seat = played
                            .contestant_by_seat
                            .iter()
                            .position(|seated| *seated == contestant)
                            .ok_or_else(wrong)?;
                        let from = standing.wins_by_seat.get_mut(seat).ok_or_else(wrong)?;
                        *from = from.checked_add(1).ok_or_else(wrong)?;
                        &mut standing.wins
                    }
                    Outcome::Shared => &mut standing.shared,
                    Outcome::Loss => &mut standing.losses,
                };
                *slot = slot.checked_add(1).ok_or_else(wrong)?;
            }
            standing.win_rate_percent = standing
                .wins
                .checked_mul(100)
                .and_then(|scaled| scaled.checked_div(standing.matches))
                .ok_or_else(wrong)?;
            Ok(standing)
        })
        .collect()
}

fn numbers<T: std::fmt::Display>(values: &[T]) -> Json {
    Json::Array(values.iter().map(|value| number(value)).collect())
}

/// One match as the result's line for it.
fn match_json(played: &MatchResult) -> Json {
    let audit = played
        .ending
        .audit
        .iter()
        .map(|line| {
            Json::Object(vec![
                (String::from("seat"), number(&line.seat)),
                (String::from("held"), number(&line.held)),
                (String::from("destroyed"), number(&line.destroyed)),
                (String::from("beacons_lost"), number(&line.beacons_lost)),
            ])
        })
        .collect();
    Json::Object(vec![
        (String::from("seed"), text(&seeds::spell(played.seed))),
        (String::from("rotation"), number(&played.rotation)),
        (
            String::from("contestant_by_seat"),
            numbers(&played.contestant_by_seat),
        ),
        (String::from("winners"), numbers(&played.ending.winners)),
        (
            String::from("winning_contestants"),
            numbers(&played.winning_contestants),
        ),
        (String::from("end_reason"), text(played.ending.reason)),
        (String::from("end_tick"), number(&played.ending.end_tick)),
        (String::from("rounds"), number(&played.ending.rounds)),
        (
            String::from("launches"),
            Json::Array(played.launches.iter().map(|round| numbers(round)).collect()),
        ),
        (
            String::from("hunts_unlaunched"),
            numbers(&played.hunts_unlaunched),
        ),
        (String::from("audit"), Json::Array(audit)),
    ])
}

/// The result as one JSON document, `pharmakos.arena-result.v1`: the header
/// keys, a contestant per line, a match per line and a standing per line, LF
/// endings and a trailing newline, so a golden of it diffs match by match.
///
/// # Errors
///
/// [`crate::exit::Exit::Internal`] when a value cannot be written as JSON.
pub fn render_json(result: &ArenaResult) -> Result<String, Failure> {
    let mut out = String::from("{\n");
    for (key, value) in [
        ("format", text(RESULT_FORMAT)),
        ("scenario", text(&result.scenario)),
        ("source", text(&result.source)),
        ("seed_file", text(&result.seed_file)),
        ("rules_hash", text(&result.rules_hash)),
    ] {
        let _ = writeln!(out, "  {}: {},", compact(&text(key)), compact(&value));
    }
    let contestants: Vec<Json> = result
        .contestants
        .iter()
        .enumerate()
        .map(|(index, contestant)| {
            let mut members = vec![
                (String::from("contestant"), number(&index)),
                (String::from("kind"), text(contestant.kind())),
            ];
            match contestant {
                Contestant::Playbook(path) => {
                    members.push((String::from("playbook"), text(path)));
                    if let Some(fingerprint) = result.fingerprints.get(index).copied().flatten() {
                        members.push((
                            String::from("plan_fingerprint"),
                            text(&crate::verify::hex(&fingerprint.to_be_bytes())),
                        ));
                    }
                }
                // The operator is said where one played, and nowhere else.
                Contestant::Builtin => {
                    members.push((String::from("operator"), text(result.operator.name())));
                }
                Contestant::Safe => {}
            }
            Json::Object(members)
        })
        .collect();
    let matches: Vec<Json> = result.matches.iter().map(match_json).collect();
    let standings: Vec<Json> = result
        .standings
        .iter()
        .enumerate()
        .map(|(index, standing)| {
            Json::Object(vec![
                (String::from("contestant"), number(&index)),
                (String::from("matches"), number(&standing.matches)),
                (String::from("wins"), number(&standing.wins)),
                (String::from("shared"), number(&standing.shared)),
                (String::from("losses"), number(&standing.losses)),
                (
                    String::from("win_rate_percent"),
                    number(&standing.win_rate_percent),
                ),
                (
                    String::from("wins_by_seat"),
                    numbers(&standing.wins_by_seat),
                ),
            ])
        })
        .collect();
    let sections = [
        ("contestants", contestants),
        ("matches", matches),
        ("standings", standings),
    ];
    let last = sections.len().checked_sub(1);
    for (at, (key, lines)) in sections.iter().enumerate() {
        let lines: Vec<String> = lines
            .iter()
            .map(|line| format!("    {}", compact(line)))
            .collect();
        let _ = writeln!(
            out,
            "  {}: [\n{}\n  ]{}",
            compact(&text(key)),
            lines.join(",\n"),
            if Some(at) == last { "" } else { "," }
        );
    }
    out.push_str("}\n");
    // Read back, so a document this function got wrong is refused here and
    // not by whatever reads the result later.
    pharmakos_proto::json::read(&out)
        .map_err(|error| Failure::internal(format!("the arena result is not JSON: {error:?}")))?;
    Ok(out)
}

fn named(contestant: usize) -> String {
    format!("contestant {contestant}")
}

fn listed_of<T: std::fmt::Display>(values: &[T]) -> String {
    let spelled: Vec<String> = values.iter().map(T::to_string).collect();
    spelled.join("/")
}

/// The result for a person: the contestants, a line per match and a line per
/// standing.
#[must_use]
pub fn render_text(result: &ArenaResult) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}: {} contestant(s) over {} match(es) from {} (rules_hash {})",
        result.scenario,
        result.contestants.len(),
        result.matches.len(),
        result.seed_file,
        result.rules_hash
    );
    for (index, contestant) in result.contestants.iter().enumerate() {
        let _ = match contestant {
            Contestant::Playbook(path) => match result.fingerprints.get(index).copied().flatten() {
                Some(fingerprint) => writeln!(
                    out,
                    "  {}: the playbook {path} (plan fingerprint {})",
                    named(index),
                    crate::verify::hex(&fingerprint.to_be_bytes())
                ),
                None => writeln!(out, "  {}: the playbook {path}", named(index)),
            },
            Contestant::Builtin => writeln!(
                out,
                "  {}: the built-in operator ({})",
                named(index),
                result.operator.name()
            ),
            Contestant::Safe => writeln!(out, "  {}: the safe playbook", named(index)),
        };
    }
    for played in &result.matches {
        let winners: Vec<String> = played
            .winning_contestants
            .iter()
            .map(|index| named(*index))
            .collect();
        let launched: Vec<String> = played
            .launches
            .iter()
            .enumerate()
            .map(|(seat, per_round)| format!("seat {seat} {}", listed_of(per_round)))
            .collect();
        let audited: Vec<String> = played
            .ending
            .audit
            .iter()
            .map(|line| {
                format!(
                    "seat {} held {} destroyed {} lost {} beacon(s)",
                    line.seat, line.held, line.destroyed, line.beacons_lost
                )
            })
            .collect();
        let _ = writeln!(
            out,
            "  {}  rotation {}, the seats holding contestants {}: {} after {} round(s), tick {}, \
             won by {}; launches by round {}; hunts never launched by seat {}; the audit: {}",
            seeds::spell(played.seed),
            played.rotation,
            listed_of(&played.contestant_by_seat),
            played.ending.reason,
            played.ending.rounds,
            played.ending.end_tick,
            if winners.is_empty() {
                String::from("nobody")
            } else {
                winners.join(" and ")
            },
            launched.join(", "),
            listed_of(&played.hunts_unlaunched),
            if audited.is_empty() {
                String::from("no seat")
            } else {
                audited.join(", ")
            }
        );
    }
    let standings: Vec<String> = result
        .standings
        .iter()
        .enumerate()
        .map(|(index, standing)| {
            format!(
                "{}: wins {}, shared {}, losses {} of {} match(es), win_rate_percent {}, wins by \
                 seat {}",
                named(index),
                standing.wins,
                standing.shared,
                standing.losses,
                standing.matches,
                standing.win_rate_percent,
                listed_of(&standing.wins_by_seat)
            )
        })
        .collect();
    out.push_str(&standings.join("\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::{
        ArenaResult, Contestant, MatchResult, contestants_of, keep_fingerprints, render_json,
        render_text, seating, standings_of,
    };
    use crate::cli::Operator;
    use crate::exit::Exit;
    use crate::scenario::run::Ending;
    use crate::scenario::{Seat, SeatKind};

    fn played(seats: &[usize], winners: &[usize]) -> MatchResult {
        // The match's own winners are seats: where each winning contestant
        // sat.
        let mut seat_winners: Vec<u8> = winners
            .iter()
            .map(|winner| {
                let seat = seats
                    .iter()
                    .position(|seated| seated == winner)
                    .expect("a winner sat somewhere");
                u8::try_from(seat).expect("a seat id")
            })
            .collect();
        seat_winners.sort_unstable();
        MatchResult {
            seed: 7,
            rotation: usize::from(seats.first() != Some(&0)),
            contestant_by_seat: seats.to_vec(),
            ending: Ending {
                winners: seat_winners,
                reason: "round_limit",
                end_tick: 1,
                rounds: 1,
                audit: Vec::new(),
            },
            winning_contestants: winners.to_vec(),
            launches: vec![vec![0]; seats.len()],
            hunts_unlaunched: vec![0; seats.len()],
        }
    }

    #[test]
    fn every_contestant_sits_on_every_seat_once() {
        for contestants in 2..=3_usize {
            let mut sat = vec![Vec::new(); contestants];
            for rotation in 0..contestants {
                let seats = seating(contestants, rotation).expect("a rotation");
                let mut sorted = seats.clone();
                sorted.sort_unstable();
                assert_eq!(sorted, (0..contestants).collect::<Vec<_>>(), "{seats:?}");
                for (seat, contestant) in seats.iter().enumerate() {
                    sat.get_mut(*contestant).expect("a contestant").push(seat);
                }
            }
            for mut seats in sat {
                seats.sort_unstable();
                assert_eq!(seats, (0..contestants).collect::<Vec<_>>());
            }
        }
        assert_eq!(seating(2, 0).expect("a rotation"), vec![0, 1]);
        assert_eq!(seating(2, 1).expect("a rotation"), vec![1, 0]);
        assert_eq!(seating(3, 1).expect("a rotation"), vec![2, 0, 1]);
        assert_eq!(seating(0, 0).expect_err("nobody").code, Exit::Internal);
    }

    #[test]
    fn the_standings_count_sole_wins_shares_and_losses_by_seat() {
        let matches = [
            played(&[0, 1], &[0]),
            played(&[1, 0], &[0]),
            played(&[0, 1], &[0, 1]),
            played(&[1, 0], &[]),
            played(&[1, 0], &[1]),
        ];
        let standings = standings_of(2, &matches).expect("standings");
        let first = standings.first().expect("contestant 0");
        assert_eq!(
            (first.matches, first.wins, first.shared, first.losses),
            (5, 2, 1, 2)
        );
        assert_eq!(first.win_rate_percent, 40);
        assert_eq!(first.wins_by_seat, vec![1, 1]);
        let second = standings.get(1).expect("contestant 1");
        assert_eq!((second.wins, second.shared, second.losses), (1, 1, 3));
        assert_eq!(second.win_rate_percent, 20);
        assert_eq!(second.wins_by_seat, vec![1, 0]);
        assert_eq!(
            standings_of(2, &[]).expect_err("no match").code,
            Exit::Internal
        );
    }

    #[test]
    fn a_sealed_seat_and_a_lone_seat_are_no_contestants() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let source = root.join("scenarios/skeleton/expand-east-segment.scenario.jsonc");
        let committed = crate::scenario::load(root, &source).expect("a committed scenario");
        let scenario = |kinds: Vec<SeatKind>| {
            let mut scenario = committed.clone();
            scenario.seats = kinds
                .into_iter()
                .zip(0_u8..)
                .map(|(kind, seat)| Seat { seat, kind })
                .collect();
            scenario
        };
        let fine = scenario(vec![
            SeatKind::Playbook(String::from("a.jsonc")),
            SeatKind::Builtin,
            SeatKind::Safe,
        ]);
        assert_eq!(
            contestants_of(&fine).expect("three contestants"),
            vec![
                Contestant::Playbook(String::from("a.jsonc")),
                Contestant::Builtin,
                Contestant::Safe
            ]
        );
        for refused in [
            scenario(vec![SeatKind::Builtin]),
            scenario(vec![
                SeatKind::Builtin,
                SeatKind::Sealed(String::from("folder")),
            ]),
        ] {
            let failure = contestants_of(&refused).expect_err("no arena");
            assert_eq!(failure.code, Exit::Input);
            assert!(
                failure.message.contains("contestant"),
                "{}",
                failure.message
            );
        }
    }

    /// A seat's fingerprint is filed under the contestant that sat there, and
    /// a contestant whose file changes under the run is refused.
    #[test]
    fn a_fingerprint_follows_its_contestant_and_may_not_change() {
        let by_seat = |pairs: &[(u8, u64)]| pairs.iter().copied().collect();
        let mut kept = vec![None, None];
        keep_fingerprints(&mut kept, &[0, 1], &by_seat(&[(1, 9)])).expect("first");
        assert_eq!(kept, vec![None, Some(9)]);
        // Rotated: contestant 1 sits on seat 0 and seals the same text.
        keep_fingerprints(&mut kept, &[1, 0], &by_seat(&[(0, 9)])).expect("the same text");
        assert_eq!(kept, vec![None, Some(9)]);
        let changed =
            keep_fingerprints(&mut kept, &[1, 0], &by_seat(&[(0, 8)])).expect_err("another text");
        assert_eq!(changed.code, Exit::Input);
        assert!(
            changed.message.contains("contestant 1"),
            "{}",
            changed.message
        );
        let nobody =
            keep_fingerprints(&mut kept, &[1, 0], &by_seat(&[(2, 9)])).expect_err("no such seat");
        assert_eq!(nobody.code, Exit::Internal);
    }

    #[test]
    fn both_spellings_say_the_same_result() {
        let matches = vec![played(&[0, 1], &[0]), played(&[1, 0], &[1])];
        let result = ArenaResult {
            scenario: String::from("s"),
            source: String::from("s.scenario.jsonc"),
            seed_file: String::from("s.txt"),
            operator: Operator::Easy,
            rules_hash: String::from("0123456789abcdef"),
            contestants: vec![
                Contestant::Playbook(String::from("a.jsonc")),
                Contestant::Safe,
            ],
            fingerprints: vec![Some(0x1122_3344_5566_7788), None],
            standings: standings_of(2, &matches).expect("standings"),
            matches,
        };
        let json = render_json(&result).expect("json");
        assert!(
            json.contains("\"format\": \"pharmakos.arena-result.v1\""),
            "{json}"
        );
        assert!(
            json.contains(
                "{\"contestant\": 0, \"kind\": \"playbook\", \"playbook\": \"a.jsonc\", \
                 \"plan_fingerprint\": \"1122334455667788\"}"
            ),
            "{json}"
        );
        // No built-in operator played, so the result names none.
        assert!(!json.contains("operator"), "{json}");
        assert!(
            json.contains(
                "\"rotation\": 1, \"contestant_by_seat\": [1, 0], \"winners\": [0], \
                 \"winning_contestants\": [1]"
            ),
            "{json}"
        );
        assert!(json.ends_with("  ]\n}\n"), "{json}");
        let text = render_text(&result);
        assert!(text.contains("contestant 1: the safe playbook"), "{text}");
        assert!(text.contains("plan fingerprint 1122334455667788"), "{text}");
        assert!(text.contains("the seats holding contestants 1/0"), "{text}");
        assert!(text.contains("win_rate_percent 50"), "{text}");
        assert!(!text.contains("operator"), "{text}");

        let with_easy = ArenaResult {
            contestants: vec![Contestant::Builtin, Contestant::Safe],
            fingerprints: vec![None, None],
            ..result
        };
        let json = render_json(&with_easy).expect("json");
        assert!(
            json.contains("{\"contestant\": 0, \"kind\": \"builtin\", \"operator\": \"easy\"}"),
            "{json}"
        );
    }
}
