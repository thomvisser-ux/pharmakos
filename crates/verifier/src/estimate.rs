// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stage 5, **estimate**: the first half of FULL.
//!
//! The stage shipped present and empty (decisions-log item 82), so the API
//! shape and the hash contract were fixed before anything depended on them.
//! S1's `proj` task fills it (S1's plan; the register's S1-47): the `$` and
//! `kW` projection, and whether the route fits the coming segment.
//!
//! # What it raises
//!
//! | Code | Severity | When |
//! |---|---|---|
//! | `W0601` | warning | The grid is already short before this playbook runs: the seat's supply is below its draw |
//! | `E0601` | **error** | The route's orders add draw beyond supply, and `allow_dormant_beacons` is not set |
//! | `W0603` | warning | The same, with `allow_dormant_beacons` set: the shortfall is accepted |
//! | `W0602` | warning | The route's committed spending outruns the treasury |
//! | `W0701` | warning | The route cannot fit the coming segment, if every step runs |
//! | `W0604` | warning | Only the upper bound adds draw beyond supply: an edit of a beacon that may already field what it orders |
//! | `W0707` | warning | The snapshot does not decode, so the schedule was not checked |
//! | `I0001` | information | How much walking the route's known legs are, at least |
//!
//! `E0601` is the one error FULL can add, and it is spec section 11's own
//! rule: "Adding draw beyond supply is an error unless 'allow dormant beacons'
//! is set." An existing shortfall is "only a warning". Everything else here is
//! advice.
//!
//! The order is fixed — power, then money, then the schedule — and each
//! points at the **first** step that crosses its line, so a report is the same
//! bytes every time and the editor can underline the step where the route
//! stops being affordable, powered or on time.
//!
//! # The three things it reads
//!
//! * **The projection** ([`crate::projection`]): what the route orders, at the
//!   rules table's prices and the sim's draw rules, against the seat's
//!   treasury, supply and draw in the [`Scope`].
//! * **The interface arithmetic** ([`crate::interface`]): exact, row by row.
//! * **The walking bound** ([`crate::walking`]): the least walking each leg
//!   between two places the view knows can take, never an arrival time.
//!
//! and the one fact it takes from the snapshot itself: the coming segment's
//! length. Spec section 3 puts it there ("the coming segment's length is part
//! of the frozen snapshot, so the editor's clock, the fits pill and
//! `render_plan` all read the same number rather than a constant"), and the
//! snapshot bytes are already one of `report_hash`'s five inputs. A snapshot
//! that carries no positive length (one taken before a match opened) gives no
//! segment, and the schedule check then says nothing rather than guessing one:
//! `W0701` is about the segment the snapshot says is coming, and there is none
//! to compare with. Bytes that do not decode as a snapshot this build reads
//! are a different case, a fault in whatever assembled the [`Input`] rather
//! than in the playbook, and the check says so with `W0707` rather than going
//! quiet.
//!
//! Decoding the snapshot is FULL's cost alone; QUICK never pays it (decisions-
//! log item 33 (c)). It decodes the whole snapshot, every modified chunk
//! included, to read one number; the cheaper road is the gateway handing the
//! length in with the seat's view, which is a change to `report_hash`'s scope
//! encoding and so a contract change, and is left to the owner with FULL's
//! budget (P1).
//!
//! # What may never happen here
//!
//! Stepping the sim. Running mandates, programs, combat or construction.
//! Modelling an enemy. Evaluating a rule condition over a projected future. The
//! verifier estimates; it does not rehearse (AGENTS.md section 3 rule 2). So:
//!
//! * a description (`covering`, a selector, `safest`) has no place here until
//!   the sim or the gateway resolves it, and a leg to or from one is not
//!   counted: the walking bound covers only the legs whose two ends the view
//!   names;
//! * a `wait_until` counts nothing, because when its condition comes true is
//!   a fact about the future;
//! * handler bodies, `on_death` and the fallback count nothing, because they
//!   may never run;
//! * a step's `skip_if` and `on_fail` are not evaluated, because they are
//!   conditions over a future state: the schedule assumes every step runs,
//!   and the message says so.
//!
//! # What remains for later stages
//!
//! | Work | Code | Brought by |
//! |---|---|---|
//! | Mast coverage for a broadcast | `E0602` | **S4**, with radio |

