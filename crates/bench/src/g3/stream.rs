// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The crater stream at the destruction rate the budget is set with (S2's
//! plan, section 5; item 65's 20 craters a second).
//!
//! Real play's craters are its shells', and on this `main` no mortar stands in
//! the gate's layout (a mortar is a structure, and the layout places beacons
//! and units only), so the rate the budget is set with is filed from outside
//! through the host's filing seam (`Host::file_voxel_edit`, S2's plan, E5),
//! between ticks, as a shell's landing files one: a mortar's ball
//! (`structures.mortar.weapon.projectile.crater_radius_voxels`), seat-less.
//! The voxel phase, the support rule, the graph repair and every repath it
//! causes are then the sim's own (no charged substitute, item 64).
//!
//! **Where.** Inside the layout's beacon spheres, where the units stand and
//! walk, so a crater lands in the graph the routes run over: a beacon picked
//! by a counter-based generator over `(seed, crater index)`, then an offset
//! inside its sphere's square, on the column's top solid voxel. Deterministic
//! and integer, so the stream is an input of the run like the layout: the
//! report's seed reproduces it.
//!
//! **Nothing is dropped silently.** A crater the host's share of the queue
//! cannot take is counted under its reason ([`StreamCounts`]) and the report
//! prints the count; a column with no ground is redrawn, and the redraws are
//! counted too. An edit off the map is the harness's bug, and an error.

use pharmakos_gateway::host::Host;
use pharmakos_sim::runner::Layout;
use pharmakos_sim::voxels::VoxelEdit;
use pharmakos_sim::world::{FileRefused, World};

use crate::error::Error;

/// Craters a second at which item 65 set the provisional budget.
pub const CRATERS_PER_SECOND: u32 = 20;

/// How many times a crater is redrawn before the stream says the map has no
/// ground where the layout stands.
const REDRAWS: u32 = 64;

/// What the stream filed, and what it could not.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StreamCounts {
    /// Craters filed.
    pub filed: u64,
    /// Craters the host's share of the edit queue refused as full.
    pub queue_full: u64,
    /// Draws that landed on a column with no ground, and were drawn again.
    pub redrawn: u64,
}

/// The crater stream for one match.
#[derive(Clone, Debug)]
pub struct CraterStream {
    seed: u64,
    rate: u32,
    radius: i32,
    sites: Vec<[i32; 2]>,
    reach: i32,
    next: u64,
    counts: StreamCounts,
}

