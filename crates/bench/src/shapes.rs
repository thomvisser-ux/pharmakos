// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! P1's generated worst-shaped playbooks (S3's plan, section 5.2 item 1, task
//! `bench3` and E24; decisions-log items 157 (7) and 158 (1)).
//!
//! The committed verifier cases are small by design, and `budget_128` (64
//! route steps and 16 handlers of three steps) is the only one at the size
//! budget. A size unit bounds route steps, handlers, their bodies, build
//! targets and protected areas (item 94) but **not** a condition's nodes nor a
//! playbook's bytes, so a legal playbook at the budget can cost the verifier
//! several times what `budget_128` does. These generators spend every unit of
//! a size on one thing, so the curve E24 reads the size budget from is taken
//! on the costliest playbooks a size allows:
//!
//! | [`Shape`] | Every unit spent on |
//! |---|---|
//! | [`Shape::Handlers24`] | handlers whose conditions are 24 nodes, `any(any(any(21)))` and `all(all(all(21)))` in turn over distinct leaves, one body step each, the rest route steps: item 158 (1)'s first shape, `vfy5`'s `lint::tests::the_worst_case_is_bounded_by_the_memo` (63 handlers and a route step at 127 units) |
//! | [`Shape::RouteGuards24`] | route steps, each with a 24-node `skip_if` of the same form: item 158 (1)'s second shape |
//! | [`Shape::HandlersDepth4`] | handlers with depth-4 conditions (`any(all(any(leaf)))`), every unit past the one route step in handlers and their bodies |
//! | [`Shape::BuildTargets`] | Build targets: one interface step at the fixture's core adding a Generator a unit |
//! | [`Shape::ProtectedAreas`] | protected areas: one interface step at the core whose Build settings row lists an area a unit |
//! | [`Shape::LongestRoute`] | route steps, each a `move` to a voxel of its own: the longest route |
//!
//! Each generator answers exactly the units it is asked for, or a typed
//! [`Error::Shape`] when the shape cannot be laid at that size (a handler
//! shape below three units, a route past the interpreter's step ceiling); the
//! unit test `every_shape_is_exactly_its_size` holds the count to the
//! verifier's own `size_units`. The handlers are bounded by the
//! interpreter's `MAX_HANDLERS`, so past 129 units a handler shape spends the
//! rest on route steps; `tests/harness.rs` checks that every shape at every
//! size qualifies against the bench's fixture under a rules table whose budget
//! is that size, so a size's figure is the figure of a playbook the verifier
//! accepts, not of a refusal.
//!
//! The playbooks are built as `gp.v1` messages and written by the canonical
//! codec ([`pharmakos_proto::json::encode`]), so their bytes are what the
//! editor would write for them. Every number in them is the harness's own
//! input, fixed here, so a shape and a size reproduce its bytes.

use pharmakos_proto::gp::v1::condition::Node;
use pharmakos_proto::gp::v1::handler::Resume;
use pharmakos_proto::gp::v1::int_compare::Op;
use pharmakos_proto::gp::v1::meta::AuthorKind;
use pharmakos_proto::gp::v1::move_step::Pace;
use pharmakos_proto::gp::v1::on_death::OnRespawn;
use pharmakos_proto::gp::v1::playbook::Kind;
use pharmakos_proto::gp::v1::{
    AddBuildTargetRow, All, Any, Area, BeaconRef, BuildSettings, BuildTarget, Condition,
    Declarative, Fallback, FallbackHold, Handler, HoldStep, IntCompare, InterfaceRow,
    InterfaceStep, Location, MandateSettings, Meta, MoveStep, OnDeath, Playbook, Safest,
    SchemaVersion, SegmentElapsed, Step, beacon_ref, fallback, interface_row, location,
    mandate_settings, step,
};
use pharmakos_proto::json;
use pharmakos_sim::interpreter::{MAX_HANDLERS, MAX_ROUTE_STEPS};

use crate::error::Error;

/// The sizes P1's curve is taken at (S3's plan, E24): from half the S1 budget
/// to twice it, so the budget is read from measurements on both sides of it.
pub const SIZES: [u32; 5] = [64, 96, 128, 192, 256];

/// The leaves under each group of a 24-node condition: three groups and 21
/// leaves are `CONDITION_MAX_NODES`, the most spec section 10 allows.
const LEAVES_PER_CONDITION: usize = 21;

