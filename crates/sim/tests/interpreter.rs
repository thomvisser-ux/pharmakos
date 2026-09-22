// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The playbook interpreter: the transcripts, the named rules, and the
//! halting property.
//!
//! # The transcripts
//!
//! `tests/golden/interpreter/<case>/playbook.json` is a committed playbook —
//! canonical `gp.v1` JSON, one per vocabulary construct — and
//! `expected.transcript.txt` is the pinned run of it. The transcript is made of
//! the **events** the interpreter emits (item 97's assertable names), one per
//! line:
//!
//! ```text
//! <tick>\t<seq>\t<kind>\t<seat>\t<subject>\t<value>
//! ```
//!
//! Which is also how the transcript pins the **decision cadence**: every line's
//! tick is a decision tick, five apart at `match.decision_tick_ms` of 250 ms, so
//! a cadence that moved would move every tick in every transcript.
//!
//! # The property
//!
//! `every_playbook_halts` runs a generated corpus — a stream-seeded generator
//! modelled on `crates/verifier/tests/fuzz.rs`, because `proptest` is rejected
//! (item 101: it pulls three RNG crates into the crate that *defines* the RNG
//! contract). Every generated playbook has bounded durations, so a route of
//! *n* steps must reach its fallback; the test asserts that it does, and that
//! the route cursor never moves backwards on the way.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::PathBuf;

use pharmakos_proto::gp;
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::interpreter::state::{NO_INDEX, StepFailure};
use pharmakos_sim::interpreter::{Plan, PlanError, VisitState};
use pharmakos_sim::math::quantity::Hp;
use pharmakos_sim::math::random::{Stream, StreamRng};
use pharmakos_sim::runner::{DEFAULT_ROUND_LIMIT, MatchSettings, Runner};
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::world::{DamageOrder, DamageTarget};
use pharmakos_sim::{RulesTable, World, WorldConfig};

/// The committed transcript cases, in the order the golden tree holds them.
const CASES: &[&str] = &["guards", "handlers", "place_beacon", "reflex", "visit"];

/// How many ticks a transcript case plays, per case.
///
/// 300 ticks is 15 s of game time at 20 Hz, which covers a visit and its rows;
/// `place_beacon` pays a 12 s deploy before its rows even start, so it runs
/// long enough to reach its second step's commit.
fn case_ticks(case: &str) -> u32 {
    match case {
        "place_beacon" => 420,
        _ => 300,
    }
}

/// The seat every case drives. The other two seats seal nothing, which is also
/// what proves a seat with no playbook does nothing at all.
const CASE_SEAT: u8 = 0;

fn repo_root() -> PathBuf {
    let here = PathBuf::from(".");
    if here.join("rules").join("rules.v1.json").is_file() {
        return here;
    }
    PathBuf::from("..").join("..")
}

/// The cargo target directory: `<target>/<profile>/deps/<test exe>`.
fn target_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let deps = exe.parent()?;
    let profile = deps.parent()?;
    Some(profile.parent()?.to_path_buf())
}

fn rules() -> RulesTable {
    RulesTable::load(&repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

/// The world every case runs in: the determinism seed at three seats, no
/// harness walkers, one long segment.
///
/// Three seats rather than the harness's four, because three is what v1 plays
/// and what the map has zones for; no walkers, because a transcript is about
/// one commander and a hundred and fifty wandering raiders would only move the
/// chain underneath it.
fn case_world(rules: RulesTable) -> World {
    World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: 3,
        units_per_seat: 0,
        rules,
        match_settings: MatchSettings {
            segment_lengths_ms: vec![60_000],
            round_limit: DEFAULT_ROUND_LIMIT,
        },
    })
    .expect("the rules table describes a map and a broadphase grid")
}

/// One generated world, cloned per run.
///
/// Generating a 384 × 384 × 64 map takes seconds and every case and every
/// corpus entry wants the *same* one, so it is generated once and copied. A
/// clone is the whole world, so no run can see another's writes —
/// `every_playbook_halts` runs a hundred of them.
fn fresh_world() -> World {
    static BASE: std::sync::OnceLock<World> = std::sync::OnceLock::new();
    BASE.get_or_init(|| case_world(rules())).clone()
}

fn decode(text: &str) -> gp::v1::Playbook {
    json::decode(text).expect("a committed case playbook is canonical gp.v1 JSON")
}

fn compile(playbook: &gp::v1::Playbook) -> Result<Plan, PlanError> {
    Plan::compile(playbook, &rules())
}

/// The damage a case injects, as `(tick, hit points)` against the commander.
///
/// Written here rather than in the playbook because damage is something the
/// *world* does to a seat, not something a playbook can ask for — and the
/// reflex is exactly the rule that has to be driven from outside.
fn script(case: &str) -> &'static [(u32, i32)] {
    match case {
        // 240 of 300 hit points taken at tick 70 leaves the commander at 20 %,
        // which is the reflex's threshold exactly — and lands *after* the
        // visit's first row has committed, so the transcript shows a committed
        // row staying while the visit it belonged to is aborted. 30 more at
        // tick 250 is the **new damage** that lets the reflex fire a second
        // time; every decision between the two sees a commander well under
        // 20 % and does nothing.
        "reflex" => &[(70, 240), (250, 30)],
        _ => &[],
    }
}

/// Run one case and return its transcript.
fn transcript(case: &str) -> String {
    let text = std::fs::read_to_string(case_dir(case).join("playbook.json"))
        .unwrap_or_else(|error| panic!("reading {case}'s playbook: {error}"));
    let plan = compile(&decode(&text))
        .unwrap_or_else(|error| panic!("compiling {case}'s playbook: {error}"));

    let mut runner = Runner::new(fresh_world());
    assert!(
        runner
            .world_mut()
            .seal_playbook(SeatId::new(CASE_SEAT), plan),
        "the case's seat seals its playbook"
    );
    assert!(runner.begin_push(), "a match opens in a Lull");

    let mut lines = String::from("# tick\tseq\tkind\tseat\tsubject\tvalue\n");
    let damage = script(case);
    for tick in 0..case_ticks(case) {
        for (at, amount) in damage {
            if *at == tick {
                let commander = runner.world().commander_of(SeatId::new(CASE_SEAT));
                assert!(
                    runner.world_mut().request_damage(DamageOrder {
                        target: DamageTarget::Unit(commander),
                        amount: Hp::new(*amount),
                        by: SeatId::NEUTRAL,
                    }),
                    "the damage queue has room"
                );
            }
        }
        if runner.step().is_none() {
            break;
        }
        for event in runner.events() {
            lines.push_str(&line(event));
        }
        runner.clear_events();
    }
    lines
}

/// One event, as the transcript writes it.
///
/// Positions are deliberately **not** in the transcript: a transcript is about
/// what the interpreter decided, and a map change would otherwise move every
/// line of every case. Where a thing happened is in the determinism chain,
/// which is the golden that is about the world.
fn line(event: &Event) -> String {
    let subject = event
        .subject
        .map_or_else(|| "-".to_owned(), |id| id.raw().to_string());
    let seat = event
        .seat
        .map_or_else(|| "-".to_owned(), |seat| seat.raw().to_string());
    format!(
        "{}\t{}\t{}\t{seat}\t{subject}\t{}\n",
        event.tick.raw(),
        event.seq,
        event.kind.name(),
        event.value
    )
}

