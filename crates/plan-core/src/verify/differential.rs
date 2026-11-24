// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The two readers of a player's text held to one answer (S3's `vfyq2`).
//!
//! `verify_jsonc` blanks a file's comments and hands the codec's reader the
//! rest; `render_plan`, `patch_plan` and `submit_plan`'s compile read the same
//! file through the JSONC reader, comments and all. Both are one grammar
//! (`pharmakos_proto::json::grammar`) and one comment scanner
//! (`crate::jsonc::comment_end`), so they must agree on every text: the same
//! accept or refuse, a refusal at the same byte in the same words, and an
//! accepted text to the same canonical bytes. These tests hold that with no
//! exception:
//!
//! * [`reference_hand_over`] is the JSONC chain: the canonical JSON
//!   [`canonicalise_text`] writes, or the blanked file when there is none.
//!   Every text's [`hand_over`] must be its bytes exactly, so a `report_hash`
//!   cannot depend on which reader read the file.
//! * [`readers_agree`] compares the two readers themselves, the JSONC reader
//!   over the text and the codec's reader over its blanked copy.
//!
//! The texts are every committed playbook and every committed JSON file
//! beside them, the two shapes `quick_cost.rs` times, mutations of each (a
//! comment at a token boundary, a byte changed, a truncation, another
//! indentation, members reordered, defaults written out, a field under its
//! JSON name and under both names), the edge cases where two hand-written
//! readers could part, and arbitrary bytes. Mutations are drawn from a seeded
//! generator, so a failure names the seed and the text that failed.

#![cfg(test)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_methods,
    clippy::too_many_lines,
    reason = "a test: a panic is its failure report, its arithmetic is on small counts it made \
              itself, its one read_dir listing is sorted before use (clippy.toml's own \
              sanction), and its edge cases are one table"
)]

use std::path::{Path, PathBuf};

use pharmakos_proto::json::{self, Json};

use super::{HandedOver, blank_comments, hand_over};
use crate::canonical::{Canonical, canonicalise, canonicalise_text};
use crate::jsonc::Document;

/// The JSONC chain: the canonical JSON `canonicalise_text` writes, or the
/// text by the module's rule for a refusal: blanked up to the byte the JSONC
/// reader refuses it at (its own refusal, read off its pointer), as written
/// from there; blanked throughout when the schema refuses it.
fn reference_hand_over(text: &str) -> Handed {
    match canonicalise_text(text) {
        Ok(canonical) => Handed::Canonical(canonical.json),
        Err(refused) => {
            let blanked = blank_comments(text).expect("blanked");
            let at = refused
                .pointer()
                .strip_prefix("/byte/")
                .map(|offset| offset.parse::<usize>().expect("a byte offset"));
            Handed::Blanked(match at {
                Some(at) => format!(
                    "{}{}",
                    blanked
                        .get(..at)
                        .unwrap_or_else(|| panic!("blanked {at} of {text:?} -> {blanked:?}")),
                    &text[at..]
                ),
                None => blanked,
            })
        }
    }
}

/// What a text is handed over as: its canonical text, or its blanked text.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Handed {
    Canonical(String),
    Blanked(String),
}

/// [`hand_over`]'s answer, as [`reference_hand_over`] spells one.
fn lean_hand_over(text: &str) -> Handed {
    match hand_over(text).expect("a refusal at a character boundary") {
        HandedOver::Canonical(canonical) => Handed::Canonical(canonical.into_text()),
        HandedOver::Refused { bytes, .. } => Handed::Blanked(bytes),
    }
}

/// The JSONC reader over `text` and the codec's reader over its blanked copy
/// agree: both accept, or both refuse at the same byte in the same words.
fn readers_agree(what: &str, text: &str) {
    let jsonc = Document::parse(text)
        .map(|_| ())
        .map_err(|error| (error.pointer().to_owned(), error.message().to_owned()));
    let codec = json::read(&blank_comments(text).expect("blanked"))
        .map(|_| ())
        .map_err(|error| (error.pointer().to_owned(), error.message().to_owned()));
    if jsonc != codec {
        let shown: String = text.chars().take(400).collect();
        panic!(
            "{what}: the readers part\n  JSONC: {jsonc:?}\n  codec: {codec:?}\n  text ({} bytes): \
             {shown:?}",
            text.len()
        );
    }
}

/// The answers seen, so each arm is shown to be taken.
#[derive(Default, Debug)]
struct Seen {
    canonical: u64,
    blanked: u64,
    texts: u64,
}

impl Seen {
    fn check(&mut self, what: &str, text: &str) {
        readers_agree(what, text);
        decode_agrees(what, text);
        let expected = reference_hand_over(text);
        let handed = lean_hand_over(text);
        if handed != expected {
            let shown: String = text.chars().take(400).collect();
            panic!(
                "{what}: the lean route hands over other bytes than the JSONC chain\n  lean: {}\n  \
                 chain: {}\n  text ({} bytes): {shown:?}",
                summary(&handed),
                summary(&expected),
                text.len(),
            );
        }
        self.texts += 1;
        match handed {
            Handed::Canonical(_) => self.canonical += 1,
            Handed::Blanked(bytes) => {
                self.blanked += 1;
                refused_reports_agree(what, text, &bytes);
            }
        }
    }
}

/// `decode_jsonc`, the one decode the gateway's readers share (fix pass 1,
/// item 3), answers what the JSONC chain answers: the same message and
/// canonical JSON for a file with a canonical form, and the same pointer and
/// words for one without. Past the playbook's cap it refuses by
/// `Bound::Size`, as `verify_jsonc` does and before any reading; the chain,
/// which the editor's writer uses on texts the gateway has already bounded,
/// has no cap of its own.
fn decode_agrees(what: &str, text: &str) {
    let lean = super::decode_jsonc(text);
    if pharmakos_verifier::is_too_long(text) {
        assert_eq!(
            lean.map_err(|error| error.bound()),
            Err(Some(crate::error::Bound::Size)),
            "{what}: past the cap"
        );
        return;
    }
    let chain = crate::canonical::canonicalise_text(text);
    match (&lean, &chain) {
        (Ok(lean), Ok(chain)) => {
            assert_eq!(lean.text(), chain.json, "{what}: the canonical JSON");
            assert_eq!(
                lean.decoded().message(),
                &chain.playbook,
                "{what}: the message"
            );
        }
        (Err(lean), Err(chain)) => assert_eq!(
            (lean.pointer(), lean.message()),
            (chain.pointer(), chain.message()),
            "{what}: the refusal"
        ),
        _ => panic!("{what}: decode_jsonc answers {lean:?}, the JSONC chain {chain:?}"),
    }
}

/// The committed rules table, read once.
fn shared_rules() -> &'static pharmakos_sim::rules::RulesTable {
    static RULES: std::sync::OnceLock<pharmakos_sim::rules::RulesTable> =
        std::sync::OnceLock::new();
    RULES.get_or_init(|| {
        pharmakos_sim::rules::RulesTable::load(&workspace_root().join("rules/rules.v1.json"))
            .expect("the committed rules table")
    })
}

