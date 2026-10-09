// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **The one stand-in gateway** that `tests/rate_budget.rs` and `tests/phase_budget.rs`
//! drive the real watch rig against (register S1-45; decisions-log item 124 (5) (n)). Each
//! file used to carry its own copy; one stand-in now serves both, so the day a limit or an
//! answer moves, it moves for both.
//!
//! It does exactly two things the real gateway does, and answers everything else
//! unremarkably:
//!
//! * **its clock is `crates/gateway/src/surface.rs`'s `sync_time`**, phase by phase: outside
//!   a Push the gateway's tick is the ticks carried from every closed phase (`lull_offset`)
//!   plus this phase's reported elapsed time floored to whole 50 ms ticks and kept as a
//!   high-water mark (`phase_elapsed`); `close_phase` folds the second into the first and
//!   starts the next phase from zero. Within one phase this is the elapsed time's whole
//!   ticks, which is all `rate_budget.rs` needs; across a recap and the Lull after it, it
//!   is the clock `phase_budget.rs` exists to model;
//! * **it counts the seat token's calls** per tick and per window against the gateway's
//!   own default limits, read out of `crates/gateway/src/limit.rs` as text, because
//!   `client-gdext` may not depend on the gateway (it would reach the sim, which
//!   `tests/no_sim.rs` forbids).
//!
//! Its `patch_plan` does not apply the patch: it records the new step's label in a trailing
//! comment, which is enough for the editor to see an edit land and for the next click's
//! label to be chosen against it, and it fails the test on a label the text already quotes,
//! which the verifier would call E0105 (a name used twice). Its meter answer is the
//! skeleton's settlement golden (`tests/golden/economy/expected.settlement.txt`: supply
//! 10 kW, draw 4 kW), the draw a live beacon nets out (decisions-log item 114).

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

use pharmakos_client_gdext::rig::{Outgoing, SEAT};
use pharmakos_proto::json::{self, Json};

/// The gateway's step: 20 Hz, so 50 game milliseconds a tick.
const MS_PER_TICK: i64 = 50;

/// The playbook the stand-in carries into a Lull and instantiates for the wizard.
pub(crate) const EXPAND_EAST: &str =
    include_str!("../../../../examples/playbooks/expand_east.jsonc");

/// The gateway's default limits for a seat token, read from its source.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// `CALLS_PER_TICK`.
    pub(crate) per_tick: u32,
    /// `CALLS_PER_WINDOW`.
    pub(crate) per_window: u32,
    /// `WINDOW_TICKS`.
    pub(crate) window_ticks: i64,
}

