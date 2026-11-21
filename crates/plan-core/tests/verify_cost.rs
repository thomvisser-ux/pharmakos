// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's wait for a check, pinned by counted allocations rather than by
//! a clock (S3's `vfyq2`; the pattern of `crates/verifier/tests/quick_cost.rs`).
//!
//! `verify_plan` hands `verify_jsonc` the player's text, and the editor waits
//! on the whole call, not only on the verifier inside it. Before `vfyq2` the
//! call reached the verifier through the JSONC document's chain: a
//! trivia-keeping tree, a plain tree, the message, a third tree and a
//! commented text the verifier never read, each an allocation per node, and
//! together several times the verifier's own cost on the shapes P1 is
//! measured at. This crate reads no clock (AGENTS.md section 4.5); the wall
//! time is published by the walled `crates/bench`. What it can hold is the
//! count, which a return to a tree per node moves by tens of thousands.
//!
//! The shapes are `quick_cost.rs`' two at the size budget, 63 handlers and 127
//! route steps each with a 24-node condition, sent compact, as P1's
//! end-to-end figure sends them, and indented with a comment on every line, as
//! a player's editor holds them. The count is QUICK end to end through
//! `verify_jsonc`: the verifier's own allocations, which `quick_cost.rs` pins
//! apart, are inside it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — not an \
              integration test's helper functions. A panic is this file's failure report."
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::Path;

use pharmakos_plan_core::verify::verify_jsonc;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::Scope;

thread_local! {
    /// How many times **this thread** has allocated: `const`-initialised and
    /// holding a type with no destructor, so touching it from inside the
    /// global allocator neither allocates nor registers a teardown hook.
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    /// How many bytes this thread has asked for, every allocation and every
    /// growth counted at its new size: what a quadratic rebuild of a string
    /// shows in.
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

/// Count one allocation of `size` bytes against the calling thread.
fn charge(size: usize) {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get().saturating_add(1)));
    let size = u64::try_from(size).unwrap_or(u64::MAX);
    let _ = BYTES.try_with(|count| count.set(count.get().saturating_add(size)));
}

/// How many bytes this thread has asked for so far.
fn bytes() -> u64 {
    BYTES.try_with(Cell::get).unwrap_or(0)
}

/// How many times this thread has allocated so far.
fn allocations() -> u64 {
    ALLOCATIONS.try_with(Cell::get).unwrap_or(0)
}

/// The system allocator, counting.
struct Counting;

