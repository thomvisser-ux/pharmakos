// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl seat doctor` — check this installation and report what is wrong.
//!
//! Spec §12 lists it beside `verify`, `schema`, `docs` and `scenarios`; the
//! skeleton plan's **T21** acceptance line is the one that says what it is for:
//! "`gamectl seat doctor` passes from inside the extracted zip". It is the
//! command a tester runs before writing a bug report, and its job is to turn
//! "it doesn't work" into a line naming which of six things is missing.
//!
//! # Every check is the real thing
//!
//! A doctor that asserted a file existed would pass on a truncated one. So each
//! check below *uses* what it is checking: the rules table is loaded and its
//! hash printed, the descriptor set is walked, the verifier verifies a
//! playbook, `plan-core` round-trips one, and the last check **opens a match
//! behind a `Surface` and steps it**. That last one is slow — a map is
//! generated and a search graph is built — and it is the only check that proves
//! this machine can host the thing the game is.
//!
//! Behind a `Surface` and not beside it, for the reason
//! `crate::scenario::run`'s header gives at length: there is one way to play a
//! match in this crate, and a check that drove the `Host` on its own would be a
//! second one.
//!
//! The loopback check is the one that is allowed to be uncertain. The gateway
//! binds `127.0.0.1` and `::1` and nothing else (AGENTS.md §7), and a host with
//! IPv6 switched off can still play a match — so a `::1` that will not bind is
//! `warn`, not `FAIL`, and the line says which half answered.

use std::fmt::Write as _;
use std::path::Path;

use pharmakos_proto::gp::api::v1::verify_plan::Depth;

use crate::exit::{Exit, Failure};
use crate::rules;
use crate::seat;
use crate::strings;

/// How one check came out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Ok,
    Warn,
    Fail,
}

impl State {
    const fn label(self) -> &'static str {
        match self {
            State::Ok => "ok",
            State::Warn => "warn",
            State::Fail => "FAIL",
        }
    }
}

/// The seed the match check hosts on.
///
/// Any seed generates a map; this one is the scenarios' own, so a doctor run
/// and a scenario run are looking at the same world and a map bug shows up in
/// both.
const CHECK_SEED: u64 = 0x0000_0000_ca5c_aded;

/// A segment one tick long. The check is "can this machine host and step a
/// match", and one tick answers it; the expensive part is the map and the
/// search graph, which `Host::open` builds either way.
///
/// Read from the sim rather than typed: "one tick long" is only true while the
/// tick is fifty milliseconds, and a literal here would have been a second
/// spelling of a sim constant this crate already reads in
/// [`crate::scenario::ticks_of`].
const CHECK_SEGMENT_MS: i32 = pharmakos_sim::math::quantity::MS_PER_TICK;

/// The match id the check hosts under. Lower-case letters and hyphens only,
/// because `MatchCache::valid_match_id` says so.
const CHECK_MATCH: &str = "m-doctor";

/// Run every check.
///
/// # Errors
///
/// [`Exit::Failed`] when a check failed — the answer, not an error. Nothing
/// here returns [`Exit::Internal`]: a doctor that could not finish reporting is
/// reporting.
pub fn run(root: &Path) -> Result<String, Failure> {
    let mut lines: Vec<(State, String)> = Vec::new();
    let rules = check_rules(root, &mut lines);
    check_schema(&mut lines);
    check_verifier(rules.as_ref(), &mut lines);
    check_plan_core(rules.as_ref(), &mut lines);
    check_loopback(&mut lines);
    check_match(rules.as_ref(), &mut lines);

    let failed = lines
        .iter()
        .filter(|(state, _)| *state == State::Fail)
        .count();
    let mut text = String::new();
    for (_, line) in &lines {
        let _ = writeln!(text, "{line}");
    }
    // The reference seat view, printed rather than made a check of its own:
    // nothing about it can fail, and a `seat doctor` line that can only ever
    // say `ok` is noise. It is here because `gamectl verify`'s own footer says
    // "`gamectl seat doctor` prints it", and a tool that points at a thing it
    // does not print is a tool telling the reader to guess.
    let _ = writeln!(text, "{}", strings::doctor_reference_heading());
    for line in reference_view() {
        let _ = writeln!(text, "      {line}");
    }
    let _ = writeln!(
        text,
        "{}",
        if failed == 0 {
            strings::doctor_ok(lines.len())
        } else {
            strings::doctor_failed(failed, lines.len())
        }
    );
    if failed == 0 {
        Ok(text)
    } else {
        Err(Failure::new(Exit::Failed, text))
    }
}

fn note(lines: &mut Vec<(State, String)>, state: State, name: &str, detail: &str) {
    lines.push((state, strings::doctor_line(state.label(), name, detail)));
}

