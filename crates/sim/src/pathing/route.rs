// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The abstract search, the per-leg refinement and the smoother.
//!
//! # Temporary endpoints, and their clean-up
//!
//! A query inserts its two endpoints as abstract nodes numbered
//! `capacity` and `capacity + 1`, held in the [`Scratch`] rather than in the
//! [`Clusters`]. "Cleaning up the temporary nodes" is therefore the *absence*
//! of a mutation: the abstract graph is never touched by a query, so two
//! queries cannot interfere and the graph a repair produced is the graph every
//! later query sees.
//!
//! Insertion is one **bounded sweep per endpoint** over the endpoint's own
//! cluster: at most one cluster of cells, and it yields the cost from the
//! endpoint to *every* transition node of that cluster at once. When both
//! endpoints share a cluster it yields the direct cost between them for free as
//! well — which matters, because without that edge a short query inside one
//! cluster would be forced out through an entrance and back, and the estimate
//! would be wildly pessimistic exactly where the editor shows short routes.
//!
//! # One abstract level
//!
//! The search reaches the graph only through the per-cluster intra rows and the
//! border edges; it asks the decomposition nothing about geometry. A second
//! level would be a second [`Clusters`] over a coarser grid and a two-stage
//! search, with nothing here changing shape — and item 58 bounded what it could
//! save at 9 %, because the endpoint sweeps it cannot touch are 91 % of a
//! median query. `tests/pathing.rs` reports that split so S3 inherits the
//! number rather than the opinion.
//!
//! # Smoothing can only remove cost
//!
//! A shortcut is accepted only when a greedy octile walk between the two nodes
//! is legal at **every** step and costs no more than the sub-path it replaces.
//! So the smoother can neither introduce an illegal step nor make a route
//! worse; the only thing it can do is fail to find a shortcut. That is what
//! makes the estimate never optimistic (item 61): the estimate prices the
//! unsmoothed refinement, and the walker walks the smoothed one.

use crate::pathing::clusters::Clusters;
use crate::pathing::estimate::{Fog, Speed, fog_price};
use crate::pathing::search::{Bound, Item, Scratch, astar_costed, dijkstra_bounded};
use crate::pathing::surface::{Node, Surface};
use crate::pathing::{MAX_ROUTE_NODES, SMOOTH_LOOKAHEAD};

/// One stretch of ground a route pays a surcharge to cross (S2's `atk`): a
/// known threat's weapon range, or an Attack beacon's avoid area.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    /// The columns within `limit` of `centre` on the map's plane: a threat's
    /// reach (decision 12's `AVOID_KNOWN_THREATS`).
    Disc {
        /// The threat's column, `z` zero.
        centre: [crate::math::fixed::Fx; 3],
        /// Its weapon's range, squared.
        limit: crate::math::fixed::Sq,
    },
    /// The columns of a box: an avoid area.
    Box(crate::tables::ColumnBox),
}

impl Zone {
    /// Whether the column `node` stands on lies inside the zone.
    #[must_use]
    pub fn holds(self, surface: &Surface, node: Node) -> bool {
        let (x, y) = surface.coord_of(node);
        match self {
            Zone::Disc { centre, limit } => {
                let (Ok(x), Ok(y)) = (i16::try_from(x), i16::try_from(y)) else {
                    return false;
                };
                let at = [
                    crate::math::fixed::Fx::from_voxels(x),
                    crate::math::fixed::Fx::from_voxels(y),
                    crate::math::fixed::Fx::from_raw(0),
                ];
                crate::math::fixed::Sq::between(at, centre) <= limit
            }
            Zone::Box(area) => area.contains(x, y),
        }
    }
}

/// The route surcharge one walker pays (S2's `atk`; register S2-03): every
/// step onto a column inside one of `zones` costs `multiplier` times its
/// ordinary cost, so the route goes round them when a way round is cheaper.
/// [`Surcharge::NONE`] charges nothing, and a route under it is exactly the
/// route [`route`] has always walked.
#[derive(Clone, Copy, Debug)]
pub struct Surcharge<'a> {
    zones: &'a [Zone],
    multiplier: i32,
}

