// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The travel estimate (decisions log item 61, amended by item 90 and by lane
//! `p3fix`'s near pairs).
//!
//! # The contract, and what it forbids
//!
//! Spec section 11's allowed-estimates row reads *"Pathfinder travel estimates
//! over known terrain; fogged voxels use last-known state or ×1.5 cost"*, and
//! its forbidden column rules out stepping or forking the sim. AGENTS.md §3
//! rule 2 says the same in dependency terms. So [`estimate`] does these things
//! and nothing else:
//!
//! 1. ask the connectivity oracle whether a route can exist at all;
//! 2. for a **far** pair, search the **abstract** graph and sum integer edge
//!    costs; for a **near** pair (below), compute the route the router would
//!    serve and sum its steps;
//! 3. divide by the walker's speed, rounding **up**.
//!
//! It never walks a route, never steps or forks the sim and never touches the
//! stepping API: computing a route is a pure function of the graph, the same
//! one the router calls, and nothing it computes is kept. The low-level work a
//! far pair does is the two bounded sweeps that attach the endpoints to their
//! own clusters' transition nodes, which are cluster-local by construction.
//! Spike G2 measured them at 91 % of a median query (item 58); on the sim's
//! generated map they are nearly all of it: a median far query popped about
//! 2 050 nodes, of which the abstract search's own pops were a handful (lane
//! `p3fix`'s step 0, `tests/pathing_bench.rs` publishes the share). The sweep is
//! what S3 attacks if the budget bites, not the abstract graph.
//!
//! # Near pairs: the served route's cost
//!
//! The abstract cost is the cost of the **unsmoothed** refinement exactly. On a
//! short route that crosses a cluster border, the border's transition node can
//! lie several cells off the straight line, and the smoother cuts that detour
//! for the walker while the abstract cost still charges it: before this, the
//! p90 error was 822 per mille on routes of 4 to 8 cells, with the abstract
//! cost up to six times the walk. Lane `p3fix`'s step 0 measured where it came
//! from on 1 080 walks over three seeds: a single border crossing off the line
//! carried 90 % to 99 % of the excess in every band of 32 cells and under, a
//! cluster corner under 1 %.
//!
//! So a pair whose two clusters **touch** — the same cluster, or two that share
//! a border or a corner ([`Clusters::touch`]) — is priced at the route the
//! router serves it: [`crate::pathing::route::route_with`] under no surcharge,
//! the router's own function on the same graph, written into the scratch's own
//! buffer. The radius is the cluster edge and nothing picked: two columns at
//! most one cluster edge apart on both axes always lie in touching clusters, so
//! every route of 32 cells or less is priced exactly, which is where one
//! crossing's detour is a large share of the walk. A far pair keeps the
//! abstract answer, whose error the map-wide draw holds inside the gate. `legs`
//! stays the abstract edge count either way.
//!
//! A near pair whose route does not fit [`crate::pathing::MAX_ROUTE_NODES`]
//! (a detour longer than any map holds today) is answered at the abstract cost,
//! the rule a far pair uses: the walker walks the partial route and asks for
//! the rest from its end, and each of those asks costs no more than the raw
//! refinement's remainder, so the abstract cost still bounds the walk. Each
//! further ask also waits for its own serving tick, which no estimate counts,
//! as for a far pair's partial route. `tests/pathing.rs`'s
//! `a_partial_near_route_is_answered_at_no_less_than_its_walk` builds one. A route
//! the router could not refine (`NoPath::Unrefinable` or `NoPath::Unpriced`)
//! answers `None`, as no route: the walker would be sealed in too, and the
//! estimate never falls back to the abstract answer for it.
//!
//! # The error contract
//!
//! Within ±15 % at p90 of walked time on static terrain (G2 measured 2.6 %),
//! and **never optimistic** — the editor's rounding depends on that, so it is
//! part of the contract rather than an implementation detail (item 61).
//! "Walked" counts from the tick that serves the route, as `tests/pathing.rs`'s
//! P3 gate reads it. Three things make it hold here rather than by luck:
//!
//! * a near pair's cost **is** the served route's cost, by construction;
//! * a far pair's abstract cost is the cost of the unsmoothed refinement
//!   exactly, because every intra edge is an exact bounded search cost and
//!   every inter edge is one grid step; the smoother only ever removes cost,
//!   so the walker is never later than the estimate;
//! * `ticks` rounds up ([`ticks_for_cost`]), and the walker's own accumulator
//!   arrives on `ceil(cost * 20 / cost_per_second)` exactly (item 90), so the
//!   two computations of "when does it get there" agree at the tick.
//!
//! `tests/pathing.rs`'s `the_estimate_is_never_optimistic_at_any_percentile`
//! asserts the **signed** error, not `|error|`: an estimate that came in under
//! the walk is the failure mode, and an absolute-error percentile would hide
//! it. Its P3 tests walk real commanders, map-wide and on short draws.
//!
//! What the estimate does not price: the route surcharge the router applies to
//! a unit under an Attack beacon's avoid areas or on an `AVOID_KNOWN_THREATS`
//! step ([`crate::pathing::route::Surcharge`]). Such a walk can be later than
//! any estimate here; that predates the near pairs and is owed to the next
//! pathing lane.
//!
//! # Fog
//!
//! Priced per **abstract edge**, ×3/2 truncated ([`fog_price`]), when either
//! endpoint cell is unknown — `locomotion.fog_cost_numerator` over
//! `fog_cost_denominator`. Edge granularity is a deliberate approximation of
//! section 11's per-cell rule. What the editor may render from a fogged leg is
//! a dashed leg labelled as an upper bound, never an ETA (item 61).
//!
//! A query with a fog mask takes the abstract path for **every** pair, a near
//! one included: the served route read through fog would tell a seat things
//! about columns it has not seen that the abstract answer does not, and fog
//! policy is the gateway's and the owner's (AGENTS.md section 7).
//! `tests/pathing.rs`'s `a_near_route_through_fog_tells_what_the_abstract_one_does_not`
//! is the evidence: cratering only unseen ground moved the served-route answer
//! alone on 26 of 50 near pairs, the abstract answer alone on none, and both
//! on 17.
//!
//! **The gap, plainly.** Under a mask a near pair is priced as it was before
//! the near pairs existed, so the short-route miss the P3 gate measured comes
//! back for any caller that sets a mask; and both fogged answers read true
//! terrain under the unseen columns (the 17 above), which section 11's
//! "last-known state" does not allow. Nothing reachable has the gap today: no
//! production caller sets a mask (the gateway's `RouteAdapter` holds an empty
//! one, so every query runs `Fog::Clear`). Before a stage wires the mask, the
//! fogged estimate needs a design of its own and the owner's fog ruling.
//!
//! PLACEHOLDER: the fogged estimate's own design (last-known state) and the owner's fog ruling, before the mask is wired — owner, at S4, with the knowledge store's mask (S3's plan, decision 2: S3-05 moved to S4).
//!
//! The cap item 61 sets, "+50 % over the clear estimate", was written when the
//! clear estimate was the abstract one; it reads as the markup's own bound, at
//! most [`fog_price`] of the **abstract** clear cost.
//! `fog_never_makes_a_route_look_cheaper` asserts that, and that the fogged
//! answer is never below the clear one as answered, which holds because a
//! served route never costs more than the abstract answer.
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
use crate::pathing::MAX_ROUTE_NODES;
use crate::pathing::clusters::Clusters;
use crate::pathing::route::{NoPath, Surcharge, abstract_search, route_and_abstract};
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
                // E7: rule: a column the fog mask does not cover is not unknown
                .unwrap_or(false),
        }
    }

    /// Whether nothing at all is hidden: [`Fog::Clear`], or a mask with no
    /// unknown column. Such a query prices nothing at ×3/2, and a near pair
    /// under it is priced at the served route like any clear query (lane
    /// `p3fix`): a mask that hides nothing is a clear query, whoever built it.
    #[must_use]
    pub fn is_clear(self) -> bool {
        match self {
            Fog::Clear => true,
            Fog::Unknown(mask) => !mask.contains(&true),
        }
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

/// Price one abstract edge under fog: integer `cost * numerator / denominator`,
/// when either endpoint cell is unknown.
///
/// Item 61 spells the rule as integer `cost * 3 / 2` and bounds the result at
/// **+50 % over the clear estimate**. Integer division in Rust truncates, so
/// that is the arithmetic written here and no rounding term is added: a ceiling
/// would price a one-unit edge at two, which is +100 % and outside the cap the
/// same sentence sets. The fogged total is therefore at most `3/2` of the clear
/// total exactly, which is what `fog_never_makes_a_route_look_cheaper`
/// asserts.
#[must_use]
#[allow(
    clippy::integer_division,
    reason = "item 61 spells the fog multiplier as integer cost * 3 / 2, so the truncation is the rule rather than an accident of the type"
)]
pub fn fog_price(cost: i32, numerator: i32, denominator: i32) -> i32 {
    if denominator <= 0 || numerator <= 0 {
        return cost;
    }
    let scaled = i64::from(cost).saturating_mul(i64::from(numerator));
    // E7: rule: item 61's fog price saturates at the most a cost holds
    i32::try_from(scaled / i64::from(denominator)).unwrap_or(i32::MAX)
}