/// A refused text's report, made from the refusal its one decode found
/// (`Input::refused`, which reads nothing again), is the report the verifier
/// makes of the same bytes by decoding them (`Input::new`): fix pass 1, item
/// 10.
fn refused_reports_agree(what: &str, text: &str, bytes: &str) {
    use pharmakos_proto::gp::api::v1::verify_plan::Depth;
    let rules = shared_rules();
    let scope = pharmakos_verifier::Scope::new(
        pharmakos_sim::tables::SeatId::new(0),
        pharmakos_sim::knowledge::SeatEconomy::default(),
    );
    let carried = super::verify_jsonc(text, &[], &scope, rules, Depth::Quick).expect("a report");
    let input =
        pharmakos_verifier::Input::new(bytes.as_bytes(), &[], &scope, rules).expect("an input");
    let read_again = pharmakos_verifier::verify(&input, Depth::Quick);
    assert_eq!(
        carried, read_again,
        "{what}: the carried refusal reports otherwise"
    );
}
fn summary(handed: &Handed) -> String {
    let (kind, text) = match handed {
        Handed::Canonical(text) => ("canonical", text),
        Handed::Blanked(text) => ("blanked", text),
    };
    let head: String = text.chars().take(160).collect();
    format!("{kind}, {} bytes: {head:?}", text.len())
}

/// splitmix64, the repository's seeded generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            let n = u64::try_from(n).expect("a count fits u64");
            usize::try_from(self.next() % n).expect("below a usize")
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/plan-core sits two levels below the workspace root")
        .to_path_buf()
}

/// Every `.json` and `.jsonc` file under `dir`, recursively, in path order.
fn files_under(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| entry.expect("a dir entry").path())
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            files_under(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json" || extension == "jsonc")
        {
            found.push(path);
        }
    }
}

/// The committed corpus: every playbook, template, sample, scenario and
/// verifier case, and every JSON golden beside them.
fn corpus() -> Vec<(String, String)> {
    let root = workspace_root();
    let mut paths = Vec::new();
    for dir in [
        "examples",
        "library",
        "scenarios",
        "tests/golden",
        "crates/verifier/tests/cases",
        "crates/plan-core/tests/cases",
        "docs/demo",
        "godot/fixtures",
    ] {
        files_under(&root.join(dir), &mut paths);
    }
    assert!(
        paths.len() > 300,
        "the corpus is the committed files: {}",
        paths.len()
    );
    paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).expect("a committed file is UTF-8");
            let name = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            (name, text)
        })
        .collect()
}

/// One predicate of a 24-node condition (`crates/verifier/tests/quick_cost.rs`).
fn leaf(value: usize) -> String {
    format!(r#"{{"segment_elapsed":{{"ms":{{"op":"EQ","value":{value}}}}}}}"#)
}

/// A 24-node condition: 21 predicates under three nested groups.
fn condition(id: usize, next: &mut usize) -> String {
    let leaves: Vec<String> = (0..21)
        .map(|_| {
            *next += 1;
            leaf(*next)
        })
        .collect();
    let group = if id.is_multiple_of(2) { "any" } else { "all" };
    let wrap = |inner: String| format!(r#"{{"{group}":{{"items":[{inner}]}}}}"#);
    wrap(wrap(wrap(leaves.join(","))))
}

const HEAD: &str = r#""schema_version":{"major":1},"meta":{"title":"vfyq2","author_kind":"HUMAN"},"kind":"PLAYBOOK""#;
const TAIL: &str = r#""on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":1},"fallback":{"hold":{"at":{"beacon_anchor":{"safest":{}}}}}"#;

/// `quick_cost.rs`' two shapes at the size budget: 63 handlers and 127 route
/// steps, each with a 24-node condition.
fn shapes() -> Vec<(String, String)> {
    let mut next = 0;
    let handlers: Vec<String> = (0..63)
        .map(|id| {
            format!(
                r#"{{"id":"h{id:02}","when":{},"body":[{{"label":"h{id:02}_a","hold":{{"ms":1000}}}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":2}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    let handlers = format!(
        r#"{{{HEAD},"declarative":{{"route":[{{"label":"r00","hold":{{"ms":1000}}}}],"handlers":[{}]}},{TAIL}}}"#,
        handlers.join(",")
    );
    let mut next = 0;
    let steps: Vec<String> = (0..127)
        .map(|id| {
            format!(
                r#"{{"label":"r{id:03}","skip_if":{},"hold":{{"ms":1000}}}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    let route = format!(
        r#"{{{HEAD},"declarative":{{"route":[{}]}},{TAIL}}}"#,
        steps.join(",")
    );
    vec![
        ("the handlers shape".to_owned(), handlers),
        ("the route shape".to_owned(), route),
    ]
}

/// The byte offsets in `text` that stand between two tokens, outside its
/// strings: where whitespace, and so a comment, may stand.
fn boundaries(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for (at, &byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
                found.push(at + 1);
            }
            continue;
        }
        match byte {
            b'"' => {
                found.push(at);
                in_string = true;
            }
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                found.push(at);
                found.push(at + 1);
            }
            b' ' | b'\t' | b'\r' | b'\n' => found.push(at),
            _ => {}
        }
    }
    found.push(bytes.len());
    found.sort_unstable();
    found.dedup();
    found
}

/// The comments injected at a boundary: a line comment and a block comment,
/// each holding a multi-byte character and the characters a comment reader
/// could mistake for its end or for a string.
const COMMENTS: [&str; 7] = [
    "// a note, \u{e9}t\u{e9} \"quoted\" /* not a block */\n",
    "/* a note, \u{1f4a1} // not a line \"*/",
    "/**/",
    "/* across\n   two lines */",
    "// a line\n/* then a block\n */",
    // A file with CR line ends (fix pass 2, item D4: `line_break_lf_only`
    // survived): a line comment ends at the lone CR, and a block comment
    // spans one.
    "// a CR-only line\r",
    "/* across\r a CR */\r",
];

fn with_comment(text: &str, at: usize, comment: &str) -> String {
    let mut out = String::with_capacity(text.len() + comment.len());
    out.push_str(&text[..at]);
    out.push_str(comment);
    out.push_str(&text[at..]);
    out
}

/// Characters a changed byte is drawn from: JSON's own, a comment's, and some
/// that are neither.
const MUTANTS: &[u8] = b"{}[]:,\"\\/*0123456789-+.eEtrufalsn \t\r\nxZ\x0b\x0c\x00";

/// The mutations of one text, drawn from `rng`.
fn mutations(text: &str, rng: &mut Rng, comments_at: usize) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let spots = boundaries(text);
    let every = spots.len() <= comments_at;
    let picks: Vec<usize> = if every {
        spots.clone()
    } else {
        (0..comments_at)
            .map(|_| spots[rng.below(spots.len())])
            .collect()
    };
    for (index, at) in picks.iter().enumerate() {
        let comment = COMMENTS[index % COMMENTS.len()];
        out.push((
            format!("a comment at byte {at}"),
            with_comment(text, *at, comment),
        ));
    }
    // A byte changed, where it is ASCII, so the text stays UTF-8.
    for _ in 0..4 {
        let ascii: Vec<usize> = (0..3)
            .map(|_| rng.below(text.len()))
            .filter(|at| text.as_bytes().get(*at).is_some_and(u8::is_ascii))
            .collect();
        if let Some(at) = ascii.first() {
            let mut bytes = text.as_bytes().to_vec();
            bytes[*at] = MUTANTS[rng.below(MUTANTS.len())];
            if let Ok(changed) = String::from_utf8(bytes) {
                out.push((format!("byte {at} changed"), changed));
            }
        }
    }
    // Truncated at a character boundary.
    for _ in 0..2 {
        let mut at = rng.below(text.len() + 1);
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        out.push((format!("truncated at byte {at}"), text[..at].to_owned()));
    }
    // An unclosed block comment at a boundary.
    let at = spots[rng.below(spots.len())];
    out.push((
        format!("an unclosed comment at byte {at}"),
        with_comment(text, at, "/* never closed"),
    ));
    // The tree, rewritten.
    if let Ok(tree) = json::read(&blank_comments(text).expect("blanked")) {
        out.push(("compact".to_owned(), compact(&tree)));
        out.push(("indented".to_owned(), json::write(&tree)));
        let mut reordered = tree.clone();
        reverse_members(&mut reordered);
        out.push(("members reversed".to_owned(), compact(&reordered)));
        let mut defaulted = tree.clone();
        write_defaults(&mut defaulted);
        out.push(("defaults written".to_owned(), json::write(&defaulted)));
        let mut renamed = tree.clone();
        if rename(&mut renamed, "schema_version", "schemaVersion", false) {
            out.push(("a field under its JSON name".to_owned(), compact(&renamed)));
        }
        let mut both = tree;
        if rename(&mut both, "schema_version", "schemaVersion", true) {
            out.push(("a field under both names".to_owned(), compact(&both)));
        }
    }
    out
}

fn compact(tree: &Json) -> String {
    let mut out = String::new();
    write_compact(tree, &mut out);
    out
}

fn write_compact(tree: &Json, out: &mut String) {
    match tree {
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
                out.push_str(&json::write(&Json::String(key.clone())));
                out.push(':');
                write_compact(item, out);
            }
            out.push('}');
        }
        scalar => out.push_str(&json::write(scalar)),
    }
}

