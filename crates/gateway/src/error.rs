// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The closed error set, and how it reaches a JSON-RPC client.
//!
//! **Closed means closed.** [`Code`] is `gp.api.v1.GatewayError.Code` and
//! nothing else; adding a code is a change to a contract file and needs the
//! owner's approval (AGENTS.md section 5). Nothing in this crate invents one,
//! and there is no `Other(String)` escape hatch -- an error the set cannot name
//! is a sign the set is wrong, which is a conversation rather than a patch.
//!
//! **An invalid playbook is not a method error.** `verify_plan` and
//! `submit_plan` answer a full verifier report with `qualifies: false` and
//! diagnostics from the E/W/I families, which are a different namespace from
//! these codes entirely (spec section 12). A gateway error means the call could
//! not be made; a report means the call was made and the answer is no.
//!
//! # The spelling on the wire
//!
//! The code travels as its Protobuf value name -- `PHASE_CLOSED`,
//! `FORBIDDEN_SCOPE` -- because that is how spec section 12 writes them and
//! because it is what a client branches on. Decisions-log item 80's lower-case
//! rule is about enum-valued **parameters** (`depth: "quick"`), not about these;
//! [`crate::scopes`] holds that translation and it is deliberately not applied
//! here.
//!
//! # The JSON-RPC number
//!
//! JSON-RPC 2.0 has its own small set of numbers and reserves -32768..-32000 for
//! them. A client written against JSON-RPC and nothing else still has to be able
//! to tell "I sent rubbish" from "the server broke", so the standard numbers are
//! used where one applies and -32000 -- the first application-defined server
//! error -- everywhere else. The authority is always `error.data.code`: the
//! number is a courtesy to a generic client, the name is the contract.

use pharmakos_proto::gp::api::v1::gateway_error;
use pharmakos_proto::json::Json;

/// The closed set, straight from the schema.
pub type Code = gateway_error::Code;

/// One gateway error: a code from the closed set and an English message.
///
/// The message is for a person; every client branches on the code. Messages are
/// English because v1 has one string table and translation is roadmap.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Error {
    /// Which of the closed set this is.
    pub code: Code,
    /// English, precise, and safe to show whoever is at the keyboard.
    pub message: String,
}

impl Error {
    /// An error with a code and a message.
    #[must_use]
    pub fn new(code: Code, message: impl Into<String>) -> Error {
        Error {
            code,
            message: message.into(),
        }
    }

    /// The token is missing, malformed, expired, revoked, or the transport's own
    /// checks refused the connection.
    #[must_use]
    pub fn unauthenticated(message: impl Into<String>) -> Error {
        Error::new(Code::Unauthenticated, message)
    }

    /// The token is valid and does not carry the scope this method needs.
    #[must_use]
    pub fn forbidden(message: impl Into<String>) -> Error {
        Error::new(Code::ForbiddenScope, message)
    }

    /// The request is malformed: an unknown method, a missing param, a param out
    /// of range.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Error {
        Error::new(Code::InvalidArgument, message)
    }

    /// A name that does not exist, or is not this seat's.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Error {
        Error::new(Code::NotFound, message)
    }

    /// The method is not available in this phase.
    #[must_use]
    pub fn phase_closed(message: impl Into<String>) -> Error {
        Error::new(Code::PhaseClosed, message)
    }

    /// A cursor or a request pinned to a snapshot that is no longer current.
    #[must_use]
    pub fn stale_snapshot(message: impl Into<String>) -> Error {
        Error::new(Code::StaleSnapshot, message)
    }

    /// Over the rate limit.
    #[must_use]
    pub fn rate_limited(message: impl Into<String>) -> Error {
        Error::new(Code::RateLimited, message)
    }

    /// The gateway failed at something the caller could not have avoided.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Error {
        Error::new(Code::Internal, message)
    }

    /// The code's name on the wire: `PHASE_CLOSED`, `FORBIDDEN_SCOPE`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.code.as_str_name()
    }

    /// The JSON-RPC number a generic client reads.
    ///
    /// -32601 and -32602 are the standard's own "method not found" and "invalid
    /// params"; -32603 is its "internal error"; -32000 is the first
    /// application-defined server error and covers everything the standard has
    /// no word for. `data.code` is the authority in every case.
    #[must_use]
    pub const fn rpc_number(&self) -> i64 {
        match self.code {
            Code::InvalidArgument => -32602,
            Code::Internal => -32603,
            _ => -32000,
        }
    }

    /// The JSON-RPC `error` member.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::Object(vec![
            (
                String::from("code"),
                Json::Number(self.rpc_number().to_string()),
            ),
            (String::from("message"), Json::String(self.message.clone())),
            (
                String::from("data"),
                Json::Object(vec![(
                    String::from("code"),
                    Json::String(String::from(self.name())),
                )]),
            ),
        ])
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.name(), self.message)
    }
}

impl std::error::Error for Error {}

/// A method whose name is not in the schema. Its own constructor because the
/// JSON-RPC number differs: the standard has a word for this one.
#[must_use]
pub fn unknown_method(name: &str) -> Error {
    Error::new(
        Code::InvalidArgument,
        format!("`{name}` is not a Seat Gateway method"),
    )
}

#[cfg(test)]
mod tests {
    use super::{Code, Error};
    use pharmakos_proto::json::Json;

    #[test]
    fn a_code_spells_itself_as_the_spec_writes_it() {
        assert_eq!(Error::phase_closed("x").name(), "PHASE_CLOSED");
        assert_eq!(Error::rate_limited("x").name(), "RATE_LIMITED");
        assert_eq!(Error::forbidden("x").name(), "FORBIDDEN_SCOPE");
    }

    #[test]
    fn the_json_carries_the_name_as_well_as_the_number() {
        let json = Error::new(Code::StaleSnapshot, "the cursor is from the last Lull").to_json();
        let data = json.get("data").expect("data");
        assert_eq!(
            data.get("code"),
            Some(&Json::String(String::from("STALE_SNAPSHOT")))
        );
        assert_eq!(
            json.get("code"),
            Some(&Json::Number(String::from("-32000"))),
            "an application-defined server error"
        );
    }

    #[test]
    fn the_standard_numbers_are_used_where_the_standard_has_a_word() {
        assert_eq!(Error::invalid("x").rpc_number(), -32602);
        assert_eq!(Error::internal("x").rpc_number(), -32603);
    }
}
