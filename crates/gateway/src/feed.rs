// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The event bus, the 60-second digest cadence, and the opaque snapshot-tied
//! cursors.
//!
//! Spec section 12, "During play": `get_segment_feed` returns "fog-filtered
//! events (no-fog for every seat in casual matches) plus a digest for every 60 s
//! of game time. Engine chatter rides the same feed as labelled one-way lines."
//!
//! # Every event has a kind a scenario file can assert on
//!
//! Decisions-log item 97 is what this module owes the rest of the harness. The
//! scenario format's `event_fired` assertion names an event by a string
//! (`"event": "beacon_placed"`), and item 97's extension at T15 adds
//! `event_count_in_range` on top of the same names. So a [`Kind`] is not "a
//! string": it is validated at construction to be a name a scenario file can
//! carry -- lower-case ASCII, digits and underscores, starting with a letter --
//! and **every digest carries a per-kind count**, so the T15 assertions need no
//! new event shape and no format change.
//!
//! The catalogue of kinds is not here. It grows with every stage and belongs to
//! the sim's event bus (T10); what belongs here is the rule that a kind is
//! assertable and the counting that makes it useful.
//!
//! # A note on the schema
//!
//! `gp.api.v1.Digest` has `from_ms`, `to_ms` and `text` and no count field
//! today. T9 makes no `gp.api.v1` change (skeleton plan T9: a method that needs
//! one goes back to the proto lane), so the per-kind counts live in this crate's
//! own [`Digest`] and are rendered into the digest's deterministic prose, where
//! a scenario and a person both read the same numbers. The pull request records
//! the field the schema is owed.
//!
//! # Cursors
//!
//! Opaque, and tied to the snapshot they were issued against (spec section 12,
//! "Output"). A cursor from the previous segment is a
//! [`crate::error::Code::StaleSnapshot`], not a silent restart from the
//! beginning: a client that pages through a feed it thinks is still current is
//! the one case where returning something plausible is worse than returning an
//! error.
//!
//! A cursor counts **what its viewer has been shown**, never what happened. An
//! index over the unfiltered bus would be a number a fogged seat could subtract
//! from its own page length to learn how many events it was not told about --
//! including another seat's `Private` ones -- which is a fog leak by arithmetic
//! and exactly what decisions-log item 26 is about. Two seats paging the same
//! segment therefore see two different cursor sequences, and that is correct.

use crate::error::Error;
use crate::fog::{Audience, FogFilter, Viewer, Vision};
use pharmakos_sim::math::quantity::Ms;
use std::collections::BTreeMap;

/// The digest cadence, in game time. Sixty seconds, from spec section 12; a
/// constant of the design rather than a tuning value, which is why it is not a
/// rules-table row.
pub const DIGEST_PERIOD: Ms = Ms::new(60_000);

/// The most events one page of the feed carries, whatever a caller asks for.
///
/// The ceiling above the `detail` ladder in [`crate::detail`]: `full` is the
/// widest budget a client can name and this is the cap a client cannot raise.
///
/// PLACEHOLDER: 256 is a working number. OWNER settles it with the read-method
/// detail budgets at hardening.
pub const MAX_PAGE_EVENTS: usize = 256;

/// The longest a [`Kind`] may be.
pub const MAX_KIND_CHARS: usize = 48;

/// An event kind: a name a scenario file can assert on.
///
/// Validated at construction, so an unassertable kind cannot reach the feed at
/// all. `beacon_placed`, `commander_moved`, `phase_changed`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Kind(String);

impl Kind {
    /// A kind from its name.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a name a scenario file could
    /// not carry: empty, too long, or holding anything but lower-case ASCII
    /// letters, digits and underscores, starting with a letter.
    pub fn new(name: &str) -> Result<Kind, Error> {
        if name.is_empty() {
            return Err(Error::invalid("an event kind cannot be empty"));
        }
        if name.chars().count() > MAX_KIND_CHARS {
            return Err(Error::invalid(format!(
                "an event kind is at most {MAX_KIND_CHARS} characters"
            )));
        }
        let first_is_letter = name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase());
        if !first_is_letter {
            return Err(Error::invalid(format!(
                "`{name}` does not start with a lower-case ASCII letter, so a scenario file \
                 could not name it"
            )));
        }
        if !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        }) {
            return Err(Error::invalid(format!(
                "`{name}` is not lower_snake_case, so a scenario file could not name it"
            )));
        }
        Ok(Kind(name.to_owned()))
    }

    /// The name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One thing that happened.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Event {
    /// Game milliseconds since the start of the segment.
    pub at_ms: Ms,
    /// What kind of thing it was.
    pub kind: Kind,
    /// English, from the one string table. Engine chatter is a line of this kind
    /// and is labelled as one-way: no seat reads another seat's anything.
    pub text: String,
    /// Who may be told.
    pub audience: Audience,
}

