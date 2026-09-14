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

/// Peak resident set size of this process, in bytes, from the platform API.
///
/// The plan's numbers table asks for peak RSS next to ticks/s, and a figure
/// measured by hand outside the harness is one CI can neither reproduce nor
/// regress on — so the harness prints it itself. Dependency-free on purpose:
/// the spike pins five crates and adding a sixth for one number would weaken
/// the point of the pinning.
///
/// Returns 0 when the platform is not one of the three the gate names, which is
/// honest: a 0 in the CI log reads as "not measured here", not as "no memory".
mod peak_rss {
    #[cfg(windows)]
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    // `K32GetProcessMemoryInfo` is exported from kernel32.dll (the psapi.dll
    // entry point of the same name is a forwarder), and kernel32 is linked into
    // every MSVC target already, so this needs no `#[link]` and no crate.
    #[cfg(windows)]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    #[cfg(windows)]
    #[must_use]
    pub fn bytes() -> u64 {
        let mut c = ProcessMemoryCounters {
            cb: 0,
            page_fault_count: 0,
            peak_working_set_size: 0,
            working_set_size: 0,
            quota_peak_paged_pool_usage: 0,
            quota_paged_pool_usage: 0,
            quota_peak_non_paged_pool_usage: 0,
            quota_non_paged_pool_usage: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
        };
        let size = u32::try_from(core::mem::size_of::<ProcessMemoryCounters>())
            .expect("PROCESS_MEMORY_COUNTERS size");
        c.cb = size;
        // SAFETY: `c` is a live, correctly sized PROCESS_MEMORY_COUNTERS and
        // the pseudo-handle from GetCurrentProcess is always valid.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut c, size) };
        if ok == 0 {
            0
        } else {
            u64::try_from(c.peak_working_set_size).unwrap_or(0)
        }
    }

    /// Linux: `VmHWM` in `/proc/self/status`, reported in kB.
    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn bytes() -> u64 {
        let Ok(s) = std::fs::read_to_string("/proc/self/status") else {
            return 0;
        };
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                let kb: u64 = rest
                    .trim()
                    .trim_end_matches(" kB")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                return kb.saturating_mul(1024);
            }
        }
        0
    }

    /// macOS: `getrusage(RUSAGE_SELF).ru_maxrss`, which is bytes on Darwin (it
    /// is kilobytes on Linux, which is why the two arms differ).
    #[cfg(target_os = "macos")]
    #[must_use]
    pub fn bytes() -> u64 {
        #[repr(C)]
        struct TimeVal {
            sec: i64,
            usec: i32,
            _pad: i32,
        }
        #[repr(C)]
        struct RUsage {
            ru_utime: TimeVal,
            ru_stime: TimeVal,
            ru_maxrss: i64,
            // The remaining 15 `long` counters. Declared so the struct is at
            // least as large as the kernel's, never smaller.
            _rest: [i64; 15],
        }
        unsafe extern "C" {
            fn getrusage(who: i32, usage: *mut RUsage) -> i32;
        }
        let mut u = RUsage {
            ru_utime: TimeVal { sec: 0, usec: 0, _pad: 0 },
            ru_stime: TimeVal { sec: 0, usec: 0, _pad: 0 },
            ru_maxrss: 0,
            _rest: [0; 15],
        };
        // SAFETY: `u` is live and no smaller than the kernel's struct rusage.
        let ok = unsafe { getrusage(0, &raw mut u) };
        if ok == 0 { u64::try_from(u.ru_maxrss).unwrap_or(0) } else { 0 }
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    #[must_use]
    pub fn bytes() -> u64 {
        0
    }
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
    // Measured after the run and after the trace buffer has been written, so it
    // is the peak of the whole process, trace buffer included.
    println!("peak_rss_bytes\t{}", peak_rss::bytes());
}

fn write_line(buf: &mut Vec<u8>, m: usize, t: u32, h: u64) {
    buf.extend_from_slice(m.to_string().as_bytes());
    buf.push(b'\t');
    buf.extend_from_slice(t.to_string().as_bytes());
    buf.push(b'\t');
    buf.extend_from_slice(hex(h).as_bytes());
    buf.push(b'\n');
}