fn case_dir(case: &str) -> PathBuf {
    repo_root()
        .join("tests")
        .join("golden")
        .join("interpreter")
        .join(case)
}

fn write_actual(case: &str, body: &str) {
    let Some(root) = target_dir() else {
        return;
    };
    let dir = root.join("golden").join("interpreter").join(case);
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("creating {}: {error}", dir.display()));
    let path = dir.join("actual.transcript.txt");
    std::fs::write(&path, body)
        .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
}

// ---------------------------------------------------------------------------
// The transcripts
// ---------------------------------------------------------------------------

#[test]
fn every_case_produces_its_transcript() {
    for case in CASES {
        let body = transcript(case);
        write_actual(case, &body);
        let expected = case_dir(case).join("expected.transcript.txt");
        let Ok(golden) = std::fs::read_to_string(&expected) else {
            // The `golden` step is what fails a missing golden (rule 1 of
            // tests/golden/README.md); the producer's job is to produce.
            continue;
        };
        assert_eq!(
            golden.replace("\r\n", "\n"),
            body,
            "{case}: the transcript moved. tests/golden/interpreter/README.md says what a diff \
             there means; `cargo xtask golden --bless` accepts it once the pull request says why."
        );
    }
}

#[test]
fn a_transcript_is_a_record_of_decisions_five_ticks_apart() {
    // The cadence, asserted rather than assumed: `match.decision_tick_ms` is
    // 250 ms and a tick is 50 ms, so every line of every transcript falls on a
    // tick one more than a multiple of five (the first decision is the Push's
    // first played tick).
    let mut checked = 0_usize;
    for case in CASES {
        let body = transcript(case);
        for row in body.lines().skip(1) {
            // The runner's own lines — `match_started`, `push_started` — are
            // not decisions and are not on this clock, and neither is
            // `plan_sealed`, which a **host** emits during a Lull. Everything
            // after it in the catalogue is the interpreter's own.
            let kind = field(row, 2).and_then(EventKind::from_name);
            if kind.is_none_or(|kind| kind.id() <= EventKind::PlanSealed.id()) {
                continue;
            }
            let tick: u32 = field(row, 0)
                .and_then(|field| field.parse().ok())
                .unwrap_or_else(|| panic!("{case}: a transcript line starts with a tick"));
            assert_eq!(tick % 5, 1, "{case}: {row} did not land on a decision tick");
            checked = checked.saturating_add(1);
        }
    }
    assert!(
        checked > 20,
        "only {checked} interpreter lines were checked; the transcripts are empty"
    );
}

// ---------------------------------------------------------------------------
// The named rules
// ---------------------------------------------------------------------------

#[test]
fn every_wait_has_a_timeout() {
    // Half of why every playbook halts (spec section 10, "Limits and
    // termination"). The verifier rejects it at plan time; the interpreter
    // refuses it at its own door, because a wait with no timeout is a segment
    // that never ends.
    let mut playbook = minimal();
    let route = &mut playbook
        .declarative
        .as_mut()
        .expect("the minimal playbook has a body")
        .route;
    route.push(wait_step(0));
    assert_eq!(
        compile(&playbook).unwrap_err(),
        PlanError::WaitWithoutTimeout
    );

    let mut with_timeout = minimal();
    with_timeout
        .declarative
        .as_mut()
        .expect("the minimal playbook has a body")
        .route
        .push(wait_step(1_000));
    assert!(compile(&with_timeout).is_ok(), "a wait with one compiles");
}

#[test]
fn a_jump_only_goes_forward() {
    // The other half. A route step's jump is resolved at compile time and
    // refused when it names its own step or an earlier one...
    let mut playbook = minimal();
    {
        let route = &mut playbook
            .declarative
            .as_mut()
            .expect("the minimal playbook has a body")
            .route;
        route.push(hold_step("first", 1_000));
        route.push(hold_step("second", 1_000));
        route[1].on_fail = Some(gp::v1::OnFail {
            action: i32::from(gp::v1::on_fail::Action::JumpForward),
            jump_to_label: "first".to_owned(),
        });
    }
    assert_eq!(
        compile(&playbook).unwrap_err(),
        PlanError::BackwardJump { from: 1, to: 0 }
    );

    // ...and a handler's `resume_at_label` is clamped at run time, because the
    // route's cursor is not known until the rule fires. A backward resume
    // leaves the cursor where it is; it never moves it back.
    //
    // The route below is built so the clamp is actually **reached**: two one-
    // second holds put the cursor on step 2 by four seconds in, and the handler
    // does not fire until then, so its jump to "one" (route index 0) is a
    // backward target evaluated against a cursor of 2. With the clamp deleted,
    // the cursor would be set to 0 and the assertion below would fail — which
    // is the only shape in which this half of the test is load-bearing.
    let mut forward = minimal();
    {
        let body = forward
            .declarative
            .as_mut()
            .expect("the minimal playbook has a body");
        body.route = vec![
            hold_step("one", 1_000),
            hold_step("two", 1_000),
            hold_step("three", 60_000),
        ];
        body.handlers = vec![gp::v1::Handler {
            id: "back".to_owned(),
            when: Some(elapsed_at_least(4_000)),
            body: vec![hold_step("body", 1_000)],
            resume: i32::from(gp::v1::handler::Resume::JumpForward),
            resume_at_label: "one".to_owned(),
            cooldown_ms: 5_000,
            max_fires: 1,
            ..gp::v1::Handler::default()
        }];
    }
    let plan = compile(&forward).expect("a forward-resolved label compiles");
    let mut runner = Runner::new(fresh_world());
    runner
        .world_mut()
        .seal_playbook(SeatId::new(CASE_SEAT), plan);
    assert!(runner.begin_push());
    let mut lowest = u32::MAX;
    let mut seen_two = false;
    let mut body = String::new();
    for _ in 0..200 {
        if runner.step().is_none() {
            break;
        }
        for event in runner.events() {
            body.push_str(&line(event));
        }
        let cursor = cursor_of(runner.world());
        if cursor != NO_INDEX {
            assert!(
                cursor >= lowest || lowest == u32::MAX,
                "the route cursor went backwards: {lowest} then {cursor}"
            );
            lowest = cursor;
            seen_two |= cursor >= 2;
        }
        runner.clear_events();
    }
    // The clamp is only load-bearing if the rule actually fired while the
    // cursor was past its target, so both are asserted rather than assumed.
    assert!(
        body.lines().any(|row| field(row, 2) == Some("rule_fired")),
        "the handler fired, so the clamp was reached:\n{body}"
    );
    assert!(
        seen_two,
        "the route had advanced past the resume target before the rule fired:\n{body}"
    );
    assert!(
        lowest >= 2,
        "a backward resume left the cursor where it was; it ended at {lowest}:\n{body}"
    );
}

