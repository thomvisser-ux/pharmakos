// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `run --matches N --ticks T --out FILE`
//!
//! Emits one line per tick, `match_index<TAB>tick_index<TAB>hash_hex`, then a
//! final `digest<TAB>hash_hex` line holding the xxh3 of every byte that came
//! before it.
//!
//! The file is opened in binary mode and every newline is written as an explicit
//! `\n` byte. Nothing goes through a formatting path that could translate line
//! endings, because a CRLF on Windows would change the digest and turn a passing
//! cross-OS comparison into a failing one for no sim reason at all.
//!
//! `std::time::Instant` appears here, in a binary, and nowhere inside `src/`.

use std::io::Write;
use std::time::Instant;

use g4_determinism::hash::{Enc, digest, hex};
use g4_determinism::{MATCH_SEEDS, MATCH_TICKS, World};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    let mut i = 0usize;
    while i + 1 < args.len() {
        if args[i] == name {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let matches: usize = arg_value(&args, "--matches")
        .map_or(10, |v| v.parse().expect("--matches must be an integer"));
    let ticks: u32 = arg_value(&args, "--ticks").map_or(MATCH_TICKS, |v| {
        v.parse().expect("--ticks must be an integer")
    });
    let out = arg_value(&args, "--out").unwrap_or_else(|| "trace.txt".to_owned());

    assert!(
        matches <= MATCH_SEEDS.len(),
        "only {} match seeds are defined",
        MATCH_SEEDS.len()
    );
    assert!(ticks > 0, "--ticks must be positive");

    // The whole trace is built in memory and written once, in binary mode.
    let mut buf: Vec<u8> = Vec::with_capacity(matches * usize::try_from(ticks).unwrap() * 28);
    let mut finals: Vec<u64> = Vec::with_capacity(matches);
    let mut enc = Enc::with_capacity(32 * 1024);

    let start = Instant::now();
    for (m, &seed) in MATCH_SEEDS.iter().enumerate().take(matches) {
        let mut w = World::new(seed);
        let mut last: u64 = 0;
        for t in 0..ticks {
            if t > 0 {
                w.step();
            }
            w.encode(&mut enc);
            last = enc.finish();
            write_line(&mut buf, m, t, last);
        }
        finals.push(last);
    }
    let elapsed = start.elapsed();

    let d = digest(&buf);
    buf.extend_from_slice(b"digest\t");
    buf.extend_from_slice(hex(d).as_bytes());
    buf.push(b'\n');

    let mut f = std::fs::File::create(&out).expect("create trace file");
    f.write_all(&buf).expect("write trace file");
    f.sync_all().expect("sync trace file");

    let total_ticks = u64::try_from(matches).unwrap() * u64::from(ticks);
    let ms = elapsed.as_millis();
    let ticks_per_s = if elapsed.as_nanos() == 0 {
        0
    } else {
        u128::from(total_ticks) * 1_000_000_000 / elapsed.as_nanos()
    };

    println!("# g4-determinism run");
    println!("# matches={matches} ticks={ticks} out={out}");
    println!("# seed_constant=0x{:016X}", g4_determinism::hash::STATE_HASH_SEED);
    for (m, h) in finals.iter().enumerate() {
        println!(
            "final\t{m}\t{seed:016x}\t{h}",
            seed = MATCH_SEEDS[m],
            h = hex(*h)
        );
    }
    println!("digest\t{}", hex(d));
    println!("trace_bytes\t{}", buf.len());
    println!("elapsed_ms\t{ms}");
    println!("total_ticks\t{total_ticks}");
    println!("ticks_per_s\t{ticks_per_s}");
}

fn write_line(buf: &mut Vec<u8>, m: usize, t: u32, h: u64) {
    buf.extend_from_slice(m.to_string().as_bytes());
    buf.push(b'\t');
    buf.extend_from_slice(t.to_string().as_bytes());
    buf.push(b'\t');
    buf.extend_from_slice(hex(h).as_bytes());
    buf.push(b'\n');
}
