// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The feature table: every heat vent and scrap seam on a map, by name
//! (`docs/design/targeting.md`, "Names"; decisions-log item 127 (12)).
//!
//! # What it is
//!
//! One row per vent and per seam the generator laid, with its **generation
//! anchor column** -- the patch or disc centre -- its grade and its
//! **footprint**, the columns the generator stamped it into. A feature is named
//! by its kind and its anchor column with no z, because craters change z:
//! `vent_120_88`, `seam_40_212`. An anchor reveals no count and no order, so a
//! name stays safe under S3's fog, and a guessed name answers `no_target`
//! exactly as a hidden one does.
//!
//! # Why it is not hashed
//!
//! The table is a pure function of `(match seed, rules table, occupied seats)`,
//! exactly as the pristine map is: [`crate::mapgen::generate`] builds it beside
//! the voxels, and a restore regenerates it with the map rather than carrying
//! it ([`crate::snapshot`]). Nothing a tick does changes it, so it adds nothing
//! to the per-tick hash. What does change -- whether a feature is still there
//! -- is **derived** from the voxels, which the hash already covers through
//! the chunk digests, by scanning the footprint when somebody asks
//! ([`FeatureTable::is_live`]): 9 columns for a vent, at most about 40 columns
//! of four voxels for a seam. So liveness needs no hashed column and no hook in
//! `set` or `crater`.
//!
//! # Lost
//!
//! A vent is **lost** when no exposed vent material is left in its footprint:
//! no footprint column's top solid voxel is vent. A seam is **lost** when no
//! ore is left in its footprint at all, at any depth the generator laid it
//! (targeting.md's definitions, irreversible once files use them).
//!
//! # The generator's check
//!
//! Two features never share an anchor column and never share a footprint
//! column: [`FeatureTable::new`] refuses either with
//! [`crate::mapgen::MapError::FeatureOverlap`]. Before S1 a zone's vent and
//! seam were kept apart only by the committed distance bands; the table makes
//! "one feature per column" true by construction, which is also what lets
//! [`crate::power`] tell two Generators on one vent from Generators on two
//! (register S1-32).

use crate::mapgen::MapError;
use crate::pathing::surface::Surface;
use crate::voxels::{Material, Richness, VoxelStore};

/// The most features a map may carry.
///
/// A ceiling rather than a tuning value: the resolver ranks a kind's
/// candidates in a fixed-size array so that a decision allocates nothing
/// (G3' section 9.17), and this is that array's length. The committed table
/// lays twelve at three seats (a vent and a seam per zone, three contested
/// vents and three contested seams); a rules table that asks for more is
/// refused by the generator ([`crate::mapgen::MapError::OutOfRange`]) rather
/// than ranked short.
pub const MAX_FEATURES: usize = 32;

/// The most footprint columns one feature may carry.
///
/// A ceiling rather than a tuning value, for the reason [`MAX_FEATURES`] is
/// one: the Mine program sorts a seam's candidate voxels in a fixed-size array
/// so that a dig search allocates nothing ([`crate::mining`]), and this is
/// that array's length. S1's seam is a disc of radius five, at most 81
/// columns; [`FeatureTable::new`] refuses a feature with more, and one whose
/// box with its ring would not fit the pit-safe flood's grid
/// ([`crate::mining::BOX_CELLS`]), rather than mining it short.
pub const MAX_FOOTPRINT_COLUMNS: usize = 128;

/// How many voxels deep, from the top of the column the generator found, a
/// seam's ore can lie.
///
/// The generator lays a seam from a column's top solid voxel downwards
/// (`crate::mapgen`'s `stamp_seam`), at most this many voxels a column; nothing
/// ever adds ore, so a liveness scan of these voxels finds every ore voxel the
/// seam still has. Defined as the generator's own depth, which is a tuning
/// PLACEHOLDER, so the two cannot part: a deeper seam is scanned deeper.
pub const SEAM_DEPTH_VOXELS: i32 = crate::mapgen::SEAM_VOXELS_PER_COLUMN;

/// What a feature is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum FeatureKind {
    /// A heat vent: a Generator's site.
    Vent,
    /// A scrap seam: ore a Mine mandate digs.
    Seam,
}

