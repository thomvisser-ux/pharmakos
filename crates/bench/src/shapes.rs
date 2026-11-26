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
//! codec ([`pharmakos_proto::json::encode`]): the bytes the verifier is handed
//! whatever the player typed, since `plan-core`'s `verify_jsonc` canonicalises
//! a file before `Input::new` (`crates/plan-core/src/verify.rs`). The same
//! playbook as the player could send it, with no whitespace outside its
//! strings, is [`compact`]: the text the gateway's `MAX_PLAYBOOK_CHARS` caps,
//! and what the bench's end-to-end figure sends (`full`). Every number in
//! them is the harness's own input, fixed here, so a shape and a size
//! reproduce its bytes.
//!
//! # Timing-only inputs: the per-beacon target room
//!
//! [`Shape::BuildTargets`] and [`Shape::ProtectedAreas`] put every unit on
//! **one** beacon: up to 255 rows at 256 units. The verifier takes that (no
//! check counts a beacon's targets), but the sim holds 8 targets a beacon
//! (`TARGETS_PER_BEACON` in `crates/sim/src/world.rs`), so in a match every
//! row past the eighth fails its step. They are timing inputs for the
//! verifier's walks over targets and areas, never playbooks a seat would seal;
//! E17's rooms are `tune3`'s.
//!
//! # The decision tick's shapes
//!
//! [`Shape::Handlers24`] is the verifier's worst case, not the decision
//! tick's: its `all(all(all(..)))` handlers stop at their first false leaf,
//! and a leaf that comes true stops an `any`. [`Shape::FullEval`] is the
//! decision tick's: every handler is `any(any(any(21)))` over leaves that are
//! never true (`segment_elapsed EQ` a value no 50 ms tick reaches), so a
//! decision evaluates every node of every condition; its route steps are
//! `move`s, so the commander walks. [`Shape::FullEvalSelectors`] is the same
//! with selector leaves (`beacon_hp_pct` of the `nearest` own beacon below 0
//! per cent), each of which ranks the seat's beacons through the sim's
//! selector catalogue (`resolve_on`) when it is evaluated. Neither is in
//! [`Shape::ALL`], the verifier's curve.
//!
//! The decision tick's kinds (`crate::kinds`, item 196) take five more, none
//! in [`Shape::ALL`] either: [`Shape::Idle`], a route of holds that is the
//! baseline every kind is measured against; [`Shape::ConditionHolds`],
//! [`Shape::RankerHolds`] and [`Shape::ThreatHolds`], the full-evaluation
//! handlers over `segment_elapsed`, `nearest` and `safest` leaves with a route
//! of holds, so the commander stands still and every decision runs the kind;
//! and [`Shape::Toward`], `place_beacon` steps `toward` the nearest vent the
//! seat does not cover, each resolved when its step starts and deployed where
//! the commander walks.
//!
//! # The transport cap
//!
//! [`max_text`] is [`Shape::RouteGuards24`]'s structure at a size, its
//! labels and title padded until its compact text is exactly
//! `MAX_PLAYBOOK_CHARS` characters, in ASCII or in four-byte characters
//! ([`Pad`]): QUICK's cost at the transport cap, published and never a budget
//! failure.

