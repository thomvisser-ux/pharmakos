// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a Lull holds of the Push before it that lives only in the running
//! gateway, as a save carries it, and how a resume puts it back (lane
//! `opsave`; the owner's ruling, decisions-log item 177 (5)).
//!
//! A Lull's seat reads the Push it has just watched in two places the sim's
//! snapshot does not hold, because both are the gateway's derived state:
//!
//! * **the segment feed** ([`crate::feed::SegmentFeed`]), which the gateway
//!   opens when a Push begins and keeps through its recap and the next Lull:
//!   every line of the Push (its step outcomes and failures, its deaths, its
//!   `segment_ended`, kill credit as it was told), the Ledger's memos and the
//!   safe-playbook lines, the recap's and the Lull's own lines, each with its
//!   audience and the seats that witnessed it as it was published, and the
//!   60-second digests and cursors `get_segment_feed`, `wait_for` and
//!   `list_beacons` compute from them;
//! * **the recap's record** ([`super::RecapFacts`]): the segment's length,
//!   the Ledger's credits, each seat's held value, dark beacons, failures and
//!   tally as the segment ended, which `get_recap` reads in the recap, the Lull
//!   and the Push that follow; or the record's loss, when the sim's bus dropped
//!   the segment's end.
//!
//! A save written without them resumed into a Lull whose feed showed no Push
//! and whose recap told none, so a seat read the last Push as unread (the
//! operator's `PushUnread::Resumed`). [`LastPush`] is both, captured where a
//! save is made ([`Surface::last_push`]) and put back by the one restore
//! ([`Surface::restore_last_push`], from `Surface::resume`), so every read of
//! a resumed Lull answers byte for byte what the live Lull did.
//!
//! # Which boundary carries what
//!
//! A `lull` save carries the feed and the record: the Lull resumes with both.
//! A `sealed` save is made as a Push begins, once its feed is that Push's
//! own, and resumes **into** that Push, which replays it line for line; it
//! carries the record alone, because `get_recap` in that Push still tells the
//! round before it.
//!
//! # Secrecy
//!
//! One bus, as the live gateway keeps it: each line with its audience and its
//! witnesses, so the fog filter decides what each seat is shown of the
//! resumed feed exactly as it decided of the live one, and a seat's private
//! line reaches no other seat through the save's read-back. The file is the
//! private match cache's, which holds every seat's playbooks already
//! (AGENTS.md section 7).
//!
//! # Bounds
//!
//! The save is read from disk, so every length and count in it is held to a
//! bound the live feed already has, and a save past one is refused as damaged
//! ([`crate::error::Code::InvalidArgument`]), never cut:
//!
//! * a line's text is at most [`crate::feed::MAX_LINE_CHARS`] and its kind is
//!   a [`Kind`] (at most [`crate::feed::MAX_KIND_CHARS`], the scenario
//!   grammar), and the kind is the sim's ([`EventKind::from_name`]) or one of
//!   the gateway's two lines, each with the shape the gateway gives it;
//! * a line's time is a whole tick of game time, not before the segment's
//!   start, never earlier than the line before it, and not after the tick
//!   the saved world stands at;
//! * the lines are at most [`EVENT_BUS_CAPACITY`] for every drain the
//!   segment made (one a tick it played, and [`DRAINS_BEYOND_TICKS`] more),
//!   plus a safe-playbook line a seat and a memo a [`Memo`];
//! * every seat a line, a witness set or the record names is one of the
//!   match's, a witness set never names its line's owner, and each list the
//!   record keeps a seat is ordered, unique and no longer than the match's
//!   seats (the dark beacons no more than the world's beacon rows);
//! * the feed's anchor is the tick its Push began, which the world fixes:
//!   its round's whole length before the world's tick (`check_anchor`);
//! * a count of dropped events is any `u64`: the live gateway counts a
//!   segment's drops in a `u64` with a checked add, so every value the codec
//!   reads is one it could have held, and none is refused past it.

use super::{DarkAtEnd, RecapFacts, SAFE_PLAYBOOK_FILED_KIND, Surface};
use crate::error::Error;
use crate::feed::{Event, Kind, Witnesses};
use crate::fog::Audience;
use crate::probation::{LEDGER_MEMO_KIND, Memo};
use crate::save::Boundary;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;
use pharmakos_proto::json::grammar::Said;
use pharmakos_proto::said;
use pharmakos_sim::EVENT_BUS_CAPACITY;
use pharmakos_sim::events::EventKind;
use pharmakos_sim::math::quantity::{Kw, MS_PER_TICK, Money, Ms, Tick};
use pharmakos_sim::tables::{SeatId, Tally};

/// How many drains of the sim's bus a segment's feed takes beyond one a tick
/// it played: the Push's opening drain at its first tick's anchor, and the
/// recap's end and the next Lull's opening at its last ([`Surface`]'s
/// `seal_and_open_push`, `end_recap` and `open_lull`), each at most
/// [`EVENT_BUS_CAPACITY`] lines. Round 1's opening feed takes two at tick 0
/// (the attach and the opening Lull), within the same three.
pub const DRAINS_BEYOND_TICKS: u64 = 3;