fn reverse_members(tree: &mut Json) {
    match tree {
        Json::Array(items) => items.iter_mut().for_each(reverse_members),
        Json::Object(entries) => {
            entries.reverse();
            for (_, item) in entries.iter_mut() {
                reverse_members(item);
            }
        }
        _ => {}
    }
}

/// Writes out, at their proto3 defaults, members the canonical form leaves
/// out: a version's `minor`, and a `null` for a `meta`'s fingerprint.
fn write_defaults(tree: &mut Json) {
    match tree {
        Json::Array(items) => items.iter_mut().for_each(write_defaults),
        Json::Object(entries) => {
            for (key, item) in entries.iter_mut() {
                if key == "schema_version"
                    && let Json::Object(version) = item
                    && !version.iter().any(|(name, _)| name == "minor")
                {
                    version.push(("minor".to_owned(), Json::Number("0".to_owned())));
                }
                if key == "meta"
                    && let Json::Object(meta) = item
                    && !meta.iter().any(|(name, _)| name == "fingerprint")
                {
                    meta.push(("fingerprint".to_owned(), Json::Null));
                }
                write_defaults(item);
            }
        }
        _ => {}
    }
}

/// Renames the root's `from` member to `to`, or with `keep` adds `to` beside
/// it; whether there was one.
fn rename(tree: &mut Json, from: &str, to: &str, keep: bool) -> bool {
    let Json::Object(entries) = tree else {
        return false;
    };
    let Some(index) = entries.iter().position(|(key, _)| key == from) else {
        return false;
    };
    if keep {
        let value = entries[index].1.clone();
        entries.push((to.to_owned(), value));
    } else {
        entries[index].0 = to.to_owned();
    }
    true
}

/// A plain playbook the edge cases are written into.
const MINIMAL: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

/// MINIMAL with its title written as `title` (a JSON string's source).
fn titled(title: &str) -> String {
    MINIMAL.replace("\"title\":\"t\"", &format!("\"title\":{title}"))
}

/// MINIMAL with its hold written as `ms` (a JSON number's source).
fn held(ms: &str) -> String {
    MINIMAL.replace("\"ms\":1000", &format!("\"ms\":{ms}"))
}

