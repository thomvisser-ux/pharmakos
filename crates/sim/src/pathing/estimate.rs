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
//! 2. under no fog, price the **walk the router serves** ([`walk_cost`], the
//!    function every "nearest" ranks by); under a fog mask, search the
//!    **abstract** graph and sum integer edge costs (below, "Fog");
//! 3. divide by the walker's speed, rounding **up**.
//!
//! It never walks a route, never steps or forks the sim and never touches the
//! stepping API: computing a route is a pure function of the graph, the same
//! one the router calls, and nothing it computes is kept.
//!
//! # A clear query: the served walk
//!
//! The abstract cost is the cost of the **unsmoothed** refinement exactly. On a
//! route that crosses a cluster border, the border's transition node can lie
//! several cells off the straight line, and the smoother cuts that detour for
//! the walker while the abstract cost still charges it: the p90 error was 822
//! per mille on routes of 4 to 8 cells, with the abstract cost up to six times
//! the walk (lane `p3fix`'s step 0, on 1 080 walks over three seeds). Lane
//! `p3fix` first priced only near pairs (two clusters that touch) at the route
//! the router serves; its gateway step prices **every** clear query so, near
//! or far, because the abstract answer and the served walk are two measures,
//! and a chip's `travel_ms`, `resolve_refs`' binding, `estimate_route`'s
//! answer and the Push's walk must be one. A clear estimate's cost **is**
//! [`walk_cost`]'s: the same function the sim's
//! [`crate::targeting::Ranker`] ranks by, over the same graph, under no
//! surcharge. `legs` stays the abstract edge count, the first query's.
//!
//! A walk whose route does not fit [`crate::pathing::MAX_ROUTE_NODES`] is
//! priced as the walker walks it: the partial route, then the router's next
//! ask from its end, chained until the goal ([`walk_cost`]'s "The chain
//! ends"). A route the router could not refine (`NoPath::Unrefinable` or
//! `NoPath::Unpriced`) answers `None`, as no route: the walker would be sealed
//! in too. A chain the router broke ([`WalkError::Stalled`]) is
//! [`EstimateError::Stalled`], never "no route" and never an abstract figure
//! standing in.
//!
//! The cost of pricing the walk: a far query now refines and smooths its route
//! as the router does, where it used to stop at the abstract search, whose
//! endpoint sweeps were nearly all of a median far query (about 2 050 pops,
//! item 58 and `tests/pathing_bench.rs`). It runs outside the tick
//! (`estimate_route` in a Lull), so no budget caps it; the gateway step's PR
//! states its counted work and its time.
//!
//! # The error contract
//!
//! Within ±15 % at p90 of walked time on static terrain (G2 measured 2.6 %),
//! and **never optimistic** — the editor's rounding depends on that, so it is
//! part of the contract rather than an implementation detail (item 61).
//! "Walked" counts from the tick that serves the route, as `tests/pathing.rs`'s
//! P3 gate reads it. Three things make it hold here rather than by luck:
//!
//! * a clear query's cost **is** the served walk's cost, by construction;
//! * `ticks` rounds up ([`ticks_for_cost`]), and the walker's own accumulator
//!   arrives on `ceil(cost * 20 / cost_per_second)` exactly (item 90), so the
//!   two computations of "when does it get there" agree at the tick, for a
//!   walk the router serves in one route;
//! * a fogged query's abstract cost is the cost of the unsmoothed refinement
//!   exactly, because every intra edge is an exact bounded search cost and
//!   every inter edge is one grid step; the smoother only ever removes cost,
//!   and fog only adds it.
//!
//! **A chained walk arrives later than its cost says.** A walker at the end of
//! a partial route banks nothing into the next one and waits for the repath
//! queue to serve its next ask (`crate::pathing::router`), so a walk of more
//! than [`crate::pathing::MAX_ROUTE_NODES`] columns takes up to a tick more per
//! ask than `ticks`, plus the queue's wait, which depends on every other
//! walker's asks: pricing it would be modelling the match, which no estimate
//! may do (AGENTS.md section 3 rule 2). Before the gateway step the abstract
//! cost's slack over the walk covered that; it was never counted either.
//! `tests/pathing.rs`'s serpentine builds such a walk.
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
//! A query with a fog mask takes the abstract path for **every** pair: the
//! served route read through fog would tell a seat things about columns it has
//! not seen that the abstract answer does not, and fog policy is the gateway's
//! and the owner's (AGENTS.md section 7).
//! `tests/pathing.rs`'s `a_near_route_through_fog_tells_what_the_abstract_one_does_not`
//! is the evidence: cratering only unseen ground moved the served-route answer
//! alone on 26 of 50 near pairs, the abstract answer alone on none, and both
//! on 17.
//!
//! **The gap, plainly.** Under a mask a pair is priced as every pair was before
//! lane `p3fix`, so the short-route miss the P3 gate measured comes back for
//! any caller that sets a mask; and both fogged answers read true
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
use crate::pathing::route::{
    AbstractResult, NoPath, RouteOutcome, Surcharge, abstract_search, route_and_abstract,
};
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
/// Everything else item 61 fixes is unchanged: no route from the oracle before
/// any search, `legs` as the abstract-edge count, fog at ×3/2 per abstract edge
/// as integer `cost * 3 / 2`, and no cache. Two sentences of item 61 are
/// amended: a clear query is priced at the walk the router serves rather than
/// by the abstract search (the module docs' "A clear query"), and "never
/// optimistic" holds for a walk served in one route (the module docs' "A
/// chained walk arrives later than its cost says").
///
/// `fog_numerator` and `fog_denominator` come from the rules table, so the
/// multiplier is data rather than a constant here (AGENTS.md §12).
///
/// # Errors
///
/// [`EstimateError`] when the served walk cannot be priced: a chain the router
/// broke, or a cost past the estimate's `i32`. `Ok(None)` is no route, the
/// normal answer.
pub fn estimate(
    world: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    start: Node,
    goal: Node,
    fog: Fog<'_>,
    speed: Speed,
) -> Result<Option<Estimate>, EstimateError> {
    // A column off the graph has no route at all, as the router says
    // (`NoPath::Unplaced`): no estimate, clear or fogged.
    if clusters.touch(start, goal).is_err() {
        return Ok(None);
    }
    if fog.is_clear() {
        return served(world, clusters, scratch, (start, goal), speed);
    }
    let Some(result) = abstract_search(world, clusters, scratch, start, goal, fog, speed) else {
        return Ok(None);
    };
    // The partial routes the walk can chain, bounded by its cost as
    // `walk_cost`'s chain is ("The chain ends"): at least `PARTIAL_PROGRESS`
    // cardinal steps a partial route, and the fogged cost is at least the
    // clear abstract cost the chain starts from.
    let fall = i64::from(PARTIAL_PROGRESS) * i64::from(world.costs().cheapest().get());
    let links = i64::from(result.cost).div_euclid(fall);
    let walk = ServedWalk {
        cost: i64::from(result.cost),
        partials: u32::try_from(links).map_err(|_| EstimateError::PastTicks { ticks: links })?,
    };
    // A walker with no speed walks nowhere: no estimate.
    let Some(ticks) = walk_ticks(walk, speed)? else {
        return Ok(None);
    };
    Ok(Some(Estimate {
        cost: result.cost,
        ticks,
        legs: result.legs,
        fogged: result.fogged,
    }))
}

