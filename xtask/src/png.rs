// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The vista screenshot comparator: a minimal PNG reader and a golden compare.
//!
//! This is the assertion the `screenshot` step is *for*. The obvious shape of
//! that step — "run Godot, upload the PNG as an artefact" — asserts nothing:
//! cracks, missing faces and inverted winding, the bug class the screenshot
//! exists to catch, all go through a green tick (spike G1, section 10.12).
//! Three checks run here instead:
//!
//! 1. **Structural.** The file exists and decodes. `godot --headless` selects
//!    the dummy rendering driver, `frame_post_draw` never fires under it, and a
//!    screenshot coroutine awaiting it parks for ever — so "no image at all" is
//!    the single most likely failure and must be an error rather than an empty
//!    artefact directory.
//! 2. **Not blank.** A single-colour frame is a failed render, not a passed
//!    test, so the red channel's variance has to clear a floor.
//! 3. **Golden compare**, with a tolerance rather than byte equality: mean
//!    absolute difference per channel plus the fraction of pixels differing by
//!    more than a hard threshold. A tolerance because the two sides may be
//!    different rasterisers — G1 measured 1.14 % of pixels differing *at all*
//!    between a Quadro and lavapipe on identical geometry — while a
//!    back-face-culled hole or a missing chunk moves both statistics by far more
//!    than a rasteriser tie-break does. The tolerance is nonetheless tight: the
//!    two thresholds below are G1's *measured* shape, not the loose gate its
//!    script shipped with, because a gate wide enough to admit a missing chunk
//!    is a green tick that means nothing.
//!
//! # Why this is Rust and not the spike's Python
//!
//! `spikes/g1-remesh/ci/compare_vista.py` at the `spike-end` tag does the same
//! job in the standard library plus `zlib`, and shelling to it would be fewer
//! lines. It is written again here because of what the *checker's own test* has
//! to be: T3's acceptance is that the comparator passes on a committed fixture
//! and fails on a doctored copy, and if the comparator is a Python script then
//! that test skips wherever an interpreter is missing. A check that silently
//! checks nothing is the exact failure G1 section 10.12 wrote down. As a plain
//! `cargo test` it cannot skip, it runs inside `cargo xtask ci`'s own `test`
//! step on all three operating systems, and xtask keeps its no-dependency rule
//! — which bans crates, not code.
//!
//! Only 8-bit non-interlaced RGB and RGBA are handled, which is what Godot's
//! `Image.save_png` writes. Anything else is an error naming what it found.
//!
//! No floats anywhere: the statistics are integers and the thresholds are
//! compared by cross-multiplication, so the verdict is identical on every
//! platform. Percentages are rendered from integers for the report only.

use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// Thresholds
// ---------------------------------------------------------------------------

/// Mean absolute per-channel difference, in thousandths of one 0–255 level.
///
/// 4 = 0.004/255: G1 measured 0.0039/255 between two rasterisers on identical
/// geometry, and this is that figure rounded up to the next thousandth.
/// `spikes/g1-remesh/ci/compare_vista.py` shipped with 6.0/255, which is roughly
/// 1 500× looser: at that gate a missing chunk passes, and a gate a missing
/// chunk passes is a green tick that means nothing.
///
/// PLACEHOLDER: skeleton-plan section 7 decision 22 (recommended, not yet
/// logged) names these two numbers; the owner re-ratifies them at T16 against
/// the first real vista, and T20 promotes this step from skipping to required.
/// Keep both in one place so the change is one line and one PR.
pub(crate) const MAX_MEAN_THOUSANDTHS: u64 = 4;

/// Share of pixels allowed to differ by more than [`HARD_DELTA`] on any
/// channel, in parts per million. Zero: G1 measured 1.14 % of pixels differing
/// *at all* between a Quadro and lavapipe, and **none at all** by more than 32.
/// A rasteriser tie-break does not move a channel by 33 levels, so one pixel
/// that does is news. Re-ratified with the mean above at T16.
pub(crate) const MAX_HARD_PPM: u64 = 0;

/// A per-channel difference above this counts as a *hard* difference: a
/// rasteriser tie-break does not move a channel by 33 levels, a missing face
/// does.
pub(crate) const HARD_DELTA: u8 = 32;

/// Floor on the red channel's variance. Below it the frame is blank or
/// near-uniform and nothing was drawn.
///
/// PLACEHOLDER: owner/T16 re-ratifies this floor against the first real render.
/// 200 is chosen only so that the committed blank fixture (variance 0) fails and
/// the committed gradient fixture passes; the world is destructible voxels under
/// a permanent ash sky, so a legitimately low-contrast vista is not far-fetched
/// and a floor set too high fails a good render.
pub(crate) const MIN_VARIANCE: u64 = 200;

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

/// A decoded 8-bit image: `channels` interleaved bytes per pixel, row-major.
#[derive(Debug)]
pub(crate) struct Image {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) channels: usize,
    pub(crate) pixels: Vec<u8>,
}

impl Image {
    /// Pixel count, as the statistics need it.
    fn count(&self) -> Result<u64, String> {
        u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .ok_or_else(|| "image dimensions overflow".to_owned())
    }