#[test]
fn the_reflex_interrupts_a_visit_and_re_fires_only_after_new_damage() {
    let body = transcript("reflex");
    let fired: Vec<u32> = ticks_of(&body, EventKind::ReflexFired);
    assert_eq!(
        fired.len(),
        2,
        "the reflex fires once per helping of new damage, and there were two:\n{body}"
    );
    // It aborted a visit: a `visit_ended` lands on the same tick as the first
    // reflex, before the visit's last row could commit.
    let aborted = body
        .lines()
        .any(|row| row.contains("visit_ended") && row.starts_with(&format!("{}\t", fired[0])));
    assert!(aborted, "the reflex aborts the visit in progress:\n{body}");
    // And it does not fire again between the two: every decision between them
    // sees a commander below 20 % and does nothing, because no *new* damage
    // arrived.
    let between = body
        .lines()
        .filter(|row| row.contains("reflex_fired"))
        .count();
    assert_eq!(between, 2, "no third firing without a third helping");
}

#[test]
fn a_resumed_visit_pays_the_handshake_again() {
    // Item 24: "a resumed visit is a new visit and pays the handshake again".
    // The reflex case's transcript holds two `visit_started` lines for one
    // interface step, and the gap between the second one and its first
    // `row_committed` is the handshake plus that row — the same gap as the
    // first visit's, paid over again.
    let body = transcript("reflex");
    let starts = ticks_of(&body, EventKind::VisitStarted);
    assert!(
        starts.len() >= 2,
        "the interrupted visit is resumed as a new visit:\n{body}"
    );
    let rows = ticks_of(&body, EventKind::RowCommitted);
    // For each visit, the gap from its start to the first row it commits. That
    // gap is the handshake (1.5 s) plus that row's own duration (a priority row
    // is 1.5 s), so it is 60 ticks — and it is 60 ticks **again** for the
    // resumed visit, which is the whole claim.
    let mut gaps: Vec<u32> = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let next = starts
            .get(index.saturating_add(1))
            .copied()
            .unwrap_or(u32::MAX);
        if let Some(row) = rows.iter().copied().find(|row| row > start && *row < next) {
            gaps.push(row.saturating_sub(*start));
        }
    }
    assert!(
        gaps.len() >= 2,
        "two visits commit a row, so two handshakes are visible:\n{body}"
    );
    assert!(
        gaps.iter().all(|gap| *gap == 60),
        "a resumed visit pays the handshake again: gaps were {gaps:?}\n{body}"
    );
}

#[test]
fn an_interface_row_commits_at_the_end_of_its_own_duration() {
    // Spec section 5's rates, read from `interface_times.*` and not from a
    // constant: the `visit` case commits `set_mandate` (8 s) and then
    // `set_priority` (1.5 s), after a 1.5 s handshake. At 20 Hz and one
    // decision every five ticks those land on the first decision tick at or
    // after 30, 190 and 220 ticks from the visit's start.
    let body = transcript("visit");
    let start = ticks_of(&body, EventKind::VisitStarted)
        .first()
        .copied()
        .expect("the visit starts");
    let rows = ticks_of(&body, EventKind::RowCommitted);
    assert_eq!(rows.len(), 2, "two rows commit:\n{body}");
    assert_eq!(
        rows[0].saturating_sub(start),
        190,
        "the handshake is 1.5 s and the mandate switch 8 s:\n{body}"
    );
    assert_eq!(
        rows[1].saturating_sub(rows[0]),
        30,
        "a priority row is 1.5 s:\n{body}"
    );
    // And the writ landed: `set_mandate` is the one row this build can write.
    let mandate = ticks_of(&body, EventKind::VisitEnded);
    assert!(!mandate.is_empty(), "the visit ends:\n{body}");
}

#[test]
fn a_broadcast_with_no_mast_fails_loudly_rather_than_doing_nothing() {
    let body = transcript("guards");
    let failed: Vec<i64> = values_of(&body, EventKind::StepFailed);
    assert!(
        failed.contains(&i64::from(StepFailure::NoMast.id())),
        "a broadcast step fails with `no_mast`, never a silent no-op:\n{body}"
    );
}

#[test]
fn a_late_bound_selector_resolves_at_step_start_and_stays_pinned() {
    // The `place_beacon` case places a second beacon and then interfaces with
    // it by id, so the transcript's `beacon_placed` subject and the following
    // `visit_started` subject are the same asset.
    let body = transcript("place_beacon");
    let placed = subjects_of(&body, EventKind::BeaconPlaced);
    let visited = subjects_of(&body, EventKind::VisitStarted);
    assert_eq!(placed.len(), 1, "one beacon is deployed:\n{body}");
    assert!(
        visited.contains(&placed[0]),
        "the deployed beacon is the one the next step interfaces with:\n{body}"
    );
}

#[test]
fn a_construct_this_build_cannot_execute_is_refused_rather_than_skipped() {
    // AGENTS.md §12: don't invent rules. Every one of these is in the v1
    // vocabulary and none of them has an effect until the stage named in
    // `PlanError::NotAtThisStage`, so the interpreter refuses the playbook at
    // seal time rather than treating the construct as true, false or absent.
    let cases: [(&str, gp::v1::Step); 2] = [
        (
            "queue_structure",
            interface_step(
                "q",
                beacon_id("b_00"),
                vec![gp::v1::InterfaceRow {
                    row: Some(gp::v1::interface_row::Row::QueueStructure(
                        gp::v1::QueueStructureRow {
                            blueprint_id: "radio_mast".to_owned(),
                        },
                    )),
                }],
            ),
        ),
        (
            "most_threatened",
            gp::v1::Step {
                label: "m".to_owned(),
                kind: Some(gp::v1::step::Kind::Move(gp::v1::MoveStep {
                    to: Some(gp::v1::Location {
                        place: Some(gp::v1::location::Place::BeaconAnchor(gp::v1::BeaconRef {
                            r#ref: Some(gp::v1::beacon_ref::Ref::MostThreatened(
                                gp::v1::MostThreatened { filter: None },
                            )),
                        })),
                    }),
                    pace: i32::from(gp::v1::move_step::Pace::Direct),
                })),
                ..gp::v1::Step::default()
            },
        ),
    ];
    for (name, step) in cases {
        let mut playbook = minimal();
        playbook
            .declarative
            .as_mut()
            .expect("the minimal playbook has a body")
            .route
            .push(step);
        let error = compile(&playbook).unwrap_err();
        assert!(
            matches!(error, PlanError::NotAtThisStage { .. }),
            "{name} is refused with the stage that fills it, not with {error}"
        );
    }
}

#[test]
fn every_resume_and_every_condition_shape_is_in_a_committed_transcript() {
    // The plan's acceptance line asks for one committed playbook per
    // vocabulary construct. `all`, `any` and `not` are three separate tree
    // shapes and `resume` names four separate things, so the `handlers` case
    // carries one rule of each and the transcript pins what each one did.
    let text = std::fs::read_to_string(case_dir("handlers").join("playbook.json"))
        .expect("the handlers case is committed");
    for construct in ["\"all\"", "\"any\"", "\"not\"", "SKIP_STEP", "JUMP_FORWARD"] {
        assert!(
            text.contains(construct),
            "no committed playbook writes {construct}"
        );
    }

    let body = transcript("handlers");
    // Every resume, by its `rule_ended` value (`Resume::id`): 1 CONTINUE,
    // 2 SKIP_STEP, 3 JUMP_FORWARD, 4 END_ROUTE.
    let ended = values_of(&body, EventKind::RuleEnded);
    for (id, name) in [
        (1, "CONTINUE"),
        (2, "SKIP_STEP"),
        (3, "JUMP_FORWARD"),
        (4, "END_ROUTE"),
    ] {
        assert!(
            ended.contains(&id),
            "no rule ended with {name}; the transcript was:\n{body}"
        );
    }
    // h1's `max_fires` of 2 is **reached**, so the ceiling is pinned rather
    // than merely written down.
    let fired = values_of(&body, EventKind::RuleFired);
    assert_eq!(
        fired.iter().filter(|value| **value == 0).count(),
        2,
        "h1 fires its full two times:\n{body}"
    );
    // And the second of those lands after the route has ended: handlers are
    // evaluated in the guaranteed tail too (spec section 10 lists rules and
    // the tail as two things a playbook has and does not say one ends the
    // other).
    let tail = ticks_of(&body, EventKind::FallbackEngaged)
        .first()
        .copied()
        .expect("the route ends within the case's ticks");
    let late = body
        .lines()
        .filter(|row| field(row, 2) == Some(EventKind::RuleFired.name()))
        .filter_map(|row| field(row, 0).and_then(|tick| tick.parse::<u32>().ok()))
        .any(|tick| tick > tail);
    assert!(late, "a handler fired after the fallback engaged:\n{body}");
}

