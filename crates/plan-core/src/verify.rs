// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pipeline from a JSONC file to a verifier report.
//!
//! `plan-core` runs in process with the Seat Gateway and is the layer between
//! the **file** and the **verifier**: the verifier takes bytes and hashes what
//! it decodes (`pharmakos_verifier::Input::new`), so somebody has to decide
//! *which* bytes. This module does, and the decision is worth stating:
//!
//! **The verifier sees the canonical form.** A file's comments and formatting
//! are the author's, not the plan's, so two files that differ only in their
//! comments produce the same `report_hash` — a player can annotate a playbook
//! without moving its hash, and the pre-check at edit time and the check at
//! submit compare, which is what spec section 11's "byte-identical" promise is
//! for.
//!
//! **A file that does not parse is handed over as it stands.** There is no
//! canonical form of a broken file, and this crate must not invent a
//! diagnostic of its own: the verifier owns the catalogue. So the raw bytes go
//! through and come back as `E0001` with a `/byte/<offset>` pointer, which is
//! the shape decisions-log item 96 (2) settled for exactly this case.

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_sim::rules::RulesTable;
use pharmakos_verifier::{Input, Scope};

use crate::canonical::canonicalise_text;
use crate::error::Error;

/// Verifies a JSONC playbook file.
///
/// `snapshot` is the seat's frozen planning snapshot as postcard bytes, and
/// `scope` is the seat's view of it — both opaque here and both hashed inputs
/// of the report.
///
/// # Errors
///
/// Only when the rules table does not carry a block the checks read. Anything
/// wrong with the *playbook* comes back as a diagnostic inside the report,
/// which is the difference between a verifier and a parser.
pub fn verify_jsonc(
    playbook_jsonc: &str,
    snapshot: &[u8],
    scope: &Scope,
    rules: &RulesTable,
    depth: Depth,
) -> Result<VerifyReport, Error> {
    let canonical = canonicalise_text(playbook_jsonc).map(|canonical| canonical.json);
    let bytes: &[u8] = match canonical.as_ref() {
        Ok(json) => json.as_bytes(),
        Err(_unparseable) => playbook_jsonc.as_bytes(),
    };
    let input = Input::new(bytes, snapshot, scope, rules)
        .map_err(|gap| Error::at("", format!("the rules table is incomplete: {gap}")))?;
    Ok(pharmakos_verifier::verify(&input, depth))
}

#[cfg(test)]
mod tests {
    use super::verify_jsonc;
    use pharmakos_proto::gp::api::v1::verify_plan::Depth;
    use pharmakos_sim::rules::RulesTable;
    use pharmakos_verifier::Scope;

    const MINIMAL: &str = concat!(
        "{\"schema_version\":{\"major\":1},",
        "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
        "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
        "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
        "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
        "\"kind\":\"PLAYBOOK\"}"
    );

    fn rules() -> RulesTable {
        RulesTable::load(std::path::Path::new("../../rules/rules.v1.json"))
            .expect("the committed rules table")
    }

    fn scope() -> Scope {
        Scope::new(
            pharmakos_sim::tables::SeatId::new(0),
            pharmakos_sim::knowledge::SeatEconomy {
                treasury: pharmakos_sim::math::quantity::Money::new(200),
                supply: pharmakos_sim::math::quantity::Kw::new(10),
                draw: pharmakos_sim::math::quantity::Kw::new(2),
            },
        )
    }

    #[test]
    fn a_file_qualifies_through_the_canonical_form() {
        let report = verify_jsonc(MINIMAL, &[], &scope(), &rules(), Depth::Full)
            .expect("the rules table is complete");
        assert!(report.qualifies, "{:?}", report.diagnostics);
    }

    #[test]
    fn comments_do_not_move_the_report_hash() {
        let plain = verify_jsonc(MINIMAL, &[], &scope(), &rules(), Depth::Full).expect("a report");
        let noisy = verify_jsonc(
            &format!("// a note\n{MINIMAL}\n"),
            &[],
            &scope(),
            &rules(),
            Depth::Full,
        )
        .expect("a report");
        assert_eq!(plain.report_hash, noisy.report_hash);
        assert_eq!(plain.plan_fingerprint, noisy.plan_fingerprint);
    }

    #[test]
    fn a_file_that_is_not_json_comes_back_as_a_diagnostic_at_a_byte() {
        let report = verify_jsonc("{ this is not json", &[], &scope(), &rules(), Depth::Quick)
            .expect("a report");
        assert!(!report.qualifies);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path.starts_with("/byte/")),
            "{:?}",
            report.diagnostics
        );
    }

    #[test]
    fn quick_and_full_are_both_reachable() {
        let quick = verify_jsonc(MINIMAL, &[], &scope(), &rules(), Depth::Quick).expect("a report");
        let full = verify_jsonc(MINIMAL, &[], &scope(), &rules(), Depth::Full).expect("a report");
        assert_ne!(quick.report_hash, full.report_hash);
    }
}