use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::v1::{
    BeaconRef, Declarative, Location, Playbook, Voxel, beacon_ref, location, step,
};
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::Ms;

use crate::Input;
use crate::interface;
use crate::pointer;
use crate::projection::{self, Tally};
use crate::report::{Builder, Diag, patch_add};
use crate::scope::Scope;
use crate::strings;

/// Where the route's steps are, as a JSON Pointer.
const ROUTE: &str = "/declarative/route";

/// Run the stage.
///
/// The playbook, and the whole [`Input`] it was decoded from: the seat's
/// view, the rules-table numbers and the snapshot, which is where the coming
/// segment's length is.
pub(crate) fn run(playbook: &Playbook, input: &Input<'_>, out: &mut Builder) {
    let Some(body) = playbook.declarative.as_ref() else {
        return;
    };
    economy(body, input, out);
    schedule(body, input, out);
}

// ---------------------------------------------------------------------------
// `$` and `kW`
// ---------------------------------------------------------------------------

/// `W0601`, then `E0601` or `W0603` (or `W0604`), then `W0602`.
///
/// The route is summed in [`Tally`], which holds any playbook's orders
/// exactly, so no count is large enough to silence the check: an order past
/// the sim's own `$` and `kW` types is past every supply, and is reported as
/// what it is (`ProjectionError::Overflow` cannot arise here).
///
/// **A step that orders a blueprint the rules table does not price ends the
/// projection there.** There is no honest figure for that step or any after
/// it, but everything found before it stands and is reported: draw and spend
/// only grow along a route, so a line crossed before the unpriced step stays
/// crossed. Such a playbook is refused by the sim at the seal's compile (only
/// the Generator is buildable before S4), and `E0506` names it once the
/// capability catalogue exists (S4).
fn economy(body: &Declarative, input: &Input<'_>, out: &mut Builder) {
    let scope = input.scope();
    let economy = scope.economy();
    let supply = i64::from(economy.supply.raw());
    let draw = i64::from(economy.draw.raw());
    let headroom = supply.saturating_sub(draw);
    if headroom < 0 {
        out.emit(Diag::new("W0601", pointer::ROOT).arg("found", headroom.saturating_neg()));
    }

    let prices = input.prices();
    // What the route certainly adds, and the upper bound with what it may add
    // (`projection`'s module doc, "How sure it is").
    let mut adds = Tally::ZERO;
    let mut upper = Tally::ZERO;
    let mut first_short: Option<String> = None;
    let mut first_maybe_short: Option<String> = None;
    let mut first_over: Option<String> = None;
    // The draw the grid can still take before it is short. An existing
    // shortfall leaves none: every `kW` the route adds is beyond supply.
    let room = i128::from(headroom.max(0));
    let treasury = i128::from(economy.treasury.raw());
    for (index, entry) in body.route.iter().enumerate() {
        let Ok(orders) = projection::step_orders(entry, prices, scope) else {
            break;
        };
        adds = adds.plus(orders.adds);
        upper = upper.plus(orders.upper());
        let at = pointer::at(ROUTE, index);
        if first_short.is_none() && adds.draw > room {
            first_short = Some(at.clone());
        }
        if first_maybe_short.is_none() && upper.draw > room {
            first_maybe_short = Some(at.clone());
        }
        if first_over.is_none() && upper.spend > treasury {
            first_over = Some(at);
        }
    }

    if let Some(at) = first_short {
        let beyond = adds.draw.saturating_sub(room);
        let allowed = body
            .options
            .as_ref()
            .is_some_and(|options| options.allow_dormant_beacons);
        if allowed {
            out.emit(Diag::new("W0603", at).arg("found", beyond).related(OPTION));
        } else {
            out.emit(Diag::new("E0601", at).arg("found", beyond).fix(
                strings::FIX_ALLOW_DORMANT,
                allow_dormant(body),
                Applicability::MaybeIncorrect,
            ));
        }
    } else if let Some(at) = first_maybe_short {
        // Only the upper bound crosses the line: the route may add the draw,
        // and the view cannot say whether it does, so this is a warning and
        // never spec section 11's error.
        out.emit(Diag::new("W0604", at).arg("found", upper.draw.saturating_sub(room)));
    }
    if let Some(at) = first_over {
        out.emit(
            Diag::new("W0602", at)
                .arg("found", dollars(upper.spend))
                .arg("available", dollars(treasury)),
        );
    }
}

