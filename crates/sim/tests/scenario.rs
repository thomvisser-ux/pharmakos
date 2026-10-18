// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scenario chain: a full three-minute segment driven by the spec's own
//! worked playbook.
//!
//! `scenarios/skeleton/expand-east-segment.scenario.jsonc` is the scenario;
//! `tests/golden/scenarios/expand-east-segment/expected.hashes.txt` is the
//! per-tick chain it asserts on. This file is that chain's **producer**, and it
//! exists because `gamectl scenario run` does not yet: T15 writes the runner,
//! and until it does the chain would be a path in a file nothing ever compared
//! (`tests/golden/README.md` rule 1 — a golden with nothing to compare against
//! checks nothing).
//!
//! # It reproduces the scenario exactly, and says how
//!
//! Every input the scenario names is named here, with the same value:
//!
//! | Scenario | Here |
//! | --- | --- |
//! | `map.seed` `0x00000000ca5caded` | [`SEED`] |
//! | `rules` `rules/rules.v1.json` | the committed table |
//! | two `playbook` seats on `scenarios/skeleton/expand-east-segment.playbook.jsonc` | [`PLAYBOOK`] |
//! | `segments[0].length_ms` 180000 | [`SEGMENT_MS`], 3 600 ticks |
//!
//! The one indirection is the playbook. The scenario's playbook is **JSONC**
//! -- proto JSON *plus comments* -- and comments are `plan-core`'s to read;
//! the sim's only JSON reader is the proto crate's codec, and the sim may not
//! depend on `plan-core` (AGENTS.md §3's crate map). So the file keeps every
//! comment on a line of its own, and [`playbook`] drops those lines before the
//! codec decodes the rest; `gamectl scenario run` loads the same file through
//! `plan-core`, and the two producers' chains agree.
//!
//! Since S2's `base` lane the scenario's playbook is no longer the spec's
//! worked example itself: no one first site is legal both on this map and on
//! the verifier's reference view, so the scenario has its own copy with the
//! site moved to where seat 0's walk can reach it inside its timeout (ruling 3
//! of S1's stage demo). `the_scenarios_playbook_is_the_example_but_its_site`
//! holds the copy to the example on every other line that decides the run.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::fmt::Write as _;
use std::path::PathBuf;

use pharmakos_proto::gp;
use pharmakos_proto::json;
use pharmakos_sim::events::EventKind;
use pharmakos_sim::hex;
use pharmakos_sim::runner::{DEFAULT_ROUND_LIMIT, MatchPhase, MatchSettings, Runner};
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::{Plan, RulesTable, World, WorldConfig};

/// The scenario's map seed, as its `map.seed` spells it.
const SEED: u64 = 0x0000_0000_ca5c_aded;

/// The scenario's segment: three minutes of game time, the first rung of item
/// 68's 3 / 5 / 8 ladder.
const SEGMENT_MS: i32 = 180_000;

/// 20 Hz × 180 s. The segment ends **on** the tick, not near it.
const SEGMENT_TICKS: u32 = 3_600;

/// The scenario's two seats.
const SEATS: u32 = 2;

/// The scenario's playbook: the worked example with its first site moved.
const PLAYBOOK: &str = "scenarios/skeleton/expand-east-segment.playbook.jsonc";

/// The canonical form of `examples/playbooks/expand_east.jsonc`, T1's golden.
const EXAMPLE: &str = "tests/golden/proto/expected.expand_east.json";

/// The site the scenario's playbook moves the first step to: 20 voxels east
/// of seat 0's core on this map.
const SITE: [i32; 3] = [378, 24, 36];

/// The scenario's name, which is also its golden directory.
const NAME: &str = "expand-east-segment";

/// The event log's header line, the same shape an interpreter transcript has.
const HEADER: &str = "# tick\tseq\tkind\tseat\tsubject\tvalue\n";

fn repo_root() -> PathBuf {
    let here = PathBuf::from(".");
    if here.join("rules").join("rules.v1.json").is_file() {
        return here;
    }
    PathBuf::from("..").join("..")
}

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

