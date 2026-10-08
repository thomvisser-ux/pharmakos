// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The gateway's section of the one English string table.
//!
//! Spec section 12 asks every result for "structured JSON plus deterministic
//! template prose", and spec section 15 says there is one string table and one
//! language in v1. So every sentence a seat reads from this crate is written
//! here, as a template, and assembled by a function beside it.
//!
//! **Template prose, not generated prose.** A template is the only kind of
//! prose that is deterministic, and a prose golden that differed between
//! Windows, Linux and macOS would be a determinism hole rather than a wording
//! change (the same argument `plan-core`'s `strings` module makes). Every
//! function here is a pure function of integers and names: no clock, no float,
//! no locale, no iteration over an unordered collection.
//!
//! Engine chatter is a line of this kind and is **one-way**: nothing here ever
//! renders one seat's playbook, draft, notebook or knowledge into a line
//! another seat could be shown. What decides who is shown a line is
//! [`crate::fog`]; what this module decides is what the line says.

use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::runner::{MatchEndReason, MatchPhase};

/// One feed line, from a sim event.
///
/// One arm per [`EventKind`], so a kind added to the sim's bus fails to
/// compile here rather than reaching a player as a blank line. The numbers
/// each kind carries are documented on the kind itself; this is where they are
/// spelled.
#[must_use]
pub fn event_text(event: &Event) -> String {
    let seat = event.seat.map_or_else(
        || String::from("an unclaimed asset"),
        |id| format!("seat {}", id.raw()),
    );
    match event.kind {
        EventKind::MatchStarted => String::from("The match opened."),
        EventKind::LullOpened => String::from("The Lull opened: seals may be written."),
        EventKind::PushStarted => format!(
            "The Push began. This segment runs {}.",
            clock(Ms::new(i32::try_from(event.value).unwrap_or(i32::MAX)))
        ),
        EventKind::SegmentEnded => format!("The Push ended after {} ticks.", event.value),
        EventKind::RecapOpened => String::from("The recap opened: the Ledger settles."),
        // `seat` is the winner when one seat won: the last standing, or the
        // final audit's; a shared win names nobody here, and the recap names
        // every seat it ties.
        EventKind::MatchEnded => event.seat.map_or_else(
            || String::from("The match ended."),
            |winner| format!("The match ended: seat {} won.", winner.raw()),
        ),
        EventKind::BeaconDestroyed => format!("A beacon of {seat} was destroyed."),
        EventKind::StructureRuined => {
            format!("A structure of {seat} became a neutral ruin.")
        }
        EventKind::SeatEliminated => format!("{seat} was eliminated.", seat = capitalise(&seat)),
        EventKind::CommanderDied => format!(
            "The commander of {seat} died. That is death {} in this Push.",
            event.value
        ),
        EventKind::CommanderRespawned => format!(
            "The commander of {seat} came back after {} ticks.",
            event.value
        ),
        EventKind::UnitSealedIn => {
            format!("A unit of {seat} is sealed in and has parked where it stopped.")
        }
        // The interpreter's kinds (T11). Every one of them but `beacon_placed`
        // is the seat's own commander's orders, so `Surface::audience_of`
        // keeps the line private to that seat: a step or a rule is named by
        // its index here and is still never shown to anybody else.
        // The count agrees with itself ("1 route step"), the demo's F5.
        EventKind::PlanSealed => format!(
            "The playbook of {seat} was sealed: {}.",
            count(
                u32::try_from(event.value).unwrap_or(u32::MAX),
                "route step",
                "route steps"
            )
        ),
        EventKind::StepStarted => format!("Step {} started.", event.value),
        EventKind::StepCompleted => format!("Step {} completed.", event.value),
        EventKind::StepSkipped => format!(
            "Step {} was skipped: its skip_if guard was true.",
            event.value
        ),
        EventKind::StepFailed => step_failed(event.value),
        EventKind::RuleFired => format!("Rule {} fired.", event.value),
        EventKind::RuleEnded => format!("A rule's body ended with resume code {}.", event.value),
        EventKind::ReflexFired => format!(
            "The commander's reflex fired at {} % hit points and it is withdrawing.",
            event.value
        ),
        EventKind::ReflexCleared => format!(
            "The commander's reflex cleared at {} % hit points and the route continues.",
            event.value
        ),
        EventKind::VisitStarted => format!(
            "The commander began a visit of {} interface rows.",
            event.value
        ),
        EventKind::RowCommitted => format!("Interface row {} committed.", event.value),
        EventKind::VisitEnded => format!("The visit ended with {} rows committed.", event.value),
        EventKind::BeaconPlaced => format!("A beacon of {seat} was deployed."),
        // T14's economy lines. Every figure a line carries is named in words
        // as well as in the number, because a feed is read rather than parsed.
        EventKind::BeaconBrownedOut => format!("A beacon of {seat} went dark: draw outran supply."),
        EventKind::BeaconRevived => format!("A beacon of {seat} came back on."),
        EventKind::UnitFabricated => format!("A fabricator of {seat} produced a unit."),
        EventKind::StructureQueued => format!("{seat} paid for a structure; it is going up now."),
        EventKind::StructureCompleted => format!("A structure of {seat} is finished."),
        EventKind::OreDelivered => {
            format!(
                "A mining drone of {seat} delivered ore worth $ {}.",
                event.value
            )
        }
        EventKind::SalvageDelivered => {
            format!(
                "A reclaim drone of {seat} delivered salvage worth $ {}.",
                event.value
            )
        }
        EventKind::BeaconRecycled => {
            format!(
                "A beacon of {seat} was recycled on site for $ {}.",
                event.value
            )
        }
        EventKind::Settled => format!("The Ledger settled and credited {seat} $ {}.", event.value),
        EventKind::KillCredited => {
            format!("{seat} was credited $ {} of a destruction.", event.value)
        }
        EventKind::FallbackEngaged => fallback_engaged(event.value),
        EventKind::FinalAudit => format!("The final audit scored {seat} at $ {}.", event.value),
    }
}