/// The fixture's own core, by its id in `quick::fixture_scope`: a Build
/// beacon of seat 0 at (80, 11, 55), whose 24-voxel sphere the target and
/// area shapes fill.
const CORE_ID: &str = "b_01";

/// The core's column and height in `quick::fixture_scope`.
const CORE_AT: [i32; 3] = [80, 11, 55];

/// The farthest an anchor of the target and area shapes stands from the
/// core, along either axis and as a column distance: inside its 24-voxel
/// sphere with a voxel to spare for an area's far corner.
const ANCHOR_REACH: i32 = 22;

/// The anchors stand two voxels apart, so no two share a column.
const ANCHOR_STEP: usize = 2;

/// One generated shape (see the module docs' table).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Shape {
    /// Handlers with 24-node conditions (item 158 (1)).
    Handlers24,
    /// Route steps with 24-node guards (item 158 (1)).
    RouteGuards24,
    /// Handlers with depth-4 conditions, every unit in handlers and bodies.
    HandlersDepth4,
    /// Build targets.
    BuildTargets,
    /// Protected areas.
    ProtectedAreas,
    /// The longest route: a `move` a unit.
    LongestRoute,
}

impl Shape {
    /// Every shape, in the order the notices print them.
    pub const ALL: [Shape; 6] = [
        Shape::Handlers24,
        Shape::RouteGuards24,
        Shape::HandlersDepth4,
        Shape::BuildTargets,
        Shape::ProtectedAreas,
        Shape::LongestRoute,
    ];

    /// The shape's name, as notices and errors print it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Shape::Handlers24 => "handlers_24_node",
            Shape::RouteGuards24 => "route_guards_24_node",
            Shape::HandlersDepth4 => "handlers_depth_4",
            Shape::BuildTargets => "build_targets",
            Shape::ProtectedAreas => "protected_areas",
            Shape::LongestRoute => "longest_route",
        }
    }

    /// The shape at `units` size units, as a message.
    ///
    /// # Errors
    ///
    /// [`Error::Shape`] when the shape cannot be laid at that size, and
    /// [`Error::Overflow`] for a coordinate or a duration past its type.
    pub fn playbook(self, units: u32) -> Result<Playbook, Error> {
        let refuse = |reason: String| Error::Shape {
            shape: self.name(),
            units,
            reason,
        };
        let size = usize::try_from(units).map_err(|_| Error::Overflow("a shape's size"))?;
        let declarative = match self {
            Shape::Handlers24 => handlers(size, Group::Deep, Bodies::One).map_err(refuse)?,
            Shape::HandlersDepth4 => handlers(size, Group::Chain, Bodies::Fill).map_err(refuse)?,
            Shape::RouteGuards24 => guarded_route(size).map_err(refuse)?,
            Shape::LongestRoute => longest_route(size).map_err(refuse)?,
            Shape::BuildTargets => build_targets(size).map_err(refuse)?,
            Shape::ProtectedAreas => protected_areas(size).map_err(refuse)?,
        };
        Ok(Playbook {
            schema_version: Some(SchemaVersion { major: 1, minor: 0 }),
            meta: Some(Meta {
                title: format!("P1 shape {} at {units} units", self.name()),
                author_kind: i32::from(AuthorKind::Human),
                ..Meta::default()
            }),
            kind: i32::from(Kind::Playbook),
            declarative: Some(declarative),
            on_death: Some(OnDeath {
                on_respawn: i32::from(OnRespawn::Continue),
                max_deaths_before_fallback: 1,
            }),
            fallback: Some(Fallback {
                posture: Some(fallback::Posture::Hold(FallbackHold { at: Some(safest()) })),
            }),
        })
    }

    /// The shape at `units` size units, as canonical JSON bytes: what a timed
    /// call reads.
    ///
    /// # Errors
    ///
    /// As [`Shape::playbook`], and [`Error::Codec`] when the canonical codec
    /// refuses the message.
    pub fn bytes(self, units: u32) -> Result<Vec<u8>, Error> {
        let playbook = self.playbook(units)?;
        json::encode(&playbook)
            .map(String::into_bytes)
            .map_err(|error| Error::Codec {
                what: format!("encoding the {} shape at {units} units", self.name()),
                reason: error.to_string(),
            })
    }
}

