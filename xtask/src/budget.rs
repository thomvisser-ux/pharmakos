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
//! ms, a FULL p99 over 100 ms, or an end-to-end `verify_plan` p99 over 100 ms
//! (the editor's own 50 ms bar, decisions-log item 162 (1)), at the size
//! budget. A regression that doubles a figure reddens it; a shared runner's
//! noise does not.
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
use std::path::Path;

/// One bar a report's figure is held to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Bar {
    /// The report field, in whole nanoseconds.
    pub(crate) field: &'static str,
    /// The report field the bench prints its own bar in, which must equal
    /// [`Bar::gate_ns`]: the bench's constant is the one fact, and a line
    /// whose bar moved alone gives no verdict ([`BudgetError::BarMismatch`]).
    pub(crate) printed: &'static str,
    /// What the figure is, as the verdict names it.
    pub(crate) what: &'static str,
    /// The gate's bar, quoted beside the figure and never gated here.
    pub(crate) gate_ns: u128,
    /// The alarm's bar: the gate's doubled. The alarm fails above it.
    pub(crate) alarm_ns: u128,
}

/// One planned count a report prints, and this alarm's own copy of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Planned {
    /// The report field the bench prints its plan in.
    pub(crate) field: &'static str,
    /// The plan as this alarm holds it. The bench's constant is printed back
    /// on the line, and a line whose plan moved alone gives no verdict
    /// ([`BudgetError::PlanMismatch`]): a plan lowered in the bench alone
    /// would otherwise turn a short run into a complete one.
    pub(crate) count: u128,
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
    /// `(measured, planned)` pairs that must be equal: a run that measured
    /// more than it planned is no verdict either.
    pub(crate) exact: &'static [(&'static str, &'static str)],
    /// Every planned count the line prints, as this alarm holds it.
    pub(crate) plans: &'static [Planned],
    /// The bars.
    pub(crate) bars: &'static [Bar],
}

