// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The one surface format the bridge uploads, and two of item 53's four guards: the
//! **surface-format probe** and the **colour quantisation that must match Godot byte for
//! byte**.
//!
//! # The format
//!
//! Position and colour, nothing else:
//!
//! | attribute | per vertex | why |
//! |---|---|---|
//! | position | 12 bytes, three `f32` | `Vector3`, the vertex buffer |
//! | colour | 4 bytes, RGBA8 | the attribute buffer |
//! | index | 2 bytes, `u16` | G1 measured 6 660 vertices at worst against the 65 536 cap |
//!
//! No normals and no tangents, deliberately. The mesher already folds the per-face shade
//! and the baked light into the vertex colour, and the material is unshaded with
//! albedo-from-vertex-colour, so a normal would carry no information and would change the
//! vertex stride — which is exactly what path B's in-place region write depends on. It is
//! also the format `pharmakos_mesher::gpu_surface_bytes` charges the byte budget in, so
//! the two agree by construction.
//!
//! PLACEHOLDER: the surface format's vertex layout. A heavier layout — a second UV set, a
//! per-vertex material id, a real normal for a shaded material — is what item 54's
//! `B = 512 KiB` margin exists for ("one doubling above the point where B stops binding").
//! Owner, at S6's art pass; changing it means re-probing the strides and re-measuring B.
//!
//! # Why the quantisation is probed rather than assumed
//!
//! Path A hands Godot a `PackedColorArray` of floats and lets Godot convert them to the
//! bytes it stores. Path B's in-place update writes those bytes itself. If the two
//! conversions disagree by one least-significant bit, a chunk's shade depends on whether
//! its last upload happened to take the in-place branch — a rendering difference with no
//! timing signature at all. So the conversion is **measured** against the bytes Godot
//! actually wrote for the first surface, and in-place updates are disabled for the run if
//! nothing reproduces them ([`SurfaceProbe`]).
//!
//! # What the probe found, which is not what the spike's did
//!
//! The direction of the round trip has changed since G1. The spike's mesher produced
//! **float** colours, so the two candidate conversions gave different bytes and the probe
//! had to pick one. This mesher produces RGBA **bytes**, so the bridge sends
//! `f32::from(byte) / 255.0` and asks for the byte back — and measured over all 256
//! values, `truncate` and `round-half-up` are *both* exact round trips for that input.
//! `f32` carries 24 significand bits, `byte / 255.0 * 255.0` lands within half an ulp of
//! the integer, and neither conversion can fall off it.
//!
//! So on today's inputs the probe's **choice does not matter**, and
//! `the_two_candidates_agree_on_every_byte_derived_colour` is the test that says so
//! rather than a comment claiming it. What the probe still earns its place for is the
//! other half of its job, which has not changed at all: it asks the engine what it
//! actually wrote and refuses the in-place path when the answer is not something the
//! region write can reproduce. An engine that packed the colour differently, a
//! `bytes_per_frame` format change at S6's art pass, or a vertex layout that is not
//! 12 bytes would all be caught there — and the day the surface format carries a colour
//! that did not start life as a byte, the choice starts mattering again and this module
//! is already making it by measurement.

use pharmakos_mesher::{MAX_VERTICES, MeshBuffers};

use crate::error::BridgeError;

/// Bytes one vertex occupies in the vertex buffer: a `Vector3` position.
pub const VERTEX_STRIDE: usize = 12;

/// Bytes one vertex occupies in the attribute buffer: RGBA8.
pub const ATTRIBUTE_STRIDE: usize = 4;

/// One chunk surface, as the bridge hands it to the engine.
///
/// A plain copy of what `pharmakos-mesher` produced, narrowed to the format above. The
/// bridge does not reorder, merge, weld or simplify anything: the geometry is the
/// mesher's, and this type is the envelope.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Surface {
    positions: Vec<[f32; 3]>,
    colours: Vec<[u8; 4]>,
    indices: Vec<u16>,
}

