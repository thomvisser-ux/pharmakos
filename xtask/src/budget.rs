// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask p3-budget` and `cargo xtask p1-budget`: S3's two authoring
//! alarms (S3's plan, E12 and task `bench3`), beside G3′'s `g3-budget`
//! ([`crate::g3`]).
//!
//! **Alarms against gross regression, not the gates.** P3's gate (spec §16's
//! G2 + P3 row: a repath p99 of at most 5 ms, an estimate p99 of at most 1 ms)
//! and P1's (QUICK at most 5 ms and FULL at most 50 ms p99 at the size budget)
//! are certified by recorded runs on the owner's machine (S3's plan, sections
//! 5.1 and 5.2; `cert3`). A wall-clock budget is a property of the machine, so
//! on a shared runner each alarm fails only at its gate's numbers **doubled**:
//! a repath p99 over 10 ms or an estimate p99 over 2 ms; a QUICK p99 over 10
//! ms or a FULL p99 over 100 ms at the size budget. A regression that doubles
//! a figure reddens it; a shared runner's noise does not.
//!
//! Like `g3-budget` they sit outside the step table: `ci` never runs them, and
//! `list` does not know them. Every timing is taken inside `crates/bench`, the
//! walled harness (AGENTS.md §4.5): this module only runs one `#[ignore]`d
//! bench test by its exact name and reads the one report line it prints
//! ([`Budget::prefix`]). xtask reads no clock.
//!
//! Every way a run can fail to give a verdict is a [`BudgetError`], and the
//! command fails on each: a missing, doubled, short or malformed report never
//! reads as a pass.

use std::fmt;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Stdio;

/// One bar a report's figure is held to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Bar {
    /// The report field, in whole nanoseconds.
    pub(crate) field: &'static str,
    /// What the figure is, as the verdict names it.
    pub(crate) what: &'static str,
    /// The gate's bar, quoted beside the figure and never gated here.
    pub(crate) gate_ns: u128,
    /// The alarm's bar: the gate's doubled. The alarm fails above it.
    pub(crate) alarm_ns: u128,
}

/// One authoring alarm: what it runs, the line it reads and its bars.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Budget {
    /// The command, `cargo xtask <command>`.
    pub(crate) command: &'static str,
    /// The annotation title its lines go under.
    pub(crate) title: &'static str,
    /// The report line's prefix and format version.
    pub(crate) prefix: &'static str,
    /// The bench source that spells the prefix as `BUDGET_REPORT_PREFIX`.
    pub(crate) source: &'static str,
    /// The bench's test target.
    pub(crate) target: &'static str,
    /// The `#[ignore]`d test of the target that measures and prints the line.
    pub(crate) test: &'static str,
    /// Every field the line carries, in the order the bench prints them.
    pub(crate) fields: &'static [&'static str],
    /// Fields that must be above zero: a run over none of them measured
    /// nothing.
    pub(crate) nonzero: &'static [&'static str],
    /// `(measured, planned)` pairs: a run that measured fewer than it planned
    /// is a short run, and no verdict.
    pub(crate) complete: &'static [(&'static str, &'static str)],
    /// The bars.
    pub(crate) bars: &'static [Bar],
}

/// P3's alarm: the bench's `crates/bench/src/p3.rs` and `tests/p3_alarm.rs`.
pub(crate) const P3_BUDGET: Budget = Budget {
    command: "p3-budget",
    title: "P3 budget",
    prefix: "p3-budget-report v1",
    source: "crates/bench/src/p3.rs",
    target: "p3_alarm",
    test: "p3_budget",
    fields: &[
        "seeds",
        "planned_seeds",
        "pairs",
        "planned_pairs",
        "repath_p99_ns",
        "estimate_p99_ns",
    ],
    nonzero: &["seeds", "pairs"],
    complete: &[("seeds", "planned_seeds"), ("pairs", "planned_pairs")],
    bars: &[
        Bar {
            field: "repath_p99_ns",
            what: "repath p99 (cold or under the crater stream)",
            gate_ns: 5_000_000,
            alarm_ns: 10_000_000,
        },
        Bar {
            field: "estimate_p99_ns",
            what: "one-leg estimate_route p99",
            gate_ns: 1_000_000,
            alarm_ns: 2_000_000,
        },
    ],
};