impl<'a> Surcharge<'a> {
    /// No surcharge.
    pub const NONE: Surcharge<'static> = Surcharge {
        zones: &[],
        multiplier: 1,
    };

    /// A surcharge of `multiplier` inside `zones`.
    #[must_use]
    pub const fn new(zones: &'a [Zone], multiplier: i32) -> Surcharge<'a> {
        Surcharge { zones, multiplier }
    }

    /// Whether it charges nothing.
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.zones.is_empty() || self.multiplier <= 1
    }

    /// Whether the column `node` stands on is surcharged.
    #[must_use]
    pub fn holds(self, surface: &Surface, node: Node) -> bool {
        !self.is_none() && self.zones.iter().any(|zone| zone.holds(surface, node))
    }

    /// What a step onto `to` costs that would cost `cost` ordinarily.
    #[must_use]
    pub fn step(self, surface: &Surface, to: Node, cost: i32) -> i32 {
        if self.holds(surface, to) {
            cost.saturating_mul(self.multiplier)
        } else {
            cost
        }
    }
}

/// What an abstract search found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AbstractResult {
    /// The summed abstract cost, in cost units.
    pub cost: i32,
    /// Abstract edges crossed.
    pub legs: u32,
    /// Whether any edge was priced through fog.
    pub fogged: bool,
}

/// What a refined route came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RouteOutcome {
    /// The cost of the route actually written out, after smoothing.
    pub cost: i32,
    /// Abstract edges the route was assembled from.
    pub legs: u32,
    /// Whether the route stops short of the goal because it reached
    /// [`MAX_ROUTE_NODES`]. The walker asks for the rest when it gets there.
    pub partial: bool,
}

/// Search the abstract graph from `start` to `goal`.
///
/// **This is the whole of the estimate query**: no refinement, no stepping, no
/// fork. [`crate::pathing::estimate::estimate`] is a contractual wrapper around
/// it.
pub fn abstract_search(
    surface: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    start: Node,
    goal: Node,
    fog: Fog<'_>,
    speed: Speed,
) -> Option<AbstractResult> {
    abstract_search_with(
        surface,
        clusters,
        scratch,
        (start, goal),
        fog,
        speed,
        Surcharge::NONE,
    )
}

/// [`abstract_search`] under a route surcharge (S2's `atk`): an abstract
/// edge either end of which is surcharged costs the multiplier times its
/// price.
fn abstract_search_with(
    surface: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    (start, goal): (Node, Node),
    fog: Fog<'_>,
    speed: Speed,
    surcharge: Surcharge<'_>,
) -> Option<AbstractResult> {
    scratch.abstract_path.clear();
    if !surface.walkable(start) || !surface.walkable(goal) {
        return None;
    }
    if start == goal {
        scratch.abstract_path.push(0);
        return Some(AbstractResult {
            cost: 0,
            legs: 0,
            fogged: false,
        });
    }
    // The cheap no-path answer, before any search: two array reads (item 60).
    if !clusters.connected(surface, start, goal) {
        return None;
    }

    let capacity = clusters.abstract_capacity();
    let temp_start = capacity;
    let temp_goal = capacity.saturating_add(1);

    let mut start_edges = core::mem::take(&mut scratch.start_edges);
    let mut goal_edges = core::mem::take(&mut scratch.goal_edges);
    let direct = insert_endpoint(surface, clusters, scratch, start, goal, &mut start_edges);
    insert_endpoint(surface, clusters, scratch, goal, start, &mut goal_edges);
    if let Some(cost) = direct {
        start_edges.push((temp_goal, cost));
    }
    // The key IS the abstract node id, unique within one endpoint's cluster, so
    // the order is total (item 62).
    start_edges.sort_unstable();
    goal_edges.sort_unstable();
    scratch.start_edges = start_edges;
    scratch.goal_edges = goal_edges;

    let query = Query {
        temp_start,
        temp_goal,
        start,
        goal,
        fog,
        speed,
        surcharge,
    };

    abstract_begin(scratch);
    scratch.abstract_expansions = 0;
    let h0 = surface.heuristic(start, goal);
    abstract_reach(scratch, temp_start, 0, temp_start);
    scratch.abstract_heap.push(Item {
        f: h0,
        h: h0,
        n: temp_start,
    });

    let mut found = false;
    while let Some(item) = scratch.abstract_heap.pop() {
        if abstract_is_closed(scratch, item.n) {
            continue;
        }
        abstract_close(scratch, item.n);
        scratch.abstract_expansions = scratch.abstract_expansions.saturating_add(1);
        if item.n == temp_goal {
            found = true;
            break;
        }
        expand(surface, clusters, scratch, &query, item.n);
    }

    if !found {
        return None;
    }
    Some(reconstruct(clusters, scratch, &query, capacity))
}

