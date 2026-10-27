// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask g3-budget`: G3′-real's CI alarm (S2's plan, E18 and task
//! `g3r`; decision 31).
//!
//! **An alarm against gross regression, not the gate.** The gate (spec §16's
//! G3′ row: a tick p99 of at most 25 ms and at least 4x real time, a mean of
//! at most 12.5 ms) is certified by recorded runs on the owner's machine under
//! Windows and WSL2 Linux, the slower deciding (S2's plan, section 5 item 4).
//! A wall-clock budget is a property of the machine, so on a shared runner
//! this command fails only at the gate's numbers **doubled**: a tick p99 over
//! the whole 50 ms tick, or a mean over 25 ms. A regression that doubles the
//! tick reddens it; a shared runner's noise does not.
//!
//! Like `package` and `perf-alarms` it sits outside the step table: `ci` never
//! runs it, and `list` does not know it. Every timing is taken inside
//! `crates/bench`, the walled harness (AGENTS.md §4.5): this module only runs
//! `crates/bench/tests/g3_real.rs`'s `g3_budget_segment` and reads the one
//! report line it prints ([`REPORT_PREFIX`]). xtask reads no clock.
//!
//! Every way the run can fail to give a verdict is a [`BudgetError`], and the
//! command fails on each: a missing, doubled or malformed report never reads
//! as a pass.

use std::fmt;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Stdio;

/// The report line's prefix and format version, as
/// `crates/bench/src/g3.rs`'s `BUDGET_REPORT_PREFIX` spells it; a unit test
/// holds the two together.
pub(crate) const REPORT_PREFIX: &str = "g3-budget-report v1";

/// The bench's test that plays the segment and prints the report.
pub(crate) const BUDGET_TEST: &str = "g3_budget_segment";

/// The alarm's p99 bar: the whole 50 ms tick, G3′-a's 25 ms doubled.
pub(crate) const ALARM_P99_NS: u128 = 50_000_000;

/// The alarm's mean bar: 25 ms, G3′-b's 12.5 ms doubled.
pub(crate) const ALARM_MEAN_NS: u128 = 25_000_000;

/// G3′-a's bar, quoted beside the figure and never gated here.
pub(crate) const GATE_P99_NS: u128 = 25_000_000;

/// G3′-b's bar, quoted beside the figure and never gated here.
pub(crate) const GATE_MEAN_NS: u128 = 12_500_000;

/// Why the alarm gave no verdict. Each one fails the command.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum BudgetError {
    /// cargo could not be started.
    Launch { cargo: String, reason: String },
    /// The child's output could not be read.
    Read(String),
    /// The measurement exited unsuccessfully.
    Exited { command: String, status: String },
    /// The output held no report line.
    NoReport,
    /// The output held more than one report line.
    SeveralReports(usize),
    /// A report field is not `key=value`.
    NotAField(String),
    /// A report field this version does not know.
    UnknownField(String),
    /// A report field given twice.
    DuplicateField(&'static str),
    /// A report field the line left out.
    MissingField(&'static str),
    /// A report field whose value is not a whole number.
    BadValue { field: &'static str, value: String },
    /// A report over no ticks, which has no figures.
    NoTicks,
    /// A report over fewer ticks than its segment holds: a short run.
    ShortSegment { ticks: u128, segment_ticks: u128 },
    /// A report whose figures cannot all be true at once.
    Inconsistent(&'static str),
    /// `CARGO` holds a value that is not Unicode.
    CargoNotUnicode,
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Launch { cargo, reason } => write!(f, "could not launch `{cargo}`: {reason}"),
            Self::Read(reason) => write!(f, "could not read the measurement's output: {reason}"),
            Self::Exited { command, status } => write!(f, "`{command}` failed ({status})"),
            Self::NoReport => write!(f, "the measurement printed no `{REPORT_PREFIX}` line"),
            Self::SeveralReports(count) => {
                write!(
                    f,
                    "the measurement printed {count} `{REPORT_PREFIX}` lines, not one"
                )
            }
            Self::NotAField(token) => write!(f, "report token `{token}` is not `key=value`"),
            Self::UnknownField(key) => write!(f, "report field `{key}` is not in {REPORT_PREFIX}"),
            Self::DuplicateField(key) => write!(f, "report field `{key}` is given twice"),
            Self::MissingField(key) => write!(f, "report field `{key}` is missing"),
            Self::BadValue { field, value } => {
                write!(f, "report field `{field}` is `{value}`, not a whole number")
            }
            Self::NoTicks => write!(f, "the report covers no ticks"),
            Self::ShortSegment {
                ticks,
                segment_ticks,
            } => write!(
                f,
                "the report covers {ticks} ticks of a segment of {segment_ticks}: a short run"
            ),
            Self::Inconsistent(why) => write!(f, "the report's figures disagree: {why}"),
            Self::CargoNotUnicode => write!(f, "`CARGO` is set to a value that is not Unicode"),
        }
    }
}

/// The figures the alarm reads, in nanoseconds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Report {
    pub(crate) ticks: u128,
    pub(crate) segment_ticks: u128,
    pub(crate) p50_ns: u128,
    pub(crate) p99_ns: u128,
    pub(crate) mean_ns: u128,
    pub(crate) max_ns: u128,
}

