// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **A burst at the opening of a Lull that follows a recap is not rate limited** — the
//! watch check's flake of decisions-log item 121 (4), reproduced with no engine and no
//! host.
//!
//! On run 36338546091 (#23), the headless watch check's first client failed in round 2's
//! Lull, right after the recap, with "round 2: the carried draft was never opened" and
//! "the gateway refused 1 call(s): `RATE_LIMITED`: over 8 calls in one tick": the editor's
//! call on the **seat** connection was refused (its editor state read `gateway_refused`,
//! one refusal), not the admin connection's.
//!
//! A stand-in that moved its tick to the whole 50 ms steps of the reported elapsed time and
//! never started a phase's clock again could not show this. The real gateway's clock is `crates/gateway/src/surface.rs`'s `sync_time`: the sim's
//! tick, plus `lull_offset`, plus `phase_elapsed`, the phase's reported elapsed time
//! floored to whole ticks and kept as a high-water mark; and `close_phase` folds
//! `phase_elapsed` into `lull_offset` and starts the next phase's clock from zero. So a
//! phase change moves no tick, and neither does a phase's first clock report, which the
//! pacer sends at once with about 0 ms spent. The rig used to refill the seat's budget on
//! every clock answer, that first one included: the calls left from the recap's last
//! refill and the six after the Lull's first report could all land in one gateway tick.
//!
//! The stand-in gateway in `tests/common/mod.rs`, shared with `tests/rate_budget.rs`
//! (register S1-45), models exactly that clock, and counts the seat token's calls against
//! the gateway's own default limits, read out of `crates/gateway/src/limit.rs`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

mod common;

use common::{Gateway, limits};
use pharmakos_client_gdext::editor::{Action, Selector, Target};
use pharmakos_client_gdext::rig::{ADMIN, Phase, Rig, SEAT};
use pharmakos_client_gdext::view::{Entity, EntityKind};

/// One frame of a fast headless client, in wall microseconds: a headless Godot draws as
/// fast as it can, so the seat's calls go out close together.
const FRAME_US: u64 = 4_000;

/// Plays a recap, Continue, and a burst of edits the moment round 2's Lull has opened the
/// carried draft, as the watch check's round 2 does, answering after `latency` frames; Continue is pressed `continue_after` frames into the
/// recap. Returns the gateway and the rig.
fn recap_then_burst(continue_after: usize, latency: usize, seat_first: bool) -> (Gateway, Rig) {
    let mut gateway = Gateway::new("recap");
    let mut rig = Rig::new();
    rig.set_lull_length(0);
    rig.opened(ADMIN);
    rig.opened(SEAT);
    rig.editor_mut().set_seat("seat.0");
    rig.editor_mut().set_entities(&[Entity {
        id: "u_1".to_owned(),
        kind: EntityKind::Unit,
        subtype: "commander".to_owned(),
        owner: "seat.0".to_owned(),
        at: [358, 22, 36],
    }]);

    let mut in_transit: Vec<(usize, usize, String)> = Vec::new();
    let mut now = 0_u64;
    let mut recap_frames = 0_usize;
    let mut started = false;
    let mut ready_asked = false;
    for frame in 0..20_000_usize {
        now += FRAME_US;
        if rig.phase() == Phase::Recap {
            recap_frames += 1;
            if recap_frames == continue_after {
                rig.end_recap();
            }
        }
        if rig.phase() == Phase::Lull && !started && rig.editor().has_text() {
            started = true;
            for index in 0..24 {
                let target = if index % 2 == 0 {
                    Target::Beacon("b_00".to_owned())
                } else {
                    Target::Selector(Selector::Nearest)
                };
                assert!(rig.editor_mut().act(Action::Go, &target));
            }
        }
        // Submit and be ready once every edit has been answered.
        if started && !ready_asked && gateway.count("patch_plan") == 24 && rig.editor().settled() {
            ready_asked = true;
            assert!(rig.editor_mut().submit());
            rig.ready();
        }
        let mut sent = rig.poll(now);
        if seat_first {
            sent.sort_by_key(|outgoing| usize::from(outgoing.link == ADMIN));
        }
        for outgoing in sent {
            let answer = gateway.serve(&outgoing);
            in_transit.push((frame + latency, outgoing.link, answer));
        }
        let (due, later): (Vec<_>, Vec<_>) =
            in_transit.into_iter().partition(|(at, _, _)| *at <= frame);
        in_transit = later;
        for (_, link, answer) in due {
            rig.receive(link, &answer)
                .expect("the rig reads the answer");
        }
        if gateway.phase == "push" {
            break;
        }
    }
    (gateway, rig)
}

#[test]
fn a_burst_as_the_lull_after_a_recap_opens_is_not_rate_limited() {
    let limits = limits();
    // Continue at several points of the recap's report cycle (a report goes out every
    // 250 ms, about 62 frames here), so one of them lands just after a report.
    for continue_after in [2, 20, 64, 70, 90, 125] {
        for latency in [0, 1, 2] {
            for seat_first in [false, true] {
                let (gateway, rig) = recap_then_burst(continue_after, latency, seat_first);
                let context = format!(
                    "Continue {continue_after} frames into the recap, {latency} frame(s) of \
                     latency, seat first: {seat_first}"
                );
                assert_eq!(
                    gateway.phase,
                    "push",
                    "{context}: Ready ended round 2's Lull ({:?}; revision {}, settled {})",
                    gateway.methods,
                    rig.editor().revision(),
                    rig.editor().settled()
                );
                assert_eq!(
                    gateway.rate_limited,
                    0,
                    "{context}: the seat token was refused; busiest tick {} calls against {} \
                     ({:?}; from the Lull's opening, by tick: {:?})",
                    gateway.busiest_tick(),
                    limits.per_tick,
                    gateway.methods,
                    gateway.in_tick_in_lull
                );
                assert_eq!(rig.editor().refusals(), 0, "{context}");
                assert_eq!(
                    gateway.count("get_draft"),
                    1,
                    "{context}: the carried draft was fetched once"
                );
            }
        }
    }
}