/// A rule whose condition is `not` within `not`, `depth` times, around a
/// predicate: nesting the JSON three containers a level.
fn nested_rule(depth: usize) -> String {
    let mut when = r#"{"segment_elapsed":{"ms":{"op":"GE","value":1}}}"#.to_owned();
    for _ in 0..depth {
        when = format!(r#"{{"not":{{"item":{when}}}}}"#);
    }
    MINIMAL.replace(
        "\"declarative\":{",
        &format!(
            r#""declarative":{{"handlers":[{{"id":"h","when":{when},"body":[{{"label":"b","hold":{{"ms":1}}}}],"resume":"CONTINUE"}}],"#
        ),
    )
}

/// An array nested `depth` deep in the title's place, to the readers' limits.
fn nested_array(depth: usize) -> String {
    let mut value = "1".to_owned();
    for _ in 0..depth {
        value = format!("[{value}]");
    }
    titled(&value)
}

/// The edge cases where the JSONC reader and the codec's reader could part.
fn probes() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut add = |what: &str, text: String| out.push((what.to_owned(), text));
    add("the plain playbook", MINIMAL.to_owned());
    for (what, title) in [
        ("a raw control character", "\"a\u{1}b\""),
        ("a raw tab", "\"a\tb\""),
        ("a raw newline", "\"a\nb\""),
        ("DEL", "\"a\u{7f}b\""),
        ("an escaped NUL", "\"a\\u0000b\""),
        ("a paired surrogate", "\"\\ud83d\\udca1\""),
        ("a lone high surrogate", "\"\\ud83d\""),
        ("a lone low surrogate", "\"\\udca1\""),
        ("a high surrogate and a plain escape", "\"\\ud83d\\u0041\""),
        ("an upper-case escape", "\"\\u00E9\""),
        ("a short escape", "\"\\u00e\""),
        ("a signed escape", "\"\\u+041\""),
        ("an unknown escape", "\"\\x41\""),
        ("every short escape", "\"\\\"\\\\\\/\\b\\f\\n\\r\\t\""),
        ("an escape at the end", "\"abc\\"),
        ("a string never closed", "\"abc"),
        ("a comment inside a string", "\"/* not */ // not\""),
        ("U+2028 in a string", "\"a\u{2028}b\""),
        ("a four-byte character", "\"\u{1f4a1}\""),
        ("a number", "5"),
        ("null", "null"),
    ] {
        add(what, titled(title));
    }
    for (what, ms) in [
        ("a leading zero", "01000"),
        ("two zeros", "00"),
        ("minus zero", "-0"),
        ("a negative", "-1000"),
        ("an exponent", "1E3"),
        ("a lower-case exponent", "1e3"),
        ("a huge exponent", "1e999999999999"),
        ("a fraction", "1000.0"),
        ("a bare point", "1."),
        ("a bare exponent", "1e"),
        ("a plus", "+1000"),
        ("a quoted number", "\"1000\""),
        ("a quoted leading zero", "\"01000\""),
        ("past i32", "2147483648"),
        ("a huge number", "123456789012345678901234567890"),
        ("a comment inside a number", "10/**/00"),
        ("a minus and a comment", "-/**/1000"),
        ("a literal", "true"),
        ("a broken literal", "tr/**/ue"),
        ("null", "null"),
    ] {
        add(what, held(ms));
    }
    add("an empty file", String::new());
    add("only whitespace", " \n\t\r ".to_owned());
    add("only comments", "// one\n/* two */\n".to_owned());
    add("a BOM", format!("\u{feff}{MINIMAL}"));
    add("a form feed", format!("\u{c}{MINIMAL}"));
    add("a vertical tab", format!("{MINIMAL}\u{b}"));
    add("a no-break space", format!("\u{a0}{MINIMAL}"));
    add(
        "U+2028 between tokens",
        MINIMAL.replace(",\"meta\"", ",\u{2028}\"meta\""),
    );
    add("content after the value", format!("{MINIMAL} {{}}"));
    add("a comment after the value", format!("{MINIMAL} // the end"));
    add("a lone slash after the value", format!("{MINIMAL} /"));
    add(
        "a lone slash between tokens",
        MINIMAL.replace(",\"meta\"", ",/\"meta\""),
    );
    add("a trailing comma", MINIMAL.replace("}}}}}},", "}}}}},"));
    add(
        "a trailing comma in an object",
        MINIMAL.replace("\"major\":1}", "\"major\":1,}"),
    );
    add(
        "a trailing comma in an array",
        MINIMAL.replace("1000}}]", "1000}},]"),
    );
    add(
        "a key written twice",
        MINIMAL.replace("\"major\":1", "\"major\":1,\"major\":1"),
    );
    add(
        "a key written twice, one escaped",
        MINIMAL.replace("\"major\":1", "\"major\":1,\"m\\u0061jor\":1"),
    );
    add(
        "an unclosed block comment",
        format!("{MINIMAL} /* never closed"),
    );
    add(
        "an unclosed block comment at the start",
        format!("/* {MINIMAL}"),
    );
    add(
        "an unclosed block comment inside",
        MINIMAL.replace(",\"meta\"", ",/* \"meta\""),
    );
    add(
        "a block comment closed at the end",
        format!("{MINIMAL} /* closed */"),
    );
    add("a block comment of stars", format!("/***/{MINIMAL}/**/"));
    add(
        "a block comment opening on a slash",
        format!("/*/ x */{MINIMAL}"),
    );
    add("a line comment ended by CR", format!("// note\r{MINIMAL}"));
    add(
        "a line comment ended by CRLF",
        format!("// note\r\n{MINIMAL}"),
    );
    add(
        "a line comment at the end with no newline",
        format!("{MINIMAL}// end"),
    );
    add(
        "a comment between every token",
        every_boundary(MINIMAL, "/**/"),
    );
    add(
        "a line comment between every token",
        every_boundary(MINIMAL, "//\n"),
    );
    add(
        "a comment inside a key",
        MINIMAL.replace("\"major\"", "\"ma/**/jor\""),
    );
    for depth in 26..=33 {
        add(
            &format!("conditions nested {depth} deep"),
            nested_rule(depth),
        );
    }
    for depth in [62, 63, 64, 65, 66] {
        add(
            &format!("an array nested {depth} deep"),
            nested_array(depth),
        );
    }
    let deep = |depth: usize| {
        let mut value = String::new();
        for _ in 0..depth {
            value.push('[');
        }
        for _ in 0..depth {
            value.push(']');
        }
        value
    };
    for depth in [63, 64, 65] {
        add(&format!("empty arrays nested {depth} deep"), deep(depth));
    }
    add("an unbalanced close", format!("{MINIMAL}]"));
    add("an object", "{}".to_owned());
    add("an array", "[]".to_owned());
    add("a string", "\"PLAYBOOK\"".to_owned());
    add("an unknown field", MINIMAL.replace("\"kind\"", "\"kinds\""));
    add(
        "two oneof arms",
        MINIMAL.replace(
            "\"hold\":{\"ms\":1000}",
            "\"hold\":{\"ms\":1000},\"wait\":{\"ms\":1}",
        ),
    );
    add("an enum as a number", MINIMAL.replace("\"HUMAN\"", "1"));
    add(
        "an enum as a leading-zero number",
        MINIMAL.replace("\"HUMAN\"", "01"),
    );
    add(
        "an unknown enum name",
        MINIMAL.replace("\"HUMAN\"", "\"ROBOT\""),
    );
    out
}

/// `text` with `comment` at every boundary between two tokens.
fn every_boundary(text: &str, comment: &str) -> String {
    let spots = boundaries(text);
    let mut out = String::new();
    let mut from = 0;
    for at in spots {
        out.push_str(&text[from..at]);
        out.push_str(comment);
        from = at;
    }
    out.push_str(&text[from..]);
    out
}

#[test]
fn the_readers_agree_on_the_edge_cases() {
    let mut seen = Seen::default();
    for (what, text) in probes() {
        seen.check(&what, &text);
        let mut rng = Rng(0x7f4a_7c15);
        for (how, mutated) in mutations(&text, &mut rng, usize::MAX) {
            seen.check(&format!("{what}, {how}"), &mutated);
        }
    }
    assert!(seen.canonical > 0 && seen.blanked > 0, "{seen:?}");
}

#[test]
fn the_readers_agree_on_every_committed_file() {
    let mut seen = Seen::default();
    for (index, (name, text)) in corpus().into_iter().enumerate() {
        seen.check(&name, &text);
        let start = 0x5eed_0000_u64 + u64::try_from(index).expect("an index");
        let mut rng = Rng(start);
        for (how, mutated) in mutations(&text, &mut rng, 12) {
            seen.check(&format!("{name} (seed {start:#x}), {how}"), &mutated);
        }
    }
    assert!(seen.canonical > 100 && seen.blanked > 100, "{seen:?}");
}