impl FeatureKind {
    /// The prefix of the kind's names, without the trailing underscore.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            FeatureKind::Vent => "vent",
            FeatureKind::Seam => "seam",
        }
    }
}

/// One column of a feature's footprint.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FootprintColumn {
    /// Its x, in whole voxels.
    pub x: i32,
    /// Its y, in whole voxels.
    pub y: i32,
    /// The column's top solid voxel when the generator stamped it: the voxel
    /// the feature's material was laid in, and the top of a seam's ore.
    pub top: i32,
}

/// One vent or seam.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Feature {
    /// Vent or seam.
    pub kind: FeatureKind,
    /// The generation anchor column, `[x, y]`: the patch or disc centre.
    pub anchor: [i32; 2],
    /// Its grade.
    pub grade: Richness,
    /// The columns it was stamped into, in `(y, x)` order.
    pub footprint: Vec<FootprintColumn>,
    /// The footprint's bounding box, `[min x, min y, max x, max y]`, so a
    /// "which feature is under this column" question skips every feature
    /// whose box is elsewhere without walking its columns.
    pub bounds: [i32; 4],
}

impl Feature {
    /// A feature from its kind, anchor, grade and stamped columns, the
    /// columns put in `(y, x)` order.
    #[must_use]
    pub fn new(
        kind: FeatureKind,
        anchor: [i32; 2],
        grade: Richness,
        mut footprint: Vec<FootprintColumn>,
    ) -> Feature {
        // item 62: a footprint's columns are distinct, so `(y, x)` is unique.
        footprint.sort_unstable_by_key(|column| (column.y, column.x));
        let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for column in &footprint {
            let [min_x, min_y, max_x, max_y] = &mut bounds;
            *min_x = (*min_x).min(column.x);
            *min_y = (*min_y).min(column.y);
            *max_x = (*max_x).max(column.x);
            *max_y = (*max_y).max(column.y);
        }
        Feature {
            kind,
            anchor,
            grade,
            footprint,
            bounds,
        }
    }

    /// Its name: `vent_<x>_<y>` or `seam_<x>_<y>`, from the anchor column.
    #[must_use]
    pub fn name(&self) -> String {
        let [x, y] = self.anchor;
        feature_name(self.kind, x, y)
    }

    /// Whether the column `(x, y)` is one of its footprint columns.
    #[must_use]
    pub fn covers(&self, x: i32, y: i32) -> bool {
        self.covers_metered(x, y, &())
    }

    /// [`Feature::covers`], reporting to `meter` one unit a footprint column
    /// it compares ([`crate::seams::Meter`]; decisions-log item 175): none
    /// for a column outside the bounding box, which is one comparison of the
    /// feature's own row.
    pub fn covers_metered<M: crate::seams::Meter + ?Sized>(
        &self,
        x: i32,
        y: i32,
        meter: &M,
    ) -> bool {
        let [min_x, min_y, max_x, max_y] = self.bounds;
        if x < min_x || x > max_x || y < min_y || y > max_y {
            return false;
        }
        let mut compared: usize = 0;
        let found = self
            .footprint
            .iter()
            .inspect(|_| compared += 1)
            .any(|column| column.x == x && column.y == y);
        meter.rows(compared);
        found
    }

    /// The footprint column at the anchor, when the generator stamped one.
    #[must_use]
    pub fn anchor_column(&self) -> Option<FootprintColumn> {
        self.anchor_column_metered(&())
    }

    /// [`Feature::anchor_column`], reporting to `meter` one unit a footprint
    /// column it compares, up to the anchor's.
    pub fn anchor_column_metered<M: crate::seams::Meter + ?Sized>(
        &self,
        meter: &M,
    ) -> Option<FootprintColumn> {
        let x = self.anchor.first().copied()?;
        let y = self.anchor.get(1).copied()?;
        let mut compared: usize = 0;
        let found = self
            .footprint
            .iter()
            .copied()
            .inspect(|_| compared += 1)
            .find(|column| column.x == x && column.y == y);
        meter.rows(compared);
        found
    }
}

