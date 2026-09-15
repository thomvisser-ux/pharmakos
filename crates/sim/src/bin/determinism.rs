// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `determinism --ticks N --out FILE [--seed HEX] [--rules PATH]`
//! `[--save-at TICK --snapshot FILE] [--resume FILE]`
//!
//! The binary `cargo xtask ci`'s `determinism` step runs. It writes one line
//! per tick, `tick<TAB>hash`, where the hash is sixteen lowercase hex digits,
//! and the chain is compared byte for byte against
//! `tests/golden/determinism/expected.hashes.txt` — and, on the CI matrix,
//! against the chains the other two operating systems produced.
//!
//! Tick 0 is the hash of the **initial state**, before any phase has run.
//!
//! # Save and resume, and why they are in the binary rather than only in a test
//!
//! T2's acceptance line asks for save/restore round-trips that hold **in fresh
//! processes**, and a save file's whole job is to be written by one process and
//! read by another. `--save-at TICK --snapshot FILE` writes a snapshot at the
//! tick whose hash was just emitted; `--resume FILE` starts from one instead of
//! from a fresh world, numbering its lines from the snapshot's own tick, so the
//! two runs' chains can be compared line for line.
//! `tests/determinism.rs` does exactly that across two spawned processes.
//!
//! # Line endings are a determinism concern here, not a style one
//!
//! The file is opened in binary mode and every newline is written as an
//! explicit `\n` byte. A CRLF on Windows changes the bytes and turns a passing
//! cross-OS comparison into a failing one for no sim reason at all; `xtask`'s
//! own `validate_hash_file` rejects a carriage return outright, and
//! `.gitattributes` marks `*.hashes.txt` as `-text` so git's rewriter cannot
//! put one back.
//!
//! # No clock
//!
//! Nothing here reads a wall clock, not even to report throughput. This crate
//! is linted by `cargo xtask clippy` pass 1 with the full determinism deny set,
//! `--all-targets` included, so `Instant` is as illegal in this binary as it is
//! in the sim. Performance measurement belongs in a bench target or behind the
//! wall (AGENTS.md §4.5).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use pharmakos_sim::encoding::{Enc, hex};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("determinism: {message}");
            ExitCode::FAILURE
        }
    }
}

fn value_of(args: &[String], name: &str) -> Option<String> {
    let mut index: usize = 0;
    while index.saturating_add(1) < args.len() {
        if args.get(index).is_some_and(|a| a == name) {
            return args.get(index.saturating_add(1)).cloned();
        }
        index = index.saturating_add(1);
    }
    None
}

/// The command line, parsed.
struct Cli {
    ticks: u32,
    out: PathBuf,
    rules_path: PathBuf,
    seed: Option<u64>,
    resume: Option<PathBuf>,
    snapshot_out: Option<PathBuf>,
    save_at: Option<u32>,
}

fn run(args: &[String]) -> Result<String, String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(HELP.to_owned());
    }
    let cli = parse(args)?;
    let mut world = build_world(&cli)?;
    let text = chain(&mut world, &cli)?;

    if let Some(parent) = cli.out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    let mut file = std::fs::File::create(&cli.out)
        .map_err(|error| format!("creating {}: {error}", cli.out.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|error| format!("writing {}: {error}", cli.out.display()))?;

    Ok(format!(
        "{} ticks written to {} (rules {}, rules_hash {})",
        cli.ticks,
        cli.out.display(),
        cli.rules_path.display(),
        hex(world.rules().rules_hash())
    ))
}

fn parse(args: &[String]) -> Result<Cli, String> {
    let ticks: u32 = match value_of(args, "--ticks") {
        Some(text) => text
            .parse()
            .map_err(|error| format!("--ticks {text}: {error}"))?,
        None => return Err(format!("--ticks is required\n\n{HELP}")),
    };
    let out = match value_of(args, "--out") {
        Some(text) => PathBuf::from(text),
        None => return Err(format!("--out is required\n\n{HELP}")),
    };
    let rules_path = match value_of(args, "--rules") {
        Some(text) => PathBuf::from(text),
        None => pharmakos_sim::default_rules_path().ok_or_else(|| {
            format!(
                "could not find {} from the working directory; pass --rules PATH",
                pharmakos_sim::RULES_PATH
            )
        })?,
    };
    let seed = match value_of(args, "--seed") {
        Some(text) => Some(
            u64::from_str_radix(text.trim_start_matches("0x"), 16)
                .map_err(|error| format!("--seed {text}: {error}"))?,
        ),
        None => None,
    };
    let resume = value_of(args, "--resume").map(PathBuf::from);
    let snapshot_out = value_of(args, "--snapshot").map(PathBuf::from);
    let save_at: Option<u32> = match value_of(args, "--save-at") {
        Some(text) => Some(
            text.parse()
                .map_err(|error| format!("--save-at {text}: {error}"))?,
        ),
        None => None,
    };
    if save_at.is_some() != snapshot_out.is_some() {
        return Err(format!(
            "--save-at and --snapshot are used together\n\n{HELP}"
        ));
    }

    Ok(Cli {
        ticks,
        out,
        rules_path,
        seed,
        resume,
        snapshot_out,
        save_at,
    })
}