#[test]
fn the_readers_agree_on_the_shapes_at_the_size_budget() {
    let mut seen = Seen::default();
    for (index, (name, text)) in shapes().into_iter().enumerate() {
        seen.check(&name, &text);
        let start = 0x5a9e_0000_u64 + u64::try_from(index).expect("an index");
        let mut rng = Rng(start);
        for (how, mutated) in mutations(&text, &mut rng, 3) {
            seen.check(&format!("{name} (seed {start:#x}), {how}"), &mutated);
        }
    }
    assert!(seen.canonical >= 2, "{seen:?}");
}

/// Pieces of text arbitrary text is drawn from: JSON's and a comment's own
/// characters, with some that are neither and some multi-byte characters.
const PIECES: &[&str] = &[
    "{",
    "}",
    "[",
    "]",
    ":",
    ",",
    "\"",
    "\\",
    "/",
    "*",
    "//",
    "/*",
    "*/",
    "0",
    "1",
    "-",
    ".",
    "e",
    "true",
    "false",
    "null",
    " ",
    "\n",
    "\r",
    "\t",
    "\u{c}",
    "\u{feff}",
    "\u{e9}",
    "\u{1f4a1}",
    "\\u",
    "d83d",
    "dca1",
    "\"kind\"",
    "\"PLAYBOOK\"",
    "\"schema_version\"",
    "\"major\"",
    "x",
    "\u{0}",
];

fn arbitrary(rng: &mut Rng) -> String {
    let length = rng.below(48);
    let mut out = String::new();
    for _ in 0..length {
        out.push_str(PIECES[rng.below(PIECES.len())]);
    }
    out
}

/// Arbitrary text, and arbitrary splices of committed files: nothing but the
/// typed refusal ever leaves either reader (a panic fails the test), and the
/// two agree.
#[test]
fn the_readers_agree_on_arbitrary_text() {
    let mut seen = Seen::default();
    let mut rng = Rng(0xa4b1_7a47);
    for index in 0..20_000 {
        let text = arbitrary(&mut rng);
        seen.check(&format!("arbitrary text {index} (seed 0xa4b17a47)"), &text);
    }
    let files: Vec<String> = corpus()
        .into_iter()
        .map(|(_, text)| text)
        .take(60)
        .collect();
    for index in 0..2_000 {
        let one = &files[rng.below(files.len())];
        let two = &files[rng.below(files.len())];
        let mut cut = rng.below(one.len() + 1);
        while !one.is_char_boundary(cut) {
            cut -= 1;
        }
        let mut from = rng.below(two.len() + 1);
        while !two.is_char_boundary(from) {
            from -= 1;
        }
        let text = format!("{}{}", &one[..cut], &two[from..]);
        seen.check(&format!("splice {index} (seed 0xa4b17a47)"), &text);
    }
    assert_eq!(seen.texts, 22_000, "{seen:?}");
}

/// A rule whose condition is one `all` group around `nots` nested `not`s
/// around a predicate: with 27, the predicate's numbers stand inside exactly
/// [`pharmakos_proto::json::grammar::MAX_DEPTH`] containers.
fn nested_to_the_limit(nots: usize) -> String {
    let mut when = r#"{"segment_elapsed":{"ms":{"op":"GE","value":1}}}"#.to_owned();
    when = format!(r#"{{"all":{{"items":[{when}]}}}}"#);
    for _ in 0..nots {
        when = format!(r#"{{"not":{{"item":{when}}}}}"#);
    }
    MINIMAL.replace(
        "\"declarative\":{",
        &format!(
            r#""declarative":{{"handlers":[{{"id":"h","when":{when},"body":[{{"label":"b","hold":{{"ms":1}}}}],"resume":"CONTINUE"}}],"#
        ),
    )
}

/// What `verify_jsonc` reports for `text` at FULL: whether it qualifies, and
/// each diagnostic's code and path.
fn verified(text: &str) -> (bool, Vec<(String, String)>) {
    use pharmakos_proto::gp::api::v1::verify_plan::Depth;
    let rules =
        pharmakos_sim::rules::RulesTable::load(&workspace_root().join("rules/rules.v1.json"))
            .expect("the committed rules table");
    let scope = pharmakos_verifier::Scope::new(
        pharmakos_sim::tables::SeatId::new(0),
        pharmakos_sim::knowledge::SeatEconomy::default(),
    );
    let report = super::verify_jsonc(text, &[], &scope, &rules, Depth::Full).expect("a report");
    let found = report
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.clone(), diagnostic.path.clone()))
        .collect();
    (report.qualifies, found)
}

/// Before `vfyq2`, the blanker blanked a block comment that never closed to the
/// end of the file, so the codec read what came before it as a whole playbook:
/// `verify_plan` called it qualifying, and `submit_plan` then failed to compile
/// it with an internal error, because the JSONC reader refused it. Every
/// reader now refuses it, as `E0001` at the byte where it opens.
#[test]
fn an_unclosed_block_comment_is_refused_where_it_opens() {
    let closed = format!("/* closed */ {MINIMAL} // a note\n/* also closed */ ");
    for (what, text, open, message) in [
        (
            "after the value",
            format!("{MINIMAL} /* never closed"),
            MINIMAL.len() + 1,
            "trailing content after the JSON value",
        ),
        (
            "before a key",
            MINIMAL.replace(",\"kind\"", ",/* \"kind\""),
            MINIMAL.find(",\"kind\"").expect("a kind") + 1,
            "expected `\"`",
        ),
        (
            "after closed ones",
            format!("{closed}/* never closed"),
            closed.len(),
            "trailing content after the JSON value",
        ),
    ] {
        let pointer = format!("/byte/{open}");
        let refused = Document::parse(&text).expect_err(what);
        assert_eq!(
            (refused.pointer(), refused.message()),
            (pointer.as_str(), message),
            "{what}"
        );
        readers_agree(what, &text);
        assert_eq!(lean_hand_over(&text), reference_hand_over(&text), "{what}");
        let (qualifies, found) = verified(&text);
        assert!(!qualifies, "{what}");
        assert_eq!(found, [("E0001".to_owned(), pointer.clone())], "{what}");
    }
}

/// Before `vfyq2`, the JSONC reader refused a value inside exactly
/// `MAX_DEPTH` containers, which the codec took: the verifier read the file,
/// and the editor could not apply its Fix to it. Both take it now, and both
/// refuse one level more, at the same byte.
#[test]
fn a_value_inside_max_depth_containers_is_read_by_both() {
    let deepest = nested_to_the_limit(27);
    assert!(canonicalise_text(&deepest).is_ok());
    assert!(matches!(lean_hand_over(&deepest), Handed::Canonical(_)));
    readers_agree("inside 64 containers", &deepest);
    let (qualifies, found) = verified(&deepest);
    assert!(!qualifies);
    assert!(
        found
            .iter()
            .all(|(code, path)| code != "E0001" && !path.starts_with("/byte/")),
        "{found:?}"
    );
    let past = format!("[{deepest}]");
    let too_deep = nested_to_the_limit(28);
    for text in [past.as_str(), too_deep.as_str()] {
        let refused = Document::parse(text).expect_err("past the limit");
        assert!(
            refused.message().contains("nested deeper than 64"),
            "{refused:?}"
        );
        readers_agree("past the limit", text);
        assert_eq!(lean_hand_over(text), reference_hand_over(text));
    }
}