/// `step_failed`'s line: **why** the step stopped, in words, with the failure's
/// name as the feed and a scenario file spell it.
///
/// Targeting's "the recap names why a step found nothing"
/// (`docs/design/targeting.md`, "Surfaces"; S1's plan, task `tgtw`): a
/// `no_target` and an `illegal_site` used to read as "failure code 2" and
/// "failure code 6". The step's index is not on the event (its `value` is the
/// failure), so the line names the reason and not the step. The value is read
/// through the sim's one decode of its layout
/// ([`pharmakos_sim::interpreter::StepFailed::decode`]), which since S1's
/// `fog` also carries how many candidates a description matched; this line
/// does not say the count (decisions-log item 133 (3) (h)): the recap's
/// [`recap_step_failed`] does, and the Lull's counted sentence
/// ([`this_round`]) says it before the Push. A value that does not decode -- a failure id this build does
/// not define, or bits its layout keeps zero -- is said as its code rather
/// than guessed at.
fn step_failed(value: i64) -> String {
    use pharmakos_sim::interpreter::StepFailed;
    match StepFailed::decode(value) {
        Ok(failed) => format!(
            "A step failed ({}): {}. Its on_fail decided what came next.",
            failed.reason.name(),
            failure_reason(failed.reason)
        ),
        Err(refused) => format!(
            "A step failed with a failure this build does not name (code {}). Its \
             on_fail decided what came next.",
            refused.value()
        ),
    }
}

/// The recap's line for one of the seat's own step failures in the segment
/// that ended (`docs/design/targeting.md`, "Surfaces": "The recap names why a
/// step found nothing"), `times` being how often the segment failed a step
/// with this very value.
///
/// A `no_target` whose description matched something says how much it
/// matched and that none of it was reachable -- "3 matched, none reachable"
/// (targeting.md, "Unreachable") -- from the count `fog` put on the event and
/// the sim's one decode of it
/// ([`pharmakos_sim::interpreter::StepFailed::decode`]). Every other failure,
/// and a `no_target` that matched nothing or read no feature, keeps the feed's
/// words ([`step_failed`], whose sentence this does not change). A value that
/// does not decode is said as its code, as the feed says it.
#[must_use]
pub fn recap_step_failed(value: i64, times: usize) -> String {
    use pharmakos_sim::interpreter::{StepFailed, StepFailure};
    let lead = match times {
        0 | 1 => String::from("A step"),
        2 => String::from("Twice, a step"),
        many => format!("{many} times, a step"),
    };
    match StepFailed::decode(value) {
        Ok(StepFailed {
            reason: StepFailure::NoTarget,
            matched: Some(matched),
        }) if matched > 0 => {
            format!("{lead} found nothing: {matched} matched, none reachable.")
        }
        Ok(failed) => format!(
            "{lead} failed ({}): {}.",
            failed.reason.name(),
            failure_reason(failed.reason)
        ),
        Err(refused) => format!(
            "{lead} failed with a failure this build does not name (code {}).",
            refused.value()
        ),
    }
}

