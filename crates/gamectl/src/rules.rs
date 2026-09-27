// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where this binary finds the rules table, and the one place it loads one.
//!
//! `rules/rules.v1.json` is the canonical JSON of one `gp.v1.RulesTable`
//! (decisions-log item 78): every `$`, every `kW`, the interface times, the
//! segment ladder and the verifier's own limits. The sim is a pure function of
//! (map seed, playbooks, rules hash), so nothing this binary reports means
//! anything without saying which table produced it — which is why every command
//! that needs one loads it through here and why `seat doctor` prints its hash.
//!
//! **Where the table lives beside a shipped binary** (decisions-log item
//! 117 (3)): the zip is laid out like the repository, so a shipped `gamectl`
//! reads `rules/rules.v1.json` beside itself exactly as a checkout's does,
//! relative to `--root` or the working directory. `gamectl seat doctor` passes
//! when run from the extracted folder, and from anywhere else it needs `--root`
//! (the zip's note says so); the lobby passes the executable's folder as the
//! root. `gamectl`'s default root stays the working directory (item 117 (14)).

use std::path::Path;

use pharmakos_sim::rules::RulesTable;

use crate::exit::Failure;
use crate::strings;

/// The table's path from the repository root.
pub const RULES_PATH: &str = "rules/rules.v1.json";

/// Load the shipped rules table.
///
/// # Errors
///
/// [`crate::exit::Exit::Input`] when the file is not there or will not decode
/// — from a shell that is almost always "you are not in a checkout", so the
/// message says the path it looked at.
pub fn load(root: &Path) -> Result<RulesTable, Failure> {
    let path = root.join(RULES_PATH);
    RulesTable::load(&path).map_err(|error| {
        Failure::input(strings::unreadable(
            "rules table",
            &crate::display(&path),
            &error.to_string(),
        ))
    })
}