#[test]
fn every_step_failure_has_a_unique_id_and_is_listed_in_order() {
    // `StepFailure::ALL` claims to be in ascending id order and nothing read
    // it, so it drifted. A scenario file and a transcript both read these ids,
    // and an id that moved would silently re-point every committed assertion.
    let mut last = 0_u8;
    for failure in StepFailure::ALL {
        assert!(
            failure.id() > last,
            "{} has id {} after {last}; StepFailure::ALL is in ascending id order and the ids are \
             unique",
            failure.name(),
            failure.id()
        );
        last = failure.id();
    }
}

#[test]
fn damage_does_not_interrupt_an_interface_row() {
    // Spec section 5, and the module's own contract: "a visit stops only when
    // the commander dies, the reflex fires, the beacon is destroyed, or the
    // commander leaves range". Damage that leaves the commander **above** the
    // reflex's 20 % is on none of those lists, so the rows commit on exactly
    // the schedule they would have without it.
    let clean = rows_of_the_visit_case(&[]);
    // 120 of 300 hit points leaves 60 %, and it lands inside the 8 s mandate
    // row — the longest row spec section 5 prices — so a row that could be
    // interrupted would be.
    let hurt = rows_of_the_visit_case(&[(100, 120)]);
    assert_eq!(
        clean, hurt,
        "damage above the reflex threshold neither delayed nor cancelled a row"
    );
    assert_eq!(clean.len(), 2, "the visit case commits two rows: {clean:?}");
}

/// The ticks the `visit` case's rows commit on, under a damage script.
fn rows_of_the_visit_case(damage: &[(u32, i32)]) -> Vec<u32> {
    let text = std::fs::read_to_string(case_dir("visit").join("playbook.json"))
        .expect("the visit case is committed");
    let plan = compile(&decode(&text)).expect("it compiles");
    let mut runner = Runner::new(fresh_world());
    runner
        .world_mut()
        .seal_playbook(SeatId::new(CASE_SEAT), plan);
    assert!(runner.begin_push());
    let mut ticks: Vec<u32> = Vec::new();
    for tick in 0..300 {
        for (at, amount) in damage {
            if *at == tick {
                let commander = runner.world().commander_of(SeatId::new(CASE_SEAT));
                assert!(
                    runner.world_mut().request_damage(DamageOrder {
                        target: DamageTarget::Unit(commander),
                        amount: Hp::new(*amount),
                        by: SeatId::NEUTRAL,
                    }),
                    "the damage queue has room"
                );
            }
        }
        if runner.step().is_none() {
            break;
        }
        for event in runner.events() {
            if event.kind == EventKind::RowCommitted {
                ticks.push(event.tick.raw());
            }
        }
        runner.clear_events();
    }
    ticks
}

#[test]
fn a_move_to_a_voxel_arrives_on_the_ground_under_it() {
    // The router puts a walker on a **column**: it routes to `node_at`, which
    // discards `z`, and stands the unit at the terrain height. Arrival is a
    // squared distance over all three axes, so a target left at the author's
    // own `z` could only ever be reached when that `z` happened to fall within
    // `arrive_radius_voxels` (2) of the ground. The spec's worked playbook
    // writes `z: 62` in a map 64 voxels tall, which no ground column comes
    // near — so before the target was projected onto its column, a `move` to a
    // voxel could not complete at all, and the same comparison is what advances
    // a `patrol` leg.
    //
    // The step below names a column three voxels away and a `z` of 63, the top
    // of the map. It completes because the ground under it is the answer.
    let world = fresh_world();
    let commander = world.commander_of(SeatId::new(CASE_SEAT));
    let index = usize::try_from(commander.raw()).expect("a unit index");
    let at = world.units().positions()[index];
    let (x, y) = (at[0].floor_voxels() + 3, at[1].floor_voxels());

    let mut playbook = minimal();
    playbook
        .declarative
        .as_mut()
        .expect("the minimal playbook has a body")
        .route
        .push(gp::v1::Step {
            label: "over_there".to_owned(),
            timeout_ms: 30_000,
            kind: Some(gp::v1::step::Kind::Move(gp::v1::MoveStep {
                to: Some(gp::v1::Location {
                    place: Some(gp::v1::location::Place::Voxel(gp::v1::Voxel {
                        x,
                        y,
                        z: 63,
                    })),
                }),
                pace: i32::from(gp::v1::move_step::Pace::Direct),
            })),
            ..gp::v1::Step::default()
        });
    let plan = compile(&playbook).expect("a voxel inside the map compiles");
    let mut runner = Runner::new(world);
    runner
        .world_mut()
        .seal_playbook(SeatId::new(CASE_SEAT), plan);
    assert!(runner.begin_push());
    let mut body = String::new();
    for _ in 0..400 {
        if runner.step().is_none() {
            break;
        }
        for event in runner.events() {
            body.push_str(&line(event));
        }
        runner.clear_events();
    }
    assert!(
        !values_of(&body, EventKind::StepFailed).contains(&i64::from(StepFailure::Timeout.id())),
        "the move did not time out:\n{body}"
    );
    assert!(
        body.lines()
            .any(|row| field(row, 2) == Some(EventKind::StepCompleted.name())),
        "a move to a voxel completes when the commander reaches the ground under it:\n{body}"
    );
}