/// P1's alarm: the bench's `crates/bench/src/full.rs` and
/// `tests/perf_alarm.rs`.
pub(crate) const P1_BUDGET: Budget = Budget {
    command: "p1-budget",
    title: "P1 budget",
    prefix: "p1-budget-report v1",
    source: "crates/bench/src/full.rs",
    target: "perf_alarm",
    test: "p1_budget",
    fields: &[
        "budget_units",
        "inputs",
        "planned_inputs",
        "calls",
        "quick_p99_ns",
        "full_p99_ns",
    ],
    nonzero: &["budget_units", "inputs", "calls"],
    complete: &[("inputs", "planned_inputs")],
    bars: &[
        Bar {
            field: "quick_p99_ns",
            what: "QUICK p99 at the size budget",
            gate_ns: 5_000_000,
            alarm_ns: 10_000_000,
        },
        Bar {
            field: "full_p99_ns",
            what: "FULL p99 at the size budget",
            gate_ns: 50_000_000,
            alarm_ns: 100_000_000,
        },
    ],
};

/// Every authoring alarm, by command.
pub(crate) const BUDGETS: [Budget; 2] = [P3_BUDGET, P1_BUDGET];

/// Why an alarm gave no verdict. Each one fails the command.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum BudgetError {
    /// cargo could not be started.
    Launch { cargo: String, reason: String },
    /// The child's output could not be read.
    Read(String),
    /// The measurement exited unsuccessfully.
    Exited { command: String, status: String },
    /// The output held no report line.
    NoReport(&'static str),
    /// The output held more than one report line.
    SeveralReports { prefix: &'static str, count: usize },
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
    /// A field that must be above zero is zero: the run measured nothing.
    Zero(&'static str),
    /// A run that measured fewer than it planned.
    Short {
        field: &'static str,
        measured: u128,
        planned: u128,
    },
    /// `CARGO` holds a value that is not Unicode.
    CargoNotUnicode,
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Launch { cargo, reason } => write!(f, "could not launch `{cargo}`: {reason}"),
            Self::Read(reason) => write!(f, "could not read the measurement's output: {reason}"),
            Self::Exited { command, status } => write!(f, "`{command}` failed ({status})"),
            Self::NoReport(prefix) => write!(f, "the measurement printed no `{prefix}` line"),
            Self::SeveralReports { prefix, count } => {
                write!(
                    f,
                    "the measurement printed {count} `{prefix}` lines, not one"
                )
            }
            Self::NotAField(token) => write!(f, "report token `{token}` is not `key=value`"),
            Self::UnknownField(key) => write!(f, "report field `{key}` is not in this version"),
            Self::DuplicateField(key) => write!(f, "report field `{key}` is given twice"),
            Self::MissingField(key) => write!(f, "report field `{key}` is missing"),
            Self::BadValue { field, value } => {
                write!(f, "report field `{field}` is `{value}`, not a whole number")
            }
            Self::Zero(field) => write!(f, "report field `{field}` is 0: the run measured nothing"),
            Self::Short {
                field,
                measured,
                planned,
            } => write!(
                f,
                "the report's `{field}` is {measured} of the {planned} planned: a short run"
            ),
            Self::CargoNotUnicode => write!(f, "`CARGO` is set to a value that is not Unicode"),
        }
    }
}

/// A report's fields, in the order [`Budget::fields`] names them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Report {
    pub(crate) values: Vec<(&'static str, u128)>,
}

impl Report {
    /// The value of `field`, which [`parse`] guarantees for every field of
    /// the budget it read the line for.
    fn get(&self, field: &str) -> Result<u128, BudgetError> {
        self.values
            .iter()
            .find(|(name, _)| *name == field)
            .map(|(_, value)| *value)
            .ok_or_else(|| BudgetError::UnknownField(field.to_owned()))
    }
}

