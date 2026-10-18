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
use crate::scenario::run::{Ending, play_to_end};
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
}

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
        seeds_played.push(SeedResult {
            seed: entry.seed,
            arm_seat,
            ending,
            outcome,
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
        let _ = writeln!(
            out,
            "  {}  arm on seat {}: {} after {} round(s), tick {}, won by {}: {}",
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
            seed.outcome.name()
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
    use super::{ArmOutcome, SeedResult, summarise};
    use crate::scenario::run::Ending;

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
        }
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