/// **The estimate query.** `None` means no route exists, and it is a normal,
/// cheap answer: the connectivity oracle decides it before any search runs (a
/// crater seals you in as well as a moat does).
///
/// Item 61 spells the parameters `(world, clusters, scratch, start, goal, fog)`
/// and the result `Estimate { cost, ticks, legs }`. Three things differ, all of
/// them stated in T7's PR body rather than left to be discovered at the call
/// site:
///
/// 1. **A seventh parameter**, [`Speed`]. Item 90 made the walker's speed a
///    per-kind rules row in cost units per second, and `ticks` is the one
///    number of the three that depends on it.
/// 2. **The first parameter is [`Surface`], not `World`.** The estimator needs
///    the columns and the step rule and nothing else, and taking the surface is
///    what makes it structurally unable to reach the stepping API that
///    AGENTS.md §3 rule 2 forbids it. A caller holding a `World` reaches it
///    through `World::surface()`.
/// 3. **[`Estimate`] carries a fourth field**, `fogged`, which is additive and
///    is not one of item 61's three numbers — it is how the caller knows to
///    render the leg as a bound rather than an ETA.
///
/// Everything else item 61 fixes is unchanged: `None` from the oracle before
/// any search, `legs` as the abstract-edge count, fog at ×3/2 per abstract edge
/// as integer `cost * 3 / 2`, never optimistic, and no cache. One sentence of
/// item 61 is amended: a near pair is priced at the route the router serves
/// rather than by the abstract search alone (the module docs' "Near pairs").
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
    if clusters.touch(start, goal) && fog.is_clear() {
        return near(world, clusters, scratch, (start, goal), speed);
    }
    let result = abstract_search(world, clusters, scratch, start, goal, fog, speed)?;
    let ticks = ticks_for_cost(result.cost, speed.cost_per_second)?;
    Some(Estimate {
        cost: result.cost,
        ticks,
        legs: result.legs,
        fogged: result.fogged,
    })
}