/// The fields a v1 report carries, in the order the bench prints them.
const FIELDS: [&str; 6] = [
    "ticks",
    "segment_ticks",
    "p50_ns",
    "p99_ns",
    "mean_ns",
    "max_ns",
];

/// Read the one report line out of the measurement's output.
pub(crate) fn parse(output: &str) -> Result<Report, BudgetError> {
    let reports: Vec<&str> = output
        .lines()
        .map(str::trim_end)
        .filter_map(|line| {
            line.strip_prefix(REPORT_PREFIX)
                .filter(|rest| rest.is_empty() || rest.starts_with(' '))
        })
        .collect();
    let rest = match reports.as_slice() {
        [] => return Err(BudgetError::NoReport),
        [one] => *one,
        many => return Err(BudgetError::SeveralReports(many.len())),
    };
    let mut values: [(&'static str, Option<u128>); 6] = FIELDS.map(|field| (field, None));
    for token in rest.split_whitespace() {
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| BudgetError::NotAField(token.to_owned()))?;
        let (field, slot) = values
            .iter_mut()
            .find(|(field, _)| *field == key)
            .ok_or_else(|| BudgetError::UnknownField(key.to_owned()))?;
        if slot.is_some() {
            return Err(BudgetError::DuplicateField(field));
        }
        *slot = Some(value.parse::<u128>().map_err(|_| BudgetError::BadValue {
            field,
            value: value.to_owned(),
        })?);
    }
    let [ticks, segment_ticks, p50_ns, p99_ns, mean_ns, max_ns] =
        values.map(|(field, value)| value.ok_or(BudgetError::MissingField(field)));
    let report = Report {
        ticks: ticks?,
        segment_ticks: segment_ticks?,
        p50_ns: p50_ns?,
        p99_ns: p99_ns?,
        mean_ns: mean_ns?,
        max_ns: max_ns?,
    };
    if report.ticks == 0 {
        return Err(BudgetError::NoTicks);
    }
    if report.ticks < report.segment_ticks {
        return Err(BudgetError::ShortSegment {
            ticks: report.ticks,
            segment_ticks: report.segment_ticks,
        });
    }
    if report.p50_ns > report.p99_ns {
        return Err(BudgetError::Inconsistent("the p50 is above the p99"));
    }
    if report.p99_ns > report.max_ns {
        return Err(BudgetError::Inconsistent("the p99 is above the max"));
    }
    if report.mean_ns > report.max_ns {
        return Err(BudgetError::Inconsistent("the mean is above the max"));
    }
    Ok(report)
}

/// Milliseconds with three places, from nanoseconds, by integer arithmetic.
fn ms(ns: u128) -> String {
    let micros = ns.div_euclid(1_000);
    format!(
        "{}.{:03} ms",
        micros.div_euclid(1_000),
        micros.rem_euclid(1_000)
    )
}

/// The alarm's verdict: the lines it prints, and whether it passed. Pure, so
/// the bars and the words are pinned by unit tests.
pub(crate) fn verdict(report: &Report, os: &str) -> (Vec<String>, bool) {
    let p99_over = report.p99_ns > ALARM_P99_NS;
    let mean_over = report.mean_ns > ALARM_MEAN_NS;
    let gate = |figure: u128, bar: u128| if figure <= bar { "within" } else { "over" };
    let mut lines = vec![format!(
        "::notice title=G3' budget::on {os}: tick p99 {}, mean {} over {} ticks (p50 {}, max {}). \
         The alarm fails at a p99 over {} or a mean over {}, the gate's numbers doubled. For \
         reference only, this runner is {} G3'-a's {} and {} G3'-b's {}; the gate is certified \
         on the owner's machine, never here",
        ms(report.p99_ns),
        ms(report.mean_ns),
        report.ticks,
        ms(report.p50_ns),
        ms(report.max_ns),
        ms(ALARM_P99_NS),
        ms(ALARM_MEAN_NS),
        gate(report.p99_ns, GATE_P99_NS),
        ms(GATE_P99_NS),
        gate(report.mean_ns, GATE_MEAN_NS),
        ms(GATE_MEAN_NS),
    )];
    if p99_over {
        lines.push(format!(
            "::error title=G3' budget::tick p99 {} on {os} is over the alarm's {}: a regression \
             that roughly doubles the tick",
            ms(report.p99_ns),
            ms(ALARM_P99_NS)
        ));
    }
    if mean_over {
        lines.push(format!(
            "::error title=G3' budget::tick mean {} on {os} is over the alarm's {}: a regression \
             that roughly doubles the tick",
            ms(report.mean_ns),
            ms(ALARM_MEAN_NS)
        ));
    }
    (lines, !p99_over && !mean_over)
}

/// The cargo arguments the alarm runs. Pure, so its flags are pinned.
pub(crate) fn args(locked: bool) -> Vec<String> {
    let mut args: Vec<String> = vec!["test".to_owned(), "--release".to_owned()];
    if locked {
        args.push("--locked".to_owned());
    }
    for arg in [
        "--package",
        "pharmakos-bench",
        "--test",
        "g3_real",
        "--",
        "--ignored",
        "--nocapture",
        "--exact",
        BUDGET_TEST,
    ] {
        args.push(arg.to_owned());
    }
    args
}

/// Run the measurement, echoing its output as it comes, and answer whether
/// the alarm passed.
pub(crate) fn run(root: &Path, require_tools: bool, locked: bool) -> Result<bool, BudgetError> {
    // cargo sets `CARGO` for the commands it runs, `cargo xtask` among them;
    // unset (xtask run some other way), the one on PATH is the rule.
    let cargo = match std::env::var_os("CARGO") {
        None => "cargo".to_owned(),
        Some(value) => value
            .into_string()
            .map_err(|_| BudgetError::CargoNotUnicode)?,
    };
    let args = args(locked);
    let command = crate::render_command(&cargo, &args);
    println!("   $ {command}");
    let mut child = crate::child_command(require_tools, &cargo, &args, root)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| BudgetError::Launch {
            cargo: cargo.clone(),
            reason: error.to_string(),
        })?;
    let mut output = String::new();
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    println!("{line}");
                    output.push_str(&line);
                    output.push('\n');
                }
                Err(error) => {
                    // Never leave the measurement running unattended. The
                    // read error is the one reported; the kill's and the
                    // wait's would only repeat that the child is gone.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(BudgetError::Read(error.to_string()));
                }
            }
        }
    }
    let status = child
        .wait()
        .map_err(|error| BudgetError::Read(error.to_string()))?;
    if !status.success() {
        return Err(BudgetError::Exited {
            command,
            status: crate::describe_exit(status),
        });
    }
    let report = parse(&output)?;
    let (lines, passed) = verdict(&report, std::env::consts::OS);
    for line in lines {
        println!("{line}");
    }
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "g3-budget-report v1 ticks=9600 segment_ticks=9600 p50_ns=3000000 \
                        p99_ns=9000000 mean_ns=4000000 max_ns=20000000";

    #[test]
    fn a_report_is_read_among_other_output() {
        let output = format!("running 1 test\n::notice::x\n{GOOD}\ntest ok\n");
        assert_eq!(
            parse(&output),
            Ok(Report {
                ticks: 9_600,
                segment_ticks: 9_600,
                p50_ns: 3_000_000,
                p99_ns: 9_000_000,
                mean_ns: 4_000_000,
                max_ns: 20_000_000,
            })
        );
    }

    #[test]
    fn every_malformed_report_is_its_own_error() {
        assert_eq!(parse("nothing here"), Err(BudgetError::NoReport));
        assert_eq!(
            parse(&format!("{GOOD}\n{GOOD}")),
            Err(BudgetError::SeveralReports(2))
        );
        let swap = |from: &str, to: &str| parse(&GOOD.replacen(from, to, 1));
        assert_eq!(
            swap("ticks=9600", "ticks"),
            Err(BudgetError::NotAField("ticks".to_owned()))
        );
        assert_eq!(
            swap("ticks=9600", "tick=9600"),
            Err(BudgetError::UnknownField("tick".to_owned()))
        );
        assert_eq!(
            swap("p50_ns=3000000", "p99_ns=1"),
            Err(BudgetError::DuplicateField("p99_ns"))
        );
        assert_eq!(
            swap(" max_ns=20000000", ""),
            Err(BudgetError::MissingField("max_ns"))
        );
        assert_eq!(
            swap("mean_ns=4000000", "mean_ns=-4"),
            Err(BudgetError::BadValue {
                field: "mean_ns",
                value: "-4".to_owned()
            })
        );
        assert_eq!(swap("ticks=9600", "ticks=0"), Err(BudgetError::NoTicks));
        assert_eq!(
            swap("ticks=9600", "ticks=9599"),
            Err(BudgetError::ShortSegment {
                ticks: 9_599,
                segment_ticks: 9_600
            })
        );
        assert!(matches!(
            swap("p50_ns=3000000", "p50_ns=9000001"),
            Err(BudgetError::Inconsistent(_))
        ));
        assert!(matches!(
            swap("p99_ns=9000000", "p99_ns=20000001"),
            Err(BudgetError::Inconsistent(_))
        ));
        assert!(matches!(
            swap("mean_ns=4000000", "mean_ns=20000001"),
            Err(BudgetError::Inconsistent(_))
        ));
        // A longer word that merely starts with the prefix is not a report.
        assert_eq!(
            parse("g3-budget-report v10 ticks=1"),
            Err(BudgetError::NoReport)
        );
    }

    #[test]
    fn the_alarm_fails_only_past_the_doubled_bars() {
        let at = |p99_ns: u128, mean_ns: u128| Report {
            ticks: 1,
            segment_ticks: 1,
            p50_ns: 0,
            p99_ns,
            mean_ns,
            max_ns: p99_ns,
        };
        assert!(verdict(&at(ALARM_P99_NS, ALARM_MEAN_NS), "linux").1);
        assert!(!verdict(&at(ALARM_P99_NS + 1, 0), "linux").1);
        assert!(!verdict(&at(0, ALARM_MEAN_NS + 1), "linux").1);
        // Over the gate but under the alarm: a notice, never a failure.
        let (lines, passed) = verdict(&at(GATE_P99_NS + 1, GATE_MEAN_NS + 1), "linux");
        assert!(passed);
        assert_eq!(lines.len(), 1);
        assert!(lines.iter().all(|line| line.starts_with("::notice ")));
        let (lines, _) = verdict(&at(ALARM_P99_NS + 1, ALARM_MEAN_NS + 1), "linux");
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with("::error "))
                .count(),
            2
        );
        assert_eq!(ms(12_500_000), "12.500 ms");
    }

    #[test]
    fn the_alarm_runs_one_released_test_by_its_exact_name() {
        assert_eq!(
            args(true).join(" "),
            "test --release --locked --package pharmakos-bench --test g3_real -- --ignored \
             --nocapture --exact g3_budget_segment"
        );
        assert!(!args(false).iter().any(|arg| arg == "--locked"));
    }

    #[test]
    fn the_prefix_and_the_test_are_the_benchs_own() {
        let root = crate::workspace_root().unwrap_or_else(|error| panic!("{error}"));
        let lib = std::fs::read_to_string(root.join("crates/bench/src/g3.rs"))
            .unwrap_or_else(|error| panic!("crates/bench/src/g3.rs: {error}"));
        assert!(
            lib.contains(&format!(
                "pub const BUDGET_REPORT_PREFIX: &str = \"{REPORT_PREFIX}\";"
            )),
            "crates/bench/src/g3.rs no longer spells the report prefix as xtask reads it"
        );
        let test = std::fs::read_to_string(root.join("crates/bench/tests/g3_real.rs"))
            .unwrap_or_else(|error| panic!("crates/bench/tests/g3_real.rs: {error}"));
        // `#[test]`, then the `#[ignore = ...]` line, then the function: an
        // un-ignored segment would run, in debug, in every shard's test step.
        let lines: Vec<&str> = test.lines().map(str::trim).collect();
        let at = lines
            .iter()
            .position(|line| *line == format!("fn {BUDGET_TEST}() {{"))
            .unwrap_or_else(|| panic!("crates/bench/tests/g3_real.rs has no `{BUDGET_TEST}` test"));
        assert!(
            at >= 2
                && lines
                    .get(at - 1)
                    .is_some_and(|line| line.starts_with("#[ignore"))
                && lines.get(at - 2).is_some_and(|line| *line == "#[test]"),
            "`{BUDGET_TEST}` must be `#[test]` and `#[ignore]`d"
        );
    }
}
