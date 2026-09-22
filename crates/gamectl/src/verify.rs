// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl verify <playbook.jsonc>` — seal inspection from a shell.
//!
//! The pipeline is exactly the editor's and exactly `submit_plan`'s: the file's
//! comments come off in `plan-core`, which decides *which bytes* the verifier
//! hashes, and `pharmakos_verifier::verify` does the rest
//! (`pharmakos_plan_core::verify_jsonc`). Nothing here re-implements a check,
//! re-words a diagnostic or counts a size unit: the report this prints is the
//! one a client gets back over the wire, rendered.
//!
//! What it is verified *against* is [`crate::seat`] — the reference seat view,
//! which is that module's whole subject. The footer says so in the output,
//! because a report is a claim about a snapshot and a reader who does not know
//! which one cannot use it.
//!
//! # The exit code is the answer
//!
//! A playbook that does not qualify exits [`Exit::Failed`], not
//! [`Exit::Internal`]: the tool did what was asked and the news is bad. A
//! playbook that does not *parse* is the same answer — `E0001` is a diagnostic,
//! not a crash, which is the difference between a verifier and a parser — so it
//! is `Failed` too, and only a missing or unreadable **file** is
//! [`Exit::Input`].

use std::fmt::Write as _;
use std::path::Path;

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;

use crate::exit::{Exit, Failure};
use crate::rules;
use crate::seat;
use crate::strings;

/// Inspect one playbook file and render its report.
///
/// # Errors
///
/// [`Exit::Input`] for a file that is not there, [`Exit::Failed`] for a
/// playbook that does not qualify, [`Exit::Internal`] when the rules table is
/// missing a block the checks read.
pub fn run(root: &Path, path: &Path, depth: Depth) -> Result<String, Failure> {
    let rules = rules::load(root)?;
    let display = crate::display(path);
    let text = std::fs::read_to_string(root.join(path)).map_err(|error| {
        Failure::input(strings::unreadable(
            "playbook",
            &display,
            &error.to_string(),
        ))
    })?;

    let snapshot = seat::snapshot().map_err(|error| {
        Failure::internal(format!(
            "this build's snapshot encoder refused its own reference snapshot: {error}"
        ))
    })?;
    let scope = seat::scope();
    let report = pharmakos_plan_core::verify_jsonc(&text, &snapshot, &scope, &rules, depth)
        .map_err(|error| {
            Failure::internal(strings::rules_gap(rules::RULES_PATH, &error.to_string()))
        })?;

    let rendered = render(&display, &report);
    if report.qualifies {
        Ok(rendered)
    } else {
        Err(Failure::new(Exit::Failed, rendered))
    }
}

/// The report as a person reads it.
///
/// Public because `seat doctor` renders one too, and because the shape of this
/// output is the thing `tests/verify.rs` reads `report_hash` out of.
#[must_use]
pub fn render(display: &str, report: &VerifyReport) -> String {
    let mut text = strings::verify_heading(display, depth_name(report.depth));
    text.push('\n');
    let errors = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == i32::from(Severity::Error))
        .count();
    let _ = writeln!(
        text,
        "{}",
        if report.qualifies {
            strings::verify_qualifies(report.size_units, report.size_budget)
        } else {
            strings::verify_refuses(errors)
        }
    );
    let _ = writeln!(
        text,
        "{}",
        strings::verify_report_hash(&hex(&report.report_hash))
    );
    for diagnostic in &report.diagnostics {
        let _ = writeln!(
            text,
            "{}",
            strings::verify_diagnostic(
                severity_name(diagnostic.severity),
                &diagnostic.code,
                &diagnostic.path,
                &diagnostic.message,
            )
        );
    }
    let _ = writeln!(text, "{}", strings::verify_footer());
    text
}

/// `report_hash` and `rules_hash` are bytes on the wire; a person reads hex.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn depth_name(depth: i32) -> &'static str {
    if depth == i32::from(Depth::Quick) {
        "QUICK"
    } else {
        "FULL"
    }
}

fn severity_name(severity: i32) -> &'static str {
    if severity == i32::from(Severity::Error) {
        "error"
    } else if severity == i32::from(Severity::Warning) {
        "warning"
    } else if severity == i32::from(Severity::Info) {
        "info"
    } else {
        "?"
    }
}
