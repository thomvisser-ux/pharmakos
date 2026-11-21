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
use pharmakos_proto::json::grammar::Said;
use pharmakos_proto::said;

/// The closed set, straight from the schema.
pub type Code = gateway_error::Code;

/// One gateway error: a code from the closed set and an English message.
///
/// The message is for a person; every client branches on the code. Messages are
/// English because v1 has one string table and translation is roadmap.
///
/// **Every message is a [`Said`]** (S3's `vfyq2`, fix pass 3, item M2 (b)):
/// every constructor takes one, so text a seat sent reaches a message only
/// [`quoted`](pharmakos_proto::json::grammar::quoted), and a raw `String`
/// does not compile; the `said` field is the crate's, so no other crate can
/// make an `Error` around one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Error {
    /// Which of the closed set this is.
    pub code: Code,
    /// English, precise, and safe to show whoever is at the keyboard.
    pub message: String,
    /// The message as it was said.
    pub(crate) said: Said,
}

impl Error {
    /// An error with a code and a message.
    #[must_use]
    pub fn new(code: Code, message: impl Into<Said>) -> Error {
        let said = message.into();
        Error {
            code,
            message: said.as_str().to_owned(),
            said,
        }
    }

    /// The message, as it was said: what another refusal that carries this
    /// one composes it from.
    #[must_use]
    pub fn said(&self) -> Said {
        self.said.clone()
    }

    /// The token is missing, malformed, expired, revoked, or the transport's own
    /// checks refused the connection.
    #[must_use]
    pub fn unauthenticated(message: impl Into<Said>) -> Error {
        Error::new(Code::Unauthenticated, message)
    }

    /// The token is valid and does not carry the scope this method needs.
    #[must_use]
    pub fn forbidden(message: impl Into<Said>) -> Error {
        Error::new(Code::ForbiddenScope, message)
    }

    /// The request is malformed: an unknown method, a missing param, a param out
    /// of range.
    ///
    /// The message is a [`Said`] (S3's `vfyq2`, fix pass 3, item M2 (b)):
    /// these two refusals are the ones that answer what a seat sent, so a
    /// raw echo of it does not compile; what the seat sent goes in quoted.
    #[must_use]
    pub fn invalid(message: impl Into<Said>) -> Error {
        Error::new(Code::InvalidArgument, message)
    }

    /// A name that does not exist, or is not this seat's. The message is a
    /// [`Said`], as [`Error::invalid`]'s is.
    #[must_use]
    pub fn not_found(message: impl Into<Said>) -> Error {
        Error::new(Code::NotFound, message)
    }

    /// The method is not available in this phase.
    #[must_use]
    pub fn phase_closed(message: impl Into<Said>) -> Error {
        Error::new(Code::PhaseClosed, message)
    }

    /// A cursor or a request pinned to a snapshot that is no longer current.
    #[must_use]
    pub fn stale_snapshot(message: impl Into<Said>) -> Error {
        Error::new(Code::StaleSnapshot, message)
    }

    /// Over the rate limit.
    #[must_use]
    pub fn rate_limited(message: impl Into<Said>) -> Error {
        Error::new(Code::RateLimited, message)
    }

    /// The gateway failed at something the caller could not have avoided.
    #[must_use]
    pub fn internal(message: impl Into<Said>) -> Error {
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
        said!(
            "`{}` is not a Seat Gateway method",
            pharmakos_proto::json::grammar::quoted(name)
        ),
    )
}

/// A path on the host, as a refusal shows it: escaped by the grammar's rule
/// and never cut (S3's `vfyq2`, fix pass 3, item M2 (b)). A path is the
/// host's, not a seat's, and a refusal that names one is about the host's
/// own disk.
pub(crate) fn shown_path(path: &std::path::Path) -> Said {
    pharmakos_proto::json::grammar::escaped(&path.display().to_string()).into()
}

/// An engine error of the sim's or this crate's, said in its own words.
///
/// [`Engine`] declares, type by type, that the error's words carry no text a
/// seat wrote (S3's `vfyq2`, fix pass 3, item M2 (d)); a type not declared
/// below does not compile here. The sim's `PlanError`, whose words carry a
/// playbook's labels and ids, is not declared: the planning handlers say it
/// through `plan_refusal`.
pub(crate) fn engine<T: std::fmt::Display>(error: &T) -> Said
where
    for<'a> Engine<'a, T>: pharmakos_proto::json::grammar::EngineWords,
{
    Said::engine_error(&Engine(error))
}

/// [`engine`]'s declaration of an error type whose words are the engine's.
pub(crate) struct Engine<'a, T>(&'a T);

