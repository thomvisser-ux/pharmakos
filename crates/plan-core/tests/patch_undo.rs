// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An undo restores exactly what was there (S3's `vfyq2`, fix pass 2, item A1).
//!
//! The editor holds the inverse patch `patch_plan` answers as text and sends it
//! back to undo. So the property is over that text: for every operation kind
//! (`add`, `replace`, `remove`, `move`, `copy`, `test`), at every sampled
//! pointer of every committed playbook, of documents that write a field under
//! both its names, and of generated commented documents, under each spelling
//! of a playbook field's name, the forward patch either is refused with a
//! typed error, or applies; and when it applies, its inverse, read back from
//! its own text, gives the original file back **byte for byte**: the value,
//! the member's position, its key's spelling and its comments. Never
//! different bytes.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pharmakos_plan_core::error::{push_pointer, split_pointer};
use pharmakos_plan_core::jsonc::Document;
use pharmakos_plan_core::patch::{Patch, apply};
use pharmakos_proto::json::{Json, write};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/plan-core sits two levels below the workspace root")
        .to_path_buf()
}

/// Every committed playbook the editor could hold, sorted by path.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected \
              and sorted before use, which is what happens below"
)]
fn corpus() -> Vec<(String, String)> {
    let root = workspace_root();
    let mut found = Vec::new();
    for dir in [
        "library",
        "examples/playbooks",
        "crates/plan-core/tests/cases",
    ] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .map(|entry| entry.expect("an entry").path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "jsonc")
            })
            .collect();
        paths.sort();
        for path in paths {
            let text = std::fs::read_to_string(&path).expect("a committed playbook");
            found.push((path.display().to_string(), text));
        }
    }
    found
}

/// Documents that write a playbook field under both its names, and under each
/// one alone, with comments around each member.
fn spellings() -> Vec<(String, String)> {
    vec![
        (
            "both, camelCase last".to_owned(),
            "{\n  \"meta\": {\n    // the author\n    \"author_kind\": \"HUMAN\",\n    \
             \"title\": \"t\", /* why */ \"authorKind\": \"AI\"\n  },\n  \"kind\": \"PLAYBOOK\"\n}\n"
                .to_owned(),
        ),
        (
            "both, camelCase first".to_owned(),
            "{\"meta\":{\"authorKind\":\"AI\",\n// note\n\"author_kind\":\"HUMAN\"}}".to_owned(),
        ),
        (
            "camelCase alone".to_owned(),
            "{\"declarative\":{\"route\":[{\"label\":\"a\", // first\n\"skipIf\":{\"cmdrHpPct\":\
             {\"cmp\":\"LE\",\"pct\":10}},\"timeoutMs\":3000}]}}"
                .to_owned(),
        ),
        (
            "CR line ends".to_owned(),
            "{\r  // one\r  \"a\": 1,\r  \"b\": [1, // two\r 2]\r}\r".to_owned(),
        ),
    ]
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
        let n = u64::try_from(n.max(1)).expect("a count");
        usize::try_from(self.next().checked_rem(n).expect("non-zero")).expect("below a usize")
    }
}

/// Keys the generator writes: playbook fields under each of their names, and
/// keys of no field.
const KEYS: [&str; 8] = [
    "author_kind",
    "authorKind",
    "timeout_ms",
    "timeoutMs",
    "label",
    "x",
    "y~z",
    "a/b",
];

const TRIVIA: [&str; 11] = [
    "",
    " ",
    "\n  ",
    " // c\n",
    "/* b */",
    "\r\n",
    "\n/* x\n y */ ",
    // CR-only line ends, a line comment ended by a lone CR, and one by a
    // CRLF (fix pass 3, item M1: the empty containers' insides hold these).
    "\r",
    " // cr\r",
    " // crlf\r\n",
    " /* in */ ",
];