/// A feature's name: the kind, then the anchor column's x and y.
#[must_use]
pub fn feature_name(kind: FeatureKind, x: i32, y: i32) -> String {
    format!("{}_{x}_{y}", kind.name())
}

/// The kind and anchor a name spells, or `None` when it is not a feature name.
///
/// Decided on **characters**, as a beacon name is
/// ([`crate::tables::parse_beacon_name`]): a kind, an underscore, decimal
/// digits, an underscore, decimal digits, and nothing else -- no sign, no
/// space, no padding a parse would forgive.
#[must_use]
pub fn parse_feature_name(text: &str) -> Option<(FeatureKind, [i32; 2])> {
    let (kind, rest) = if let Some(rest) = text.strip_prefix("vent_") {
        (FeatureKind::Vent, rest)
    } else {
        (FeatureKind::Seam, text.strip_prefix("seam_")?)
    };
    let (x, y) = rest.split_once('_')?;
    let number = |digits: &str| -> Option<i32> {
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        digits.parse::<i32>().ok()
    };
    Some((kind, [number(x)?, number(y)?]))
}

/// Every feature of one map, in `(anchor y, anchor x)` order.
///
/// That order is the resolver's tie-break ("ties go to the lowest anchor y,
/// then x", targeting.md's "Nearest"), and because no two features share an
/// anchor it is a total order (item 62).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FeatureTable {
    features: Vec<Feature>,
}

impl FeatureTable {
    /// The table of `features`, refused when two share an anchor column or a
    /// footprint column, or when there are more than [`MAX_FEATURES`].
    ///
    /// # Errors
    ///
    /// [`MapError::FeatureOverlap`] naming the two features, or
    /// [`MapError::OutOfRange`] when the map carries too many.
    pub fn new(mut features: Vec<Feature>) -> Result<FeatureTable, MapError> {
        if features.len() > MAX_FEATURES {
            return Err(MapError::OutOfRange {
                field: "map.contested_*",
                why: format!(
                    "the map would carry {} vents and seams; the resolver ranks at most {MAX_FEATURES}",
                    features.len()
                ),
            });
        }
        for feature in &features {
            let [min_x, min_y, max_x, max_y] = feature.bounds;
            let side = |low: i32, high: i32| {
                i64::from(high)
                    .saturating_sub(i64::from(low))
                    .saturating_add(3)
            };
            let cells = side(min_x, max_x).saturating_mul(side(min_y, max_y));
            let fits = usize::try_from(cells).is_ok_and(|cells| cells <= crate::mining::BOX_CELLS);
            if feature.footprint.len() > MAX_FOOTPRINT_COLUMNS
                || (!feature.footprint.is_empty() && !fits)
            {
                return Err(MapError::OutOfRange {
                    field: "mapgen seam shape",
                    why: format!(
                        "{} spans {} columns; the Mine program works at most {MAX_FOOTPRINT_COLUMNS} columns in a box of at most {} cells",
                        feature.name(),
                        feature.footprint.len(),
                        crate::mining::BOX_CELLS
                    ),
                });
            }
        }
        // item 62: anchors are unique once the check below passes, and the
        // kind ends the key so even a refused duplicate sorts the same way.
        features.sort_unstable_by_key(|feature| {
            let [x, y] = feature.anchor;
            (y, x, feature.kind)
        });
        for (index, first) in features.iter().enumerate() {
            for second in features.iter().skip(index.saturating_add(1)) {
                let same_anchor = first.anchor == second.anchor;
                let overlapping = first
                    .footprint
                    .iter()
                    .any(|column| second.covers(column.x, column.y));
                if same_anchor || overlapping {
                    return Err(MapError::FeatureOverlap {
                        first: first.name(),
                        second: second.name(),
                    });
                }
            }
        }
        Ok(FeatureTable { features })
    }

