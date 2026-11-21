// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host checks its rules table once, when it starts (S3's `vfyq2`, fix
//! pass 5, item L8).
//!
//! The verifier refuses a rules table whose interface rates let a row run
//! past the sim's `i32` (`RulesGap::RowPastTheSim`), so under such a table
//! every `verify_plan` would answer a refusal. A host that opened a match on
//! one would host a match nobody can plan. So the host reads its table
//! through the verifier's own rules checks, the same call `Input` makes,
//! before anything is generated or announced. `gamectl host` (through
//! `serve::run` and `Host::open_from`), `scenario run` and `seat doctor`
//! (through `Host::open`) all meet it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::Code;
use pharmakos_gateway::host::{Host, Settings};
use pharmakos_sim::rules::RulesTable;

/// The shipped table with the build-target rate at `i32::MAX`: one row a
/// playbook within the size budget can write passes the sim's `i32`.
fn gapped() -> RulesTable {
    let mut message = support::rules().message().clone();
    if let Some(times) = message.interface_times.as_mut() {
        times.build_target_ms = i32::MAX;
    }
    RulesTable::from_message(&message).expect("the sim's loader takes it")
}

#[test]
fn a_rules_table_the_verifier_refuses_hosts_no_match() {
    let settings = Settings {
        segment_lengths_ms: vec![3_000],
        round_limit: 3,
    };
    let Err(refused) =
        Host::open_from(gapped().canonical_json(), support::SEED, 2, &settings, None)
    else {
        panic!("the host refuses the table");
    };
    assert_eq!(
        refused.code(),
        Code::InvalidArgument,
        "{}",
        refused.message()
    );
    assert!(
        refused
            .message()
            .contains("interface_times.build_target_ms"),
        "the refusal names the row: {}",
        refused.message()
    );
    // The shipped table hosts.
    assert!(Host::open_from(&support::rules_json(), support::SEED, 2, &settings, None).is_ok());
}

/// A refused start writes nothing (S3's `vfyq2`, fix pass 6, item 3): the
/// rules check comes before the private match cache is opened, so no match
/// folder, `match.json` or `README.txt` is left behind for a match that
/// never started.
#[test]
fn a_refused_start_writes_nothing() {
    use pharmakos_gateway::serve::{Config, ConfigLine, NoOperators, Setup};
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rules-door-empty");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a data root");
    let line = ConfigLine {
        config: Config {
            match_id: "m-rules-door".to_owned(),
            seed: support::SEED,
            seats: 2,
            human_seat: None,
            segment_lengths_ms: vec![1_000],
            round_limit: 2,
            preset: pharmakos_gateway::probation::Preset::Custom,
        },
        resume: false,
    };
    let setup = Setup {
        rules_json: gapped().canonical_json().to_owned(),
        library: None,
        operators: Box::new(NoOperators),
    };
    let control = std::io::Cursor::new(format!("{}\n", line.render()).into_bytes());
    let mut announce: Vec<u8> = Vec::new();
    let refused = pharmakos_gateway::serve::run_in(setup, &root, control, &mut announce)
        .expect_err("the host refuses the table");
    assert!(
        refused
            .message()
            .contains("interface_times.build_target_ms"),
        "{}",
        refused.message()
    );
    assert!(announce.is_empty(), "nothing was announced");
    let left: Vec<std::path::PathBuf> = walk(&root);
    assert!(left.is_empty(), "a refused start left {left:?}");
}

/// Every file under `root`, sorted.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml sanctions read_dir when the listing is collected and sorted before use, which is what happens here"
)]
fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            found.push(path);
        }
    }
    found.sort();
    found
}
