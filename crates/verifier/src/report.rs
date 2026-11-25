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
use pharmakos_proto::gp::api::v1::{Diagnostic, LeftOut, PatchSuggestion};
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
/// code are counted, not built: the report's `left_out` field carries one
/// `gp.api.v1.LeftOut` for the code, and the report closes with one `I0007`
/// for it, rendered from the same fact, saying how many were left out.
///
/// # Why 4, and what it must fit
///
/// Four is a chosen reading size: enough of one code for a player to see the
/// pattern and fix it, and few enough that a report stays a page. It is
/// PROVEN to fit, not derived as the most that fits. The client reads a
/// report in one WebSocket message, and the gateway sends no answer past
/// `pharmakos_proto::transport::MAX_ANSWER_BYTES` (12 MiB, S3's `outcap`):
/// one past it is withheld with `RESPONSE_TOO_LARGE`, and the client's
/// inbound buffer is that cap and a frame's header
/// (`crates/client-gdext/src/transport.rs`, `INBOUND_BUFFER_BYTES`). The
/// inbound cap, `pharmakos_proto::transport::MAX_MESSAGE_BYTES` (256 KiB),
/// bounds what a client sends, the playbook among it, and not a report.
///
/// # The proof
///
/// Every part of a diagnostic is bounded where it enters the report
/// ([`Builder`]'s one door, `push`): each argument at
/// [`argument_bytes_max`], the path and a related path at
/// [`MAX_POINTER_BYTES`], at most [`MAX_RELATED`] related paths,
/// [`MAX_FIXES`] Fixes (a title at the argument bound, a patch at
/// [`PATCH_BYTES_MAX`]) and [`MAX_MAP_REFS`] map references, each measured
/// in the bytes the report's JSON carries ([`json_bytes`]); so a diagnostic
/// is at most [`diagnostic_bytes_max`] by construction, and one past it is a
/// [`ReportFault`], never a longer report. The worst report this build can
/// write lists **every code a stage raises at the cap** (each of
/// [`ONCE_PER_REPORT`] once), every part at its bound, and one `I0007` per
/// code. `report.rs`'s `the_worst_report_fits_the_answer_with_room_to_grow`
/// builds that report through this builder and the real serialiser, writes
/// it as `verify_plan` sends it (two levels deep, an id at the gateway's
/// cap, the footer), and holds it, plus eight more shares of the catalogue's
/// largest code at this cap, to the gateway's outgoing cap: the catalogue may
/// grow by eight of its largest codes before anyone must act. When it fails,
/// the fixes are to raise `MAX_ANSWER_BYTES` (the owner's, at hardening) or
/// to lower this cap (every golden that lists four of a code moves). Below
/// 2, Easy's probe has no part to ask in, and a compile-time check fails.
pub const MAX_PER_CODE: u32 = 4;

/// The most bytes one character of a player's text takes in a report once
/// [`pharmakos_proto::json::grammar::quoted`] has written it and the report's
/// JSON has escaped that: a character `quoted` escapes outside the Basic
/// Multilingual Plane is two `\uXXXX` escapes, twelve characters, each `\`
/// doubled by the JSON, so fourteen bytes. `report.rs`'s
/// `the_quoting_rule_s_worst_character_is_the_bound` measures it over every
/// Unicode scalar value.
pub const QUOTED_CHAR_JSON_BYTES_MAX: usize = 14;

/// The most bytes of the string table's own words one argument carries around
/// a quoted piece of player text (a composed `said!`: "the seat has no
/// sighting of `…`"): the longest such composition is about 60 bytes.
pub const ARGUMENT_WORDS_BYTES_MAX: usize = 256;

/// The most bytes one argument of a message adds once the report is JSON: the
/// quoting rule's bound (`MAX_QUOTED_CHARS` characters at
/// [`QUOTED_CHAR_JSON_BYTES_MAX`] each, and the cut's `…`, three bytes) and
/// the words around it ([`ARGUMENT_WORDS_BYTES_MAX`]). It also bounds a
/// number, a word of the string table and a code. An argument past it is cut
/// where it enters the report, and counted ([`Cuts::texts`]).
pub const ARGUMENT_BYTES_MAX: usize = pharmakos_proto::json::grammar::MAX_QUOTED_CHARS
    * QUOTED_CHAR_JSON_BYTES_MAX
    + ELLIPSIS_BYTES
    + ARGUMENT_WORDS_BYTES_MAX;

/// The most bytes `E0001`'s one argument, the codec's own words
/// (`MAX_ENGINE_CHARS` characters, already quoted), adds once the report is
/// JSON: four bytes a character, the most UTF-8 takes, which is also more
/// than a doubled `\`.
pub const ENGINE_ARGUMENT_BYTES_MAX: usize = pharmakos_proto::json::grammar::MAX_ENGINE_CHARS * 4;

/// The bytes of the `…` that marks a cut.
const ELLIPSIS_BYTES: usize = 3;

/// The most bytes a pointer a report names takes as JSON. The schema's own
/// pointers are far shorter (its names, indices and at most
/// [`crate::CONDITION_MAX_DEPTH`] condition levels); only a member name of the
/// player's own makes a longer one, and a pointer past this is cut where it
/// enters the report ([`bounded`]: the deepest object whose pointer fits, the
/// Fix dropped), and counted ([`Cuts::pointers`]).
pub const MAX_POINTER_BYTES: usize = 512;

/// The most related paths a diagnostic lists (every raiser names one at most).
pub const MAX_RELATED: usize = 1;

/// The most Fixes a diagnostic offers (every raiser offers one at most).
pub const MAX_FIXES: usize = 1;

