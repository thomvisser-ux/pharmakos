// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Building a report: the shared diagnostic builder every stage writes into,
//! and the JSON Patch suggestions a diagnostic carries.
//!
//! # Order is the walk, and the walk is the order
//!
//! Diagnostics are appended in the order the stages run and, inside a stage, in
//! the order the walk reaches them — document order. Nothing is sorted
//! afterwards, because a sort needs a total key and "where in the file" already
//! is one (AGENTS.md section 4.6). Two runs over the same input therefore append
//! the same diagnostics in the same order, which is what
//! `a_report_is_the_same_bytes_twice` asserts and what `report_hash` promises.
//!
//! # Every diagnostic carries the same seven things
//!
//! Spec section 11, modelled on rustc's JSON output: a code, a severity, a JSON
//! Pointer, related paths, map references, a precise message, a plain-language
//! beginner sentence, and patch suggestions labelled by how safely they apply.
//! The code, the severity and the two strings come from the catalogue and the
//! string table, so a call site supplies only what it found — the place, the
//! numbers, and the fix.

use core::fmt::Write as _;

use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::api::v1::{Diagnostic, PatchSuggestion};
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;

use crate::catalogue;
use crate::strings;

/// One diagnostic under construction.
///
/// Built by a stage, finished by [`Builder::emit`], which is the only place the
/// catalogue and the string table are consulted.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Diag {
    code: &'static str,
    path: String,
    related: Vec<String>,
    args: Vec<(&'static str, String)>,
    map_refs: Vec<Voxel>,
    suggestions: Vec<PatchSuggestion>,
}

impl Diag {
    /// A diagnostic for `code`, pointing at `path`.
    pub(crate) fn new(code: &'static str, path: impl Into<String>) -> Diag {
        Diag {
            code,
            path: path.into(),
            related: Vec::new(),
            args: Vec::new(),
            map_refs: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// One substitution for the message template.
    pub(crate) fn arg(mut self, name: &'static str, value: impl core::fmt::Display) -> Diag {
        self.args.push((name, value.to_string()));
        self
    }

    /// Another place in the file that matters to this diagnostic.
    pub(crate) fn related(mut self, path: impl Into<String>) -> Diag {
        self.related.push(path.into());
        self
    }

    /// A place on the map this diagnostic refers to.
    pub(crate) fn map_ref(mut self, voxel: Voxel) -> Diag {
        self.map_refs.push(voxel);
        self
    }

    /// A fix the editor can offer, labelled by how safely it applies.
    pub(crate) fn fix(
        mut self,
        title: impl Into<String>,
        json_patch: String,
        applicability: Applicability,
    ) -> Diag {
        self.suggestions.push(PatchSuggestion {
            title: title.into(),
            json_patch,
            applicability: i32::from(applicability),
        });
        self
    }
}

/// The report under construction, shared by every stage.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Builder {
    diagnostics: Vec<Diagnostic>,
}

impl Builder {
    /// Finish a diagnostic and append it.
    pub(crate) fn emit(&mut self, diag: Diag) {
        let severity = catalogue::entry(diag.code).map_or(Severity::Error, |row| row.severity);
        let (message, beginner) = match strings::text(diag.code) {
            Some(row) => (
                strings::fill(row.message, &diag.args),
                row.beginner.to_owned(),
            ),
            // Unreachable while `the_two_tables_name_the_same_codes` passes. It
            // is written out rather than asserted because a verifier that
            // panics on a playbook is worse than one that reports an ugly
            // message about itself.
            None => (
                format!("`{}` is not in the diagnostic catalogue", diag.code),
                String::new(),
            ),
        };
        self.diagnostics.push(Diagnostic {
            code: diag.code.to_owned(),
            severity: i32::from(severity),
            path: diag.path,
            related_paths: diag.related,
            message,
            beginner,
            suggestions: diag.suggestions,
            map_refs: diag.map_refs,
        });
    }

    /// Whether anything so far stops the playbook qualifying.
    ///
    /// Spec section 11: a playbook qualifies when it has **zero errors**. A
    /// warning is information, not a refusal.
    pub(crate) fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == i32::from(Severity::Error))
    }

    /// The diagnostics, in the order they were emitted.
    pub(crate) fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

// ---------------------------------------------------------------------------
// JSON Patch suggestions (RFC 6902)
// ---------------------------------------------------------------------------

/// `[{"op":"add","path":…,"value":…}]`.
pub(crate) fn patch_add(path: &str, value: &Json) -> String {
    patch("add", path, Some(value))
}

