// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl host`: serve one match to the Godot client over this process's own
//! stdio pipe.
//!
//! Decisions-log item 107 (2), confirmed by the owner as item 108 (2): the
//! Godot lobby spawns this command beside its executable, writes **one** config
//! line on its standard input, reads **one** announce line (the port, the match
//! id, the admin token and the human seat's token) from its standard output,
//! keeps standard input open for the life of the match and closes it on quit.
//! Every line of that protocol, and all of the hosting, is
//! `pharmakos_gateway::serve`; this file is the thirty lines the plan names as
//! run 2's place in this crate (skeleton plan T16, amended by
//! `docs/design/skeleton-plan-t16a-notes.md` section A (3)).
//!
//! # What it does, in order
//!
//! 1. **Refuses to run with a terminal on standard output.** The announce line
//!    carries two bearer tokens, and a token printed to a console is a token in
//!    a scrollback buffer, a screen share and a terminal log. A pipe is the only
//!    reader this command writes for.
//! 2. Reads the rules table to **text** — the gateway parses it, so this crate
//!    names no `pharmakos-sim` type here (item 107 (11)).
//! 3. Hands text, the template library's folder ([`LIBRARY_PATH`] under
//!    `--root`) and the built-in operator's factory ([`EasyOperators`], over
//!    the same rules text) to `serve::run`, with the process's own standard
//!    input and standard output. It returns when standard input reaches end
//!    of file, which is the whole of the shutdown protocol: no heartbeat and
//!    no clock.
//!
//! # The operator's adapters (T18)
//!
//! `pharmakos-operator` depends on `pharmakos-proto` alone and is driven
//! through a call closure, `FnMut(&str, Json) -> Json` (decisions-log item
//! 111, decision C4). The gateway's two seams hand a closure over its own
//! `Request` instead. The adapters below are the whole of the bridge: they
//! wrap a method name and its params into a `Request` and hand the answer
//! back untouched, and they copy the operator's advice into the gateway's
//! plain types field for field. Nothing here decides anything.
//!
//! [`EasyOperators`] is the factory `serve::run` asks once the config line is
//! read: an Easy **built-in seat** for every seat that is not the human's
//! (plan, submit, `set_ready`), and an Easy **advisor** for every other seat
//! (its safe playbook and one suggestion per template). Each is a fresh
//! `Easy` built from the rules text, so no seat shares state with another.
//!
//! A built-in seat's round says on standard error what the gateway refused it
//! (S3's `opr3`, fix pass 2, item 5): the refusal that kept the round from
//! being read, every route estimate refused for another reason than
//! `NOT_FOUND`, each of which dropped the goal it weighed, and why last
//! round's failed stones were not all known when they were not
//! (`Played::failed_stones`). One line a round,
//! and none for a round with nothing refused ([`refusals_said`]). A line
//! standard error would not take is not dropped silently: it is counted by
//! its reason ([`Unsaid`]), and the next line that is written says how many
//! went unwritten before it. Standard error is the only door this command
//! has besides the announce line: the host's other counters (the dropped
//! connections) are the gateway's, which `serve.rs` writes into the audit log
//! it closes, and no seam takes a count of this crate's there. Returning the
//! error would not help: the gateway's caller of a built-in seat cannot act
//! on a full pipe.
//!
//! # Exit codes
//!
//! The table in [`crate::exit`], and nothing new:
//!
//! | code | when |
//! |---|---|
//! | 0 | the parent closed standard input and the match was put down |
//! | 1 | standard output is a terminal, so nothing was started |
//! | 2 | the rules table could not be read, the rules table is one the verifier refuses (S3's `vfyq2`, fix pass 5, item L8), or the config line was one this host refuses |
//! | 4 | the host could not start: no loopback bind, no entropy for a token, no private match cache |
//!
//! Standard output belongs to the announce line alone. Everything else this
//! command has to say goes to standard error, which is where the lobby's
//! "host failed to start" message reads it from.
//!
//! The rules table and the template library are both read from `--root`, as
//! every other command reads the rules. Beside a shipped binary they are the
//! zip's `rules/` and `library/`, laid out like the repository's, and the
//! exported lobby passes the executable's folder as `--root` (decisions-log
//! item 117 (3)).

