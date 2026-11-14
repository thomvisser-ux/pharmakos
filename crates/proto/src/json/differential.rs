// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The differential tests: the codec against `super::reference`, the codec as
//! it stood before S3's `vfyq`, on the same generated documents (decisions-log
//! item 158 (2)).
//!
//! Every property is "equal bytes, or the same error": the same `Json` from
//! [`read`], the same wire bytes from [`json_to_wire`], the same tree from
//! [`wire_to_json`], the same text from [`write_wire`] as from `write` over
//! the reference's tree, and the same canonical text — the bytes a plan
//! fingerprint is taken over — whether it is written from the bytes a playbook
//! was decoded from or, as before, from the playbook encoded again.
//!
//! The documents come from a seeded generator that walks the schema: every
//! message, every field kind, both spellings of a field name (and sometimes
//! both at once), defaults spelled every way the codec accepts, `null`s,
//! escapes, unknown and reserved names, values that do not fit, two arms of a
//! `oneof`, syntax broken at a random byte, and wire bytes broken at a random
//! byte. The seeds are fixed, so a failure is reproducible from its seed.
//!
//! The reference and these tests are kept until the old path is deleted, and
//! committed vectors replace them then.

use std::fmt::Write as _;

use prost::Message as _;

use super::direct;
use super::index::{IndexedField, index};
use super::reference;
use super::{
    Error, Json, canonicalise, decode_json_encoded, json_to_encoded, json_to_wire, read,
    wire_to_json, write, write_wire, write_wire_into,
};
use crate::descriptor::{ScalarKind, schema};
use crate::gp::v1::Playbook;

/// splitmix64: small, seeded, and the same on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        let n = u64::try_from(n.max(1)).unwrap();
        usize::try_from(self.next() % n).unwrap()
    }

    fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        *items.get(self.below(items.len())).unwrap()
    }
}

/// Names `gp.v1` reserves; the verifier's decode stage reports them as `E0003`.
const RESERVED: &[&str] = &["set_flag", "branch", "team_id", "repeat", "flag_set"];

const STRINGS: &[&str] = &[
    "",
    "a",
    "east ridge",
    "line\nbreak",
    "tab\tand \"quotes\" and \\ backslash",
    "unicode: é ü 漢字 🜁",
    "control \u{1} \u{1f} \u{7f}",
    "slash / and ~ tilde",
    "h00_a",
];

const SIGNED: &[&str] = &[
    "0",
    "-0",
    "00",
    "1",
    "-1",
    "7",
    "1000",
    "+3",
    "2147483647",
    "-2147483648",
];
const SIGNED_BAD: &[&str] = &[
    "2147483648",
    "1.5",
    "1e3",
    "-",
    "0x10",
    "9223372036854775808",
];
const UNSIGNED: &[&str] = &["0", "00", "1", "42", "4294967295", "65536"];
const UNSIGNED_BAD: &[&str] = &["-1", "4294967296", "2.0", "18446744073709551616"];
const WIDE: &[&str] = &[
    "0",
    "-0",
    "1",
    "-9223372036854775808",
    "9223372036854775807",
    "123456789012",
];
const BASE64: &[&str] = &["", "AQID", "AAAAAAAAAAE=", "====", "Zg", "-_-_", "AQ=="];
const BASE64_BAD: &[&str] = &["!!!!", "A", "AQIDB"];

struct Gen {
    rng: Rng,
    dirty: bool,
}

impl Gen {
    fn message(&mut self, at: usize, depth: usize) -> Json {
        let message = index().get(at).unwrap();
        let mut entries: Vec<(String, Json)> = Vec::new();
        let mut oneofs: Vec<i32> = Vec::new();
        let keep = 2_usize.saturating_add(depth);
        for field in &message.fields {
            if !self.rng.chance(keep) {
                continue;
            }
            if field.field.kind == ScalarKind::Message && depth >= 9 {
                continue;
            }
            if let Some(oneof) = field.field.oneof_index {
                if oneofs.contains(&oneof) && !(self.dirty && self.rng.chance(4)) {
                    continue;
                }
                oneofs.push(oneof);
            }
            let spelled = if self.rng.chance(4) {
                field.field.json_name.clone()
            } else {
                field.field.name.clone()
            };
            if self.rng.chance(25) {
                entries.push((spelled.clone(), Json::Null));
                continue;
            }
            let value = self.field(field, depth);
            entries.push((spelled, value));
            // Both spellings at once, which the codec accepts as one field
            // written twice. Not for a oneof member, which is refused.
            let twice = field.field.oneof_index.is_none() || self.dirty;
            if twice && field.field.name != field.field.json_name && self.rng.chance(12) {
                let other = if self.rng.chance(2) {
                    field.field.json_name.clone()
                } else {
                    field.field.name.clone()
                };
                let value = self.field(field, depth);
                entries.push((other, value));
            }
        }
        if self.dirty && self.rng.chance(30) {
            entries.push(("wombat".to_owned(), Json::Number("1".to_owned())));
        }
        if self.dirty && self.rng.chance(40) {
            let name = self.rng.pick(RESERVED);
            entries.push((name.to_owned(), Json::Object(Vec::new())));
        }
        if self.dirty
            && self.rng.chance(60)
            && let Some((key, _)) = entries.first().cloned()
        {
            entries.push((key, Json::Null));
        }
        for at in (1..entries.len()).rev() {
            let other = self.rng.below(at.saturating_add(1));
            entries.swap(at, other);
        }
        Json::Object(entries)
    }

