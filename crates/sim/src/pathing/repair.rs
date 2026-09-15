// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Eager chunk-footprint repair (decisions log item 60).
//!
//! # What an edit dirties, and why the neighbours come too
//!
//! A cluster is a chunk footprint, so a crater's dirty chunk list *is* its
//! dirty cluster list. But repairing one cluster means rescanning its
//! entrances, and an entrance is **shared**: rescanning the border between `A`
//! and `B` changes `B`'s transition set too, so `B`'s intra edges are stale even
//! though no voxel inside `B` moved. Missing that is the classic HPA\* repair
//! bug — an abstract edge to a transition node that no longer exists, found
//! hours later on one seed.
//!
//! "Affected neighbour" is **computed, not assumed**: a border that comes back
//! identical costs its neighbour nothing, and most do. The three tests item 60
//! names pin it — repair equals a from-scratch build, repair equals the
//! conservative all-neighbours set, and both hold at every cluster size.
//!
//! One thing here is wider than the spike's shape, and for a reason worth
//! stating. The spike rebuilt one global CSR per tick, so abstract ids were
//! renumbered wholesale and a cluster's neighbours needed nothing. This
//! decomposition numbers abstract nodes `cluster * max_trans + slot`, which is
//! what lets a repair touch five clusters instead of rebuilding a graph — and
//! the price is that a cluster whose transition set changed shifts its own
//! slots, so **every cluster with a border edge into it** has to rebuild those
//! edges. That is the `inter` set below: the transition-dirty clusters and
//! their four neighbours.
//!
//! # One connectivity rebuild per tick
//!
//! The union-find is rebuilt once, after every dirty cluster has been repaired,
//! rather than once per crater: the labels are a function of the whole
//! decomposition and there is no cheaper correct increment. That is item 60's
//! "one CSR + connectivity rebuild per tick", and it is the 0.50 ms the spike
//! measured.

use crate::pathing::clusters::Clusters;
use crate::pathing::search::Scratch;
use crate::pathing::surface::Surface;

/// What one tick's repair did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RepairReport {
    /// Clusters an edit touched.
    pub dirty: u32,
    /// Clusters whose transition set was recomputed.
    pub transitions_rebuilt: u32,
    /// Clusters whose intra edges were recomputed.
    pub intra_rebuilt: u32,
    /// Clusters whose border edges were recomputed.
    pub inter_rebuilt: u32,
}

/// The repair's working sets, owned once so a repair inside a tick allocates
/// nothing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Repairer {
    dirty: Vec<u16>,
    dirty_flag: Vec<bool>,
    transitions: Vec<u16>,
    transitions_flag: Vec<bool>,
    inter: Vec<u16>,
    inter_flag: Vec<bool>,
    repaired: Vec<u16>,
}

impl Repairer {
    /// Room for a map of `clusters` clusters.
    #[must_use]
    pub fn new(clusters: usize) -> Repairer {
        Repairer {
            dirty: Vec::with_capacity(clusters),
            dirty_flag: vec![false; clusters],
            transitions: Vec::with_capacity(clusters),
            transitions_flag: vec![false; clusters],
            inter: Vec::with_capacity(clusters),
            inter_flag: vec![false; clusters],
            repaired: Vec::with_capacity(clusters),
        }
    }

    /// Mark a cluster as holding an edited column.
    ///
    /// Called from the voxel phase with the chunks the store just settled, so
    /// the dirty set is the settled set with the `z` dropped.
    pub fn mark(&mut self, cluster: u16) {
        let index = usize::from(cluster);
        if matches!(self.dirty_flag.get(index), Some(false)) {
            if let Some(flag) = self.dirty_flag.get_mut(index) {
                *flag = true;
            }
            self.dirty.push(cluster);
        }
    }

    /// Whether anything is waiting to be repaired.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// The clusters the last repair rebuilt, ascending.
    ///
    /// The router reads it to re-arm the units it parked as sealed in: a walker
    /// that could not reach its destination tries again exactly when the graph
    /// it was refused by has changed, which is what keeps "sealed in" a state
    /// rather than a repath loop (item 60).
    #[must_use]
    pub fn repaired(&self) -> &[u16] {
        &self.repaired
    }
}