use std::io::{IsTerminal as _, Write};
use std::path::Path;

use pharmakos_gateway::Code;
use pharmakos_gateway::advice::{Advice, StartHere, SuggestedValue, Suggestion};
use pharmakos_gateway::rpc::Request;
use pharmakos_gateway::serve::{self, Advisor, BuiltInSeat, Operators, Setup};
use pharmakos_operator::{Easy, FailedStones, Played, RulesError, StonesUnread};
use pharmakos_proto::json::Json;

use crate::exit::Failure;
use crate::{rules, strings};

/// The built-in operator's factory: Easy for every seat, as a built-in seat
/// or as an advisor (see the module docs).
///
/// It holds the public rules text and the count of lines its seats could not
/// write ([`Unsaid`]), and builds a fresh [`Easy`] from the text for every
/// seat it is asked about.
#[derive(Clone, Debug)]
pub struct EasyOperators {
    rules_json: String,
    unsaid: std::sync::Arc<Unsaid>,
}

/// Why a built-in seat's line was not written to standard error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnsaidReason {
    /// The reader hung up (`ErrorKind::BrokenPipe`): the lobby has gone.
    BrokenPipe,
    /// Any other write error.
    Other,
}

impl UnsaidReason {
    /// Every reason, in the order a line names them.
    pub const ALL: [UnsaidReason; 2] = [UnsaidReason::BrokenPipe, UnsaidReason::Other];

    /// Its name in a line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            UnsaidReason::BrokenPipe => "broken_pipe",
            UnsaidReason::Other => "other",
        }
    }

    /// The reason a write error is counted under.
    #[must_use]
    pub fn of(error: &std::io::Error) -> UnsaidReason {
        match error.kind() {
            std::io::ErrorKind::BrokenPipe => UnsaidReason::BrokenPipe,
            _ => UnsaidReason::Other,
        }
    }
}

/// The lines a host's built-in seats could not write to standard error, by
/// reason, since the last line that was written: shared by every seat of the
/// host, and said by the next line that is written ([`EasySeat`]).
#[derive(Debug, Default)]
pub struct Unsaid {
    broken_pipe: std::sync::atomic::AtomicU64,
    other: std::sync::atomic::AtomicU64,
}

impl Unsaid {
    const fn counter(&self, reason: UnsaidReason) -> &std::sync::atomic::AtomicU64 {
        match reason {
            UnsaidReason::BrokenPipe => &self.broken_pipe,
            UnsaidReason::Other => &self.other,
        }
    }