/// What a Lull holds of the Push before it that a save carries: the feed and
/// the recap's record (the module doc).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LastPush {
    /// The feed, in a `lull` save; none in a `sealed` one, whose Push
    /// replays its own.
    feed: Option<SavedFeed>,
    /// The recap's record, in either.
    recap: SavedRecap,
}

/// A segment's feed as a save carries it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct SavedFeed {
    /// The round it was opened for.
    round: u32,
    /// The segment of that round.
    segment: u32,
    /// The sim tick it was opened at, which every line's time counts from.
    anchor: Tick,
    /// Every line, in order, with the seats that witnessed it.
    lines: Vec<(Event, Witnesses)>,
    /// What the sim's bus has dropped since the segment's end, which the next
    /// segment's record takes (`segment_dropped`).
    dropped_since: u64,
}

/// The recap's record as a save carries it.
#[derive(Clone, PartialEq, Eq, Debug)]
enum SavedRecap {
    /// No segment has ended yet.
    None,
    /// The last segment's end, as the sim reported it.
    Facts(RecapFacts),
    /// The last segment's end was dropped by the bus, with this many events.
    Lost(u64),
}

impl LastPush {
    /// The save's `last_push` member.
    #[must_use]
    pub fn render(&self) -> Json {
        let feed = nullable(self.feed.as_ref(), render_feed);
        let recap = match &self.recap {
            SavedRecap::None => object(vec![("none", Json::Bool(true))]),
            SavedRecap::Lost(dropped) => object(vec![("lost", unsigned(*dropped))]),
            SavedRecap::Facts(facts) => object(vec![("facts", render_facts(facts))]),
        };
        object(vec![("feed", feed), ("recap", recap)])
    }

    /// Read a save's `last_push` member: its shape and the bounds that need
    /// no match to check. [`Surface::restore_last_push`] checks the rest
    /// against the match it is put into.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a member this build did not
    /// write: a field missing or of the wrong shape, a line past a bound, a
    /// seat past the eight a witness set holds.
    pub fn parse(value: &Json) -> Result<LastPush, Error> {
        let feed = match member(value, "feed")? {
            Json::Null => None,
            feed => Some(parse_feed(feed)?),
        };
        let recap_json = member(value, "recap")?;
        let recap = match recap_json {
            Json::Object(entries) => match entries.as_slice() {
                [(key, Json::Bool(true))] if key == "none" => SavedRecap::None,
                [(key, dropped)] if key == "lost" => SavedRecap::Lost(u64_of(dropped, "lost")?),
                [(key, facts)] if key == "facts" => SavedRecap::Facts(parse_facts(facts)?),
                _ => {
                    return Err(damaged("its recap is neither `none`, `lost` nor `facts`"));
                }
            },
            _ => return Err(damaged("its recap is not an object")),
        };
        Ok(LastPush { feed, recap })
    }
}

impl Surface {
    /// What a save made at `boundary` carries of the last Push (the module
    /// doc): the feed at a `lull` boundary, and the recap's record at both.
    ///
    /// **Host-side**, like [`Surface::lull_save`]: what a save is built from.
    /// It reads the whole bus, every seat's lines, so no handler names it
    /// (`tests/confinement.rs`'s `no_handler_reads_the_save_or_the_replay`).
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the surface holds a recap's
    /// record and its loss at once, which [`Surface`]'s own bookkeeping never
    /// does.
    pub fn last_push(&self, boundary: Boundary) -> Result<LastPush, Error> {
        let feed = match boundary {
            Boundary::Sealed | Boundary::Ended => None,
            Boundary::Lull => Some(SavedFeed {
                round: self.feed.round(),
                segment: self.feed.segment(),
                anchor: self.feed_anchor,
                lines: self
                    .feed
                    .lines()
                    .map(|(event, witnesses)| (event.clone(), witnesses))
                    .collect(),
                dropped_since: self.segment_dropped,
            }),
        };
        let recap = match (&self.recap, self.recap_lost) {
            (None, None) => SavedRecap::None,
            (Some(facts), None) => SavedRecap::Facts(facts.clone()),
            (None, Some(dropped)) => SavedRecap::Lost(dropped),
            (Some(_), Some(_)) => {
                return Err(Error::internal(
                    "the surface holds the last segment's record and its loss at once",
                ));
            }
        };
        Ok(LastPush { feed, recap })
    }

