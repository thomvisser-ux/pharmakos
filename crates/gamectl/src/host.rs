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
//!    `--root`) and an operator factory that plays and advises nobody
//!    (`serve::NoOperators`, until **T18** lands the built-in operator) to
//!    `serve::run`, with the process's own standard input and standard
//!    output. It returns when standard input reaches end of file, which is the
//!    whole of the shutdown protocol: no heartbeat and no clock.
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
use pharmakos_gateway::serve::{self, NoOperators, Setup};

use crate::exit::Failure;
use crate::{rules, strings};

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
    let setup = Setup {
        rules_json,
        // A path, not a check: the gateway reads the folder on every
        // `list_templates` and says so when it cannot, and a checkout always
        // has one.
        library: Some(root.join(LIBRARY_PATH)),
        // PLACEHOLDER: T18 hands its operator factory here; until then every
        // seat that seals nothing is filed the gateway's fallback.
        operators: Box::new(NoOperators),
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