    /// The red channel's integer variance over a sample of at most 100 000
    /// pixels, evenly strided. Sampled rather than exhaustive because the check
    /// is "is anything there at all", and strided sampling of a rendered frame
    /// answers that; the stride is derived from the size, so it is the same
    /// sample on every platform.
    pub(crate) fn red_variance(&self) -> Result<u64, String> {
        let count = usize::try_from(self.count()?)
            .map_err(|error| format!("image too large for this platform: {error}"))?;
        if count == 0 {
            return Ok(0);
        }
        let step = count.div_euclid(100_000).max(1);
        let mut samples: Vec<i64> = Vec::new();
        let mut index = 0;
        while index < count {
            let offset = index
                .checked_mul(self.channels)
                .ok_or_else(|| "pixel offset overflow".to_owned())?;
            let red = self
                .pixels
                .get(offset)
                .ok_or_else(|| "the pixel buffer is shorter than the header claims".to_owned())?;
            samples.push(i64::from(*red));
            index += step;
        }
        let n = i64::try_from(samples.len())
            .map_err(|error| format!("sample count does not fit: {error}"))?;
        let sum: i64 = samples.iter().copied().fold(0, i64::wrapping_add);
        let mean = sum.div_euclid(n);
        let mut total: i64 = 0;
        for value in &samples {
            let delta = value.saturating_sub(mean);
            total = total.saturating_add(delta.saturating_mul(delta));
        }
        u64::try_from(total.div_euclid(n))
            .map_err(|error| format!("variance does not fit: {error}"))
    }
}

/// The outcome of a golden comparison. Every field is an integer, so the
/// verdict does not depend on a platform's floating-point rounding.
#[derive(Debug)]
pub(crate) struct Diff {
    /// Pixels compared.
    pub(crate) pixels: u64,
    /// Pixels differing on any of the three colour channels at all.
    pub(crate) differing: u64,
    /// Pixels differing by more than [`HARD_DELTA`] on some channel.
    pub(crate) hard: u64,
    /// The largest single-channel difference seen.
    pub(crate) worst: u8,
    /// Sum of per-channel absolute differences over the three colour channels.
    pub(crate) total: u64,
}

impl Diff {
    /// Mean absolute per-channel difference, in thousandths of a 0–255 level.
    ///
    /// For the *report* only. [`Diff::check`] cross-multiplies instead, because
    /// this ratio is truncated: at the vista's own resolution two pixels wrong
    /// by a full 255 on every channel still render as `0.000`.
    pub(crate) fn mean_thousandths(&self) -> u64 {
        let denominator = self.pixels.saturating_mul(3);
        if denominator == 0 {
            return 0;
        }
        self.total.saturating_mul(1_000).div_euclid(denominator)
    }

    /// Share of hard differences, in parts per million.
    ///
    /// For the *report* only, and truncated like the mean above: at 1920×1080
    /// one hard pixel is 1 000 000 / 2 073 600 = 0 ppm. [`Diff::check`] never
    /// reads it, so that `MAX_HARD_PPM = 0` means the zero pixels it says.
    pub(crate) fn hard_ppm(&self) -> u64 {
        if self.pixels == 0 {
            return 0;
        }
        self.hard.saturating_mul(1_000_000).div_euclid(self.pixels)
    }

    /// `Ok(())` when the image is within the thresholds, `Err(report)` when it
    /// is not. The report names both statistics and what to look for, because
    /// the first thing anyone does with a red screenshot check is ask whether
    /// it is the renderer or the geometry.
    ///
    /// The verdict cross-multiplies rather than comparing the two rendered
    /// ratios, which are integer divisions and therefore truncate towards a
    /// pass. Comparing them would make `MAX_HARD_PPM = 0` mean "up to two hard
    /// pixels at 1920×1080" instead of the none at all it is documented to
    /// mean — a step reporting ok for an assertion it never made, which is the
    /// failure this whole module exists to prevent.
    pub(crate) fn check(&self, max_mean_thousandths: u64, max_hard_ppm: u64) -> Result<(), String> {
        let mean_ok = self.total.saturating_mul(1_000)
            <= max_mean_thousandths.saturating_mul(self.pixels.saturating_mul(3));
        let hard_ok =
            self.hard.saturating_mul(1_000_000) <= max_hard_ppm.saturating_mul(self.pixels);
        if mean_ok && hard_ok {
            return Ok(());
        }
        let mean = self.mean_thousandths();
        let hard = self.hard_ppm();
        Err(format!(
            "the vista differs from its golden: mean {} of 255 (limit {}), {} of {} pixels ({} %, \
             limit {} %) over {HARD_DELTA}, worst channel delta {}.\n      Look for cracks, \
             missing faces or inverted winding before regenerating the golden — a rasteriser \
             tie-break moves the mean, not the worst delta.",
            decimal(mean, 3),
            decimal(max_mean_thousandths, 3),
            // The counts are printed beside the shares because the shares are
            // truncated: a single hard pixel in a 1920×1080 vista fails the
            // gate and renders as "0.0000 % (limit 0.0000 %)", which without
            // the counts would read as a contradiction.
            self.hard,
            self.pixels,
            // Parts per million rendered with four decimal places *is* the
            // percentage: 20 000 ppm renders as "2.0000".
            decimal(hard, 4),
            decimal(max_hard_ppm, 4),
            self.worst,
        ))
    }

