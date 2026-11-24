// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The watch rig's two connections: which call goes out next, and what each answer means.
//!
//! The lobby (GDScript) spawns `gamectl host`, reads its announce line, holds the two
//! tokens and the two WebSocket connections — the **admin** connection, which controls the
//! match, and the **seat** connection, which watches it (decisions-log item 107 (2) and
//! (3); `docs/design/skeleton-plan-t16a-notes.md` section A (2)). What goes over them is
//! decided here, because every one of those decisions is either a JSON-RPC frame to build
//! or read, which is marshalling, or a question of *when*, which is the pacer's and the
//! host clock's ([`crate::pacer`]) — and GDScript holds neither.
//!
//! Tokens never reach this module: they ride the WebSocket upgrade, which GDScript owns.
//!
//! # One request in flight per connection
//!
//! The gateway answers one request per connection at a time and in order, so the rig
//! keeps exactly one in flight on each and matches every answer to it by id. An answer
//! whose id is not the one in flight is dropped as stale — it can only be from before a
//! reconnect.
//!
//! # Whose footer is believed
//!
//! Every answer carries the `_status` footer, and the two connections' answers can arrive
//! in either order relative to each other. Only the **admin** connection changes the
//! phase — `end_lull`, `advance_push` and `end_recap` are admin methods — so only its
//! answers move the rig's phase. A seat answer served just before an `end_lull` and read
//! just after it would otherwise flip the rig back into the Lull and restart the clock.
//!
//! # What each connection sends, in priority order
//!
//! | admin | seat |
//! |---|---|
//! | `get_status` until the phase is known | a keyframe, or the next page of one |
//! | `end_lull` once the Lull is ready to end | `set_ready`, once the human says so |
//! | `end_recap` once the human continues | the editor's planning calls, in a Lull |
//! | `advance_push`, when the pacer says | `get_economy_forecast`, for the meter |
//! | | `get_recap`, once a recap opens or the match ends |
//! | | `get_status`, at its open, each Lull's opening and after an accepted seal |
//! | `report_host_clock`, outside a Push | `get_view` from the cursor after the match moved |
//! | | `get_segment_feed` after the match moved |
//! | a keep-alive `get_status` | a keep-alive `get_status` |
//!
//! "Ready ends the Lull": the host clock's answer carries `all_ready`, and when it is true
//! or the Lull's countdown has run out, the admin connection calls `end_lull`. That is the
//! one gameplay-shaped decision in this file, and it is the spec's, not the client's.
//! `set_ready` waits behind anything the editor still owes the gateway, so a Ready pressed
//! after Submit reaches the gateway after the submission it is ready with.
//!
//! # The seat connection's rate budget
//!
//! The editor shares the seat connection with the vista's polls
//! (`docs/design/skeleton-plan-t16a-notes.md` section B, "T19" (5)), and the seat token's
//! budget is the gateway's: `pharmakos_gateway::limit::CALLS_PER_TICK` calls per gateway
//! tick. That tick moves only when the match's clock does: outside a Push, when the admin
//! connection's `report_host_clock` is answered, about four times a second; in a Push, when
//! an `advance_push` ran any game time (section B, "T19" (3)). So the rig spends at most
//! [`SEAT_CALLS_PER_REFILL`] seat calls between two such answers. With one call in flight per
//! connection, at most one more can land in the same gateway tick, the one that was already
//! on its way when the clock moved, which keeps the seat under the gateway's limit however
//! fast the player edits (`tests/rate_budget.rs`).
//!
//! **Not every clock answer moves the clock** (decisions-log item 121 (4), the watch check's
//! flake). The gateway floors a phase's reported time to whole ticks, keeps it as a
//! high-water mark, and starts every phase's clock again from zero, carrying what the last
//! one spent; so a phase change moves no tick, and neither does a phase's first report,
//! which goes out at once with almost nothing spent. So the rig refills on the answer to
//! every report but a phase's first. The pacer sends each later report with the phase's
//! clock at least one report interval ([`CLOCK_REPORT_US`]) past the last report the
//! gateway accepted (a larger step after a stall), which moves the gateway's tick as long
//! as a tick is shorter than a report interval; the rig counts reports and computes no time
//! of its own. Before, every
//! answer refilled, and at a Lull that follows a recap the calls left from the recap's last
//! refill and six more could all land in one tick (`tests/phase_budget.rs`).
//!
//! # The meter
//!
//! The own `$`/`kW` meter (T19, pull request 2) is `get_economy_forecast` on the seat
//! connection, inside the same budget: due when a Lull opens (a resumed one too) and whenever
//! the match moved in a Push, as the view and the feed are. Its four fields are kept exactly
//! as the gateway answered them ([`Meter`]); headroom is the gateway's, never supply minus
//! draw. When to poll is scheduling, which is this file's; what the numbers are is the
//! gateway's.
//!
//! PLACEHOLDER: the meter's refresh cadence and layout — OWNER, S6, with the real lobby.
//! Once a Lull and once per advance in a Push are the skeleton's; Tuning.
//!
//! # The editor's door
//!
//! The rig sends the editor's planning calls only in a Lull, and none once this seat is
//! ready. It tells the editor so ([`Rig::door`], [`crate::editor::Door`]) every time the
//! admin footer moves the phase and every time Ready goes out or is refused, so the editor
//! never waits for a call the rig will not send (S1's stage demo, finding K). It also hands
//! the editor the `safe_playbook_filed` line of the seat's own feed, so the panel can say the
//! orders on screen were not sealed (finding J).
//!
//! # A first phase of Push
//!
//! A match resumed from a `sealed` save starts in its Push with no Lull (decisions-log item
//! 111, decision C8). The rig takes whatever phase the first admin footer names: a Push is
//! paced from its first answer, and the editor waits for the next Lull.

use pharmakos_proto::gp::api::v1::{
    GetEconomyForecastResponse, GetRecapResponse, GetSegmentFeedResponse, GetStatusResponse,
    Preset, Status, event, ledger_memo, status,
};
use pharmakos_proto::json::{self, Json};

use crate::editor::{Door, Editor, ReadFailed, SAFE_PLAYBOOK_FILED_KIND};
use crate::enums;
use crate::error::BridgeError;
use crate::pacer::{CLOCK_REPORT_US, ClockPhase, KEEPALIVE_US, Timing, clock_text};
use crate::variant::proto3_integer;
use crate::view::{read_status, split_footer};

/// Seat calls the rig sends between two moves of the gateway's clock.
///
/// The gateway admits a token eight calls per tick (`pharmakos_gateway::limit::
/// CALLS_PER_TICK`, itself a PLACEHOLDER for hardening). Six here plus the one call that can
/// straddle a clock move is seven, one under the limit, so a keep-alive or a late answer
/// never tips a burst of edits into `RATE_LIMITED`. `tests/rate_budget.rs` reads the
/// gateway's constants out of its source and drives the rig against them.
///
/// PLACEHOLDER: the seat's calls per refill — OWNER, at hardening, with the gateway's
/// rate limits. The margin is the client's; the numbers are the owner's, and this follows
/// them.
pub const SEAT_CALLS_PER_REFILL: u32 = 6;

/// The admin connection's index.
pub const ADMIN: usize = 0;

/// The seat connection's index.
pub const SEAT: usize = 1;

/// The match phase, as the admin connection's latest footer says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// No admin answer has been read yet.
    Unknown,
    /// Before the match: not a phase this host serves, carried for completeness.
    Lobby,
    /// Planning.
    Lull,
    /// The segment plays.
    Push,
    /// The Ledger settles.
    Recap,
    /// The match is over.
    Ended,
}

impl Phase {
    /// The name GDScript and the logs print.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Lobby => "lobby",
            Self::Lull => "lull",
            Self::Push => "push",
            Self::Recap => "recap",
            Self::Ended => "ended",
        }
    }

    fn of(wire: i32) -> Self {
        match status::Phase::try_from(wire) {
            Ok(status::Phase::Lobby) => Self::Lobby,
            Ok(status::Phase::Lull) => Self::Lull,
            Ok(status::Phase::Push) => Self::Push,
            Ok(status::Phase::Recap) => Self::Recap,
            Ok(status::Phase::Ended) => Self::Ended,
            _ => Self::Unknown,
        }
    }
}

/// What a request was for, so its answer can be routed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Purpose {
    Status,
    View,
    Feed,
    Advance,
    Clock,
    EndLull,
    EndRecap,
    Ready,
    /// One of the editor's planning calls ([`crate::editor`]).
    Plan,
    /// `get_economy_forecast`, for the meter.
    Meter,
    /// `get_recap`, for the recap's lines.
    Recap,
}

impl Purpose {
    /// The call's name in the string table's `call_word_<word>` rows, for the words that say a
    /// close on a message (fix pass 3).
    const fn word(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::View => "view",
            Self::Feed => "feed",
            Self::Advance => "advance",
            Self::Clock => "clock",
            Self::EndLull => "end_lull",
            Self::EndRecap => "end_recap",
            Self::Ready => "ready",
            Self::Plan => "plan",
            Self::Meter => "meter",
            Self::Recap => "recap",
        }
    }
}

/// The last close on a message itself (S3's `fixq-c`, fix pass 3): a message too big for a
/// side's buffer (1009, the client's own close when an answer is past its inbound buffer, or
/// the gateway's), or one a side could not read. Kept for the lobby's words, which say it with
/// whether the connection is open again ([`Rig::link_open`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClosedOnMessage {
    /// [`ADMIN`] or [`SEAT`].
    pub link: usize,
    /// The connection's name in the string table's `connection_<name>` rows: `admin` or `seat`.
    pub connection: &'static str,
    /// The close code.
    pub code: u16,
    /// The call the message carried, as the string table's `call_word_<call>` names it; none
    /// when no call was in flight.
    pub call: Option<&'static str>,
}

/// A connection's name in the string table's `connection_<name>` rows, for [`ADMIN`] and
/// [`SEAT`]; no other link exists.
const fn connection_word(link: usize) -> Option<&'static str> {
    match link {
        ADMIN => Some("admin"),
        SEAT => Some("seat"),
        _ => None,
    }
}

/// The own `$`/`kW` meter: the last `get_economy_forecast` answer, field for field, and
/// which phase it was served in.
///
/// Nothing here is computed: each number is the one the gateway wrote, so a headroom that
/// is not supply minus draw is shown as the headroom the gateway sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Meter {
    /// How many answers the meter has taken; zero before the first.
    pub answers: u64,
    /// The phase the last answer was served in, as its own footer says.
    pub phase: Phase,
    /// `treasury_now`, whole `$`.
    pub treasury_now: i32,
    /// `supply_kw_now`, whole `kW`.
    pub supply_kw_now: i32,
    /// `draw_kw_now`, whole `kW`.
    pub draw_kw_now: i32,
    /// `headroom_kw_now`, whole `kW`, as the gateway wrote it.
    pub headroom_kw_now: i32,
    /// `bmi_next_dollars`, whole `$`: the BMI at the band the seat holds now, a prediction
    /// that ignores committed spend (decisions-log item 135 (2) (g)). `None` when the answer
    /// left it out — a seat out of the match, a match that has ended — which is not 0.
    pub bmi_next_dollars: Option<i32>,
    /// `committed_dollars`, whole `$`: the upper bound of what the seal orders and has not
    /// yet paid. `None` when the answer left it out, which it does outside a Lull.
    pub committed_dollars: Option<i32>,
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            answers: 0,
            phase: Phase::Unknown,
            treasury_now: 0,
            supply_kw_now: 0,
            draw_kw_now: 0,
            headroom_kw_now: 0,
            bmi_next_dollars: None,
            committed_dollars: None,
        }
    }
}

/// How long each Lull is declared to be, as the rules table says: `match.first_lull_ms` for
/// round 1's and `match.lull_ms` for every later one (spec section 3's ten and five minutes;
/// decisions-log item 127 (2)). `None` is a Lull with no countdown.
///
/// The client's to know because the Lull's timer is the client's: the gateway's footer
/// shows the countdown this client reports and has none of its own before the first report
/// (skeleton-plan-t16a-notes.md section D: "the client counts, the gateway is told").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LullLengths {
    /// Round 1's Lull, in milliseconds.
    pub first_ms: Option<u64>,
    /// Every later Lull, in milliseconds.
    pub later_ms: Option<u64>,
}

impl LullLengths {
    /// The Lull that opens round `round`: the first Lull's for round 1, the later Lulls'
    /// after it. Round 0, before any Lull, reads as the first, as the gateway's
    /// `LullLengths::for_round` does.
    #[must_use]
    pub const fn for_round(self, round: u32) -> Option<u64> {
        if round <= 1 {
            self.first_ms
        } else {
            self.later_ms
        }
    }
}

/// One seat's tally in the recap once the match is decided, as the gateway counted it
/// (`gp.api.v1.SeatTally`; decisions-log item 145 (7)).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SeatTallyRow {
    /// The seat, spelt as the gateway spells a seat: `seat.0`.
    pub seat: String,
    /// Enemy assets destroyed with a share of the credit.
    pub kills: u32,
    /// Of `kills`, those whose credit was split.
    pub shared_kills: u32,
    /// Its own assets destroyed.
    pub losses: u32,
    /// The enemy value it destroyed, whole dollars.
    pub value_destroyed_dollars: i32,
}

/// The recap: the last `get_recap` answer's prose, as the gateway wrote it, and every seat's
/// tally once the match is decided.
///
/// The settlement's line, the shortfall's and why a step found nothing ("3 matched, none
/// reachable") are all sentences of that prose (`econ`, decisions-log item 135 (1)); the
/// client draws them and words none of them. The prose's own tally is the calling seat's;
/// every seat's is on the answer's `seat_tallies` and nowhere in the prose (item 155 (3) 7).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Recap {
    /// How many answers the recap has taken; zero before the first.
    pub answers: u64,
    /// The round the answer's own footer named.
    pub round: u32,
    /// The prose, exactly as it came.
    pub prose: String,
    /// Every seat's tally, ascending by seat as it came; empty between rounds.
    pub seat_tallies: Vec<SeatTallyRow>,
    /// The Ledger's memos to this seat over the match so far, in the order the gateway
    /// issued them (`ledger_memos`, S3's plan, E5); empty outside a Probation match. The
    /// recap is where a memo stays after it scrolled off the event list.
    pub memos: Vec<MemoRow>,
    /// The gateway's refusal of this recap's `get_recap`, `code: message` as it came, or
    /// empty. A recap whose feed lost a `segment_ended` is refused with `INTERNAL` naming
    /// the loss (decisions-log item 135 (1)), and that is the line the player is owed.
    pub refusal: String,
}

/// One of the Ledger's memos, as the gateway issued it (`gp.api.v1.LedgerMemo`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MemoRow {
    /// Which memo, spelt as the wire spells it (`power_shortfall`). Carried, never drawn
    /// and never branched on: the text is the memo.
    pub id: String,
    /// The memo, exactly as the gateway wrote it.
    pub text: String,
}

/// The preset a hosted match is played under, as `get_status` names it (S3's `probw`). What
/// a preset changes is the gateway's: the client names it and decides nothing from it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchPreset {
    /// `custom`.
    Custom,
    /// `probation`.
    Probation,
}

impl MatchPreset {
    /// Every preset the schema names, in its order (`gp.api.v1.Preset` without its
    /// unspecified zero); a test holds the two lists together.
    pub const ALL: [Self; 2] = [Self::Custom, Self::Probation];

    /// The preset's name as the wire and the config line spell it, and the key the lobby's
    /// string table names it by (`preset_<name>`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Custom => "custom",
            Self::Probation => "probation",
        }
    }

    /// The preset a name spells, or `None` for a name no preset has.
    #[must_use]
    pub fn of_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.name() == name)
    }
}

/// What a `get_status` answer says the gateway hosts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ServedPreset {
    /// The answer names no preset: the schema's word for "no match is hosted"
    /// (`PRESET_UNSPECIFIED`, absent on the wire).
    NoMatch,
    /// A match is hosted under this preset.
    Hosted(MatchPreset),
}

impl ServedPreset {
    /// What an answer's decoded field says, or `None` for a number the schema names no
    /// preset by, which this build cannot read.
    fn of_wire(wire: i32) -> Option<Self> {
        match Preset::try_from(wire) {
            Ok(Preset::Unspecified) => Some(Self::NoMatch),
            Ok(Preset::Custom) => Some(Self::Hosted(MatchPreset::Custom)),
            Ok(Preset::Probation) => Some(Self::Hosted(MatchPreset::Probation)),
            Err(_) => None,
        }
    }

    /// The name the lobby's words are looked up by: the preset's own, or `none`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NoMatch => "none",
            Self::Hosted(preset) => preset.name(),
        }
    }
}

/// What `get_status`'s body says beside its footer (S3's `probw`): the hosted match's preset,
/// told to every caller, and for this seat whether it holds a seal of the current round, told
/// to a seat's token alone. The preset is read from the admin connection's first answer and
/// every later one; the seat's seal from the seat connection's `get_status`, which the rig asks
/// when the seat connection opens, at every Lull's opening and once after this seat's
/// `submit_plan` is accepted (and which a keep-alive also answers). Each field is the gateway's
/// last word on it, never a guess: the client never sets the seal because it submitted, it asks.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct StatusFacts {
    /// How many `get_status` answers were read; zero before the first.
    pub answers: u64,
    /// What the gateway hosts; `None` before the first readable answer.
    pub preset: Option<ServedPreset>,
    /// The seat connection's newest readable word on its seal (`sealed_this_round`), with the
    /// round and phase its answer's footer names; `None` before one, or when that answer did not
    /// say (the field's explicit presence: "not told" is never a false). It is that round's and
    /// that phase's alone: [`Rig::sealed_this_round`] reads it only while the rig is still there.
    pub sealed: Option<SealFact>,
    /// The newest read's refusal or unreadable answer, until a later read is read: the
    /// facts above are then the last read's, and the lobby says the newest did not come.
    pub failed: Option<ReadFailed>,
}

/// What the gateway told this seat of its seal, and when: the round and phase its answer's
/// footer named.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealFact {
    /// The round the answer was served in.
    pub round: u32,
    /// The phase the answer was served in.
    pub phase: Phase,
    /// `sealed_this_round`, as told.
    pub sealed: bool,
}

/// Whether the hosted match is the preset the lobby asked for (`Rig::expect_preset`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresetCheck {
    /// No `get_status` answer has been read yet.
    NotRead,
    /// The gateway says it hosts no match.
    NoMatch,
    /// The lobby asked for none (a Resume repeats a remembered line): the gateway's word
    /// names the match.
    Unasked(MatchPreset),
    /// The gateway hosts what the lobby asked for.
    Agrees(MatchPreset),
    /// The gateway hosts another preset than the one the lobby asked for: a refusal the
    /// lobby shows, never papered over with the name it asked for.
    Disagrees {
        /// What the lobby wrote on the config line.
        asked: MatchPreset,
        /// What the gateway says it hosts.
        served: MatchPreset,
    },
}

impl PresetCheck {
    /// The name GDScript looks the words up by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotRead => "not_read",
            Self::NoMatch => "no_match",
            Self::Unasked(_) => "unasked",
            Self::Agrees(_) => "agrees",
            Self::Disagrees { .. } => "disagrees",
        }
    }
}

/// Who an event-list row is from: the feed's ordinary event, or one of the Ledger's memos
/// (an event whose typed payload is a `LedgerMemo`), which the lobby draws under the
/// Ledger's speaker word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Speaker {
    /// An ordinary event: drawn with its kind.
    Event,
    /// The Ledger's memo: drawn with the speaker word in place of the kind.
    Ledger,
}

/// One event-list row, as the feed sent it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EventRow {
    /// The event's time in the segment, as the pacer's clock spells it.
    pub clock: String,
    /// Its kind, as the gateway wrote it.
    pub kind: String,
    /// Its words: the gateway's text, or for a memo the memo's own text.
    pub text: String,
    /// Who it is from.
    pub speaker: Speaker,
}

impl EventRow {
    /// The row as the event list's golden and the log print it: the time, the kind and the
    /// words, exactly as they came.
    #[must_use]
    pub fn line(&self) -> String {
        format!("{}  {}  {}", self.clock, self.kind, self.text)
    }
}

/// A JSON-RPC request's text, as the rig sends it: the frame whose size, in UTF-8 bytes after
/// JSON's own escaping, is held to the gateway's message cap.
fn frame_text(id: i64, method: &str, params: Json) -> String {
    json::write(&object(vec![
        ("jsonrpc", Json::String("2.0".to_owned())),
        ("id", number(id)),
        ("method", Json::String(method.to_owned())),
        ("params", params),
    ]))
}

/// One connection's state.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Link {
    open: bool,
    next_id: i64,
    in_flight: Option<(i64, Purpose)>,
    drops: u32,
    /// Drops closed on the message in flight, by either side ([`Close::ThisMessage`]): counted
    /// under their own reason, never asked again.
    message_drops: u32,
    /// Calls not sent because their frame was past the gateway's message cap.
    unsent: u32,
    /// Calls the transport refused to send (`Rig::send_failed`).
    send_failures: u32,
    calls: u64,
}

/// Why a connection closed, as far as the call in flight goes (S3's `fixq-c`, fix pass 1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Close {
    /// The connection went: the call in flight is asked again once it is back.
    Connection,
    /// A side closed on the message itself: the gateway, or this client when an answer is past
    /// its inbound buffer (Godot closes with 1009, fix pass 3). RFC 6455 close codes 1003
    /// unsupported data, 1007 invalid payload, 1008 policy violation, 1009 message too big: the
    /// same call would close it again, so it is settled as refused and never asked again.
    ThisMessage(u16),
}

impl Close {
    /// The close a WebSocket close code names; anything but the four codes about a message is
    /// the connection's.
    #[must_use]
    pub fn of_code(code: i64) -> Self {
        match u16::try_from(code) {
            Ok(code @ (1003 | 1007 | 1008 | 1009)) => Self::ThisMessage(code),
            _ => Self::Connection,
        }
    }
}