    /// Put a save's [`LastPush`] back into a surface resumed at `boundary`:
    /// the feed, its anchor and its cursors' snapshot exactly as the saved
    /// Lull held them, and the recap's record. **Host-side**: the one
    /// restore's, `Surface::resume`'s, and no handler's
    /// (`tests/confinement.rs`'s `no_handler_reads_the_save_or_the_replay`),
    /// called
    /// once the seats' stores are restored (a memo line is checked against
    /// the memos its seat holds) and before a `sealed` save's Push opens.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a member that does not fit
    /// this match (the module doc's bounds): a feed in a `sealed` save or none
    /// in a `lull` one, a round, a tick or a seat the match has not got, a
    /// line out of time or past the count, a line whose kind, audience,
    /// witnesses or memo the gateway never writes, a record whose lists are
    /// not the match's seats. As [`Surface::host`] otherwise.
    pub fn restore_last_push(&mut self, boundary: Boundary, saved: &LastPush) -> Result<(), Error> {
        let host = self.host()?;
        let round = host.runner().round();
        let now = host.runner().tick();
        let world_seats: Vec<SeatId> = host
            .world()
            .seats()
            .seats()
            .iter()
            .copied()
            .map(SeatId::new)
            .collect();
        let beacon_rows = host.world().beacons().ids().len();
        let ours: Vec<SeatId> = self.seats.iter().map(|slot| slot.seat).collect();
        let feed = match (boundary, saved.feed.as_ref()) {
            (Boundary::Lull, Some(feed)) => Some(feed),
            (Boundary::Sealed | Boundary::Ended, None) => None,
            (Boundary::Lull, None) => {
                return Err(damaged(
                    "a save made in a Lull carries that Lull's feed, and this one does not",
                ));
            }
            (Boundary::Sealed | Boundary::Ended, Some(_)) => {
                return Err(damaged(
                    "a save made as a Push began carries no feed, since its Push replays its \
                     own, and this one does",
                ));
            }
        };
        // Every count of dropped events is a `u64` the codec read, which is every
        // count the live gateway could have held ([`Surface::count_drops`]
        // adds a segment's drops with `checked_add`, a typed `INTERNAL` at its
        // cause): no further bound applies, and none is written.
        match &saved.recap {
            SavedRecap::None | SavedRecap::Lost(_) => {}
            SavedRecap::Facts(facts) => {
                check_facts(
                    facts,
                    round,
                    feed.map(|feed| feed.round),
                    feed.map(|feed| now.since(feed.anchor)),
                    host_ticks_of(self, facts.round)?,
                    &world_seats,
                    beacon_rows,
                )?;
            }
        }
        // The record first, then the feed's anchor against it.
        let elapsed = match feed {
            Some(feed) => Some(check_anchor(
                feed,
                &saved.recap,
                round,
                now,
                host_ticks_of(self, feed.round)?,
            )?),
            None => None,
        };
        if let (Some(feed), Some(elapsed)) = (feed, elapsed) {
            self.check_lines(feed, elapsed, &ours)?;
            let mut rebuilt =
                crate::feed::SegmentFeed::new(self.match_seed, feed.round, feed.segment);
            for (event, witnesses) in &feed.lines {
                rebuilt
                    .publish(event.clone(), *witnesses)
                    .map_err(|refused| damaged(said!("a line of its feed: {}", refused.said())))?;
            }
            self.feed = rebuilt;
            self.feed_anchor = feed.anchor;
            self.segment_dropped = feed.dropped_since;
        }
        let (recap, lost) = match &saved.recap {
            SavedRecap::None => (None, None),
            SavedRecap::Facts(facts) => (Some(facts.clone()), None),
            SavedRecap::Lost(dropped) => (None, Some(*dropped)),
        };
        self.recap = recap;
        self.recap_lost = lost;
        Ok(())
    }

    /// The feed's lines against this match (the module doc's bounds).
    fn check_lines(&self, feed: &SavedFeed, elapsed: u32, ours: &[SeatId]) -> Result<(), Error> {
        // A safe-playbook line a seat, and a memo a `Memo`, beside the sim's.
        let extra = ours
            .len()
            .checked_add(Memo::ALL.len())
            .ok_or_else(|| Error::internal("more seats and memos than a count holds"))?;
        let most = most_lines(elapsed, extra)?;
        let lines = u64::try_from(feed.lines.len())
            .map_err(|_| Error::internal("more lines than a count holds"))?;
        if lines > most {
            return Err(damaged(said!(
                "its feed holds {} lines, past the {} a segment of {} ticks can",
                lines,
                most,
                elapsed
            )));
        }
        let latest = Ms::from_ticks(elapsed);
        let in_match = |seat: SeatId| ours.contains(&seat);
        for (event, witnesses) in &feed.lines {
            if event.at_ms > latest || event.at_ms.raw().rem_euclid(MS_PER_TICK) != 0 {
                return Err(damaged(said!(
                    "a line of its feed is dated {} ms, which is not a tick of the {} ms its \
                     segment has run",
                    event.at_ms.raw(),
                    latest.raw()
                )));
            }
            let named = match event.audience {
                Audience::Public => None,
                Audience::Private(seat) => Some(seat),
                Audience::World { owner, .. } => owner,
            };
            if named.is_some_and(|seat| !in_match(seat))
                || witnesses.seats().into_iter().any(|seat| !in_match(seat))
            {
                return Err(damaged(
                    "a line of its feed names a seat this match has not got",
                ));
            }
            match event.audience {
                Audience::World { owner, .. } => {
                    if owner.is_some_and(|owner| witnesses.contains(owner)) {
                        return Err(damaged(
                            "a line of its feed is witnessed by the seat it belongs to",
                        ));
                    }
                }
                Audience::Public | Audience::Private(_) => {
                    if *witnesses != Witnesses::NONE {
                        return Err(damaged(
                            "a line of its feed that fog does not decide carries witnesses",
                        ));
                    }
                }
            }
            self.check_line_kind(event)?;
        }
        Ok(())
    }