/// The world the run starts from: a fresh one, or a restored snapshot.
fn build_world(cli: &Cli) -> Result<pharmakos_sim::World, String> {
    let mut world = match cli.seed {
        None => {
            pharmakos_sim::determinism_world(&cli.rules_path).map_err(|error| error.to_string())?
        }
        Some(seed) => {
            let rules = pharmakos_sim::RulesTable::load(&cli.rules_path)
                .map_err(|error| error.to_string())?;
            pharmakos_sim::World::new(&pharmakos_sim::WorldConfig {
                match_seed: seed,
                seats: pharmakos_sim::DETERMINISM_SEATS,
                units_per_seat: pharmakos_sim::DETERMINISM_UNITS_PER_SEAT,
                rules,
            })
            .map_err(|error| error.to_string())?
        }
    };

    if let Some(path) = cli.resume.as_ref() {
        let bytes =
            std::fs::read(path).map_err(|error| format!("reading {}: {error}", path.display()))?;
        let snapshot =
            pharmakos_sim::Snapshot::from_bytes(&bytes).map_err(|error| error.to_string())?;
        snapshot
            .restore_into(&mut world)
            .map_err(|error| error.to_string())?;
    }
    Ok(world)
}

/// The chain itself, one `tick<TAB>hash` line per tick.
fn chain(world: &mut pharmakos_sim::World, cli: &Cli) -> Result<String, String> {
    // A resumed run numbers its lines from the snapshot's own tick, so the two
    // chains line up without either side having to know the other's offset.
    let first_tick: u32 = world.tick().raw();
    let past_the_end = first_tick.saturating_add(cli.ticks);
    if let Some(at) = cli.save_at {
        if at < first_tick || at >= past_the_end {
            return Err(format!(
                "--save-at {at} is outside this run's ticks {first_tick}..{past_the_end}"
            ));
        }
    }

    // One encoder for the whole run: a tick allocates nothing (G3′ §9.17).
    let mut enc = Enc::with_capacity(64 * 1024);
    let mut text =
        String::with_capacity(usize::try_from(cli.ticks).unwrap_or(0).saturating_mul(24));
    let mut written: u32 = 0;
    while written < cli.ticks {
        let tick = first_tick.saturating_add(written);
        let hash = if written == 0 {
            world.encode(&mut enc);
            enc.finish()
        } else {
            world.step(&mut enc)
        };
        text.push_str(&tick.to_string());
        text.push('\t');
        text.push_str(&hex(hash));
        text.push('\n');
        if cli.save_at == Some(tick) {
            if let Some(path) = cli.snapshot_out.as_ref() {
                write_snapshot(world, path)?;
            }
        }
        written = written.saturating_add(1);
    }
    Ok(text)
}

/// Write the world's snapshot to `path`, in binary, creating the directory.
fn write_snapshot(world: &pharmakos_sim::World, path: &std::path::Path) -> Result<(), String> {
    let bytes = pharmakos_sim::Snapshot::capture(world)
        .to_bytes()
        .map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    let mut file = std::fs::File::create(path)
        .map_err(|error| format!("creating {}: {error}", path.display()))?;
    file.write_all(&bytes)
        .map_err(|error| format!("writing {}: {error}", path.display()))?;
    Ok(())
}

const HELP: &str = "\
determinism --ticks N --out FILE [--seed HEX] [--rules PATH]
            [--save-at TICK --snapshot FILE] [--resume FILE]

    --ticks N      how many per-tick hashes to write; tick 0 is the initial state
    --out FILE     where to write the chain, one `tick<TAB>hash` line per tick
    --seed HEX     override the pinned harness match seed
    --rules PATH   the rules table (default: rules/rules.v1.json, probed from the
                   working directory and from two levels above it)
    --save-at TICK write a snapshot once TICK's hash has been emitted
    --snapshot FILE  where that snapshot goes; used with --save-at
    --resume FILE  start from a snapshot instead of a fresh world, numbering the
                   chain from the snapshot's own tick";