/// Before `vfyq2`, the JSONC reader took a number with a leading zero, which
/// the codec refused: a file could be patched and rendered that never
/// verified. Both refuse it now, at the second digit.
#[test]
fn a_leading_zero_is_refused_by_both() {
    let text = held("01000");
    let at = MINIMAL.find("\"ms\":1000").expect("a hold") + "\"ms\":0".len();
    let refused = Document::parse(&text).expect_err("a leading zero");
    assert_eq!(
        (refused.pointer().to_owned(), refused.message().to_owned()),
        (
            format!("/byte/{at}"),
            "a number with a leading zero".to_owned()
        )
    );
    readers_agree("a leading zero", &text);
    assert_eq!(
        verified(&text).1,
        [("E0001".to_owned(), format!("/byte/{at}"))]
    );
}

/// Every comment a document holds, with the anchor it was written against,
/// in the order it stands in.
fn spotted(document: &Document) -> Vec<Spotted> {
    document
        .comments()
        .into_iter()
        .map(|comment| Spotted {
            anchor: format!("{:?}", comment.anchor),
            pointer: comment.anchor.pointer().map(str::to_owned),
            text: comment.text,
        })
        .collect()
}

/// One comment as a text holds it: its anchor, the pointer the anchor names
/// (none for the prologue and the epilogue) and its text.
struct Spotted {
    anchor: String,
    pointer: Option<String>,
    text: String,
}

/// One relocation, by identity (fix pass 3, item L9): the comment's text and
/// the pointer it was written against and moved to, so two comments with one
/// text are never mistaken for each other.
struct Moved {
    text: String,
    from: String,
    to: String,
}

/// Whether `out` holds `inn`'s comments (fix pass 2, item D4: the comparison
/// sorted before, and so could not see a reordering). The canonical writer
/// puts members in field order, so a comment travels with the node it was
/// written against; what must hold is that every anchor's comments come out
/// in the order they went in, and that the only comments to change anchor
/// are the ones `relocated` names, each exactly once: in `inn` at the pointer
/// it was moved from, in `out` at the pointer it was moved to (the prologue
/// when that is the root).
fn same_comments(inn: &[Spotted], out: &[Spotted], relocated: &[Moved]) -> bool {
    use std::collections::BTreeMap;
    /// Each anchor's comments in order, and the relocated ones by identity.
    type Grouped<'c> = (BTreeMap<&'c str, Vec<&'c str>>, Vec<(&'c str, usize)>);
    fn by_anchor<'c>(comments: &'c [Spotted], relocated: &[Moved], before: bool) -> Grouped<'c> {
        let mut taken = vec![false; relocated.len()];
        let mut kept: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut loose: Vec<(&str, usize)> = Vec::new();
        for seen in comments {
            let here = seen.pointer.as_deref().unwrap_or_default();
            let found = relocated.iter().enumerate().position(|(index, moved)| {
                let place = if before { &moved.from } else { &moved.to };
                !taken.get(index).copied().unwrap_or(true)
                    && moved.text == seen.text
                    && here == place.as_str()
            });
            if let Some(index) = found {
                if let Some(slot) = taken.get_mut(index) {
                    *slot = true;
                }
                loose.push((seen.text.as_str(), index));
            } else {
                kept.entry(seen.anchor.as_str())
                    .or_default()
                    .push(seen.text.as_str());
            }
        }
        loose.sort_unstable();
        (kept, loose)
    }
    by_anchor(inn, relocated, true) == by_anchor(out, relocated, false)
}

/// The comment-keeping canonicaliser [`Kept`] holds to its two promises:
/// [`canonicalise`] itself, or, in the checker's own test, one broken on
/// purpose.
type Canonicaliser = fn(&Document) -> Result<Canonical, crate::error::Error>;

/// The two promises the canonical writer makes of every text it writes
/// (fix pass 1, item 8): every comment comes out, text for text (a comment
/// on a written-out default is moved, never dropped), and canonicalising the
/// canonical text gives it back unchanged.
///
/// Each text is read once and its document shared by the canonicaliser and
/// the comment listing, and so is the canonical text's (`pcsplit`): reading a
/// text twice gave the same document twice, since `canonicalise_text` is
/// `canonicalise` over `Document::parse`.
#[derive(Debug)]
struct Kept {
    canonicalise: Canonicaliser,
    cases: u64,
    lost: Vec<String>,
    unstable: Vec<String>,
}

impl Kept {
    fn new(canonicalise: Canonicaliser) -> Self {
        Self {
            canonicalise,
            cases: 0,
            lost: Vec::new(),
            unstable: Vec::new(),
        }
    }

    fn check(&mut self, what: &str, text: &str) {
        let Ok(document) = Document::parse(text) else {
            return;
        };
        let Ok(once) = (self.canonicalise)(&document) else {
            return;
        };
        self.cases += 1;
        let relocated: Vec<Moved> = once
            .relocated
            .iter()
            .map(|relocation| Moved {
                text: relocation.comment.clone(),
                from: relocation.from.clone(),
                to: relocation.to.clone(),
            })
            .collect();
        let again = Document::parse(&once.text).expect("the canonical text parses");
        if !same_comments(&spotted(&document), &spotted(&again), &relocated) {
            self.lost.push(format!(
                "{what}: {:?}",
                text.chars().take(300).collect::<String>()
            ));
        }
        match (self.canonicalise)(&again) {
            Ok(twice) if twice.text == once.text => {}
            _ => self.unstable.push(format!(
                "{what}: {:?}",
                text.chars().take(300).collect::<String>()
            )),
        }
    }
}

/// The largest file spliced at every boundary: every committed playbook is
/// smaller.
const EVERY_BOUNDARY_UP_TO: usize = 16 * 1024;

/// How many boundaries of every file take every one of [`COMMENTS`].
const EVERY_COMMENT_AT: usize = 32;

/// One boundary a comment is spliced at: its place in the file's
/// [`boundaries`], its byte offset, and the comments spliced there.
struct Splice {
    at_index: usize,
    at: usize,
    comments: &'static [&'static str],
}