impl Surface {
    /// Copies the whole of a meshed chunk into one surface.
    ///
    /// The mesher may split a chunk's quads into several ranges; they share one vertex
    /// and one index array, and Godot takes one surface per chunk, so the ranges are
    /// carried through as they are. Every index is already relative to the array.
    ///
    /// # Errors
    ///
    /// [`BridgeError::SurfaceSize`] when the buffers carry more vertices than a `u16`
    /// index can address. The mesher refuses first
    /// ([`MeshError::TooManyVertices`](pharmakos_mesher::MeshError::TooManyVertices)), so
    /// reaching this is a mesher bug rather than a big chunk; it is checked because the
    /// alternative is a silently wrapped index and a surface that draws nonsense.
    pub fn from_mesh(buffers: &MeshBuffers) -> Result<Self, BridgeError> {
        let vertices = buffers.positions().len();
        if vertices > MAX_VERTICES {
            return Err(BridgeError::SurfaceSize {
                what: "the vertex count",
                got: vertices,
                cap: MAX_VERTICES,
            });
        }
        Ok(Self {
            positions: buffers.positions().to_vec(),
            colours: buffers.colours().to_vec(),
            indices: buffers.indices().to_vec(),
        })
    }

    /// A surface built from its three arrays, for tests and for the headless comparison.
    ///
    /// Both checks are the envelope doing its job. The lengths have to agree because the
    /// two arrays are written as one vertex stream, and every index has to address a
    /// vertex that exists because this type is handed straight to
    /// `mesh_add_surface_from_arrays`: an index past the end is not caught there, it is
    /// read past the end of the engine's own buffer. [`Self::from_mesh`] cannot produce
    /// one — the mesher caps the vertex count first — so only a caller building a surface
    /// by hand can, which is exactly who this constructor is for.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Length`] when the position and colour arrays disagree in length;
    /// [`BridgeError::SurfaceSize`] when an index addresses a vertex the surface does not
    /// have.
    pub fn new(
        positions: Vec<[f32; 3]>,
        colours: Vec<[u8; 4]>,
        indices: Vec<u16>,
    ) -> Result<Self, BridgeError> {
        if positions.len() != colours.len() {
            return Err(BridgeError::Length {
                what: "the colour array",
                expected: positions.len(),
                got: colours.len(),
            });
        }
        if let Some(highest) = indices.iter().copied().max() {
            let addressed = usize::from(highest).saturating_add(1);
            if addressed > positions.len() {
                return Err(BridgeError::SurfaceSize {
                    what: "the highest index",
                    got: addressed,
                    cap: positions.len(),
                });
            }
        }
        Ok(Self {
            positions,
            colours,
            indices,
        })
    }

    /// The vertex positions, in emission order.
    #[must_use]
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// The RGBA8 vertex colours, in emission order.
    #[must_use]
    pub fn colours(&self) -> &[[u8; 4]] {
        &self.colours
    }

    /// The 16-bit indices, in emission order.
    #[must_use]
    pub fn indices(&self) -> &[u16] {
        &self.indices
    }

    /// How many vertices the surface carries.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// True when the chunk meshed to nothing — all air, or fully enclosed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty() || self.indices.is_empty()
    }

    /// The vertex-buffer bytes for an in-place region write: three little-endian `f32`
    /// per vertex, in order.
    pub fn vertex_bytes(&self, out: &mut Vec<u8>) {
        out.clear();
        out.reserve(self.positions.len().saturating_mul(VERTEX_STRIDE));
        for position in &self.positions {
            for axis in position {
                out.extend_from_slice(&axis.to_le_bytes());
            }
        }
    }

    /// The attribute-buffer bytes for an in-place region write, quantised the way the
    /// probe found Godot quantises.
    pub fn attribute_bytes(&self, quantisation: ColourQuantisation, out: &mut Vec<u8>) {
        out.clear();
        out.reserve(self.colours.len().saturating_mul(ATTRIBUTE_STRIDE));
        for colour in &self.colours {
            for channel in colour {
                out.push(quantisation.apply(channel_to_float(*channel)));
            }
        }
    }

    /// The colours as the floats path A hands Godot, in `[r, g, b, a]` order.
    ///
    /// The engine side turns each of these into a `Color`; this function exists so that
    /// the headless comparison converts exactly what the engine call would.
    #[must_use]
    pub fn colour_floats(&self) -> Vec<[f32; 4]> {
        self.colours
            .iter()
            .map(|colour| {
                [
                    channel_to_float(colour[0]),
                    channel_to_float(colour[1]),
                    channel_to_float(colour[2]),
                    channel_to_float(colour[3]),
                ]
            })
            .collect()
    }
}

