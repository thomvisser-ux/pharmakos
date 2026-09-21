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
//! | two `playbook` seats on `examples/playbooks/expand_east.jsonc` | [`PLAYBOOK`] |
//! | `segments[0].length_ms` 180000 | [`SEGMENT_MS`], 3 600 ticks |
//!
//! The one indirection is the playbook. The example on disk is **JSONC** —
//! canonical proto JSON *plus comments* — and comments are `plan-core`'s to
//! read; the sim's only JSON reader is the proto crate's canonical codec, and
//! the sim may not depend on `plan-core` (AGENTS.md §3's crate map). So this
//! file reads `tests/golden/proto/expected.expand_east.json`, which is T1's
//! committed golden of *the same playbook* in canonical form, and
//! `the_canonical_playbook_is_the_committed_example` asserts the two are the
//! same playbook rather than trusting it. When T15's runner loads the `.jsonc`
//! through `plan-core`, it will load these bytes.

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

/// The canonical form of `examples/playbooks/expand_east.jsonc`.
const PLAYBOOK: &str = "tests/golden/proto/expected.expand_east.json";

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

fn playbook() -> gp::v1::Playbook {
    let text = std::fs::read_to_string(repo_root().join(PLAYBOOK))
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
        let Some(report) = runner.step() else {
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
fn the_canonical_playbook_is_the_committed_example() {
    // The indirection this file's header describes, asserted rather than
    // assumed: the canonical golden holds the same route, the same handler and
    // the same fallback as `examples/playbooks/expand_east.jsonc`. The example
    // itself cannot be decoded here — it is JSONC and its comments are
    // `plan-core`'s to read — so what is checked is that the golden is the
    // playbook the scenario names, field by field on the parts that decide what
    // the run does.
    let example = std::fs::read_to_string(repo_root().join("examples/playbooks/expand_east.jsonc"))
        .expect("the worked example is committed");
    let decoded = playbook();
    let body = decoded
        .declarative
        .as_ref()
        .expect("the example has a body");
    assert_eq!(body.route.len(), 3, "three route steps");
    assert_eq!(body.handlers.len(), 1, "one handler, `flee`");
    assert!(
        example.contains("\"label\":\"to_site\"")
            && example.contains("\"label\":\"place_east\"")
            && example.contains("\"label\":\"back_home\""),
        "the canonical golden and the JSONC example name the same three steps"
    );
    assert!(example.contains("\"id\":\"flee\""), "and the same handler");
    assert!(
        decoded
            .declarative
            .as_ref()
            .and_then(|body| body.handlers.first())
            .is_some_and(|handler| handler.id == "flee"),
        "the canonical golden's handler is the example's"
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
        (EventKind::StepFailed.name(), "0", 2_410),
        (EventKind::FallbackEngaged.name(), "0", 2_411),
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
