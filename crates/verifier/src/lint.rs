// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stage 6, **lint**: the second half of FULL.
//!
//! The stage existed from day one, empty, so the pipeline's shape and the hash
//! contract were fixed before anything depended on them (decisions-log item
//! 82). S1's targeting verifier (S1's plan, task `tgtv`) brings its first
//! findings: targeting's three lints (docs/design/targeting.md, "Surfaces").
//!
//! # What lives here, and who brings the rest
//!
//! The `W07xx` family — schedule, conflicts and staleness — and the information
//! codes:
//!
//! | Work | Code | Brought by |
//! |---|---|---|
//! | A description, or a name, that matches nothing on the map right now | `W0704` | **S1**, here |
//! | "An earlier step places a beacon; this may read differently" | `W0705` | **S1**, here |
//! | A fixed site beside a vent the seat already covers, with a Fix that swaps in the description | `W0706` | **S1**, here |
//! | A handler that can never fire because an earlier one always wins | `W0702` | **S3**, with the rule list at full depth |
//! | A reference resting on a sighting older than the reach window | `W0703` | **S2/S3**, once there are sightings to be stale |
//! | Selector previews — "if this resolved now, it would pick…" | `I0002` | **S3** |
//!
//! A lint is **advice**, never a refusal: everything this stage can raise is a
//! warning or information, and spec section 11 is explicit that a playbook
//! qualifies on having zero **errors**. Nothing added here may change whether a
//! playbook qualifies without that being a deliberate, stated change, and the
//! catalogue's own test refuses an error row that names this stage.
//!
//! # None of the three ranks, and none of them is a dry run
//!
//! "Nearest" is the sim's and the gateway's, where the pathing graph is; the
//! verifier checks and **never ranks** (targeting.md, "Surfaces"). So:
//!
//! * `W0704` asks only whether *anything* passes a pick's filters — the kind,
//!   liveness, and, under UNCOVERED, the seat's own coverage, all read from the
//!   [`Scope`] the gateway built with the sim's rules. It never asks which one
//!   would win.
//! * `W0705` is a syntactic check on the text: whether a step that may run
//!   before this one is a `place_beacon` — an earlier route step, or any
//!   handler body's, since a handler can fire at any decision tick. It is never
//!   a projection of what that placement would cover.
//! * `W0706` compares a fixed site's column with each live vent's anchor column
//!   by squared distance in x and y against the sphere radius, and names the
//!   first such vent in anchor (y, x) order. The view carries no feature's
//!   height (a name has no z, because craters change it), so this is the
//!   column test, not the sim's sphere test: a vent it counts as within reach
//!   may in fact lie out of the sphere above or below, and the lint is the
//!   warning it is for that reason. It orders by anchor, never by distance.
//!
//! "A handler that can never fire" is likewise a question about the text and
//! not about a projected world. The moment a lint needs a future state to
//! answer, it is out of bounds and belongs nowhere in this crate (AGENTS.md
//! section 3 rule 2).

use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::v1::feature_ref::{self, Coverage, Rank};
use pharmakos_proto::gp::v1::{Location, Playbook, Step, Voxel, location, step};
use pharmakos_proto::json::Json;

use crate::limits::Limits;
use crate::pointer;
use crate::report::{Builder, Diag, patch_replace};
use crate::resolve::Symbols;
use crate::scope::{FeatureKind, KnownFeature, Scope};
use crate::strings;
use crate::structure::arm_name;
use crate::walk::{self, List, Slot, Visit};

/// Run the stage.
pub(crate) fn run(
    playbook: &Playbook,
    scope: &Scope,
    limits: &Limits,
    _symbols: &Symbols,
    out: &mut Builder,
) {
    let placements = Placements::of(playbook);
    let mut lints = Lints {
        scope,
        limits,
        placements: &placements,
        current: None,
        out,
    };
    walk::walk(playbook, &mut lints);
}

/// Which steps are `place_beacon`s, list by list, for `W0705`.
struct Placements {
    route: Vec<bool>,
    bodies: Vec<Vec<bool>>,
}