/// `options.allow_dormant_beacons`, as a pointer.
const OPTION: &str = "/declarative/options/allow_dormant_beacons";

/// The patch that sets `allow_dormant_beacons`.
///
/// RFC 6902's `add` needs the parent to exist, and proto3's canonical form
/// leaves `options` out entirely when nothing in it is set, so a file with no
/// `options` gets the whole object and a file with one gets the member.
fn allow_dormant(body: &Declarative) -> String {
    if body.options.is_some() {
        patch_add(OPTION, &Json::Bool(true))
    } else {
        patch_add(
            &pointer::child("/declarative", "options"),
            &Json::Object(vec![("allow_dormant_beacons".to_owned(), Json::Bool(true))]),
        )
    }
}

/// A `$` figure as a player reads it: `$ 60`, the spelling the gateway's own
/// feed lines use.
fn dollars(money: i128) -> String {
    format!("$ {money}")
}

// ---------------------------------------------------------------------------
// The schedule
// ---------------------------------------------------------------------------

/// `W0701`, then `I0001`.
fn schedule(body: &Declarative, input: &Input<'_>, out: &mut Builder) {
    let scope = input.scope();
    let rates = input.rates();
    let walk = input.walk();

    // Where the commander is known to stand, and how far short of that point
    // it may have stopped. The view's commander position, when it carries
    // one, is where the route starts.
    let mut here: Option<(Voxel, i64)> = scope.commander().map(|at| (at, 0));
    let mut walking = Ms::ZERO;
    let mut elapsed = Ms::ZERO;
    let mut first_late: Option<String> = None;
    let segment = match segment(input) {
        Segment::Coming(length) => Some(length),
        Segment::NoneYet => None,
        Segment::Unreadable => {
            out.emit(Diag::new("W0707", pointer::ROOT));
            None
        }
    };

    for (index, entry) in body.route.iter().enumerate() {
        // A leg to walk first, as the place the view names (or `None` when it
        // names none) and the radius the step stops at; then the time spent
        // where it stops.
        let (leg, on_site) = match entry.kind.as_ref() {
            Some(step::Kind::Move(walk_to)) => (
                Some((place(walk_to.to.as_ref(), scope), walk.arrive_radius())),
                Ms::ZERO,
            ),
            Some(step::Kind::PlaceBeacon(deploy)) => (
                Some((place(deploy.at.as_ref(), scope), walk.interface_range())),
                interface::place_beacon_ms(deploy, rates).total,
            ),
            Some(step::Kind::Interface(visit)) => (
                Some((beacon(visit.beacon.as_ref(), scope), walk.interface_range())),
                interface::interface_ms(visit, rates).total,
            ),
            // A hold is exactly its duration; a negative one is `E0109`'s and
            // counts nothing here.
            Some(step::Kind::Hold(hold)) => (None, Ms::new(hold.ms.max(0))),
            // A wait ends when its condition comes true, which is the future;
            // a broadcast is radio's (S4). Neither moves the commander.
            Some(step::Kind::WaitUntil(_) | step::Kind::Broadcast(_)) | None => (None, Ms::ZERO),
        };
        if let Some((target, radius)) = leg {
            let walked = match (here, target) {
                (Some((from, slack)), Some(to)) => walk.leg(from, slack, to, radius),
                // A leg with an end the view does not name is not counted:
                // the bound stays a bound.
                _ => Ms::ZERO,
            };
            walking = plus(walking, walked);
            elapsed = plus(elapsed, walked);
            here = target.map(|to| (to, radius));
        }
        elapsed = plus(elapsed, on_site);
        if let Some(length) = segment {
            if first_late.is_none() && elapsed > length {
                first_late = Some(pointer::at(ROUTE, index));
            }
        }
    }

    if let (Some(at), Some(length)) = (first_late, segment) {
        out.emit(
            Diag::new("W0701", at)
                .arg("found", elapsed.raw())
                .arg("segment", length.raw()),
        );
    }
    if walking > Ms::ZERO {
        out.emit(Diag::new("I0001", ROUTE).arg("found", walking.raw()));
    }
}

