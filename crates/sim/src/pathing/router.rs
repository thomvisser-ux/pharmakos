// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-unit routes, the speed accumulator, the repath cap and "sealed in".
//!
//! # The cap, and the order it is served in
//!
//! Peak demand in the spike was 1 474 repaths a game-second against 1 235/s of
//! single-thread throughput, so the per-tick cap is a bound on work rather than
//! a safety net (item 60). Its value is
//! `locomotion.repath_cap_per_tick` = **16** (item 69): sustainable — the
//! backlog does not grow over a full segment and every request is eventually
//! served — where 8 drops 14 % of the work for ever and 32 misses the tick
//! deadline.
//!
//! Which requests a busy tick defers is a function of the state and nothing
//! else: the queue is served **round-robin in `(seat, beacon, unit)` order**
//! from a cursor that is itself hashed state. The queue is not a container —
//! it is a `waiting` flag per unit plus that cursor, scanned in table order.
//! Units are stored seat-major with dense ids, so table order *is*
//! `(seat, unit)`; the beacon key is the middle of the three and is
//! constant today.
//!
//! PLACEHOLDER: the beacon half of the key is [`crate::tables::BeaconId::NONE`]
//! for every unit, because nothing assigns a unit to a beacon yet. T11's
//! interpreter and T14's mandates are what give it a value, and the order below
//! becomes `(seat, beacon, unit)` in full then without moving this code —
//! owner, at T11.
//!
//! # "Sealed in" is a designed state
//!
//! One walker in the spike was buried by its own destruction, and "no path"
//! fired on 4.5 % of repaths. So a unit whose route the oracle refuses **parks
//! and reports** — [`WalkState::Sealed`] — rather than asking again every tick.
//! It tries again exactly when the graph that refused it has changed: on a tick
//! whose repair rebuilt the components, the oracle is asked again about every
//! sealed unit, and one it now calls connected to its destination is re-armed.
//! The question is the same two array reads that refused the route in the first
//! place (item 60), and asking it of the whole graph rather than of the
//! repaired clusters alone is what stops a unit whose moat was opened two
//! clusters away from parking for the rest of the match. That is the difference
//! between a state and a repath loop.
//!
//! # The speed accumulator (item 90)
//!
//! Each tick a walker adds its `cost_per_second` to an integer accumulator and
//! spends one cost unit per 20 accumulated — 20 being the tick rate — so
//! nothing rounds a speed down. It arrives on tick
//! `ceil(cost * 20 / cost_per_second)` exactly, which is the same number
//! [`crate::pathing::estimate::ticks_for_cost`] computes in one division: that
//! equality is why the estimator is allowed to divide instead of walk, and it
//! is what makes the estimate never optimistic.
//!
//! # What is hashed
//!
//! Everything in this module that a later tick can read: the walk state, the
//! accumulator, the route's length, the cursor along it, whether it is partial,
//! the round-robin cursor — and the route itself as a **digest**, the way the
//! chunk store enters the hash through per-chunk digests (item 66). The route's
//! nodes travel in the snapshot beside the digest, so a restored world walks
//! the route it was walking rather than one that merely hashes the same.

use crate::math::fixed::{Angle, Fx};
use crate::math::quantity::{Hp, TICK_HZ};
use crate::pathing::MAX_ROUTE_NODES;
use crate::pathing::clusters::Clusters;
use crate::pathing::route::{RouteOutcome, route};
use crate::pathing::search::Scratch;
use crate::pathing::surface::{Node, Surface};
use crate::tables::UnitKind;

/// What a unit is doing about walking.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum WalkState {
    /// Standing still with nowhere to be.
    #[default]
    Idle,
    /// Waiting for the repath queue to reach it.
    Waiting,
    /// Following a route.
    Walking,
    /// Parked: the oracle says no route exists. Re-armed when the graph that
    /// refused it is repaired.
    Sealed,
}

