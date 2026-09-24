// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The planning wire's seams (T18a, decisions-log item 111): the advisor, its
//! allow-list and its scratch view, the in-process rate, the seat's own safe
//! playbook, the wizard's suggestion, and the economy and beacon fields an
//! ordinary client plans with.
//!
//! The real operator is **T18**'s, in run 2. Everything here is driven by
//! **scripted** operators -- closures behind the two seams -- so what is under
//! test is the gateway's side of the door: what an advisor may call, what it
//! is told, what is filed, who is told it, and what it leaves untouched. The
//! seams are driven through [`InProcessSeats`], exactly as the host loop drives
//! them, against a surface the test holds, so the private store can be read
//! back without a socket.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use std::sync::{Arc, Mutex};

use pharmakos_gateway::advice::{Advice, SuggestedValue, Suggestion};
use pharmakos_gateway::fog::{FogPolicy, Vision};
use pharmakos_gateway::host::SAFE_PLAYBOOK;
use pharmakos_gateway::rpc::{self, Request};
use pharmakos_gateway::serve::{Advisor, BuiltInSeat, InProcessSeats, Operators};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;
use pharmakos_sim::tables::SeatId;

use support::{MATCH, SEGMENT_MS, library_folder, request, result, text_of};

// ---------------------------------------------------------------------------
// Scripted operators
// ---------------------------------------------------------------------------

/// The closure every scripted seat is handed.
type Call<'a> = dyn FnMut(&Request) -> Json + 'a;

/// A scripted advisor: whatever the closure does.
struct ScriptedAdvisor<F>(F);

