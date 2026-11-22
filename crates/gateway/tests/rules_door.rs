// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host checks its rules table once, when it starts (S3's `vfyq2`, fix
//! pass 5, item L8).
//!
//! The verifier refuses a rules table it cannot read a rate card from (a
//! negative rate, a missing block), so under such a table every
//! `verify_plan` would answer a refusal. A host that opened a match on one
//! would host a match nobody can plan. So the host reads its table through
//! the verifier's own rules checks, the same call `Input` makes, before
//! anything is generated or announced. `gamectl host` (through `serve::run`
//! and `Host::open_from`), `scenario run` and `seat doctor` (through
//! `Host::open`) all meet it.
//!
//! A table whose interface rates let a row run past the sim's `i32` is never
//! built: the sim's own loader refuses it (S3's `p3fix`:
//! `RulesError::RowPastTheSim`), at the bound the verifier's retired
//! `RulesGap::RowPastTheSim` stated (`the_loader_holds_the_row_bound`), so the
//! host's load refuses it before the verifier is asked.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::Code;
use pharmakos_gateway::host::{Host, Settings};
use pharmakos_sim::rules::RulesTable;

/// The shipped table with a negative build-target rate: the sim's loader
/// takes it (its compile refuses every playbook on it, `NegativeRate`), and
/// the verifier reads no rate card from it.
fn gapped() -> RulesTable {
    let mut message = support::rules().message().clone();
    if let Some(times) = message.interface_times.as_mut() {
        times.build_target_ms = -1;
    }
    RulesTable::from_message(&message).expect("the sim's loader takes it")
}

/// The shipped table with the rates and budget that put the longest row a
/// playbook within the budget can write at exactly `i32::MAX` ms, moved by
/// `past` ms on the switch: 21 647 + 6 000 + 16 777 000 x 128 = 2^31 - 1.
fn at_the_row_bound(past: i32) -> pharmakos_proto::gp::v1::RulesTable {
    let mut message = support::rules().message().clone();
    let times = message.interface_times.as_mut().expect("the shipped rates");
    times.switch_mandate_ms = 21_647 + past;
    times.edit_settings_max_ms = 6_000;
    times.build_target_ms = 16_777_000;
    message
        .verifier
        .as_mut()
        .expect("the verifier block")
        .size_budget_units = 128;
    message
}

/// The sim's loader holds a table to one bound on the longest row a playbook
/// within its size budget can write (S3's `p3fix`, from `vfyq2`'s reviews):
/// its own, derived from `Row::duration_ms`'s sums
/// (`InterfaceTimes::longest_row_ms`), which is the bound the verifier's
/// retired `RulesGap::RowPastTheSim` stated, `switch_mandate +
/// edit_settings_max + build_target x size_budget_units <= i32::MAX`. On a
/// table at the bound, one below it and one above it, the loader takes, takes
/// and refuses (`RulesError::RowPastTheSim`, naming the three rates, the
/// budget and the sum), and the verifier takes the two it is handed; and the
/// sim's sum, asked of the at-bound table's rates one size unit below, at and
/// above its budget, agrees with the stated formula each time, at exactly
/// `i32::MAX` on the budget. So `PlanError::RowTooLong` cannot be reached
/// from a loaded table by a playbook within its budget; the compile keeps it
/// for a playbook past the budget, which the compile does not count.
#[test]
fn the_loader_holds_the_row_bound() {
    use pharmakos_sim::interpreter::InterfaceTimes;
    use pharmakos_sim::rules::RulesError;
    for past in [-1, 0] {
        let table = RulesTable::from_message(&at_the_row_bound(past))
            .unwrap_or_else(|error| panic!("{past} ms past the bound: {error}"));
        assert!(
            pharmakos_verifier::read_rules(&table).is_ok(),
            "{past} ms past the bound: the verifier refuses what the sim loads"
        );
    }
    let above = RulesTable::from_message(&at_the_row_bound(1));
    assert_eq!(
        above.as_ref().err(),
        Some(&RulesError::RowPastTheSim {
            switch_ms: 21_648,
            edit_max_ms: 6_000,
            build_target_ms: 16_777_000,
            budget: 128,
        }),
        "one ms past the bound, the sim's loader refuses the table"
    );
    let said = above
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    for part in [
        "interface_times.switch_mandate_ms",
        "21648",
        "interface_times.edit_settings_max_ms",
        "6000",
        "interface_times.build_target_ms",
        "16777000",
        "128",
        "2147483648",
    ] {
        assert!(said.contains(part), "the refusal names {part}: {said}");
    }

    let at = RulesTable::from_message(&at_the_row_bound(0)).expect("the bound loads");
    let sim = InterfaceTimes::from_rules(&at).expect("the rate card");
    for budget in [127_u32, 128, 129] {
        let stated = 21_647_i128 + 6_000 + 16_777_000 * i128::from(budget);
        assert_eq!(
            sim.longest_row_ms(budget).is_ok(),
            stated <= i128::from(i32::MAX),
            "budget {budget}: the sim's bound and the stated one disagree ({stated} ms)"
        );
    }
    assert_eq!(sim.longest_row_ms(128), Ok(i32::MAX), "the bound is exact");
}

/// A table past the row bound hosts no match: the host's own load of it
/// refuses, before the verifier is asked, naming the rates.
#[test]
fn a_rules_table_past_the_row_bound_hosts_no_match() {
    let settings = Settings {
        segment_lengths_ms: vec![3_000],
        round_limit: 3,
    };
    let json = pharmakos_proto::json::encode(&at_the_row_bound(1)).expect("the table encodes");
    let Err(refused) = Host::open_from(&json, support::SEED, 2, &settings, None) else {
        panic!("the host refuses the table");
    };
    assert!(
        refused
            .message()
            .contains("interface_times.build_target_ms"),
        "the refusal names the row: {}",
        refused.message()
    );
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
