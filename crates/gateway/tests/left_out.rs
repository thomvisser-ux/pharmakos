// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **What an answer left out, on the wire as typed data** (S3's `leftout`;
//! decisions-log item 183 (5)).
//!
//! * `verify_plan` sends the report's `left_out` field as the verifier wrote
//!   it: one entry per closing `I0007`, in the same order, and no field at all
//!   on a report that left nothing out.
//! * `list_templates` says the library files it refused, by reason and count
//!   and never by name: one entry per reason, in the enum's order, the whole
//!   listing's on its one page, the same on every listing and never a sum.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_plan_core::library::Refusal;
use pharmakos_proto::gp::api::v1::refused_templates::Reason;
use pharmakos_proto::gp::api::v1::{ListTemplatesResponse, VerifyPlanResponse};
use pharmakos_proto::json::{self, Json};

use support::{MATCH, SEGMENT_MS, result};

/// A playbook of `steps` holds, each with its `on_fail` omitted: one `I0003`
/// note a step, so past the cap's number the report leaves some out.
fn holds(steps: usize) -> String {
    let route: Vec<String> = (0..steps)
        .map(|index| format!(r#"{{"label":"s{index}","hold":{{"ms":1}}}}"#))
        .collect();
    format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"t","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{}]}},"on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#,
        route.join(",")
    )
}

/// `verify_plan`'s answer for `text`, read as a client reads it.
fn verified(text: &str) -> (Json, VerifyPlanResponse) {
    let mut surface = support::hosted(1, SEGMENT_MS, 2);
    let token = support::seat_token(&mut surface, 0);
    let mut left = support::FIRST_LULL_MS;
    let params = json::write(&Json::Object(vec![(
        String::from("playbook_jsonc"),
        Json::String(text.to_owned()),
    )]));
    let answer = result(
        &support::call_in_lull(&mut surface, &token, &mut left, "verify_plan", &params),
        "verify_plan",
    );
    let typed = json::decode_json(&support::canonical_answer(
        "gp.api.v1.VerifyPlanResponse",
        &answer,
    ))
    .expect("the answer is a VerifyPlanResponse");
    (answer, typed)
}

#[test]
fn verify_plan_sends_the_left_out_field_as_the_verifier_wrote_it() {
    let (_, typed) = verified(&holds(30));
    let report = typed.report.expect("a report");
    let entries: Vec<(&str, u32, u32)> = report
        .left_out
        .iter()
        .map(|gap| (gap.code.as_str(), gap.count, gap.max))
        .collect();
    let cap = pharmakos_verifier::MAX_PER_CODE.get();
    let notes = report
        .diagnostics
        .iter()
        .filter(|found| found.code == "I0003")
        .count();
    let left = report
        .left_out
        .iter()
        .find(|gap| gap.code == "I0003")
        .expect("the notes past the cap are left out");
    assert_eq!(u32::try_from(notes).expect("fits"), cap, "{entries:?}");
    assert_eq!(left.max, cap);
    assert_eq!(
        left.severity(),
        pharmakos_proto::gp::api::v1::diagnostic::Severity::Info,
        "the entry carries its code's severity"
    );
    // Thirty steps' notes and the one for the death limit's absence are all
    // there, listed or counted.
    assert!(left.count >= 30 - cap, "{entries:?}");
    let markers: Vec<&str> = report
        .diagnostics
        .iter()
        .filter(|found| found.code == "I0007")
        .map(|found| found.message.as_str())
        .collect();
    assert_eq!(markers.len(), report.left_out.len(), "one I0007 an entry");
    for (marker, gap) in markers.iter().zip(&report.left_out) {
        assert!(
            marker.starts_with(&format!("{} more `{}`", gap.count, gap.code)),
            "{marker} is not {gap:?}"
        );
    }
}

#[test]
fn a_report_that_left_nothing_out_carries_no_field() {
    let (answer, typed) = verified(&holds(1));
    let report = answer.get("report").expect("a report");
    assert!(report.get("left_out").is_none(), "{report:?}");
    assert!(typed.report.expect("a report").left_out.is_empty());
}

/// A library folder holding one file of each refused kind beside the shipped
/// templates: a directory named like a template (unreadable), a file past
/// the cap (too large), and two files that are not playbooks.
fn refusing_library(name: &str) -> std::path::PathBuf {
    let folder = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a folder");
    // The order does not matter: each file is copied, and nothing is listed.
    #[allow(
        clippy::disallowed_methods,
        reason = "the listing only copies each file, and its order never reaches an output"
    )]
    let entries = std::fs::read_dir(support::library_folder()).expect("the library");
    for entry in entries {
        let path = entry.expect("an entry").path();
        if let Some(file) = path.file_name() {
            std::fs::copy(&path, folder.join(file)).expect("copied");
        }
    }
    std::fs::create_dir_all(folder.join("a_folder.jsonc")).expect("a directory");
    std::fs::write(
        folder.join("big.jsonc"),
        vec![b' '; pharmakos_plan_core::library::MAX_FILE_BYTES + 1],
    )
    .expect("written");
    std::fs::write(folder.join("not_text.jsonc"), [0xff_u8, 0xfe, 0x00]).expect("written");
    std::fs::write(folder.join("not_json.jsonc"), "{ this is not a playbook").expect("written");
    folder
}