/// The identity of the snapshot a feed and its cursors are tied to.
///
/// Derived from the match seed, the round and the segment with the project's one
/// hash function (`pharmakos_sim::digest`, AGENTS.md section 5) -- not a counter,
/// so a cursor from a different match is refused rather than accepted by
/// coincidence.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SnapshotId(u64);

impl SnapshotId {
    /// The id for one segment of one match.
    #[must_use]
    pub fn of(match_seed: u64, round: u32, segment: u32) -> SnapshotId {
        let mut bytes: Vec<u8> = Vec::with_capacity(16);
        bytes.extend_from_slice(&match_seed.to_le_bytes());
        bytes.extend_from_slice(&round.to_le_bytes());
        bytes.extend_from_slice(&segment.to_le_bytes());
        SnapshotId(pharmakos_sim::digest(&bytes))
    }

    /// The raw value, for rendering a cursor.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A place in the feed, opaque to the client.
///
/// The rendering is 16 hex digits of the snapshot id, 8 of the index and 8 of a
/// check value over both, lower case throughout. The check is not a signature
/// and does not claim to be: the token is what authenticates a caller, and the
/// check is here so a mangled cursor is refused as mangled rather than read as a
/// different position.
///
/// The index is **how many events this viewer has already been shown**, not how
/// many happened -- see the module docs. It follows that a cursor is a viewer's
/// as well as a snapshot's, and handing one seat's cursor to another seat
/// resumes at the wrong place rather than revealing anything: the position is
/// counted again over the second seat's own visible events.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cursor {
    snapshot: SnapshotId,
    index: u32,
}

impl Cursor {
    /// The cursor at the start of a segment's feed.
    #[must_use]
    pub const fn start(snapshot: SnapshotId) -> Cursor {
        Cursor { snapshot, index: 0 }
    }

    /// A cursor at a place in a snapshot-tied listing.
    ///
    /// The feed is not the only listing a seat pages through -- `list_beacons`
    /// is another -- and both want the same properties: opaque, tied to the
    /// snapshot so a stale one is refused rather than silently restarted, and
    /// counting **what this viewer has been shown** rather than what exists.
    /// One rendering and one parser for both, rather than a second cursor
    /// format that would have to be got right twice.
    #[must_use]
    pub const fn at(snapshot: SnapshotId, index: u32) -> Cursor {
        Cursor { snapshot, index }
    }

    /// Which snapshot it belongs to.
    #[must_use]
    pub const fn snapshot(self) -> SnapshotId {
        self.snapshot
    }

    /// How many of **this viewer's visible** events it is past.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// The opaque text a client receives and hands back.
    #[must_use]
    pub fn render(self) -> String {
        let check = self.check();
        format!("{:016x}{:08x}{:08x}", self.snapshot.0, self.index, check)
    }

    /// Read a cursor a client handed back.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] when the text is not a cursor
    /// this gateway wrote, and [`crate::error::Code::StaleSnapshot`] when it is
    /// one but belongs to another snapshot.
    pub fn parse(text: &str, current: SnapshotId) -> Result<Cursor, Error> {
        // Exactly 32 lower-case hex digits, because that is exactly what
        // `render` writes. `from_str_radix` would also take `FFFF` and a leading
        // `+`, and "a cursor this gateway issued" should mean literally that
        // rather than "something that parses to the same number".
        if text.len() != 32
            || !text
                .bytes()
                .all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(Error::invalid("that is not a cursor this gateway issued"));
        }
        let snapshot = text
            .get(..16)
            .and_then(|digits| u64::from_str_radix(digits, 16).ok())
            .ok_or_else(|| Error::invalid("that is not a cursor this gateway issued"))?;
        let index = text
            .get(16..24)
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(|| Error::invalid("that is not a cursor this gateway issued"))?;
        let check = text
            .get(24..32)
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(|| Error::invalid("that is not a cursor this gateway issued"))?;
        let cursor = Cursor {
            snapshot: SnapshotId(snapshot),
            index,
        };
        if cursor.check() != check {
            return Err(Error::invalid("that cursor is damaged"));
        }
        if cursor.snapshot != current {
            return Err(Error::stale_snapshot(
                "that cursor was issued against an earlier snapshot; start the feed again",
            ));
        }
        Ok(cursor)
    }

    /// The check value: the low 32 bits of the project's one hash function over
    /// the snapshot id and the index.
    fn check(self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(12);
        bytes.extend_from_slice(&self.snapshot.0.to_le_bytes());
        bytes.extend_from_slice(&self.index.to_le_bytes());
        let hash = pharmakos_sim::digest(&bytes);
        u32::try_from(hash & 0xffff_ffff).unwrap_or(0)
    }
}

