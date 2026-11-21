// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The inverse of every accepted patch is accepted (S3's `vfyq2`, fix pass 5,
//! MEDIUM 1).
//!
//! `patch_plan` hands back an inverse the editor sends back to undo. The
//! door that issued it must take it, under every bound it meets: the count of
//! operations, the text's characters, the inverse's own size, depth and the
//! document's size. A forward patch whose undo could not be issued is refused
//! up front, never applied with an undo that does not work. The property is
//! over the wire: every accepted patch's inverse, read from its own text
//! through `patch_text`, applies and gives the file back byte for byte, and
//! the inverse of that (the redo) does too.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_plan_core::error::Bound;
use pharmakos_plan_core::patch::{
    MAX_PATCH_OPERATIONS, MAX_PATCH_TEXT_CHARS, MAX_UNDO_OPERATIONS, patch_text,
};
use pharmakos_proto::json::{Json, write};
use pharmakos_verifier::MAX_PLAYBOOK_CHARS;

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

/// A document with an array of `n` numbers, a commented member and an
/// object to put into.
fn document(n: usize) -> String {
    let items: Vec<String> = (0..n).map(|i| i.to_string()).collect();
    format!(
        "{{\n  // the title\n  \"meta\": {{\"title\": \"t\", \"note\": \"n\"}},\n  \"a\": [{}],\n  \
         \"o\": {{ /* inside */ }}\n}}\n",
        items.join(", ")
    )
}

fn quoted(text: &str) -> String {
    write(&Json::String(text.to_owned()))
}

