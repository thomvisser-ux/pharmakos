// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Probation preset, headless (S3's plan, task `gwp`; decisions 4, 5 and
//! 13; E5, E6, E7, E18 and E25).
//!
//! * the config line carries the preset, and the lobby's line and this
//!   gateway's reader agree (E6);
//! * a Probation match's Lulls are untimed, and a reported countdown is
//!   refused naming the preset (E7);
//! * each memo fires once, on the seat's own feed and its recap, survives a
//!   resume without repeating, and never fires in a `custom` match (E5);
//! * the template list marks the advisor's start in the first Lull (E25);
//! * a seat's token is told no seed (E18);
//! * the safe playbook's filing line says what ended the Lull (item 143 (4)).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::advice::{Advice, StartHere, Suggestion};
use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, Settings};
use pharmakos_gateway::probation::{Memo, Preset};
use pharmakos_gateway::save::{Continuation, Save, SavedMatch, Stamp};
use pharmakos_gateway::serve::{Config, ConfigLine};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Token;
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::fixed::Sq;
use pharmakos_sim::runner::MatchSettings;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::WorldConfig;

use support::{
    SEED, admin_token, array_of, call, code, library_folder, quote, result, rules, rules_json,
    seat_token, spectator_token, text_of, walk_east, workspace_root,
};

/// Both seats of a two-seat match.
const SEATS: [SeatId; 2] = [SeatId::new(0), SeatId::new(1)];

/// A two-seat match on the golden seed, in its opening Lull, under `preset`
/// with `human` as the person's seat, and **no countdown reported**: what the
/// host loop builds from a config line ([`pharmakos_gateway::serve`]).
fn hosted_under(
    match_id: &str,
    preset: Preset,
    human: Option<u8>,
    segment_ms: i32,
    round_limit: u32,
) -> Surface {
    let mut surface = Surface::new(match_id, SEED, rules(), FogPolicy::fogged(), &SEATS)
        .expect("a match id")
        .with_preset(preset, human.map(SeatId::new));
    let host = Host::open(
        &WorldConfig {
            match_seed: SEED,
            seats: 2,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![segment_ms],
                round_limit,
            },
        },
        Some(library_folder()),
    )
    .expect("a match");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    surface
}

/// Report `elapsed` of host time spent in the phase, with no countdown, as a
/// Probation client does: it moves the gateway's tick and every token's
/// budget.
fn spend(surface: &mut Surface, admin: &Token, elapsed: &mut i32) {
    *elapsed = elapsed.saturating_add(1_000);
    let answer = call(
        surface,
        admin,
        "report_host_clock",
        &format!("{{\"elapsed_ms\":{elapsed}}}"),
    );
    answered(&answer, "report_host_clock");
}

/// Submit `playbook` for `seat` through `submit_plan`, spending host time
/// first so the seat's rate budget is never the reason for a refusal.
fn submit(surface: &mut Surface, admin: &Token, elapsed: &mut i32, seat: u8, playbook: &str) {
    spend(surface, admin, elapsed);
    let token = seat_token(surface, seat);
    let answer = result(
        &call(
            surface,
            &token,
            "submit_plan",
            &format!("{{\"playbook_jsonc\":{}}}", quote(playbook)),
        ),
        "submit_plan",
    );
    assert_eq!(
        answer.get("accepted"),
        Some(&Json::Bool(true)),
        "seat {seat}'s playbook: {answer:?}"
    );
}

/// A call that must be answered, its result read and dropped.
fn answered(response: &Json, what: &str) {
    let answer = result(response, what);
    assert!(matches!(answer, Json::Object(_)), "{what}: {answer:?}");
}

/// A committed file's text.
fn committed(path: &str) -> String {
    std::fs::read_to_string(workspace_root().join(path))
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Begin the Push and play it to its end, leaving the match in its recap.
fn play_push(surface: &mut Surface) {
    assert!(surface.begin_push().expect("the Push begins"));
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
}

/// Close the recap and open the next Lull.
fn next_lull(surface: &mut Surface) {
    assert!(surface.end_recap().expect("the recap ends"));
    assert_eq!(surface.time().phase, Phase::Lull, "the match goes on");
    surface.open_lull().expect("the next Lull");
}

/// Every line of the segment's feed `seat` is shown, through
/// `get_segment_feed`, as `(kind, text, memo)`.
fn feed_of(surface: &mut Surface, seat: u8) -> Vec<(String, String, Option<Json>)> {
    let token = seat_token(surface, seat);
    let mut cursor = String::new();
    let mut lines = Vec::new();
    loop {
        let answer = result(
            &call(
                surface,
                &token,
                "get_segment_feed",
                &format!("{{\"cursor\":{}}}", quote(&cursor)),
            ),
            "get_segment_feed",
        );
        let events = array_of(&answer, "events");
        if events.is_empty() {
            return lines;
        }
        for event in events {
            lines.push((
                text_of(&event, "kind"),
                text_of(&event, "text"),
                event.get("ledger_memo").cloned(),
            ));
        }
        cursor = text_of(&answer, "next_cursor");
    }
}

/// The memos on `seat`'s lines of the segment's feed, by id.
fn memos_on_the_feed(surface: &mut Surface, seat: u8) -> Vec<String> {
    feed_of(surface, seat)
        .into_iter()
        .filter(|(kind, ..)| kind == "ledger_memo")
        .map(|(_, text, memo)| {
            let memo = memo.expect("a memo's line carries the memo");
            assert_eq!(text_of(&memo, "text"), text, "the same text as the line");
            text_of(&memo, "id")
        })
        .collect()
}

/// The memo ids on `seat`'s recap.
fn memos_on_the_recap(surface: &mut Surface, seat: u8) -> Vec<String> {
    let token = seat_token(surface, seat);
    let recap = result(&call(surface, &token, "get_recap", "{}"), "get_recap");
    match recap.get("ledger_memos") {
        None => Vec::new(),
        Some(Json::Array(memos)) => memos.iter().map(|memo| text_of(memo, "id")).collect(),
        Some(other) => panic!("`ledger_memos` is a list: {other:?}"),
    }
}

/// How many times the sim reported `kind` for `seat` on this segment's bus,
/// unfiltered.
fn fired(surface: &Surface, kind: &str, seat: u8) -> usize {
    use pharmakos_gateway::fog::Audience;
    surface
        .feed()
        .events()
        .iter()
        .filter(|event| event.kind.name() == kind)
        .filter(|event| match &event.audience {
            Audience::World { owner, .. } => *owner == Some(SeatId::new(seat)),
            Audience::Private(owner) => *owner == SeatId::new(seat),
            Audience::Public => false,
        })
        .count()
}

// ---------------------------------------------------------------------------
// E6: the preset on the config line
// ---------------------------------------------------------------------------

fn config(preset: Preset) -> Config {
    Config {
        match_id: String::from("m-probation"),
        seed: 20,
        seats: 2,
        human_seat: Some(0),
        segment_lengths_ms: Vec::new(),
        round_limit: 3,
        preset,
    }
}

/// The line carries the preset as its seventh field and reads back; a line
/// without one -- six fields, as a lobby remembered before S3, or six and
/// `resume` -- is refused with a typed error naming the field, never given a
/// preset by guess; and a preset that is not one is refused.
#[test]
fn the_config_line_carries_the_preset_and_a_line_without_one_is_refused_naming_it() {
    for preset in [Preset::Probation, Preset::Custom] {
        for resume in [false, true] {
            let line = ConfigLine {
                config: config(preset),
                resume,
            };
            let text = line.render();
            assert_eq!(
                text.trim_end().split('\t').count(),
                if resume { 8 } else { 7 },
                "{text:?}"
            );
            assert_eq!(ConfigLine::parse(&text).expect("its own line"), line);
        }
    }
    for old in [
        "m-old\t0x0000000000000014\t2\t0\t-\t3\n",
        "m-old\t0x0000000000000014\t2\t0\t-\t3\tresume\n",
    ] {
        let error = ConfigLine::parse(old).expect_err("no preset");
        assert_eq!(error.name(), "INVALID_ARGUMENT", "{old:?}");
        assert!(
            error.message().contains("`preset`") && error.message().contains("start a new match"),
            "{}",
            error.message()
        );
    }
    let error = ConfigLine::parse("m-x\t20\t2\t0\t-\t3\tcasual\n").expect_err("not a preset");
    assert!(error.message().contains("probation"), "{}", error.message());
    let error = ConfigLine::parse("m-x\t20\t2\t0\t-\t3\tcustom\tresume\textra\n")
        .expect_err("a ninth field");
    assert!(error.message().contains("eight"), "{}", error.message());
}

/// **The lobby's line and the gateway's reader agree** (E6's same-PR rule):
/// the line `host_link.gd`'s `config_line` writes, built here from that
/// file's own format string and constants, is a line this gateway reads, on
/// the Probation seed with the lobby's preset; and its resume line too.
#[test]
fn the_lobbys_line_is_one_this_gateway_reads() {
    let script = std::fs::read_to_string(
        workspace_root()
            .join("godot")
            .join("scripts")
            .join("host_link.gd"),
    )
    .expect("host_link.gd");
    let constant = |name: &str| -> String {
        let prefix = format!("const {name} := ");
        let found: Vec<&str> = script
            .lines()
            .filter_map(|line| line.strip_prefix(prefix.as_str()))
            .collect();
        let [value] = found.as_slice() else {
            panic!("host_link.gd defines {name} once: {found:?}");
        };
        value.trim_matches('"').to_owned()
    };
    assert!(
        script.contains(
            "return \"%s\\t%s\\t%d\\t%d\\t%s\\t%s\\t%s\\n\" % [match_id, seed, seats, \
             HUMAN_SEAT, ladder, round_limit(), preset]"
        ),
        "the format this test fills"
    );
    let line = format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        "local-1-1",
        constant("MATCH_SEED"),
        constant("MATCH_SEATS"),
        constant("HUMAN_SEAT"),
        constant("LADDER"),
        constant("ROUND_LIMIT"),
        constant("PRESET"),
    );
    let read = ConfigLine::parse(&line).expect("the lobby's line");
    assert!(!read.resume);
    let seed = constant("MATCH_SEED");
    let digits = seed.strip_prefix("0x").expect("a `0x` seed");
    assert_eq!(
        read.config.seed,
        u64::from_str_radix(digits, 16).expect("hex"),
        "the lobby's MATCH_SEED, Probation's (decision 13)"
    );
    assert_eq!(read.config.human_seat, Some(0));
    assert_eq!(read.config.preset.name(), constant("PRESET"));
    let resumed = format!("{}\tresume\n", line.trim_end());
    assert!(ConfigLine::parse(&resumed).expect("its resume line").resume);
}