/// A clear query's estimate: the walk the router serves, priced by
/// [`walk_cost`]'s own chain (the module docs' "A clear query").
fn served(
    world: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    pair: (Node, Node),
    speed: Speed,
) -> Result<Option<Estimate>, EstimateError> {
    scratch.served_estimates.bump();
    let walked = match served_walk(world, clusters, scratch, pair) {
        Ok(walked) => walked,
        // Any `NoPath` is no route: the walker would be sealed in as well.
        Err(WalkError::NoPath(_)) => return Ok(None),
        Err(WalkError::Stalled { partials }) => {
            return Err(EstimateError::Stalled { partials });
        }
    };
    let cost =
        i32::try_from(walked.cost).map_err(|_| EstimateError::PastCost { cost: walked.cost })?;
    // A walker with no speed walks nowhere: no estimate.
    let Some(ticks) = walk_ticks(walked.walk(), speed)? else {
        return Ok(None);
    };
    Ok(Some(Estimate {
        cost,
        ticks,
        legs: walked.legs,
        fogged: false,
    }))
}

/// A served walk, as [`walk_cost`] prices it: its summed step costs, and how
/// many partial routes it chained before the one that reached the goal.
/// "Nearest" ranks by `cost`; [`walk_ticks`] turns it into the tick the walker
/// arrives on, at the latest.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ServedWalk {
    /// The walk's summed step costs.
    pub cost: i64,
    /// The partial routes before the last: each a re-ask the walker makes
    /// where one stops.
    pub partials: u32,
}

