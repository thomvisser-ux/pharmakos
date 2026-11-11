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
//! | A handler an earlier one wins every time both are true | `W0702` | **S3**, here (task `vfy5`) |
//! | A reference to an enemy beacon resting on a sighting older than the reach window, an Attack target aside (`E0418` refuses a stale one, `W0708` warns of a seatless commander) | `W0703` | **S2**, here (task `vfy2`) |
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
//!
//! # A rule an earlier rule always wins (`W0702`, S3's `vfy5`)
//!
//! The sim tries the handlers in order at a decision tick and starts the
//! first ready one whose condition is true (`try_fire`); no handler pre-empts
//! another. So a later rule whose condition **implies** an earlier one's gets
//! a turn only while the earlier one is cooling down or has spent its fires.
//! The test is conservative and reads the two trees alone, never a projected
//! world: a later condition implies an earlier one when the trees are equal,
//! through `all`, `any` and `not` taken apart in the ways that keep the claim
//! true, and at the leaves where one predicate reads the same thing with a
//! tighter constant (an integer comparison whose values are a subset of the
//! other's, or a damage window inside the other's). Anything it cannot show
//! it does not claim. A condition past spec section 10's limits (`E0204`,
//! `E0205`) and a playbook past the sim's handler ceiling (`E0113`) are
//! refused already and are not read.
//!
//! **The search's cost is bounded by the memo, not by the shapes.** Each
//! condition is laid out once, in preorder, and one pair of rules is searched
//! with a memo keyed on (a node of the later condition, a node of the
//! earlier, and a direction, since `not` asks the question the other way
//! round), so a pair of rules decides at most `2 × 24 × 24 = 1 152`
//! questions, each once, whatever the trees look like. A playbook holds at
//! most the sim's 64 handlers, so at most `64 × 63 / 2 = 2 016` pairs and
//! `2 322 432` questions in all: a bound on the whole search, not a figure
//! per pair. A search clears only the slots the last one filled, so its cost
//! is what it decides. `the_worst_case_is_bounded_by_the_memo` counts the
//! worst legal shape the reviews found, 63 rules of 24 nodes each over
//! distinct leaves: 315 115 questions. Before the memo a model search found
//! 9 469 calls for one pair of shapes inside the limits (a figure per pair,
//! with no bound on the whole), and the reviews measured that playbook at
//! 87 ms FULL p99.

use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::v1::cmdr_hp_pct::Cmp;
use pharmakos_proto::gp::v1::feature_ref::{self, Coverage, Rank};
use pharmakos_proto::gp::v1::int_compare::Op;
use pharmakos_proto::gp::v1::{
    BeaconRef, Condition, Handler, IntCompare, Location, Playbook, Step, Voxel, beacon_ref,
    condition, location, step,
};
use pharmakos_proto::json::Json;

use crate::limits::{CONDITION_MAX_DEPTH, CONDITION_MAX_NODES, Limits};
use crate::pointer;
use crate::report::{Builder, Diag, patch_replace};
use crate::resolve::Symbols;
use crate::scope::{FeatureKind, KnownFeature, Ownership, Scope};
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
    shadowed(playbook, out);
}

/// `W0702`: each handler whose condition implies an earlier handler's, named
/// against the first such earlier one.
fn shadowed(playbook: &Playbook, out: &mut Builder) {
    let Some(declarative) = playbook.declarative.as_ref() else {
        return;
    };
    let (found, _) = shadows(&declarative.handlers);
    let at = |index: usize| pointer::child(&pointer::at("/declarative/handlers", index), "when");
    for (later_index, earlier_index) in found {
        let (Some(later), Some(earlier)) = (
            declarative.handlers.get(later_index),
            declarative.handlers.get(earlier_index),
        ) else {
            continue;
        };
        out.emit(
            Diag::new("W0702", at(later_index))
                .arg("id", &later.id)
                .arg("winner", &earlier.id)
                .related(at(earlier_index)),
        );
    }
}

