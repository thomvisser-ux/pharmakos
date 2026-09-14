// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `roundtrip` — save / restore round trips, in whichever snapshot format was
//! compiled in.
//!
//! For each of the ten matches, the uninterrupted run is traced once and
//! snapshots are written at ticks `{0, 1, 137, 4800, 9599}`. Each snapshot is
//! then restored **in a fresh process**: this binary re-invokes itself with
//! `--restore FILE`, the child restores, emits its own state hash and every tick
//! hash to the end of the match, and the parent byte-compares that tail against
//! the uninterrupted trace.
//!
//! Every checkpoint of every match also prints
//! `snapshot<TAB>match<TAB>tick<TAB>bytes<TAB>file_hash`, and the same lines go
//! to `--snapshots FILE` (default `snapshots-<format>.txt`) so the CI compare
//! job can byte-compare snapshot hashes across the three runners exactly the way
//! it compares traces. That list is the evidence for the plan's "snapshot byte
//! identity across OSes, per format" row.
//!
//! A fresh process is the whole point. Restoring in-process would share the
//! parent's allocator state, its lazily built tables and its warm caches, and
//! would prove nothing about a save written on Monday and loaded on Tuesday.
//!
//! Exits non-zero on any mismatch.

use std::io::Write;
use std::process::Command;
use std::time::Instant;

use g4_determinism::hash::{Enc, digest, hex};
use g4_determinism::snapshot::{Snapshot, decode, encode, format_name};
use g4_determinism::{MATCH_SEEDS, MATCH_TICKS, World};

/// Snapshot points inside a match: the first tick, the tick after it, an odd
/// non-aligned tick, the midpoint, and the last tick.
const CHECKPOINTS: [u32; 5] = [0, 1, 137, 4_800, 9_599];

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
    if let Some(path) = arg_value(&args, "--restore") {
        child_restore(&path, &args);
        return;
    }
    parent(&args);
}

/// The fresh process. Restores, then runs to the end of the match, writing
/// `tick<TAB>hash` lines (binary mode, explicit `\n`) to `--out`.
fn child_restore(path: &str, args: &[String]) {
    let until: u32 = arg_value(args, "--until").map_or(MATCH_TICKS, |v| v.parse().expect("--until"));
    let out = arg_value(args, "--out").expect("--restore needs --out");

    let t0 = Instant::now();
    let bytes = std::fs::read(path).expect("read snapshot");
    let snap: Snapshot = decode(&bytes);
    let mut w: World = snap.restore();
    let restore_ns = t0.elapsed().as_nanos();

    let mut enc = Enc::with_capacity(32 * 1024);
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);

    // First line: the restored state's own hash at its own tick. This is the
    // hash-transparency assertion; everything after it is the tail.
    w.encode(&mut enc);
    push_line(&mut buf, w.tick, enc.finish());
    while w.tick + 1 < until {
        w.step();
        w.encode(&mut enc);
        push_line(&mut buf, w.tick, enc.finish());
    }

    let mut f = std::fs::File::create(&out).expect("create tail file");
    f.write_all(&buf).expect("write tail file");
    f.sync_all().expect("sync tail file");
    println!("restore_ns\t{restore_ns}");
}

fn push_line(buf: &mut Vec<u8>, tick: u32, h: u64) {
    buf.extend_from_slice(tick.to_string().as_bytes());
    buf.push(b'\t');
    buf.extend_from_slice(hex(h).as_bytes());
    buf.push(b'\n');
}