/// The tick a walker arrives on, at the latest, walking `walk` at `speed`:
/// the walk's cost rounded up to the tick once ([`ticks_for_cost`]), and
/// [`Speed::link_ticks`] for each partial route, the most a re-ask can cost it
/// under any load ([`crate::rules::RulesTable::link_ticks`]). **Never
/// optimistic** (decisions-log items 57 and 61): a walk served in one route
/// arrives on exactly this tick; a chained walk on this tick at the latest,
/// and earlier by its rounding and the queue's wait as they fall. The one
/// conversion `estimate_route`, `get_map_summary`'s `travel_ms` and
/// `resolve_refs`' candidates all read.
///
/// `Ok(None)` for a walker with no speed, which walks nowhere.
///
/// # Errors
///
/// [`EstimateError::PastCost`] for a cost past an `i32`, and
/// [`EstimateError::PastTicks`] for a tick count past one: the walk's
/// argument says neither occurs, and each is said rather than truncated.
pub fn walk_ticks(walk: ServedWalk, speed: Speed) -> Result<Option<i32>, EstimateError> {
    let cost = i32::try_from(walk.cost).map_err(|_| EstimateError::PastCost { cost: walk.cost })?;
    let Some(ticks) = ticks_for_cost(cost, speed.cost_per_second) else {
        return Ok(None);
    };
    // A `u32` count times an `i32`, plus an `i32`: inside an `i64`.
    let total = i64::from(ticks) + i64::from(walk.partials) * i64::from(speed.link_ticks);
    i32::try_from(total)
        .map(Some)
        .map_err(|_| EstimateError::PastTicks { ticks: total })
}

/// Why [`estimate`] priced no walk for a clear query that has a route.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EstimateError {
    /// [`WalkError::Stalled`]: a partial route's next query did not fall, so
    /// the chain that prices the walk could not be trusted to end.
    Stalled {
        /// The partial routes chained before the query that did not fall.
        partials: u32,
    },
    /// The walk's summed cost passes the estimate's `i32`. A chained walk
    /// costs no more than its first query's abstract cost, an `i32` (each
    /// partial route costs at most the refinement prefix it smooths, and the
    /// next query at most the refinement's remainder), so this is the sim
    /// disagreeing with its own argument, said rather than truncated.
    PastCost {
        /// The walk's summed cost.
        cost: i64,
    },
    /// The walk's ticks, its re-asks' bound included, pass a signed 32-bit
    /// tick count: past any match by orders of magnitude, said rather than
    /// truncated.
    PastTicks {
        /// The ticks.
        ticks: i64,
    },
}

impl core::fmt::Display for EstimateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EstimateError::Stalled { partials } => write!(
                f,
                "a walk's partial route did not bring its next query closer, after {partials} \
                 partial routes"
            ),
            EstimateError::PastCost { cost } => write!(
                f,
                "a walk costs {cost}, past the estimate's signed 32-bit cost"
            ),
            EstimateError::PastTicks { ticks } => write!(
                f,
                "a walk takes up to {ticks} ticks, past the estimate's signed 32-bit tick count"
            ),
        }
    }
}