/// One frame to send: which connection, and the JSON-RPC text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outgoing {
    /// [`ADMIN`] or [`SEAT`].
    pub link: usize,
    /// The request, as the gateway reads it.
    pub text: String,
}

/// What one answer meant.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Answer {
    /// A `get_view` result, footer included, for [`crate::view`] to decode and apply.
    pub view: Option<Json>,
    /// New event-list rows from `get_segment_feed`, in the feed's order.
    pub events: Vec<EventRow>,
    /// The gateway's refusal, as `CODE: message`, when the call was refused.
    pub error: Option<String>,
    /// Whether this answer moved the phase.
    pub phase_changed: bool,
}

/// What the seat connection owes the view and the feed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Due {
    /// A keyframe: at the start, after a reconnect, and after a refused cursor.
    keyframe: bool,
    /// A `get_view` from the cursor, because the match moved.
    view: bool,
    /// A `get_segment_feed` from the cursor, because the match moved.
    feed: bool,
}

/// The reads the seat connection owes the panel, beside the view and the feed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Reads {
    /// A `get_economy_forecast`, because a Lull opened or the match moved in a Push.
    meter: bool,
    /// A `get_recap`, because a recap opened or the match ended.
    recap: bool,
    /// A `get_status` on the seat connection: the seat's own facts ([`StatusFacts`]), owed
    /// when the seat connection opens, at every Lull's opening and after this seat's accepted
    /// `submit_plan`.
    status: bool,
}

/// What the human, the timer or the host clock asked for and the rig has not yet sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Wants {
    end_lull: bool,
    end_recap: bool,
    ready: bool,
}

/// Whether the phase has a countdown, as the admin connection's latest footer says
/// (`_status.untimed` and `_status.untimed_by_rule`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Untimed {
    /// A countdown was reported: the lobby shows it.
    Timed,
    /// No countdown has been reported yet (`untimed`).
    NotReported,
    /// The Lull has none by rule (`untimed_by_rule`, a Probation match's).
    ByRule,
}

/// Where a timed Lull's run-out stands: its countdown is reported at zero, and only once the
/// gateway has answered that report is the Lull ended, so the gateway can tell a Lull that ran
/// out from one Ready ended (decisions-log item 160 (4)). A refused or lost zero is sent again.
/// Scheduling, as the pacer's clock is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum RanOut {
    /// The countdown has not run out this Lull, or the Lull is untimed.
    #[default]
    No,
    /// It ran out: the closing reports are owed, ahead of `end_lull`.
    Owed,
    /// The zero is on its way.
    InFlight,
    /// The gateway answered the zero (or the wire could not carry it); `end_lull` follows.
    Reported,
    /// The gateway refused the zero [`ZERO_REFUSALS`] times: no further zero is sent this Lull,
    /// its refusal is said ([`Rig::zero_refusal`]), and `end_lull` follows. What the gateway
    /// then files is its own; the client claims nothing about it.
    GivenUp,
}

/// How many times the gateway may refuse a Lull's closing zero before the rig gives it up and
/// ends the Lull without it: enough for a rate limit to clear at the clock's cadence (one
/// report a quarter-second apart), few enough that a gateway which will never take the zero
/// does not hold the Lull open behind it. A zero lost with its link is no refusal, and is
/// sent again when the link is back, uncounted.
pub const ZERO_REFUSALS: u8 = 3;

/// How many refused or unreadable reads of the seat's seal (`get_status`) are asked again in one
/// phase before the rig stops asking until the next event that owes one (S3's `fixq-c`, fix pass
/// 1): as [`ZERO_REFUSALS`], enough for a rate limit to clear, few enough that a gateway which
/// never answers does not spend the seat's budget for the whole Lull. While it is not told, the
/// Ready line says the client is asking, and the read's refusal is said beside it.
pub const STATUS_RETRIES: u8 = 3;

/// Where a phase's clock reports stand, for the seat's budget: a phase's first report
/// moves no gateway tick, and every later one does (the module doc's "Not every clock
/// answer moves the clock").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClockReports {
    /// The phase has sent no report yet.
    Unsent,
    /// The phase's first report is in flight; its answer refills nothing.
    FirstOut,
    /// The phase's first report has gone out; the answer to any later one refills.
    Refilling,
}

/// The watch rig: the two connections' state, the view and feed cursors, the timing, and
/// the editor that shares the seat connection.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rig {
    links: [Link; 2],
    phase: Phase,
    round: u32,
    timing: Timing,
    view_cursor: String,
    view_paging: Option<String>,
    feed_cursor: String,
    due: Due,
    wants: Wants,
    all_ready: bool,
    last_error: Option<String>,
    /// The last close on a message itself, for the player's words.
    closed_on: Option<ClosedOnMessage>,
    /// How long each Lull is, from the rules table's two Lull rows.
    lulls: LullLengths,
    /// Whether the admin connection's latest footer said the phase has no countdown
    /// (`_status.untimed`), or that the Lull is untimed by rule (`_status.untimed_by_rule`): the
    /// lobby then shows none (S1-11's client half, item 135).
    untimed: Untimed,
    /// How many `report_host_clock` calls the gateway answered, and how many the rig sent with
    /// a countdown: what the watch check reads to see a Lull untimed by rule asked about and
    /// sent no countdown.
    clock_answers: u64,
    countdowns_sent: u64,
    /// Where this Lull's run-out stands ([`RanOut`]).
    ran_out: RanOut,
    /// How many times the gateway refused this Lull's closing zero.
    zero_refusals: u8,
    /// The gateway's refusal of this Lull's closing zero, as it came, once the rig gave it up.
    zero_refusal: Option<String>,
    /// How many `get_view` pages this client refused after the gateway served them.
    view_refusals: u32,
    /// After a refused page: the wall time before which no keyframe is asked again.
    keyframe_not_before_us: u64,
    /// Seat calls left before the gateway's clock next moves.
    seat_calls_left: u32,
    /// Where this phase's clock reports stand, for the seat's budget.
    clock_reports: ClockReports,
    /// Whether `set_ready` has gone out this Lull: the seat's orders are final, and the
    /// editor sends nothing more until the next Lull.
    ///
    /// PLACEHOLDER: Ready final for the rest of the Lull — OWNER, S6, with the real lobby.
    /// An un-ready toggle that reopens the editor (`set_ready {ready: false}`) is the real
    /// lobby's.
    ready_sent: bool,
    /// The playbook editor.
    editor: Editor,
    /// The own `$`/`kW` meter.
    meter: Meter,
    /// The recap's lines, as the gateway last wrote them.
    recap: Recap,
    /// The reads the seat connection owes the meter and the recap.
    reads: Reads,
    /// What `get_status`'s body last said beside its footer.
    status_facts: StatusFacts,
    /// The seat's refused or unreadable status reads in this round's phase: asked again up to
    /// [`STATUS_RETRIES`] times, then given up until an event owes a read afresh.
    status_retries: u8,
    /// A refused status read is owed again once the seat link has been quiet for the
    /// keep-alive interval (fix pass 2: the retries went back to back, so one `RATE_LIMITED`
    /// spent them all).
    status_retry_owed: bool,
    /// The preset the lobby wrote on a new match's config line, to hold the gateway's answer
    /// to; `None` for a Resume, which repeats a remembered line.
    asked_preset: Option<MatchPreset>,
}

impl Default for Rig {
    fn default() -> Self {
        Self::new()
    }
}