/// The six facts a query carries through the expansion: its two temporary
/// endpoints, the two columns they stand on, what the asking seat cannot see
/// and how the fog is priced.
#[derive(Clone, Copy, Debug)]
struct Query<'a> {
    temp_start: u32,
    temp_goal: u32,
    start: Node,
    goal: Node,
    fog: Fog<'a>,
    speed: Speed,
    surcharge: Surcharge<'a>,
}

impl Query<'_> {
    /// The column an abstract node stands on, temporary endpoints included.
    fn cell_of(self, clusters: &Clusters, node: u32) -> Node {
        if node == self.temp_start {
            self.start
        } else if node == self.temp_goal {
            self.goal
        } else {
            clusters.abstract_cell(node).unwrap_or(self.start)
        }
    }
}

/// Relax every edge out of one abstract node.
///
/// Three kinds, and no fourth: the temporary start's own insertion edges; a
/// base node's intra row and border edges; and, for a node the goal's
/// insertion reached, the edge to the temporary goal.
fn expand(
    surface: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    query: &Query<'_>,
    node: u32,
) {
    let here = abstract_cost(scratch, node);
    let from_cell = query.cell_of(clusters, node);

    if node == query.temp_start {
        let mut index = 0;
        while index < scratch.start_edges.len() {
            let Some((target, cost)) = scratch.start_edges.get(index).copied() else {
                break;
            };
            index = index.saturating_add(1);
            let edge = Edge {
                from: node,
                target,
                here,
                cost,
                from_cell,
            };
            relax_edge(surface, clusters, scratch, query, edge);
        }
        return;
    }

    let (cluster, slot) = clusters.split_abstract(node);
    let stride = clusters.max_trans();
    let (intra_to, intra_cost) = clusters.intra_edges(cluster, slot);
    let mut index = 0;
    while index < intra_to.len() {
        let (Some(target_slot), Some(cost)) =
            (intra_to.get(index).copied(), intra_cost.get(index).copied())
        else {
            break;
        };
        index = index.saturating_add(1);
        let Ok(target) = u32::try_from(
            cluster
                .saturating_mul(stride)
                .saturating_add(usize::from(target_slot)),
        ) else {
            continue;
        };
        let edge = Edge {
            from: node,
            target,
            here,
            cost,
            from_cell,
        };
        relax_edge(surface, clusters, scratch, query, edge);
    }

    let (inter_to, inter_cost) = clusters.inter_edges(cluster, slot);
    let mut index = 0;
    while index < inter_to.len() {
        let (Some(target), Some(cost)) =
            (inter_to.get(index).copied(), inter_cost.get(index).copied())
        else {
            break;
        };
        index = index.saturating_add(1);
        let edge = Edge {
            from: node,
            target,
            here,
            cost,
            from_cell,
        };
        relax_edge(surface, clusters, scratch, query, edge);
    }

    // The key IS the abstract node id, unique in the goal's own edge list, so
    // the order is total (item 62).
    if let Ok(at) = scratch
        .goal_edges
        .binary_search_by_key(&node, |(edge, _)| *edge)
        && let Some((_, cost)) = scratch.goal_edges.get(at).copied()
    {
        let edge = Edge {
            from: node,
            target: query.temp_goal,
            here,
            cost,
            from_cell,
        };
        relax_edge(surface, clusters, scratch, query, edge);
    }
}

