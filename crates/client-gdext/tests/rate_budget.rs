// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **An editing burst is not rate limited under the default limits** — T19's acceptance
//! (`docs/design/skeleton-plan-w6-notes.md` section A4), with no engine and no host.
//!
//! The seat token's budget is the gateway's: `CALLS_PER_TICK` calls per gateway tick and
//! `CALLS_PER_WINDOW` per `WINDOW_TICKS`, and in a Lull the gateway's tick moves only when
//! the admin connection reports the host clock (`skeleton-plan-t16a-notes.md` section B,
//! "T19" (3)). The editor's calls go out on the seat connection beside the vista's polls,
//! scheduled by the watch rig (`src/rig.rs`), so a player who edits as fast as they can
//! click must never meet `RATE_LIMITED`.
//!
//! This file drives the real rig against a stand-in gateway that does exactly two things
//! the real one does: it moves its tick from the reported host clock (whole 50 ms steps of
//! the reported elapsed time), and it counts the seat token's calls per tick and per window
//! against the gateway's own default limits — **read out of
//! `crates/gateway/src/limit.rs`**, so the day those numbers move this test is about the
//! new ones. Everything else it answers is unremarkable. The live half of the same claim is
//! `godot/scripts/watch_check.gd`, which edits, verifies and submits against a real
//! `gamectl host` and fails on any refused call.
//!
//! `client-gdext` may not depend on the gateway (it would reach the sim, which
//! `tests/no_sim.rs` forbids), which is why the limits are read as text.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use pharmakos_client_gdext::editor::{Action, Selector, Target};
use pharmakos_client_gdext::rig::{ADMIN, Outgoing, Phase, Rig, SEAT, SEAT_CALLS_PER_REFILL};
use pharmakos_client_gdext::view::{Entity, EntityKind};
use pharmakos_proto::json::{self, Json};

/// One frame of a 60 Hz client, in wall microseconds.
const FRAME_US: u64 = 16_667;

/// The gateway's step: 20 Hz, so 50 game milliseconds a tick.
const MS_PER_TICK: i64 = 50;

/// The gateway's default limits, read from its source.
#[derive(Clone, Copy, Debug)]
struct Limits {
    per_tick: u32,
    per_window: u32,
    window_ticks: i64,
}

fn limits() -> Limits {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("gateway")
        .join("src")
        .join("limit.rs");
    let text = std::fs::read_to_string(&path).expect("crates/gateway/src/limit.rs");
    let constant = |name: &str| -> u32 {
        let line = text
            .lines()
            .find(|line| line.starts_with(&format!("pub const {name}: u32 = ")))
            .unwrap_or_else(|| panic!("{name} is not a `pub const {name}: u32` in limit.rs"));
        line.trim_end_matches(';')
            .rsplit(' ')
            .next()
            .and_then(|value| value.replace('_', "").parse().ok())
            .unwrap_or_else(|| panic!("{name} is not a number: {line}"))
    };
    Limits {
        per_tick: constant("CALLS_PER_TICK"),
        per_window: constant("CALLS_PER_WINDOW"),
        window_ticks: i64::from(constant("WINDOW_TICKS")),
    }
}

/// A stand-in gateway: the tick, the seat token's counters, and plausible answers.
struct Gateway {
    limits: Limits,
    phase: &'static str,
    tick: i64,
    all_ready: bool,
    in_tick: BTreeMap<i64, u32>,
    window_start: i64,
    in_window: u32,
    rate_limited: u32,
    methods: BTreeMap<String, u32>,
}

impl Gateway {
    fn new() -> Self {
        Self {
            limits: limits(),
            phase: "lull",
            tick: 0,
            all_ready: false,
            in_tick: BTreeMap::new(),
            window_start: 0,
            in_window: 0,
            rate_limited: 0,
            methods: BTreeMap::new(),
        }
    }

    /// The most seat calls counted in any one tick.
    fn busiest_tick(&self) -> u32 {
        self.in_tick.values().copied().max().unwrap_or(0)
    }