    fn field(&mut self, field: &IndexedField, depth: usize) -> Json {
        if field.field.repeated {
            if self.dirty && self.rng.chance(30) {
                return self.scalar(field, depth);
            }
            let count = self.rng.below(4);
            return Json::Array((0..count).map(|_| self.scalar(field, depth)).collect());
        }
        self.scalar(field, depth)
    }

    fn number(&mut self, good: &[&str], bad: &[&str]) -> Json {
        let lexeme = if self.dirty && self.rng.chance(15) {
            self.rng.pick(bad)
        } else {
            self.rng.pick(good)
        };
        if self.rng.chance(6) {
            Json::String(lexeme.to_owned())
        } else {
            Json::Number(lexeme.to_owned())
        }
    }

    fn scalar(&mut self, field: &IndexedField, depth: usize) -> Json {
        if self.dirty && self.rng.chance(40) {
            return match self.rng.below(4) {
                0 => Json::Bool(true),
                1 => Json::String("x".to_owned()),
                2 => Json::Array(Vec::new()),
                _ => Json::Object(Vec::new()),
            };
        }
        match field.field.kind {
            ScalarKind::Message => match field.nested {
                Some(nested) => self.message(nested, depth.saturating_add(1)),
                None => Json::Object(Vec::new()),
            },
            ScalarKind::Bool => Json::Bool(self.rng.chance(2)),
            ScalarKind::Int32 | ScalarKind::Sint32 | ScalarKind::Sfixed32 => {
                self.number(SIGNED, SIGNED_BAD)
            }
            ScalarKind::Uint32 | ScalarKind::Fixed32 => self.number(UNSIGNED, UNSIGNED_BAD),
            ScalarKind::Int64
            | ScalarKind::Sint64
            | ScalarKind::Uint64
            | ScalarKind::Fixed64
            | ScalarKind::Sfixed64 => self.number(WIDE, SIGNED_BAD),
            ScalarKind::String => Json::String(self.rng.pick(STRINGS).to_owned()),
            ScalarKind::Bytes => {
                let text = if self.dirty && self.rng.chance(10) {
                    self.rng.pick(BASE64_BAD)
                } else {
                    self.rng.pick(BASE64)
                };
                Json::String(text.to_owned())
            }
            ScalarKind::Enum => {
                let declared = field
                    .enumeration
                    .expect("every enum field's enum is indexed");
                let value = declared
                    .values
                    .get(self.rng.below(declared.values.len()))
                    .expect("every enum declares a value");
                match self.rng.below(8) {
                    0 => Json::Number(value.number.to_string()),
                    1 => Json::Number("99".to_owned()),
                    2 if self.dirty => Json::String("WOMBAT".to_owned()),
                    _ => Json::String(value.name.clone()),
                }
            }
            ScalarKind::Double | ScalarKind::Float => Json::Number("1".to_owned()),
        }
    }

    /// The value as text, with whitespace and escapes chosen at random.
    fn text(&mut self, value: &Json) -> String {
        let mut out = String::new();
        self.space(&mut out);
        self.write(value, &mut out);
        self.space(&mut out);
        out
    }

    fn space(&mut self, out: &mut String) {
        match self.rng.below(6) {
            0 => out.push(' '),
            1 => out.push_str("\n  "),
            2 => out.push_str("\r\n\t"),
            _ => {}
        }
    }