/// The scenario's playbook, its whole-line comments dropped.
fn playbook() -> gp::v1::Playbook {
    let text = std::fs::read_to_string(repo_root().join(PLAYBOOK))
        .expect("the scenario's playbook is committed");
    let body: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<&str>>()
        .join("\n");
    json::decode(&body).expect("it is gp.v1 JSON once its comment lines are dropped")
}

/// The worked example, in its canonical golden.
fn example() -> gp::v1::Playbook {
    let text = std::fs::read_to_string(repo_root().join(EXAMPLE))
        .expect("the committed canonical example exists");
    json::decode(&text).expect("it is canonical gp.v1 JSON")
}

/// The scenario's world: its seed, its seats, its one segment.
fn scenario_world() -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: SEATS,
        units_per_seat: 0,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: DEFAULT_ROUND_LIMIT,
        },
    })
    .expect("the rules table describes a map for this seed")
}

/// Play the scenario and return `(chain, event log)`.
///
/// The event log is the same shape as an interpreter transcript
/// (`tests/golden/interpreter/README.md`), because it is read for the same
/// reason: to see what the segment actually did.
fn play() -> (String, String) {
    let plan = Plan::compile(&playbook(), &rules()).expect("the worked example compiles");
    let mut runner = Runner::new(scenario_world());
    for seat in 0..SEATS {
        let id = SeatId::new(u8::try_from(seat).expect("two seats"));
        assert!(
            runner.world_mut().seal_playbook(id, plan.clone()),
            "seat {seat} seals the scenario's playbook"
        );
    }
    assert!(runner.begin_push(), "a match opens in a Lull");

    let mut chain = String::with_capacity(
        usize::try_from(SEGMENT_TICKS)
            .unwrap_or(0)
            .saturating_mul(24),
    );
    let mut log = String::from(HEADER);
    let mut tick: u32 = 0;
    while tick < SEGMENT_TICKS {
        let Some(report) = runner.step().unwrap() else {
            break;
        };
        for event in runner.events() {
            let seat = event
                .seat
                .map_or_else(|| "-".to_owned(), |seat| seat.raw().to_string());
            let subject = event
                .subject
                .map_or_else(|| "-".to_owned(), |id| id.raw().to_string());
            let _ = writeln!(
                log,
                "{}\t{}\t{}\t{seat}\t{subject}\t{}",
                event.tick.raw(),
                event.seq,
                event.kind.name(),
                event.value
            );
        }
        runner.clear_events();
        chain.push_str(&report.tick.raw().to_string());
        chain.push('\t');
        chain.push_str(&hex(report.hash));
        chain.push('\n');
        tick = tick.saturating_add(1);
    }
    assert_eq!(
        runner.phase(),
        MatchPhase::Recap,
        "3 600 ticks of a 180 000 ms segment end it"
    );
    (chain, log)
}

#[test]
fn the_scenarios_playbook_is_the_example_but_its_site() {
    // The scenario's copy of the worked example differs from it in exactly
    // one thing, the first site, which both the walk and the deploy name; the
    // rest of what decides the run -- the steps, the timeout, the handler, the
    // tail, `on_death` and the fallback -- is the example's.
    let copy = playbook();
    let mut original = example();
    let site = |playbook: &gp::v1::Playbook| -> Vec<String> {
        playbook
            .declarative
            .as_ref()
            .map(|body| {
                body.route
                    .iter()
                    .take(2)
                    .map(|step| format!("{step:?}"))
                    .collect()
            })
            .unwrap_or_default()
    };
    let moved = site(&copy);
    assert_eq!(moved.len(), 2, "the copy keeps the walk and the deploy");
    for step in &moved {
        assert!(
            step.contains(&format!("x: {}", SITE[0]))
                && step.contains(&format!("y: {}", SITE[1]))
                && step.contains(&format!("z: {}", SITE[2])),
            "each of the first two steps names the moved site: {step}"
        );
    }
    // Put the example's site back into the copy, voxel for voxel and nothing
    // else, and the two are one playbook: a drift in the steps' timeout, pace
    // or anything but the site fails here.
    let site_of = |playbook: &gp::v1::Playbook| {
        playbook
            .declarative
            .as_ref()
            .and_then(|body| body.route.first())
            .and_then(|step| match step.kind.as_ref() {
                Some(gp::v1::step::Kind::Move(walk)) => walk.to.as_ref(),
                _ => None,
            })
            .and_then(|to| match to.place.as_ref() {
                Some(gp::v1::location::Place::Voxel(voxel)) => Some(*voxel),
                _ => None,
            })
            .expect("the first step walks to a voxel")
    };
    let theirs = format!("{:?}", site_of(&original));
    let ours = format!("{:?}", site_of(&copy));
    let restored = format!("{copy:?}").replace(&ours, &theirs);
    assert_eq!(
        restored.matches(&theirs).count(),
        2,
        "the site appears exactly twice, in the walk and in the deploy"
    );
    original.meta = copy.meta.clone();
    assert_eq!(
        restored,
        format!("{original:?}"),
        "apart from its site, the scenario's playbook is the worked example"
    );
}