impl Placements {
    fn of(playbook: &Playbook) -> Placements {
        let places = |entry: &Step| matches!(entry.kind, Some(step::Kind::PlaceBeacon(_)));
        let Some(declarative) = playbook.declarative.as_ref() else {
            return Placements {
                route: Vec::new(),
                bodies: Vec::new(),
            };
        };
        Placements {
            route: declarative.route.iter().map(places).collect(),
            bodies: declarative
                .handlers
                .iter()
                .map(|handler| handler.body.iter().map(places).collect())
                .collect(),
        }
    }

    /// The pointer to the first `place_beacon` that may run before the step at
    /// `index` of `list`, if any does.
    ///
    /// For a route step: an earlier route step, then any handler body's step,
    /// because a handler can fire before the route reaches this one. For a
    /// step in a handler body: any route step and any other body step, because
    /// the route and the other handlers may all have run before this handler
    /// fires. The step itself never counts: its own targets are read when it
    /// starts, before anything it places.
    fn before(&self, list: List, index: usize) -> Option<String> {
        let route_limit = match list {
            List::Route => index,
            List::Body(_) => self.route.len(),
        };
        if let Some(found) = self
            .route
            .iter()
            .take(route_limit)
            .position(|places| *places)
        {
            return Some(pointer::at("/declarative/route", found));
        }
        for (handler, body) in self.bodies.iter().enumerate() {
            for (position, places) in body.iter().enumerate() {
                let itself = list == List::Body(handler) && position == index;
                if *places && !itself {
                    return Some(pointer::at(
                        &pointer::child(&pointer::at("/declarative/handlers", handler), "body"),
                        position,
                    ));
                }
            }
        }
        None
    }
}

struct Lints<'a> {
    scope: &'a Scope,
    limits: &'a Limits,
    placements: &'a Placements,
    current: Option<(List, usize)>,
    out: &'a mut Builder,
}

impl Visit for Lints<'_> {
    fn entry(&mut self, _at: &str, _entry: &Step, list: List, index: usize) {
        self.current = Some((list, index));
    }

    fn location(&mut self, at: &str, place: &Location, slot: Slot<'_>) {
        // Only an arm standing where it is legal: anywhere else the structure
        // stage has already refused it, and advice about it would be noise.
        let (arm, feature, covering) = match (place.place.as_ref(), slot) {
            (Some(location::Place::Covering(feature)), Slot::PlaceSite) => {
                ("covering", feature, true)
            }
            (Some(location::Place::On(feature)), Slot::BuildAnchor { .. }) => {
                ("on", feature, false)
            }
            _ => return,
        };
        let Some(chosen) = feature.r#ref.as_ref() else {
            return;
        };
        let here = pointer::child(&pointer::child(at, arm), arm_name(chosen));
        let (kind, rank, coverage) = match chosen {
            feature_ref::Ref::FeatureId(id) => {
                if self.scope.feature(id).is_some_and(|known| !known.live) {
                    self.out.emit(
                        Diag::new("W0704", here)
                            .arg("what", format!("`{id}`"))
                            .arg("why", strings::WHY_LOST),
                    );
                }
                return;
            }
            feature_ref::Ref::Covered(_) => return,
            feature_ref::Ref::Vent(pick) => (FeatureKind::Vent, pick.rank(), pick.coverage()),
            feature_ref::Ref::Seam(pick) => (FeatureKind::Seam, pick.rank(), pick.coverage()),
        };
        // A pick the structure stage refused (no rank, or a coverage that may
        // not stand under this arm) has no meaning to advise on.
        let well_formed = rank != Rank::Unspecified
            && if covering {
                coverage != Coverage::Unspecified
            } else {
                coverage == Coverage::Unspecified
            };
        if !well_formed {
            return;
        }
        let uncovered = coverage == Coverage::Uncovered;
        let what = describe(kind, uncovered);
        self.matches_nothing(&here, kind, uncovered, what);
        if let Some((list, index)) = self.current {
            if let Some(earlier) = self.placements.before(list, index) {
                self.out
                    .emit(Diag::new("W0705", here).arg("what", what).related(earlier));
            }
        }
    }

    fn place_site(&mut self, at: &str, site: &Location) {
        let Some(location::Place::Voxel(voxel)) = site.place.as_ref() else {
            return;
        };
        self.already_covered(at, voxel);
    }
}