/// Every `(later, earlier)` pair of handler indices W0702 names, and the
/// search's counted work: the node pairs its memos answered, summed over every
/// pair of rules it searched.
fn shadows(handlers: &[Handler]) -> (Vec<(usize, usize)>, usize) {
    let mut found: Vec<(usize, usize)> = Vec::new();
    let mut work: usize = 0;
    if handlers.len() > pharmakos_sim::interpreter::MAX_HANDLERS {
        // `E0113`'s: the sim compiles none of them.
        return (found, work);
    }
    // Each condition is judged and laid out once, not once per pair.
    let trees: Vec<Option<Tree<'_>>> = handlers
        .iter()
        .map(|handler| judged(handler).map(Tree::of))
        .collect();
    let mut memo = Memo::default();
    for (later_index, later) in trees.iter().enumerate() {
        let Some(later) = later.as_ref() else {
            continue;
        };
        for (earlier_index, earlier) in trees.iter().enumerate().take(later_index) {
            let Some(earlier) = earlier.as_ref() else {
                continue;
            };
            let mut search = Search::new(later, earlier, &mut memo);
            let shadowed = search.root();
            work = work.saturating_add(search.answered());
            if shadowed {
                found.push((later_index, earlier_index));
                break;
            }
        }
    }
    (found, work)
}

/// A handler's condition, when it is one this lint reads: present, and
/// inside spec section 10's depth and node limits (past them it is `E0204`
/// or `E0205`, and the sim compiles none of it).
fn judged(handler: &Handler) -> Option<&Condition> {
    let when = handler.when.as_ref()?;
    let inside = crate::structure::tree_depth(when) <= CONDITION_MAX_DEPTH
        && crate::structure::tree_nodes(when) <= CONDITION_MAX_NODES;
    inside.then_some(when)
}

/// One condition laid out in preorder: each node, and the indices of its
/// children (a group's items, a `not`'s item). The root is index 0.
struct Tree<'a> {
    nodes: Vec<&'a Condition>,
    children: Vec<Vec<usize>>,
}

impl<'a> Tree<'a> {
    fn of(root: &'a Condition) -> Tree<'a> {
        let mut tree = Tree {
            nodes: Vec::new(),
            children: Vec::new(),
        };
        tree.push(root);
        tree
    }

    /// Lay out `node` and everything under it, and answer its index.
    fn push(&mut self, node: &'a Condition) -> usize {
        let index = self.nodes.len();
        self.nodes.push(node);
        self.children.push(Vec::new());
        let items: Vec<&'a Condition> = match node.node.as_ref() {
            Some(condition::Node::All(all)) => all.items.iter().collect(),
            Some(condition::Node::Any(any)) => any.items.iter().collect(),
            Some(condition::Node::Not(not)) => not.item.as_deref().into_iter().collect(),
            _ => Vec::new(),
        };
        let mut below: Vec<usize> = Vec::with_capacity(items.len());
        for item in items {
            below.push(self.push(item));
        }
        if let Some(slot) = self.children.get_mut(index) {
            *slot = below;
        }
        index
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }
}

/// One pair of rules' search: whether the later condition (`a`) implies the
/// earlier (`b`), memoised on (a direction, a node of `a`, a node of `b`), so
/// each question is decided once. The direction is there for `not`: "`not x`
/// gives `not y`" is "`y` gives `x`", a question the other way round over the
/// same two trees.
struct Search<'s, 'a> {
    a: &'s Tree<'a>,
    b: &'s Tree<'a>,
    memo: &'s mut Memo,
}

/// The answers one search has decided, kept across searches so one
/// allocation serves every pair of rules: a slot per question, and the slots
/// this search filled, which are the only ones the next search clears and
/// the search's counted work.
#[derive(Default)]
struct Memo {
    slots: Vec<Option<bool>>,
    filled: Vec<usize>,
}