// ---------------------------------------------------------------------------
// E7: untimed Lulls
// ---------------------------------------------------------------------------

/// A Probation match's Lull is untimed: the footer says so, a report of the
/// time spent is taken, and a report that puts a countdown on the Lull is
/// refused with `INVALID_ARGUMENT` naming the preset -- never clamped or
/// ignored. A `custom` match takes the same countdown.
#[test]
fn a_countdown_reported_in_a_probation_match_is_refused_naming_the_preset() {
    let mut probation = hosted_under("m-untimed", Preset::Probation, Some(0), 1_000, 3);
    let admin = admin_token(&mut probation);
    let spent = call(
        &mut probation,
        &admin,
        "report_host_clock",
        "{\"elapsed_ms\":1000}",
    );
    answered(&spent, "a report of the time spent");
    assert!(probation.time().untimed, "the Lull carries no countdown");
    for remaining in [0, 120_000] {
        let refused = call(
            &mut probation,
            &admin,
            "report_host_clock",
            &format!("{{\"elapsed_ms\":2000,\"remaining_ms\":{remaining}}}"),
        );
        assert_eq!(code(&refused), "INVALID_ARGUMENT", "{refused:?}");
        let message = refused
            .get("error")
            .and_then(|error| error.get("message"))
            .map(|message| format!("{message:?}"))
            .unwrap_or_default();
        assert!(message.contains("probation"), "{message}");
        assert!(
            probation.time().untimed,
            "nothing of the refused report was kept"
        );
    }

    let mut custom = hosted_under("m-timed", Preset::Custom, Some(0), 1_000, 3);
    let admin = admin_token(&mut custom);
    let timed = call(
        &mut custom,
        &admin,
        "report_host_clock",
        "{\"elapsed_ms\":1000,\"remaining_ms\":120000}",
    );
    answered(&timed, "a custom match's countdown");
    assert!(!custom.time().untimed);
}

/// The `_status` footer `seat` reads off `get_status`, decoded as the
/// schema's `gp.api.v1.Status` once its lower-case phase is translated
/// (decisions-log item 80), so the wire's spelling of the new field is held
/// to the schema's.
fn footer_of(surface: &mut Surface, seat: u8) -> (Json, pharmakos_proto::gp::api::v1::Status) {
    let token = seat_token(surface, seat);
    let answer = result(&call(surface, &token, "get_status", "{}"), "get_status");
    let footer = answer
        .get("_status")
        .cloned()
        .expect("every result has one");
    let Json::Object(entries) = &footer else {
        panic!("the footer is an object: {footer:?}");
    };
    let canonical = Json::Object(
        entries
            .iter()
            .map(|(key, value)| match (key.as_str(), value) {
                ("phase", Json::String(wire)) => (
                    key.clone(),
                    Json::String(
                        pharmakos_proto::scope::from_wire_name("gp.api.v1.Status.Phase", wire)
                            .expect("a phase the schema names"),
                    ),
                ),
                _ => (key.clone(), value.clone()),
            })
            .collect(),
    );
    let decoded = pharmakos_proto::json::decode_json(&canonical)
        .unwrap_or_else(|error| panic!("not a gp.api.v1.Status: {error}: {footer:?}"));
    (footer, decoded)
}