impl std::error::Error for EstimateError {}

/// **The cost of the walk the router serves** from `start` to `goal`, near or
/// far, under no surcharge (no ranked choice is walked under one: `targeting`'s
/// "Two measures"): [`route_and_abstract`], the router's own function, written
/// into the scratch's own buffer, and when the route stops short at
/// [`MAX_ROUTE_NODES`] the rest asked for from its end, as the walker asks
/// when it gets there. It is what every "nearest" a unit walks to ranks by
/// (lane `p3fix`): the walk itself, with no estimate standing in for it. The
/// answer is the walk's step costs ([`crate::pathing::search::route_cost`])
/// and how many partial routes it chained ([`ServedWalk`]), which
/// [`walk_ticks`] turns into the tick the walker arrives on at the latest. It
/// steps nothing, and is a pure function of the graph.
///
/// # The chain ends
///
/// A partial route is the first [`MAX_ROUTE_NODES`] columns of the
/// refinement of the abstract graph's best path, and the abstract cost is
/// that refinement's cost exactly (the module docs). From the partial route's
/// last column the rest of that refinement is still a path the abstract
/// search can price: to the next transition node inside the column's own
/// cluster, which the endpoint sweep finds at no more than the refinement's
/// cost, then the same abstract edges. So the next query's abstract cost is
/// at most the last one less the [`PARTIAL_PROGRESS`] steps just walked,
/// each costing at least a cardinal step (the rules hold a diagonal no
/// cheaper). The chain carries that falling cost and holds every query to the
/// fall, so it holds at most the first query's abstract cost divided by
/// `PARTIAL_PROGRESS × cardinal` partial routes: the loop cannot run past it.
/// When that first cost is at most the map's `max_path_cost` (the dearest
/// step times the map's columns, the most any simple walk costs, which an
/// optimal refinement is), the count is at most [`max_partials`] (259 on the
/// shipped map); the chain test asserts the first cost's bound on its chains.
/// A query that does not fall is [`WalkError::Stalled`], never a missing
/// route. `tests/pathing.rs`'s `a_partial_route_chain_spends_its_abstract_cost`
/// holds real chains to the fall, and this module's tests reach `Stalled`
/// with a router that does not progress.
///
/// # Errors
///
/// [`WalkError::NoPath`], the router's reason, when it serves no route from
/// where the walk stands; [`WalkError::Stalled`] when a partial route's next
/// query does not fall as the argument above says it must.
pub fn walk_cost(
    world: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    (start, goal): (Node, Node),
) -> Result<ServedWalk, WalkError> {
    scratch.walks.bump();
    served_walk(world, clusters, scratch, (start, goal)).map(Walked::walk)
}

/// A served walk: its cost, and the abstract edges its first query crossed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Walked {
    /// The walk's summed step costs.
    pub(crate) cost: i64,
    /// The abstract edges of the first query's route: what the editor draws
    /// a leg's polyline from ([`Estimate::legs`]).
    pub(crate) legs: u32,
    /// The partial routes chained before the last.
    pub(crate) partials: u32,
}

impl Walked {
    /// The walk as [`walk_cost`] answers it.
    pub(crate) const fn walk(self) -> ServedWalk {
        ServedWalk {
            cost: self.cost,
            partials: self.partials,
        }
    }
}

/// [`walk_cost`] and a clear [`estimate`]'s one walk: [`walk_chain`] over the
/// router's own function, in the scratch's own buffer, under no surcharge.
fn served_walk(
    world: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    pair: (Node, Node),
) -> Result<Walked, WalkError> {
    let mut served = core::mem::take(&mut scratch.served);
    let cheapest = world.costs().cheapest();
    let walked = walk_chain(pair, cheapest, &mut served, |pair, out| {
        route_and_abstract(world, clusters, scratch, pair, Surcharge::NONE, out)
    });
    scratch.served = served;
    walked
}