fn parent(args: &[String]) {
    let ticks: u32 = arg_value(args, "--ticks").map_or(MATCH_TICKS, |v| v.parse().expect("--ticks"));
    let matches: usize = arg_value(args, "--matches").map_or(MATCH_SEEDS.len(), |v| {
        v.parse().expect("--matches")
    });
    let fmt = format_name();
    assert!(
        fmt != "none",
        "build with --features snap-rkyv or --features snap-postcard"
    );
    // Same bound `run` asserts. `.take(matches)` would otherwise clamp silently
    // to the ten defined seeds and report a pass over a smaller run than asked
    // for, which is a worse failure mode than refusing the argument.
    assert!(
        matches <= MATCH_SEEDS.len(),
        "only {} match seeds are defined",
        MATCH_SEEDS.len()
    );
    assert!(ticks > 0, "--ticks must be positive");

    let exe = std::env::current_exe().expect("current_exe");
    let dir = std::env::temp_dir().join(format!("g4-roundtrip-{fmt}"));
    std::fs::create_dir_all(&dir).expect("create scratch dir");

    let mut attempted = 0usize;
    let mut passed = 0usize;
    let mut save_ns_total: u128 = 0;
    let mut restore_ns_total: u128 = 0;
    let mut saves = 0usize;
    // (match, tick, bytes, file hash) for EVERY checkpoint of every match, not
    // just match 0. These hashes are the only evidence behind step 6's "the
    // snapshot file's own hash on each OS" and step 7's cross-OS byte identity
    // row; five values from one seed would let a format whose bytes are stable
    // for seed 1 and unstable for a seed with more credit entries pass unseen.
    let mut sizes: Vec<(usize, u32, usize, u64)> = Vec::new();
    let mut first_failure: Option<String> = None;

    println!("# g4-determinism roundtrip");
    println!("# format={fmt} matches={matches} ticks={ticks}");
    println!("# checkpoints={CHECKPOINTS:?}");

    for (m, &seed) in MATCH_SEEDS.iter().enumerate().take(matches) {

        // 1. The uninterrupted run: full trace, snapshots at the checkpoints.
        let mut w = World::new(seed);
        let mut enc = Enc::with_capacity(32 * 1024);
        let mut reference: Vec<u64> = Vec::with_capacity(usize::try_from(ticks).unwrap());
        let mut snap_paths: Vec<(u32, std::path::PathBuf)> = Vec::new();

        for t in 0..ticks {
            if t > 0 {
                w.step();
            }
            w.encode(&mut enc);
            reference.push(enc.finish());
            if CHECKPOINTS.contains(&t) {
                let t1 = Instant::now();
                let snap = Snapshot::capture(&w);
                let bytes = encode(&snap);
                let save_ns = t1.elapsed().as_nanos();
                save_ns_total += save_ns;
                saves += 1;

                let p = dir.join(format!("m{m}-t{t}.snap"));
                let mut f = std::fs::File::create(&p).expect("create snapshot");
                f.write_all(&bytes).expect("write snapshot");
                f.sync_all().expect("sync snapshot");
                sizes.push((m, t, bytes.len(), digest(&bytes)));
                snap_paths.push((t, p));
            }
        }

        // 2. Restore each checkpoint in a fresh process and compare the tail.
        for (t, p) in &snap_paths {
            attempted += 1;
            let tail_path = dir.join(format!("m{m}-t{t}.tail"));
            let t2 = Instant::now();
            let output = Command::new(&exe)
                .arg("--restore")
                .arg(p)
                .arg("--until")
                .arg(ticks.to_string())
                .arg("--out")
                .arg(&tail_path)
                .output()
                .expect("spawn restore child");
            let wall_ns = t2.elapsed().as_nanos();
            assert!(
                output.status.success(),
                "restore child failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            // The child reports its own restore cost; the parent's wall time
            // includes process creation, which is not a property of the format.
            let child_out = String::from_utf8_lossy(&output.stdout);
            let mut child_restore_ns: u128 = 0;
            for line in child_out.lines() {
                if let Some(v) = line.strip_prefix("restore_ns\t") {
                    child_restore_ns = v.trim().parse().unwrap_or(0);
                }
            }
            restore_ns_total += child_restore_ns;
            let _ = wall_ns;

            // Expected tail: reference[t..ticks], rendered exactly as the child
            // renders it, then compared byte for byte.
            let mut expect: Vec<u8> = Vec::with_capacity(64 * 1024);
            let start = usize::try_from(*t).unwrap();
            for (k, h) in reference.iter().enumerate().skip(start) {
                push_line(&mut expect, u32::try_from(k).unwrap(), *h);
            }
            let got = std::fs::read(&tail_path).expect("read tail file");
            if got == expect {
                passed += 1;
            } else if first_failure.is_none() {
                let bad = first_diff_line(&got, &expect);
                first_failure = Some(format!("match {m} checkpoint {t}: {bad}"));
            }
            let _ = std::fs::remove_file(&tail_path);
            let _ = std::fs::remove_file(p);
        }
    }

    // One `snapshot` line per checkpoint of every match: 5 x 10 = 50 lines, a
    // negligible cost, and the whole list is what the CI `compare` job diffs
    // between the three runners. Written to a file in binary mode with explicit
    // `\n` for exactly the reason the trace file is — a CRLF would make the
    // cross-OS byte comparison fail for no snapshot-format reason at all.
    let snap_out = arg_value(args, "--snapshots")
        .unwrap_or_else(|| format!("snapshots-{fmt}.txt"));
    let mut snap_buf: Vec<u8> = Vec::with_capacity(sizes.len() * 48);
    for (m, t, bytes, h) in &sizes {
        let line = format!("snapshot\t{m}\t{t}\t{bytes}\t{}", hex(*h));
        println!("{line}");
        snap_buf.extend_from_slice(line.as_bytes());
        snap_buf.push(b'\n');
    }
    let mut sf = std::fs::File::create(&snap_out).expect("create snapshot hash file");
    sf.write_all(&snap_buf).expect("write snapshot hash file");
    sf.sync_all().expect("sync snapshot hash file");
    println!("snapshots_out\t{snap_out}");

    let save_us = if saves == 0 {
        0
    } else {
        save_ns_total / u128::try_from(saves).unwrap() / 1_000
    };
    let restore_us = if attempted == 0 {
        0
    } else {
        restore_ns_total / u128::try_from(attempted).unwrap() / 1_000
    };
    println!("format\t{fmt}");
    println!("roundtrips\t{passed}/{attempted}");
    println!("save_us_mean\t{save_us}");
    println!("restore_us_mean\t{restore_us}");
    if let Some(f) = &first_failure {
        println!("FAIL\t{f}");
    }
    let _ = std::fs::remove_dir_all(&dir);

    if passed != attempted {
        std::process::exit(1);
    }
    println!("OK");
}

fn first_diff_line(got: &[u8], expect: &[u8]) -> String {
    let g = String::from_utf8_lossy(got);
    let e = String::from_utf8_lossy(expect);
    let mut gl = g.lines();
    let mut el = e.lines();
    let mut n = 0usize;
    loop {
        match (gl.next(), el.next()) {
            (Some(a), Some(b)) => {
                if a != b {
                    return format!("line {n}: got `{a}` expected `{b}`");
                }
            }
            (None, None) => return "identical text but different bytes".to_owned(),
            (a, b) => {
                return format!("length differs at line {n}: got {a:?} expected {b:?}");
            }
        }
        n += 1;
    }
}
