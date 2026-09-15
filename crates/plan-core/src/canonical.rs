// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The canonical form, which is **authoritative for the file format**.
//!
//! Spec section 13: "The plan core's canonical form is authoritative, and
//! golden tests byte-compare the output." The canonical *JSON* is
//! `crates/proto`'s — field names as the `.proto` spells them, fields in
//! field-number order, enums as bare value names, durations as bare numbers
//! because every duration field is `int32` (decisions-log item 46), unknown
//! fields rejected rather than stripped. What this module adds is the half
//! that makes a playbook a **JSONC** file: the comments go back where they
//! were written.
//!
//! # How a comment finds its way home
//!
//! Canonicalising reorders an object's members and drops the ones whose value
//! is the proto3 default, so a comment cannot simply be copied byte-for-byte
//! from one text to the other. Each comment is anchored to the **JSON Pointer**
//! of the member or element it was written against ([`crate::jsonc::Anchor`]),
//! and pointers are stable across canonicalisation: an object member is named
//! by its key and an array element by its index, and canonicalisation changes
//! neither.
//!
//! The one case where a pointer does *not* survive is a member that was
//! written with its default value — `"minor": 0` on a schema version, say —
//! which the canonical form omits. A comment on such a member would be
//! destroyed by a naive rewrite, so instead it is **relocated** to the nearest
//! surviving ancestor and reported in [`Canonical::relocated`]. Nothing is
//! ever silently dropped: `tests/golden/plan-core/README.md` says a dropped
//! comment is a bug even when the JSON is identical, and this is the mechanism
//! that keeps that true.
//!
//! # Two ways to keep a file
//!
//! * **Save what was loaded** — [`crate::jsonc::Document::to_text`] gives back
//!   the exact bytes, which is spec section 13's "Exact round-trip" and is
//!   what the editor does to a file it did not change.
//! * **Save canonically** — [`canonicalise`], which is what `submit_plan`
//!   hashes and what the goldens pin.

use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json::{self, Json};

use crate::error::{Error, push_pointer, split_pointer};
use crate::jsonc::{Anchor, Comment, CommentKind, Document};

/// The message a playbook file is. Templates and samples are the same message
/// with a different `kind` (decisions-log item 76).
pub const PLAYBOOK_MESSAGE: &str = "gp.v1.Playbook";

/// A comment that could not stay where it was written, because the member it
/// was written against is not in the canonical form.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Relocation {
    /// The comment, delimiters included.
    pub comment: String,
    /// The pointer it was written against.
    pub from: String,
    /// The pointer it was moved to. `""` means the document header.
    pub to: String,
}

/// A canonicalised file.
#[derive(Clone, PartialEq, Debug)]
pub struct Canonical {
    /// The playbook itself, decoded. Everything else in this crate that reads
    /// a playbook — the prose, the interface-time arithmetic, the `$`/`kW`
    /// projection, the size meter — reads this rather than the text, so there
    /// is one decode per file rather than one per question.
    pub playbook: Playbook,
    /// The canonical JSONC text, comments included, ending in a newline.
    pub text: String,
    /// The canonical JSON text with no comments at all: the bytes the verifier
    /// hashes and the plan fingerprint is taken over.
    pub json: String,
    /// Comments the canonical form had no home for, each moved to the nearest
    /// surviving ancestor rather than dropped.
    pub relocated: Vec<Relocation>,
}

