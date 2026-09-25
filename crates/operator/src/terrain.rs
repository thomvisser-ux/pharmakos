// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The ground and its vents and seams, read from the view feed.
//!
//! `get_view` serves every seat the whole generated map (decisions-log item
//! 108 (1)): chunks of materials, run-length encoded with
//! [`pharmakos_proto::chunk_rle`]. The operator decodes them once per round
//! into one column per `(x, y)` -- the top solid voxel and its material --
//! which is everything it needs: where a commander can stand, and where the
//! heat vents and ore seams are.
//!
//! The material byte values and the voxel order are the wire's
//! (`gp.api.v1.ViewChunk`'s comment, pinned to the sim's by a gateway test);
//! they are written out below rather than guessed.

use std::collections::BTreeSet;

use crate::easy::MAP_COLUMNS_MAX;
use crate::tuning::Richness;

/// A chunk's edge, in voxels (`gp.api.v1.ViewChunk`: 32 x 32 x 32).
const EDGE: usize = 32;

/// `gp.api.v1.ViewChunk`'s material table: air.
const AIR: u8 = 0;
/// Scrap seam, lean.
const SEAM_LEAN: u8 = 3;
/// Scrap seam, standard.
const SEAM_STANDARD: u8 = 4;
/// Scrap seam, rich.
const SEAM_RICH: u8 = 5;
/// Heat vent, lean.
const VENT_LEAN: u8 = 6;
/// Heat vent, standard.
const VENT_STANDARD: u8 = 7;
/// Heat vent, rich.
const VENT_RICH: u8 = 8;

/// What a patch of the ground is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Feature {
    /// A heat vent: a Generator stands on it.
    Vent,
    /// An ore seam: a Mine beacon's drones dig it.
    Seam,
}

impl Feature {
    /// The lower-case name, for the "why" note.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Feature::Vent => "heat vent",
            Feature::Seam => "ore seam",
        }
    }
}

/// The feature and grade a material is, if it is one.
const fn resource(material: u8) -> Option<(Feature, Richness)> {
    match material {
        SEAM_LEAN => Some((Feature::Seam, Richness::Lean)),
        SEAM_STANDARD => Some((Feature::Seam, Richness::Standard)),
        SEAM_RICH => Some((Feature::Seam, Richness::Rich)),
        VENT_LEAN => Some((Feature::Vent, Richness::Lean)),
        VENT_STANDARD => Some((Feature::Vent, Richness::Standard)),
        VENT_RICH => Some((Feature::Vent, Richness::Rich)),
        _ => None,
    }
}

/// One connected patch of vent or seam at the surface.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Patch {
    /// Vent or seam.
    pub(crate) feature: Feature,
    /// Its grade.
    pub(crate) richness: Richness,
    /// Its columns, `(x, y, top z)`, in ascending `(x, y)`.
    pub(crate) columns: Vec<[i32; 3]>,
    /// The column nearest its centre, as the voxel a thing stands on there:
    /// one above the top solid voxel.
    pub(crate) centre: [i32; 3],
}

/// The ground: one column per `(x, y)`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Terrain {
    size_x: usize,
    size_y: usize,
    /// The top solid voxel's height and material, per column, `x` fastest.
    tops: Vec<Option<(i32, u8)>>,
}

impl Terrain {
    /// An empty map of this size: no column has ground yet. `None` when it
    /// has more than [`MAP_COLUMNS_MAX`] columns, checked before anything is
    /// allocated (a negative side reads as zero).
    pub(crate) fn new(size_x: i32, size_y: i32) -> Option<Terrain> {
        let columns = i64::from(size_x.max(0)).checked_mul(i64::from(size_y.max(0)))?;
        if columns > MAP_COLUMNS_MAX {
            return None;
        }
        let size_x = usize::try_from(size_x.max(0)).ok()?;
        let size_y = usize::try_from(size_y.max(0)).ok()?;
        Some(Terrain {
            size_x,
            size_y,
            tops: vec![None; usize::try_from(columns).ok()?],
        })
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let x = usize::try_from(x).ok()?;
        let y = usize::try_from(y).ok()?;
        (x < self.size_x && y < self.size_y)
            .then(|| x.saturating_add(y.saturating_mul(self.size_x)))
    }

    /// Fold one decoded chunk in: `voxels` is the chunk's 32 768 materials in
    /// the wire's order, `x + 32 y + 1024 z`.
    pub(crate) fn add_chunk(&mut self, origin: [i32; 3], voxels: &[u8]) {
        let [ox, oy, oz] = origin;
        for ly in 0..EDGE {
            for lx in 0..EDGE {
                let mut found: Option<(usize, u8)> = None;
                for lz in (0..EDGE).rev() {
                    let at = lx
                        .saturating_add(ly.saturating_mul(EDGE))
                        .saturating_add(lz.saturating_mul(EDGE * EDGE));
                    match voxels.get(at).copied() {
                        Some(AIR) | None => {}
                        Some(material) => {
                            found = Some((lz, material));
                            break;
                        }
                    }
                }
                let Some((lz, material)) = found else {
                    continue;
                };
                let (Ok(dx), Ok(dy), Ok(dz)) =
                    (i32::try_from(lx), i32::try_from(ly), i32::try_from(lz))
                else {
                    continue;
                };
                let (x, y, z) = (
                    ox.saturating_add(dx),
                    oy.saturating_add(dy),
                    oz.saturating_add(dz),
                );
                let Some(index) = self.index(x, y) else {
                    continue;
                };
                if let Some(slot) = self.tops.get_mut(index) {
                    if slot.is_none_or(|(held, _)| z > held) {
                        *slot = Some((z, material));
                    }
                }
            }
        }
    }