/// Where `text` is spliced: every comment at the first
/// [`EVERY_COMMENT_AT`] boundaries, and, in a file of a playbook's size, a
/// comment at every boundary after them, the comments taken in turn (fix pass
/// 2, item D4: only the first 32 were spliced at before). A larger file (a
/// report or a response golden) keeps the first 32.
fn splices(text: &str) -> Vec<Splice> {
    let every = text.len() <= EVERY_BOUNDARY_UP_TO;
    boundaries(text)
        .into_iter()
        .enumerate()
        .take_while(|(at_index, _)| *at_index < EVERY_COMMENT_AT || every)
        .map(|(at_index, at)| Splice {
            at_index,
            at,
            comments: if at_index < EVERY_COMMENT_AT {
                &COMMENTS
            } else {
                let turn = at_index % COMMENTS.len();
                &COMMENTS[turn..=turn]
            },
        })
        .collect()
}

/// How many tests the comment differential is split into (`pcsplit`): one
/// serial test took 456 s on windows-latest, and libtest runs tests on the
/// runner's threads.
const GROUPS: usize = 16;

/// One piece of a group's work: a run of one text's [`splices`], and, for
/// the text's first run, the text itself and its seeded mutations.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Unit {
    /// The text's place in [`differential_texts`].
    text: usize,
    /// Which of the text's [`splices`] this unit makes.
    splices: std::ops::Range<usize>,
    /// Whether this unit also checks the text and its mutations.
    first: bool,
    /// Its modelled cost: bytes canonicalised.
    cost: u64,
}

/// The texts the differential reads: every committed file, then the edge
/// cases, in that order (each text's place seeds its mutations).
fn differential_texts() -> Vec<(String, String)> {
    let mut texts = corpus();
    texts.extend(probes());
    texts
}

/// The work split into [`GROUPS`] groups, a pure function of the texts: each
/// text's splices are cut into runs of at most an eighth of a group's fair
/// share of the cost, and the runs dealt, dearest first, to the group with
/// the least so far (ties to the lowest group, then the earliest text and
/// run), so that every group costs about the same and every splice falls in
/// exactly one group.
fn plan(texts: &[(String, String)]) -> Vec<Vec<Unit>> {
    let cost_of = |text: &str, splice: &Splice| -> u64 {
        let one: u64 = splice
            .comments
            .iter()
            .map(|comment| u64::try_from(text.len() + comment.len()).expect("a length"))
            .sum();
        one
    };
    let per_text: Vec<Vec<u64>> = texts
        .iter()
        .map(|(_, text)| {
            splices(text)
                .iter()
                .map(|splice| cost_of(text, splice))
                .collect()
        })
        .collect();
    let total: u64 = per_text.iter().flatten().sum();
    let runs = u64::try_from(GROUPS * 8).expect("a count");
    let run_at_most = total.checked_div(runs).expect("GROUPS is not zero").max(1);
    let mut units: Vec<Unit> = Vec::new();
    for (text, costs) in per_text.iter().enumerate() {
        let mut start = 0;
        let mut cost = 0;
        for (index, splice_cost) in costs.iter().enumerate() {
            if index > start && cost + splice_cost > run_at_most {
                units.push(Unit {
                    text,
                    splices: start..index,
                    first: start == 0,
                    cost,
                });
                start = index;
                cost = 0;
            }
            cost += splice_cost;
        }
        units.push(Unit {
            text,
            splices: start..costs.len(),
            first: start == 0,
            cost,
        });
    }
    units.sort_by(|one, two| {
        two.cost
            .cmp(&one.cost)
            .then(one.text.cmp(&two.text))
            .then(one.splices.start.cmp(&two.splices.start))
    });
    let mut planned: Vec<Vec<Unit>> = vec![Vec::new(); GROUPS];
    let mut load = [0_u64; GROUPS];
    for unit in units {
        let lightest = (0..GROUPS)
            .min_by_key(|group| (load[*group], *group))
            .expect("a group");
        load[lightest] += unit.cost;
        planned[lightest].push(unit);
    }
    for group in &mut planned {
        group.sort_by_key(|unit| (unit.text, unit.splices.start));
    }
    planned
}

/// The texts, by name, and the work [`plan`] splits them into.
type Planned = (Vec<(String, String)>, Vec<Vec<Unit>>);

/// The texts and their plan. Each test makes its own, in well under a second:
/// a static shared by the tests would be one more static behind the type
/// door (`tests/door.rs`) for no gain worth a review.
fn planned_texts() -> Planned {
    let texts = differential_texts();
    let planned = plan(&texts);
    (texts, planned)
}

/// One group of the differential: every unit's text and mutations, and every
/// comment at every boundary it plans, each held to [`Kept`]'s promises.
/// What it splices is checked against the plan boundary for boundary, and
/// each group holds more than its share of the 10 000 canonical cases the one
/// serial test asked for, so the groups together hold more than that.
fn kept_in_group(group: usize) {
    let (texts, planned) = planned_texts();
    let units = &planned[group];
    assert!(!units.is_empty(), "group {group} has no work");
    let mut kept = Kept::new(canonicalise);
    // Every splice checked, and every splice the plan names: (text, boundary,
    // comment).
    let mut done: Vec<(usize, usize, &str)> = Vec::new();
    let mut wanted: Vec<(usize, usize, &str)> = Vec::new();
    for unit in units {
        let (name, text) = &texts[unit.text];
        if unit.first {
            kept.check(name, text);
            let start = 0xc0de_0000_u64 + u64::try_from(unit.text).expect("an index");
            let mut rng = Rng(start);
            for (how, mutated) in mutations(text, &mut rng, 24) {
                kept.check(&format!("{name} (seed {start:#x}), {how}"), &mutated);
            }
        }
        let list = splices(text);
        for splice in &list[unit.splices.clone()] {
            for comment in splice.comments {
                kept.check(
                    &format!(
                        "{name}, {comment:?} at byte {} ({})",
                        splice.at, splice.at_index
                    ),
                    &with_comment(text, splice.at, comment),
                );
                done.push((unit.text, splice.at_index, comment));
            }
        }
        for splice in &list[unit.splices.clone()] {
            wanted.extend(
                splice
                    .comments
                    .iter()
                    .map(|comment| (unit.text, splice.at_index, *comment)),
            );
        }
    }
    println!(
        "group {group}: {} units, {} splices, comments kept over {} canonical cases",
        units.len(),
        done.len(),
        kept.cases
    );
    assert_eq!(
        done, wanted,
        "group {group} splices every boundary it plans"
    );
    assert!(
        kept.cases * u64::try_from(GROUPS).expect("a count") > 10_000,
        "group {group}: {} canonical cases",
        kept.cases
    );
    assert!(
        kept.lost.is_empty() && kept.unstable.is_empty(),
        "{} of {} cases lost a comment, {} were not stable:\nlost: {:#?}\nunstable: {:#?}",
        kept.lost.len(),
        kept.cases,
        kept.unstable.len(),
        kept.lost.iter().take(5).collect::<Vec<_>>(),
        kept.unstable.iter().take(5).collect::<Vec<_>>()
    );
}