    /// How many features the map carries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// Whether the map carries none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Every feature, in `(anchor y, anchor x)` order.
    #[must_use]
    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    /// One feature, by its index in this table.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Feature> {
        self.features.get(index)
    }

    /// The index of the feature `name` names, or `None`.
    #[must_use]
    pub fn index_of_name(&self, name: &str) -> Option<usize> {
        let (kind, anchor) = parse_feature_name(name)?;
        self.index_of(kind, anchor)
    }

    /// The index of the feature of `kind` anchored at `anchor`, or `None`.
    #[must_use]
    pub fn index_of(&self, kind: FeatureKind, anchor: [i32; 2]) -> Option<usize> {
        self.index_of_metered(kind, anchor, &())
    }

    /// [`FeatureTable::index_of`], reporting to `meter` one unit a feature
    /// row it compares, up to the match ([`crate::seams::Meter`];
    /// decisions-log item 175).
    pub fn index_of_metered<M: crate::seams::Meter + ?Sized>(
        &self,
        kind: FeatureKind,
        anchor: [i32; 2],
        meter: &M,
    ) -> Option<usize> {
        let mut compared: usize = 0;
        let found = self
            .features
            .iter()
            .inspect(|_| compared += 1)
            .position(|feature| feature.kind == kind && feature.anchor == anchor);
        meter.rows(compared);
        found
    }

    /// The index of the feature whose footprint holds the column `(x, y)`, or
    /// `None`. At most one does ([`FeatureTable::new`]).
    #[must_use]
    pub fn at_column(&self, x: i32, y: i32) -> Option<usize> {
        self.at_column_metered(x, y, &())
    }

    /// [`FeatureTable::at_column`], reporting to `meter` one unit a feature
    /// row it asks, up to the one that covers the column, and the footprint
    /// columns each asked feature compares ([`Feature::covers_metered`]).
    pub fn at_column_metered<M: crate::seams::Meter + ?Sized>(
        &self,
        x: i32,
        y: i32,
        meter: &M,
    ) -> Option<usize> {
        let mut asked: usize = 0;
        let found = self
            .features
            .iter()
            .inspect(|_| asked += 1)
            .position(|feature| feature.covers_metered(x, y, meter));
        meter.rows(asked);
        found
    }

    /// Whether the feature at `index` is still there: for a vent, some
    /// footprint column's top solid voxel is still vent material; for a seam,
    /// some footprint column still holds ore at a depth the generator laid it.
    ///
    /// Read from the live voxels and the pathing surface's per-column top,
    /// which the voxel phase refreshes in the same phase as the bytes. A
    /// feature that is not in the table is not live.
    #[must_use]
    pub fn is_live(&self, index: usize, surface: &Surface, voxels: &VoxelStore) -> bool {
        self.is_live_metered(index, surface, voxels, &())
    }

    /// [`FeatureTable::is_live`], reporting to `meter` one unit a footprint
    /// column it asks, up to the live one, and for a seam one unit a voxel of
    /// a column's depth it reads ([`crate::seams::Meter`]; decisions-log item
    /// 175).
    pub fn is_live_metered<M: crate::seams::Meter + ?Sized>(
        &self,
        index: usize,
        surface: &Surface,
        voxels: &VoxelStore,
        meter: &M,
    ) -> bool {
        let Some(feature) = self.features.get(index) else {
            return false;
        };
        let mut read: usize = 0;
        let live = match feature.kind {
            FeatureKind::Vent => feature.footprint.iter().any(|column| {
                read += 1;
                surface.node_of(column.x, column.y).is_some_and(|node| {
                    voxels
                        .get([column.x, column.y, surface.top(node)])
                        .and_then(Material::vent_richness)
                        .is_some()
                })
            }),
            FeatureKind::Seam => feature.footprint.iter().any(|column| {
                read += 1;
                let mut depth: i32 = 0;
                while depth < SEAM_DEPTH_VOXELS {
                    read += 1;
                    let z = column.top.saturating_sub(depth);
                    if voxels
                        .get([column.x, column.y, z])
                        .and_then(Material::ore_richness)
                        .is_some()
                    {
                        return true;
                    }
                    depth = depth.saturating_add(1);
                }
                false
            }),
        };
        meter.rows(read);
        live
    }
}

#[cfg(test)]
mod tests {
    //! Dunit part 2: the feature table's scans charge one unit a visit of
    //! each loop, counted here from the table itself.

