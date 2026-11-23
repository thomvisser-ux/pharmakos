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
//!
//! A file past the size cap (`pharmakos_verifier::MAX_PLAYBOOK_CHARS`) is the
//! same answer as one that does not parse: `E0029`, `Failed`. It is read no
//! further than the cap needs, so a huge file is refused without being held in
//! memory.

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;

use crate::cli::Format;
use crate::exit::{Exit, Failure};
use crate::rules;
use crate::seat;
use crate::strings;

/// Which seat view a playbook is verified against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    /// The reference seat view ([`crate::seat`]), the default.
    Reference,
    /// The opening Lull of the match the lobby opens for this preset
    /// (`--preset`; [`crate::lobby`]).
    Preset(Preset),
}

/// A preset `--preset` names: the lobby's match of that preset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    /// The newcomer's first match (spec section 3).
    Probation,
}

impl Preset {
    /// Every preset `--preset` accepts, as the command line spells them.
    pub const NAMES: &[&str] = &["probation"];

    /// The preset a command line names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Preset> {
        match name {
            "probation" => Some(Preset::Probation),
            _ => None,
        }
    }

    /// How a report's heading and footer name it.
    #[must_use]
    pub const fn shown(self) -> &'static str {
        match self {
            Preset::Probation => "Probation",
        }
    }
}

/// Inspect one playbook file and render its report.
///
/// # Errors
///
/// [`Exit::Input`] for a file that is not there, [`Exit::Failed`] for a
/// playbook that does not qualify, [`Exit::Internal`] when the rules table is
/// missing a block the checks read, or, under a preset, when the lobby's match
/// will not open or the gateway's answer is not a report.
pub fn run(
    root: &Path,
    path: &Path,
    depth: Depth,
    view: View,
    format: Format,
) -> Result<String, Failure> {
    let display = crate::display(path);
    let text = read_bounded(&root.join(path)).map_err(|error| {
        Failure::input(strings::unreadable(
            "playbook",
            &display,
            &error.to_string(),
        ))
    })?;
    let (report, shown) = match view {
        View::Reference => (
            reference_report(root, text.as_deref(), depth)?,
            Shown::Reference,
        ),
        View::Preset(preset) => {
            let config = crate::lobby::probation()?;
            let seat = crate::lobby::seat_of(&config)?;
            let report = crate::lobby::verify(root, text.as_deref(), depth)?;
            (
                report,
                Shown::Preset {
                    preset,
                    seed: config.seed,
                    seats: config.seats,
                    seat,
                },
            )
        }
    };

    match format {
        Format::Text => {
            let rendered = render_against(&display, &report, &shown)?;
            if report.qualifies {
                Ok(rendered)
            } else {
                Err(Failure::new(Exit::Failed, rendered))
            }
        }
        // The report as data, on standard output whatever the answer: exit
        // 3 still says it does not qualify, and standard error stays empty.
        Format::Json => {
            let json = json_of(&report)?;
            if report.qualifies {
                Ok(json)
            } else {
                Err(Failure::answered(Exit::Failed, json))
            }
        }
    }
}

/// The report as the verifier's own JSON: canonical `gp.api.v1.VerifyReport`,
/// laid out as the verifier's goldens are (`pharmakos_proto::json::encode`),
/// with every character the human report escapes
/// ([`pharmakos_proto::json::grammar::HIDDEN`]: a control, a direction mark
/// such as U+202E, an invisible) written as a JSON `\uXXXX` escape. The two
/// are the same document to a JSON reader, and the escaped one cannot drive or
/// reorder a terminal it is printed on.
///
/// # Errors
///
/// [`Exit::Internal`] when the report does not encode, which would be this
/// build's report and its own schema disagreeing.
pub fn json_of(report: &VerifyReport) -> Result<String, Failure> {
    let text = pharmakos_proto::json::encode(report).map_err(|error| {
        Failure::internal(format!(
            "this build's report does not encode as JSON: {error}"
        ))
    })?;
    Ok(escape_hidden(&text))
}

