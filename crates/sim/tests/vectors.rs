// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reference vectors and pinned literals for the determinism core.
//!
//! Everything here is pinned against a **literal**, never against a second
//! evaluation of the same expression: `digest(x) == xxh3_64_with_seed(x, SEED)`
//! is a tautology that cannot fail and therefore cannot catch anything. If a
//! test in this file moves, a golden file in the project moved with it, and the
//! pull request owes the explanation (AGENTS.md §5).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_sim::encoding::{ENCODING_VERSION, Enc, STATE_HASH_SEED, digest, hex};
use pharmakos_sim::math::fixed::{Angle, Fx, QUARTER_TURN, SIN_TABLE_LEN, Sq, cos, sin, sin_table};
use pharmakos_sim::math::quantity::{Hp, Kw, MS_PER_TICK, Money, Ms, TICK_HZ, Tick};
use pharmakos_sim::math::random::{Stream, StreamRng, mix64};
use xxhash_rust::xxh3::{xxh3_64, xxh3_64_with_seed};

const PRIME32: u64 = 2_654_435_761;
const PRIME64: u64 = 11_400_714_785_074_694_797;

/// The upstream sanity buffer from `xsum_sanity_check.c`:
///
/// ```text
/// byteGen = PRIME32;
/// for i in 0..len { buffer[i] = byteGen >> 56; byteGen *= PRIME64; }
/// ```
fn sanity_buffer(len: usize) -> Vec<u8> {
    let mut byte_gen: u64 = PRIME32;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        out.push(u8::try_from(byte_gen >> 56).expect("the top byte of a u64 fits in u8"));
        byte_gen = byte_gen.wrapping_mul(PRIME64);
    }
    out
}

/// `(len, seed, expected)` from upstream `XSUM_XXH3_testdata`. The comment on
/// each row is the size class it exercises, which is why upstream chose these
/// lengths: between them they cover every code path in XXH3-64.
const XXH3_VECTORS: &[(usize, u64, u64)] = &[
    (0, 0, 0x2D06_8005_38D3_94C2),       // empty input, seed 0
    (0, PRIME64, 0xA8A6_B918_B2F0_364A), // empty input, seeded
    (1, 0, 0xC44B_DFF4_074E_ECDB),       // 1..3
    (1, PRIME64, 0x032B_E332_DD76_6EF8),
    (6, 0, 0x27B5_6A84_CD2D_7325), // 4..8
    (6, PRIME64, 0x8458_9C11_6AB5_9AB9),
    (12, 0, 0xA713_DAF0_DFBB_77E7), // 9..16
    (12, PRIME64, 0xE730_3E1B_2336_DE0E),
    (24, 0, 0xA3FE_70BF_9D35_10EB), // 17..32
    (24, PRIME64, 0x850E_80FC_35BD_D690),
    (48, 0, 0x397D_A259_ECBA_1F11), // 33..64
    (48, PRIME64, 0xADC2_CBAA_44AC_C616),
    (80, 0, 0xBCDE_FBBB_2C47_C90A), // 65..96
    (80, PRIME64, 0xC6DD_0CB6_9953_2E73),
    (195, 0, 0xCD94_217E_E362_EC3A), // 129..240
    (195, PRIME64, 0xBA68_003D_370C_B3D9),
    (403, 0, 0xCDEB_804D_65C6_DEA4), // >240, last stripe overlapping
    (403, PRIME64, 0x6259_F6EC_FD64_43FD),
    (512, 0, 0x617E_4959_9013_CB6B), // >240, ends on a stripe boundary
    (512, PRIME64, 0x3CE4_57DE_14C2_7708),
    (2048, 0, 0xDD59_E2C3_A5F0_38E0), // two blocks
    (2048, PRIME64, 0x66F8_1670_669A_BABC),
    (2240, 0, 0x6E73_A905_39CF_2948), // three blocks
    (2240, PRIME64, 0x757B_A848_7D1B_5247),
    (2367, 0, 0xCB37_AEB9_E5D3_61ED), // three blocks, last stripe overlapping
    (2367, PRIME64, 0xD2DB_3415_B942_B42A),
];