/// A step failure, in words.
const fn failure_reason(failure: pharmakos_sim::interpreter::StepFailure) -> &'static str {
    use pharmakos_sim::interpreter::StepFailure;
    match failure {
        StepFailure::Timeout => "it ran past its timeout",
        StepFailure::NoTarget => {
            "it found nothing: nothing matched what it named, or nothing it matched could be \
             reached"
        }
        StepFailure::NoPath => "the commander could not reach its target",
        StepFailure::OutOfRange => "the commander left the beacon's range mid-visit",
        StepFailure::BeaconGone => "the beacon it was visiting is gone",
        StepFailure::IllegalSite => {
            "it found no legal site: what it matched had no site the seat may place on within \
             reach"
        }
        StepFailure::NoBeaconRoom => "the seat has no room for another beacon",
        StepFailure::NoMast => "a broadcast needs a Radio Mast",
        StepFailure::CommanderDead => "the commander died",
        StepFailure::FeatureLost => "the vent or seam it was bound to is gone",
        StepFailure::Unaffordable => "the treasury could not cover it",
    }
}

/// What one route step's `covering` reads this round, for [`this_round`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundRead<'a> {
    /// It places a new beacon on the column `(x, y)`, for `$ dollars`.
    Places {
        /// The route step, from 0.
        step: usize,
        /// The site's column.
        x: i32,
        /// The site's column.
        y: i32,
        /// The beacon's deploy cost, whole `$`.
        dollars: u32,
    },
    /// Nothing of `kind` matches its description, or its name names nothing.
    NothingMatches {
        /// The route step, from 0.
        step: usize,
        /// `vent` or `seam`.
        kind: &'a str,
    },
    /// `matched` features matched and none of them can be reached.
    NoneReachable {
        /// The route step, from 0.
        step: usize,
        /// How many matched.
        matched: u32,
    },
    /// Features of `kind` matched and none has a legal site within reach.
    NoneCoverable {
        /// The route step, from 0.
        step: usize,
        /// `vent` or `seam`.
        kind: &'a str,
    },
    /// It fails for another reason, named as the feed names it.
    Fails {
        /// The route step, from 0.
        step: usize,
        /// The step failure's name.
        failure: &'a str,
    },
}

/// The Lull's "this round" sentence: what the seat's playbook will do this
/// round wherever it reads the map (`docs/design/targeting.md`, "Surfaces":
/// "On re-seal the Lull says what this round will do"). `None` when the
/// playbook reads the map nowhere on its route, which is nothing to say.
///
/// Steps are numbered from 1, as `render_plan` numbers them. Every number is
/// the caller's and every word is here: "This round: step 2 places a new
/// beacon near (150, 20), $ 60." or "This round: step 2 finds no vent you can
/// cover."
#[must_use]
pub fn this_round(reads: &[RoundRead<'_>]) -> Option<String> {
    let parts: Vec<String> = reads
        .iter()
        .map(|read| match *read {
            RoundRead::Places {
                step,
                x,
                y,
                dollars,
            } => format!(
                "step {} places a new beacon near ({x}, {y}), $ {dollars}",
                step.saturating_add(1)
            ),
            RoundRead::NothingMatches { step, kind } => {
                format!("step {} finds no {kind} to cover", step.saturating_add(1))
            }
            RoundRead::NoneReachable { step, matched } => format!(
                "step {} finds nothing to cover: {matched} matched, none reachable",
                step.saturating_add(1)
            ),
            RoundRead::NoneCoverable { step, kind } => {
                format!(
                    "step {} finds no {kind} you can cover",
                    step.saturating_add(1)
                )
            }
            RoundRead::Fails { step, failure } => {
                format!(
                    "step {} fails `{failure}` before it reads the map",
                    step.saturating_add(1)
                )
            }
        })
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(format!("This round: {}.", parts.join("; ")))
}

/// `fallback_engaged`'s line: the posture by name, never its wire code (the
/// demo's F5).
fn fallback_engaged(value: i64) -> String {
    match u8::try_from(value).ok().and_then(posture_name) {
        Some(posture) => format!("The route ended and the fallback took over: {posture}."),
        None => format!(
            "The route ended and the fallback took over, in a posture this build does not \
             name (code {value})."
        ),
    }
}

/// The posture a `fallback_engaged` value names, as the editor spells it, or
/// `None` for an id this build does not define. The ids are the sim's
/// `pharmakos_sim::interpreter::Posture::id`: 1 Hold, 2 Shadow, 3 Patrol. The
/// words live here, in the gateway's one string table, and not in the sim.
const fn posture_name(id: u8) -> Option<&'static str> {
    match id {
        1 => Some("Hold"),
        2 => Some("Shadow"),
        3 => Some("Patrol"),
        _ => None,
    }
}

