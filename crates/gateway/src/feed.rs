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
//! T9 left `gp.api.v1.Digest` with `from_ms`, `to_ms` and `text` and no count
//! field, because T9 makes no `gp.api.v1` change, and recorded in its pull
//! request the field the schema was owed. **T13's proto bundle paid it**
//! (decisions-log item 100 (10)): `Digest` now carries
//! `repeated KindCount counts = 4`, this crate's own [`Digest::counts`] maps
//! straight onto it, and `crate::surface::Surface::segment_feed` writes it.
//!
//! The counts are still rendered into the digest's deterministic prose as well,
//! and that is not duplication for its own sake: the prose is what a person
//! reads and what a `render`-style golden pins, the field is what a client
//! sums without parsing English, and both come from the same [`Digest`] so
//! they cannot say different things.
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
use crate::fog::{Audience, FogPolicy, Viewer, Vision};
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::tables::SeatId;
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
/// PLACEHOLDER: 256 events a page, a working number — owner, at hardening, with the read-method detail budgets.
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
    /// The Ledger's memo this line carries (`gp.api.v1.Event.ledger_memo`), on
    /// a line of kind `ledger_memo` and no other ([`crate::probation`]).
    pub memo: Option<crate::probation::Memo>,
}

/// The seats that could see a world event as it reached the bus.
///
/// **Taken when the event is published, never at a read** (S3's `gwp`; item
/// 155 (3) 6). A seat's sight moves with its units, and a listing re-filtered
/// by the sight of the moment at every read changes length behind the cursor,
/// which counts what the viewer was shown: sight that grew re-delivered
/// earlier rows, out of time order, and sight that shrank lost later ones,
/// the seat's own among them. Stamped once, the list a seat is shown under
/// one fog policy never changes, so the cursor's count stays a place in it.
/// It is also what fog means for an event: a seat is told of what it could
/// see when it happened, not of what happened at a place it sees later.
///
/// One bit per seat id, so it holds the eight a `u8` counts, past v1's three
/// (decisions-log item 110 (5)); a seat past that is refused at publish.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Witnesses(u8);

impl Witnesses {
    /// The seats of `seats` that see where `audience` says the event
    /// happened, by `vision` as it stands. Only a world event is witnessed: a
    /// private line and an announcement are decided by their audience alone.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for a seat id past the eight a witness
    /// set holds.
    pub fn of<V: Vision>(
        vision: &V,
        seats: &[SeatId],
        audience: &Audience,
    ) -> Result<Witnesses, Error> {
        let Audience::World { owner, at } = audience else {
            return Ok(Witnesses(0));
        };
        let mut bits = 0_u8;
        for seat in seats {
            if *owner != Some(*seat) && vision.sees(*seat, at) {
                bits |= Witnesses::bit(*seat).ok_or_else(|| {
                    Error::internal(format!(
                        "seat {} is past the eight seats a feed's witness set holds",
                        seat.raw()
                    ))
                })?;
            }
        }
        Ok(Witnesses(bits))
    }

    /// True when `seat` saw it.
    #[must_use]
    pub fn contains(self, seat: SeatId) -> bool {
        Witnesses::bit(seat).is_some_and(|bit| self.0 & bit != 0)
    }

    /// A seat's bit, or `None` past the eighth.
    fn bit(seat: SeatId) -> Option<u8> {
        1_u8.checked_shl(u32::from(seat.raw()))
    }
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

/// Which listing a cursor is a place in.
///
/// Part of a cursor's checked bytes, so a cursor issued by one listing is
/// refused by the other rather than read as an offset into it. The feed's
/// index counts events and `list_beacons`'s counts beacons; the two are the
/// same shape and mean entirely different things, which is exactly the
/// confusion [`crate::time::PhaseMark`] is a separate type to avoid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Listing {
    /// The segment feed: `get_segment_feed` and `wait_for{feed_digest}`.
    Feed,
    /// `list_beacons`.
    Beacons,
    /// `get_view`. Three numbers wide rather than one, and match-tied rather
    /// than segment-tied, so it is [`ViewCursor`] rather than [`Cursor`] --
    /// but the rendering, the parser and the check value are this module's,
    /// once.
    View,
}

impl Listing {
    /// The byte the check is taken over.
    const fn tag(self) -> u8 {
        match self {
            Listing::Feed => 0,
            Listing::Beacons => 1,
            Listing::View => 2,
        }
    }

    /// What a client is told it handed in.
    const fn name(self) -> &'static str {
        match self {
            Listing::Feed => "the segment feed",
            Listing::Beacons => "the beacon listing",
            Listing::View => "the view",
        }
    }
}