impl<'s, 'a> Search<'s, 'a> {
    /// A search over a memo of two slots per node pair, one each way, with
    /// the last search's answers cleared: only the slots it filled, so a
    /// search costs what it decides and not what the trees could hold.
    fn new(a: &'s Tree<'a>, b: &'s Tree<'a>, memo: &'s mut Memo) -> Search<'s, 'a> {
        for slot in memo.filled.drain(..) {
            if let Some(cell) = memo.slots.get_mut(slot) {
                *cell = None;
            }
        }
        let wanted = a.len().saturating_mul(b.len()).saturating_mul(2);
        if memo.slots.len() < wanted {
            memo.slots.resize(wanted, None);
        }
        Search { a, b, memo }
    }

    /// How many questions this search decided: its counted work.
    fn answered(&self) -> usize {
        self.memo.filled.len()
    }

    /// Whether `a`'s root implies `b`'s.
    fn root(&mut self) -> bool {
        self.implies(false, 0, 0)
    }

    /// Whether node `l` implies node `r` at the same decision tick, as far as
    /// the two trees show: `l` of `a` and `r` of `b` forwards, `l` of `b` and
    /// `r` of `a` reversed. Sound and incomplete: every `true` is a proof, and
    /// a `false` only says none was found.
    fn implies(&mut self, reversed: bool, l: usize, r: usize) -> bool {
        let (ia, jb) = if reversed { (r, l) } else { (l, r) };
        let half = self.a.len().saturating_mul(self.b.len());
        let slot = ia
            .checked_mul(self.b.len())
            .and_then(|row| row.checked_add(jb))
            .and_then(|slot| {
                if reversed {
                    slot.checked_add(half)
                } else {
                    Some(slot)
                }
            });
        if let Some(Some(known)) = slot.and_then(|slot| self.memo.slots.get(slot).copied()) {
            return known;
        }
        let answer = self.decide(reversed, l, r);
        if let Some(slot) = slot
            && let Some(cell) = self.memo.slots.get_mut(slot)
        {
            *cell = Some(answer);
            self.memo.filled.push(slot);
        }
        answer
    }

    fn decide(&mut self, reversed: bool, l: usize, r: usize) -> bool {
        use condition::Node;
        let (lhs, rhs) = if reversed {
            (self.b, self.a)
        } else {
            (self.a, self.b)
        };
        let (Some(left), Some(right)) = (
            lhs.nodes.get(l).and_then(|node| node.node.as_ref()),
            rhs.nodes.get(r).and_then(|node| node.node.as_ref()),
        ) else {
            // An unset node is `E0206`'s and means nothing here.
            return false;
        };
        let (Some(below_l), Some(below_r)) = (lhs.children.get(l), rhs.children.get(r)) else {
            return false;
        };
        // Equal groups need no test of their own: taking them apart below
        // meets each item against its own copy. Only leaves are compared
        // whole.
        match (left, right) {
            // `right` holds when each of its items does.
            (_, Node::All(_)) => below_r.iter().all(|&item| self.implies(reversed, l, item)),
            // `left` holds only when one of its items does, so each must.
            (Node::Any(_), _) => below_l.iter().all(|&item| self.implies(reversed, item, r)),
            // `right` holds when one of its items does; or `left` holds only
            // when each of its items does, so one of them alone may be enough.
            (_, Node::Any(_)) => {
                below_r.iter().any(|&item| self.implies(reversed, l, item))
                    || (matches!(left, Node::All(_))
                        && below_l.iter().any(|&item| self.implies(reversed, item, r)))
            }
            (Node::All(_), _) => below_l.iter().any(|&item| self.implies(reversed, item, r)),
            // The contrapositive: `not x` gives `not y` when `y` gives `x`,
            // the question the other way round.
            (Node::Not(_), Node::Not(_)) => match (below_l.first(), below_r.first()) {
                (Some(&x), Some(&y)) => self.implies(!reversed, y, x),
                _ => false,
            },
            // **Equal leaves read the same value.** Sound only while no
            // predicate's value depends on which handler is evaluating it --
            // on "this rule itself" -- since the two are two handlers'
            // conditions evaluated at the same decision tick
            // (`cond::evaluate` over one view). Every predicate today reads
            // the seat's world and its playbook state, and `rule_fired`
            // names its handler by id, so the same leaf in two rules reads
            // the same thing. A predicate that read the evaluating rule's own
            // fires or cooldown would break this, and the lane that adds one
            // excludes it here.
            (left, right) => left == right || leaf_implies(left, right),
        }
    }
}

