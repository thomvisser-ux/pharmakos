// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl host` refuses to resume an ended match (lane `opsave`, from lane
//! `fixq-c`'s finding against a real host: given an ended match's config line
//! and `resume`, the host announced the match and served it).
//!
//! A two-seat match of one round is played to its end in process, and the
//! save its end leaves (the `ended` boundary) is written into a private match
//! cache of this test's own. The real binary is then started on it, its
//! standard input the match's config line with `resume`: it announces
//! nothing, says on standard error that the match has ended, and exits with
//! the refused config line's code, as any line it refuses.
//!
//! And `gamectl host` stops at once on a write the match needs and cannot
//! make (fix pass 2): it says why on standard error, naming what the disk
//! holds, and exits with the internal code while its parent still holds its
//! standard input.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use pharmakos_gamectl::exit::Exit;
use pharmakos_gateway::cache::{Header, MatchCache};
use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, Settings};
use pharmakos_gateway::probation::Preset;
use pharmakos_gateway::save::{Boundary, Save, Stamp};
use pharmakos_gateway::serve::{Config, ConfigLine};
use pharmakos_gateway::surface::Surface;
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;

/// The match: two seats, one round of one second.
const SEED: u64 = 0x0000_0000_ca5c_aded;
const SEGMENT_MS: i32 = 1_000;
const MATCH_ID: &str = "m-opsave-ended-host";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/gamectl sits two levels below the workspace root")
        .to_path_buf()
}

/// A folder of this test's own under the target directory, fresh.
fn scratch(name: &str) -> PathBuf {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("opsave-{name}-{}", std::process::id()));
    if base.exists() {
        std::fs::remove_dir_all(&base).expect("a stale scratch folder is removed");
    }
    std::fs::create_dir_all(&base).expect("the scratch folder");
    base
}