/// P3's alarm: the bench's `crates/bench/src/p3.rs` and `tests/p3_alarm.rs`.
pub(crate) const P3_BUDGET: Budget = Budget {
    command: "p3-budget",
    title: "P3 budget",
    prefix: "p3-budget-report v2",
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
        "repath_bar_ns",
        "estimate_bar_ns",
    ],
    nonzero: &[
        "seeds",
        "planned_seeds",
        "pairs",
        "planned_pairs",
        "repath_p99_ns",
        "estimate_p99_ns",
    ],
    complete: &[("seeds", "planned_seeds"), ("pairs", "planned_pairs")],
    exact: &[],
    // `crates/bench/src/p3.rs`'s `P3_SEEDS.len()` and `PAIRS`.
    plans: &[
        Planned {
            field: "planned_seeds",
            count: 3,
        },
        Planned {
            field: "planned_pairs",
            count: 2_000,
        },
    ],
    bars: &[
        Bar {
            field: "repath_p99_ns",
            printed: "repath_bar_ns",
            what: "repath p99 (cold or under the crater stream)",
            gate_ns: 5_000_000,
            alarm_ns: 10_000_000,
        },
        Bar {
            field: "estimate_p99_ns",
            printed: "estimate_bar_ns",
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
    prefix: "p1-budget-report v2",
    source: "crates/bench/src/full.rs",
    target: "perf_alarm",
    test: "p1_budget",
    fields: &[
        "budget_units",
        "inputs",
        "planned_inputs",
        "e2e_inputs",
        "planned_e2e_inputs",
        "calls",
        "planned_calls",
        "quick_p99_ns",
        "full_p99_ns",
        "e2e_p99_ns",
        "quick_bar_ns",
        "full_bar_ns",
        "e2e_bar_ns",
    ],
    nonzero: &[
        "budget_units",
        "inputs",
        "planned_inputs",
        "e2e_inputs",
        "planned_e2e_inputs",
        "calls",
        "planned_calls",
        "quick_p99_ns",
        "full_p99_ns",
        "e2e_p99_ns",
    ],
    complete: &[
        ("inputs", "planned_inputs"),
        ("e2e_inputs", "planned_e2e_inputs"),
        ("calls", "planned_calls"),
    ],
    exact: &[("e2e_inputs", "planned_e2e_inputs")],
    // `crates/bench/src/full.rs`'s `BUDGET_INPUTS` (the shapes and
    // `budget_128`), `BUDGET_END_TO_END_INPUTS` (the shapes) and `BUDGET_CALLS`.
    plans: &[
        Planned {
            field: "planned_inputs",
            count: 7,
        },
        Planned {
            field: "planned_e2e_inputs",
            count: 6,
        },
        Planned {
            field: "planned_calls",
            count: 500,
        },
    ],
    bars: &[
        Bar {
            field: "quick_p99_ns",
            printed: "quick_bar_ns",
            what: "QUICK p99 at the size budget",
            gate_ns: 5_000_000,
            alarm_ns: 10_000_000,
        },
        Bar {
            field: "full_p99_ns",
            printed: "full_bar_ns",
            what: "FULL p99 at the size budget",
            gate_ns: 50_000_000,
            alarm_ns: 100_000_000,
        },
        // The editor's wait for a check, its own bar by decisions-log item
        // 162 (1): `verify_plan` through the hosted gateway, the worse depth.
        Bar {
            field: "e2e_p99_ns",
            printed: "e2e_bar_ns",
            what: "end-to-end verify_plan p99 at the size budget",
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
    /// The checked run gave no output to read: a refused environment, a build
    /// that broke its profile's promise, a launch, a read or a failed exit.
    Run(String),
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
    /// A run that measured more than a count it must meet exactly.
    Over {
        field: &'static str,
        measured: u128,
        planned: u128,
    },
    /// A report field whose value is signed, which no count or time is.
    Signed { field: &'static str, value: String },
    /// The bench printed a bar other than the one this alarm holds.
    BarMismatch {
        field: &'static str,
        bench: u128,
        alarm: u128,
    },
    /// The bench printed a planned count other than the one this alarm holds.
    PlanMismatch {
        field: &'static str,
        bench: u128,
        alarm: u128,
    },
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Run(reason) => write!(f, "the measurement did not run: {reason}"),
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
            Self::Over {
                field,
                measured,
                planned,
            } => write!(
                f,
                "the report's `{field}` is {measured}, past the {planned} planned exactly"
            ),
            Self::Signed { field, value } => {
                write!(f, "report field `{field}` is `{value}`, signed")
            }
            Self::BarMismatch {
                field,
                bench,
                alarm,
            } => write!(
                f,
                "the bench prints `{field}` as {bench} and this alarm holds {alarm}: the bar \
                 moved on one side alone"
            ),
            Self::PlanMismatch {
                field,
                bench,
                alarm,
            } => write!(
                f,
                "the bench prints `{field}` as {bench} and this alarm holds {alarm}: the plan \
                 moved on one side alone"
            ),
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
        if value.starts_with('+') || value.starts_with('-') {
            return Err(BudgetError::Signed {
                field,
                value: value.to_owned(),
            });
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
    for plan in budget.plans {
        let bench = report.get(plan.field)?;
        if bench != plan.count {
            return Err(BudgetError::PlanMismatch {
                field: plan.field,
                bench,
                alarm: plan.count,
            });
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
    for (measured, planned) in budget.exact {
        let (got, wanted) = (report.get(measured)?, report.get(planned)?);
        if got != wanted {
            return Err(BudgetError::Over {
                field: measured,
                measured: got,
                planned: wanted,
            });
        }
    }
    for bar in budget.bars {
        let bench = report.get(bar.printed)?;
        if bench != bar.gate_ns {
            return Err(BudgetError::BarMismatch {
                field: bar.printed,
                bench,
                alarm: bar.gate_ns,
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

/// The checked run `budget`'s alarm measures: its one released test, by its
/// exact name, through the one door ([`crate::resolved::execute`]). Pure, so
/// its command line is pinned.
pub(crate) fn test_run(budget: &Budget, locked: bool) -> crate::resolved::TestRun {
    crate::resolved::TestRun::new(
        vec!["--package".to_owned(), "pharmakos-bench".to_owned()],
        crate::resolved::Invocation {
            selectors: vec!["--test".to_owned(), budget.target.to_owned()],
            sim: crate::resolved::SimRule::OptimisedIfBuilt,
        },
        locked,
        false,
    )
    .release()
    .with_harness(vec![
        crate::resolved::Harness::Ignored,
        crate::resolved::Harness::Nocapture,
        crate::resolved::Harness::Exact(budget.test.to_owned()),
    ])
}

/// The command's decision on a measurement's output: the one report line
/// read, then its verdict. Pure, so every way to no verdict is pinned where
/// the command decides.
///
/// # Errors
///
/// Every [`parse`] refusal: no line, several, or one that gives no verdict.
pub(crate) fn judge(
    budget: &Budget,
    output: &str,
    os: &str,
) -> Result<(Vec<String>, bool), BudgetError> {
    let report = parse(budget, output)?;
    verdict(budget, &report, os)
}

/// Run `budget`'s measurement through the checked call, echoing its output
/// as it comes, and answer whether the alarm passed.
pub(crate) fn run(
    budget: &Budget,
    root: &Path,
    require_tools: bool,
    locked: bool,
) -> Result<bool, BudgetError> {
    let output = crate::run_measurement(root, require_tools, &test_run(budget, locked))
        .map_err(BudgetError::Run)?;
    let (lines, passed) = judge(budget, &output, std::env::consts::OS)?;
    for line in lines {
        println!("{line}");
    }
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bars as the bench prints them at the end of each line.
    const P3_BARS: &str = " repath_bar_ns=5000000 estimate_bar_ns=1000000";

    const P1_BARS: &str = " quick_bar_ns=5000000 full_bar_ns=50000000 e2e_bar_ns=50000000";

    const P3: &str = concat!(
        "p3-budget-report v2 seeds=3 planned_seeds=3 pairs=2000 planned_pairs=2000 ",
        "repath_p99_ns=900000 estimate_p99_ns=400000",
        " repath_bar_ns=5000000 estimate_bar_ns=1000000"
    );

    const P1: &str = concat!(
        "p1-budget-report v2 budget_units=128 inputs=7 planned_inputs=7 e2e_inputs=6 ",
        "planned_e2e_inputs=6 calls=500 planned_calls=500 quick_p99_ns=3000000 ",
        "full_p99_ns=30000000 e2e_p99_ns=32000000",
        " quick_bar_ns=5000000 full_bar_ns=50000000 e2e_bar_ns=50000000"
    );

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
            Err(BudgetError::NoReport("p3-budget-report v2"))
        );
        assert_eq!(
            parse(&P3_BUDGET, &format!("{P3}\n{P3}")),
            Err(BudgetError::SeveralReports {
                prefix: "p3-budget-report v2",
                count: 2
            })
        );
        // One alarm's line is not the other's.
        assert_eq!(
            parse(&P1_BUDGET, P3),
            Err(BudgetError::NoReport("p1-budget-report v2"))
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
        // A line without the end-to-end verdict is no verdict, and so is one
        // that sent fewer shapes than it planned (one the transport refused).
        assert_eq!(
            parse(&P1_BUDGET, &P1.replacen(" e2e_p99_ns=32000000", "", 1)),
            Err(BudgetError::MissingField("e2e_p99_ns"))
        );
        assert_eq!(
            parse(&P1_BUDGET, &P1.replacen("e2e_inputs=6", "e2e_inputs=5", 1)),
            Err(BudgetError::Short {
                field: "e2e_inputs",
                measured: 5,
                planned: 6
            })
        );
        assert_eq!(
            parse(
                &P1_BUDGET,
                &P1.replacen("e2e_p99_ns=32000000", "e2e_p99_ns=none", 1)
            ),
            Err(BudgetError::BadValue {
                field: "e2e_p99_ns",
                value: "none".to_owned()
            })
        );
        // A count past an exact plan, a zero figure, and a signed value are
        // each no verdict, each under its own reason.
        assert_eq!(
            parse(&P1_BUDGET, &P1.replacen("e2e_inputs=6", "e2e_inputs=7", 1)),
            Err(BudgetError::Over {
                field: "e2e_inputs",
                measured: 7,
                planned: 6
            })
        );
        for field in ["quick_p99_ns", "full_p99_ns", "e2e_p99_ns"] {
            let line = P1
                .split(' ')
                .map(|token| {
                    if token.starts_with(&format!("{field}=")) {
                        format!("{field}=0")
                    } else {
                        token.to_owned()
                    }
                })
                .collect::<Vec<String>>()
                .join(" ");
            assert_eq!(parse(&P1_BUDGET, &line), Err(BudgetError::Zero(field)));
        }
        assert_eq!(
            swap("repath_p99_ns=900000", "repath_p99_ns=0"),
            Err(BudgetError::Zero("repath_p99_ns"))
        );
        assert_eq!(
            swap("estimate_p99_ns=400000", "estimate_p99_ns=0"),
            Err(BudgetError::Zero("estimate_p99_ns"))
        );
        assert_eq!(
            swap("estimate_p99_ns=400000", "estimate_p99_ns=+400000"),
            Err(BudgetError::Signed {
                field: "estimate_p99_ns",
                value: "+400000".to_owned()
            })
        );
        // The must-trip of the one fact: a bar moved on the bench's side
        // alone is no verdict, and so is one moved on this side alone.
        assert_eq!(
            parse(
                &P1_BUDGET,
                &P1.replacen("quick_bar_ns=5000000", "quick_bar_ns=4000000", 1)
            ),
            Err(BudgetError::BarMismatch {
                field: "quick_bar_ns",
                bench: 4_000_000,
                alarm: 5_000_000
            })
        );
        let moved = Budget {
            bars: &[Bar {
                field: "repath_p99_ns",
                printed: "repath_bar_ns",
                what: "repath",
                gate_ns: 6_000_000,
                alarm_ns: 12_000_000,
            }],
            ..P3_BUDGET
        };
        assert_eq!(
            parse(&moved, P3),
            Err(BudgetError::BarMismatch {
                field: "repath_bar_ns",
                bench: 5_000_000,
                alarm: 6_000_000
            })
        );
        // A longer word that merely starts with the prefix is not a report.
        assert_eq!(
            parse(&P3_BUDGET, "p3-budget-report v20 seeds=1"),
            Err(BudgetError::NoReport("p3-budget-report v2"))
        );
    }

    /// B1, the must-trip of each planned count: a plan lowered in the bench
    /// alone (the line prints it, with a measured count to match) is no
    /// verdict, and so is one moved on this side alone. Zero plans are refused
    /// as zero.
    #[test]
    fn a_plan_moved_on_one_side_alone_is_no_verdict() {
        let p1 = |from: &str, to: &str| parse(&P1_BUDGET, &P1.replacen(from, to, 1));
        let p3 = |from: &str, to: &str| parse(&P3_BUDGET, &P3.replacen(from, to, 1));
        assert_eq!(
            p1("calls=500 planned_calls=500", "calls=50 planned_calls=50"),
            Err(BudgetError::PlanMismatch {
                field: "planned_calls",
                bench: 50,
                alarm: 500
            })
        );
        assert_eq!(
            p1("inputs=7 planned_inputs=7", "inputs=1 planned_inputs=1"),
            Err(BudgetError::PlanMismatch {
                field: "planned_inputs",
                bench: 1,
                alarm: 7
            })
        );
        assert_eq!(
            p1(
                "e2e_inputs=6 planned_e2e_inputs=6",
                "e2e_inputs=2 planned_e2e_inputs=2"
            ),
            Err(BudgetError::PlanMismatch {
                field: "planned_e2e_inputs",
                bench: 2,
                alarm: 6
            })
        );
        assert_eq!(
            p3("seeds=3 planned_seeds=3", "seeds=1 planned_seeds=1"),
            Err(BudgetError::PlanMismatch {
                field: "planned_seeds",
                bench: 1,
                alarm: 3
            })
        );
        assert_eq!(
            p3("pairs=2000 planned_pairs=2000", "pairs=8 planned_pairs=8"),
            Err(BudgetError::PlanMismatch {
                field: "planned_pairs",
                bench: 8,
                alarm: 2_000
            })
        );
        assert_eq!(
            p3("planned_seeds=3", "planned_seeds=0"),
            Err(BudgetError::Zero("planned_seeds"))
        );
        // This side's plan moved alone.
        let moved = Budget {
            plans: &[Planned {
                field: "planned_pairs",
                count: 4_000,
            }],
            ..P3_BUDGET
        };
        assert_eq!(
            parse(&moved, P3),
            Err(BudgetError::PlanMismatch {
                field: "planned_pairs",
                bench: 2_000,
                alarm: 4_000
            })
        );
        // Every planned field is held, and is non-zero.
        for budget in BUDGETS {
            for field in budget
                .fields
                .iter()
                .filter(|field| field.starts_with("planned_"))
            {
                assert!(
                    budget.plans.iter().any(|plan| plan.field == *field),
                    "{}: `{field}` has no copy here",
                    budget.command
                );
                assert!(budget.nonzero.contains(field), "{field}");
            }
        }
    }

    /// B4: where the command decides, no report, two reports, a report past
    /// the alarm and one within it each give their own answer, and only the
    /// last is a pass.
    #[test]
    fn the_command_passes_only_a_report_within_its_alarm() {
        assert_eq!(
            judge(&P3_BUDGET, "", "linux"),
            Err(BudgetError::NoReport("p3-budget-report v2"))
        );
        assert_eq!(
            judge(&P3_BUDGET, &format!("{P3}\n{P3}"), "linux"),
            Err(BudgetError::SeveralReports {
                prefix: "p3-budget-report v2",
                count: 2
            })
        );
        let past = P3.replacen("repath_p99_ns=900000", "repath_p99_ns=10000001", 1);
        let (lines, passed) = judge(&P3_BUDGET, &past, "linux").expect("a verdict");
        assert!(!passed);
        assert!(
            lines.iter().any(|line| line.starts_with("::error ")),
            "{lines:?}"
        );
        let (lines, passed) = judge(&P3_BUDGET, P3, "linux").expect("a verdict");
        assert!(passed);
        assert_eq!(lines.len(), 1);
    }

    /// A run cut short of its plan gives no verdict, whichever figure was cut:
    /// the lines the bench prints for a cut plan (its harness tests' own
    /// lines) are refused as short, never read as a pass.
    #[test]
    fn a_cut_run_is_no_verdict() {
        assert_eq!(
            parse(
                &P1_BUDGET,
                &format!(
                    "p1-budget-report v2 budget_units=128 inputs=7 planned_inputs=7 e2e_inputs=6 \
                     planned_e2e_inputs=6 calls=1 planned_calls=500 quick_p99_ns=1 full_p99_ns=1 \
                     e2e_p99_ns=1{P1_BARS}"
                )
            ),
            Err(BudgetError::Short {
                field: "calls",
                measured: 1,
                planned: 500
            })
        );
        assert_eq!(
            parse(
                &P3_BUDGET,
                &format!(
                    "p3-budget-report v2 seeds=1 planned_seeds=3 pairs=8 planned_pairs=2000 \
                     repath_p99_ns=1 estimate_p99_ns=1{P3_BARS}"
                )
            ),
            Err(BudgetError::Short {
                field: "seeds",
                measured: 1,
                planned: 3
            })
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
                "p3-budget-report v2 seeds=3 planned_seeds=3 pairs=2000 planned_pairs=2000 \
                 repath_p99_ns={repath} estimate_p99_ns={estimate}{P3_BARS}"
            )
        };
        assert!(at(&P3_BUDGET, &p3(10_000_000, 2_000_000)).1);
        // B5: exactly at the gate is within it; a nanosecond past is over.
        let (lines, _) = at(&P3_BUDGET, &p3(5_000_000, 1_000_000));
        assert!(
            lines.iter().any(|line| line
                .contains("for reference only, within the gate's 5.000 ms")
                && line.contains("for reference only, within the gate's 1.000 ms")),
            "{lines:?}"
        );
        let (lines, _) = at(&P3_BUDGET, &p3(5_000_001, 1_000_001));
        assert!(
            lines.iter().any(
                |line| line.contains("for reference only, over the gate's 5.000 ms")
                    && line.contains("for reference only, over the gate's 1.000 ms")
            ),
            "{lines:?}"
        );
        assert!(!at(&P3_BUDGET, &p3(10_000_001, 1)).1);
        assert!(!at(&P3_BUDGET, &p3(1, 2_000_001)).1);
        // Over the gate but under the alarm: a notice, never a failure.
        let (lines, passed) = at(&P3_BUDGET, &p3(5_000_001, 1_000_001));
        assert!(passed);
        assert_eq!(lines.len(), 1);
        assert!(lines.iter().all(|line| line.starts_with("::notice ")));
        let p1 = |quick: u128, full: u128, e2e: u128| {
            format!(
                "p1-budget-report v2 budget_units=128 inputs=7 planned_inputs=7 e2e_inputs=6 \
                 planned_e2e_inputs=6 calls=500 planned_calls=500 quick_p99_ns={quick} \
                 full_p99_ns={full} e2e_p99_ns={e2e}{P1_BARS}"
            )
        };
        assert!(at(&P1_BUDGET, &p1(10_000_000, 100_000_000, 100_000_000)).1);
        let (lines, passed) = at(&P1_BUDGET, &p1(10_000_001, 100_000_001, 100_000_001));
        assert!(!passed);
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with("::error "))
                .count(),
            3
        );
        // The end-to-end bar alone, doubled and passed, fails the alarm.
        let (lines, passed) = at(&P1_BUDGET, &p1(1, 1, 100_000_001));
        assert!(!passed);
        assert!(
            lines.iter().any(|line| line.starts_with(
                "::error title=P1 budget::end-to-end verify_plan p99 at the size budget 100.000 ms"
            )),
            "{lines:?}"
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
                 reference only, within the gate's 50.000 ms); end-to-end verify_plan p99 at the \
                 size budget 32.000 ms (the alarm fails over 100.000 ms; for reference only, \
                 within the gate's 50.000 ms). The alarm fails at the gate's numbers doubled; the \
                 gate is certified on the owner's machine, never here"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn each_alarm_runs_one_released_test_by_its_exact_name() {
        let p3 = test_run(&P3_BUDGET, true).run_line().join(" ");
        assert!(
            p3.contains(
                "test --release --package pharmakos-bench --test p3_alarm --locked \
                 --message-format=json -- --ignored --nocapture --exact p3_budget"
            ),
            "{p3}"
        );
        let p1 = test_run(&P1_BUDGET, false).run_line().join(" ");
        assert!(
            p1.contains(
                "test --release --package pharmakos-bench --test perf_alarm \
                 --message-format=json -- --ignored --nocapture --exact p1_budget"
            ),
            "{p1}"
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