#[test]
fn the_scenarios_segment_reproduces_its_chain() {
    let (chain, log) = play();
    assert_eq!(
        chain.lines().count(),
        usize::try_from(SEGMENT_TICKS).unwrap_or(0),
        "one line per played tick"
    );

    if let Some(root) = target_dir() {
        let dir = root.join("golden").join("scenarios").join(NAME);
        std::fs::create_dir_all(&dir)
            .unwrap_or_else(|error| panic!("creating {}: {error}", dir.display()));
        for (name, body) in [("actual.hashes.txt", &chain), ("actual.events.txt", &log)] {
            let path = dir.join(name);
            std::fs::write(&path, body)
                .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
        }
    }

    let expected = repo_root()
        .join("tests")
        .join("golden")
        .join("scenarios")
        .join(NAME)
        .join("expected.hashes.txt");
    if let Ok(golden) = std::fs::read_to_string(&expected) {
        assert_eq!(
            golden.replace("\r\n", "\n"),
            chain,
            "the scenario's chain moved. tests/golden/scenarios/README.md says what a diff there \
             means; say in the pull request which rule moved it."
        );
    }

    let events = repo_root()
        .join("tests")
        .join("golden")
        .join("scenarios")
        .join(NAME)
        .join("expected.events.txt");
    if let Ok(golden) = std::fs::read_to_string(&events) {
        assert_eq!(
            golden.replace("\r\n", "\n"),
            log,
            "the scenario's event log moved"
        );
    }

    // Events **and** hashes: the scenario asserts on both, and the pair is what
    // makes the chain a claim about the right match rather than about a
    // reproducible nothing.
    for wanted in [
        EventKind::PushStarted.name(),
        EventKind::StepStarted.name(),
        EventKind::StepFailed.name(),
        EventKind::FallbackEngaged.name(),
        EventKind::SegmentEnded.name(),
    ] {
        assert!(
            log.lines().any(|row| row.contains(wanted)),
            "the segment never reported `{wanted}`:\n{log}"
        );
    }
    // The scenario file's own assertions, in the two shapes that file writes
    // today: "this kind fired for this seat by this tick". Written out here
    // until `gamectl scenario run` (T15) reads the file itself, so that the
    // committed assertions are checked by something rather than only read.
    for (kind, seat, by_tick) in [
        (EventKind::StepStarted.name(), "0", 20_u32),
        (EventKind::StepCompleted.name(), "0", 400),
        (EventKind::BeaconPlaced.name(), "0", 640),
        (EventKind::RowCommitted.name(), "0", 720),
        (EventKind::FallbackEngaged.name(), "0", 725),
        (EventKind::StepFailed.name(), "1", 2_410),
        (EventKind::FallbackEngaged.name(), "1", 2_411),
    ] {
        assert!(
            log.lines().any(|row| {
                let mut fields = row.split('\t');
                let tick: u32 = fields
                    .next()
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(u32::MAX);
                let _seq = fields.next();
                fields.next() == Some(kind) && fields.next() == Some(seat) && tick <= by_tick
            }),
            "the scenario asserts `{kind}` for seat {seat} by tick {by_tick}, and it did not \
             happen:\n{log}"
        );
    }
}

#[test]
fn the_same_seed_and_playbook_produce_the_same_chain_twice() {
    // The scenario's whole premise, in one line: the sim is a pure function of
    // (map seed, playbooks, rules hash).
    let (first, _) = play();
    let (second, _) = play();
    assert_eq!(first, second, "two runs of one scenario disagreed");
}
