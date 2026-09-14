// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `forkcheck` — research build only (`required-features = ["research"]`).
//!
//! For each match, at ticks `{100, 5000}`: fork, step the child 200 ticks and
//! compare the child's hash trace against the parent's over the same ticks; then
//! continue the parent to the end and confirm the parent's own full-trace digest
//! equals the digest of a run that never forked.
//!
//! Both halves matter. The first is fork equivalence; the second is that the
//! fork left no trace on the parent — no shared RNG counter, no mutated
//! copy-on-write node, no cached index.

use std::time::Instant;

use g4_determinism::fork::{fork, fork_and_step, heap_bytes};
use g4_determinism::hash::{Enc, digest, hex};
use g4_determinism::{MATCH_SEEDS, MATCH_TICKS, World};

const FORK_AT: [u32; 2] = [100, 5_000];
const CHILD_TICKS: u32 = 200;

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

fn trace_bytes(m: usize, trace: &[u64]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(trace.len() * 28);
    for (t, h) in trace.iter().enumerate() {
        buf.extend_from_slice(m.to_string().as_bytes());
        buf.push(b'\t');
        buf.extend_from_slice(t.to_string().as_bytes());
        buf.push(b'\t');
        buf.extend_from_slice(hex(*h).as_bytes());
        buf.push(b'\n');
    }
    buf
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ticks: u32 = arg_value(&args, "--ticks").map_or(MATCH_TICKS, |v| v.parse().expect("--ticks"));
    let matches: usize =
        arg_value(&args, "--matches").map_or(MATCH_SEEDS.len(), |v| v.parse().expect("--matches"));

    println!("# g4-determinism forkcheck (research build)");
    println!("# matches={matches} ticks={ticks} fork_at={FORK_AT:?} child_ticks={CHILD_TICKS}");

    let mut attempted = 0usize;
    let mut equivalent = 0usize;
    let mut parents_clean = 0usize;
    let mut fork_ns_total: u128 = 0;
    let mut child_ns_total: u128 = 0;
    let mut forks = 0usize;
    let mut fork_size = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for (m, &seed) in MATCH_SEEDS.iter().enumerate().take(matches) {

        // 1. The reference run: no forks at all.
        let reference = g4_determinism::trace_match(seed, ticks);

        // 2. The same run, forking at each of FORK_AT.
        let mut w = World::new(seed);
        let mut enc = Enc::with_capacity(32 * 1024);
        let mut forked: Vec<u64> = Vec::with_capacity(usize::try_from(ticks).unwrap());
        w.encode(&mut enc);
        forked.push(enc.finish());

        for t in 1..ticks {
            w.step();
            w.encode(&mut enc);
            forked.push(enc.finish());

            if FORK_AT.contains(&t) {
                attempted += 1;
                // The fork itself, timed alone: the SoA tables are deep-copied
                // and the OrdMap credit table is shared until one side writes.
                let t0 = Instant::now();
                let child0 = fork(&w);
                fork_ns_total += t0.elapsed().as_nanos();
                forks += 1;
                fork_size = heap_bytes(&child0);
                drop(child0);

                // Stepping the child is a separate cost and is reported
                // separately; conflating them would make `fork` look expensive
                // when what is expensive is simulating 200 ticks.
                let t1 = Instant::now();
                let (_, child_trace) = fork_and_step(&w, CHILD_TICKS);
                child_ns_total += t1.elapsed().as_nanos();

                // The parent's own trace over the same ticks. `reference` is the
                // unforked run, so comparing against it proves both that the
                // child matches the parent and that the parent was already on
                // the unperturbed timeline when it forked.
                let lo = usize::try_from(t + 1).unwrap();
                let hi = lo + usize::try_from(CHILD_TICKS).unwrap();
                assert!(hi <= reference.len(), "fork window runs past the match");
                let want = &reference[lo..hi];
                if want == child_trace.as_slice() {
                    equivalent += 1;
                } else {
                    let bad = want
                        .iter()
                        .zip(child_trace.iter())
                        .position(|(a, b)| a != b)
                        .unwrap_or(0);
                    failures.push(format!(
                        "match {m} fork at {t}: child diverges at child tick {bad} \
                         (parent {} vs child {})",
                        hex(want[bad]),
                        hex(child_trace[bad])
                    ));
                }
            }
        }

        // 3. The parent must be bit-identical to the unforked run.
        let a = digest(&trace_bytes(m, &reference));
        let b = digest(&trace_bytes(m, &forked));
        if a == b && reference == forked {
            parents_clean += 1;
        } else {
            let bad = reference
                .iter()
                .zip(forked.iter())
                .position(|(x, y)| x != y)
                .unwrap_or(0);
            failures.push(format!(
                "match {m}: parent perturbed by forking, first difference at tick {bad}"
            ));
        }
        println!("match\t{m}\tparent_digest\t{}", hex(b));
    }

    let (fork_ns, child_ns) = if forks == 0 {
        (0, 0)
    } else {
        let n = u128::try_from(forks).unwrap();
        (fork_ns_total / n, child_ns_total / n)
    };
    println!("fork_equivalences\t{equivalent}/{attempted}");
    println!("parents_unperturbed\t{parents_clean}/{matches}");
    // The fork alone, in nanoseconds: microseconds are too coarse a unit for a
    // clone of a 7 kB world, and rounding it to 0 ms would hide the answer.
    println!("fork_ns_mean\t{fork_ns}");
    println!("fork_ms_mean\t0.{:06}", fork_ns % 1_000_000);
    println!("child_{CHILD_TICKS}_ticks_us_mean\t{}", child_ns / 1_000);
    println!("fork_bytes\t{fork_size}");
    for f in &failures {
        println!("FAIL\t{f}");
    }
    if !failures.is_empty() || equivalent != attempted || parents_clean != matches {
        std::process::exit(1);
    }
    println!("OK");
}