impl WalkState {
    /// The wire id. Written out rather than taken from the enum's order, so a
    /// reordering cannot change a hashed byte by accident.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            WalkState::Idle => 0,
            WalkState::Waiting => 1,
            WalkState::Walking => 2,
            WalkState::Sealed => 3,
        }
    }

    /// The state a wire id names, or `None` for one no build has issued.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<WalkState> {
        match id {
            0 => Some(WalkState::Idle),
            1 => Some(WalkState::Waiting),
            2 => Some(WalkState::Walking),
            3 => Some(WalkState::Sealed),
            _ => None,
        }
    }
}

/// What one tick's serving did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ServeReport {
    /// Repaths actually run this tick, at most the cap.
    pub served: u32,
    /// Units still waiting after the cap was spent.
    pub backlog: u32,
    /// Units parked as sealed in this tick.
    pub sealed: u32,
}

/// Every unit's route and walk state.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Router {
    /// Walking speed per unit kind, indexed by [`UnitKind::id`], which runs
    /// `1..=6` — so the array is seven long and slot 0 is the unused id no
    /// kind carries. Sizing it at six would drop the highest kind's row
    /// silently and freeze that kind in place for ever, which is what
    /// `every_unit_kind_walks_at_the_speed_its_rules_row_names` pins.
    speeds: [i32; 7],
    state: Vec<u8>,
    accumulator: Vec<i32>,
    route_len: Vec<u32>,
    route_cursor: Vec<u32>,
    route_hash: Vec<u64>,
    route_partial: Vec<bool>,
    nodes: Vec<Node>,
    cursor: u32,
    /// Route assembly buffer, sized once so a repath allocates nothing.
    buffer: Vec<Node>,
    /// The bytes a route is digested over, sized once for the same reason.
    /// Always empty between repaths, so it is never part of a comparison.
    digest_bytes: Vec<u8>,
    /// Units that reached the end of a route this tick, in unit-index order.
    arrived: Vec<u32>,
    /// Units served this tick, in the order the round robin reached them.
    /// Diagnostic, never hashed: it is what lets a test compare the discipline
    /// against a model of it, and what T10's event bus will carry.
    served_this_tick: Vec<u32>,
    served_total: u64,
    sealed_total: u64,
    backlog_peak: u32,
}

impl Router {
    /// Room for `units` units, at the walking speeds of the rules table.
    #[must_use]
    pub fn new(units: u32, rules: &crate::rules::RulesTable) -> Router {
        let count = usize::try_from(units).unwrap_or(0);
        let stride = usize::try_from(MAX_ROUTE_NODES).unwrap_or(0);
        let mut speeds = [0_i32; 7];
        for kind in UnitKind::ALL {
            if let Some(speed) = rules.cost_per_second(kind)
                && let Some(slot) = speeds.get_mut(usize::from(kind.id()))
            {
                *slot = speed;
            }
        }
        Router {
            speeds,
            state: vec![WalkState::Idle.id(); count],
            accumulator: vec![0; count],
            route_len: vec![0; count],
            route_cursor: vec![0; count],
            route_hash: vec![0; count],
            route_partial: vec![false; count],
            nodes: vec![0; count.saturating_mul(stride)],
            cursor: 0,
            buffer: Vec::with_capacity(stride),
            digest_bytes: Vec::with_capacity(stride.saturating_mul(4)),
            arrived: Vec::with_capacity(count),
            served_this_tick: Vec::with_capacity(count),
            served_total: 0,
            sealed_total: 0,
            backlog_peak: 0,
        }
    }

    /// How many units the router carries.
    #[must_use]
    pub fn len(&self) -> u32 {
        u32::try_from(self.state.len()).unwrap_or(0)
    }

    /// Whether it carries none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// One unit's walk state.
    #[must_use]
    pub fn state(&self, unit: u32) -> WalkState {
        let id = usize::try_from(unit)
            .ok()
            .and_then(|index| self.state.get(index).copied())
            .unwrap_or(0);
        WalkState::from_id(id).unwrap_or_default()
    }

