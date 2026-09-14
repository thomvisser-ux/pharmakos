// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic, clippy::as_conversions)]
//! G2 + P3 spike library: HPA\* pathing and the travel-time estimator.
//!
//! Everything in this crate root and in the seven modules below is **sim code**
//! in the sense of `AGENTS.md` §4: integer only, no floats, no `as` casts, no
//! hash-keyed containers, and no wall clock. The standard library's clock types
//! appear only in `src/bin/bench.rs` and `src/bin/accuracy.rs`; see
//! `clippy.toml`'s header for the mechanism that enforces that split, and
//! `mod wall` at the bottom of this file for the half of it that is a test.
//!
//! This file is itself under that test — it holds sim-side code ([`pairs`],
//! [`stats`], [`hash`], [`json`]) that would be just as fatal if it grew a hash
//! map or a float — so the prose here deliberately never spells the banned type
//! names out. That is the price of a text check, and it is cheap next to a hole
//! in the wall that nothing reports.
//!
//! Module map (plan §2):
//!
//! | Module | Does |
//! |---|---|
//! | [`world`] | 384×384×64 grid, the symmetric generator, surface walkability |
//! | [`cost`] | integer step costs, the octile heuristic, cost→ticks |
//! | [`astar`] | low-level A\* with a total-order tiebreak, and Dijkstra |
//! | [`clusters`] | HPA\* decomposition: entrances, intra edges, abstract graph |
//! | [`hpa`] | abstract search, refinement, smoothing |
//! | [`repair`] | craters, and the incremental rebuild an edit forces |
//! | [`estimate`] | the P3 query: abstract cost → ticks, with the fog ×3/2 rule |
//!
//! Three small helpers live here rather than in a module of their own so the
//! file list matches plan §2 exactly: [`hash`] (FNV-1a-64 for path identity and
//! splitmix64 for the deterministic query sets), [`stats`] (integer
//! percentiles and fixed-point formatting) and [`json`] (a twenty-line writer).

pub mod astar;
pub mod clusters;
pub mod cost;
pub mod estimate;
pub mod hpa;
pub mod repair;
pub mod world;

/// `u32` node id → slice index. Audited widening: every id in this crate comes
/// from [`world::node_of`], which bounds it below `world::NODES`.
#[inline]
#[must_use]
pub fn ix(id: u32) -> usize {
    usize::try_from(id).expect("u32 node id fits in usize on every target we build for")
}

/// Slice index → `u32` node id. Panics rather than truncating; the grid has
/// 147 456 nodes, three orders of magnitude below `u32::MAX` (plan §5).
#[inline]
#[must_use]
pub fn id32(i: usize) -> u32 {
    u32::try_from(i).expect("index fits in u32: the grid is 147 456 nodes")
}