/// The most map references a diagnostic carries (an area's two corners).
pub const MAX_MAP_REFS: usize = 2;

/// The most bytes a Fix's patch takes as JSON inside the report: a pointer at
/// [`MAX_POINTER_BYTES`], escaped once more by the patch's own string, twice
/// over, and a value (the stubs a Fix adds are a few hundred bytes).
pub const PATCH_BYTES_MAX: usize = 4 * MAX_POINTER_BYTES + 1024;

/// The most bytes one map reference takes as JSON: three `i32`s at their
/// widest, their keys, braces and indentation at the gateway's nesting.
pub const MAP_REF_BYTES: usize = 160;

/// What a diagnostic's JSON takes beside the bytes of its strings: its field
/// names, quotes, commas and indentation (at the gateway's nesting, inside an
/// answer), its severity and a Fix's applicability; each map reference is
/// [`MAP_REF_BYTES`] of its own. `report.rs`'s `the_framing_covers_the_serialiser` holds it to the
/// real serialiser.
pub const DIAGNOSTIC_FRAMING_BYTES: usize = 1024;

/// The most bytes one diagnostic of `code` takes as JSON, by its parts'
/// bounds; `None` for a code the string table does not have.
#[must_use]
pub fn diagnostic_bytes_max(code: &str) -> Option<usize> {
    let row = strings::text(code)?;
    let argument = argument_bytes_max(code);
    let text = |template: &str| {
        json_bytes(template).saturating_add(placeholders(template).saturating_mul(argument))
    };
    Some(
        DIAGNOSTIC_FRAMING_BYTES
            .saturating_add(json_bytes(code))
            .saturating_add(MAX_POINTER_BYTES.saturating_mul(1 + MAX_RELATED))
            .saturating_add(text(row.message))
            .saturating_add(text(row.beginner))
            .saturating_add(MAX_FIXES.saturating_mul(argument.saturating_add(PATCH_BYTES_MAX)))
            .saturating_add(MAX_MAP_REFS.saturating_mul(MAP_REF_BYTES)),
    )
}

/// The most bytes one argument of `code`'s message takes as JSON.
#[must_use]
pub fn argument_bytes_max(code: &str) -> usize {
    if code == "E0001" {
        ENGINE_ARGUMENT_BYTES_MAX
    } else {
        ARGUMENT_BYTES_MAX
    }
}

/// How many `{name}` arguments a template names.
fn placeholders(template: &str) -> usize {
    template.matches('{').count()
}

/// How many bytes `text` takes as the body of a JSON string, as
/// `pharmakos_proto::json`'s writer writes it: `"`, `\` and the five short
/// escapes in two bytes, every other C0 control in six, anything else its
/// UTF-8. `report.rs`'s `json_bytes_is_the_serialiser_s` holds it to the
/// writer on every Unicode scalar value.
#[must_use]
pub fn json_bytes(text: &str) -> usize {
    text.chars().fold(0_usize, |total, character| {
        total.saturating_add(json_char_bytes(character))
    })
}

/// [`json_bytes`] of one character.
const fn json_char_bytes(character: char) -> usize {
    match character {
        '"' | '\\' | '\u{8}' | '\u{c}' | '\n' | '\r' | '\t' => 2,
        '\u{0}'..='\u{1f}' => 6,
        other => other.len_utf8(),
    }
}

/// `text` cut on a character boundary, with a `…`, so that it takes at most
/// `bound` bytes as JSON: the quoting rule's own mark for a cut.
fn cut_text(text: &str, bound: usize) -> String {
    let room = bound.saturating_sub(ELLIPSIS_BYTES);
    let mut out = String::new();
    let mut taken = 0_usize;
    for character in text.chars() {
        let bytes = json_char_bytes(character);
        if taken.saturating_add(bytes) > room {
            break;
        }
        taken = taken.saturating_add(bytes);
        out.push(character);
    }
    out.push('…');
    out
}

/// `pointer`, or the longest run of its whole tokens whose JSON is within
/// [`MAX_POINTER_BYTES`] when it is not, and whether it was cut. A cut pointer
/// addresses an ancestor of the place, never a different place: only a member
/// name of the player's own makes a pointer that long.
#[must_use]
pub fn bounded(pointer: &str) -> (Cow<'_, str>, bool) {
    if json_bytes(pointer) <= MAX_POINTER_BYTES {
        return (Cow::Borrowed(pointer), false);
    }
    let mut kept = String::new();
    let mut bytes = 0_usize;
    // A pointer is empty or starts with `/`, so the piece before the first
    // `/` is empty and skipped.
    for token in pointer.split('/').skip(1) {
        bytes = bytes.saturating_add(json_bytes(token)).saturating_add(1);
        if bytes > MAX_POINTER_BYTES {
            break;
        }
        kept.push('/');
        kept.push_str(token);
    }
    (Cow::Owned(kept), true)
}

/// How many diagnostics' parts the report cut or dropped where they entered
/// it to keep each within its bound: a typed fact, never a silent `min`. Each
/// cut text ends with `…`; a dropped Fix, related path or map reference is
/// counted here.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Cuts {
    /// Arguments, messages, beginner sentences and Fix titles cut.
    pub texts: u64,
    /// Paths and related paths cut to an ancestor ([`bounded`]).
    pub pointers: u64,
    /// Fixes dropped: past [`MAX_FIXES`], a patch past [`PATCH_BYTES_MAX`],
    /// or a path cut (a Fix would have to spell it whole).
    pub fixes: u64,
    /// Related paths past [`MAX_RELATED`] dropped.
    pub related: u64,
    /// Map references past [`MAX_MAP_REFS`] dropped.
    pub map_refs: u64,
}