fn generated_value(rng: &mut Rng, depth: usize, out: &mut String) {
    match if depth > 3 { 2 } else { rng.below(4) } {
        0 => {
            out.push('{');
            let count = rng.below(4);
            if count == 0 {
                // An empty object's inside, which a put into it must keep.
                out.push_str(TRIVIA.get(rng.below(TRIVIA.len())).expect("trivia"));
            }
            let mut used: Vec<&str> = Vec::new();
            for index in 0..count {
                let key = KEYS.get(rng.below(KEYS.len())).expect("a key");
                if used.contains(key) {
                    continue;
                }
                used.push(key);
                if index > 0 && !out.ends_with('{') {
                    out.push(',');
                }
                out.push_str(TRIVIA.get(rng.below(TRIVIA.len())).expect("trivia"));
                out.push_str(&write(&Json::String((*key).to_owned())));
                out.push(':');
                out.push_str(TRIVIA.get(rng.below(TRIVIA.len())).expect("trivia"));
                generated_value(rng, depth.saturating_add(1), out);
                out.push_str(TRIVIA.get(rng.below(TRIVIA.len())).expect("trivia"));
            }
            out.push('}');
        }
        1 => {
            out.push('[');
            let count = rng.below(4);
            if count == 0 {
                // An empty array's inside, which a put into it must keep.
                out.push_str(TRIVIA.get(rng.below(TRIVIA.len())).expect("trivia"));
            }
            for index in 0..count {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(TRIVIA.get(rng.below(TRIVIA.len())).expect("trivia"));
                generated_value(rng, depth.saturating_add(1), out);
            }
            out.push(']');
        }
        2 => out.push_str(&rng.below(100).to_string()),
        _ => out.push_str("\"s\""),
    }
}

fn generated(count: usize) -> Vec<(String, String)> {
    let mut rng = Rng(0x0a1d_0e0d);
    let mut found = Vec::new();
    while found.len() < count {
        let mut text = String::new();
        text.push_str(TRIVIA.get(rng.below(TRIVIA.len())).expect("trivia"));
        text.push('{');
        let mut inner = String::new();
        generated_value(&mut rng, 1, &mut inner);
        text.push_str("\"root\":");
        text.push_str(&inner);
        text.push_str(",\"meta\":{\"author_kind\":\"HUMAN\"}");
        text.push('}');
        if Document::parse(&text).is_ok() {
            found.push((format!("generated {}", found.len()), text));
        }
    }
    found
}

/// Every pointer in a value, the root left out, in document order.
fn pointers(value: &Json, at: &str, found: &mut Vec<String>) {
    match value {
        Json::Object(entries) => {
            for (key, item) in entries {
                let here = push_pointer(at, key);
                found.push(here.clone());
                pointers(item, &here, found);
            }
        }
        Json::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let here = push_pointer(at, &index.to_string());
                found.push(here.clone());
                pointers(item, &here, found);
            }
        }
        _ => {}
    }
}

/// A playbook field's other name, for every field of every message.
fn other_names() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let schema = pharmakos_proto::descriptor::schema();
    for message in schema.messages() {
        for field in &message.fields {
            if field.name != field.json_name {
                map.insert(field.name.clone(), field.json_name.clone());
                map.insert(field.json_name.clone(), field.name.clone());
            }
        }
    }
    map
}

/// `pointer` with its last token in the field's other name, when it has one.
fn respelled(pointer: &str, names: &BTreeMap<String, String>) -> Option<String> {
    let mut tokens = split_pointer(pointer).ok()?;
    let last = tokens.pop()?;
    let other = names.get(&last)?;
    let parent = tokens
        .iter()
        .fold(String::new(), |acc, token| push_pointer(&acc, token));
    Some(push_pointer(&parent, other))
}

fn parent(pointer: &str) -> String {
    let mut tokens = split_pointer(pointer).expect("a pointer the walk wrote");
    tokens.pop();
    tokens
        .iter()
        .fold(String::new(), |acc, token| push_pointer(&acc, token))
}