use pharmakos_proto::gp::v1::condition::Node;
use pharmakos_proto::gp::v1::handler::Resume;
use pharmakos_proto::gp::v1::int_compare::Op;
use pharmakos_proto::gp::v1::meta::AuthorKind;
use pharmakos_proto::gp::v1::move_step::Pace;
use pharmakos_proto::gp::v1::on_death::OnRespawn;
use pharmakos_proto::gp::v1::playbook::Kind;
use pharmakos_proto::gp::v1::{
    AddBuildTargetRow, All, Any, Area, BeaconHpPct, BeaconRef, BuildSettings, BuildTarget,
    Condition, Declarative, Fallback, FallbackHold, FeatureRef, Handler, HoldStep, IntCompare,
    InterfaceRow, InterfaceStep, Location, MandateSettings, Meta, MoveStep, Nearest, OnDeath,
    OnFail, PlaceBeaconStep, Playbook, Safest, SchemaVersion, SegmentElapsed, Step, Toward,
    VentPick, beacon_ref, fallback, feature_ref, interface_row, location, mandate_settings,
    on_fail, step,
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

/// The map's extent the fixture's anchors are kept inside: the committed
/// rules' `map.size_x` and `map.size_y`, which `tests/harness.rs` holds to the
/// table.
pub const FIXTURE_EXTENT: [i32; 2] = [384, 384];

/// The core a shape's interface step visits and its anchors stand round.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Core {
    /// Its name, as the seat's view spells it.
    pub id: String,
    /// Its column and height.
    pub at: [i32; 3],
    /// The map's extent, `[x, y]`: no anchor is laid off it.
    pub extent: [i32; 2],
}

impl Core {
    /// The fixture's own core, `quick::fixture_scope`'s `b_01`: a Build beacon
    /// of seat 0 at [`crate::quick::FIXTURE_CORE_AT`], whose 24-voxel sphere
    /// the target and area shapes fill.
    #[must_use]
    pub fn fixture() -> Core {
        Core {
            id: "b_01".to_owned(),
            at: crate::quick::FIXTURE_CORE_AT,
            extent: FIXTURE_EXTENT,
        }
    }
}

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
    /// The decision tick's: handlers evaluated in full every decision, and
    /// `move` route steps (see the module docs). Not in [`Shape::ALL`].
    FullEval,
    /// [`Shape::FullEval`] with selector leaves. Not in [`Shape::ALL`].
    FullEvalSelectors,
    /// A route of one-second holds and nothing else: the decision tick's
    /// baseline, a playbook that asks a decision for almost nothing
    /// (`crate::kinds`). Not in [`Shape::ALL`].
    Idle,
    /// The condition kind's: [`Shape::FullEval`]'s handlers over a route of
    /// holds, so the commander stands still. Not in [`Shape::ALL`].
    ConditionHolds,
    /// The served-walk ranker's: the same with `nearest` leaves, each ranking
    /// the seat's beacons by the walk the router serves. Not in
    /// [`Shape::ALL`].
    RankerHolds,
    /// The threat-ranking selector's: the same with `safest` leaves, each
    /// ranking the seat's beacons through the threat scan. Not in
    /// [`Shape::ALL`].
    ThreatHolds,
    /// A route of `place_beacon` steps, each `toward` the nearest vent the
    /// seat does not cover: a stepping stone a unit, each resolved when its
    /// step starts and deployed where the commander walks (S3's `pred`, E23).
    /// Not in [`Shape::ALL`].
    Toward,
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
            Shape::FullEval => "full_eval",
            Shape::FullEvalSelectors => "full_eval_selectors",
            Shape::Idle => "idle",
            Shape::ConditionHolds => "condition_holds",
            Shape::RankerHolds => "ranker_holds",
            Shape::ThreatHolds => "threat_holds",
            Shape::Toward => "toward",
        }
    }

    /// How many handlers the shape holds at `units`: the size's share, past
    /// 129 units capped by the interpreter's `MAX_HANDLERS`, which is why a
    /// handler shape's curve flattens there.
    ///
    /// # Errors
    ///
    /// As [`Shape::playbook`].
    pub fn handlers_at(self, units: u32) -> Result<usize, Error> {
        self.playbook(units)?
            .declarative
            .map(|declarative| declarative.handlers.len())
            .ok_or_else(|| Error::Shape {
                shape: self.name(),
                units,
                reason: "a generated shape with no declarative block".to_owned(),
            })
    }

    /// The shape at `units` size units, as a message.
    ///
    /// # Errors
    ///
    /// [`Error::Shape`] when the shape cannot be laid at that size, and
    /// [`Error::Overflow`] for a coordinate or a duration past its type.
    pub fn playbook(self, units: u32) -> Result<Playbook, Error> {
        self.playbook_at(units, &Core::fixture())
    }

    /// The shape at `units` size units, its interface steps visiting `core`
    /// and its anchors laid round it: a hosted seat's own core for the
    /// end-to-end figure.
    ///
    /// # Errors
    ///
    /// As [`Shape::playbook`].
    pub fn playbook_at(self, units: u32, core: &Core) -> Result<Playbook, Error> {
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
            Shape::BuildTargets => build_targets(size, core).map_err(refuse)?,
            Shape::ProtectedAreas => protected_areas(size, core).map_err(refuse)?,
            Shape::FullEval => {
                handlers(size, Group::FullAny(LeafKind::Elapsed), Bodies::Moves).map_err(refuse)?
            }
            Shape::FullEvalSelectors => {
                handlers(size, Group::FullAny(LeafKind::Selector), Bodies::Moves).map_err(refuse)?
            }
            Shape::Idle => idle(size).map_err(refuse)?,
            Shape::ConditionHolds => {
                handlers(size, Group::FullAny(LeafKind::Elapsed), Bodies::One).map_err(refuse)?
            }
            Shape::RankerHolds => {
                handlers(size, Group::FullAny(LeafKind::Selector), Bodies::One).map_err(refuse)?
            }
            Shape::ThreatHolds => {
                handlers(size, Group::FullAny(LeafKind::Safest), Bodies::One).map_err(refuse)?
            }
            Shape::Toward => stepping_stones(size).map_err(refuse)?,
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
        canonical(&self.playbook(units)?, self.name()).map(String::into_bytes)
    }
}

