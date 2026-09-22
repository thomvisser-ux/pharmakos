// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl schema [--part <name>]` — the JSON Schema a seat authors against.
//!
//! Spec §12, "Generated docs": `get_schema` and the `gamectl` docs output "are
//! generated from the schema so they can't drift". This command is four lines
//! long for exactly that reason: it prints
//! [`pharmakos_gateway::schema::text`], which is the same generator the gateway
//! serves `get_schema` from, walked out of the checked-in descriptor set. A
//! `gamectl schema` that built its own answer would be a second spelling of the
//! schema, and `tests/golden/schema/README.md` names that as the failure it
//! exists to catch.
//!
//! `--part` names a smaller root by its `lower_snake_case` short name, which is
//! the spelling spec §12's own walkthrough uses (`get_schema{part:"step"}`).

use crate::exit::{Exit, Failure};
use crate::strings;

/// The generated JSON Schema for one part.
///
/// # Errors
///
/// [`Exit::Usage`] when `part` names no message of `gp.v1` — that is a
/// misspelled operand, not a broken build — and [`Exit::Internal`] when the
/// checked-in descriptor set does not carry the root it was asked for, which
/// would mean the generated tree and this build disagree.
///
/// **Both of the gateway's ways of saying "that is not a part" are the
/// caller's fault**, and a review found only one of them mapped here.
/// `schema::text` answers `NOT_FOUND` for a well-formed name that matches no
/// message (`--part nonesuch`) and `INVALID_ARGUMENT` for one that is not a
/// `lower_snake_case` name at all (`--part Step`, `--part gp.v1.Playbook`,
/// `--part ../../etc`) — and the second used to fall to [`Exit::Internal`],
/// which [`crate::exit`] documents as "this build is inconsistent with itself;
/// nothing the caller did caused it". A misspelled operand is not that.
pub fn run(part: &str) -> Result<String, Failure> {
    use pharmakos_gateway::Code;

    match pharmakos_gateway::schema::text(part) {
        Ok(text) => Ok(text),
        Err(error) if error.code == Code::NotFound || error.code == Code::InvalidArgument => Err(
            Failure::new(Exit::Usage, strings::unknown_part(&error.message)),
        ),
        Err(error) => Err(Failure::internal(strings::unknown_part(&error.message))),
    }
}
