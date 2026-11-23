// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl scenario run <file> --seeds <file>` — the nightly adversarial
//! harness (S2's plan, task `night`; register S2-01 and M-01; decisions-log
//! item 144 (6)).
//!
//! An **arm** is a scenario that seats one `playbook` seat, the arm's
//! hand-written playbook, and one `builtin` seat, the built-in operator
//! (Easy, which carries the Balanced weighting). A run over a seed file plays
//! it once per seed, **to the match's end**, each seed replacing the file's
//! `map.seed`, and writes a result instead of checking the file's assertions:
//! who won each match, why it ended, and how often the arm won.
//!
//! # The seat layout (decision 29)
//!
//! The arm alternates seats by the seed's position in the file: the first,
//! third, fifth... seed (even positions, counting from 0) put the arm on seat
//! 0 and the built-in operator on seat 1, and the others the other way round,
//! whatever order the file lists its two seats in. Twenty seeds give the arm
//! ten matches from each seat, not every seed from both.
//!
//! # Winning (decision 29, M-01)
//!
//! The arm **wins** a seed when it is the sole winner: the last seat standing,
//! or the final audit's single winner. A **shared** win, the audit tied on
//! every term with the arm among the tied, is not a win for the alarm band; a
//! **loss** is every seed the arm is not a winner of. `win_rate_percent` is
//! `arm_wins × 100 / seeds`, an integer rounded down: no floats anywhere in
//! this project, CI included.
//!
//! # The result's two spellings
//!
//! `--format text` (the default) is a line per seed and a summary for a
//! person; `--format json` is one JSON document, `pharmakos.nightly-result.v1`,
//! laid out one seed per line so two nights diff line by line. The JSON is the
//! one the workflow's alarm-band step reads and the one
//! `tests/golden/scenarios/nightly-result/expected.result.json` pins.

use std::fmt::Write as _;
use std::path::Path;

use pharmakos_proto::json::Json;

use crate::cli::{Format, Operator, RunOptions};
use crate::exit::{Exit, Failure};
use crate::scenario::run::{Ending, Fired, play_to_end};
use crate::scenario::seeds::{self, Listed};
use crate::scenario::{Scenario, Seat, SeatKind};
use crate::strings;

/// The value of the result's `format` key. A new shape is a new string.
pub const RESULT_FORMAT: &str = "pharmakos.nightly-result.v1";

/// How one seed went for the arm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArmOutcome {
    /// The arm was the sole winner.
    Win,
    /// The arm was among several winners: not a win for the alarm band.
    Shared,
    /// The arm was not a winner.
    Loss,
}

impl ArmOutcome {
    /// How a result spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ArmOutcome::Win => "win",
            ArmOutcome::Shared => "shared",
            ArmOutcome::Loss => "loss",
        }
    }

    fn of(arm: u8, winners: &[u8]) -> ArmOutcome {
        match winners {
            [only] if *only == arm => ArmOutcome::Win,
            _ if winners.contains(&arm) => ArmOutcome::Shared,
            _ => ArmOutcome::Loss,
        }
    }
}

/// One seed's match.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeedResult {
    /// The seed played.
    pub seed: u64,
    /// The seat the arm played.
    pub arm_seat: u8,
    /// How the match ended.
    pub ending: Ending,
    /// What that was for the arm.
    pub outcome: ArmOutcome,
    /// How many times each seat's Attack beacons launched in each round
    /// (`attack_launched`, S3's plan, E14): the outer index is the seat, 0
    /// then 1, the inner the round less one, one entry for every round the
    /// match played.
    pub launches: [Vec<u32>; 2],
    /// How many hunts, Attack beacons aimed at an enemy commander, the seats
    /// filed and never launched over the whole match
    /// ([`crate::scenario::run::Hunts`]).
    pub hunts_unlaunched: usize,
}