impl<F> Advisor for ScriptedAdvisor<F>
where
    F: FnMut(u8, &mut Call<'_>) -> Advice + Send,
{
    fn advise(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json) -> Advice {
        (self.0)(seat, call)
    }
}

/// A scripted built-in seat: whatever the closure does.
struct ScriptedSeat<F>(F);

impl<F> BuiltInSeat for ScriptedSeat<F>
where
    F: FnMut(u8, &mut Call<'_>) + Send,
{
    fn plan(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json) {
        (self.0)(seat, call);
    }
}

/// A factory handing out the instances it was given, one per seat.
#[derive(Default)]
struct Given {
    played: Vec<(u8, Box<dyn BuiltInSeat>)>,
    advised: Vec<(u8, Box<dyn Advisor>)>,
}

impl Given {
    fn plays(mut self, seat: u8, operator: impl BuiltInSeat + 'static) -> Given {
        self.played.push((seat, Box::new(operator)));
        self
    }

    fn advises(mut self, seat: u8, advisor: impl Advisor + 'static) -> Given {
        self.advised.push((seat, Box::new(advisor)));
        self
    }
}

impl Operators for Given {
    fn built_in(&mut self, seat: u8) -> Option<Box<dyn BuiltInSeat>> {
        let index = self.played.iter().position(|(held, _)| *held == seat)?;
        Some(self.played.remove(index).1)
    }

    fn advisor(&mut self, seat: u8) -> Option<Box<dyn Advisor>> {
        let index = self.advised.iter().position(|(held, _)| *held == seat)?;
        Some(self.advised.remove(index).1)
    }
}

/// A two-seat match on the golden seed, in its opening Lull, reading the
/// committed library, under a fog policy.
fn match_with(policy: FogPolicy) -> Surface {
    support::hosted_with(MATCH, policy, 2, SEGMENT_MS, 3, Some(library_folder()))
}

/// The same, fogged.
fn hosted() -> Surface {
    match_with(FogPolicy::fogged())
}

/// Mint and register the in-process seats a factory answers, with seat 0 as
/// the human's, and run them for the Lull the match is in.
fn run_seats(surface: &mut Surface, factory: Given) -> InProcessSeats {
    let mut factory = factory;
    let mut seats = InProcessSeats::open(
        surface,
        &[SeatId::new(0), SeatId::new(1)],
        Some(0),
        &mut factory,
    )
    .expect("in-process seats");
    seats.plan(surface);
    seats
}

/// One request, as a scripted operator builds it.
fn ask(method: &str, params: &str) -> Request {
    request(method, params)
}

/// A JSON string literal, for building params.
fn quote(text: &str) -> String {
    pharmakos_proto::json::write(&Json::String(text.to_owned()))
        .trim_end()
        .to_owned()
}

/// The closed-set code of a refusal, or `ok`.
fn outcome(response: &Json) -> String {
    if response.get("result").is_some() {
        return String::from("ok");
    }
    support::code(response)
}

/// An answer with everything the host's clock moves taken out -- the
/// `_status` footer, which an operator must never read (T18's rule) -- and
/// the notebook, which the operator ignores (spec section 14), rendered as
/// text.
fn without_clock_or_notes(response: &Json) -> String {
    fn strip(value: &Json) -> Json {
        match value {
            Json::Object(entries) => Json::Object(
                entries
                    .iter()
                    .filter(|(key, _)| key != "_status" && key != "notes")
                    .map(|(key, item)| (key.clone(), strip(item)))
                    .collect(),
            ),
            Json::Array(items) => Json::Array(items.iter().map(strip).collect()),
            other => other.clone(),
        }
    }
    pharmakos_proto::json::write(&strip(response))
}

/// The Hold & Build suggestion every reading advisor below makes.
fn hold_and_build(anchor: [i32; 3], why: &str) -> Suggestion {
    Suggestion {
        template_id: String::from("hold_and_build"),
        parameters: vec![SuggestedValue {
            pointer: String::from(
                "/declarative/route/1/interface/rows/0/add_build_target/target/anchor/voxel",
            ),
            value: format!(
                r#"{{"x":{},"y":{},"z":{}}}"#,
                anchor[0], anchor[1], anchor[2]
            ),
        }],
        why: why.to_owned(),
    }
}

/// An advisor that reads everything an advisor may read, writes down every
/// answer (clock and notebook stripped), and advises from what it read.
///
/// Its suggestion is built from its own seat's first beacon, so the advice is
/// a function of the answers and nothing else -- which is what the two
/// "function of its own calls" tests compare.
fn reading_advisor(transcript: Arc<Mutex<Vec<String>>>) -> impl Advisor {
    ScriptedAdvisor(move |seat: u8, call: &mut Call<'_>| {
        let note = |label: &str, response: &Json| {
            if let Ok(mut held) = transcript.lock() {
                held.push(format!("{label}\n{}", without_clock_or_notes(response)));
            }
        };
        for (method, params) in [
            ("get_status", String::from("{}")),
            ("get_briefing", String::from("{}")),
            ("list_beacons", String::from("{}")),
            ("get_map_summary", String::from("{}")),
            ("get_economy_forecast", String::from("{}")),
            ("list_templates", String::from("{}")),
            (
                "instantiate_template",
                String::from(r#"{"template_id":"hold_and_build"}"#),
            ),
            (
                "verify_plan",
                format!(r#"{{"playbook_jsonc":{}}}"#, quote(SAFE_PLAYBOOK)),
            ),
            (
                "render_plan",
                format!(r#"{{"playbook_jsonc":{}}}"#, quote(SAFE_PLAYBOOK)),
            ),
            ("get_safe_plan", String::from("{}")),
            ("get_view", String::from("{}")),
            // Off the allow-list: refused, and the refusal is part of what it
            // was told.
            ("list_drafts", String::from("{}")),
        ] {
            let response = call(&ask(method, &params));
            note(method, &response);
        }
        let beacons = call(&ask("list_beacons", "{}"));
        let first = beacons
            .get("result")
            .and_then(|answer| match answer.get("beacons") {
                Some(Json::Array(rows)) => rows.first().cloned(),
                _ => None,
            })
            .and_then(|row| row.get("at").cloned())
            .map_or([0, 0, 0], |at| {
                let axis = |name: &str| match at.get(name) {
                    Some(Json::Number(lexeme)) => lexeme.parse::<i32>().unwrap_or(0),
                    _ => 0,
                };
                [axis("x"), axis("y"), axis("z")]
            });
        Advice {
            safe_playbook_jsonc: String::from(SAFE_PLAYBOOK),
            suggestions: vec![hold_and_build(
                first,
                &format!("Seat {seat}'s first beacon is where the Generator goes."),
            )],
        }
    })
}

/// An advisor that advises a fixed thing and calls nothing.
fn fixed_advisor(advice: Advice) -> impl Advisor {
    ScriptedAdvisor(move |_seat: u8, _call: &mut Call<'_>| advice.clone())
}

/// The safe playbook with its flee rule's wait changed, so two seats' safe
/// playbooks can be told apart by one number.
fn safe_with_wait(ms: u32) -> String {
    SAFE_PLAYBOOK.replace("\"ms\": 8000", &format!("\"ms\": {ms}"))
}

/// What a seat's private store holds of its advice.
fn advice_of(surface: &Surface, seat: u8) -> Option<pharmakos_gateway::surface::Advised> {
    surface
        .seat_state(Subject::Seat(SeatId::new(seat)), SeatId::new(seat))
        .expect("its own")
        .advice
        .clone()
}

// ---------------------------------------------------------------------------
// The wizard's suggestion
// ---------------------------------------------------------------------------

/// Decision C2: `instantiate_template{suggested}` lists every declared
/// parameter, in the template's order, with the value applied and where it
/// came from, and carries the operator's "why".
///
/// This test also writes the `instantiate_suggested` golden: the wire answer
/// T19's wizard reads, which its run-2 pull request copies byte for byte into
/// `godot/fixtures/`.
#[test]
fn a_suggested_instantiation_reports_every_declared_parameter_in_order() {
    let mut surface = hosted();
    let human = support::seat_token(&mut surface, 0);
    let _seats = run_seats(
        &mut surface,
        Given::default().advises(
            0,
            fixed_advisor(Advice {
                safe_playbook_jsonc: String::from(SAFE_PLAYBOOK),
                suggestions: vec![hold_and_build(
                    [150, 13, 118],
                    "The heat vent nearest your core is inside its sphere.",
                )],
            }),
        ),
    );

    let response = support::call(
        &mut surface,
        &human,
        "instantiate_template",
        r#"{"template_id":"hold_and_build","suggested":true}"#,
    );
    let answer = result(&response, "instantiate_template{suggested}");
    let filled = support::array_of(&answer, "parameters");
    let seen: Vec<(String, String, bool)> = filled
        .iter()
        .map(|parameter| {
            (
                text_of(parameter, "pointer"),
                text_of(parameter, "value"),
                parameter.get("suggested") == Some(&Json::Bool(true)),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (
                String::from(
                    "/declarative/route/1/interface/rows/0/add_build_target/target/anchor/voxel"
                ),
                String::from(r#"{"x":150,"y":13,"z":118}"#),
                true,
            ),
            (
                String::from("/declarative/route/2/hold/ms"),
                String::from("30000"),
                false,
            ),
        ],
        "every declared parameter, in the template's order: the suggestion where there is one, \
         the template's own value where there is not"
    );
    assert!(
        filled
            .iter()
            .all(|parameter| !text_of(parameter, "label").is_empty()),
        "each carries the template's label"
    );
    assert_eq!(
        text_of(&answer, "why"),
        "The heat vent nearest your core is inside its sphere."
    );
    let playbook = text_of(&answer, "playbook_jsonc");
    assert!(playbook.contains(r#""x":150,"y":13,"z":118"#), "{playbook}");
    assert!(
        !playbook.contains("\"parameters\""),
        "a playbook carries no declaration"
    );

    // Asked without `suggested`, the same call is the template's own values
    // and no why.
    let plain = result(
        &support::call(
            &mut surface,
            &human,
            "instantiate_template",
            r#"{"template_id":"hold_and_build"}"#,
        ),
        "instantiate_template",
    );
    assert_eq!(text_of(&plain, "why"), "");
    assert!(
        support::array_of(&plain, "parameters")
            .iter()
            .all(|parameter| parameter.get("suggested") == Some(&Json::Bool(false)))
    );

    write_golden(&rpc::render(&response));
}

/// The golden the wizard's fixture is copied from.
fn write_golden(rendered: &str) {
    let case = "instantiate_suggested";
    let file = "response.json";
    assert!(!rendered.contains('\r') && rendered.ends_with('\n'));
    let fresh = support::target_dir()
        .join("golden")
        .join("gateway")
        .join(case);
    std::fs::create_dir_all(&fresh).expect("the fresh output's folder");
    std::fs::write(fresh.join(format!("actual.{file}")), rendered).expect("the fresh output");
    let committed = support::workspace_root()
        .join("tests")
        .join("golden")
        .join("gateway")
        .join(case)
        .join(format!("expected.{file}"));
    let Ok(expected) = std::fs::read_to_string(&committed) else {
        panic!(
            "no committed golden at {}. Run `cargo xtask golden --bless` once the fresh output \
             is right.",
            committed.display()
        );
    };
    assert_eq!(
        expected,
        rendered,
        "the golden at {} moved; T19's wizard fixture is a byte-for-byte copy of it, so the pull \
         request has to say which behaviour changed.",
        committed.display()
    );
}

/// An explicit value beats a suggestion, always.
#[test]
fn an_explicit_parameter_beats_a_suggestion() {
    let mut surface = hosted();
    let human = support::seat_token(&mut surface, 0);
    let _seats = run_seats(
        &mut surface,
        Given::default().advises(
            0,
            fixed_advisor(Advice {
                safe_playbook_jsonc: String::from(SAFE_PLAYBOOK),
                suggestions: vec![hold_and_build([150, 13, 118], "Because.")],
            }),
        ),
    );
    let answer = result(
        &support::call(
            &mut surface,
            &human,
            "instantiate_template",
            r#"{"template_id":"hold_and_build","suggested":true,"parameters":[
                 {"name":"/declarative/route/1/interface/rows/0/add_build_target/target/anchor/voxel",
                  "value":"{\"x\":7,\"y\":8,\"z\":9}"}]}"#,
        ),
        "instantiate_template",
    );
    let first = support::array_of(&answer, "parameters")
        .first()
        .cloned()
        .expect("the first declaration");
    assert_eq!(text_of(&first, "value"), r#"{"x":7,"y":8,"z":9}"#);
    assert_eq!(first.get("suggested"), Some(&Json::Bool(false)));
    let playbook = text_of(&answer, "playbook_jsonc");
    assert!(!playbook.contains("150"), "the suggestion lost: {playbook}");
    assert_eq!(
        text_of(&answer, "why"),
        "Because.",
        "the why is the suggestion's, still shown beside the player's own value"
    );
}

// ---------------------------------------------------------------------------
// Secrecy of the advice
// ---------------------------------------------------------------------------

/// Seat 1 asks for its safe playbook and its suggestion and is told its own,
/// never seat 0's -- and the other way round -- under both fog policies; a
/// subject that is not a seat, a nofog spectator included, is told neither.
#[test]
fn advice_for_one_seat_never_reaches_another() {
    let advice_for = |seat: u8| Advice {
        safe_playbook_jsonc: safe_with_wait(9_000_u32.saturating_add(u32::from(seat))),
        suggestions: vec![hold_and_build(
            [100_i32.saturating_add(i32::from(seat)), 12, 100],
            &format!("For seat {seat} alone."),
        )],
    };
    for policy in [FogPolicy::fogged(), FogPolicy::casual()] {
        let mut surface = match_with(policy.clone());
        let _seats = run_seats(
            &mut surface,
            Given::default()
                .advises(0, fixed_advisor(advice_for(0)))
                .advises(1, fixed_advisor(advice_for(1))),
        );

        for seat in [0_u8, 1] {
            let token = support::seat_token(&mut surface, seat);
            let other = 1_u8.saturating_sub(seat);
            let safe = text_of(
                &result(
                    &support::call(&mut surface, &token, "get_safe_plan", "{}"),
                    "get_safe_plan",
                ),
                "playbook_jsonc",
            );
            assert_eq!(
                safe,
                advice_for(seat).safe_playbook_jsonc,
                "{policy:?}: seat {seat} is shown its own"
            );
            assert_ne!(safe, advice_for(other).safe_playbook_jsonc);

            let made = result(
                &support::call(
                    &mut surface,
                    &token,
                    "instantiate_template",
                    r#"{"template_id":"hold_and_build","suggested":true}"#,
                ),
                "instantiate_template",
            );
            assert_eq!(text_of(&made, "why"), format!("For seat {seat} alone."));
            let value = text_of(
                &support::array_of(&made, "parameters")
                    .first()
                    .cloned()
                    .expect("a parameter"),
                "value",
            );
            assert_eq!(
                value,
                format!(r#"{{"x":{},"y":12,"z":100}}"#, 100_u8.saturating_add(seat)),
                "{policy:?}: seat {seat}'s own suggestion"
            );
        }

        // And nobody who is not a seat has a safe playbook or a suggestion:
        // the lobby's token does not even hold `plan`, and a spectator that
        // sees through the fog holds neither `plan` nor a seat.
        let lobby = support::admin_token(&mut surface);
        assert_eq!(
            outcome(&support::call(&mut surface, &lobby, "get_safe_plan", "{}")),
            "FORBIDDEN_SCOPE"
        );
        let spectator = support::spectator_token(&mut surface, true);
        assert_eq!(
            outcome(&support::call(
                &mut surface,
                &spectator,
                "get_safe_plan",
                "{}"
            )),
            "FORBIDDEN_SCOPE",
            "{policy:?}: a nofog spectator has no safe playbook"
        );
        assert_eq!(
            outcome(&support::call(
                &mut surface,
                &spectator,
                "instantiate_template",
                r#"{"template_id":"hold_and_build","suggested":true}"#,
            )),
            "FORBIDDEN_SCOPE",
            "{policy:?}: nor a suggestion"
        );
    }
}

/// The operator ignores the notebook (spec section 14) and never reads the
/// drafts (the allow-list): whatever the seat's own notebook and drafts hold,
/// the advice and everything the advisor was told are byte for byte the same.
#[test]
fn a_seats_advice_is_the_same_whatever_its_own_notebook_and_drafts_hold() {
    let run = |write: bool| -> (Vec<String>, Option<pharmakos_gateway::surface::Advised>) {
        let mut surface = hosted();
        let human = support::seat_token(&mut surface, 0);
        if write {
            let _ = result(
                &support::call(
                    &mut surface,
                    &human,
                    "save_notes",
                    r#"{"notes":"the east is a trap; do not expand"}"#,
                ),
                "save_notes",
            );
            let _ = result(
                &support::call(
                    &mut surface,
                    &human,
                    "save_draft",
                    &format!(
                        r#"{{"label":"secret plan","playbook_jsonc":{}}}"#,
                        quote(&safe_with_wait(12_000))
                    ),
                ),
                "save_draft",
            );
        }
        let transcript = Arc::new(Mutex::new(Vec::new()));
        let _seats = run_seats(
            &mut surface,
            Given::default().advises(0, reading_advisor(Arc::clone(&transcript))),
        );
        let told = transcript.lock().expect("the transcript").clone();
        (told, advice_of(&surface, 0))
    };
    let (quiet, quiet_advice) = run(false);
    let (noisy, noisy_advice) = run(true);
    assert!(quiet_advice.is_some(), "the advice was filed");
    assert_eq!(quiet.len(), 12, "every call the advisor made was answered");
    // Answered, not rate-limited: the advisor's thirteen calls land on one
    // Lull tick, past a socket's CALLS_PER_TICK, so this holds only because
    // its token counts against IN_PROCESS_LIMITS.
    // Twelve written down, and the `list_beacons` it advises from.
    let calls = u32::try_from(quiet.len().saturating_add(1)).expect("fits");
    assert!(
        calls > pharmakos_gateway::limit::CALLS_PER_TICK,
        "the round is longer than a socket's per-tick cap, so this pins the in-process rate"
    );
    for line in &quiet {
        if line.starts_with("list_drafts") {
            continue;
        }
        assert!(
            line.contains("\"result\"") && !line.contains("RATE_LIMITED"),
            "an allowed call is answered under the in-process rate: {line}"
        );
    }
    assert!(
        quiet
            .iter()
            .any(|line| line.starts_with("list_drafts") && line.contains("FORBIDDEN_SCOPE")),
        "the drafts were refused to it: {quiet:?}"
    );
    assert_eq!(quiet, noisy, "what the advisor was told");
    assert_eq!(quiet_advice, noisy_advice, "what it advised");
}

/// The advice for seat 0 is a function of seat 0's own calls: byte for byte
/// the same whether or not seat 1 wrote drafts and a notebook first.
#[test]
fn the_advice_for_a_seat_is_a_function_of_that_seats_own_calls() {
    let run = |other_writes: bool| -> (Vec<String>, Option<pharmakos_gateway::surface::Advised>) {
        let mut surface = hosted();
        if other_writes {
            let other = support::seat_token(&mut surface, 1);
            for (method, params) in [
                ("save_notes", String::from(r#"{"notes":"seat 1's own"}"#)),
                (
                    "save_draft",
                    format!(
                        r#"{{"label":"seat 1's draft","playbook_jsonc":{}}}"#,
                        quote(&safe_with_wait(15_000))
                    ),
                ),
            ] {
                let _ = result(
                    &support::call(&mut surface, &other, method, &params),
                    method,
                );
            }
        }
        let transcript = Arc::new(Mutex::new(Vec::new()));
        let _seats = run_seats(
            &mut surface,
            Given::default().advises(0, reading_advisor(Arc::clone(&transcript))),
        );
        let told = transcript.lock().expect("the transcript").clone();
        (told, advice_of(&surface, 0))
    };
    let (alone, alone_advice) = run(false);
    let (beside, beside_advice) = run(true);
    assert!(alone_advice.is_some());
    assert_eq!(alone, beside, "what seat 0's advisor was told");
    assert_eq!(alone_advice, beside_advice, "what it advised seat 0");
}

// ---------------------------------------------------------------------------
// The built-in seat beside the advisor
// ---------------------------------------------------------------------------

/// A built-in seat's seal is the same whether or not the human's seat has an
/// advisor running beside it.
#[test]
fn a_built_in_seats_sealed_plan_is_the_same_with_and_without_an_advisor_running() {
    let player = || {
        ScriptedSeat(|_seat: u8, call: &mut Call<'_>| {
            let made = call(&ask(
                "instantiate_template",
                r#"{"template_id":"expand_and_mine"}"#,
            ));
            let playbook = made
                .get("result")
                .map(|answer| text_of(answer, "playbook_jsonc"))
                .expect("instantiated");
            let submitted = call(&ask(
                "submit_plan",
                &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&playbook)),
            ));
            assert_eq!(
                submitted
                    .get("result")
                    .and_then(|answer| answer.get("accepted")),
                Some(&Json::Bool(true)),
                "{submitted:?}"
            );
            let _ = call(&ask("set_ready", r#"{"ready":true}"#));
        })
    };
    let run = |advise: bool| {
        let mut surface = hosted();
        let mut factory = Given::default().plays(1, player());
        if advise {
            factory = factory.advises(0, reading_advisor(Arc::new(Mutex::new(Vec::new()))));
        }
        let seats = run_seats(&mut surface, factory);
        assert_eq!(seats.played_seats(), [1]);
        assert_eq!(seats.advised_seats().len(), usize::from(advise));
        let sealed = surface
            .sealed_plan(Subject::Seat(SeatId::new(1)), SeatId::new(1))
            .expect("seat 1 sealed")
            .clone();
        assert!(surface.begin_push().expect("the Push opens"));
        let in_world = surface
            .host()
            .expect("hosted")
            .world()
            .interpreter()
            .plan(1)
            .cloned();
        (sealed, in_world)
    };
    let (alone, alone_world) = run(false);
    let (beside, beside_world) = run(true);
    assert!(!alone.filed_by_the_gateway, "seat 1 sealed its own");
    assert_eq!(alone, beside, "the seal, report hash included");
    assert_eq!(alone_world, beside_world, "and what the match plays");
}

// ---------------------------------------------------------------------------
// The allow-list
// ---------------------------------------------------------------------------

/// The five methods the design names -- every write and the drafts -- are
/// answered `FORBIDDEN_SCOPE` to an advisor and audited, and nothing of the
/// seat's changes; a read on the list is answered.
#[test]
fn an_advisor_is_refused_every_method_off_its_allow_list() {
    const REFUSED: [(&str, &str); 5] = [
        ("save_notes", r#"{"notes":"written by the operator"}"#),
        ("save_draft", r#"{"label":"x","playbook_jsonc":"{}"}"#),
        ("list_drafts", "{}"),
        ("submit_plan", r#"{"playbook_jsonc":"{}"}"#),
        ("set_ready", r#"{"ready":true}"#),
    ];
    let mut surface = hosted();
    let before = surface
        .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .expect("its own")
        .clone();
    let answers = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&answers);
    let _seats = run_seats(
        &mut surface,
        Given::default().advises(
            0,
            ScriptedAdvisor(move |_seat: u8, call: &mut Call<'_>| {
                for (method, params) in REFUSED {
                    let response = call(&ask(method, params));
                    if let Ok(mut held) = kept.lock() {
                        held.push((method.to_owned(), outcome(&response)));
                    }
                }
                for method in ["get_status", "get_segment_feed", "end_lull", "query_area"] {
                    let response = call(&ask(method, "{}"));
                    if let Ok(mut held) = kept.lock() {
                        held.push((method.to_owned(), outcome(&response)));
                    }
                }
                Advice {
                    safe_playbook_jsonc: String::from(SAFE_PLAYBOOK),
                    suggestions: Vec::new(),
                }
            }),
        ),
    );

    let answers = answers.lock().expect("the answers").clone();
    let expected: Vec<(String, String)> = REFUSED
        .iter()
        .map(|(method, _)| ((*method).to_owned(), String::from("FORBIDDEN_SCOPE")))
        .chain([
            (String::from("get_status"), String::from("ok")),
            // On the list although C5 did not name it: the feed keeps no
            // per-viewer state, so reading it disturbs nobody's.
            (String::from("get_segment_feed"), String::from("ok")),
            (String::from("end_lull"), String::from("FORBIDDEN_SCOPE")),
            (String::from("query_area"), String::from("FORBIDDEN_SCOPE")),
        ])
        .collect();
    assert_eq!(answers, expected);

    // Audited: one refusal per forbidden call, against seat 0 and the call's
    // own method, before any handler ran.
    let refusals: Vec<String> = surface
        .audit()
        .entries()
        .iter()
        .filter(|entry| entry.subject == Some(Subject::Seat(SeatId::new(0))))
        .filter(|entry| entry.outcome.render() == "FORBIDDEN_SCOPE")
        .map(|entry| entry.action.clone())
        .collect();
    let mut wanted: Vec<String> = REFUSED
        .iter()
        .map(|(method, _)| format!("call {method}"))
        .collect();
    wanted.push(String::from("call end_lull"));
    wanted.push(String::from("call query_area"));
    assert_eq!(refusals, wanted);

    // And the seat's store is exactly as it was, but for the advice itself.
    let mut after = surface
        .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .expect("its own")
        .clone();
    assert!(after.advice.is_some());
    after.advice = None;
    assert_eq!(after, before);
}

// ---------------------------------------------------------------------------
// The view feed (H15)
// ---------------------------------------------------------------------------

/// An advisor's `get_view` leaves the human's view feed as it found it: the
/// human's keyframe, its next delta after a Push and the handles it is given
/// are byte for byte the same with and without an advisor having looked first.
#[test]
fn the_advisor_leaves_the_humans_view_feed_as_it_found_it() {
    let run = |advise: bool| -> Vec<String> {
        let mut surface = support::hosted_with(MATCH, FogPolicy::fogged(), 2, 2_000, 3, None);
        let human = support::seat_token(&mut surface, 0);
        let pages_seen = Arc::new(Mutex::new(0_usize));
        if advise {
            let counted = Arc::clone(&pages_seen);
            let _seats = run_seats(
                &mut surface,
                Given::default().advises(
                    0,
                    ScriptedAdvisor(move |_seat: u8, call: &mut Call<'_>| {
                        // A whole keyframe, page by page, and a delta.
                        let mut cursor = String::new();
                        for _ in 0..64 {
                            let page = call(&ask(
                                "get_view",
                                &format!(r#"{{"cursor":{}}}"#, quote(&cursor)),
                            ));
                            let answer = page.get("result").cloned().expect("a view");
                            if let Ok(mut held) = counted.lock() {
                                *held = held.saturating_add(1);
                            }
                            cursor = text_of(&answer, "next_cursor");
                            if answer.get("complete") == Some(&Json::Bool(true)) {
                                break;
                            }
                        }
                        let _ = call(&ask(
                            "get_view",
                            &format!(r#"{{"cursor":{}}}"#, quote(&cursor)),
                        ));
                        Advice {
                            safe_playbook_jsonc: String::from(SAFE_PLAYBOOK),
                            suggestions: Vec::new(),
                        }
                    }),
                ),
            );
            assert!(
                *pages_seen.lock().expect("the count") > 0,
                "the advisor read the view"
            );
        }
        let mut seen: Vec<String> = Vec::new();
        let keyframe = support::view_pages(&mut surface, &human, "");
        let mut cursor = String::new();
        for page in &keyframe {
            seen.push(pharmakos_proto::json::write(page));
            cursor = text_of(page, "next_cursor");
        }
        assert!(surface.begin_push().expect("the Push opens"));
        let _ = support::step(&mut surface, 20);
        for page in support::view_pages(&mut surface, &human, &cursor) {
            seen.push(pharmakos_proto::json::write(&page));
        }
        seen
    };
    let alone = run(false);
    let after_advice = run(true);
    assert!(alone.len() >= 2, "a keyframe and a delta");
    assert_eq!(
        alone, after_advice,
        "the human's keyframe, delta and handles"
    );
}

// ---------------------------------------------------------------------------
// The seat's own safe playbook
// ---------------------------------------------------------------------------

/// An advised safe playbook is verified FULL and compiled when it is filed.
/// One that does not qualify, or qualifies and will not compile, is replaced
/// by the gateway's fallback, and the audit log says so; one that does is what
/// `get_safe_plan` shows and what `begin_push` files.
#[test]
fn an_advised_safe_playbook_that_does_not_qualify_is_replaced_by_the_fallback_and_audited() {
    // A negative hold: the verifier's E0109.
    let refused = SAFE_PLAYBOOK.replace("\"ms\": 8000", "\"ms\": -1");
    // A queued capability structure: the verifier qualifies it and this build
    // cannot execute it (S4's).
    let uncompilable = SAFE_PLAYBOOK.replace(
        "{\"label\": \"to_safety\", \"move\": {\"to\": {\"safest\": {}}, \"pace\": \"AVOID_KNOWN_THREATS\"}}",
        "{\"label\": \"q\", \"interface\": {\"beacon\": {\"safest\": {}}, \"rows\": [{\"queue_structure\": {\"blueprint_id\": \"radio_mast\"}}]}, \"timeout_ms\": 30000}",
    );
    assert_ne!(
        uncompilable, SAFE_PLAYBOOK,
        "the replacement found its step"
    );
    let good = safe_with_wait(11_000);

    for (advised, code, own) in [
        (refused, Some("NO_QUALIFYING_PLAN"), false),
        (uncompilable, Some("INVALID_ARGUMENT"), false),
        (good.clone(), None, true),
    ] {
        let mut surface = hosted();
        let human = support::seat_token(&mut surface, 0);
        let _seats = run_seats(
            &mut surface,
            Given::default().advises(
                0,
                fixed_advisor(Advice {
                    safe_playbook_jsonc: advised.clone(),
                    suggestions: Vec::new(),
                }),
            ),
        );
        let line = surface
            .audit()
            .entries()
            .iter()
            .rev()
            .find(|entry| entry.action == "file advice")
            .cloned()
            .expect("filing is always audited");
        assert_eq!(line.subject, Some(Subject::Seat(SeatId::new(0))));
        assert_eq!(
            line.outcome.render(),
            code.unwrap_or("ok"),
            "never replaced silently"
        );

        let expected = if own {
            advised.clone()
        } else {
            String::from(SAFE_PLAYBOOK)
        };
        let shown = text_of(
            &result(
                &support::call(&mut surface, &human, "get_safe_plan", "{}"),
                "get_safe_plan",
            ),
            "playbook_jsonc",
        );
        assert_eq!(
            shown, expected,
            "what a timeout would file is what is shown"
        );

        assert!(surface.begin_push().expect("the Push opens"));
        let sealed = surface
            .sealed_plan(Subject::Seat(SeatId::new(0)), SeatId::new(0))
            .expect("something is always filed")
            .clone();
        assert!(sealed.filed_by_the_gateway, "filed on the seat's behalf");
        assert_eq!(sealed.playbook_jsonc, expected);
        assert_eq!(
            sealed.report_hash.is_empty(),
            !own,
            "an advised one carries the report that qualified it"
        );
    }
}

/// A wizard suggestion that would not instantiate is the operator's mistake,
/// never the human's: it is dropped when it is filed, with an audit line
/// naming the code, and the human's `instantiate_template{suggested}` answers
/// the template's own values instead of failing for the rest of the round.
#[test]
fn a_suggestion_that_would_not_instantiate_is_dropped_and_audited() {
    let not_json = Suggestion {
        template_id: String::from("hold_and_build"),
        parameters: vec![SuggestedValue {
            pointer: String::from("/declarative/route/2/hold/ms"),
            value: String::from("30s"),
        }],
        why: String::from("Not JSON."),
    };
    let nowhere = Suggestion {
        template_id: String::from("no_such_template"),
        parameters: Vec::new(),
        why: String::from("No such template."),
    };
    let good = hold_and_build([7, 12, 9], "A good one.");
    for (suggestion, code) in [
        (not_json, Some("INVALID_ARGUMENT")),
        (nowhere, Some("NOT_FOUND")),
        (good, None),
    ] {
        let mut surface = hosted();
        let human = support::seat_token(&mut surface, 0);
        let _seats = run_seats(
            &mut surface,
            Given::default().advises(
                0,
                fixed_advisor(Advice {
                    safe_playbook_jsonc: String::from(SAFE_PLAYBOOK),
                    suggestions: vec![suggestion.clone()],
                }),
            ),
        );
        let lines: Vec<(Option<Subject>, String)> = surface
            .audit()
            .entries()
            .iter()
            .filter(|entry| entry.action == "file advice suggestion")
            .map(|entry| (entry.subject, entry.outcome.render()))
            .collect();
        let filed = advice_of(&surface, 0).expect("the advice was filed");
        let made = result(
            &support::call(
                &mut surface,
                &human,
                "instantiate_template",
                r#"{"template_id":"hold_and_build","suggested":true}"#,
            ),
            "instantiate_template",
        );
        let parameters = support::array_of(&made, "parameters");
        let any_suggested = parameters
            .iter()
            .any(|parameter| parameter.get("suggested") == Some(&Json::Bool(true)));
        if let Some(code) = code {
            assert_eq!(
                lines,
                vec![(Some(Subject::Seat(SeatId::new(0))), String::from(code))],
                "{}: dropped, and never silently",
                suggestion.why
            );
            assert!(filed.advice.suggestions.is_empty(), "{}", suggestion.why);
            assert_eq!(text_of(&made, "why"), "", "{}", suggestion.why);
            assert!(
                !any_suggested,
                "{}: the template's own values",
                suggestion.why
            );
        } else {
            assert!(lines.is_empty(), "a good suggestion is filed as it is");
            assert_eq!(filed.advice.suggestions, vec![suggestion.clone()]);
            assert_eq!(text_of(&made, "why"), "A good one.");
            assert!(any_suggested, "the good suggestion is applied");
        }
    }
}

// ---------------------------------------------------------------------------
// The in-process rate (C6, H8)
// ---------------------------------------------------------------------------

/// An in-process seat finishes a round of many calls on one Lull tick under
/// its own limits -- counted, audited and capped there too -- and a socket
/// token on the same tick is still held to the default.
#[test]
fn an_in_process_seat_finishes_a_round_under_its_limits_and_a_socket_does_not_get_them() {
    let limit = pharmakos_gateway::limit::IN_PROCESS_LIMITS.per_tick;
    let default = pharmakos_gateway::limit::CALLS_PER_TICK;
    let round = usize::try_from(default.saturating_mul(6)).expect("fits");
    let over = usize::try_from(limit).expect("fits");
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&outcomes);
    let mut surface = hosted();
    let tick = surface.time().tick;
    let _seats = run_seats(
        &mut surface,
        Given::default().plays(
            1,
            ScriptedSeat(move |_seat: u8, call: &mut Call<'_>| {
                // A round six times a socket's per-tick budget, then on to the
                // in-process cap and one past it.
                for _ in 0..=over {
                    let answered = outcome(&call(&ask("get_status", "{}")));
                    if let Ok(mut held) = kept.lock() {
                        held.push(answered);
                    }
                }
            }),
        ),
    );
    assert_eq!(surface.time().tick, tick, "every call landed on one tick");
    let outcomes = outcomes.lock().expect("the outcomes").clone();
    assert!(
        outcomes.iter().take(round).all(|answer| answer == "ok"),
        "a round of {round} calls finishes: {outcomes:?}"
    );
    assert_eq!(
        outcomes.iter().filter(|answer| *answer == "ok").count(),
        over,
        "counted against its own cap of {limit} per tick"
    );
    assert_eq!(outcomes.last().map(String::as_str), Some("RATE_LIMITED"));
    let audited = surface
        .audit()
        .entries()
        .iter()
        .filter(|entry| entry.subject == Some(Subject::Seat(SeatId::new(1))))
        .count();
    assert_eq!(audited, over.saturating_add(1), "every call is audited");

    // A socket's token, on the same tick, is held to the default.
    let socket = support::seat_token(&mut surface, 0);
    let answers: Vec<String> = (0..=default)
        .map(|_| outcome(&support::call(&mut surface, &socket, "get_status", "{}")))
        .collect();
    assert!(
        answers
            .iter()
            .take(usize::try_from(default).expect("fits"))
            .all(|answer| answer == "ok")
    );
    assert_eq!(answers.last().map(String::as_str), Some("RATE_LIMITED"));
}

// ---------------------------------------------------------------------------
// The operator's inputs and the meter (C7)
// ---------------------------------------------------------------------------

/// Each seat is told its own `$` and kW as the world stands, and never
/// another seat's, under both fog policies; a subject that is not a seat has
/// no economy.
#[test]
fn a_seat_is_told_its_own_economy_and_never_another_seats() {
    for policy in [FogPolicy::fogged(), FogPolicy::casual()] {
        let mut surface = support::hosted_with(MATCH, policy.clone(), 2, 2_000, 3, None);
        // Knock seat 1's beacons down and play the Push out, so that the two
        // seats' economies are not the same numbers by construction.
        support::fell(&mut surface, 1);
        assert!(surface.begin_push().expect("the Push opens"));
        let _ = support::step(&mut surface, 40);

        let mut answers: Vec<[i64; 4]> = Vec::new();
        for seat in [0_u8, 1] {
            let token = support::seat_token(&mut surface, seat);
            let answer = result(
                &support::call(&mut surface, &token, "get_economy_forecast", "{}"),
                "get_economy_forecast",
            );
            let number = |key: &str| match answer.get(key) {
                Some(Json::Number(lexeme)) => lexeme.parse::<i64>().expect("a whole number"),
                other => panic!("`{key}` is a number, and it is {other:?}"),
            };
            let told = [
                number("treasury_now"),
                number("supply_kw_now"),
                number("draw_kw_now"),
                number("headroom_kw_now"),
            ];
            let seats = surface.host().expect("hosted").world().seats();
            let row = seats
                .seats()
                .iter()
                .position(|held| *held == seat)
                .expect("a row");
            let supply = i64::from(seats.supplies().get(row).expect("supply").raw());
            let draw = i64::from(seats.draws().get(row).expect("draw").raw());
            assert_eq!(
                told,
                [
                    seats.treasuries().get(row).expect("treasury").raw(),
                    supply,
                    draw,
                    supply.saturating_sub(draw),
                ],
                "seat {seat} is told its own row, as the world stands"
            );
            answers.push(told);
        }
        assert_ne!(
            answers.first(),
            answers.get(1),
            "the two seats' economies differ, so each being told its own is a test of something"
        );

        let lobby = support::admin_token(&mut surface);
        assert_eq!(
            outcome(&support::call(
                &mut surface,
                &lobby,
                "get_economy_forecast",
                "{}"
            )),
            "FORBIDDEN_SCOPE",
            "the lobby has no economy"
        );
        let spectator = support::spectator_token(&mut surface, true);
        assert_eq!(
            outcome(&support::call(
                &mut surface,
                &spectator,
                "get_economy_forecast",
                "{}"
            )),
            "FORBIDDEN_SCOPE",
            "nor does a spectator, even one that sees through the fog"
        );
    }
}

/// A vision that sees everywhere, so a fogged match can show seat 0 another
/// seat's beacon the way a sighting will.
struct Everywhere;

impl Vision for Everywhere {
    fn sees(&self, _seat: SeatId, _at: &Voxel) -> bool {
        true
    }
}

/// Another seat's beacon carries its owner and nothing more -- under both fog
/// policies, through both methods -- while the seat's own carries core,
/// priority and power state.
#[test]
fn another_seats_beacon_carries_its_owner_and_nothing_else() {
    for policy in [FogPolicy::fogged(), FogPolicy::casual()] {
        let mut surface = match_with(policy.clone());
        let token: Token = support::seat_token(&mut surface, 0);
        let listed = result(
            &surface.call(Some(&token), &request("list_beacons", "{}"), &Everywhere),
            "list_beacons",
        );
        let rows = support::array_of(&listed, "beacons");
        assert!(rows.len() >= 2, "both seats' cores: {rows:?}");
        let (theirs, _) = support::core_beacon(&surface, 1);
        let theirs = pharmakos_gateway::view::beacon_id(theirs);
        for row in &rows {
            let keys: Vec<String> = match row {
                Json::Object(entries) => entries.iter().map(|(key, _)| key.clone()).collect(),
                other => panic!("a summary is an object, and it is {other:?}"),
            };
            let owner = text_of(row, "owner");
            if owner == "seat.0" {
                assert_eq!(
                    keys,
                    ["beacon_id", "at", "owner", "core", "priority", "powered"],
                    "{policy:?}: an own beacon"
                );
            } else {
                assert_eq!(owner, "seat.1");
                assert_eq!(
                    keys,
                    ["beacon_id", "at", "owner"],
                    "{policy:?}: another seat's beacon carries its owner and nothing else"
                );
            }
        }
        let own_core = rows
            .iter()
            .find(|row| text_of(row, "owner") == "seat.0")
            .expect("an own beacon");
        assert_eq!(own_core.get("core"), Some(&Json::Bool(true)));
        assert_eq!(text_of(own_core, "priority"), "normal");
        assert_eq!(own_core.get("powered"), Some(&Json::Bool(true)));

        let one = result(
            &surface.call(
                Some(&token),
                &request("get_beacon", &format!(r#"{{"beacon_id":"{theirs}"}}"#)),
                &Everywhere,
            ),
            "get_beacon",
        );
        let summary = one.get("summary").cloned().expect("a summary");
        assert_eq!(
            summary,
            Json::Object(vec![
                (String::from("beacon_id"), Json::String(theirs.clone())),
                (
                    String::from("at"),
                    rows.iter()
                        .find(|row| text_of(row, "beacon_id") == theirs)
                        .and_then(|row| row.get("at").cloned())
                        .expect("listed")
                ),
                (String::from("owner"), Json::String(String::from("seat.1"))),
            ]),
            "{policy:?}: get_beacon answers the same summary as the listing"
        );
    }
}