/// Canonicalises a playbook document.
///
/// The route is deliberately **through the decoded message**: the document's
/// JSON is decoded into a `gp.v1.Playbook` and re-encoded from it, so a field
/// somebody wrote out at its proto3 default — `"minor": 0` on a schema
/// version — is normalised away exactly as `tests/golden/proto/`'s committed
/// canonical form has it. Going value-to-value through the codec's
/// `json_to_wire`/`wire_to_json` pair instead would keep the written-out
/// default and give the canonical form two spellings, which is the one thing
/// it may not have (`crates/proto/src/json/codec.rs`, the note against T8).
///
/// # Errors
///
/// Anything the schema refuses: an unknown field, a value that does not fit
/// its field, two members of one `oneof` set at once. The error carries the
/// JSON Pointer of the node that caused it.
pub fn canonicalise(document: &Document) -> Result<Canonical, Error> {
    let playbook: Playbook = json::decode_json(&document.to_json())?;
    let value = json::encode_json(&playbook)?;

    let mut relocated: Vec<Relocation> = Vec::new();
    let mut placed: Vec<(Anchor, Comment)> = Vec::new();
    for comment in document.comments() {
        let anchor = rehome(&value, &comment, &mut relocated);
        placed.push((anchor, comment));
    }

    let mut text = String::new();
    for comment in placed
        .iter()
        .filter(|(anchor, _)| *anchor == Anchor::Prologue)
    {
        text.push_str(&comment.1.text);
        text.push('\n');
    }
    write_value(&value, "", 0, &placed, &mut text);
    text.push('\n');
    for comment in placed
        .iter()
        .filter(|(anchor, _)| *anchor == Anchor::Epilogue)
    {
        text.push_str(&comment.1.text);
        text.push('\n');
    }

    Ok(Canonical {
        playbook,
        text,
        json: json::write(&value),
        relocated,
    })
}

/// Canonicalises a playbook file's text: read it, check it against the schema,
/// and write it back in canonical form with its comments.
///
/// # Errors
///
/// As [`canonicalise`], plus a syntax error at `/byte/NNN`.
pub fn canonicalise_text(text: &str) -> Result<Canonical, Error> {
    canonicalise(&Document::parse(text)?)
}

/// The canonical JSON bytes of a playbook file, comments discarded.
///
/// This is what `verify_plan` hashes, so two files that differ only in their
/// comments verify identically — which is the property that lets a player
/// annotate a playbook without moving its `report_hash`.
///
/// # Errors
///
/// As [`canonicalise_text`].
pub fn canonical_json(text: &str) -> Result<String, Error> {
    Ok(canonicalise_text(text)?.json)
}

/// Finds the anchor a comment can actually be written at, recording a
/// relocation when it is not the one it was written at.
fn rehome(value: &Json, comment: &Comment, relocated: &mut Vec<Relocation>) -> Anchor {
    let Some(pointer) = comment.anchor.pointer() else {
        return comment.anchor.clone();
    };
    if exists(value, pointer) {
        return comment.anchor.clone();
    }
    let mut tokens = split_pointer(pointer).unwrap_or_default();
    while !tokens.is_empty() {
        tokens.pop();
        let ancestor = tokens
            .iter()
            .fold(String::new(), |acc, token| push_pointer(&acc, token));
        if ancestor.is_empty() || exists(value, &ancestor) {
            relocated.push(Relocation {
                comment: comment.text.clone(),
                from: pointer.to_owned(),
                to: ancestor.clone(),
            });
            return if ancestor.is_empty() {
                Anchor::Prologue
            } else {
                Anchor::Before(ancestor)
            };
        }
    }
    relocated.push(Relocation {
        comment: comment.text.clone(),
        from: pointer.to_owned(),
        to: String::new(),
    });
    Anchor::Prologue
}

/// Whether a pointer resolves in a JSON value.
fn exists(value: &Json, pointer: &str) -> bool {
    let Ok(tokens) = split_pointer(pointer) else {
        return false;
    };
    let mut here = value;
    for token in tokens {
        let next = match here {
            Json::Object(entries) => entries
                .iter()
                .find(|(key, _)| *key == token)
                .map(|(_, item)| item),
            Json::Array(items) => token
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get(index)),
            _ => None,
        };
        match next {
            Some(child) => here = child,
            None => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------
// The writer
// ---------------------------------------------------------------------------

/// Two spaces per level, the same indent `crates/proto`'s writer uses.
///
/// `a_comment_free_file_is_exactly_what_the_codec_writes` pins the two
/// together, so the JSONC writer cannot drift from the canonical JSON writer
/// it is a superset of.
fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn comments_at<'a>(
    placed: &'a [(Anchor, Comment)],
    wanted: &Anchor,
) -> impl Iterator<Item = &'a Comment> {
    let wanted = wanted.clone();
    placed
        .iter()
        .filter(move |(anchor, _)| *anchor == wanted)
        .map(|(_, comment)| comment)
}