/// One predicate implying another of the same kind, reading the same thing
/// with a tighter constant. Equal predicates are answered by the caller.
fn leaf_implies(a: &condition::Node, b: &condition::Node) -> bool {
    use condition::Node;
    match (a, b) {
        (Node::CmdrHpPct(a), Node::CmdrHpPct(b)) => {
            match (hp_range(a.cmp, a.pct), hp_range(b.cmp, b.pct)) {
                (Some(a), Some(b)) => a.within(b),
                _ => false,
            }
        }
        (Node::CmdrTookDamageWithin(a), Node::CmdrTookDamageWithin(b)) => a.ms <= b.ms,
        (Node::CmdrDeaths(a), Node::CmdrDeaths(b)) => compare_implies(a.deaths, b.deaths),
        (Node::SegmentElapsed(a), Node::SegmentElapsed(b)) => compare_implies(a.ms, b.ms),
        (Node::BeaconHpPct(a), Node::BeaconHpPct(b)) => {
            a.beacon == b.beacon && compare_implies(a.pct, b.pct)
        }
        (Node::BeaconUnderAttack(a), Node::BeaconUnderAttack(b)) => {
            a.beacon == b.beacon && a.within_ms <= b.within_ms
        }
        (Node::Treasury(a), Node::Treasury(b)) => compare_implies(a.dollars, b.dollars),
        (Node::KwHeadroom(a), Node::KwHeadroom(b)) => compare_implies(a.kw, b.kw),
        _ => false,
    }
}

/// One integer comparison's values inside another's, both present.
fn compare_implies(a: Option<IntCompare>, b: Option<IntCompare>) -> bool {
    let range = |compare: Option<IntCompare>| {
        let compare = compare?;
        Values::of(Op::try_from(compare.op).ok()?, i64::from(compare.value))
    };
    match (range(a), range(b)) {
        (Some(a), Some(b)) => a.within(b),
        _ => false,
    }
}

/// `cmdr_hp_pct`'s own comparison, read as the one `IntCompare` spells: the
/// two enums name the same six operators with the same numbers
/// (`playbook.proto`'s `IntCompare` says so).
fn hp_range(cmp: i32, pct: u32) -> Option<Values> {
    let op = match Cmp::try_from(cmp).ok()? {
        Cmp::Unspecified => return None,
        Cmp::Lt => Op::Lt,
        Cmp::Le => Op::Le,
        Cmp::Eq => Op::Eq,
        Cmp::Ne => Op::Ne,
        Cmp::Ge => Op::Ge,
        Cmp::Gt => Op::Gt,
    };
    Values::of(op, i64::from(pct))
}

/// The integers one comparison holds for. Read over every integer, so a
/// claim made here holds whatever the predicate's own range is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Values {
    AtMost(i64),
    AtLeast(i64),
    Exactly(i64),
    Except(i64),
}

impl Values {
    /// The values `left op value` holds for, `None` for an unset operator
    /// (`E0201`'s). The value is a widened `i32` or `u32`, so one either side
    /// of it is an `i64`.
    fn of(op: Op, value: i64) -> Option<Values> {
        Some(match op {
            Op::Unspecified => return None,
            Op::Lt => Values::AtMost(value.checked_sub(1)?),
            Op::Le => Values::AtMost(value),
            Op::Eq => Values::Exactly(value),
            Op::Ne => Values::Except(value),
            Op::Ge => Values::AtLeast(value),
            Op::Gt => Values::AtLeast(value.checked_add(1)?),
        })
    }

    fn holds(self, x: i64) -> bool {
        match self {
            Values::AtMost(most) => x <= most,
            Values::AtLeast(least) => x >= least,
            Values::Exactly(only) => x == only,
            Values::Except(not) => x != not,
        }
    }