/// `json`, a document `pharmakos_proto::json` wrote, with every
/// [`pharmakos_proto::json::grammar::HIDDEN`] character written as `\uXXXX`,
/// a UTF-16 unit at a time, except the layout's own line feeds, carriage
/// returns and tabs.
///
/// Exact on such a document. Outside its strings it is punctuation, digits,
/// the literals and the layout's whitespace; inside them the writer has
/// already escaped every C0 control, so a raw line feed, carriage return or
/// tab is always layout, and every other hidden character stands inside a
/// string, where the escape reads back as the same character.
#[must_use]
pub fn escape_hidden(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for character in json.chars() {
        if is_hidden(character) && !matches!(character, '\n' | '\r' | '\t') {
            for unit in character.encode_utf16(&mut [0; 2]) {
                let _ = write!(out, "\\u{unit:04x}");
            }
        } else {
            out.push(character);
        }
    }
    out
}

fn is_hidden(character: char) -> bool {
    pharmakos_proto::json::grammar::HIDDEN
        .iter()
        .any(|(low, high)| (*low..=*high).contains(&character))
}

/// The report against the reference seat view.
fn reference_report(
    root: &Path,
    text: Option<&str>,
    depth: Depth,
) -> Result<VerifyReport, Failure> {
    let rules = rules::load(root)?;
    let snapshot = seat::snapshot().map_err(|error| {
        Failure::internal(format!(
            "this build's snapshot encoder refused its own reference snapshot: {error}"
        ))
    })?;
    let scope = seat::scope();
    let gap = |error: &dyn std::fmt::Display| {
        Failure::internal(strings::rules_gap(rules::RULES_PATH, &error.to_string()))
    };
    if let Some(text) = text {
        pharmakos_plan_core::verify_jsonc(text, &snapshot, &scope, &rules, depth)
            .map_err(|error| gap(&error))
    } else {
        // Past the cap in bytes, so past it whatever the bytes hold: `E0029`,
        // and the rest of the file is never read.
        let input = pharmakos_verifier::Input::too_long(&snapshot, &scope, &rules)
            .map_err(|error| gap(&error))?;
        Ok(pharmakos_verifier::verify(&input, depth))
    }
}

/// The view a report was verified against, as its heading and footer say it.
enum Shown {
    Reference,
    Preset {
        preset: Preset,
        seed: u64,
        seats: u32,
        seat: u8,
    },
}

/// The file's text, read no further than the size cap needs (S3's `vfy6`):
/// `None` for a file past [`pharmakos_verifier::MAX_PLAYBOOK_READ`] bytes,
/// which is past [`pharmakos_verifier::MAX_PLAYBOOK_CHARS`] whatever it holds,
/// having read one byte past that and no more. A shorter file past the cap in
/// characters is `plan-core`'s to refuse, and is refused before it is
/// canonicalised.
///
/// # Errors
///
/// The file cannot be opened or read, or is not UTF-8.
pub(crate) fn read_bounded(path: &Path) -> std::io::Result<Option<String>> {
    let most = u64::try_from(pharmakos_verifier::MAX_PLAYBOOK_READ)
        .ok()
        .and_then(|most| most.checked_add(1))
        .ok_or_else(|| std::io::Error::other("the size cap does not fit a file length"))?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(most)
        .read_to_end(&mut bytes)?;
    if pharmakos_verifier::is_too_long_in_bytes(bytes.len()) {
        return Ok(None);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// The report as a person reads it, against the reference seat view.
///
/// Public because the shape of this output is the thing `tests/verify.rs`
/// reads `report_hash` out of.
///
/// # Errors
///
/// [`Exit::Internal`] for a report whose depth or a diagnostic's severity the
/// schema does not name.
pub fn render(display: &str, report: &VerifyReport) -> Result<String, Failure> {
    render_against(display, report, &Shown::Reference)
}

fn render_against(display: &str, report: &VerifyReport, shown: &Shown) -> Result<String, Failure> {
    let view = match shown {
        Shown::Reference => strings::verify_view_reference(),
        Shown::Preset {
            preset, seed, seat, ..
        } => strings::verify_view_preset(preset.shown(), *seed, *seat),
    };
    let mut text = strings::verify_heading(display, depth_name(report.depth)?, &view);
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
                severity_name(diagnostic.severity)?,
                &diagnostic.code,
                &diagnostic.path,
                &diagnostic.message,
            )
        );
    }
    // E19: an `E0403` against the reference view says that a match may
    // accept the beacon, from the report's codes alone. A preset's report is
    // the match's own, so it needs no such sentence.
    let reference_e0403 = matches!(shown, Shown::Reference)
        && report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0403");
    if reference_e0403 {
        let _ = writeln!(text, "{}", strings::verify_e0403_note());
    }
    let footer = match shown {
        Shown::Reference => strings::verify_footer(),
        Shown::Preset {
            preset,
            seed,
            seats,
            seat,
        } => strings::verify_footer_preset(preset.shown(), *seed, *seats, *seat),
    };
    let _ = writeln!(text, "{footer}");
    Ok(text)
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