/// One edge, as the expansion sees it.
#[derive(Clone, Copy, Debug)]
struct Edge {
    from: u32,
    target: u32,
    here: i32,
    cost: i32,
    from_cell: Node,
}

/// Price one edge and offer it to the open set.
fn relax_edge(
    surface: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    query: &Query<'_>,
    edge: Edge,
) {
    let to_cell = query.cell_of(clusters, edge.target);
    let fogged = price(edge.cost, edge.from_cell, to_cell, query.fog, query.speed);
    let priced = if query.surcharge.holds(surface, edge.from_cell)
        || query.surcharge.holds(surface, to_cell)
    {
        fogged.saturating_mul(query.surcharge.multiplier)
    } else {
        fogged
    };
    abstract_relax(
        surface,
        scratch,
        edge.target,
        edge.here.saturating_add(priced),
        edge.from,
        query.goal,
        to_cell,
    );
}

/// Walk the predecessor array back to the temporary start and read off the
/// three numbers item 61 asks for.
fn reconstruct(
    clusters: &Clusters,
    scratch: &mut Scratch,
    query: &Query<'_>,
    capacity: u32,
) -> AbstractResult {
    let cost = abstract_cost(scratch, query.temp_goal);
    let mut path = core::mem::take(&mut scratch.abstract_path);
    path.clear();
    let mut current = query.temp_goal;
    let mut guard: u32 = 0;
    loop {
        path.push(current);
        if current == query.temp_start || guard > capacity {
            break;
        }
        current = abstract_came(scratch, current);
        guard = guard.saturating_add(1);
    }
    path.reverse();
    // E7: cannot miss: an abstract path holds at most the graph's nodes, a `u32` count
    let legs = u32::try_from(path.len().saturating_sub(1)).unwrap_or(0);
    let mut fogged = false;
    for node in &path {
        if query.fog.is_unknown(query.cell_of(clusters, *node)) {
            fogged = true;
        }
    }
    scratch.abstract_path = path;
    AbstractResult { cost, legs, fogged }
}

/// Why [`route`] found no route: a typed `no_path`, each reason counted by the
/// router rather than a silent stop (S2's plan, E27).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NoPath {
    /// The walker's column or its goal's is not a node of the search graph:
    /// off the map, or no ground to stand on.
    Unplaced,
    /// The abstract search, bounded by the graph, reached every node it could
    /// and never the goal: the walker is sealed in (item 60).
    Unreachable,
    /// An abstract leg found no walk inside its cluster.
    Unrefinable,
    /// The smoothed walk could not be priced.
    Unpriced,
}

impl NoPath {
    /// Every reason.
    pub const ALL: [NoPath; 4] = [
        NoPath::Unplaced,
        NoPath::Unreachable,
        NoPath::Unrefinable,
        NoPath::Unpriced,
    ];

    /// The reason's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            NoPath::Unplaced => "unplaced",
            NoPath::Unreachable => "unreachable",
            NoPath::Unrefinable => "unrefinable",
            NoPath::Unpriced => "unpriced",
        }
    }
}

/// A full route: abstract search, per-leg refinement, then smoothing.
///
/// Writes the column sequence into `out`, which the caller owns and which never
/// grows past [`MAX_ROUTE_NODES`].
///
/// # Errors
///
/// A typed [`NoPath`], which the router counts under its reason (S2's plan,
/// E27).
pub fn route(
    surface: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    start: Node,
    goal: Node,
    out: &mut Vec<Node>,
) -> Result<RouteOutcome, NoPath> {
    route_with(
        surface,
        clusters,
        scratch,
        (start, goal),
        Surcharge::NONE,
        out,
    )
}

