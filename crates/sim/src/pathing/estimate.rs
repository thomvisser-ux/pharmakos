// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The travel estimate (decisions log item 61, amended by item 90).
//!
//! # The contract, and what it forbids
//!
//! Spec section 11's allowed-estimates row reads *"Pathfinder travel estimates
//! over known terrain; fogged voxels use last-known state or ×1.5 cost"*, and
//! its forbidden column rules out stepping or forking the sim. AGENTS.md §3
//! rule 2 says the same in dependency terms. So [`estimate`] does exactly three
//! things and nothing else:
//!
//! 1. ask the connectivity oracle whether a route can exist at all;
//! 2. search the **abstract** graph and sum integer edge costs;
//! 3. divide by the walker's speed, rounding **up**.
//!
//! It never refines a leg, never walks a route and never touches the stepping
//! API. The only low-level work it does is the two bounded sweeps that attach
//! the endpoints to their own clusters' transition nodes, which are
//! cluster-local by construction — and, at cluster 32, 91 % of a median query
//! (item 58; the sweep is what S3 attacks if the budget bites, not the abstract
//! graph).
//!
//! # The error contract
//!
//! Within ±15 % at p90 of walked time on static terrain (G2 measured 2.6 %),
//! and **never optimistic** — the editor's rounding depends on that, so it is
//! part of the contract rather than an implementation detail (item 61). Two
//! things make it hold here rather than by luck:
//!
//! * the abstract cost is the cost of the **unsmoothed** refinement exactly,
//!   because every intra edge is an exact bounded search cost and every inter
//!   edge is one grid step; the smoother only ever removes cost, so the walker
//!   is never later than the estimate;
//! * `ticks` rounds up ([`ticks_for_cost`]), and the walker's own accumulator
//!   arrives on `ceil(cost * 20 / cost_per_second)` exactly (item 90), so the
//!   two computations of "when does it get there" agree at the tick.
//!
//! `tests/pathing.rs`'s `the_estimate_is_never_optimistic_at_any_percentile`
//! asserts the **signed** error, not `|error|`: an estimate that came in under
//! the walk is the failure mode, and an absolute-error percentile would hide
//! it.
//!
//! # Fog
//!
//! Priced per **abstract edge**, ×3/2 rounded up, when either endpoint cell is
//! unknown — `locomotion.fog_cost_numerator` over `fog_cost_denominator`. Edge
//! granularity is a deliberate approximation of section 11's per-cell rule:
//! counting an edge's unknown cells means walking them, and walking them is the
//! refinement the estimator may not do. What the editor may render from a
//! fogged leg is a dashed leg labelled as an upper bound, never an ETA
//! (item 61).
//!
//! # No cache
//!
//! P3-a passed with 18 % of headroom, and a cache is hashed state that has to
//! invalidate identically on three platforms. There is none, and adding one is
//! item 61's decision to reopen.
//!
//! # How a caller outside the sim reaches this without naming a stepping API
//!
//! The three things a query needs — the [`Surface`], the [`Clusters`] and a
//! [`Scratch`] — are the graph and a work buffer, and none of them can step
//! anything: [`Surface`] answers "is this step legal and what does it cost",
//! [`Clusters`] answers "is there a route at all", and the scratch holds arrays.
//! The gateway hosts the match, so it lends its own through
//! [`World::surface`](crate::world::World::surface),
//! [`World::clusters`](crate::world::World::clusters) and
//! [`World::scratch_mut`](crate::world::World::scratch_mut), and `plan-core`
//! names no other sim type to ask a travel question (AGENTS.md §3 rule 2).
//! A caller that has only a [`Snapshot`](crate::snapshot::Snapshot) can build
//! its own pair from the store the restore rebuilds — but it should not want to:
//! the decomposition is seconds of work, and the whole point of borrowing the
//! host's is that the answer is about the match that is actually running.

use crate::math::quantity::TICK_HZ;
use crate::pathing::clusters::Clusters;
use crate::pathing::route::abstract_search;
use crate::pathing::search::Scratch;
use crate::pathing::surface::{Node, Surface};

/// What the query knows about the terrain it is pricing.
///
/// The mask is one entry per column, `true` where the asking seat has never
/// seen it. Producing it is the gateway's fog filter (T9); the sim only prices
/// what it is handed, which is what keeps fog a per-match server-side policy
/// rather than something baked into the graph.
#[derive(Clone, Copy, Debug)]
pub enum Fog<'a> {
    /// Everything on the route is known: every edge costs what it costs.
    Clear,
    /// One entry per column, `true` where the seat has not seen it.
    Unknown(&'a [bool]),
}

impl Fog<'_> {
    /// Whether the seat has never seen this column.
    #[must_use]
    pub fn is_unknown(self, node: Node) -> bool {
        match self {
            Fog::Clear => false,
            Fog::Unknown(mask) => usize::try_from(node)
                .ok()
                .and_then(|index| mask.get(index).copied())
                .unwrap_or(false),
        }
    }

    /// Whether anything at all is hidden. A query with a clear mask prices
    /// nothing at ×3/2 and can say so to the caller that renders the leg.
    #[must_use]
    pub const fn is_clear(self) -> bool {
        matches!(self, Fog::Clear)
    }
}