/// The forward patches tried at one pointer.
fn patches_at(pointer: &str, value: &Json, siblings: &[String]) -> Vec<String> {
    let sentinel = r#"{"s":"☠"}"#;
    let up = parent(pointer);
    let new_member = push_pointer(&up, "zz_new");
    let quoted = write(&Json::String(pointer.to_owned()));
    let quoted_new = write(&Json::String(new_member.clone()));
    let mut found = vec![
        format!(r#"[{{"op":"remove","path":{quoted}}}]"#),
        format!(r#"[{{"op":"replace","path":{quoted},"value":{sentinel}}}]"#),
        format!(r#"[{{"op":"add","path":{quoted},"value":{sentinel}}}]"#),
        format!(r#"[{{"op":"add","path":{quoted_new},"value":{sentinel}}}]"#),
        format!(
            r#"[{{"op":"test","path":{quoted},"value":{}}}]"#,
            write(value)
        ),
        format!(r#"[{{"op":"move","from":{quoted},"path":{quoted_new}}}]"#),
        format!(r#"[{{"op":"move","from":{quoted},"path":{quoted}}}]"#),
        format!(r#"[{{"op":"copy","from":{quoted},"path":{quoted_new}}}]"#),
    ];
    // A put into the value itself, when it is a container: the first put
    // into an empty one is where its inside's trivia was lost (fix pass 3,
    // item M1).
    match value {
        Json::Object(_) => {
            let inside = write(&Json::String(push_pointer(pointer, "zz_in")));
            found.push(format!(
                r#"[{{"op":"add","path":{inside},"value":{sentinel}}}]"#
            ));
            found.push(format!(
                r#"[{{"op":"copy","from":{quoted},"path":{inside}}}]"#
            ));
        }
        Json::Array(_) => {
            for token in ["-", "0"] {
                let inside = write(&Json::String(push_pointer(pointer, token)));
                found.push(format!(
                    r#"[{{"op":"add","path":{inside},"value":{sentinel}}}]"#
                ));
            }
        }
        _ => {}
    }
    for sibling in siblings
        .iter()
        .filter(|sibling| sibling.as_str() != pointer)
    {
        let other = write(&Json::String(sibling.clone()));
        found.push(format!(
            r#"[{{"op":"move","from":{quoted},"path":{other}}}]"#
        ));
        found.push(format!(
            r#"[{{"op":"copy","from":{quoted},"path":{other}}}]"#
        ));
    }
    found
}

/// The JSON at a pointer of a value.
fn at<'a>(value: &'a Json, pointer: &str) -> Option<&'a Json> {
    let mut here = value;
    for token in split_pointer(pointer).ok()? {
        here = match here {
            Json::Object(entries) => entries
                .iter()
                .find(|(key, _)| *key == token)
                .map(|(_, inner)| inner)?,
            Json::Array(items) => items.get(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(here)
}

/// Every comment's text in a document, in order.
fn comment_texts(document: &Document) -> Vec<String> {
    document
        .comments()
        .into_iter()
        .map(|comment| comment.text)
        .collect()
}

#[derive(Default)]
struct Tally {
    applied: u64,
    refused: u64,
    wrong: Vec<String>,
}

impl Tally {
    fn check(&mut self, name: &str, text: &str, patch_text: &str) {
        let document = Document::parse(text).expect("a document");
        let forward = Patch::from_text(patch_text).expect("a forward patch");
        let Ok((patched, inverse)) = apply(&document, &forward) else {
            self.refused = self.refused.saturating_add(1);
            return;
        };
        self.applied = self.applied.saturating_add(1);
        // A put that removes nothing keeps every comment (fix pass 3, item
        // M1): an `add` of a new member or element, or a `copy` to one.
        let puts_only = (patch_text.contains(r#""op":"add""#)
            || patch_text.contains(r#""op":"copy""#))
            && (patch_text.contains("zz_")
                || patch_text.contains("/-\"")
                || patch_text.contains("/0\""));
        if puts_only {
            let mut kept = comment_texts(&document);
            for comment in comment_texts(&patched) {
                if let Some(found) = kept.iter().position(|one| *one == comment) {
                    kept.remove(found);
                }
            }
            if !kept.is_empty() {
                self.wrong.push(format!(
                    "{name}: {patch_text}: the put dropped {kept:?}\n  was: {text:?}\n  now: {:?}",
                    patched.to_text()
                ));
                return;
            }
        }
        // The undo is applied to the patched TEXT, read again, as the client
        // holds it and the gateway reads it (fix pass 3, item M1): never to
        // the document still in memory.
        let wire = inverse.to_text();
        let back = Document::parse(&patched.to_text())
            .map_err(|error| format!("the patched text does not read: {error:?}"))
            .and_then(|reread| {
                Patch::from_text(&wire)
                    .map_err(|error| format!("the inverse's text does not read: {error:?}"))
                    .map(|undo| (reread, undo))
            })
            .and_then(|(reread, undo)| {
                apply(&reread, &undo)
                    .map_err(|error| format!("the inverse does not apply: {error:?}"))
            })
            .map(|(back, _)| back.to_text());
        match back {
            Ok(back) if back == text => {}
            Ok(back) => self.wrong.push(format!(
                "{name}: {patch_text}: undo gave other bytes\n  inverse: {wire}\n  was: \
                 {text:?}\n  now: {back:?}"
            )),
            Err(why) => self
                .wrong
                .push(format!("{name}: {patch_text}: {why}\n  inverse: {wire}")),
        }
    }
}

#[test]
fn every_undo_through_its_text_gives_the_same_bytes() {
    let names = other_names();
    let mut documents = corpus();
    let committed = documents.len();
    documents.extend(spellings());
    documents.extend(generated(60));
    let mut tally = Tally::default();
    for (name, text) in &documents {
        let document = Document::parse(text).expect("a document");
        let json = document.to_json();
        let mut all = Vec::new();
        pointers(&json, "", &mut all);
        // Every pointer of a small document; a stride through a large one.
        let stride = all.len().div_ceil(120).max(1);
        for (index, pointer) in all.iter().enumerate() {
            if index.checked_rem(stride) != Some(0) {
                continue;
            }
            let value = at(&json, pointer).expect("a pointer the walk wrote");
            let siblings: Vec<String> = all
                .iter()
                .filter(|other| parent(other) == parent(pointer))
                .take(3)
                .cloned()
                .collect();
            let mut spelled = vec![pointer.clone()];
            spelled.extend(respelled(pointer, &names));
            for pointer in &spelled {
                for patch in patches_at(pointer, value, &siblings) {
                    tally.check(name, text, &patch);
                }
            }
        }
    }
    println!(
        "undo property: {} documents ({committed} committed): {} applied, {} refused, {} differ",
        documents.len(),
        tally.applied,
        tally.refused,
        tally.wrong.len()
    );
    assert!(tally.applied > 10_000, "only {} applied", tally.applied);
    assert!(
        tally.wrong.is_empty(),
        "{} of {} undos gave other bytes or did not apply; the first:\n{}",
        tally.wrong.len(),
        tally.applied,
        tally
            .wrong
            .iter()
            .take(6)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The review's case: in an object holding both spellings, removing
/// `authorKind` and undoing it must not write over `author_kind`.
#[test]
fn undoing_a_remove_beside_the_other_spelling_restores_it() {
    let text = "{\"meta\":{\"author_kind\":\"HUMAN\",\"authorKind\":\"AI\"}}";
    let document = Document::parse(text).expect("a document");
    let forward =
        Patch::from_text(r#"[{"op":"remove","path":"/meta/authorKind"}]"#).expect("a patch");
    let (patched, inverse) = apply(&document, &forward).expect("it applies");
    let undo = Patch::from_text(&inverse.to_text()).expect("the inverse reads");
    let (back, _) = apply(&patched, &undo).expect("the undo applies");
    assert_eq!(back.to_text(), text);
}

/// RFC 6901: `~` is followed by `0` or `1`, and nothing else.
#[test]
fn a_tilde_not_followed_by_zero_or_one_is_refused() {
    for pointer in ["/a~2", "/a~", "/~x/b"] {
        assert_eq!(
            split_pointer(pointer).map_err(|error| error.malformed),
            Err(Some(pharmakos_plan_core::error::Malformed::PointerEscape)),
            "{pointer}"
        );
        let text = "{\"a~2\":1,\"a\":2}";
        let document = Document::parse(text).expect("a document");
        let quoted = write(&Json::String(pointer.to_owned()));
        for patch in [
            format!(r#"[{{"op":"remove","path":{quoted}}}]"#),
            format!(r#"[{{"op":"copy","from":{quoted},"path":"/c"}}]"#),
        ] {
            let refused = Patch::from_text(&patch)
                .and_then(|patch| apply(&document, &patch).map(|_| ()))
                .expect_err("an escape RFC 6901 does not have");
            assert_eq!(
                refused.malformed,
                Some(pharmakos_plan_core::error::Malformed::PointerEscape),
                "{patch}: {refused:?}"
            );
        }
    }
}

/// The review's case (fix pass 3, item M1): the first put into an empty
/// container keeps the comment inside it, and the undo, applied to the patched
/// text read again, gives the file back byte for byte.
#[test]
fn a_put_into_an_empty_container_keeps_its_inside_and_undoes_exactly() {
    let text = "{\"a\": { /* keep */ }, \"b\": [ // keep too\n ]}";
    for patch in [
        r#"[{"op":"add","path":"/a/x","value":1}]"#,
        r#"[{"op":"add","path":"/b/-","value":1}]"#,
        r#"[{"op":"add","path":"/b/0","value":1}]"#,
        r#"[{"op":"copy","from":"/b","path":"/a/x"}]"#,
    ] {
        let (patched, inverse) =
            pharmakos_plan_core::patch::patch_text(text, patch).expect("it applies");
        assert!(patched.contains("/* keep */"), "{patch}: {patched:?}");
        assert!(patched.contains("// keep too"), "{patch}: {patched:?}");
        let (back, _) =
            pharmakos_plan_core::patch::patch_text(&patched, &inverse).expect("the undo applies");
        assert_eq!(back, text, "{patch}: inverse {inverse}");
    }
}
