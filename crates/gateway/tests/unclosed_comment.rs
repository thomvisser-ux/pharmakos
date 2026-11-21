// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A block comment that never closes, end to end through the gateway (S3's
//! `vfyq2`).
//!
//! Before `vfyq2`, `verify_plan` blanked such a comment to the end of the
//! file, and the codec read what came before it as a whole playbook, so a
//! qualifying playbook with `/*` after it was reported as qualifying; and
//! `submit_plan`, whose compile reads the file through the JSONC reader, which
//! refused it, then answered `INTERNAL`: "a playbook the verifier read has no
//! canonical form". Both readers now refuse it, as `E0001` at the byte where
//! the comment opens, so the report does not qualify and the submission is a
//! report that says so, not an error.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_proto::json::Json;

use support::{FIRST_LULL_MS, SEGMENT_MS, call_in_lull, hosted_as, quote, result, seat_token};

/// The report's verdict and each diagnostic's code and path.
fn verdict(response: &Json, what: &str) -> (bool, Vec<(String, String)>) {
    let answer = result(response, what);
    let report = answer.get("report").expect("a report");
    let qualifies = report.get("qualifies") == Some(&Json::Bool(true));
    let found = support::array_of(report, "diagnostics")
        .iter()
        .map(|diagnostic| {
            (
                support::text_of(diagnostic, "code"),
                support::text_of(diagnostic, "path"),
            )
        })
        .collect();
    (qualifies, found)
}

#[test]
fn an_unclosed_block_comment_is_refused_by_verify_and_by_submit_alike() {
    let mut surface = hosted_as("m-vfyq2-unclosed", FogPolicy::fogged(), 2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let playbook = support::walk_east(&surface, 0, 3, 0);
    let plain = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "verify_plan",
        &format!(
            r#"{{"playbook_jsonc":{},"depth":"full"}}"#,
            quote(&playbook)
        ),
    );
    assert!(
        verdict(&plain, "verify_plan").0,
        "the playbook alone qualifies"
    );

    let open = playbook.trim_end().len().saturating_add(1);
    let unclosed = format!("{} /* a note that is never closed", playbook.trim_end());
    let expected = (false, vec![("E0001".to_owned(), format!("/byte/{open}"))]);
    let verified = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "verify_plan",
        &format!(
            r#"{{"playbook_jsonc":{},"depth":"full"}}"#,
            quote(&unclosed)
        ),
    );
    assert_eq!(verdict(&verified, "verify_plan"), expected);

    let submitted = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&unclosed)),
    );
    assert_eq!(verdict(&submitted, "submit_plan"), expected);
    assert_eq!(
        result(&submitted, "submit_plan").get("accepted"),
        Some(&Json::Bool(false)),
        "the submission is refused by its report, not by an error"
    );
}