/// A near pair's estimate: the cost of the route the router serves (the
/// module docs' "Near pairs").
fn near(
    world: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    (start, goal): (Node, Node),
    speed: Speed,
) -> Option<Estimate> {
    scratch.near_queries.bump();
    let mut served = core::mem::take(&mut scratch.served);
    let routed = route_and_abstract(
        world,
        clusters,
        scratch,
        (start, goal),
        Surcharge::NONE,
        &mut served,
    );
    scratch.served = served;
    // Any `NoPath` is no route: the walker would be sealed in as well.
    let (abstract_result, outcome) = routed.ok()?;
    // A partial route is priced at the abstract cost, which bounds the walk
    // of a partial route and its later asks (the module docs).
    let cost = if outcome.partial {
        abstract_result.cost
    } else {
        outcome.cost
    };
    let ticks = ticks_for_cost(cost, speed.cost_per_second)?;
    Some(Estimate {
        cost,
        ticks,
        legs: abstract_result.legs,
        fogged: false,
    })
}

/// Why [`walk_cost`] priced no walk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoWalk {
    /// The router serves no route from where the walk stands: its typed
    /// reason.
    NoPath(NoPath),
    /// The router's partial routes, chained, walked more columns than the map
    /// holds without arriving. Each partial route is [`MAX_ROUTE_NODES`]
    /// columns of a refined walk, so a walk that arrives in a simple path
    /// never needs more of them than the map's columns divided by that; a
    /// chain past it revisits columns, which no route the router refines
    /// does. `tests/targeting.rs` pins it unreached on generated maps.
    Overrun,
}