/// `playbook` as canonical JSON text.
///
/// # Errors
///
/// [`Error::Codec`] when the canonical codec refuses the message.
pub fn canonical(playbook: &Playbook, what: &str) -> Result<String, Error> {
    json::encode(playbook).map_err(|error| Error::Codec {
        what: format!("encoding {what}"),
        reason: error.to_string(),
    })
}

/// JSON text with every whitespace character outside its strings removed:
/// the same document, as compact as a player could send it. Inside a string
/// nothing changes, escapes included.
#[must_use]
pub fn compact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for character in text.chars() {
        if in_string {
            out.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
        } else if character == '"' {
            in_string = true;
            out.push(character);
        } else if !character.is_whitespace() {
            out.push(character);
        }
    }
    out
}

/// What [`max_text`] pads with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pad {
    /// ASCII `x`: one byte a character.
    Ascii,
    /// U+1F9EA, four bytes a character in UTF-8.
    FourByte,
}

impl Pad {
    /// The character.
    #[must_use]
    pub const fn character(self) -> char {
        match self {
            Pad::Ascii => 'x',
            Pad::FourByte => '\u{1F9EA}',
        }
    }

    /// The variant's name, as notices print it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Pad::Ascii => "ascii",
            Pad::FourByte => "four_byte",
        }
    }
}