    /// A one-paragraph human report, used in the step log and in the
    /// `::notice::` annotation.
    pub(crate) fn describe(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(
            text,
            "mean abs diff {} of 255 (limit {})",
            decimal(self.mean_thousandths(), 3),
            decimal(MAX_MEAN_THOUSANDTHS, 3)
        );
        let _ = writeln!(
            text,
            "{} of {} pixels differ at all; {} differ by more than {HARD_DELTA} ({} %, limit {} %)",
            self.differing,
            self.pixels,
            self.hard,
            decimal(self.hard_ppm(), 4),
            decimal(MAX_HARD_PPM, 4)
        );
        let _ = write!(text, "worst single-channel delta {}", self.worst);
        text
    }
}

/// Renders `value`, scaled by `10^places`, as a decimal string — the report's
/// only formatting, and it is integer arithmetic so a percentage never drags a
/// float into a crate that forbids them.
fn decimal(value: u64, places: u32) -> String {
    let scale = 10_u64.pow(places);
    let whole = value.div_euclid(scale);
    let fraction = value.rem_euclid(scale);
    let width = usize::try_from(places).unwrap_or(0);
    format!("{whole}.{fraction:0width$}")
}

/// Compares two decoded images. A size mismatch is an error rather than a
/// difference: the vista must be rendered at the golden's resolution or the
/// comparison means nothing, and a windowed run is clamped by the xvfb screen
/// size (G1 asked for 1920×1080 on a 1920×1080 screen and got 1920×1061).
pub(crate) fn compare(shot: &Image, golden: &Image) -> Result<Diff, String> {
    if shot.width != golden.width || shot.height != golden.height {
        return Err(format!(
            "the screenshot is {}x{} but the golden is {}x{}; render at the golden's resolution \
             (note that a windowed run is clamped by the xvfb screen size)",
            shot.width, shot.height, golden.width, golden.height
        ));
    }
    let pixels = usize::try_from(shot.count()?)
        .map_err(|error| format!("image too large for this platform: {error}"))?;

    let mut total: u64 = 0;
    let mut differing: u64 = 0;
    let mut hard: u64 = 0;
    let mut worst: u8 = 0;

    for index in 0..pixels {
        let shot_base = index
            .checked_mul(shot.channels)
            .ok_or_else(|| "pixel offset overflow".to_owned())?;
        let golden_base = index
            .checked_mul(golden.channels)
            .ok_or_else(|| "pixel offset overflow".to_owned())?;
        let mut pixel_worst: u8 = 0;
        for channel in 0..3 {
            let left = *shot
                .pixels
                .get(shot_base + channel)
                .ok_or_else(|| "the screenshot's pixel buffer is short".to_owned())?;
            let right = *golden
                .pixels
                .get(golden_base + channel)
                .ok_or_else(|| "the golden's pixel buffer is short".to_owned())?;
            let delta = left.abs_diff(right);
            total = total.saturating_add(u64::from(delta));
            pixel_worst = pixel_worst.max(delta);
        }
        if pixel_worst > 0 {
            differing = differing.saturating_add(1);
        }
        if pixel_worst > HARD_DELTA {
            hard = hard.saturating_add(1);
        }
        worst = worst.max(pixel_worst);
    }

    Ok(Diff {
        pixels: shot.count()?,
        differing,
        hard,
        worst,
        total,
    })
}