/// One generated operation over [`document`]'s shape: some apply, some are
/// refused by the document as it then stands.
fn operation(rng: &mut Rng, n: usize) -> String {
    let index = rng.below(n.saturating_add(2));
    let other = rng.below(n.saturating_add(2));
    let element = quoted(&format!("/a/{index}"));
    let other_element = quoted(&format!("/a/{other}"));
    let member = quoted(&format!("/o/k{}", rng.below(4)));
    match rng.below(9) {
        0 => format!(r#"{{"op":"move","from":{element},"path":"/a/-"}}"#),
        1 => format!(r#"{{"op":"move","from":{element},"path":{other_element}}}"#),
        2 => format!(r#"{{"op":"add","path":{member},"value":{{"v":[1,2]}}}}"#),
        3 => format!(r#"{{"op":"remove","path":{element}}}"#),
        4 => format!(r#"{{"op":"replace","path":"/meta/note","value":"r{index}"}}"#),
        5 => format!(r#"{{"op":"copy","from":{element},"path":{member}}}"#),
        6 => format!(r#"{{"op":"move","from":{member},"path":"/meta/m"}}"#),
        7 => r#"{"op":"test","path":"/meta/title","value":"t"}"#.to_owned(),
        _ => format!(r#"{{"op":"add","path":"/a/0","value":{index}}}"#),
    }
}

/// What one accepted patch's inverse did, through the wire.
fn closes(text: &str, patch: &str) -> Result<bool, String> {
    let Ok((patched, inverse)) = patch_text(text, patch) else {
        return Ok(false);
    };
    let (back, redo) = patch_text(&patched, &inverse)
        .map_err(|error| format!("the inverse was refused: {error:?}\n  patch: {patch}"))?;
    if back != text {
        return Err(format!("the inverse gave other bytes\n  patch: {patch}"));
    }
    let (again, _) = patch_text(&back, &redo)
        .map_err(|error| format!("the redo was refused: {error:?}\n  patch: {patch}"))?;
    if again != patched {
        return Err(format!("the redo gave other bytes\n  patch: {patch}"));
    }
    Ok(true)
}

#[test]
fn the_inverse_of_every_accepted_patch_is_accepted() {
    let mut rng = Rng(0x5eed_c105);
    let mut accepted = 0_u32;
    let mut longest = 0_usize;
    let mut wrong = Vec::new();
    for round in 0..600_usize {
        let n = 1_usize.saturating_add(rng.below(70));
        let text = document(n);
        // Every count up to the cap, the cap itself included.
        let count = if round < 64 {
            round.saturating_add(1)
        } else {
            1_usize.saturating_add(rng.below(MAX_PATCH_OPERATIONS))
        };
        let ops: Vec<String> = (0..count).map(|_| operation(&mut rng, n)).collect();
        let patch = format!("[{}]", ops.join(","));
        match closes(&text, &patch) {
            Ok(true) => {
                accepted = accepted.saturating_add(1);
                longest = longest.max(count);
            }
            Ok(false) => {}
            Err(why) => wrong.push(why),
        }
    }
    // Families that apply at every count: moves (two undos each), add and
    // remove pairs, replaces and copies.
    for count in 1..=MAX_PATCH_OPERATIONS {
        let text = document(70);
        for op in [
            r#"{"op":"move","from":"/a/0","path":"/a/-"}"#,
            r#"{"op":"move","from":"/a/1","path":"/o/m"}"#,
            r#"{"op":"replace","path":"/meta/note","value":"rr"}"#,
            r#"{"op":"copy","from":"/meta","path":"/o/c"}"#,
        ] {
            let patch = format!("[{}]", vec![op; count].join(","));
            match closes(&text, &patch) {
                Ok(true) => {
                    accepted = accepted.saturating_add(1);
                    longest = longest.max(count);
                }
                Ok(false) => {}
                Err(why) => wrong.push(why),
            }
        }
    }
    println!("closure: {accepted} accepted, the longest {longest} operations");
    assert!(accepted > 300, "only {accepted} accepted");
    assert_eq!(longest, MAX_PATCH_OPERATIONS);
    assert!(
        wrong.is_empty(),
        "{} accepted patches whose inverse is not accepted; the first:\n{}",
        wrong.len(),
        wrong.iter().take(4).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// The review's case: sixty-four moves yield 128 undos, and the inverse is
/// taken by the door that issued it.
#[test]
fn the_inverse_of_sixty_four_moves_applies_and_gives_the_file_back() {
    let text = document(40);
    let ops = vec![r#"{"op":"move","from":"/a/0","path":"/a/-"}"#; MAX_PATCH_OPERATIONS];
    let patch = format!("[{}]", ops.join(","));
    assert_eq!(closes(&text, &patch), Ok(true));
}

/// An undo patch has its own bound, the most undos the forward cap's patch
/// can yield; past it, its own refusal.
#[test]
fn an_undo_patch_is_held_to_its_own_bound() {
    // Two undos a forward operation at most (a move's take and put).
    assert_eq!(MAX_UNDO_OPERATIONS, MAX_PATCH_OPERATIONS.saturating_mul(2));
    let document = "{\"a\":[]}";
    let take = r#"{"op":"remove","path":"/a/0","undo":{}}"#;
    let past = format!(
        "[{}]",
        vec![take; MAX_UNDO_OPERATIONS.saturating_add(1)].join(",")
    );
    let refused = patch_text(document, &past).expect_err("past the undo bound");
    assert_eq!(refused.bound(), Some(Bound::UndoOperations));
    // At the bound, the count is not what refuses it (the first undo takes
    // what is not there).
    let at = format!("[{}]", vec![take; MAX_UNDO_OPERATIONS].join(","));
    let refused = patch_text(document, &at).expect_err("nothing to take");
    assert_ne!(refused.bound(), Some(Bound::UndoOperations));
    assert_ne!(refused.bound(), Some(Bound::Operations));
    // A forward patch past the forward cap is still the forward bound.
    let test = r#"{"op":"test","path":"/a","value":[]}"#;
    let forward = format!(
        "[{}]",
        vec![test; MAX_PATCH_OPERATIONS.saturating_add(1)].join(",")
    );
    let refused = patch_text(document, &forward).expect_err("past the forward bound");
    assert_eq!(refused.bound(), Some(Bound::Operations));
}

/// A forward patch whose inverse fits, but whose inverse's own inverse (the
/// redo the undo would hand back) would not: sixty-four replaces of a short
/// value with a long one. Refused up front, with a sentence that says the
/// undo could not be issued, rather than applied with an undo the door would
/// refuse.
#[test]
fn a_patch_whose_undo_could_not_be_issued_is_refused_up_front() {
    let text = "{\"meta\":{\"note\":\"n\"}}";
    let path = "/meta/note";
    let paths = MAX_PATCH_OPERATIONS.saturating_mul(path.len());
    // Each replace's undo holds the value it replaced, written: the first
    // `"n"`, every later one the long value. The redo holds the long value
    // every time.
    let written = MAX_PLAYBOOK_CHARS
        .saturating_sub(paths)
        .checked_div(MAX_PATCH_OPERATIONS)
        .expect("a non-zero cap")
        .saturating_add(1);
    let long = "v".repeat(written.saturating_sub(2));
    let inverse = paths + 3 + (MAX_PATCH_OPERATIONS - 1) * written;
    let redo = paths + MAX_PATCH_OPERATIONS * written;
    assert!(inverse <= MAX_PLAYBOOK_CHARS && redo > MAX_PLAYBOOK_CHARS);
    let op = format!(r#"{{"op":"replace","path":"{path}","value":"{long}"}}"#);
    let replaces = format!("[{}]", vec![op.as_str(); MAX_PATCH_OPERATIONS].join(","));
    assert!(replaces.chars().count() <= MAX_PATCH_TEXT_CHARS);
    let refused = patch_text(text, &replaces).expect_err("its undo could not be issued");
    assert_eq!(refused.bound(), Some(Bound::Inverse));
    assert!(refused.message().contains("undo"), "{}", refused.message());
}
