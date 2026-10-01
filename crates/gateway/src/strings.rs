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
        EventKind::StepFailed => format!(
            "A step failed with failure code {}, and its on_fail decided what came next.",
            event.value
        ),
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

/// `get_recap`'s prose.
///
/// `outcome` is the reason and **every** winner, in seat order: the last seat
/// standing, the final audit's single winner, or the seats a shared win ties
/// (spec section 3: "only an exact tie on every term is recorded as a shared
/// win"). The recap names each of them.
#[must_use]
pub fn recap(round: u32, ticks: u32, outcome: Option<(MatchEndReason, &[u8])>) -> String {
    let segment = format!("Round {round} ran {ticks} ticks.");
    let Some((reason, winners)) = outcome else {
        return format!("{segment} The match continues.");
    };
    let ended = format!("{segment} The match ended: {}.", reason.name());
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

/// The seat's own displayed standing, when the audit that settles it has not
/// been built.
///
/// PLACEHOLDER: the score and the rank are the full audit score of spec
/// section 3. The audit now exists (`pharmakos_sim::audit`, which names a
/// round-limit winner), but a seat's own live score and rank on the briefing
/// are the economy's surfaces, S1's `econ` lane's (owner, at S1). Until then
/// the standing carries the one number this surface shows -- how many seats
/// are still standing -- and says so rather than showing a zero a player would
/// read as a rank.
#[must_use]
pub fn standing(living: u32) -> String {
    format!(
        "{} still standing. Your score and rank are settled by the final audit, which the \
         economy has not been built to run yet.",
        count(living, "seat", "seats"),
    )
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
    use super::{briefing, capitalise, clock, count, event_text, map_summary, recap, standing};
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
    fn the_map_prose_and_the_standing_prose_say_what_they_know() {
        let prose = map_summary([384, 384, 64], "0x00000000ca5caded", 2);
        assert!(prose.contains("384 by 384 by 64"), "{prose}");
        assert!(prose.contains("0x00000000ca5caded"), "{prose}");
        assert!(standing(2).starts_with("2 seats still standing."));
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
}
