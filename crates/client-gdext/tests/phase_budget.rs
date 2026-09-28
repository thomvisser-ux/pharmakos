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
//! `tests/rate_budget.rs`'s stand-in moves its tick to the whole 50 ms steps of the
//! reported elapsed time and never starts a phase's clock again, so it cannot show this.
//! The real gateway's clock is `crates/gateway/src/surface.rs`'s `sync_time`: the sim's
//! tick, plus `lull_offset`, plus `phase_elapsed`, the phase's reported elapsed time
//! floored to whole ticks and kept as a high-water mark; and `close_phase` folds
//! `phase_elapsed` into `lull_offset` and starts the next phase's clock from zero. So a
//! phase change moves no tick, and neither does a phase's first clock report, which the
//! pacer sends at once with about 0 ms spent. The rig used to refill the seat's budget on
//! every clock answer, that first one included: the calls left from the recap's last
//! refill and the six after the Lull's first report could all land in one gateway tick.
//!
//! The stand-in below models exactly that clock, and counts the seat token's calls against
//! the gateway's own default limits, read out of `crates/gateway/src/limit.rs` as
//! `tests/rate_budget.rs` reads them.

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
use pharmakos_client_gdext::rig::{ADMIN, Outgoing, Phase, Rig, SEAT};
use pharmakos_client_gdext::view::{Entity, EntityKind};
use pharmakos_proto::json::{self, Json};

/// One frame of a fast headless client, in wall microseconds: a headless Godot draws as
/// fast as it can, so the seat's calls go out close together.
const FRAME_US: u64 = 4_000;

/// The gateway's step: 20 Hz, so 50 game milliseconds a tick.
const MS_PER_TICK: i64 = 50;

/// The gateway's per-token limits, read from its source.
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

/// A stand-in gateway whose clock is `Surface::sync_time`'s, phase by phase.
struct Gateway {
    limits: Limits,
    phase: &'static str,
    round: u32,
    /// Ticks carried from every closed phase (`lull_offset`).
    offset: i64,
    /// This phase's reported elapsed ticks, a high-water mark (`phase_elapsed`).
    phase_elapsed: i64,
    all_ready: bool,
    /// The seat token's calls, by gateway tick.
    in_tick: BTreeMap<i64, u32>,
    /// The seat token's calls, by gateway tick, from the Lull's opening on.
    in_tick_since_lull: BTreeMap<i64, u32>,
    window_start: i64,
    in_window: u32,
    rate_limited: u32,
    methods: BTreeMap<String, u32>,
}

impl Gateway {
    fn new() -> Self {
        Self {
            limits: limits(),
            phase: "recap",
            round: 1,
            offset: 0,
            phase_elapsed: 0,
            all_ready: false,
            in_tick: BTreeMap::new(),
            in_tick_since_lull: BTreeMap::new(),
            window_start: 0,
            in_window: 0,
            rate_limited: 0,
            methods: BTreeMap::new(),
        }
    }

    /// `MatchTime::tick` outside a Push: no sim tick runs here.
    fn tick(&self) -> i64 {
        self.offset + self.phase_elapsed
    }

    /// `Surface::close_phase`: carry what the phase spent, and start the next from zero.
    fn close_phase(&mut self) {
        self.offset += self.phase_elapsed;
        self.phase_elapsed = 0;
    }

