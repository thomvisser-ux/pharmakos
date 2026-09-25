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
    pub(crate) fn call(&mut self, method: &str, params: Json) -> Result<Json, Refused> {
        self.calls = self.calls.saturating_add(1);
        let response = (self.call)(method, params);
        if let Some(result) = response.get("result") {
            return Ok(without_status(result));
        }
        let error = response.get("error");
        Err(Refused {
            method: method.to_owned(),
            code: error
                .and_then(|error| error.get("data"))
                .and_then(|data| data.get("code"))
                .and_then(text)
                .unwrap_or("MALFORMED")
                .to_owned(),
            message: error
                .and_then(|error| error.get("message"))
                .and_then(text)
                .unwrap_or("")
                .to_owned(),
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

/// A string member, or the empty string (proto3's default) when absent.
pub(crate) fn text_of<'j>(value: &'j Json, key: &str) -> &'j str {
    value.get(key).and_then(text).unwrap_or("")
}

/// A whole-number member, or zero (proto3's default) when absent or not a
/// whole number.
pub(crate) fn int_of(value: &Json, key: &str) -> i64 {
    value.get(key).and_then(int).unwrap_or(0)
}

/// A whole number.
pub(crate) fn int(value: &Json) -> Option<i64> {
    match value {
        // proto3 JSON quotes 64-bit integers; accept that spelling too.
        Json::Number(lexeme) | Json::String(lexeme) => lexeme.parse::<i64>().ok(),
        _ => None,
    }
}

/// A boolean member, or false (proto3's default) when absent.
pub(crate) fn bool_of(value: &Json, key: &str) -> bool {
    matches!(value.get(key), Some(Json::Bool(true)))
}

/// An array member, or an empty slice when absent.
pub(crate) fn array_of<'j>(value: &'j Json, key: &str) -> &'j [Json] {
    match value.get(key) {
        Some(Json::Array(items)) => items.as_slice(),
        _ => &[],
    }
}

/// A `gp.v1.Voxel` as three whole numbers. An absent axis is zero, which is
/// how proto3 JSON spells a zero; an axis that is present and not a whole
/// number that fits is no voxel at all.
pub(crate) fn voxel(value: &Json) -> Option<[i32; 3]> {
    let axis = |name: &str| -> Option<i32> {
        match value.get(name) {
            None => Some(0),
            Some(found) => int(found).and_then(|whole| i32::try_from(whole).ok()),
        }
    };
    if !matches!(value, Json::Object(_)) {
        return None;
    }
    Some([axis("x")?, axis("y")?, axis("z")?])
}

/// A `gp.v1.Location` read as the voxel it names: `{"voxel": {...}}`, the
/// declared shape, or a bare `{x, y, z}`, which `estimate_route`'s `Leg.to`
/// answered on `main` before T17 fixed it to the declared one.
pub(crate) fn location_voxel(value: &Json) -> Option<[i32; 3]> {
    match value.get("voxel") {
        Some(inner) => voxel(inner),
        None => voxel(value),
    }
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
        assert_eq!(location_voxel(&declared), Some([1, 2, 3]));
        assert_eq!(location_voxel(&bare), Some([1, 2, 3]));
    }

    #[test]
    fn comments_go_and_strings_stay() {
        let text = "{\n  // a comment\n  \"a\": \"// not a comment\", /* gone */ \"b\": 1\n}\n";
        let read = pharmakos_proto::json::read(&strip_comments(text)).unwrap();
        assert_eq!(text_of(&read, "a"), "// not a comment");
        assert_eq!(int_of(&read, "b"), 1);
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
        assert_eq!(int_of(&answer, "a"), 1);
        let refused = wire.call("no", object(vec![])).unwrap_err();
        assert_eq!(refused.code, "FORBIDDEN_SCOPE");
        assert_eq!(wire.calls(), 2);
    }
}