/// [`Shape::RouteGuards24`] at `units`, its step labels and its title padded
/// with `pad` until its [`compact`] text is exactly `cap` characters (the
/// gateway's `MAX_PLAYBOOK_CHARS`): the costliest text the transport takes at
/// that size.
///
/// # Errors
///
/// As [`Shape::playbook`]; [`Error::Shape`] when the unpadded text is already
/// past `cap`, or the padded text does not land exactly on it.
pub fn max_text(units: u32, pad: Pad, cap: usize) -> Result<Playbook, Error> {
    let refuse = |reason: String| Error::Shape {
        shape: "max_text",
        units,
        reason,
    };
    let mut playbook = Shape::RouteGuards24.playbook(units)?;
    let base = compact(&canonical(&playbook, "the max_text shape")?)
        .chars()
        .count();
    let extra = cap
        .checked_sub(base)
        .ok_or_else(|| refuse(format!("{base} characters unpadded, past the cap of {cap}")))?;
    let steps = playbook
        .declarative
        .as_ref()
        .map(|declarative| declarative.route.len())
        .ok_or_else(|| refuse("no declarative block".to_owned()))?;
    // A share for every label and the title, the remainder on the title.
    let shares = steps
        .checked_add(1)
        .ok_or_else(|| refuse("the labels".to_owned()))?;
    let each = extra.div_euclid(shares);
    let rest = extra.rem_euclid(shares);
    let padding =
        |count: usize| -> String { std::iter::repeat_n(pad.character(), count).collect() };
    if let Some(declarative) = playbook.declarative.as_mut() {
        for entry in &mut declarative.route {
            entry.label.push_str(&padding(each));
        }
    }
    if let Some(meta) = playbook.meta.as_mut() {
        meta.title.push_str(&padding(each + rest));
    }
    let length = compact(&canonical(&playbook, "the max_text shape")?)
        .chars()
        .count();
    if length != cap {
        return Err(refuse(format!(
            "padded to {length} characters, not the cap of {cap}"
        )));
    }
    Ok(playbook)
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
    /// `any(any(any(21 leaves that are never true)))`: 24 nodes, every one
    /// evaluated every decision.
    FullAny(LeafKind),
}

/// The never-true leaves of [`Group::FullAny`].
#[derive(Clone, Copy, Debug)]
enum LeafKind {
    /// `segment_elapsed EQ` a value that is not a multiple of the 50 ms tick.
    Elapsed,
    /// `beacon_hp_pct` of the `nearest` own beacon, below 0 per cent.
    Selector,
    /// `beacon_hp_pct` of the `safest` own beacon, below 0 per cent.
    Safest,
}

/// A leaf of [`Group::FullAny`]: never true, and distinct from every other.
fn never(kind: LeafKind, leaves: &mut Leaves) -> Result<Condition, String> {
    leaves.next = leaves
        .next
        .checked_add(1)
        .ok_or_else(|| "more leaves than an i32 counts".to_owned())?;
    Ok(match kind {
        // n * 50 + 1 ms: never a tick's elapsed time.
        LeafKind::Elapsed => node(Node::SegmentElapsed(SegmentElapsed {
            ms: Some(IntCompare {
                op: i32::from(Op::Eq),
                value: leaves
                    .next
                    .checked_mul(50)
                    .and_then(|ms| ms.checked_add(1))
                    .ok_or_else(|| "a leaf's milliseconds".to_owned())?,
            }),
        })),
        LeafKind::Selector => node(Node::BeaconHpPct(BeaconHpPct {
            beacon: Some(BeaconRef {
                r#ref: Some(beacon_ref::Ref::Nearest(Nearest::default())),
            }),
            pct: Some(IntCompare {
                op: i32::from(Op::Lt),
                value: 0,
            }),
        })),
        LeafKind::Safest => node(Node::BeaconHpPct(BeaconHpPct {
            beacon: Some(BeaconRef {
                r#ref: Some(beacon_ref::Ref::Safest(Safest::default())),
            }),
            pct: Some(IntCompare {
                op: i32::from(Op::Lt),
                value: 0,
            }),
        })),
    })
}

/// How many body steps a handler holds.
#[derive(Clone, Copy, Debug)]
enum Bodies {
    /// One each; the units left over go to route steps.
    One,
    /// Every unit past the one route step goes to handlers and their bodies.
    Fill,
    /// One each, and the units left over go to `move` route steps.
    Moves,
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
        Bodies::One | Bodies::Moves => (left, 0),
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
            Group::FullAny(kind) => {
                let inner: Vec<Condition> = (0..LEAVES_PER_CONDITION)
                    .map(|_| never(kind, &mut leaves))
                    .collect::<Result<_, _>>()?;
                let any = |items: Vec<Condition>| node(Node::Any(Any { items }));
                any(vec![any(vec![any(inner)])])
            }
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
    let route = match bodies {
        Bodies::Moves => moves(route_steps)?,
        Bodies::One | Bodies::Fill => holds(route_steps),
    };
    Ok(Declarative {
        route,
        handlers: list,
        options: None,
    })
}