/// [`route`] under a route surcharge (S2's `atk`; register S2-03): the
/// abstract search, each leg's refinement and the smoother all price a step
/// onto a surcharged column at the multiplier, so the walk goes round the
/// zones where a way round is cheaper. Under [`Surcharge::NONE`] it is
/// [`route`].
///
/// # Errors
///
/// As [`route`].
#[allow(
    clippy::too_many_lines,
    reason = "route's body under a surcharge: the abstract search, the leg refinement and the smoother in the order they run"
)]
pub fn route_with(
    surface: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    (start, goal): (Node, Node),
    surcharge: Surcharge<'_>,
    out: &mut Vec<Node>,
) -> Result<RouteOutcome, NoPath> {
    out.clear();
    // A route is walked over terrain the sim can see in full, so nothing on it
    // is priced through fog and the multiplier is never reached. Fog belongs to
    // the *estimate*, which is a seat's question about a route it cannot see.
    let speed = Speed {
        cost_per_second: 1,
        fog_numerator: 1,
        fog_denominator: 1,
    };
    let result = abstract_search_with(
        surface,
        clusters,
        scratch,
        (start, goal),
        Fog::Clear,
        speed,
        surcharge,
    )
    .ok_or(NoPath::Unreachable)?;
    if result.legs == 0 {
        out.push(start);
        return Ok(RouteOutcome {
            cost: 0,
            legs: 0,
            partial: false,
        });
    }

    let capacity = clusters.abstract_capacity();
    let temp_start = capacity;
    let temp_goal = capacity.saturating_add(1);
    let limit = crate::math::index::row(MAX_ROUTE_NODES);

    let path = core::mem::take(&mut scratch.abstract_path);
    let mut raw = core::mem::take(&mut scratch.raw);
    let mut leg = core::mem::take(&mut scratch.leg);
    raw.clear();
    let mut partial = false;
    let mut ok = true;
    if let Some(first) = path.first().copied() {
        raw.push(abstract_to_cell(
            clusters, first, temp_start, temp_goal, start, goal,
        ));
    }
    for pair in path.windows(2) {
        let (Some(from), Some(to)) = (pair.first().copied(), pair.get(1).copied()) else {
            ok = false;
            break;
        };
        let a = abstract_to_cell(clusters, from, temp_start, temp_goal, start, goal);
        let b = abstract_to_cell(clusters, to, temp_start, temp_goal, start, goal);
        let ca = clusters.cluster_of(a);
        let cb = clusters.cluster_of(b);
        if ca == cb {
            if astar_costed(
                surface,
                Bound::Cluster(clusters.of_node(), ca),
                scratch,
                (a, b),
                surcharge,
                &mut leg,
            )
            .is_none()
            {
                ok = false;
                break;
            }
            for node in leg.iter().skip(1) {
                if raw.len() >= limit {
                    partial = true;
                    break;
                }
                raw.push(*node);
            }
        } else {
            // An inter-cluster abstract edge is exactly one grid step.
            if surface.step_cost_between(a, b).is_none() {
                ok = false;
                break;
            }
            if raw.len() >= limit {
                partial = true;
            } else {
                raw.push(b);
            }
        }
        if partial {
            break;
        }
    }
    scratch.abstract_path = path;
    scratch.leg = leg;
    if !ok {
        scratch.raw = raw;
        return Err(NoPath::Unrefinable);
    }

    let mut line = core::mem::take(&mut scratch.line);
    let mut prefix = core::mem::take(&mut scratch.prefix);
    smooth_costed(surface, &raw, out, &mut line, &mut prefix, surcharge);
    scratch.raw = raw;
    scratch.line = line;
    scratch.prefix = prefix;
    let cost = crate::pathing::search::route_cost(surface, out).ok_or(NoPath::Unpriced)?;
    Ok(RouteOutcome {
        cost,
        legs: result.legs,
        partial,
    })
}

/// Insert one endpoint: a bounded sweep over its cluster, harvested as edges to
/// that cluster's transition nodes. Returns the direct cost to `other` when the
/// two share a cluster.
fn insert_endpoint(
    surface: &Surface,
    clusters: &Clusters,
    scratch: &mut Scratch,
    cell: Node,
    other: Node,
    out: &mut Vec<(u32, i32)>,
) -> Option<i32> {
    out.clear();
    let tag = clusters.cluster_of(cell);
    dijkstra_bounded(
        surface,
        Bound::Cluster(clusters.of_node(), tag),
        scratch,
        cell,
        true,
    );
    let cluster = usize::from(tag);
    let stride = clusters.max_trans();
    let cells = clusters.trans_cells(cluster);
    let mut slot = 0;
    while slot < cells.len() {
        if let Some(target_cell) = cells.get(slot).copied()
            && let Some(cost) = scratch.cost_of(target_cell)
            && let Ok(target) = u32::try_from(cluster.saturating_mul(stride).saturating_add(slot))
        {
            out.push((target, cost));
        }
        slot = slot.saturating_add(1);
    }
    if clusters.cluster_of(other) == tag {
        scratch.cost_of(other)
    } else {
        None
    }
}