/// `beacon_anchor { safest {} }`: the one place every shape's fallback holds.
fn safest() -> Location {
    Location {
        place: Some(location::Place::BeaconAnchor(BeaconRef {
            r#ref: Some(beacon_ref::Ref::Safest(Safest::default())),
        })),
    }
}

fn node(node: Node) -> Condition {
    Condition { node: Some(node) }
}

/// The distinct leaves: `segment_elapsed EQ n` for a counter shared by the
/// whole playbook, so no condition implies another and W0702's search runs
/// every pair to its end (`vfy5`'s worst case).
#[derive(Debug, Default)]
struct Leaves {
    next: i32,
}

impl Leaves {
    fn leaf(&mut self) -> Result<Condition, String> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| "more leaves than an i32 counts".to_owned())?;
        Ok(node(Node::SegmentElapsed(SegmentElapsed {
            ms: Some(IntCompare {
                op: i32::from(Op::Eq),
                value: self.next,
            }),
        })))
    }
}

/// How a handler's condition is grouped.
#[derive(Clone, Copy, Debug)]
enum Group {
    /// `group(group(group(21 leaves)))`, `any` and `all` in turn: 24 nodes.
    Deep,
    /// `any(all(any(leaf)))`: four levels, four nodes.
    Chain,
}

/// How many body steps a handler holds.
#[derive(Clone, Copy, Debug)]
enum Bodies {
    /// One each; the units left over go to route steps.
    One,
    /// Every unit past the one route step goes to handlers and their bodies.
    Fill,
}

/// A 24-node condition: `any(any(any(21)))` when `any`, else `all(all(all(21)))`.
fn deep(any: bool, leaves: &mut Leaves) -> Result<Condition, String> {
    let wrap = |items: Vec<Condition>| {
        if any {
            node(Node::Any(Any { items }))
        } else {
            node(Node::All(All { items }))
        }
    };
    let inner: Vec<Condition> = (0..LEAVES_PER_CONDITION)
        .map(|_| leaves.leaf())
        .collect::<Result<_, _>>()?;
    Ok(wrap(vec![wrap(vec![wrap(inner)])]))
}

/// `any(all(any(leaf)))`: a condition four levels deep.
fn chain(leaves: &mut Leaves) -> Result<Condition, String> {
    let leaf = leaves.leaf()?;
    Ok(node(Node::Any(Any {
        items: vec![node(Node::All(All {
            items: vec![node(Node::Any(Any { items: vec![leaf] }))],
        }))],
    })))
}

/// A one-second hold, labelled.
fn hold(label: String) -> Step {
    Step {
        label,
        kind: Some(step::Kind::Hold(HoldStep { ms: 1_000 })),
        ..Step::default()
    }
}

/// `count` route steps, `r000` on, each a one-second hold.
fn holds(count: usize) -> Vec<Step> {
    (0..count).map(|at| hold(format!("r{at:03}"))).collect()
}

/// The handler shapes: as many handlers as the size and `MAX_HANDLERS`
/// allow, each with its condition and body, and route steps for the rest.
fn handlers(size: usize, group: Group, bodies: Bodies) -> Result<Declarative, String> {
    // A handler and one body step are two units, and the route holds one.
    let count = size
        .checked_sub(1)
        .map(|rest| rest.div_euclid(2))
        .filter(|count| *count > 0)
        .ok_or_else(|| {
            "a handler shape needs three units: a handler, its step and a route step".to_owned()
        })?
        .min(MAX_HANDLERS);
    // `count` is at most (size - 1) / 2, so both remainders are at least one.
    let left = count
        .checked_mul(2)
        .and_then(|in_handlers| size.checked_sub(in_handlers))
        .ok_or_else(|| "the handlers' units".to_owned())?;
    let (route_steps, extra_bodies) = match bodies {
        Bodies::One => (left, 0),
        Bodies::Fill => (
            1,
            left.checked_sub(1)
                .ok_or_else(|| "the route step's unit".to_owned())?,
        ),
    };
    if route_steps > MAX_ROUTE_STEPS {
        return Err(format!(
            "{route_steps} route steps, past the interpreter's {MAX_ROUTE_STEPS}"
        ));
    }
    let mut leaves = Leaves::default();
    let mut list: Vec<Handler> = Vec::with_capacity(count);
    for id in 0..count {
        let when = match group {
            Group::Deep => deep(id % 2 == 0, &mut leaves)?,
            Group::Chain => chain(&mut leaves)?,
        };
        // Spread the extra body steps round the handlers, the first ones first.
        let more =
            extra_bodies.div_euclid(count) + usize::from(id < extra_bodies.rem_euclid(count));
        let body: Vec<Step> = (0..=more)
            .map(|at| hold(format!("h{id:02}_{at:02}")))
            .collect();
        list.push(Handler {
            id: format!("h{id:02}"),
            when: Some(when),
            body,
            resume: i32::from(Resume::Continue),
            cooldown_ms: 5_000,
            max_fires: 2,
            ..Handler::default()
        });
    }
    Ok(Declarative {
        route: holds(route_steps),
        handlers: list,
        options: None,
    })
}