/// One 60-second digest of game time, with its per-kind counts (item 97).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Digest {
    /// Game milliseconds since the start of the segment, inclusive.
    pub from_ms: Ms,
    /// Game milliseconds since the start of the segment, exclusive.
    pub to_ms: Ms,
    /// How many events of each kind fell in the window, in kind order.
    ///
    /// Ordered by kind name, so two machines render the same digest. Counts are
    /// of the events **this viewer may see**: a digest is part of a fog-filtered
    /// answer, and a count of events a seat cannot see would be a fog leak of
    /// exactly the kind item 26 is about.
    pub counts: Vec<(Kind, u32)>,
    /// Deterministic template prose.
    pub text: String,
}

impl Digest {
    /// How many events the window held, all kinds summed.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.counts
            .iter()
            .fold(0_u32, |sum, (_, count)| sum.saturating_add(*count))
    }
}

/// One page of the feed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Page {
    /// The events this viewer may see, in order, at most one budget's worth.
    pub events: Vec<Event>,
    /// The digests covering the **whole segment** as this viewer may see it,
    /// not merely the events on this page.
    ///
    /// Item 97 owes T15's `event_count_in_range` a per-kind count it can assert
    /// on, and a count that shrank when a client asked for a smaller page would
    /// not be assertable at all. So the page is what `limit` and the `detail`
    /// budget cut; the digest is the segment's, and two calls with different
    /// budgets produce the same digests.
    pub digests: Vec<Digest>,
    /// Where to read from next.
    pub next_cursor: Cursor,
}

/// The event bus for one segment.
///
/// One segment, because the cursors are tied to the segment's snapshot: a new
/// segment is a new [`SegmentFeed`] with a new [`SnapshotId`], and every cursor
/// from the old one is refused.
#[derive(Clone, Debug)]
pub struct SegmentFeed {
    snapshot: SnapshotId,
    events: Vec<Event>,
}

impl SegmentFeed {
    /// An empty feed for a segment.
    #[must_use]
    pub const fn new(snapshot: SnapshotId) -> SegmentFeed {
        SegmentFeed {
            snapshot,
            events: Vec::new(),
        }
    }

    /// Which snapshot this feed is tied to.
    #[must_use]
    pub const fn snapshot(&self) -> SnapshotId {
        self.snapshot
    }

    /// How many events are on the bus, before any filtering.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// True when nothing has happened yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Put an event on the bus.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a negative time, or for an
    /// event earlier than the one before it. Order is part of the feed's
    /// contract: cursors are indices, and an event inserted behind a cursor
    /// would be one a client could never be told about.
    pub fn publish(&mut self, event: Event) -> Result<(), Error> {
        if event.at_ms.raw() < 0 {
            return Err(Error::invalid(
                "an event's time is game milliseconds since the segment began, never negative",
            ));
        }
        if let Some(last) = self.events.last() {
            if event.at_ms < last.at_ms {
                return Err(Error::invalid(
                    "events reach the bus in time order; one behind a cursor could never be \
                     delivered",
                ));
            }
        }
        self.events.push(event);
        Ok(())
    }