    /// One unit's speed accumulator, in twentieths of a cost unit.
    #[must_use]
    pub fn accumulator(&self, unit: u32) -> i32 {
        usize::try_from(unit)
            .ok()
            .and_then(|index| self.accumulator.get(index).copied())
            .unwrap_or(0)
    }

    /// One unit's route, from its start to its goal.
    #[must_use]
    pub fn route(&self, unit: u32) -> &[Node] {
        let Ok(index) = usize::try_from(unit) else {
            return &[];
        };
        let stride = usize::try_from(MAX_ROUTE_NODES).unwrap_or(0);
        let len = usize::try_from(self.route_len.get(index).copied().unwrap_or(0)).unwrap_or(0);
        let base = index.saturating_mul(stride);
        self.nodes
            .get(base..base.saturating_add(len))
            .unwrap_or(&[])
    }

    /// How far along its route a unit is.
    #[must_use]
    pub fn route_cursor(&self, unit: u32) -> u32 {
        usize::try_from(unit)
            .ok()
            .and_then(|index| self.route_cursor.get(index).copied())
            .unwrap_or(0)
    }

    /// The digest of one unit's route — the form the state hash sees.
    #[must_use]
    pub fn route_hash(&self, unit: u32) -> u64 {
        usize::try_from(unit)
            .ok()
            .and_then(|index| self.route_hash.get(index).copied())
            .unwrap_or(0)
    }

    /// Whether a unit's route stops short of its goal.
    #[must_use]
    pub fn route_partial(&self, unit: u32) -> bool {
        usize::try_from(unit)
            .ok()
            .and_then(|index| self.route_partial.get(index).copied())
            .unwrap_or(false)
    }

    /// The round-robin cursor: where the next tick's serving starts.
    #[must_use]
    pub const fn queue_cursor(&self) -> u32 {
        self.cursor
    }

    /// How many units are waiting for a repath.
    #[must_use]
    pub fn backlog(&self) -> u32 {
        let waiting = WalkState::Waiting.id();
        let mut count: u32 = 0;
        let mut at = 0;
        while at < self.state.len() {
            if self.state.get(at).copied() == Some(waiting) {
                count = count.saturating_add(1);
            }
            at = at.saturating_add(1);
        }
        count
    }

    /// The largest backlog seen since construction. Diagnostic: the number the
    /// cap's sustainability is read off.
    #[must_use]
    pub const fn backlog_peak(&self) -> u32 {
        self.backlog_peak
    }

    /// Repaths served since construction. Diagnostic.
    #[must_use]
    pub const fn served_total(&self) -> u64 {
        self.served_total
    }

    /// Units parked as sealed in since construction. Diagnostic.
    #[must_use]
    pub const fn sealed_total(&self) -> u64 {
        self.sealed_total
    }

    /// The units that reached the end of a route in the last [`Router::advance`],
    /// in unit-index order.
    #[must_use]
    pub fn arrived(&self) -> &[u32] {
        &self.arrived
    }

    /// The units the last [`Router::serve`] reached, in the order the round
    /// robin reached them.
    #[must_use]
    pub fn served_this_tick(&self) -> &[u32] {
        &self.served_this_tick
    }

    /// Ask for a route. Idempotent: a unit already waiting stays waiting.
    pub fn request(&mut self, unit: u32) {
        self.set_state(unit, WalkState::Waiting);
    }

    /// Put a unit back to idle and forget its route.
    pub fn clear(&mut self, unit: u32) {
        self.set_state(unit, WalkState::Idle);
        self.set_route_len(unit, 0, 0, 0, false);
    }

    fn set_state(&mut self, unit: u32, state: WalkState) {
        if let Ok(index) = usize::try_from(unit)
            && let Some(slot) = self.state.get_mut(index)
        {
            *slot = state.id();
        }
    }