impl Rig {
    /// A rig with both connections closed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            links: [Link::default(), Link::default()],
            phase: Phase::Unknown,
            round: 0,
            timing: Timing::default(),
            view_cursor: String::new(),
            view_paging: None,
            feed_cursor: String::new(),
            due: Due {
                keyframe: true,
                view: false,
                feed: false,
            },
            wants: Wants::default(),
            all_ready: false,
            last_error: None,
            closed_on: None,
            lulls: LullLengths::default(),
            untimed: Untimed::NotReported,
            clock_answers: 0,
            countdowns_sent: 0,
            ran_out: RanOut::No,
            zero_refusals: 0,
            zero_refusal: None,
            view_refusals: 0,
            keyframe_not_before_us: 0,
            seat_calls_left: SEAT_CALLS_PER_REFILL,
            clock_reports: ClockReports::Unsent,
            ready_sent: false,
            editor: Editor::default(),
            meter: Meter::default(),
            recap: Recap::default(),
            reads: Reads::default(),
            status_facts: StatusFacts::default(),
            status_retries: 0,
            status_retry_owed: false,
            asked_preset: None,
        }
    }

    /// The editor, to act on.
    pub fn editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }

    /// The editor, to read.
    #[must_use]
    pub const fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Times round 1's Lull at `lulls.first_ms` and every later one at `lulls.later_ms`, the
    /// rules table's `match.first_lull_ms` and `match.lull_ms` ([`LullLengths`]).
    pub fn set_lull_length(&mut self, lulls: LullLengths) {
        self.lulls = lulls;
    }

    /// Connection `link` is open (again). A seat connection starts with a keyframe,
    /// because the feed keeps its state per viewer and asks nothing of a new connection.
    pub fn opened(&mut self, link: usize) {
        if let Some(state) = self.links.get_mut(link) {
            state.open = true;
            state.in_flight = None;
        }
        if link == SEAT {
            self.due.keyframe = true;
            self.view_paging = None;
            self.due.feed = true;
            // A seat connection open again (a reconnect, or a resumed client) is told its seal
            // afresh: nothing it was told before stands (fix pass 1).
            self.forget_seal();
        }
    }

    /// The editor's edits restart the idle timer that owes FULL, and a submission the gateway
    /// accepted owes the seat's `get_status`, whose answer says the seal.
    fn sync_editor(&mut self) {
        if self.editor.take_edit_mark() {
            self.timing.idle.edited();
        }
        if self.editor.take_sealed_mark() {
            // A seal the gateway accepted changes the fact: the old word goes, and the line says
            // the client is asking until the gateway's own answer comes (fix pass 1).
            self.forget_seal();
        }
    }

    /// The seat's seal is no longer known: the fact goes (the Ready line says the client is
    /// asking) and `get_status` is owed, with its retries counted afresh.
    fn forget_seal(&mut self) {
        self.status_facts.sealed = None;
        self.status_retries = 0;
        self.status_retry_owed = false;
        self.reads.status = true;
    }

    /// A seat status read was refused or could not be read: the seal is not told, and it is
    /// asked again once the link has been quiet for the keep-alive interval, up to
    /// [`STATUS_RETRIES`] times in a round's phase; then given up ([`Rig::seal_not_told`]).
    fn status_read_failed(&mut self) {
        self.status_facts.sealed = None;
        if self.status_retries < STATUS_RETRIES {
            self.status_retries = self.status_retries.saturating_add(1);
            self.status_retry_owed = true;
        }
    }

    /// Whether the seat's seal is not told and no read of it is owed: the refused reads of this
    /// round's phase spent [`STATUS_RETRIES`], so the Ready line says the gateway's refusal and
    /// not that the client is asking.
    #[must_use]
    pub fn seal_not_told(&self) -> bool {
        self.sealed_this_round().is_none()
            && !self.reads.status
            && !self.status_retry_owed
            && self.status_retries >= STATUS_RETRIES
    }

    /// A zero report the gateway refused is owed again, up to [`ZERO_REFUSALS`] refusals, and
    /// then given up ([`RanOut::GivenUp`]).
    fn zero_refused(&mut self) {
        if self.ran_out != RanOut::InFlight {
            return;
        }
        self.zero_refusals = self.zero_refusals.saturating_add(1);
        self.ran_out = if self.zero_refusals >= ZERO_REFUSALS {
            RanOut::GivenUp
        } else {
            RanOut::Owed
        };
    }

    /// The gateway's refusal of this Lull's closing zero, as it came, once the rig gave the zero
    /// up: the lobby says it on the Lull's line. `None` otherwise.
    #[must_use]
    pub fn zero_refusal(&self) -> Option<&str> {
        self.zero_refusal.as_deref()
    }

    /// A zero report lost with its link (no answer, no refusal) is owed again, uncounted.
    fn zero_lost(&mut self) {
        if self.ran_out == RanOut::InFlight {
            self.ran_out = RanOut::Owed;
        }
    }

    /// Connection `link` dropped. Whatever it had in flight is carried or forgotten, and
    /// the drop is counted: the watch check asserts there were none.
    pub fn dropped(&mut self, link: usize) {
        self.closed(link, Close::Connection);
    }

    /// The transport refused to send the frame `link` has in flight (Godot's `send_text` answered
    /// the error `code`; fix pass 2): the call is settled as refused under its own reason, said
    /// where that call's refusal is shown, counted, and never asked again; the link stays as it is.
    pub fn send_failed(&mut self, link: usize, code: i64) {
        let Some(state) = self.links.get_mut(link) else {
            return;
        };
        state.send_failures = state.send_failures.saturating_add(1);
        match state.in_flight.take() {
            Some((_, Purpose::Plan)) => {
                self.editor.send_failed(code);
                self.sync_editor();
            }
            Some((_, purpose)) => self.refused(purpose, "SEND_FAILED"),
            None => {}
        }
    }

    /// Connection `link` closed, for the reason `close` names. A close on the message itself
    /// settles what was in flight as refused and asks nothing again ([`Close::ThisMessage`]);
    /// any other is [`Rig::dropped`]'s.
    pub fn closed(&mut self, link: usize, close: Close) {
        let (Some(state), Some(connection)) = (self.links.get_mut(link), connection_word(link))
        else {
            return;
        };
        if state.open {
            state.drops = state.drops.saturating_add(1);
            if matches!(close, Close::ThisMessage(_)) {
                state.message_drops = state.message_drops.saturating_add(1);
            }
        }
        state.open = false;
        if let Close::ThisMessage(code) = close {
            let call = state.in_flight.as_ref().map(|(_, purpose)| purpose.word());
            let in_flight = state.in_flight.take();
            self.closed_on = Some(ClosedOnMessage {
                link,
                connection,
                code,
                call,
            });
            match in_flight {
                Some((_, Purpose::Plan)) => {
                    self.editor.closed_on(code);
                    self.sync_editor();
                }
                // A page too big to read is a refused page: a keyframe is asked again, and no
                // sooner than the keep-alive, so it is not asked in a loop (fix pass 3).
                Some((_, Purpose::View)) => self.view_refused(),
                Some((_, purpose)) => self.refused(purpose, "CLOSED_ON_MESSAGE"),
                None => {}
            }
            return;
        }
        match state.in_flight.take() {
            Some((_, Purpose::Advance)) => self.timing.pacer.refused(),
            Some((_, Purpose::Clock)) => {
                self.timing.clock.refused();
                self.clock_reports = ClockReports::Refilling;
                self.zero_lost();
            }
            Some((_, Purpose::Plan)) => self.editor.dropped(),
            Some((_, Purpose::Meter)) => self.reads.meter = true,
            Some((_, Purpose::Recap)) => self.reads.recap = true,
            Some((_, Purpose::Status)) if link == SEAT => self.reads.status = true,
            _ => {}
        }
    }

    /// The frames to send now, given a [`crate::pacer::WallClock`] reading.
    pub fn poll(&mut self, now_us: u64) -> Vec<Outgoing> {
        self.sync_editor();
        self.timing.advance(now_us);
        if self.phase == Phase::Lull && self.timing.clock.run_out() {
            if self.ran_out == RanOut::No {
                self.ran_out = RanOut::Owed;
            }
            self.wants.end_lull = true;
        }
        let mut out = Vec::new();
        if let Some((purpose, method, params)) = self.admin_next()
            && let Some(frame) = self.send(ADMIN, purpose, method, params)
        {
            out.push(frame);
        }
        if let Some((purpose, method, params)) = self.seat_next()
            && let Some(frame) = self.send(SEAT, purpose, method, params)
        {
            out.push(frame);
        }
        out
    }

    /// Whether `link` is open with nothing in flight.
    fn idle(&self, link: usize) -> bool {
        self.links
            .get(link)
            .is_some_and(|state| state.open && state.in_flight.is_none())
    }

    fn admin_next(&mut self) -> Option<(Purpose, &'static str, Json)> {
        if !self.idle(ADMIN) {
            return None;
        }
        if self.phase == Phase::Unknown {
            // At once the first time; after that at the clock's cadence, so a footer this
            // build cannot place (an unreadable one, or an unspecified phase) is asked about
            // again without spinning against the rate limiter.
            let first = self.links.get(ADMIN).is_some_and(|state| state.calls == 0);
            if first || self.timing.quiet_at_least(ADMIN, CLOCK_REPORT_US) {
                return Some((Purpose::Status, "get_status", object(Vec::new())));
            }
            return None;
        }
        if self.ran_out == RanOut::Owed && self.phase == Phase::Lull {
            // The countdown down to zero, in steps the gateway takes, ahead of `end_lull`: only a
            // timed Lull that ran out owes it, never one Ready or the human ended.
            if let Some((spent, left, last)) = self.timing.clock.closing_report() {
                if last {
                    self.ran_out = RanOut::InFlight;
                }
                self.clock_reports = ClockReports::Refilling;
                self.countdowns_sent = self.countdowns_sent.wrapping_add(1);
                return Some((
                    Purpose::Clock,
                    "report_host_clock",
                    object(vec![
                        ("elapsed_ms", number(i64::from(spent))),
                        ("remaining_ms", number(i64::from(left))),
                    ]),
                ));
            }
            if self.timing.clock.unreportable() {
                // The wire cannot carry the end; said where the lobby reads the rig's errors.
                self.ran_out = RanOut::Reported;
            }
        }
        if self.wants.end_lull
            && self.phase == Phase::Lull
            && matches!(
                self.ran_out,
                RanOut::No | RanOut::Reported | RanOut::GivenUp
            )
        {
            return Some((Purpose::EndLull, "end_lull", object(Vec::new())));
        }
        if self.wants.end_recap && self.phase == Phase::Recap {
            return Some((Purpose::EndRecap, "end_recap", object(Vec::new())));
        }
        if self.phase == Phase::Push
            && let Some(ask) = self.timing.pacer.next_ask()
        {
            return Some((
                Purpose::Advance,
                "advance_push",
                object(vec![("ms", number(i64::from(ask)))]),
            ));
        }
        if self.ran_out == RanOut::No
            && let Some((spent, countdown)) = self.timing.clock.next_report()
        {
            self.clock_reports = match self.clock_reports {
                ClockReports::Unsent => ClockReports::FirstOut,
                ClockReports::FirstOut | ClockReports::Refilling => ClockReports::Refilling,
            };
            // No countdown is left out rather than sent as 0, which the gateway would show as
            // a timer that ran out (S1-11).
            let mut params = vec![("elapsed_ms", number(i64::from(spent)))];
            if let Some(countdown) = countdown {
                params.push(("remaining_ms", number(i64::from(countdown))));
                self.countdowns_sent = self.countdowns_sent.wrapping_add(1);
            }
            return Some((Purpose::Clock, "report_host_clock", object(params)));
        }
        if self.timing.clock.unreportable() && self.last_error.is_none() {
            // Said once, where the lobby reads the rig's errors: the gateway's clock stops
            // here rather than being reported at a bound it would read as time not passing.
            self.last_error = Some(
                "this phase has run longer than report_host_clock can carry (i32::MAX ms), \
                 so the host clock is no longer reported"
                    .to_owned(),
            );
        }
        if self.timing.keepalive_due(ADMIN) {
            return Some((Purpose::Status, "get_status", object(Vec::new())));
        }
        None
    }

    fn seat_next(&mut self) -> Option<(Purpose, &'static str, Json)> {
        if !self.idle(SEAT) || self.seat_calls_left == 0 {
            return None;
        }
        // A refused status read is asked again once the link has been quiet for the keep-alive
        // interval: spaced by the existing scheduling constant, never back to back.
        if self.status_retry_owed && self.timing.keepalive_due(SEAT) {
            self.status_retry_owed = false;
            self.reads.status = true;
        }
        if let Some(cursor) = self.view_paging.clone() {
            return Some((Purpose::View, "get_view", cursor_params(&cursor)));
        }
        if self.due.keyframe && self.timing.now_us() >= self.keyframe_not_before_us {
            return Some((Purpose::View, "get_view", cursor_params("")));
        }
        // Ready waits behind the seat's owed `get_status` as it waits behind the editor's
        // calls, so a seal the gateway accepted is told in the Lull it was sealed in (S3's
        // `prob`).
        if self.wants.ready
            && self.phase == Phase::Lull
            && !self.editor.has_pending_jobs()
            && !self.reads.status
        {
            self.ready_sent = true;
            self.sync_door();
            return Some((
                Purpose::Ready,
                "set_ready",
                object(vec![("ready", Json::Bool(true))]),
            ));
        }
        // Planning is open in a Lull and closed once this seat is ready or the Lull is
        // ending: a call sent into the end of a Lull would come back PHASE_CLOSED.
        if self.phase == Phase::Lull && !self.ready_sent && !self.wants.end_lull && !self.all_ready
        {
            let full_due = self.timing.idle.due();
            if let Some(call) = self.editor.next_call(full_due) {
                if call.full {
                    self.timing.idle.clear();
                }
                return Some((Purpose::Plan, call.method, call.params));
            }
        }
        // The meter goes ahead of the view and the feed: in a Push all three fall due at
        // every advance, and a debug host can take longer than the pacer's period over a
        // view page, so a meter behind them would never be asked. It is one small call.
        if self.reads.status {
            return Some((Purpose::Status, "get_status", object(Vec::new())));
        }
        if self.reads.meter {
            return Some((Purpose::Meter, "get_economy_forecast", object(Vec::new())));
        }
        if self.reads.recap {
            return Some((Purpose::Recap, "get_recap", object(Vec::new())));
        }
        if self.due.view {
            let cursor = self.view_cursor.clone();
            return Some((Purpose::View, "get_view", cursor_params(&cursor)));
        }
        if self.due.feed {
            let cursor = self.feed_cursor.clone();
            return Some((Purpose::Feed, "get_segment_feed", cursor_params(&cursor)));
        }
        if self.timing.keepalive_due(SEAT) {
            return Some((Purpose::Status, "get_status", object(Vec::new())));
        }
        None
    }

    /// Records the request as in flight and renders it.
    fn send(
        &mut self,
        link: usize,
        purpose: Purpose,
        method: &str,
        params: Json,
    ) -> Option<Outgoing> {
        let state = self.links.get_mut(link)?;
        state.next_id = state.next_id.saturating_add(1);
        let id = state.next_id;
        let text = frame_text(id, method, params);
        // A frame past the gateway's message cap would close the connection (1009): it is not
        // sent, and the call is settled as refused where its refusal is shown. A transport
        // limit, read from the one number both sides read (S3's `fixq-c`, fix pass 1).
        if text.len() > pharmakos_proto::transport::MAX_MESSAGE_BYTES {
            state.unsent = state.unsent.saturating_add(1);
            if purpose == Purpose::Plan {
                self.editor.unsendable(text.len());
                self.sync_editor();
            } else {
                self.refused(purpose, "TOO_LARGE_TO_SEND");
            }
            return None;
        }
        state.in_flight = Some((id, purpose));
        state.calls = state.calls.saturating_add(1);
        if link == SEAT {
            self.seat_calls_left = self.seat_calls_left.saturating_sub(1);
        }
        self.timing.sent(link);
        match purpose {
            Purpose::View => self.due.view = false,
            Purpose::Feed => self.due.feed = false,
            Purpose::Meter => self.reads.meter = false,
            Purpose::Recap => self.reads.recap = false,
            Purpose::Status if link == SEAT => self.reads.status = false,
            _ => {}
        }
        Some(Outgoing { link, text })
    }

    /// Reads one answer from connection `link`.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Rpc`] for a frame that is not a JSON-RPC answer at all (an `error`
    /// with no code or no message included) or a field that is not what its type says,
    /// and [`BridgeError::Schema`] for a result or footer the schema refuses. A refusal by the
    /// gateway is not an error here: it is an [`Answer`] whose `error` says what it was.
    pub fn receive(&mut self, link: usize, text: &str) -> Result<Answer, BridgeError> {
        let frame = json::read(text)?;
        let id = frame
            .get("id")
            .and_then(json_integer)
            .ok_or_else(|| BridgeError::Rpc("an answer with no numeric id".to_owned()))?;
        let Some(state) = self.links.get_mut(link) else {
            return Err(BridgeError::Rpc(format!("there is no connection {link}")));
        };
        let purpose = match state.in_flight {
            Some((waiting, purpose)) if waiting == id => {
                state.in_flight = None;
                purpose
            }
            // From before a reconnect: nobody is waiting for it.
            _ => return Ok(Answer::default()),
        };

        let mut answer = Answer::default();
        if let Some(error) = frame.get("error") {
            let Some((code, message)) = refusal_of(error) else {
                // The call is settled either way: nothing is left waiting on it.
                self.refused(purpose, "RPC");
                return Err(BridgeError::Rpc(
                    "a refusal with no code or no message, which JSON-RPC requires of every \
                     error"
                        .to_owned(),
                ));
            };
            if purpose == Purpose::Plan {
                // A refusal is an answer: the editor settles the call and says what it was.
                // Some are the gateway's answer to a question rather than a refused call
                // (an `estimate_route` waypoint that resolves to nothing): the editor draws
                // those, and they are not reported as refusals.
                let settled = self.editor.answered(Err((code.as_str(), message.as_str())));
                self.sync_editor();
                if matches!(settled, Ok(crate::editor::Settled::Answered)) {
                    return Ok(answer);
                }
            }
            self.refused(purpose, &code);
            if purpose == Purpose::Recap {
                // The recap column shows the refusal in place of the prose: the gateway's
                // words, as they came.
                self.recap.refusal = format!("{code}: {message}");
            }
            if purpose == Purpose::Status {
                // The preset stands as the last read gave it; the seal does not (it may have
                // moved since), and the newest read's refusal is said beside them.
                self.status_facts.failed = Some(ReadFailed::Refused(format!("{code}: {message}")));
                if link == SEAT {
                    self.status_read_failed();
                }
            }
            let said = format!("{code}: {message}");
            if purpose == Purpose::Clock
                && self.ran_out == RanOut::GivenUp
                && self.zero_refusal.is_none()
            {
                self.zero_refusal = Some(said.clone());
            }
            self.last_error = Some(said.clone());
            answer.error = Some(said);
            return Ok(answer);
        }
        let Some(result) = frame.get("result") else {
            // The call is settled either way: nothing is left waiting on it.
            self.refused(purpose, "RPC");
            return Err(BridgeError::Rpc(
                "an answer with neither result nor error".to_owned(),
            ));
        };
        // What the call was for is settled BEFORE the footer is read, so a footer this
        // build cannot read never leaves the pacer or the host clock waiting on an answer
        // that has already arrived.
        let settled = self.settle(link, purpose, result, &mut answer);
        self.sync_editor();
        settled?;
        if link == ADMIN
            && let (_, Some(footer)) = split_footer(result)
        {
            answer.phase_changed = self.observe(&read_status(&footer)?)?;
        }
        Ok(answer)
    }

    /// Routes one successful answer by what its call was for.
    fn settle(
        &mut self,
        link: usize,
        purpose: Purpose,
        result: &Json,
        answer: &mut Answer,
    ) -> Result<(), BridgeError> {
        match purpose {
            Purpose::View => {
                let complete = matches!(result.get("complete"), Some(Json::Bool(true)));
                let next = proto3_text(result, "next_cursor")?.to_owned();
                if complete {
                    self.due.keyframe = false;
                    self.view_paging = None;
                    self.view_cursor = next;
                } else {
                    self.view_paging = Some(next);
                }
                answer.view = Some(result.clone());
            }
            Purpose::Feed => {
                let (body, _) = split_footer(result);
                let feed: GetSegmentFeedResponse = json::decode_json(&enums::canonical(
                    "gp.api.v1.GetSegmentFeedResponse",
                    &body,
                ))?;
                answer.events = event_rows(&feed)?;
                // The gateway filed this seat's safe playbook: a line in the seat's own feed
                // (finding J). The editor says so, and that its text was not sealed.
                if let Some(filed) = feed
                    .events
                    .iter()
                    .find(|event| event.kind == SAFE_PLAYBOOK_FILED_KIND)
                {
                    self.editor.safe_playbook_filed(&filed.text);
                }
                if !feed.next_cursor.is_empty() {
                    self.feed_cursor = feed.next_cursor;
                }
                // A page is at most the gateway's budget. In a Push the next advance asks
                // again; outside one nothing would, so a backlog left at the Push's end is
                // read out here, until a page comes back empty.
                if !feed.events.is_empty() && self.phase != Phase::Push {
                    self.due.feed = true;
                }
            }
            Purpose::Advance => {
                let ran = match proto3_integer(result, "advanced_ms").and_then(|ran| {
                    u32::try_from(ran).map_err(|_| {
                        BridgeError::Rpc(format!("advance_push ran {ran} ms, which is no duration"))
                    })
                }) {
                    Ok(ran) => ran,
                    Err(error) => {
                        // The ask is settled either way: carried whole, as a refused one is.
                        self.timing.pacer.refused();
                        return Err(error);
                    }
                };
                self.timing.pacer.answered(ran)?;
                if ran > 0 {
                    self.due.view = true;
                    self.due.feed = true;
                    self.reads.meter = true;
                    // The Push moved the gateway's clock, and with it every token's budget.
                    self.seat_calls_left = SEAT_CALLS_PER_REFILL;
                }
            }
            Purpose::Clock => {
                self.timing.clock.answered();
                self.clock_answers = self.clock_answers.wrapping_add(1);
                if self.ran_out == RanOut::InFlight {
                    self.ran_out = RanOut::Reported;
                }
                // Every report but a phase's first moved the gateway's clock, and with it
                // every token's budget (the module doc's "Not every clock answer moves the
                // clock").
                if self.clock_reports == ClockReports::Refilling {
                    self.seat_calls_left = SEAT_CALLS_PER_REFILL;
                }
                self.clock_reports = ClockReports::Refilling;
                self.all_ready = matches!(result.get("all_ready"), Some(Json::Bool(true)));
                if self.all_ready && self.phase == Phase::Lull {
                    self.wants.end_lull = true;
                }
            }
            Purpose::EndLull => self.wants.end_lull = false,
            Purpose::EndRecap => self.wants.end_recap = false,
            Purpose::Ready => self.wants.ready = false,
            Purpose::Plan => {
                self.editor.answered(Ok(result))?;
            }
            Purpose::Meter => {
                let (body, footer) = split_footer(result);
                let forecast: GetEconomyForecastResponse = json::decode_json(&enums::canonical(
                    "gp.api.v1.GetEconomyForecastResponse",
                    &body,
                ))?;
                // Which phase the numbers are from: the answer's own footer, which says when
                // the gateway served it. It labels the meter and moves nothing (only the
                // admin connection's footers move the rig's phase).
                // Every gateway answer carries the footer, so one without it, or with one
                // this build cannot read, is refused rather than labelled with a guess.
                let served =
                    Phase::of(served_status(footer.as_ref(), "get_economy_forecast")?.phase);
                // The next BMI and the committed spend are left out of an answer that has
                // none (item 135 (2) (g)), and an absent field is not 0: the schema marks
                // the two `optional`, so the decoded type carries their presence.
                self.meter = Meter {
                    answers: self.meter.answers.saturating_add(1),
                    phase: served,
                    treasury_now: forecast.treasury_now,
                    supply_kw_now: forecast.supply_kw_now,
                    draw_kw_now: forecast.draw_kw_now,
                    headroom_kw_now: forecast.headroom_kw_now,
                    bmi_next_dollars: forecast.bmi_next_dollars,
                    committed_dollars: forecast.committed_dollars,
                };
            }
            Purpose::Recap => self.take_recap(result)?,
            Purpose::Status => self.take_status(link, result)?,
        }
        Ok(())
    }

    /// Takes in a `get_status` answer's body: what the gateway hosts, and on the seat
    /// connection what the seat was told of its seal of the round.
    ///
    /// # Errors
    ///
    /// [`BridgeError`] for a body this build cannot read, or a preset number the schema names
    /// none by. The reason is kept as the facts' `failed`, so the lobby says it.
    fn take_status(&mut self, link: usize, result: &Json) -> Result<(), BridgeError> {
        match read_status_facts(result) {
            Ok((preset, sealed)) => {
                self.status_facts.answers = self.status_facts.answers.saturating_add(1);
                self.status_facts.preset = Some(preset);
                if link == SEAT {
                    self.status_facts.sealed = sealed;
                    self.status_retries = 0;
                }
                self.status_facts.failed = None;
                Ok(())
            }
            Err(error) => {
                self.status_facts.failed = Some(ReadFailed::Unreadable(error.to_string()));
                if link == SEAT {
                    self.status_read_failed();
                }
                Err(error)
            }
        }
    }

    /// Takes in a `get_recap` answer: its prose, and the round its own footer names (the
    /// rig's round when the answer carries no footer).
    ///
    /// # Errors
    ///
    /// [`BridgeError`] for an answer, or a footer, this build cannot read.
    fn take_recap(&mut self, result: &Json) -> Result<(), BridgeError> {
        match self.read_recap(result) {
            Ok(recap) => {
                self.recap = recap;
                Ok(())
            }
            Err(error) => {
                // An answer this build cannot read is shown in the column, as a refusal is:
                // left to the log alone, the column would go on showing the last round's.
                self.recap.refusal = error.to_string();
                Err(error)
            }
        }
    }

    /// A `get_recap` answer, read.
    fn read_recap(&self, result: &Json) -> Result<Recap, BridgeError> {
        recap_of(result, self.recap.answers.saturating_add(1))
    }

    /// The bridge could not decode or apply a `get_view` page the rig had already read.
    ///
    /// The cursor the page carried is not trusted: the page is lost, and a view with a hole
    /// in it cannot be told from a map with one, so the seat asks for a keyframe again —
    /// no sooner than [`KEEPALIVE_US`] from now, so a page this build can never read is
    /// not asked for in a loop.
    pub fn view_refused(&mut self) {
        self.view_refusals = self.view_refusals.saturating_add(1);
        self.due.keyframe = true;
        self.view_paging = None;
        self.keyframe_not_before_us = self.timing.now_us().saturating_add(KEEPALIVE_US);
    }

    /// Undoes what a refused call had counted on.
    fn refused(&mut self, purpose: Purpose, code: &str) {
        match purpose {
            Purpose::Advance => self.timing.pacer.refused(),
            Purpose::Clock => {
                self.timing.clock.refused();
                self.clock_reports = ClockReports::Refilling;
                self.zero_refused();
            }
            Purpose::View => {
                // A cursor from before an attach, or any other refusal: start again from
                // a keyframe, which is what the feed asks of a client that lost its place.
                self.due.keyframe = true;
                self.view_paging = None;
            }
            Purpose::Feed => {
                if code == "STALE_SNAPSHOT" {
                    self.feed_cursor.clear();
                }
            }
            Purpose::EndLull => self.wants.end_lull = false,
            Purpose::EndRecap => self.wants.end_recap = false,
            Purpose::Ready => {
                self.wants.ready = false;
                self.ready_sent = false;
                self.sync_door();
            }
            Purpose::Plan | Purpose::Status | Purpose::Meter | Purpose::Recap => {}
        }
    }

    /// Takes in an admin footer. Returns whether the phase moved.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Rpc`] for a timed footer whose countdown is negative, which no gateway
    /// writes: the countdown is game milliseconds left. Such a footer is refused before
    /// anything moves, so the rig stays in the phase it was in and the next good footer
    /// opens the Lull with its clock.
    fn observe(
        &mut self,
        footer: &pharmakos_proto::gp::api::v1::Status,
    ) -> Result<bool, BridgeError> {
        let phase = Phase::of(footer.phase);
        // A Lull untimed by rule has no countdown at all: the rig starts none, reports only the
        // time spent, and the lobby shows none (decisions-log item 160 (4)). Which preset that is stays the
        // gateway's word.
        let countdown = if phase == Phase::Lull && phase != self.phase && !footer.untimed_by_rule {
            self.lull_countdown(footer)?
        } else {
            None
        };
        self.round = footer.round;
        self.untimed = if footer.untimed_by_rule {
            Untimed::ByRule
        } else if footer.untimed {
            Untimed::NotReported
        } else {
            Untimed::Timed
        };
        if phase == self.phase {
            return Ok(false);
        }
        self.phase = phase;
        // The status reads' retries are a round's phase's: a phase spent them, the next asks
        // afresh (fix pass 2).
        self.status_retries = 0;
        self.status_retry_owed = false;
        if phase == Phase::Lull {
            self.ready_sent = false;
        }
        self.sync_door();
        // The new phase's clock starts from zero at the gateway, so its first report moves
        // no tick; the change itself moved none either, so nothing is refilled.
        self.clock_reports = ClockReports::Unsent;
        self.wants.end_lull = false;
        self.wants.end_recap = false;
        self.ran_out = RanOut::No;
        if phase == Phase::Lull {
            // A given-up zero is said until the next Lull opens; the count starts again there.
            self.zero_refusals = 0;
            self.zero_refusal = None;
        }
        self.all_ready = false;
        // Something about the view may have changed at the boundary: the unlock at match
        // end is a policy change, and a new segment makes the feed cursor stale.
        self.due.view = true;
        self.due.feed = true;
        match phase {
            Phase::Push => {
                self.feed_cursor.clear();
                self.timing.pacer.start();
                self.timing.clock.enter(ClockPhase::Silent);
            }
            Phase::Lull => {
                self.reads.meter = true;
                // The seat's seal is the new round's: asked, never carried (S3's `prob`), and the
                // old word goes at once (fix pass 1).
                self.forget_seal();
                self.editor.lull_opened(footer.round);
                self.timing.pacer.stop();
                self.timing.clock.enter(ClockPhase::Lull {
                    countdown_ms: countdown,
                });
            }
            Phase::Recap | Phase::Ended => {
                self.timing.pacer.stop();
                self.timing.clock.enter(ClockPhase::Open);
                // The recap's lines; at match end, the last recap with the outcome. A
                // refusal belongs to the read it answered, so the new read starts clean.
                self.reads.recap = true;
                self.recap.refusal.clear();
            }
            Phase::Unknown | Phase::Lobby => {
                self.timing.pacer.stop();
                self.timing.clock.enter(ClockPhase::Silent);
            }
        }
        Ok(true)
    }

    /// The countdown a Lull opened by `footer` starts from.
    ///
    /// A timed footer shows a countdown this client reported before a reconnect, and the
    /// Lull goes on from it; an untimed one has had no countdown reported yet, and the Lull
    /// is as long as the rules say for its round (`first_lull_ms` for round 1, `lull_ms`
    /// after it).
    ///
    /// PLACEHOLDER: a resumed Lull's timer restarts — OWNER, at hardening. The timer is the
    /// client's (w6 notes A2, decisions-log item 99) and a new host has had no countdown
    /// reported, so a resumed Lull restarts its timer in full and quitting in a Lull buys
    /// planning time.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Rpc`] for a timed footer whose countdown is negative.
    fn lull_countdown(
        &self,
        footer: &pharmakos_proto::gp::api::v1::Status,
    ) -> Result<Option<u64>, BridgeError> {
        if footer.untimed {
            return Ok(self.lulls.for_round(footer.round));
        }
        u64::try_from(footer.phase_remaining_ms)
            .map(Some)
            .map_err(|_| {
                BridgeError::Rpc(format!(
                    "a timed Lull's footer counts down {} ms, and a countdown is never \
                     negative",
                    footer.phase_remaining_ms
                ))
            })
    }

    /// Whether the editor's planning calls can go out, from the phase the admin connection
    /// last heard and this seat's Ready: the door the rig's own gate in `seat_next` keeps,
    /// told to the editor so nothing waits behind it (finding K). Before any phase is known
    /// the door is open, and the editor's calls wait for the Lull.
    #[must_use]
    pub const fn door(&self) -> Door {
        match self.phase {
            Phase::Lull if self.ready_sent => Door::Ready,
            Phase::Unknown | Phase::Lobby | Phase::Lull => Door::Open,
            Phase::Push => Door::Push,
            Phase::Recap => Door::Recap,
            Phase::Ended => Door::Ended,
        }
    }

    fn sync_door(&mut self) {
        let door = self.door();
        self.editor.set_door(door);
    }

    /// Chooses a speed; refused when it is not one of [`crate::pacer::SPEEDS`].
    pub fn set_speed(&mut self, speed: u32) -> bool {
        self.timing.pacer.set_speed(speed)
    }

    /// Skip to the end of the segment. Only in a Push.
    pub fn skip(&mut self) {
        self.timing.pacer.skip();
    }

    /// The human pressed Ready.
    pub fn ready(&mut self) {
        if self.phase == Phase::Lull {
            self.wants.ready = true;
        }
    }

    /// End the Lull now: what the lobby does when every seat is ready or the timer runs
    /// out, and what the headless watch check does after its idle stretch.
    pub fn end_lull(&mut self) {
        if self.phase == Phase::Lull {
            self.wants.end_lull = true;
        }
    }

    /// Leave the recap for the next Lull.
    pub fn end_recap(&mut self) {
        if self.phase == Phase::Recap {
            self.wants.end_recap = true;
        }
    }

    /// The phase, as the admin connection last heard it.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// The round, as the admin connection last heard it.
    #[must_use]
    pub const fn round(&self) -> u32 {
        self.round
    }

    /// The pacer's speed.
    #[must_use]
    pub const fn speed(&self) -> u32 {
        self.timing.pacer.speed()
    }

    /// Whether a skip is under way.
    #[must_use]
    pub const fn skipping(&self) -> bool {
        self.timing.pacer.skipping()
    }

    /// Whether the last host-clock answer said every seat is ready.
    #[must_use]
    pub const fn all_ready(&self) -> bool {
        self.all_ready
    }

    /// The Lull timer as the lobby shows it: empty outside a timed Lull, and empty while the
    /// admin connection's latest footer says the phase has no countdown (`_status.untimed`;
    /// S1-11's client half, decisions-log item 135). The client draws what the footer says,
    /// which is no countdown until the gateway has been told one.
    #[must_use]
    pub fn timer_text(&self) -> String {
        if self.untimed != Untimed::Timed {
            return String::new();
        }
        self.timing.clock.timer_text()
    }

    /// Whether the admin connection's latest footer said the phase has no countdown.
    #[must_use]
    pub fn untimed(&self) -> bool {
        self.untimed != Untimed::Timed
    }

    /// Whether the admin connection's latest footer said the Lull is untimed by rule
    /// (`_status.untimed_by_rule`).
    #[must_use]
    pub fn untimed_by_rule(&self) -> bool {
        self.untimed == Untimed::ByRule
    }

    /// How many `report_host_clock` calls the gateway answered.
    #[must_use]
    pub const fn clock_answers(&self) -> u64 {
        self.clock_answers
    }

    /// How many `report_host_clock` calls carried a countdown (`remaining_ms`).
    #[must_use]
    pub const fn countdowns_sent(&self) -> u64 {
        self.countdowns_sent
    }

    /// The recap's lines, as the gateway last wrote them.
    #[must_use]
    pub const fn recap(&self) -> &Recap {
        &self.recap
    }

    /// How many times each connection dropped while open: [`ADMIN`]'s, then [`SEAT`]'s.
    #[must_use]
    pub const fn drops(&self) -> [u32; 2] {
        let [admin, seat] = &self.links;
        [admin.drops, seat.drops]
    }

    /// How many of each connection's drops either side closed on the message in flight
    /// ([`Close::ThisMessage`]), and how many calls were not sent for being past the message cap:
    /// `[[admin, seat] closes on a message, [admin, seat] unsent]`.
    #[must_use]
    pub const fn message_refusals(&self) -> [[u32; 2]; 2] {
        let [admin, seat] = &self.links;
        [
            [admin.message_drops, seat.message_drops],
            [admin.unsent, seat.unsent],
        ]
    }

    /// How many calls the transport refused to send on each connection: [`ADMIN`]'s, then
    /// [`SEAT`]'s.
    #[must_use]
    pub const fn send_failures(&self) -> [u32; 2] {
        let [admin, seat] = &self.links;
        [admin.send_failures, seat.send_failures]
    }

    /// How many calls each connection has sent: [`ADMIN`]'s, then [`SEAT`]'s.
    #[must_use]
    pub const fn calls(&self) -> [u64; 2] {
        let [admin, seat] = &self.links;
        [admin.calls, seat.calls]
    }

    /// The last refusal the gateway answered with, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// The last close on a message itself, if any (fix pass 3).
    #[must_use]
    pub const fn closed_on_message(&self) -> Option<ClosedOnMessage> {
        self.closed_on
    }

    /// Whether connection `link` is open now.
    #[must_use]
    pub fn link_open(&self, link: usize) -> bool {
        self.links.get(link).is_some_and(|state| state.open)
    }

    /// Whether the seat's view is complete and current: no keyframe or page waiting.
    #[must_use]
    pub const fn view_settled(&self) -> bool {
        !self.due.keyframe && self.view_paging.is_none()
    }

    /// Whether the seat connection has caught up: its view settled, no view or feed poll
    /// owed, and nothing in flight.
    #[must_use]
    pub fn seat_settled(&self) -> bool {
        self.view_settled() && !self.due.view && !self.due.feed && self.idle(SEAT)
    }

    /// How many `get_view` pages the bridge refused after the gateway served them.
    #[must_use]
    pub const fn view_refusals(&self) -> u32 {
        self.view_refusals
    }

    /// The own `$`/`kW` meter, as the gateway last answered it.
    #[must_use]
    pub const fn meter(&self) -> Meter {
        self.meter
    }

    /// What `get_status`'s body last said beside its footer.
    #[must_use]
    pub const fn status_facts(&self) -> &StatusFacts {
        &self.status_facts
    }

    /// Whether this seat holds a seal of the current round, as the gateway told it in this
    /// round and this phase; `None` ("not told") once the rig's round or phase has moved past the
    /// answer, until the next one comes (asked at the next Lull's opening).
    #[must_use]
    pub fn sealed_this_round(&self) -> Option<bool> {
        self.status_facts
            .sealed
            .filter(|fact| fact.round == self.round && fact.phase == self.phase)
            .map(|fact| fact.sealed)
    }

    /// The lobby wrote `preset` on a new match's config line: the gateway's answer is held to
    /// it ([`Rig::preset_check`]).
    pub fn expect_preset(&mut self, preset: MatchPreset) {
        self.asked_preset = Some(preset);
    }

    /// Whether the hosted match is the preset the lobby asked for, by the gateway's last
    /// readable word on it.
    #[must_use]
    pub fn preset_check(&self) -> PresetCheck {
        match (self.status_facts.preset, self.asked_preset) {
            (None, _) => PresetCheck::NotRead,
            (Some(ServedPreset::NoMatch), _) => PresetCheck::NoMatch,
            (Some(ServedPreset::Hosted(served)), None) => PresetCheck::Unasked(served),
            (Some(ServedPreset::Hosted(served)), Some(asked)) if served == asked => {
                PresetCheck::Agrees(served)
            }
            (Some(ServedPreset::Hosted(served)), Some(asked)) => {
                PresetCheck::Disagrees { asked, served }
            }
        }
    }
}