#[test]
fn xxh3_empty_input_reference_vector() {
    // The vector that is quoted everywhere and anchors the whole table.
    assert_eq!(xxh3_64(b""), 0x2D06_8005_38D3_94C2);
    assert_eq!(xxh3_64_with_seed(b"", 0), 0x2D06_8005_38D3_94C2);
}

#[test]
fn xxh3_reference_vectors() {
    assert_eq!(XXH3_VECTORS.len(), 26, "the upstream table is 26 vectors");
    for &(len, seed, expected) in XXH3_VECTORS {
        let buffer = sanity_buffer(len);
        let got = xxh3_64_with_seed(&buffer, seed);
        assert_eq!(
            got, expected,
            "XXH3-64 mismatch at len={len} seed={seed:#018x}: got {got:#018x}, want {expected:#018x}"
        );
    }
}

#[test]
fn xxh3_unseeded_matches_seed_zero() {
    for &(len, _, _) in XXH3_VECTORS {
        let buffer = sanity_buffer(len);
        assert_eq!(xxh3_64(&buffer), xxh3_64_with_seed(&buffer, 0), "len={len}");
    }
}

#[test]
fn the_project_seed_and_its_digests_are_pinned() {
    assert_eq!(STATE_HASH_SEED, 0x5048_4152_4D4B_4F53);
    assert_eq!(STATE_HASH_SEED.to_be_bytes(), *b"PHARMKOS");
    // Literals, not a second evaluation of `digest`: these are the two values
    // decisions log item 48 pinned, and they are what proves the seed and the
    // hash crate together still produce what every golden file was stamped
    // with.
    assert_eq!(digest(b""), 0xFE1A_F732_B028_10AD);
    assert_eq!(digest(b"pharmakos/g4"), 0xE170_26C8_A5EA_4D30);
}

#[test]
fn the_encoder_carries_its_version_byte_on_every_path() {
    assert_eq!(ENCODING_VERSION, 1);
    for enc in [
        Enc::default(),
        Enc::with_capacity(0),
        Enc::with_capacity(64),
    ] {
        assert_eq!(
            enc.as_bytes().first().copied(),
            Some(ENCODING_VERSION),
            "an encoder reached a caller without its version byte"
        );
    }
    let mut enc = Enc::with_capacity(16);
    enc.u64(1);
    enc.clear();
    assert_eq!(enc.as_bytes(), &[ENCODING_VERSION]);
}

#[test]
fn the_encoding_is_little_endian_and_fixed_stride() {
    let mut enc = Enc::with_capacity(64);
    enc.u32(0x0102_0304);
    enc.opt_u32(None);
    enc.opt_u32(Some(7));
    enc.bool(true);
    assert_eq!(
        enc.as_bytes(),
        &[
            ENCODING_VERSION,
            0x04,
            0x03,
            0x02,
            0x01, // u32, little-endian
            0,
            0,
            0,
            0,
            0, // absent Option: tag then four zero bytes
            1,
            7,
            0,
            0,
            0, // present Option: tag then the value
            1, // bool
        ]
    );
    // The absent and present arms occupy the same five bytes, which is what
    // "fixed stride" means: a field can never be confused with its neighbour.
    let mut absent = Enc::with_capacity(8);
    absent.opt_u32(None);
    let mut present = Enc::with_capacity(8);
    present.opt_u32(Some(u32::MAX));
    assert_eq!(absent.encoded_len(), present.encoded_len());
}

#[test]
fn rng_construction_is_pinned() {
    assert_eq!(mix64(1), 0x5692_161D_100B_05E5);
    assert_eq!(mix64(0x9E37_79B9_7F4A_7C15), 0xE220_A839_7B1D_CDAF);

    let mut combat = StreamRng::new(0x0102_0304_0506_0708, Stream::Combat, 10, 1, 7);
    assert_eq!(combat.next_u64(), 0xCDC6_5EE9_1829_2C78);
    assert_eq!(combat.next_u64(), 0x1005_52F8_977C_9F98);

    // The same position in a different stream is a different sequence, which is
    // the whole point of splitting them.
    let mut spawn = StreamRng::new(0x0102_0304_0506_0708, Stream::Spawn, 10, 1, 7);
    assert_eq!(spawn.next_u64(), 0x0A84_9AA1_ED23_D46C);

    let mut zero = StreamRng::new(0, Stream::Combat, 0, 0, 0);
    assert_eq!(zero.next_u64(), 0x1957_A760_4E21_5178);

    let mut ranged = StreamRng::new(0x0102_0304_0506_0708, Stream::Combat, 10, 1, 7);
    let sequence: Vec<i32> = (0..8).map(|_| ranged.range_i32(-3, 3)).collect();
    assert_eq!(
        sequence,
        vec![2, -3, 3, -2, 0, -2, -1, 2],
        "range_i32(-3, 3) at a fixed position moved"
    );
}