    /// Everything on the bus, unfiltered. For the host and for tests -- never
    /// for a client, which reads [`SegmentFeed::page`].
    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// One page of the feed as `viewer` may see it.
    ///
    /// `cursor` is `None` to start at the beginning of the segment. At most
    /// `limit` events come back, and never more than [`MAX_PAGE_EVENTS`]; the
    /// returned cursor says where the next call resumes, whether or not this one
    /// filled the page. The digests cover the whole segment as this viewer sees
    /// it, and do not move with the page size ([`Page::digests`]).
    ///
    /// The fog filter runs **before** the cursor arithmetic, not after: the
    /// position is a count of this viewer's own visible events, so no number
    /// that leaves the gateway is derived from an event the viewer may not see.
    ///
    /// # Errors
    ///
    /// As [`Cursor::parse`]: an unreadable cursor is
    /// [`crate::error::Code::InvalidArgument`] and one from another snapshot is
    /// [`crate::error::Code::StaleSnapshot`].
    pub fn page<V: Vision>(
        &self,
        viewer: Viewer,
        filter: &FogFilter<'_, V>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page, Error> {
        let from = match cursor {
            Some(text) if !text.is_empty() => Cursor::parse(text, self.snapshot)?,
            _ => Cursor::start(self.snapshot),
        };
        let start = usize::try_from(from.index()).unwrap_or(usize::MAX);
        let limit = limit.clamp(1, MAX_PAGE_EVENTS);

        let visible: Vec<&Event> = self
            .events
            .iter()
            .filter(|event| filter.visible(viewer, &event.audience))
            .collect();

        let events: Vec<Event> = visible
            .iter()
            .skip(start)
            .take(limit)
            .map(|event| (*event).clone())
            .collect();
        let digests = digests_of(&visible);
        let index = start.min(visible.len()).saturating_add(events.len());
        let next = Cursor {
            snapshot: self.snapshot,
            index: u32::try_from(index).unwrap_or(u32::MAX),
        };
        Ok(Page {
            events,
            digests,
            next_cursor: next,
        })
    }
}

/// The digests covering `events`: one per 60 seconds of game time, from the
/// window the first event falls in to the window the last one does.
///
/// Empty windows between two events do get a digest, with a zero count: a
/// minute in which nothing happened is news, and a client that renders a
/// timeline needs the gap to be there.
#[must_use]
pub fn digests(events: &[Event]) -> Vec<Digest> {
    let borrowed: Vec<&Event> = events.iter().collect();
    digests_of(&borrowed)
}

/// [`digests`] over borrowed events, which is what [`SegmentFeed::page`] has
/// after the fog filter has run.
fn digests_of(events: &[&Event]) -> Vec<Digest> {
    let Some(first) = events.first() else {
        return Vec::new();
    };
    let Some(last) = events.last() else {
        return Vec::new();
    };
    let first_window = window_of(first.at_ms);
    let last_window = window_of(last.at_ms);

    let mut out: Vec<Digest> = Vec::new();
    let mut window = first_window;
    while window <= last_window {
        let from = Ms::new(window.saturating_mul(DIGEST_PERIOD.raw()));
        let to = Ms::new(window.saturating_add(1).saturating_mul(DIGEST_PERIOD.raw()));
        let mut counts: BTreeMap<Kind, u32> = BTreeMap::new();
        for event in events {
            if event.at_ms >= from && event.at_ms < to {
                let slot = counts.entry(event.kind.clone()).or_insert(0);
                *slot = slot.saturating_add(1);
            }
        }
        let counts: Vec<(Kind, u32)> = counts.into_iter().collect();
        let text = render_digest(from, to, &counts);
        out.push(Digest {
            from_ms: from,
            to_ms: to,
            counts,
            text,
        });
        window = window.saturating_add(1);
    }
    out
}

/// Which 60-second window a time falls in.
fn window_of(at: Ms) -> i32 {
    at.raw()
        .max(0)
        .checked_div(DIGEST_PERIOD.raw())
        .unwrap_or(0)
}

/// The digest's deterministic template prose.
///
/// `"0:00-1:00: 12 events. beacon_placed 1, commander_moved 11."`, and
/// `"1:00-2:00: nothing."` for an empty window. Template prose rather than a
/// sentence written per kind: spec section 12 asks every result for "structured
/// JSON plus deterministic template prose", and a template is the only kind of
/// prose that is deterministic.
fn render_digest(from: Ms, to: Ms, counts: &[(Kind, u32)]) -> String {
    let span = format!("{}-{}", clock(from), clock(to));
    if counts.is_empty() {
        return format!("{span}: nothing.");
    }
    let total = counts
        .iter()
        .fold(0_u32, |sum, (_, count)| sum.saturating_add(*count));
    let plural = if total == 1 { "event" } else { "events" };
    let detail = counts
        .iter()
        .map(|(kind, count)| format!("{kind} {count}"))
        .collect::<Vec<String>>()
        .join(", ");
    format!("{span}: {total} {plural}. {detail}.")
}