/// `SplitMix64`: one step of a counter-based generator, so draw `n` depends on
/// `(seed, n)` alone.
const fn mix(seed: u64, n: u64) -> u64 {
    let mut z = seed.wrapping_add(n.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl CraterStream {
    /// A stream of `rate` craters a second over `layout`'s beacons, on `world`'s
    /// rules.
    ///
    /// # Errors
    ///
    /// [`Error::Rules`] when the rules carry no mortar crater or beacon
    /// sphere, and [`Error::Layout`] for a layout with no beacon.
    pub fn new(
        seed: u64,
        rate: u32,
        layout: &Layout,
        world: &World,
    ) -> Result<CraterStream, Error> {
        let message = world.rules().message();
        let radius = message
            .structures
            .as_ref()
            .and_then(|block| block.mortar.as_ref())
            .and_then(|mortar| mortar.weapon.as_ref())
            .and_then(|weapon| weapon.projectile.as_ref())
            .map(|projectile| projectile.crater_radius_voxels)
            .ok_or_else(|| Error::Rules("no `structures.mortar.weapon.projectile`".to_owned()))?;
        let reach = message
            .beacon
            .as_ref()
            .map(|beacon| beacon.sphere_radius_voxels)
            .ok_or_else(|| Error::Rules("no `beacon` block".to_owned()))?;
        let sites: Vec<[i32; 2]> = layout.beacons.iter().map(|beacon| beacon.column).collect();
        if sites.is_empty() {
            return Err(Error::Layout("a crater stream over no beacon".to_owned()));
        }
        Ok(CraterStream {
            seed,
            rate,
            radius: i32::try_from(radius).map_err(|_| Error::Overflow("the crater radius"))?,
            sites,
            reach: i32::try_from(reach).map_err(|_| Error::Overflow("the sphere radius"))?,
            next: 0,
            counts: StreamCounts::default(),
        })
    }

    /// What the stream has filed so far.
    #[must_use]
    pub const fn counts(&self) -> StreamCounts {
        self.counts
    }

    /// How many craters land on the tick after `played` ticks: the rate spread
    /// evenly, `floor((t + 1) r / 20) - floor(t r / 20)`.
    fn due(&self, played: u64) -> Result<u64, Error> {
        let ticks_per_second = u64::from(pharmakos_sim::math::quantity::TICK_HZ);
        let rate = u64::from(self.rate);
        let overflow = || Error::Overflow("the crater stream's schedule");
        let after = played
            .checked_add(1)
            .and_then(|next| next.checked_mul(rate))
            .and_then(|sum| sum.checked_div(ticks_per_second))
            .ok_or_else(overflow)?;
        let before = played
            .checked_mul(rate)
            .and_then(|sum| sum.checked_div(ticks_per_second))
            .ok_or_else(overflow)?;
        // A floor of a larger product is never smaller.
        after.checked_sub(before).ok_or_else(overflow)
    }

    /// One crater's centre, drawn against the world as it stands.
    fn draw(&mut self, world: &World) -> Result<[i32; 3], Error> {
        for _ in 0..REDRAWS {
            let roll = mix(self.seed, self.next);
            self.next = self.next.wrapping_add(1);
            let count =
                u64::try_from(self.sites.len()).map_err(|_| Error::Overflow("the sites"))?;
            let index = usize::try_from(roll.rem_euclid(count))
                .map_err(|_| Error::Overflow("a site index"))?;
            let site = self
                .sites
                .get(index)
                .copied()
                .ok_or(Error::Overflow("a site index"))?;
            let span = self
                .reach
                .checked_mul(2)
                .and_then(|both| both.checked_add(1))
                .ok_or(Error::Overflow("the sphere's span"))?;
            let span = u64::try_from(span).map_err(|_| Error::Overflow("the sphere's span"))?;
            let along = |bits: u32| -> Result<i32, Error> {
                i32::try_from((roll >> bits).rem_euclid(span))
                    .map_err(|_| Error::Overflow("an offset"))
                    .map(|offset| offset - self.reach)
            };
            let x = site[0]
                .checked_add(along(16)?)
                .ok_or(Error::Overflow("a crater's x"))?;
            let y = site[1]
                .checked_add(along(40)?)
                .ok_or(Error::Overflow("a crater's y"))?;
            if let Some(z) = world.voxels().top_solid_z(x, y) {
                return Ok([x, y, z]);
            }
            self.counts.redrawn += 1;
        }
        Err(Error::Layout(format!(
            "{REDRAWS} crater draws in a row found no ground around the layout's beacons"
        )))
    }

    /// File the craters due before the tick after `played` ticks.
    ///
    /// # Errors
    ///
    /// As [`CraterStream::draw`], and [`Error::Layout`] when the host refuses
    /// an edit as off the map or mid-tick, which is the harness's bug.
    pub fn file(&mut self, host: &mut Host, played: u64) -> Result<(), Error> {
        for _ in 0..self.due(played)? {
            let centre = self.draw(host.world())?;
            let edit = VoxelEdit::Crater {
                centre,
                radius: self.radius,
                by: None,
            };
            match host.file_voxel_edit(edit) {
                Ok(()) => self.counts.filed += 1,
                Err(FileRefused::QueueFull { .. }) => {
                    self.counts.queue_full += 1;
                }
                Err(refused @ (FileRefused::MidTick | FileRefused::OffTheMap)) => {
                    return Err(Error::Layout(format!(
                        "the host refused a crater at {centre:?}: {refused}"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_is_spread_evenly() {
        let stream = CraterStream {
            seed: 1,
            rate: 20,
            radius: 2,
            sites: vec![[0, 0]],
            reach: 24,
            next: 0,
            counts: StreamCounts::default(),
        };
        assert!((0..40).all(|tick| stream.due(tick) == Ok(1)));
        let half = CraterStream {
            rate: 10,
            ..stream.clone()
        };
        let total: u64 = (0..40)
            .map(|tick| half.due(tick).expect("the schedule fits"))
            .sum();
        assert_eq!(total, 20);
        let fast = CraterStream { rate: 40, ..stream };
        assert!((0..40).all(|tick| fast.due(tick) == Ok(2)));
    }

    #[test]
    fn the_generator_is_counter_based() {
        assert_eq!(mix(7, 3), mix(7, 3));
        assert_ne!(mix(7, 3), mix(7, 4));
        assert_ne!(mix(7, 3), mix(8, 3));
    }
}
