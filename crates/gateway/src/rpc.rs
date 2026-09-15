// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! JSON-RPC 2.0, over [`pharmakos_proto::json`].
//!
//! Hand-written, like the rest of the transport (decisions-log item 99), and
//! built on the proto crate's `Json` value rather than a second JSON parser --
//! which is the part of item 99 that matters most: the gateway reads the same
//! JSON, with the same rules about numbers never being floats, as the codec that
//! reads a playbook off disk.
//!
//! # The subset
//!
//! * **One request per message.** A JSON-RPC *batch* -- an array of requests --
//!   is refused with `Invalid Request`. No v1 client batches: the editor calls
//!   one method at a time and waits, `gamectl` is a command line, and the
//!   built-in operator is a loop. Accepting batches would mean deciding what a
//!   partially rate-limited batch means, and answering that question before
//!   anybody has the problem is how a surface grows things nobody needs.
//! * **No notifications.** Every method the gateway serves answers something,
//!   even if only a `_status` footer, so a request without an `id` is an
//!   `Invalid Request` rather than a silently swallowed call.
//! * **`params` is an object or absent.** Positional parameters are legal
//!   JSON-RPC and would be a second spelling of every method's signature.
//!
//! # The result envelope
//!
//! Every result is a JSON object carrying a `_status` footer with the phase and
//! the timer (spec section 12, "Budgets"). The leading underscore is not a legal
//! Protobuf identifier, which is exactly why the footer is an envelope concern:
//! `gp.api.v1.Status` is its shape, and [`crate::time::MatchTime::footer`]
//! writes it.

use crate::error::Error;
use pharmakos_proto::json::{Json, read, write};

/// The version string every request and response carries.
pub const VERSION: &str = "2.0";

/// A request, once it has been read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Request {
    /// The call's id, echoed in the response. A string or a number; JSON-RPC
    /// allows null, and this gateway does not, because a null id is how a
    /// notification is spelled.
    pub id: Json,
    /// The method string: `get_status`, `submit_plan`.
    pub method: String,
    /// The parameters, always an object. An absent `params` reads as an empty
    /// object, so a method with no parameters needs no special case.
    pub params: Json,
}

impl Request {
    /// One parameter, or `None` when it is absent or null.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&Json> {
        match self.params.get(name) {
            Some(Json::Null) | None => None,
            Some(value) => Some(value),
        }
    }

    /// One string parameter.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] when the parameter is present and
    /// is not a string.
    pub fn string_param(&self, name: &str) -> Result<Option<&str>, Error> {
        match self.param(name) {
            None => Ok(None),
            Some(Json::String(text)) => Ok(Some(text.as_str())),
            Some(other) => Err(Error::invalid(format!(
                "`{name}` is a string, and this is {}",
                other.kind()
            ))),
        }
    }

    /// One boolean parameter.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] when the parameter is present and
    /// is not a boolean.
    pub fn bool_param(&self, name: &str) -> Result<Option<bool>, Error> {
        match self.param(name) {
            None => Ok(None),
            Some(Json::Bool(value)) => Ok(Some(*value)),
            Some(other) => Err(Error::invalid(format!(
                "`{name}` is a boolean, and this is {}",
                other.kind()
            ))),
        }
    }

    /// One integer parameter, as game milliseconds or a count.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] when the parameter is present and
    /// is not an integer that fits `i64`. A fractional literal is refused rather
    /// than rounded: no field of `gp.v1` or `gp.api.v1` is floating point
    /// (AGENTS.md section 4.2).
    pub fn integer_param(&self, name: &str) -> Result<Option<i64>, Error> {
        match self.param(name) {
            None => Ok(None),
            Some(Json::Number(lexeme)) => lexeme.parse::<i64>().map(Some).map_err(|_| {
                Error::invalid(format!("`{name}` is a whole number, and `{lexeme}` is not"))
            }),
            Some(other) => Err(Error::invalid(format!(
                "`{name}` is a whole number, and this is {}",
                other.kind()
            ))),
        }
    }
}

/// Why a message could not be read as a request at all.
///
/// Separate from [`crate::error::Error`] because these are JSON-RPC's own
/// failures rather than the gateway's: they happen before there is a method to
/// refuse, and they carry the standard's own numbers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Malformed {
    /// Not JSON at all. JSON-RPC calls this -32700.
    Parse(String),
    /// JSON, but not a JSON-RPC 2.0 request this gateway serves. -32600.
    Invalid(String),
}

