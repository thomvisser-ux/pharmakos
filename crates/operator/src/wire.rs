// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The wire: the one door the operator has to the match.
//!
//! The operator is handed a **call closure**, `FnMut(&str, Json) -> Json`:
//! a method name and its params in, the whole JSON-RPC response out
//! (decisions-log item 111, decision C4). `gamectl host` binds it to the
//! seat's own in-process token, so every call goes through the same door, the
//! same audit and the same fog as a socket's. This module is the operator's
//! side of that door and nothing more: it counts calls, takes the `result`
//! member apart from a refusal, and reads the answer's values.
//!
//! # Two rules the wire makes necessary
//!
//! * **The `_status` footer is dropped unread.** It carries the phase timer,
//!   which the host clock moves, and an operator whose playbook depended on it
//!   would not be a function of (match, seat, round). [`Wire::call`] strips it
//!   before anything else sees the answer, and the timer inside
//!   `get_status`'s own `status` is never read either
//!   ([`crate::situation`] reads the round and the segment length there and
//!   nothing else).
//! * **Enum values are lower case on the wire** (decisions-log item 80): the
//!   answers are JSON-RPC results, not canonical proto JSON, so an enum value
//!   is translated back with [`pharmakos_proto::scope::from_wire_name`]
//!   before it is compared with anything ([`Wire::enum_name`]).

use pharmakos_proto::json::Json;

/// The closure every operator is driven through: a method name and its
/// params, and the whole JSON-RPC response (the `result` member or the
/// `error` member) back.
pub type Call<'a> = dyn FnMut(&str, Json) -> Json + 'a;

/// A method the gateway refused, with its closed-set code.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Refused {
    /// The method that was called.
    pub method: String,
    /// The gateway's closed-set code (`RATE_LIMITED`, `INVALID_ARGUMENT`, ...),
    /// or `MALFORMED` for an answer that was neither a result nor an error.
    pub code: String,
    /// The gateway's message.
    pub message: String,
}

/// The operator's side of the call closure: it counts every call.
pub(crate) struct Wire<'c, 'a> {
    call: &'c mut Call<'a>,
    calls: u32,
}

impl<'c, 'a> Wire<'c, 'a> {
    /// Wrap a call closure.
    pub(crate) fn new(call: &'c mut Call<'a>) -> Wire<'c, 'a> {
        Wire { call, calls: 0 }
    }

    /// How many calls this wire has made.
    pub(crate) const fn calls(&self) -> u32 {
        self.calls
    }

    /// Call one method, and answer its `result` with the `_status` footer
    /// taken out, or the refusal.
    ///
    /// # Errors
    ///
    /// The gateway's refusal; and `OVER_BUDGET`, without calling, once the
    /// count of calls would not fit, which no round within Easy's derived
    /// budget reaches.
    pub(crate) fn call(&mut self, method: &str, params: Json) -> Result<Json, Refused> {
        self.calls = self.calls.checked_add(1).ok_or_else(|| Refused {
            method: method.to_owned(),
            code: String::from("OVER_BUDGET"),
            message: String::from("the operator's call count does not fit"),
        })?;
        let response = (self.call)(method, params);
        if let Some(result) = response.get("result") {
            return Ok(without_status(result));
        }
        // A refusal names its closed-set code. An answer that carries
        // neither a result nor a refusal with a code is one the operator
        // cannot read, and says so by the one code it writes itself.
        let error = response.get("error");
        let code = error
            .and_then(|error| error.get("data"))
            .and_then(|data| data.get("code"))
            .and_then(text);
        let message = error.and_then(|error| error.get("message")).and_then(text);
        Err(match (code, message) {
            (Some(code), Some(message)) => Refused {
                method: method.to_owned(),
                code: code.to_owned(),
                message: message.to_owned(),
            },
            (Some(code), None) => Refused {
                method: method.to_owned(),
                code: code.to_owned(),
                message: String::from("the refusal carries no message"),
            },
            (None, _) => malformed(
                method,
                "the answer is neither a result nor a refusal with a code",
            ),
        })
    }