#[test]
fn a_move_a_commander_cannot_walk_fails_with_no_path() {
    // The interpreter's half of "blocked by walls, craters and trenches": the
    // router parks a walled-in walker (item 60, "park and report"), and a
    // `move` step that finds its walker parked fails with `no_path` so that
    // `on_fail` decides. Without this, such a step would run until its timeout
    // with nothing in the feed to say why.
    let mut playbook = minimal();
    {
        let route = &mut playbook
            .declarative
            .as_mut()
            .expect("the minimal playbook has a body")
            .route;
        route.push(gp::v1::Step {
            label: "far".to_owned(),
            timeout_ms: 600_000,
            on_fail: Some(gp::v1::OnFail {
                action: i32::from(gp::v1::on_fail::Action::Skip),
                jump_to_label: String::new(),
            }),
            kind: Some(gp::v1::step::Kind::Move(gp::v1::MoveStep {
                to: Some(gp::v1::Location {
                    place: Some(gp::v1::location::Place::Voxel(gp::v1::Voxel {
                        x: 200,
                        y: 200,
                        z: 20,
                    })),
                }),
                pace: i32::from(gp::v1::move_step::Pace::Direct),
            })),
            ..gp::v1::Step::default()
        });
    }
    let plan = compile(&playbook).expect("a move to a voxel inside the map compiles");
    let mut runner = Runner::new(fresh_world());
    // Seal first, then wall the commander in, so the step is already running
    // when the graph says there is nowhere to go.
    runner
        .world_mut()
        .seal_playbook(SeatId::new(CASE_SEAT), plan);
    assert!(runner.begin_push());
    wall_in_the_commander(runner.world_mut());
    let mut body = String::new();
    for _ in 0..200 {
        if runner.step().is_none() {
            break;
        }
        for event in runner.events() {
            body.push_str(&line(event));
        }
        runner.clear_events();
    }
    let failures = values_of(&body, EventKind::StepFailed);
    assert!(
        failures.contains(&i64::from(StepFailure::NoPath.id())),
        "a sealed-in commander's move fails with `no_path`, not with a silent wait:\n{body}"
    );
}

/// Wall the case seat's commander in: a solid voxel five above each of its
/// eight neighbours' surfaces.
///
/// The same shape `tests/pathing.rs`'s `a_sealed_in_walker_parks_and_is_re_armed
/// _when_the_graph_changes` uses, applied to a commander instead of a drone.
/// The columns stay walkable; every step out of the middle one becomes a
/// five-voxel climb, which the one-voxel rule refuses.
fn wall_in_the_commander(world: &mut World) {
    let commander = world.commander_of(SeatId::new(CASE_SEAT));
    let index = usize::try_from(commander.raw()).expect("a unit index");
    let at = world.units().positions()[index];
    let x = at[0].floor_voxels();
    let y = at[1].floor_voxels();
    for dy in -1..=1_i32 {
        for dx in -1..=1_i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let top = world
                .voxels()
                .top_solid_z(x + dx, y + dy)
                .expect("a column with a floor");
            assert!(world.request_voxel_edit(VoxelEdit::Set {
                at: [x + dx, y + dy, top + 5],
                material: Material::STONE,
            }));
        }
    }
}

#[test]
fn a_playbook_reads_only_its_own_seats_beacons() {
    // Spec section 10: "conditions read only the seat's knowledge store", and
    // its predicate families list *own* beacon. A fixed `b_NN` is the one
    // selector that skips the ranking, so it is the one that could answer from
    // another seat's live state — and there is no knowledge store for it to be
    // answering from. An interface with somebody else's beacon is told apart
    // from an unresolvable one: `not_own`, because beacon capture is out of v1.
    let mut playbook = minimal();
    {
        let route = &mut playbook
            .declarative
            .as_mut()
            .expect("the minimal playbook has a body")
            .route;
        // b_01 is seat 1's core in a three-seat match; the case seat is 0.
        route.push(interface_step(
            "theirs",
            beacon_id("b_01"),
            vec![gp::v1::InterfaceRow {
                row: Some(gp::v1::interface_row::Row::SetPriority(i32::from(
                    gp::v1::interface_row::QuartermasterPriority::Low,
                ))),
            }],
        ));
    }
    let plan = compile(&playbook).expect("the reference compiles; it is a run-time refusal");
    let mut runner = Runner::new(fresh_world());
    runner
        .world_mut()
        .seal_playbook(SeatId::new(CASE_SEAT), plan);
    assert!(runner.begin_push());
    let mut body = String::new();
    for _ in 0..40 {
        if runner.step().is_none() {
            break;
        }
        for event in runner.events() {
            body.push_str(&line(event));
        }
        runner.clear_events();
    }
    assert!(
        values_of(&body, EventKind::StepFailed).contains(&i64::from(StepFailure::NotOwn.id())),
        "interfacing with another seat's beacon fails with `not_own`:\n{body}"
    );
}

#[test]
fn a_voxel_outside_the_map_is_refused_rather_than_walked_to_the_origin() {
    // The map is 384 x 384 x 64, and a coordinate outside it is not a place.
    // Before this check the executor's `i16` conversion answered `x: 40000`
    // with `x: 0` — a legal spot inside the map that the commander then walked
    // to and reported arriving at (AGENTS.md §12: the confident wrong answer).
    for voxel in [
        gp::v1::Voxel {
            x: 40_000,
            y: 5,
            z: 5,
        },
        gp::v1::Voxel { x: 5, y: 5, z: -1 },
        gp::v1::Voxel { x: 5, y: 5, z: 64 },
    ] {
        let mut playbook = minimal();
        playbook
            .declarative
            .as_mut()
            .expect("the minimal playbook has a body")
            .route
            .push(gp::v1::Step {
                label: "far".to_owned(),
                kind: Some(gp::v1::step::Kind::Move(gp::v1::MoveStep {
                    to: Some(gp::v1::Location {
                        place: Some(gp::v1::location::Place::Voxel(voxel)),
                    }),
                    pace: i32::from(gp::v1::move_step::Pace::Direct),
                })),
                ..gp::v1::Step::default()
            });
        assert!(
            matches!(
                compile(&playbook).unwrap_err(),
                PlanError::VoxelOutOfMap { .. }
            ),
            "{voxel:?} is outside the map and is refused at the door"
        );
    }
}

#[test]
fn the_plans_are_not_in_the_state_encoding() {
    // A playbook is an **input**, exactly like the rules table: the sim is a
    // pure function of (map seed, playbooks, rules hash), and an input is not
    // hashed. What *is* hashed is where the plan has got to, which is why the
    // two worlds below diverge the moment one of them takes a decision.
    let bare = fresh_world();
    let mut sealed = fresh_world();
    let plan = compile(&minimal()).expect("the minimal playbook compiles");
    sealed.seal_playbook(SeatId::new(CASE_SEAT), plan);
    assert_ne!(
        bare.state_hash(),
        sealed.state_hash(),
        "sealing writes the seat's execution state, which is hashed"
    );

    // And the state's *shape* does not depend on the plan's contents beyond its
    // handler count: two different plans with the same handler count seal to
    // the same opening state, because a cursor at zero is a cursor at zero.
    let mut one = fresh_world();
    let mut two = fresh_world();
    one.seal_playbook(
        SeatId::new(CASE_SEAT),
        compile(&minimal()).expect("compiles"),
    );
    let mut other = minimal();
    other
        .declarative
        .as_mut()
        .expect("the minimal playbook has a body")
        .route
        .push(hold_step("extra", 1_000));
    two.seal_playbook(SeatId::new(CASE_SEAT), compile(&other).expect("compiles"));
    assert_eq!(
        one.state_hash(),
        two.state_hash(),
        "the plan itself is not hashed; only the execution state is"
    );
}

#[test]
fn a_seat_with_no_playbook_leaves_its_commander_where_it_stands() {
    // The harness's destination draw does not reach a commander any more
    // (T11), so a seat that sealed nothing is a seat whose commander is still
    // exactly where the generator put it.
    let mut runner = Runner::new(fresh_world());
    let commander = runner.world().commander_of(SeatId::new(1));
    let index = usize::try_from(commander.raw()).expect("a unit index");
    let before = runner.world().units().positions()[index];
    assert!(runner.begin_push());
    for _ in 0..100 {
        if runner.step().is_none() {
            break;
        }
        runner.clear_events();
    }
    assert_eq!(
        runner.world().units().positions()[index],
        before,
        "a commander with no playbook does not wander"
    );
}