impl<T: std::fmt::Display> std::fmt::Display for Engine<'_, T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The operating system's words for a file or a socket.
impl pharmakos_proto::json::grammar::EngineWords for Engine<'_, std::io::Error> {}
/// The map generator's refusal: a seed and the rules table's map rows.
impl pharmakos_proto::json::grammar::EngineWords for Engine<'_, pharmakos_sim::MapError> {}
/// A rules table's refusal: the host's table, a field and a value.
impl pharmakos_proto::json::grammar::EngineWords for Engine<'_, pharmakos_sim::rules::RulesError> {}
/// A snapshot's refusal: the save's bytes, read from the host's own disk.
impl pharmakos_proto::json::grammar::EngineWords
    for Engine<'_, pharmakos_sim::snapshot::SnapshotError>
{
}
/// The audit's rank refusal: a count.
impl pharmakos_proto::json::grammar::EngineWords
    for Engine<'_, pharmakos_sim::audit::AuditRankError>
{
}
/// The settlement ladder's refusal: seat indices and ranks.
impl pharmakos_proto::json::grammar::EngineWords
    for Engine<'_, pharmakos_sim::economy::SettlementReadError>
{
}
/// An id's refusal: the id's own layout.
impl pharmakos_proto::json::grammar::EngineWords for Engine<'_, pharmakos_sim::ids::IdError> {}
/// A seat token's entropy refusal: the operating system's words.
impl pharmakos_proto::json::grammar::EngineWords for Engine<'_, getrandom::Error> {}
/// A Lull-length row's refusal: the rules table's own rows.
impl pharmakos_proto::json::grammar::EngineWords for Engine<'_, crate::time::LullRowError> {}
/// A seal's refusal: a phase, a seat number, a tick in flight.
impl pharmakos_proto::json::grammar::EngineWords
    for Engine<'_, pharmakos_sim::runner::SealRefused>
{
}
/// A layout's refusal: the host's own lobby layout, by index, column and seat.
impl pharmakos_proto::json::grammar::EngineWords
    for Engine<'_, pharmakos_sim::runner::LayoutRefused>
{
}
/// A tick's refusal: the sim's own phases and chunk errors.
impl pharmakos_proto::json::grammar::EngineWords for Engine<'_, pharmakos_sim::world::TickError> {}
/// The power grid's refusal: figures.
impl pharmakos_proto::json::grammar::EngineWords for Engine<'_, pharmakos_sim::power::PowerError> {}
/// A fingerprint's refusal: the codec's own message, composed from words and
/// quoted text.
impl pharmakos_proto::json::grammar::EngineWords
    for Engine<'_, pharmakos_sim::hash::FingerprintError>
{
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

#[cfg(test)]
mod engine_words {
    use super::engine;
    use pharmakos_proto::json::grammar::{HIDDEN, Said};

    /// ESC, an OSC sequence, BEL, a right-to-left override, a tag character
    /// and a variation selector.
    const HOSTILE: &str = "\u{1b}]0;x\u{7}\u{202e}z\u{e0041}\u{fe0f}";

    fn clean(said: &Said, what: &str) {
        let raw: Vec<String> = said
            .as_str()
            .chars()
            .filter(|character| {
                HIDDEN
                    .iter()
                    .any(|(low, high)| *low <= *character && *character <= *high)
            })
            .map(|character| format!("U+{:04X}", u32::from(character)))
            .collect();
        assert!(raw.is_empty(), "{what}: raw {raw:?} in {:?}", said.as_str());
    }

    /// Every declared engine-words type that has a text field, with hostile
    /// text in it, comes back escaped (S3's `vfyq2`, fix pass 4, item L-a):
    /// `Said::engine_error` escapes whatever a declared type's words carry.
    /// The types with no text field (numbers, `&'static str`, enums) carry
    /// none to escape.
    #[test]
    fn every_declared_engine_type_with_text_says_it_escaped() {
        use pharmakos_sim::MapError;
        use pharmakos_sim::rules::RulesError;
        use pharmakos_sim::snapshot::SnapshotError;
        clean(
            &engine(&RulesError::Io {
                path: HOSTILE.to_owned(),
                message: HOSTILE.to_owned(),
            }),
            "RulesError::Io",
        );
        clean(
            &engine(&RulesError::OutOfRange {
                field: HOSTILE.to_owned(),
                value: HOSTILE.to_owned(),
            }),
            "RulesError::OutOfRange",
        );
        clean(
            &engine(&MapError::OutOfRange {
                field: "x",
                why: HOSTILE.to_owned(),
            }),
            "MapError::OutOfRange",
        );
        clean(
            &engine(&MapError::Codec(HOSTILE.to_owned())),
            "MapError::Codec",
        );
        clean(
            &engine(&SnapshotError::Decode(HOSTILE.to_owned())),
            "SnapshotError::Decode",
        );
        clean(
            &Said::engine_error(&pharmakos_verifier::ScopeError::UnknownBeacon(
                HOSTILE.to_owned(),
            )),
            "ScopeError::UnknownBeacon",
        );
        clean(
            &Said::engine_error(&pharmakos_verifier::ScopeError::NotOwnBeacon(
                HOSTILE.to_owned(),
            )),
            "ScopeError::NotOwnBeacon",
        );
        clean(
            &engine(&std::io::Error::other(HOSTILE.to_owned())),
            "io::Error",
        );
    }
}