fn check_rules(
    root: &Path,
    lines: &mut Vec<(State, String)>,
) -> Option<pharmakos_sim::rules::RulesTable> {
    match rules::load(root) {
        Ok(table) => {
            let hash = crate::verify::hex(&table.rules_hash().to_be_bytes());
            note(
                lines,
                State::Ok,
                "rules table",
                &format!("{} loaded, rules_hash {hash}", rules::RULES_PATH),
            );
            Some(table)
        }
        Err(failure) => {
            note(lines, State::Fail, "rules table", &failure.message);
            None
        }
    }
}

fn check_schema(lines: &mut Vec<(State, String)>) {
    let schema = pharmakos_proto::descriptor::schema();
    let messages = schema
        .messages()
        .filter(|message| message.full_name.starts_with("gp.v1."))
        .count();
    match pharmakos_gateway::schema::text("") {
        Ok(text) if messages > 0 => note(
            lines,
            State::Ok,
            "schema",
            &format!(
                "{messages} gp.v1 messages, {} bytes of generated JSON Schema",
                text.len()
            ),
        ),
        Ok(_) => note(
            lines,
            State::Fail,
            "schema",
            "the checked-in descriptor set carries no gp.v1 messages",
        ),
        Err(error) => note(lines, State::Fail, "schema", &error.message),
    }
}

fn check_verifier(
    table: Option<&pharmakos_sim::rules::RulesTable>,
    lines: &mut Vec<(State, String)>,
) {
    let Some(table) = table else {
        note(
            lines,
            State::Fail,
            "verifier",
            "not checked: there is no rules table to check it against",
        );
        return;
    };
    let Ok(snapshot) = seat::snapshot() else {
        note(
            lines,
            State::Fail,
            "verifier",
            "this build's snapshot encoder refused its own reference snapshot",
        );
        return;
    };
    let scope = seat::scope();
    match pharmakos_plan_core::verify_jsonc(
        pharmakos_gateway::host::SAFE_PLAYBOOK,
        &snapshot,
        &scope,
        table,
        Depth::Full,
    ) {
        Ok(report) if report.qualifies => note(
            lines,
            State::Ok,
            "verifier",
            &format!(
                "{}, {} diagnostic codes, the safe playbook qualifies at {} of {} size units",
                pharmakos_verifier::VERIFIER_VERSION,
                pharmakos_verifier::catalogue::CATALOGUE.len(),
                report.size_units,
                report.size_budget
            ),
        ),
        Ok(report) => note(
            lines,
            State::Fail,
            "verifier",
            &format!(
                "the safe playbook does not qualify against this build ({} diagnostics), so a \
                 seat that let its Lull run out would have nothing to play",
                report.diagnostics.len()
            ),
        ),
        Err(error) => note(lines, State::Fail, "verifier", &error.to_string()),
    }
}

fn check_plan_core(
    table: Option<&pharmakos_sim::rules::RulesTable>,
    lines: &mut Vec<(State, String)>,
) {
    let _ = table;
    let safe = pharmakos_gateway::host::SAFE_PLAYBOOK;
    match pharmakos_plan_core::canonicalise_text(safe) {
        Ok(canonical) => {
            // The round trip, not the parse: `plan-core`'s promise is that a
            // file's comments survive it (spec §10), and re-canonicalising the
            // canonical form has to be a fixed point or the editor's Save
            // would rewrite a file it had just written.
            let again = pharmakos_plan_core::canonicalise_text(&canonical.text)
                .is_ok_and(|second| second.text == canonical.text);
            if again {
                note(
                    lines,
                    State::Ok,
                    "plan-core",
                    &format!(
                        "JSONC round trip is a fixed point, {} comments kept, {} canonical bytes",
                        pharmakos_plan_core::Document::parse(safe)
                            .map_or(0, |document| document.comments().len()),
                        canonical.json.len()
                    ),
                );
            } else {
                note(
                    lines,
                    State::Fail,
                    "plan-core",
                    "canonicalising the canonical form changed it, so a file saved twice is two \
                     different files",
                );
            }
        }
        Err(error) => note(lines, State::Fail, "plan-core", &error.to_string()),
    }
}