/// The match played to its end in process, and the save its end leaves,
/// with the rules table's hash.
fn ended_save(rules: &str) -> (pharmakos_gateway::save::SavedMatch, u64) {
    // Played to its end in process: no seat seals, so the gateway files
    // each its safe playbook, and the one round ends the match.
    let host = Host::open_from(
        rules,
        SEED,
        2,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: 1,
        },
        None,
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let rules_hash = table.rules_hash();
    let mut surface = Surface::new(
        MATCH_ID,
        SEED,
        table,
        FogPolicy::fogged(),
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    assert!(surface.begin_push().expect("the Push begins"));
    let mut saved = surface.take_persistence().save;
    while let Some(report) = surface.step().expect("a tick") {
        if let Some(save) = surface.take_persistence().save {
            saved = Some(save);
        }
        if report.segment_ended {
            break;
        }
    }
    let _ = surface.end_recap().expect("the recap ends");
    if let Some(save) = surface.take_persistence().save {
        saved = Some(save);
    }
    assert_eq!(
        surface.time().phase,
        Phase::Ended,
        "one round ends the match"
    );
    let saved = saved.expect("a save");
    assert_eq!(saved.boundary, Boundary::Ended);

    (saved, rules_hash)
}

#[test]
fn the_host_refuses_to_resume_an_ended_match_and_announces_nothing() {
    let rules = std::fs::read_to_string(root().join("rules").join("rules.v1.json"))
        .expect("the rules table");
    let config = Config {
        match_id: MATCH_ID.to_owned(),
        seed: SEED,
        seats: 2,
        human_seat: None,
        segment_lengths_ms: vec![SEGMENT_MS],
        round_limit: 1,
        preset: Preset::Custom,
    };
    let (saved, rules_hash) = ended_save(&rules);

    // The private match cache, where the host looks for it.
    let home = scratch("ended-host");
    let (variable, data_root) = if cfg!(target_os = "windows") {
        ("LOCALAPPDATA", home.join(pharmakos_gateway::cache::APP_DIR))
    } else if cfg!(target_os = "macos") {
        (
            "HOME",
            home.join("Library")
                .join("Application Support")
                .join(pharmakos_gateway::cache::APP_DIR),
        )
    } else {
        (
            "XDG_DATA_HOME",
            home.join(pharmakos_gateway::cache::APP_DIR_LOWER),
        )
    };
    let cache = MatchCache::open(
        &data_root,
        MATCH_ID,
        &Header {
            match_seed: SEED,
            seats: 2,
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: 1,
            rules_hash,
        },
    )
    .expect("a match cache");
    cache
        .write_save(
            &Save {
                stamp: Stamp::current(rules_hash),
                config: config.clone(),
                state: saved,
            }
            .render(),
        )
        .expect("the save is written");

    let line = ConfigLine {
        config,
        resume: true,
    }
    .render();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gamectl"))
        .arg("host")
        .current_dir(root())
        .env(variable, &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("gamectl starts");
    child
        .stdin
        .take()
        .expect("its standard input")
        .write_all(line.as_bytes())
        .expect("the config line is written");
    let output = child.wait_with_output().expect("gamectl ends");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.is_empty(), "nothing is announced: {stdout}");
    assert!(
        stderr.contains("this match has ended")
            && stderr.contains("gamectl host: the match could not be hosted: "),
        "the refusal is said, before any announce line: {stderr}"
    );
    assert_eq!(
        output.status.code(),
        Some(i32::from(Exit::Input.code())),
        "the refused line's exit code: {stderr}"
    );
    std::fs::remove_dir_all(&home).expect("the scratch folder is removed");
}

/// The match in its opening Lull, saved there, with the rules table's hash.
fn opening_lull_save(rules: &str) -> (pharmakos_gateway::save::SavedMatch, u64) {
    let host = Host::open_from(
        rules,
        SEED,
        2,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: 1,
        },
        None,
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let rules_hash = table.rules_hash();
    let mut surface = Surface::new(
        LULL_MATCH_ID,
        SEED,
        table,
        FogPolicy::fogged(),
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    (surface.lull_save().expect("a Lull saves"), rules_hash)
}

/// The match the stop test resumes.
const LULL_MATCH_ID: &str = "m-opsave-stopped-host";

/// `gamectl host` stops at once on a write the match needs and cannot make
/// (lane `opsave`, fix pass 2: a boundary is durable before anyone is told,
/// and a host that cannot make it durable says why and exits). The match's
/// audit log, which a resume continues from, is read-only, so the host's
/// first append to it fails, before it announces (fix pass 3, item 4: a host
/// that cannot serve does not announce): with its standard input still held
/// open, it announces nothing, says on standard error that the match could
/// not be hosted, what could not be written and what the disk holds, and
/// exits with the internal code.
#[test]
fn the_host_exits_at_once_when_a_write_fails_and_says_why() {
    let rules = std::fs::read_to_string(root().join("rules").join("rules.v1.json"))
        .expect("the rules table");
    let config = Config {
        match_id: LULL_MATCH_ID.to_owned(),
        seed: SEED,
        seats: 2,
        human_seat: Some(0),
        segment_lengths_ms: vec![SEGMENT_MS],
        round_limit: 1,
        preset: Preset::Custom,
    };
    let (saved, rules_hash) = opening_lull_save(&rules);
    let home = scratch("stopped-host");
    let (variable, data_root) = private_cache(&home);
    let cache = MatchCache::open(
        &data_root,
        LULL_MATCH_ID,
        &Header {
            match_seed: SEED,
            seats: 2,
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: 1,
            rules_hash,
        },
    )
    .expect("a match cache");
    cache
        .write_save(
            &Save {
                stamp: Stamp::current(rules_hash),
                config: config.clone(),
                state: saved,
            }
            .render(),
        )
        .expect("the save is written");
    let log = cache.directory().join("audit.log");
    std::fs::write(&log, format!("{}\n", pharmakos_gateway::audit::HEADER)).expect("a log");
    let mut permissions = std::fs::metadata(&log).expect("the log").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&log, permissions.clone()).expect("read-only");

    let line = ConfigLine {
        config,
        resume: true,
    }
    .render();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gamectl"))
        .arg("host")
        .current_dir(root())
        .env(variable, &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("gamectl starts");
    // Standard input is held open for the whole wait: the host returns by
    // itself, not because its parent let go.
    let mut stdin = child.stdin.take().expect("its standard input");
    stdin
        .write_all(line.as_bytes())
        .expect("the config line is written");
    stdin.flush().expect("the config line is sent");
    let mut exited = None;
    for _ in 0..6_000 {
        if let Some(status) = child.try_wait().expect("the child's state") {
            exited = Some(status);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let status = exited.expect("the host exited by itself (waited a minute)");
    drop(stdin);
    let output = child.wait_with_output().expect("gamectl ends");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        status.code(),
        Some(i32::from(Exit::Internal.code())),
        "the internal exit code: {stderr}"
    );
    assert!(
        stderr.contains("the audit log could not be written (")
            && stderr.contains("; on disk is the `lull` save of round 1; this host stops"),
        "the host says what failed and what the disk holds: {stderr}"
    );
    assert!(
        stderr.contains("gamectl host: the match could not be hosted: INTERNAL: the audit log"),
        "a failure before the announce line: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "nothing announced: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "the test's own file, made writable again so its folder can be removed"
    )]
    permissions.set_readonly(false);
    std::fs::set_permissions(&log, permissions).expect("writable again");
    std::fs::remove_dir_all(&home).expect("the scratch folder is removed");
}

/// The environment variable that names this platform's per-user data folder,
/// and the private match cache's root under `home` set as it.
fn private_cache(home: &Path) -> (&'static str, PathBuf) {
    if cfg!(target_os = "windows") {
        ("LOCALAPPDATA", home.join(pharmakos_gateway::cache::APP_DIR))
    } else if cfg!(target_os = "macos") {
        (
            "HOME",
            home.join("Library")
                .join("Application Support")
                .join(pharmakos_gateway::cache::APP_DIR),
        )
    } else {
        (
            "XDG_DATA_HOME",
            home.join(pharmakos_gateway::cache::APP_DIR_LOWER),
        )
    }
}

/// A match `gamectl host` announced and then stopped is said as stopped
/// (fix pass 3, item 6): a new match is announced, its save is blocked (the
/// match's `save.json.partial` made a folder), and the parent closes standard
/// input in the opening Lull, so the Lull's save cannot be written: the host
/// says "the match stopped", what could not be written and what the disk
/// holds, and exits with the internal code.
#[test]
fn a_host_that_stops_after_its_announce_says_the_match_stopped() {
    use std::io::BufRead as _;
    let match_id = "m-opsave-stopped-after";
    let config = Config {
        match_id: match_id.to_owned(),
        seed: SEED,
        seats: 2,
        human_seat: Some(0),
        segment_lengths_ms: vec![SEGMENT_MS],
        round_limit: 1,
        preset: Preset::Custom,
    };
    let home = scratch("stopped-after");
    let (variable, data_root) = private_cache(&home);
    let line = ConfigLine {
        config,
        resume: false,
    }
    .render();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gamectl"))
        .arg("host")
        .current_dir(root())
        .env(variable, &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("gamectl starts");
    let mut stdin = child.stdin.take().expect("its standard input");
    stdin
        .write_all(line.as_bytes())
        .expect("the config line is written");
    stdin.flush().expect("the config line is sent");
    let mut announce = String::new();
    std::io::BufReader::new(child.stdout.take().expect("its standard output"))
        .read_line(&mut announce)
        .expect("the announce line");
    assert!(!announce.is_empty(), "the match was announced");
    let folder = data_root.join("matches").join(match_id);
    std::fs::create_dir_all(folder.join("save.json.partial")).expect("the blocker");
    drop(stdin);
    let output = child.wait_with_output().expect("gamectl ends");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(i32::from(Exit::Internal.code())),
        "the internal exit code: {stderr}"
    );
    assert!(
        stderr.contains(
            "gamectl host: the match stopped: INTERNAL: the `lull` save of round 1 could not be written ("
        ) && stderr.contains("; on disk is no save yet; this host stops"),
        "a failure after the announce line: {stderr}"
    );
    std::fs::remove_dir_all(&home).expect("the scratch folder is removed");
}