// ---------------------------------------------------------------------------
// PNG container
// ---------------------------------------------------------------------------

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// Decodes an 8-bit non-interlaced RGB or RGBA PNG.
pub(crate) fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.get(..8) != Some(&SIGNATURE[..]) {
        return Err("not a PNG: the eight-byte signature is missing".to_owned());
    }
    let mut position: usize = 8;
    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut channels: usize = 0;
    let mut seen_header = false;
    let mut compressed: Vec<u8> = Vec::new();

    while position + 8 <= bytes.len() {
        let length = usize::try_from(read_u32(bytes, position)?)
            .map_err(|error| format!("chunk length does not fit: {error}"))?;
        let kind = bytes
            .get(position + 4..position + 8)
            .ok_or_else(|| "truncated chunk header".to_owned())?;
        let body_start = position + 8;
        let body_end = body_start
            .checked_add(length)
            .ok_or_else(|| "chunk length overflows".to_owned())?;
        let body = bytes
            .get(body_start..body_end)
            .ok_or_else(|| "a chunk runs past the end of the file".to_owned())?;

        if kind == b"IHDR" {
            width = read_u32(body, 0)?;
            height = read_u32(body, 4)?;
            let depth = *body.get(8).ok_or_else(|| "short IHDR".to_owned())?;
            let colour = *body.get(9).ok_or_else(|| "short IHDR".to_owned())?;
            let interlace = *body.get(12).ok_or_else(|| "short IHDR".to_owned())?;
            if depth != 8 || interlace != 0 || (colour != 2 && colour != 6) {
                return Err(format!(
                    "only 8-bit non-interlaced RGB or RGBA is handled (bit depth {depth}, colour \
                     type {colour}, interlace {interlace}) — that is what Godot's Image.save_png \
                     writes"
                ));
            }
            channels = if colour == 2 { 3 } else { 4 };
            seen_header = true;
        } else if kind == b"IDAT" {
            compressed.extend_from_slice(body);
        } else if kind == b"IEND" {
            break;
        }

        position = body_end
            .checked_add(4)
            .ok_or_else(|| "chunk CRC runs past the end of the file".to_owned())?;
    }

    if !seen_header {
        return Err("the PNG has no IHDR chunk".to_owned());
    }
    if compressed.is_empty() {
        return Err("the PNG has no image data (no IDAT chunk)".to_owned());
    }
    let raw = zlib_decompress(&compressed)?;
    let pixels = unfilter(&raw, width, height, channels)?;
    Ok(Image {
        width,
        height,
        channels,
        pixels,
    })
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| format!("expected four bytes at offset {offset}"))?;
    let mut value: u32 = 0;
    for byte in slice {
        value = value
            .checked_mul(256)
            .and_then(|shifted| shifted.checked_add(u32::from(*byte)))
            .ok_or_else(|| "big-endian u32 overflows".to_owned())?;
    }
    Ok(value)
}

/// Reverses the five PNG row filters.
fn unfilter(raw: &[u8], width: u32, height: u32, channels: usize) -> Result<Vec<u8>, String> {
    let width = usize::try_from(width).map_err(|error| format!("width: {error}"))?;
    let height = usize::try_from(height).map_err(|error| format!("height: {error}"))?;
    let stride = width
        .checked_mul(channels)
        .ok_or_else(|| "row stride overflows".to_owned())?;
    let expected = height
        .checked_mul(stride + 1)
        .ok_or_else(|| "image size overflows".to_owned())?;
    if raw.len() != expected {
        return Err(format!(
            "the decompressed image is {} bytes, but {height} rows of {stride} bytes plus one \
             filter byte each is {expected}",
            raw.len()
        ));
    }

    let mut out: Vec<u8> = vec![0; height * stride];
    let mut previous: Vec<u8> = vec![0; stride];
    let mut cursor: usize = 0;

    for row in 0..height {
        let filter = *raw
            .get(cursor)
            .ok_or_else(|| "missing row filter byte".to_owned())?;
        cursor += 1;
        let mut line: Vec<u8> = raw
            .get(cursor..cursor + stride)
            .ok_or_else(|| "truncated image row".to_owned())?
            .to_vec();
        cursor += stride;

        for index in 0..stride {
            let left = if index >= channels {
                i32::from(*line.get(index - channels).unwrap_or(&0))
            } else {
                0
            };
            let above = i32::from(*previous.get(index).unwrap_or(&0));
            let above_left = if index >= channels {
                i32::from(*previous.get(index - channels).unwrap_or(&0))
            } else {
                0
            };
            let addend: i32 = match filter {
                0 => 0,
                1 => left,
                2 => above,
                3 => (left + above).div_euclid(2),
                4 => paeth(left, above, above_left),
                other => {
                    return Err(format!("unknown PNG row filter {other} on row {row}"));
                }
            };
            let slot = line
                .get_mut(index)
                .ok_or_else(|| "row index out of range".to_owned())?;
            let sum = i32::from(*slot).wrapping_add(addend);
            *slot = u8::try_from(sum.rem_euclid(256))
                .map_err(|error| format!("filter arithmetic: {error}"))?;
        }

        let start = row * stride;
        let target = out
            .get_mut(start..start + stride)
            .ok_or_else(|| "output row out of range".to_owned())?;
        target.copy_from_slice(&line);
        previous = line;
    }

    Ok(out)
}

fn paeth(left: i32, above: i32, above_left: i32) -> i32 {
    let estimate = left + above - above_left;
    let da = (estimate - left).abs();
    let db = (estimate - above).abs();
    let dc = (estimate - above_left).abs();
    if da <= db && da <= dc {
        left
    } else if db <= dc {
        above
    } else {
        above_left
    }
}

// ---------------------------------------------------------------------------
// zlib / DEFLATE (RFC 1950 and RFC 1951)
// ---------------------------------------------------------------------------