/// The column an abstract node stands on, temporary endpoints included.
fn abstract_to_cell(
    clusters: &Clusters,
    node: u32,
    temp_start: u32,
    temp_goal: u32,
    start: Node,
    goal: Node,
) -> Node {
    if node == temp_start {
        start
    } else if node == temp_goal {
        goal
    } else {
        clusters.abstract_cell(node).unwrap_or(start)
    }
}

/// The fog rule, per abstract edge (item 61).
fn price(cost: i32, from: Node, to: Node, fog: Fog<'_>, speed: Speed) -> i32 {
    if fog.is_unknown(from) || fog.is_unknown(to) {
        fog_price(cost, speed.fog_numerator, speed.fog_denominator)
    } else {
        cost
    }
}

fn abstract_begin(scratch: &mut Scratch) {
    if scratch.abstract_epoch == u32::MAX {
        scratch.abstract_stamp.fill(0);
        scratch.abstract_closed.fill(0);
        scratch.abstract_epoch = 0;
    }
    scratch.abstract_epoch = scratch.abstract_epoch.saturating_add(1);
    scratch.abstract_heap.clear();
}

fn abstract_is_closed(scratch: &Scratch, node: u32) -> bool {
    usize::try_from(node)
        .ok()
        .and_then(|index| scratch.abstract_closed.get(index).copied())
        .is_some_and(|stamp| stamp == scratch.abstract_epoch)
}

fn abstract_close(scratch: &mut Scratch, node: u32) {
    let epoch = scratch.abstract_epoch;
    if let Ok(index) = usize::try_from(node)
        && let Some(slot) = scratch.abstract_closed.get_mut(index)
    {
        *slot = epoch;
    }
}

fn abstract_seen(scratch: &Scratch, node: u32) -> bool {
    usize::try_from(node)
        .ok()
        .and_then(|index| scratch.abstract_stamp.get(index).copied())
        .is_some_and(|stamp| stamp == scratch.abstract_epoch)
}

fn abstract_cost(scratch: &Scratch, node: u32) -> i32 {
    usize::try_from(node)
        .ok()
        .and_then(|index| scratch.abstract_g.get(index).copied())
        // E7: rule: an abstract node the query never reached costs nothing yet
        .unwrap_or(0)
}

fn abstract_came(scratch: &Scratch, node: u32) -> u32 {
    usize::try_from(node)
        .ok()
        .and_then(|index| scratch.abstract_came.get(index).copied())
        // E7: rule: a node the query never reached is its own predecessor, which ends the walk back
        .unwrap_or(node)
}

fn abstract_reach(scratch: &mut Scratch, node: u32, cost: i32, from: u32) {
    let epoch = scratch.abstract_epoch;
    let Ok(index) = usize::try_from(node) else {
        return;
    };
    if let Some(slot) = scratch.abstract_stamp.get_mut(index) {
        *slot = epoch;
    }
    if let Some(slot) = scratch.abstract_g.get_mut(index) {
        *slot = cost;
    }
    if let Some(slot) = scratch.abstract_came.get_mut(index) {
        *slot = from;
    }
}

fn abstract_relax(
    surface: &Surface,
    scratch: &mut Scratch,
    target: u32,
    cost: i32,
    from: u32,
    goal: Node,
    target_cell: Node,
) {
    if abstract_is_closed(scratch, target) {
        return;
    }
    if abstract_seen(scratch, target) && abstract_cost(scratch, target) <= cost {
        return;
    }
    abstract_reach(scratch, target, cost, from);
    let h = surface.heuristic(target_cell, goal);
    scratch.abstract_heap.push(Item {
        f: cost.saturating_add(h),
        h,
        n: target,
    });
}