/// Game milliseconds as `m:ss`, for the digest's prose.
fn clock(at: Ms) -> String {
    let seconds = at.raw().max(0).checked_div(1000).unwrap_or(0);
    let minutes = seconds.checked_div(60).unwrap_or(0);
    let rest = seconds.saturating_sub(minutes.saturating_mul(60));
    format!("{minutes}:{rest:02}")
}

#[cfg(test)]
mod tests {
    use super::{
        Cursor, DIGEST_PERIOD, Event, Kind, MAX_PAGE_EVENTS, SegmentFeed, SnapshotId, digests,
    };
    use crate::error::Code;
    use crate::fog::{Audience, Blind, FogFilter, FogPolicy, Viewer};
    use pharmakos_proto::gp::v1::Voxel;
    use pharmakos_sim::math::quantity::Ms;
    use pharmakos_sim::tables::SeatId;

    fn kind(name: &str) -> Kind {
        Kind::new(name).expect("a well formed kind")
    }

    fn event(at_ms: i32, name: &str, audience: Audience) -> Event {
        Event {
            at_ms: Ms::new(at_ms),
            kind: kind(name),
            text: format!("{name} happened"),
            audience,
        }
    }

    fn world(seat: u8) -> Audience {
        Audience::World {
            owner: Some(SeatId::new(seat)),
            at: Voxel { x: 1, y: 2, z: 3 },
        }
    }

    fn snapshot() -> SnapshotId {
        SnapshotId::of(0x00ca_5cad_ed00_0001, 1, 0)
    }

    #[test]
    fn a_kind_is_a_name_a_scenario_file_can_assert_on() {
        assert_eq!(kind("beacon_placed").name(), "beacon_placed");
        assert_eq!(kind("seat0_ready").name(), "seat0_ready");
        for bad in [
            "",
            "Beacon_Placed",
            "beacon placed",
            "beacon-placed",
            "9lives",
            "_leading",
            "beacon.placed",
        ] {
            assert_eq!(
                Kind::new(bad).expect_err("refused").code,
                Code::InvalidArgument,
                "`{bad}` should not be a kind"
            );
        }
        assert!(Kind::new(&"a".repeat(49)).is_err(), "too long");
    }

    #[test]
    fn events_reach_the_bus_in_time_order_or_not_at_all() {
        let mut feed = SegmentFeed::new(snapshot());
        feed.publish(event(1_000, "commander_moved", Audience::Public))
            .expect("first");
        feed.publish(event(2_000, "beacon_placed", Audience::Public))
            .expect("later");
        let error = feed
            .publish(event(1_500, "beacon_placed", Audience::Public))
            .expect_err("out of order");
        assert_eq!(error.code, Code::InvalidArgument);
        let error = feed
            .publish(event(-1, "beacon_placed", Audience::Public))
            .expect_err("negative");
        assert_eq!(error.code, Code::InvalidArgument);
        assert_eq!(feed.len(), 2);
        assert!(!feed.is_empty());
    }

    #[test]
    fn a_digest_carries_a_per_kind_count() {
        let events = vec![
            event(0, "commander_moved", Audience::Public),
            event(1_000, "commander_moved", Audience::Public),
            event(2_000, "beacon_placed", Audience::Public),
            event(61_000, "ore_delivered", Audience::Public),
        ];
        let digests = digests(&events);
        assert_eq!(digests.len(), 2);
        let first = digests.first().expect("first window");
        assert_eq!(
            first
                .counts
                .iter()
                .map(|(kind, count)| (kind.name().to_owned(), *count))
                .collect::<Vec<(String, u32)>>(),
            vec![
                (String::from("beacon_placed"), 1),
                (String::from("commander_moved"), 2)
            ],
            "counts are in kind order, so two machines render the same digest"
        );
        assert_eq!(first.total(), 3);
        assert_eq!(
            first.text,
            "0:00-1:00: 3 events. beacon_placed 1, commander_moved 2."
        );
        let second = digests.get(1).expect("second window");
        assert_eq!(second.from_ms, DIGEST_PERIOD);
        assert_eq!(second.text, "1:00-2:00: 1 event. ore_delivered 1.");
    }