impl Malformed {
    /// The JSON-RPC number.
    #[must_use]
    pub const fn number(&self) -> i64 {
        match self {
            Malformed::Parse(_) => -32700,
            Malformed::Invalid(_) => -32600,
        }
    }

    /// The standard's own word for it.
    #[must_use]
    pub const fn title(&self) -> &'static str {
        match self {
            Malformed::Parse(_) => "Parse error",
            Malformed::Invalid(_) => "Invalid Request",
        }
    }

    /// The detail, for a person.
    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            Malformed::Parse(text) | Malformed::Invalid(text) => text.as_str(),
        }
    }
}

/// Read one JSON-RPC request.
///
/// # Errors
///
/// A [`Malformed`], which the caller turns into a response with
/// [`malformed_response`].
pub fn parse(text: &str) -> Result<Request, Malformed> {
    let value = read(text).map_err(|error| Malformed::Parse(error.to_string()))?;

    if matches!(value, Json::Array(_)) {
        return Err(Malformed::Invalid(String::from(
            "this gateway does not take batches: send one request per message",
        )));
    }
    let Json::Object(_) = value else {
        return Err(Malformed::Invalid(format!(
            "a JSON-RPC request is an object, and this is {}",
            value.kind()
        )));
    };

    match value.get("jsonrpc") {
        Some(Json::String(version)) if version == VERSION => {}
        _ => {
            return Err(Malformed::Invalid(String::from(
                "every request carries `\"jsonrpc\": \"2.0\"`",
            )));
        }
    }

    let id = match value.get("id") {
        Some(id @ (Json::String(_) | Json::Number(_))) => id.clone(),
        Some(Json::Null) | None => {
            return Err(Malformed::Invalid(String::from(
                "every request carries an `id`: this gateway answers every call, so it serves \
                 no notifications",
            )));
        }
        Some(other) => {
            return Err(Malformed::Invalid(format!(
                "`id` is a string or a number, and this is {}",
                other.kind()
            )));
        }
    };

    let Some(Json::String(method)) = value.get("method") else {
        return Err(Malformed::Invalid(String::from(
            "`method` is the method's name, as a string",
        )));
    };

    let params = match value.get("params") {
        None | Some(Json::Null) => Json::Object(Vec::new()),
        Some(object @ Json::Object(_)) => object.clone(),
        Some(Json::Array(_)) => {
            return Err(Malformed::Invalid(String::from(
                "`params` is an object: this gateway takes named parameters only",
            )));
        }
        Some(other) => {
            return Err(Malformed::Invalid(format!(
                "`params` is an object, and this is {}",
                other.kind()
            )));
        }
    };

    Ok(Request {
        id,
        method: method.clone(),
        params,
    })
}

/// A successful response.
#[must_use]
pub fn success(id: &Json, result: Json) -> Json {
    Json::Object(vec![
        (String::from("jsonrpc"), Json::String(String::from(VERSION))),
        (String::from("id"), id.clone()),
        (String::from("result"), result),
    ])
}

/// A response carrying a gateway error from the closed set.
#[must_use]
pub fn failure(id: &Json, error: &Error) -> Json {
    Json::Object(vec![
        (String::from("jsonrpc"), Json::String(String::from(VERSION))),
        (String::from("id"), id.clone()),
        (String::from("error"), error.to_json()),
    ])
}

/// A response to a message that was not a request. The id is null, as JSON-RPC
/// requires when it could not be read.
#[must_use]
pub fn malformed_response(malformed: &Malformed) -> Json {
    Json::Object(vec![
        (String::from("jsonrpc"), Json::String(String::from(VERSION))),
        (String::from("id"), Json::Null),
        (
            String::from("error"),
            Json::Object(vec![
                (
                    String::from("code"),
                    Json::Number(malformed.number().to_string()),
                ),
                (
                    String::from("message"),
                    Json::String(format!("{}: {}", malformed.title(), malformed.detail())),
                ),
            ]),
        ),
    ])
}

/// A response as the text that goes on the wire.
///
/// The canonical writer's one-entry-per-line form, which is what every other
/// JSON in this project is written as, and what makes a captured message
/// diffable.
#[must_use]
pub fn render(response: &Json) -> String {
    write(response)
}