    fn set_route_len(&mut self, unit: u32, len: u32, cursor: u32, hash: u64, partial: bool) {
        let Ok(index) = usize::try_from(unit) else {
            return;
        };
        if let Some(slot) = self.route_len.get_mut(index) {
            *slot = len;
        }
        if let Some(slot) = self.route_cursor.get_mut(index) {
            *slot = cursor;
        }
        if let Some(slot) = self.route_hash.get_mut(index) {
            *slot = hash;
        }
        if let Some(slot) = self.route_partial.get_mut(index) {
            *slot = partial;
        }
    }

    /// The walking speed of one unit kind, in cost units per second.
    #[must_use]
    pub fn speed_of(&self, kind: UnitKind) -> i32 {
        self.speeds
            .get(usize::from(kind.id()))
            .copied()
            .unwrap_or(0)
    }

    /// Serve up to `cap` waiting units, round-robin from the cursor.
    ///
    /// Every route is computed against the graph as it stands *now*: the repair
    /// has already run this tick, so no route is planned over terrain that has
    /// moved.
    pub fn serve(
        &mut self,
        surface: &Surface,
        clusters: &Clusters,
        scratch: &mut Scratch,
        positions: &[[Fx; 3]],
        destinations: &[[Fx; 3]],
        cap: u32,
    ) -> ServeReport {
        self.served_this_tick.clear();
        let count = self.len();
        if count == 0 || cap == 0 {
            return ServeReport {
                served: 0,
                backlog: self.backlog(),
                sealed: 0,
            };
        }
        let mut served: u32 = 0;
        let mut sealed: u32 = 0;
        let mut offset: u32 = 0;
        let mut at = self.cursor % count;
        while offset < count && served < cap {
            let unit = at;
            at = at.saturating_add(1) % count;
            offset = offset.saturating_add(1);
            if self.state(unit) != WalkState::Waiting {
                continue;
            }
            let start = node_at(surface, positions, unit);
            let goal = node_at(surface, destinations, unit);
            served = served.saturating_add(1);
            self.served_total = self.served_total.saturating_add(1);
            self.served_this_tick.push(unit);
            let (Some(start), Some(goal)) = (start, goal) else {
                self.set_state(unit, WalkState::Sealed);
                self.set_route_len(unit, 0, 0, 0, false);
                sealed = sealed.saturating_add(1);
                self.sealed_total = self.sealed_total.saturating_add(1);
                continue;
            };
            let mut buffer = core::mem::take(&mut self.buffer);
            let outcome = route(surface, clusters, scratch, start, goal, &mut buffer);
            match outcome {
                Some(RouteOutcome { partial, .. }) if buffer.len() > 1 => {
                    self.write_route(unit, &buffer, partial);
                    self.set_state(unit, WalkState::Walking);
                }
                Some(_) => {
                    // Already standing on the goal: nothing to walk.
                    self.set_route_len(unit, 0, 0, 0, false);
                    self.set_state(unit, WalkState::Idle);
                    self.arrived.push(unit);
                }
                None => {
                    self.set_route_len(unit, 0, 0, 0, false);
                    self.set_state(unit, WalkState::Sealed);
                    sealed = sealed.saturating_add(1);
                    self.sealed_total = self.sealed_total.saturating_add(1);
                }
            }
            self.buffer = buffer;
        }
        self.cursor = at;
        let backlog = self.backlog();
        self.backlog_peak = self.backlog_peak.max(backlog);
        ServeReport {
            served,
            backlog,
            sealed,
        }
    }