/// The report builder's own failures, kept as typed data beside the report
/// ([`crate::Counted::faults`]): each is unreachable by construction, and a
/// test holds every report to none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportFault {
    /// A cut's count past `u64`: the cut counted no further.
    CountPastU64 {
        /// The code counted.
        code: &'static str,
    },
    /// A code's listed or left-out count past `u32`, the width
    /// `gp.api.v1.LeftOut.count` carries: the diagnostic counted no further.
    CountPastU32 {
        /// The code counted.
        code: &'static str,
    },
    /// A diagnostic past [`diagnostic_bytes_max`] after every part was
    /// bounded: counted as left out instead of listed.
    DiagnosticPastItsBound {
        /// Its code.
        code: &'static str,
        /// Its bytes.
        bytes: usize,
        /// Its bound.
        max: usize,
    },
}

/// The codes a report raises at most once, whatever the playbook: each is
/// raised at one fixed place by a stage that runs once (the decode family's
/// refusals, which end the decode; the version, kind, author and fingerprint
/// checks; the size budget; the estimate's one comparison of draw with
/// supply). The derivation of [`MAX_PER_CODE`] counts each once, and
/// `tests/verifier.rs` holds every one of them to it
/// (`a_code_raised_once_a_report_has_one_raising_place`,
/// `a_code_raised_once_a_report_is_once_however_often_it_is_provoked`), so a
/// stage that starts raising one per step fails there before it can pass the
/// bound.
pub const ONCE_PER_REPORT: &[&str] = &[
    "E0001", "E0002", "E0004", "E0005", "E0006", "E0007", "E0008", "E0029", "E0030", "E0110",
    "E0601", "W0603",
];

const _: () = assert!(
    MAX_PER_CODE >= 2,
    "a cap under two leaves Easy's probe no part"
);

/// The code of decision 8's per-setting note.
const DEFAULT_NOTE: &str = "I0003";

/// The code that closes a report, once per code with diagnostics left out.
pub(crate) const LEFT_OUT: &str = "I0007";

/// How many diagnostics of one code a report has listed and left out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Tally {
    /// Whether the code is an error, read from the catalogue once per code.
    error: bool,
    /// Listed one by one, at most the cap.
    listed: u32,
    /// Counted and not built: past the cap, or (for `I0003`) after `E0110`.
    /// A count past `u32`, the width `gp.api.v1.LeftOut.count` carries, is a
    /// [`ReportFault::CountPastU32`].
    left_out: u32,
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
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Builder {
    diagnostics: Vec<Diagnostic>,
    /// The cap: [`MAX_PER_CODE`], or another for a comparison
    /// ([`crate::verify_counted_at_cap`]).
    cap: u32,
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
    /// What was cut to keep each diagnostic within its bound.
    cuts: Cuts,
    /// The builder's own failures.
    faults: Vec<ReportFault>,
}

impl Default for Builder {
    fn default() -> Builder {
        Builder::with_cap(MAX_PER_CODE)
    }
}

/// One more of a cut's count, or the fault that it cannot be.
fn bump(count: &mut u64, code: &'static str, faults: &mut Vec<ReportFault>) {
    match count.checked_add(1) {
        Some(next) => *count = next,
        None => faults.push(ReportFault::CountPastU64 { code }),
    }
}

/// One more of a code's tally, or the fault that it cannot be.
fn tally_one(count: &mut u32, code: &'static str, faults: &mut Vec<ReportFault>) {
    match count.checked_add(1) {
        Some(next) => *count = next,
        None => faults.push(ReportFault::CountPastU32 { code }),
    }
}