/// A travel estimate: item 61's three numbers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Estimate {
    /// The summed abstract cost, in cost units.
    pub cost: i32,
    /// `ceil(cost * 20 / cost_per_second)` — the tick the walker arrives on.
    pub ticks: i32,
    /// Abstract edges crossed. What the editor draws the polyline from.
    pub legs: u32,
    /// Whether any edge on the route was priced through fog.
    ///
    /// Not one of item 61's three numbers and not part of the estimate: it is
    /// how the caller knows to draw the leg dashed and label it a bound rather
    /// than an ETA (item 57). A caller that ignores it renders a bound as a
    /// promise, which is the one rendering item 61 forbids.
    pub fogged: bool,
}

/// Cost units to ticks, rounding **up** (items 59 and 90).
///
/// A walker adds `cost_per_second` to an integer accumulator each tick and
/// spends one cost unit per [`TICK_HZ`] accumulated, so it finishes a route of
/// cost `c` on tick `ceil(c * 20 / cost_per_second)` exactly. Rounding up is
/// what keeps the estimate one-sided: a unit has not arrived until the tick on
/// which it arrives.
///
/// A speed of zero cannot walk, and the answer is `None` rather than a
/// division by zero.
#[must_use]
#[allow(
    clippy::integer_division,
    reason = "the rounding is the point of the function: the numerator carries the +(cps - 1) that makes the division a ceiling, and the doc comment above is its justification"
)]
pub fn ticks_for_cost(cost: i32, cost_per_second: i32) -> Option<i32> {
    if cost_per_second <= 0 {
        return None;
    }
    if cost <= 0 {
        return Some(0);
    }
    let scaled = i64::from(cost).saturating_mul(i64::from(TICK_HZ));
    let rounded = scaled.saturating_add(i64::from(cost_per_second).saturating_sub(1));
    i32::try_from(rounded / i64::from(cost_per_second)).ok()
}

/// Price one abstract edge under fog: `cost * numerator / denominator`, rounded
/// up, when either endpoint cell is unknown.
#[must_use]
#[allow(
    clippy::integer_division,
    reason = "the rounding is the rule: item 61 prices a fogged edge at cost * 3 / 2 rounded up, so the numerator carries the +(denominator - 1)"
)]
pub fn fog_price(cost: i32, numerator: i32, denominator: i32) -> i32 {
    if denominator <= 0 || numerator <= 0 {
        return cost;
    }
    let scaled = i64::from(cost)
        .saturating_mul(i64::from(numerator))
        .saturating_add(i64::from(denominator).saturating_sub(1));
    i32::try_from(scaled / i64::from(denominator)).unwrap_or(i32::MAX)
}

/// **The estimate query.** `None` means no route exists, and it is a normal,
/// cheap answer: the connectivity oracle decides it before any search runs (a
/// crater seals you in as well as a moat does).
///
/// Item 61 spells the parameters `(world, clusters, scratch, start, goal,
/// fog)`. The seventh is item 90's amendment and nothing else moved: the
/// walker's speed became a per-kind rules row in cost units per second, and
/// `ticks` is the one number of the three that depends on it. Everything else
/// item 61 fixes is unchanged — `None` from the oracle before any search,
/// `legs` as the abstract-edge count, fog at ×3/2 per abstract edge, and no
/// cache.
///
/// `fog_numerator` and `fog_denominator` come from the rules table, so the
/// multiplier is data rather than a constant here (AGENTS.md §12).
pub fn estimate(
    world: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    start: Node,
    goal: Node,
    fog: Fog<'_>,
    speed: Speed,
) -> Option<Estimate> {
    let result = abstract_search(world, clusters, scratch, start, goal, fog, speed)?;
    let ticks = ticks_for_cost(result.cost, speed.cost_per_second)?;
    Some(Estimate {
        cost: result.cost,
        ticks,
        legs: result.legs,
        fogged: result.fogged,
    })
}

/// The walker's speed and the fog multiplier, which are the two rules-table
/// facts a query needs beyond the graph itself.
///
/// One struct rather than three parameters because all three come from the same
/// table and a caller that gets one of them from somewhere else has already
/// made the mistake AGENTS.md §12 is about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Speed {
    /// `locomotion.<kind>_cost_per_second` (item 90).
    pub cost_per_second: i32,
    /// `locomotion.fog_cost_numerator` (item 61: 3).
    pub fog_numerator: i32,
    /// `locomotion.fog_cost_denominator` (item 61: 2).
    pub fog_denominator: i32,
}

impl Speed {
    /// The three rows for one unit kind.
    ///
    /// `None` when the kind has no `cost_per_second` row, which is a rules-table
    /// mistake rather than a game state.
    #[must_use]
    pub fn of_kind(
        rules: &crate::rules::RulesTable,
        kind: crate::tables::UnitKind,
    ) -> Option<Speed> {
        Some(Speed {
            cost_per_second: rules.cost_per_second(kind)?,
            fog_numerator: rules.fog_cost_numerator(),
            fog_denominator: rules.fog_cost_denominator(),
        })
    }

    /// The commander's speed, which is what the editor renders a route at until
    /// the playbook names another walker (item 57).
    #[must_use]
    pub fn commander(rules: &crate::rules::RulesTable) -> Speed {
        Speed {
            cost_per_second: rules.commander_cost_per_second(),
            fog_numerator: rules.fog_cost_numerator(),
            fog_denominator: rules.fog_cost_denominator(),
        }
    }
}