#[test]
fn the_interpreters_state_survives_a_save_and_a_restore() {
    // AGENTS.md §4.8, the three places: the state hash, the snapshot round
    // trip, and the goldens. This is the middle one, and the save is taken
    // **mid-visit**, with a row's clock running — which is the shape of the
    // state that is hardest to restore, because `visit_due` is a tick and a
    // tick restored one off is a row that commits at the wrong moment.
    //
    // The two other tick-carrying shapes, a `wait_until`'s `deadline` and a
    // handler's `ready`, are plain `u32` columns restored by the same loop as
    // `visit_due`; the assertion below is deliberately not widened to a
    // playbook that has them, because that would mean moving a committed
    // transcript to test a different thing.
    let plan = compile(&decode(
        &std::fs::read_to_string(case_dir("visit").join("playbook.json"))
            .expect("the visit case is committed"),
    ))
    .expect("it compiles");
    let mut runner = Runner::new(fresh_world());
    runner
        .world_mut()
        .seal_playbook(SeatId::new(CASE_SEAT), plan.clone());
    assert!(runner.begin_push());
    for _ in 0..60 {
        runner.step().expect("the segment outlasts the test");
        runner.clear_events();
    }
    assert_eq!(
        stage_of(runner.world()),
        VisitState::Committing,
        "the save is taken mid-visit, with a row's clock running"
    );

    let snapshot = runner.capture();
    let before = runner.world().state_hash();

    let mut resumed = Runner::new(fresh_world());
    resumed
        .world_mut()
        .seal_playbook(SeatId::new(CASE_SEAT), plan);
    resumed.restore(&snapshot).expect("the snapshot restores");
    assert_eq!(
        resumed.world().state_hash(),
        before,
        "a restore is hash-identical, the interpreter's state included"
    );

    // And the two run on identically from there, which is the property a save
    // exists for.
    for _ in 0..120 {
        let one = runner.step().map(|report| report.hash);
        let two = resumed.step().map(|report| report.hash);
        assert_eq!(one, two, "the resumed run diverged from the saved one");
        runner.clear_events();
        resumed.clear_events();
    }
}

// ---------------------------------------------------------------------------
// every_playbook_halts
// ---------------------------------------------------------------------------

/// How many generated playbooks the property runs over.
const CORPUS: u32 = 96;

/// An arbitrary, pinned corpus seed. A harness seed, not a game value.
const CORPUS_SEED: u64 = 0x504C_414E_4841_4C54;

#[test]
fn every_playbook_halts() {
    let rules = rules();
    let mut compiled: u32 = 0;
    for index in 0..CORPUS {
        let text = json::write(&Shape::new(index).playbook());
        let Ok(message) = json::decode::<gp::v1::Playbook>(&text) else {
            continue;
        };
        let Ok(plan) = Plan::compile(&message, &rules) else {
            continue;
        };
        compiled = compiled.saturating_add(1);

        // The bound: every step's own clock is at most `MAX_MS`, a decision is
        // 250 ms, and the route can enter each step at most once because the
        // cursor never decreases. Handlers add their bodies, each bounded the
        // same way and fired at most `max_fires` times.
        let steps = plan.route().len().saturating_add(
            plan.handlers()
                .iter()
                .map(|rule| rule.body.len().saturating_mul(8))
                .sum(),
        );
        let decisions = steps.saturating_mul(MAX_MS_DECISIONS).saturating_add(8);
        let ticks = u32::try_from(decisions.saturating_mul(5)).unwrap_or(u32::MAX);

        let mut runner = Runner::new(fresh_world());
        runner
            .world_mut()
            .seal_playbook(SeatId::new(CASE_SEAT), plan);
        assert!(runner.begin_push());
        let mut highest: u32 = 0;
        let mut reached_tail = false;
        for _ in 0..ticks {
            if runner.step().is_none() {
                break;
            }
            runner.clear_events();
            let cursor = cursor_of(runner.world());
            if cursor == NO_INDEX {
                reached_tail = true;
                break;
            }
            assert!(
                cursor >= highest,
                "playbook {index}: the route cursor went backwards ({highest} then {cursor}); a \
                 jump only ever goes forward"
            );
            highest = cursor;
        }
        assert!(
            reached_tail,
            "playbook {index} did not reach its fallback within {ticks} ticks. Every playbook \
             halts: every wait has a timeout, every jump goes forward, and the route enters each \
             step at most once."
        );
    }
    assert!(
        compiled.saturating_mul(2) >= CORPUS,
        "only {compiled} of {CORPUS} generated playbooks compiled; the generator is only \
         exercising the refusal paths, so the halting claim is about very little"
    );
}

/// The longest duration the generator writes, in decisions.
///
/// 4 000 ms at a 250 ms decision tick is sixteen decisions, and the deploy a
/// `place_beacon` step pays is twelve seconds — so the bound is taken from the
/// deploy rather than from the generator's own ceiling.
const MAX_MS_DECISIONS: usize = 80;

/// The generated corpus: playbook-shaped documents with bounded durations.
///
/// Modelled on `crates/verifier/tests/fuzz.rs` — same counter-based
/// [`StreamRng`], same "shaped, then damaged" idea — because `proptest` is
/// rejected (item 101). The sim cannot depend on the verifier (that would be a
/// dependency cycle), so the shape is written again here rather than shared.
struct Shape {
    rng: StreamRng,
    clean: bool,
    labels: u32,
}

impl Shape {
    fn new(index: u32) -> Shape {
        Shape {
            // `Stream::Map` under a corpus seed of this file's own, which is
            // **not** the stream reuse AGENTS.md §4.7 forbids: that rule is
            // about two purposes drawing from one stream *inside the sim*,
            // where a reordering moves the hash chain. Nothing here reaches sim
            // state — the corpus is generated before any world is stepped, from
            // a seed no match uses — so the draws cannot collide with the map
            // generator's. A `Stream::Harness` variant would read better and is
            // additive; it is a change to the stream enum, which is a contract
            // file (AGENTS.md §5), so it waits for a reason bigger than
            // legibility.
            rng: StreamRng::new(CORPUS_SEED, Stream::Map, index, 0, 0),
            clean: index % 3 != 0,
            labels: 0,
        }
    }

    fn below(&mut self, n: i32) -> i32 {
        self.rng.range_i32(0, n.saturating_sub(1).max(0))
    }

    fn rarely(&mut self, n: i32) -> bool {
        self.below(n) == 0
    }

    fn label(&mut self) -> String {
        self.labels = self.labels.saturating_add(1);
        format!("s{:03}", self.labels)
    }

    /// A duration, always bounded: that is what makes the halting claim a
    /// claim about the *interpreter* rather than about the generator.
    fn duration(&mut self) -> Json {
        let ms = self.rng.range_i32(250, 4_000);
        Json::Number(ms.to_string())
    }