    /// Whether every value `self` holds for, `other` holds for too.
    fn within(self, other: Values) -> bool {
        match (self, other) {
            (Values::Exactly(only), _) => other.holds(only),
            (Values::AtMost(a), Values::AtMost(b)) => a <= b,
            (Values::AtLeast(a), Values::AtLeast(b)) => a >= b,
            (Values::AtMost(a), Values::Except(not)) => not > a,
            (Values::AtLeast(a), Values::Except(not)) => not < a,
            (Values::Except(a), Values::Except(b)) => a == b,
            _ => false,
        }
    }
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

    fn beacon(&mut self, at: &str, reference: &BeaconRef) {
        self.stale(at, reference);
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
        if let Some((list, index)) = self.current
            && let Some(earlier) = self.placements.before(list, index)
        {
            self.out
                .emit(Diag::new("W0705", here).arg("what", what).related(earlier));
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
    /// `W0703`: a reference to an enemy beacon by its handle, resting on a
    /// sighting older than the reach window (spec section 10's one window).
    ///
    /// An Attack target is not advised here: its freshness is a qualification
    /// test (spec section 6), so the semantics stage refuses a stale one as
    /// `E0418` and saying it twice helps nobody. A handle the view holds no
    /// sighting for says nothing here either: there is no age to compare, and
    /// a name the view does not hold at all is `E0401`'s. Ages are read and
    /// compared; nothing is ranked.
    fn stale(&mut self, at: &str, reference: &BeaconRef) {
        let Some(beacon_ref::Ref::BeaconId(id)) = reference.r#ref.as_ref() else {
            return;
        };
        if at.ends_with("/attack/target/beacon") {
            return;
        }
        let Some(known) = self.scope.beacon(id) else {
            return;
        };
        if known.side != Ownership::EnemyKnown {
            return;
        }
        let Some(sighting) = self.scope.beacon_sighting(id) else {
            return;
        };
        let window = i64::from(self.limits.reach_memory_ms());
        if i64::from(sighting.age_ms) <= window {
            return;
        }
        self.out.emit(
            Diag::new("W0703", pointer::child(at, "beacon_id"))
                .arg("found", sighting.age_ms)
                .arg("max", window)
                .map_ref(sighting.at),
        );
    }

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
        // Only a covered vent is ever taken as `first`, so the beacon is there;
        // reading it in the same pattern leaves no empty name to fall back on.
        let Some((vent, beacon)) =
            first.and_then(|vent| vent.covered_by.as_deref().map(|beacon| (vent, beacon)))
        else {
            return;
        };
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

#[cfg(test)]
mod tests {
    //! `W0702`'s search, counted and checked against a reference evaluator
    //! (S3's `vfy5`, and its two reviews).

    use super::{Search, Tree, shadows};
    use pharmakos_proto::gp::v1::cmdr_hp_pct::Cmp;
    use pharmakos_proto::gp::v1::int_compare::Op;
    use pharmakos_proto::gp::v1::{
        All, Any, CmdrHpPct, CmdrTookDamageWithin, Condition, Handler, IntCompare, KwHeadroom, Not,
        SegmentElapsed, Treasury, condition,
    };

    fn node(node: condition::Node) -> Condition {
        Condition { node: Some(node) }
    }

    fn elapsed(op: Op, value: i32) -> Condition {
        node(condition::Node::SegmentElapsed(SegmentElapsed {
            ms: Some(IntCompare {
                op: i32::from(op),
                value,
            }),
        }))
    }

    fn rule(id: usize, when: Condition) -> Handler {
        Handler {
            id: format!("h{id:02}"),
            when: Some(when),
            ..Handler::default()
        }
    }

    /// `group(group(group(21 leaves)))`: 24 nodes, four levels, the most
    /// spec section 10 allows of both.
    fn deep(any: bool, leaves: impl Iterator<Item = Condition>) -> Condition {
        let wrap = |items: Vec<Condition>| {
            if any {
                node(condition::Node::Any(Any { items }))
            } else {
                node(condition::Node::All(All { items }))
            }
        };
        wrap(vec![wrap(vec![wrap(leaves.collect())])])
    }

    /// The review's worst legal shape: 63 handlers whose conditions are
    /// `any(any(any(21)))` and `all(all(all(21)))` in turn, over distinct
    /// `segment_elapsed EQ` leaves, so no pair implies another and every
    /// pair is searched to the end. The work is the questions the memos
    /// answered: at most `2 × 24 × 24` a pair of rules, and in all at most
    /// that times the pairs. No clock: this crate is not walled.
    #[test]
    fn the_worst_case_is_bounded_by_the_memo() {
        let mut next: i32 = 0;
        let handlers: Vec<Handler> = (0..63)
            .map(|id| {
                let leaves: Vec<Condition> = (0..21)
                    .map(|_| {
                        next = next.saturating_add(1);
                        elapsed(Op::Eq, next)
                    })
                    .collect();
                rule(id, deep(id % 2 == 0, leaves.into_iter()))
            })
            .collect();
        let tree = handlers
            .first()
            .and_then(|handler| handler.when.as_ref())
            .map_or_else(|| panic!("the first rule has a condition"), Tree::of);
        assert_eq!(tree.len(), 24, "the shape is the limit's");
        let (found, work) = shadows(&handlers);
        assert!(found.is_empty(), "distinct leaves imply nothing: {found:?}");
        // 63 rules, each searched against every rule before it.
        let pairs: usize = (1..63).sum();
        let per_pair = 2 * 24 * 24;
        assert!(
            work <= pairs * per_pair,
            "{work} questions for {pairs} pairs of rules, past the memo's bound"
        );
        // The figure itself, so a change that costs more is seen.
        assert_eq!(work, 315_115, "the counted work moved");
    }

    /// A seeded generator of small conditions over integer leaves, and a
    /// reference evaluator over sampled valuations: wherever the search says
    /// a later condition implies an earlier one, every sampled valuation that
    /// satisfies the later satisfies the earlier. No new dependency: the
    /// generator is a 64-bit linear congruential one.
    #[test]
    fn every_implication_holds_on_every_sampled_valuation() {
        let mut rng = Lcg(0x5644_5935_5F57_3037);
        let mut proved: u32 = 0;
        for _ in 0..4_000 {
            let later = gen_condition(&mut rng, 3);
            let earlier = gen_condition(&mut rng, 3);
            let (a, b) = (Tree::of(&later), Tree::of(&earlier));
            let mut memo = super::Memo::default();
            if !Search::new(&a, &b, &mut memo).root() {
                continue;
            }
            proved = proved.saturating_add(1);
            for _ in 0..200 {
                let world = Valuation::sample(&mut rng);
                assert!(
                    !world.holds(&later) || world.holds(&earlier),
                    "{later:?} was said to imply {earlier:?}, and {world:?} breaks it"
                );
            }
        }
        assert!(
            proved > 200,
            "only {proved} implications were proved, so the property says little"
        );
    }

    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }

        /// A number below `n`, which is small.
        fn below(&mut self, n: u64) -> u64 {
            self.next().checked_rem(n).unwrap_or(0)
        }

        fn small(&mut self) -> i32 {
            i32::try_from(self.below(9)).unwrap_or(0)
        }
    }