    /// Processes one request and returns the answer's text.
    fn serve(&mut self, frame: &Outgoing) -> String {
        let request = json::read(&frame.text).expect("a request");
        let id = match request.get("id") {
            Some(Json::Number(id)) => id.clone(),
            other => panic!("a request id, not {other:?}"),
        };
        let method = match request.get("method") {
            Some(Json::String(method)) => method.clone(),
            other => panic!("a method, not {other:?}"),
        };
        let params = request.get("params").cloned().unwrap_or(Json::Null);
        *self.methods.entry(method.clone()).or_insert(0) += 1;

        if frame.link == SEAT && !self.admit() {
            self.rate_limited += 1;
            return format!(
                r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32000,"message":"slow down","data":{{"code":"RATE_LIMITED"}}}}}}"#
            );
        }
        let result = match method.as_str() {
            "report_host_clock" => {
                if let Some(Json::Number(spent)) = params.get("elapsed_ms") {
                    let spent: i64 = spent.parse().expect("a number");
                    self.tick = self.tick.max(spent.checked_div(MS_PER_TICK).unwrap_or(0));
                }
                format!(r#"{{"all_ready":{}}}"#, self.all_ready)
            }
            "set_ready" => {
                self.all_ready = true;
                "{}".to_owned()
            }
            "end_lull" => {
                self.phase = "push";
                "{}".to_owned()
            }
            "get_view" => r#"{"next_cursor":"c1","complete":true}"#.to_owned(),
            "get_segment_feed" => r#"{"events":[],"next_cursor":"f1"}"#.to_owned(),
            "verify_plan" => r#"{"report":{"qualifies":true,"depth":"quick"}}"#.to_owned(),
            "submit_plan" => {
                r#"{"report":{"qualifies":true,"depth":"full"},"accepted":true}"#.to_owned()
            }
            "patch_plan" => {
                let text = match params.get("playbook_jsonc") {
                    Some(Json::String(text)) => text.clone(),
                    other => panic!("a playbook, not {other:?}"),
                };
                // The stand-in applies nothing; a changed text is all the editor needs to
                // see an edit land.
                let patched = format!("{text} ");
                json::write(&Json::Object(vec![
                    ("playbook_jsonc".to_owned(), Json::String(patched)),
                    (
                        "inverse_json_patch".to_owned(),
                        Json::String("[]".to_owned()),
                    ),
                ]))
            }
            "estimate_route" => {
                r#"{"reachable":true,"ms":1000,"legs":[{"to":{"voxel":{"x":1}},"ms":1000}]}"#
                    .to_owned()
            }
            "get_briefing" => r#"{"notes":""}"#.to_owned(),
            "list_drafts" => r#"{"drafts":[]}"#.to_owned(),
            "save_notes" => r#"{"characters":5}"#.to_owned(),
            _ => "{}".to_owned(),
        };
        let body = result
            .trim_end()
            .strip_suffix('}')
            .expect("a result is an object");
        let separator = if body.len() > 1 { "," } else { "" };
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{body}{separator}"_status":{{"phase":"{}","round":1}}}}}}"#,
            self.phase
        )
    }

    /// The gateway's fixed-window counters, for the seat token.
    fn admit(&mut self) -> bool {
        if self.tick - self.window_start >= self.limits.window_ticks {
            self.window_start = self.tick;
            self.in_window = 0;
        }
        let in_tick = self.in_tick.entry(self.tick).or_insert(0);
        if *in_tick >= self.limits.per_tick || self.in_window >= self.limits.per_window {
            return false;
        }
        *in_tick += 1;
        self.in_window += 1;
        true
    }
}

const EXPAND_EAST: &str = include_str!("../../../examples/playbooks/expand_east.jsonc");

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
    let mut gateway = Gateway::new();
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
            assert!(rig.editor_mut().submit());
            rig.editor_mut().save_notes("burst");
        }
        if started && !ready_asked && rig.editor().revision() > u64::try_from(edits).expect("small")
        {
            ready_asked = true;
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