/// The line a seat's own feed carries when the gateway filed its playbook for
/// it: the Lull ran out with nothing sealed, and the safe playbook was filed
/// (spec section 14). Decision 8 of S1's plan, ruled by item 128, and the
/// demo's F5: a timeout's filing used to read like the player's own seal.
pub const SAFE_PLAYBOOK_FILED: &str = "The Lull ran out, so the safe playbook was filed for you.";

/// `get_briefing`'s prose.
///
/// The notebook is **not** in it: the notebook is its own field, at the top of
/// the result, as spec section 12 asks, and repeating it in the prose would
/// double a seat's own words back at it.
#[must_use]
pub fn briefing(
    phase: MatchPhase,
    round: u32,
    round_limit: u32,
    segment_ms: Ms,
    beacons: u32,
    units: u32,
) -> String {
    format!(
        "Round {round} of {round_limit}, {phase}. The coming segment runs {segment}. You hold \
         {beacons} and field {units}.",
        phase = phase_prose(phase),
        segment = clock(segment_ms),
        beacons = count(beacons, "beacon", "beacons"),
        units = count(units, "unit", "units"),
    )
}

/// `get_recap`'s prose, once a segment has ended: the round, how many ticks
/// its segment ran, and how the match stands.
///
/// `outcome` is the reason and **every** winner, in seat order: the last seat
/// standing, the final audit's single winner, or the seats a shared win ties
/// (spec section 3: "only an exact tie on every term is recorded as a shared
/// win"). The recap names each of them.
#[must_use]
pub fn recap(round: u32, ticks: u32, outcome: Option<(MatchEndReason, &[u8])>) -> String {
    format!("Round {round} ran {ticks} ticks. {}", ending(outcome))
}

/// `get_recap`'s prose when there is no segment end to read: before the first
/// one (`first_round`), or in a match resumed since its last one, whose recap
/// figures a resume does not carry (the surface module's PLACEHOLDER, owner
/// at S7).
#[must_use]
pub fn no_recap(first_round: bool, outcome: Option<(MatchEndReason, &[u8])>) -> String {
    let lead = if first_round {
        "No round has ended yet."
    } else {
        "This match was resumed, and its last recap is not kept across a resume."
    };
    format!("{lead} {}", ending(outcome))
}

/// How the match stands, for the recap.
fn ending(outcome: Option<(MatchEndReason, &[u8])>) -> String {
    let Some((reason, winners)) = outcome else {
        return String::from("The match continues.");
    };
    let ended = format!("The match ended: {}.", reason.name());
    match (reason, winners) {
        (MatchEndReason::LastSeatStanding, [seat, ..]) => format!("{ended} Seat {seat} stands."),
        (_, []) => format!("{ended} The final audit names no winner."),
        (_, [seat]) => format!("{ended} The final audit names seat {seat} the winner."),
        (_, several) => format!(
            "{ended} The final audit ties {} on every term: a shared win.",
            seats_prose(several)
        ),
    }
}

/// The recap's settlement line, the seat's own (spec section 7,
/// "Settlement"): what the Ledger credited it, at which band (its place on
/// held value among the living seats, from 1 for the leader) and that band's
/// adjustment in whole percent, and the award fund, which pays nobody until
/// S4 brings the awards it is split by.
#[must_use]
pub fn settlement(bmi_dollars: i32, band_rank: u32, band_percent: i32) -> String {
    format!(
        "The Ledger settled and credited you $ {bmi_dollars}, your Basic Minimum Income at band \
         {band_rank} ({band_percent:+} %). The award fund pays out from S4, when there are \
         awards to split it by."
    )
}