/// Count each seat's launches per round from what a match fired.
///
/// # Errors
///
/// [`Exit::Internal`] for an `attack_launched` with no seat or a seat neither
/// of the two an arm's match has, one in a round the match did not play, or a
/// count past a `u32`: each is the sim, the gateway and this runner
/// disagreeing, never a launch to leave out.
pub fn launches_of(events: &[Fired], rounds: u32) -> Result<[Vec<u32>; 2], Failure> {
    let width = usize::try_from(rounds)
        .map_err(|_| Failure::internal("a match of more rounds than this platform counts"))?;
    let mut launches = [vec![0_u32; width], vec![0_u32; width]];
    for fired in events.iter().filter(|fired| fired.kind == LAUNCHED) {
        let refused = |why: &str| {
            Failure::internal(format!(
                "an `{LAUNCHED}` at tick {} in round {} {why}",
                fired.tick, fired.round
            ))
        };
        let seat = fired.seat.ok_or_else(|| refused("names no seat"))?;
        let per_round = launches.get_mut(usize::from(seat)).ok_or_else(|| {
            refused(&format!(
                "names seat {seat}, and an arm's match seats 0 and 1"
            ))
        })?;
        let slot = usize::try_from(fired.round)
            .ok()
            .and_then(|round| round.checked_sub(1))
            .and_then(|index| per_round.get_mut(index))
            .ok_or_else(|| refused(&format!("is outside the {rounds} rounds the match played")))?;
        *slot = slot
            .checked_add(1)
            .ok_or_else(|| refused("is one launch past a u32"))?;
    }
    Ok(launches)
}

/// The event kind a launch is (`EventKind::AttackLaunched`'s name).
const LAUNCHED: &str = "attack_launched";

/// The counts over every seed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Summary {
    /// Seeds played.
    pub seeds: usize,
    /// Seeds the arm was the sole winner of.
    pub arm_wins: usize,
    /// Seeds whose win the arm shared.
    pub shared: usize,
    /// Seeds the arm was not a winner of.
    pub losses: usize,
    /// `arm_wins × 100 / seeds`, rounded down.
    pub win_rate_percent: usize,
}

/// A whole run over a seed file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NightlyResult {
    /// The scenario's `name`.
    pub scenario: String,
    /// The scenario file, as it was named.
    pub source: String,
    /// The seed file, as it was named.
    pub seed_file: String,
    /// The built-in operator the `builtin` seat played.
    pub operator: Operator,
    /// The rules table's hash, sixteen hex digits.
    pub rules_hash: String,
    /// Every seed, in file order.
    pub seeds: Vec<SeedResult>,
    /// The counts.
    pub summary: Summary,
}

/// Run `source` over the seed file `options.seeds` names and render, or write,
/// the result.
///
/// # Errors
///
/// [`Exit::Input`] for a scenario that is not an arm or a seed file that is
/// refused, [`Exit::Failed`] for an arm `submit_plan` refused on a seed, and
/// [`Exit::Internal`] for a result that could not be written.
pub fn run(root: &Path, source: &Path, options: &RunOptions) -> Result<String, Failure> {
    let Some(seed_path) = options.seeds.as_ref() else {
        return Err(Failure::internal(
            "the nightly runner was reached without a seed file",
        ));
    };
    let scenario = crate::scenario::load(root, source)?;
    let seed_file = crate::display(seed_path);
    let listed = read_seeds(root, seed_path, &seed_file)?;
    let operator = options.operator.unwrap_or(Operator::Easy);
    let result = play_seeds(root, &scenario, &listed, operator, &seed_file)?;
    let rendered = match options.format.unwrap_or(Format::Text) {
        Format::Text => render_text(&result),
        Format::Json => render_json(&result)?,
    };
    match options.out.as_ref() {
        None => Ok(rendered),
        Some(out) => {
            let path = root.join(out);
            write_result(&path, &rendered)?;
            Ok(strings::result_written(
                &crate::display(out),
                result.summary.seeds,
                result.summary.arm_wins,
            ))
        }
    }
}

/// Read and decode the seed file, its size checked before it is read.
fn read_seeds(root: &Path, relative: &Path, shown: &str) -> Result<Vec<Listed>, Failure> {
    use std::io::Read as _;
    let path = root.join(relative);
    let unreadable = |error: std::io::Error| {
        Failure::input(strings::unreadable("seed file", shown, &error.to_string()))
    };
    // One byte past the cap is read and no more, so a file of any size costs
    // at most the cap to refuse: the decoder sees the extra byte and says the
    // file is too large.
    let limit = u64::try_from(seeds::MAX_FILE_BYTES)
        .map_err(|_| Failure::internal("the seed file's cap does not fit a u64"))?
        .saturating_add(1);
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(unreadable)?
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    seeds::decode(&bytes)
        .map_err(|error| Failure::input(strings::seeds_refused(shown, &error.to_string())))
}