/// The greedy octile walk from `from` to `to`: diagonal while both axes need
/// covering, then straight. Validated step by step, so it can only ever return
/// a legal walk.
fn straight_walk(
    surface: &Surface,
    from: Node,
    to: Node,
    out: &mut Vec<Node>,
    surcharge: Surcharge<'_>,
) -> Option<i32> {
    out.clear();
    out.push(from);
    let (tx, ty) = surface.coord_of(to);
    let mut current = from;
    let mut total: i32 = 0;
    let mut guard: u32 = 0;
    loop {
        let (x, y) = surface.coord_of(current);
        if x == tx && y == ty {
            return Some(total);
        }
        if guard > MAX_ROUTE_NODES {
            return None;
        }
        guard = guard.saturating_add(1);
        let next = surface.node_of(
            x.saturating_add(tx.saturating_sub(x).signum()),
            y.saturating_add(ty.saturating_sub(y).signum()),
        )?;
        let step = surcharge.step(surface, next, surface.step_cost_between(current, next)?);
        total = total.checked_add(step)?;
        out.push(next);
        current = next;
    }
}

/// Smooth `path` into `out`: legal by construction, and never more expensive
/// than the input.
///
/// An input that is not itself a legal walk is **refused** rather than
/// smoothed — `out` is left empty — because the alternative is pricing an
/// illegal step and summing it into the prefix array, which with overflow
/// checks on in every profile is a panic inside a tick.
pub fn smooth_into(
    surface: &Surface,
    path: &[Node],
    out: &mut Vec<Node>,
    line: &mut Vec<Node>,
    prefix: &mut Vec<i32>,
) {
    smooth_costed(surface, path, out, line, prefix, Surcharge::NONE);
}

/// [`smooth_into`] under a route surcharge: a shortcut is priced, and the
/// sub-path it replaces, at the surcharge, so smoothing never cuts back into
/// a zone the refinement went round.
fn smooth_costed(
    surface: &Surface,
    path: &[Node],
    out: &mut Vec<Node>,
    line: &mut Vec<Node>,
    prefix: &mut Vec<i32>,
    surcharge: Surcharge<'_>,
) {
    out.clear();
    let Some(first) = path.first().copied() else {
        return;
    };
    prefix.clear();
    prefix.push(0);
    let mut total: i32 = 0;
    for pair in path.windows(2) {
        let (Some(from), Some(to)) = (pair.first().copied(), pair.get(1).copied()) else {
            out.clear();
            return;
        };
        let Some(step) = surface.step_cost_between(from, to) else {
            out.clear();
            return;
        };
        total = total.saturating_add(surcharge.step(surface, to, step));
        prefix.push(total);
    }
    out.push(first);
    let mut index = 0_usize;
    while index.saturating_add(1) < path.len() {
        let limit = index
            .saturating_add(SMOOTH_LOOKAHEAD)
            .min(path.len().saturating_sub(1));
        let mut chosen = index.saturating_add(1);
        let mut ahead = limit;
        while ahead > index.saturating_add(1) {
            let (Some(from), Some(to)) = (path.get(index).copied(), path.get(ahead).copied())
            else {
                break;
            };
            let budget = prefix
                .get(ahead)
                .copied()
                // E7: cannot miss: `ahead` and `index` are inside the prefix array the walk built
                .unwrap_or(0)
                // E7: cannot miss: as above
                .saturating_sub(prefix.get(index).copied().unwrap_or(0));
            if let Some(cost) = straight_walk(surface, from, to, line, surcharge)
                && cost <= budget
            {
                for node in line.iter().skip(1) {
                    out.push(*node);
                }
                chosen = ahead;
                break;
            }
            ahead = ahead.saturating_sub(1);
        }
        if chosen == index.saturating_add(1)
            && let Some(node) = path.get(index.saturating_add(1)).copied()
        {
            out.push(node);
        }
        index = chosen;
    }
}
