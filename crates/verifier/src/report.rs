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
use std::borrow::Cow;

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
    args: Vec<(&'static str, Cow<'static, str>)>,
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
        self.args.push((name, Cow::Owned(value.to_string())));
        self
    }

    /// One substitution that is a word of the string table, held without a
    /// copy (S3's `vfy6`: a note raised once per step pays for every copy).
    pub(crate) fn arg_word(mut self, name: &'static str, value: &'static str) -> Diag {
        self.args.push((name, Cow::Borrowed(value)));
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

/// The most `I0003` notes one report carries (S3's `vfy6`, the review of PR
/// #179, item 3).
///
/// Decision 8's notes come one per omitted setting, so a long file of bare
/// steps made a report of megabytes, past the client's 1 MiB inbound buffer
/// (`godot/scripts/host_link.gd`'s `INBOUND_BYTES`). Past this many the rest
/// are counted, and one `I0007` says how many were left out. A note with its
/// Fix is about 450 bytes of JSON, so the notes of the worst capped report
/// are about 29 KB. `tests/report_size.rs` holds the longest file's report
/// to an eighth of the client's buffer.
pub const MAX_DEFAULT_NOTES: u32 = 64;

/// The code of decision 8's per-setting note.
const DEFAULT_NOTE: &str = "I0003";

/// The report under construction, shared by every stage.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Builder {
    diagnostics: Vec<Diagnostic>,
    /// `I0003` notes written so far, at most [`MAX_DEFAULT_NOTES`].
    notes: u32,
    /// `I0003` notes left out: past the cap, or after `E0110`.
    withheld: u32,
    /// Whether `E0110` has fired: past the size budget, no per-setting
    /// note is written, because the file is not one to fill in.
    over_budget: bool,
}

impl Builder {
    /// Finish a diagnostic and append it.
    pub(crate) fn emit(&mut self, diag: Diag) {
        if diag.code == DEFAULT_NOTE {
            // Every writer asks `takes_default_note` before it builds the
            // note, so that a note left out costs nothing; this keeps the cap
            // whole should one not.
            if self.notes >= MAX_DEFAULT_NOTES {
                self.withheld = self.withheld.saturating_add(1);
                return;
            }
            self.notes = self.notes.saturating_add(1);
        }
        if diag.code == "E0110" {
            self.over_budget = true;
        }
        self.push(diag);
    }

    /// Whether an `I0003` note is to be written: `false`, and the note
    /// counted as left out, past [`MAX_DEFAULT_NOTES`] or once `E0110` has
    /// fired. A writer asks before it builds the note, so a note left out
    /// costs no allocation.
    pub(crate) fn takes_default_note(&mut self) -> bool {
        if self.over_budget || self.notes >= MAX_DEFAULT_NOTES {
            self.withheld = self.withheld.saturating_add(1);
            return false;
        }
        true
    }