    /// Copy a freshly computed route into the unit's slot and digest it.
    fn write_route(&mut self, unit: u32, nodes: &[Node], partial: bool) {
        let Ok(index) = usize::try_from(unit) else {
            return;
        };
        let stride = usize::try_from(MAX_ROUTE_NODES).unwrap_or(0);
        let base = index.saturating_mul(stride);
        let take = nodes.len().min(stride);
        let mut at = 0;
        while at < take {
            if let Some(node) = nodes.get(at)
                && let Some(slot) = self.nodes.get_mut(base.saturating_add(at))
            {
                *slot = *node;
            }
            at = at.saturating_add(1);
        }
        let len = u32::try_from(take).unwrap_or(0);
        let mut bytes = core::mem::take(&mut self.digest_bytes);
        let hash = route_digest(nodes.get(..take).unwrap_or(&[]), &mut bytes);
        bytes.clear();
        self.digest_bytes = bytes;
        self.set_route_len(unit, len, 0, hash, partial || take < nodes.len());
    }

    /// Re-arm the sealed units the repair might have freed.
    ///
    /// The repair publishes the clusters it rebuilt; a non-empty list means the
    /// components were rebuilt with them, so **the oracle is asked again about
    /// every sealed unit**: one that is now connected to its destination asks
    /// for a route, and one that is still cut off stays parked. That is two
    /// array reads per sealed unit (item 60), and it is the whole property
    /// rather than a local approximation of it — an earlier version re-armed
    /// only a unit standing in a repaired cluster or bound for one, which left
    /// a unit behind a moat parked for the rest of the match when the moat was
    /// opened two clusters away.
    ///
    /// What keeps this from being the repath loop item 60 rules out is that it
    /// runs only on a tick whose repair changed the graph, and only for a unit
    /// the oracle has changed its mind about.
    pub fn rearm_sealed(
        &mut self,
        surface: &Surface,
        clusters: &Clusters,
        repaired: &[u16],
        positions: &[[Fx; 3]],
        destinations: &[[Fx; 3]],
    ) -> u32 {
        if repaired.is_empty() {
            return 0;
        }
        let mut rearmed: u32 = 0;
        let mut unit: u32 = 0;
        while unit < self.len() {
            if self.state(unit) == WalkState::Sealed {
                let here = node_at(surface, positions, unit);
                let there = node_at(surface, destinations, unit);
                let freed = match (here, there) {
                    (Some(here), Some(there)) => clusters.connected(surface, here, there),
                    _ => false,
                };
                if freed {
                    self.set_state(unit, WalkState::Waiting);
                    rearmed = rearmed.saturating_add(1);
                }
            }
            unit = unit.saturating_add(1);
        }
        rearmed
    }

    /// Advance every walking unit along its route by one tick.
    ///
    /// Item 90's accumulator, and the step validation that notices terrain has
    /// moved: an illegal next step asks for a repath rather than walking
    /// through a crater.
    pub fn advance(
        &mut self,
        surface: &Surface,
        kinds: &[u8],
        hit_points: &[Hp],
        positions: &mut [[Fx; 3]],
        headings: &mut [Angle],
    ) {
        self.arrived.clear();
        let count = self.len();
        let mut walker: u32 = 0;
        while walker < count {
            let id = walker;
            walker = walker.saturating_add(1);
            let index = usize::try_from(id).unwrap_or(usize::MAX);
            if !hit_points
                .get(index)
                .copied()
                .unwrap_or(Hp::ZERO)
                .is_alive()
            {
                continue;
            }
            if self.state(id) != WalkState::Walking {
                continue;
            }
            let speed = kinds
                .get(index)
                .copied()
                .and_then(UnitKind::from_id)
                .map_or(0, |kind| self.speed_of(kind));
            if speed <= 0 {
                continue;
            }
            let mut accumulator = self.accumulator(id).saturating_add(speed);
            let mut cursor = self.route_cursor(id);
            let mut ask_again = false;
            loop {
                let route = self.route(id);
                let Some(from) = route
                    .get(usize::try_from(cursor).unwrap_or(usize::MAX))
                    .copied()
                else {
                    break;
                };
                let Some(to) = route
                    .get(usize::try_from(cursor.saturating_add(1)).unwrap_or(usize::MAX))
                    .copied()
                else {
                    break;
                };
                let Some(step) = surface.step_cost_between(from, to) else {
                    // The terrain moved under the route.
                    ask_again = true;
                    break;
                };
                let price = step.saturating_mul(i32::try_from(TICK_HZ).unwrap_or(20));
                if accumulator < price {
                    break;
                }
                accumulator = accumulator.saturating_sub(price);
                cursor = cursor.saturating_add(1);
            }
            self.set_accumulator(id, accumulator);
            self.set_cursor(id, cursor);
            let route_len = self.route(id).len();
            if let Some(node) = self
                .route(id)
                .get(usize::try_from(cursor).unwrap_or(usize::MAX))
                .copied()
            {
                place(
                    surface,
                    positions,
                    headings,
                    index,
                    node,
                    self.next_node(id, cursor),
                );
            }
            if ask_again {
                self.set_state(id, WalkState::Waiting);
            } else if usize::try_from(cursor).unwrap_or(usize::MAX) >= route_len.saturating_sub(1) {
                if self.route_partial(id) {
                    self.set_state(id, WalkState::Waiting);
                } else {
                    self.set_state(id, WalkState::Idle);
                    self.arrived.push(id);
                }
            }
        }
    }