/// The recap's shortfall line, the seat's own: which of its beacons were
/// dark when the segment ended, by name, and the kW their going dark shed
/// (spec section 7, "flagged in plain language in the recap").
#[must_use]
pub fn shortfall(beacon_ids: &[String], shed_kw: i32) -> String {
    let (verb, names) = match beacon_ids.split_last() {
        None => ("was", String::from("none of your beacons")),
        Some((last, [])) => ("was", last.clone()),
        Some((last, rest)) => ("were", format!("{} and {last}", rest.join(", "))),
    };
    format!(
        "When the segment ended, {names} {verb} dark: draw outran supply, and the brownout shed \
         {shed_kw} kW."
    )
}

/// `seats 0 and 1`, `seats 0, 1 and 2`.
fn seats_prose(seats: &[u8]) -> String {
    let names: Vec<String> = seats.iter().map(u8::to_string).collect();
    match names.split_last() {
        None => String::from("no seats"),
        Some((last, [])) => format!("seat {last}"),
        Some((last, rest)) => format!("seats {} and {last}", rest.join(", ")),
    }
}

/// `get_map_summary`'s prose.
#[must_use]
pub fn map_summary(size: [u32; 3], seed: &str, seats: u32) -> String {
    let axis = |index: usize| size.get(index).copied().unwrap_or(0);
    format!(
        "The map is {} by {} by {} voxels, generated from seed {seed} for {}.",
        axis(0),
        axis(1),
        axis(2),
        count(seats, "seat", "seats"),
    )
}

/// `get_beacon`'s prose.
#[must_use]
pub fn beacon(id: &str, mandate: &str, hit_points: i32, own: bool) -> String {
    let whose = if own { "Yours" } else { "Not yours" };
    format!("{whose}. Beacon {id} carries the {mandate} mandate and has {hit_points} HP.")
}

/// `render_plan`'s fallback when a playbook has no canonical form.
///
/// `render_plan` asks `plan-core` for the rendering; a file that will not
/// parse has none, and this is what the editor shows instead of an empty box.
pub const UNRENDERABLE: &str = "This file has no canonical form yet, so there is nothing to read back. Verify it: the \
     diagnostics say where it stops being a playbook.";

/// `get_briefing`'s standing sentence: how many seats still stand, and the
/// seat's own place on the displayed standing (spec section 3: "live
/// standings show a seat its own score and displayed rank only", on the full
/// audit score). `place` is the seat's rank and how many seats the standing
/// ranks, or `None` for a seat out of the match, which holds no place; the
/// score is its own either way (the register's X-03).
#[must_use]
pub fn standing(living: u32, place: Option<(u32, u32)>, score: i32) -> String {
    let standing = count(living, "seat", "seats");
    match place {
        Some((rank, of)) => format!(
            "{standing} still standing. Your score is $ {score}, which ranks you {rank} of {of} on \
             the final audit's terms."
        ),
        None => format!(
            "{standing} still standing. You are out of the match, so you hold no place on the \
             standing; your score is $ {score}."
        ),
    }
}

/// A phase, as a sentence says it.
#[must_use]
pub const fn phase_prose(phase: MatchPhase) -> &'static str {
    match phase {
        MatchPhase::Lull => "the Lull",
        MatchPhase::Push => "the Push",
        MatchPhase::Recap => "the recap",
        MatchPhase::Ended => "the match is over",
    }
}

/// `12 beacons`, `1 beacon`, `no beacons`.
#[must_use]
pub fn count(many: u32, one: &str, several: &str) -> String {
    match many {
        0 => format!("no {several}"),
        1 => format!("1 {one}"),
        _ => format!("{many} {several}"),
    }
}