/// `crates/gateway/src/limit.rs`'s three default limits.
pub(crate) fn limits() -> Limits {
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

/// The stand-in gateway: the phase clock, the seat token's counters, and plausible answers.
pub(crate) struct Gateway {
    /// The limits it counts against.
    pub(crate) limits: Limits,
    /// The phase, as the footer spells it.
    pub(crate) phase: &'static str,
    /// The round.
    pub(crate) round: u32,
    /// Ticks carried from every closed phase (`lull_offset`).
    offset: i64,
    /// This phase's reported elapsed ticks, a high-water mark (`phase_elapsed`).
    phase_elapsed: i64,
    /// The countdown last reported this phase (`lull_remaining_ms`); `None` until a report
    /// carries one, while the footer says `untimed` (S1-11).
    countdown: Option<i64>,
    all_ready: bool,
    /// The seat token's calls, by gateway tick.
    in_tick: BTreeMap<i64, u32>,
    /// The seat token's calls, by gateway tick, in a Lull.
    pub(crate) in_tick_in_lull: BTreeMap<i64, u32>,
    window_start: i64,
    in_window: u32,
    /// How many seat calls were refused `RATE_LIMITED`.
    pub(crate) rate_limited: u32,
    /// Every call, by method.
    pub(crate) methods: BTreeMap<String, u32>,
}

impl Gateway {
    /// A stand-in in `phase` (`"lull"` or `"recap"`) of round 1, with no time spent.
    pub(crate) fn new(phase: &'static str) -> Self {
        Self {
            limits: limits(),
            phase,
            round: 1,
            offset: 0,
            phase_elapsed: 0,
            countdown: None,
            all_ready: false,
            in_tick: BTreeMap::new(),
            in_tick_in_lull: BTreeMap::new(),
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
        self.countdown = None;
    }

    /// The most seat calls counted in any one tick.
    pub(crate) fn busiest_tick(&self) -> u32 {
        self.in_tick.values().copied().max().unwrap_or(0)
    }

    /// How many calls of `method` it served.
    pub(crate) fn count(&self, method: &str) -> u32 {
        self.methods.get(method).copied().unwrap_or(0)
    }

    /// Processes one request and returns the answer's text.
    pub(crate) fn serve(&mut self, frame: &Outgoing) -> String {
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
            "report_host_clock" => self.report_host_clock(&params),
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
            "save_notes" => r#"{"characters":5}"#.to_owned(),
            "list_templates" => {
                r#"{"templates":[{"template_id":"t","title":"T","summary":"s"}]}"#.to_owned()
            }
            "instantiate_template" => json::write(&Json::Object(vec![
                (
                    "playbook_jsonc".to_owned(),
                    Json::String(EXPAND_EAST.to_owned()),
                ),
                (
                    "parameters".to_owned(),
                    json::read(r#"[{"pointer":"/p","label":"P","value":"0","suggested":true}]"#)
                        .expect("json"),
                ),
                ("why".to_owned(), Json::String("w".to_owned())),
            ])),
            "render_plan" => r#"{"prose":"Playbook"}"#.to_owned(),
            "get_economy_forecast" => {
                r#"{"treasury_now":200,"supply_kw_now":10,"draw_kw_now":4,"headroom_kw_now":6}"#
                    .to_owned()
            }
            _ => "{}".to_owned(),
        };
        let body = result
            .trim_end()
            .strip_suffix('}')
            .expect("a result is an object");
        let separator = if body.len() > 1 { "," } else { "" };
        let footer = self.footer();
        format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{body}{separator}"_status":{footer}}}}}"#)
    }

    /// `report_host_clock`: the phase's reported time, floored to ticks and kept as a
    /// high-water mark (`to_ticks_floor`), and the countdown when the report carries one.
    fn report_host_clock(&mut self, params: &Json) -> String {
        if let Some(Json::Number(spent)) = params.get("elapsed_ms") {
            let spent: i64 = spent.parse().expect("a number");
            self.phase_elapsed = self
                .phase_elapsed
                .max(spent.checked_div(MS_PER_TICK).unwrap_or(0));
        }
        if let Some(Json::Number(left)) = params.get("remaining_ms") {
            self.countdown = Some(left.parse().expect("a number"));
        }
        format!(r#"{{"all_ready":{}}}"#, self.all_ready)
    }

    /// The `_status` footer: outside a Push, `sync_time`'s, with the countdown last reported,
    /// or `untimed` when none has been this phase (S1-11).
    fn footer(&self) -> String {
        let clock = match (self.phase, self.countdown) {
            ("push", _) => String::new(),
            (_, Some(left)) => format!(r#","phase_remaining_ms":{left}"#),
            (_, None) => r#","untimed":true"#.to_owned(),
        };
        format!(
            r#"{{"phase":"{}","round":{}{clock}}}"#,
            self.phase, self.round
        )
    }

    /// `RateLimiter::admit`, for the seat token: the gateway's fixed-window counters.
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
            *self.in_tick_in_lull.entry(tick).or_insert(0) += 1;
        }
        true
    }
}

/// `patch_plan`'s answer: the text with the new step's label in a trailing comment. A label
/// the text already quotes fails the test (E0105): every click in a burst must get its own.
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
    assert!(
        !text.contains(&format!("\"{label}\"")),
        "two edits carry the label {label}; the verifier would say E0105"
    );
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