/// E7, as the client reads it (S3's `gwu`): a Probation Lull's footer says it
/// is untimed **by rule**, `untimed_by_rule` beside `untimed`, before and
/// after a report of the time spent, so a client starts no countdown and
/// reports no `remaining_ms` in it. A Probation Push and recap carry no
/// `untimed_by_rule`, and neither does a `custom` Lull, whether fresh (which
/// is `untimed` until its host reports a countdown) or timed.
#[test]
fn a_probation_lull_is_untimed_by_rule_and_a_custom_lull_never_is() {
    let mut probation = hosted_under("m-by-rule", Preset::Probation, Some(0), 1_000, 3);
    let admin = admin_token(&mut probation);
    let mut elapsed = 0;
    for when in ["fresh", "after a report of the time spent"] {
        let (footer, decoded) = footer_of(&mut probation, 0);
        assert_eq!(
            footer.get("untimed_by_rule"),
            Some(&Json::Bool(true)),
            "{when}: {footer:?}"
        );
        assert_eq!(footer.get("untimed"), Some(&Json::Bool(true)), "{when}");
        assert!(decoded.untimed_by_rule && decoded.untimed, "{when}");
        assert!(probation.time().untimed_by_rule, "{when}");
        spend(&mut probation, &admin, &mut elapsed);
    }

    assert!(probation.begin_push().expect("the Push begins"));
    let (footer, decoded) = footer_of(&mut probation, 0);
    assert_eq!(footer.get("untimed_by_rule"), None, "a Push: {footer:?}");
    assert!(!decoded.untimed_by_rule);
    while let Some(report) = probation.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
    assert_eq!(probation.time().phase, Phase::Recap);
    let (footer, decoded) = footer_of(&mut probation, 0);
    assert_eq!(footer.get("untimed"), Some(&Json::Bool(true)), "a recap");
    assert_eq!(footer.get("untimed_by_rule"), None, "a recap: {footer:?}");
    assert!(!decoded.untimed_by_rule);
    next_lull(&mut probation);
    let (footer, _) = footer_of(&mut probation, 0);
    assert_eq!(
        footer.get("untimed_by_rule"),
        Some(&Json::Bool(true)),
        "the next Lull: {footer:?}"
    );

    let mut custom = hosted_under("m-by-host", Preset::Custom, Some(0), 1_000, 3);
    let (footer, decoded) = footer_of(&mut custom, 0);
    assert_eq!(footer.get("untimed"), Some(&Json::Bool(true)), "fresh");
    assert_eq!(footer.get("untimed_by_rule"), None, "fresh: {footer:?}");
    assert!(!decoded.untimed_by_rule);
    let admin = admin_token(&mut custom);
    answered(
        &call(
            &mut custom,
            &admin,
            "report_host_clock",
            "{\"elapsed_ms\":1000,\"remaining_ms\":120000}",
        ),
        "a custom match's countdown",
    );
    let (footer, decoded) = footer_of(&mut custom, 0);
    assert_eq!(footer.get("untimed"), None, "timed: {footer:?}");
    assert_eq!(footer.get("untimed_by_rule"), None, "timed: {footer:?}");
    assert!(!decoded.untimed && !decoded.untimed_by_rule);
}

/// An in-process seat that reads the `_status` footer of its first call, as
/// the built-in operator's own calls are answered.
struct FooterReader(std::sync::Arc<std::sync::Mutex<Option<Json>>>);

impl pharmakos_gateway::serve::BuiltInSeat for FooterReader {
    fn plan(&mut self, _seat: u8, call: &mut dyn FnMut(&pharmakos_gateway::rpc::Request) -> Json) {
        let request = pharmakos_gateway::rpc::parse(
            r#"{"jsonrpc":"2.0","id":1,"method":"get_status","params":{}}"#,
        )
        .expect("well formed");
        let answer = call(&request);
        let footer = answer
            .get("result")
            .and_then(|result| result.get("_status"))
            .cloned();
        if let Ok(mut held) = self.0.lock() {
            *held = footer;
        }
    }
}

/// Plays seat 0 with a [`FooterReader`] and advises nobody.
struct PlaysSeatZero(Option<FooterReader>);

impl pharmakos_gateway::serve::Operators for PlaysSeatZero {
    fn built_in(&mut self, seat: u8) -> Option<Box<dyn pharmakos_gateway::serve::BuiltInSeat>> {
        if seat != 0 {
            return None;
        }
        let reader: Box<dyn pharmakos_gateway::serve::BuiltInSeat> = Box::new(self.0.take()?);
        Some(reader)
    }

    fn advisor(&mut self, _seat: u8) -> Option<Box<dyn pharmakos_gateway::serve::Advisor>> {
        None
    }
}

/// S3's `gwu`, fix pass 1 (the review's C2): a resumed Probation Lull says
/// it is untimed by rule on the very first call, to a socket seat and to an
/// in-process seat alike, so the client starts no countdown the gateway would
/// refuse; a resumed `custom` Lull never does.
#[test]
fn a_resumed_probation_lull_is_untimed_by_rule_from_its_first_call() {
    for preset in [Preset::Probation, Preset::Custom] {
        let probation = preset == Preset::Probation;
        let surface = hosted_under("m-resume-rule", preset, Some(1), 1_000, 3);
        let saved = surface.lull_save().expect("a Lull saves");
        let mut resumed = resume("m-resume-rule", &saved, preset, 1_000, 3);
        assert_eq!(resumed.time().untimed_by_rule, probation, "{preset:?}");
        let (footer, decoded) = footer_of(&mut resumed, 1);
        assert_eq!(decoded.untimed_by_rule, probation, "{preset:?}: {footer:?}");
        assert!(decoded.untimed, "{preset:?}: no countdown reported yet");

        let mut resumed = resume("m-resume-rule", &saved, preset, 1_000, 3);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
        let mut operators = PlaysSeatZero(Some(FooterReader(std::sync::Arc::clone(&seen))));
        let mut seats = pharmakos_gateway::serve::InProcessSeats::open(
            &mut resumed,
            &SEATS,
            Some(1),
            &mut operators,
        )
        .expect("in-process seats");
        seats.plan(&mut resumed);
        let footer = seen
            .lock()
            .ok()
            .and_then(|held| held.clone())
            .expect("the in-process seat read a footer");
        assert_eq!(
            footer.get("untimed_by_rule") == Some(&Json::Bool(true)),
            probation,
            "{preset:?}: an in-process seat's footer: {footer:?}"
        );
    }
}

/// S3's `gwu`, fix pass 1 (the review's C6): the last countdown the host
/// reported is not in the save (its container is a contract this lane does
/// not change), so a Lull resumed after a reported 0 and ended with no new 0
/// files the neutral line; a 0 reported after the resume runs out.
#[test]
fn a_resumed_lull_forgets_a_countdown_of_0() {
    let mut surface = hosted_under("m-resume-0", Preset::Custom, Some(1), 1_000, 3);
    let admin = admin_token(&mut surface);
    answered(
        &call(
            &mut surface,
            &admin,
            "report_host_clock",
            "{\"elapsed_ms\":1000,\"remaining_ms\":0}",
        ),
        "report_host_clock",
    );
    let saved = surface.lull_save().expect("a Lull saves");
    let filing = |surface: &mut Surface| -> String {
        feed_of(surface, 1)
            .into_iter()
            .find(|(kind, ..)| kind == "safe_playbook_filed")
            .map(|(_, text, _)| text)
            .expect("a filing line")
    };
    for (again, line) in [(false, ENDED), (true, RAN_OUT)] {
        let mut resumed = resume("m-resume-0", &saved, Preset::Custom, 1_000, 3);
        let admin = admin_token(&mut resumed);
        if again {
            answered(
                &call(
                    &mut resumed,
                    &admin,
                    "report_host_clock",
                    "{\"elapsed_ms\":1000,\"remaining_ms\":0}",
                ),
                "report_host_clock",
            );
        }
        answered(&call(&mut resumed, &admin, "end_lull", "{}"), "end_lull");
        assert_eq!(filing(&mut resumed), line, "a 0 reported again: {again}");
    }
}

// ---------------------------------------------------------------------------
// E5: the memos
// ---------------------------------------------------------------------------