fn check_loopback(lines: &mut Vec<(State, String)>) {
    // The gateway binds 127.0.0.1 and ::1 and nothing else, with no flag that
    // widens it (AGENTS.md §7). This asks the operating system whether those
    // two are available here; it binds port 0, so nothing is held.
    let four = std::net::TcpListener::bind(("127.0.0.1", 0));
    let six = std::net::TcpListener::bind(("::1", 0));
    match (four, six) {
        (Ok(_), Ok(_)) => note(
            lines,
            State::Ok,
            "loopback",
            "127.0.0.1 and ::1 both accept a bind; the gateway binds these and nothing else",
        ),
        (Ok(_), Err(error)) => note(
            lines,
            State::Warn,
            "loopback",
            &format!(
                "127.0.0.1 accepts a bind and ::1 does not ({error}). A match plays over either, \
                 so this is worth knowing rather than fixing"
            ),
        ),
        (Err(error), _) => note(
            lines,
            State::Fail,
            "loopback",
            &format!(
                "127.0.0.1 will not accept a bind ({error}), so no client on this machine can \
                 reach a seat"
            ),
        ),
    }
}

fn check_match(table: Option<&pharmakos_sim::rules::RulesTable>, lines: &mut Vec<(State, String)>) {
    let Some(table) = table else {
        note(
            lines,
            State::Fail,
            "host a match",
            "not checked: there is no rules table to generate a map from",
        );
        return;
    };
    let host = pharmakos_gateway::host::Host::open(
        &pharmakos_sim::world::WorldConfig {
            match_seed: CHECK_SEED,
            seats: 2,
            units_per_seat: 0,
            rules: table.clone(),
            match_settings: pharmakos_sim::runner::MatchSettings {
                segment_lengths_ms: vec![CHECK_SEGMENT_MS],
                round_limit: pharmakos_sim::runner::DEFAULT_ROUND_LIMIT,
            },
        },
        None,
    );
    let host = match host {
        Ok(host) => host,
        Err(error) => {
            note(lines, State::Fail, "host a match", &error.message);
            return;
        }
    };

    // Behind a `Surface`, and not by driving the `Host` directly. Decisions-log
    // item 106 (3) defines the client path as "a match hosted by the gateway's
    // `Host` **behind a `Surface`**", and a review found this check going
    // around it — a second way to play a match inside the crate whose runner
    // module says there must not be one. Going through the surface also makes
    // the check strictly stronger: `Surface::begin_push` files the safe
    // playbook for every seat that sealed nothing and compiles it, so a
    // machine whose safe playbook will not compile now fails here rather than
    // at the first Lull of a real match.
    let mut surface = match surface_for(table, host) {
        Ok(surface) => surface,
        Err(detail) => {
            note(lines, State::Fail, "host a match", &detail);
            return;
        }
    };
    match surface.begin_push() {
        Ok(true) => {}
        Ok(false) => {
            note(
                lines,
                State::Fail,
                "host a match",
                "a match that has just opened would not leave its Lull",
            );
            return;
        }
        Err(error) => {
            note(lines, State::Fail, "host a match", &error.message);
            return;
        }
    }
    match surface.step() {
        Ok(Some(report)) => note(
            lines,
            State::Ok,
            "host a match",
            &format!(
                "seed {CHECK_SEED:#018x} generated, safe playbooks filed, stepped to tick {} \
                 with state hash {}",
                report.tick.raw(),
                pharmakos_sim::hex(report.hash)
            ),
        ),
        Ok(None) => note(
            lines,
            State::Fail,
            "host a match",
            "a Push that had just opened produced no tick",
        ),
        Err(error) => note(lines, State::Fail, "host a match", &error.message),
    }
}

/// The surface the match check drives, in its opening Lull.
///
/// `Err` carries the line the check prints, because everything that can go
/// wrong here is something a report has to name rather than something to
/// unwrap.
fn surface_for(
    table: &pharmakos_sim::rules::RulesTable,
    host: pharmakos_gateway::host::Host,
) -> Result<pharmakos_gateway::surface::Surface, String> {
    let lull = table
        .message()
        .r#match
        .as_ref()
        .map(|block| block.lull_ms)
        .ok_or_else(|| {
            String::from(
                "the rules table carries no `match` block, so nothing says how long a Lull is",
            )
        })?;
    let seats = [
        pharmakos_sim::tables::SeatId::new(0),
        pharmakos_sim::tables::SeatId::new(1),
    ];
    let mut surface = pharmakos_gateway::surface::Surface::new(
        CHECK_MATCH,
        CHECK_SEED,
        table.clone(),
        pharmakos_gateway::fog::FogPolicy::fogged(),
        &seats,
    )
    .map_err(|error| error.message)?;
    surface.attach(host).map_err(|error| error.message)?;
    surface.set_phase_remaining_ms(pharmakos_sim::math::quantity::Ms::new(lull));
    surface.open_lull().map_err(|error| error.message)?;
    Ok(surface)
}

/// The reference seat view `verify` answers about, printed so that a person
/// reading a report can see what it was a report about.
#[must_use]
pub fn reference_view() -> Vec<String> {
    seat::described()
}