/// One RGBA8 channel as the float a `Color` carries.
#[must_use]
pub fn channel_to_float(channel: u8) -> f32 {
    f32::from(channel) / 255.0
}

/// How the engine turns a `Color` channel into the byte it stores.
///
/// Two candidates, because those are the two things engines do. Which one this build of
/// Godot does is measured, never assumed — see [`SurfaceProbe`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColourQuantisation {
    /// `u8(clamp(channel * 255, 0, 255))` — a truncating conversion.
    Truncate,
    /// `u8(clamp(channel * 255 + 0.5, 0, 255))` — round half up.
    RoundHalfUp,
}

impl ColourQuantisation {
    /// The two candidates, in the order the probe tries them.
    pub const ALL: [Self; 2] = [Self::Truncate, Self::RoundHalfUp];

    /// The byte this conversion produces for a channel.
    #[must_use]
    pub fn apply(self, channel: f32) -> u8 {
        let scaled = channel.clamp(0.0, 1.0) * 255.0;
        let rounded = match self {
            Self::Truncate => scaled,
            Self::RoundHalfUp => scaled + 0.5,
        };
        // A float-to-integer cast saturates in Rust and the clamp above already bounds
        // the value, so this cannot wrap. `as` is legal here and nowhere outside the
        // wall (AGENTS.md section 4.9); the cast is the conversion being modelled.
        rounded as u8
    }

    /// A short name for a diagnostic line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Truncate => "truncate",
            Self::RoundHalfUp => "round-half-up",
        }
    }
}

/// What the engine said about the first surface it built, and what follows from it.
///
/// Item 53's surface-format guard. Path B may only patch a surface in place when the
/// buffers are laid out the way it is about to write them, and the only authority on that
/// is the engine: the probe reads the strides back out of the surface Godot actually
/// built and refuses the fast path when they are not the expected 12 and 4.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SurfaceProbe {
    /// Bytes per vertex in the vertex buffer, as read back. `None` until probed.
    pub vertex_stride: Option<usize>,
    /// Bytes per vertex in the attribute buffer, as read back. `None` until probed.
    pub attribute_stride: Option<usize>,
    /// The conversion that reproduced the engine's attribute bytes exactly. `None` means
    /// nothing did, and in-place updates stay off for the run.
    pub quantisation: Option<ColourQuantisation>,
}

impl SurfaceProbe {
    /// Reads a probe out of what the engine returned for one built surface.
    ///
    /// `vertex_data` and `attribute_data` are the buffers as the engine holds them, and
    /// `vertex_count` is the engine's own count — not ours, because a mismatch between
    /// the two is precisely the kind of thing the probe exists to notice.
    #[must_use]
    pub fn read_back(
        surface: &Surface,
        vertex_data: &[u8],
        attribute_data: &[u8],
        vertex_count: usize,
    ) -> Self {
        if vertex_count == 0 {
            // Nothing was built, so nothing was learned. A probe that guessed here would
            // be the assumption this type exists to replace.
            return Self::default();
        }
        let count = vertex_count;
        let vertex_stride = vertex_data.len().checked_div(count);
        let attribute_stride = attribute_data.len().checked_div(count);
        let quantisation = if attribute_stride == Some(ATTRIBUTE_STRIDE) {
            ColourQuantisation::ALL
                .into_iter()
                .find(|candidate| reproduces(*candidate, surface, attribute_data, count))
        } else {
            None
        };
        Self {
            vertex_stride,
            attribute_stride,
            quantisation,
        }
    }

    /// The conversion to use for an in-place write, or `None` when the surface must be
    /// rebuilt instead.
    ///
    /// Both halves have to hold: the layout must be the one the region write assumes,
    /// **and** the colour conversion must be known. Either alone would let path B write
    /// bytes the engine did not mean.
    #[must_use]
    pub fn patchable(self) -> Option<ColourQuantisation> {
        if self.vertex_stride == Some(VERTEX_STRIDE)
            && self.attribute_stride == Some(ATTRIBUTE_STRIDE)
        {
            self.quantisation
        } else {
            None
        }
    }