/// One test per group, so that a group cannot exist without its test: the
/// list is [`GROUP_TESTS`], and `every_boundary_of_every_text_has_a_test`
/// holds it to `0..GROUPS`.
macro_rules! comment_groups {
    ($($test:ident = $group:literal),* $(,)?) => {
        $(
            #[test]
            fn $test() {
                kept_in_group($group);
            }
        )*
        const GROUP_TESTS: &[usize] = &[$($group),*];
    };
}

comment_groups!(
    every_comment_comes_out_and_canonicalising_twice_is_once_00 = 0,
    every_comment_comes_out_and_canonicalising_twice_is_once_01 = 1,
    every_comment_comes_out_and_canonicalising_twice_is_once_02 = 2,
    every_comment_comes_out_and_canonicalising_twice_is_once_03 = 3,
    every_comment_comes_out_and_canonicalising_twice_is_once_04 = 4,
    every_comment_comes_out_and_canonicalising_twice_is_once_05 = 5,
    every_comment_comes_out_and_canonicalising_twice_is_once_06 = 6,
    every_comment_comes_out_and_canonicalising_twice_is_once_07 = 7,
    every_comment_comes_out_and_canonicalising_twice_is_once_08 = 8,
    every_comment_comes_out_and_canonicalising_twice_is_once_09 = 9,
    every_comment_comes_out_and_canonicalising_twice_is_once_10 = 10,
    every_comment_comes_out_and_canonicalising_twice_is_once_11 = 11,
    every_comment_comes_out_and_canonicalising_twice_is_once_12 = 12,
    every_comment_comes_out_and_canonicalising_twice_is_once_13 = 13,
    every_comment_comes_out_and_canonicalising_twice_is_once_14 = 14,
    every_comment_comes_out_and_canonicalising_twice_is_once_15 = 15,
);

/// The plan covers the corpus as the directories list it, and the edge
/// cases: every text has exactly one first unit, every boundary the rule
/// names (every one in a file of a playbook's size, the first 32 in a larger
/// one; read here from [`boundaries`] directly, not through [`splices`]) is
/// spliced in exactly one group with the comments the rule gives it, every
/// group has work and a test of its own.
#[test]
fn every_boundary_of_every_text_has_a_test() {
    let texts = differential_texts();
    let (shared_texts, planned) = planned_texts();
    let names: Vec<&String> = texts.iter().map(|(name, _)| name).collect();
    let shared: Vec<&String> = shared_texts.iter().map(|(name, _)| name).collect();
    assert_eq!(names, shared, "the plan reads the corpus as listed");
    assert_eq!(
        GROUP_TESTS,
        (0..GROUPS).collect::<Vec<_>>().as_slice(),
        "one test per group"
    );
    assert_eq!(planned.len(), GROUPS);
    let mut firsts = vec![0_usize; texts.len()];
    let mut spliced: Vec<Vec<usize>> = vec![Vec::new(); texts.len()];
    for (group, units) in planned.iter().enumerate() {
        assert!(!units.is_empty(), "group {group} has no work");
        for unit in units {
            assert!(unit.text < texts.len(), "{unit:?}");
            if unit.first {
                firsts[unit.text] += 1;
            }
            let list = splices(&texts[unit.text].1);
            spliced[unit.text].extend(
                list.get(unit.splices.clone())
                    .unwrap_or_else(|| panic!("{unit:?} past the text's splices"))
                    .iter()
                    .map(|splice| splice.at_index),
            );
        }
    }
    let (mut boundaries_spliced, mut cases) = (0_usize, 0_usize);
    for (index, (name, text)) in texts.iter().enumerate() {
        assert_eq!(firsts[index], 1, "{name}: one unit checks the text itself");
        let spots = boundaries(text);
        let ruled = if text.len() <= EVERY_BOUNDARY_UP_TO {
            spots.len()
        } else {
            spots.len().min(EVERY_COMMENT_AT)
        };
        let mut seen = spliced[index].clone();
        seen.sort_unstable();
        assert_eq!(
            seen,
            (0..ruled).collect::<Vec<_>>(),
            "{name}: every boundary spliced exactly once"
        );
        for splice in splices(text) {
            assert_eq!(spots.get(splice.at_index), Some(&splice.at), "{name}");
            let comments: Vec<&str> = if splice.at_index < EVERY_COMMENT_AT {
                COMMENTS.to_vec()
            } else {
                vec![COMMENTS[splice.at_index % COMMENTS.len()]]
            };
            assert_eq!(splice.comments, comments.as_slice(), "{name}");
            cases += comments.len();
        }
        boundaries_spliced += ruled;
    }
    println!(
        "{} texts, {boundaries_spliced} boundaries spliced, {cases} splice cases, in {GROUPS} \
         groups costing {:?}",
        texts.len(),
        planned
            .iter()
            .map(|units| units.iter().map(|unit| unit.cost).sum::<u64>())
            .collect::<Vec<_>>()
    );
}

/// A canonicaliser that drops every empty block comment, trailing or on a
/// line of its own, and so writes a text that is stable but has lost a
/// comment.
fn dropping(document: &Document) -> Result<Canonical, crate::error::Error> {
    let mut canonical = canonicalise(document)?;
    let mut kept = String::new();
    let text = canonical.text.replace(" /**/", "");
    for line in text.lines().filter(|line| line.trim() != "/**/") {
        kept.push_str(line);
        kept.push('\n');
    }
    canonical.text = kept;
    Ok(canonical)
}

/// A canonicaliser that writes a space before a text that has none, and so
/// writes the canonical text once differently.
fn wavering(document: &Document) -> Result<Canonical, crate::error::Error> {
    let mut canonical = canonicalise(document)?;
    if !document.to_text().starts_with(' ') {
        canonical.text.insert(0, ' ');
    }
    Ok(canonical)
}

/// [`Kept`] sees what it exists to see: a canonicaliser that loses a comment,
/// and one whose second pass differs from its first, and passes the real one.
#[test]
fn the_comment_checker_sees_a_lost_comment_and_an_unstable_text() {
    let text = MINIMAL.replace(",\"meta\"", ",/**/\"meta\"");
    let mut real = Kept::new(canonicalise);
    real.check("the real writer", &text);
    assert_eq!(
        (real.cases, real.lost.len(), real.unstable.len()),
        (1, 0, 0),
        "{real:?}"
    );
    let mut lossy = Kept::new(dropping);
    lossy.check("a dropped comment", &text);
    assert_eq!(
        (lossy.lost.len(), lossy.unstable.len()),
        (1, 0),
        "{lossy:?}"
    );
    let mut unsteady = Kept::new(wavering);
    unsteady.check("a wavering writer", &text);
    assert_eq!(
        (unsteady.lost.len(), unsteady.unstable.len()),
        (0, 1),
        "{unsteady:?}"
    );
}