/// Why [`walk_cost`] priced no walk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkError {
    /// The router serves no route from where the walk stands: its reason.
    NoPath(NoPath),
    /// A partial route's next query did not fall by [`PARTIAL_PROGRESS`]
    /// cardinal steps (`walk_cost`'s "The chain ends"): a router that broke
    /// the argument the chain's end rests on. `partials` is how many partial
    /// routes the walk had chained.
    Stalled {
        /// The partial routes chained before the query that did not fall.
        partials: u32,
    },
}

impl From<NoPath> for WalkError {
    fn from(reason: NoPath) -> WalkError {
        WalkError::NoPath(reason)
    }
}

/// The new columns a partial route adds to a walk: all of its
/// [`MAX_ROUTE_NODES`] but the one it starts on.
pub const PARTIAL_PROGRESS: u32 = MAX_ROUTE_NODES - 1;
const _: () = assert!(PARTIAL_PROGRESS > 0, "a partial route makes progress");

/// The most partial routes one walk chains on `surface` ([`walk_cost`]'s "The
/// chain ends"): its largest path cost, spent at least [`PARTIAL_PROGRESS`]
/// cardinal steps a partial route.
#[must_use]
pub fn max_partials(surface: &Surface) -> u64 {
    let costs = surface.costs();
    let fall = u64::from(PARTIAL_PROGRESS) * u64::from(costs.cheapest().get().unsigned_abs());
    let most = u64::from(surface.node_count()) * u64::from(costs.max_step().unsigned_abs());
    // `fall` is a positive count of cost units: `PARTIAL_PROGRESS` is
    // positive (asserted above) and the cheapest step is never zero by type.
    most.div_euclid(fall)
}

/// The fall check guards this loop's end: pricing chains every partial route
/// to the goal in one call, so it must know the chain ends. The walker runs
/// no such loop: the router serves it one partial route, it walks it and asks
/// again, bounded by play itself, so it carries no falling cost and no
/// per-unit state guards it. A `Stalled` chain is therefore the pricing's
/// outcome alone, counted where the ranker meets it (`Scratch::stalled`) and
/// held at zero by `tests/nearest.rs`'s oracle.
///
/// [`walk_cost`]'s chain over `route` (the router, or a test's double), with
/// the served buffer the caller took out of the scratch. The loop carries the
/// last query's abstract cost and refuses a query that does not fall below it
/// by [`PARTIAL_PROGRESS`] × `cheapest`, so it cannot run past that cost's
/// fall: an `i32` cost, at least `PARTIAL_PROGRESS` units a partial route.
pub(crate) fn walk_chain<R>(
    (start, goal): (Node, Node),
    cheapest: core::num::NonZeroI32,
    served: &mut Vec<Node>,
    mut route: R,
) -> Result<Walked, WalkError>
where
    R: FnMut((Node, Node), &mut Vec<Node>) -> Result<(AbstractResult, RouteOutcome), NoPath>,
{
    // At most `PARTIAL_PROGRESS` (a `u32`) times an `i32`: inside an `i64`.
    let fall = i64::from(PARTIAL_PROGRESS) * i64::from(cheapest.get());
    let (first, mut outcome) = route((start, goal), served)?;
    let legs = first.legs;
    let mut left = i64::from(first.cost);
    let mut total = i64::from(outcome.cost);
    let mut partials: u32 = 0;
    while let Some(end) = outcome.rest {
        let (next, then) = route((end, goal), served)?;
        let now = i64::from(next.cost);
        // The cost carried falls by at least `fall` a turn and starts at no
        // more than `i32::MAX`, so this loop turns fewer than `i32::MAX /
        // PARTIAL_PROGRESS` times, a `u32` count; the walk's summed cost,
        // that many routes of at most `i32::MAX` each, fits an `i64`.
        if now < 0 || now > left.saturating_sub(fall) {
            return Err(WalkError::Stalled { partials });
        }
        partials += 1;
        left = now;
        total += i64::from(then.cost);
        outcome = then;
    }
    Ok(Walked {
        cost: total,
        legs,
        partials,
    })
}