/// Repair the graph for everything marked since the last call.
///
/// The order is the one item 60 fixes: local components and entrances for the
/// dirty clusters, then transition sets for them and for the neighbours whose
/// shared border actually changed, then intra edges for those, then border
/// edges for those and their neighbours, then **one** connectivity rebuild.
pub fn repair(
    surface: &Surface,
    clusters: &mut Clusters,
    scratch: &mut Scratch,
    repairer: &mut Repairer,
) -> RepairReport {
    let mut dirty = core::mem::take(&mut repairer.dirty);
    let mut transitions = core::mem::take(&mut repairer.transitions);
    let mut inter = core::mem::take(&mut repairer.inter);
    let mut rebuilt = core::mem::take(&mut repairer.repaired);
    transitions.clear();
    inter.clear();
    rebuilt.clear();
    if dirty.is_empty() {
        repairer.dirty = dirty;
        repairer.transitions = transitions;
        repairer.inter = inter;
        repairer.repaired = rebuilt;
        return RepairReport::default();
    }
    // The key IS the cluster index, unique by the flag guard in `mark`, so the
    // order is total (item 62).
    dirty.sort_unstable();

    for cluster in &dirty {
        mark_in(&mut transitions, &mut repairer.transitions_flag, *cluster);
        let index = usize::from(*cluster);
        clusters.rebuild_local_comp(surface, index);
        let (cx, cy) = clusters.cluster_coord(index);
        if clusters.rebuild_vborder(surface, cx, cy)
            && let Some(neighbour) = clusters.cluster_index(cx.saturating_add(1), cy)
            && let Ok(tag) = u16::try_from(neighbour)
        {
            mark_in(&mut transitions, &mut repairer.transitions_flag, tag);
        }
        if clusters.rebuild_vborder(surface, cx.saturating_sub(1), cy)
            && let Some(neighbour) = clusters.cluster_index(cx.saturating_sub(1), cy)
            && let Ok(tag) = u16::try_from(neighbour)
        {
            mark_in(&mut transitions, &mut repairer.transitions_flag, tag);
        }
        if clusters.rebuild_hborder(surface, cx, cy)
            && let Some(neighbour) = clusters.cluster_index(cx, cy.saturating_add(1))
            && let Ok(tag) = u16::try_from(neighbour)
        {
            mark_in(&mut transitions, &mut repairer.transitions_flag, tag);
        }
        if clusters.rebuild_hborder(surface, cx, cy.saturating_sub(1))
            && let Some(neighbour) = clusters.cluster_index(cx, cy.saturating_sub(1))
            && let Ok(tag) = u16::try_from(neighbour)
        {
            mark_in(&mut transitions, &mut repairer.transitions_flag, tag);
        }
    }
    transitions.sort_unstable();

    for cluster in &transitions {
        clusters.rebuild_trans(usize::from(*cluster));
    }
    for cluster in &transitions {
        mark_in(&mut inter, &mut repairer.inter_flag, *cluster);
        let (cx, cy) = clusters.cluster_coord(usize::from(*cluster));
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            if let Some(neighbour) =
                clusters.cluster_index(cx.saturating_add(dx), cy.saturating_add(dy))
                && let Ok(tag) = u16::try_from(neighbour)
            {
                mark_in(&mut inter, &mut repairer.inter_flag, tag);
            }
        }
    }
    inter.sort_unstable();

    for cluster in &transitions {
        clusters.rebuild_intra(surface, scratch, usize::from(*cluster));
    }
    for cluster in &inter {
        clusters.rebuild_inter(usize::from(*cluster));
    }
    clusters.rebuild_components();

    rebuilt.extend_from_slice(&inter);
    let report = RepairReport {
        dirty: u32::try_from(dirty.len()).unwrap_or(0),
        transitions_rebuilt: u32::try_from(transitions.len()).unwrap_or(0),
        intra_rebuilt: u32::try_from(transitions.len()).unwrap_or(0),
        inter_rebuilt: u32::try_from(inter.len()).unwrap_or(0),
    };

    for cluster in &dirty {
        if let Some(flag) = repairer.dirty_flag.get_mut(usize::from(*cluster)) {
            *flag = false;
        }
    }
    for cluster in &transitions {
        if let Some(flag) = repairer.transitions_flag.get_mut(usize::from(*cluster)) {
            *flag = false;
        }
    }
    for cluster in &inter {
        if let Some(flag) = repairer.inter_flag.get_mut(usize::from(*cluster)) {
            *flag = false;
        }
    }
    dirty.clear();
    repairer.dirty = dirty;
    repairer.transitions = transitions;
    repairer.inter = inter;
    repairer.repaired = rebuilt;
    report
}

fn mark_in(list: &mut Vec<u16>, flags: &mut [bool], cluster: u16) {
    let index = usize::from(cluster);
    if matches!(flags.get(index), Some(false)) {
        if let Some(flag) = flags.get_mut(index) {
            *flag = true;
        }
        list.push(cluster);
    }
}

/// The **conservative** superset of the clusters an edit can invalidate: the
/// dirty ones and every cluster sharing a border with one of them, plus their
/// neighbours again for the border edges.
///
/// [`repair`] uses the precise set instead, and
/// `repair_matches_the_conservative_set` in `tests/pathing.rs` checks that the
/// two reach the same graph — which is the only thing that makes the narrower
/// set safe. Allocates, and is a test's tool rather than a tick's.
#[must_use]
pub fn conservative_affected(clusters: &Clusters, dirty: &[u16]) -> Vec<u16> {
    let mut wide: Vec<u16> = Vec::with_capacity(dirty.len().saturating_mul(9));
    for cluster in dirty {
        let (cx, cy) = clusters.cluster_coord(usize::from(*cluster));
        for dy in -2_i32..=2 {
            for dx in -2_i32..=2 {
                if let Some(index) =
                    clusters.cluster_index(cx.saturating_add(dx), cy.saturating_add(dy))
                    && let Ok(tag) = u16::try_from(index)
                {
                    wide.push(tag);
                }
            }
        }
    }
    // The key IS the cluster index, unique after the dedup, so the order is
    // total (item 62).
    wide.sort_unstable();
    wide.dedup();
    wide
}
