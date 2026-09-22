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
//! PLACEHOLDER: **where the table lives beside a shipped binary** is packaging's
//! question, and packaging is **T21** (skeleton plan §3: "`gamectl` shipped
//! alongside; the template and sample folders where T13 expects them"). Until
//! then it is read from the repository, relative to `--root` or the working
//! directory — which is what `cargo xtask ci` gives it and what a developer in
//! a checkout has. T21 decides whether a shipped `gamectl` carries the table,
//! reads it from an install directory, or refuses without `--root`.

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
            &path.display().to_string(),
            &error.to_string(),
        ))
    })
}