fn zlib_decompress(stream: &[u8]) -> Result<Vec<u8>, String> {
    let cmf = *stream
        .first()
        .ok_or_else(|| "the zlib stream is empty".to_owned())?;
    let flg = *stream
        .get(1)
        .ok_or_else(|| "the zlib stream is truncated".to_owned())?;
    if cmf & 0x0f != 8 {
        return Err(format!(
            "zlib compression method {} is not DEFLATE",
            cmf & 0x0f
        ));
    }
    if (u32::from(cmf) * 256 + u32::from(flg)).rem_euclid(31) != 0 {
        return Err("the zlib header check failed".to_owned());
    }
    if flg & 0x20 != 0 {
        return Err("zlib preset dictionaries are not supported".to_owned());
    }
    let body = stream
        .get(2..)
        .ok_or_else(|| "the zlib stream has no body".to_owned())?;
    let (out, end) = inflate(body)?;

    // The adler32 trailer is checked rather than skipped: a screenshot that
    // arrives corrupted should say so, not become a geometry difference.
    if let Some(expected) = body.get(end..end + 4) {
        let mut stated: u32 = 0;
        for byte in expected {
            stated = stated
                .checked_mul(256)
                .and_then(|shifted| shifted.checked_add(u32::from(*byte)))
                .ok_or_else(|| "adler32 trailer overflows".to_owned())?;
        }
        let actual = adler32(&out);
        if stated != actual {
            return Err(format!(
                "the zlib adler32 checksum does not match ({stated:#010x} stated, {actual:#010x} \
                 computed): the file is corrupt"
            ));
        }
    }
    Ok(out)
}

fn adler32(data: &[u8]) -> u32 {
    let mut low: u32 = 1;
    let mut high: u32 = 0;
    for byte in data {
        low = (low + u32::from(*byte)).rem_euclid(65_521);
        high = (high + low).rem_euclid(65_521);
    }
    (high << 16) | low
}