/// How the heading names a report's depth.
///
/// # Errors
///
/// [`Exit::Internal`] for a depth the report's schema does not name, or one
/// left unspecified: every report the verifier makes says which depth made
/// it, so either is the wire's shape having moved (S3's plan, E22; the
/// earlier version printed every such report as `FULL`).
fn depth_name(depth: i32) -> Result<&'static str, Failure> {
    match Depth::try_from(depth) {
        Ok(Depth::Quick) => Ok("QUICK"),
        Ok(Depth::Full) => Ok("FULL"),
        Ok(Depth::Unspecified) | Err(_) => Err(Failure::internal(format!(
            "a report whose depth is {depth}, which is neither QUICK nor FULL"
        ))),
    }
}

/// How a diagnostic's line names its severity.
///
/// # Errors
///
/// [`Exit::Internal`] for a severity the schema does not name, or one left
/// unspecified: the wire's shape having moved, refused rather than printed as
/// `?` (S3's plan, E22).
fn severity_name(severity: i32) -> Result<&'static str, Failure> {
    match Severity::try_from(severity) {
        Ok(Severity::Error) => Ok("error"),
        Ok(Severity::Warning) => Ok("warning"),
        Ok(Severity::Info) => Ok("info"),
        Ok(Severity::Unspecified) | Err(_) => Err(Failure::internal(format!(
            "a diagnostic whose severity is {severity}, which is none of error, warning and info"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{depth_name, render, severity_name};
    use crate::exit::Exit;
    use pharmakos_proto::gp::api::v1::diagnostic::Severity;
    use pharmakos_proto::gp::api::v1::verify_plan::Depth;
    use pharmakos_proto::gp::api::v1::{Diagnostic, VerifyReport};

    /// E22: a depth or a severity the schema does not name, or left
    /// unspecified, is refused rather than printed as `FULL` or `?`.
    #[test]
    fn an_unnamed_depth_or_severity_is_refused() {
        assert_eq!(depth_name(i32::from(Depth::Quick)), Ok("QUICK"));
        assert_eq!(depth_name(i32::from(Depth::Full)), Ok("FULL"));
        for depth in [i32::from(Depth::Unspecified), 99] {
            let refused = depth_name(depth).expect_err("refused");
            assert_eq!(refused.code, Exit::Internal);
        }
        assert_eq!(severity_name(i32::from(Severity::Error)), Ok("error"));
        assert_eq!(severity_name(i32::from(Severity::Warning)), Ok("warning"));
        assert_eq!(severity_name(i32::from(Severity::Info)), Ok("info"));
        for severity in [i32::from(Severity::Unspecified), -1, 99] {
            let refused = severity_name(severity).expect_err("refused");
            assert_eq!(refused.code, Exit::Internal);
        }
        let report = VerifyReport {
            depth: i32::from(Depth::Full),
            diagnostics: vec![Diagnostic {
                code: String::from("E0001"),
                severity: 99,
                ..Diagnostic::default()
            }],
            ..VerifyReport::default()
        };
        let refused = render("a.jsonc", &report).expect_err("a severity of 99 is refused");
        assert!(refused.message.contains("99"), "{}", refused.message);
        let unspecified = VerifyReport::default();
        assert!(
            render("a.jsonc", &unspecified).is_err(),
            "a report with no depth"
        );
    }
}