/// Game milliseconds as `3:00` or `45 s`, rounded **down** to whole seconds.
///
/// Down, and deliberately not `plan-core`'s `whole_seconds`: that one rounds
/// **up** because it renders an *estimate* the player plans against and the
/// estimator is never optimistic (item 57). This one renders a *fact* -- how
/// long the segment is, how long is left -- and a fact is not rounded away
/// from itself.
#[must_use]
pub fn clock(ms: Ms) -> String {
    let seconds = ms.raw().max(0).checked_div(1000).unwrap_or(0);
    if seconds < 60 {
        return format!("{seconds} s");
    }
    let minutes = seconds.checked_div(60).unwrap_or(0);
    let rest = seconds.saturating_sub(minutes.saturating_mul(60));
    format!("{minutes}:{rest:02}")
}

/// The first character upper-cased, for a sentence that starts with a rendered
/// name. ASCII only, because the one string table is English.
fn capitalise(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| {
        format!("{}{}", first.to_ascii_uppercase(), characters.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::{
        briefing, capitalise, clock, count, event_text, failure_reason, map_summary, no_recap,
        recap, recap_step_failed, settlement, shortfall, standing, step_failed,
    };
    use pharmakos_sim::events::{Event, EventKind};
    use pharmakos_sim::math::quantity::{Ms, Tick};
    use pharmakos_sim::runner::{MatchEndReason, MatchPhase};
    use pharmakos_sim::tables::SeatId;

    fn event(kind: EventKind, seat: Option<u8>, value: i64) -> Event {
        Event {
            tick: Tick::new(7),
            seq: 0,
            kind,
            seat: seat.map(SeatId::new),
            subject: None,
            at: None,
            value,
        }
    }

    #[test]
    fn every_kind_of_the_sims_bus_renders_a_sentence() {
        for kind in EventKind::ALL {
            let text = event_text(&event(kind, Some(1), 3));
            assert!(!text.is_empty(), "{kind:?} renders nothing");
            assert!(text.ends_with('.'), "{kind:?}: `{text}`");
            assert!(
                !text.contains('\t') && !text.contains('\n'),
                "{kind:?} would forge an audit record"
            );
        }
    }

    #[test]
    fn a_line_names_the_seat_it_is_about_and_never_a_playbook() {
        let text = event_text(&event(EventKind::CommanderDied, Some(2), 3));
        assert_eq!(
            text,
            "The commander of seat 2 died. That is death 3 in this Push."
        );
        let text = event_text(&event(EventKind::StructureRuined, None, 0));
        assert!(text.contains("an unclaimed asset"), "{text}");
    }

    #[test]
    fn a_clock_rounds_down_because_it_renders_a_fact() {
        assert_eq!(clock(Ms::new(0)), "0 s");
        assert_eq!(clock(Ms::new(1_999)), "1 s");
        assert_eq!(clock(Ms::new(180_000)), "3:00");
        assert_eq!(clock(Ms::new(185_400)), "3:05");
        assert_eq!(clock(Ms::new(-5)), "0 s", "never a negative clock");
    }

    #[test]
    fn a_count_agrees_with_itself() {
        assert_eq!(count(0, "beacon", "beacons"), "no beacons");
        assert_eq!(count(1, "beacon", "beacons"), "1 beacon");
        assert_eq!(count(7, "beacon", "beacons"), "7 beacons");
    }

    #[test]
    fn the_briefing_prose_carries_the_segment_from_its_argument() {
        let prose = briefing(MatchPhase::Lull, 1, 3, Ms::new(180_000), 1, 3);
        assert!(prose.contains("Round 1 of 3, the Lull"), "{prose}");
        assert!(prose.contains("runs 3:00"), "{prose}");
        assert!(prose.contains("1 beacon"), "{prose}");
        assert!(prose.contains("3 units"), "{prose}");
    }

    #[test]
    fn the_recap_says_whether_the_match_is_over() {
        assert!(recap(1, 3_600, None).ends_with("The match continues."));
        let ended = recap(3, 100, Some((MatchEndReason::RoundLimit, &[2])));
        assert!(ended.contains("round_limit"), "{ended}");
        assert!(
            ended.ends_with("The final audit names seat 2 the winner."),
            "{ended}"
        );
        let won = recap(3, 100, Some((MatchEndReason::LastSeatStanding, &[1])));
        assert!(won.contains("Seat 1 stands."), "{won}");
        let shared = recap(6, 100, Some((MatchEndReason::RoundLimit, &[0, 2])));
        assert!(
            shared.ends_with("The final audit ties seats 0 and 2 on every term: a shared win."),
            "the recap names every winner: {shared}"
        );
        let three = recap(6, 100, Some((MatchEndReason::NoSurvivor, &[0, 1, 2])));
        assert!(three.contains("seats 0, 1 and 2"), "{three}");
    }

    #[test]
    fn the_recap_reads_a_segment_end_or_says_there_is_none() {
        assert_eq!(
            recap(2, 6_000, None),
            "Round 2 ran 6000 ticks. The match continues."
        );
        assert_eq!(
            no_recap(true, None),
            "No round has ended yet. The match continues."
        );
        assert!(no_recap(false, None).starts_with("This match was resumed"));
    }

    #[test]
    fn the_settlement_and_shortfall_lines_name_the_seats_own_figures() {
        let line = settlement(113, 1, -10);
        assert!(line.contains("credited you $ 113"), "{line}");
        assert!(line.contains("at band 1 (-10 %)"), "{line}");
        assert!(settlement(137, 2, 10).contains("at band 2 (+10 %)"));
        assert!(
            line.contains("S4"),
            "the award fund is named as S4's: {line}"
        );
        assert_eq!(
            shortfall(&[String::from("b_02")], 4),
            "When the segment ended, b_02 was dark: draw outran supply, and the brownout shed \
             4 kW."
        );
        assert_eq!(
            shortfall(
                &[
                    String::from("b_01"),
                    String::from("b_02"),
                    String::from("b_04")
                ],
                12
            ),
            "When the segment ended, b_01, b_02 and b_04 were dark: draw outran supply, and the \
             brownout shed 12 kW."
        );
    }

    #[test]
    fn the_recaps_step_failure_says_how_much_matched_and_keeps_the_feeds_words_otherwise() {
        use pharmakos_sim::interpreter::{StepFailed, StepFailure};
        let none_reachable = StepFailed::counted(StepFailure::NoTarget, 3).value();
        assert_eq!(
            recap_step_failed(none_reachable, 1),
            "A step found nothing: 3 matched, none reachable."
        );
        assert_eq!(
            recap_step_failed(none_reachable, 2),
            "Twice, a step found nothing: 3 matched, none reachable."
        );
        assert_eq!(
            recap_step_failed(none_reachable, 4),
            "4 times, a step found nothing: 3 matched, none reachable."
        );
        // Nothing matched, or the step read no feature: the feed's words.
        let words = "A step failed (no_target): it found nothing: nothing matched what it named, \
                     or nothing it matched could be reached.";
        assert_eq!(
            recap_step_failed(StepFailed::counted(StepFailure::NoTarget, 0).value(), 1),
            words
        );
        assert_eq!(
            recap_step_failed(StepFailed::bare(StepFailure::NoTarget).value(), 1),
            words
        );
        // Another failure keeps its words, counted or not.
        assert_eq!(
            recap_step_failed(StepFailed::counted(StepFailure::IllegalSite, 1).value(), 1),
            format!(
                "A step failed (illegal_site): {}.",
                failure_reason(StepFailure::IllegalSite)
            )
        );
        // The feed's own line does not say the count (`fog`'s words).
        assert!(!step_failed(none_reachable).contains("none reachable"));
        // A value that does not decode is said as its code.
        assert_eq!(
            recap_step_failed(10, 1),
            "A step failed with a failure this build does not name (code 10)."
        );
    }

    #[test]
    fn the_map_prose_and_the_standing_prose_say_what_they_know() {
        let prose = map_summary([384, 384, 64], "0x00000000ca5caded", 2);
        assert!(prose.contains("384 by 384 by 64"), "{prose}");
        assert!(prose.contains("0x00000000ca5caded"), "{prose}");
        assert_eq!(
            standing(2, Some((1, 2)), 412),
            "2 seats still standing. Your score is $ 412, which ranks you 1 of 2 on the final \
             audit's terms."
        );
        assert!(standing(1, None, 0).contains("You are out of the match"));
    }

    #[test]
    fn a_one_step_seal_reads_one_route_step() {
        assert_eq!(
            event_text(&event(EventKind::PlanSealed, Some(0), 1)),
            "The playbook of seat 0 was sealed: 1 route step."
        );
        assert_eq!(
            event_text(&event(EventKind::PlanSealed, Some(0), 4)),
            "The playbook of seat 0 was sealed: 4 route steps."
        );
    }

    #[test]
    fn fallback_engaged_names_its_posture() {
        for (id, posture) in [(1, "Hold"), (2, "Shadow"), (3, "Patrol")] {
            let text = event_text(&event(EventKind::FallbackEngaged, Some(0), id));
            assert_eq!(
                text,
                format!("The route ended and the fallback took over: {posture}.")
            );
            assert!(!text.contains("code"), "{text}");
        }
    }

    #[test]
    fn capitalise_is_ascii_and_survives_an_empty_string() {
        assert_eq!(capitalise("seat 1"), "Seat 1");
        assert_eq!(capitalise(""), "");
    }

    #[test]
    fn every_step_failure_is_named_and_said_in_words() {
        use pharmakos_sim::interpreter::StepFailure;
        for failure in StepFailure::ALL {
            let line = event_text(&event(
                EventKind::StepFailed,
                Some(0),
                i64::from(failure.id()),
            ));
            assert!(line.contains(&format!("({})", failure.name())), "{line}");
            assert!(!line.contains("code"), "named, not numbered: {line}");
        }
        let unknown = event_text(&event(
            EventKind::StepFailed,
            Some(0),
            i64::from(StepFailure::RETIRED_NOT_OWN),
        ));
        assert!(unknown.contains("code 10"), "{unknown}");
    }

    #[test]
    fn a_counted_step_failure_reads_as_its_bare_reason() {
        // S1's `fog` put the candidate count on the event's value; the line's
        // words are unchanged by it (decisions-log item 134 (2) (a)).
        use pharmakos_sim::interpreter::{StepFailed, StepFailure};
        for failure in StepFailure::ALL {
            let bare = event_text(&event(
                EventKind::StepFailed,
                Some(0),
                StepFailed::bare(failure).value(),
            ));
            for matched in [0, 3, u32::MAX] {
                let counted = event_text(&event(
                    EventKind::StepFailed,
                    Some(0),
                    StepFailed::counted(failure, matched).value(),
                ));
                assert_eq!(counted, bare, "{failure:?} with {matched} matched");
            }
        }
        let bare = event_text(&event(
            EventKind::StepFailed,
            Some(0),
            i64::from(StepFailure::NoTarget.id()),
        ));
        assert_eq!(
            bare,
            "A step failed (no_target): it found nothing: nothing matched what it named, or \
             nothing it matched could be reached. Its on_fail decided what came next.",
            "the words are the line's before the count"
        );
    }

    #[test]
    fn a_step_failed_value_that_does_not_decode_is_said_as_its_code() {
        use pharmakos_sim::interpreter::{StepFailed, StepFailure};
        let counted = StepFailed::counted(StepFailure::NoTarget, 3).value();
        for value in [-1, 1 << 9, 1 << 48, (3 << 16) | 2, counted | (1 << 12)] {
            let line = event_text(&event(EventKind::StepFailed, Some(0), value));
            assert!(
                line.contains(&format!("(code {value})")),
                "{value:#x}: {line}"
            );
        }
        assert_eq!(
            event_text(&event(EventKind::StepFailed, Some(0), 10)),
            "A step failed with a failure this build does not name (code 10). Its on_fail \
             decided what came next.",
            "the words are the line's before the count"
        );
    }

    #[test]
    fn this_round_says_each_covering_step_or_nothing() {
        use super::{RoundRead, this_round};
        assert_eq!(this_round(&[]), None);
        assert_eq!(
            this_round(&[RoundRead::Places {
                step: 0,
                x: 150,
                y: 20,
                dollars: 60
            }]),
            Some(String::from(
                "This round: step 1 places a new beacon near (150, 20), $ 60."
            ))
        );
        assert_eq!(
            this_round(&[
                RoundRead::NoneCoverable {
                    step: 1,
                    kind: "vent"
                },
                RoundRead::NoneReachable {
                    step: 2,
                    matched: 3
                },
                RoundRead::NothingMatches {
                    step: 3,
                    kind: "seam"
                },
                RoundRead::Fails {
                    step: 4,
                    failure: "commander_dead"
                },
            ]),
            Some(String::from(
                "This round: step 2 finds no vent you can cover; step 3 finds nothing to cover: \
                 3 matched, none reachable; step 4 finds no seam to cover; step 5 fails \
                 `commander_dead` before it reads the map."
            ))
        );
    }
}