impl Lints<'_> {
    /// `W0704` for a pick: nothing passes its filters.
    fn matches_nothing(&mut self, at: &str, kind: FeatureKind, uncovered: bool, what: &str) {
        let mut live = self
            .scope
            .features()
            .iter()
            .filter(|feature| feature.kind == kind && feature.live);
        let why = if uncovered {
            let mut any_live = false;
            for feature in live.by_ref() {
                any_live = true;
                if feature.covered_by.is_none() {
                    return;
                }
            }
            if any_live {
                strings::WHY_ALL_COVERED
            } else {
                strings::WHY_NONE_LIVE
            }
        } else {
            if live.next().is_some() {
                return;
            }
            strings::WHY_NONE_LIVE
        };
        self.out.emit(
            Diag::new("W0704", at.to_owned())
                .arg("what", what)
                .arg("why", why),
        );
    }

    /// `W0706`: every live vent within a sphere's reach of a fixed site's
    /// column is already covered by one of the seat's own beacons.
    fn already_covered(&mut self, at: &str, site: &Voxel) {
        let radius = i64::from(self.limits.beacon_sphere_radius_voxels());
        let reach = radius.saturating_mul(radius);
        let mut first: Option<&KnownFeature> = None;
        for vent in self.scope.features().iter().filter(|feature| {
            feature.kind == FeatureKind::Vent
                && feature.live
                && column_distance(feature, site) <= reach
        }) {
            if vent.covered_by.is_none() {
                return;
            }
            let earlier = first.is_none_or(|held| (vent.y, vent.x) < (held.y, held.x));
            if earlier {
                first = Some(vent);
            }
        }
        let Some(vent) = first else {
            return;
        };
        let beacon = vent.covered_by.as_deref().unwrap_or_default();
        self.out.emit(
            Diag::new("W0706", pointer::child(at, "voxel"))
                .arg(
                    "feature",
                    format!("{} ({}, {})", strings::VENT_NAME, vent.x, vent.y),
                )
                .arg("beacon", beacon)
                .map_ref(*site)
                .fix(
                    strings::FIX_COVER_NEAREST_VENT,
                    // `replace`: the site is there by construction, since this
                    // lint fired on it. MAYBE_INCORRECT, not a one-click Fix
                    // button: the swap moves where the beacon goes (the sim
                    // resolves the description at step start), and the lint's
                    // premise is the column test above, which can count a vent
                    // that lies outside the sphere. The author looks first.
                    patch_replace(at, &cover_nearest_uncovered_vent()),
                    Applicability::MaybeIncorrect,
                ),
        );
    }
}

/// How a pick reads in a message: "the nearest vent you do not cover".
const fn describe(kind: FeatureKind, uncovered: bool) -> &'static str {
    match (kind, uncovered) {
        (FeatureKind::Vent, false) => strings::PICK_NEAREST_VENT,
        (FeatureKind::Vent, true) => strings::PICK_NEAREST_UNCOVERED_VENT,
        (FeatureKind::Seam, false) => strings::PICK_NEAREST_SEAM,
        (FeatureKind::Seam, true) => strings::PICK_NEAREST_UNCOVERED_SEAM,
    }
}

/// Squared distance in x and y between a feature's anchor column and a voxel,
/// in whole voxels squared, saturating as `crate::semantics` does.
fn column_distance(feature: &KnownFeature, site: &Voxel) -> i64 {
    let axis = |one: i32, other: i32| {
        let delta = i64::from(one).saturating_sub(i64::from(other));
        delta.saturating_mul(delta)
    };
    axis(feature.x, site.x).saturating_add(axis(feature.y, site.y))
}

/// `{"covering":{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}}`: the
/// description a carried fixed site should have been (targeting.md,
/// "Surfaces").
fn cover_nearest_uncovered_vent() -> Json {
    Json::Object(vec![(
        "covering".to_owned(),
        Json::Object(vec![(
            "vent".to_owned(),
            Json::Object(vec![
                ("rank".to_owned(), Json::String("NEAREST".to_owned())),
                ("coverage".to_owned(), Json::String("UNCOVERED".to_owned())),
            ]),
        )]),
    )])
}