/// Route steps with 24-node guards, one a unit.
fn guarded_route(size: usize) -> Result<Declarative, String> {
    if size == 0 || size > MAX_ROUTE_STEPS {
        return Err(format!(
            "a route of {size} steps, outside the interpreter's 1 to {MAX_ROUTE_STEPS}"
        ));
    }
    let mut leaves = Leaves::default();
    let mut route: Vec<Step> = Vec::with_capacity(size);
    for at in 0..size {
        let mut entry = hold(format!("r{at:03}"));
        entry.skip_if = Some(deep(at % 2 == 0, &mut leaves)?);
        route.push(entry);
    }
    Ok(Declarative {
        route,
        handlers: Vec::new(),
        options: None,
    })
}

/// A `move` a unit, each to its own voxel, 64 to a row two voxels apart.
fn longest_route(size: usize) -> Result<Declarative, String> {
    if size == 0 || size > MAX_ROUTE_STEPS {
        return Err(format!(
            "a route of {size} steps, outside the interpreter's 1 to {MAX_ROUTE_STEPS}"
        ));
    }
    let mut route: Vec<Step> = Vec::with_capacity(size);
    for at in 0..size {
        let column = i32::try_from(at.rem_euclid(64)).map_err(|_| "a column".to_owned())?;
        let row = i32::try_from(at.div_euclid(64)).map_err(|_| "a row".to_owned())?;
        route.push(Step {
            label: format!("r{at:03}"),
            timeout_ms: 60_000,
            kind: Some(step::Kind::Move(MoveStep {
                to: Some(Location {
                    place: Some(location::Place::Voxel(pharmakos_proto::gp::v1::Voxel {
                        x: 100 + column * 2,
                        y: 100 + row * 4,
                        z: 30,
                    })),
                }),
                pace: i32::from(Pace::Direct),
            })),
            ..Step::default()
        });
    }
    Ok(Declarative {
        route,
        handlers: Vec::new(),
        options: None,
    })
}

/// The anchor columns of the target and area shapes, in order: every second
/// column within [`ANCHOR_REACH`] of the core and on the map (the core stands
/// at y = 11, so the rows below y = 0 are left out), rows from the lowest y,
/// columns from the lowest x, at the core's height.
fn anchors(count: usize) -> Result<Vec<pharmakos_proto::gp::v1::Voxel>, String> {
    let reach_squared = ANCHOR_REACH * ANCHOR_REACH;
    let mut found: Vec<pharmakos_proto::gp::v1::Voxel> = Vec::with_capacity(count);
    for dy in (-CORE_AT[1]..=ANCHOR_REACH).step_by(ANCHOR_STEP) {
        for dx in (-ANCHOR_REACH..=ANCHOR_REACH).step_by(ANCHOR_STEP) {
            if found.len() == count {
                return Ok(found);
            }
            if dx * dx + dy * dy <= reach_squared {
                found.push(pharmakos_proto::gp::v1::Voxel {
                    x: CORE_AT[0] + dx,
                    y: CORE_AT[1] + dy,
                    z: CORE_AT[2],
                });
            }
        }
    }
    if found.len() == count {
        return Ok(found);
    }
    Err(format!(
        "{count} anchors asked, and the core's sphere holds {} on this grid",
        found.len()
    ))
}

