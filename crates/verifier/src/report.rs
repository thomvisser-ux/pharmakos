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
use std::collections::BTreeMap;

use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::api::v1::{Diagnostic, PatchSuggestion, VerifyReport};
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;
use pharmakos_proto::json::grammar::Said;

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

    /// One substitution for the message template: static words, a number,
    /// or player text quoted (`pharmakos_proto::json::grammar::Said`; S3's
    /// `vfyq2`, fix pass 2, item B). A `String` is not one, so text read from
    /// the file cannot reach a message without the quoting rule.
    pub(crate) fn arg(mut self, name: &'static str, value: impl Into<Said>) -> Diag {
        self.args
            .push((name, Cow::Owned(value.into().into_string())));
        self
    }

    /// One substitution that is the player's own text (a label, an id, a
    /// key), quoted through the codec's one rule
    /// (`pharmakos_proto::json::grammar::quoted`; S3's `vfyq2`): control
    /// characters escaped and the length capped, so a report never carries a
    /// terminal control sequence or the whole of a 200 KB name.
    pub(crate) fn arg_text(mut self, name: &'static str, value: impl AsRef<str>) -> Diag {
        self.args.push((
            name,
            Cow::Owned(
                pharmakos_proto::json::grammar::quoted(value.as_ref())
                    .as_str()
                    .to_owned(),
            ),
        ));
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

    /// A fix the editor can offer, labelled by how safely it applies. Its
    /// title is shown to the player, so it is a `Said` as a message argument
    /// is.
    pub(crate) fn fix(
        mut self,
        title: impl Into<Said>,
        json_patch: String,
        applicability: Applicability,
    ) -> Diag {
        self.suggestions.push(PatchSuggestion {
            title: title.into().into_string(),
            json_patch,
            applicability: i32::from(applicability),
        });
        self
    }
}

/// The most diagnostics of one code a report lists one by one (S3's `vcap`;
/// decisions-log item 183 (4), "Cap all codes"). Past it the rest of that
/// code are counted, not built, and the report closes with one `I0007` for
/// the code saying how many were left out ([`LeftOut`]).
///
/// # The bound it is derived from
///
/// The client reads a report in one WebSocket message, and a message past
/// the peer's **inbound** buffer is never handed over: Godot closes the
/// connection with 1009 (`crates/client-gdext/src/transport.rs`,
/// `INBOUND_BUFFER_BYTES`, 4 MiB, itself a PLACEHOLDER for lane `outcap`).
/// That buffer is the bound. The client's *outbound* cap, the gateway's
/// `pharmakos_proto::transport::MAX_MESSAGE_BYTES` (256 KiB), bounds what a
/// client sends, the playbook among it, and not a report.
///
/// # The derivation (`tests/report_size.rs` recomputes it from the tables)
///
/// The worst report this build can write lists **every code a stage raises at
/// its cap**, each diagnostic as long as its two strings let it be, and one
/// `I0007` per code. A diagnostic is at most its message template and
/// beginner sentence plus [`DIAGNOSTIC_ROOM_BYTES`] plus
/// [`ARGUMENT_BYTES_MAX`] for every argument its message template names
/// (every argument at the quoting rule's bound). A code
/// `tests/multiplicity.rs` holds to once per report (the decode family's
/// refusals, `E0110` and the rest that table names) counts once, not at the
/// cap, and its arguments at their own bound (`E0001`'s engine words). The sum
/// is held to **three quarters** of the inbound buffer, a quarter left for
/// the answer's envelope and for what the model leaves out; this constant is
/// the **largest** cap whose sum fits, and the test fails when it is not
/// (a cap one larger must not fit, so the number is the top of the range).
/// For this build: 99 codes a stage raises, 12 of them once a report; at 6 a
/// code the worst report is 3,078,224 bytes and at 7 it is 3,538,521, against
/// 3,145,728, three quarters of the 4 MiB buffer. A report built to the
/// model's every bound encodes to 2,328,493 bytes. If `outcap` lowers the
/// buffer, the test fails and the cap is derived again; below 2, Easy's probe
/// has no part to ask in, and the test fails at compile time.
///
/// The same file holds the reports for the longest real files to an eighth of
/// the buffer.
pub const MAX_PER_CODE: u32 = 6;

/// What one diagnostic carries at most beside its two fixed strings and its
/// arguments: the JSON framing of its eight fields (about 130 bytes), its
/// code and severity, a pointer and a related pointer (a pointer's tokens are
/// the schema's names and indices, and a member name of the player's past
/// the quoting bound is never one: `decode.rs` points at the object that
/// holds it), a Fix's title (quoted words at most [`ARGUMENT_BYTES_MAX`]) and
/// its patch, which repeats a pointer, and a map reference. The longest
/// committed diagnostic carries about 450 bytes of it.
pub const DIAGNOSTIC_ROOM_BYTES: usize = 4096;