/// The `list_templates` answer, read as a client reads it.
fn listed(
    surface: &mut pharmakos_gateway::surface::Surface,
    left: &mut i32,
) -> (Json, ListTemplatesResponse) {
    let token = support::seat_token(surface, 0);
    let answer = result(
        &support::call_in_lull(surface, &token, left, "list_templates", "{}"),
        "list_templates",
    );
    let typed = json::decode_json(&support::canonical_answer(
        "gp.api.v1.ListTemplatesResponse",
        &answer,
    ))
    .expect("the answer is a ListTemplatesResponse");
    (answer, typed)
}

#[test]
fn list_templates_says_each_refused_kind_by_count_and_never_by_name() {
    let folder = refusing_library("leftout-library");
    let mut surface = support::hosted_with(
        MATCH,
        FogPolicy::fogged(),
        1,
        SEGMENT_MS,
        2,
        Some(folder.clone()),
    );
    let mut left = support::FIRST_LULL_MS;
    for listing in 1..=2_u64 {
        let (answer, typed) = listed(&mut surface, &mut left);
        assert!(typed.next_cursor.is_empty(), "the listing is one page");
        assert_eq!(typed.templates.len(), 5, "the shipped five are listed");
        let refused: Vec<(Reason, u32)> = typed
            .refused
            .iter()
            .map(|entry| (entry.reason(), entry.count))
            .collect();
        // In the enum's order; the same on every listing, never a sum.
        assert_eq!(
            refused,
            vec![
                (Reason::Unreadable, 1),
                (Reason::TooLarge, 1),
                (Reason::NotAPlaybook, 2),
            ],
            "listing {listing}"
        );
        let text = json::write(&answer);
        for named in ["a_folder", "big", "not_text", "not_json"] {
            assert!(!text.contains(named), "{named} is named: {text}");
        }
        assert!(!text.contains(&folder.display().to_string()), "{text}");
        // The wire spells the reason in lower case, as every enum.
        assert!(text.contains("\"not_a_playbook\""), "{text}");
        // The audit's counts still count every listing, each what the
        // listing said, and its lines name no refused file (fix pass 1).
        for (reason, per_listing) in [
            (Refusal::Unreadable, 1_u64),
            (Refusal::TooLarge, 1),
            (Refusal::NotAPlaybook, 2),
        ] {
            assert_eq!(
                surface.audit().library_refused_count(reason),
                per_listing * listing,
                "{reason:?} after listing {listing}"
            );
        }
        let lines = surface.audit().lines();
        assert!(!lines.is_empty(), "the audit logged the calls");
        for line in &lines {
            for named in ["a_folder", "big", "not_text", "not_json"] {
                assert!(!line.contains(named), "the audit names {named}: {line}");
            }
        }
    }
}

/// **`submit_plan` answers the report `verify_plan` does, `left_out` and all**
/// (fix pass 1): a capped playbook's FULL `verify_plan` report and its
/// `submit_plan` report are the same bytes, the field present in both.
#[test]
fn submit_plan_sends_the_same_left_out_as_verify_plan() {
    let mut surface = support::hosted(1, SEGMENT_MS, 2);
    let token = support::seat_token(&mut surface, 0);
    let mut left = support::FIRST_LULL_MS;
    let text = holds(30);
    let params = |depth: Option<&str>| {
        let mut members = vec![(String::from("playbook_jsonc"), Json::String(text.clone()))];
        if let Some(depth) = depth {
            members.push((String::from("depth"), Json::String(depth.to_owned())));
        }
        json::write(&Json::Object(members))
    };
    let verify = result(
        &support::call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "verify_plan",
            &params(Some("full")),
        ),
        "verify_plan",
    );
    let submit = result(
        &support::call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "submit_plan",
            &params(None),
        ),
        "submit_plan",
    );
    let verified = verify.get("report").expect("verify_plan's report");
    let submitted = submit.get("report").expect("submit_plan's report");
    assert!(
        submitted.get("left_out").is_some(),
        "submit_plan's report carries left_out: {submitted:?}"
    );
    assert_eq!(json::write(verified), json::write(submitted));
}

#[test]
fn a_listing_that_refused_nothing_carries_no_field() {
    let mut surface = support::hosted_with(
        MATCH,
        FogPolicy::fogged(),
        1,
        SEGMENT_MS,
        2,
        Some(support::library_folder()),
    );
    let mut left = support::FIRST_LULL_MS;
    let (answer, typed) = listed(&mut surface, &mut left);
    assert!(answer.get("refused").is_none(), "{answer:?}");
    assert!(typed.refused.is_empty());
}