#[test]
fn stream_ids_are_explicit_dense_and_distinct() {
    assert_eq!(Stream::Combat.id(), 1);
    assert_eq!(Stream::Spawn.id(), 2);
    assert_eq!(Stream::Map.id(), 3);
    let mut ids: Vec<u64> = Stream::ALL.iter().map(|s| s.id()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), Stream::ALL.len(), "two streams share an id");
    assert_eq!(ids.first().copied(), Some(1), "stream ids start at 1");
}

#[test]
fn a_draw_is_a_pure_function_of_its_position() {
    // Two generators built at the same position agree, and neither is affected
    // by the other having been drawn from: there is no shared state to shift.
    let mut a = StreamRng::new(9, Stream::Map, 4, 2, 11);
    let mut b = StreamRng::new(9, Stream::Map, 4, 2, 11);
    for _ in 0..4 {
        let _ = a.next_u64();
    }
    let mut c = StreamRng::new(9, Stream::Map, 4, 2, 11);
    assert_eq!(b.next_u64(), c.next_u64());
}

#[test]
fn range_stays_inside_its_bounds() {
    let mut rng = StreamRng::new(0xDEAD_BEEF, Stream::Combat, 0, 0, 0);
    for _ in 0..10_000 {
        let value = rng.range_i32(-3, 3);
        assert!((-3..=3).contains(&value), "out of range: {value}");
    }
    let mut degenerate = StreamRng::new(1, Stream::Map, 0, 0, 0);
    assert_eq!(degenerate.range_i32(5, 5), 5);
    // The full i32 span: `hi - lo + 1` is exactly 2^32, the only value that
    // stresses the widening in `range_i32`.
    let mut wide = StreamRng::new(0xA5A5_A5A5, Stream::Combat, 7, 2, 9);
    let mut saw_negative = false;
    let mut saw_positive = false;
    for _ in 0..1_000 {
        let value = wide.range_i32(i32::MIN, i32::MAX);
        saw_negative |= value < 0;
        saw_positive |= value > 0;
    }
    assert!(saw_negative && saw_positive, "the full span collapsed");
}

#[test]
fn fixed_point_rounding_is_pinned() {
    assert_eq!(Fx::ONE.raw(), 1 << 16);
    assert_eq!(Fx::from_voxels(3).raw(), 3 << 16);
    assert_eq!(Fx::from_voxels(-3).raw(), -3 << 16);
    // The shift is arithmetic, so multiplication floors toward negative
    // infinity rather than truncating toward zero. That is the documented
    // rounding and it is what the goldens pin.
    let half = Fx::from_raw(1 << 15);
    assert_eq!(
        Fx::from_voxels(-1).saturating_mul_fx(half).raw(),
        -(1 << 15)
    );
    assert_eq!(Fx::from_voxels(1).saturating_mul_fx(half).raw(), 1 << 15);
    assert_eq!(Fx::from_raw(-1).floor_voxels(), -1, "floor, not truncate");
    // Saturation, never a wrap.
    assert_eq!(
        Fx::from_raw(i32::MAX).saturating_add(Fx::ONE).raw(),
        i32::MAX
    );
    assert_eq!(Fx::from_raw(i32::MAX).checked_add(Fx::ONE), None);
    assert_eq!(Fx::from_raw(5).clamp_to(Fx::ZERO, Fx::from_raw(3)).raw(), 3);
}