    const OPS: [Op; 6] = [Op::Lt, Op::Le, Op::Eq, Op::Ne, Op::Ge, Op::Gt];
    const CMPS: [Cmp; 6] = [Cmp::Lt, Cmp::Le, Cmp::Eq, Cmp::Ne, Cmp::Ge, Cmp::Gt];

    fn op(rng: &mut Lcg) -> Op {
        let index = usize::try_from(rng.below(6)).unwrap_or(0);
        OPS.get(index).copied().unwrap_or(Op::Eq)
    }

    fn compare(rng: &mut Lcg) -> IntCompare {
        IntCompare {
            op: i32::from(op(rng)),
            value: rng.small(),
        }
    }

    fn gen_condition(rng: &mut Lcg, depth: u32) -> Condition {
        if depth > 0 && rng.below(3) == 0 {
            let count = rng.below(3).saturating_add(1);
            let items: Vec<Condition> = (0..count)
                .map(|_| gen_condition(rng, depth.saturating_sub(1)))
                .collect();
            return match rng.below(3) {
                0 => node(condition::Node::All(All { items })),
                1 => node(condition::Node::Any(Any { items })),
                _ => node(condition::Node::Not(Box::new(Not {
                    item: items.into_iter().next().map(Box::new),
                }))),
            };
        }
        match rng.below(5) {
            0 => node(condition::Node::SegmentElapsed(SegmentElapsed {
                ms: Some(compare(rng)),
            })),
            1 => node(condition::Node::Treasury(Treasury {
                dollars: Some(compare(rng)),
            })),
            2 => node(condition::Node::KwHeadroom(KwHeadroom {
                kw: Some(compare(rng)),
            })),
            3 => {
                let index = usize::try_from(rng.below(6)).unwrap_or(0);
                node(condition::Node::CmdrHpPct(CmdrHpPct {
                    cmp: i32::from(CMPS.get(index).copied().unwrap_or(Cmp::Eq)),
                    pct: u32::try_from(rng.small()).unwrap_or(0),
                }))
            }
            _ => node(condition::Node::CmdrTookDamageWithin(
                CmdrTookDamageWithin { ms: rng.small() },
            )),
        }
    }