    #[test]
    fn an_empty_minute_still_gets_a_digest() {
        let events = vec![
            event(0, "phase_changed", Audience::Public),
            event(130_000, "phase_changed", Audience::Public),
        ];
        let digests = digests(&events);
        assert_eq!(digests.len(), 3);
        assert_eq!(
            digests.get(1).expect("the quiet minute").text,
            "1:00-2:00: nothing."
        );
        assert!(
            super::digests(&[]).is_empty(),
            "no events, no digests: a feed that has not started is not a quiet minute"
        );
    }

    #[test]
    fn a_page_is_fog_filtered_and_its_digests_count_only_what_the_seat_sees() {
        let mut feed = SegmentFeed::new(snapshot());
        feed.publish(event(0, "phase_changed", Audience::Public))
            .expect("published");
        feed.publish(event(1_000, "beacon_placed", world(1)))
            .expect("published");
        feed.publish(event(2_000, "ore_delivered", world(0)))
            .expect("published");
        feed.publish(event(
            3_000,
            "draft_saved",
            Audience::Private(SeatId::new(1)),
        ))
        .expect("published");

        let policy = FogPolicy::fogged();
        let filter = FogFilter::new(&policy, &Blind);
        let page = feed
            .page(Viewer::Seat(SeatId::new(0)), &filter, None, 64)
            .expect("a page");
        let kinds: Vec<&str> = page.events.iter().map(|event| event.kind.name()).collect();
        assert_eq!(
            kinds,
            vec!["phase_changed", "ore_delivered"],
            "seat 0 sees the announcement and its own ore, and neither seat 1's beacon \
             nor seat 1's draft"
        );
        let digest = page.digests.first().expect("one window");
        assert_eq!(
            digest.total(),
            2,
            "a count of unseen events would be a leak"
        );
    }

    /// The cursor is a count of what the viewer was shown. If it counted the
    /// bus, a fogged seat could subtract its page length from it and learn how
    /// many events it was not told about -- including another seat's `Private`
    /// ones (decisions-log item 26).
    #[test]
    fn a_fogged_seats_cursor_does_not_count_events_it_cannot_see() {
        let mut feed = SegmentFeed::new(snapshot());
        feed.publish(event(0, "phase_changed", Audience::Public))
            .expect("published");
        for index in 0..7_i32 {
            feed.publish(event(
                index.saturating_add(1).saturating_mul(100),
                "ore_delivered",
                world(1),
            ))
            .expect("published");
            feed.publish(event(
                index.saturating_add(1).saturating_mul(100),
                "draft_saved",
                Audience::Private(SeatId::new(1)),
            ))
            .expect("published");
        }

        let policy = FogPolicy::fogged();
        let filter = FogFilter::new(&policy, &Blind);
        let page = feed
            .page(Viewer::Seat(SeatId::new(0)), &filter, None, 64)
            .expect("a page");
        assert_eq!(
            page.events.len(),
            1,
            "seat 0 sees the announcement and nothing of seat 1's"
        );
        assert_eq!(
            page.next_cursor.index(),
            1,
            "the cursor counts the one event seat 0 was shown, not the fifteen on the bus"
        );

        // And a no-fog viewer's cursor counts its own larger view, which is the
        // same rule rather than a second one.
        let nofog = FogPolicy::casual();
        let open = FogFilter::new(&nofog, &Blind);
        let page = feed
            .page(Viewer::Seat(SeatId::new(0)), &open, None, 64)
            .expect("a page");
        assert_eq!(
            page.next_cursor.index(),
            8,
            "everything but seat 1's drafts"
        );
    }