    fn busiest_tick(&self) -> u32 {
        self.in_tick.values().copied().max().unwrap_or(0)
    }

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
                r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32000,"message":"over {} calls in one tick","data":{{"code":"RATE_LIMITED"}}}}}}"#,
                self.limits.per_tick
            );
        }
        let result = match method.as_str() {
            "report_host_clock" => {
                if let Some(Json::Number(spent)) = params.get("elapsed_ms") {
                    let spent: i64 = spent.parse().expect("a number");
                    // `to_ticks_floor`, kept as a high-water mark.
                    self.phase_elapsed = self
                        .phase_elapsed
                        .max(spent.checked_div(MS_PER_TICK).unwrap_or(0));
                }
                format!(r#"{{"all_ready":{}}}"#, self.all_ready)
            }
            "end_recap" => {
                self.close_phase();
                self.phase = "lull";
                self.round += 1;
                self.all_ready = false;
                "{}".to_owned()
            }
            "set_ready" => {
                self.all_ready = true;
                "{}".to_owned()
            }
            "end_lull" => {
                self.close_phase();
                self.phase = "push";
                "{}".to_owned()
            }
            "get_view" => r#"{"next_cursor":"c1","complete":true}"#.to_owned(),
            "get_segment_feed" => r#"{"events":[],"next_cursor":"f1"}"#.to_owned(),
            "verify_plan" => r#"{"report":{"qualifies":true,"depth":"quick"}}"#.to_owned(),
            "submit_plan" => {
                r#"{"report":{"qualifies":true,"depth":"full"},"accepted":true}"#.to_owned()
            }
            "patch_plan" => patched(&params),
            "estimate_route" => {
                r#"{"reachable":true,"ms":1000,"legs":[{"to":{"voxel":{"x":1}},"ms":1000}]}"#
                    .to_owned()
            }
            "get_briefing" => r#"{"notes":""}"#.to_owned(),
            // Last round's sealed playbook, carried into this round's Lull.
            "list_drafts" => format!(
                r#"{{"drafts":[{{"draft_id":"carried","label":"carried","round":{}}}]}}"#,
                self.round
            ),
            "get_draft" => json::write(&Json::Object(vec![
                (
                    "playbook_jsonc".to_owned(),
                    Json::String(EXPAND_EAST.to_owned()),
                ),
                ("label".to_owned(), Json::String("carried".to_owned())),
                ("round".to_owned(), Json::Number(self.round.to_string())),
            ])),
            "list_templates" => {
                r#"{"templates":[{"template_id":"t","title":"T","summary":"s"}]}"#.to_owned()
            }
            "render_plan" => r#"{"prose":"Playbook"}"#.to_owned(),
            "get_economy_forecast" => {
                r#"{"treasury_now":200,"supply_kw_now":10,"draw_kw_now":6,"headroom_kw_now":4}"#
                    .to_owned()
            }
            _ => "{}".to_owned(),
        };
        let body = result
            .trim_end()
            .strip_suffix('}')
            .expect("a result is an object");
        let separator = if body.len() > 1 { "," } else { "" };
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{body}{separator}"_status":{{"phase":"{}","round":{}}}}}}}"#,
            self.phase, self.round
        )
    }

    /// `RateLimiter::admit`, for the seat token.
    fn admit(&mut self) -> bool {
        let tick = self.tick();
        if tick - self.window_start >= self.limits.window_ticks {
            self.window_start = tick;
            self.in_window = 0;
        }
        let in_tick = self.in_tick.entry(tick).or_insert(0);
        if *in_tick >= self.limits.per_tick || self.in_window >= self.limits.per_window {
            return false;
        }
        *in_tick += 1;
        self.in_window += 1;
        if self.phase == "lull" {
            *self.in_tick_since_lull.entry(tick).or_insert(0) += 1;
        }
        true
    }
}

/// `patch_plan`'s answer: the text with the new step's label in a trailing comment, as
/// `tests/rate_budget.rs`'s stand-in answers it.
fn patched(params: &Json) -> String {
    let text = match params.get("playbook_jsonc") {
        Some(Json::String(text)) => text.clone(),
        other => panic!("a playbook, not {other:?}"),
    };
    let patch = match params.get("json_patch") {
        Some(Json::String(patch)) => patch.clone(),
        other => panic!("a patch, not {other:?}"),
    };
    let label = patch
        .split("\"label\":")
        .nth(1)
        .and_then(|rest| rest.trim_start().strip_prefix('"'))
        .and_then(|rest| rest.split('"').next())
        .expect("a map action's step carries a label")
        .to_owned();
    json::write(&Json::Object(vec![
        (
            "playbook_jsonc".to_owned(),
            Json::String(format!("{text}\n// \"{label}\"\n")),
        ),
        (
            "inverse_json_patch".to_owned(),
            Json::String("[]".to_owned()),
        ),
    ]))
}

const EXPAND_EAST: &str = include_str!("../../../examples/playbooks/expand_east.jsonc");

/// Plays a recap, Continue, and a burst of edits the moment round 2's Lull has opened the
/// carried draft, as the watch check's round 2 does, answering after `latency` frames; Continue is pressed `continue_after` frames into the
/// recap. Returns the gateway and the rig.
fn recap_then_burst(continue_after: usize, latency: usize, seat_first: bool) -> (Gateway, Rig) {
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
        if started
            && !ready_asked
            && gateway.methods.get("patch_plan") == Some(&24)
            && rig.editor().settled()
        {
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
                    gateway.in_tick_since_lull
                );
                assert_eq!(rig.editor().refusals(), 0, "{context}");
                assert_eq!(
                    gateway.methods.get("get_draft").copied(),
                    Some(1),
                    "{context}: the carried draft was fetched once"
                );
            }
        }
    }
}
