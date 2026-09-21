// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sim chunk order in, mesher chunk order out — decisions log section 2.7 item 92.
//!
//! # The two orders, and why there are two
//!
//! Neither crate knows the other's axes, and that is the decision rather than an
//! accident: the sim's layout is the order its per-chunk digests are taken over, so a
//! presentation concern must not choose it, and the mesher's layout is Godot's, so the
//! sim must not choose that either. The conversion is this file, and item 92 costs it at
//! "a 32 KiB copy per remeshed chunk, trivial at K = 4 per frame".
//!
//! | | fastest | middle | slowest | linear index |
//! |---|---|---|---|---|
//! | `pharmakos-sim` | x, east | y, **north** | z, **up** | `x + 32*y + 1024*z` |
//! | `pharmakos-mesher` | x, east | z, **north** | y, **up** | `x + 32*z + 1024*y` |
//!
//! So the conversion is an **axis relabel**, not a reshuffle of unrelated numbers: the
//! sim's `y` is the mesher's `z` and the sim's `z` is the mesher's `y`, because the two
//! disagree about which letter means "up" and agree about everything else.
//!
//! # The copy is a straight one, and the test says why
//!
//! Work the relabel through and the two index formulas cancel. A voxel at east `e`,
//! north `n`, up `u` sits at
//!
//! ```text
//!   sim:     x=e, y=n, z=u   ->  e + 32*n + 1024*u
//!   mesher:  x=e, y=u, z=n   ->  e + 32*n + 1024*u
//! ```
//!
//! — the same offset. The permutation this module applies is therefore the identity for
//! *this* pair of conventions, and [`sim_to_mesher_chunk`] compiles to a memcpy.
//!
//! That is a finding, not a licence to write `copy_from_slice` and move on. The code
//! below is written as the relabel it is, from [`mesher_coords_of_sim`], so that it stays
//! correct if either side ever renames an axis or reorders an index; and
//! `the_axis_relabel_and_the_index_orders_cancel` asserts the identity explicitly, so
//! that the day one of them moves, a test fails with this paragraph attached rather than
//! a chunk rendering inside out.

use pharmakos_mesher::{CHUNK_EDGE, CHUNK_VOLUME, FACE_AREA, Face, voxel_index};

use crate::error::BridgeError;

/// The linear index of a chunk-local voxel in **sim** order: x east fastest, then y
/// north, then z up — `index = x + 32*y + 1024*z`.
///
/// The mesher's own order is [`pharmakos_mesher::voxel_index`]; the two are related by
/// [`mesher_coords_of_sim`].
///
/// Meaningful only for coordinates below [`pharmakos_mesher::CHUNK_EDGE`]; like the
/// mesher's, it does not range-check, because it sits in a 32 768-iteration loop.
#[must_use]
pub const fn sim_voxel_index(x: usize, y: usize, z: usize) -> usize {
    (z * CHUNK_EDGE + y) * CHUNK_EDGE + x
}

/// The mesher's coordinates for a voxel the sim calls `(x, y, z)`.
///
/// The whole of the conversion, in one line: east stays east, and the two crates swap
/// their names for north and up.
#[must_use]
pub const fn mesher_coords_of_sim(x: usize, y: usize, z: usize) -> (usize, usize, usize) {
    (x, z, y)
}

/// The mesher's face for the direction the sim calls this one.
///
/// The sim's `+z` is up, which is the mesher's `+y`; the sim's `+y` is north, which is
/// the mesher's `+z`. `+x` is east on both sides and does not move.
#[must_use]
pub const fn mesher_face_of_sim(face: Face) -> Face {
    match face {
        Face::NegX => Face::NegX,
        Face::PosX => Face::PosX,
        Face::NegY => Face::NegZ,
        Face::PosY => Face::PosZ,
        Face::NegZ => Face::NegY,
        Face::PosZ => Face::PosY,
    }
}

/// Rewrites one chunk's material or light bytes from sim order into mesher order.
///
/// `out` is cleared and refilled, so a caller that keeps one buffer per frame allocates
/// once (item 92's "32 KiB copy", reused).
///
/// # Errors
///
/// [`BridgeError::Length`] when `sim` is not [`pharmakos_mesher::CHUNK_VOLUME`] bytes.
pub fn sim_to_mesher_chunk(
    what: &'static str,
    sim: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), BridgeError> {
    if sim.len() != CHUNK_VOLUME {
        return Err(BridgeError::Length {
            what,
            expected: CHUNK_VOLUME,
            got: sim.len(),
        });
    }
    out.clear();
    out.resize(CHUNK_VOLUME, 0);
    for z in 0..CHUNK_EDGE {
        for y in 0..CHUNK_EDGE {
            for x in 0..CHUNK_EDGE {
                let (mx, my, mz) = mesher_coords_of_sim(x, y, z);
                let Some(source) = sim.get(sim_voxel_index(x, y, z)) else {
                    continue;
                };
                if let Some(slot) = out.get_mut(voxel_index(mx, my, mz)) {
                    *slot = *source;
                }
            }
        }
    }
    Ok(())
}