/// `[{"op":"replace","path":…,"value":…}]`.
pub(crate) fn patch_replace(path: &str, value: &Json) -> String {
    patch("replace", path, Some(value))
}

/// `[{"op":"remove","path":…}]`.
pub(crate) fn patch_remove(path: &str) -> String {
    patch("remove", path, None)
}

fn patch(op: &str, path: &str, value: Option<&Json>) -> String {
    let mut entries = vec![
        ("op".to_owned(), Json::String(op.to_owned())),
        ("path".to_owned(), Json::String(path.to_owned())),
    ];
    if let Some(value) = value {
        entries.push(("value".to_owned(), value.clone()));
    }
    compact(&Json::Array(vec![Json::Object(entries)]))
}

/// A whole number as a JSON value.
///
/// `Json::Number` holds the lexeme as text and is never parsed into a float
/// (`crates/proto`'s reader is explicit about it), so an integer written here
/// stays exactly the integer that was written.
pub(crate) fn number(value: impl core::fmt::Display) -> Json {
    Json::Number(value.to_string())
}

/// A JSON value written **compactly**, for embedding in a patch string.
///
/// `pharmakos_proto::json::write` is the canonical writer and it indents,
/// because the file format it writes is one a player reads. A patch is not that
/// file: it lives inside a string field of a report, where a newline would be an
/// escape sequence and nothing more. So this writer exists, it is small, and it
/// is pinned by the report goldens that quote its output.
fn compact(value: &Json) -> String {
    let mut out = String::new();
    write_compact(value, &mut out);
    out
}

fn write_compact(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(lexeme) => out.push_str(lexeme),
        Json::String(text) => write_string(text, out),
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_compact(item, out);
            }
            out.push(']');
        }
        Json::Object(entries) => {
            out.push('{');
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_compact(item, out);
            }
            out.push('}');
        }
    }
}

/// One JSON string, escaped as the grammar requires.
fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            other if other < ' ' => {
                let code = u32::from(other);
                // Writing into a `String` cannot fail; the result is dropped
                // rather than unwrapped, because `unwrap` is denied and a panic
                // inside a diagnostic would be the worse failure.
                let _ = write!(out, "\\u{code:04x}");
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::{Builder, Diag, number, patch_add, patch_remove, patch_replace};
    use pharmakos_proto::gp::api::v1::diagnostic::Severity;
    use pharmakos_proto::json::{Json, read};

    #[test]
    fn a_patch_is_valid_json_and_carries_the_operation() {
        for text in [
            patch_add("/declarative/route/0/timeout_ms", &number(30_000)),
            patch_replace("/meta/author_kind", &Json::String("HUMAN".to_owned())),
            patch_remove("/declarative/route/0/set_flag"),
        ] {
            let value = read(&text).expect("a suggestion is always valid JSON");
            let Json::Array(items) = value else {
                panic!("a JSON Patch is an array of operations");
            };
            assert_eq!(items.len(), 1);
        }
    }

    #[test]
    fn a_patch_is_written_on_one_line() {
        let text = patch_add("/a", &Json::String("b".to_owned()));
        assert_eq!(text, "[{\"op\":\"add\",\"path\":\"/a\",\"value\":\"b\"}]");
    }

    #[test]
    fn a_string_in_a_patch_is_escaped() {
        let text = patch_replace("/meta/note", &Json::String("a\"b\\c\nd".to_owned()));
        assert!(text.contains("a\\\"b\\\\c\\nd"), "{text}");
        assert!(read(&text).is_ok());
    }

    #[test]
    fn severity_comes_from_the_catalogue_and_the_message_from_the_table() {
        let mut builder = Builder::default();
        builder.emit(
            Diag::new("E0107", "/declarative/handlers/0/max_fires")
                .arg("found", 9)
                .arg("max", 8),
        );
        assert!(builder.has_errors());
        let diagnostics = builder.into_diagnostics();
        let first = diagnostics.first().expect("one diagnostic");
        assert_eq!(first.code, "E0107");
        assert_eq!(first.severity, i32::from(Severity::Error));
        assert_eq!(
            first.message,
            "`max_fires` is 9; it must be between 1 and 8."
        );
        assert!(!first.beginner.is_empty());
    }

    #[test]
    fn a_warning_does_not_stop_a_playbook_qualifying() {
        let mut builder = Builder::default();
        builder.emit(Diag::new("E0008", "/meta/fingerprint"));
        assert!(!builder.has_errors());
    }
}
