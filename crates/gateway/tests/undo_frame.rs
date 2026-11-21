// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An undo always fits the frame it must travel in (S3's `vfyq2`, fix pass
//! 6, item 2).
//!
//! `patch_plan` hands back an inverse the editor sends back, as the
//! `json_patch` of another `patch_plan` request, beside the patched file. The
//! inverse holds the text it takes out written as JSON, and the request
//! writes it as JSON again, so an undo request can be far larger than the
//! request that made it. Past the gateway's message limit, the gateway
//! would close the seat's connection (1009). So `patch_plan` measures the
//! undo request as the wire carries it, and refuses the forward patch up
//! front when it would not fit, with the refusal for an undo that could not
//! be issued.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_proto::json::Json;

use support::{FIRST_LULL_MS, call_in_lull, hosted_with, quote, seat_token};

/// The review's document: `n` routes of quoted words, about 31 bytes each.
fn routes(n: usize) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| format!("{{\"id\": \"b{i}\", \"n\": {i}}}"))
        .collect();
    format!(
        "{{\n  \"meta\": {{\"title\": \"t\"}},\n  \"routes\": [\n    {}\n  ]\n}}\n",
        items.join(",\n    ")
    )
}

/// The review's case: a 205,827-byte file whose `remove /routes` makes an
/// undo request of about 336 KB.
#[test]
fn a_patch_whose_undo_request_would_not_fit_a_message_is_refused_up_front() {
    let mut surface = hosted_with("m-undo-frame", FogPolicy::fogged(), 2, 3_000, 3, None);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let text = routes(6_500);
    assert!(text.len() > 200_000, "{}", text.len());
    let params = format!(
        r#"{{"playbook_jsonc":{},"json_patch":{}}}"#,
        quote(&text),
        quote(r#"[{"op":"remove","path":"/routes"}]"#)
    );
    let answer = call_in_lull(&mut surface, &token, &mut left, "patch_plan", &params);
    let Some(Json::String(message)) = answer.get("error").and_then(|error| error.get("message"))
    else {
        panic!(
            "refused, not answered: {}",
            pharmakos_proto::json::write(&answer)
        );
    };
    assert!(
        message.contains("could not be issued"),
        "the undo's refusal: {message}"
    );
}
