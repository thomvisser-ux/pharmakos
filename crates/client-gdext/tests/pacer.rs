// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pacer's four acceptance tests (`docs/design/skeleton-plan-t16a-notes.md` section
//! A (4)), with no host and no engine.
//!
//! T16's two hash-chain tests moved to T16a, where the chain can be read in process
//! without putting a hash on the wire. What is left for the client to prove is that its
//! half of the contract holds: it asks for **game milliseconds**, carries what was asked
//! and not run, lets speed change nothing but the size of the ask, never has two asks in
//! flight, and never so much as names a step of the sim.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::path::Path;

use pharmakos_client_gdext::pacer::{PACER_PERIOD_US, Pacer};

/// A pacer in a Push at `speed`.
fn pushing(speed: u32) -> Pacer {
    let mut pacer = Pacer::new();
    assert!(pacer.set_speed(speed), "{speed}x is a speed");
    pacer.start();
    pacer
}

/// Drives `pacer` through `walls` (wall microseconds between polls), answering every ask
/// with `run(asked)`, and returns what it asked, poll by poll (`None` where nothing was).
fn drive(pacer: &mut Pacer, walls: &[u64], run: impl Fn(i32) -> i32) -> Vec<Option<i32>> {
    let mut asks = Vec::with_capacity(walls.len());
    for wall in walls {
        pacer.elapse(*wall);
        let ask = pacer.next_ask();
        if let Some(asked) = ask {
            pacer.answered(run(asked));
        }
        asks.push(ask);
    }
    asks
}

/// The gateway floors an advance to whole steps of 50 game milliseconds. The pacer does
/// not know that number; this stand-in for the host does.
fn floored(asked: i32) -> i32 {
    asked - asked % 50
}

#[test]
fn the_pacer_asks_for_game_milliseconds_and_carries_what_was_not_run() {
    let mut pacer = pushing(1);
    // 130 ms of wall time at 1x is 130 game ms owed, all of it asked for at once.
    pacer.elapse(130_000);
    assert_eq!(pacer.next_ask(), Some(130));
    // The host ran 100 of it: the 30 it did not run are carried, not lost and not re-asked
    // as a fresh debt.
    pacer.answered(100);
    assert_eq!(pacer.owed_ms(), 30);
    pacer.elapse(PACER_PERIOD_US);
    assert_eq!(pacer.next_ask(), Some(130), "30 carried plus 100 new");
    // An ask too small to run anything is answered with 0, which is not an error: all of
    // it is carried.
    pacer.answered(0);
    assert_eq!(pacer.owed_ms(), 130);

    // Over a long run the game time the host RAN adds up to the wall time: nothing is
    // dropped below the backlog bound, nothing is run twice and nothing is invented. (The
    // asks themselves add up to more, because a carried remainder is asked for again.)
    let mut steady = pushing(1);
    let walls = vec![16_667_u64; 600];
    let asks = drive(&mut steady, &walls, floored);
    let asked: i32 = asks.iter().flatten().map(|ask| floored(*ask)).sum();
    let wall_ms = walls
        .iter()
        .sum::<u64>()
        .checked_div(1_000)
        .expect("a divisor");
    assert!(
        u64::try_from(asked).expect("positive") <= wall_ms,
        "asked {asked} ms over {wall_ms} ms of wall time"
    );
    assert!(
        wall_ms - u64::try_from(asked).expect("positive") < 200,
        "the carry keeps the ask within a period or two of the wall: asked {asked} of \
         {wall_ms}"
    );
}

#[test]
fn speed_changes_only_how_much_game_time_the_pacer_asks_for() {
    // The same wall-time schedule at every speed: frames of uneven length, as a real one
    // has.
    let walls: Vec<u64> = (0..300_u64)
        .map(|frame| 12_000 + (frame % 7) * 3_000)
        .collect();
    let one = drive(&mut pushing(1), &walls, |asked| asked);
    for speed in [2_i32, 4] {
        let faster = drive(
            &mut pushing(u32::try_from(speed).expect("small")),
            &walls,
            |asked| asked,
        );
        let when = |asks: &[Option<i32>]| asks.iter().map(Option::is_some).collect::<Vec<_>>();
        assert_eq!(
            when(&faster),
            when(&one),
            "{speed}x asks at exactly the moments 1x does: speed is not a frequency"
        );
        let total = |asks: &[Option<i32>]| asks.iter().flatten().sum::<i32>();
        let (slow, fast) = (total(&one), total(&faster));
        assert!(
            (fast - slow * speed).abs() <= speed,
            "{speed}x asks for {speed} times the game time 1x does, within a millisecond of \
             rounding per multiple: {fast} against {slow}"
        );
    }
}

#[test]
fn the_pacer_never_has_two_advances_in_flight() {
    let mut pacer = pushing(4);
    pacer.elapse(PACER_PERIOD_US);
    let first = pacer.next_ask();
    assert!(first.is_some());
    // However much time passes, nothing more is asked until the first is answered.
    for _ in 0..50 {
        pacer.elapse(PACER_PERIOD_US * 3);
        assert_eq!(pacer.next_ask(), None, "one advance in flight at a time");
        assert!(pacer.in_flight());
    }
    pacer.answered(first.expect("asked"));
    assert!(!pacer.in_flight());
    assert!(
        pacer.next_ask().is_some(),
        "and the next goes out once it is answered"
    );

    // A skip is the same: repeated with no period, but never two at once.
    let mut skipping = pushing(1);
    skipping.skip();
    assert!(skipping.next_ask().is_some());
    assert_eq!(skipping.next_ask(), None);
}

/// **The pacer names no tick** — a source-text check, because the property is about what
/// the module can know rather than what it happens to do.
///
/// The pacer asks for game milliseconds and reads `advanced_ms` back; the gateway floors to
/// whole steps of the sim and never says how long one is (decisions-log item 107 (3)). If
/// the word appears in the pacer's code, somebody has taught it the step length, and the
/// client has started doing the gateway's arithmetic. Comments and string literals are
/// stripped first: the module's documentation says what it does not know, by name.
#[test]
fn the_pacer_names_no_tick() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("pacer.rs");
    let text = std::fs::read_to_string(&path).expect("src/pacer.rs");
    let mut offending: Vec<String> = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let code = code_of(line).to_ascii_lowercase();
        for needle in ["tick", "hash"] {
            if code.contains(needle) {
                offending.push(format!("{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(
        offending.is_empty(),
        "src/pacer.rs names a step of the sim or a hash in its code. The pacer asks for \
         game milliseconds and is told what ran; it never learns the step length and never \
         sees a hash (decisions-log item 107 (3)):\n{}",
        offending.join("\n")
    );
    // The check has teeth: the scanner sees the word in code and not in a comment.
    assert!(code_of("let per_tick = 50;").contains("tick"));
    assert!(!code_of("// never a tick").contains("tick"));
    assert!(!code_of("let s = \"tick\";").contains("tick"));
}

/// One line with its comment and string literals removed.
fn code_of(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    let mut in_string = false;
    while let Some(character) = characters.next() {
        if in_string {
            if character == '\\' {
                characters.next();
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '/' if characters.peek() == Some(&'/') => break,
            _ => out.push(character),
        }
    }
    out
}