    /// An enum value as the wire spells it, translated to its proto name
    /// (`"normal"` becomes `"NORMAL"`). A value already spelt in upper case
    /// is taken as it is, and anything else is `None`.
    pub(crate) fn enum_name(enum_full_name: &str, value: Option<&Json>) -> Option<String> {
        let spelt = value.and_then(text)?;
        pharmakos_proto::scope::from_wire_name(enum_full_name, spelt).or_else(|| {
            spelt
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
                .then(|| spelt.to_owned())
        })
    }
}

/// A result with its `_status` footer taken out.
fn without_status(result: &Json) -> Json {
    match result {
        Json::Object(entries) => Json::Object(
            entries
                .iter()
                .filter(|(key, _)| key != "_status")
                .cloned()
                .collect(),
        ),
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------
// Reading values
// ---------------------------------------------------------------------------

/// A string value.
pub(crate) fn text(value: &Json) -> Option<&str> {
    match value {
        Json::String(found) => Some(found.as_str()),
        _ => None,
    }
}

/// A refusal the operator writes itself: the answer came and did not read.
pub(crate) fn malformed(method: &str, message: &str) -> Refused {
    Refused {
        method: method.to_owned(),
        code: String::from("MALFORMED"),
        message: message.to_owned(),
    }
}

/// A member of an answer that is present and is not the shape the schema
/// gives it: a typed read's one error (decisions-log item 136 (5); S2's plan,
/// E7). An **absent** member is proto3's spelling of its default and reads as
/// that default, which is the exact value and no fallback; a member of the
/// wrong shape is never read as one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Misread {
    /// The member's key.
    pub(crate) key: String,
    /// The shape it should have had.
    pub(crate) wanted: &'static str,
}

impl Misread {
    fn new(key: &str, wanted: &'static str) -> Misread {
        Misread {
            key: key.to_owned(),
            wanted,
        }
    }

    /// The refusal this misread is, in an answer to `method`.
    pub(crate) fn refused(&self, method: &str) -> Refused {
        malformed(
            method,
            &format!("the member `{}` is not {}", self.key, self.wanted),
        )
    }
}

/// A string member: the empty string (proto3's default) when absent.
///
/// # Errors
///
/// [`Misread`] when the member is present and not a string.
pub(crate) fn text_of<'j>(value: &'j Json, key: &str) -> Result<&'j str, Misread> {
    match value.get(key) {
        None => Ok(""),
        Some(Json::String(found)) => Ok(found.as_str()),
        Some(_) => Err(Misread::new(key, "a string")),
    }
}

/// A whole-number member: zero (proto3's default) when absent.
///
/// # Errors
///
/// [`Misread`] when the member is present and not a whole number that fits
/// an `i64`.
pub(crate) fn int_of(value: &Json, key: &str) -> Result<i64, Misread> {
    match value.get(key) {
        None => Ok(0),
        Some(found) => int(found).ok_or_else(|| Misread::new(key, "a whole number")),
    }
}

/// A whole number.
pub(crate) fn int(value: &Json) -> Option<i64> {
    match value {
        // proto3 JSON quotes 64-bit integers; accept that spelling too.
        Json::Number(lexeme) | Json::String(lexeme) => lexeme.parse::<i64>().ok(),
        _ => None,
    }
}

/// A boolean member: false (proto3's default) when absent.
///
/// # Errors
///
/// [`Misread`] when the member is present and not a boolean.
pub(crate) fn bool_of(value: &Json, key: &str) -> Result<bool, Misread> {
    match value.get(key) {
        None => Ok(false),
        Some(Json::Bool(found)) => Ok(*found),
        Some(_) => Err(Misread::new(key, "a boolean")),
    }
}

/// An array member: empty (proto3's default) when absent.
///
/// # Errors
///
/// [`Misread`] when the member is present and not an array.
pub(crate) fn array_of<'j>(value: &'j Json, key: &str) -> Result<&'j [Json], Misread> {
    match value.get(key) {
        None => Ok(&[]),
        Some(Json::Array(items)) => Ok(items.as_slice()),
        Some(_) => Err(Misread::new(key, "an array")),
    }
}