/// A Probation match's human seat is issued **first death** at its
/// commander's first death, once: on its own feed (the line's text and its
/// `ledger_memo` payload), and on its recap. The memo survives a save and a
/// resume, through the file's own text, as the store a later trigger is
/// checked against. The same match `custom` issues no memo at all, nor does
/// the other seat of the Probation match get one.
///
/// `scenarios/s2/hunt-the-commander`: seat 0's Attack beacon hunts seat 1's
/// commander, here the human's, when it walks into sight.
#[test]
fn a_memo_fires_once_survives_a_resume_and_never_in_a_custom_match() {
    const SEGMENT_MS: i32 = 480_000;
    let hunter = committed("scenarios/s2/hunt-the-commander.playbook.jsonc");
    let quarry = committed("scenarios/s2/hunt-the-commander.enemy.playbook.jsonc");

    for preset in [Preset::Probation, Preset::Custom] {
        let mut surface = hosted_under("m-memo", preset, Some(1), SEGMENT_MS, 2);
        let admin = admin_token(&mut surface);
        let mut elapsed = 0_i32;
        submit(&mut surface, &admin, &mut elapsed, 0, &hunter);
        submit(&mut surface, &admin, &mut elapsed, 1, &quarry);
        play_push(&mut surface);
        assert!(
            fired(&surface, "commander_died", 1) > 0,
            "the recipe: seat 1's commander dies in round 1"
        );
        let expected: Vec<String> = match preset {
            Preset::Probation => vec![String::from(Memo::FirstDeath.name())],
            Preset::Custom => Vec::new(),
        };
        assert_eq!(memos_on_the_feed(&mut surface, 1), expected, "{preset:?}");
        assert_eq!(memos_on_the_recap(&mut surface, 1), expected, "{preset:?}");
        assert!(
            memos_on_the_feed(&mut surface, 0).is_empty(),
            "the other seat is not the person's"
        );
        assert!(memos_on_the_recap(&mut surface, 0).is_empty());
        if preset == Preset::Custom {
            continue;
        }
        if let Some((_, text, _)) = feed_of(&mut surface, 1)
            .into_iter()
            .find(|(kind, ..)| kind == "ledger_memo")
        {
            assert_eq!(text, pharmakos_gateway::strings::MEMO_FIRST_DEATH);
        }

        // A save at round 2's Lull, through the file's own text, resumed.
        next_lull(&mut surface);
        let saved = surface.lull_save().expect("a Lull saves");
        assert_eq!(
            saved
                .seats
                .iter()
                .find(|seat| seat.seat == 1)
                .map(|seat| seat.memos.clone()),
            Some(vec![Memo::FirstDeath]),
            "the save holds the memo issued"
        );
        let mut resumed = resume("m-memo", &saved, Preset::Probation, SEGMENT_MS, 2);
        assert_eq!(
            memos_on_the_recap(&mut resumed, 1),
            vec![String::from(Memo::FirstDeath.name())],
            "the memo the match issued is still the seat's"
        );
        // What a resume put back is the store a memo is checked against
        // before it is issued (`Surface::issue_memo`); the unit test
        // `a_memo_is_the_human_seats_once_and_never_a_recycles` pins that a
        // held memo is never issued again. A resumed Lull's feed is a fresh
        // segment's, so asserting it holds no memo could not fail, and is
        // not asserted.
    }
}

/// **Dark Stake**: one of the human seat's beacons destroyed issues the memo
/// once (`scenarios/s2/breach-and-raze`: seat 0's raiders destroy seat 1's
/// walled beacon).
#[test]
fn a_beacon_destroyed_issues_the_dark_stake_memo() {
    let mut surface = hosted_under("m-stake", Preset::Probation, Some(1), 480_000, 1);
    let admin = admin_token(&mut surface);
    let mut elapsed = 0_i32;
    submit(
        &mut surface,
        &admin,
        &mut elapsed,
        0,
        &committed("scenarios/s2/breach-and-raze.playbook.jsonc"),
    );
    submit(
        &mut surface,
        &admin,
        &mut elapsed,
        1,
        &committed("scenarios/s2/breach-and-raze.enemy.playbook.jsonc"),
    );
    play_push(&mut surface);
    assert!(fired(&surface, "beacon_destroyed", 1) > 0, "the recipe");
    assert_eq!(
        memos_on_the_feed(&mut surface, 1),
        vec![String::from(Memo::DarkStake.name())]
    );
}

/// **Power shortfall**: the human seat's first brownout issues the memo
/// once; and the beacon it **recycled** on the way, which the sim reports as
/// destroyed too, is the seat's own act and no dark Stake
/// (`scenarios/s1/brownout-by-recycle`).
#[test]
fn a_brownout_issues_the_shortfall_memo_and_a_recycle_is_no_dark_stake() {
    let mut surface = hosted_under("m-shortfall", Preset::Probation, Some(0), 180_000, 3);
    let playbook = committed("scenarios/s1/brownout-by-recycle.playbook.jsonc");
    for round in 1..=2_u32 {
        let admin = admin_token(&mut surface);
        let mut elapsed = 0_i32;
        submit(&mut surface, &admin, &mut elapsed, 0, &playbook);
        play_push(&mut surface);
        if round == 2 {
            assert!(fired(&surface, "beacon_recycled", 0) > 0, "the recipe");
            assert!(fired(&surface, "beacon_destroyed", 0) > 0, "the recipe");
            assert!(fired(&surface, "beacon_browned_out", 0) > 0, "the recipe");
            assert_eq!(
                memos_on_the_feed(&mut surface, 0),
                vec![String::from(Memo::PowerShortfall.name())]
            );
        } else {
            next_lull(&mut surface);
        }
    }
}

/// A saved match resumed through the file's own text (`tests/save.rs`'s
/// way), under `preset`.
fn resume(
    match_id: &str,
    saved: &SavedMatch,
    preset: Preset,
    segment_ms: i32,
    round_limit: u32,
) -> Surface {
    let config = Config {
        match_id: match_id.to_owned(),
        seed: SEED,
        seats: 2,
        human_seat: Some(1),
        segment_lengths_ms: vec![segment_ms],
        round_limit,
        preset,
    };
    let hash = rules().rules_hash();
    let text = Save {
        stamp: Stamp::current(hash),
        config: config.clone(),
        state: saved.clone(),
    }
    .render();
    let save = Save::parse(&text).expect("a save reads back");
    save.check(&config, hash).expect("the same match");
    assert_eq!(&save.state, saved, "the file carries all of it");
    let host = Host::open_from(
        &rules_json(),
        SEED,
        2,
        &Settings {
            segment_lengths_ms: vec![segment_ms],
            round_limit,
        },
        Some(library_folder()),
    )
    .expect("a match");
    let rules = host.rules().clone();
    Surface::new(match_id, SEED, rules, FogPolicy::fogged(), &SEATS)
        .expect("a surface")
        .with_preset(preset, Some(SeatId::new(1)))
        .resume(
            host,
            &save.state,
            Continuation {
                generation: 1,
                last_tick: 0,
            },
        )
        .expect("a Lull resumes")
}

// ---------------------------------------------------------------------------
// E25: the guided pick
// ---------------------------------------------------------------------------

/// The advice filed for the human seat, starting at `start` with its own
/// sentence, and a suggestion for each of `templates` whose `why` is the
/// operator's bookkeeping.
fn advice(start: Option<&str>, templates: &[&str]) -> Advice {
    Advice {
        safe_playbook_jsonc: String::new(),
        suggestions: templates
            .iter()
            .map(|template| Suggestion {
                template_id: (*template).to_owned(),
                parameters: Vec::new(),
                why: format!("Bookkeeping for {template}."),
            })
            .collect(),
        start_here: start.map(|template| StartHere {
            template_id: template.to_owned(),
            why: format!("Start with {template}: it is the advisor's pick."),
        }),
    }
}

/// `(template_id, guided_pick, guided_pick_reason)` of `seat`'s template list.
fn listed(surface: &mut Surface, seat: u8) -> Vec<(String, bool, String)> {
    let token = seat_token(surface, seat);
    let answer = result(
        &call(surface, &token, "list_templates", "{}"),
        "list_templates",
    );
    array_of(&answer, "templates")
        .iter()
        .map(|template| {
            (
                text_of(template, "template_id"),
                template.get("guided_pick") == Some(&Json::Bool(true)),
                match template.get("guided_pick_reason") {
                    None => String::new(),
                    Some(Json::String(reason)) => reason.clone(),
                    Some(other) => panic!("`guided_pick_reason` is a string: {other:?}"),
                },
            )
        })
        .collect()
}