/// Hashes. Ours, because the spike takes no dependencies (plan §3 step 0).
pub mod hash {
    /// FNV-1a, 64-bit. Used for the cross-OS path-identity check (plan §3
    /// step 9): a path is hashed as its node sequence, little-endian, so the
    /// hash cannot pick up target endianness or pointer width.
    #[must_use]
    pub fn fnv1a64(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// The FNV-1a-64 offset basis: the starting value of a running digest.
    pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

    /// Continue a running FNV-1a-64 over a node sequence, each node as four
    /// little-endian bytes. Start from [`FNV_BASIS`].
    ///
    /// The running form exists so the bench can fold *every* route the
    /// destruction stream accepts into one digest, rather than hashing only the
    /// queries taken on the pristine map.
    #[must_use]
    pub fn fnv1a64_feed(mut h: u64, nodes: &[u32]) -> u64 {
        for n in nodes {
            for b in n.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    /// FNV-1a-64 over a node sequence, each node as four little-endian bytes.
    #[must_use]
    pub fn fnv1a64_nodes(nodes: &[u32]) -> u64 {
        fnv1a64_feed(FNV_BASIS, nodes)
    }

    /// splitmix64. The deterministic stream behind every query set, crater
    /// placement and fog mask in the spike — one function, seeded per purpose,
    /// in the spirit of `AGENTS.md` §4.7's split streams.
    #[derive(Clone, Copy, Debug)]
    pub struct Rng(u64);

    impl Rng {
        #[must_use]
        pub const fn new(seed: u64) -> Self {
            Self(seed)
        }

        pub fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }

        /// Uniform-ish in `0..n`. Modulo bias is irrelevant for a test-set
        /// generator and the alternative costs a rejection loop.
        pub fn below(&mut self, n: i32) -> i32 {
            debug_assert!(n > 0);
            let n64 = u64::try_from(n).expect("positive");
            let v = self.next_u64() % n64;
            i32::try_from(v).expect("v < n, and n is an i32")
        }
    }
}

/// Integer percentiles and fixed-point formatting. No floats anywhere, so the
/// numbers in the JSON are the numbers that were measured.
pub mod stats {
    /// Nearest-rank percentile over an already-sorted slice.
    #[must_use]
    pub fn pct(sorted: &[i64], p: i64) -> i64 {
        if sorted.is_empty() {
            return 0;
        }
        let n = i64::try_from(sorted.len()).expect("sample count fits i64");
        let idx = ((p * (n - 1)) + 50) / 100;
        let i = usize::try_from(idx.clamp(0, n - 1)).expect("clamped into range");
        sorted[i]
    }

    #[must_use]
    pub fn max(sorted: &[i64]) -> i64 {
        sorted.last().copied().unwrap_or(0)
    }

    /// Nanoseconds → milliseconds, six decimals, as a JSON number. The bench
    /// times in nanoseconds because an abstract-only estimate can land under a
    /// microsecond and a p50 of "0" would be a measurement failure, not a
    /// result.
    #[must_use]
    pub fn ms_ns(ns: i64) -> String {
        let neg = ns < 0;
        let a = ns.abs();
        format!(
            "{}{}.{:06}",
            if neg { "-" } else { "" },
            a / 1_000_000,
            a % 1_000_000
        )
    }

    /// The five numbers of a nanosecond-valued distribution, in milliseconds.
    #[must_use]
    pub fn dist_ms_ns(samples: &mut [i64]) -> String {
        samples.sort_unstable();
        format!(
            "{{\"n\":{},\"p50\":{},\"p90\":{},\"p99\":{},\"max\":{}}}",
            samples.len(),
            ms_ns(pct(samples, 50)),
            ms_ns(pct(samples, 90)),
            ms_ns(pct(samples, 99)),
            ms_ns(max(samples))
        )
    }

    /// Microseconds → milliseconds, three decimals, as a JSON number.
    #[must_use]
    pub fn ms(us: i64) -> String {
        let neg = us < 0;
        let a = us.abs();
        format!("{}{}.{:03}", if neg { "-" } else { "" }, a / 1000, a % 1000)
    }

    /// Permille → percent, one decimal, as a JSON number.
    #[must_use]
    pub fn permille_as_pct(pm: i64) -> String {
        let neg = pm < 0;
        let a = pm.abs();
        format!("{}{}.{}", if neg { "-" } else { "" }, a / 10, a % 10)
    }

    /// Signed relative error in permille: `(got - want) * 1000 / want`.
    #[must_use]
    pub fn rel_permille(got: i64, want: i64) -> i64 {
        if want == 0 {
            return 0;
        }
        (got - want) * 1000 / want
    }

    /// The five numbers the plan's results table asks for, ready for JSON.
    #[must_use]
    pub fn dist_ms(samples: &mut [i64]) -> String {
        samples.sort_unstable();
        format!(
            "{{\"n\":{},\"p50\":{},\"p90\":{},\"p99\":{},\"max\":{}}}",
            samples.len(),
            ms(pct(samples, 50)),
            ms(pct(samples, 90)),
            ms(pct(samples, 99)),
            ms(max(samples))
        )
    }

    /// Same, for a permille-valued distribution rendered as percent.
    #[must_use]
    pub fn dist_pct(samples: &mut [i64]) -> String {
        samples.sort_unstable();
        let absmax = samples.iter().map(|v| v.abs()).max().unwrap_or(0);
        format!(
            "{{\"n\":{},\"p50\":{},\"p90\":{},\"p99\":{},\"max\":{},\"absmax\":{}}}",
            samples.len(),
            permille_as_pct(pct(samples, 50)),
            permille_as_pct(pct(samples, 90)),
            permille_as_pct(pct(samples, 99)),
            permille_as_pct(max(samples)),
            permille_as_pct(absmax)
        )
    }

    /// A distribution of |error|, which is what the gate is phrased against.
    #[must_use]
    pub fn dist_abs_pct(samples: &[i64]) -> String {
        let mut a: Vec<i64> = samples.iter().map(|v| v.abs()).collect();
        dist_pct(&mut a)
    }
}

/// The deterministic, distance-stratified query sets both binaries measure
/// over. Shared so that "the same distance stratification" (plan §3 steps 3, 5
/// and 6) is literally the same code and not two descriptions of one intent.
pub mod pairs {
    use crate::clusters::Clusters;
    use crate::hash::Rng;
    use crate::world::{W, World, coord_of, node_of};

    /// The plan's bands: ≤32, 32–96, 96–256, >256 cells of straight-line
    /// distance.
    pub const BAND_NAMES: [&str; 4] = ["le32", "32_96", "96_256", "gt256"];

    #[derive(Clone, Copy, Debug)]
    pub struct Pair {
        pub a: u32,
        pub b: u32,
        pub band: usize,
        pub dist: i32,
    }

    /// Straight-line distance in cells, as an integer square root. No floats,
    /// and no `as`: `i32::isqrt` is exact.
    #[must_use]
    pub fn straight_line(a: u32, b: u32) -> i32 {
        let (ax, az) = coord_of(a);
        let (bx, bz) = coord_of(b);
        let (dx, dz) = (bx - ax, bz - az);
        (dx * dx + dz * dz).isqrt()
    }

    #[must_use]
    pub fn band_of(d: i32) -> usize {
        if d <= 32 {
            0
        } else if d <= 96 {
            1
        } else if d <= 256 {
            2
        } else {
            3
        }
    }

    /// `n` connected pairs, as evenly spread over the four bands as the map
    /// allows. Deterministic in `seed`; never timed.
    #[must_use]
    pub fn stratified(world: &World, cl: &Clusters, n: usize, seed: u64) -> Vec<Pair> {
        let mut rng = Rng::new(seed);
        let quota = n.div_ceil(4);
        let mut count = [0usize; 4];
        let mut out: Vec<Pair> = Vec::with_capacity(n);
        let mut attempts = 0usize;
        while out.len() < n && attempts < n * 600 {
            attempts += 1;
            let a = node_of(rng.below(W), rng.below(W)).expect("in bounds");
            let b = node_of(rng.below(W), rng.below(W)).expect("in bounds");
            if a == b || !cl.connected(world, a, b) {
                continue;
            }
            let dist = straight_line(a, b);
            let band = band_of(dist);
            if count[band] >= quota {
                continue;
            }
            count[band] += 1;
            out.push(Pair { a, b, band, dist });
        }
        // If a band could not be filled, top up from anywhere rather than
        // returning short: the measurement wants n samples.
        while out.len() < n && attempts < n * 1200 {
            attempts += 1;
            let a = node_of(rng.below(W), rng.below(W)).expect("in bounds");
            let b = node_of(rng.below(W), rng.below(W)).expect("in bounds");
            if a == b || !cl.connected(world, a, b) {
                continue;
            }
            let dist = straight_line(a, b);
            out.push(Pair {
                a,
                b,
                band: band_of(dist),
                dist,
            });
        }
        out
    }

    /// Per-band counts of a pair set, for the JSON.
    #[must_use]
    pub fn band_counts(pairs: &[Pair]) -> [usize; 4] {
        let mut c = [0usize; 4];
        for p in pairs {
            c[p.band] += 1;
        }
        c
    }
}

/// A twenty-line JSON object writer. Values are emitted raw, so the caller
/// controls number formatting (and the formatting is integer — see [`stats`]).
pub mod json {
    #[derive(Debug, Default)]
    pub struct Obj {
        body: String,
    }

    impl Obj {
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        fn sep(&mut self) {
            if !self.body.is_empty() {
                self.body.push(',');
            }
        }

        pub fn int(&mut self, key: &str, v: i64) -> &mut Self {
            self.sep();
            self.body.push_str(&format!("\"{key}\":{v}"));
            self
        }

        pub fn bool(&mut self, key: &str, v: bool) -> &mut Self {
            self.sep();
            self.body.push_str(&format!("\"{key}\":{v}"));
            self
        }

        pub fn str(&mut self, key: &str, v: &str) -> &mut Self {
            self.sep();
            let esc = v.replace('\\', "\\\\").replace('"', "\\\"");
            self.body.push_str(&format!("\"{key}\":\"{esc}\""));
            self
        }

        /// A pre-rendered JSON value (number, object or array).
        pub fn raw(&mut self, key: &str, v: &str) -> &mut Self {
            self.sep();
            self.body.push_str(&format!("\"{key}\":{v}"));
            self
        }

        #[must_use]
        pub fn render(&self) -> String {
            format!("{{{}}}", self.body)
        }
    }
}

#[cfg(test)]
mod wall {
    //! The half of the determinism wall that clippy cannot express.
    //!
    //! `clippy.toml`'s `disallowed-types` is crate-wide, and plan §5 sanctions
    //! the standard clock in `src/bin/` — so the clock ban and the hash-map ban
    //! are checked here instead, against the library's source text, embedded at
    //! compile time by `include_str!`. Crude on purpose: it travels with the
    //! code rather than with the working directory, and an `#[allow]` cannot
    //! widen it.
    //!
    //! `lib.rs` is in the list, because it is sim code too. The list is
    //! hand-maintained, which is a hole of its own — a module added to the crate
    //! and forgotten here would escape both tests silently — so
    //! [`no_library_module_escapes_the_list`] walks `src/*.rs` and fails if any
    //! file is missing from it.

    const MODULES: [(&str, &str); 8] = [
        ("lib.rs", include_str!("lib.rs")),
        ("world.rs", include_str!("world.rs")),
        ("cost.rs", include_str!("cost.rs")),
        ("astar.rs", include_str!("astar.rs")),
        ("clusters.rs", include_str!("clusters.rs")),
        ("hpa.rs", include_str!("hpa.rs")),
        ("repair.rs", include_str!("repair.rs")),
        ("estimate.rs", include_str!("estimate.rs")),
    ];

    /// Assembled from fragments so this file's own source does not trip the
    /// check — `lib.rs` is one of the files being checked.
    fn banned() -> Vec<String> {
        let clock = ["Inst", "ant"].concat();
        let sys = ["System", "Time"].concat();
        let dur = ["Dura", "tion"].concat();
        let map = ["Hash", "Map"].concat();
        let set = ["Hash", "Set"].concat();
        let rs = ["Random", "State"].concat();
        let float_single = ["f", "32"].concat();
        let float_double = ["f", "64"].concat();
        vec![clock, sys, dur, map, set, rs, float_single, float_double]
    }

    #[test]
    fn library_has_no_clock_no_hash_map_and_no_float_type() {
        let banned = banned();
        let mut bad: Vec<String> = Vec::new();
        for (name, src) in MODULES {
            for b in &banned {
                if src.contains(b.as_str()) {
                    bad.push(format!("{name} mentions {b}"));
                }
            }
        }
        assert!(bad.is_empty(), "determinism wall breached: {bad:?}");
    }

    /// The list above is hand-maintained; this is what stops it from silently
    /// going stale. A new module under `src/` that nobody added to `MODULES`
    /// would otherwise be exempt from both checks above, and nothing would say
    /// so — the failure mode `AGENTS.md` §4.9 warns about.
    #[test]
    fn no_library_module_escapes_the_list() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut missing: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(dir).expect("src/ is readable") {
            let entry = entry.expect("a readable directory entry");
            if !entry.file_type().expect("a file type").is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".rs") {
                continue;
            }
            if !MODULES.iter().any(|(m, _)| *m == name) {
                missing.push(name);
            }
        }
        missing.sort();
        assert!(
            missing.is_empty(),
            "these library modules are not in `MODULES`, so the wall does not cover them: {missing:?}"
        );
    }