impl Builder {
    /// A builder listing at most `cap` diagnostics of one code: the
    /// verifier's own is [`MAX_PER_CODE`] ([`Builder::default`]); another is
    /// for a comparison of reports with and without the cap.
    pub(crate) fn with_cap(cap: u32) -> Builder {
        Builder {
            diagnostics: Vec::new(),
            cap,
            tallies: BTreeMap::new(),
            errors: false,
            over_budget: false,
            cuts: Cuts::default(),
            faults: Vec::new(),
        }
    }

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
    /// counted as left out, once the cap's number of that code are listed,
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
        let cap = self.cap;
        let tally = self.tallies.entry(code).or_insert_with(|| Tally {
            error: is_error(code),
            ..Tally::default()
        });
        self.errors |= tally.error;
        if over_budget || tally.listed >= cap {
            tally_one(&mut tally.left_out, code, &mut self.faults);
            return false;
        }
        if admit == Admit::List {
            tally_one(&mut tally.listed, code, &mut self.faults);
        }
        true
    }

    /// Whether an `I0003` note is to be written (see [`Builder::takes`]).
    pub(crate) fn takes_default_note(&mut self) -> bool {
        self.takes(DEFAULT_NOTE)
    }

    /// Counts one cut of `kind`.
    fn cut(&mut self, kind: fn(&mut Cuts) -> &mut u64, code: &'static str) {
        bump(kind(&mut self.cuts), code, &mut self.faults);
    }

    /// `text` within `bound` bytes of JSON: cut, and counted, when it is
    /// past it.
    fn within(&mut self, text: String, bound: usize, code: &'static str) -> String {
        if json_bytes(&text) <= bound {
            return text;
        }
        self.cut(|cuts| &mut cuts.texts, code);
        cut_text(&text, bound)
    }

    /// `pointer` within [`MAX_POINTER_BYTES`]: cut to an ancestor, and
    /// counted, when it is past it; and whether it was.
    fn within_pointer(&mut self, pointer: String, code: &'static str) -> (String, bool) {
        let kept = match bounded(&pointer) {
            (Cow::Owned(kept), true) => kept,
            (Cow::Borrowed(_) | Cow::Owned(_), _) => return (pointer, false),
        };
        self.cut(|cuts| &mut cuts.pointers, code);
        (kept, true)
    }

    /// Finish a diagnostic and append it, uncounted: the one door every
    /// diagnostic enters the report by, where each of its parts is held to
    /// its bound (see [`MAX_PER_CODE`]'s derivation).
    fn push(&mut self, diag: Diag) {
        let code = diag.code;
        let severity = severity_of(code);
        let argument = argument_bytes_max(code);
        let mut args = diag.args;
        for (_, value) in &mut args {
            if json_bytes(value) > argument {
                self.cut(|cuts| &mut cuts.texts, code);
                *value = Cow::Owned(cut_text(value, argument));
            }
        }
        let (path, path_cut) = self.within_pointer(diag.path, code);
        let mut related: Vec<String> = Vec::new();
        for (index, one) in diag.related.into_iter().enumerate() {
            if index >= MAX_RELATED {
                self.cut(|cuts| &mut cuts.related, code);
                continue;
            }
            related.push(self.within_pointer(one, code).0);
        }
        let mut map_refs = diag.map_refs;
        while map_refs.len() > MAX_MAP_REFS {
            map_refs.pop();
            self.cut(|cuts| &mut cuts.map_refs, code);
        }
        let mut suggestions: Vec<PatchSuggestion> = Vec::new();
        for (index, mut suggestion) in diag.suggestions.into_iter().enumerate() {
            if path_cut
                || index >= MAX_FIXES
                || json_bytes(&suggestion.json_patch) > PATCH_BYTES_MAX
            {
                self.cut(|cuts| &mut cuts.fixes, code);
                continue;
            }
            suggestion.title = self.within(suggestion.title, argument, code);
            suggestions.push(suggestion);
        }
        let (message, beginner) = match strings::text(code) {
            Some(row) => {
                let bound = |template: &str| {
                    json_bytes(template)
                        .saturating_add(placeholders(template).saturating_mul(argument))
                };
                let message = strings::fill(row.message, &args);
                let beginner = strings::fill(row.beginner, &args);
                (
                    self.within(message, bound(row.message), code),
                    self.within(beginner, bound(row.beginner), code),
                )
            }
            // Unreachable while `the_two_tables_name_the_same_codes` passes. It
            // is written out rather than asserted because a verifier that
            // panics on a playbook is worse than one that reports an ugly
            // message about itself.
            None => (
                format!("`{code}` is not in the diagnostic catalogue"),
                String::new(),
            ),
        };
        let diagnostic = Diagnostic {
            code: code.to_owned(),
            severity: i32::from(severity),
            path,
            related_paths: related,
            message,
            beginner,
            suggestions,
            map_refs,
        };
        let bytes = diagnostic_bytes(&diagnostic);
        // A code the string table does not have is unreachable (see the
        // message above); its diagnostic is listed as it is.
        if let Some(max) = diagnostic_bytes_max(code)
            && bytes > max
        {
            self.faults
                .push(ReportFault::DiagnosticPastItsBound { code, bytes, max });
            if let Some(tally) = self.tallies.get_mut(code) {
                tally_one(&mut tally.left_out, code, &mut self.faults);
            }
            return;
        }
        self.diagnostics.push(diagnostic);
    }

    /// Whether anything so far stops the playbook qualifying.
    ///
    /// Spec section 11: a playbook qualifies when it has **zero errors**. A
    /// warning is information, not a refusal.
    ///
    /// Every error raised counts, listed or left out past the cap: the cap
    /// changes what a report lists and never whether it qualifies.
    pub(crate) fn has_errors(&self) -> bool {
        self.errors
    }

    /// What the cap left out, per code in code order (the tallies are an
    /// ordered map): the facts `gp.api.v1.VerifyReport.left_out` carries and
    /// each closing `I0007` is rendered from.
    fn gaps(&self) -> Vec<Gap> {
        self.tallies
            .iter()
            .filter(|(_, tally)| tally.left_out > 0)
            .map(|(code, tally)| Gap {
                code,
                count: tally.left_out,
                max: self.cap,
            })
            .collect()
    }

    /// The diagnostics, in the order they were emitted, then, in code order,
    /// one `I0007` for each code with diagnostics left out, rendered from the
    /// same fact as its `gp.api.v1.LeftOut` entry and worded by the left-out
    /// code's severity; and the report's typed facts. An `I0007` never passes
    /// through the cap and is never counted against one: there is at most one
    /// per code.
    pub(crate) fn finish(mut self) -> Finished {
        let gaps = self.gaps();
        for gap in &gaps {
            let verdict = match severity_of(gap.code) {
                Severity::Error | Severity::Unspecified => strings::LEFT_OUT_STILL_REFUSE,
                Severity::Warning | Severity::Info => "",
            };
            let budget = if gap.code == DEFAULT_NOTE && self.over_budget {
                strings::LEFT_OUT_PAST_THE_BUDGET
            } else {
                ""
            };
            let next = match severity_of(gap.code) {
                Severity::Info => strings::LEFT_OUT_NEXT_NOTE,
                Severity::Error | Severity::Warning | Severity::Unspecified => {
                    strings::LEFT_OUT_NEXT_FIX
                }
            };
            self.push(
                Diag::new(LEFT_OUT, crate::pointer::ROOT)
                    .arg_word("code", gap.code)
                    .arg("count", gap.count)
                    .arg("max", gap.max)
                    .arg_word("verdict", verdict)
                    .arg_word("budget", budget)
                    .arg_word("next", next),
            );
        }
        Finished {
            diagnostics: self.diagnostics,
            left_out: gaps
                .into_iter()
                .map(|gap| LeftOut {
                    code: gap.code.to_owned(),
                    count: gap.count,
                    max: gap.max,
                })
                .collect(),
            cuts: self.cuts,
            faults: self.faults,
        }
    }
}