/// A place in the feed, opaque to the client.
///
/// The rendering is 16 hex digits of the snapshot id, 8 of the index and 8 of a
/// check value over both **and the listing**, lower case throughout. The check
/// is not a signature and does not claim to be: the token is what authenticates
/// a caller, and the check is here so a mangled cursor is refused as mangled
/// rather than read as a different position -- and so a cursor from the wrong
/// listing is refused rather than read as a place in this one.
///
/// The index is **how many events this viewer has already been shown**, not how
/// many happened -- see the module docs. It follows that a cursor is a viewer's
/// as well as a snapshot's, and handing one seat's cursor to another seat
/// resumes at the wrong place rather than revealing anything: the position is
/// counted again over the second seat's own visible events.
///
/// A feed cursor also says **whether the viewer's fog had lifted** when it was
/// issued (`lifted`), because the list it counts in depends on it: a seat is
/// shown every world event once it is unfogged (eliminated, or the match
/// ended), and only what it witnessed before ([`SegmentFeed::page`]). The flag
/// is in the check and not in the text, so the rendering is the same 32 digits
/// and a cursor issued before the lift still reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cursor {
    snapshot: SnapshotId,
    listing: Listing,
    index: u32,
    lifted: bool,
}

impl Cursor {
    /// The cursor at the start of a segment's feed.
    #[must_use]
    pub const fn start(snapshot: SnapshotId) -> Cursor {
        Cursor {
            snapshot,
            listing: Listing::Feed,
            index: 0,
            lifted: false,
        }
    }

    /// A cursor at a place in a snapshot-tied listing.
    ///
    /// The feed is not the only listing a seat pages through -- `list_beacons`
    /// is another -- and both want the same properties: opaque, tied to the
    /// snapshot so a stale one is refused rather than silently restarted, and
    /// counting **what this viewer has been shown** rather than what exists.
    /// One rendering and one parser for both, rather than a second cursor
    /// format that would have to be got right twice -- but **not** one cursor:
    /// the listing is in the check, so a feed cursor handed to `list_beacons`
    /// is refused rather than read as a beacon offset.
    #[must_use]
    pub const fn at(snapshot: SnapshotId, listing: Listing, index: u32) -> Cursor {
        Cursor {
            snapshot,
            listing,
            index,
            lifted: false,
        }
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
    /// this gateway wrote **or is one the other listing issued**, and
    /// [`crate::error::Code::StaleSnapshot`] when it is this listing's but
    /// belongs to another snapshot.
    pub fn parse(text: &str, current: SnapshotId, listing: Listing) -> Result<Cursor, Error> {
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
        let fogged = Cursor {
            snapshot: SnapshotId(snapshot),
            listing,
            index,
            lifted: false,
        };
        // A feed cursor issued once the viewer's fog had lifted carries the
        // flag in its check alone ([`Cursor`]'s doc).
        let lifted = Cursor {
            lifted: true,
            ..fogged
        };
        let cursor = if fogged.check() == check {
            fogged
        } else if listing == Listing::Feed && lifted.check() == check {
            lifted
        } else {
            fogged
        };
        if cursor.check() != check {
            // A cursor of the other listing fails here, which is the point of
            // the tag: it is a place in something else, and resuming a beacon
            // page at an event count is a place nobody meant.
            return Err(Error::invalid(format!(
                "that is not a cursor {} issued",
                listing.name()
            )));
        }
        if cursor.snapshot != current {
            return Err(Error::stale_snapshot(
                "that cursor was issued against an earlier snapshot; start the feed again",
            ));
        }
        Ok(cursor)
    }

    /// The check value: the low 32 bits of the project's one hash function over
    /// the snapshot id, the listing and the index.
    fn check(self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(14);
        bytes.extend_from_slice(&self.snapshot.0.to_le_bytes());
        bytes.push(self.listing.tag());
        bytes.extend_from_slice(&self.index.to_le_bytes());
        // Only a lifted cursor's bytes grow, so every cursor issued under fog
        // checks exactly as it did before the flag existed.
        if self.lifted {
            bytes.push(1);
        }
        let [a, b, c, d, ..] = pharmakos_sim::digest(&bytes).to_le_bytes();
        u32::from_le_bytes([a, b, c, d])
    }

    /// True for a feed cursor issued once the viewer's fog had lifted.
    #[must_use]
    pub const fn lifted(self) -> bool {
        self.lifted
    }
}

/// A place in the view, opaque to the client.
///
/// [`Cursor`] is a place in one **segment's** listing and goes stale when a
/// segment ends. A view is not a segment's: a camera watches the Lull, the
/// Push and the recap without interruption, and a cursor that went stale at
/// every phase change would make the client ask for a fresh keyframe of the
/// whole map three times a round. So this one is tied to the **match** --
/// strictly, to the match as this gateway attached it -- and goes stale only
/// when the world behind the view was replaced, which is
/// [`crate::surface::Surface::attach`] and nothing else.
///
/// # Two stamps, and the second is what makes paging leak-proof
///
/// The rendering is 16 hex digits of the view's id, 16 of `from_seq`, 16 of
/// `to_seq`, 8 of the next chunk index and 8 of a check over all four and the
/// listing -- sixty-four lower-case hex digits.
///
/// `from_seq` is **what the answer is measured against**: a delta carries the
/// chunks whose bytes for this viewer changed after it, and `from_seq == 0`
/// means a keyframe, which no live stamp ever is (the feed's stamp starts at
/// one). `to_seq` is **the stamp the listing was started at**, and it is the
/// one the completing page hands back as the client's new position.
///
/// That second number is the whole trick, and the design sketch in
/// `docs/design/skeleton-plan-t16a-notes.md` -- which called this three
/// numbers wide -- did not have it. Both a keyframe and a delta are paged, so
/// the world can move while the client is halfway through one. If the
/// completing page handed back "wherever the feed is now", a chunk that
/// changed during the paging at an index already passed would never be sent
/// again. Handing back the stamp the listing **started** at costs one
/// re-delivery of whatever moved and loses nothing, which is the right way
/// round for a camera.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewCursor {
    /// The view this cursor belongs to. A cursor from before an `attach` is
    /// [`crate::error::Code::StaleSnapshot`].
    pub view_id: u64,
    /// What the listing is measured against. Zero is a keyframe.
    ///
    /// **Never a sim tick**: visibility changes in a Lull and in a recap, and
    /// neither spends one.
    pub from_seq: u64,
    /// The feed's stamp when this listing was started.
    pub to_seq: u64,
    /// The next chunk index to deliver. Zero starts a listing.
    pub index: u32,
}