/// Rewrites one neighbour boundary slice from sim order into mesher order.
///
/// A border is the 32-by-32 layer just outside one chunk face. Each side indexes it by
/// the two axes its face direction leaves free, in its own volume order's precedence, so
/// the relabel has to be applied to the pair as well as to the face name — which is what
/// [`mesher_face_of_sim`] is for. The `sim_face` argument is the face **as the sim names
/// it**; the returned bytes belong in the slot of `mesher_face_of_sim(sim_face)`.
///
/// # Errors
///
/// [`BridgeError::Length`] when `sim` is not [`pharmakos_mesher::FACE_AREA`] bytes.
pub fn sim_to_mesher_border(
    what: &'static str,
    sim_face: Face,
    sim: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), BridgeError> {
    if sim.len() != FACE_AREA {
        return Err(BridgeError::Length {
            what,
            expected: FACE_AREA,
            got: sim.len(),
        });
    }
    out.clear();
    out.resize(FACE_AREA, 0);
    let mesher_face = mesher_face_of_sim(sim_face);
    for second in 0..CHUNK_EDGE {
        for first in 0..CHUNK_EDGE {
            // The pair of free coordinates, read back into a full (x, y, z) triple in the
            // sim's own axes, then relabelled.
            let (x, y, z) = sim_border_coords(sim_face, first, second);
            let (mx, my, mz) = mesher_coords_of_sim(x, y, z);
            let Some(source) = sim.get(sim_border_offset(first, second)) else {
                continue;
            };
            if let Some(slot) = out.get_mut(mesher_border_offset(mesher_face, mx, my, mz)) {
                *slot = *source;
            }
        }
    }
    Ok(())
}

/// The sim's offset into a border slice for the two free coordinates of a face.
///
/// It takes no face, and that is the point: the sim's volume order is x, then y, then z,
/// so a border keeps that precedence among whichever two axes survive — a
/// plus-or-minus-x border is `y + 32*z`, a plus-or-minus-y border `x + 32*z`, and a
/// plus-or-minus-z border `x + 32*y`, which is `first + 32*second` in all three cases.
/// The mesher's [`mesher_border_offset`] does depend on the face, because its volume
/// order puts z before y and so reverses the pair for one of the three.
const fn sim_border_offset(first: usize, second: usize) -> usize {
    second * CHUNK_EDGE + first
}

/// The `(x, y, z)` in the sim's axes that a border offset stands for.
const fn sim_border_coords(face: Face, first: usize, second: usize) -> (usize, usize, usize) {
    // The coordinate on the face's own axis is the layer just outside the chunk; it is
    // not part of the offset and is only needed so the relabel has a full triple to work
    // on. Which end it is does not matter here, because the relabel never mixes the free
    // pair with the fixed axis — a face perpendicular to one axis stays perpendicular to
    // that axis's new name.
    match face {
        Face::NegX | Face::PosX => (0, first, second),
        Face::NegY | Face::PosY => (first, 0, second),
        Face::NegZ | Face::PosZ => (first, second, 0),
    }
}