/// A `get_recap` answer read as the `answers`-th the recap has taken: its prose, every seat's
/// tally, the Ledger's memos, and the round its own footer names.
///
/// # Errors
///
/// [`BridgeError`] for an answer, or a footer, this build cannot read.
fn recap_of(result: &Json, answers: u64) -> Result<Recap, BridgeError> {
    let (body, footer) = split_footer(result);
    let recap: GetRecapResponse =
        json::decode_json(&enums::canonical("gp.api.v1.GetRecapResponse", &body))?;
    let round = served_status(footer.as_ref(), "get_recap")?.round;
    // A seat with no tally is not a seat that destroyed nothing: the gateway writes every
    // field, so one without it is refused rather than drawn as zeros.
    let seat_tallies = recap
        .seat_tallies
        .into_iter()
        .map(|one| {
            let tally = one.tally.ok_or_else(|| {
                BridgeError::Rpc(format!("`seat_tallies` names {} with no tally", one.seat))
            })?;
            Ok(SeatTallyRow {
                seat: one.seat,
                kills: tally.kills,
                shared_kills: tally.shared_kills,
                losses: tally.losses,
                value_destroyed_dollars: tally.value_destroyed_dollars,
            })
        })
        .collect::<Result<Vec<_>, BridgeError>>()?;
    let memos = recap
        .ledger_memos
        .into_iter()
        .map(memo_row)
        .collect::<Result<Vec<_>, BridgeError>>()?;
    Ok(Recap {
        answers,
        round,
        prose: recap.prose,
        seat_tallies,
        memos,
        refusal: String::new(),
    })
}

/// A `get_recap` answer's text read with no gateway behind it (a whole JSON-RPC answer, or
/// its result): what the hostless lobby shot draws, read as the rig reads a recap.
///
/// # Errors
///
/// [`BridgeError`] for a text this build cannot read.
pub fn recap_of_text(text: &str) -> Result<Recap, BridgeError> {
    let frame = json::read(text)?;
    let result = frame
        .get("result")
        .cloned()
        .ok_or_else(|| BridgeError::Rpc("a committed answer with no result".to_owned()))?;
    recap_of(&result, 1)
}

/// A `get_segment_feed` answer's text read with no gateway behind it (a whole JSON-RPC answer,
/// or its result): its event-list rows, as the rig reads a feed page.
///
/// # Errors
///
/// [`BridgeError`] for a text this build cannot read.
pub fn feed_rows_of_text(text: &str) -> Result<Vec<EventRow>, BridgeError> {
    let frame = json::read(text)?;
    let result = frame
        .get("result")
        .cloned()
        .ok_or_else(|| BridgeError::Rpc("a committed answer with no result".to_owned()))?;
    let (body, _) = split_footer(&result);
    let feed: GetSegmentFeedResponse =
        json::decode_json(&enums::canonical("gp.api.v1.GetSegmentFeedResponse", &body))?;
    event_rows(&feed)
}

/// A `get_status` result read: what the gateway hosts, and `sealed_this_round` as the
/// answer told it, with the round and phase its footer names (`None` when it did not tell it:
/// an admin's token is no seat's and is told none).
///
/// # Errors
///
/// [`BridgeError::Schema`] for a body or footer the schema refuses, and [`BridgeError::Rpc`]
/// for a preset number the schema names none by, or a told seal with no footer to say when.
pub(crate) fn read_status_facts(
    result: &Json,
) -> Result<(ServedPreset, Option<SealFact>), BridgeError> {
    let (body, footer) = split_footer(result);
    let status: GetStatusResponse =
        json::decode_json(&enums::canonical("gp.api.v1.GetStatusResponse", &body))?;
    let preset = ServedPreset::of_wire(status.preset).ok_or_else(|| {
        BridgeError::Rpc(format!(
            "a get_status answer names the preset {}, which this build has no name for",
            status.preset
        ))
    })?;
    let sealed = match status.sealed_this_round {
        None => None,
        Some(sealed) => {
            let served = served_status(footer.as_ref(), "get_status")?;
            Some(SealFact {
                round: served.round,
                phase: Phase::of(served.phase),
                sealed,
            })
        }
    };
    Ok((preset, sealed))
}

/// One of the recap's memos, read.
///
/// # Errors
///
/// [`BridgeError::Rpc`] for a memo that names no memo (the unspecified zero), which no
/// gateway issues.
fn memo_row(memo: pharmakos_proto::gp::api::v1::LedgerMemo) -> Result<MemoRow, BridgeError> {
    let id = memo_id(memo.id)?;
    Ok(MemoRow {
        id,
        text: memo.text,
    })
}

/// A memo's id as the wire spells it: the schema's name, in lower case (decisions-log item
/// 80).
fn memo_id(wire: i32) -> Result<String, BridgeError> {
    match ledger_memo::Id::try_from(wire) {
        Ok(ledger_memo::Id::Unspecified) | Err(_) => Err(BridgeError::Rpc(format!(
            "a Ledger memo with the id {wire}, which names no memo"
        ))),
        Ok(id) => Ok(id.as_str_name().to_ascii_lowercase()),
    }
}

/// The event list's rows for one `get_segment_feed` page, in the feed's order.
///
/// Each row is the event's time in the segment, its kind and its text, exactly as the
/// gateway wrote them. Nothing is added, merged, reworded or filtered: the event list shows
/// what the feed said, which is what its golden
/// (`tests/golden/vista/expected.events.txt`) holds the client to. An event whose typed
/// payload is one of the Ledger's memos is the Ledger's row ([`Speaker::Ledger`]) carrying
/// the memo's own text: the branch is on the payload the schema types, never on the kind's
/// spelling, so no feed kind is named here (S3's plan, E5).
///
/// # Errors
///
/// [`BridgeError::Rpc`] for an event at a negative time, which no gateway writes: the time
/// is game milliseconds into the segment. The page is refused whole rather than shown with
/// a row at a time it does not have.
pub fn event_rows(feed: &GetSegmentFeedResponse) -> Result<Vec<EventRow>, BridgeError> {
    feed.events
        .iter()
        .map(|event| {
            let at_ms = u64::try_from(event.at_ms).map_err(|_| {
                BridgeError::Rpc(format!("an event at {} ms into the segment", event.at_ms))
            })?;
            let (speaker, text) = match &event.payload {
                Some(event::Payload::LedgerMemo(memo)) => (Speaker::Ledger, memo.text.clone()),
                None => (Speaker::Event, event.text.clone()),
            };
            Ok(EventRow {
                clock: clock_text(at_ms),
                kind: event.kind.clone(),
                text,
                speaker,
            })
        })
        .collect()
}

/// A JSON-RPC `error` member's code and message.
///
/// The code is the gateway's name for the refusal, `data.code`; an error from anything
/// that does not name one is named by the JSON-RPC number the standard requires of every
/// error. `None` when the member carries no code or no message, which the standard
/// requires too.
fn refusal_of(error: &Json) -> Option<(String, String)> {
    let named = error
        .get("data")
        .and_then(|data| data.get("code"))
        .and_then(json_text)
        .map(str::to_owned);
    let code = match named {
        Some(code) => code,
        None => error.get("code").and_then(json_integer)?.to_string(),
    };
    let message = error.get("message").and_then(json_text)?.to_owned();
    Some((code, message))
}

/// The string field `name` of an answer: proto3's canonical JSON leaves an empty string
/// out, so an absent field is `""`, and anything but a string is refused.
pub(crate) fn proto3_text<'a>(answer: &'a Json, name: &str) -> Result<&'a str, BridgeError> {
    match answer.get(name) {
        None => Ok(""),
        Some(value) => {
            json_text(value).ok_or_else(|| BridgeError::Rpc(format!("`{name}` is not a string")))
        }
    }
}