impl ViewCursor {
    /// How many characters a rendered view cursor has.
    pub const CHARS: usize = 64;

    /// True when this cursor is a keyframe still being paged.
    #[must_use]
    pub const fn is_keyframe(self) -> bool {
        self.from_seq == 0
    }

    /// The opaque text a client receives and hands back.
    #[must_use]
    pub fn render(self) -> String {
        format!(
            "{:016x}{:016x}{:016x}{:08x}{:08x}",
            self.view_id,
            self.from_seq,
            self.to_seq,
            self.index,
            self.check()
        )
    }

    /// Read a cursor a client handed back.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] when the text is not a cursor
    /// this gateway wrote, and [`crate::error::Code::StaleSnapshot`] when it
    /// belongs to a view this gateway no longer has.
    pub fn parse(text: &str, current: u64) -> Result<ViewCursor, Error> {
        let refuse = || Error::invalid("that is not a cursor the view issued");
        if text.len() != ViewCursor::CHARS
            || !text
                .bytes()
                .all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(refuse());
        }
        let field = |from: usize, to: usize| text.get(from..to).ok_or_else(refuse);
        let view_id = u64::from_str_radix(field(0, 16)?, 16).map_err(|_| refuse())?;
        let from_seq = u64::from_str_radix(field(16, 32)?, 16).map_err(|_| refuse())?;
        let to_seq = u64::from_str_radix(field(32, 48)?, 16).map_err(|_| refuse())?;
        let index = u32::from_str_radix(field(48, 56)?, 16).map_err(|_| refuse())?;
        let check = u32::from_str_radix(field(56, 64)?, 16).map_err(|_| refuse())?;
        let cursor = ViewCursor {
            view_id,
            from_seq,
            to_seq,
            index,
        };
        if cursor.check() != check {
            return Err(refuse());
        }
        if cursor.view_id != current {
            return Err(Error::stale_snapshot(
                "that cursor was issued against a view this gateway no longer has; ask for a \
                 keyframe",
            ));
        }
        Ok(cursor)
    }

    /// The check value: the low 32 bits of the project's one hash function
    /// over the view id, the listing, both stamps and the index.
    fn check(self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(29);
        bytes.extend_from_slice(&self.view_id.to_le_bytes());
        bytes.push(Listing::View.tag());
        bytes.extend_from_slice(&self.from_seq.to_le_bytes());
        bytes.extend_from_slice(&self.to_seq.to_le_bytes());
        bytes.extend_from_slice(&self.index.to_le_bytes());
        let [a, b, c, d, ..] = pharmakos_sim::digest(&bytes).to_le_bytes();
        u32::from_le_bytes([a, b, c, d])
    }
}

/// One 60-second digest of game time, with its per-kind counts (item 97).
///
/// PLACEHOLDER: the digest payload's exact contents — owner, at S7 (watch & share).
/// **The digest payload's exact contents** are the skeleton plan's
/// own named T13 PLACEHOLDER. What this build carries is the window, the
/// per-kind counts item 97 asks for, and the deterministic prose; what a
/// digest is *for* -- the watch view's "what happened while I was reading" --
/// may want a settlement line, a loss line or a standing delta once there are
/// any, and every one of those is a field `gp.api.v1.Digest` does not have
/// yet. OWNER settles the payload at **S7** (watch & share), where the view
/// that reads it is built; a stage that adds one of those numbers adds its
/// field in the same proto change.
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
    /// Who witnessed each event as it was published, index for index with
    /// `events` ([`Witnesses`]).
    witnesses: Vec<Witnesses>,
}