    /// The top solid voxel of a column, and its material.
    pub(crate) fn top(&self, x: i32, y: i32) -> Option<(i32, u8)> {
        self.index(x, y)
            .and_then(|index| self.tops.get(index).copied().flatten())
    }

    /// Where a thing stands on a column: one voxel above its top solid voxel.
    pub(crate) fn stand(&self, x: i32, y: i32) -> Option<[i32; 3]> {
        self.top(x, y).map(|(z, _)| [x, y, z.saturating_add(1)])
    }

    /// Every vent and seam patch at the surface, in ascending order of each
    /// patch's first column `(y, x)`: a total order that depends on the map
    /// alone.
    pub(crate) fn patches(&self) -> Vec<Patch> {
        let mut seen: BTreeSet<(i32, i32)> = BTreeSet::new();
        let mut out: Vec<Patch> = Vec::new();
        let (Ok(size_x), Ok(size_y)) = (i32::try_from(self.size_x), i32::try_from(self.size_y))
        else {
            return out;
        };
        for y in 0..size_y {
            for x in 0..size_x {
                let Some((_, material)) = self.top(x, y) else {
                    continue;
                };
                let Some((feature, richness)) = resource(material) else {
                    continue;
                };
                if seen.contains(&(x, y)) {
                    continue;
                }
                let columns = self.flood(x, y, material, &mut seen);
                if let Some(centre) = self.centre_of(&columns) {
                    out.push(Patch {
                        feature,
                        richness,
                        columns,
                        centre,
                    });
                }
            }
        }
        out
    }

    /// The 8-connected columns whose top is `material`, from one.
    fn flood(
        &self,
        x: i32,
        y: i32,
        material: u8,
        seen: &mut BTreeSet<(i32, i32)>,
    ) -> Vec<[i32; 3]> {
        let mut columns: Vec<[i32; 3]> = Vec::new();
        let mut stack: Vec<(i32, i32)> = vec![(x, y)];
        let _ = seen.insert((x, y));
        while let Some((cx, cy)) = stack.pop() {
            if let Some((z, _)) = self.top(cx, cy) {
                columns.push([cx, cy, z]);
            }
            for dy in -1..=1_i32 {
                for dx in -1..=1_i32 {
                    let (nx, ny) = (cx.saturating_add(dx), cy.saturating_add(dy));
                    if seen.contains(&(nx, ny)) {
                        continue;
                    }
                    if self.top(nx, ny).map(|(_, found)| found) == Some(material) {
                        let _ = seen.insert((nx, ny));
                        stack.push((nx, ny));
                    }
                }
            }
        }
        columns.sort_unstable();
        columns
    }

    /// The column nearest a patch's integer centroid, ties to the lowest
    /// `(x, y)`, as the voxel a thing stands on there.
    fn centre_of(&self, columns: &[[i32; 3]]) -> Option<[i32; 3]> {
        let count = i64::try_from(columns.len()).ok().filter(|n| *n > 0)?;
        let sum_x: i64 = columns.iter().map(|c| i64::from(c[0])).sum();
        let sum_y: i64 = columns.iter().map(|c| i64::from(c[1])).sum();
        let mid_x = sum_x.checked_div(count)?;
        let mid_y = sum_y.checked_div(count)?;
        let best = columns.iter().min_by_key(|c| {
            let dx = i64::from(c[0]).saturating_sub(mid_x);
            let dy = i64::from(c[1]).saturating_sub(mid_y);
            (
                dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)),
                c[0],
                c[1],
            )
        })?;
        self.stand(best[0], best[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 32 x 32 chunk of flat dirt at z = 4 with a 3 x 3 standard vent at
    /// (10..=12, 20..=22) and one lean seam column at (2, 2).
    fn chunk() -> Vec<u8> {
        let mut voxels = vec![AIR; EDGE * EDGE * EDGE];
        for y in 0..EDGE {
            for x in 0..EDGE {
                for z in 0..=4 {
                    let at = x + EDGE * y + EDGE * EDGE * z;
                    if let Some(slot) = voxels.get_mut(at) {
                        *slot = 1;
                    }
                }
                let top = x + EDGE * y + EDGE * EDGE * 4;
                let material = if (10..=12).contains(&x) && (20..=22).contains(&y) {
                    VENT_STANDARD
                } else if x == 2 && y == 2 {
                    SEAM_LEAN
                } else {
                    1
                };
                if let Some(slot) = voxels.get_mut(top) {
                    *slot = material;
                }
            }
        }
        voxels
    }

    #[test]
    fn a_patch_is_found_with_its_centre_column() {
        let mut terrain = Terrain::new(32, 32).unwrap();
        terrain.add_chunk([0, 0, 0], &chunk());
        let patches = terrain.patches();
        assert_eq!(patches.len(), 2);
        let seam = patches.first().unwrap();
        assert_eq!(
            (seam.feature, seam.richness),
            (Feature::Seam, Richness::Lean)
        );
        assert_eq!(seam.centre, [2, 2, 5]);
        let vent = patches.get(1).unwrap();
        assert_eq!(vent.feature, Feature::Vent);
        assert_eq!(vent.columns.len(), 9);
        assert_eq!(vent.centre, [11, 21, 5]);
        assert_eq!(terrain.stand(0, 0), Some([0, 0, 5]));
        assert_eq!(terrain.stand(40, 0), None);
    }

    #[test]
    fn a_map_too_large_to_hold_is_refused_before_it_is_allocated() {
        assert!(Terrain::new(i32::MAX, i32::MAX).is_none());
        assert!(Terrain::new(100_000, 100_000).is_none());
        assert!(Terrain::new(1024, 1024).is_some());
        assert!(Terrain::new(1025, 1024).is_none());
        assert!(Terrain::new(-5, 64).is_some_and(|empty| empty.tops.is_empty()));
    }
}