/// The mesher's offset into a border slice for a voxel at the mesher's `(x, y, z)`.
///
/// The mesher's volume order is x, then z, then y, so its borders read: a
/// plus-or-minus-x border is `z + 32*y`, a plus-or-minus-y border `x + 32*z`, and a
/// plus-or-minus-z border `x + 32*y` (`pharmakos_mesher::NeighbourBorder`).
const fn mesher_border_offset(face: Face, x: usize, y: usize, z: usize) -> usize {
    match face {
        Face::NegX | Face::PosX => y * CHUNK_EDGE + z,
        Face::NegY | Face::PosY => z * CHUNK_EDGE + x,
        Face::NegZ | Face::PosZ => y * CHUNK_EDGE + x,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The finding the module header explains: for today's two conventions the relabel
    /// and the two index orders cancel, so the marshalling copy is a straight one.
    ///
    /// If this test ever fails, do **not** delete it. It means one of the two crates
    /// changed its axis names or its index order, the copy is now a real permutation,
    /// and the header's table and this assertion are what tell the next reader that the
    /// change was deliberate.
    #[test]
    fn the_axis_relabel_and_the_index_orders_cancel() {
        for z in 0..CHUNK_EDGE {
            for y in 0..CHUNK_EDGE {
                for x in 0..CHUNK_EDGE {
                    let (mx, my, mz) = mesher_coords_of_sim(x, y, z);
                    assert_eq!(
                        sim_voxel_index(x, y, z),
                        voxel_index(mx, my, mz),
                        "sim ({x},{y},{z}) and mesher ({mx},{my},{mz}) must be the same offset"
                    );
                }
            }
        }
    }

    #[test]
    fn a_transposed_chunk_holds_every_byte_at_its_relabelled_place() {
        let sim: Vec<u8> = (0..CHUNK_VOLUME)
            .map(|index| u8::try_from(index % 251).unwrap_or(0))
            .collect();
        let mut out = Vec::new();
        sim_to_mesher_chunk("chunk materials", &sim, &mut out).expect("a full chunk");

        assert_eq!(out.len(), CHUNK_VOLUME);
        for z in 0..CHUNK_EDGE {
            for y in 0..CHUNK_EDGE {
                for x in 0..CHUNK_EDGE {
                    let (mx, my, mz) = mesher_coords_of_sim(x, y, z);
                    assert_eq!(
                        out.get(voxel_index(mx, my, mz)),
                        sim.get(sim_voxel_index(x, y, z))
                    );
                }
            }
        }
    }

    #[test]
    fn a_chunk_of_the_wrong_length_is_refused_rather_than_padded() {
        let mut out = Vec::new();
        let error = sim_to_mesher_chunk("chunk materials", &[0_u8; 16], &mut out)
            .expect_err("a short chunk is not a chunk");
        assert_eq!(
            error,
            BridgeError::Length {
                what: "chunk materials",
                expected: CHUNK_VOLUME,
                got: 16,
            }
        );
    }

    #[test]
    fn the_six_sim_faces_relabel_onto_six_distinct_mesher_faces() {
        let mut seen: Vec<usize> = Face::ALL
            .iter()
            .map(|face| mesher_face_of_sim(*face).index())
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2, 3, 4, 5]);
        // Up is the pair that moves: the sim's +z (up) is the mesher's +y (up).
        assert_eq!(mesher_face_of_sim(Face::PosZ), Face::PosY);
        assert_eq!(mesher_face_of_sim(Face::PosY), Face::PosZ);
        assert_eq!(mesher_face_of_sim(Face::PosX), Face::PosX);
    }

    #[test]
    fn a_transposed_border_holds_every_byte_at_its_relabelled_place() {
        let sim: Vec<u8> = (0..FACE_AREA)
            .map(|index| u8::try_from(index % 251).unwrap_or(0))
            .collect();
        for face in Face::ALL {
            let mut out = Vec::new();
            sim_to_mesher_border("a border", face, &sim, &mut out).expect("a full border");
            assert_eq!(out.len(), FACE_AREA);

            let mesher_face = mesher_face_of_sim(face);
            for second in 0..CHUNK_EDGE {
                for first in 0..CHUNK_EDGE {
                    let (x, y, z) = sim_border_coords(face, first, second);
                    let (mx, my, mz) = mesher_coords_of_sim(x, y, z);
                    assert_eq!(
                        out.get(mesher_border_offset(mesher_face, mx, my, mz)),
                        sim.get(sim_border_offset(first, second)),
                        "{face} -> {mesher_face} at ({first},{second})"
                    );
                }
            }
            // Every byte landed somewhere: the relabel is a bijection, so the multiset of
            // bytes is preserved. A permutation that dropped a cell would leave a zero.
            let mut before = sim.clone();
            let mut after = out.clone();
            before.sort_unstable();
            after.sort_unstable();
            assert_eq!(before, after, "{face} lost or duplicated a border cell");
        }
    }

    #[test]
    fn a_border_of_the_wrong_length_is_refused() {
        let mut out = Vec::new();
        let error = sim_to_mesher_border("a border", Face::PosZ, &[1_u8; 3], &mut out)
            .expect_err("a short border is not a border");
        assert_eq!(
            error,
            BridgeError::Length {
                what: "a border",
                expected: FACE_AREA,
                got: 3,
            }
        );
    }
}
