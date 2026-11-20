// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The deepest playbook the readers accept, read on the smallest stack any
//! caller really has (S3's `vfyq2`, fix pass 1, item 7).
//!
//! Every reader, writer and renderer of a playbook recurses on its nesting,
//! and the grammar bounds that nesting at `MAX_DEPTH` containers. What the
//! bound is worth depends on the stack the caller runs on, so this test runs
//! each of them, on a playbook at the bound, on a thread no larger than the
//! smallest real caller's:
//!
//! * `gamectl`'s main thread, which reads a playbook in `gamectl verify` and
//!   `gamectl seat doctor`: on Windows a 1 MiB reserve, the MSVC linker's
//!   default (read from `gamectl.exe`'s PE header, 2026-11-20); 8 MiB on
//!   Linux and macOS.
//! * the gateway's surface thread and its connection threads (`serve.rs`,
//!   `net.rs`), which answer `verify_plan`, `render_plan`, `patch_plan` and
//!   `submit_plan` and parse every frame: `std::thread::spawn`'s 2 MiB.
//! * the Godot client's main thread, which calls the bridge and reads the
//!   gateway's answers with the codec: 8 MiB (`Pharmakos.exe`'s PE header,
//!   Godot's own linker flag).
//!
//! The smallest is `gamectl`'s 1 MiB on Windows, and the test runs at that, in
//! the build the tests are (debug, where frames are largest). A reader that
//! needs more is a stack overflow, which aborts this test binary: red. When
//! this was written the whole set needed between 640 KiB (overflowed) and
//! 768 KiB (passed) in a Windows debug build.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::path::{Path, PathBuf};

use pharmakos_plan_core::canonical::canonicalise_text;
use pharmakos_plan_core::context::PlanContext;
use pharmakos_plan_core::jsonc::Document;
use pharmakos_plan_core::patch::patch_text;
use pharmakos_plan_core::render::render_plan;
use pharmakos_plan_core::verify::verify_jsonc;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json::{self, grammar};
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::Scope;

/// The smallest stack a real caller reads a playbook on: `gamectl`'s main
/// thread on Windows (above).
const SMALLEST_CALLER_STACK: usize = 1 << 20;

const MINIMAL: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/plan-core sits two levels below the workspace root")
        .to_path_buf()
}

/// A rule whose condition is one `all` group around `nots` nested `not`s
/// around a predicate, each level carrying a block comment when `commented`:
/// with 27, the predicate's numbers stand inside exactly `MAX_DEPTH`
/// containers.
fn nested(nots: usize, commented: bool) -> String {
    let comment = if commented { "/* a level */ " } else { "" };
    let mut when = r#"{"segment_elapsed":{"ms":{"op":"GE","value":1}}}"#.to_owned();
    when = format!(r#"{{"all":{{"items":[{when}]}}}}"#);
    for _ in 0..nots {
        when = format!(r#"{{ {comment}"not":{{ {comment}"item":{when}}}}}"#);
    }
    MINIMAL.replace(
        "\"declarative\":{",
        &format!(
            r#""declarative":{{"handlers":[{{"id":"h","when":{when},"body":[{{"label":"b","hold":{{"ms":1}}}}],"resume":"CONTINUE"}}],"#
        ),
    )
}

/// The pointer of the predicate's `value` under `nots` nested `not`s.
fn deepest_pointer(nots: usize) -> String {
    let mut pointer = "/declarative/handlers/0/when".to_owned();
    for _ in 0..nots {
        pointer.push_str("/not/item");
    }
    pointer.push_str("/all/items/0/segment_elapsed/ms/value");
    pointer
}

/// Every reader, writer and renderer, over `text`.
fn read_everything(text: &str, nots: usize) {
    let rules = RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table");
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
    let document = Document::parse(text).expect("the deepest playbook parses");
    let tree = document.to_json();
    let written = json::write(&tree);
    json::read(&written).expect("the codec's tree reader takes it");
    json::decode::<Playbook>(&written).expect("the codec decodes it");
    let canonical = canonicalise_text(text).expect("a canonical form");
    canonicalise_text(&canonical.text).expect("the canonical form is canonical");
    json::canonical_decode::<Playbook>(&canonical.json).expect("one decode");
    for depth in [Depth::Quick, Depth::Full] {
        verify_jsonc(text, &[], &scope, &rules, depth).expect("a report");
    }
    let snapshot = Snapshot {
        seat_id: vec![0],
        seat_treasury: vec![200],
        seat_supply: vec![10],
        seat_draw: vec![2],
        ..Snapshot::default()
    };
    let context = PlanContext::from_snapshot(&snapshot, 0).expect("seat 0 is in the snapshot");
    render_plan(&canonical.playbook, &context, &rules).expect("prose");
    let patch = format!(
        r#"[{{"op":"replace","path":"{}","value":2}}]"#,
        deepest_pointer(nots)
    );
    let (patched, inverse) = patch_text(text, &patch).expect("the deepest value patches");
    patch_text(&patched, &inverse).expect("and the inverse undoes it");
}

#[test]
fn the_deepest_playbook_is_read_on_the_smallest_callers_stack() {
    // The deepest the grammar accepts, and one level past it, refused.
    let deepest = 27;
    assert!(Document::parse(&nested(deepest, false)).is_ok());
    let past = Document::parse(&nested(deepest + 1, false)).expect_err("past MAX_DEPTH");
    assert!(
        past.message.contains(&grammar::MAX_DEPTH.to_string()),
        "{}",
        past.message
    );
    for commented in [false, true] {
        let text = nested(deepest, commented);
        std::thread::Builder::new()
            .name(format!("smallest caller, commented {commented}"))
            .stack_size(SMALLEST_CALLER_STACK)
            .spawn(move || read_everything(&text, deepest))
            .expect("a thread")
            .join()
            .expect("every reader returns on the smallest caller's stack");
    }
}