    #[test]
    fn every_library_module_carries_the_per_file_deny() {
        let want = [
            "#![deny(clippy::float_arithmetic",
            "clippy::as_conversions)]",
        ];
        for (name, src) in MODULES {
            for w in want {
                assert!(src.contains(w), "{name} is missing `{w}`");
            }
        }
    }
}

#[cfg(test)]
mod helper_tests {
    use super::{hash, id32, ix, stats};

    #[test]
    fn index_conversions_round_trip() {
        for i in [0usize, 1, 383, 147_455] {
            assert_eq!(ix(id32(i)), i);
        }
    }

    #[test]
    fn fnv_matches_the_reference_vector() {
        // The canonical FNV-1a-64 test vector.
        assert_eq!(hash::fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(hash::fnv1a64(b"foobar"), 0x8594_4171_f739_67e8_u64);
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let v: Vec<i64> = (0..101).collect();
        assert_eq!(stats::pct(&v, 0), 0);
        assert_eq!(stats::pct(&v, 50), 50);
        assert_eq!(stats::pct(&v, 99), 99);
        assert_eq!(stats::pct(&v, 100), 100);
    }

    #[test]
    fn fixed_point_formatting_is_exact() {
        assert_eq!(stats::ms(1234), "1.234");
        assert_eq!(stats::ms(7), "0.007");
        assert_eq!(stats::permille_as_pct(-83), "-8.3");
        assert_eq!(stats::permille_as_pct(150), "15.0");
    }
}