/// What the snapshot says about the coming segment (see the module doc).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Segment {
    /// A segment of this length is coming.
    Coming(Ms),
    /// The snapshot carries no positive length: no match has opened a segment
    /// yet.
    NoneYet,
    /// The bytes are not a snapshot this build reads.
    Unreadable,
}

fn segment(input: &Input<'_>) -> Segment {
    match input.decode_snapshot() {
        Ok(snapshot) if snapshot.coming_segment_ms > 0 => {
            Segment::Coming(Ms::new(snapshot.coming_segment_ms))
        }
        Ok(_) => Segment::NoneYet,
        Err(_) => Segment::Unreadable,
    }
}

/// Where a location is, when the view names it: a voxel, or a beacon by its
/// id. A description is resolved by the sim or the gateway, never here.
fn place(at: Option<&Location>, scope: &Scope) -> Option<Voxel> {
    match at?.place.as_ref()? {
        location::Place::Voxel(voxel) => Some(*voxel),
        location::Place::BeaconAnchor(reference) => beacon(Some(reference), scope),
        location::Place::Safest(_) | location::Place::On(_) | location::Place::Covering(_) => None,
    }
}

/// Where a beacon is, when it is named by an id the view knows.
fn beacon(reference: Option<&BeaconRef>, scope: &Scope) -> Option<Voxel> {
    match reference?.r#ref.as_ref()? {
        beacon_ref::Ref::BeaconId(id) => scope.beacon(id).map(|known| known.at),
        beacon_ref::Ref::Safest(_)
        | beacon_ref::Ref::Nearest(_)
        | beacon_ref::Ref::Weakest(_)
        | beacon_ref::Ref::MostThreatened(_) => None,
    }
}

/// Two durations added, saturating upwards: past `i32` is past every segment,
/// and that is all the figure is compared with.
fn plus(left: Ms, right: Ms) -> Ms {
    Ms::new(left.raw().saturating_add(right.raw()))
}

#[cfg(test)]
mod tests {
    use super::dollars;

    /// M-10, ruled by S1's plan (task `proj`): `Options` stays
    /// `allow_dormant_beacons` alone, the one option spec section 7 names.
    /// The pattern is exhaustive, so an option added to the schema stops this
    /// compiling until somebody decides what the estimate stage does with it.
    #[test]
    fn options_is_allow_dormant_beacons_alone() {
        let pharmakos_proto::gp::v1::Options {
            allow_dormant_beacons,
        } = pharmakos_proto::gp::v1::Options::default();
        assert!(!allow_dormant_beacons, "omitted, the shortfall is an error");
    }

    #[test]
    fn a_dollar_figure_reads_as_the_feed_spells_it() {
        assert_eq!(dollars(220), "$ 220");
        assert_eq!(dollars(-5), "$ -5");
    }
}
