// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `determinism --ticks N --out FILE [--seed HEX] [--rules PATH]`
//!
//! The binary `cargo xtask ci`'s `determinism` step runs. It writes one line
//! per tick, `tick<TAB>hash`, where the hash is sixteen lowercase hex digits,
//! and the chain is compared byte for byte against
//! `tests/golden/determinism/expected.hashes.txt` — and, on the CI matrix,
//! against the chains the other two operating systems produced.
//!
//! Tick 0 is the hash of the **initial state**, before any phase has run.
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

fn run(args: &[String]) -> Result<String, String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(HELP.to_owned());
    }

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

    let mut world = match seed {
        None => pharmakos_sim::determinism_world(&rules_path)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "the rules table cannot describe a broadphase grid".to_owned())?,
        Some(seed) => {
            let rules =
                pharmakos_sim::RulesTable::load(&rules_path).map_err(|error| error.to_string())?;
            pharmakos_sim::World::new(&pharmakos_sim::WorldConfig {
                match_seed: seed,
                seats: pharmakos_sim::DETERMINISM_SEATS,
                units_per_seat: pharmakos_sim::DETERMINISM_UNITS_PER_SEAT,
                chunk_count: pharmakos_sim::chunks::SKELETON_CHUNK_COUNT,
                rules,
            })
            .ok_or_else(|| "the rules table cannot describe a broadphase grid".to_owned())?
        }
    };

    // One encoder for the whole run: a tick allocates nothing (G3′ §9.17).
    let mut enc = Enc::with_capacity(64 * 1024);
    let mut text = String::with_capacity(usize::try_from(ticks).unwrap_or(0).saturating_mul(24));

    let mut tick: u32 = 0;
    while tick < ticks {
        let hash = if tick == 0 {
            world.encode(&mut enc);
            enc.finish()
        } else {
            world.step(&mut enc)
        };
        text.push_str(&tick.to_string());
        text.push('\t');
        text.push_str(&hex(hash));
        text.push('\n');
        tick = tick.saturating_add(1);
    }

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    let mut file = std::fs::File::create(&out)
        .map_err(|error| format!("creating {}: {error}", out.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|error| format!("writing {}: {error}", out.display()))?;

    Ok(format!(
        "{ticks} ticks written to {} (rules {}, rules_hash {})",
        out.display(),
        rules_path.display(),
        hex(world.rules().rules_hash())
    ))
}

const HELP: &str = "\
determinism --ticks N --out FILE [--seed HEX] [--rules PATH]

    --ticks N      how many per-tick hashes to write; tick 0 is the initial state
    --out FILE     where to write the chain, one `tick<TAB>hash` line per tick
    --seed HEX     override the pinned harness match seed
    --rules PATH   the rules table (default: rules/rules.v1.json, probed from the
                   working directory and from two levels above it)";