/// One interface step at the fixture's core carrying `rows`.
fn at_the_core(rows: Vec<InterfaceRow>) -> Declarative {
    Declarative {
        route: vec![Step {
            label: "r000".to_owned(),
            kind: Some(step::Kind::Interface(InterfaceStep {
                beacon: Some(BeaconRef {
                    r#ref: Some(beacon_ref::Ref::BeaconId(CORE_ID.to_owned())),
                }),
                rows,
            })),
            ..Step::default()
        }],
        handlers: Vec::new(),
        options: None,
    }
}

/// One interface step adding a Generator a unit past the step's own.
fn build_targets(size: usize) -> Result<Declarative, String> {
    let targets = size
        .checked_sub(1)
        .filter(|count| *count > 0)
        .ok_or_else(|| "a target shape needs two units: the step and a target".to_owned())?;
    let rows: Vec<InterfaceRow> = anchors(targets)?
        .into_iter()
        .enumerate()
        .map(|(index, anchor)| {
            Ok(InterfaceRow {
                row: Some(interface_row::Row::AddBuildTarget(AddBuildTargetRow {
                    target: Some(BuildTarget {
                        blueprint_id: "generator".to_owned(),
                        anchor: Some(Location {
                            place: Some(location::Place::Voxel(anchor)),
                        }),
                        order: u32::try_from(index).map_err(|_| "an order".to_owned())?,
                        ..BuildTarget::default()
                    }),
                })),
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(at_the_core(rows))
}

/// One interface step whose settings row lists an area a unit past the
/// step's own: each a one-voxel-high box of 2 x 2 columns at an anchor.
fn protected_areas(size: usize) -> Result<Declarative, String> {
    let areas = size
        .checked_sub(1)
        .filter(|count| *count > 0)
        .ok_or_else(|| "an area shape needs two units: the step and an area".to_owned())?;
    let protected: Vec<Area> = anchors(areas)?
        .into_iter()
        .map(|min| {
            let max = pharmakos_proto::gp::v1::Voxel {
                x: min.x + 1,
                y: min.y + 1,
                z: min.z,
            };
            Ok(Area {
                min: Some(min),
                max: Some(max),
            })
        })
        .collect::<Result<_, String>>()?;
    let settings = MandateSettings {
        mandate: Some(mandate_settings::Mandate::Build(BuildSettings {
            protected_areas: protected,
            ..BuildSettings::default()
        })),
        ..MandateSettings::default()
    };
    Ok(at_the_core(vec![InterfaceRow {
        row: Some(interface_row::Row::SetMandateSettings(settings)),
    }]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_is_exactly_its_size() {
        for shape in Shape::ALL {
            for units in SIZES.into_iter().chain([127, 129, 3]) {
                let playbook = shape
                    .playbook(units)
                    .unwrap_or_else(|error| panic!("{error}"));
                assert_eq!(
                    pharmakos_verifier::size_units(&playbook),
                    units,
                    "{} at {units}",
                    shape.name()
                );
            }
        }
    }

    #[test]
    fn the_handler_shape_at_127_is_item_157s_worst_case() {
        let playbook = Shape::Handlers24.playbook(127).expect("the shape");
        let declarative = playbook.declarative.expect("declarative");
        assert_eq!(declarative.handlers.len(), 63);
        assert_eq!(declarative.route.len(), 1);
        assert!(
            declarative
                .handlers
                .iter()
                .all(|handler| handler.body.len() == 1)
        );
    }

    #[test]
    fn a_shape_too_small_or_too_long_is_refused() {
        assert!(matches!(
            Shape::Handlers24.playbook(2),
            Err(Error::Shape { .. })
        ));
        assert!(matches!(
            Shape::RouteGuards24.playbook(257),
            Err(Error::Shape { .. })
        ));
        assert!(matches!(
            Shape::BuildTargets.playbook(400),
            Err(Error::Shape { .. })
        ));
        assert!(matches!(
            Shape::LongestRoute.playbook(0),
            Err(Error::Shape { .. })
        ));
    }

    #[test]
    fn the_bytes_are_a_pure_function_of_shape_and_size() {
        let first = Shape::RouteGuards24.bytes(64).expect("bytes");
        assert_eq!(first, Shape::RouteGuards24.bytes(64).expect("again"));
        assert_ne!(first, Shape::RouteGuards24.bytes(96).expect("another size"));
    }
}