/// The most bytes one argument of a message adds once the report is JSON: the
/// quoting rule's bound (`pharmakos_proto::json::grammar::MAX_QUOTED_CHARS`
/// characters, each escaped in at most twelve bytes, and two for the cut's
/// ellipsis), which also bounds a number, a word of the string table and a
/// code.
pub const ARGUMENT_BYTES_MAX: usize = pharmakos_proto::json::grammar::MAX_QUOTED_CHARS * 12 + 2;

/// The most bytes a pointer a report names takes as JSON. The schema's own
/// pointers are far shorter (its names, indices and at most
/// [`crate::CONDITION_MAX_DEPTH`] condition levels; `tests/report_size.rs`
/// builds the deepest); only a member name of the player's own can make a
/// longer one, and the decode stage then points at the deepest object whose
/// pointer is within this (`decode.rs`'s `bounded`).
pub const MAX_POINTER_BYTES: usize = 512;

/// The codes a report raises at most once, whatever the playbook: each is
/// raised at one fixed place by a stage that runs once (the decode family's
/// refusals, which end the decode; the version, kind, author and fingerprint
/// checks; the size budget; the estimate's one comparison of draw with
/// supply). The derivation of [`MAX_PER_CODE`] counts each once, and
/// `tests/multiplicity.rs` holds every one of them to it, so a stage that
/// starts raising one per step fails there before it can pass the bound.
pub const ONCE_PER_REPORT: &[&str] = &[
    "E0001", "E0002", "E0004", "E0005", "E0006", "E0007", "E0008", "E0029", "E0030", "E0110",
    "E0601", "W0603",
];

/// The code of decision 8's per-setting note.
const DEFAULT_NOTE: &str = "I0003";

/// The code that closes a report, once per code with diagnostics left out.
pub(crate) const LEFT_OUT: &str = "I0007";

/// What a finished report's **rows** say about the cap, for a reader that has
/// only the report (`gp.api.v1.VerifyReport` carries no left-out field until
/// the contract change after S3's `vcap`): read from codes and counts, never
/// from a sentence.
///
/// The rule, exact by construction of [`Builder`]: a code is left out of only
/// once [`MAX_PER_CODE`] of it are listed, so a code with fewer rows listed
/// is complete, and a report with no `I0007` is complete for every code. The
/// one other way a row is left out, an `I0003` note past `E0110`, is a note
/// no reader infers from.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CapRead {
    /// How many `I0007` rows close the report: how many codes were left out
    /// of.
    pub left_out_codes: usize,
    /// The codes listed exactly [`MAX_PER_CODE`] times, in code order: the
    /// only codes anything can have been left out of.
    pub at_cap: Vec<String>,
}

impl CapRead {
    /// Reads `report`'s rows.
    #[must_use]
    pub fn of(report: &VerifyReport) -> CapRead {
        let mut listed: BTreeMap<&str, u32> = BTreeMap::new();
        let mut left_out_codes = 0_usize;
        for found in &report.diagnostics {
            if found.code == LEFT_OUT {
                left_out_codes = left_out_codes.saturating_add(1);
            } else {
                let count = listed.entry(found.code.as_str()).or_default();
                *count = count.saturating_add(1);
            }
        }
        CapRead {
            left_out_codes,
            at_cap: listed
                .into_iter()
                .filter(|(_, count)| *count >= MAX_PER_CODE)
                .map(|(code, _)| code.to_owned())
                .collect(),
        }
    }

    /// Whether every diagnostic of `code` the verifier raised is listed.
    #[must_use]
    pub fn complete(&self, code: &str) -> bool {
        self.left_out_codes == 0 || !self.at_cap.iter().any(|at| at == code)
    }

    /// Whether any code at the cap may have been left out of: some `I0007`
    /// closes the report and some code is at the cap.
    #[must_use]
    pub fn any_left_out(&self) -> bool {
        self.left_out_codes > 0
    }
}

/// One code's diagnostics that a report counted and did not list, as typed
/// data: the `I0007` that closes the report for that code is rendered from
/// it, and nothing reads the sentence back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LeftOut {
    /// The code left out.
    pub code: &'static str,
    /// How many of that code the report counted and did not list.
    pub count: u64,
    /// The cap, [`MAX_PER_CODE`]: how many of the code the report could list.
    pub max: u32,
}

/// How many diagnostics of one code a report has listed and left out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Tally {
    /// Whether the code is an error, read from the catalogue once per code.
    error: bool,
    /// Listed one by one, at most [`MAX_PER_CODE`].
    listed: u32,
    /// Counted and not built: past the cap, or (for `I0003`) after `E0110`.
    ///
    /// A `u64` counted by one per call: a playbook is at most
    /// [`crate::MAX_CANONICAL_BYTES`] (eight MiB), and no walk raises more
    /// than a handful of diagnostics per byte it reads, so the count stays
    /// below 2^32, let alone 2^64; [`u64::checked_add`]'s `None` is not
    /// reachable, and is kept as the count it was rather than wrapped.
    left_out: u64,
}

/// What [`Builder::admit`] is asked: whether a diagnostic would be listed
/// (a writer's question before it builds one), or to list one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Admit {
    /// Ask only: nothing is counted as listed.
    Ask,
    /// The diagnostic is built and listed when admitted.
    List,
}