/// Writes one own-line comment at `depth`.
fn write_own_line(comment: &Comment, depth: usize, out: &mut String) {
    indent(depth, out);
    out.push_str(&comment.text);
    out.push('\n');
}

/// Writes one trailing comment, on the line that has just been written.
///
/// A line comment swallows the rest of its line, which is why the caller emits
/// the separating comma *before* calling this and the newline after it.
fn write_trailing(comment: &Comment, out: &mut String) {
    out.push(' ');
    match comment.kind {
        CommentKind::Line | CommentKind::Block => out.push_str(&comment.text),
    }
}

fn write_value(
    value: &Json,
    pointer: &str,
    depth: usize,
    placed: &[(Anchor, Comment)],
    out: &mut String,
) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(lexeme) => out.push_str(lexeme),
        Json::String(text) => write_string(text, out),
        Json::Array(items) => {
            let inner = depth.saturating_add(1);
            if items.is_empty() {
                write_empty(pointer, depth, placed, ('[', ']'), out);
                return;
            }
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                let child = push_pointer(pointer, &index.to_string());
                for comment in comments_at(placed, &Anchor::Before(child.clone())) {
                    write_own_line(comment, inner, out);
                }
                indent(inner, out);
                write_value(item, &child, inner, placed, out);
                if index.saturating_add(1) < items.len() {
                    out.push(',');
                }
                for comment in comments_at(placed, &Anchor::After(child)) {
                    write_trailing(comment, out);
                }
                out.push('\n');
            }
            for comment in comments_at(placed, &Anchor::Inside(pointer.to_owned())) {
                write_own_line(comment, inner, out);
            }
            indent(depth, out);
            out.push(']');
        }
        Json::Object(entries) => {
            let inner = depth.saturating_add(1);
            if entries.is_empty() {
                write_empty(pointer, depth, placed, ('{', '}'), out);
                return;
            }
            out.push_str("{\n");
            for (index, (key, item)) in entries.iter().enumerate() {
                let child = push_pointer(pointer, key);
                for comment in comments_at(placed, &Anchor::Before(child.clone())) {
                    write_own_line(comment, inner, out);
                }
                indent(inner, out);
                write_string(key, out);
                out.push_str(": ");
                write_value(item, &child, inner, placed, out);
                if index.saturating_add(1) < entries.len() {
                    out.push(',');
                }
                for comment in comments_at(placed, &Anchor::After(child)) {
                    write_trailing(comment, out);
                }
                out.push('\n');
            }
            for comment in comments_at(placed, &Anchor::Inside(pointer.to_owned())) {
                write_own_line(comment, inner, out);
            }
            indent(depth, out);
            out.push('}');
        }
    }
}

/// An empty container. `{}` unless a comment was written inside it, in which
/// case it opens up to hold the comment rather than losing it.
fn write_empty(
    pointer: &str,
    depth: usize,
    placed: &[(Anchor, Comment)],
    brackets: (char, char),
    out: &mut String,
) {
    let inside: Vec<&Comment> = comments_at(placed, &Anchor::Inside(pointer.to_owned())).collect();
    out.push(brackets.0);
    if inside.is_empty() {
        out.push(brackets.1);
        return;
    }
    out.push('\n');
    for comment in inside {
        write_own_line(comment, depth.saturating_add(1), out);
    }
    indent(depth, out);
    out.push(brackets.1);
}