/// The arm's playbook: the file's one `playbook` seat, beside its one
/// `builtin` seat.
fn arm_of(scenario: &Scenario) -> Result<String, Failure> {
    let mut arm = None;
    let mut builtin = 0_usize;
    for seat in &scenario.seats {
        match &seat.kind {
            SeatKind::Playbook(relative) if arm.is_none() => arm = Some(relative.clone()),
            SeatKind::Builtin => builtin = builtin.saturating_add(1),
            _ => return Err(Failure::input(strings::nightly_layout(&scenario.source))),
        }
    }
    match arm {
        Some(arm) if builtin == 1 && scenario.seats.len() == 2 => Ok(arm),
        _ => Err(Failure::input(strings::nightly_layout(&scenario.source))),
    }
}

/// Play every seed in order and count.
///
/// Public because `tests/nightly.rs` plays the arms through it.
///
/// # Errors
///
/// As [`run`], less the writing.
pub fn play_seeds(
    root: &Path,
    scenario: &Scenario,
    listed: &[Listed],
    operator: Operator,
    seed_file: &str,
) -> Result<NightlyResult, Failure> {
    let arm = arm_of(scenario)?;
    let mut seeds_played: Vec<SeedResult> = Vec::with_capacity(listed.len());
    let mut rules_hash = String::new();
    for (position, entry) in listed.iter().enumerate() {
        // Decision 29: even positions put the arm on seat 0, odd on seat 1.
        let arm_seat = u8::from(!position.is_multiple_of(2));
        let seats = [0_u8, 1]
            .into_iter()
            .map(|seat| Seat {
                seat,
                kind: if seat == arm_seat {
                    SeatKind::Playbook(arm.clone())
                } else {
                    SeatKind::Builtin
                },
            })
            .collect();
        let match_of_seed = Scenario {
            seed: entry.seed,
            seats,
            ..scenario.clone()
        };
        let (played, ending) = play_to_end(root, &match_of_seed).map_err(|mut failure| {
            failure.message = format!(
                "seed {} (line {} of {seed_file}), the arm on seat {arm_seat}: {}",
                seeds::spell(entry.seed),
                entry.line,
                failure.message
            );
            failure
        })?;
        rules_hash = crate::verify::hex(&played.rules_hash.to_be_bytes());
        let outcome = ArmOutcome::of(arm_seat, &ending.winners);
        let launches = launches_of(&played.events, ending.rounds)?;
        seeds_played.push(SeedResult {
            seed: entry.seed,
            arm_seat,
            ending,
            outcome,
            launches,
            hunts_unlaunched: played.hunts.unlaunched(),
        });
    }
    let summary = summarise(&seeds_played)?;
    Ok(NightlyResult {
        scenario: scenario.name.clone(),
        source: scenario.source.clone(),
        seed_file: seed_file.to_owned(),
        operator,
        rules_hash,
        seeds: seeds_played,
        summary,
    })
}

/// The counts, and the win rate rounded down (M-01).
///
/// # Errors
///
/// [`Exit::Internal`] for a run over no seed, which the seed decoder refuses
/// first, or a count past what the arithmetic holds.
pub fn summarise(seeds_played: &[SeedResult]) -> Result<Summary, Failure> {
    let count = |wanted: ArmOutcome| {
        seeds_played
            .iter()
            .filter(|seed| seed.outcome == wanted)
            .count()
    };
    let seeds = seeds_played.len();
    let arm_wins = count(ArmOutcome::Win);
    let win_rate_percent = arm_wins
        .checked_mul(100)
        .and_then(|scaled| scaled.checked_div(seeds))
        .ok_or_else(|| Failure::internal("a win rate over no seed, or past a usize"))?;
    Ok(Summary {
        seeds,
        arm_wins,
        shared: count(ArmOutcome::Shared),
        losses: count(ArmOutcome::Loss),
        win_rate_percent,
    })
}

fn number(value: &dyn std::fmt::Display) -> Json {
    Json::Number(value.to_string())
}

fn text(value: &str) -> Json {
    Json::String(value.to_owned())
}