    /// A line's kind, and the shape the gateway gives a line of that kind.
    fn check_line_kind(&self, event: &Event) -> Result<(), Error> {
        let kind = event.kind.name();
        if kind == LEDGER_MEMO_KIND {
            let Some(memo) = event.memo else {
                return Err(damaged("a memo line of its feed carries no memo"));
            };
            let to = match event.audience {
                Audience::Private(seat) => seat,
                Audience::Public | Audience::World { .. } => {
                    return Err(damaged("a memo line of its feed is not the seat's own"));
                }
            };
            let held = self
                .seats
                .iter()
                .find(|slot| slot.seat == to)
                .is_some_and(|slot| slot.memos.contains(&memo));
            if Some(to) != self.human_seat || !held {
                return Err(damaged(said!(
                    "its feed tells seat {} the memo `{}`, which the seat was not issued",
                    to.raw(),
                    memo.name()
                )));
            }
            if event.text != memo.text(&self.rules)? {
                return Err(damaged(said!(
                    "its feed's memo `{}` is not in the Ledger's words",
                    memo.name()
                )));
            }
            return Ok(());
        }
        if event.memo.is_some() {
            return Err(damaged(said!(
                "a `{}` line of its feed carries a memo",
                pharmakos_proto::json::grammar::quoted(kind)
            )));
        }
        if kind == SAFE_PLAYBOOK_FILED_KIND {
            let words = [
                crate::strings::SAFE_PLAYBOOK_FILED,
                crate::strings::SAFE_PLAYBOOK_FILED_ON_READY,
                crate::strings::SAFE_PLAYBOOK_FILED_RAN_OUT,
            ];
            if !matches!(event.audience, Audience::Private(_))
                || !words.contains(&event.text.as_str())
            {
                return Err(damaged(
                    "a safe-playbook line of its feed is not the gateway's own words to one seat",
                ));
            }
            return Ok(());
        }
        if EventKind::from_name(kind).is_none() {
            return Err(damaged(said!(
                "its feed holds a `{}` line, a kind neither the sim nor the gateway writes",
                pharmakos_proto::json::grammar::quoted(kind)
            )));
        }
        Ok(())
    }
}

/// The feed's round, segment and anchor against the world the save restores,
/// and the ticks its segment has run. The feed was opened for this round (a
/// Lull whose feed shows no Push, as after a version-3 save's resume) or the
/// round before (the Push this Lull follows), always as the round's segment 0
/// (every segment the gateway opens is), at the tick its Push began.
///
/// That tick is the world's own, whatever the record says: a Push a Lull
/// follows ran its round's whole length (`full`, the match state's ladder),
/// because the only other end a Push has, the one-tick rule, decides the
/// match, and a decided match opens no Lull. So the anchor of the round
/// before's feed is exactly `full` ticks before the world's tick, with the
/// record's facts, its loss or none; with facts, they say the same ticks.
/// The anchor of this round's own feed is the world's tick.
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] for a round, segment or anchor
/// the match's world does not bear out.
fn check_anchor(
    feed: &SavedFeed,
    recap: &SavedRecap,
    round: u32,
    now: Tick,
    full: u32,
) -> Result<u32, Error> {
    let opened_here = feed.round == round;
    let opened_before = feed.round.checked_add(1) == Some(round);
    if feed.segment != 0 || !(opened_here || opened_before) || feed.anchor > now {
        return Err(damaged(said!(
            "its feed was opened as segment {} of round {} at tick {}, which the world, in round \
             {} at tick {}, does not bear out",
            feed.segment,
            feed.round,
            feed.anchor.raw(),
            round,
            now.raw()
        )));
    }
    let elapsed = now.since(feed.anchor);
    let ran = if opened_here { 0 } else { full };
    let said = match recap {
        SavedRecap::Facts(facts) if opened_before => Some(facts.ticks),
        SavedRecap::Facts(_) | SavedRecap::None | SavedRecap::Lost(_) => None,
    };
    if elapsed != ran || said.is_some_and(|said| said != ran) {
        return Err(damaged(said!(
            "its feed's anchor, tick {}, is not where its segment began: the world stands at \
             tick {}, {} ticks after a Push of round {} began",
            feed.anchor.raw(),
            now.raw(),
            ran,
            feed.round
        )));
    }
    Ok(elapsed)
}

/// The most ticks round `round`'s segment can run: its length on the
/// match state's own ladder, in whole ticks, rounded up.
///
/// # Errors
///
/// As [`Surface::host`].
fn host_ticks_of(surface: &Surface, round: u32) -> Result<u32, Error> {
    Ok(surface
        .host()?
        .world()
        .match_state()
        .length_for_round(round)
        .to_ticks_ceil())
}