#[cfg(test)]
mod tests {
    use super::{Malformed, failure, malformed_response, parse, success};
    use crate::error::Error;
    use pharmakos_proto::json::Json;

    fn call(text: &str) -> Result<super::Request, Malformed> {
        parse(text)
    }

    #[test]
    fn a_well_formed_call_reads() {
        let request =
            call(r#"{"jsonrpc":"2.0","id":7,"method":"verify_plan","params":{"depth":"quick"}}"#)
                .expect("well formed");
        assert_eq!(request.method, "verify_plan");
        assert_eq!(request.id, Json::Number(String::from("7")));
        assert_eq!(
            request.string_param("depth").expect("a string"),
            Some("quick")
        );
        assert_eq!(request.param("missing"), None);
    }

    #[test]
    fn params_may_be_absent() {
        let request =
            call(r#"{"jsonrpc":"2.0","id":"a","method":"get_status"}"#).expect("well formed");
        assert_eq!(request.params, Json::Object(Vec::new()));
        assert_eq!(request.string_param("anything").expect("absent"), None);
    }

    #[test]
    fn a_batch_is_refused_rather_than_half_served() {
        let malformed =
            call(r#"[{"jsonrpc":"2.0","id":1,"method":"get_status"}]"#).expect_err("refused");
        assert_eq!(malformed.number(), -32600);
        assert!(malformed.detail().contains("batches"), "{malformed:?}");
    }

    #[test]
    fn a_notification_is_refused_because_every_call_is_answered() {
        let malformed =
            call(r#"{"jsonrpc":"2.0","method":"set_ready","params":{}}"#).expect_err("refused");
        assert!(malformed.detail().contains("`id`"), "{malformed:?}");
    }

    #[test]
    fn positional_params_are_refused() {
        let malformed = call(r#"{"jsonrpc":"2.0","id":1,"method":"get_beacon","params":["b_04"]}"#)
            .expect_err("refused");
        assert!(
            malformed.detail().contains("named parameters"),
            "{malformed:?}"
        );
    }

    #[test]
    fn a_missing_version_is_refused() {
        let malformed = call(r#"{"id":1,"method":"get_status"}"#).expect_err("refused");
        assert!(malformed.detail().contains("2.0"), "{malformed:?}");
        let malformed =
            call(r#"{"jsonrpc":"1.0","id":1,"method":"get_status"}"#).expect_err("refused");
        assert_eq!(malformed.number(), -32600);
    }

    #[test]
    fn rubbish_is_a_parse_error_and_not_an_invalid_request() {
        let malformed = call("{not json").expect_err("refused");
        assert_eq!(malformed.number(), -32700);
        let response = malformed_response(&malformed);
        assert_eq!(response.get("id"), Some(&Json::Null));
    }

    #[test]
    fn a_number_that_is_not_whole_is_refused_rather_than_rounded() {
        let request =
            call(r#"{"jsonrpc":"2.0","id":1,"method":"wait_for","params":{"timeout_ms":1.5}}"#)
                .expect("well formed JSON-RPC");
        let error = request.integer_param("timeout_ms").expect_err("refused");
        assert!(error.message.contains("whole number"), "{}", error.message);
    }

    #[test]
    fn a_wrongly_typed_param_names_what_it_should_have_been() {
        let request =
            call(r#"{"jsonrpc":"2.0","id":1,"method":"set_ready","params":{"ready":"yes"}}"#)
                .expect("well formed JSON-RPC");
        let error = request.bool_param("ready").expect_err("refused");
        assert!(error.message.contains("boolean"), "{}", error.message);
    }

    #[test]
    fn a_response_echoes_the_id_and_names_the_version() {
        let id = Json::String(String::from("call-1"));
        let response = success(&id, Json::Object(Vec::new()));
        assert_eq!(
            response.get("jsonrpc"),
            Some(&Json::String(String::from("2.0")))
        );
        assert_eq!(response.get("id"), Some(&id));
        assert!(response.get("error").is_none());

        let response = failure(&id, &Error::rate_limited("slow down"));
        assert_eq!(response.get("id"), Some(&id));
        let error = response.get("error").expect("an error member");
        assert_eq!(
            error.get("data").and_then(|data| data.get("code")),
            Some(&Json::String(String::from("RATE_LIMITED")))
        );
        assert!(response.get("result").is_none(), "never both");
    }
}