/// **The cost of the walk the router serves** from `start` to `goal`, near or
/// far: [`route_and_abstract`] under no surcharge, the router's own function,
/// written into the scratch's own buffer, and when the route stops short at
/// [`MAX_ROUTE_NODES`] the rest asked for from its end, as the walker asks
/// when it gets there. It is what a "nearest" ranks by (lane `p3fix`): exact,
/// with no estimate standing in for the walk. It steps nothing; like a near
/// pair's estimate it is a pure function of the graph.
///
/// # Errors
///
/// [`NoWalk::NoPath`] when the router serves no route, and
/// [`NoWalk::Overrun`] when chained partial routes pass the map's columns.
pub fn walk_cost(
    world: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    start: Node,
    goal: Node,
) -> Result<i64, NoWalk> {
    scratch.walks.bump();
    let mut served = core::mem::take(&mut scratch.served);
    let walked = walk_into(world, clusters, scratch, (start, goal), &mut served);
    scratch.served = served;
    walked
}

/// The new columns a partial route adds to a walk: all of its
/// [`MAX_ROUTE_NODES`] but the one it starts on.
const PARTIAL_PROGRESS: u32 = MAX_ROUTE_NODES - 1;
const _: () = assert!(PARTIAL_PROGRESS > 0, "a partial route makes progress");

/// [`walk_cost`]'s chain, with the served buffer taken out of the scratch so
/// that every return hands it back.
fn walk_into(
    world: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    (start, goal): (Node, Node),
    served: &mut Vec<Node>,
) -> Result<i64, NoWalk> {
    // A partial route adds `MAX_ROUTE_NODES - 1` new columns to the walk, so a
    // walk over at most every column of the map takes at most this many
    // partial routes before the whole one.
    let partials = world.node_count().div_ceil(PARTIAL_PROGRESS);
    let mut from = start;
    let mut total = 0_i64;
    let mut asked = 0_u32;
    loop {
        let (_, outcome) = route_and_abstract(
            world,
            clusters,
            scratch,
            (from, goal),
            Surcharge::NONE,
            served,
        )
        .map_err(NoWalk::NoPath)?;
        total = total
            .checked_add(i64::from(outcome.cost))
            .ok_or(NoWalk::Overrun)?;
        if !outcome.partial {
            return Ok(total);
        }
        if asked >= partials {
            return Err(NoWalk::Overrun);
        }
        asked += 1;
        // E7: cannot miss: a partial route holds `MAX_ROUTE_NODES` columns
        from = served.last().copied().ok_or(NoWalk::Overrun)?;
    }
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
    /// Every kind has one: the rules loader refuses a table missing a row.
    #[must_use]
    pub const fn of_kind(rules: &crate::rules::RulesTable, kind: crate::tables::UnitKind) -> Speed {
        Speed {
            cost_per_second: rules.cost_per_second(kind),
            fog_numerator: rules.fog_cost_numerator(),
            fog_denominator: rules.fog_cost_denominator(),
        }
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