/// One code's diagnostics the report counted and did not list, while the
/// report is built: the code still the catalogue's own `&'static str`, so the
/// `I0007` can name it without a copy.
struct Gap {
    /// The code left out.
    code: &'static str,
    /// How many of that code the report counted and did not list.
    count: u32,
    /// The cap: how many of the code the report could list.
    max: u32,
}

/// A finished report's diagnostics and its typed facts.
pub(crate) struct Finished {
    /// The diagnostics, the closing `I0007`s last.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// What the cap left out, per code in code order:
    /// `gp.api.v1.VerifyReport.left_out`.
    pub(crate) left_out: Vec<LeftOut>,
    /// What was cut to keep each diagnostic within its bound.
    pub(crate) cuts: Cuts,
    /// The builder's own failures.
    pub(crate) faults: Vec<ReportFault>,
}

/// The bytes a diagnostic takes as JSON, by its strings' [`json_bytes`] and
/// [`DIAGNOSTIC_FRAMING_BYTES`].
#[must_use]
pub fn diagnostic_bytes(diagnostic: &Diagnostic) -> usize {
    let strings = [
        diagnostic.code.as_str(),
        diagnostic.path.as_str(),
        diagnostic.message.as_str(),
        diagnostic.beginner.as_str(),
    ]
    .into_iter()
    .chain(diagnostic.related_paths.iter().map(String::as_str))
    .chain(
        diagnostic
            .suggestions
            .iter()
            .flat_map(|fix| [fix.title.as_str(), fix.json_patch.as_str()]),
    )
    .map(json_bytes)
    .fold(0_usize, usize::saturating_add);
    DIAGNOSTIC_FRAMING_BYTES
        .saturating_add(strings)
        .saturating_add(diagnostic.map_refs.len().saturating_mul(MAP_REF_BYTES))
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
        self.finish().diagnostics
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

    // -----------------------------------------------------------------------
    // The cap, its bounds and its derivation (S3's `vcap`)
    // -----------------------------------------------------------------------

    use super::{
        ARGUMENT_BYTES_MAX, Cuts, MAX_MAP_REFS, MAX_PER_CODE, MAX_POINTER_BYTES, ONCE_PER_REPORT,
        PATCH_BYTES_MAX, QUOTED_CHAR_JSON_BYTES_MAX, ReportFault, argument_bytes_max, cut_text,
        diagnostic_bytes, diagnostic_bytes_max, json_bytes,
    };
    use crate::catalogue::{CATALOGUE, Emitter, Entry as CatalogueRow};
    use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
    use pharmakos_proto::gp::api::v1::{Diagnostic, PatchSuggestion, VerifyReport};
    use pharmakos_proto::gp::v1::Voxel;
    use pharmakos_proto::json::grammar::{MAX_QUOTED_CHARS, quoted};
    use std::borrow::Cow;

    /// Every Unicode scalar value, in runs the tests below take a string of.
    fn scalar_runs() -> Vec<String> {
        let mut runs = Vec::new();
        let mut run = String::new();
        for code in 0_u32..=0x10_ffff {
            if let Some(character) = char::from_u32(code) {
                run.push(character);
                if run.len() > 16_000 {
                    runs.push(core::mem::take(&mut run));
                }
            }
        }
        runs.push(run);
        runs
    }

    /// `json_bytes` is the bytes the serialiser writes, on every scalar value.
    #[test]
    fn json_bytes_is_the_serialiser_s() {
        // The writer ends a document with a newline; the string is the rest.
        let body = |text: &str| -> usize {
            pharmakos_proto::json::write(&Json::String(text.to_owned()))
                .trim_end_matches('\n')
                .len()
                .saturating_sub(2)
        };
        for run in scalar_runs() {
            assert_eq!(body(&run), json_bytes(&run));
        }
    }

    /// The quoting rule's worst character, over every scalar value, is
    /// `QUOTED_CHAR_JSON_BYTES_MAX` bytes in a report; a quoted text at the
    /// rule's cut is within the argument bound.
    #[test]
    fn the_quoting_rule_s_worst_character_is_the_bound() {
        let mut worst = 0_usize;
        for code in 0_u32..=0x10_ffff {
            if let Some(character) = char::from_u32(code) {
                let one = quoted(&character.to_string());
                worst = worst.max(json_bytes(one.as_str()));
            }
        }
        assert_eq!(worst, QUOTED_CHAR_JSON_BYTES_MAX);
        let tags: String = core::iter::repeat_n('\u{e0041}', 200).collect();
        let cut = quoted(&tags);
        assert_eq!(
            json_bytes(cut.as_str()),
            MAX_QUOTED_CHARS * QUOTED_CHAR_JSON_BYTES_MAX + 3
        );
        assert!(json_bytes(cut.as_str()) <= ARGUMENT_BYTES_MAX);
    }

    /// `diagnostic_bytes` covers what the serialiser writes for a
    /// diagnostic, at the gateway's nesting (an answer's `result`, its
    /// `report`, its `diagnostics`: three more levels of two spaces on every
    /// line), on a diagnostic at every bound.
    #[test]
    fn the_framing_covers_the_serialiser() {
        for code in ["E0003", "E0404", "W0702", "I0007"] {
            let mut builder = Builder::with_cap(MAX_PER_CODE);
            builder.push(worst_diag(code));
            let finished = builder.finish();
            let diagnostic = finished.diagnostics.first().expect("listed").clone();
            let empty = VerifyReport::default();
            let one = VerifyReport {
                diagnostics: vec![diagnostic.clone()],
                ..VerifyReport::default()
            };
            let alone = pharmakos_proto::json::encode(&empty).expect("encodes");
            let with = pharmakos_proto::json::encode(&one).expect("encodes");
            let lines = with.lines().count().saturating_sub(alone.lines().count());
            let written = with
                .len()
                .saturating_sub(alone.len())
                .saturating_add(lines.saturating_mul(6));
            assert!(
                written <= diagnostic_bytes(&diagnostic),
                "{code}: {written} bytes written, {} counted",
                diagnostic_bytes(&diagnostic)
            );
        }
    }

    /// A diagnostic of `code` with every part at or past its bound: every
    /// argument its strings name past the argument bound, a path and a
    /// related path exactly at the pointer bound (so its Fix is kept), a Fix
    /// whose title is past the argument bound and whose patch is at its
    /// bound, two map references at the widest numbers.
    fn worst_diag(code: &'static str) -> Diag {
        let row = crate::strings::text(code).expect("a code with strings");
        let mut diag = Diag::new(
            code,
            format!("/{}", "a".repeat(MAX_POINTER_BYTES.saturating_sub(1))),
        )
        .related(format!(
            "/{}",
            "b".repeat(MAX_POINTER_BYTES.saturating_sub(1))
        ));
        let long = "x".repeat(argument_bytes_max(code).saturating_mul(2));
        for template in [row.message, row.beginner] {
            for name in template
                .split('{')
                .skip(1)
                .filter_map(|tail| tail.split_once('}').map(|(name, _)| name))
            {
                diag.args.push((name, Cow::Owned(long.clone())));
            }
        }
        diag.suggestions.push(PatchSuggestion {
            title: long,
            json_patch: "p".repeat(PATCH_BYTES_MAX),
            applicability: i32::from(Applicability::MaybeIncorrect),
        });
        for _ in 0..=MAX_MAP_REFS {
            diag = diag.map_ref(Voxel {
                x: i32::MIN,
                y: i32::MIN,
                z: i32::MIN,
            });
        }
        diag
    }
    /// How many more of the catalogue's largest codes the worst report must
    /// leave room for in the gateway's answer: the catalogue may grow by this
    /// many of its largest codes before anyone must act (S3's `outcap`, fix
    /// pass 1).
    const GROWTH_CODES: usize = 8;
    // The ruled eight, held at compile time: lowering it is a ruling, not an edit.
    const _: () = assert!(GROWTH_CODES == 8);

    /// The report the builder writes when `codes` (rows of the catalogue a
    /// stage raises) are each raised one more time than `cap` (once for a
    /// code raised once a report), every part at its bound.
    fn report_of(codes: &[&CatalogueRow], cap: u32) -> VerifyReport {
        let mut builder = Builder::with_cap(cap);
        for row in codes {
            let times = if ONCE_PER_REPORT.contains(&row.code) {
                1
            } else {
                cap.saturating_add(1)
            };
            for _ in 0..times {
                builder.emit(worst_diag(row.code));
            }
        }
        let finished = builder.finish();
        assert!(finished.faults.is_empty(), "{:?}", finished.faults);
        VerifyReport {
            qualifies: false,
            depth: 2,
            diagnostics: finished.diagnostics,
            report_hash: vec![0xff; 8],
            rules_hash: vec![0xff; 8],
            plan_fingerprint: vec![0xff; 8],
            verifier_version: crate::VERIFIER_VERSION.to_owned(),
            size_units: u32::MAX,
            size_budget: u32::MAX,
            // The typed entries are sent too, one per code left out of, so
            // the answer's bytes count them.
            left_out: finished.left_out,
        }
    }

    /// Every catalogue row a stage raises.
    fn stage_rows() -> Vec<&'static CatalogueRow> {
        CATALOGUE
            .iter()
            .filter(|row| matches!(row.emitter, Emitter::Stage(_)))
            .collect()
    }

    /// The bytes `report` takes as `verify_plan` sends it: two levels deep
    /// under `result` and `report` (each of its lines indented four bytes
    /// more), under an id at the gateway's cap, with a `_status` footer.
    fn sent_bytes(report: &VerifyReport) -> usize {
        use pharmakos_proto::json::Json;
        use pharmakos_proto::transport::MAX_ID_BYTES;
        let text = pharmakos_proto::json::encode(report).expect("a report encodes");
        let value = pharmakos_proto::json::read(&text).expect("a report reads back");
        let footer = Json::Object(
            ["phase", "phase_remaining_ms", "segment_length_ms", "round"]
                .iter()
                .map(|name| ((*name).to_owned(), Json::Number(i32::MIN.to_string())))
                .collect(),
        );
        let answer = Json::Object(vec![
            ("jsonrpc".to_owned(), Json::String("2.0".to_owned())),
            ("id".to_owned(), Json::String("x".repeat(MAX_ID_BYTES))),
            (
                "result".to_owned(),
                Json::Object(vec![
                    ("report".to_owned(), value),
                    ("_status".to_owned(), footer),
                ]),
            ),
        ]);
        pharmakos_proto::json::write(&answer).len()
    }

    /// The bytes the largest single code adds to a report as it is sent, at
    /// `cap`: its listed diagnostics and its `I0007`.
    fn largest_share(cap: u32) -> (usize, &'static str) {
        let empty = sent_bytes(&report_of(&[], cap));
        stage_rows()
            .into_iter()
            .map(|row| {
                (
                    sent_bytes(&report_of(&[row], cap)).saturating_sub(empty),
                    row.code,
                )
            })
            .max()
            .expect("a catalogue of some codes")
    }

    /// **The worst report fits the gateway's answer, with room to grow** (S3's
    /// `outcap`, fix pass 1, design call B): the worst report at
    /// `MAX_PER_CODE`, as `verify_plan` sends it, plus `GROWTH_CODES` shares of
    /// the catalogue's largest code, is within the gateway's outgoing cap.
    #[test]
    fn the_worst_report_fits_the_answer_with_room_to_grow() {
        use pharmakos_proto::transport::MAX_ANSWER_BYTES;
        let worst = sent_bytes(&report_of(&stage_rows(), MAX_PER_CODE));
        let (share, code) = largest_share(MAX_PER_CODE);
        let grown = worst.saturating_add(share.saturating_mul(GROWTH_CODES));
        let room = MAX_ANSWER_BYTES
            .saturating_sub(worst)
            .checked_div(share.max(1))
            .unwrap_or(0);
        println!(
            "the worst report as sent: {worst} bytes; the largest code, {code}: {share} bytes; with \
             {GROWTH_CODES} more of it: {grown}; against {MAX_ANSWER_BYTES}: {room} more fit"
        );
        assert!(
            grown <= MAX_ANSWER_BYTES,
            "the worst report as `verify_plan` sends it ({worst} bytes) and {GROWTH_CODES} more of \
             the largest code ({code}, {share} bytes each) come to {grown} bytes, past the \
             gateway's {MAX_ANSWER_BYTES}: raise `MAX_ANSWER_BYTES` (the owner's, at hardening) \
             or lower `MAX_PER_CODE` (every golden that lists four of a code moves)"
        );
    }

    /// One more than `MAX_PER_CODE`, as a published figure and not a rule:
    /// whether it would fit the answer too.
    #[test]
    fn one_more_of_a_code_is_measured() {
        use pharmakos_proto::transport::MAX_ANSWER_BYTES;
        let past = sent_bytes(&report_of(&stage_rows(), MAX_PER_CODE.saturating_add(1)));
        println!(
            "the worst report at {}: {past} bytes as sent, {} the gateway's {MAX_ANSWER_BYTES}",
            MAX_PER_CODE.saturating_add(1),
            if past <= MAX_ANSWER_BYTES {
                "within"
            } else {
                "past"
            }
        );
        assert!(past > 0);
    }

    /// The model is an upper bound: no diagnostic the builder lists is past
    /// `diagnostic_bytes_max`, so the builder records no fault on any code's
    /// worst diagnostic.
    #[test]
    fn every_code_s_worst_diagnostic_is_within_its_bound() {
        for row in CATALOGUE {
            let mut builder = Builder::default();
            builder.emit(worst_diag(row.code));
            let finished = builder.finish();
            assert!(
                finished.faults.is_empty(),
                "{}: {:?}",
                row.code,
                finished.faults
            );
            for diagnostic in &finished.diagnostics {
                let max = diagnostic_bytes_max(&diagnostic.code).expect("a bound");
                assert!(diagnostic_bytes(diagnostic) <= max, "{}", diagnostic.code);
            }
        }
    }

    /// Each part past its bound is cut and counted: never a silent `min`.
    #[test]
    fn a_part_past_its_bound_is_cut_and_counted() {
        let mut builder = Builder::default();
        let deep = format!("/{}", "k".repeat(MAX_POINTER_BYTES.saturating_mul(3)));
        builder.emit(
            Diag::new("E0404", format!("/declarative{deep}"))
                .related("/a")
                .related("/b")
                .map_ref(Voxel { x: 1, y: 1, z: 1 })
                .map_ref(Voxel { x: 2, y: 2, z: 2 })
                .map_ref(Voxel { x: 3, y: 3, z: 3 })
                .fix("t", String::from("[]"), Applicability::MaybeIncorrect),
        );
        let mut long = Diag::new("E0405", "/x");
        long.args.push((
            "id",
            Cow::Owned("y".repeat(ARGUMENT_BYTES_MAX.saturating_mul(3))),
        ));
        builder.emit(long);
        let finished = builder.finish();
        assert_eq!(
            finished.cuts,
            Cuts {
                texts: 1,
                pointers: 1,
                fixes: 1,
                related: 1,
                map_refs: 1,
            }
        );
        let first = finished.diagnostics.first().expect("listed");
        assert_eq!(
            first.path, "/declarative",
            "cut to the deepest fitting ancestor"
        );
        assert!(
            first.suggestions.is_empty(),
            "the Fix of a cut path is dropped"
        );
        let second = finished.diagnostics.get(1).expect("listed");
        assert!(second.message.contains('…'), "{}", second.message);
        // The argument was cut, not the message: the template's own words after
        // the argument are whole.
        assert!(
            second
                .message
                .ends_with("…` is a beacon this seat does not own."),
            "{}",
            second.message
        );
        assert!(cut_text("abc", 4).ends_with('…'));
    }

    /// The closing `I0007`s never pass through the cap: seven codes past the
    /// cap close with seven, one per code, in code order.
    #[test]
    fn every_code_left_out_of_has_its_i0007() {
        let mut builder = Builder::default();
        let codes = [
            "E0301", "E0302", "E0303", "E0305", "E0401", "E0403", "W0702",
        ];
        for code in codes {
            for _ in 0..=MAX_PER_CODE {
                builder.emit(Diag::new(code, "/declarative/route/0"));
            }
        }
        let finished = builder.finish();
        let markers: Vec<&Diagnostic> = finished
            .diagnostics
            .iter()
            .filter(|found| found.code == "I0007")
            .collect();
        assert_eq!(markers.len(), codes.len());
        for (marker, code) in markers.iter().zip(codes) {
            assert!(
                marker.message.starts_with(&format!("1 more `{code}`")),
                "{}",
                marker.message
            );
        }
    }

    /// `I0007`'s words are true for the code they name: an error's say the
    /// left-out ones still refuse; a warning's and a note's say nothing about
    /// qualifying; only `I0003`'s name the size budget, and only once it was
    /// passed; a note's beginner sentence does not ask to deal with anything.
    #[test]
    fn an_i0007_s_words_are_its_code_s() {
        let words = |code: &'static str| -> Diagnostic {
            let mut builder = Builder::default();
            for _ in 0..=MAX_PER_CODE {
                builder.emit(Diag::new(code, "/declarative/route/0"));
            }
            builder
                .finish()
                .diagnostics
                .into_iter()
                .find(|found| found.code == "I0007")
                .expect("an I0007")
        };
        let error = words("E0303");
        assert!(
            error
                .message
                .contains("still stops the playbook qualifying")
        );
        assert!(!error.message.contains("size budget"));
        assert!(error.beginner.contains("Deal with"));
        let warning = words("W0702");
        assert!(!warning.message.contains("qualifying"));
        assert!(warning.beginner.contains("Deal with"));
        let note = words("I0003");
        assert!(!note.message.contains("qualifying"));
        assert!(
            !note.message.contains("size budget"),
            "the budget is named only once it was passed: {}",
            note.message
        );
        assert!(!note.beginner.contains("Deal with"));
        // Past the size budget, `I0003`'s words say so.
        let mut builder = Builder::default();
        builder.emit(Diag::new("E0110", "/declarative"));
        builder.emit(Diag::new("I0003", "/declarative/route/0/on_fail"));
        let past = builder
            .finish()
            .diagnostics
            .into_iter()
            .find(|found| found.code == "I0007")
            .expect("an I0007");
        assert!(past.message.contains("size budget"), "{}", past.message);
        for found in [&error, &warning, &note] {
            assert!(!found.message.contains('{') && !found.beginner.contains('{'));
        }
    }

    /// A count past `u64` is a fault, not a wrap or a silent stop.
    #[test]
    fn a_count_past_u64_is_a_fault() {
        let mut count = u64::MAX;
        let mut faults = Vec::new();
        super::bump(&mut count, "E0303", &mut faults);
        assert_eq!(count, u64::MAX);
        assert_eq!(faults, vec![ReportFault::CountPastU64 { code: "E0303" }]);
    }

    /// A code's tally past `u32`, the width `LeftOut.count` carries, is a
    /// fault, not a wrap or a silent stop.
    #[test]
    fn a_tally_past_u32_is_a_fault() {
        let mut count = u32::MAX;
        let mut faults = Vec::new();
        super::tally_one(&mut count, "E0303", &mut faults);
        assert_eq!(count, u32::MAX);
        assert_eq!(faults, vec![ReportFault::CountPastU32 { code: "E0303" }]);
    }

    /// The typed entries and the closing `I0007`s are one fact: one entry per
    /// code left out of, in code order, each with its count and the cap, and
    /// one `I0007` for each, in the same order, naming the same count.
    #[test]
    fn each_i0007_is_its_left_out_entry() {
        let mut builder = Builder::default();
        // Emitted out of code order: the entries come in code order anyway.
        for (code, times) in [("W0702", 6_u32), ("E0303", 5), ("E0301", 9)] {
            for _ in 0..times {
                builder.emit(Diag::new(code, "/declarative/route/0"));
            }
        }
        builder.emit(Diag::new("E0302", "/declarative/route/0"));
        let finished = builder.finish();
        let entries: Vec<(&str, u32, u32)> = finished
            .left_out
            .iter()
            .map(|gap| (gap.code.as_str(), gap.count, gap.max))
            .collect();
        assert_eq!(
            entries,
            vec![
                ("E0301", 5, MAX_PER_CODE),
                ("E0303", 1, MAX_PER_CODE),
                ("W0702", 2, MAX_PER_CODE),
            ]
        );
        let markers: Vec<&str> = finished
            .diagnostics
            .iter()
            .filter(|found| found.code == "I0007")
            .map(|found| found.message.as_str())
            .collect();
        assert_eq!(markers.len(), entries.len());
        for (marker, (code, count, _)) in markers.iter().zip(&entries) {
            assert!(
                marker.starts_with(&format!("{count} more `{code}`")),
                "{marker}"
            );
        }
        // Past the size budget an `I0003` is left out with none listed.
        let mut builder = Builder::default();
        builder.emit(Diag::new("E0110", "/declarative"));
        builder.emit(Diag::new("I0003", "/declarative/route/0/on_fail"));
        let finished = builder.finish();
        assert_eq!(
            finished
                .left_out
                .iter()
                .map(|gap| (gap.code.as_str(), gap.count))
                .collect::<Vec<_>>(),
            vec![("I0003", 1)]
        );
    }
}