/// A bit reader over a DEFLATE stream: least-significant bit first within a
/// byte, bytes in order.
struct Bits<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u32,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<u32, String> {
        let byte = *self
            .data
            .get(self.byte)
            .ok_or_else(|| "the DEFLATE stream is truncated".to_owned())?;
        let value = (u32::from(byte) >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.byte += 1;
        }
        Ok(value)
    }

    fn take(&mut self, count: u32) -> Result<u32, String> {
        let mut value: u32 = 0;
        for index in 0..count {
            value |= self.bit()? << index;
        }
        Ok(value)
    }

    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.byte += 1;
        }
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12_289, 16_385, 24_577,
];
const DISTANCE_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// The order in which the code-length code's own lengths are stored.
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// A canonical Huffman decoder, in the counts-and-symbols form zlib's own
/// `puff.c` uses: no table build, one bit at a time, and it cannot index out of
/// range because every step is bounded by the counts.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn build(lengths: &[u8]) -> Result<Huffman, String> {
        let mut counts = [0_u16; 16];
        for length in lengths {
            let slot = counts
                .get_mut(usize::from(*length))
                .ok_or_else(|| format!("code length {length} exceeds 15"))?;
            *slot += 1;
        }
        if let Some(zero) = counts.get_mut(0) {
            *zero = 0;
        }
        let mut offsets = [0_u16; 16];
        for length in 1..16 {
            let previous_offset = *offsets
                .get(length - 1)
                .ok_or_else(|| "offset table index".to_owned())?;
            let previous_count = *counts
                .get(length - 1)
                .ok_or_else(|| "count table index".to_owned())?;
            let slot = offsets
                .get_mut(length)
                .ok_or_else(|| "offset table index".to_owned())?;
            *slot = previous_offset + previous_count;
        }
        let mut symbols = vec![0_u16; lengths.len()];
        for (symbol, length) in lengths.iter().enumerate() {
            if *length == 0 {
                continue;
            }
            let offset = offsets
                .get_mut(usize::from(*length))
                .ok_or_else(|| "offset table index".to_owned())?;
            let position = usize::from(*offset);
            let slot = symbols
                .get_mut(position)
                .ok_or_else(|| "over-subscribed Huffman code".to_owned())?;
            *slot = u16::try_from(symbol)
                .map_err(|error| format!("symbol {symbol} does not fit: {error}"))?;
            *offset += 1;
        }
        Ok(Huffman { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, String> {
        let mut code: u32 = 0;
        let mut first: u32 = 0;
        let mut index: usize = 0;
        for length in 1..16 {
            code |= bits.bit()?;
            let count = u32::from(
                *self
                    .counts
                    .get(length)
                    .ok_or_else(|| "count table index".to_owned())?,
            );
            if code < first + count {
                let offset = usize::try_from(code - first)
                    .map_err(|error| format!("symbol offset: {error}"))?;
                return self
                    .symbols
                    .get(index + offset)
                    .copied()
                    .ok_or_else(|| "incomplete Huffman code".to_owned());
            }
            index += usize::try_from(count).map_err(|error| format!("count: {error}"))?;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("a Huffman code ran past 15 bits".to_owned())
    }
}

fn fixed_literal() -> Result<Huffman, String> {
    let mut lengths = vec![8_u8; 288];
    for (symbol, slot) in lengths.iter_mut().enumerate() {
        *slot = if symbol < 144 {
            8
        } else if symbol < 256 {
            9
        } else if symbol < 280 {
            7
        } else {
            8
        };
    }
    Huffman::build(&lengths)
}

fn fixed_distance() -> Result<Huffman, String> {
    Huffman::build(&[5_u8; 30])
}

/// Inflates a raw DEFLATE stream and returns the output plus the index of the
/// first byte after it (where zlib's adler32 trailer starts).
fn inflate(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let mut bits = Bits {
        data,
        byte: 0,
        bit: 0,
    };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.take(1)?;
        let kind = bits.take(2)?;
        match kind {
            0 => stored_block(&mut bits, &mut out)?,
            1 => {
                let literal = fixed_literal()?;
                let distance = fixed_distance()?;
                huffman_block(&mut bits, &mut out, &literal, &distance)?;
            }
            2 => {
                let (literal, distance) = dynamic_tables(&mut bits)?;
                huffman_block(&mut bits, &mut out, &literal, &distance)?;
            }
            _ => return Err("reserved DEFLATE block type 3".to_owned()),
        }
        if last == 1 {
            break;
        }
    }
    bits.align();
    Ok((out, bits.byte))
}

fn stored_block(bits: &mut Bits<'_>, out: &mut Vec<u8>) -> Result<(), String> {
    bits.align();
    let length = bits.take(16)?;
    let complement = bits.take(16)?;
    if length ^ 0xffff != complement {
        return Err("a stored DEFLATE block's length and its complement disagree".to_owned());
    }
    let count = usize::try_from(length).map_err(|error| format!("stored length: {error}"))?;
    for _ in 0..count {
        let byte = *bits
            .data
            .get(bits.byte)
            .ok_or_else(|| "a stored block runs past the end of the stream".to_owned())?;
        bits.byte += 1;
        out.push(byte);
    }
    Ok(())
}

fn dynamic_tables(bits: &mut Bits<'_>) -> Result<(Huffman, Huffman), String> {
    let literal_count =
        usize::try_from(bits.take(5)?).map_err(|error| format!("HLIT: {error}"))? + 257;
    let distance_count =
        usize::try_from(bits.take(5)?).map_err(|error| format!("HDIST: {error}"))? + 1;
    let code_count = usize::try_from(bits.take(4)?).map_err(|error| format!("HCLEN: {error}"))? + 4;

    let mut code_lengths = [0_u8; 19];
    for position in 0..code_count {
        let slot_index = *CODE_LENGTH_ORDER
            .get(position)
            .ok_or_else(|| "HCLEN exceeds 19".to_owned())?;
        let value = u8::try_from(bits.take(3)?).map_err(|error| format!("code length: {error}"))?;
        let slot = code_lengths
            .get_mut(slot_index)
            .ok_or_else(|| "code-length index".to_owned())?;
        *slot = value;
    }
    let code_huffman = Huffman::build(&code_lengths)?;

    let total = literal_count + distance_count;
    let mut lengths: Vec<u8> = Vec::with_capacity(total);
    while lengths.len() < total {
        let symbol = code_huffman.decode(bits)?;
        match symbol {
            0..=15 => {
                let length =
                    u8::try_from(symbol).map_err(|error| format!("code length: {error}"))?;
                lengths.push(length);
            }
            16 => {
                let previous = *lengths
                    .last()
                    .ok_or_else(|| "a repeat code with no previous length".to_owned())?;
                let repeat = 3 + usize::try_from(bits.take(2)?)
                    .map_err(|error| format!("repeat: {error}"))?;
                lengths.resize(lengths.len() + repeat, previous);
            }
            17 => {
                let repeat = 3 + usize::try_from(bits.take(3)?)
                    .map_err(|error| format!("repeat: {error}"))?;
                lengths.resize(lengths.len() + repeat, 0);
            }
            18 => {
                let repeat = 11
                    + usize::try_from(bits.take(7)?).map_err(|error| format!("repeat: {error}"))?;
                lengths.resize(lengths.len() + repeat, 0);
            }
            other => return Err(format!("code-length symbol {other} is out of range")),
        }
    }
    if lengths.len() != total {
        return Err("the code-length sequence overran its table".to_owned());
    }
    let literal = Huffman::build(
        lengths
            .get(..literal_count)
            .ok_or_else(|| "literal length table".to_owned())?,
    )?;
    let distance = Huffman::build(
        lengths
            .get(literal_count..)
            .ok_or_else(|| "distance length table".to_owned())?,
    )?;
    Ok((literal, distance))
}

fn huffman_block(
    bits: &mut Bits<'_>,
    out: &mut Vec<u8>,
    literal: &Huffman,
    distance: &Huffman,
) -> Result<(), String> {
    loop {
        let symbol = literal.decode(bits)?;
        if symbol < 256 {
            out.push(u8::try_from(symbol).map_err(|error| format!("literal: {error}"))?);
            continue;
        }
        if symbol == 256 {
            return Ok(());
        }
        let index = usize::from(symbol - 257);
        let base = *LENGTH_BASE
            .get(index)
            .ok_or_else(|| format!("length symbol {symbol} is out of range"))?;
        let extra = *LENGTH_EXTRA
            .get(index)
            .ok_or_else(|| format!("length symbol {symbol} is out of range"))?;
        let length = usize::try_from(u32::from(base) + bits.take(extra)?)
            .map_err(|error| format!("match length: {error}"))?;

        let distance_symbol = usize::from(distance.decode(bits)?);
        let distance_base = *DISTANCE_BASE
            .get(distance_symbol)
            .ok_or_else(|| format!("distance symbol {distance_symbol} is out of range"))?;
        let distance_extra = *DISTANCE_EXTRA
            .get(distance_symbol)
            .ok_or_else(|| format!("distance symbol {distance_symbol} is out of range"))?;
        let back = usize::try_from(u32::from(distance_base) + bits.take(distance_extra)?)
            .map_err(|error| format!("match distance: {error}"))?;
        if back > out.len() {
            return Err("a back-reference points before the start of the stream".to_owned());
        }
        let start = out.len() - back;
        for offset in 0..length {
            let byte = *out
                .get(start + offset)
                .ok_or_else(|| "back-reference out of range".to_owned())?;
            out.push(byte);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests — the checker is tested, not just the thing it checks
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The committed fixtures. `vista-fixture.png` stands in for a rendered
    /// vista until T16 produces one; `vista-fixture-doctored.png` is the same
    /// image with one patch of pixels moved, which is what a missing face or an
    /// inverted winding looks like to this comparator.
    fn fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("data")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|error| {
            panic!("reading the committed fixture {}: {error}", path.display())
        })
    }

    #[test]
    fn the_fixture_decodes() {
        let image = decode(&fixture("vista-fixture.png")).expect("the fixture is a readable PNG");
        assert_eq!((image.width, image.height), (160, 90));
        assert_eq!(image.channels, 3);
        assert_eq!(
            image.pixels.len(),
            160 * 90 * 3,
            "every pixel is present after unfiltering"
        );
    }

    #[test]
    fn a_fixture_compared_with_itself_passes() {
        let image = decode(&fixture("vista-fixture.png")).expect("decodes");
        let golden = decode(&fixture("vista-fixture.png")).expect("decodes");
        let diff = compare(&image, &golden).expect("same size");
        assert_eq!(diff.total, 0);
        assert_eq!(diff.differing, 0);
        assert_eq!(diff.hard, 0);
        diff.check(MAX_MEAN_THOUSANDTHS, MAX_HARD_PPM)
            .expect("an identical image is within every threshold");
    }

    #[test]
    fn a_doctored_fixture_fails() {
        let shot = decode(&fixture("vista-fixture-doctored.png")).expect("decodes");
        let golden = decode(&fixture("vista-fixture.png")).expect("decodes");
        let diff = compare(&shot, &golden).expect("same size");
        assert!(diff.hard > 0, "the doctored patch differs by more than 32");
        let report = diff
            .check(MAX_MEAN_THOUSANDTHS, MAX_HARD_PPM)
            .expect_err("a doctored vista must not pass");
        assert!(
            report.contains("differs from its golden"),
            "the report says what is wrong: {report}"
        );
        assert!(
            report.contains("inverted winding"),
            "the report says what to look for: {report}"
        );

        // The share is pinned by value, not only by substring. Rendering parts
        // per million as a percentage is one division, and getting it wrong by a
        // factor of a hundred produces a report that reads as
        // self-contradictory — "0.0833 % (limit 0.0200 %)" — and invites
        // loosening a threshold that is already looser than it looks. This is
        // the diagnostic that carries the `::notice::` annotation, the only
        // channel a logged-out viewer can read (G1 section 10.12).
        assert_eq!(diff.pixels, 14_400);
        assert_eq!(diff.hard, 1_200);
        assert_eq!(diff.hard_ppm(), 83_333);
        assert!(
            report.contains("8.3333 %"),
            "1 200 of 14 400 pixels is 8.3333 %, not 0.0833 %: {report}"
        );
        assert!(
            diff.describe().contains("8.3333 %"),
            "the annotation carries the same number: {}",
            diff.describe()
        );

        // And the limit is rendered on the same scale. Checked against the loose
        // gate the spike script shipped with, so that the two numbers in one
        // sentence are comparable.
        let loose = diff
            .check(6_000, 20_000)
            .expect_err("the doctored vista fails even the spike's generous gate");
        assert!(
            loose.contains("limit 2.0000 %"),
            "20 000 ppm is 2 %, not 0.02 %: {loose}"
        );
    }

    #[test]
    fn a_blank_frame_is_a_failed_render() {
        let blank = decode(&fixture("vista-fixture-blank.png")).expect("decodes");
        assert!(
            blank.red_variance().expect("variance") < MIN_VARIANCE,
            "a single-colour frame must not clear the variance floor"
        );
        let real = decode(&fixture("vista-fixture.png")).expect("decodes");
        assert!(
            real.red_variance().expect("variance") >= MIN_VARIANCE,
            "the fixture has real content in it"
        );
    }

    #[test]
    fn a_size_mismatch_is_an_error_not_a_difference() {
        let image = decode(&fixture("vista-fixture.png")).expect("decodes");
        let small = Image {
            width: 8,
            height: 8,
            channels: 3,
            pixels: vec![0; 8 * 8 * 3],
        };
        let error = compare(&image, &small).expect_err("sizes differ");
        assert!(error.contains("golden's resolution"), "{error}");
    }

    #[test]
    fn a_missing_signature_is_rejected() {
        let error = decode(b"not a png at all").expect_err("rejected");
        assert!(error.contains("signature"), "{error}");
    }

    #[test]
    fn corruption_is_caught_by_the_checksum() {
        let mut bytes = fixture("vista-fixture.png");
        // Flip a bit deep inside the IDAT payload. The adler32 trailer no
        // longer matches whatever inflates out of it, which is the point.
        let middle = bytes.len().div_euclid(2);
        if let Some(byte) = bytes.get_mut(middle) {
            *byte ^= 0x01;
        }
        assert!(
            decode(&bytes).is_err(),
            "a corrupted PNG must be an error, not a geometry difference"
        );
    }

    #[test]
    fn statistics_are_integers_and_read_correctly() {
        let diff = Diff {
            pixels: 1_000,
            differing: 40,
            hard: 20,
            worst: 64,
            total: 3_000,
        };
        // 3000 / (1000 * 3) = 1.000 levels of 255.
        assert_eq!(diff.mean_thousandths(), 1_000);
        // 20 of 1000 pixels = 2 %.
        assert_eq!(diff.hard_ppm(), 20_000);
        // The limits are passed in rather than taken from the constants, so that
        // re-ratifying the shipped thresholds at T16 does not quietly change
        // what this test is asserting.
        assert!(
            diff.check(1_000, 20_000).is_ok(),
            "exactly at the limit still passes"
        );
        assert!(
            diff.check(999, 20_000).is_err(),
            "one thousandth over the mean limit fails"
        );
        assert!(
            diff.check(1_000, 19_999).is_err(),
            "one part per million over the hard limit fails"
        );
        // 2 %, not 0.02 %: the failure report and the limit beside it are on the
        // same scale.
        let report = diff
            .check(1_000, 0)
            .expect_err("twenty hard pixels fail a zero-tolerance gate");
        assert!(report.contains("2.0000 %"), "{report}");
    }

    /// The boundary the shipped thresholds actually stand on, pinned before
    /// T16 makes the step live.
    ///
    /// `hard_ppm()` is a truncating division, so at the vista's own resolution
    /// (1920×1080 = 2 073 600 pixels) one hard pixel is 0 ppm and two are 0 ppm
    /// as well — comparing that rendered ratio against a limit of 0 would let
    /// two pixels wrong by a full 255 on every channel through a gate whose
    /// documentation, here and in `tests/golden/vista/README.md`, says **zero**.
    /// One missing face is one pixel before it is three.
    #[test]
    fn one_hard_pixel_fails_the_zero_tolerance_gate() {
        let diff = Diff {
            // VISTA_RESOLUTION, 1920 × 1080.
            pixels: 2_073_600,
            differing: 1,
            hard: 1,
            worst: 255,
            total: 765,
        };
        // Both rendered shares truncate to zero: the report cannot be what the
        // verdict is made of.
        assert_eq!(diff.hard_ppm(), 0);
        assert_eq!(diff.mean_thousandths(), 0);
        let report = diff
            .check(MAX_MEAN_THOUSANDTHS, MAX_HARD_PPM)
            .expect_err("one pixel over 32 is news, not a rasteriser tie-break");
        assert!(
            report.contains("1 of 2073600 pixels"),
            "the counts are named, because the shares both render as 0.0000 %: {report}"
        );
        // Two of them fail as well — the truncated comparison passed both.
        let two = Diff {
            pixels: 2_073_600,
            differing: 2,
            hard: 2,
            worst: 255,
            total: 1_530,
        };
        assert!(two.check(MAX_MEAN_THOUSANDTHS, MAX_HARD_PPM).is_err());
        // And a clean vista at that resolution still passes.
        let clean = Diff {
            pixels: 2_073_600,
            differing: 0,
            hard: 0,
            worst: 0,
            total: 0,
        };
        clean
            .check(MAX_MEAN_THOUSANDTHS, MAX_HARD_PPM)
            .expect("an identical vista is within every threshold");
    }

    #[test]
    fn the_shipped_thresholds_are_g1s_measured_shape() {
        // skeleton-plan section 7 decision 22 (recommended, not yet logged):
        // mean 0.004 of 255 and zero pixels over 32. A guard so that loosening
        // them is a deliberate edit to this test as well as to the constants.
        assert_eq!(MAX_MEAN_THOUSANDTHS, 4);
        assert_eq!(MAX_HARD_PPM, 0);
        assert_eq!(decimal(MAX_MEAN_THOUSANDTHS, 3), "0.004");
        assert_eq!(decimal(MAX_HARD_PPM, 4), "0.0000");
    }

    #[test]
    fn decimals_render_without_floats() {
        assert_eq!(decimal(6_000, 3), "6.000");
        assert_eq!(decimal(4, 3), "0.004");
        // Parts per million rendered with four places is the percentage.
        assert_eq!(decimal(20_000, 4), "2.0000");
        assert_eq!(decimal(83_333, 4), "8.3333");
        assert_eq!(decimal(200, 4), "0.0200");
    }
}