#[test]
fn squared_distance_never_takes_a_root() {
    let a = [Fx::ZERO, Fx::ZERO, Fx::ZERO];
    let b = [Fx::from_voxels(3), Fx::from_voxels(4), Fx::ZERO];
    assert_eq!(Sq::between(a, b), Sq::of_radius(Fx::from_voxels(5)));
    assert!(Sq::between(a, b) > Sq::of_radius(Fx::from_voxels(4)));
    assert_eq!(Sq::between(a, a), Sq::ZERO);
}

#[test]
fn the_sine_table_is_integer_built_and_pinned() {
    let table = sin_table();
    assert_eq!(table.len(), SIN_TABLE_LEN);
    assert_eq!(table.first().copied(), Some(0), "sin(0) is 0");
    assert_eq!(table.get(1024).copied(), Some(1 << 16), "sin(pi/2) is one");
    assert_eq!(table.get(2048).copied(), Some(0), "sin(pi) is 0");
    assert_eq!(
        table.get(3072).copied(),
        Some(-(1 << 16)),
        "sin(3pi/2) is -one"
    );
    // The whole table digested, so a single moved entry is a failure rather
    // than something only three spot checks would have to be lucky to catch.
    let mut enc = Enc::with_capacity(SIN_TABLE_LEN * 4 + 8);
    for value in table {
        enc.i32(*value);
    }
    assert_eq!(
        hex(enc.finish()),
        "47875e0328c9c307",
        "the sine table moved; every heading in the project moved with it"
    );
    assert_eq!(sin(Angle::ZERO), Fx::ZERO);
    assert_eq!(cos(Angle::ZERO), Fx::ONE);
    assert_eq!(sin(Angle::from_raw(QUARTER_TURN)), Fx::ONE);
}

#[test]
fn a_heading_turns_the_short_way_and_ties_are_total() {
    let north = Angle::from_raw(QUARTER_TURN);
    assert_eq!(Angle::ZERO.turn_toward(north, 65_535), north);
    // Exactly half a turn: the positive direction wins, so two machines never
    // disagree about which way a unit turns.
    let half = Angle::from_raw(32_768);
    assert_eq!(Angle::ZERO.turn_toward(half, 16).raw(), 16);
    assert_eq!(
        Angle::from_delta(Fx::ONE, Fx::ZERO),
        Angle::ZERO,
        "+x is heading zero"
    );
    assert_eq!(
        Angle::from_delta(Fx::ZERO, Fx::ONE).raw(),
        QUARTER_TURN,
        "+y is a quarter turn"
    );
    assert_eq!(Angle::from_delta(Fx::ZERO, Fx::ZERO), Angle::ZERO);
}

#[test]
fn ms_per_tick_agrees_with_tick_hz() {
    assert_eq!(i64::from(MS_PER_TICK) * i64::from(TICK_HZ), 1_000);
    assert_eq!(Ms::new(3_000).to_ticks_floor(), 60);
    assert_eq!(Ms::new(3_000).to_ticks_ceil(), 60);
    assert_eq!(Ms::new(49).to_ticks_floor(), 0);
    assert_eq!(
        Ms::new(49).to_ticks_ceil(),
        1,
        "a timeout is never cut short"
    );
    assert_eq!(Ms::new(-1).to_ticks_floor(), 0);
    assert_eq!(Ms::new(-1).to_ticks_ceil(), 0);
    assert_eq!(Ms::from_ticks(60), Ms::new(3_000));
    assert_eq!(Ms::from_ticks(u32::MAX), Ms::new(i32::MAX), "saturates");
}

#[test]
fn the_quantities_say_what_they_do_at_the_limit() {
    assert_eq!(Hp::new(10).damaged_by(Hp::new(30)), Hp::ZERO);
    assert!(!Hp::ZERO.is_alive());
    assert!(Hp::new(1).is_alive());
    assert_eq!(Money::new(i64::MAX).checked_add(Money::new(1)), None);
    assert_eq!(Kw::new(i32::MIN).checked_sub(Kw::new(1)), None);
    assert_eq!(Tick::new(u32::MAX).next(), None);
    assert_eq!(Tick::new(4).since(Tick::new(9)), 0, "saturates at zero");
    assert_eq!(Tick::new(9).since(Tick::new(4)), 5);
}