/// In a Probation match's first Lull, the human seat's template list marks
/// the one template its advisor says to start with, and carries that
/// suggestion's `why` as the reason; no other template is marked, no other
/// seat is told, and a `custom` match, a later round, or an advice whose
/// start it has no suggestion for marks nothing.
#[test]
fn the_first_lull_marks_the_advisors_start_with_its_reason() {
    let mut surface = hosted_under("m-guided", Preset::Probation, Some(0), 1_000, 2);
    surface
        .file_advice(
            SeatId::new(0),
            advice(
                Some("hold_and_build"),
                &["expand_and_mine", "hold_and_build"],
            ),
        )
        .expect("filed");
    let list = listed(&mut surface, 0);
    let marked: Vec<&(String, bool, String)> = list.iter().filter(|entry| entry.1).collect();
    assert_eq!(marked.len(), 1, "one template marked: {list:?}");
    let (template, _, reason) = marked.first().copied().expect("one");
    assert_eq!(template, "hold_and_build");
    assert_eq!(
        reason,
        "Start with hold_and_build: it is the advisor's pick."
    );
    assert!(
        list.iter()
            .filter(|entry| !entry.1)
            .all(|entry| entry.2.is_empty()),
        "no reason without the mark: {list:?}"
    );
    assert!(
        listed(&mut surface, 1).iter().all(|entry| !entry.1),
        "seat 1 is not the person's, and is told nothing of seat 0's advice"
    );

    // An advice whose start has no reason marks nothing.
    let mut silent = advice(Some("hold_and_build"), &["hold_and_build"]);
    if let Some(start) = silent.start_here.as_mut() {
        start.why = String::from("  ");
    }
    surface.file_advice(SeatId::new(0), silent).expect("filed");
    assert!(listed(&mut surface, 0).iter().all(|entry| !entry.1));

    // An advice that starts at a template it gave no suggestion for marks
    // nothing: it would not instantiate as suggested.
    surface
        .file_advice(
            SeatId::new(0),
            advice(Some("raid_an_enemy_beacon"), &["hold_and_build"]),
        )
        .expect("filed");
    assert!(listed(&mut surface, 0).iter().all(|entry| !entry.1));

    // Round 2 marks nothing.
    surface
        .file_advice(
            SeatId::new(0),
            advice(Some("hold_and_build"), &["hold_and_build"]),
        )
        .expect("filed");
    play_push(&mut surface);
    next_lull(&mut surface);
    surface
        .file_advice(
            SeatId::new(0),
            advice(Some("hold_and_build"), &["hold_and_build"]),
        )
        .expect("filed");
    assert!(
        listed(&mut surface, 0).iter().all(|entry| !entry.1),
        "round 2"
    );

    // A custom match marks nothing.
    let mut custom = hosted_under("m-unguided", Preset::Custom, Some(0), 1_000, 2);
    custom
        .file_advice(
            SeatId::new(0),
            advice(Some("hold_and_build"), &["hold_and_build"]),
        )
        .expect("filed");
    assert!(listed(&mut custom, 0).iter().all(|entry| !entry.1));
}

// ---------------------------------------------------------------------------
// E18: the seed withheld
// ---------------------------------------------------------------------------