    fn condition(&mut self) -> Json {
        match self.below(4) {
            0 => Json::Object(vec![(
                "cmdr_hp_pct".to_owned(),
                Json::Object(vec![
                    ("cmp".to_owned(), Json::String("LE".to_owned())),
                    ("pct".to_owned(), Json::Number(self.below(101).to_string())),
                ]),
            )]),
            1 => Json::Object(vec![(
                "segment_elapsed".to_owned(),
                Json::Object(vec![(
                    "ms".to_owned(),
                    Json::Object(vec![
                        ("op".to_owned(), Json::String("GE".to_owned())),
                        ("value".to_owned(), self.duration()),
                    ]),
                )]),
            )]),
            2 => Json::Object(vec![(
                "treasury".to_owned(),
                Json::Object(vec![(
                    "dollars".to_owned(),
                    Json::Object(vec![
                        ("op".to_owned(), Json::String("GE".to_owned())),
                        (
                            "value".to_owned(),
                            Json::Number(self.below(200).to_string()),
                        ),
                    ]),
                )]),
            )]),
            _ => Json::Object(vec![(
                "beacon_powered".to_owned(),
                Json::Object(vec![
                    (
                        "beacon".to_owned(),
                        Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))]),
                    ),
                    ("powered".to_owned(), Json::Bool(true)),
                ]),
            )]),
        }
    }

    fn step(&mut self, labels: &mut Vec<String>) -> Json {
        let label = self.label();
        labels.push(label.clone());
        let mut fields: Vec<(String, Json)> = vec![("label".to_owned(), Json::String(label))];
        let timeout = self.duration();
        fields.push(("timeout_ms".to_owned(), timeout));
        if self.rarely(4) {
            let guard = self.condition();
            fields.push(("skip_if".to_owned(), guard));
        }
        match self.below(6) {
            0 => {
                let place = if self.clean {
                    Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))])
                } else {
                    Json::Object(vec![(
                        "voxel".to_owned(),
                        Json::Object(vec![
                            ("x".to_owned(), Json::Number(self.below(384).to_string())),
                            ("y".to_owned(), Json::Number(self.below(384).to_string())),
                            ("z".to_owned(), Json::Number(self.below(64).to_string())),
                        ]),
                    )])
                };
                fields.push((
                    "move".to_owned(),
                    Json::Object(vec![("to".to_owned(), place)]),
                ));
            }
            1 => {
                let ms = self.duration();
                fields.push(("hold".to_owned(), Json::Object(vec![("ms".to_owned(), ms)])));
            }
            2 => {
                let guard = self.condition();
                fields.push((
                    "wait_until".to_owned(),
                    Json::Object(vec![("condition".to_owned(), guard)]),
                ));
            }
            3 => {
                fields.push((
                    "interface".to_owned(),
                    Json::Object(vec![
                        (
                            "beacon".to_owned(),
                            Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))]),
                        ),
                        (
                            "rows".to_owned(),
                            Json::Array(vec![Json::Object(vec![(
                                "set_priority".to_owned(),
                                Json::String("NORMAL".to_owned()),
                            )])]),
                        ),
                    ]),
                ));
            }
            4 => {
                let deploy = self.deploy();
                fields.push(("place_beacon".to_owned(), deploy));
            }
            _ => {
                fields.push((
                    "broadcast".to_owned(),
                    Json::Object(vec![(
                        "go_code".to_owned(),
                        Json::Object(vec![("code".to_owned(), Json::String("A".to_owned()))]),
                    )]),
                ));
            }
        }
        Json::Object(fields)
    }

    /// A `place_beacon` step's body.
    ///
    /// The deploy is the longest clock in the vocabulary
    /// (`place_beacon_deploy_ms`, twelve seconds) and the one step that grows
    /// a table, so it is the step `MAX_MS_DECISIONS`'s bound is actually
    /// justified by. Most generated sites are outside the seat's spheres and
    /// are refused; that is the `on_fail` path, and it halts too.
    fn deploy(&mut self) -> Json {
        let site = Json::Object(vec![(
            "voxel".to_owned(),
            Json::Object(vec![
                ("x".to_owned(), Json::Number(self.below(384).to_string())),
                ("y".to_owned(), Json::Number(self.below(384).to_string())),
                ("z".to_owned(), Json::Number(self.below(64).to_string())),
            ]),
        )]);
        let depth = Json::Number(self.below(6).saturating_add(1).to_string());
        Json::Object(vec![
            ("at".to_owned(), site),
            (
                "initial".to_owned(),
                Json::Object(vec![
                    ("priority".to_owned(), Json::String("NORMAL".to_owned())),
                    (
                        "mandate".to_owned(),
                        Json::Object(vec![(
                            "mine".to_owned(),
                            Json::Object(vec![("dig_max_depth".to_owned(), depth)]),
                        )]),
                    ),
                ]),
            ),
        ])
    }

    /// One handler, whose `resume` is drawn from **all four** values.
    ///
    /// `SKIP_STEP` and `JUMP_FORWARD` are the two that move the route cursor,
    /// which is the thing the halting property is about, so a corpus that only
    /// ever wrote `CONTINUE` was not exercising it.
    fn handler(&mut self, route_labels: &[String]) -> Json {
        let when = self.condition();
        // Body labels go in their own list: a handler's `resume_at_label` and a
        // route jump both resolve against the **route**, so a body label
        // offered as a target would only ever be refused.
        let mut body_labels: Vec<String> = Vec::new();
        let mut body: Vec<Json> = Vec::new();
        let steps = self.below(2).saturating_add(1);
        for _ in 0..steps {
            let mut step = self.step(&mut body_labels);
            if let Json::Object(fields) = &mut step {
                let on_fail = self.on_fail(None);
                fields.push(("on_fail".to_owned(), on_fail));
            }
            body.push(step);
        }
        let (resume, target) = match (self.below(4), route_labels.first().cloned()) {
            (0, _) => ("CONTINUE", None),
            (1, _) => ("SKIP_STEP", None),
            (2, _) => ("END_ROUTE", None),
            (_, Some(label)) => ("JUMP_FORWARD", Some(label)),
            (_, None) => ("CONTINUE", None),
        };
        let mut fields = vec![
            ("id".to_owned(), Json::String("h1".to_owned())),
            ("when".to_owned(), when),
            ("body".to_owned(), Json::Array(body)),
            ("resume".to_owned(), Json::String(resume.to_owned())),
            ("cooldown_ms".to_owned(), Json::Number("5000".to_owned())),
            (
                "max_fires".to_owned(),
                Json::Number(self.below(8).saturating_add(1).to_string()),
            ),
        ];
        if let Some(label) = target {
            fields.push(("resume_at_label".to_owned(), Json::String(label)));
        }
        Json::Object(fields)
    }

    /// One of the three `on_fail` actions, the jump only when a later label
    /// exists to name.
    fn on_fail(&mut self, later: Option<&str>) -> Json {
        let action = match (self.below(3), later) {
            (0, _) => "SKIP",
            (1, _) => "ABORT_ROUTE",
            (_, Some(_)) => "JUMP_FORWARD",
            (_, None) => "SKIP",
        };
        let mut fields = vec![("action".to_owned(), Json::String(action.to_owned()))];
        if action == "JUMP_FORWARD"
            && let Some(label) = later
        {
            fields.push(("jump_to_label".to_owned(), Json::String(label.to_owned())));
        }
        Json::Object(fields)
    }

    fn playbook(&mut self) -> Json {
        let mut route_labels: Vec<String> = Vec::new();
        let count = self.below(5).saturating_add(1);
        let mut route: Vec<Json> = Vec::new();
        for _ in 0..count {
            route.push(self.step(&mut route_labels));
        }
        // A jump is written *after* the route, so it can only ever name a later
        // label — which is what the compiler then checks rather than trusts.
        // Every `on_fail` action is written, not only the jump: `ABORT_ROUTE`
        // is the one that ends a route early, and a corpus that never wrote it
        // would never take that exit.
        let last = route_labels.last().cloned();
        if let Some(Json::Object(fields)) = route.first_mut() {
            let action = match (self.rng.range_i32(0, 2), last.as_deref()) {
                (0, _) => {
                    Json::Object(vec![("action".to_owned(), Json::String("SKIP".to_owned()))])
                }
                (1, _) => Json::Object(vec![(
                    "action".to_owned(),
                    Json::String("ABORT_ROUTE".to_owned()),
                )]),
                (_, Some(label)) => Json::Object(vec![
                    ("action".to_owned(), Json::String("JUMP_FORWARD".to_owned())),
                    ("jump_to_label".to_owned(), Json::String(label.to_owned())),
                ]),
                (_, None) => {
                    Json::Object(vec![("action".to_owned(), Json::String("SKIP".to_owned()))])
                }
            };
            fields.push(("on_fail".to_owned(), action));
        }
        let mut handlers: Vec<Json> = Vec::new();
        if self.rarely(2) {
            let handler = self.handler(&route_labels);
            handlers.push(handler);
        }
        Json::Object(vec![
            (
                "schema_version".to_owned(),
                Json::Object(vec![("major".to_owned(), Json::Number("1".to_owned()))]),
            ),
            (
                "meta".to_owned(),
                Json::Object(vec![
                    ("title".to_owned(), Json::String("corpus".to_owned())),
                    ("author_kind".to_owned(), Json::String("HUMAN".to_owned())),
                ]),
            ),
            ("kind".to_owned(), Json::String("PLAYBOOK".to_owned())),
            (
                "declarative".to_owned(),
                Json::Object(vec![
                    ("route".to_owned(), Json::Array(route)),
                    ("handlers".to_owned(), Json::Array(handlers)),
                ]),
            ),
            (
                "on_death".to_owned(),
                Json::Object(vec![
                    ("on_respawn".to_owned(), Json::String("CONTINUE".to_owned())),
                    (
                        "max_deaths_before_fallback".to_owned(),
                        Json::Number("2".to_owned()),
                    ),
                ]),
            ),
            (
                "fallback".to_owned(),
                Json::Object(vec![(
                    "hold".to_owned(),
                    Json::Object(vec![(
                        "at".to_owned(),
                        Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))]),
                    )]),
                )]),
            ),
        ])
    }
}