/// The integer field `name` of an answer: proto3's canonical JSON leaves a zero out, so an
/// absent field is 0, and anything but an integer is refused.
fn object(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn number(value: i64) -> Json {
    Json::Number(value.to_string())
}

/// The `_status` footer of an answer from `method`, read.
///
/// # Errors
///
/// [`BridgeError::Rpc`] when the answer carries none, which every gateway answer does (this
/// module's doc, "Whose footer is believed"), and [`BridgeError::Schema`] for a footer this
/// build cannot read.
fn served_status(footer: Option<&Json>, method: &str) -> Result<Status, BridgeError> {
    let footer = footer.ok_or_else(|| {
        BridgeError::Rpc(format!("a {method} answer came with no _status footer"))
    })?;
    read_status(footer)
}

fn cursor_params(cursor: &str) -> Json {
    if cursor.is_empty() {
        object(Vec::new())
    } else {
        object(vec![("cursor", Json::String(cursor.to_owned()))])
    }
}

fn json_integer(value: &Json) -> Option<i64> {
    match value {
        Json::Number(lexeme) => lexeme.parse::<i64>().ok(),
        _ => None,
    }
}

fn json_text(value: &Json) -> Option<&str> {
    match value {
        Json::String(text) => Some(text.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::pacer::PACER_PERIOD_US;

    fn answer(id: i64, result: &str, phase: &str) -> String {
        let body = result.strip_suffix('}').expect("a result is an object");
        let sep = if body.len() > 1 { "," } else { "" };
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{body}{sep}"_status":{{"phase":"{phase}","phase_remaining_ms":180000,"round":1}}}}}}"#
        )
    }

    fn id_of(frame: &Outgoing) -> i64 {
        json::read(&frame.text)
            .expect("json")
            .get("id")
            .and_then(json_integer)
            .expect("an id")
    }

    fn method_of(frame: &Outgoing) -> String {
        json::read(&frame.text)
            .expect("json")
            .get("method")
            .and_then(json_text)
            .expect("a method")
            .to_owned()
    }

    /// A rig with both connections open, the phase learnt, and the keyframe complete.
    fn in_lull() -> Rig {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        rig.opened(SEAT);
        let sent = rig.poll(0);
        assert_eq!(sent.len(), 2);
        let status = sent.first().expect("admin first");
        assert_eq!(method_of(status), "get_status");
        let view = sent.get(1).expect("seat second");
        assert_eq!(method_of(view), "get_view");
        let got = rig
            .receive(ADMIN, &answer(id_of(status), "{}", "lull"))
            .expect("reads");
        assert!(got.phase_changed);
        assert_eq!(rig.phase(), Phase::Lull);
        let got = rig
            .receive(
                SEAT,
                &answer(
                    id_of(view),
                    r#"{"next_cursor":"c1","complete":true}"#,
                    "lull",
                ),
            )
            .expect("reads");
        assert!(got.view.is_some());
        assert!(rig.view_settled());
        settle(&mut rig, 0);
        rig
    }

    /// Answers every call the rig makes at `now` with an unremarkable result, until it
    /// has nothing left to send.
    ///
    /// Time passes between two rounds of answers as far as the gateway's budget is
    /// concerned: the seat's budget is refilled each round, as a clock report would.
    fn settle(rig: &mut Rig, now: u64) {
        for _ in 0..16 {
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            let sent = rig.poll(now);
            if sent.is_empty() {
                return;
            }
            for frame in sent {
                let result = match method_of(&frame).as_str() {
                    "get_view" => r#"{"next_cursor":"c9","complete":true}"#,
                    "get_segment_feed" => r#"{"events":[],"next_cursor":"f1"}"#,
                    "report_host_clock" => r#"{"all_ready":false}"#,
                    _ => "{}",
                };
                let phase = rig.phase().name();
                rig.receive(frame.link, &answer(id_of(&frame), result, phase))
                    .expect("reads");
            }
        }
        panic!("the rig never went quiet");
    }

    #[test]
    fn a_lull_reports_the_clock_and_ends_when_every_seat_is_ready() {
        let mut rig = in_lull();
        let sent = rig.poll(CLOCK_REPORT_US);
        let clock = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an admin call");
        assert_eq!(method_of(clock), "report_host_clock");
        assert!(clock.text.contains("\"remaining_ms\""), "{}", clock.text);
        rig.receive(
            ADMIN,
            &answer(id_of(clock), r#"{"all_ready":true}"#, "lull"),
        )
        .expect("reads");
        let sent = rig.poll(CLOCK_REPORT_US + 1);
        let end = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an admin call");
        assert_eq!(method_of(end), "end_lull");
    }

    #[test]
    fn a_push_is_paced_and_the_seat_follows_each_advance() {
        let mut rig = in_lull();
        rig.end_lull();
        let sent = rig.poll(1);
        let end = sent.first().expect("end_lull");
        assert_eq!(method_of(end), "end_lull");
        rig.receive(ADMIN, &answer(id_of(end), "{}", "push"))
            .expect("reads");
        assert_eq!(rig.phase(), Phase::Push);
        assert!(rig.set_speed(4));
        settle(&mut rig, 1);

        let sent = rig.poll(1 + PACER_PERIOD_US);
        let advance = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an advance");
        assert_eq!(method_of(advance), "advance_push");
        assert!(advance.text.contains("400"), "{}", advance.text);
        rig.receive(
            ADMIN,
            &answer(id_of(advance), r#"{"advanced_ms":400}"#, "push"),
        )
        .expect("reads");
        // The meter first (one small call), then the view.
        let sent = rig.poll(2 + PACER_PERIOD_US);
        let meter = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a seat call");
        assert_eq!(method_of(meter), "get_economy_forecast");
        rig.receive(SEAT, &answer(id_of(meter), "{}", "push"))
            .expect("reads");
        let sent = rig.poll(3 + PACER_PERIOD_US);
        assert!(
            sent.iter()
                .any(|frame| frame.link == SEAT && method_of(frame) == "get_view"),
            "the seat looks again after the match moved: {sent:?}"
        );
    }

    #[test]
    fn a_seat_answer_never_moves_the_phase() {
        let mut rig = in_lull();
        rig.due.view = true;
        let sent = rig.poll(1);
        let view = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a view poll");
        let got = rig
            .receive(
                SEAT,
                &answer(
                    id_of(view),
                    r#"{"next_cursor":"c2","complete":true}"#,
                    "push",
                ),
            )
            .expect("reads");
        assert!(!got.phase_changed);
        assert_eq!(rig.phase(), Phase::Lull);
    }

    #[test]
    fn a_stale_cursor_asks_for_a_keyframe_again() {
        let mut rig = in_lull();
        rig.due.view = true;
        let sent = rig.poll(1);
        let view = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a view poll");
        let refusal = format!(
            r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":-32000,"message":"stale","data":{{"code":"STALE_SNAPSHOT"}}}}}}"#,
            id_of(view)
        );
        let got = rig.receive(SEAT, &refusal).expect("reads");
        assert!(got.error.is_some());
        let sent = rig.poll(2);
        let again = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a keyframe");
        assert!(again.text.contains("\"params\": {}"), "{}", again.text);
    }

    #[test]
    fn a_quiet_connection_sends_a_keepalive_and_a_drop_is_counted() {
        let mut rig = in_lull();
        let sent = rig.poll(KEEPALIVE_US + 10);
        assert!(
            sent.iter()
                .any(|frame| frame.link == SEAT && method_of(frame) == "get_status"),
            "{sent:?}"
        );
        rig.dropped(SEAT);
        let [_, seat_drops] = rig.drops();
        assert_eq!(seat_drops, 1);
        rig.opened(SEAT);
        assert!(!rig.view_settled(), "a reconnect starts from a keyframe");
    }

    /// A rig in a Push at 4x, with nothing owed yet.
    fn in_push() -> Rig {
        let mut rig = in_lull();
        rig.end_lull();
        let sent = rig.poll(1);
        let end = sent.first().expect("end_lull");
        rig.receive(ADMIN, &answer(id_of(end), "{}", "push"))
            .expect("reads");
        assert!(rig.set_speed(4));
        settle(&mut rig, 1);
        rig
    }

    #[test]
    fn the_host_clock_is_reported_in_a_recap_and_after_the_match_ends() {
        for phase in ["recap", "ended"] {
            let mut rig = in_push();
            let sent = rig.poll(1 + PACER_PERIOD_US);
            let advance = sent
                .iter()
                .find(|frame| frame.link == ADMIN)
                .expect("an advance");
            rig.receive(
                ADMIN,
                &answer(id_of(advance), r#"{"advanced_ms":400}"#, phase),
            )
            .expect("reads");
            assert_eq!(rig.phase().name(), phase);
            let sent = rig.poll(2 + PACER_PERIOD_US);
            let clock = sent
                .iter()
                .find(|frame| frame.link == ADMIN)
                .expect("an admin call");
            assert_eq!(method_of(clock), "report_host_clock", "{phase}");
            assert!(
                !clock.text.contains("remaining_ms"),
                "{phase} has no countdown, so the report carries none (S1-11): {}",
                clock.text
            );
            rig.receive(ADMIN, &answer(id_of(clock), "{}", phase))
                .expect("reads");
            let sent = rig.poll(2 + PACER_PERIOD_US + CLOCK_REPORT_US);
            assert!(
                sent.iter()
                    .any(|frame| frame.link == ADMIN && method_of(frame) == "report_host_clock"),
                "{phase}: reported again at the cadence: {sent:?}"
            );
        }
    }

    #[test]
    fn an_unreadable_footer_leaves_nothing_waiting() {
        let mut rig = in_push();
        let sent = rig.poll(1 + PACER_PERIOD_US);
        let advance = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an advance");
        let broken = format!(
            r#"{{"jsonrpc":"2.0","id":{},"result":{{"advanced_ms":400,"_status":{{"phase":"push","not_a_field":1}}}}}}"#,
            id_of(advance)
        );
        rig.receive(ADMIN, &broken)
            .expect_err("the footer is not a Status");
        let sent = rig.poll(1 + 2 * PACER_PERIOD_US);
        assert!(
            sent.iter()
                .any(|frame| frame.link == ADMIN && method_of(frame) == "advance_push"),
            "the pacer asks again: {sent:?}"
        );
    }

    #[test]
    fn an_unknown_phase_is_asked_about_at_the_clocks_cadence() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        let sent = rig.poll(0);
        let status = sent.first().expect("the first get_status");
        let no_phase = format!(
            r#"{{"jsonrpc":"2.0","id":{},"result":{{"_status":{{"round":1}}}}}}"#,
            id_of(status)
        );
        rig.receive(ADMIN, &no_phase).expect("reads");
        assert_eq!(rig.phase(), Phase::Unknown);
        assert!(rig.poll(1).is_empty(), "not again at once");
        let sent = rig.poll(CLOCK_REPORT_US);
        assert_eq!(sent.len(), 1, "{sent:?}");
    }

    #[test]
    fn a_page_the_client_refused_is_asked_for_again_as_a_keyframe() {
        let mut rig = in_lull();
        rig.due.view = true;
        let sent = rig.poll(1);
        let view = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a view poll");
        rig.receive(
            SEAT,
            &answer(
                id_of(view),
                r#"{"next_cursor":"c2","complete":true}"#,
                "lull",
            ),
        )
        .expect("reads");
        rig.view_refused();
        assert_eq!(rig.view_refusals(), 1);
        assert!(!rig.view_settled());
        assert!(
            !rig.poll(2)
                .iter()
                .any(|frame| frame.link == SEAT && method_of(frame) == "get_view"),
            "not in a loop"
        );
        let sent = rig.poll(2 + KEEPALIVE_US);
        let again = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a keyframe");
        assert_eq!(method_of(again), "get_view");
        assert!(again.text.contains("\"params\": {}"), "{}", again.text);
    }

    #[test]
    fn an_answer_nobody_is_waiting_for_is_dropped() {
        let mut rig = in_lull();
        let got = rig
            .receive(ADMIN, &answer(999, "{}", "push"))
            .expect("reads");
        assert_eq!(got, Answer::default());
        assert_eq!(rig.phase(), Phase::Lull);
    }

    /// A rig in a Lull whose editor holds a playbook it has checked.
    fn editing() -> Rig {
        let mut rig = in_lull();
        assert!(rig.editor_mut().load(b"{}"));
        let sent = rig.poll(2);
        let check = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("the load check");
        assert_eq!(method_of(check), "verify_plan");
        rig.receive(
            SEAT,
            &answer(
                id_of(check),
                r#"{"report":{"qualifies":true,"depth":"quick"}}"#,
                "lull",
            ),
        )
        .expect("reads");
        assert!(rig.editor().has_text());
        // The rule list for the loaded text.
        let sent = rig.poll(2);
        let render = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("the rule list");
        assert_eq!(method_of(render), "render_plan");
        rig.receive(
            SEAT,
            &answer(id_of(render), r#"{"prose":"Playbook\n"}"#, "lull"),
        )
        .expect("reads");
        rig
    }

    #[test]
    fn a_covering_that_covers_nothing_is_the_gateways_answer_not_a_refusal() {
        let mut rig = in_lull();
        rig.editor_mut().set_seat("seat.0");
        assert!(rig.editor_mut().load(
            br#"{"declarative":{"route":[{"label":"p","place_beacon":{"at":{"covering":{"feature_id":"vent_1_1"}}}}]}}"#
        ));
        // A second apart, so the seat's rate budget never holds a call back.
        let mut now: u64 = 2;
        let estimate = loop {
            assert!(now < 20_000_000, "the route was never priced");
            let sent = rig.poll(now);
            now += 1_000_000;
            let Some(frame) = sent.iter().find(|frame| frame.link == SEAT) else {
                continue;
            };
            if method_of(frame) == "estimate_route" {
                break id_of(frame);
            }
            let reply = if method_of(frame) == "render_plan" {
                r#"{"prose":"Playbook\n"}"#
            } else if method_of(frame) == "resolve_refs" {
                r#"{"refs":[]}"#
            } else {
                r#"{"report":{"qualifies":true,"depth":"quick"}}"#
            };
            rig.receive(SEAT, &answer(id_of(frame), reply, "lull"))
                .expect("reads");
            if rig.editor().has_text() && rig.editor().route().points.is_empty() {
                // The commander, as the view finds it once the playbook is on screen.
                rig.editor_mut().set_entities(&[crate::view::Entity {
                    id: "u_1".to_owned(),
                    kind: crate::view::EntityKind::Unit,
                    subtype: "commander".to_owned(),
                    owner: "seat.0".to_owned(),
                    at: [358, 22, 36],
                    hp: None,
                    heading: None,
                    ring: None,
                    look: None,
                    title: String::new(),
                    building: false,
                }]);
            }
        };
        let refusal = format!(
            r#"{{"jsonrpc":"2.0","id":{estimate},"error":{{"code":-32000,"message":"waypoint 1 covers nothing: its step would fail `no_target`","data":{{"code":"NOT_FOUND"}}}}}}"#
        );
        let got = rig.receive(SEAT, &refusal).expect("reads");
        assert_eq!(got.error, None, "an answer, not a refused call");
        assert_eq!(rig.last_error(), None);
        assert_eq!(rig.editor().refusals(), 0);
        assert_eq!(
            rig.editor().route().found_nothing,
            "waypoint 1 covers nothing: its step would fail `no_target`"
        );
    }

    /// **A call past the gateway's message cap is not sent** (S3's `fixq-c`, fix pass 1, the
    /// HIGH: a 300 KB paste closed the seat connection with 1009, and was sent again after every
    /// reconnect). The rig refuses the frame before it goes, counts it, settles the editor's call
    /// as refused in this client's words, and asks it never again; the connection stays up.
    #[test]
    fn a_call_past_the_message_cap_is_never_sent() {
        let mut rig = editing();
        let notes = "x".repeat(pharmakos_proto::transport::MAX_MESSAGE_BYTES);
        rig.editor_mut().save_notes(&notes);
        let sent = rig.poll(3);
        assert!(
            sent.iter()
                .all(|frame| frame.link != SEAT || method_of(frame) != "save_notes"),
            "nothing past the cap goes out"
        );
        assert_eq!(rig.message_refusals(), [[0, 0], [0, 1]]);
        assert_eq!(rig.editor().status().key, "call_too_large");
        assert_eq!(rig.drops(), [0, 0], "the connection stays up");
        assert_eq!(rig.editor().refusals(), 0, "not the gateway's refusal");
        for now in 4..12 {
            for frame in rig.poll(now) {
                assert_ne!(method_of(&frame), "save_notes", "never asked again");
            }
        }
    }

    /// The notes text that makes the seat's next `save_notes` frame exactly `bytes` long, in UTF-8
    /// bytes, with the filler `unit` (one character of `unit_bytes` bytes once JSON has written
    /// it) repeated; `None` when no whole number of units fits.
    fn notes_for_frame(rig: &Rig, bytes: usize, unit: &str) -> Option<String> {
        let id = rig.links.get(SEAT)?.next_id.checked_add(1)?;
        let empty = frame_text(
            id,
            "save_notes",
            object(vec![("notes", Json::String(String::new()))]),
        );
        let one = frame_text(
            id,
            "save_notes",
            object(vec![("notes", Json::String(unit.to_owned()))]),
        );
        let per = one.len().checked_sub(empty.len())?;
        let room = bytes.checked_sub(empty.len())?;
        let units = room.checked_div(per)?;
        (units.checked_mul(per)? == room).then(|| unit.repeat(units))
    }

    /// **The cap is a byte count of the frame as sent** (fix pass 2, review 2's mutants R1, the
    /// cap counted in characters, and R2, `>=` for `>`): a frame of exactly the cap goes out and
    /// one byte more does not; a text under the cap in characters and over it in UTF-8 bytes, and a
    /// text under it before JSON's escaping and over it after, are not sent.
    #[test]
    fn the_cap_is_counted_in_bytes_of_the_frame_as_sent() {
        let cap = pharmakos_proto::transport::MAX_MESSAGE_BYTES;
        let sent_notes = |notes: &str| {
            let mut rig = editing();
            rig.editor_mut().save_notes(notes);
            let sent = rig.poll(3);
            let frame = sent
                .iter()
                .find(|frame| frame.link == SEAT && method_of(frame) == "save_notes");
            (
                frame.map(|frame| frame.text.len()),
                rig.message_refusals()[1][1],
            )
        };
        let rig = editing();
        let exact = notes_for_frame(&rig, cap, "x").expect("x fills a frame to the cap");
        assert_eq!(
            sent_notes(&exact),
            (Some(cap), 0),
            "exactly the cap is sent"
        );
        let over = format!("{exact}x");
        assert_eq!(sent_notes(&over), (None, 1), "one byte more is not");
        // 90 000 three-byte characters: under the cap in characters, over it in bytes.
        let euros = "\u{20ac}".repeat(90_000);
        assert!(euros.chars().count() < cap && euros.len() > cap);
        assert_eq!(
            sent_notes(&euros),
            (None, 1),
            "a text over the cap in bytes"
        );
        // Quotes and backslashes: under the cap as typed, over it once JSON escapes each.
        let quoted = "\"\\".repeat(87_000);
        assert!(quoted.len() < cap);
        assert_eq!(
            sent_notes(&quoted),
            (None, 1),
            "a text over the cap once escaped"
        );
    }

    /// **A send the transport refuses is settled at once** (fix pass 2, the HIGH's mechanism:
    /// Godot's `send_text` refused a frame its outbound buffer could not hold, the error was
    /// ignored, and the call sat "Sending..." until the link timed out and was asked again): the
    /// call is refused under its own reason, said, counted, and neither left in flight nor asked
    /// again.
    #[test]
    fn a_send_the_transport_refuses_is_settled_and_never_asked_again() {
        let mut rig = editing();
        rig.editor_mut().save_notes("notes");
        let sent = rig.poll(3);
        assert!(sent.iter().any(|frame| method_of(frame) == "save_notes"));
        rig.send_failed(SEAT, 6);
        assert_eq!(rig.send_failures(), [0, 1]);
        assert_eq!(rig.editor().status().key, "call_send_failed");
        assert_eq!(rig.drops(), [0, 0]);
        assert!(
            rig.links
                .get(SEAT)
                .is_some_and(|link| link.in_flight.is_none()),
            "not left in flight"
        );
        for now in 4..12 {
            for frame in rig.poll(now) {
                assert_ne!(method_of(&frame), "save_notes", "never asked again");
                let _ = rig.receive(
                    frame.link,
                    &answer(id_of(&frame), r#"{"notes":""}"#, "lull"),
                );
            }
        }
    }

    /// **A connection closed on the message itself never re-sends it** (fix pass 1):
    /// **A close on a message is kept for the player's words** (fix pass 3, item 3): which
    /// connection, the close code, the call it carried, and whether the connection is open
    /// again; a close of the connection itself is not one.
    #[test]
    fn a_close_on_a_message_is_kept_with_its_connection_code_and_call() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        rig.opened(SEAT);
        assert_eq!(rig.closed_on_message(), None);
        let view = rig
            .poll(1)
            .into_iter()
            .find(|frame| frame.link == SEAT)
            .expect("the seat's first call");
        assert_eq!(method_of(&view), "get_view");
        rig.closed(SEAT, Close::ThisMessage(1009));
        assert_eq!(
            rig.closed_on_message(),
            Some(ClosedOnMessage {
                link: SEAT,
                connection: "seat",
                code: 1009,
                call: Some("view"),
            })
        );
        assert!(!rig.link_open(SEAT));
        // A view closed on its message is a refused page: counted, and a keyframe asked again
        // only once the keep-alive has passed, so a page too big to read is not asked in a loop.
        assert_eq!(rig.view_refusals(), 1);
        rig.opened(SEAT);
        assert!(rig.link_open(SEAT));
        assert!(
            rig.poll(2)
                .iter()
                .all(|frame| !(frame.link == SEAT && method_of(frame) == "get_view")),
            "a keyframe asked again at once"
        );
        rig.closed(SEAT, Close::Connection);
        assert_eq!(
            rig.closed_on_message().map(|closed| closed.code),
            Some(1009),
            "a close of the connection leaves the last close on a message as it was"
        );
    }

    /// a close with 1009 (or 1003, 1007, 1008) settles the call in flight as refused and counts the
    /// drop under its own reason; any other close asks the call again once the link is back.
    #[test]
    fn a_close_on_the_message_settles_it_and_any_other_close_asks_again() {
        for (close, again) in [(Close::ThisMessage(1009), false), (Close::Connection, true)] {
            let mut rig = editing();
            assert!(rig.editor_mut().submit());
            let sent = rig.poll(3);
            assert!(sent.iter().any(|frame| method_of(frame) == "submit_plan"));
            rig.closed(SEAT, close);
            rig.opened(SEAT);
            let mut resent = false;
            for now in 4..12 {
                for frame in rig.poll(now) {
                    if method_of(&frame) == "submit_plan" {
                        resent = true;
                    }
                    let body = match method_of(&frame).as_str() {
                        "get_view" => r#"{"next_cursor":"c1","complete":true}"#,
                        "get_status" => r#"{"preset":"custom"}"#,
                        _ => "{}",
                    };
                    let _ = rig.receive(frame.link, &answer(id_of(&frame), body, "lull"));
                }
            }
            assert_eq!(resent, again, "{close:?}");
            if !again {
                assert_eq!(rig.message_refusals()[0], [0, 1]);
                assert_eq!(rig.editor().status().key, "call_closed_on");
            }
        }
        assert_eq!(Close::of_code(1009), Close::ThisMessage(1009));
        assert_eq!(Close::of_code(1008), Close::ThisMessage(1008));
        assert_eq!(Close::of_code(1006), Close::Connection);
        assert_eq!(Close::of_code(-1), Close::Connection);
    }

    #[test]
    fn ready_waits_behind_the_submission_it_is_ready_with() {
        let mut rig = editing();
        assert!(rig.editor_mut().submit());
        rig.ready();
        let sent = rig.poll(3);
        let first = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a seat call");
        assert_eq!(method_of(first), "submit_plan", "the submission goes first");
        rig.receive(
            SEAT,
            &answer(
                id_of(first),
                r#"{"report":{"qualifies":true,"depth":"full"},"accepted":true}"#,
                "lull",
            ),
        )
        .expect("reads");
        // The seal changes the Lull's "this round" sentence, so the briefing is read again,
        // and Ready waits behind it too.
        let sent = rig.poll(4);
        let briefing = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a seat call");
        assert_eq!(method_of(briefing), "get_briefing");
        rig.receive(SEAT, &answer(id_of(briefing), r#"{"notes":""}"#, "lull"))
            .expect("reads");
        // The accepted seal is asked of the gateway (S3's `prob`), and Ready waits behind that
        // read too, so the seal is told in the Lull it was sealed in.
        let sent = rig.poll(4);
        let status = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a seat call");
        assert_eq!(method_of(status), "get_status");
        rig.receive(
            SEAT,
            &answer(
                id_of(status),
                r#"{"preset":"custom","sealed_this_round":true}"#,
                "lull",
            ),
        )
        .expect("reads");
        assert_eq!(
            rig.sealed_this_round(),
            Some(true),
            "as the gateway told it"
        );
        let sent = rig.poll(4);
        let next = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a seat call");
        assert_eq!(method_of(next), "set_ready");
        rig.receive(SEAT, &answer(id_of(next), "{}", "lull"))
            .expect("reads");
        // Ready means the orders are final: the editor sends nothing more this Lull, and an
        // ask is refused at once rather than queued behind a door that stays shut (finding K).
        assert_eq!(rig.editor().door(), Door::Ready);
        assert!(!rig.editor_mut().act(
            crate::editor::Action::Go,
            &crate::editor::Target::Beacon("b_00".to_owned())
        ));
        assert_eq!(rig.editor().status().key, "closed_ready");
        assert!(!rig.editor().has_pending_jobs());
        assert!(
            !rig.poll(5)
                .iter()
                .any(|frame| frame.link == SEAT && method_of(frame) == "patch_plan"),
            "no planning call after Ready"
        );
    }

    /// Planning is the Lull's: in a Push an ask is refused at once with the reason, and an
    /// edit queued as the Lull ended is dropped and said to be, rather than waiting for a
    /// Lull (finding K; before, a submission made in a Push waited for the next Lull and
    /// said "Submitting..." until then, or for ever in the last round).
    #[test]
    fn planning_asks_are_refused_at_once_outside_a_lull() {
        let mut rig = editing();
        assert!(rig.editor_mut().act(
            crate::editor::Action::Go,
            &crate::editor::Target::Beacon("b_00".to_owned())
        ));
        rig.end_lull();
        let sent = rig.poll(3);
        let end = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("end_lull");
        assert!(
            !sent.iter().any(|frame| frame.link == SEAT),
            "the edit is held while the Lull ends: {sent:?}"
        );
        rig.receive(ADMIN, &answer(id_of(end), "{}", "push"))
            .expect("reads");
        assert_eq!(rig.phase(), Phase::Push);
        assert_eq!(rig.editor().door(), Door::Push);
        assert!(
            !rig.editor().has_pending_jobs(),
            "the edit the Push shut out is dropped, not kept for a Lull"
        );
        assert_eq!(rig.editor().status().key, "closed_push");
        assert!(
            !rig.editor_mut().submit(),
            "a submission in a Push is refused"
        );
        assert_eq!(rig.editor().status().key, "closed_push");
        settle(&mut rig, 4);
        assert!(!rig.editor().has_pending_jobs());
        assert_eq!(rig.editor().refusals(), 0, "nothing was sent to be refused");
    }

    /// A refusal answer to the call in flight on the seat connection, with `code`.
    fn refusal(id: i64, code: &str, message: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32000,"message":"{message}","data":{{"code":"{code}"}}}}}}"#
        )
    }

    /// A check sent in the Lull's last moment comes back `PHASE_CLOSED` once the Push has
    /// begun: the editor settles it, and the panel says the check was refused, with the
    /// gateway's words, instead of "Checking..." for the whole Push (finding K).
    #[test]
    fn a_check_refused_as_the_lull_ends_says_so_and_nothing_waits() {
        let mut rig = editing();
        assert!(rig.editor_mut().act(
            crate::editor::Action::Go,
            &crate::editor::Target::Beacon("b_00".to_owned())
        ));
        let patch = rig
            .poll(3)
            .into_iter()
            .find(|frame| frame.link == SEAT)
            .expect("the patch");
        assert_eq!(method_of(&patch), "patch_plan");
        rig.receive(
            SEAT,
            &answer(
                id_of(&patch),
                r#"{"playbook_jsonc":"{ }","inverse_json_patch":"[]"}"#,
                "lull",
            ),
        )
        .expect("reads");
        let check = rig
            .poll(4)
            .into_iter()
            .find(|frame| frame.link == SEAT)
            .expect("the check");
        assert_eq!(method_of(&check), "verify_plan");
        // The Lull ends while the check is in flight.
        rig.end_lull();
        let end = rig
            .poll(5)
            .into_iter()
            .find(|frame| frame.link == ADMIN)
            .expect("end_lull");
        rig.receive(ADMIN, &answer(id_of(&end), "{}", "push"))
            .expect("reads");
        let said = "`verify_plan` is a planning method, and planning is closed";
        let got = rig
            .receive(SEAT, &refusal(id_of(&check), "PHASE_CLOSED", said))
            .expect("reads");
        assert!(got.error.is_some(), "a refused call, reported as one");
        let editor = rig.editor();
        assert!(!editor.rows_current(), "the edited text was never checked");
        assert_eq!(
            editor.unchecked(crate::editor::Check::Rows),
            Some(crate::editor::Unchecked::Refused(format!(
                "PHASE_CLOSED: {said}"
            ))),
            "the panel shows the gateway's refusal in place of Checking..."
        );
        assert!(
            !editor.busy() && !editor.has_pending_jobs(),
            "nothing waits"
        );
        assert_eq!(editor.status().key, "gateway_refused");
    }

    /// Once the match has ended no planning call will ever go out: an ask is refused at
    /// once, and a placement ghost queued as it ended is dropped rather than left on
    /// "Checking this spot..." (finding K).
    #[test]
    fn after_the_match_ends_no_ask_waits_and_no_ghost_hangs() {
        let mut rig = editing();
        assert!(rig.editor_mut().preview_place([350, 22, 36]));
        rig.end_lull();
        let end = rig
            .poll(3)
            .into_iter()
            .find(|frame| frame.link == ADMIN)
            .expect("end_lull");
        rig.receive(ADMIN, &answer(id_of(&end), "{}", "ended"))
            .expect("reads");
        assert_eq!(rig.phase(), Phase::Ended);
        let editor = rig.editor_mut();
        assert_eq!(editor.door(), Door::Ended);
        assert!(editor.ghost().is_none(), "no ghost is left waiting");
        assert_eq!(editor.status().key, "closed_ended");
        assert!(!editor.has_pending_jobs());
        assert!(!editor.preview_place([350, 22, 36]));
        assert!(!editor.act(
            crate::editor::Action::Go,
            &crate::editor::Target::Beacon("b_00".to_owned())
        ));
        assert!(!editor.submit());
        assert!(!editor.load(b"{}"));
        assert!(editor.ghost().is_none() && !editor.has_pending_jobs());
        assert_eq!(editor.status().key, "closed_ended");
    }

    /// The gateway's `safe_playbook_filed` line in this seat's own feed tells the editor
    /// that the safe playbook was filed and its text was not sealed (finding J).
    #[test]
    fn a_safe_playbook_filed_line_in_the_feed_reaches_the_editor() {
        let mut rig = in_push();
        let advance = rig
            .poll(1 + PACER_PERIOD_US)
            .into_iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an advance");
        rig.receive(
            ADMIN,
            &answer(id_of(&advance), r#"{"advanced_ms":400}"#, "push"),
        )
        .expect("reads");
        let said = "The Lull ran out, so the safe playbook was filed for you.";
        let mut fed = false;
        for step in 2..12_u64 {
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            for frame in rig.poll(PACER_PERIOD_US.saturating_add(step)) {
                let result = match method_of(&frame).as_str() {
                    "get_segment_feed" => {
                        fed = true;
                        format!(
                            r#"{{"events":[{{"at_ms":0,"kind":"safe_playbook_filed","text":"{said}"}},{{"at_ms":0,"kind":"plan_sealed","text":"Seat 0 sealed its playbook."}}],"next_cursor":"f2"}}"#
                        )
                    }
                    "get_view" => r#"{"next_cursor":"c9","complete":true}"#.to_owned(),
                    _ => "{}".to_owned(),
                };
                rig.receive(frame.link, &answer(id_of(&frame), &result, "push"))
                    .expect("reads");
            }
        }
        assert!(fed, "the feed was read after the match moved");
        let editor = rig.editor();
        assert!(editor.filed());
        assert_eq!(editor.status().key, "safe_filed");
        assert_eq!(
            editor.status().detail,
            said,
            "the gateway's words, as they came"
        );
        assert_eq!(
            editor.seal_line(),
            crate::editor::SealLine::Filed,
            "outside a Lull the verdict line says the text was not sealed"
        );
    }

    /// The feed kind the rig looks for is the one the gateway writes
    /// (`crates/gateway/src/surface.rs`, `SAFE_PLAYBOOK_FILED_KIND`), read out of its source
    /// because the client may not depend on the gateway (`CLIENT_WALL`).
    #[test]
    fn the_filed_kind_is_the_one_the_gateway_writes() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../gateway/src/surface.rs");
        let source = std::fs::read_to_string(path).expect("the gateway's source");
        assert!(
            source.contains(&format!(
                "pub const SAFE_PLAYBOOK_FILED_KIND: &str = \"{SAFE_PLAYBOOK_FILED_KIND}\";"
            )),
            "the gateway's filing kind is not `{SAFE_PLAYBOOK_FILED_KIND}`"
        );
    }

    #[test]
    fn the_seat_spends_no_more_than_its_budget_between_clock_reports() {
        let mut rig = editing();
        for _ in 0..12 {
            assert!(rig.editor_mut().act(
                crate::editor::Action::Go,
                &crate::editor::Target::Beacon("b_00".to_owned())
            ));
        }
        let mut seat_calls = 0_u32;
        // Answer every seat call at once, with no clock report in between.
        for step in 0..64_u64 {
            let sent = rig.poll(3 + step);
            for frame in sent.iter().filter(|frame| frame.link == SEAT) {
                seat_calls += 1;
                let result = match method_of(frame).as_str() {
                    "patch_plan" => {
                        r#"{"playbook_jsonc":"{ }","inverse_json_patch":"[]"}"#.to_owned()
                    }
                    _ => r#"{"report":{"qualifies":true,"depth":"quick"}}"#.to_owned(),
                };
                rig.receive(SEAT, &answer(id_of(frame), &result, "lull"))
                    .expect("reads");
            }
            // The admin connection's calls are left unanswered: the clock never moves.
        }
        assert!(
            seat_calls <= SEAT_CALLS_PER_REFILL,
            "{seat_calls} seat calls with the gateway's clock standing still"
        );
    }

    /// Polls at `now` with the seat's budget spent, answers the clock report the rig sends
    /// as the gateway would in `phase`, and says the report's `elapsed_ms` and whether the
    /// answer refilled the seat's budget.
    fn clock_answer(rig: &mut Rig, now: u64, phase: &str) -> (i64, bool) {
        rig.seat_calls_left = 0;
        let sent = rig.poll(now);
        let clock = sent
            .iter()
            .find(|frame| frame.link == ADMIN && method_of(frame) == "report_host_clock")
            .expect("a clock report");
        let elapsed = json::read(&clock.text)
            .expect("json")
            .get("params")
            .and_then(|params| params.get("elapsed_ms"))
            .and_then(json_integer)
            .expect("an elapsed_ms");
        rig.receive(
            ADMIN,
            &answer(id_of(clock), r#"{"all_ready":false}"#, phase),
        )
        .expect("reads");
        (elapsed, rig.seat_calls_left == SEAT_CALLS_PER_REFILL)
    }

    /// The gateway's clock outside a Push (`Surface::sync_time`) floors a phase's reported
    /// time to whole ticks, keeps it as a high-water mark and starts every phase again from
    /// zero, so neither a phase change nor a phase's first report, sent at once, moves its
    /// tick (decisions-log item 121 (4); `tests/phase_budget.rs` plays the flake itself).
    #[test]
    fn only_a_clock_answer_that_moved_the_gateways_tick_refills_the_seat() {
        // `in_lull` answered the Lull's first report; the next goes out a report interval
        // later and refills.
        let mut rig = in_lull();
        let (elapsed, refilled) = clock_answer(&mut rig, CLOCK_REPORT_US, "lull");
        assert!(elapsed >= 250, "{elapsed}");
        assert!(refilled, "a report a whole interval on refilled nothing");
        // A phase change moves no tick, so it refills nothing.
        rig.seat_calls_left = 0;
        let footer = pharmakos_proto::gp::api::v1::Status {
            phase: pharmakos_proto::gp::api::v1::status::Phase::Recap.into(),
            ..Default::default()
        };
        assert!(rig.observe(&footer).expect("a readable footer"));
        assert_eq!(
            rig.seat_calls_left, 0,
            "the phase change refilled the budget"
        );
        // The recap's first report goes out at once, with almost nothing spent: it moves
        // no tick and refills nothing.
        let recap_opened = CLOCK_REPORT_US + 1;
        let (elapsed, refilled) = clock_answer(&mut rig, recap_opened, "recap");
        assert!(elapsed < 50, "{elapsed}");
        assert!(!refilled, "the recap's first report refilled the budget");
        // Its second, a report interval later, does.
        let interval_on = CLOCK_REPORT_US.saturating_mul(2) + 1;
        let (elapsed, refilled) = clock_answer(&mut rig, interval_on, "recap");
        assert!(elapsed >= 250, "{elapsed}");
        assert!(refilled, "the recap's second report refilled nothing");
    }

    /// A timed Lull footer with a negative countdown is refused before anything moves: the
    /// rig stays in the recap it was in, and the next good footer opens the Lull with its
    /// clock (review of `ui`: the phase used to move first, leaving a Lull whose clock
    /// never started).
    #[test]
    fn a_refused_lull_footer_moves_nothing() {
        let mut rig = in_lull();
        let recap = pharmakos_proto::gp::api::v1::Status {
            phase: pharmakos_proto::gp::api::v1::status::Phase::Recap.into(),
            round: 1,
            ..Default::default()
        };
        assert!(rig.observe(&recap).expect("a readable footer"));
        let broken = pharmakos_proto::gp::api::v1::Status {
            phase: pharmakos_proto::gp::api::v1::status::Phase::Lull.into(),
            round: 2,
            phase_remaining_ms: -1,
            ..Default::default()
        };
        assert!(rig.observe(&broken).is_err());
        assert_eq!(
            rig.phase,
            Phase::Recap,
            "the refused footer moved the phase"
        );
        assert_eq!(rig.round, 1, "the refused footer moved the round");
        let good = pharmakos_proto::gp::api::v1::Status {
            phase_remaining_ms: 5_000,
            ..broken
        };
        assert!(rig.observe(&good).expect("a readable footer"));
        assert_eq!(rig.phase, Phase::Lull);
        assert!(
            !rig.timer_text().is_empty(),
            "the Lull opened by the good footer shows its countdown"
        );
    }

    #[test]
    fn the_meter_shows_the_gateways_numbers_as_they_came() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        rig.opened(SEAT);
        let sent = rig.poll(0);
        let status = sent.first().expect("admin first");
        let view = sent.get(1).expect("seat second");
        rig.receive(ADMIN, &answer(id_of(status), "{}", "lull"))
            .expect("reads");
        rig.receive(
            SEAT,
            &answer(
                id_of(view),
                r#"{"next_cursor":"c1","complete":true}"#,
                "lull",
            ),
        )
        .expect("reads");
        // The Lull opened: the meter is due on the seat connection. Its headroom is NOT
        // supply minus draw, on purpose: the meter shows the gateway's number, not a sum.
        let mut read = false;
        for step in 1..16_u64 {
            // A clock report every step, so the seat's budget refills as it would live.
            let now = CLOCK_REPORT_US.saturating_mul(step);
            for frame in rig.poll(now) {
                let result = match method_of(&frame).as_str() {
                    "get_economy_forecast" => {
                        assert_eq!(frame.link, SEAT, "the meter rides the seat connection");
                        read = true;
                        r#"{"treasury_now":200,"supply_kw_now":10,"draw_kw_now":4,"headroom_kw_now":-3}"#
                    }
                    "get_view" => r#"{"next_cursor":"c9","complete":true}"#,
                    "get_segment_feed" => r#"{"events":[],"next_cursor":"f1"}"#,
                    _ => "{}",
                };
                rig.receive(frame.link, &answer(id_of(&frame), result, "lull"))
                    .expect("reads");
            }
        }
        assert!(read, "the meter was asked for when the Lull opened");
        let meter = rig.meter();
        assert_eq!(meter.answers, 1, "once per Lull, not in a loop");
        assert_eq!(meter.phase, Phase::Lull);
        assert_eq!(
            (
                meter.treasury_now,
                meter.supply_kw_now,
                meter.draw_kw_now,
                meter.headroom_kw_now
            ),
            (200, 10, 4, -3),
            "each field exactly as it came; headroom is the gateway's, never supply minus draw"
        );
    }

    #[test]
    fn the_meter_follows_the_match_in_a_push() {
        let mut rig = in_push();
        let before = rig.meter().answers;
        let sent = rig.poll(1 + PACER_PERIOD_US);
        let advance = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an advance");
        rig.receive(
            ADMIN,
            &answer(id_of(advance), r#"{"advanced_ms":400}"#, "push"),
        )
        .expect("reads");
        settle(&mut rig, 2 + PACER_PERIOD_US);
        assert_eq!(
            rig.meter().answers,
            before + 1,
            "read again after the match moved"
        );
        assert_eq!(rig.meter().phase, Phase::Push);
    }

    /// An answer whose footer is `footer`, a whole `_status` object.
    fn answer_with(id: i64, result: &str, footer: &str) -> String {
        let body = result.strip_suffix('}').expect("a result is an object");
        let sep = if body.len() > 1 { "," } else { "" };
        format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{body}{sep}"_status":{footer}}}}}"#)
    }

    /// A rig timed by the committed table's two rows, whose first admin answer has `footer`.
    fn opened_with(footer: &str) -> Rig {
        let mut rig = Rig::new();
        rig.set_lull_length(LullLengths {
            first_ms: Some(600_000),
            later_ms: Some(300_000),
        });
        rig.opened(ADMIN);
        let sent = rig.poll(0);
        let status = sent.first().expect("the first get_status");
        rig.receive(ADMIN, &answer_with(id_of(status), "{}", footer))
            .expect("reads");
        rig
    }

    /// The countdown the rig reports next, read off the `report_host_clock` it sends.
    fn reported_countdown(rig: &mut Rig, now: u64) -> Option<i64> {
        let sent = rig.poll(now);
        let clock = sent
            .iter()
            .find(|frame| frame.link == ADMIN && method_of(frame) == "report_host_clock")
            .expect("a clock report");
        json::read(&clock.text)
            .expect("json")
            .get("params")
            .and_then(|params| params.get("remaining_ms"))
            .and_then(json_integer)
    }

    /// Round 1's Lull counts down from `first_lull_ms` and every later one from `lull_ms`
    /// (spec section 3's ten and five minutes; decisions-log item 127 (2)): the countdown is
    /// the client's, and the gateway's footer is untimed until it is told one.
    #[test]
    fn the_first_lull_is_timed_by_its_own_row_and_later_lulls_by_the_other() {
        let mut first =
            opened_with(r#"{"phase":"lull","phase_remaining_ms":0,"round":1,"untimed":true}"#);
        assert_eq!(reported_countdown(&mut first, 1), Some(600_000));
        let mut later =
            opened_with(r#"{"phase":"lull","phase_remaining_ms":0,"round":2,"untimed":true}"#);
        assert_eq!(reported_countdown(&mut later, 1), Some(300_000));
    }

    /// A timed footer is a countdown this client reported before it reconnected: the Lull
    /// goes on from it, including from zero, which has run out and ends the Lull.
    #[test]
    fn a_timed_footer_is_the_countdown_to_go_on_from() {
        let mut rig = opened_with(r#"{"phase":"lull","phase_remaining_ms":42000,"round":1}"#);
        assert_eq!(reported_countdown(&mut rig, 1), Some(42_000));
        let mut ran_out = opened_with(r#"{"phase":"lull","phase_remaining_ms":0,"round":2}"#);
        assert_eq!(
            admin_calls(&mut ran_out, 1, 2),
            [
                ("report_host_clock".to_owned(), Some(0)),
                ("end_lull".to_owned(), None)
            ],
            "a Lull rejoined at zero has run out: its zero, then its end"
        );
    }

    /// The admin calls the rig sends over `polls` polls from `start`, a clock report's cadence
    /// apart, each with the countdown it reported (`None` for none), every one answered with
    /// a Lull's footer that is not all ready.
    fn admin_calls(rig: &mut Rig, start: u64, polls: u64) -> Vec<(String, Option<i64>)> {
        let mut out = Vec::new();
        for poll in 0..polls {
            let now = start.saturating_add(poll.saturating_mul(CLOCK_REPORT_US));
            for frame in rig.poll(now) {
                if frame.link != ADMIN {
                    continue;
                }
                let method = method_of(&frame);
                let countdown = json::read(&frame.text)
                    .expect("json")
                    .get("params")
                    .and_then(|params| params.get("remaining_ms"))
                    .and_then(json_integer);
                let result = if method == "report_host_clock" {
                    r#"{"all_ready":false}"#
                } else {
                    "{}"
                };
                // `end_lull` ends the Lull, as the gateway's answer to it says.
                let phase = if method == "end_lull" { "push" } else { "lull" };
                rig.receive(ADMIN, &answer(id_of(&frame), result, phase))
                    .expect("reads");
                out.push((method, countdown));
            }
        }
        out
    }

    /// The admin frames the rig sends at `now`, each as `(method, elapsed_ms, remaining_ms)`,
    /// answered as `answer_as` says (`Some` a result, `None` left unanswered).
    fn admin_frames(
        rig: &mut Rig,
        now: u64,
        answer_as: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<(String, Option<i64>, Option<i64>)> {
        let mut out = Vec::new();
        for frame in rig.poll(now) {
            if frame.link != ADMIN {
                continue;
            }
            let method = method_of(&frame);
            let params = json::read(&frame.text)
                .expect("json")
                .get("params")
                .cloned()
                .unwrap_or(Json::Null);
            let read = |key: &str| params.get(key).and_then(json_integer);
            out.push((method.clone(), read("elapsed_ms"), read("remaining_ms")));
            if let Some(answer) = answer_as(&method) {
                let phase = if method == "end_lull" { "push" } else { "lull" };
                rig.receive(ADMIN, &answer_frame(id_of(&frame), &answer, phase))
                    .expect("reads");
            }
        }
        out
    }

    fn answer_frame(id: i64, result: &str, phase: &str) -> String {
        answer(id, result, phase)
    }

    fn plain_answer(method: &str) -> String {
        if method == "report_host_clock" {
            r#"{"all_ready":false}"#.to_owned()
        } else {
            "{}".to_owned()
        }
    }

    /// **After a stall the run-out is caught up in steps the gateway takes**: the gateway
    /// refuses a report more than `MAX_CLOCK_STEP_MS` past its last, so a 120 s Lull polled again
    /// 125 s later reports at most 60 s past the last accepted report each time, the zero only
    /// when the end is within a step, and `end_lull` after it.
    #[test]
    fn a_stalled_run_out_is_reported_in_steps_before_its_zero() {
        let mut rig = opened_with(r#"{"phase":"lull","phase_remaining_ms":120000,"round":2}"#);
        let mut sent = admin_frames(&mut rig, 1, &|method| Some(plain_answer(method)));
        let mut now = 125_000_000_u64;
        for _ in 0..12 {
            sent.extend(admin_frames(&mut rig, now, &|method| {
                Some(plain_answer(method))
            }));
            now = now.saturating_add(CLOCK_REPORT_US);
        }
        let mut last = 0_i64;
        for (method, elapsed, _) in &sent {
            if method == "report_host_clock"
                && let Some(elapsed) = elapsed
            {
                assert!(
                    elapsed.saturating_sub(last) <= 60_000,
                    "a step past the gateway's limit: {sent:?}"
                );
                last = *elapsed;
            }
        }
        let zero = sent
            .iter()
            .position(|(_, _, left)| *left == Some(0))
            .expect("the zero");
        let end = sent
            .iter()
            .position(|(method, _, _)| method == "end_lull")
            .expect("end_lull");
        assert!(zero < end, "{sent:?}");
        assert_eq!(sent.get(zero).and_then(|frame| frame.1), Some(120_000));
    }

    /// **A zero the link lost is sent again** before `end_lull`: the zero counts as reported
    /// only on the gateway's answer.
    #[test]
    fn a_zero_lost_with_its_link_is_sent_again() {
        let mut rig = opened_with(r#"{"phase":"lull","phase_remaining_ms":1000,"round":2}"#);
        let mut now = 1_u64;
        let mut first_zero = false;
        for _ in 0..12 {
            let frames = admin_frames(&mut rig, now, &|method| {
                (method != "report_host_clock").then(|| "{}".to_owned())
            });
            now = now.saturating_add(CLOCK_REPORT_US);
            if frames.iter().any(|(_, _, left)| *left == Some(0)) {
                first_zero = true;
                break;
            }
            rig.dropped(ADMIN);
            rig.opened(ADMIN);
        }
        assert!(first_zero, "the zero went out");
        assert!(
            !admin_frames(&mut rig, now, &|method| Some(plain_answer(method)))
                .iter()
                .any(|(method, _, _)| method == "end_lull"),
            "no end_lull while the zero is unanswered"
        );
        rig.dropped(ADMIN);
        rig.opened(ADMIN);
        let mut after = Vec::new();
        for _ in 0..8 {
            now = now.saturating_add(CLOCK_REPORT_US);
            after.extend(admin_frames(&mut rig, now, &|method| {
                Some(plain_answer(method))
            }));
        }
        let zero = after
            .iter()
            .position(|(_, _, left)| *left == Some(0))
            .expect("the zero is sent again");
        let end = after
            .iter()
            .position(|(method, _, _)| method == "end_lull")
            .expect("end_lull");
        assert!(zero < end, "{after:?}");
    }

    /// Drives `rig` (a timed Lull that runs out at once) for `polls` polls a clock report's
    /// cadence apart, answering each admin call: a zero report with a refusal while `refuse`
    /// says so (given how many zeros were refused so far), everything else plainly. Returns
    /// what was sent, `(method, remaining_ms)`.
    fn drive_zeros(
        rig: &mut Rig,
        start: u64,
        polls: u64,
        refuse: &dyn Fn(usize) -> bool,
    ) -> Vec<(String, Option<i64>)> {
        let mut sent = Vec::new();
        let mut refused = 0_usize;
        let mut now = start;
        for _ in 0..polls {
            for frame in rig.poll(now) {
                if frame.link != ADMIN {
                    continue;
                }
                let method = method_of(&frame);
                let left = json::read(&frame.text)
                    .expect("json")
                    .get("params")
                    .and_then(|params| params.get("remaining_ms"))
                    .and_then(json_integer);
                sent.push((method.clone(), left));
                let text = if method == "report_host_clock" && left == Some(0) && refuse(refused) {
                    refused = refused.saturating_add(1);
                    refusal(id_of(&frame), "RATE_LIMITED", "too many reports")
                } else {
                    let phase = if method == "end_lull" { "push" } else { "lull" };
                    answer_frame(id_of(&frame), &plain_answer(&method), phase)
                };
                let _ = rig.receive(ADMIN, &text);
            }
            // The Lull is over once `end_lull` is answered: the drive stops there.
            if sent.iter().any(|(method, _)| method == "end_lull") {
                break;
            }
            now = now.saturating_add(CLOCK_REPORT_US);
        }
        sent
    }

    fn zeros(sent: &[(String, Option<i64>)]) -> usize {
        sent.iter()
            .filter(|(method, left)| method == "report_host_clock" && *left == Some(0))
            .count()
    }

    /// **A zero lost with its link, the link back inside one report interval**: no `end_lull`
    /// goes out before the zero is sent again.
    #[test]
    fn a_lost_zero_with_the_link_back_inside_the_cadence_holds_end_lull() {
        let mut rig = opened_with(r#"{"phase":"lull","phase_remaining_ms":1000,"round":2}"#);
        let mut now = 1_u64;
        let mut zero_at = None;
        for _ in 0..12 {
            let frames = admin_frames(&mut rig, now, &|method| {
                (method != "report_host_clock").then(|| "{}".to_owned())
            });
            if frames.iter().any(|(_, _, left)| *left == Some(0)) {
                zero_at = Some(now);
                break;
            }
            rig.dropped(ADMIN);
            rig.opened(ADMIN);
            now = now.saturating_add(CLOCK_REPORT_US);
        }
        assert!(zero_at.is_some());
        rig.dropped(ADMIN);
        rig.opened(ADMIN);
        let frames = admin_frames(&mut rig, now.saturating_add(1_000), &|method| {
            Some(plain_answer(method))
        });
        assert!(
            !frames.iter().any(|(method, _, _)| method == "end_lull"),
            "end_lull before the zero was sent again: {frames:?}"
        );
    }

    /// **After a stall, a poll inside the cadence once an intermediate step is answered sends
    /// no `end_lull`**: the zero is still owed.
    #[test]
    fn a_poll_between_catch_up_steps_holds_end_lull() {
        let mut rig = opened_with(r#"{"phase":"lull","phase_remaining_ms":120000,"round":2}"#);
        let _ = admin_frames(&mut rig, 1, &|method| Some(plain_answer(method)));
        let step = admin_frames(&mut rig, 125_000_000, &|method| Some(plain_answer(method)));
        assert!(
            step.iter()
                .any(|(method, _, left)| method == "report_host_clock" && *left == Some(60_000)),
            "an intermediate step: {step:?}"
        );
        let inside = admin_frames(&mut rig, 125_001_000, &|method| Some(plain_answer(method)));
        assert!(
            !inside.iter().any(|(method, _, _)| method == "end_lull"),
            "end_lull while the zero is owed: {inside:?}"
        );
    }

    /// **A zero the gateway refuses is sent again**, and `end_lull` waits for it (the
    /// gateway may refuse a zero under a rate limit): one refusal, then an answer, is reported
    /// as before.
    #[test]
    fn a_refused_zero_is_sent_again() {
        let mut rig = opened_with(r#"{"phase":"lull","phase_remaining_ms":1000,"round":2}"#);
        let sent = drive_zeros(&mut rig, 1, 30, &|refused| refused == 0);
        assert_eq!(zeros(&sent), 2, "{sent:?}");
        let end = sent
            .iter()
            .position(|(method, _)| method == "end_lull")
            .expect("end_lull");
        let last_zero = sent
            .iter()
            .rposition(|(method, left)| method == "report_host_clock" && *left == Some(0))
            .expect("a zero");
        assert!(last_zero < end, "{sent:?}");
        assert_eq!(
            rig.zero_refusal(),
            None,
            "an answered zero is no refusal to say"
        );
    }

    /// **A zero the gateway keeps refusing is given up after `ZERO_REFUSALS`**: no further
    /// zero this Lull, the gateway's own refusal is said, and `end_lull` goes out. The zeros go
    /// at the clock's cadence, never faster.
    #[test]
    fn a_zero_refused_every_time_is_given_up_and_said() {
        let mut rig = opened_with(r#"{"phase":"lull","phase_remaining_ms":1000,"round":2}"#);
        let sent = drive_zeros(&mut rig, 1, 40, &|_| true);
        assert_eq!(zeros(&sent), usize::from(ZERO_REFUSALS), "{sent:?}");
        assert_eq!(
            sent.iter()
                .filter(|(method, _)| method == "end_lull")
                .count(),
            1,
            "{sent:?}"
        );
        assert_eq!(rig.zero_refusal(), Some("RATE_LIMITED: too many reports"));
        assert_eq!(rig.last_error(), Some("RATE_LIMITED: too many reports"));
        // The next Lull starts its count again.
        let next = read_status(
            &json::read(r#"{"phase":"lull","phase_remaining_ms":1000,"round":3}"#).expect("json"),
        )
        .expect("a Status");
        assert!(rig.observe(&next).expect("a readable footer"));
        assert_eq!(rig.zero_refusal(), None);
        let again = drive_zeros(&mut rig, 20_000_000, 40, &|refused| refused < 2);
        assert_eq!(zeros(&again), 3, "{again:?}");
        assert_eq!(rig.zero_refusal(), None);
    }

    /// The zero reports go out no faster than the clock's cadence: at most one per report
    /// interval, however fast the poll.
    #[test]
    fn zero_retries_keep_the_clocks_cadence() {
        let mut rig = opened_with(r#"{"phase":"lull","phase_remaining_ms":1000,"round":2}"#);
        let mut sent = 0_usize;
        let mut now = 1_000_000_u64;
        for _ in 0..1_000 {
            for frame in rig.poll(now) {
                if frame.link != ADMIN {
                    continue;
                }
                if method_of(&frame) == "report_host_clock"
                    && frame.text.contains("\"remaining_ms\": 0")
                {
                    sent = sent.saturating_add(1);
                }
                let _ = rig.receive(ADMIN, &refusal(id_of(&frame), "RATE_LIMITED", "slow"));
            }
            now = now.saturating_add(1_000);
        }
        // 1000 polls 1 ms apart are one second: four report intervals at most.
        assert!(sent <= usize::from(ZERO_REFUSALS), "{sent}");
    }

    /// **The "ran out" line's client half** (decisions-log item 160 (4)): when a timed Lull's countdown is
    /// spent, the rig reports the countdown at zero once, and only then calls `end_lull`.
    #[test]
    fn a_spent_countdown_is_reported_at_zero_once_before_end_lull() {
        let mut rig = opened_with(r#"{"phase":"lull","phase_remaining_ms":1000,"round":2}"#);
        let calls = admin_calls(&mut rig, 1, 12);
        let zero = calls
            .iter()
            .position(|(method, countdown)| method == "report_host_clock" && *countdown == Some(0))
            .expect("the zero report");
        let end = calls
            .iter()
            .position(|(method, _)| method == "end_lull")
            .expect("end_lull");
        assert!(zero < end, "{calls:?}");
        assert_eq!(
            calls
                .iter()
                .filter(|(_, countdown)| *countdown == Some(0))
                .count(),
            1,
            "once: {calls:?}"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|(method, _)| method == "end_lull")
                .count(),
            1,
            "{calls:?}"
        );
    }

    /// An untimed Lull never runs out and never reports a zero; a Lull Ready ended reports no
    /// zero before its `end_lull`.
    #[test]
    fn an_untimed_lull_and_a_ready_lull_send_no_zero() {
        let mut untimed = Rig::new();
        untimed.opened(ADMIN);
        let sent = untimed.poll(0);
        let status = sent.first().expect("the first get_status");
        untimed
            .receive(
                ADMIN,
                &answer_with(
                    id_of(status),
                    "{}",
                    r#"{"phase":"lull","round":1,"untimed":true}"#,
                ),
            )
            .expect("reads");
        let calls = admin_calls(&mut untimed, 1, 40);
        assert!(
            calls
                .iter()
                .all(|(method, countdown)| countdown.is_none() && method != "end_lull"),
            "{calls:?}"
        );
        // Every seat ready: the clock's answer says so, and `end_lull` follows with no zero.
        let mut ready = opened_with(r#"{"phase":"lull","phase_remaining_ms":42000,"round":1}"#);
        let mut sent_calls: Vec<(String, Option<i64>)> = Vec::new();
        for poll in 0..6_u64 {
            for frame in ready.poll(1 + poll * CLOCK_REPORT_US) {
                if frame.link != ADMIN {
                    continue;
                }
                let method = method_of(&frame);
                let countdown = json::read(&frame.text)
                    .expect("json")
                    .get("params")
                    .and_then(|params| params.get("remaining_ms"))
                    .and_then(json_integer);
                let result = if method == "report_host_clock" {
                    r#"{"all_ready":true}"#
                } else {
                    "{}"
                };
                let phase = if method == "end_lull" { "push" } else { "lull" };
                ready
                    .receive(ADMIN, &answer(id_of(&frame), result, phase))
                    .expect("reads");
                sent_calls.push((method, countdown));
            }
        }
        assert!(
            sent_calls.iter().any(|(method, _)| method == "end_lull"),
            "{sent_calls:?}"
        );
        assert!(
            sent_calls
                .iter()
                .all(|(_, countdown)| *countdown != Some(0)),
            "{sent_calls:?}"
        );
    }

    /// **The untimed-by-rule footer field is the generated `Status`'s** (`gp.api.v1.Status`
    /// field 6): read by the strict decode itself, absent is false, and anything but a boolean is
    /// the decode's refusal.
    #[test]
    fn the_footer_field_is_the_generated_statuss() {
        // The footer as the gateway writes it in a Probation Lull (`gp.api.v1.Status` field 6,
        // always with `untimed`): the generated type reads the field itself.
        let footer =
            json::read(r#"{"phase":"lull","round":1,"untimed":true,"untimed_by_rule":true}"#)
                .expect("json");
        let status = read_status(&footer).expect("a Status");
        assert!(status.untimed_by_rule && status.untimed);
        let custom = json::read(r#"{"phase":"lull","round":1,"untimed":true}"#).expect("json");
        assert!(!read_status(&custom).expect("a Status").untimed_by_rule);
        assert!(read_status(&json::read(r#"{"untimed_by_rule":1}"#).expect("json")).is_err());
    }

    /// **A Lull untimed by rule** (a Probation match's; decisions-log item 160 (4)): the rig
    /// starts no countdown, never sends `remaining_ms` (which a Probation gateway would refuse),
    /// and the lobby shows none; a custom match's Lull still counts down.
    #[test]
    fn a_lull_untimed_by_rule_sends_no_countdown() {
        let footer = r#"{"phase":"lull","round":1,"untimed":true,"untimed_by_rule":true}"#;
        let mut probation = opened_with(footer);
        assert_eq!(probation.timer_text(), "");
        assert!(probation.untimed());
        assert!(probation.untimed_by_rule());
        let calls = admin_calls(&mut probation, 1, 3_000);
        assert!(
            calls
                .iter()
                .any(|(method, _)| method == "report_host_clock"),
            "the time spent is still reported: {calls:?}"
        );
        assert!(
            calls
                .iter()
                .all(|(method, countdown)| countdown.is_none() && method != "end_lull"),
            "no countdown and no run-out: {calls:?}"
        );
        assert_eq!(probation.timer_text(), "");
        // What the watch check reads: clock reports answered, and none carrying a countdown.
        assert!(probation.clock_answers() > 0);
        assert_eq!(probation.countdowns_sent(), 0);
        let mut custom =
            opened_with(r#"{"phase":"lull","phase_remaining_ms":0,"round":1,"untimed":true}"#);
        assert_eq!(reported_countdown(&mut custom, 1), Some(600_000));
        assert_eq!(custom.countdowns_sent(), 1);
        assert!(!custom.untimed_by_rule());
        let mut off =
            opened_with(r#"{"phase":"lull","round":1,"untimed":true,"untimed_by_rule":false}"#);
        assert_eq!(reported_countdown(&mut off, 1), Some(600_000));
    }

    /// A Lull the rules give no length is untimed: its reports carry no countdown, so the
    /// gateway never shows a timer at zero that ran out (S1-11).
    #[test]
    fn an_untimed_lull_reports_no_countdown() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        let sent = rig.poll(0);
        let status = sent.first().expect("the first get_status");
        rig.receive(
            ADMIN,
            &answer_with(
                id_of(status),
                "{}",
                r#"{"phase":"lull","round":1,"untimed":true}"#,
            ),
        )
        .expect("reads");
        assert_eq!(reported_countdown(&mut rig, 1), None);
        assert_eq!(rig.timer_text(), "");
    }

    /// The lobby shows no countdown while the admin footer says the phase is untimed, and the
    /// client's own countdown once the gateway has been told it (decisions-log item 135).
    #[test]
    fn no_countdown_is_shown_while_the_footer_is_untimed() {
        let mut rig =
            opened_with(r#"{"phase":"lull","phase_remaining_ms":0,"round":1,"untimed":true}"#);
        assert!(rig.untimed());
        assert_eq!(rig.timer_text(), "", "the footer has no countdown yet");
        let sent = rig.poll(1);
        let clock = sent
            .iter()
            .find(|frame| method_of(frame) == "report_host_clock")
            .expect("the first report");
        rig.receive(
            ADMIN,
            &answer_with(
                id_of(clock),
                r#"{"all_ready":false}"#,
                r#"{"phase":"lull","phase_remaining_ms":600000,"round":1}"#,
            ),
        )
        .expect("reads");
        assert!(!rig.untimed());
        assert_eq!(rig.timer_text(), "10:00");
    }

    /// A recap opening asks for `get_recap` on the seat connection and keeps its prose as
    /// it came: the settlement's, the shortfall's and the "found nothing" lines are the
    /// gateway's sentences (`econ`, decisions-log item 135 (1)).
    #[test]
    fn a_recap_is_read_once_and_drawn_as_the_gateway_wrote_it() {
        let mut rig = in_push();
        let sent = rig.poll(1 + PACER_PERIOD_US);
        let advance = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an advance");
        rig.receive(
            ADMIN,
            &answer(id_of(advance), r#"{"advanced_ms":400}"#, "recap"),
        )
        .expect("reads");
        let prose = "Round 1 ran 20 ticks. The match continues. A step found nothing: 3 \
                     matched, none reachable.";
        let mut asked = 0;
        for step in 2..12_u64 {
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            let now = PACER_PERIOD_US.saturating_add(step);
            for frame in rig.poll(now) {
                let result = match method_of(&frame).as_str() {
                    "get_recap" => {
                        asked += 1;
                        assert_eq!(frame.link, SEAT, "the recap rides the seat connection");
                        format!(
                            r#"{{"prose":"{prose}","shortfall":{{"kw":10,"beacon_ids":["b_01"]}}}}"#
                        )
                    }
                    "get_view" => r#"{"next_cursor":"c9","complete":true}"#.to_owned(),
                    "get_segment_feed" => r#"{"events":[],"next_cursor":"f1"}"#.to_owned(),
                    _ => "{}".to_owned(),
                };
                rig.receive(frame.link, &answer(id_of(&frame), &result, "recap"))
                    .expect("reads");
            }
        }
        assert_eq!(asked, 1, "once per recap, not in a loop");
        assert_eq!(rig.recap().prose, prose);
        assert_eq!(rig.recap().round, 1);
        assert!(
            rig.recap().seat_tallies.is_empty(),
            "between rounds a seat learns only its own tally"
        );
    }

    /// A Push's end can leave more events than one page holds: outside a Push nothing else
    /// asks for the next page, so the list stopped short of them. Each page with events asks
    /// again, from its own cursor, until one comes back empty; the rows stay in the feed's order.
    #[test]
    fn a_backlog_at_the_pushs_end_is_read_out_after_it() {
        let mut rig = in_push();
        let sent = rig.poll(1 + PACER_PERIOD_US);
        let advance = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an advance");
        rig.receive(
            ADMIN,
            &answer(id_of(advance), r#"{"advanced_ms":400}"#, "recap"),
        )
        .expect("reads");
        let mut cursors = Vec::new();
        let mut rows = Vec::new();
        for step in 2..16_u64 {
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            let now = PACER_PERIOD_US.saturating_add(step);
            for frame in rig.poll(now) {
                let result = match method_of(&frame).as_str() {
                    "get_segment_feed" => {
                        cursors.push(rig.feed_cursor.clone());
                        match cursors.len() {
                            1 => r#"{"events":[{"at_ms":1000,"kind":"unit_destroyed","text":"a"}],"next_cursor":"f1"}"#,
                            2 => r#"{"events":[{"at_ms":2000,"kind":"unit_destroyed","text":"b"}],"next_cursor":"f2"}"#,
                            _ => r#"{"events":[],"next_cursor":"f2"}"#,
                        }
                        .to_owned()
                    }
                    "get_view" => r#"{"next_cursor":"c9","complete":true}"#.to_owned(),
                    "get_recap" => r#"{"prose":"p"}"#.to_owned(),
                    _ => "{}".to_owned(),
                };
                let answered = rig
                    .receive(frame.link, &answer(id_of(&frame), &result, "recap"))
                    .expect("reads");
                rows.extend(answered.events.iter().map(EventRow::line));
            }
        }
        assert_eq!(cursors.len(), 3, "two pages and the empty one: {cursors:?}");
        assert_eq!(cursors.last().map(String::as_str), Some("f2"));
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(
            rows.first().is_some_and(|row| row.ends_with('a')),
            "{rows:?}"
        );
        assert!(
            rows.get(1).is_some_and(|row| row.ends_with('b')),
            "{rows:?}"
        );
    }

    /// Once the match is decided the answer carries every seat's tally beside the prose
    /// (item 145 (7)), in the gateway's own shape, the zeros written. The client kept only
    /// the prose, whose tally is the calling seat's, so the demo's match-end recap showed one
    /// seat's (item 155 (3) 7).
    #[test]
    fn the_recap_keeps_every_seats_tally_as_it_came() {
        let mut rig = in_push();
        let recap = r#"{"prose":"The match ended.","seat_tallies":[
            {"seat":"seat.0","tally":{"kills":3,"shared_kills":1,"losses":2,"value_destroyed_dollars":60}},
            {"seat":"seat.1","tally":{"kills":0,"shared_kills":0,"losses":3,"value_destroyed_dollars":0}}
        ],"_status":{"phase":"ended","round":3}}"#;
        rig.take_recap(&json::read(recap).expect("json"))
            .expect("reads");
        let seats = &rig.recap().seat_tallies;
        assert_eq!(seats.len(), 2);
        assert_eq!(
            seats.first(),
            Some(&SeatTallyRow {
                seat: "seat.0".to_owned(),
                kills: 3,
                shared_kills: 1,
                losses: 2,
                value_destroyed_dollars: 60,
            })
        );
        assert_eq!(seats.get(1).map(|one| one.losses), Some(3));
        assert_eq!(rig.recap().prose, "The match ended.");
        assert_eq!(rig.recap().round, 3);

        let no_tally = r#"{"prose":"x","seat_tallies":[{"seat":"seat.0"}],"_status":{"phase":"ended","round":3}}"#;
        let refused = rig
            .take_recap(&json::read(no_tally).expect("json"))
            .expect_err("a seat with no tally is refused, not drawn as zeros");
        assert!(refused.to_string().contains("seat.0"), "{refused}");
        assert!(
            rig.recap().refusal.contains("seat.0"),
            "the column says why, rather than go on showing the last answer: {:?}",
            rig.recap().refusal
        );

        // So does an answer this build cannot decode at all, and a good one clears it.
        let unreadable = r#"{"nope":1,"_status":{"phase":"ended","round":3}}"#;
        assert!(
            rig.take_recap(&json::read(unreadable).expect("json"))
                .is_err()
        );
        assert!(
            rig.recap().refusal.contains("nope"),
            "{:?}",
            rig.recap().refusal
        );
        rig.take_recap(&json::read(recap).expect("json"))
            .expect("reads");
        assert_eq!(rig.recap().refusal, "");
    }

    /// A refused `get_recap` is kept, as the gateway wrote it, for the recap column to
    /// show (review of `ui`: an `INTERNAL` naming a lost `segment_ended`, item 135 (1),
    /// used to reach only a warning), and it is not asked again in a loop.
    #[test]
    fn a_refused_recap_is_kept_for_the_column() {
        let mut rig = in_push();
        let sent = rig.poll(1 + PACER_PERIOD_US);
        let advance = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an advance");
        rig.receive(
            ADMIN,
            &answer(id_of(advance), r#"{"advanced_ms":400}"#, "recap"),
        )
        .expect("reads");
        let mut asked = 0;
        for step in 2..12_u64 {
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            let now = PACER_PERIOD_US.saturating_add(step);
            for frame in rig.poll(now) {
                let text = if method_of(&frame) == "get_recap" {
                    asked += 1;
                    format!(
                        r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":-32603,"message":"the feed lost segment_ended","data":{{"code":"INTERNAL"}}}}}}"#,
                        id_of(&frame)
                    )
                } else {
                    let result = match method_of(&frame).as_str() {
                        "get_view" => r#"{"next_cursor":"c9","complete":true}"#,
                        "get_segment_feed" => r#"{"events":[],"next_cursor":"f1"}"#,
                        _ => "{}",
                    };
                    answer(id_of(&frame), result, "recap")
                };
                rig.receive(frame.link, &text).expect("reads");
            }
        }
        assert_eq!(asked, 1, "a refusal is not asked again in a loop");
        assert_eq!(rig.recap().refusal, "INTERNAL: the feed lost segment_ended");
        assert_eq!(rig.recap().answers, 0);
    }

    /// The next BMI and the committed spend are kept as they came, and an answer that leaves
    /// one out leaves it absent rather than 0 (decisions-log item 135 (2) (g)).
    #[test]
    fn the_next_bmi_and_the_committed_spend_are_absent_when_the_answer_leaves_them_out() {
        for (result, bmi, committed) in [
            (
                r#"{"treasury_now":200,"bmi_next_dollars":95,"committed_dollars":0}"#,
                Some(95),
                Some(0),
            ),
            (
                r#"{"treasury_now":200,"bmi_next_dollars":95}"#,
                Some(95),
                None,
            ),
            (
                r#"{"treasury_now":200,"committed_dollars":0}"#,
                None,
                Some(0),
            ),
            (r#"{"treasury_now":200}"#, None, None),
        ] {
            let mut rig = in_push();
            let sent = rig.poll(1 + PACER_PERIOD_US);
            let advance = sent
                .iter()
                .find(|frame| frame.link == ADMIN)
                .expect("an advance");
            rig.receive(
                ADMIN,
                &answer(id_of(advance), r#"{"advanced_ms":400}"#, "push"),
            )
            .expect("reads");
            let sent = rig.poll(2 + PACER_PERIOD_US);
            let meter = sent
                .iter()
                .find(|frame| frame.link == SEAT)
                .expect("the meter");
            assert_eq!(method_of(meter), "get_economy_forecast");
            rig.receive(SEAT, &answer(id_of(meter), result, "push"))
                .expect("reads");
            assert_eq!(rig.meter().bmi_next_dollars, bmi, "{result}");
            assert_eq!(rig.meter().committed_dollars, committed, "{result}");
        }
    }

    #[test]
    fn a_resumed_push_is_paced_with_no_lull() {
        // A match resumed from a sealed save: the first footer the rig ever reads says PUSH.
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        rig.opened(SEAT);
        let sent = rig.poll(0);
        let status = sent.first().expect("admin first");
        let view = sent.get(1).expect("seat second");
        let got = rig
            .receive(ADMIN, &answer(id_of(status), "{}", "push"))
            .expect("reads");
        assert!(got.phase_changed);
        assert_eq!(rig.phase(), Phase::Push);
        rig.receive(
            SEAT,
            &answer(
                id_of(view),
                r#"{"next_cursor":"c1","complete":true}"#,
                "push",
            ),
        )
        .expect("reads");
        assert!(rig.set_speed(4));
        settle(&mut rig, 1);
        let sent = rig.poll(1 + PACER_PERIOD_US);
        let admin: Vec<String> = sent
            .iter()
            .filter(|frame| frame.link == ADMIN)
            .map(method_of)
            .collect();
        assert_eq!(
            admin,
            vec!["advance_push".to_owned()],
            "paced from its first answer"
        );
        assert!(
            !sent
                .iter()
                .any(|frame| method_of(frame) == "end_lull"
                    || method_of(frame) == "report_host_clock"),
            "no Lull is waited for or ended: {sent:?}"
        );
        assert!(
            !rig.editor().has_pending_jobs(),
            "the editor waits for the next Lull and asks nothing in the Push"
        );
    }

    // --- S3's `prob`: the typed status read, the Ledger's memos -----------------------

    /// The gateway's `get_status_probation` golden, through its byte copy.
    const STATUS_PROBATION: &str =
        include_str!("../../../godot/fixtures/get_status_probation.json");
    /// The gateway's `recap_probation_round` golden, through its byte copy.
    const RECAP_ROUND: &str = include_str!("../../../godot/fixtures/recap_probation_round.json");
    /// The gateway's `recap_probation_ended` golden, through its byte copy.
    const RECAP_ENDED: &str = include_str!("../../../godot/fixtures/recap_probation_ended.json");

    /// A committed whole answer, renumbered to the call it answers.
    fn fixture_answer(id: i64, fixture: &str) -> String {
        let frame = json::read(fixture).expect("a committed answer");
        let result = frame.get("result").expect("a result").clone();
        json::write(&object(vec![
            ("jsonrpc", Json::String("2.0".to_owned())),
            ("id", number(id)),
            ("result", result),
        ]))
    }

    /// The rig's first admin call, answered with `fixture`.
    fn first_status(rig: &mut Rig, fixture: &str) {
        let sent = rig.poll(0);
        let status = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("the admin's first call");
        assert_eq!(method_of(status), "get_status");
        rig.receive(ADMIN, &fixture_answer(id_of(status), fixture))
            .expect("reads");
    }

    #[test]
    fn the_preset_is_read_from_get_status_and_held_to_what_the_lobby_asked() {
        let mut rig = Rig::new();
        assert_eq!(rig.preset_check(), PresetCheck::NotRead);
        rig.expect_preset(MatchPreset::Probation);
        rig.opened(ADMIN);
        first_status(&mut rig, STATUS_PROBATION);
        assert_eq!(
            rig.status_facts().preset,
            Some(ServedPreset::Hosted(MatchPreset::Probation))
        );
        assert_eq!(
            rig.preset_check(),
            PresetCheck::Agrees(MatchPreset::Probation)
        );
        assert_eq!(
            rig.sealed_this_round(),
            None,
            "the admin's token is no seat's: what it is told of a seal is not taken"
        );

        let mut asked_other = Rig::new();
        asked_other.expect_preset(MatchPreset::Custom);
        asked_other.opened(ADMIN);
        first_status(&mut asked_other, STATUS_PROBATION);
        assert_eq!(
            asked_other.preset_check(),
            PresetCheck::Disagrees {
                asked: MatchPreset::Custom,
                served: MatchPreset::Probation,
            },
            "a hosted match that is not what New match asked for is said, never renamed"
        );

        let mut resumed = Rig::new();
        resumed.opened(ADMIN);
        first_status(&mut resumed, STATUS_PROBATION);
        assert_eq!(
            resumed.preset_check(),
            PresetCheck::Unasked(MatchPreset::Probation),
            "a Resume asks for none: the gateway's word names the match"
        );
    }

    #[test]
    fn every_preset_the_schema_names_has_a_name_and_no_other_does() {
        let mut named = Vec::new();
        for wire in 1..64_i32 {
            if let Ok(preset) = Preset::try_from(wire) {
                named.push(preset.as_str_name().to_ascii_lowercase());
            }
        }
        let ours: Vec<String> = MatchPreset::ALL
            .iter()
            .map(|preset| preset.name().to_owned())
            .collect();
        assert_eq!(
            ours, named,
            "MatchPreset::ALL is gp.api.v1.Preset, in order"
        );
        for preset in MatchPreset::ALL {
            assert_eq!(MatchPreset::of_name(preset.name()), Some(preset));
        }
        assert_eq!(
            MatchPreset::of_name("Probation"),
            None,
            "the wire's spelling only"
        );
        assert_eq!(MatchPreset::of_name(""), None);
    }

    #[test]
    fn the_seat_asks_get_status_when_it_opens_and_keeps_what_it_was_told() {
        let mut rig = in_lull();
        // `in_lull` answered the seat's open-time read with no fields: nothing was told.
        assert_eq!(rig.sealed_this_round(), None);
        assert_eq!(rig.status_facts().preset, Some(ServedPreset::NoMatch));
        assert_eq!(rig.preset_check(), PresetCheck::NoMatch);
        // A reconnect asks again, once, ahead of the panel's reads.
        rig.dropped(SEAT);
        rig.opened(SEAT);
        let mut asked = 0;
        for _ in 0..8 {
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            for frame in rig.poll(1) {
                let method = method_of(&frame);
                let result = match method.as_str() {
                    "get_status" if frame.link == SEAT => {
                        asked += 1;
                        fixture_answer(id_of(&frame), STATUS_PROBATION)
                    }
                    "get_view" => answer(
                        id_of(&frame),
                        r#"{"next_cursor":"c9","complete":true}"#,
                        "lull",
                    ),
                    "get_segment_feed" => {
                        answer(id_of(&frame), r#"{"events":[],"next_cursor":"f1"}"#, "lull")
                    }
                    "report_host_clock" => answer(id_of(&frame), r#"{"all_ready":false}"#, "lull"),
                    _ => answer(id_of(&frame), "{}", "lull"),
                };
                rig.receive(frame.link, &result).expect("reads");
            }
        }
        assert_eq!(asked, 1, "one read at the open, not a poll");
        assert_eq!(rig.sealed_this_round(), Some(true));
        assert_eq!(
            rig.status_facts().preset,
            Some(ServedPreset::Hosted(MatchPreset::Probation))
        );
        assert_eq!(rig.status_facts().failed, None);
    }

    /// **The seal is never told on stale facts** (fix pass 1; the review's probe
    /// `probe_seal_after_accepted_submit_and_refused_status`): an accepted submission, a new
    /// round's Lull, a seat connection opened again and a refused read each clear the fact, so
    /// the Ready line says the client is asking rather than "with nothing sealed".
    #[test]
    fn the_seal_is_forgotten_at_every_event_that_moves_it() {
        let mut rig = editing();
        rig.status_facts.sealed = Some(SealFact {
            round: rig.round,
            phase: rig.phase,
            sealed: false,
        });
        assert_eq!(rig.sealed_this_round(), Some(false));
        // An accepted submission: the old "not sealed" goes at once.
        assert!(rig.editor_mut().submit());
        let sent = rig.poll(3);
        let submit = sent
            .iter()
            .find(|frame| method_of(frame) == "submit_plan")
            .expect("the submission");
        rig.receive(
            SEAT,
            &answer(
                id_of(submit),
                r#"{"report":{"qualifies":true,"depth":"full"},"accepted":true}"#,
                "lull",
            ),
        )
        .expect("reads");
        assert_eq!(
            rig.sealed_this_round(),
            None,
            "an accepted seal: not told yet"
        );
        // The read after it refused: still not told, and asked again.
        let mut asked = 0;
        for step in 0..6_u64 {
            let now = 4 + step * (crate::pacer::KEEPALIVE_US + 1);
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            for frame in rig.poll(now) {
                let text = if method_of(&frame) == "get_status" && frame.link == SEAT {
                    asked += 1;
                    refusal(id_of(&frame), "RATE_LIMITED", "slow down")
                } else {
                    answer(id_of(&frame), r#"{"notes":""}"#, "lull")
                };
                let _ = rig.receive(frame.link, &text);
            }
        }
        assert!(asked >= 2, "a refused read is asked again ({asked})");
        assert_eq!(
            rig.sealed_this_round(),
            None,
            "a refused read tells nothing"
        );
        // A fact standing when a read is refused goes with the refusal.
        rig.status_facts.sealed = Some(SealFact {
            round: rig.round,
            phase: rig.phase,
            sealed: false,
        });
        rig.status_retries = 0;
        rig.reads.status = true;
        let mut refused_now = false;
        for now in 40_000_000_u64..40_000_020 {
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            for frame in rig.poll(now) {
                let text = if method_of(&frame) == "get_status" && frame.link == SEAT {
                    refused_now = true;
                    refusal(id_of(&frame), "RATE_LIMITED", "slow down")
                } else {
                    answer(id_of(&frame), r#"{"notes":""}"#, "lull")
                };
                let _ = rig.receive(frame.link, &text);
            }
            if refused_now {
                break;
            }
        }
        assert!(refused_now, "the status read went out");
        assert_eq!(
            rig.sealed_this_round(),
            None,
            "the refused read took the old fact with it"
        );
        // A seat connection opened again: told afresh.
        rig.status_facts.sealed = Some(SealFact {
            round: rig.round,
            phase: rig.phase,
            sealed: true,
        });
        rig.opened(SEAT);
        assert_eq!(rig.sealed_this_round(), None, "a reconnect: told afresh");
    }

    #[test]
    fn a_refused_or_unreadable_status_is_said_and_the_last_facts_stand() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        first_status(&mut rig, STATUS_PROBATION);
        let answers = rig.status_facts().answers;
        // The seat's open-time read refused: said, and the preset read before still stands.
        rig.opened(SEAT);
        let mut refused = 0;
        // In one instant: the open-time read alone, never a retry back to back (fix pass 2).
        let mut now = 1_u64;
        let serve = |rig: &mut Rig, now: u64, refused: &mut usize| {
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            for frame in rig.poll(now) {
                let text = match method_of(&frame).as_str() {
                    "get_status" if frame.link == SEAT => {
                        *refused += 1;
                        refusal(id_of(&frame), "RATE_LIMITED", "slow down")
                    }
                    "get_view" => answer(
                        id_of(&frame),
                        r#"{"next_cursor":"c9","complete":true}"#,
                        "lull",
                    ),
                    "get_segment_feed" => {
                        answer(id_of(&frame), r#"{"events":[],"next_cursor":"f1"}"#, "lull")
                    }
                    "report_host_clock" => answer(id_of(&frame), r#"{"all_ready":false}"#, "lull"),
                    _ => answer(id_of(&frame), "{}", "lull"),
                };
                rig.receive(frame.link, &text)
                    .expect("a refusal is an answer");
            }
        };
        for _ in 0..40 {
            serve(&mut rig, now, &mut refused);
        }
        assert_eq!(refused, 1, "no retry back to back");
        assert!(!rig.seal_not_told(), "a retry is owed");
        // Each retry once the link has been quiet for the keep-alive interval, up to the bound;
        // then the line says the gateway's refusal, not that the client is asking.
        for _ in 0..usize::from(STATUS_RETRIES) {
            now = now
                .saturating_add(crate::pacer::KEEPALIVE_US)
                .saturating_add(1);
            for _ in 0..40 {
                serve(&mut rig, now, &mut refused);
            }
        }
        assert_eq!(
            refused,
            1 + usize::from(STATUS_RETRIES),
            "a refused read is asked again a few times, spaced"
        );
        assert!(
            rig.seal_not_told(),
            "the retries are spent: not told, and said"
        );
        assert_eq!(rig.sealed_this_round(), None, "not told");
        assert_eq!(
            rig.status_facts().failed,
            Some(ReadFailed::Refused("RATE_LIMITED: slow down".to_owned()))
        );
        assert_eq!(
            rig.status_facts().preset,
            Some(ServedPreset::Hosted(MatchPreset::Probation))
        );
        assert_eq!(rig.status_facts().answers, answers);

        // A preset number the schema has no name for cannot be read, and is said.
        let mut unreadable = Rig::new();
        unreadable.opened(ADMIN);
        let sent = unreadable.poll(0);
        let status = sent.first().expect("the first call");
        let read = unreadable.receive(ADMIN, &answer(id_of(status), r#"{"preset":9}"#, "lull"));
        assert!(read.is_err(), "{read:?}");
        assert!(
            matches!(
                unreadable.status_facts().failed,
                Some(ReadFailed::Unreadable(_))
            ),
            "{:?}",
            unreadable.status_facts()
        );
        assert_eq!(unreadable.status_facts().preset, None);
    }

    /// The recap's memos, from the gateway's golden of a Probation round: in issue order, each
    /// with its id as the wire spells it and its text as sent.
    #[test]
    fn the_recap_carries_the_ledgers_memos_as_the_gateway_issued_them() {
        let rig = Rig::new();
        let result = json::read(RECAP_ROUND)
            .expect("json")
            .get("result")
            .expect("a result")
            .clone();
        let recap = rig.read_recap(&result).expect("the golden reads");
        assert_eq!(
            recap.memos,
            vec![MemoRow {
                id: "first_licence".to_owned(),
                text: "Notice of licence. You ran Generators on two vents at settlement. You \
                       may now queue a Resonance Spire at any of your beacons."
                    .to_owned(),
            }]
        );
        assert!(
            recap.prose.ends_with("Licence awarded: Resonance Spire."),
            "the licence line is the prose's own sentence, kept whole: {}",
            recap.prose
        );
        let ended = json::read(RECAP_ENDED)
            .expect("json")
            .get("result")
            .expect("a result")
            .clone();
        let ended = rig.read_recap(&ended).expect("the golden reads");
        assert!(
            ended.memos.is_empty(),
            "the match's end carries none: {ended:?}"
        );
        assert!(
            ended.prose.ends_with(
                "The licence that stands at the match's end: seat 0's Resonance Spire, from round 1."
            ),
            "the gateway's final words name every standing award: {}",
            ended.prose
        );
        let custom = rig
            .read_recap(&json::read(r#"{"prose":"p","_status":{"round":1}}"#).expect("json"))
            .expect("reads");
        assert!(custom.memos.is_empty(), "a custom match is sent none");
    }

    #[test]
    fn a_memo_that_names_no_memo_is_refused() {
        let rig = Rig::new();
        let text = r#"{"prose":"p","ledger_memos":[{"text":"t"}],"_status":{"round":1}}"#;
        let read = rig.read_recap(&json::read(text).expect("json"));
        assert!(read.is_err(), "{read:?}");
    }

    /// One answer with `extra` fields before the footer, served in `phase` of `round`.
    fn served(id: i64, extra: &str, phase: &str, round: u32) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{{extra}"_status":{{"phase":"{phase}","phase_remaining_ms":180000,"round":{round}}}}}}}"#
        )
    }

    /// Drives the rig through `schedule` (one admin phase and round a step), answering every
    /// call as a gateway would: the seat's `get_status` tells `sealed(round)`. Returns how many
    /// seat `get_status` calls went out in each round, and what the rig said of the seal after
    /// each step.
    fn drive(
        rig: &mut Rig,
        schedule: &[(&str, u32)],
        sealed: &dyn Fn(u32) -> bool,
    ) -> (BTreeMap<u32, u32>, Vec<Option<bool>>) {
        let mut asks: BTreeMap<u32, u32> = BTreeMap::new();
        let mut seen = Vec::new();
        // From the rig's own clock, so a second drive carries on where the first left off.
        let mut now = rig.timing.now_us();
        for &(phase, round) in schedule {
            // A step is longer than the clock's report interval, so the admin connection asks
            // at every step and the phase the schedule names reaches the rig.
            now += CLOCK_REPORT_US + 50_000;
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            for frame in rig.poll(now) {
                let method = method_of(&frame);
                let extra = match method.as_str() {
                    "get_status" if frame.link == SEAT => {
                        *asks.entry(round).or_insert(0) += 1;
                        format!(
                            r#""preset":"probation","sealed_this_round":{},"#,
                            sealed(round)
                        )
                    }
                    "get_status" => r#""preset":"probation","#.to_owned(),
                    "get_view" => r#""next_cursor":"c9","complete":true,"#.to_owned(),
                    "get_segment_feed" => r#""events":[],"next_cursor":"f1","#.to_owned(),
                    "report_host_clock" => r#""all_ready":false,"#.to_owned(),
                    "get_recap" => r#""prose":"p","#.to_owned(),
                    _ => String::new(),
                };
                rig.receive(frame.link, &served(id_of(&frame), &extra, phase, round))
                    .expect("reads");
            }
            seen.push(rig.sealed_this_round());
        }
        (asks, seen)
    }

    /// The review's probe P1, as the behaviour it asks for: a seal the gateway told in round
    /// 1 is round 1's, and in round 2's Lull the rig neither carries it nor guesses: it reads
    /// "not told" until round 2's own answer, which it asks for at the Lull's opening.
    #[test]
    fn the_seats_seal_is_its_rounds_and_is_asked_again_at_every_lull() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        rig.opened(SEAT);
        let schedule: Vec<(&str, u32)> = (0..12)
            .map(|_| ("lull", 1))
            .chain((0..12).map(|_| ("push", 1)))
            .chain((0..12).map(|_| ("lull", 2)))
            .collect();
        let (asks, seen) = drive(&mut rig, &schedule, &|round| round == 1);
        assert_eq!(
            asks.get(&2).copied(),
            Some(1),
            "round 2's Lull asks once: {asks:?}"
        );
        assert_eq!(rig.sealed_this_round(), Some(false), "round 2's own answer");
        assert!(
            seen.get(11).is_some_and(|told| *told == Some(true)),
            "round 1's Lull was told its seal: {seen:?}"
        );
        assert!(
            seen.get(12..24)
                .is_some_and(|push| push.iter().all(Option::is_none)),
            "the Push moved past the Lull's answer, so nothing is claimed: {seen:?}"
        );
    }

    /// **The status reads' retries are a round's phase's** (fix pass 2; review 2's mutant R6): a
    /// round whose retries were spent does not carry the count into the next round, whose Lull
    /// asks its seal afresh and is not "not told".
    #[test]
    fn spent_retries_do_not_carry_into_the_next_round() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        rig.opened(SEAT);
        let schedule: Vec<(&str, u32)> = (0..8).map(|_| ("lull", 1)).collect();
        let _ = drive(&mut rig, &schedule, &|_| false);
        rig.status_facts.sealed = None;
        rig.status_retries = STATUS_RETRIES;
        rig.status_retry_owed = false;
        rig.reads.status = false;
        assert!(rig.seal_not_told(), "round 1's retries are spent");
        let push: Vec<(&str, u32)> = (0..12).map(|_| ("push", 1)).collect();
        let _ = drive(&mut rig, &push, &|_| false);
        // Round 2's Lull, every seat status read refused: its first refusal is retried, because
        // round 1's spent count does not carry into round 2.
        let mut refused = 0;
        let mut now = rig.timing.now_us();
        for _ in 0..12 {
            now += CLOCK_REPORT_US + 50_000;
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            for frame in rig.poll(now) {
                let text = match method_of(&frame).as_str() {
                    "get_status" if frame.link == SEAT => {
                        refused += 1;
                        refusal(id_of(&frame), "RATE_LIMITED", "slow down")
                    }
                    "get_status" => served(id_of(&frame), r#""preset":"probation","#, "lull", 2),
                    "get_view" => served(
                        id_of(&frame),
                        r#""next_cursor":"c9","complete":true,"#,
                        "lull",
                        2,
                    ),
                    "get_segment_feed" => served(
                        id_of(&frame),
                        r#""events":[],"next_cursor":"f1","#,
                        "lull",
                        2,
                    ),
                    "report_host_clock" => {
                        served(id_of(&frame), r#""all_ready":false,"#, "lull", 2)
                    }
                    _ => served(id_of(&frame), "", "lull", 2),
                };
                let _ = rig.receive(frame.link, &text);
            }
        }
        assert_eq!(rig.round, 2);
        assert!(refused >= 1, "round 2's read went out");
        assert!(
            !rig.seal_not_told(),
            "round 2's refused read is retried, not given up on round 1's count ({refused} refused)"
        );
    }

    /// The seat's `get_status` over a five-round match: once when the seat connection opens
    /// (which is round 1's Lull), once at each later Lull's opening, and no more (the rate
    /// budget's rows).
    #[test]
    fn the_seats_status_reads_over_a_five_round_match() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        rig.opened(SEAT);
        let mut schedule: Vec<(&str, u32)> = Vec::new();
        for round in 1..=5 {
            schedule.extend((0..6).map(|_| ("lull", round)));
            schedule.extend((0..6).map(|_| ("push", round)));
            schedule.extend((0..6).map(|_| ("recap", round)));
        }
        let (asks, _) = drive(&mut rig, &schedule, &|_| false);
        let expected: BTreeMap<u32, u32> = (1..=5).map(|round| (round, 1)).collect();
        assert_eq!(asks, expected);
    }

    /// An accepted `submit_plan` of this seat asks the gateway once for the seal: the rig never
    /// sets it on its own word.
    #[test]
    fn an_accepted_submission_asks_for_the_seal_and_never_infers_it() {
        let mut rig = in_lull();
        settle(&mut rig, 0);
        assert!(rig.editor_mut().load(b"{}"));
        let mut submitted = false;
        let mut asked_after = 0;
        for step in 0..24_u64 {
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            if step == 2 {
                assert!(rig.editor_mut().submit());
                submitted = true;
            }
            for frame in rig.poll(step) {
                let method = method_of(&frame);
                let result = match method.as_str() {
                    "get_status" if frame.link == SEAT => {
                        if submitted {
                            asked_after += 1;
                        }
                        r#"{"preset":"probation"}"#.to_owned()
                    }
                    "submit_plan" => {
                        assert_eq!(rig.sealed_this_round(), None);
                        r#"{"accepted":true,"report":{"qualifies":true,"depth":"full","diagnostics":[]}}"#
                            .to_owned()
                    }
                    "verify_plan" => {
                        r#"{"report":{"qualifies":true,"depth":"quick","diagnostics":[]}}"#
                            .to_owned()
                    }
                    "get_view" => r#"{"next_cursor":"c9","complete":true}"#.to_owned(),
                    "get_segment_feed" => r#"{"events":[],"next_cursor":"f1"}"#.to_owned(),
                    "report_host_clock" => r#"{"all_ready":false}"#.to_owned(),
                    "estimate_route" => r#"{"reachable":true}"#.to_owned(),
                    "render_plan" => r#"{"prose":"P"}"#.to_owned(),
                    "list_drafts" => r#"{"drafts":[]}"#.to_owned(),
                    "list_templates" => r#"{"templates":[]}"#.to_owned(),
                    "get_vocabulary" => r#"{"families":[]}"#.to_owned(),
                    "get_map_summary" => r#"{"features":[]}"#.to_owned(),
                    "resolve_refs" => r#"{"refs":[]}"#.to_owned(),
                    "get_briefing" => r#"{"notes":""}"#.to_owned(),
                    _ => "{}".to_owned(),
                };
                let _ = rig.receive(frame.link, &answer(id_of(&frame), &result, "lull"));
            }
        }
        assert!(submitted);
        assert_eq!(asked_after, 1, "one read after the accepted submission");
        assert_eq!(
            rig.sealed_this_round(),
            None,
            "the answer told nothing, and the rig does not fill it in"
        );
    }
}