    /// Item 97's counts are what T15's `event_count_in_range` asserts on, so a
    /// count that moved with the caller's `limit` would not be assertable.
    #[test]
    fn a_digest_does_not_move_with_the_page_size() {
        let mut feed = SegmentFeed::new(snapshot());
        for index in 0..5_i32 {
            feed.publish(event(
                index.saturating_mul(1_000),
                "phase_changed",
                Audience::Public,
            ))
            .expect("published");
        }
        let policy = FogPolicy::casual();
        let filter = FogFilter::new(&policy, &Blind);
        let whole = feed
            .page(Viewer::Seat(SeatId::new(0)), &filter, None, 64)
            .expect("a page");
        let narrow = feed
            .page(Viewer::Seat(SeatId::new(0)), &filter, None, 2)
            .expect("a page");
        assert_eq!(narrow.events.len(), 2, "the page is what a limit cuts");
        assert_eq!(
            narrow.digests, whole.digests,
            "the digest is the segment's, not the page's"
        );
        assert_eq!(
            narrow.digests.first().expect("one window").text,
            "0:00-1:00: 5 events. phase_changed 5."
        );
    }

    #[test]
    fn a_cursor_resumes_where_it_left_off_and_is_opaque() {
        let mut feed = SegmentFeed::new(snapshot());
        for index in 0..5_i32 {
            feed.publish(event(
                index.saturating_mul(100),
                "commander_moved",
                Audience::Public,
            ))
            .expect("published");
        }
        let policy = FogPolicy::casual();
        let filter = FogFilter::new(&policy, &Blind);
        let first = feed
            .page(Viewer::Seat(SeatId::new(0)), &filter, None, 2)
            .expect("a page");
        assert_eq!(first.events.len(), 2);
        let text = first.next_cursor.render();
        assert_eq!(text.len(), 32);
        assert_eq!(first.next_cursor.index(), 2);
        let second = feed
            .page(Viewer::Seat(SeatId::new(0)), &filter, Some(&text), 2)
            .expect("a page");
        assert_eq!(second.events.len(), 2);
        assert_eq!(second.events.first().map(|e| e.at_ms), Some(Ms::new(200)));
    }

    #[test]
    fn a_cursor_from_another_snapshot_is_stale_rather_than_a_silent_restart() {
        let feed = SegmentFeed::new(snapshot());
        let other = Cursor::start(SnapshotId::of(0x00ca_5cad_ed00_0001, 1, 1));
        let policy = FogPolicy::casual();
        let filter = FogFilter::new(&policy, &Blind);
        let error = feed
            .page(
                Viewer::Seat(SeatId::new(0)),
                &filter,
                Some(&other.render()),
                8,
            )
            .expect_err("refused");
        assert_eq!(error.code, Code::StaleSnapshot);
    }

    #[test]
    fn a_damaged_cursor_is_refused_rather_than_read_as_another_position() {
        let current = snapshot();
        let mut text = Cursor::start(current).render();
        text.replace_range(20..21, "f");
        let error = Cursor::parse(&text, current).expect_err("refused");
        assert_eq!(error.code, Code::InvalidArgument);
        assert_eq!(
            Cursor::parse("nonsense", current)
                .expect_err("refused")
                .code,
            Code::InvalidArgument
        );
        // A spelling this gateway never issues is not a cursor this gateway
        // issued, however it would parse.
        let issued = Cursor::start(current).render();
        assert_eq!(
            Cursor::parse(&issued.to_uppercase(), current)
                .expect_err("refused")
                .code,
            Code::InvalidArgument,
            "upper-case hex is not the rendering"
        );
        assert!(
            Cursor::parse(&issued, current).is_ok(),
            "and the rendering itself still reads"
        );
    }

    #[test]
    fn a_page_is_capped_however_large_a_limit_is_asked_for() {
        let mut feed = SegmentFeed::new(snapshot());
        for _ in 0..(MAX_PAGE_EVENTS + 10) {
            feed.publish(event(0, "commander_moved", Audience::Public))
                .expect("published");
        }
        let policy = FogPolicy::casual();
        let filter = FogFilter::new(&policy, &Blind);
        let page = feed
            .page(Viewer::Seat(SeatId::new(0)), &filter, None, usize::MAX)
            .expect("a page");
        assert_eq!(page.events.len(), MAX_PAGE_EVENTS);
    }

    #[test]
    fn a_snapshot_id_differs_per_match_round_and_segment() {
        let base = SnapshotId::of(7, 1, 0);
        assert_ne!(base, SnapshotId::of(8, 1, 0));
        assert_ne!(base, SnapshotId::of(7, 2, 0));
        assert_ne!(base, SnapshotId::of(7, 1, 1));
        assert_eq!(base, SnapshotId::of(7, 1, 0), "and it is a function");
        assert_ne!(base.raw(), 0);
    }
}