/// One value on one line: `{"key": value, ...}` and `[a, b]`, each scalar
/// written by the project's own JSON writer (so a string's escapes are its),
/// whose layout puts a newline after every value and so cannot lay a seed
/// out on one line.
fn compact(value: &Json) -> String {
    match value {
        Json::Object(members) => {
            let inner: Vec<String> = members
                .iter()
                .map(|(key, value)| format!("{}: {}", compact(&text(key)), compact(value)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        Json::Array(items) => {
            let inner: Vec<String> = items.iter().map(compact).collect();
            format!("[{}]", inner.join(", "))
        }
        scalar => pharmakos_proto::json::write(scalar).trim_end().to_owned(),
    }
}

/// The result as one JSON document, `pharmakos.nightly-result.v1`: the
/// header keys, then one seed per line, then the summary, LF endings and a
/// trailing newline, so a golden of it is human-diffable and two nights diff
/// seed by seed.
///
/// # Errors
///
/// [`Exit::Internal`] when a value cannot be written as JSON.
pub fn render_json(result: &NightlyResult) -> Result<String, Failure> {
    let write = compact;
    let mut out = String::from("{\n");
    for (key, value) in [
        ("format", text(RESULT_FORMAT)),
        ("scenario", text(&result.scenario)),
        ("source", text(&result.source)),
        ("seed_file", text(&result.seed_file)),
        ("operator", text(result.operator.name())),
        ("rules_hash", text(&result.rules_hash)),
    ] {
        let _ = writeln!(out, "  {}: {},", write(&text(key)), write(&value));
    }
    out.push_str("  \"seeds\": [\n");
    let mut lines: Vec<String> = Vec::with_capacity(result.seeds.len());
    for seed in &result.seeds {
        let winners = Json::Array(
            seed.ending
                .winners
                .iter()
                .map(|seat| number(seat))
                .collect(),
        );
        let entry = Json::Object(vec![
            (String::from("seed"), text(&seeds::spell(seed.seed))),
            (String::from("arm_seat"), number(&seed.arm_seat)),
            (String::from("winners"), winners),
            (String::from("end_reason"), text(seed.ending.reason)),
            (String::from("end_tick"), number(&seed.ending.end_tick)),
            (String::from("rounds"), number(&seed.ending.rounds)),
            (String::from("arm"), text(seed.outcome.name())),
            (
                String::from("launches"),
                Json::Array(
                    seed.launches
                        .iter()
                        .map(|per_round| Json::Array(per_round.iter().map(|n| number(n)).collect()))
                        .collect(),
                ),
            ),
            (
                String::from("hunts_unlaunched"),
                number(&seed.hunts_unlaunched),
            ),
        ]);
        lines.push(format!("    {}", write(&entry)));
    }
    out.push_str(&lines.join(",\n"));
    out.push_str("\n  ],\n");
    let summary = Json::Object(vec![
        (String::from("seeds"), number(&result.summary.seeds)),
        (String::from("arm_wins"), number(&result.summary.arm_wins)),
        (String::from("shared"), number(&result.summary.shared)),
        (String::from("losses"), number(&result.summary.losses)),
        (
            String::from("win_rate_percent"),
            number(&result.summary.win_rate_percent),
        ),
    ]);
    let _ = writeln!(out, "  \"summary\": {}", write(&summary));
    out.push_str("}\n");
    // Read back, so a document this function got wrong is refused here rather
    // than by the workflow's grep a night later.
    pharmakos_proto::json::read(&out)
        .map_err(|error| Failure::internal(format!("the nightly result is not JSON: {error:?}")))?;
    Ok(out)
}

/// The result for a person: one line per seed and the summary.
#[must_use]
pub fn render_text(result: &NightlyResult) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} against {} over {} seed(s) from {} (rules_hash {})",
        result.scenario,
        result.operator.name(),
        result.summary.seeds,
        result.seed_file,
        result.rules_hash
    );
    for seed in &result.seeds {
        let winners: Vec<String> = seed
            .ending
            .winners
            .iter()
            .map(|seat| format!("seat {seat}"))
            .collect();
        let launched: Vec<String> = seed
            .launches
            .iter()
            .enumerate()
            .map(|(seat, per_round)| {
                let rounds: Vec<String> = per_round.iter().map(u32::to_string).collect();
                format!("seat {seat} {}", rounds.join("/"))
            })
            .collect();
        let _ = writeln!(
            out,
            "  {}  arm on seat {}: {} after {} round(s), tick {}, won by {}: {}; launches by \
             round {}; {} hunt(s) never launched",
            seeds::spell(seed.seed),
            seed.arm_seat,
            seed.ending.reason,
            seed.ending.rounds,
            seed.ending.end_tick,
            if winners.is_empty() {
                String::from("nobody")
            } else {
                winners.join(" and ")
            },
            seed.outcome.name(),
            launched.join(", "),
            seed.hunts_unlaunched
        );
    }
    let _ = write!(
        out,
        "arm_wins {}, shared {}, losses {} of {} seed(s): win_rate_percent {}",
        result.summary.arm_wins,
        result.summary.shared,
        result.summary.losses,
        result.summary.seeds,
        result.summary.win_rate_percent
    );
    out
}