    /// One sampled world: the integer each leaf reads, and how long ago the
    /// commander was last hit (`None`: never), on a range a little wider than
    /// the generator's constants so every comparison is tested both ways.
    #[derive(Debug)]
    struct Valuation {
        elapsed: i64,
        treasury: i64,
        headroom: i64,
        hp: i64,
        hit_ago: Option<i64>,
    }

    impl Valuation {
        fn sample(rng: &mut Lcg) -> Valuation {
            let mut value = || i64::from(rng.small()).saturating_sub(1);
            let (elapsed, treasury, headroom, hp) = (value(), value(), value(), value());
            let hit_ago = (rng.below(4) != 0).then(|| i64::from(rng.small()));
            Valuation {
                elapsed,
                treasury,
                headroom,
                hp,
                hit_ago,
            }
        }

        fn holds(&self, condition: &Condition) -> bool {
            let cmp = |compare: &Option<IntCompare>, left: i64| {
                compare.is_some_and(|compare| {
                    let right = i64::from(compare.value);
                    match Op::try_from(compare.op) {
                        Ok(Op::Lt) => left < right,
                        Ok(Op::Le) => left <= right,
                        Ok(Op::Eq) => left == right,
                        Ok(Op::Ne) => left != right,
                        Ok(Op::Ge) => left >= right,
                        Ok(Op::Gt) => left > right,
                        _ => false,
                    }
                })
            };
            match condition.node.as_ref() {
                Some(condition::Node::All(all)) => all.items.iter().all(|item| self.holds(item)),
                Some(condition::Node::Any(any)) => any.items.iter().any(|item| self.holds(item)),
                Some(condition::Node::Not(not)) => {
                    !not.item.as_deref().is_some_and(|item| self.holds(item))
                }
                Some(condition::Node::SegmentElapsed(leaf)) => cmp(&leaf.ms, self.elapsed),
                Some(condition::Node::Treasury(leaf)) => cmp(&leaf.dollars, self.treasury),
                Some(condition::Node::KwHeadroom(leaf)) => cmp(&leaf.kw, self.headroom),
                Some(condition::Node::CmdrHpPct(leaf)) => cmp(
                    &Some(IntCompare {
                        op: leaf.cmp,
                        value: i32::try_from(leaf.pct).unwrap_or(i32::MAX),
                    }),
                    self.hp,
                ),
                Some(condition::Node::CmdrTookDamageWithin(leaf)) => {
                    self.hit_ago.is_some_and(|ago| ago <= i64::from(leaf.ms))
                }
                _ => false,
            }
        }
    }
}