    fn next_node(&self, unit: u32, cursor: u32) -> Option<Node> {
        self.route(unit)
            .get(usize::try_from(cursor.saturating_add(1)).unwrap_or(usize::MAX))
            .copied()
    }

    fn set_accumulator(&mut self, unit: u32, value: i32) {
        if let Ok(index) = usize::try_from(unit)
            && let Some(slot) = self.accumulator.get_mut(index)
        {
            *slot = value;
        }
    }

    fn set_cursor(&mut self, unit: u32, value: u32) {
        if let Ok(index) = usize::try_from(unit)
            && let Some(slot) = self.route_cursor.get_mut(index)
        {
            *slot = value;
        }
    }

    /// Append the router to the canonical encoding, in unit order.
    pub fn encode(&self, enc: &mut crate::encoding::Enc) {
        enc.len(self.len());
        let mut unit: u32 = 0;
        while unit < self.len() {
            let index = usize::try_from(unit).unwrap_or(usize::MAX);
            enc.u8(self.state.get(index).copied().unwrap_or(0));
            enc.i32(self.accumulator.get(index).copied().unwrap_or(0));
            enc.u32(self.route_len.get(index).copied().unwrap_or(0));
            enc.u32(self.route_cursor.get(index).copied().unwrap_or(0));
            enc.bool(self.route_partial.get(index).copied().unwrap_or(false));
            enc.u64(self.route_hash.get(index).copied().unwrap_or(0));
            unit = unit.saturating_add(1);
        }
        enc.u32(self.cursor);
    }

    /// The walk-state column, as [`WalkState::id`] bytes.
    #[must_use]
    pub fn states(&self) -> &[u8] {
        &self.state
    }

    /// The accumulator column.
    #[must_use]
    pub fn accumulators(&self) -> &[i32] {
        &self.accumulator
    }

    /// The route-length column.
    #[must_use]
    pub fn route_lengths(&self) -> &[u32] {
        &self.route_len
    }

    /// The route-cursor column.
    #[must_use]
    pub fn route_cursors(&self) -> &[u32] {
        &self.route_cursor
    }

    /// The route-digest column.
    #[must_use]
    pub fn route_hashes(&self) -> &[u64] {
        &self.route_hash
    }

    /// The partial-route column.
    #[must_use]
    pub fn route_partials(&self) -> &[bool] {
        &self.route_partial
    }