/// The recap's record against this match (the module doc's bounds).
fn check_facts(
    facts: &RecapFacts,
    round: u32,
    feed_round: Option<u32>,
    elapsed: Option<u32>,
    longest: u32,
    world_seats: &[SeatId],
    beacon_rows: usize,
) -> Result<(), Error> {
    if facts.round >= round || feed_round.is_some_and(|feed| feed != facts.round) {
        return Err(damaged(said!(
            "its recap tells round {}, which is not the round before round {} or not its feed's",
            facts.round,
            round
        )));
    }
    // The segment ran at most its round's length, the match state's own ladder
    // (`MatchState::length_for_round`): a record of a longer one is not this
    // match's. With a feed, its anchor holds the ticks exactly
    // (`check_anchor`).
    if facts.ticks > longest {
        return Err(damaged(said!(
            "its recap's segment ran {} ticks, past the {} its round's length allows",
            facts.ticks,
            longest
        )));
    }
    let in_match = |seat: &SeatId| world_seats.contains(seat);
    let ascending = |seats: &[SeatId]| {
        seats
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if a.raw() < b.raw()))
    };
    let settled: Vec<SeatId> = facts.settled.iter().map(|(seat, _)| *seat).collect();
    let dark: Vec<SeatId> = facts.dark.iter().map(|dark| dark.seat).collect();
    let held: Vec<SeatId> = facts.held.iter().map(|(seat, _)| *seat).collect();
    let tallies: Vec<SeatId> = facts.tallies.iter().map(|(seat, _)| *seat).collect();
    if !settled.iter().all(in_match)
        || !ascending(&settled)
        || !dark.iter().all(in_match)
        || !ascending(&dark)
        || held != world_seats
        || {
            let mut sorted = world_seats.to_vec();
            sorted.sort_unstable_by_key(|seat| seat.raw());
            tallies != sorted
        }
        || !facts.failures.iter().all(|(seat, _)| in_match(seat))
    {
        return Err(damaged(
            "its recap's lists do not name this match's seats, each once and in order",
        ));
    }
    for dark in &facts.dark {
        let ordered = dark
            .ordinals
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if a < b));
        if !ordered || dark.ordinals.len() > beacon_rows {
            return Err(damaged(said!(
                "its recap's dark beacons of seat {} are not the beacons a seat can have, each \
                 once and in order",
                dark.seat.raw()
            )));
        }
    }
    // The segment's own length, as the feed saw it or, with no feed (a
    // `sealed` save), as the record says it ran.
    let ticks = match elapsed {
        Some(elapsed) => elapsed,
        None => facts.ticks,
    };
    let most = most_lines(ticks, 0)?;
    let failures = u64::try_from(facts.failures.len())
        .map_err(|_| Error::internal("more failures than a count holds"))?;
    if failures > most {
        return Err(damaged(said!(
            "its recap holds {} failed steps, past the {} a segment of {} ticks can report",
            failures,
            most,
            ticks
        )));
    }
    Ok(())
}

// -------------------------------------------------------------------------
// The codec
// -------------------------------------------------------------------------

fn render_feed(feed: &SavedFeed) -> Json {
    let lines: Vec<Json> = feed
        .lines
        .iter()
        .map(|(event, witnesses)| {
            let audience = match event.audience {
                Audience::Public => object(vec![("to", text("public"))]),
                Audience::Private(seat) => {
                    object(vec![("to", text("private")), ("seat", seat_json(seat))])
                }
                Audience::World { owner, at } => object(vec![
                    ("to", text("world")),
                    ("owner", nullable(owner, seat_json)),
                    (
                        "at",
                        Json::Array(vec![signed(at.x), signed(at.y), signed(at.z)]),
                    ),
                ]),
            };
            object(vec![
                ("at_ms", signed(event.at_ms.raw())),
                ("kind", text(event.kind.name())),
                ("text", Json::String(event.text.clone())),
                ("audience", audience),
                (
                    "witnesses",
                    Json::Array(witnesses.seats().into_iter().map(seat_json).collect()),
                ),
                ("memo", nullable(event.memo, |memo| text(memo.name()))),
            ])
        })
        .collect();
    object(vec![
        ("round", unsigned(feed.round)),
        ("segment", unsigned(feed.segment)),
        ("anchor_tick", unsigned(feed.anchor.raw())),
        ("dropped_since", unsigned(feed.dropped_since)),
        ("lines", Json::Array(lines)),
    ])
}