/// The report under construction, shared by every stage.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Builder {
    diagnostics: Vec<Diagnostic>,
    /// Per code, what the cap has listed and left out. An ordered map
    /// (AGENTS.md section 4.4), so the closing `I0007`s come in code order.
    tallies: BTreeMap<&'static str, Tally>,
    /// Whether any error was raised, listed or left out: the verdict is the
    /// raising, never the listing, so the cap cannot move it.
    errors: bool,
    /// Whether `E0110` has fired: past the size budget, no per-setting
    /// note is written, because the file is not one to fill in. Those notes
    /// are counted as left out like any other.
    over_budget: bool,
}

impl Builder {
    /// Finish a diagnostic and append it, unless its code is at its cap, when
    /// it is counted instead.
    pub(crate) fn emit(&mut self, diag: Diag) {
        if diag.code == "E0110" {
            self.over_budget = true;
        }
        if self.admit(diag.code, Admit::List) {
            self.push(diag);
        }
    }

    /// Whether a diagnostic of `code` is to be listed: `false`, and it is
    /// counted as left out, once [`MAX_PER_CODE`] of that code are listed,
    /// or for an `I0003` note once `E0110` has fired. A writer whose
    /// diagnostic is raised once per step asks before it builds it, so one
    /// left out costs no allocation; [`Builder::emit`] asks again, so a
    /// writer that does not ask is capped all the same. The verdict counts an
    /// error left out as raised.
    pub(crate) fn takes(&mut self, code: &'static str) -> bool {
        self.admit(code, Admit::Ask)
    }

    /// The one place the cap is applied: whether one more diagnostic of
    /// `code` is listed, counting it as left out when it is not, and as
    /// listed when `admit` is [`Admit::List`].
    fn admit(&mut self, code: &'static str, admit: Admit) -> bool {
        let over_budget = self.over_budget && code == DEFAULT_NOTE;
        let tally = self.tallies.entry(code).or_insert_with(|| Tally {
            error: is_error(code),
            ..Tally::default()
        });
        self.errors |= tally.error;
        if over_budget || tally.listed >= MAX_PER_CODE {
            // Not reachable at `None` (see `Tally::left_out`).
            if let Some(left_out) = tally.left_out.checked_add(1) {
                tally.left_out = left_out;
            }
            return false;
        }
        if admit == Admit::List {
            // Below `MAX_PER_CODE` here, so one more is at most the cap.
            if let Some(listed) = tally.listed.checked_add(1) {
                tally.listed = listed;
            }
        }
        true
    }

    /// Whether an `I0003` note is to be written (see [`Builder::takes`]).
    pub(crate) fn takes_default_note(&mut self) -> bool {
        self.takes(DEFAULT_NOTE)
    }

    /// Finish a diagnostic and append it, uncounted.
    fn push(&mut self, diag: Diag) {
        let severity = severity_of(diag.code);
        let (message, beginner) = match strings::text(diag.code) {
            Some(row) => (
                strings::fill(row.message, &diag.args),
                strings::fill(row.beginner, &diag.args),
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
    ///
    /// Every error raised counts, listed or left out past [`MAX_PER_CODE`]:
    /// the cap changes what a report lists and never whether it qualifies.
    pub(crate) fn has_errors(&self) -> bool {
        self.errors
    }

    /// What the cap left out, per code in code order: the typed data each
    /// closing `I0007` is rendered from.
    pub(crate) fn left_out(&self) -> Vec<LeftOut> {
        self.tallies
            .iter()
            .filter(|(_, tally)| tally.left_out > 0)
            .map(|(code, tally)| LeftOut {
                code,
                count: tally.left_out,
                max: MAX_PER_CODE,
            })
            .collect()
    }

    /// The diagnostics, in the order they were emitted, then, in code order,
    /// one `I0007` for each code with diagnostics left out, rendered from its
    /// [`LeftOut`]; and the left-out list itself. An `I0007` is never capped
    /// and never counted against a cap: there is at most one per code.
    pub(crate) fn finish(mut self) -> (Vec<Diagnostic>, Vec<LeftOut>) {
        let left_out = self.left_out();
        for gap in &left_out {
            self.push(
                Diag::new(LEFT_OUT, crate::pointer::ROOT)
                    .arg_word("code", gap.code)
                    .arg("count", gap.count)
                    .arg("max", gap.max),
            );
        }
        (self.diagnostics, left_out)
    }
}

/// A code's severity, from the catalogue. A code the catalogue does not have
/// is reported as an error, the safe reading, and
/// `the_two_tables_name_the_same_codes` keeps every code the stages raise in
/// it.
fn severity_of(code: &str) -> Severity {
    catalogue::entry(code).map_or(Severity::Error, |row| row.severity)
}

/// Whether `code` is an error, which stops a playbook qualifying.
fn is_error(code: &str) -> bool {
    severity_of(code) == Severity::Error
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

/// [`Builder::finish`]'s diagnostics alone, for the stages' unit tests.
#[cfg(test)]
impl Builder {
    pub(crate) fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.finish().0
    }
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
