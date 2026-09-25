// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The templates as the gateway declares them, and JSON Pointers into them.
//!
//! The operator has no copy of a template: it asks the gateway for each one
//! it knows (`instantiate_template` with no parameters), which answers the
//! playbook with the template's own values **and the declared parameters**
//! (`gp.v1.Meta.parameters`, decision C1) -- every pointer, in declaration
//! order, with its own value. That is the one instantiate per template the
//! call budget counts, and it is what lets Easy fill a template by the
//! pointers the file itself declares rather than by pointers written into
//! code.

use pharmakos_proto::json::Json;

use crate::wire::{Refused, Wire, array_of, object, string, strip_comments, text_of};

/// One template, instantiated with its own values.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Declared {
    /// The library's id for it.
    pub(crate) template_id: String,
    /// The playbook, JSONC, comments included, exactly as the gateway
    /// answered it.
    pub(crate) jsonc: String,
    /// The same, comments taken out and read, to copy a step's shape from.
    /// `Json::Null` when it would not read.
    pub(crate) body: Json,
    /// The declared parameters, in declaration order: pointer and own value
    /// (compact JSON text).
    pub(crate) parameters: Vec<(String, String)>,
}

impl Declared {
    /// The declared pointer that ends with `suffix`, if the template has one.
    pub(crate) fn pointer_ending(&self, suffix: &str) -> Option<&str> {
        self.parameters
            .iter()
            .map(|(pointer, _)| pointer.as_str())
            .find(|pointer| pointer.ends_with(suffix))
    }

    /// A declared parameter's own value, read.
    pub(crate) fn own_value(&self, pointer: &str) -> Option<Json> {
        self.parameters
            .iter()
            .find(|(held, _)| held == pointer)
            .and_then(|(_, value)| pharmakos_proto::json::read(value).ok())
    }
}

/// Instantiate every template in `known` that the library lists, in `known`'s
/// order, with its own values.
///
/// A refusal skips that template: the operator plans with what it has.
pub(crate) fn declare(wire: &mut Wire<'_, '_>, listed: &[String], known: &[&str]) -> Vec<Declared> {
    let mut out: Vec<Declared> = Vec::new();
    for template_id in known {
        if !listed.iter().any(|listed| listed == template_id) {
            continue;
        }
        let Ok(answer) = instantiate(wire, template_id, &[]) else {
            continue;
        };
        out.push(answer);
    }
    out
}

/// Instantiate one template with explicit parameters, `(pointer, JSON text)`.
///
/// # Errors
///
/// The gateway's refusal.
pub(crate) fn instantiate(
    wire: &mut Wire<'_, '_>,
    template_id: &str,
    parameters: &[(String, String)],
) -> Result<Declared, Refused> {
    let explicit: Vec<Json> = parameters
        .iter()
        .map(|(pointer, value)| object(vec![("name", string(pointer)), ("value", string(value))]))
        .collect();
    let mut params = vec![("template_id", string(template_id))];
    if !explicit.is_empty() {
        params.push(("parameters", Json::Array(explicit)));
    }
    let answer = wire.call("instantiate_template", object(params))?;
    let jsonc = text_of(&answer, "playbook_jsonc").to_owned();
    let body = pharmakos_proto::json::read(&strip_comments(&jsonc)).unwrap_or(Json::Null);
    let parameters = array_of(&answer, "parameters")
        .iter()
        .map(|filled| {
            (
                text_of(filled, "pointer").to_owned(),
                text_of(filled, "value").to_owned(),
            )
        })
        .collect();
    Ok(Declared {
        template_id: (*template_id).to_owned(),
        jsonc,
        body,
        parameters,
    })
}

/// The value an RFC 6901 JSON Pointer names.
pub(crate) fn pointer_get<'j>(root: &'j Json, pointer: &str) -> Option<&'j Json> {
    let mut at = root;
    if pointer.is_empty() {
        return Some(at);
    }
    for raw in pointer.strip_prefix('/')?.split('/') {
        let token = raw.replace("~1", "/").replace("~0", "~");
        at = match at {
            Json::Object(_) => at.get(&token)?,
            Json::Array(items) => items.get(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(at)
}

/// Replace one member of an object value, or add it: a small builder for the
/// steps the operator copies from a template.
pub(crate) fn with_member(value: &Json, key: &str, member: Json) -> Json {
    match value {
        Json::Object(entries) => {
            let mut entries = entries.clone();
            match entries.iter_mut().find(|(held, _)| held == key) {
                Some(slot) => slot.1 = member,
                None => entries.push((key.to_owned(), member)),
            }
            Json::Object(entries)
        }
        other => other.clone(),
    }
}

/// One JSON Patch operation.
pub(crate) fn op(kind: &str, path: &str, value: Option<Json>) -> Json {
    let mut members = vec![("op", string(kind)), ("path", string(path))];
    if let Some(value) = value {
        members.push(("value", value));
    }
    object(members)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pointer_walks_objects_and_arrays() {
        let root = pharmakos_proto::json::read(r#"{"a":[{"b/c":1},{"d":2}]}"#).unwrap();
        assert_eq!(
            pointer_get(&root, "/a/1/d"),
            Some(&Json::Number(String::from("2")))
        );
        assert_eq!(
            pointer_get(&root, "/a/0/b~1c"),
            Some(&Json::Number(String::from("1")))
        );
        assert_eq!(pointer_get(&root, "/a/2"), None);
    }
}