    /// Count one line not written for `reason`. A host that fails `u64::MAX`
    /// writes wraps rather than panics; at one a round that is never.
    pub fn count(&self, reason: UnsaidReason) {
        self.counter(reason)
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// How many lines are unwritten for `reason`.
    #[must_use]
    pub fn of(&self, reason: UnsaidReason) -> u64 {
        self.counter(reason)
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Take the counts, leaving zero: what the next written line says.
    fn take(&self) -> Vec<(UnsaidReason, u64)> {
        UnsaidReason::ALL
            .iter()
            .map(|reason| {
                (
                    *reason,
                    self.counter(*reason)
                        .swap(0, std::sync::atomic::Ordering::SeqCst),
                )
            })
            .collect()
    }

    /// Put taken counts back, for a line that was not written either.
    fn restore(&self, taken: &[(UnsaidReason, u64)]) {
        for (reason, count) in taken {
            self.counter(*reason)
                .fetch_add(*count, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

impl EasyOperators {
    /// A factory scoring with this rules text: the same text the host is
    /// handed (`Setup.rules_json`, decision C17).
    ///
    /// # Errors
    ///
    /// [`RulesError`] when the operator cannot read its rows from the text,
    /// said once here rather than once per seat.
    pub fn new(rules_json: &str) -> Result<EasyOperators, RulesError> {
        let _checked = Easy::new(rules_json)?;
        Ok(EasyOperators {
            rules_json: rules_json.to_owned(),
            unsaid: std::sync::Arc::default(),
        })
    }

    /// The lines this host's built-in seats could not write, by reason, not
    /// yet said by a line that was.
    #[must_use]
    pub fn unsaid(&self) -> &Unsaid {
        &self.unsaid
    }
}

impl Operators for EasyOperators {
    fn built_in(&mut self, _seat: u8) -> Option<Box<dyn BuiltInSeat>> {
        let easy = Easy::new(&self.rules_json).ok()?;
        Some(Box::new(EasySeat {
            easy,
            said: Box::new(std::io::stderr()),
            unsaid: std::sync::Arc::clone(&self.unsaid),
        }))
    }

    fn advisor(&mut self, _seat: u8) -> Option<Box<dyn Advisor>> {
        let easy = Easy::new(&self.rules_json).ok()?;
        Some(Box::new(EasyAdvisor(easy)))
    }
}

/// Easy playing a seat, and where its round says what was refused: this
/// command's standard error (the module docs).
struct EasySeat {
    easy: Easy,
    said: Box<dyn Write + Send>,
    unsaid: std::sync::Arc<Unsaid>,
}

impl BuiltInSeat for EasySeat {
    fn plan(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json) {
        let mut bridged = bridge(call);
        let played = self.easy.play(seat, &mut bridged);
        if let Some(line) = refusals_said(seat, &played) {
            self.say(&line);
        }
    }
}

impl EasySeat {
    /// Write one line, with the count of lines not written before it; a line
    /// that is not written is counted by its reason and its earlier counts
    /// kept for the next (the module docs).
    fn say(&mut self, line: &str) {
        let taken = self.unsaid.take();
        let earlier: Vec<(&str, u64)> = taken
            .iter()
            .filter(|(_, count)| *count > 0)
            .map(|(reason, count)| (reason.name(), *count))
            .collect();
        let written = if earlier.is_empty() {
            writeln!(self.said, "{line}")
        } else {
            writeln!(self.said, "{line} {}", strings::operator_unsaid(&earlier))
        };
        if let Err(error) = written {
            self.unsaid.restore(&taken);
            self.unsaid.count(UnsaidReason::of(&error));
        }
    }
}

/// What a built-in seat's round says on standard error (the module docs):
/// the refusal that kept it from being read, and the route estimates refused
/// for another reason than `NOT_FOUND`; `None` when nothing was refused.
#[must_use]
pub fn refusals_said(seat: u8, played: &Played) -> Option<String> {
    let read = played.refused.as_ref().map(|refused| {
        strings::operator_read_refused(&refused.method, &refused.code, &refused.message)
    });
    let estimates: Vec<(&str, &str)> = played
        .estimates_refused
        .iter()
        .map(|refused| (refused.code.as_str(), refused.message.as_str()))
        .collect();
    let estimates =
        (!estimates.is_empty()).then(|| strings::operator_estimates_refused(&estimates));
    let stones = stones_unread(&played.failed_stones);
    let said: Vec<String> = read.into_iter().chain(estimates).chain(stones).collect();
    (!said.is_empty()).then(|| strings::operator_round(seat, played.round, &said.join(" ")))
}

/// Why last round's failed stones were not all known, when they were not.
fn stones_unread(stones: &FailedStones) -> Option<String> {
    let FailedStones::Unread { why, .. } = stones else {
        return None;
    };
    Some(match why {
        StonesUnread::NoCarriedSeal => strings::operator_no_carried_seal(),
        StonesUnread::CarriedOfRound(round) => strings::operator_carried_of_round(*round),
        StonesUnread::CarriedUnread(refused) => {
            strings::operator_carried_unread(&refused.method, &refused.code, &refused.message)
        }
    })
}

/// Easy advising a seat.
#[derive(Debug)]
struct EasyAdvisor(Easy);

impl Advisor for EasyAdvisor {
    fn advise(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json) -> Advice {
        let mut bridged = bridge(call);
        advice_of(self.0.advise(seat, &mut bridged))
    }
}

/// The operator's call closure over the gateway's: a method name and params
/// in, a `Request` to the seat's own door, the answer back as it came.
fn bridge(call: &mut dyn FnMut(&Request) -> Json) -> impl FnMut(&str, Json) -> Json + '_ {
    move |method: &str, params: Json| {
        call(&Request {
            id: Json::Number(String::from("1")),
            method: method.to_owned(),
            params,
        })
    }
}

/// The operator's advice in the gateway's plain types, field for field.
#[must_use]
fn advice_of(advice: pharmakos_operator::Advice) -> Advice {
    Advice {
        safe_playbook_jsonc: advice.safe_playbook_jsonc,
        suggestions: advice
            .suggestions
            .into_iter()
            .map(|suggestion| Suggestion {
                template_id: suggestion.template_id,
                parameters: suggestion
                    .parameters
                    .into_iter()
                    .map(|value| SuggestedValue {
                        pointer: value.pointer,
                        value: value.value,
                    })
                    .collect(),
                why: suggestion.why,
            })
            .collect(),
        start_here: advice.start_here.map(|start| StartHere {
            template_id: start.template_id,
            why: start.why,
        }),
    }
}

/// The template library, relative to the root: the flat `library/` folder
/// the gateway reads to list and instantiate templates (decisions-log item
/// 111, decision C13), beside [`crate::rules::RULES_PATH`].
///
/// Beside a shipped binary it is the zip's `library/`, next to `gamectl` and
/// `rules/`, as in a checkout (decisions-log item 117 (3)); the zip's manifest
/// golden pins the three templates' paths.
pub const LIBRARY_PATH: &str = "library";

/// Host one match until standard input closes.
///
/// # Errors
///
/// As the module's exit-code table: [`crate::exit::Exit::Usage`] with a
/// terminal on standard output, [`crate::exit::Exit::Input`] for an unreadable
/// rules table or a refused config line, and [`crate::exit::Exit::Internal`]
/// when the host could not start.
pub fn run(root: &Path) -> Result<String, Failure> {
    if std::io::stdout().is_terminal() {
        return Err(Failure::usage(strings::host_on_a_terminal()));
    }
    let path = root.join(rules::RULES_PATH);
    let rules_json = std::fs::read_to_string(&path).map_err(|error| {
        Failure::input(strings::unreadable(
            "rules table",
            &crate::display(&path),
            &error.to_string(),
        ))
    })?;
    let operators = EasyOperators::new(&rules_json).map_err(|error| {
        Failure::input(strings::unreadable(
            "rules table",
            &crate::display(&path),
            &error.to_string(),
        ))
    })?;
    let setup = Setup {
        rules_json,
        // A path, not a check: the gateway reads the folder on every
        // `list_templates` and says so when it cannot, and a checkout always
        // has one.
        library: Some(root.join(LIBRARY_PATH)),
        // Easy for every seat: it plays the seats nobody at this machine
        // plays and advises the human's (T18).
        operators: Box::new(operators),
    };
    serve::run(setup, std::io::stdin().lock(), std::io::stdout().lock()).map_err(|error| {
        let message = strings::host_refused(&error.to_string());
        if error.code() == Code::InvalidArgument {
            Failure::input(message)
        } else {
            Failure::internal(message)
        }
    })?;
    // Nothing more goes to standard output: the announce line was the whole of
    // what this command writes there.
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use pharmakos_operator::{Refused, Submitted};

    use super::*;

    fn refused(method: &str, code: &str, message: &str) -> Refused {
        Refused {
            method: method.to_owned(),
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    fn played(refused: Option<Refused>, estimates_refused: Vec<Refused>) -> Played {
        Played {
            round: 2,
            submitted: Submitted::Own,
            playbook_jsonc: None,
            repairs: 0,
            ready: true,
            calls: 40,
            why: String::new(),
            refused,
            estimates_refused,
            fundable: 0,
            failed_stones: FailedStones::None,
        }
    }

    #[test]
    fn a_round_with_nothing_refused_says_nothing() {
        assert_eq!(refusals_said(1, &played(None, Vec::new())), None);
    }

    #[test]
    fn every_estimate_refused_is_said_with_its_code() {
        let said = refusals_said(
            1,
            &played(
                None,
                vec![
                    refused("estimate_route", "RATE_LIMITED", "too many calls"),
                    refused("estimate_route", "INTERNAL", "the router broke a chain"),
                ],
            ),
        )
        .expect("a line");
        assert!(said.starts_with("Easy, seat 1, round 2:"), "{said}");
        assert!(said.contains("2 route estimates"), "{said}");
        assert!(said.contains("RATE_LIMITED (too many calls)"), "{said}");
        assert!(
            said.contains("INTERNAL (the router broke a chain)"),
            "{said}"
        );
    }

    #[test]
    fn a_round_not_read_says_which_call_was_refused() {
        let said = refusals_said(
            0,
            &played(
                Some(refused("get_view", "RATE_LIMITED", "slow down")),
                Vec::new(),
            ),
        )
        .expect("a line");
        assert!(said.contains("get_view"), "{said}");
        assert!(said.contains("RATE_LIMITED (slow down)"), "{said}");
    }

    /// A writer the test reads back.
    #[derive(Clone, Default)]
    struct Heard(Arc<Mutex<Vec<u8>>>);

    impl Write for Heard {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("the buffer").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The seat writes its line through its door: a gateway refusing every
    /// call refuses the round's first read, and the seat says so.
    #[test]
    fn the_built_in_seat_says_its_refusals_through_its_door() {
        let heard = Heard::default();
        let mut seat = EasySeat {
            easy: Easy::new(
                &std::fs::read_to_string(
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules/rules.v1.json"),
                )
                .expect("the rules text"),
            )
            .expect("the operator reads it"),
            said: Box::new(heard.clone()),
            unsaid: std::sync::Arc::default(),
        };
        let mut refusing = |_: &Request| {
            pharmakos_proto::json::read(
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"no","data":{"code":"RATE_LIMITED"}}}"#,
            )
            .expect("a refusal")
        };
        seat.plan(1, &mut refusing);
        let text = String::from_utf8(heard.0.lock().expect("the buffer").clone()).expect("UTF-8");
        assert!(text.starts_with("Easy, seat 1, round 0:"), "{text}");
        assert!(text.contains("RATE_LIMITED (no)"), "{text}");
        assert!(text.ends_with('\n'), "{text}");
    }

    /// A writer that refuses every write, as a full or closed pipe does.
    struct Refusing(std::io::ErrorKind);

    impl Write for Refusing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(self.0))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A line standard error would not take is counted by its reason, and the
    /// next line that is written says how many went unwritten (fix pass 3,
    /// item 7).
    #[test]
    fn a_line_not_written_is_counted_and_said_by_the_next() {
        let unsaid = std::sync::Arc::new(Unsaid::default());
        let rules = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules/rules.v1.json"),
        )
        .expect("the rules text");
        let mut seat = EasySeat {
            easy: Easy::new(&rules).expect("the operator reads it"),
            said: Box::new(Refusing(std::io::ErrorKind::BrokenPipe)),
            unsaid: std::sync::Arc::clone(&unsaid),
        };
        seat.say("first");
        seat.say("second");
        assert_eq!(unsaid.of(UnsaidReason::BrokenPipe), 2);
        seat.said = Box::new(Refusing(std::io::ErrorKind::WriteZero));
        seat.say("third");
        assert_eq!(unsaid.of(UnsaidReason::BrokenPipe), 2);
        assert_eq!(unsaid.of(UnsaidReason::Other), 1);
        let heard = Heard::default();
        seat.said = Box::new(heard.clone());
        seat.say("fourth");
        let text = String::from_utf8(heard.0.lock().expect("the buffer").clone()).expect("UTF-8");
        assert!(text.starts_with("fourth "), "{text}");
        assert!(text.contains("broken_pipe 2"), "{text}");
        assert!(text.contains("other 1"), "{text}");
        assert_eq!(unsaid.of(UnsaidReason::BrokenPipe), 0);
        assert_eq!(unsaid.of(UnsaidReason::Other), 0);
    }
}
