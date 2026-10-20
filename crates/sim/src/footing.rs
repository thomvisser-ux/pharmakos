// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Footings: the columns no edit reaches (the owner's ruling of 2026-10-20,
//! built by S2's `site`).
//!
//! # The rule
//!
//! From the tick a structure's construction begins, or a beacon is placed,
//! every voxel of its footprint columns, from bedrock to the top of the map,
//! **ignores every edit** and counts as anchored ground for the support rule.
//! "Every edit" is every write after generation: a blast (a mortar shell, a
//! demolition charge), a Mine dig, terraform, and any other `set` or `crater`
//! caller. A building therefore falls only by losing its hit points: nothing
//! undermines it and nothing lands on it.
//!
//! Covered: a live structure of a seat, from the tick it is placed at one hit
//! point (a footprint's levelling comes before that, [`crate::site`]), and a
//! live beacon. Not covered: a ruin, whose footing is editable again the tick
//! it turns to one, a dead structure, and a seat's voxel walls, which stay
//! breachable. The core's bore ([`crate::bore`]) is one case of the rule: it
//! is a core's footing, from bedrock to the generated surface, and stays
//! unbreakable whatever stands on it.
//!
//! # Derived, never state
//!
//! The footings are a pure function of the structure and beacon tables, which
//! are hashed, and of the bore, which is derived from the seed. So they add no
//! hashed column: they are read, never written ([`Footings`] borrows the
//! tables), and a restored world reads the same footings as the unbroken run
//! (`crates/sim/tests/site.rs`' `the_footings_are_derived_from_hashed_tables`).
//!
//! # Cost
//!
//! A structure and a beacon each stand on one column (spec section 5; a wider
//! footprint, if a later stage gives one, is additive here). A query walks the
//! structure and beacon tables once, allocation-free; an edit asks once for
//! the box it reaches ([`Footings::touches`]) and per voxel only when that box
//! holds a footing. The worst case, for G3', is a blast whose ball meets a
//! footing: `(2r + 1)^3` voxels times the live structures and beacons, about
//! 33 voxels by 400 rows at a mortar's radius 2.

use core::cell::Cell;

use crate::bore::Bore;
use crate::math::fixed::Fx;
use crate::tables::{BeaconTable, SeatId, StructureTable};

/// Why an edit wrote nothing to a voxel: it fell on a footing. Diagnostic,
/// never hashed, each counted rather than dropped silently.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FootingRefusals {
    /// Voxels of a blast's ball that lay in a footing column.
    pub blast_voxels: u64,
    /// `set` edits (a dig, a fill, a host's write) that named a footing voxel.
    pub sets: u64,
}

/// The footings as one tick reads them: the bore and every live structure of
/// a seat and live beacon, borrowed from the world's tables.
#[derive(Debug)]
pub struct Footings<'a> {
    bore: &'a Bore,
    structures: &'a StructureTable,
    beacons: &'a BeaconTable,
    /// How many voxels [`Footings::contains`] has answered `true` for: what a
    /// blast skipped, which the world adds to [`FootingRefusals`].
    hits: Cell<u64>,
}

impl<'a> Footings<'a> {
    /// The footings of these tables.
    #[must_use]
    pub const fn of(
        bore: &'a Bore,
        structures: &'a StructureTable,
        beacons: &'a BeaconTable,
    ) -> Footings<'a> {
        Footings {
            bore,
            structures,
            beacons,
            hits: Cell::new(0),
        }
    }

    /// Whether the column `(x, y)` is a footing: the bore's, a live
    /// structure's of a seat, or a live beacon's.
    #[must_use]
    pub fn column(&self, x: i32, y: i32) -> bool {
        self.building_column(x, y) || self.bore.touches([x, y], [x, y])
    }

    /// Whether the voxel `at` lies in a footing column (the bore's only up
    /// to the generated surface, as the bore is). Counted when it does.
    #[must_use]
    pub fn contains(&self, at: [i32; 3]) -> bool {
        let [x, y, _] = at;
        let held = self.bore.contains(at) || self.building_column(x, y);
        if held {
            self.hits.set(self.hits.get().saturating_add(1));
        }
        held
    }

    /// Whether any footing column lies inside the box of columns `min` to
    /// `max`, inclusive: an edit whose box meets none skips the per-voxel
    /// test.
    #[must_use]
    pub fn touches(&self, min: [i32; 2], max: [i32; 2]) -> bool {
        let ([lx, ly], [hx, hy]) = (min, max);
        let inside = |at: &[Fx; 3]| {
            let [x, y] = crate::targeting::column_of(*at);
            x >= lx && x <= hx && y >= ly && y <= hy
        };
        self.bore.touches(min, max)
            || self
                .structures
                .hit_points()
                .iter()
                .zip(self.structures.seats())
                .zip(self.structures.positions())
                .any(|((hp, seat), at)| hp.is_alive() && SeatId::new(*seat).is_some() && inside(at))
            || self
                .beacons
                .hit_points()
                .iter()
                .zip(self.beacons.positions())
                .any(|(hp, at)| hp.is_alive() && inside(at))
    }

    /// How many voxels [`Footings::contains`] answered `true` for.
    #[must_use]
    pub fn hits(&self) -> u64 {
        self.hits.get()
    }

    /// Whether a live structure of a seat or a live beacon stands on `(x, y)`.
    fn building_column(&self, x: i32, y: i32) -> bool {
        let on = |at: &[Fx; 3]| crate::targeting::column_of(*at) == [x, y];
        self.structures
            .hit_points()
            .iter()
            .zip(self.structures.seats())
            .zip(self.structures.positions())
            .any(|((hp, seat), at)| hp.is_alive() && SeatId::new(*seat).is_some() && on(at))
            || self
                .beacons
                .hit_points()
                .iter()
                .zip(self.beacons.positions())
                .any(|(hp, at)| hp.is_alive() && on(at))
    }
}