    fn write(&mut self, value: &Json, out: &mut String) {
        match value {
            Json::Null => out.push_str("null"),
            Json::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
            Json::Number(lexeme) => out.push_str(lexeme),
            Json::String(text) => self.string(text, out),
            Json::Array(items) => {
                out.push('[');
                for (at, item) in items.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    self.space(out);
                    self.write(item, out);
                    self.space(out);
                }
                out.push(']');
            }
            Json::Object(entries) => {
                out.push('{');
                for (at, (key, item)) in entries.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    self.space(out);
                    self.string(key, out);
                    self.space(out);
                    out.push(':');
                    self.space(out);
                    self.write(item, out);
                    self.space(out);
                }
                out.push('}');
            }
        }
    }

    fn string(&mut self, text: &str, out: &mut String) {
        out.push('"');
        for character in text.chars() {
            match character {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '/' if self.rng.chance(2) => out.push_str("\\/"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                other if u32::from(other) < 0x20 => {
                    write!(out, "\\u{:04x}", u32::from(other)).unwrap();
                }
                other if u32::from(other) > 0xffff && self.rng.chance(2) => {
                    let mut units = [0_u16; 2];
                    for unit in other.encode_utf16(&mut units) {
                        write!(out, "\\u{unit:04X}").unwrap();
                    }
                }
                other if self.rng.chance(20) && u32::from(other) <= 0xffff => {
                    write!(out, "\\u{:04x}", u32::from(other)).unwrap();
                }
                other => out.push(other),
            }
        }
        out.push('"');
    }

    /// Breaks the text at a random byte, the way a hand edit or a truncated
    /// file does.
    fn corrupt(&mut self, text: &str) -> String {
        let cut = self.rng.below(text.len().saturating_add(1));
        let cut = (0..=cut)
            .rev()
            .find(|at| text.is_char_boundary(*at))
            .unwrap_or(0);
        let (head, tail) = text.split_at(cut);
        match self.rng.below(5) {
            0 => head.to_owned(),
            1 => format!(
                "{head}{}{tail}",
                self.rng
                    .pick(&["{", "}", "[", "]", ",", ":", "\"", "\\", "x", "\u{1}", "-"])
            ),
            2 => format!("{head}{tail} trailing"),
            3 => format!("{head}\\u12{tail}"),
            _ => format!("{head}{}", tail.chars().skip(1).collect::<String>()),
        }
    }
}

/// The fast codec and the reference agree on one document's text, all the way
/// down. Returns the wire bytes when the document encodes.
fn agree_on_text(full_name: &str, text: &str) -> Option<Vec<u8>> {
    // The direct reader answers exactly when the exact path accepts, and with
    // the exact path's bytes and plainness.
    let exact = read(text)
        .ok()
        .and_then(|value| json_to_encoded(full_name, &value).ok());
    assert_eq!(
        direct::text_to_encoded(full_name, text),
        exact,
        "the direct reader: {text}"
    );

    let ours = read(text);
    assert_eq!(ours, reference::read(text), "read: {text}");
    let value = ours.ok()?;

    let ours = json_to_wire(full_name, &value);
    assert_eq!(
        ours,
        reference::json_to_wire(full_name, &value),
        "json_to_wire: {text}"
    );
    assert_eq!(
        canonicalise(full_name, text),
        reference_canonicalise(full_name, text),
        "canonicalise: {text}"
    );
    let bytes = ours.ok()?;
    let encoded = json_to_encoded(full_name, &value).expect("as json_to_wire");
    assert_eq!(encoded.bytes(), bytes.as_slice());
    agree_on_wire(full_name, &bytes);
    Some(bytes)
}

fn reference_canonicalise(full_name: &str, text: &str) -> Result<String, Error> {
    let value = reference::read(text)?;
    let bytes = reference::json_to_wire(full_name, &value)?;
    Ok(reference::write(&reference::wire_to_json(
        full_name, &bytes,
    )?))
}

/// The fast codec and the reference agree on one message's wire bytes.
fn agree_on_wire(full_name: &str, bytes: &[u8]) {
    let ours = wire_to_json(full_name, bytes);
    let theirs = reference::wire_to_json(full_name, bytes);
    assert_eq!(ours, theirs, "wire_to_json: {bytes:?}");
    assert_eq!(
        write_wire(full_name, bytes),
        theirs.map(|tree| reference::write(&tree)),
        "write_wire: {bytes:?}"
    );
    if let Ok(tree) = wire_to_json(full_name, bytes) {
        assert_eq!(write(&tree), reference::write(&tree), "write");
    }
}