/// Read `budget`'s one report line out of the measurement's output.
pub(crate) fn parse(budget: &Budget, output: &str) -> Result<Report, BudgetError> {
    let reports: Vec<&str> = output
        .lines()
        .map(str::trim_end)
        .filter_map(|line| {
            line.strip_prefix(budget.prefix)
                .filter(|rest| rest.is_empty() || rest.starts_with(' '))
        })
        .collect();
    let rest = match reports.as_slice() {
        [] => return Err(BudgetError::NoReport(budget.prefix)),
        [one] => *one,
        many => {
            return Err(BudgetError::SeveralReports {
                prefix: budget.prefix,
                count: many.len(),
            });
        }
    };
    let mut values: Vec<(&'static str, Option<u128>)> =
        budget.fields.iter().map(|field| (*field, None)).collect();
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
    let report = Report {
        values: values
            .into_iter()
            .map(|(field, value)| {
                value
                    .map(|value| (field, value))
                    .ok_or(BudgetError::MissingField(field))
            })
            .collect::<Result<_, _>>()?,
    };
    for field in budget.nonzero {
        if report.get(field)? == 0 {
            return Err(BudgetError::Zero(field));
        }
    }
    for (measured, planned) in budget.complete {
        let (got, wanted) = (report.get(measured)?, report.get(planned)?);
        if got < wanted {
            return Err(BudgetError::Short {
                field: measured,
                measured: got,
                planned: wanted,
            });
        }
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
pub(crate) fn verdict(
    budget: &Budget,
    report: &Report,
    os: &str,
) -> Result<(Vec<String>, bool), BudgetError> {
    let mut figures: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for bar in budget.bars {
        let figure = report.get(bar.field)?;
        let side = if figure <= bar.gate_ns {
            "within"
        } else {
            "over"
        };
        figures.push(format!(
            "{} {} (the alarm fails over {}; for reference only, {side} the gate's {})",
            bar.what,
            ms(figure),
            ms(bar.alarm_ns),
            ms(bar.gate_ns),
        ));
        if figure > bar.alarm_ns {
            errors.push(format!(
                "::error title={}::{} {} on {os} is over the alarm's {}: a regression that \
                 roughly doubles it",
                budget.title,
                bar.what,
                ms(figure),
                ms(bar.alarm_ns)
            ));
        }
    }
    let passed = errors.is_empty();
    let mut lines = vec![format!(
        "::notice title={}::on {os}: {}. The alarm fails at the gate's numbers doubled; the \
         gate is certified on the owner's machine, never here",
        budget.title,
        figures.join("; "),
    )];
    lines.extend(errors);
    Ok((lines, passed))
}

/// The cargo arguments the alarm runs. Pure, so its flags are pinned.
pub(crate) fn args(budget: &Budget, locked: bool) -> Vec<String> {
    let mut args: Vec<String> = vec!["test".to_owned(), "--release".to_owned()];
    if locked {
        args.push("--locked".to_owned());
    }
    for arg in [
        "--package",
        "pharmakos-bench",
        "--test",
        budget.target,
        "--",
        "--ignored",
        "--nocapture",
        "--exact",
        budget.test,
    ] {
        args.push(arg.to_owned());
    }
    args
}

/// Run `budget`'s measurement, echoing its output as it comes, and answer
/// whether the alarm passed.
pub(crate) fn run(
    budget: &Budget,
    root: &Path,
    require_tools: bool,
    locked: bool,
) -> Result<bool, BudgetError> {
    // cargo sets `CARGO` for the commands it runs, `cargo xtask` among them;
    // unset (xtask run some other way), the one on PATH is the rule.
    let cargo = match std::env::var_os("CARGO") {
        None => "cargo".to_owned(),
        Some(value) => value
            .into_string()
            .map_err(|_| BudgetError::CargoNotUnicode)?,
    };
    let args = args(budget, locked);
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
    let report = parse(budget, &output)?;
    let (lines, passed) = verdict(budget, &report, std::env::consts::OS)?;
    for line in lines {
        println!("{line}");
    }
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const P3: &str = "p3-budget-report v1 seeds=3 planned_seeds=3 pairs=2000 \
                      planned_pairs=2000 repath_p99_ns=900000 estimate_p99_ns=400000";

    const P1: &str = "p1-budget-report v1 budget_units=128 inputs=7 planned_inputs=7 \
                      calls=500 quick_p99_ns=3000000 full_p99_ns=30000000";

    #[test]
    fn a_report_is_read_among_other_output() {
        let output = format!("running 1 test\n::notice::x\n{P3}\ntest ok\n");
        let report = parse(&P3_BUDGET, &output).expect("a report");
        assert_eq!(report.get("repath_p99_ns"), Ok(900_000));
        assert_eq!(report.get("pairs"), Ok(2_000));
        let report = parse(&P1_BUDGET, P1).expect("a report");
        assert_eq!(report.get("full_p99_ns"), Ok(30_000_000));
    }

    #[test]
    fn every_malformed_report_is_its_own_error() {
        assert_eq!(
            parse(&P3_BUDGET, "nothing"),
            Err(BudgetError::NoReport("p3-budget-report v1"))
        );
        assert_eq!(
            parse(&P3_BUDGET, &format!("{P3}\n{P3}")),
            Err(BudgetError::SeveralReports {
                prefix: "p3-budget-report v1",
                count: 2
            })
        );
        // One alarm's line is not the other's.
        assert_eq!(
            parse(&P1_BUDGET, P3),
            Err(BudgetError::NoReport("p1-budget-report v1"))
        );
        let swap = |from: &str, to: &str| parse(&P3_BUDGET, &P3.replacen(from, to, 1));
        assert_eq!(
            swap("seeds=3", "seeds"),
            Err(BudgetError::NotAField("seeds".to_owned()))
        );
        assert_eq!(
            swap("seeds=3", "seed=3"),
            Err(BudgetError::UnknownField("seed".to_owned()))
        );
        assert_eq!(
            swap("planned_seeds=3", "seeds=3"),
            Err(BudgetError::DuplicateField("seeds"))
        );
        assert_eq!(
            swap(" estimate_p99_ns=400000", ""),
            Err(BudgetError::MissingField("estimate_p99_ns"))
        );
        assert_eq!(
            swap("estimate_p99_ns=400000", "estimate_p99_ns=none"),
            Err(BudgetError::BadValue {
                field: "estimate_p99_ns",
                value: "none".to_owned()
            })
        );
        assert_eq!(swap("seeds=3", "seeds=0"), Err(BudgetError::Zero("seeds")));
        assert_eq!(
            swap("pairs=2000", "pairs=8"),
            Err(BudgetError::Short {
                field: "pairs",
                measured: 8,
                planned: 2_000
            })
        );
        assert_eq!(
            parse(&P1_BUDGET, &P1.replacen("inputs=7", "inputs=6", 1)),
            Err(BudgetError::Short {
                field: "inputs",
                measured: 6,
                planned: 7
            })
        );
        assert_eq!(
            parse(&P1_BUDGET, &P1.replacen("calls=500", "calls=0", 1)),
            Err(BudgetError::Zero("calls"))
        );
        // A longer word that merely starts with the prefix is not a report.
        assert_eq!(
            parse(&P3_BUDGET, "p3-budget-report v10 seeds=1"),
            Err(BudgetError::NoReport("p3-budget-report v1"))
        );
    }

    #[test]
    fn each_alarm_fails_only_past_its_doubled_bars() {
        let at = |budget: &Budget, line: &str| {
            let report = parse(budget, line).expect("a report");
            verdict(budget, &report, "linux").expect("a verdict")
        };
        let p3 = |repath: u128, estimate: u128| {
            format!(
                "p3-budget-report v1 seeds=3 planned_seeds=3 pairs=2000 planned_pairs=2000 \
                 repath_p99_ns={repath} estimate_p99_ns={estimate}"
            )
        };
        assert!(at(&P3_BUDGET, &p3(10_000_000, 2_000_000)).1);
        assert!(!at(&P3_BUDGET, &p3(10_000_001, 0)).1);
        assert!(!at(&P3_BUDGET, &p3(0, 2_000_001)).1);
        // Over the gate but under the alarm: a notice, never a failure.
        let (lines, passed) = at(&P3_BUDGET, &p3(5_000_001, 1_000_001));
        assert!(passed);
        assert_eq!(lines.len(), 1);
        assert!(lines.iter().all(|line| line.starts_with("::notice ")));
        let p1 = |quick: u128, full: u128| {
            format!(
                "p1-budget-report v1 budget_units=128 inputs=7 planned_inputs=7 calls=500 \
                 quick_p99_ns={quick} full_p99_ns={full}"
            )
        };
        assert!(at(&P1_BUDGET, &p1(10_000_000, 100_000_000)).1);
        let (lines, passed) = at(&P1_BUDGET, &p1(10_000_001, 100_000_001));
        assert!(!passed);
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with("::error "))
                .count(),
            2
        );
    }

    #[test]
    fn the_verdict_is_word_for_word() {
        let report = parse(&P1_BUDGET, P1).expect("a report");
        let (lines, passed) = verdict(&P1_BUDGET, &report, "linux").expect("a verdict");
        assert!(passed);
        assert_eq!(
            lines,
            vec![
                "::notice title=P1 budget::on linux: QUICK p99 at the size budget 3.000 ms (the \
                 alarm fails over 10.000 ms; for reference only, within the gate's 5.000 ms); \
                 FULL p99 at the size budget 30.000 ms (the alarm fails over 100.000 ms; for \
                 reference only, within the gate's 50.000 ms). The alarm fails at the gate's \
                 numbers doubled; the gate is certified on the owner's machine, never here"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn each_alarm_runs_one_released_test_by_its_exact_name() {
        assert_eq!(
            args(&P3_BUDGET, true).join(" "),
            "test --release --locked --package pharmakos-bench --test p3_alarm -- --ignored \
             --nocapture --exact p3_budget"
        );
        assert_eq!(
            args(&P1_BUDGET, false).join(" "),
            "test --release --package pharmakos-bench --test perf_alarm -- --ignored \
             --nocapture --exact p1_budget"
        );
    }

    /// The prefix each alarm reads is the one its bench source prints, every
    /// gated field is a field of the line, and the test it runs exists in its
    /// target, `#[test]` and `#[ignore]`d.
    #[test]
    fn the_prefixes_and_the_tests_are_the_benchs_own() {
        let root = crate::workspace_root().unwrap_or_else(|error| panic!("{error}"));
        for budget in BUDGETS {
            let source = std::fs::read_to_string(root.join(budget.source))
                .unwrap_or_else(|error| panic!("{}: {error}", budget.source));
            assert!(
                source.contains(&format!(
                    "pub const BUDGET_REPORT_PREFIX: &str = \"{}\";",
                    budget.prefix
                )),
                "{} no longer spells the report prefix as xtask reads it",
                budget.source
            );
            for field in budget.fields {
                assert!(
                    source.contains(&format!("{field}=")),
                    "{} prints no `{field}=`",
                    budget.source
                );
            }
            for bar in budget.bars {
                assert!(budget.fields.contains(&bar.field), "{}", bar.field);
                assert_eq!(
                    bar.alarm_ns,
                    bar.gate_ns * 2,
                    "{}: the gate doubled",
                    bar.field
                );
            }
            let path = root.join(format!("crates/bench/tests/{}.rs", budget.target));
            let test = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let lines: Vec<&str> = test.lines().map(str::trim).collect();
            let at = lines
                .iter()
                .position(|line| *line == format!("fn {}() {{", budget.test))
                .unwrap_or_else(|| panic!("{} has no `{}` test", path.display(), budget.test));
            assert!(
                at >= 2
                    && lines
                        .get(at - 1)
                        .is_some_and(|line| line.starts_with("#[ignore"))
                    && lines.get(at - 2).is_some_and(|line| *line == "#[test]"),
                "`{}` must be `#[test]` and `#[ignore]`d",
                budget.test
            );
        }
    }
}