    use super::{FeatureKind, SEAM_DEPTH_VOXELS};
    use crate::seams::UnitTally;
    use crate::voxels::Material;

    fn world() -> crate::world::World {
        let path = crate::default_rules_path().unwrap_or_else(|| panic!("the rules table"));
        let rules = crate::rules::RulesTable::load(&path)
            .unwrap_or_else(|error| panic!("it loads: {error:?}"));
        crate::world::World::new(&crate::world::WorldConfig {
            match_seed: crate::DETERMINISM_MATCH_SEED,
            seats: 2,
            rules,
            match_settings: crate::runner::MatchSettings::default(),
        })
        .unwrap_or_else(|error| panic!("a world: {error:?}"))
    }

    fn read(tally: &UnitTally) -> u64 {
        tally
            .read()
            .unwrap_or_else(|error| panic!("a measured count: {error}"))
    }

    #[test]
    fn the_scans_charge_what_they_visit() {
        let world = world();
        let table = world.features();
        assert!(!table.is_empty(), "the map lays features");
        for (index, feature) in table.features().iter().enumerate() {
            // A name's lookup: the rows up to its own.
            let tally = UnitTally::new();
            assert_eq!(
                table.index_of_metered(feature.kind, feature.anchor, &tally),
                Some(index)
            );
            assert_eq!(read(&tally), u64::try_from(index + 1).unwrap_or(0));
            // A column's owner, for a column of its footprint and one beside
            // its box: each feature row asked, and the footprint columns of
            // each whose box holds the column, up to the match.
            let [min_x, _, _, _] = feature.bounds;
            for (x, y) in feature
                .footprint
                .iter()
                .map(|column| (column.x, column.y))
                .chain([(min_x - 1, feature.anchor[1])])
            {
                let mut rows: u64 = 0;
                let mut owner = None;
                'scan: for (at, other) in table.features().iter().enumerate() {
                    rows += 1;
                    let [lx, ly, hx, hy] = other.bounds;
                    if x < lx || x > hx || y < ly || y > hy {
                        continue;
                    }
                    for column in &other.footprint {
                        rows += 1;
                        if column.x == x && column.y == y {
                            owner = Some(at);
                            break 'scan;
                        }
                    }
                }
                let tally = UnitTally::new();
                assert_eq!(table.at_column_metered(x, y, &tally), owner);
                assert_eq!(read(&tally), rows, "({x}, {y})");
            }
            // Its liveness: the footprint columns asked, up to a live one,
            // and for a seam the depth voxels each reads.
            let surface = world.surface();
            let voxels = world.voxels();
            let mut rows: u64 = 0;
            let mut live = false;
            'columns: for column in &feature.footprint {
                rows += 1;
                match feature.kind {
                    FeatureKind::Vent => {
                        if surface.node_of(column.x, column.y).is_some_and(|node| {
                            voxels
                                .get([column.x, column.y, surface.top(node)])
                                .and_then(Material::vent_richness)
                                .is_some()
                        }) {
                            live = true;
                            break 'columns;
                        }
                    }
                    FeatureKind::Seam => {
                        for depth in 0..SEAM_DEPTH_VOXELS {
                            rows += 1;
                            if voxels
                                .get([column.x, column.y, column.top - depth])
                                .and_then(Material::ore_richness)
                                .is_some()
                            {
                                live = true;
                                break 'columns;
                            }
                        }
                    }
                }
            }
            let tally = UnitTally::new();
            assert_eq!(table.is_live_metered(index, surface, voxels, &tally), live);
            assert!(live, "a fresh map's features are live");
            assert_eq!(read(&tally), rows, "{}", feature.name());
            // Its anchor column: the footprint columns compared up to it.
            let tally = UnitTally::new();
            let anchor = feature.anchor_column_metered(&tally);
            let compared = feature
                .footprint
                .iter()
                .position(|column| [column.x, column.y] == feature.anchor)
                .map_or(feature.footprint.len(), |at| at + 1);
            assert_eq!(anchor, feature.anchor_column());
            assert_eq!(read(&tally), u64::try_from(compared).unwrap_or(0));
        }
    }
}