/// A `gp.v1.Voxel` as three whole numbers. An absent axis is zero, which is
/// how proto3 JSON spells a zero.
///
/// # Errors
///
/// [`Misread`] when the value is not an object, or an axis is present and not
/// a whole number that fits an `i32`.
pub(crate) fn voxel(value: &Json) -> Result<[i32; 3], Misread> {
    if !matches!(value, Json::Object(_)) {
        return Err(Misread::new("voxel", "an object"));
    }
    let axis = |name: &str| -> Result<i32, Misread> {
        i32::try_from(int_of(value, name)?).map_err(|_| Misread::new(name, "a voxel coordinate"))
    };
    Ok([axis("x")?, axis("y")?, axis("z")?])
}

/// A `gp.v1.Location` read as the voxel it names: `{"voxel": {...}}`, the
/// declared shape, or a bare `{x, y, z}`, which `estimate_route`'s `Leg.to`
/// answered on `main` before T17 fixed it to the declared one.
///
/// # Errors
///
/// [`Misread`] when the shape it has does not read as a voxel.
pub(crate) fn location_voxel(value: &Json) -> Result<[i32; 3], Misread> {
    match value.get("voxel") {
        Some(inner) => voxel(inner),
        None => voxel(value),
    }
}

// ---------------------------------------------------------------------------
// The segment feed's words
// ---------------------------------------------------------------------------

/// The feed's kind for a commander's death (`crates/sim/src/events.rs`'
/// `EventKind::CommanderDied`, a shipped name that never changes, AGENTS.md
/// section 5's event vocabulary).
pub const COMMANDER_DIED: &str = "commander_died";

/// The seat a `commander_died` line names: "The commander of seat N died."
///
/// A feed event carries no seat field until its per-kind payload
/// (`gateway.proto`'s `Event`, held to after S4), so whose commander died is
/// in the line's words alone, and they are read strictly. The gateway's one
/// string table writes them (its `strings` module's `event_text`), and
/// `crates/gamectl/tests/operator_host.rs` holds this reader to it.
///
/// # Errors
///
/// `MALFORMED` for a line that does not read so, never taken for another
/// seat's.
pub fn whose_commander_died(line: &str) -> Result<u8, Refused> {
    let unread = || {
        malformed(
            "get_segment_feed",
            &format!("a `{COMMANDER_DIED}` line that does not say whose: `{line}`"),
        )
    };
    let rest = line
        .strip_prefix("The commander of seat ")
        .ok_or_else(unread)?;
    let (number, tail) = rest.split_once(' ').ok_or_else(unread)?;
    if !tail.starts_with("died.") {
        return Err(unread());
    }
    number.parse::<u8>().map_err(|_| unread())
}

// ---------------------------------------------------------------------------
// Writing values
// ---------------------------------------------------------------------------

/// A JSON object from its members, in order.
pub(crate) fn object(members: Vec<(&str, Json)>) -> Json {
    Json::Object(
        members
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

/// A JSON string.
pub(crate) fn string(value: &str) -> Json {
    Json::String(value.to_owned())
}

/// A JSON whole number.
pub(crate) fn number(value: i64) -> Json {
    Json::Number(value.to_string())
}

/// A `gp.v1.Voxel`.
pub(crate) fn voxel_json(at: [i32; 3]) -> Json {
    let [x, y, z] = at;
    object(vec![
        ("x", number(i64::from(x))),
        ("y", number(i64::from(y))),
        ("z", number(i64::from(z))),
    ])
}

/// A `gp.v1.Location` naming a voxel.
pub(crate) fn voxel_location(at: [i32; 3]) -> Json {
    object(vec![("voxel", voxel_json(at))])
}

/// JSON text on one line, members in order: what a template parameter's value
/// and a JSON Patch are written as.
pub(crate) fn compact(value: &Json) -> String {
    let mut out = String::new();
    compact_into(value, &mut out);
    out
}

fn compact_into(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(lexeme) => out.push_str(lexeme),
        Json::String(_) => {
            // The canonical writer's own escaping, less its trailing newline.
            out.push_str(pharmakos_proto::json::write(value).trim_end_matches('\n'));
        }
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                compact_into(item, out);
            }
            out.push(']');
        }
        Json::Object(entries) => {
            out.push('{');
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                compact_into(&Json::String(key.clone()), out);
                out.push(':');
                compact_into(item, out);
            }
            out.push('}');
        }
    }
}