fn parse_feed(value: &Json) -> Result<SavedFeed, Error> {
    let mut lines: Vec<(Event, Witnesses)> = Vec::new();
    for line in array_at(value, "lines")? {
        let at_ms = Ms::new(i32_of(member(line, "at_ms")?, "at_ms")?);
        let kind = Kind::new(string_at(line, "kind")?)
            .map_err(|refused| damaged(said!("a line's kind: {}", refused.said())))?;
        let words = string_at(line, "text")?;
        // A line's length is held where every line is, live or restored:
        // `SegmentFeed::publish`, which the restore publishes through.
        let audience_json = member(line, "audience")?;
        let audience = match string_at(audience_json, "to")? {
            "public" => {
                only_keys(audience_json, &["to"])?;
                Audience::Public
            }
            "private" => {
                only_keys(audience_json, &["to", "seat"])?;
                Audience::Private(seat_of(member(audience_json, "seat")?)?)
            }
            "world" => {
                only_keys(audience_json, &["to", "owner", "at"])?;
                let owner = match member(audience_json, "owner")? {
                    Json::Null => None,
                    seat => Some(seat_of(seat)?),
                };
                let at = match array_at(audience_json, "at")? {
                    [x, y, z] => Voxel {
                        x: i32_of(x, "at")?,
                        y: i32_of(y, "at")?,
                        z: i32_of(z, "at")?,
                    },
                    _ => return Err(damaged("a line's place is not three coordinates")),
                };
                Audience::World { owner, at }
            }
            _ => return Err(damaged("a line's audience is not public, private or world")),
        };
        let listed = array_at(line, "witnesses")?;
        if listed.len() > 8 {
            return Err(damaged("a line is witnessed by more than eight seats"));
        }
        let mut seats: Vec<SeatId> = Vec::with_capacity(listed.len());
        for seat in listed {
            let seat = seat_of(seat)?;
            if seats.last().is_some_and(|last| last.raw() >= seat.raw()) {
                return Err(damaged("a line's witnesses are not ascending, each once"));
            }
            seats.push(seat);
        }
        let witnesses = Witnesses::of_seats(&seats)
            .map_err(|refused| damaged(said!("a line's witnesses: {}", refused.said())))?;
        let memo = match member(line, "memo")? {
            Json::Null => None,
            Json::String(name) => {
                Some(Memo::parse(name).map_err(|refused| damaged(refused.said()))?)
            }
            _ => return Err(damaged("a line's memo is not a memo's name")),
        };
        only_keys(
            line,
            &["at_ms", "kind", "text", "audience", "witnesses", "memo"],
        )?;
        lines.push((
            Event {
                at_ms,
                kind,
                text: words.to_owned(),
                audience,
                memo,
            },
            witnesses,
        ));
    }
    only_keys(
        value,
        &["round", "segment", "anchor_tick", "dropped_since", "lines"],
    )?;
    Ok(SavedFeed {
        round: u32_of(member(value, "round")?, "round")?,
        segment: u32_of(member(value, "segment")?, "segment")?,
        anchor: Tick::new(u32_of(member(value, "anchor_tick")?, "anchor_tick")?),
        lines,
        dropped_since: u64_of(member(value, "dropped_since")?, "dropped_since")?,
    })
}