    /// A one-line description for the engine log and for the self-check's report.
    #[must_use]
    pub fn describe(self) -> String {
        let vertex = self
            .vertex_stride
            .map_or_else(|| "?".to_owned(), |stride| stride.to_string());
        let attribute = self
            .attribute_stride
            .map_or_else(|| "?".to_owned(), |stride| stride.to_string());
        let quantisation = self
            .quantisation
            .map_or("none matched", ColourQuantisation::name);
        format!("strides {vertex}/{attribute}, colour {quantisation}")
    }
}

/// Whether `candidate` reproduces the engine's attribute bytes for every probed vertex.
fn reproduces(
    candidate: ColourQuantisation,
    surface: &Surface,
    attribute_data: &[u8],
    count: usize,
) -> bool {
    surface
        .colours()
        .iter()
        .take(count)
        .enumerate()
        .all(|(vertex, colour)| {
            colour.iter().enumerate().all(|(channel, byte)| {
                let offset = vertex
                    .checked_mul(ATTRIBUTE_STRIDE)
                    .and_then(|base| base.checked_add(channel));
                offset
                    .and_then(|offset| attribute_data.get(offset))
                    .copied()
                    .is_some_and(|engine| engine == candidate.apply(channel_to_float(*byte)))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface_of(colours: Vec<[u8; 4]>) -> Surface {
        let positions = vec![[0.0_f32, 0.0, 0.0]; colours.len()];
        // One index per vertex, so the fixture addresses only vertices it has. These
        // tests are about the colour bytes and not about the topology, and a surface
        // whose indices ran past its vertices is refused by the constructor.
        let indices: Vec<u16> = (0..colours.len())
            .map(|index| u16::try_from(index).unwrap_or(0))
            .collect();
        Surface::new(positions, colours, indices).expect("matching lengths")
    }

    /// The measured finding in the module header. Both candidates are exact round trips
    /// for a colour that started life as a byte, so the probe's choice cannot change a
    /// single byte the bridge writes — and this test, not a comment, is what says so.
    ///
    /// If it ever fails, the two conversions have started disagreeing on our own inputs,
    /// the choice has become load-bearing, and the probe is the thing making it.
    #[test]
    fn the_two_candidates_agree_on_every_byte_derived_colour() {
        for byte in 0..=u8::MAX {
            let channel = channel_to_float(byte);
            assert_eq!(
                ColourQuantisation::RoundHalfUp.apply(channel),
                byte,
                "round-half-up lost byte {byte}"
            );
            assert_eq!(
                ColourQuantisation::Truncate.apply(channel),
                byte,
                "truncate lost byte {byte}"
            );
        }
    }

    /// The two candidates are still genuinely different functions, and the probe can
    /// still tell them apart — on a channel that did not come from a byte, which is what
    /// a heavier surface format at S6's art pass would introduce.
    #[test]
    fn the_two_candidates_differ_on_a_colour_that_did_not_start_as_a_byte() {
        let midway = 0.5_f32 / 255.0;
        assert_eq!(ColourQuantisation::Truncate.apply(midway), 0);
        assert_eq!(ColourQuantisation::RoundHalfUp.apply(midway), 1);
    }

    #[test]
    fn the_probe_names_a_conversion_that_reproduces_the_engines_bytes() {
        let surface = surface_of(vec![[10, 20, 30, 255], [1, 2, 3, 4]]);
        let mut engine_bytes = Vec::new();
        surface.attribute_bytes(ColourQuantisation::RoundHalfUp, &mut engine_bytes);
        let vertex_data = vec![0_u8; surface.vertex_count() * VERTEX_STRIDE];

        let probe = SurfaceProbe::read_back(&surface, &vertex_data, &engine_bytes, 2);
        assert_eq!(probe.vertex_stride, Some(VERTEX_STRIDE));
        assert_eq!(probe.attribute_stride, Some(ATTRIBUTE_STRIDE));
        let chosen = probe.patchable().expect("the layout is patchable");
        // Whichever it named, the bytes it will write are the engine's own.
        let mut written = Vec::new();
        surface.attribute_bytes(chosen, &mut written);
        assert_eq!(
            written, engine_bytes,
            "the probe must name a conversion that reproduces what the engine wrote"
        );
    }

    #[test]
    fn a_layout_we_cannot_patch_disables_the_in_place_path() {
        let surface = surface_of(vec![[10, 20, 30, 255]]);
        let mut engine_bytes = Vec::new();
        surface.attribute_bytes(ColourQuantisation::RoundHalfUp, &mut engine_bytes);
        // A heavier vertex layout — a normal packed alongside the position, say.
        let vertex_data = vec![0_u8; VERTEX_STRIDE + 4];

        let probe = SurfaceProbe::read_back(&surface, &vertex_data, &engine_bytes, 1);
        assert_eq!(probe.vertex_stride, Some(VERTEX_STRIDE + 4));
        assert_eq!(
            probe.patchable(),
            None,
            "a surface whose vertex buffer is not the one the region write assumes must be \
             rebuilt, never patched"
        );
    }

    #[test]
    fn an_engine_whose_bytes_match_nothing_disables_the_in_place_path() {
        let surface = surface_of(vec![[10, 20, 30, 255]]);
        // Bytes no candidate produces.
        let engine_bytes = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let vertex_data = vec![0_u8; VERTEX_STRIDE];

        let probe = SurfaceProbe::read_back(&surface, &vertex_data, &engine_bytes, 1);
        assert_eq!(probe.quantisation, None);
        assert_eq!(probe.patchable(), None);
        assert!(
            probe.describe().contains("none matched"),
            "{}",
            probe.describe()
        );
    }

    #[test]
    fn vertex_bytes_are_little_endian_triples() {
        let surface = Surface::new(vec![[1.0_f32, 2.0, 3.0]], vec![[0, 0, 0, 255]], vec![0])
            .expect("matching lengths");
        let mut bytes = Vec::new();
        surface.vertex_bytes(&mut bytes);
        assert_eq!(bytes.len(), VERTEX_STRIDE);
        assert_eq!(bytes.get(0..4), Some(1.0_f32.to_le_bytes().as_slice()));
        assert_eq!(bytes.get(4..8), Some(2.0_f32.to_le_bytes().as_slice()));
        assert_eq!(bytes.get(8..12), Some(3.0_f32.to_le_bytes().as_slice()));
    }

    #[test]
    fn a_colour_array_that_does_not_match_the_positions_is_refused() {
        let error = Surface::new(vec![[0.0_f32; 3]; 2], vec![[0_u8; 4]], Vec::new())
            .expect_err("two positions, one colour");
        assert!(matches!(error, BridgeError::Length { .. }), "{error}");
    }

    /// An index the surface cannot address is refused at construction.
    ///
    /// The envelope's whole job is that what it hands
    /// `mesh_add_surface_from_arrays` addresses itself. An out-of-range index is not
    /// caught there: it is read past the end of the engine's own vertex buffer, and what
    /// comes back is a stretched triangle or a crash, depending on what happened to be
    /// after it.
    #[test]
    fn an_index_that_addresses_a_vertex_the_surface_does_not_have_is_refused() {
        let error = Surface::new(vec![[0.0_f32; 3]], vec![[0_u8; 4]], vec![0, 9_999])
            .expect_err("one vertex, an index addressing ten thousand");
        assert!(
            matches!(
                error,
                BridgeError::SurfaceSize {
                    what: "the highest index",
                    got: 10_000,
                    cap: 1,
                }
            ),
            "{error}"
        );
    }

    /// The boundary itself: the last vertex is addressable, one past it is not.
    #[test]
    fn the_last_vertex_is_addressable_and_the_one_after_it_is_not() {
        let positions = vec![[0.0_f32; 3]; 3];
        let colours = vec![[0_u8; 4]; 3];
        Surface::new(positions.clone(), colours.clone(), vec![0, 1, 2])
            .expect("index 2 is the third of three vertices");
        Surface::new(positions, colours, vec![0, 1, 3]).expect_err("index 3 is one past the end");
    }

    /// An empty index array is legal: a chunk with nothing to draw has no indices, and
    /// refusing it would turn an ordinary empty chunk into an error.
    #[test]
    fn a_surface_with_no_indices_at_all_is_accepted() {
        Surface::new(Vec::new(), Vec::new(), Vec::new()).expect("an empty surface is legal");
    }
}
