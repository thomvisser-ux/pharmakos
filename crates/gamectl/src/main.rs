// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl` — the command-line client. Role from spec §15 (Architecture), clients layer,
//! detailed in spec §12.
//!
//! Subcommands in v1: `verify`, `schema`, `docs`, `scenarios`, `seat doctor`. There is no
//! `connect` in v1 — nothing outside the game attaches to a seat. `gamectl scenario run`
//! is what drives the three nightly adversarial scenarios (Rusher, Turtle, Hunter) against
//! the Balanced built-in operator on a fixed seed set, asserting on events and hashes.
//!
//! It is an ordinary client of the Seat Gateway: same snapshot, same verifier, same submit
//! path, no privileged reads. `schema` and `docs` output is generated from the Protobuf
//! schema so it cannot drift. Like every seat-facing crate it must never reach the
//! `research` feature that gates `fork`.
//!
//! Nothing is implemented yet: `scenario run` and the headless screenshots arrive in the
//! named half-week at the end of the walking skeleton.

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next();

    println!(
        "gamectl {} — pre-spike placeholder, nothing implemented yet.",
        env!("CARGO_PKG_VERSION")
    );
    match command.as_deref() {
        Some(name) => println!("requested subcommand: {name}"),
        None => println!("no subcommand given"),
    }
    println!();
    println!("planned subcommands (spec §12, §15):");
    println!("  verify <playbook.jsonc>     run the verifier and print the report");
    println!("  schema [--part <part>]      print the schema slice a seat may use");
    println!("  docs                        print generated docs (never hand-written)");
    println!("  scenario run <file>         headless match: map seed + playbooks + assertions");
    println!("  seat doctor                 check a seat's setup and report what is wrong");

    std::process::exit(1);
}
