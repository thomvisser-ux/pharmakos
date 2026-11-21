// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl verify` over the deepest text the grammar accepts, and the
//! shallowest it refuses, on half the stack `gamectl`'s `main` gives its
//! command (S3's `vfyq2`, fix pass 2, item E).
//!
//! `main` runs the whole command on a thread of
//! [`pharmakos_plan_core::stack::PLAYER_TEXT_STACK`] bytes; this runs the
//! library's [`run`], which is everything `main` runs, on half of it, in the
//! tests' debug build. A pass is a factor of two over the worst build. A
//! reader that needs more overflows, which aborts this test binary: red.
#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an integration test's helpers. A panic is this file's failure report."
)]

use std::path::{Path, PathBuf};

use pharmakos_gamectl::exit::Exit;
use pharmakos_gamectl::run;
use pharmakos_plan_core::stack::PLAYER_TEXT_STACK;

const MINIMAL: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

/// The `not`s that put a handler condition's numbers inside exactly
/// `MAX_DEPTH` containers (plan-core's `tests/stack.rs` counts them).
const DEEPEST_NOTS: usize = 27;

fn nested(nots: usize) -> String {
    let mut when = r#"{"segment_elapsed":{"ms":{"op":"GE","value":1}}}"#.to_owned();
    when = format!(r#"{{"all":{{"items":[{when}]}}}}"#);
    for _ in 0..nots {
        when = format!(r#"{{ /* a level */ "not":{{ /* a level */ "item":{when}}}}}"#);
    }
    MINIMAL.replace(
        "\"declarative\":{",
        &format!(
            r#""declarative":{{"handlers":[{{"id":"h","when":{when},"body":[{{"label":"b","hold":{{"ms":1}}}}],"resume":"CONTINUE"}}],"#
        ),
    )
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/gamectl sits two levels below the workspace root")
        .to_path_buf()
}

fn verify_both() {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gamectl-stack");
    std::fs::create_dir_all(&folder).expect("a scratch folder");
    for (name, nots) in [("deepest", DEEPEST_NOTS), ("past", DEEPEST_NOTS + 1)] {
        let path = folder.join(format!("{name}.jsonc"));
        std::fs::write(&path, nested(nots)).expect("the text is written");
        for depth in ["quick", "full"] {
            let outcome = run(
                [
                    "verify".to_owned(),
                    path.display().to_string(),
                    "--depth".to_owned(),
                    depth.to_owned(),
                ],
                &root(),
            );
            // The deepest text gets a report, qualifying or not; the past one
            // is refused with the verifier's report. Neither is an internal
            // failure, and neither overflowed to get here.
            assert!(
                matches!(outcome.code, Exit::Ok | Exit::Failed),
                "{name}, {depth}: {:?} {}{}",
                outcome.code,
                outcome.out,
                outcome.err
            );
            if name == "past" {
                assert!(
                    matches!(outcome.code, Exit::Failed)
                        && format!("{}{}", outcome.out, outcome.err).contains("E0001"),
                    "{name}, {depth}: refused with E0001: {}{}",
                    outcome.out,
                    outcome.err
                );
            }
        }
    }
}

#[test]
fn gamectl_verify_reads_the_deepest_text_on_half_the_stated_stack() {
    std::thread::Builder::new()
        .name("half the stated stack".to_owned())
        .stack_size(PLAYER_TEXT_STACK >> 1)
        .spawn(verify_both)
        .expect("a thread")
        .join()
        .expect("gamectl verify answers on half the stated stack");
}