    /// Finish a diagnostic and append it, uncounted.
    fn push(&mut self, diag: Diag) {
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

    /// The diagnostics, in the order they were emitted, then one `I0007` with
    /// the count of `I0003` notes left out, when any were.
    pub(crate) fn into_diagnostics(mut self) -> Vec<Diagnostic> {
        if self.withheld > 0 {
            let withheld = self.withheld;
            self.push(Diag::new("I0007", crate::pointer::ROOT).arg("count", withheld));
        }
        self.diagnostics
    }
}

// ---------------------------------------------------------------------------
// JSON Patch suggestions (RFC 6902)
// ---------------------------------------------------------------------------

// # `add` or `replace`: the rule, once, here
//
// RFC 6902 section 4.3 says a `replace` **fails** unless its target already
// exists, and section 4.1 says an `add` whose target is an object member
// replaces the value when the member is there and creates it when it is not. So
// `add` is right in both directions and `replace` is right in only one.
//
// That matters more here than it would elsewhere, because proto3 JSON omits a
// field sitting at its default: a playbook whose `hold` has no `ms`, whose
// `wait_until` has no `timeout_ms`, whose handler has no `cooldown_ms`, or whose
// `kind` is unset has **no such member in the file at all** — and those are
// exactly the files the diagnostics fire on. A `replace` there is a suggestion a
// conformant applier must refuse, which for a `MACHINE_APPLICABLE` fix is the
// editor's Fix button doing nothing.
//
// The rule this crate follows:
//
// * [`patch_add`] whenever the diagnostic is *about the member being absent or
//   at its default* — a missing tag, a zero duration, an unset enum;
// * [`patch_replace`] only where the diagnostic itself proves the member is
//   present, which for a proto3 field means its value is not the default: a
//   `max_fires` of 9, a negative `cooldown_ms`, a percent above 100, a rotation
//   above 3, an `author_kind` of `SCRIPT`;
// * [`patch_remove`] for a member the author must delete, which is always
//   present by construction.

/// `[{"op":"add","path":…,"value":…}]` — creates the member, or replaces its
/// value if it is already there (RFC 6902 section 4.1). The right op wherever
/// the file may not carry the member at all.
pub(crate) fn patch_add(path: &str, value: &Json) -> String {
    patch("add", path, Some(value))
}

/// `[{"op":"replace","path":…,"value":…}]` — only where the member is known to
/// be present (RFC 6902 section 4.3 makes it fail otherwise).
pub(crate) fn patch_replace(path: &str, value: &Json) -> String {
    patch("replace", path, Some(value))
}

/// `[{"op":"remove","path":…}]`.
pub(crate) fn patch_remove(path: &str) -> String {
    patch("remove", path, None)
}

/// [`patch_add`] for a value already written as compact JSON text, which the
/// caller holds as a constant: the same bytes, with no [`Json`] built.
pub(crate) fn patch_add_text(path: &str, value: &str) -> String {
    let mut out = String::with_capacity(path.len().saturating_add(value.len()).saturating_add(32));
    out.push_str("[{\"op\":\"add\",\"path\":");
    write_string(path, &mut out);
    out.push_str(",\"value\":");
    out.push_str(value);
    out.push_str("}]");
    out
}

/// [`patch_add`] for a value that is one string, written without building a
/// [`Json`] for it: the same bytes.
pub(crate) fn patch_add_word(path: &str, word: &str) -> String {
    let mut out = String::with_capacity(path.len().saturating_add(word.len()).saturating_add(40));
    out.push_str("[{\"op\":\"add\",\"path\":");
    write_string(path, &mut out);
    out.push_str(",\"value\":");
    write_string(word, &mut out);
    out.push_str("}]");
    out
}

/// One operation, written straight into its text: the same bytes
/// [`write_compact`] writes for `[{"op":…,"path":…,"value":…}]`, without building that tree
/// first (S3's `vfy6`: a diagnostic is raised once per omitted `on_fail`, so
/// its cost is paid per step).
fn patch(op: &str, path: &str, value: Option<&Json>) -> String {
    let mut out = String::with_capacity(path.len().saturating_add(64));
    out.push_str("[{\"op\":");
    write_string(op, &mut out);
    out.push_str(",\"path\":");
    write_string(path, &mut out);
    if let Some(value) = value {
        out.push_str(",\"value\":");
        write_compact(value, &mut out);
    }
    out.push_str("}]");
    out
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
    fn the_written_patches_are_the_tree_written_compactly() {
        let path = "/declarative/route/0/on_fail";
        let value = read(r#"{"action":"SKIP"}"#).expect("JSON");
        assert_eq!(
            super::patch_add_text(path, r#"{"action":"SKIP"}"#),
            patch_add(path, &value)
        );
        assert_eq!(
            super::patch_add_word("/a~1b/\"c\"", "DIRECT"),
            patch_add("/a~1b/\"c\"", &Json::String("DIRECT".to_owned()))
        );
        assert_eq!(
            patch_add(path, &value),
            r#"[{"op":"add","path":"/declarative/route/0/on_fail","value":{"action":"SKIP"}}]"#
        );
    }

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