/// The same escape set `crates/proto`'s canonical writer uses.
fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if u32::from(other) < 0x20 => {
                out.push_str("\\u");
                for shift in [12u32, 8, 4, 0] {
                    let nibble = u32::from(other).wrapping_shr(shift) & 0xF;
                    out.push(char::from_digit(nibble, 16).unwrap_or('0'));
                }
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::{canonical_json, canonicalise_text};

    /// The smallest file the schema accepts, written the awkward way round:
    /// `kind` first (it is field 7 and belongs last) and a written-out default.
    pub(super) const MINIMAL: &str = concat!(
        "{\"kind\":\"PLAYBOOK\",\"schema_version\":{\"major\":1,\"minor\":0},",
        "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
        "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
        "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
        "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}}}"
    );

    #[test]
    fn fields_come_back_in_field_number_order() {
        let canonical = canonicalise_text(MINIMAL).expect("a playbook the schema accepts");
        let schema_at = canonical.json.find("schema_version").unwrap_or(usize::MAX);
        let kind_at = canonical.json.find("\"kind\"").unwrap_or(0);
        assert!(
            schema_at < kind_at,
            "`kind` is field 7 and belongs last:\n{}",
            canonical.json
        );
    }

    #[test]
    fn a_written_out_default_is_normalised_away() {
        let canonical = canonicalise_text(MINIMAL).expect("a playbook the schema accepts");
        assert!(
            !canonical.json.contains("minor"),
            "`minor: 0` is the proto3 default and the canonical form omits it:\n{}",
            canonical.json
        );
    }

    #[test]
    fn a_default_valued_member_is_dropped_and_its_comment_relocates() {
        let text = MINIMAL.replace(
            "\"minor\":0",
            "\n// why the minor version is zero\n\"minor\":0",
        );
        let canonical = canonicalise_text(&text).expect("a playbook the schema accepts");
        assert_eq!(canonical.relocated.len(), 1, "{:?}", canonical.relocated);
        assert_eq!(
            canonical.relocated.first().map(|moved| moved.from.clone()),
            Some("/schema_version/minor".to_owned())
        );
        assert_eq!(
            canonical.relocated.first().map(|moved| moved.to.clone()),
            Some("/schema_version".to_owned())
        );
        assert!(
            canonical.text.contains("// why the minor version is zero"),
            "a relocated comment is still in the file:\n{}",
            canonical.text
        );
    }

    #[test]
    fn a_comment_free_file_is_exactly_what_the_codec_writes() {
        let canonical = canonicalise_text(MINIMAL).expect("a playbook the schema accepts");
        assert_eq!(canonical.text, canonical.json);
        assert_eq!(
            canonical.json,
            pharmakos_proto::json::encode(&canonical.playbook).expect("the codec writes it too")
        );
    }

    #[test]
    fn comments_do_not_move_the_canonical_json() {
        let plain = canonical_json(MINIMAL).expect("a playbook");
        let noisy =
            canonical_json(&MINIMAL.replace("{\"kind\"", "// a note\n{/* another */\"kind\""))
                .expect("the same playbook with comments");
        assert_eq!(plain, noisy);
    }

    #[test]
    fn an_unknown_field_is_refused_with_a_pointer_rather_than_a_byte() {
        let error = canonicalise_text(&MINIMAL.replace("\"kind\"", "\"nope\":1,\"kind\""))
            .expect_err("an unknown field");
        assert!(!error.is_syntax(), "{error}");
    }

    #[test]
    fn a_comment_at_the_end_of_a_container_stays_there() {
        let text = MINIMAL.replace(
            "{\"ms\":1000}}]",
            "{\"ms\":1000}}\n// the route ends here\n]",
        );
        let canonical = canonicalise_text(&text).expect("a playbook");
        assert!(canonical.relocated.is_empty(), "{:?}", canonical.relocated);
        assert!(
            canonical.text.contains("// the route ends here\n    ]"),
            "{}",
            canonical.text
        );
    }
}