impl SegmentFeed {
    /// An empty feed for a segment.
    #[must_use]
    pub const fn new(snapshot: SnapshotId) -> SegmentFeed {
        SegmentFeed {
            snapshot,
            events: Vec::new(),
            witnesses: Vec::new(),
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

    /// Put an event on the bus, witnessed by whichever of `seats` `vision` says
    /// sees where it happened, as the vision stands now ([`Witnesses`]).
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a negative time, or for an
    /// event earlier than the one before it. Order is part of the feed's
    /// contract: cursors are indices, and an event inserted behind a cursor
    /// would be one a client could never be told about. As [`Witnesses::of`]
    /// for a seat it cannot hold.
    pub fn publish<V: Vision>(
        &mut self,
        event: Event,
        vision: &V,
        seats: &[SeatId],
    ) -> Result<(), Error> {
        if event.at_ms.raw() < 0 {
            return Err(Error::invalid(
                "an event's time is game milliseconds since the segment began, never negative",
            ));
        }
        if let Some(last) = self.events.last()
            && event.at_ms < last.at_ms
        {
            return Err(Error::invalid(
                "events reach the bus in time order; one behind a cursor could never be \
                     delivered",
            ));
        }
        let witnesses = Witnesses::of(vision, seats, &event.audience)?;
        self.events.push(event);
        self.witnesses.push(witnesses);
        Ok(())
    }

    /// Everything on the bus, unfiltered. For the host and for tests -- never
    /// for a client, which reads [`SegmentFeed::page`].
    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// One page of the feed as `viewer` may see it under `policy`.
    ///
    /// `cursor` is `None` to start at the beginning of the segment. At most
    /// `limit` events come back, and never more than [`MAX_PAGE_EVENTS`]; the
    /// returned cursor says where the next call resumes, whether or not this one
    /// filled the page. The digests cover the whole segment as this viewer sees
    /// it, and do not move with the page size ([`Page::digests`]).
    ///
    /// # What the viewer is shown
    ///
    /// [`crate::fog`]'s table, with a world event's sight read from its
    /// [`Witnesses`], taken as it was published, and never from the sight of
    /// the moment: a seat is shown its own world events, and another seat's it
    /// witnessed, until its fog lifts ([`FogPolicy::unfogged`]: elimination,
    /// the match's end, a casual match), and from then on every world event.
    /// So the list a viewer is shown under one policy never changes, and the
    /// cursor, a count of it, is a place in it however the viewer's sight
    /// moves between reads (S3's `gwp`; item 155 (3) 6).
    ///
    /// # When the fog lifts between two reads
    ///
    /// The lift grows the list behind the cursor by every world event the
    /// viewer did not witness. A cursor issued under fog therefore resumes at
    /// the bus place **after the last event it was shown**, and the page goes
    /// on from there in the lifted list: nothing is delivered twice, the times
    /// a viewer is given never run backwards, and nothing the seat was owed is
    /// lost. What the lift revealed behind that place is not delivered on this
    /// cursor; a listing started again from the beginning shows it. The next
    /// cursor is a lifted one ([`Cursor::lifted`]), counting the lifted list --
    /// which the viewer may now see whole, so the count gives nothing away.
    ///
    /// The fog filter runs **before** the cursor arithmetic, not after: the
    /// position is a count of this viewer's own visible events, so no number
    /// that leaves the gateway is derived from an event the viewer may not see
    /// (decisions-log item 26).
    ///
    /// # Errors
    ///
    /// As [`Cursor::parse`]: an unreadable cursor is
    /// [`crate::error::Code::InvalidArgument`] and one from another snapshot is
    /// [`crate::error::Code::StaleSnapshot`]. A cursor issued after this
    /// viewer's fog lifted, handed back by a viewer whose fog has not, is
    /// [`crate::error::Code::InvalidArgument`]: the list it counts in is not
    /// this viewer's.
    pub fn page(
        &self,
        viewer: Viewer,
        policy: &FogPolicy,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page, Error> {
        let from = match cursor {
            Some(text) if !text.is_empty() => Cursor::parse(text, self.snapshot, Listing::Feed)?,
            _ => Cursor::start(self.snapshot),
        };
        let lifted = match viewer {
            Viewer::Seat(seat) => policy.unfogged(seat),
            Viewer::Spectator { .. } | Viewer::Admin => false,
        };
        if from.lifted() && !lifted {
            return Err(Error::invalid(
                "that cursor was issued to a viewer whose fog had lifted, and this viewer's has \
                 not: start the feed again",
            ));
        }
        let shown = usize::try_from(from.index()).map_err(|_| {
            Error::invalid(format!(
                "this cursor points at event {}, past any listing",
                from.index()
            ))
        })?;
        // As asked, never past the page cap: a limit of 0 answers no event,
        // as `list_beacons` answers a page of none (no silent `.max(1)`).
        let limit = limit.min(MAX_PAGE_EVENTS);

        // The bus place the cursor resumes at: just after the last event it
        // was shown, counted in the list it was issued in. Under one policy
        // that list is this one; across a lift it is the fogged one.
        let resume_at = match shown.checked_sub(1) {
            None => 0,
            Some(last) => match self.listing(viewer, from.lifted()).nth(last) {
                Some(place) => place.checked_add(1).ok_or_else(|| {
                    Error::internal("a bus place past `usize` cannot be resumed after")
                })?,
                // A count past the list it was issued in is a place at its
                // end, as it was before the lift: nothing more to give.
                None => self.events.len(),
            },
        };

        let visible: Vec<usize> = self.listing(viewer, lifted).collect();
        let before = visible
            .iter()
            .take_while(|place| **place < resume_at)
            .count();
        let events: Vec<Event> = visible
            .iter()
            .skip(before)
            .take(limit)
            .filter_map(|place| self.events.get(*place).cloned())
            .collect();
        let shown_events: Vec<&Event> = visible
            .iter()
            .filter_map(|place| self.events.get(*place))
            .collect();
        let digests = digests_of(&shown_events);
        let index = before.saturating_add(events.len());
        let next = Cursor {
            snapshot: self.snapshot,
            listing: Listing::Feed,
            index: u32::try_from(index).map_err(|_| {
                Error::internal(format!("a feed listing of {index} does not fit a cursor"))
            })?,
            lifted,
        };
        Ok(Page {
            events,
            digests,
            next_cursor: next,
        })
    }

    /// The bus places `viewer` is shown, in order, with its fog lifted or not.
    fn listing(&self, viewer: Viewer, lifted: bool) -> impl Iterator<Item = usize> + '_ {
        self.events
            .iter()
            .zip(&self.witnesses)
            .enumerate()
            .filter(move |(_, (event, witnesses))| {
                shown_to(viewer, lifted, &event.audience, **witnesses)
            })
            .map(|(place, _)| place)
    }
}

/// Whether `viewer` is shown an event of `audience`, witnessed by
/// `witnesses`: [`crate::fog`]'s table, with a seat's sight read from the
/// witnesses and its policy from `lifted`.
fn shown_to(viewer: Viewer, lifted: bool, audience: &Audience, witnesses: Witnesses) -> bool {
    match (viewer, audience) {
        // Secrecy is absolute and has no policy that lifts it.
        (Viewer::Seat(seat), Audience::Private(owner)) => seat == *owner,
        (_, Audience::Public) => true,
        (Viewer::Seat(seat), Audience::World { owner, .. }) => {
            *owner == Some(seat) || lifted || witnesses.contains(seat)
        }
        (Viewer::Spectator { nofog }, Audience::World { .. }) => nofog,
        (_, Audience::Private(_) | Audience::World { .. }) => false,
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
    // A time before the segment's start falls in its first window: the feed
    // stamps events from the segment's own epoch, so none is earlier.
    at.raw().max(0).div_euclid(DIGEST_PERIOD.raw())
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
    let seconds = at.raw().max(0).div_euclid(1000);
    let minutes = seconds.div_euclid(60);
    let rest = seconds.rem_euclid(60);
    format!("{minutes}:{rest:02}")
}

#[cfg(test)]
mod tests {
    use super::{
        Cursor, DIGEST_PERIOD, Event, Kind, Listing, MAX_PAGE_EVENTS, SegmentFeed, SnapshotId,
        ViewCursor, digests,
    };
    use crate::error::Code;
    use crate::fog::{Audience, Blind, FogPolicy, Viewer, Vision};
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
            memo: None,
        }
    }

    /// The two seats every test feed is published for.
    const SEATS: [SeatId; 2] = [SeatId::new(0), SeatId::new(1)];

    /// Publish `event` with nobody's sight on it.
    fn put(feed: &mut SegmentFeed, event: Event) -> Result<(), crate::error::Error> {
        feed.publish(event, &Blind, &SEATS)
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
        put(&mut feed, event(1_000, "commander_moved", Audience::Public)).expect("first");
        put(&mut feed, event(2_000, "beacon_placed", Audience::Public)).expect("later");
        let error = put(&mut feed, event(1_500, "beacon_placed", Audience::Public))
            .expect_err("out of order");
        assert_eq!(error.code, Code::InvalidArgument);
        let error =
            put(&mut feed, event(-1, "beacon_placed", Audience::Public)).expect_err("negative");
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
        put(&mut feed, event(0, "phase_changed", Audience::Public)).expect("published");
        put(&mut feed, event(1_000, "beacon_placed", world(1))).expect("published");
        put(&mut feed, event(2_000, "ore_delivered", world(0))).expect("published");
        put(
            &mut feed,
            event(3_000, "draft_saved", Audience::Private(SeatId::new(1))),
        )
        .expect("published");

        let policy = FogPolicy::fogged();
        let page = feed
            .page(Viewer::Seat(SeatId::new(0)), &policy, None, 64)
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
        put(&mut feed, event(0, "phase_changed", Audience::Public)).expect("published");
        for index in 0..7_i32 {
            put(
                &mut feed,
                event(
                    index.saturating_add(1).saturating_mul(100),
                    "ore_delivered",
                    world(1),
                ),
            )
            .expect("published");
            put(
                &mut feed,
                event(
                    index.saturating_add(1).saturating_mul(100),
                    "draft_saved",
                    Audience::Private(SeatId::new(1)),
                ),
            )
            .expect("published");
        }

        let policy = FogPolicy::fogged();
        let page = feed
            .page(Viewer::Seat(SeatId::new(0)), &policy, None, 64)
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
        let page = feed
            .page(Viewer::Seat(SeatId::new(0)), &nofog, None, 64)
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
            put(
                &mut feed,
                event(
                    index.saturating_mul(1_000),
                    "phase_changed",
                    Audience::Public,
                ),
            )
            .expect("published");
        }
        let policy = FogPolicy::casual();
        let whole = feed
            .page(Viewer::Seat(SeatId::new(0)), &policy, None, 64)
            .expect("a page");
        let narrow = feed
            .page(Viewer::Seat(SeatId::new(0)), &policy, None, 2)
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
            put(
                &mut feed,
                event(
                    index.saturating_mul(100),
                    "commander_moved",
                    Audience::Public,
                ),
            )
            .expect("published");
        }
        let policy = FogPolicy::casual();
        let first = feed
            .page(Viewer::Seat(SeatId::new(0)), &policy, None, 2)
            .expect("a page");
        assert_eq!(first.events.len(), 2);
        let text = first.next_cursor.render();
        assert_eq!(text.len(), 32);
        assert_eq!(first.next_cursor.index(), 2);
        let second = feed
            .page(Viewer::Seat(SeatId::new(0)), &policy, Some(&text), 2)
            .expect("a page");
        assert_eq!(second.events.len(), 2);
        assert_eq!(second.events.first().map(|e| e.at_ms), Some(Ms::new(200)));
    }

    #[test]
    fn a_cursor_from_another_snapshot_is_stale_rather_than_a_silent_restart() {
        let feed = SegmentFeed::new(snapshot());
        let other = Cursor::start(SnapshotId::of(0x00ca_5cad_ed00_0001, 1, 1));
        let policy = FogPolicy::casual();
        let error = feed
            .page(
                Viewer::Seat(SeatId::new(0)),
                &policy,
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
        let error = Cursor::parse(&text, current, Listing::Feed).expect_err("refused");
        assert_eq!(error.code, Code::InvalidArgument);
        assert_eq!(
            Cursor::parse("nonsense", current, Listing::Feed)
                .expect_err("refused")
                .code,
            Code::InvalidArgument
        );
        // A spelling this gateway never issues is not a cursor this gateway
        // issued, however it would parse.
        let issued = Cursor::start(current).render();
        assert_eq!(
            Cursor::parse(&issued.to_uppercase(), current, Listing::Feed)
                .expect_err("refused")
                .code,
            Code::InvalidArgument,
            "upper-case hex is not the rendering"
        );
        assert!(
            Cursor::parse(&issued, current, Listing::Feed).is_ok(),
            "and the rendering itself still reads"
        );
    }

    #[test]
    fn a_cursor_of_one_listing_is_refused_by_the_other() {
        let current = snapshot();
        let feed = Cursor::at(current, Listing::Feed, 2).render();
        let beacons = Cursor::at(current, Listing::Beacons, 2).render();
        assert_ne!(
            feed, beacons,
            "the listing is in the check, so the two renderings differ"
        );
        assert_eq!(
            Cursor::parse(&feed, current, Listing::Beacons)
                .expect_err("an event index is not a beacon offset")
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            Cursor::parse(&beacons, current, Listing::Feed)
                .expect_err("and not the other way round either")
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            Cursor::parse(&beacons, current, Listing::Beacons)
                .expect("its own listing reads it")
                .index(),
            2
        );
    }

    #[test]
    fn a_page_is_capped_however_large_a_limit_is_asked_for() {
        let mut feed = SegmentFeed::new(snapshot());
        for _ in 0..(MAX_PAGE_EVENTS + 10) {
            put(&mut feed, event(0, "commander_moved", Audience::Public)).expect("published");
        }
        let policy = FogPolicy::casual();
        let page = feed
            .page(Viewer::Seat(SeatId::new(0)), &policy, None, usize::MAX)
            .expect("a page");
        assert_eq!(page.events.len(), MAX_PAGE_EVENTS);
    }

    #[test]
    fn a_view_cursor_round_trips_and_is_opaque() {
        let cursor = ViewCursor {
            view_id: 0x00ca_5cad_ed00_0001,
            from_seq: 12,
            to_seq: 40,
            index: 7,
        };
        let text = cursor.render();
        assert_eq!(text.len(), ViewCursor::CHARS);
        assert!(
            text.bytes()
                .all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f')),
            "{text}"
        );
        assert_eq!(
            ViewCursor::parse(&text, cursor.view_id).expect("its own view"),
            cursor
        );
        assert!(!cursor.is_keyframe());
        assert!(
            ViewCursor {
                from_seq: 0,
                ..cursor
            }
            .is_keyframe(),
            "a keyframe is measured against nothing, and no live stamp is zero"
        );
    }

    #[test]
    fn a_view_cursor_from_another_view_is_stale_and_a_damaged_one_is_refused() {
        let cursor = ViewCursor {
            view_id: 9,
            from_seq: 1,
            to_seq: 1,
            index: 0,
        };
        let text = cursor.render();
        assert_eq!(
            ViewCursor::parse(&text, 10).expect_err("another view").code,
            Code::StaleSnapshot,
            "a restore mints a new view and the client asks for a keyframe"
        );
        let mut damaged = text.clone();
        damaged.replace_range(20..21, "f");
        assert_eq!(
            ViewCursor::parse(&damaged, 9).expect_err("refused").code,
            Code::InvalidArgument
        );
        assert_eq!(
            ViewCursor::parse("nonsense", 9).expect_err("refused").code,
            Code::InvalidArgument
        );
        assert_eq!(
            ViewCursor::parse(&text.to_uppercase(), 9)
                .expect_err("refused")
                .code,
            Code::InvalidArgument,
            "upper-case hex is not the rendering"
        );
    }

    /// A feed cursor and a view cursor are different lengths and different
    /// listings, so neither can be read as a place in the other.
    #[test]
    fn a_view_cursor_is_not_a_feed_cursor() {
        let snapshot = snapshot();
        let feed = Cursor::at(snapshot, Listing::Feed, 3).render();
        assert_eq!(
            ViewCursor::parse(&feed, snapshot.raw())
                .expect_err("an event index is not a place in a view")
                .code,
            Code::InvalidArgument
        );
        let view = ViewCursor {
            view_id: snapshot.raw(),
            from_seq: 3,
            to_seq: 3,
            index: 0,
        }
        .render();
        assert_eq!(
            Cursor::parse(&view, snapshot, Listing::Feed)
                .expect_err("and not the other way round")
                .code,
            Code::InvalidArgument
        );
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

    // -----------------------------------------------------------------------
    // Item 155 (3) 6: the feed under a moving sight (S3's `gwp`). The cases
    // are `ed3a`'s probe's (#166), on the probe's bus, plus the fog-lift case
    // it left unrun and a sweep over every sequence of sight, page size and
    // lift.
    // -----------------------------------------------------------------------

    /// A sight that sees everything or nothing.
    struct Sees(bool);

    impl Vision for Sees {
        fn sees(&self, _seat: SeatId, _at: &Voxel) -> bool {
            self.0
        }
    }

    const SEAT_0: Viewer = Viewer::Seat(SeatId::new(0));

    /// The probe's bus, each event published with the sight `seen` gives it.
    fn probe_bus(seen: impl Fn(&str) -> bool) -> SegmentFeed {
        let mut feed = SegmentFeed::new(snapshot());
        for (at_ms, name, owner) in [
            (60_000, "own_early", 0),
            (369_000, "theirs_6_09", 1),
            (380_000, "theirs_6_20", 1),
            (444_000, "own_7_24", 0),
            (450_000, "own_7_30", 0),
        ] {
            feed.publish(event(at_ms, name, world(owner)), &Sees(seen(name)), &SEATS)
                .expect("published");
        }
        feed
    }

    fn times(page: &super::Page) -> Vec<i32> {
        page.events.iter().map(|event| event.at_ms.raw()).collect()
    }

    /// Case 1, sight that grows: the probe read with no sight, then with
    /// sight, and was given 7:24 and 7:30 twice, after 7:30, and never
    /// 6:09 and 6:20. Sight is now what a seat had as an event happened, so a
    /// later sight neither re-delivers nor reorders: what it did not witness
    /// it is not told, and what it was told it is told once.
    #[test]
    fn sight_that_grows_after_the_events_re_delivers_nothing() {
        let policy = FogPolicy::fogged();
        let feed = probe_bus(|_| false);
        let first = feed.page(SEAT_0, &policy, None, 3).expect("a page");
        assert_eq!(times(&first), vec![60_000, 444_000, 450_000]);
        let next = first.next_cursor.render();
        let second = feed.page(SEAT_0, &policy, Some(&next), 64).expect("a page");
        assert_eq!(times(&second), Vec::<i32>::new(), "nothing twice");

        // Witnessed as they happened, the other seat's two are in the list,
        // in time order.
        let witnessed = probe_bus(|name| name.starts_with("theirs"));
        let whole = witnessed.page(SEAT_0, &policy, None, 64).expect("a page");
        assert_eq!(
            times(&whole),
            vec![60_000, 369_000, 380_000, 444_000, 450_000]
        );
    }

    /// Case 2, sight that shrinks: the probe read with sight (4 events), then
    /// without, and was given nothing: 7:30, the seat's own, was lost. Read
    /// with the sight each event was published with, it is delivered.
    #[test]
    fn sight_that_shrinks_after_the_events_loses_nothing() {
        let policy = FogPolicy::fogged();
        let feed = probe_bus(|name| name.starts_with("theirs"));
        let first = feed.page(SEAT_0, &policy, None, 4).expect("a page");
        assert_eq!(times(&first), vec![60_000, 369_000, 380_000, 444_000]);
        let next = first.next_cursor.render();
        let second = feed.page(SEAT_0, &policy, Some(&next), 64).expect("a page");
        assert_eq!(times(&second), vec![450_000], "the seat's own 7:30");
    }

    /// Case 3, the fog lifting between two reads: the match's end
    /// (`FogPolicy::end_match`) and an elimination (`eliminate`) grow the
    /// seat's list by every world event it did not witness. The cursor
    /// resumes after the last event it was shown: nothing twice, nothing
    /// behind it, and the rest of the bus, the unwitnessed included.
    #[test]
    fn the_fog_lifting_between_two_reads_re_delivers_nothing_and_runs_on() {
        let lifts: [fn(&mut FogPolicy); 2] = [FogPolicy::end_match, |policy| {
            policy.eliminate(SeatId::new(0));
        }];
        for lift in lifts {
            let feed = probe_bus(|_| false);
            let mut policy = FogPolicy::fogged();
            let first = feed.page(SEAT_0, &policy, None, 2).expect("a page");
            assert_eq!(times(&first), vec![60_000, 444_000]);
            assert!(!first.next_cursor.lifted());
            lift(&mut policy);
            let next = first.next_cursor.render();
            let second = feed.page(SEAT_0, &policy, Some(&next), 64).expect("a page");
            assert_eq!(
                times(&second),
                vec![450_000],
                "6:09 and 6:20, behind the cursor, are not delivered on it"
            );
            assert!(second.next_cursor.lifted());
            assert_eq!(
                second.next_cursor.index(),
                5,
                "the lifted list's count, which the seat may now see whole"
            );
            // A listing started again shows the whole bus.
            let again = feed.page(SEAT_0, &policy, None, 64).expect("a page");
            assert_eq!(
                times(&again),
                vec![60_000, 369_000, 380_000, 444_000, 450_000]
            );
            // And the lifted cursor reads on in the lifted list.
            let third = feed
                .page(SEAT_0, &policy, Some(&second.next_cursor.render()), 64)
                .expect("a page");
            assert!(third.events.is_empty());
        }
    }

    /// A cursor issued once a seat's fog lifted, handed to a seat whose fog has
    /// not, is refused: the list it counts in is not that seat's.
    #[test]
    fn a_lifted_cursor_is_refused_for_a_fogged_viewer() {
        let feed = probe_bus(|_| false);
        let mut lifted = FogPolicy::fogged();
        lifted.eliminate(SeatId::new(0));
        let page = feed.page(SEAT_0, &lifted, None, 1).expect("a page");
        let error = feed
            .page(
                Viewer::Seat(SeatId::new(1)),
                &lifted,
                Some(&page.next_cursor.render()),
                64,
            )
            .expect_err("seat 1's fog has not lifted");
        assert_eq!(error.code, Code::InvalidArgument);
    }

    /// The test any fix must pass, from #166: over **every** sequence here --
    /// each other seat's event witnessed or not, each read's page size, and the
    /// fog lifting before any read or not at all -- the times a seat is given
    /// never run backwards, no event is given twice, and every event of the
    /// seat's own is given. Reads are interleaved with publishing, so sight
    /// changes between reads as it does in a Push.
    #[test]
    fn over_every_sight_page_size_and_lift_the_feed_runs_forward_once_and_loses_nothing() {
        // (at_ms, owner): seat 0's and seat 1's interleaved, two at one time.
        let bus: [(i32, u8); 7] = [
            (100, 0),
            (200, 1),
            (200, 0),
            (300, 1),
            (400, 1),
            (400, 0),
            (500, 1),
        ];
        let theirs = bus.iter().filter(|(_, owner)| *owner == 1).count();
        let masks = u32::try_from(theirs)
            .ok()
            .and_then(|count| 1_u32.checked_shl(count))
            .expect("a few events");
        let mut cases = 0_u32;
        for mask in 0..masks {
            for limit in [1_usize, 2, 3] {
                for lift_at in 0..=bus.len() {
                    let mut feed = SegmentFeed::new(snapshot());
                    let mut policy = FogPolicy::fogged();
                    let mut cursor: Option<String> = None;
                    let mut given: Vec<(i32, String)> = Vec::new();
                    let mut other = 0_u32;
                    for (index, (at_ms, owner)) in bus.iter().enumerate() {
                        let name = format!("e{index}");
                        let witnessed = if *owner == 1 {
                            let seen = mask.checked_shr(other).is_some_and(|bits| bits & 1 == 1);
                            other = other.saturating_add(1);
                            seen
                        } else {
                            false
                        };
                        feed.publish(
                            event(*at_ms, &name, world(*owner)),
                            &Sees(witnessed),
                            &SEATS,
                        )
                        .expect("published");
                        if index == lift_at {
                            policy.end_match();
                        }
                        let page = feed
                            .page(SEAT_0, &policy, cursor.as_deref(), limit)
                            .expect("a page");
                        given.extend(
                            page.events
                                .iter()
                                .map(|event| (event.at_ms.raw(), event.kind.name().to_owned())),
                        );
                        cursor = Some(page.next_cursor.render());
                    }
                    // Drain what is left.
                    loop {
                        let page = feed
                            .page(SEAT_0, &policy, cursor.as_deref(), limit)
                            .expect("a page");
                        if page.events.is_empty() {
                            break;
                        }
                        given.extend(
                            page.events
                                .iter()
                                .map(|event| (event.at_ms.raw(), event.kind.name().to_owned())),
                        );
                        cursor = Some(page.next_cursor.render());
                    }
                    let what = format!("mask {mask:b}, limit {limit}, lift at {lift_at}");
                    assert!(
                        given.windows(2).all(|pair| match pair {
                            [a, b] => a.0 <= b.0,
                            _ => true,
                        }),
                        "{what}: time ran backwards: {given:?}"
                    );
                    let mut names: Vec<&String> = given.iter().map(|(_, name)| name).collect();
                    names.sort();
                    let before = names.len();
                    names.dedup();
                    assert_eq!(names.len(), before, "{what}: given twice: {given:?}");
                    for (index, (_, owner)) in bus.iter().enumerate() {
                        if *owner == 0 {
                            let name = format!("e{index}");
                            assert!(
                                given.iter().any(|(_, held)| *held == name),
                                "{what}: the seat's own {name} was lost: {given:?}"
                            );
                        }
                    }
                    cases = cases.saturating_add(1);
                }
            }
        }
        assert_eq!(cases, 16 * 3 * 8, "every sequence was played");
    }
}