/// A JSONC text with its comments taken out, so the canonical reader can read
/// it.
///
/// `//` to the end of the line and `/* ... */`, outside strings. The text is
/// the gateway's own answer (an instantiated template), read only to learn
/// the shape of a step to copy; nothing here is validation, which is the
/// verifier's, reached through `verify_plan`.
pub(crate) fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                let _ = chars.next();
                let mut last = ' ';
                for skipped in chars.by_ref() {
                    if last == '*' && skipped == '/' {
                        break;
                    }
                    last = skipped;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_is_read_in_the_declared_shape_and_the_bare_one() {
        let declared = pharmakos_proto::json::read(r#"{"voxel":{"x":1,"y":2,"z":3}}"#).unwrap();
        let bare = pharmakos_proto::json::read(r#"{"x":1,"y":2,"z":3}"#).unwrap();
        assert_eq!(location_voxel(&declared), Ok([1, 2, 3]));
        assert_eq!(location_voxel(&bare), Ok([1, 2, 3]));
    }

    #[test]
    fn comments_go_and_strings_stay() {
        let text = "{\n  // a comment\n  \"a\": \"// not a comment\", /* gone */ \"b\": 1\n}\n";
        let read = pharmakos_proto::json::read(&strip_comments(text)).unwrap();
        assert_eq!(text_of(&read, "a"), Ok("// not a comment"));
        assert_eq!(int_of(&read, "b"), Ok(1));
    }

    #[test]
    fn an_absent_member_is_its_default_and_a_misshapen_one_is_refused() {
        let value =
            pharmakos_proto::json::read(r#"{"n":"x","t":1,"b":"yes","a":{},"v":{"x":1.5}}"#)
                .unwrap();
        assert_eq!(int_of(&value, "absent"), Ok(0));
        assert_eq!(text_of(&value, "absent"), Ok(""));
        assert_eq!(bool_of(&value, "absent"), Ok(false));
        assert_eq!(array_of(&value, "absent").map(<[Json]>::len), Ok(0));
        assert!(int_of(&value, "n").is_err());
        assert!(text_of(&value, "t").is_err());
        assert!(bool_of(&value, "b").is_err());
        assert!(array_of(&value, "a").is_err());
        assert!(voxel(value.get("v").unwrap()).is_err());
        assert_eq!(
            voxel(&pharmakos_proto::json::read(r#"{"y":2}"#).unwrap()),
            Ok([0, 2, 0])
        );
        let refused = int_of(&value, "n").unwrap_err().refused("get_status");
        assert_eq!(refused.code, "MALFORMED");
        assert_eq!(refused.method, "get_status");
    }

    #[test]
    fn an_answer_with_no_coded_refusal_is_malformed() {
        let mut call = |_: &str, _: Json| -> Json {
            pharmakos_proto::json::read(r#"{"jsonrpc":"2.0","id":1}"#).unwrap()
        };
        let mut wire = Wire::new(&mut call);
        let refused = wire.call("get_status", object(vec![])).unwrap_err();
        assert_eq!(refused.code, "MALFORMED");
    }

    #[test]
    fn compact_text_is_one_line_in_order() {
        let value = object(vec![
            ("b", number(2)),
            ("a", Json::Array(vec![string("x\"y"), Json::Bool(true)])),
        ]);
        assert_eq!(compact(&value), r#"{"b":2,"a":["x\"y",true]}"#);
    }

    #[test]
    fn the_status_footer_is_dropped_and_a_refusal_carries_its_code() {
        let mut call = |method: &str, _params: Json| -> Json {
            if method == "ok" {
                pharmakos_proto::json::read(
                    r#"{"jsonrpc":"2.0","id":1,"result":{"a":1,"_status":{"phase":"lull"}}}"#,
                )
                .unwrap()
            } else {
                pharmakos_proto::json::read(
                    r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"no","data":{"code":"FORBIDDEN_SCOPE"}}}"#,
                )
                .unwrap()
            }
        };
        let mut wire = Wire::new(&mut call);
        let answer = wire.call("ok", object(vec![])).unwrap();
        assert!(answer.get("_status").is_none());
        assert_eq!(int_of(&answer, "a"), Ok(1));
        let refused = wire.call("no", object(vec![])).unwrap_err();
        assert_eq!(refused.code, "FORBIDDEN_SCOPE");
        assert_eq!(wire.calls(), 2);
    }
}