/// For a playbook, the canonical text written from the decoded bytes is the
/// canonical text of the playbook encoded again — whole, and with the
/// fingerprint cleared, which is what a plan fingerprint is taken over —
/// whenever the bytes are plain. Returns whether they were.
fn agree_on_playbook(value: &Json) -> Option<bool> {
    let decoded = decode_json_encoded::<Playbook>(value).ok()?;
    let text = write(value);
    assert_eq!(
        super::decode_direct::<Playbook>(&text).as_ref(),
        Some(&decoded),
        "decode_direct"
    );
    assert_eq!(
        super::decode_encoded::<Playbook>(&text).as_ref(),
        Ok(&decoded)
    );
    let name = "gp.v1.Playbook";
    let playbook = decoded.message();
    let again = playbook.encode_to_vec();
    let theirs = reference::write(&reference::wire_to_json(name, &again).expect("prost's bytes"));
    assert_eq!(
        super::encode(playbook).as_deref(),
        Ok(theirs.as_str()),
        "encode"
    );

    let mut cleared = playbook.clone();
    if let Some(meta) = cleared.meta.as_mut() {
        meta.fingerprint.clear();
    }
    let canonical = reference::write(
        &reference::wire_to_json(name, &cleared.encode_to_vec()).expect("prost's bytes"),
    );
    let plain = decoded.encoded().is_plain();
    if plain {
        assert_eq!(
            write_wire(name, decoded.encoded().bytes()).as_deref(),
            Ok(theirs.as_str()),
            "write_wire over plain bytes"
        );
        let mut ours = String::new();
        write_wire_into(
            name,
            decoded.encoded().bytes(),
            &["meta", "fingerprint"],
            &mut ours,
        )
        .expect("plain bytes write");
        assert_eq!(ours, canonical, "the fingerprint's bytes");
    }
    Some(plain)
}

#[test]
fn every_generated_playbook_reads_encodes_and_writes_as_the_reference_does() {
    let at = index().find("gp.v1.Playbook").unwrap();
    let mut plain = 0_usize;
    let mut not_plain = 0_usize;
    let mut refused = 0_usize;
    for seed in 0..1_500_u64 {
        let mut generator = Gen {
            rng: Rng(seed),
            dirty: seed % 3 == 2,
        };
        let value = generator.message(at, 0);
        let mut text = generator.text(&value);
        if generator.dirty && generator.rng.chance(4) {
            text = generator.corrupt(&text);
        }
        let Some(bytes) = agree_on_text("gp.v1.Playbook", &text) else {
            refused = refused.saturating_add(1);
            continue;
        };
        match agree_on_playbook(&read(&text).unwrap()) {
            Some(true) => plain = plain.saturating_add(1),
            Some(false) => not_plain = not_plain.saturating_add(1),
            None => {}
        }
        // The same bytes, broken at a random byte.
        let mut broken = bytes.clone();
        if !broken.is_empty() {
            let at = generator.rng.below(broken.len());
            match generator.rng.below(3) {
                0 => broken.truncate(at),
                1 => {
                    if let Some(byte) = broken.get_mut(at) {
                        *byte ^= 1 << generator.rng.below(8);
                    }
                }
                _ => broken.insert(at, 0x80),
            }
            agree_on_wire("gp.v1.Playbook", &broken);
        }
    }
    // The generator reaches every outcome, or the test proves less than it
    // says.
    assert!(plain > 200, "plain playbooks: {plain}");
    assert!(
        not_plain > 10,
        "playbooks written with a field twice: {not_plain}"
    );
    assert!(refused > 100, "refused documents: {refused}");
}

#[test]
fn every_message_in_the_schema_reads_encodes_and_writes_as_the_reference_does() {
    let names: Vec<String> = schema()
        .messages()
        .map(|message| message.full_name.clone())
        .collect();
    for (offset, name) in names.iter().enumerate() {
        let at = index().find(name).unwrap();
        for seed in 0..12_u64 {
            let mut generator = Gen {
                rng: Rng(seed
                    .wrapping_mul(7919)
                    .wrapping_add(u64::try_from(offset).unwrap())),
                dirty: seed % 3 == 2,
            };
            let value = generator.message(at, 0);
            let text = generator.text(&value);
            agree_on_text(name, &text);
        }
    }
}

#[test]
fn a_document_nested_past_the_limit_is_refused_as_the_reference_refuses_it() {
    for levels in [60_usize, 63, 64, 65, 70, 130] {
        let mut text = String::from(r#"{"segment_elapsed":{"ms":{"op":"EQ","value":5}}}"#);
        for _ in 0..levels {
            text = format!("{{\"not\":{{\"item\":{text}}}}}");
        }
        agree_on_text("gp.v1.Condition", &text);
        let mut text = String::from("{}");
        for _ in 0..levels {
            text = format!("[{text}]");
        }
        agree_on_text("gp.v1.Condition", &text);
    }
}

#[test]
fn a_field_spelled_twice_makes_the_bytes_not_plain() {
    for (text, plain) in [
        (r#"{"on_death":{"on_respawn":"CONTINUE"}}"#, true),
        (
            r#"{"on_death":{"on_respawn":"CONTINUE"},"onDeath":{"max_deaths_before_fallback":1}}"#,
            false,
        ),
        (r#"{"meta":{"fingerprint":"===="}}"#, false),
        (r#"{"meta":{"fingerprint":"AAAAAAAAAAE="}}"#, true),
    ] {
        let value = read(text).unwrap();
        let encoded = json_to_encoded("gp.v1.Playbook", &value).unwrap();
        assert_eq!(encoded.is_plain(), plain, "{text}");
        agree_on_playbook(&value);
    }
}