    /// Every live route's nodes, **packed**: one unit's `route_len` nodes after
    /// another's, in unit order.
    ///
    /// The snapshot carries this rather than the strided buffer, because the
    /// stride is 1 024 nodes a unit and almost all of it is empty. Allocates;
    /// a save is not a tick.
    #[must_use]
    pub fn packed_routes(&self) -> Vec<Node> {
        let mut packed: Vec<Node> = Vec::with_capacity(self.nodes.len().min(4096));
        let mut unit: u32 = 0;
        while unit < self.len() {
            packed.extend_from_slice(self.route(unit));
            unit = unit.saturating_add(1);
        }
        packed
    }

    /// Replace every column from a restored snapshot, expanding the packed
    /// routes back into the strided buffer.
    ///
    /// Returns `false` and changes **nothing** when the columns disagree in
    /// length, when the packed routes are not exactly as long as the lengths
    /// say, when one route is longer than [`MAX_ROUTE_NODES`], or when a
    /// carried route digest is **not** the digest of the nodes beside it —
    /// which is what a truncated or edited file looks like.
    ///
    /// That last check is the one that makes the claim in
    /// [`crate::world::World::encode`]'s docs true: the route reaches the state
    /// hash as a digest, so a file whose `route_nodes` and `unit_route_hash`
    /// disagree would restore into a world that walks one route and hashes
    /// another, and nothing would go red until a replay disagreed. It is the
    /// same argument the chunk store's digests get, and it is checked here for
    /// the same reason (item 66). [`crate::snapshot::Snapshot`] checks it first
    /// so that the failure carries a unit number; this is the last line of
    /// defence for any other caller.
    pub fn restore(&mut self, restored: RestoredRouter) -> bool {
        let units = restored.state.len();
        let stride = usize::try_from(MAX_ROUTE_NODES).unwrap_or(0);
        if restored.accumulator.len() != units
            || restored.route_len.len() != units
            || restored.route_cursor.len() != units
            || restored.route_hash.len() != units
            || restored.route_partial.len() != units
        {
            return false;
        }
        let mut total: usize = 0;
        for len in &restored.route_len {
            let len = usize::try_from(*len).unwrap_or(usize::MAX);
            if len > stride {
                return false;
            }
            total = total.saturating_add(len);
        }
        if total != restored.nodes.len() {
            return false;
        }
        if !route_digests_agree(&restored.route_len, &restored.route_hash, &restored.nodes) {
            return false;
        }

        let mut nodes: Vec<Node> = vec![0; units.saturating_mul(stride)];
        let mut read: usize = 0;
        for (unit, len) in restored.route_len.iter().enumerate() {
            let len = usize::try_from(*len).unwrap_or(0);
            let base = unit.saturating_mul(stride);
            let mut at = 0;
            while at < len {
                if let Some(node) = restored.nodes.get(read.saturating_add(at))
                    && let Some(slot) = nodes.get_mut(base.saturating_add(at))
                {
                    *slot = *node;
                }
                at = at.saturating_add(1);
            }
            read = read.saturating_add(len);
        }

        self.state = restored.state;
        self.accumulator = restored.accumulator;
        self.route_len = restored.route_len;
        self.route_cursor = restored.route_cursor;
        self.route_hash = restored.route_hash;
        self.route_partial = restored.route_partial;
        self.nodes = nodes;
        self.cursor = restored.cursor;
        self.buffer = Vec::with_capacity(stride);
        self.digest_bytes = Vec::with_capacity(stride.saturating_mul(4));
        self.arrived = Vec::with_capacity(units);
        self.served_this_tick = Vec::with_capacity(units);
        true
    }
}

/// The router's columns, owned, on the way back in from a snapshot.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RestoredRouter {
    /// Walk state per unit.
    pub state: Vec<u8>,
    /// Speed accumulator per unit.
    pub accumulator: Vec<i32>,
    /// Route length per unit.
    pub route_len: Vec<u32>,
    /// How far along its route each unit is.
    pub route_cursor: Vec<u32>,
    /// Route digest per unit.
    pub route_hash: Vec<u64>,
    /// Whether each route stops short of its goal.
    pub route_partial: Vec<bool>,
    /// Every live route's nodes, packed in unit order.
    pub nodes: Vec<u32>,
    /// The round-robin cursor.
    pub cursor: u32,
}