/// A seat's token is told no seed: `get_map_summary` carries no
/// `match_seed` and its prose names none. The lobby's token keeps it.
#[test]
fn a_seats_map_summary_carries_no_seed() {
    let mut surface = hosted_under("m-seed", Preset::Custom, Some(0), 1_000, 1);
    for seat in [0_u8, 1] {
        let token = seat_token(&mut surface, seat);
        let summary = result(
            &call(&mut surface, &token, "get_map_summary", "{}"),
            "get_map_summary",
        );
        assert!(summary.get("match_seed").is_none(), "{summary:?}");
        assert!(!text_of(&summary, "prose").contains("seed"), "{summary:?}");
    }
    let admin = admin_token(&mut surface);
    let summary = result(
        &call(&mut surface, &admin, "get_map_summary", "{}"),
        "get_map_summary",
    );
    assert_eq!(text_of(&summary, "match_seed"), "0x00000000ca5caded");
    for nofog in [false, true] {
        let spectator = spectator_token(&mut surface, nofog);
        let summary = result(
            &call(&mut surface, &spectator, "get_map_summary", "{}"),
            "get_map_summary",
        );
        assert_eq!(
            text_of(&summary, "match_seed"),
            "0x00000000ca5caded",
            "a spectator keeps the seed (nofog {nofog})"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 143 (4) and S3's `gwu`: the filing line says what ended the Lull
// ---------------------------------------------------------------------------

/// The line for a seat that said Ready with nothing sealed.
const ON_READY: &str =
    "You said Ready with nothing sealed, so the safe playbook was filed for you.";
/// The line for a Lull whose last reported countdown was 0.
const RAN_OUT: &str = "The Lull ran out, so the safe playbook was filed for you.";
/// The line for every other end.
const ENDED: &str = "The Lull ended before you sealed, so the safe playbook was filed for you.";

/// End a two-seat match's opening Lull, neither seat having sealed: each seat
/// in `ready` says Ready, the host reports each of `reports` in order (the
/// elapsed time and, when given, the countdown), and the lobby calls
/// `end_lull`. Answers each seat's filing line.
fn filing_lines(
    match_id: &str,
    preset: Preset,
    ready: &[u8],
    reports: &[(i32, Option<i32>)],
) -> [String; 2] {
    let mut surface = hosted_under(match_id, preset, Some(0), 1_000, 1);
    for seat in ready {
        let token = seat_token(&mut surface, *seat);
        answered(
            &call(&mut surface, &token, "set_ready", "{\"ready\":true}"),
            "set_ready",
        );
    }
    let admin = admin_token(&mut surface);
    for (elapsed, remaining) in reports {
        let params = match remaining {
            Some(left) => format!("{{\"elapsed_ms\":{elapsed},\"remaining_ms\":{left}}}"),
            None => format!("{{\"elapsed_ms\":{elapsed}}}"),
        };
        answered(
            &call(&mut surface, &admin, "report_host_clock", &params),
            "report_host_clock",
        );
    }
    answered(&call(&mut surface, &admin, "end_lull", "{}"), "end_lull");
    [0, 1].map(|seat| {
        feed_of(&mut surface, seat)
            .into_iter()
            .find(|(kind, ..)| kind == "safe_playbook_filed")
            .map(|(_, text, _)| text)
            .expect("a filing line")
    })
}

/// The line a seat's feed carries when the gateway filed its safe playbook
/// says what ended the Lull, from what the host reported and never from a
/// clock of the gateway's own: the seat's own Ready with nothing sealed has
/// its line, and wins over a countdown at 0 too; a Lull whose **last**
/// reported countdown was 0 "ran out" (the rig reports its 0 once, then calls
/// `end_lull`; S3's `ed3b`); and every other end -- a last countdown with
/// time left, a report of the elapsed time alone, no report at all, or the
/// lobby's `end_lull` -- reads "the Lull ended". A Probation match reports no
/// countdown, so it never "runs out".
#[test]
fn the_filing_line_says_what_ended_the_lull() {
    // Ready, with nothing sealed: seat 0 says it is ready; seat 1 does not.
    let mut surface = hosted_under("m-ready", Preset::Custom, Some(0), 1_000, 1);
    let token = seat_token(&mut surface, 0);
    answered(
        &call(&mut surface, &token, "set_ready", "{\"ready\":true}"),
        "set_ready",
    );
    assert!(surface.begin_push().expect("the Push begins"));
    let lines = [0, 1].map(|seat| {
        feed_of(&mut surface, seat)
            .into_iter()
            .find(|(kind, ..)| kind == "safe_playbook_filed")
            .map(|(_, text, _)| text)
            .expect("a filing line")
    });
    assert_eq!(lines, [ON_READY, ENDED]);

    // The countdown at 0, last: "ran out".
    assert_eq!(
        filing_lines("m-ran-out", Preset::Custom, &[], &[(1_000, Some(0))]),
        [RAN_OUT, RAN_OUT]
    );
    assert_eq!(
        filing_lines(
            "m-ran-out-then-spent",
            Preset::Custom,
            &[],
            &[(1_000, Some(4_000)), (5_000, Some(0)), (5_200, None)]
        ),
        [RAN_OUT, RAN_OUT],
        "an elapsed-only report after the 0 keeps the countdown at 0"
    );

    // Ready wins over the 0, seat by seat.
    assert_eq!(
        filing_lines("m-ready-and-0", Preset::Custom, &[0], &[(1_000, Some(0))]),
        [ON_READY, RAN_OUT]
    );
    assert_eq!(
        filing_lines("m-both-ready", Preset::Custom, &[0, 1], &[(1_000, Some(0))]),
        [ON_READY, ON_READY]
    );

    // Any other end: a few milliseconds left (the rig before `ed3b`), time
    // left, the elapsed time alone, no report, or a 0 the host reported and
    // then took back with time left (the last report decides).
    for left in [1, 120, 250, 5_000] {
        assert_eq!(
            filing_lines("m-left", Preset::Custom, &[], &[(1_000, Some(left))]),
            [ENDED, ENDED],
            "{left} ms left"
        );
    }
    assert_eq!(
        filing_lines("m-elapsed", Preset::Custom, &[], &[(1_000, None)]),
        [ENDED, ENDED]
    );
    assert_eq!(
        filing_lines("m-no-report", Preset::Custom, &[], &[]),
        [ENDED, ENDED]
    );
    assert_eq!(
        filing_lines(
            "m-0-then-more",
            Preset::Custom,
            &[],
            &[(1_000, Some(0)), (2_000, Some(5_000))]
        ),
        [ENDED, ENDED]
    );

    // Probation: no countdown is ever reported, so Ready or "ended".
    assert_eq!(
        filing_lines("m-probation", Preset::Probation, &[0], &[(1_000, None)]),
        [ON_READY, ENDED]
    );
}

// ---------------------------------------------------------------------------
// E7 at the host's own door
// ---------------------------------------------------------------------------

/// The host-side countdown (`Surface::set_phase_remaining_ms`, the headless
/// hosts') is refused in a Probation match as `report_host_clock`'s is, so E7
/// does not rest on which hosts set a preset; a custom match takes it.
#[test]
fn a_host_side_countdown_is_refused_in_a_probation_match() {
    let mut probation = hosted_under("m-host-clock", Preset::Probation, Some(0), 1_000, 1);
    let error = probation
        .set_phase_remaining_ms(pharmakos_sim::math::quantity::Ms::new(60_000))
        .expect_err("untimed by preset");
    assert_eq!(error.name(), "INVALID_ARGUMENT");
    assert!(error.message().contains("probation"), "{}", error.message());
    assert!(probation.time().untimed, "nothing of it was kept");
    let mut custom = hosted_under("m-host-clock-c", Preset::Custom, Some(0), 1_000, 1);
    custom
        .set_phase_remaining_ms(pharmakos_sim::math::quantity::Ms::new(60_000))
        .expect("a custom match takes a countdown");
    assert!(!custom.time().untimed);
}

// ---------------------------------------------------------------------------
// Item 155 (3) 5 and 7, hosted
// ---------------------------------------------------------------------------

/// Item 155 (3) 5: one fallback taking over is one `fallback_engaged` line in
/// the seat's feed, read in small pages between ticks as the client reads it,
/// with the seat named. The demo's two identical lines were the feed's cursor
/// re-delivering a row as sight moved between reads, which this PR's
/// witness stamp ends ([`pharmakos_gateway::feed::Witnesses`]).
#[test]
fn one_fallback_is_one_line_in_the_feed() {
    let mut surface = hosted_under("m-fallback", Preset::Custom, Some(0), 3_000, 1);
    let admin = admin_token(&mut surface);
    let mut elapsed = 0_i32;
    let route = walk_east(&surface, 0, 2, 0);
    submit(&mut surface, &admin, &mut elapsed, 0, &route);
    assert!(surface.begin_push().expect("the Push begins"));
    let token = seat_token(&mut surface, 0);
    let mut cursor = String::new();
    let mut lines: Vec<String> = Vec::new();
    loop {
        let more = surface.step().expect("a tick");
        let page = result(
            &call(
                &mut surface,
                &token,
                "get_segment_feed",
                &format!("{{\"cursor\":{},\"limit\":2}}", quote(&cursor)),
            ),
            "get_segment_feed",
        );
        for event in array_of(&page, "events") {
            if text_of(&event, "kind") == "fallback_engaged" {
                lines.push(text_of(&event, "text"));
            }
        }
        cursor = text_of(&page, "next_cursor");
        if more.is_none_or(|report| report.segment_ended) {
            break;
        }
    }
    // Read what is left of the segment.
    loop {
        let page = result(
            &call(
                &mut surface,
                &token,
                "get_segment_feed",
                &format!("{{\"cursor\":{},\"limit\":2}}", quote(&cursor)),
            ),
            "get_segment_feed",
        );
        let events = array_of(&page, "events");
        if events.is_empty() {
            break;
        }
        for event in events {
            if text_of(&event, "kind") == "fallback_engaged" {
                lines.push(text_of(&event, "text"));
            }
        }
        cursor = text_of(&page, "next_cursor");
    }
    assert_eq!(lines.len(), 1, "one fallback, one line: {lines:?}");
    assert!(
        lines
            .first()
            .is_some_and(|line| line.starts_with("The route of seat 0 ended")),
        "{lines:?}"
    );
}

/// Item 155 (3) 7: at the match's end the recap's tally reads "In this match",
/// and between rounds "This match so far".
#[test]
fn the_tally_at_the_matchs_end_reads_in_this_match() {
    let mut surface = hosted_under("m-tally", Preset::Custom, Some(0), 1_000, 2);
    play_push(&mut surface);
    let token = seat_token(&mut surface, 0);
    let between = text_of(
        &result(&call(&mut surface, &token, "get_recap", "{}"), "get_recap"),
        "prose",
    );
    assert!(between.contains("This match so far"), "{between}");
    next_lull(&mut surface);
    play_push(&mut surface);
    let ended = text_of(
        &result(&call(&mut surface, &token, "get_recap", "{}"), "get_recap"),
        "prose",
    );
    assert!(
        ended.contains("In this match") && !ended.contains("so far"),
        "{ended}"
    );
}

// ---------------------------------------------------------------------------
// The licence, earned at a real settlement (S3's `gwr` review, item 10)
// ---------------------------------------------------------------------------

/// The standing points of the two vents nearest seat 0's core, each on a
/// footprint column whose top voxel is vent material, as `crates/sim/tests/
/// licence.rs` finds them.
fn two_vent_stands(world: &pharmakos_sim::world::World) -> Vec<[Fx; 3]> {
    let row = (0..world.beacons().ids().len())
        .find(|row| world.beacons().seats().get(*row) == Some(&0))
        .expect("seat 0's core");
    let from = world
        .beacons()
        .positions()
        .get(row)
        .copied()
        .expect("its place");
    let mut vents: Vec<(Sq, usize, [Fx; 3])> = Vec::new();
    for (index, feature) in world.features().features().iter().enumerate() {
        if feature.kind != pharmakos_sim::features::FeatureKind::Vent {
            continue;
        }
        let stand = feature.footprint.iter().find_map(|column| {
            let z = world.voxels().top_solid_z(column.x, column.y)?;
            world
                .voxels()
                .get([column.x, column.y, z])
                .and_then(pharmakos_sim::Material::vent_richness)?;
            Some([
                Fx::from_voxels(i16::try_from(column.x).ok()?),
                Fx::from_voxels(i16::try_from(column.y).ok()?),
                Fx::from_voxels(i16::try_from(z + 1).ok()?),
            ])
        });
        if let Some(stand) = stand {
            vents.push((Sq::between(from, stand), index, stand));
        }
    }
    vents.sort_unstable_by_key(|(distance, index, _)| (*distance, *index));
    vents.into_iter().take(2).map(|(_, _, at)| at).collect()
}

/// S3's `gwr` review, item 10: seat 0 runs Generators on two vents, the
/// Ledger's own check awards it the Resonance Spire's licence at round 1's
/// settlement, and every read this lane added tells it: the recap's licence
/// line and its prose, the feed's `first_licence` memo in a Probation match,
/// the next Lull's briefing (`licences_held`) and the vocabulary, whose
/// Resonance Spire unlocks.
#[test]
fn a_licence_earned_at_a_settlement_reaches_the_recap_the_feed_the_briefing_and_the_vocabulary() {
    let config = WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![10_000],
            round_limit: 3,
        },
    };
    let mut world = pharmakos_sim::world::World::new(&config).expect("the world generates");
    let core = {
        let row = (0..world.beacons().ids().len())
            .find(|row| world.beacons().seats().get(*row) == Some(&0))
            .expect("seat 0's core");
        pharmakos_sim::tables::BeaconId::new(
            world.beacons().ids().get(row).copied().expect("an id"),
        )
    };
    for at in two_vent_stands(&world) {
        world
            .raise_structure(
                SeatId::new(0),
                core,
                pharmakos_sim::tables::StructureKind::Generator,
                at,
            )
            .expect("room for a Generator");
    }
    let bytes = pharmakos_sim::snapshot::Snapshot::capture(&world)
        .expect("a world between ticks")
        .to_bytes()
        .expect("it encodes");
    let mut host = Host::open(&config, Some(library_folder())).expect("a match");
    host.resume(&bytes).expect("the world resumes");
    let mut surface = Surface::new("m-licence", SEED, rules(), FogPolicy::fogged(), &SEATS)
        .expect("a match id")
        .with_preset(Preset::Probation, Some(SeatId::new(0)));
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");

    let spire = |surface: &mut Surface| {
        let token = seat_token(surface, 0);
        let words = result(
            &call(surface, &token, "get_vocabulary", "{}"),
            "get_vocabulary",
        );
        array_of(&words, "blueprints")
            .into_iter()
            .find(|entry| text_of(entry, "name") == "resonance_spire")
            .expect("the Spire is listed")
    };
    assert!(
        spire(&mut surface).get("locked").is_some(),
        "locked before the licence"
    );

    play_push(&mut surface);
    let recap = golden_answer(&mut surface, 0, "get_recap", "recap_probation_round");
    assert!(recap.get("ledger_memos").is_some(), "{recap:?}");
    let awarded = array_of(&recap, "licences_awarded");
    assert_eq!(awarded.len(), 1, "{recap:?}");
    let award = awarded.first().expect("the award");
    assert_eq!(text_of(award, "seat"), "seat.0");
    assert_eq!(text_of(award, "licence"), "resonance_spire");
    assert!(
        text_of(&recap, "prose").contains("Licence awarded: Resonance Spire."),
        "{recap:?}"
    );
    let theirs = seat_token(&mut surface, 1);
    let their_recap = result(&call(&mut surface, &theirs, "get_recap", "{}"), "get_recap");
    assert_eq!(
        their_recap.get("licences_awarded"),
        None,
        "seat 0's award is not seat 1's to know between rounds"
    );
    assert_eq!(
        memos_on_the_feed(&mut surface, 0),
        vec![String::from(Memo::FirstLicence.name())],
        "the person's first licence issues its memo"
    );
    memo_page_golden(&mut surface, 0);

    next_lull(&mut surface);
    let token = seat_token(&mut surface, 0);
    let briefing = result(
        &call(&mut surface, &token, "get_briefing", "{}"),
        "get_briefing",
    );
    assert_eq!(
        briefing.get("licences_held"),
        Some(&Json::Array(vec![Json::String(String::from(
            "resonance_spire"
        ))])),
        "{briefing:?}"
    );
    assert_eq!(
        spire(&mut surface).get("locked"),
        None,
        "the Spire unlocks with the licence"
    );
    the_match_end_names_the_award_that_stands(&mut surface);
}

/// S3's `probw`, item 6, played on from round 2's Lull of the licence test
/// above: between rounds no recap names the awards that stand; at the match's
/// end (round 3 of 3) every recap does, in words, with the seat and the round
/// -- seat 1's names seat 0's award, and seat 0's names its own earlier one.
fn the_match_end_names_the_award_that_stands(surface: &mut Surface) {
    const STANDING: &str =
        "The licence that stands at the match's end: seat 0's Resonance Spire, from round 1.";
    play_push(surface);
    for seat in [0, 1] {
        let token = seat_token(surface, seat);
        let recap = result(&call(surface, &token, "get_recap", "{}"), "get_recap");
        assert!(
            !text_of(&recap, "prose").contains("at the match's end"),
            "seat {seat}, between rounds: {recap:?}"
        );
    }
    next_lull(surface);
    play_push(surface);
    for seat in [0, 1] {
        let recap = if seat == 1 {
            golden_answer(surface, seat, "get_recap", "recap_probation_ended")
        } else {
            let token = seat_token(surface, seat);
            result(&call(surface, &token, "get_recap", "{}"), "get_recap")
        };
        let prose = text_of(&recap, "prose");
        assert!(prose.contains(STANDING), "seat {seat}, at the end: {prose}");
        assert_eq!(
            prose.matches("at the match's end").count(),
            1,
            "one sentence: {prose}"
        );
        assert!(
            !prose.contains("Licence awarded:"),
            "nothing was awarded at this settlement: {prose}"
        );
        let rows = array_of(&recap, "licences_awarded");
        assert_eq!(rows.len(), 1, "the row the sentence names: {recap:?}");
    }
}

// ---------------------------------------------------------------------------
// S3's `probw`, item 7: `get_status` tells the preset and the seal held this round
// ---------------------------------------------------------------------------

/// `token`'s `get_status`, as the wire carries it and decoded as a client
/// reads it.
fn status_of(
    surface: &mut Surface,
    token: &Token,
) -> (Json, pharmakos_proto::gp::api::v1::GetStatusResponse) {
    let answer = result(&call(surface, token, "get_status", "{}"), "get_status");
    let decoded = pharmakos_proto::json::decode_json(&support::canonical_answer(
        "gp.api.v1.GetStatusResponse",
        &answer,
    ))
    .unwrap_or_else(|error| panic!("not a gp.api.v1.GetStatusResponse: {error}: {answer:?}"));
    (answer, decoded)
}

/// S3's `probw`, item 7: the preset, in both presets and across a resume, to
/// every caller; and the seal held this round, to its seat alone: absent
/// fresh, there after an accepted `submit_plan`, still there after a refused
/// later one and through the Push, gone at the next round's Lull, and there
/// again after a resume into a Lull where it was sealed. An admin and a
/// spectator get no seal field.
#[test]
fn get_status_tells_the_preset_and_the_seal_held_this_round() {
    use pharmakos_proto::gp::api::v1::Preset as Wire;
    for (preset, wire) in [
        (Preset::Probation, Wire::Probation),
        (Preset::Custom, Wire::Custom),
    ] {
        let mut surface = hosted_under("m-status", preset, Some(1), 1_000, 3);
        let admin = admin_token(&mut surface);
        let spectator = spectator_token(&mut surface, true);
        let seat = seat_token(&mut surface, 0);
        let other = seat_token(&mut surface, 1);
        for token in [&seat, &other, &admin, &spectator] {
            let (answer, decoded) = status_of(&mut surface, token);
            assert_eq!(decoded.preset(), wire, "{preset:?}: {answer:?}");
            assert_eq!(
                answer.get("preset"),
                Some(&Json::String(String::from(preset.name()))),
                "the config line's spelling"
            );
            assert_eq!(answer.get("sealed_this_round"), None, "fresh: {answer:?}");
        }

        let mut elapsed = 0_i32;
        let route = walk_east(&surface, 0, 2, 0);
        submit(&mut surface, &admin, &mut elapsed, 0, &route);
        let (answer, decoded) = status_of(&mut surface, &seat);
        assert!(decoded.sealed_this_round, "{preset:?}: sealed: {answer:?}");
        let (answer, _) = status_of(&mut surface, &other);
        assert_eq!(
            answer.get("sealed_this_round"),
            None,
            "another seat's seal is not mine"
        );
        for token in [&admin, &spectator] {
            let (answer, _) = status_of(&mut surface, token);
            assert_eq!(answer.get("sealed_this_round"), None, "no seat: {answer:?}");
        }

        // A refused later submission leaves the seal held.
        spend(&mut surface, &admin, &mut elapsed);
        let refused = result(
            &call(
                &mut surface,
                &seat,
                "submit_plan",
                &format!("{{\"playbook_jsonc\":{}}}", quote("{ not a playbook")),
            ),
            "submit_plan",
        );
        assert_eq!(
            refused.get("accepted"),
            Some(&Json::Bool(false)),
            "{refused:?}"
        );
        let (answer, decoded) = status_of(&mut surface, &seat);
        assert!(decoded.sealed_this_round, "still sealed: {answer:?}");

        // A resume into this Lull keeps the seal, as the save does.
        let saved = surface.lull_save().expect("a Lull saves");
        let mut resumed = resume("m-status", &saved, preset, 1_000, 3);
        let resumed_seat = seat_token(&mut resumed, 0);
        let (answer, decoded) = status_of(&mut resumed, &resumed_seat);
        assert!(decoded.sealed_this_round, "resumed sealed: {answer:?}");
        assert_eq!(decoded.preset(), wire, "the preset across a resume");
        let resumed_other = seat_token(&mut resumed, 1);
        let (answer, _) = status_of(&mut resumed, &resumed_other);
        assert_eq!(
            answer.get("sealed_this_round"),
            None,
            "resumed, unsealed: {answer:?}"
        );

        // Through the Push the seal is this round's; the next Lull's is not.
        play_push(&mut surface);
        let (answer, decoded) = status_of(&mut surface, &seat);
        assert!(decoded.sealed_this_round, "in the recap: {answer:?}");
        // The gateway filed the safe playbook for seat 1 at the Push: a seal too.
        let (answer, decoded) = status_of(&mut surface, &other);
        assert!(decoded.sealed_this_round, "the filed seal: {answer:?}");
        next_lull(&mut surface);
        for token in [&seat, &other] {
            let (answer, decoded) = status_of(&mut surface, token);
            assert!(!decoded.sealed_this_round, "a new round: {answer:?}");
            assert_eq!(decoded.preset(), wire);
        }
    }
}

/// S3's `probw`, item 7: with no match hosted there is no preset to tell, so
/// the field is left out (`PRESET_UNSPECIFIED`) and nothing claims a seal.
#[test]
fn get_status_with_no_match_hosted_tells_no_preset() {
    let mut surface = Surface::new("m-lobby", SEED, rules(), FogPolicy::fogged(), &SEATS)
        .expect("a match id")
        .with_preset(Preset::Probation, Some(SeatId::new(1)));
    let admin = admin_token(&mut surface);
    let (answer, decoded) = status_of(&mut surface, &admin);
    assert_eq!(answer.get("preset"), None, "{answer:?}");
    assert_eq!(
        decoded.preset(),
        pharmakos_proto::gp::api::v1::Preset::Unspecified
    );
    assert_eq!(answer.get("sealed_this_round"), None);
}

// ---------------------------------------------------------------------------
// S3's `probw`, item 8: Probation's goldens
// ---------------------------------------------------------------------------

/// `seat`'s answer to `method` with no parameters, written as the golden
/// `case` (`support::write_golden`: the whole JSON-RPC answer, its footer
/// included), and its `result`.
fn golden_answer(surface: &mut Surface, seat: u8, method: &str, case: &str) -> Json {
    let token = seat_token(surface, seat);
    let response = call(surface, &token, method, "{}");
    let answer = result(&response, method);
    support::write_golden(case, &pharmakos_gateway::rpc::render(&response));
    answer
}

/// The first page of `seat`'s segment feed that carries a `ledger_memo` line,
/// written as the golden `feed_ledger_memo`: the line and its typed payload,
/// as a client reads them (S3's plan, E5).
fn memo_page_golden(surface: &mut Surface, seat: u8) {
    let token = seat_token(surface, seat);
    let mut cursor = String::new();
    loop {
        let response = call(
            surface,
            &token,
            "get_segment_feed",
            &format!("{{\"cursor\":{}}}", quote(&cursor)),
        );
        let page = result(&response, "get_segment_feed");
        let events = array_of(&page, "events");
        assert!(!events.is_empty(), "the feed holds a memo's line");
        if events.iter().any(|event| {
            text_of(event, "kind") == "ledger_memo" && event.get("ledger_memo").is_some()
        }) {
            support::write_golden(
                "feed_ledger_memo",
                &pharmakos_gateway::rpc::render(&response),
            );
            return;
        }
        cursor = text_of(&page, "next_cursor");
    }
}

/// The golden `list_templates_guided`: the human seat's template list in a
/// Probation match's first Lull, its advisor's start marked with its reason
/// (the setup of `the_first_lull_marks_the_advisors_start_with_its_reason`).
#[test]
fn the_guided_template_list_answers_its_golden() {
    let mut surface = hosted_under("m-guided", Preset::Probation, Some(0), 1_000, 2);
    surface
        .file_advice(
            SeatId::new(0),
            advice(
                Some("hold_and_build"),
                &["expand_and_mine", "hold_and_build"],
            ),
        )
        .expect("filed");
    let list = golden_answer(&mut surface, 0, "list_templates", "list_templates_guided");
    assert!(
        array_of(&list, "templates")
            .iter()
            .any(|template| template.get("guided_pick") == Some(&Json::Bool(true))),
        "{list:?}"
    );
}

/// The golden `get_status_probation`: a seat's `get_status` in a Probation
/// Lull it has sealed: the footer untimed by rule, the preset and the seal.
#[test]
fn a_sealed_probation_status_answers_its_golden() {
    let mut surface = hosted_under("m-status-golden", Preset::Probation, Some(0), 1_000, 2);
    let admin = admin_token(&mut surface);
    let mut elapsed = 0_i32;
    let route = walk_east(&surface, 0, 2, 0);
    submit(&mut surface, &admin, &mut elapsed, 0, &route);
    let status = golden_answer(&mut surface, 0, "get_status", "get_status_probation");
    assert_eq!(status.get("sealed_this_round"), Some(&Json::Bool(true)));
    assert_eq!(
        status.get("preset"),
        Some(&Json::String(String::from("probation")))
    );
}
