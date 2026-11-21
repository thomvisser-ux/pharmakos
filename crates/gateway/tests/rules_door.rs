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
