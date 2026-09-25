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
//! # Exit codes
//!
//! The table in [`crate::exit`], and nothing new:
//!
//! | code | when |
//! |---|---|
//! | 0 | the parent closed standard input and the match was put down |
//! | 1 | standard output is a terminal, so nothing was started |
//! | 2 | the rules table could not be read, or the config line was one this host refuses |
//! | 4 | the host could not start: no loopback bind, no entropy for a token, no private match cache |
//!
//! Standard output belongs to the announce line alone. Everything else this
//! command has to say goes to standard error, which is where the lobby's
//! "host failed to start" message reads it from.
//!
//! PLACEHOLDER: the rules table and the template library are both read from
//! `--root`, as every other command reads the rules. Where both live beside a
//! shipped binary is packaging's question (T13's and `crate::rules`'
//! PLACEHOLDER); **T21** decides it.

use std::io::IsTerminal as _;
use std::path::Path;

use pharmakos_gateway::Code;
use pharmakos_gateway::advice::{Advice, SuggestedValue, Suggestion};
use pharmakos_gateway::rpc::Request;
use pharmakos_gateway::serve::{self, Advisor, BuiltInSeat, Operators, Setup};
use pharmakos_operator::{Easy, RulesError};
use pharmakos_proto::json::Json;

use crate::exit::Failure;
use crate::{rules, strings};

/// The built-in operator's factory: Easy for every seat, as a built-in seat
/// or as an advisor (see the module docs).
///
/// It holds the public rules text and nothing else, and builds a fresh
/// [`Easy`] from it for every seat it is asked about.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EasyOperators {
    rules_json: String,
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
        })
    }
}

impl Operators for EasyOperators {
    fn built_in(&mut self, _seat: u8) -> Option<Box<dyn BuiltInSeat>> {
        let easy = Easy::new(&self.rules_json).ok()?;
        Some(Box::new(EasySeat(easy)))
    }

    fn advisor(&mut self, _seat: u8) -> Option<Box<dyn Advisor>> {
        let easy = Easy::new(&self.rules_json).ok()?;
        Some(Box::new(EasyAdvisor(easy)))
    }
}

/// Easy playing a seat.
#[derive(Debug)]
struct EasySeat(Easy);

impl BuiltInSeat for EasySeat {
    fn plan(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json) {
        let mut bridged = bridge(call);
        let _played = self.0.play(seat, &mut bridged);
    }
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
pub fn bridge(call: &mut dyn FnMut(&Request) -> Json) -> impl FnMut(&str, Json) -> Json + '_ {
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
pub fn advice_of(advice: pharmakos_operator::Advice) -> Advice {
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
    }
}

/// The template library, relative to the root: the flat `library/` folder
/// the gateway reads to list and instantiate templates (decisions-log item
/// 111, decision C13), beside [`crate::rules::RULES_PATH`].
///
/// PLACEHOLDER: where the library lives beside a shipped binary is packaging's
/// question, **T21**.
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
        if error.code == Code::InvalidArgument {
            Failure::input(message)
        } else {
            Failure::internal(message)
        }
    })?;
    // Nothing more goes to standard output: the announce line was the whole of
    // what this command writes there.
    Ok(String::new())
}