/// The first unit whose carried route digest is not the digest of the packed
/// nodes beside it, or `None` when every one of them agrees.
///
/// `route_len` and `route_hash` are the per-unit columns; `nodes` is the packed
/// concatenation the lengths cut up, in unit order. A caller that has not
/// already checked that the lengths sum to `nodes.len()` gets a mismatch on the
/// first unit the packing runs short for, which is the right answer either way.
#[must_use]
pub fn first_route_digest_mismatch(
    route_len: &[u32],
    route_hash: &[u64],
    nodes: &[Node],
) -> Option<u32> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut read: usize = 0;
    for (unit, len) in route_len.iter().enumerate() {
        let len = usize::try_from(*len).unwrap_or(usize::MAX);
        let end = read.saturating_add(len);
        let slice = nodes.get(read..end).unwrap_or(&[]);
        let carried = route_hash.get(unit).copied().unwrap_or(0);
        if route_digest(slice, &mut buffer) != carried {
            return Some(u32::try_from(unit).unwrap_or(u32::MAX));
        }
        read = end;
    }
    None
}

/// Whether every carried route digest describes the nodes beside it.
fn route_digests_agree(route_len: &[u32], route_hash: &[u64], nodes: &[Node]) -> bool {
    first_route_digest_mismatch(route_len, route_hash, nodes).is_none()
}

/// The digest of a route: xxh3 over its node sequence, four little-endian
/// bytes a node, at the project's one hash seed.
///
/// The same shape the chunk store uses (item 66): a thing too big to put in the
/// per-tick encoding enters it as a digest over its own bytes, taken with the
/// project's **one** hash function (AGENTS.md §5). `buffer` is the caller's, and
/// is what keeps a repath allocation-free; it is cleared before use.
#[must_use]
pub fn route_digest(nodes: &[Node], buffer: &mut Vec<u8>) -> u64 {
    buffer.clear();
    if nodes.is_empty() {
        return 0;
    }
    for node in nodes {
        buffer.extend_from_slice(&node.to_le_bytes());
    }
    crate::encoding::digest(buffer)
}

/// The column a unit's position or destination stands on.
fn node_at(surface: &Surface, points: &[[Fx; 3]], unit: u32) -> Option<Node> {
    let index = usize::try_from(unit).ok()?;
    let point = points.get(index)?;
    let x = point.first()?.floor_voxels();
    let y = point.get(1)?.floor_voxels();
    surface.node_of(x, y)
}

/// Put a unit on a column: its position, its ground height and its heading.
fn place(
    surface: &Surface,
    positions: &mut [[Fx; 3]],
    headings: &mut [Angle],
    index: usize,
    node: Node,
    next: Option<Node>,
) {
    let (x, y) = surface.coord_of(node);
    let z = surface.standing_z(node);
    if let Some(point) = positions.get_mut(index) {
        if let Some(slot) = point.first_mut() {
            *slot = Fx::from_voxels(i16::try_from(x).unwrap_or(0));
        }
        if let Some(slot) = point.get_mut(1) {
            *slot = Fx::from_voxels(i16::try_from(y).unwrap_or(0));
        }
        if let Some(slot) = point.get_mut(2) {
            *slot = Fx::from_voxels(i16::try_from(z).unwrap_or(0));
        }
    }
    if let Some(next) = next
        && let Some(heading) = headings.get_mut(index)
    {
        let (nx, ny) = surface.coord_of(next);
        *heading = Angle::from_delta(
            Fx::from_voxels(i16::try_from(nx.saturating_sub(x)).unwrap_or(0)),
            Fx::from_voxels(i16::try_from(ny.saturating_sub(y)).unwrap_or(0)),
        );
    }
}