// ---------------------------------------------------------------------------
// The small builders the tests share
// ---------------------------------------------------------------------------

/// The smallest playbook that compiles: one hold, a fallback, an `on_death`.
fn minimal() -> gp::v1::Playbook {
    gp::v1::Playbook {
        schema_version: Some(gp::v1::SchemaVersion { major: 1, minor: 0 }),
        meta: Some(gp::v1::Meta {
            title: "minimal".to_owned(),
            author_kind: i32::from(gp::v1::meta::AuthorKind::Human),
            ..gp::v1::Meta::default()
        }),
        kind: i32::from(gp::v1::playbook::Kind::Playbook),
        declarative: Some(gp::v1::Declarative {
            route: Vec::new(),
            handlers: Vec::new(),
            options: None,
        }),
        on_death: Some(gp::v1::OnDeath {
            on_respawn: i32::from(gp::v1::on_death::OnRespawn::Continue),
            max_deaths_before_fallback: 2,
        }),
        fallback: Some(gp::v1::Fallback {
            posture: Some(gp::v1::fallback::Posture::Hold(gp::v1::FallbackHold {
                at: Some(gp::v1::Location {
                    place: Some(gp::v1::location::Place::Safest(gp::v1::Safest {})),
                }),
            })),
        }),
    }
}

fn hold_step(label: &str, ms: i32) -> gp::v1::Step {
    gp::v1::Step {
        label: label.to_owned(),
        kind: Some(gp::v1::step::Kind::Hold(gp::v1::HoldStep { ms })),
        ..gp::v1::Step::default()
    }
}

fn wait_step(timeout_ms: i32) -> gp::v1::Step {
    gp::v1::Step {
        label: "wait".to_owned(),
        timeout_ms,
        kind: Some(gp::v1::step::Kind::WaitUntil(gp::v1::WaitUntilStep {
            condition: Some(elapsed_at_least(600_000)),
        })),
        ..gp::v1::Step::default()
    }
}

fn interface_step(
    label: &str,
    beacon: gp::v1::BeaconRef,
    rows: Vec<gp::v1::InterfaceRow>,
) -> gp::v1::Step {
    gp::v1::Step {
        label: label.to_owned(),
        kind: Some(gp::v1::step::Kind::Interface(gp::v1::InterfaceStep {
            beacon: Some(beacon),
            rows,
        })),
        ..gp::v1::Step::default()
    }
}

fn beacon_id(id: &str) -> gp::v1::BeaconRef {
    gp::v1::BeaconRef {
        r#ref: Some(gp::v1::beacon_ref::Ref::BeaconId(id.to_owned())),
    }
}

fn elapsed_at_least(ms: i32) -> gp::v1::Condition {
    gp::v1::Condition {
        node: Some(gp::v1::condition::Node::SegmentElapsed(
            gp::v1::SegmentElapsed {
                ms: Some(gp::v1::IntCompare {
                    op: i32::from(gp::v1::int_compare::Op::Ge),
                    value: ms,
                }),
            },
        )),
    }
}

/// The route cursor of the case seat.
fn cursor_of(world: &World) -> u32 {
    world
        .interpreter()
        .state(usize::from(CASE_SEAT))
        .map_or(NO_INDEX, |state| state.cursor)
}

/// The stage of the case seat's step in progress.
fn stage_of(world: &World) -> VisitState {
    world
        .interpreter()
        .state(usize::from(CASE_SEAT))
        .map_or(VisitState::NotStarted, |state| state.stage)
}

/// The ticks a kind appears on in a transcript, in order.
fn ticks_of(body: &str, kind: EventKind) -> Vec<u32> {
    body.lines()
        .filter(|row| field(row, 2) == Some(kind.name()))
        .filter_map(|row| field(row, 0).and_then(|tick| tick.parse().ok()))
        .collect()
}

/// The values a kind carries in a transcript, in order.
fn values_of(body: &str, kind: EventKind) -> Vec<i64> {
    body.lines()
        .filter(|row| field(row, 2) == Some(kind.name()))
        .filter_map(|row| field(row, 5).and_then(|value| value.parse().ok()))
        .collect()
}

/// The subjects a kind carries in a transcript, in order.
fn subjects_of(body: &str, kind: EventKind) -> Vec<String> {
    body.lines()
        .filter(|row| field(row, 2) == Some(kind.name()))
        .filter_map(|row| field(row, 4).map(str::to_owned))
        .collect()
}

fn field(row: &str, at: usize) -> Option<&str> {
    row.split('\t').nth(at)
}
