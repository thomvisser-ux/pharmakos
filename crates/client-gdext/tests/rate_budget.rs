// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **An editing burst is not rate limited under the default limits** — T19's acceptance
//! (`docs/design/skeleton-plan-w6-notes.md` section A4), with no engine and no host. Pull
//! request 2 adds its calls to the burst: the template list, the wizard's
//! `instantiate_template` (opened, one page typed, then used), `render_plan` for every new
//! text, `get_draft` for the carried draft, and the meter's `get_economy_forecast`.
//!
//! The seat token's budget is the gateway's: `CALLS_PER_TICK` calls per gateway tick and
//! `CALLS_PER_WINDOW` per `WINDOW_TICKS`, and in a Lull the gateway's tick moves only when
//! the admin connection reports the host clock (`skeleton-plan-t16a-notes.md` section B,
//! "T19" (3)). The editor's calls go out on the seat connection beside the vista's polls,
//! scheduled by the watch rig (`src/rig.rs`), so a player who edits as fast as they can
//! click must never meet `RATE_LIMITED`.
//!
//! This file drives the real rig against the stand-in gateway in `tests/common/mod.rs`,
//! shared with `tests/phase_budget.rs` (register S1-45): it moves its tick from the reported
//! host clock, and it counts the seat token's calls per tick and per window against the
//! gateway's own default limits — **read out of `crates/gateway/src/limit.rs`**, so the day
//! those numbers move this test is about the new ones. Everything else it answers is
//! unremarkable. The live half of the same claim is `godot/scripts/watch_check.gd`, which
//! edits, verifies and submits against a real `gamectl host` and fails on any refused call.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

mod common;

use common::{EXPAND_EAST, Gateway, limits};
use pharmakos_client_gdext::editor::{Action, Selector, Target};
use pharmakos_client_gdext::rig::{ADMIN, Phase, Rig, SEAT, SEAT_CALLS_PER_REFILL};
use pharmakos_client_gdext::view::{Entity, EntityKind};

/// One frame of a 60 Hz client, in wall microseconds.
const FRAME_US: u64 = 16_667;

/// How a frame's requests reach the gateway: in the order the rig wrote them (admin
/// first), or seat first, which is the order that lets a seat call land after a clock
/// report it was sent before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    AsSent,
    SeatFirst,
}

/// Runs a burst of `edits` map actions, a submission and Ready through the rig against the
/// stand-in, answering after `latency` frames. Returns the gateway and the rig.
fn burst(edits: usize, order: Order, latency: usize) -> (Gateway, Rig) {
    let mut gateway = Gateway::new("lull");
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
    let mut loading = false;
    let mut started = false;
    let mut ready_asked = false;
    let mut typed = false;
    let mut used = false;
    for frame in 0..6_000_usize {
        now += FRAME_US;
        if !loading && rig.phase() == Phase::Lull {
            loading = true;
            assert!(rig.editor_mut().load(EXPAND_EAST.as_bytes()));
        }
        if loading && !started && rig.editor().has_text() {
            // The burst: every map action at once, the moment there is a file to act on.
            started = true;
            for index in 0..edits {
                let target = if index % 2 == 0 {
                    Target::Beacon("b_00".to_owned())
                } else {
                    Target::Selector(Selector::Nearest)
                };
                assert!(rig.editor_mut().act(Action::Go, &target));
            }
            rig.editor_mut().wizard_open("t");
        }
        // The wizard, as a player uses it: one page typed once its answer is on screen, then
        // Use once the answer to that is.
        let wizard_current = rig
            .editor()
            .wizard()
            .is_some_and(pharmakos_client_gdext::wizard::Wizard::current);
        if started && wizard_current && !typed {
            typed = rig.editor_mut().wizard_set("/p", "1");
        } else if started && wizard_current && typed && !used {
            used = rig.editor_mut().wizard_use();
        }
        if started
            && used
            && !ready_asked
            && rig.editor().revision() > u64::try_from(edits).expect("small")
        {
            // Submit what the wizard and the burst left, save a note, and be ready: Ready
            // waits behind both.
            ready_asked = true;
            assert!(rig.editor_mut().submit());
            rig.editor_mut().save_notes("burst");
            rig.ready();
        }
        let mut sent = rig.poll(now);
        if order == Order::SeatFirst {
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
fn an_editing_burst_is_not_rate_limited_under_the_default_limits() {
    let limits = limits();
    for order in [Order::AsSent, Order::SeatFirst] {
        for latency in [0, 1, 3] {
            let edits = 24;
            let (gateway, rig) = burst(edits, order, latency);
            let context = format!("{order:?}, {latency} frame(s) of latency");
            assert_eq!(
                gateway.rate_limited,
                0,
                "{context}: the seat token was refused; busiest tick {} calls against {} \
                 ({:?})",
                gateway.busiest_tick(),
                limits.per_tick,
                gateway.methods
            );
            assert!(
                gateway.busiest_tick() <= limits.per_tick,
                "{context}: {} calls in one tick",
                gateway.busiest_tick()
            );
            // With answers in the same frame the rig could spend far more than a tick's
            // budget between two clock reports, so the budget, not the round trip, is what
            // held it back; with slower answers the round trip holds it back first.
            assert!(
                latency > 0 || gateway.busiest_tick() >= SEAT_CALLS_PER_REFILL,
                "{context}: the burst never filled a tick's budget, so the schedule was never \
                 tested ({:?})",
                gateway.methods
            );
            assert_eq!(
                gateway.methods.get("patch_plan").copied(),
                Some(u32::try_from(edits).expect("small")),
                "{context}: every edit went out"
            );
            assert_eq!(
                gateway.methods.get("submit_plan").copied(),
                Some(1),
                "{context}"
            );
            assert_eq!(
                gateway.methods.get("set_ready").copied(),
                Some(1),
                "{context}"
            );
            assert_eq!(
                gateway.phase, "push",
                "{context}: Ready ended the Lull ({:?})",
                gateway.methods
            );
            assert_eq!(rig.editor().refusals(), 0, "{context}");
            assert!(rig.editor().settled(), "{context}: nothing left owed");
            assert!(
                gateway.methods.get("verify_plan").copied().unwrap_or(0) >= 2,
                "{context}: the load check and at least one QUICK of the edited text"
            );
            // Pull request 2's calls share the same budget: the template list, the wizard's
            // two instantiations, the rule list, the carried draft and the meter.
            let count = |method: &str| gateway.count(method);
            assert_eq!(count("list_templates"), 1, "{context}");
            assert!(
                count("instantiate_template") >= 2,
                "{context}: {:?}",
                gateway.methods
            );
            assert!(count("render_plan") >= 1, "{context}");
            assert_eq!(count("get_draft"), 1, "{context}: {:?}", gateway.methods);
            assert!(count("get_economy_forecast") >= 1, "{context}");
        }
    }
}

#[test]
fn the_rigs_margin_is_under_the_gateways_limit() {
    // One call can straddle a clock move (it was on its way when the clock was reported),
    // so the rig's refill plus one must fit in the gateway's per-tick limit.
    assert!(
        SEAT_CALLS_PER_REFILL < limits().per_tick,
        "SEAT_CALLS_PER_REFILL is {SEAT_CALLS_PER_REFILL} and the gateway admits {} a tick",
        limits().per_tick
    );
}
