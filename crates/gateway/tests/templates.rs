// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The skeleton's template library, read the way a hosted match reads it.
//!
//! Decisions-log item 81 ships three templates -- Hold & Build, Expand & Mine
//! and the Safe Playbook -- and item 111 (decisions C13 and C16) puts them in
//! a flat `library/` at the repository root, declaring their own parameters,
//! with the safe one written to spec section 14. Two things are held here:
//! every template is a playbook both seats of the golden seed can seal, and the
//! gateway's fallback is the safe template with nothing raised ("written once,
//! tested twice").
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::SAFE_PLAYBOOK;
use pharmakos_proto::json::Json;

use support::{MATCH, SEGMENT_MS, library_folder, result, text_of};

/// The three templates item 81 names, by file stem.
const SHIPPED: [&str; 3] = ["expand_and_mine", "hold_and_build", "safe_playbook"];

/// A JSON string literal, for building params.
fn quote(text: &str) -> String {
    pharmakos_proto::json::write(&Json::String(text.to_owned()))
        .trim_end()
        .to_owned()
}

/// Every template in the library instantiates with its own values, verifies
/// FULL, and is accepted by `submit_plan` -- which also compiles it -- for
/// both seats of the golden seed.
///
/// Both seats, because a template's own values are what a seat with no
/// operator's suggestion gets, and a value that only one seat's position made
/// legal (a fixed voxel inside one seat's sphere) would be a template the
/// other seat could not submit.
#[test]
fn each_template_instantiates_and_qualifies_for_both_seats_of_the_golden_seed() {
    let mut surface = support::hosted_with(
        MATCH,
        FogPolicy::fogged(),
        2,
        SEGMENT_MS,
        3,
        Some(library_folder()),
    );
    // A client reporting its own timer as it goes, as the editor does: a
    // Lull's tick moves only on the client's word, and a socket's rate is per
    // tick.
    let mut left = support::LULL_MS;
    for seat in [0_u8, 1] {
        let token = support::seat_token(&mut surface, seat);
        let listed = result(
            &support::call_in_lull(&mut surface, &token, &mut left, "list_templates", "{}"),
            "list_templates",
        );
        let ids: Vec<String> = support::array_of(&listed, "templates")
            .iter()
            .map(|row| text_of(row, "template_id"))
            .collect();
        assert_eq!(ids, SHIPPED, "the library lists the three, in id order");
        for row in support::array_of(&listed, "templates") {
            assert!(!text_of(&row, "title").is_empty());
            assert!(!text_of(&row, "summary").is_empty());
        }

        for template in SHIPPED {
            let made = result(
                &support::call_in_lull(
                    &mut surface,
                    &token,
                    &mut left,
                    "instantiate_template",
                    &format!(r#"{{"template_id":"{template}"}}"#),
                ),
                "instantiate_template",
            );
            assert!(
                !support::array_of(&made, "parameters").is_empty(),
                "{template} declares what its wizard asks for"
            );
            let playbook = text_of(&made, "playbook_jsonc");
            let verified = result(
                &support::call_in_lull(
                    &mut surface,
                    &token,
                    &mut left,
                    "verify_plan",
                    &format!(
                        r#"{{"depth":"full","playbook_jsonc":{}}}"#,
                        quote(&playbook)
                    ),
                ),
                "verify_plan",
            );
            let report = verified.get("report").cloned().expect("a report");
            assert_eq!(
                report.get("qualifies"),
                Some(&Json::Bool(true)),
                "{template} for seat {seat}: {report:?}"
            );
            let submitted = result(
                &support::call_in_lull(
                    &mut surface,
                    &token,
                    &mut left,
                    "submit_plan",
                    &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&playbook)),
                ),
                "submit_plan: the sim's own door compiles it",
            );
            assert_eq!(
                submitted.get("accepted"),
                Some(&Json::Bool(true)),
                "{template} for seat {seat}"
            );
        }
    }
}

/// `SAFE_PLAYBOOK` is the Safe Playbook template instantiated with nothing
/// raised: canonical-equal, comments aside (decision C16, item 81's "written
/// once, tested twice").
///
/// So the file the editor renders as "the cost of a timeout" and the constant
/// a timeout files for a seat no operator advises cannot drift apart: a change
/// to either fails here until the other follows.
#[test]
fn the_gateways_fallback_is_the_safe_template_with_nothing_raised() {
    let template = std::fs::read_to_string(library_folder().join("safe_playbook.jsonc"))
        .expect("the safe template");
    let made = pharmakos_plan_core::instantiate(&template, &[], &[]).expect("it instantiates");
    assert!(
        made.parameters.iter().all(|parameter| !parameter.suggested),
        "nothing suggested"
    );
    let from_template =
        pharmakos_plan_core::canonicalise_text(&made.playbook_jsonc).expect("canonical");
    let constant = pharmakos_plan_core::canonicalise_text(SAFE_PLAYBOOK).expect("canonical");
    assert_eq!(
        constant.playbook, from_template.playbook,
        "the gateway's fallback and the safe template with nothing raised"
    );
    // Canonical text too, which is the form a fingerprint is taken over.
    let strip = |text: &str| -> String {
        text.lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<&str>>()
            .join("\n")
    };
    assert_eq!(strip(&constant.text), strip(&from_template.text));
}