// SAFETY: every method forwards to `System` unchanged; the only addition is a
// thread-local counter increment, which cannot affect the allocation itself.
//
// `unsafe_code` is denied workspace-wide. A `GlobalAlloc` cannot be written
// without `unsafe`; the allowance is scoped to this one item in a test target,
// as `crates/verifier/tests/quick_cost.rs`, `crates/sim/tests/allocations.rs`
// and `crates/mesher/tests/allocations.rs` scope theirs, and the pull request
// raises it.
#[allow(
    unsafe_code,
    reason = "a GlobalAlloc implementation cannot be written without unsafe; scoped to this \
              test's counting allocator, which only forwards to System"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        charge(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        charge(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        charge(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// One predicate of a 24-node condition.
fn leaf(value: usize) -> String {
    format!(r#"{{"segment_elapsed":{{"ms":{{"op":"EQ","value":{value}}}}}}}"#)
}

/// A 24-node condition: 21 predicates under three nested groups, alternating
/// `any` and `all` from one condition to the next.
fn condition(id: usize, next: &mut usize) -> String {
    let leaves: Vec<String> = (0..21)
        .map(|_| {
            *next = next.saturating_add(1);
            leaf(*next)
        })
        .collect();
    let group = if id.is_multiple_of(2) { "any" } else { "all" };
    let wrap = |inner: String| format!(r#"{{"{group}":{{"items":[{inner}]}}}}"#);
    wrap(wrap(wrap(leaves.join(","))))
}

const HEAD: &str = r#""schema_version":{"major":1},"meta":{"title":"vfyq2","author_kind":"HUMAN"},"kind":"PLAYBOOK""#;
const TAIL: &str = r#""on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":1},"fallback":{"hold":{"at":{"beacon_anchor":{"safest":{}}}}}"#;

/// 63 handlers, each with a 24-node `when`, every step's `on_fail` written.
fn handler_shape() -> String {
    let mut next = 0;
    let handlers: Vec<String> = (0..63)
        .map(|id| {
            format!(
                r#"{{"id":"h{id:02}","when":{},"body":[{{"label":"h{id:02}_a","on_fail":{{"action":"SKIP"}},"hold":{{"ms":1000}}}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":2}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    format!(
        r#"{{{HEAD},"declarative":{{"route":[{{"label":"r00","on_fail":{{"action":"SKIP"}},"hold":{{"ms":1000}}}}],"handlers":[{}]}},{TAIL}}}"#,
        handlers.join(",")
    )
}

/// 127 route steps, each with a 24-node `skip_if`, every `on_fail` written.
fn route_shape() -> String {
    let mut next = 0;
    let steps: Vec<String> = (0..127)
        .map(|id| {
            format!(
                r#"{{"label":"r{id:03}","skip_if":{},"on_fail":{{"action":"SKIP"}},"hold":{{"ms":1000}}}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    format!(
        r#"{{{HEAD},"declarative":{{"route":[{}]}},{TAIL}}}"#,
        steps.join(",")
    )
}

/// `compact` with a comment before it and one between every two steps or
/// rules, a line comment and a block comment in turn: a commented file, kept
/// within `MAX_PLAYBOOK_CHARS` (the shapes' indented text is past it).
fn commented(compact: &str) -> String {
    let mut out = String::from("// A player's playbook, with notes.\n");
    let mut line = true;
    for (index, piece) in compact.split("},{\"").enumerate() {
        if index > 0 {
            out.push_str("},");
            out.push_str(if line {
                "// the next one\n"
            } else {
                "/* the next one */"
            });
            line = !line;
            out.push_str("{\"");
        }
        out.push_str(piece);
    }
    out
}

/// QUICK's allocations through `verify_jsonc` for one text, and whether it
/// qualified at the size budget.
fn verify_allocations(text: &str) -> (u64, u32, bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/plan-core sits two levels below the workspace root");
    let rules = RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table");
    let snapshot = Snapshot {
        version: SNAPSHOT_VERSION,
        match_seed: 0x0102_0304_0506_0708,
        tick: 3_600,
        ..Snapshot::default()
    }
    .to_bytes()
    .expect("the fixture snapshot");
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
    // Once uncounted, so a first touch of a lazily built table (the codec's
    // schema index) is not counted against the call.
    let _first = verify_jsonc(text, &snapshot, &scope, &rules, Depth::Quick);
    let before = allocations();
    let report = verify_jsonc(text, &snapshot, &scope, &rules, Depth::Quick)
        .expect("the rules table is complete");
    let after = allocations();
    (
        after.saturating_sub(before),
        report.size_units,
        report.qualifies,
    )
}

/// The bounds: each the count measured, with about a quarter's headroom, so a
/// return of a tree per node fails rather than hides. The chain through the
/// JSONC document before `vfyq2` made 143,575 and 274,128 allocations on the
/// compact shapes and 186,518 and 360,689 on the commented ones (`main` at
/// `64b6607`); a commented file now costs what a compact one does, because
/// its comments are blanked in one copy and never parsed.
const HANDLER_SHAPE_BOUND: u64 = 2_300; // measured 1,841
const ROUTE_SHAPE_BOUND: u64 = 2_950; // measured 2,361
const HANDLER_COMMENTED_BOUND: u64 = 2_300; // measured 1,841
const ROUTE_COMMENTED_BOUND: u64 = 2_950; // measured 2,361

#[test]
fn verify_jsonc_allocates_within_its_bound_on_both_shapes_at_the_size_budget() {
    let handlers = handler_shape();
    let route = route_shape();
    for (name, text, bound) in [
        (
            "63 handlers, compact",
            handlers.clone(),
            HANDLER_SHAPE_BOUND,
        ),
        ("127 route steps, compact", route.clone(), ROUTE_SHAPE_BOUND),
        (
            "63 handlers, commented",
            commented(&handlers),
            HANDLER_COMMENTED_BOUND,
        ),
        (
            "127 route steps, commented",
            commented(&route),
            ROUTE_COMMENTED_BOUND,
        ),
    ] {
        let (counted, units, qualified) = verify_allocations(&text);
        println!("{name}: {counted} allocations, {units} size units, qualifies {qualified}");
        assert!(qualified, "{name}: the shape qualifies");
        assert!(
            units >= 127,
            "{name}: the shape is at the size budget ({units})"
        );
        assert!(
            counted <= bound,
            "{name}: verify_jsonc made {counted} allocations, past its bound of {bound}"
        );
    }
}

/// A pointer of `tokens` tokens, `/ab` each.
fn long_pointer(tokens: usize) -> String {
    "/ab".repeat(tokens)
}

/// The bytes one call asks for, per byte of its input.
fn bytes_per_byte(input: usize, call: impl FnOnce()) -> u64 {
    let before = bytes();
    call();
    bytes()
        .saturating_sub(before)
        .checked_div(u64::try_from(input.max(1)).expect("a length"))
        .expect("a length of at least one")
}

/// A pointer is read in one pass, and a pointer with more tokens than a
/// playbook has nodes is refused (S3's `vfyq2`, fix pass 2, item A2). The
/// security review measured `patch_plan` at 0.64 to 0.82 s and `instantiate`
/// at 0.93 s on a 240 KB pointer: the pointer was rebuilt from its tokens at
/// every token, so the bytes asked for grew with its square. Held in bytes
/// asked for per input byte, at two sizes sixteen times apart.
#[test]
fn a_long_pointer_costs_bytes_linear_in_its_length() {
    const BOUND: u64 = 64;
    let document = "{\"a\":1}";
    for tokens in [500, 8_000] {
        let pointer = long_pointer(tokens);
        let quoted =
            pharmakos_proto::json::write(&pharmakos_proto::json::Json::String(pointer.clone()));
        for patch in [
            format!(r#"[{{"op":"test","path":{quoted},"value":1}}]"#),
            format!(r#"[{{"op":"remove","path":{quoted}}}]"#),
            format!(r#"[{{"op":"add","path":{quoted},"value":1}}]"#),
            format!(r#"[{{"op":"copy","from":{quoted},"path":"/b"}}]"#),
        ] {
            let per = bytes_per_byte(patch.len(), || {
                let _ = pharmakos_plan_core::patch::patch_text(document, &patch);
            });
            assert!(
                per <= BOUND,
                "{tokens} tokens: patch_text asked for {per} bytes per byte of its patch, past \
                 {BOUND}"
            );
        }
        let parameter = pharmakos_plan_core::library::Parameter {
            name: pointer.clone(),
            value: "1".to_owned(),
        };
        let per = bytes_per_byte(pointer.len(), || {
            let _ = pharmakos_plan_core::library::instantiate(
                document,
                std::slice::from_ref(&parameter),
                &[],
            );
        });
        assert!(
            per <= BOUND,
            "{tokens} tokens: instantiate asked for {per} bytes per byte of its parameter, past \
             {BOUND}"
        );
    }
}

/// A `copy` that would take the document past the cap is refused before it
/// copies (S3's `vfyq2`, fix pass 2, item A4): `apply` copies the document
/// once to work on, and refusing first means nothing more. A copy taken and
/// then refused asks for the copied subtree again, about the document's size.
#[test]
fn a_copy_past_the_cap_is_refused_before_it_copies() {
    use pharmakos_plan_core::jsonc::Document;
    use pharmakos_plan_core::patch::{Patch, apply};
    let title = "x".repeat(pharmakos_verifier::MAX_PLAYBOOK_CHARS >> 1);
    let text = format!(r#"{{"meta":{{"title":"{title}"}}}}"#);
    let document = Document::parse(&text).expect("a document");
    // What `apply` asks for to copy the document and work on it, with nothing
    // copied: a `remove` of a member that is not there.
    let baseline_patch =
        Patch::from_text(r#"[{"op":"remove","path":"/meta/nothing"}]"#).expect("a patch");
    let before = bytes();
    let _ = apply(&document, &baseline_patch);
    let baseline = bytes().saturating_sub(before);
    let patch =
        Patch::from_text(r#"[{"op":"copy","from":"/meta","path":"/copy"}]"#).expect("a patch");
    let before = bytes();
    let refused = apply(&document, &patch).expect_err("past the cap");
    let asked = bytes().saturating_sub(before);
    assert_eq!(refused.bound, Some(pharmakos_plan_core::error::Bound::Size));
    // Copying `/meta` would ask for its title again, raw and unescaped: about
    // twice the text's length more than the baseline.
    let slack = u64::try_from(text.len() >> 2).expect("a length");
    assert!(
        asked <= baseline + slack,
        "a refused copy asked for {asked} bytes against {baseline} with nothing copied: it \
         copied before it refused"
    );
}

/// A patch's text is capped before it is read (fix pass 3, item L4): one
/// past `MAX_PATCH_TEXT_CHARS` is refused by its bound, and the refusal asks
/// for no more than a sentence's bytes, because no tree was built.
#[test]
fn a_patch_text_past_its_cap_is_refused_unread() {
    use pharmakos_plan_core::patch::MAX_PATCH_TEXT_CHARS;
    let patch = format!("[{}]", " ".repeat(MAX_PATCH_TEXT_CHARS.saturating_sub(1)));
    assert_eq!(
        patch.chars().count(),
        MAX_PATCH_TEXT_CHARS.saturating_add(1)
    );
    let before = bytes();
    let refused = pharmakos_plan_core::patch::patch_text("{\"a\":1}", &patch).expect_err("past");
    let asked = bytes().saturating_sub(before);
    assert_eq!(
        refused.bound,
        Some(pharmakos_plan_core::error::Bound::PatchText)
    );
    assert!(
        asked <= 4096,
        "the refusal asked for {asked} bytes: it read the patch"
    );
}

/// The bytes `patch_text` asks for are linear in its patch's bytes, at the
/// largest patches that act on a file (fix pass 3, item L4): sixty-four
/// operations, each replacing a value with a long string, at two sizes
/// sixteen times apart.
#[test]
fn a_patch_asks_for_bytes_linear_in_its_text() {
    const BOUND: u64 = 48;
    let document = "{\"meta\":{\"title\":\"t\",\"note\":\"n\"}}";
    for length in [256, 4_096] {
        let value = "v".repeat(length);
        let operations: Vec<String> = (0..64)
            .map(|index| {
                let path = if index % 2 == 0 {
                    "/meta/title"
                } else {
                    "/meta/note"
                };
                format!(r#"{{"op":"replace","path":"{path}","value":"{value}"}}"#)
            })
            .collect();
        let patch = format!("[{}]", operations.join(","));
        let per = bytes_per_byte(patch.len(), || {
            let _ = pharmakos_plan_core::patch::patch_text(document, &patch);
        });
        assert!(
            per <= BOUND,
            "a {}-byte patch asked for {per} bytes per byte, past {BOUND}",
            patch.len()
        );
    }
}

/// A chain of copies, each refused before it copies once the next would pass
/// the cap, asks for at most a constant times the cap however long the chain
/// (fix pass 3, item L4): sixty-four `copy`s of a quarter-cap subtree.
#[test]
fn a_chain_of_copies_asks_for_at_most_a_constant_times_the_cap() {
    // The bound: `apply` copies the document once to work on (the cap), each
    // copy that lands copies its subtree (a quarter of the cap, at most three
    // land), the inverse holds what each copy put, and the text written back
    // is the cap once more; twenty times the cap is room for all of it with
    // the allocator's growth.
    const TIMES_THE_CAP: u64 = 20;
    let cap = pharmakos_verifier::MAX_PLAYBOOK_CHARS;
    let title = "x".repeat(cap >> 2);
    let text = format!(r#"{{"meta":{{"title":"{title}"}}}}"#);
    let operations: Vec<String> = (0..64)
        .map(|index| format!(r#"{{"op":"copy","from":"/meta","path":"/copy{index}"}}"#))
        .collect();
    let patch = format!("[{}]", operations.join(","));
    let before = bytes();
    let refused = pharmakos_plan_core::patch::patch_text(&text, &patch).expect_err("past the cap");
    let asked = bytes().saturating_sub(before);
    assert_eq!(refused.bound, Some(pharmakos_plan_core::error::Bound::Size));
    let most = u64::try_from(cap).expect("a length") * TIMES_THE_CAP;
    assert!(
        asked <= most,
        "a chain of copies asked for {asked} bytes, past {TIMES_THE_CAP} times the cap"
    );
}