#[cfg(test)]
mod walk_tests {
    use super::{AbstractResult, RouteOutcome, WalkError, walk_chain};
    use crate::pathing::route::NoPath;

    /// A cardinal step of ten units, the committed table's.
    const TEN: core::num::NonZeroI32 = match core::num::NonZeroI32::new(10) {
        Some(ten) => ten,
        None => core::num::NonZeroI32::MIN,
    };

    fn answer(abstract_cost: i32, cost: i32, rest: Option<u32>) -> (AbstractResult, RouteOutcome) {
        (
            AbstractResult {
                cost: abstract_cost,
                legs: 1,
                fogged: false,
            },
            RouteOutcome {
                cost,
                legs: 1,
                rest,
            },
        )
    }

    /// A router double that serves partial routes whose next query never
    /// falls: the chain refuses it as stalled, after the partials it had, and
    /// never as a missing route.
    #[test]
    fn a_router_that_does_not_progress_is_stalled_not_unreachable() {
        let mut served = Vec::new();
        let mut asked = 0_u32;
        let answer_of = walk_chain((0, 9), TEN, &mut served, |_, _| {
            asked += 1;
            Ok(answer(50_000, 10_230, Some(5)))
        });
        assert_eq!(answer_of, Err(WalkError::Stalled { partials: 0 }));
        assert_eq!(asked, 2, "the first query, then the one that did not fall");
        // One that falls once, then stops falling.
        let mut costs = [60_000, 49_000, 48_999].into_iter();
        let falls_once = walk_chain((0, 9), TEN, &mut served, |_, _| {
            Ok(answer(costs.next().unwrap_or(0), 10_230, Some(5)))
        });
        assert_eq!(falls_once, Err(WalkError::Stalled { partials: 1 }));
    }

    /// The fall the chain holds a query to is `PARTIAL_PROGRESS` steps of the
    /// cheapest step's cost, not `PARTIAL_PROGRESS` units: a query that falls
    /// by more than the steps but less than their cost is stalled.
    #[test]
    fn the_fall_is_counted_in_cardinal_steps() {
        let mut served = Vec::new();
        let mut costs = [60_000, 60_000 - 5_000].into_iter();
        let short = walk_chain((0, 9), TEN, &mut served, |_, _| {
            Ok(answer(costs.next().unwrap_or(0), 10_230, Some(5)))
        });
        assert_eq!(short, Err(WalkError::Stalled { partials: 0 }));
    }

    /// A router double whose partial routes fall as the argument says: the
    /// chain sums every route and ends at the whole one.
    #[test]
    fn a_chain_that_falls_sums_its_routes() {
        let mut served = Vec::new();
        let mut turns = [
            (30_000, 10_230, Some(5)),
            (19_770, 10_230, Some(6)),
            (9_540, 9_540, None),
        ]
        .into_iter();
        let walked = walk_chain((0, 9), TEN, &mut served, |_, _| {
            let (a, c, rest) = turns.next().unwrap_or((0, 0, None));
            Ok(answer(a, c, rest))
        });
        assert_eq!(
            walked,
            Ok(super::Walked {
                cost: 30_000,
                legs: 1,
                partials: 2,
            }),
            "the three routes' costs, the first query's abstract edges and the two partials"
        );
        let none = walk_chain((0, 9), TEN, &mut served, |_, _| Err(NoPath::Unreachable));
        assert_eq!(none, Err(WalkError::NoPath(NoPath::Unreachable)));
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
    /// The most ticks a re-ask at the end of a partial route can cost the
    /// walker ([`crate::rules::RulesTable::link_ticks`]), which
    /// [`walk_ticks`] adds per partial route.
    pub link_ticks: i32,
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
            link_ticks: rules.link_ticks(),
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
            link_ticks: rules.link_ticks(),
        }
    }
}