/// A route of `size` one-second holds, `r000` on, and no handler.
fn idle(size: usize) -> Result<Declarative, String> {
    if size == 0 || size > MAX_ROUTE_STEPS {
        return Err(format!(
            "a route of {size} steps, outside the interpreter's 1 to {MAX_ROUTE_STEPS}"
        ));
    }
    Ok(Declarative {
        route: holds(size),
        handlers: Vec::new(),
        options: None,
    })
}

/// `size` `place_beacon` steps, `r000` on, each `toward` the nearest vent the
/// seat does not cover, skipped when it fails so the next one starts.
fn stepping_stones(size: usize) -> Result<Declarative, String> {
    if size == 0 || size > MAX_ROUTE_STEPS {
        return Err(format!(
            "a route of {size} steps, outside the interpreter's 1 to {MAX_ROUTE_STEPS}"
        ));
    }
    let route = (0..size)
        .map(|at| Step {
            label: format!("r{at:03}"),
            timeout_ms: 120_000,
            on_fail: Some(OnFail {
                action: i32::from(on_fail::Action::Skip),
                jump_to_label: String::new(),
            }),
            kind: Some(step::Kind::PlaceBeacon(PlaceBeaconStep {
                at: Some(Location {
                    place: Some(location::Place::Toward(Toward {
                        feature: Some(FeatureRef {
                            r#ref: Some(feature_ref::Ref::Vent(VentPick {
                                rank: i32::from(feature_ref::Rank::Nearest),
                                coverage: i32::from(feature_ref::Coverage::Uncovered),
                            })),
                        }),
                    })),
                }),
                ..PlaceBeaconStep::default()
            })),
            ..Step::default()
        })
        .collect();
    Ok(Declarative {
        route,
        handlers: Vec::new(),
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
    Ok(Declarative {
        route: moves(size)?,
        handlers: Vec::new(),
        options: None,
    })
}

/// `size` route steps, `r000` on, each a `move` to a voxel of its own, 64 to
/// a row two voxels apart.
fn moves(size: usize) -> Result<Vec<Step>, String> {
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
    Ok(route)
}

/// The anchor columns of the target and area shapes, in order: every second
/// column within [`ANCHOR_REACH`] of the core and on the map, with a column to
/// spare for an area's far corner (the fixture's core stands at y = 11, so its
/// rows below y = 0 are left out), rows from the lowest y, columns from the
/// lowest x, at the core's height.
fn anchors(count: usize, core: &Core) -> Result<Vec<pharmakos_proto::gp::v1::Voxel>, String> {
    let reach_squared = ANCHOR_REACH * ANCHOR_REACH;
    let [cx, cy, cz] = core.at;
    let on_map =
        |x: i32, y: i32| x >= 0 && y >= 0 && x + 1 < core.extent[0] && y + 1 < core.extent[1];
    let mut found: Vec<pharmakos_proto::gp::v1::Voxel> = Vec::with_capacity(count);
    for dy in (-ANCHOR_REACH..=ANCHOR_REACH).step_by(ANCHOR_STEP) {
        for dx in (-ANCHOR_REACH..=ANCHOR_REACH).step_by(ANCHOR_STEP) {
            if found.len() == count {
                return Ok(found);
            }
            if dx * dx + dy * dy <= reach_squared && on_map(cx + dx, cy + dy) {
                found.push(pharmakos_proto::gp::v1::Voxel {
                    x: cx + dx,
                    y: cy + dy,
                    z: cz,
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

/// One interface step at `core` carrying `rows`.
fn at_the_core(rows: Vec<InterfaceRow>, core: &Core) -> Declarative {
    Declarative {
        route: vec![Step {
            label: "r000".to_owned(),
            kind: Some(step::Kind::Interface(InterfaceStep {
                beacon: Some(BeaconRef {
                    r#ref: Some(beacon_ref::Ref::BeaconId(core.id.clone())),
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
fn build_targets(size: usize, core: &Core) -> Result<Declarative, String> {
    let targets = size
        .checked_sub(1)
        .filter(|count| *count > 0)
        .ok_or_else(|| "a target shape needs two units: the step and a target".to_owned())?;
    let rows: Vec<InterfaceRow> = anchors(targets, core)?
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
    Ok(at_the_core(rows, core))
}

/// One interface step whose settings row lists an area a unit past the
/// step's own: each a one-voxel-high box of 2 x 2 columns at an anchor.
fn protected_areas(size: usize, core: &Core) -> Result<Declarative, String> {
    let areas = size
        .checked_sub(1)
        .filter(|count| *count > 0)
        .ok_or_else(|| "an area shape needs two units: the step and an area".to_owned())?;
    let protected: Vec<Area> = anchors(areas, core)?
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
    Ok(at_the_core(
        vec![InterfaceRow {
            row: Some(interface_row::Row::SetMandateSettings(settings)),
        }],
        core,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_is_exactly_its_size() {
        for shape in Shape::ALL.into_iter().chain([
            Shape::FullEval,
            Shape::FullEvalSelectors,
            Shape::Idle,
            Shape::ConditionHolds,
            Shape::RankerHolds,
            Shape::ThreatHolds,
            Shape::Toward,
        ]) {
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
    fn compact_text_drops_only_the_whitespace_outside_strings() {
        let text = "{\n  \"a b\": [1, 2],\n  \"c\": \"x \\\" y\"\n}\n";
        assert_eq!(compact(text), "{\"a b\":[1,2],\"c\":\"x \\\" y\"}");
        let canonical =
            String::from_utf8(Shape::RouteGuards24.bytes(64).expect("bytes")).expect("utf-8");
        let decoded: Playbook = json::decode(&compact(&canonical)).expect("compact decodes");
        assert_eq!(
            decoded,
            Shape::RouteGuards24.playbook(64).expect("the shape")
        );
    }

    #[test]
    fn the_max_text_shape_lands_exactly_on_the_cap() {
        for pad in [Pad::Ascii, Pad::FourByte] {
            let playbook = max_text(128, pad, 400_000).expect("the shape");
            let text = compact(&canonical(&playbook, "test").expect("encodes"));
            assert_eq!(text.chars().count(), 400_000, "{pad:?}");
            assert_eq!(pharmakos_verifier::size_units(&playbook), 128);
        }
        assert!(
            max_text(128, Pad::Ascii, 1_000).is_err(),
            "a cap below the text"
        );
    }

    #[test]
    fn the_decision_shapes_are_any_over_never_true_leaves_and_walk() {
        let playbook = Shape::FullEval.playbook(128).expect("the shape");
        let declarative = playbook.declarative.expect("declarative");
        assert_eq!(declarative.handlers.len(), 63);
        assert!(
            declarative
                .route
                .iter()
                .all(|entry| matches!(entry.kind, Some(step::Kind::Move(_))))
        );
        let text = String::from_utf8(Shape::FullEval.bytes(128).expect("bytes")).expect("utf-8");
        assert!(!text.contains("\"all\""), "every group is an `any`");
        assert_eq!(
            Shape::FullEval.handlers_at(256).expect("handlers"),
            64,
            "MAX_HANDLERS"
        );
    }

    #[test]
    fn the_bytes_are_a_pure_function_of_shape_and_size() {
        let first = Shape::RouteGuards24.bytes(64).expect("bytes");
        assert_eq!(first, Shape::RouteGuards24.bytes(64).expect("again"));
        assert_ne!(first, Shape::RouteGuards24.bytes(96).expect("another size"));
    }
}