fn render_facts(facts: &RecapFacts) -> Json {
    let seat_value = |seat: SeatId, key: &str, value: Json| {
        object(vec![("seat", seat_json(seat)), (key, value)])
    };
    object(vec![
        ("round", unsigned(facts.round)),
        ("ticks", unsigned(facts.ticks)),
        (
            "settled",
            Json::Array(
                facts
                    .settled
                    .iter()
                    .map(|(seat, credit)| seat_value(*seat, "dollars", signed(*credit)))
                    .collect(),
            ),
        ),
        (
            "dark",
            Json::Array(
                facts
                    .dark
                    .iter()
                    .map(|dark| {
                        object(vec![
                            ("seat", seat_json(dark.seat)),
                            (
                                "ordinals",
                                Json::Array(dark.ordinals.iter().map(|o| unsigned(*o)).collect()),
                            ),
                            ("shed_kw", signed(dark.shed.raw())),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "held",
            Json::Array(
                facts
                    .held
                    .iter()
                    .map(|(seat, held)| {
                        seat_value(
                            *seat,
                            "dollars",
                            nullable(*held, |money| signed(money.raw())),
                        )
                    })
                    .collect(),
            ),
        ),
        (
            "failures",
            Json::Array(
                facts
                    .failures
                    .iter()
                    .map(|(seat, value)| seat_value(*seat, "value", signed(*value)))
                    .collect(),
            ),
        ),
        ("dropped", unsigned(facts.dropped)),
        (
            "tallies",
            Json::Array(
                facts
                    .tallies
                    .iter()
                    .map(|(seat, tally)| {
                        object(vec![
                            ("seat", seat_json(*seat)),
                            ("destroyed", signed(tally.destroyed.raw())),
                            ("kills", unsigned(tally.kills)),
                            ("shared_kills", unsigned(tally.shared_kills)),
                            ("losses", unsigned(tally.losses)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn parse_facts(value: &Json) -> Result<RecapFacts, Error> {
    only_keys(
        value,
        &[
            "round", "ticks", "settled", "dark", "held", "failures", "dropped", "tallies",
        ],
    )?;
    let seated = |key: &str| -> Result<Vec<&Json>, Error> {
        let rows = array_at(value, key)?;
        if rows.len() > 8 {
            return Err(damaged(said!(
                "its recap's `{}` names more seats than a match has",
                pharmakos_proto::json::grammar::quoted(key)
            )));
        }
        Ok(rows.iter().collect())
    };
    let mut settled = Vec::new();
    for row in seated("settled")? {
        only_keys(row, &["seat", "dollars"])?;
        settled.push((
            seat_of(member(row, "seat")?)?,
            i64_of(member(row, "dollars")?, "dollars")?,
        ));
    }
    let mut dark = Vec::new();
    for row in seated("dark")? {
        only_keys(row, &["seat", "ordinals", "shed_kw"])?;
        let listed = array_at(row, "ordinals")?;
        if listed.len() > usize::from(u16::MAX) {
            return Err(damaged(
                "its recap lists more dark beacons than a seat can have",
            ));
        }
        let mut ordinals = Vec::with_capacity(listed.len());
        for ordinal in listed {
            ordinals.push(u32_of(ordinal, "ordinals")?);
        }
        dark.push(DarkAtEnd {
            seat: seat_of(member(row, "seat")?)?,
            ordinals,
            shed: Kw::new(i32_of(member(row, "shed_kw")?, "shed_kw")?),
        });
    }
    let mut held = Vec::new();
    for row in seated("held")? {
        only_keys(row, &["seat", "dollars"])?;
        let dollars = match member(row, "dollars")? {
            Json::Null => None,
            dollars => Some(Money::new(i64_of(dollars, "dollars")?)),
        };
        held.push((seat_of(member(row, "seat")?)?, dollars));
    }
    let mut failures = Vec::new();
    for row in array_at(value, "failures")? {
        only_keys(row, &["seat", "value"])?;
        failures.push((
            seat_of(member(row, "seat")?)?,
            i64_of(member(row, "value")?, "value")?,
        ));
    }
    let mut tallies = Vec::new();
    for row in seated("tallies")? {
        only_keys(
            row,
            &["seat", "destroyed", "kills", "shared_kills", "losses"],
        )?;
        tallies.push((
            seat_of(member(row, "seat")?)?,
            Tally {
                destroyed: Money::new(i64_of(member(row, "destroyed")?, "destroyed")?),
                kills: u32_of(member(row, "kills")?, "kills")?,
                shared_kills: u32_of(member(row, "shared_kills")?, "shared_kills")?,
                losses: u32_of(member(row, "losses")?, "losses")?,
            },
        ));
    }
    Ok(RecapFacts {
        round: u32_of(member(value, "round")?, "round")?,
        ticks: u32_of(member(value, "ticks")?, "ticks")?,
        settled,
        dark,
        held,
        failures,
        dropped: u64_of(member(value, "dropped")?, "dropped")?,
        tallies,
    })
}

// -------------------------------------------------------------------------
// JSON helpers: the save's own spelling (`crate::save`), each refusal the
// save's `damaged` sentence
// -------------------------------------------------------------------------

/// The refusal for a member that is not one this build wrote: the save's own
/// sentence ([`crate::save`]'s), so a resume says one thing however it fails.
fn damaged(why: impl Into<Said>) -> Error {
    Error::invalid(said!(
        "this is not a save this build can resume: its last Push: {}",
        why.into()
    ))
}

fn object(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn text(value: &str) -> Json {
    Json::String(value.to_owned())
}

fn unsigned(value: impl Into<u64>) -> Json {
    Json::Number(value.into().to_string())
}

fn signed(value: impl Into<i64>) -> Json {
    Json::Number(value.into().to_string())
}

fn seat_json(seat: SeatId) -> Json {
    unsigned(seat.raw())
}

/// `null` for none, the value's JSON for one: the save's spelling of an
/// optional field.
fn nullable<T>(value: Option<T>, json: impl FnOnce(T) -> Json) -> Json {
    match value {
        Some(value) => json(value),
        None => Json::Null,
    }
}

/// The most sim lines a segment of `ticks` ticks can put on its feed:
/// [`EVENT_BUS_CAPACITY`] for each of its drains, one a tick it played and
/// [`DRAINS_BEYOND_TICKS`] more, plus `extra` lines of the gateway's own.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a sum past `u64`, which a `u32` of
/// ticks, a 512-event bus and a few seats cannot reach.
fn most_lines(ticks: u32, extra: usize) -> Result<u64, Error> {
    let past = || Error::internal("a segment's line bound does not fit a count");
    let capacity = u64::try_from(EVENT_BUS_CAPACITY).map_err(|_| past())?;
    let extra = u64::try_from(extra).map_err(|_| past())?;
    u64::from(ticks)
        .checked_add(DRAINS_BEYOND_TICKS)
        .and_then(|drains| drains.checked_mul(capacity))
        .and_then(|lines| lines.checked_add(extra))
        .ok_or_else(past)
}

/// Refuse an object holding a key this build does not write.
fn only_keys(value: &Json, keys: &[&str]) -> Result<(), Error> {
    let Json::Object(entries) = value else {
        return Err(damaged("an entry is not an object"));
    };
    match entries
        .iter()
        .find(|(key, _)| !keys.contains(&key.as_str()))
    {
        Some((key, _)) => Err(damaged(said!(
            "it holds `{}`, which this build does not write",
            pharmakos_proto::json::grammar::quoted(key)
        ))),
        None => Ok(()),
    }
}

fn member<'a>(value: &'a Json, key: &str) -> Result<&'a Json, Error> {
    value.get(key).ok_or_else(|| {
        damaged(said!(
            "it has no `{}`",
            pharmakos_proto::json::grammar::quoted(key)
        ))
    })
}

fn string_at<'a>(value: &'a Json, key: &str) -> Result<&'a str, Error> {
    match member(value, key)? {
        Json::String(words) => Ok(words.as_str()),
        _ => Err(damaged(said!(
            "`{}` is not a string",
            pharmakos_proto::json::grammar::quoted(key)
        ))),
    }
}

fn array_at<'a>(value: &'a Json, key: &str) -> Result<&'a [Json], Error> {
    match member(value, key)? {
        Json::Array(items) => Ok(items.as_slice()),
        _ => Err(damaged(said!(
            "`{}` is not an array",
            pharmakos_proto::json::grammar::quoted(key)
        ))),
    }
}

/// A whole number in the save's spelling: decimal digits, a minus sign for a
/// negative one, no leading zero, nothing else.
fn integer(value: &Json, key: &str) -> Result<i128, Error> {
    let refused = || {
        damaged(said!(
            "`{}` is not a whole number",
            pharmakos_proto::json::grammar::quoted(key)
        ))
    };
    let Json::Number(lexeme) = value else {
        return Err(refused());
    };
    let digits = match lexeme.strip_prefix('-') {
        Some(magnitude) => magnitude,
        None => lexeme,
    };
    if digits.is_empty()
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
        || digits.len() > 20
    {
        return Err(refused());
    }
    lexeme.parse::<i128>().map_err(|_| refused())
}

fn fits<T: TryFrom<i128>>(value: &Json, key: &str) -> Result<T, Error> {
    T::try_from(integer(value, key)?).map_err(|_| {
        damaged(said!(
            "`{}` does not fit",
            pharmakos_proto::json::grammar::quoted(key)
        ))
    })
}

fn u32_of(value: &Json, key: &str) -> Result<u32, Error> {
    fits(value, key)
}

fn u64_of(value: &Json, key: &str) -> Result<u64, Error> {
    fits(value, key)
}

fn i32_of(value: &Json, key: &str) -> Result<i32, Error> {
    fits(value, key)
}

fn i64_of(value: &Json, key: &str) -> Result<i64, Error> {
    fits(value, key)
}

fn seat_of(value: &Json) -> Result<SeatId, Error> {
    Ok(SeatId::new(fits::<u8>(value, "seat")?))
}

#[cfg(test)]
mod tests {
    use crate::error::Code;
    use crate::fog::FogPolicy;
    use crate::surface::Surface;
    use pharmakos_sim::math::quantity::Damage;
    use pharmakos_sim::rules::RulesTable;
    use pharmakos_sim::runner::MatchSettings;
    use pharmakos_sim::tables::{SeatId, UnitId};
    use pharmakos_sim::world::{DamageOrder, DamageTarget, WorldConfig};

    /// A two-seat match of one one-second round, hosted, in its Lull.
    fn hosted() -> Surface {
        let rules = || {
            RulesTable::load(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("..")
                    .join("rules")
                    .join("rules.v1.json"),
            )
            .expect("the shipped rules table")
        };
        let seed = 0x0000_0000_ca5c_aded;
        let mut surface = Surface::new(
            "m-opsave-unsaved",
            seed,
            rules(),
            FogPolicy::fogged(),
            &[SeatId::new(0), SeatId::new(1)],
        )
        .expect("a surface");
        let host = crate::host::Host::open(
            &WorldConfig {
                match_seed: seed,
                seats: 2,
                rules: rules(),
                match_settings: MatchSettings {
                    segment_lengths_ms: vec![1_000],
                    round_limit: 1,
                },
            },
            None,
        )
        .expect("a match");
        surface.attach(host).expect("attached");
        surface.open_lull().expect("the opening Lull");
        surface
    }

    /// A match's end whose save cannot be built is not an audit line alone:
    /// the refusal is typed, returned, and left where the host loop stops on
    /// it (`Persistence::unsaved`), so no `sealed` save of the final Push is
    /// left to be resumed as live without the host saying so. Here a host
    /// order waits for a tick, so the match cannot be saved.
    #[test]
    fn an_end_that_cannot_be_saved_is_refused_and_carried_to_the_host_loop() {
        let mut surface = hosted();
        surface
            .host_mut()
            .expect("a match")
            .file_damage(DamageOrder {
                target: DamageTarget::Unit(UnitId::new(0)),
                amount: Damage::new(1).expect("a positive amount"),
                by: SeatId::NEUTRAL,
                source: None,
            })
            .expect("an order filed");
        let error = surface.persist_end().expect_err("an order waits");
        assert_eq!(error.code(), Code::Internal);
        assert!(
            error
                .message()
                .starts_with("the `ended` save of round 1 could not be built ("),
            "{}",
            error.message()
        );
        let pending = surface.take_persistence();
        assert_eq!(pending.unsaved, Some(error));
        assert!(pending.save.is_none());
        assert!(!surface.end_saved, "nothing was saved");
    }

    /// The end a tick reports is saved at that tick (`Surface::after_tick`),
    /// and a save that cannot be built there is the step's own refusal as
    /// well as the host loop's: a caller with no loop behind it (`gamectl
    /// scenario run`, a test) is told, and nothing is swallowed (mutant N4b).
    #[test]
    fn a_tick_that_ends_the_match_unsaved_is_refused() {
        let mut surface = hosted();
        surface
            .host_mut()
            .expect("a match")
            .file_damage(DamageOrder {
                target: DamageTarget::Unit(UnitId::new(0)),
                amount: Damage::new(1).expect("a positive amount"),
                by: SeatId::NEUTRAL,
                source: None,
            })
            .expect("an order filed");
        let tick = surface.host().expect("a match").runner().tick();
        let report = pharmakos_sim::runner::TickReport {
            tick,
            hash: 0,
            phase: pharmakos_sim::runner::MatchPhase::Recap,
            segment_ended: true,
            match_ended: true,
        };
        let error = surface
            .after_tick(Some(report))
            .expect_err("the end could not be saved");
        assert_eq!(error.code(), Code::Internal);
        assert_eq!(surface.take_persistence().unsaved, Some(error));
    }
}