fn write_result(path: &Path, rendered: &str) -> Result<(), Failure> {
    let shown = crate::display(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            Failure::internal(strings::result_unwritable(&shown, &error.to_string()))
        })?;
    }
    // `write` and not a formatting writer, for the reason `run.rs` gives: the
    // text is already the bytes, LF endings and all.
    std::fs::write(path, rendered.as_bytes()).map_err(|error| {
        Failure::new(
            Exit::Internal,
            strings::result_unwritable(&shown, &error.to_string()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ArmOutcome, NightlyResult, SeedResult, launches_of, render_json, render_text, summarise,
    };
    use crate::cli::Operator;
    use crate::exit::Exit;
    use crate::scenario::run::{Ending, Fired};

    fn seed(outcome: ArmOutcome) -> SeedResult {
        SeedResult {
            seed: 1,
            arm_seat: 0,
            ending: Ending {
                winners: vec![0],
                reason: "round_limit",
                end_tick: 1,
                rounds: 1,
            },
            outcome,
            launches: [vec![0], vec![0]],
            hunts_unlaunched: 0,
        }
    }

    fn launch(seat: Option<u8>, round: u32, tick: u32) -> Fired {
        Fired {
            tick,
            kind: String::from("attack_launched"),
            seat,
            round,
        }
    }

    /// Two seeds of three rounds: the arm's raid in round 2 of the first,
    /// two launches by Easy in round 3 of the second, and one hunt filed
    /// that never launched.
    fn two_seeds() -> NightlyResult {
        let mut first = seed(ArmOutcome::Win);
        first.ending.rounds = 3;
        first.launches = launches_of(
            &[
                launch(Some(0), 2, 4_875),
                // Not a launch: counted by kind, never by seat alone.
                Fired {
                    tick: 4_900,
                    kind: String::from("attack_ended"),
                    seat: Some(0),
                    round: 2,
                },
            ],
            3,
        )
        .expect("a launch in a played round");
        let mut second = seed(ArmOutcome::Loss);
        second.seed = 2;
        second.arm_seat = 1;
        second.ending.rounds = 3;
        second.launches = launches_of(&[launch(Some(0), 3, 10_000), launch(Some(0), 3, 10_400)], 3)
            .expect("two launches in a played round");
        second.hunts_unlaunched = 1;
        let seeds = vec![first, second];
        let summary = summarise(&seeds).expect("two seeds");
        NightlyResult {
            scenario: String::from("rusher"),
            source: String::from("scenarios/nightly/rusher.scenario.jsonc"),
            seed_file: String::from("scenarios/nightly/seeds.txt"),
            operator: Operator::Easy,
            rules_hash: String::from("0011223344556677"),
            seeds,
            summary,
        }
    }

    /// E14: each seed's line carries `launches`, per seat and per round, and
    /// `hunts_unlaunched`; the document parses back, the format string is
    /// unchanged, and the summary is still the last object (the workflow
    /// reads the last `win_rate_percent` in the file).
    #[test]
    fn the_result_writes_launches_and_unlaunched_hunts_on_each_seed_s_line() {
        let json = render_json(&two_seeds()).expect("a result");
        let lines: Vec<&str> = json.lines().collect();
        let first = lines
            .iter()
            .find(|line| line.contains("\"seed\": \"0x0000000000000001\""))
            .expect("the first seed's line");
        assert!(
            first.ends_with(
                "\"arm\": \"win\", \"launches\": [[0, 1, 0], [0, 0, 0]], \"hunts_unlaunched\": 0},"
            ),
            "{first}"
        );
        let second = lines
            .iter()
            .find(|line| line.contains("\"seed\": \"0x0000000000000002\""))
            .expect("the second seed's line");
        assert!(
            second.ends_with(
                "\"arm\": \"loss\", \"launches\": [[0, 0, 2], [0, 0, 0]], \"hunts_unlaunched\": 1}"
            ),
            "{second}"
        );
        assert!(json.contains("\"format\": \"pharmakos.nightly-result.v1\""));
        assert!(
            lines
                .iter()
                .rev()
                .nth(1)
                .is_some_and(|line| line.starts_with("  \"summary\": {")),
            "the summary is the last object: {json}"
        );
        assert_eq!(
            json.matches("win_rate_percent").count(),
            1,
            "no new key is named after the one the workflow greps for"
        );
        pharmakos_proto::json::read(&json).expect("the document parses back");
    }

    /// The text form says the same counts in a person's words, on each seed's
    /// own line, and its summary line does not change.
    #[test]
    fn the_text_result_says_the_launches_and_the_unlaunched_hunts() {
        let text = render_text(&two_seeds());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines.len(),
            4,
            "a heading, one line per seed, a summary: {text}"
        );
        assert!(
            lines.get(1).is_some_and(|line| line.ends_with(
                "win; launches by round seat 0 0/1/0, seat 1 0/0/0; 0 hunt(s) never launched"
            )),
            "{text}"
        );
        assert!(
            lines.get(2).is_some_and(|line| line.ends_with(
                "loss; launches by round seat 0 0/0/2, seat 1 0/0/0; 1 hunt(s) never launched"
            )),
            "{text}"
        );
        assert_eq!(
            lines.get(3).copied(),
            Some("arm_wins 1, shared 0, losses 1 of 2 seed(s): win_rate_percent 50")
        );
    }

    /// A launch the count cannot place is the sim, the gateway and this
    /// runner disagreeing: refused, never left out (E22).
    #[test]
    fn a_launch_the_count_cannot_place_is_refused() {
        for (event, why) in [
            (launch(None, 1, 10), "names no seat"),
            (launch(Some(2), 1, 10), "names seat 2"),
            (launch(Some(0), 0, 10), "outside the 2 rounds"),
            (launch(Some(1), 3, 10), "outside the 2 rounds"),
        ] {
            let refused = launches_of(&[event], 2).expect_err("refused");
            assert_eq!(refused.code, Exit::Internal);
            assert!(refused.message.contains(why), "{}", refused.message);
        }
        assert_eq!(
            launches_of(&[], 2).expect("no launch"),
            [vec![0, 0], vec![0, 0]],
            "a match with no launch writes a zero for every seat and round"
        );
    }

    #[test]
    fn the_arm_wins_only_as_the_sole_winner() {
        assert_eq!(ArmOutcome::of(0, &[0]), ArmOutcome::Win);
        assert_eq!(ArmOutcome::of(1, &[1]), ArmOutcome::Win);
        assert_eq!(ArmOutcome::of(0, &[0, 1]), ArmOutcome::Shared);
        assert_eq!(ArmOutcome::of(0, &[1]), ArmOutcome::Loss);
        assert_eq!(ArmOutcome::of(0, &[]), ArmOutcome::Loss);
    }

    /// M-01: `arm_wins × 100 / seeds`, rounded down, and a shared win is not
    /// a win.
    #[test]
    fn the_win_rate_rounds_down() {
        let mut seeds = vec![seed(ArmOutcome::Win), seed(ArmOutcome::Win)];
        seeds.push(seed(ArmOutcome::Loss));
        let summary = summarise(&seeds).expect("three seeds");
        assert_eq!(
            summary.win_rate_percent, 66,
            "200 / 3 is 66.67, rounded down"
        );

        let thirteen_of_twenty: Vec<SeedResult> = (0..20)
            .map(|index| {
                seed(if index < 13 {
                    ArmOutcome::Win
                } else if index < 15 {
                    ArmOutcome::Shared
                } else {
                    ArmOutcome::Loss
                })
            })
            .collect();
        let summary = summarise(&thirteen_of_twenty).expect("twenty seeds");
        assert_eq!(
            (
                summary.seeds,
                summary.arm_wins,
                summary.shared,
                summary.losses,
                summary.win_rate_percent
            ),
            (20, 13, 2, 5, 65),
            "13 of 20 is exactly the band's 65 %, and two shared wins count for nothing"
        );
        let fourteen: Vec<SeedResult> = (0..20)
            .map(|index| {
                seed(if index < 14 {
                    ArmOutcome::Win
                } else {
                    ArmOutcome::Loss
                })
            })
            .collect();
        assert_eq!(summarise(&fourteen).expect("twenty").win_rate_percent, 70);
        let one_of_three = [
            seed(ArmOutcome::Win),
            seed(ArmOutcome::Loss),
            seed(ArmOutcome::Loss),
        ];
        assert_eq!(
            summarise(&one_of_three).expect("three").win_rate_percent,
            33
        );
        assert!(summarise(&[]).is_err(), "no seed has no win rate");
    }
}
