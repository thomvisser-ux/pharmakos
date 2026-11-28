// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host loop: one config line in, one announce line out, one match behind
//! many connections.
//!
//! Spec section 15 makes the Godot client a JSON-RPC client over a loopback
//! WebSocket and the gateway the in-process host of the sim, and names no
//! process for either. Decisions-log item 107 (2) names one: **the lobby
//! spawns a child**, all the logic is here, and the binary is `gamectl host`
//! -- about thirty lines, a named place granted in the wave's second run. That
//! division is why this module reads no file and takes no path in its config
//! line: the *binary* reads the rules file and hands over its text, so
//! `std::fs` stays in [`crate::cache`] and `gamectl` needs no `pharmakos-sim`
//! type to host a match.
//!
//! # The two lines
//!
//! **Config, in**, one line on the child's standard input, tab separated:
//!
//! ```text
//! <match id>\t<seed>\t<seats>\t<human seat|->\t<segment lengths|->\t<round limit>\t<preset>[\tresume]
//! ```
//!
//! The seventh field is the match's **preset**, `probation` or `custom` (S3's
//! plan, E6; [`crate::probation`]), and it is required: a line without it --
//! one a lobby remembered before S3 -- is refused with a typed error naming
//! it, and the lobby offers a fresh match rather than guessing (no silent
//! default). The eighth field is optional and has one value, `resume`
//! (decisions-log item 111, decision C9): go on with the match saved under
//! this id rather than start a new one. A resume line repeats **all seven**
//! of the saved match's values, the round limit and the preset included, and
//! one that differs is refused as [`crate::error::Code::InvalidArgument`]
//! (item 112 (8)); so is a rules table, verifier or snapshot format that is
//! not the save's. [`ConfigLine`] reads it.
//!
//! **Announce, out**, one line on the child's standard output, tab separated:
//!
//! ```text
//! <port>\t<match id>\t<admin token>\t<human seat token|->
//! ```
//!
//! No token touches argv, a console or the disk. A reader ignores any field
//! beyond the ones it knows, so the format can grow a field without a new
//! version; it is deliberately **not** `gp.api.v1`, because it is two lines
//! between a parent and its own child and freezing it into the published
//! schema would be freezing the wrong thing.
//! PLACEHOLDER: whether the config and announce lines become part of anything published — owner, at v1.1.
//!
//! # Death
//!
//! The parent holds the child's standard input open for the life of the match.
//! End of file ends the host: [`run`] returns when `control` does. There is no
//! heartbeat and no clock, because there is nothing here to time.
//!
//! # Saves (decisions-log item 84; the wave-6 notes, decision C8)
//!
//! Written here, through [`crate::cache`] alone, at the two Lull boundaries:
//! at `begin_push`, once every seal is final and before the first tick (the
//! surface builds it into a pending slot and this loop flushes it after the
//! job that ended the Lull, the pattern the audit log already uses); and when
//! the control pipe reaches end of file **during a Lull**. Never mid-Push: end
//! of file in a Push, a recap or an ended match writes no save, and the one
//! made when that Push began stands. With the save go the private replay's
//! inputs ([`crate::save`]): each seat's seal at every `begin_push`, and each
//! segment's hash chain at its end.
//!
//! **A boundary is durable before anyone is told it happened.** The call
//! that crossed it (the Push's start, a segment's end, the match's end) is
//! answered only once what it left is on disk; a write that fails answers
//! that call, and every call after it, with a typed `INTERNAL` that says what
//! could not be written, why, and what the disk holds (read back from the
//! cache), and the host stops: [`run`] returns it while the pipe is still
//! open, and so does a Lull save that fails as the pipe closes. `Writer`
//! says the rule in full.
//!
//! A save outlives its process, so a match id with a save is refused for a new
//! match ([`crate::cache::MatchCache::open`]); the resume line is the way back
//! in. [`run`] reads the save **before** it opens the surface (the wave-6
//! notes, H16), and a resumed match is announced with new tokens, as spec
//! section 3 requires ("seat tokens are reissued").
//!
//! # Why answering in arrival order is safe (AGENTS.md section 4.6)
//!
//! Several connections send requests and one thread answers them, so the order
//! two connections' calls are answered in is the order they arrived in, which
//! is not reproducible. That is allowed, and here is the argument rather than
//! the assertion:
//!
//! * **Nothing the order decides is hashed.** Orders are applied at
//!   `begin_push`, in ascending seat id, *all of them or none*, from a store
//!   whose contents do not depend on the order the submissions arrived in --
//!   the latest verified submission replaces the previous one, which is a
//!   last-writer rule and not an accumulation. The audit log and the rate
//!   limiter are per token and are not hashed at all.
//! * **Every byte-compared golden is produced through a single-threaded
//!   path.** `Surface::call` is the one door, and the goldens drive it
//!   directly.
//! * **There is exactly one race and it is documented rather than removed:**
//!   `submit_plan` against `end_lull`. Whichever arrives first wins, and the
//!   answer is "the admin's word wins" -- a seat whose submission lands after
//!   the Lull has been ended is told `PHASE_CLOSED`, which is the honest
//!   answer to "I was too late".
//!
//! # The seats this process plays itself, and the seats it advises
//!
//! Two seams, both handed a closure that **is** [`Surface::call`] bound to a
//! token this process minted for itself and never lets out of it (no
//! announce line, no file, no argv):
//!
//! * a [`BuiltInSeat`] plays a seat nobody at this machine plays: it plans,
//!   submits and says it is ready, like any client (spec section 14's
//!   operator, which is **T18**'s);
//! * an [`Advisor`] advises a seat the operator does **not** play -- the
//!   human's -- at the start of every Lull: that seat's own safe playbook and
//!   one suggestion per template for the editor's wizard (decisions-log item
//!   111, decisions C2 and C5). Its token holds `observe`, `docs` and `plan`
//!   and never `plan.submit`, and on top of the scopes this module keeps a
//!   **method allow-list** ([`ADVISOR_METHODS`]): `save_notes`, `save_draft`,
//!   `list_drafts`, `get_draft`, `submit_plan` and `set_ready` -- every
//!   write, and the drafts -- are answered `FORBIDDEN_SCOPE` and audited,
//!   whatever the token's scopes would allow. What it advises is filed host-side by
//!   [`Surface::file_advice`], which no method handler reaches.
//!
//! Both are built by the [`Operators`] factory **after** the config line is
//! read, because only then is it known which seats exist and which one is
//! the human's (item 111, H11): one [`BuiltInSeat`] per seat the factory
//! plays, one [`Advisor`] per other seat, each a fresh instance. The skeleton
//! ships [`NoOperators`], which plays and advises nobody, so every seat that
//! seals nothing is filed the gateway's fallback safe playbook.
//!
//! Both kinds of token go through the same door, the same audit and the same
//! fog as a socket, **with their own rate limit**
//! ([`crate::limit::IN_PROCESS_LIMITS`], decision C6): the host plans them
//! synchronously on one Lull tick, which a socket's
//! [`crate::limit::CALLS_PER_TICK`] would cut short.
//!
//! # The external seat (decisions-log item 231)
//!
//! [`External`] hosts a match **with no socket**, for one seat played by a
//! program outside the game: the config line's `human seat`. It is this host
//! loop with the listener, the threads and the lobby taken away, and it shares
//! everything that is left: the match is opened and resumed by the same
//! function ([`open_match`]), the other seats are planned by the same
//! [`InProcessSeats`], and the saves, the sealed playbooks, the chains and the
//! audit log are written by the same [`Writer`] under the same rule.
//!
//! What differs, and why each is safe:
//!
//! * **No token leaves the gateway, and none is minted that could move the
//!   match.** The seat's token is a private field of [`External`] with no
//!   accessor, so the binary that drives it (`gamectl match`) holds none and
//!   can print none. There is no admin token: the match moves by
//!   [`External::advance`], a host-side call no method reaches, and only once
//!   the outside seat has said it is ready. The three boundaries it crosses
//!   are the `headless` module's, the one place beside the admin handlers
//!   that may name a driving method; this file names none.
//! * **The seat's rate is its own**, [`crate::limit::EXTERNAL_LIMITS`], for
//!   the reason an in-process seat's is: a headless Lull has no timer and
//!   reports no clock, so every call of a Lull lands on one tick. It is a
//!   rate and never a read.
//! * **A Lull lasts until the seat says it may end.** No clock is read or
//!   reported, so nothing here depends on how fast the machine or the program
//!   is, and the Lull's length is not an input to anything hashed. The Push
//!   is stepped to its segment's end in one go, as `gamectl scenario run`
//!   steps it.
//! * **A message is a line or a call's text, not a WebSocket frame**, so the
//!   refusals a frame's assembler makes are made by [`External::call_bytes`]
//!   instead: a message past the largest, and one that is not UTF-8, are
//!   answered and audited, never repaired and never dropped.
//!
//! What does not differ is the door: every call the outside seat makes is
//! [`Surface::call`] under a seat's scopes, with the same fog, the same phase
//! checks and the same audit line as a socket's.
//!
//! # Bounds
//!
//! One request in flight per connection ([`crate::session`] waits for each
//! answer before reading the next frame), a bounded queue in front of the
//! surface thread, and at most [`MAX_CONNECTIONS`] live connections with an
//! audited 503 beyond that. A flood of silent sockets therefore fills the
//! pending slots, times out and disturbs nothing that has authenticated --
//! and, said plainly because the cap counts sockets rather than sessions, it
//! *does* keep a new client out for as long as it keeps them open.
//! [`MAX_CONNECTIONS`] carries that sentence and the two ways out of it.

use pharmakos_proto::json::grammar::Said;
use pharmakos_proto::said;
use std::cell::Cell;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::sync::{Arc, OnceLock};

use pharmakos_proto::json::Json;
use pharmakos_sim::tables::SeatId;

use pharmakos_proto::gp::api::v1::Method;

use crate::advice::Advice;
use crate::cache::MatchCache;
use crate::error::Error;
use crate::fog::FogPolicy;
use crate::host::{Host, LiveVision, Settings};
use crate::net::Listener;
use crate::probation::Preset;
use crate::rpc::{self, Request};
use crate::save::{Save, Stamp};
use crate::scopes::{Scope, ScopeSet};
use crate::session::{self, Door, Opened};
use crate::surface::{InProcess, Surface};
use crate::token::{Handle, Subject, Token};

/// How many connections the host serves at once.
///
/// PLACEHOLDER: eight connections at once — owner, at hardening, with the rate limits.
/// 8 is a working number with a technical reason and no
/// measurement. **A connection costs a thread**, because the transport is
/// blocking threads and `std::sync` channels (decisions-log item 99) and not
/// an async runtime; eight covers three seats, an editor, a lobby, a spectator
/// and two spare, which is more than the whole of v1 has clients.
///
/// The honest statement of what the cap does and does not do: a slot is taken
/// at **accept** and given back when the connection's thread returns, so a
/// socket that opens and then says nothing holds its slot for the whole of
/// `session::READ_TIMEOUT` (30 s). Eight silent sockets therefore deny *new*
/// connections for as long as a local process cares to keep reopening them.
/// What they cannot do is disturb a connection that has already
/// authenticated, which is what the bound is for here and what
/// `a_flood_of_silent_connections_cannot_disturb_a_live_session` pins. The
/// fix, if the owner wants one, is to count the cap over authenticated
/// connections or to give the handshake a shorter timeout of its own.
/// **OWNER**, at hardening, with `READ_TIMEOUT`, `MAX_MESSAGE_BYTES` and the
/// rate limits.
pub const MAX_CONNECTIONS: usize = 8;

/// Why the host hung up on a connection before serving it (S3's `vfyq2`,
/// fix pass 3, item L6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropReason {
    /// [`MAX_CONNECTIONS`] were already being served.
    OverCapacity,
    /// The operating system would not make the connection's thread.
    NoConnectionThread,
}

impl DropReason {
    /// Every reason, in the order the audit log closes with them.
    pub const ALL: [DropReason; 2] = [DropReason::OverCapacity, DropReason::NoConnectionThread];

    /// Its name in the audit log.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            DropReason::OverCapacity => "over_capacity",
            DropReason::NoConnectionThread => "no_connection_thread",
        }
    }
}

/// Every connection one host dropped, by reason: counted whether or not the
/// audit queue had room for the refusal's own line, so a flood that fills the
/// queue is still counted (AGENTS.md: every drop is counted under an
/// enumerated reason code). The dispatcher counts; the surface thread writes
/// the counts into the audit log it closes, one line per reason, which is
/// what an operator of the host reads (S3's `vfyq2`, fix pass 4, item L-f).
#[derive(Default, Debug)]
struct Drops {
    over_capacity: AtomicU64,
    no_connection_thread: AtomicU64,
}

impl Drops {
    /// The counter for `reason`.
    const fn counter(&self, reason: DropReason) -> &AtomicU64 {
        match reason {
            DropReason::OverCapacity => &self.over_capacity,
            DropReason::NoConnectionThread => &self.no_connection_thread,
        }
    }

    /// Counts one connection dropped for `reason`. A host that drops
    /// `u64::MAX` connections wraps rather than panics in the accept loop;
    /// at one a nanosecond that is five centuries.
    fn count(&self, reason: DropReason) {
        self.counter(reason).fetch_add(1, Ordering::SeqCst);
    }

    /// How many connections this host has dropped for `reason`.
    fn of(&self, reason: DropReason) -> u64 {
        self.counter(reason).load(Ordering::SeqCst)
    }

    /// One audit line per reason, zero included, so a log that says nothing
    /// was dropped says so rather than leaving it to be inferred.
    fn record(&self, surface: &mut Surface) {
        let tick = surface.time().tick;
        for reason in DropReason::ALL {
            surface.audit().record(
                tick,
                None,
                None,
                format!("connections dropped {}: {}", reason.name(), self.of(reason)),
                crate::audit::Outcome::Ok,
            );
        }
    }
}

/// How many requests may wait in front of the surface thread.
///
/// One per connection plus one, so a connection can always hand its single
/// in-flight request over without the sender blocking on a queue that another
/// connection filled.
const QUEUE_DEPTH: usize = MAX_CONNECTIONS + 1;

/// What the connection cap answers before the WebSocket upgrade.
const OVER_CAPACITY: &str =
    "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";

/// A seat this process plays itself.
///
/// The seam **T18** fills. A built-in seat is an ordinary client of this
/// gateway and this trait is what makes that literally true: `plan` is handed
/// a closure that *is* `Surface::call` bound to that seat's own minted token,
/// so the operator goes through **the same door as a socket**: the same
/// authentication, scopes and phase checks, **the same audit** and **the same
/// fog**. It never sees a token, the surface or a `pharmakos-sim` type.
///
/// **Its rate is its own**, and that is the one difference, stated rather than
/// hidden (decisions-log item 111, decision C6): its token is counted against
/// [`crate::limit::IN_PROCESS_LIMITS`] rather than a socket's
/// [`crate::limit::CALLS_PER_TICK`], because the host plans it synchronously
/// on one Lull tick, where a socket's limit would refuse its ninth call. It is
/// a rate, never a read: nothing it may see differs from what a socket holding
/// the same scopes may see.
///
/// A seat that files nothing gets the safe playbook at `begin_push`, which the
/// surface already does. An implementation that wants the Lull to end on "all
/// ready" must end by calling `set_ready`; one whose own plan fails its
/// repairs submits its own safe playbook through `submit_plan`.
pub trait BuiltInSeat: Send {
    /// Plan this seat's round.
    fn plan(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json);
}

/// The built-in operator advising a seat it does not play.
///
/// Decisions-log item 111, decisions C2 and C5, and the owner's question D3
/// taken on the recommendation. Called at the start of **every** Lull for each
/// seat with no [`BuiltInSeat`] -- the human's -- with a closure that is
/// `Surface::call` bound to that seat's in-process advisor token: `observe`,
/// `docs` and `plan`, never `plan.submit`, and behind [`ADVISOR_METHODS`] on
/// top of that. What it returns is filed by [`Surface::file_advice`]: the
/// seat's own safe playbook, verified FULL and compiled there, and one
/// suggestion per template for `instantiate_template{suggested: true}`.
///
/// Its `get_view` is answered from a scratch copy of the view feed, so it
/// reads what the seat may see and leaves the human's own feed state exactly
/// as it was (H15; [`crate::surface::InProcess::scratch_view`]). `get_briefing`
/// still carries the seat's notebook: the operator ignores it (spec section
/// 14), which is the operator's test to keep, and the read is the one the
/// design admits.
pub trait Advisor: Send {
    /// Advise this seat for the round the Lull has just opened.
    fn advise(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json) -> Advice;
}

/// What the host asks for each seat once the config line is read: the
/// factory the built-in operator plugs into (decisions-log item 111, H11).
///
/// Called once per seat and never again, in ascending seat id: [`Operators::built_in`]
/// for every seat that is not the human's, then [`Operators::advisor`] for
/// every seat that got no built-in operator, the human's included. Each
/// answer must be a **fresh instance with no state shared** with any other
/// seat's, because each is a client of its own seat and nobody else's.
pub trait Operators: Send {
    /// The operator that plays `seat`, or `None` to leave it to the safe
    /// playbook the gateway files. Never asked for the human's seat.
    fn built_in(&mut self, seat: u8) -> Option<Box<dyn BuiltInSeat>>;

    /// The operator that advises `seat`, or `None` for no advice: the seat is
    /// then shown and filed the gateway's fallback safe playbook, and its
    /// wizard is pre-filled with the templates' own values.
    fn advisor(&mut self, seat: u8) -> Option<Box<dyn Advisor>>;
}

/// The skeleton's factory until **T18**: it plays nobody and advises nobody.
///
/// A match hosted with it is a match every seat that seals nothing plays on
/// the gateway's fallback safe playbook, which is what `gamectl host` did
/// before the seams existed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NoOperators;

impl Operators for NoOperators {
    fn built_in(&mut self, _seat: u8) -> Option<Box<dyn BuiltInSeat>> {
        None
    }

    fn advisor(&mut self, _seat: u8) -> Option<Box<dyn Advisor>> {
        None
    }
}

/// The methods an [`Advisor`] may call, and nothing else.
///
/// **A host-side allow-list**, held here rather than in a handler, because it
/// is about who is calling rather than about the method (decisions-log item
/// 111, decision C5). The five the design names -- `save_notes`,
/// `save_draft`, `list_drafts`, `submit_plan` and `set_ready` -- are off it,
/// and so is `get_draft` (item 112 (5)): an advisor writes nothing into the
/// seat's store and never reads the seat's drafts. So are the four `admin` methods, which its scopes refuse anyway,
/// and the four the schema gives no request and response pair
/// (`query_area`, `list_known_enemies`, `get_reports`, `get_capabilities`),
/// which no client is served. Every other method a seat's `observe`, `docs`
/// and `plan` reach is on it, `get_segment_feed` included: the feed is read
/// through an opaque cursor the caller holds and keeps no per-viewer state in
/// the gateway, so an advisor reading it (the recap's events, say) leaves the
/// human's feed as it found it. `get_beacon` is on it with its S3 detail (a
/// writ, its settings, a Build beacon's targets): that detail is world state,
/// what the sim holds for the seat's own beacon, and not what the seat wrote,
/// so it reads none of the seat's drafts or playbooks (item 111's line
/// stands). Anything off it is answered `FORBIDDEN_SCOPE`
/// and audited (`an_advisor_is_refused_every_method_off_its_allow_list`).
pub const ADVISOR_METHODS: [Method; 20] = [
    Method::GetStatus,
    Method::WaitFor,
    Method::GetBriefing,
    Method::GetRecap,
    Method::ListBeacons,
    Method::GetBeacon,
    Method::GetMapSummary,
    Method::GetEconomyForecast,
    Method::EstimateRoute,
    Method::GetSchema,
    Method::GetVocabulary,
    Method::ListTemplates,
    Method::InstantiateTemplate,
    Method::VerifyPlan,
    Method::RenderPlan,
    Method::PatchPlan,
    Method::GetSafePlan,
    Method::ResolveRefs,
    Method::GetView,
    Method::GetSegmentFeed,
];

/// What the binary hands the host loop.
pub struct Setup {
    /// The rules table's canonical JSON, as the binary read it off disk. Text,
    /// not a path and not a parsed table: see the module docs.
    pub rules_json: String,
    /// The template library folder, when the binary was given one.
    pub library: Option<PathBuf>,
    /// The factory the seats this process plays or advises come from, asked
    /// once the config line has been read ([`NoOperators`] in the skeleton).
    pub operators: Box<dyn Operators>,
}

impl std::fmt::Debug for Setup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Setup")
            .field("rules_json", &self.rules_json.len())
            .field("library", &self.library)
            .finish_non_exhaustive()
    }
}

/// The lobby's config line, once it has been read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Config {
    /// The match id, which is also the private cache's folder name.
    pub match_id: String,
    /// The match seed, which is also the map seed.
    pub seed: u64,
    /// How many seats the match has.
    pub seats: u32,
    /// Which seat the person at this machine plays, if any.
    pub human_seat: Option<u8>,
    /// The Push lengths in game milliseconds, one per round. Empty is the
    /// rules table's own ladder.
    pub segment_lengths_ms: Vec<i32>,
    /// How many rounds the match runs at most.
    pub round_limit: u32,
    /// The preset the match is played under (S3's plan, E6).
    pub preset: Preset,
}

impl Config {
    /// Read one config line's first seven fields.
    ///
    /// An eighth field is not read here: [`ConfigLine::parse`] reads it, and
    /// is what the host loop uses.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a line with too few fields
    /// or a field this host cannot read, and for a seat count outside `1` to
    /// [`crate::host::MAX_SEATS`]. A line with no preset -- six fields, or six
    /// and `resume`, as a lobby wrote before S3 -- is refused naming the
    /// field. Nothing here is silently defaulted: the lobby is a program, and
    /// a host that guessed what it meant would start the wrong match.
    pub fn parse(line: &str) -> Result<Config, Error> {
        let fields: Vec<&str> = line.trim_end_matches(['\r', '\n']).split('\t').collect();
        let field = |index: usize, name: &str| -> Result<String, Error> {
            fields
                .get(index)
                .map(|text| (*text).to_owned())
                .ok_or_else(|| {
                    Error::invalid(said!(
                        "the config line has no field {} (`{}`); it is seven \
                         tab-separated fields",
                        index,
                        pharmakos_proto::json::grammar::quoted(name)
                    ))
                })
        };
        let match_id = field(0, "match id")?;
        let seed_text = field(1, "seed")?;
        let seed = seed_text
            .strip_prefix("0x")
            .and_then(|digits| u64::from_str_radix(digits, 16).ok())
            .or_else(|| seed_text.parse::<u64>().ok())
            .ok_or_else(|| {
                Error::invalid(said!(
                    "`{}` is not a match seed: `0x…` or decimal",
                    pharmakos_proto::json::grammar::quoted(&seed_text)
                ))
            })?;
        let seats = field(2, "seats")?
            .parse::<u32>()
            .map_err(|_| Error::invalid("`seats` is how many seats the match has"))?;
        let human = field(3, "human seat")?;
        let human_seat = if human == "-" {
            None
        } else {
            Some(
                human
                    .parse::<u8>()
                    .map_err(|_| Error::invalid("`human seat` is a seat number or `-`"))?,
            )
        };
        let lengths = field(4, "segment lengths")?;
        let segment_lengths_ms = if lengths == "-" || lengths.is_empty() {
            Vec::new()
        } else {
            let mut found: Vec<i32> = Vec::new();
            for piece in lengths.split(',') {
                found.push(piece.parse::<i32>().map_err(|_| {
                    Error::invalid(said!(
                        "`{}` is not a segment length in game milliseconds",
                        pharmakos_proto::json::grammar::quoted(piece)
                    ))
                })?);
            }
            found
        };
        let round_limit = field(5, "round limit")?
            .parse::<u32>()
            .map_err(|_| Error::invalid("`round limit` is how many rounds the match runs"))?;
        let preset = match fields.get(6).copied() {
            None | Some("resume") => {
                return Err(Error::invalid(
                    "the config line has no `preset` (field 6, `probation` or `custom`): a line \
                     written before S3 names none, and this host does not guess one; start a \
                     new match",
                ));
            }
            Some(text) => Preset::parse(text)?,
        };
        // Decisions-log item 110 (5): v1 plays one to three seats, and a lobby
        // that asked for more is told so here, before a map is generated.
        Host::check_seats(seats)?;
        if let Some(seat) = human_seat
            && u32::from(seat) >= seats
        {
            return Err(Error::invalid(said!(
                "this match has {} seats and the human is seat {}",
                seats,
                seat
            )));
        }
        Ok(Config {
            match_id,
            seed,
            seats,
            human_seat,
            segment_lengths_ms,
            round_limit,
            preset,
        })
    }

    /// The line a lobby writes, which is also what the tests write.
    #[must_use]
    pub fn render(&self) -> String {
        let human = self
            .human_seat
            .map_or_else(|| String::from("-"), |seat| seat.to_string());
        let lengths = if self.segment_lengths_ms.is_empty() {
            String::from("-")
        } else {
            self.segment_lengths_ms
                .iter()
                .map(i32::to_string)
                .collect::<Vec<String>>()
                .join(",")
        };
        format!(
            "{}\t{}\t{}\t{human}\t{lengths}\t{}\t{}\n",
            self.match_id,
            crate::view::seed_text(self.seed),
            self.seats,
            self.round_limit,
            self.preset.name()
        )
    }
}

/// A whole config line: the match's seven values, and whether to resume it.
///
/// A type of its own rather than a field on [`Config`], so that a resume is
/// never one of the match's values (decisions-log item 111, decision C9).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConfigLine {
    /// The seven values.
    pub config: Config,
    /// True for a line whose eighth field is `resume`: go on with the match
    /// saved under this id.
    pub resume: bool,
}

impl ConfigLine {
    /// Read one config line, with its optional eighth field.
    ///
    /// # Errors
    ///
    /// As [`Config::parse`], and [`crate::error::Code::InvalidArgument`] for an
    /// eighth field that is anything but `resume`, `-` or empty, and for a
    /// line with a ninth field or more: a resume line repeats the saved match
    /// exactly (decisions-log item 112 (8)), and a field this host does not
    /// read is not silently dropped.
    pub fn parse(line: &str) -> Result<ConfigLine, Error> {
        let config = Config::parse(line)?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let fields = trimmed.split('\t').count();
        if fields > 8 {
            return Err(Error::invalid(said!(
                "the config line has {} tab-separated fields and this host reads at most \
                 eight: the match's seven, and `resume`",
                fields
            )));
        }
        let eighth = trimmed.split('\t').nth(7).unwrap_or("");
        let resume = match eighth {
            "" | "-" => false,
            "resume" => true,
            other => {
                return Err(Error::invalid(said!(
                    "`{}` is not an eighth config field this host reads: it is `resume`, or \
                     nothing for a new match",
                    pharmakos_proto::json::grammar::quoted(other)
                )));
            }
        };
        Ok(ConfigLine { config, resume })
    }

    /// The line a lobby writes: seven fields for a new match, eight for a
    /// resume.
    #[must_use]
    pub fn render(&self) -> String {
        let line = self.config.render();
        if self.resume {
            format!("{}\tresume\n", line.trim_end_matches('\n'))
        } else {
            line
        }
    }
}

/// What the host announces once it is listening.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Announce {
    /// The loopback port both connections are made to.
    pub port: u16,
    /// The match id.
    pub match_id: String,
    /// The lobby's own token: `admin`, and `observe` so that it can watch its
    /// own match end.
    pub admin_token: String,
    /// The human seat's token, when this match has a human seat.
    pub seat_token: Option<String>,
}

impl Announce {
    /// The one line the parent reads.
    #[must_use]
    pub fn render(&self) -> String {
        let seat = self.seat_token.clone().unwrap_or_else(|| String::from("-"));
        format!(
            "{}\t{}\t{}\t{seat}\n",
            self.port, self.match_id, self.admin_token
        )
    }

    /// Read one announce line, which is what a parent and a test both do.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a line this host did not
    /// write.
    pub fn parse(line: &str) -> Result<Announce, Error> {
        let fields: Vec<&str> = line.trim_end_matches(['\r', '\n']).split('\t').collect();
        let refuse = || Error::invalid("that is not an announce line this host wrote");
        let port = fields
            .first()
            .and_then(|text| text.parse::<u16>().ok())
            .ok_or_else(refuse)?;
        let match_id = fields.get(1).ok_or_else(refuse)?;
        let admin_token = fields.get(2).ok_or_else(refuse)?;
        let seat = fields.get(3).copied().ok_or_else(refuse)?;
        Ok(Announce {
            port,
            match_id: (*match_id).to_owned(),
            admin_token: (*admin_token).to_owned(),
            seat_token: if seat == "-" {
                None
            } else {
                Some(seat.to_owned())
            },
        })
    }
}

/// One piece of work for the thread that owns the surface.
enum Job {
    /// Authenticate a connection's token, and record the upgrade.
    Open { token: Token, reply: Sender<Opened> },
    /// Answer one JSON-RPC text message.
    Call {
        token: Token,
        text: String,
        reply: Sender<String>,
    },
    /// Record a refused upgrade.
    Refused { reason: Said, status: u16 },
    /// The control pipe reached end of file: put the match down.
    ///
    /// The channel closing would not do it. A reader thread and the accept
    /// dispatcher each hold a sender, and both outlive [`run`] by design -- a
    /// blocking `accept` cannot be interrupted without dialling the listener,
    /// which this crate may not do (`tests/confinement.rs`: the gateway
    /// accepts connections and makes none). So the end of the match is a
    /// message rather than a dropped channel, and the process exiting is what
    /// collects the rest.
    Stop,
    /// The control pipe's reader ended for another reason than end of file:
    /// a read error, or a panic caught at its thread's boundary. The match is
    /// put down as at end of file, and the host stops with this error, so no
    /// host is left deaf to its parent.
    PipeFailed(Error),
}

/// What a host that stopped on a write it could not make leaves behind for
/// its connections ([`Writer::stop`]): the typed error, and the match's
/// tokens, so a connection opened after the stop is told why only when its
/// token is this match's ([`RemoteDoor::open`]).
struct HostStopped {
    error: Error,
    tokens: crate::token::TokenStore,
    match_id: String,
    tick: pharmakos_sim::math::quantity::Tick,
}

/// What a stopped host says to an upgrade with this match's token: the
/// stop's own words, the body of its 503 ([`Opened::Stopped`]).
fn stopped_words(error: &Error) -> String {
    format!("this host has stopped: {}", error.message())
}

/// The stop, once: set by the surface thread on its way out, and read by
/// every connection's door, which answers each later call and upgrade with
/// it ([`RemoteDoor::answer`], [`RemoteDoor::open`]).
type Stopped = Arc<OnceLock<HostStopped>>;

/// A connection's door: everything goes to the one thread that owns the
/// surface, and the connection waits for the answer.
struct RemoteDoor {
    jobs: SyncSender<Job>,
    stopped: Stopped,
}

impl RemoteDoor {
    fn new(jobs: SyncSender<Job>, stopped: Stopped) -> RemoteDoor {
        RemoteDoor { jobs, stopped }
    }

    /// The answer to `text` once the surface thread has gone: the typed
    /// error it stopped on, when it stopped on one (a save the disk needed
    /// and could not have), to every caller, the seats and the lobby alike;
    /// otherwise the host is shutting down because its parent closed the
    /// control pipe, and says so.
    fn after_the_surface(&self, text: &str) -> String {
        let Some(HostStopped { error, .. }) = self.stopped.get() else {
            return rpc::render(&rpc::malformed_response(&rpc::Malformed::Invalid(
                String::from("this host is shutting down"),
            )));
        };
        // The call's own id where the text is a call; a text that is not one
        // is answered under no id, as a malformed call always is.
        let id = match rpc::parse(text) {
            Ok(request) => request.id,
            Err(_) => Json::Null,
        };
        rpc::render(&rpc::failure(&id, error))
    }
}

impl Door for RemoteDoor {
    /// A channel of its own for each job, whose only sender travels with the
    /// job: a job the surface thread never answers (it stopped with the job
    /// still queued) drops that sender, and the wait ends rather than hanging.
    ///
    /// Once the surface has stopped on a write it could not make, a token of
    /// this match is told why ([`Opened::Stopped`], a 503 with the stop's
    /// words), and any other token is refused as before: the token is
    /// authenticated against the match's own tokens, which the stop left
    /// here, so a stranger learns nothing of the host's state.
    fn open(&mut self, token: &Token) -> Opened {
        let (reply, granted) = channel();
        let job = Job::Open {
            token: token.clone(),
            reply,
        };
        if self.jobs.send(job).is_ok()
            && let Ok(opened) = granted.recv()
        {
            return opened;
        }
        match self.stopped.get() {
            Some(stopped)
                if stopped
                    .tokens
                    .authenticate(token, &stopped.match_id, stopped.tick)
                    .is_ok() =>
            {
                Opened::Stopped(stopped_words(&stopped.error))
            }
            Some(_) | None => Opened::Refused,
        }
    }

    fn answer(&mut self, token: &Token, text: &str) -> String {
        let (reply, answered) = channel();
        let job = Job::Call {
            token: token.clone(),
            text: text.to_owned(),
            reply,
        };
        if self.jobs.send(job).is_err() {
            return self.after_the_surface(text);
        }
        match answered.recv() {
            Ok(answer) => answer,
            Err(_) => self.after_the_surface(text),
        }
    }

    fn refused(&mut self, refusal: &crate::handshake::Refusal) {
        let error = session::refusal_error(refusal);
        let _ = self.jobs.send(Job::Refused {
            reason: error.said(),
            status: refusal.status(),
        });
    }
}

/// Host one match until `control` reaches end of file, or until a write the
/// match needs fails, keeping its private cache in the OS-standard per-user
/// data directory ([`crate::cache::locate`]).
///
/// `control` is read on a thread of its own, so the host returns as soon as
/// it stops on a failed write, whether or not its parent has closed the
/// pipe; hence `Send + 'static`.
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] for a config line this host cannot
/// read or a lobby setting it refuses, for a new match whose id already has a
/// save, and for a resume the save refuses (another rules table, verifier or
/// snapshot format, a config line that is not the saved one, or a damaged
/// file); and [`crate::error::Code::Internal`] when the map will not generate,
/// no loopback address can be bound, the operating system will not give
/// entropy for a token, or there is nowhere standard to keep the private match
/// cache (decisions-log item 98: a missing variable is an error the lobby
/// shows, never a fallback beside the executable); and
/// [`crate::error::Code::Internal`] when a write the match needs fails while it
/// is served or as it closes (the writer's flush and its close): its words
/// say what could not be written, why, and what the disk holds, read from the
/// cache, and every call after it was answered with the same error.
pub fn run<C: Read + Send + 'static, A: Write>(
    setup: Setup,
    control: C,
    announce: A,
) -> Result<(), Error> {
    run_in(setup, &crate::cache::locate()?, control, announce)
}

/// [`run`], keeping the private match cache under `data_root` rather than the
/// OS-standard directory.
///
/// For a host, or a test, that wants its matches somewhere of its own: a save
/// now outlives its process (a new match under a saved match's id is refused),
/// so a test that hosts matches under fixed ids keeps them in a folder it owns
/// rather than in the machine's real cache.
///
/// # Errors
///
/// As [`run`].
pub fn run_in<C: Read + Send + 'static, A: Write>(
    setup: Setup,
    data_root: &Path,
    control: C,
    mut announce: A,
) -> Result<(), Error> {
    let mut lines = BufReader::new(control);
    let line = read_config(&mut lines)?;
    let config = line.config.clone();
    let mut operators = setup.operators;
    let OpenedMatch {
        cache,
        mut surface,
        seats,
        rules_hash,
        largest,
    } = open_match(&setup.rules_json, setup.library, data_root, &line)?;

    let listener = Listener::bind(0)?;
    let port = listener.port();
    let policy = listener.policy();

    let minted = mint_tokens(&mut surface, &config)?;
    // After the config line, which is the only time the factory can know
    // which seats exist and which is the human's (item 111, H11).
    let mut in_process =
        InProcessSeats::open(&mut surface, &seats, config.human_seat, operators.as_mut())?;
    let announcement = Announce {
        port,
        match_id: config.match_id.clone(),
        admin_token: minted.admin.render(),
        seat_token: minted.seat.as_ref().map(Token::render),
    };
    let written = announcement.render();

    // The private match cache was opened above. The audit log, the saves and
    // the replay's inputs are flushed into it as the match runs, which is also
    // what makes
    // `a_token_appears_on_the_announce_line_and_nowhere_else_the_host_writes`
    // a test of something rather than of nothing.
    //
    // The surface thread answers every method, so it reads every player's
    // text, and it does so on the stated stack (S3's `vfyq2`, fix pass 2,
    // item E). It and the threads that accept are made before the announce
    // line goes out: a host that cannot serve does not announce.
    let (jobs, work) = sync_channel::<Job>(QUEUE_DEPTH);
    let drops = Arc::new(Drops::default());
    let stopped: Stopped = Arc::new(OnceLock::new());
    let writer = Writer {
        cache,
        config,
        rules_hash,
        largest,
        drops: Some(Arc::clone(&drops)),
        drops_recorded: Cell::new(false),
        audit_broken: Cell::new(false),
    };
    let held = Arc::clone(&stopped);
    // The surface thread says when its first flush has landed: the announce
    // waits for it (a host that cannot serve does not announce), and that
    // flush holds whatever the opening left, a `sealed` save's resumed Push
    // start included.
    let (first, first_flushed) = channel::<Result<(), Error>>();
    let surface_thread = pharmakos_plan_core::stack::player_text_thread("gateway surface")
        .spawn(move || serve_surface(&mut surface, &writer, &mut in_process, &work, &held, &first))
        .map_err(|error| {
            Error::internal(said!(
                "starting the surface thread: {}",
                crate::error::engine(&error)
            ))
        })?;
    let dispatcher = spawn_dispatcher(listener.accept()?, policy, jobs.clone(), drops, stopped)?;
    let surface_thread = first_flush_landed(&first_flushed, surface_thread)?;
    announce
        .write_all(written.as_bytes())
        .and_then(|()| announce.flush())
        .map_err(|error| {
            Error::internal(said!(
                "writing the announce line: {}",
                crate::error::engine(&error)
            ))
        })?;

    // The parent holds this pipe open for the life of the match. Reading it to
    // end of file is the whole of the shutdown protocol -- no heartbeat, no
    // clock, nothing to time out -- and it is read on a thread of its own, so
    // a host that stops on a failed write returns at once rather than when
    // its parent lets go. That thread is not joined: it is blocked on the
    // parent, and `gamectl host` exiting is what ends it, as it ends the
    // accept threads.
    watch_the_control_pipe(lines, jobs.clone())?;
    drop(jobs);

    // The surface thread flushes the audit log on its way out, so it is joined
    // rather than abandoned. It returns on end of file (a `Job::Stop`) or on
    // the first write the match needed and could not have.
    let served = surface_thread.join();
    drop(dispatcher);
    match served {
        // The surface stopped on a write it could not make: the host's last
        // word, said by its caller (`gamectl host`'s standard error).
        Ok(Err(error)) => Err(error),
        Ok(Ok(())) => Ok(()),
        Err(_) => Err(Error::internal(
            "the surface thread ended in a panic, and the match it served is not saved past it",
        )),
    }
}

/// A match opened or resumed under a data root, before any token is minted:
/// what [`run_in`] and [`External::open`] both start from.
struct OpenedMatch {
    cache: MatchCache,
    surface: Surface,
    seats: Vec<SeatId>,
    rules_hash: u64,
    /// The largest a save of this match can be ([`Save::max_bytes`]).
    largest: u64,
}

/// Open the match a config line names, or resume the one saved under its id,
/// with its private cache under `data_root`.
///
/// One function for the socket host and the external seat's, so that a match
/// is opened, refused and resumed by the same rules whichever host serves it.
///
/// # Errors
///
/// As [`run`]'s, less the ones about a socket or a token.
fn open_match(
    rules_json: &str,
    library: Option<PathBuf>,
    data_root: &Path,
    line: &ConfigLine,
) -> Result<OpenedMatch, Error> {
    let config = &line.config;
    let rules_hash = RulesHash::of(rules_json)?;
    // The rules check before anything is written (S3's `vfyq2`, fix pass 6,
    // item 3): a table the verifier refuses leaves no match folder behind.
    Host::check_rules_text(rules_json)?;

    // The save is read before the surface is opened (the wave-6 notes, H16),
    // and a new match is refused before a map is generated if its id already
    // has one.
    // The largest a save of this match can be: what a resume reads to, and
    // what the writer reads the disk to when it says what is there.
    let largest = largest_save(config, rules_json)?;
    let (cache, mut surface, seats) = if line.resume {
        // Open and read; parse and check the save against the config line;
        // only then lay the folder out: a refused resume writes nothing.
        let cache = MatchCache::reopen(data_root, &config.match_id)?;
        let save = Save::parse(&cache.read_save(largest)?)?;
        save.check(config, rules_hash.0)?;
        cache.lay_out_for_resume(config.seats)?;
        // Read before the surface exists: the view's identity folds in the
        // log's last sequence number, so that nothing the dead process served
        // is read as this one's, and the resumed tick starts no lower than the
        // log's last.
        let continuation = cache.continuation()?;
        let (surface, seats) = resume_surface(rules_json, library, config, &save, continuation)?;
        (cache, surface, seats)
    } else {
        let cache = MatchCache::open(
            data_root,
            &config.match_id,
            &crate::cache::Header {
                match_seed: config.seed,
                seats: config.seats,
                segment_lengths_ms: config.segment_lengths_ms.clone(),
                round_limit: config.round_limit,
                rules_hash: rules_hash.0,
            },
        )?;
        let (surface, seats) = open_surface(rules_json, library, config)?;
        (cache, surface, seats)
    };
    surface.audit().continue_after(cache.last_audit_seq()?);
    if line.resume {
        let tick = surface.time().tick;
        surface
            .audit()
            .record(tick, None, None, "resumed", crate::audit::Outcome::Ok);
    }
    Ok(OpenedMatch {
        cache,
        surface,
        seats,
        rules_hash: rules_hash.0,
        largest,
    })
}

/// Wait for the surface thread's first flush, and hand its handle back once
/// it has landed.
///
/// # Errors
///
/// The first flush's own error when it failed, said before any announce
/// line, as a line the host refuses is; [`crate::error::Code::Internal`]
/// when the thread ended without saying.
fn first_flush_landed(
    first_flushed: &Receiver<Result<(), Error>>,
    surface_thread: std::thread::JoinHandle<Result<(), Error>>,
) -> Result<std::thread::JoinHandle<Result<(), Error>>, Error> {
    match first_flushed.recv() {
        Ok(Ok(())) => Ok(surface_thread),
        Ok(Err(error)) => Err(error),
        // The thread ended without saying: what it returned says why.
        Err(_) => match surface_thread.join() {
            Ok(Err(error)) => Err(error),
            Ok(Ok(())) => Err(Error::internal(
                "the surface thread ended before its first write was made",
            )),
            Err(_) => Err(Error::internal(
                "the surface thread ended in a panic before the match was announced",
            )),
        },
    }
}

/// Read the control pipe to its end on a thread of its own, holding none of
/// it, then tell the surface thread to put the match down: `Job::Stop` at end
/// of file, `Job::PipeFailed` when the read failed or the reader panicked, so
/// the thread cannot end without the host stopping.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the thread cannot be started.
fn watch_the_control_pipe<C: Read + Send + 'static>(
    mut lines: BufReader<C>,
    jobs: SyncSender<Job>,
) -> Result<(), Error> {
    pharmakos_plan_core::stack::player_text_thread("gateway control pipe")
        .spawn(move || {
            let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                std::io::copy(&mut lines, &mut std::io::sink())
            }));
            let job = match read {
                Ok(Ok(_)) => Job::Stop,
                Ok(Err(error)) => Job::PipeFailed(Error::internal(said!(
                    "the control pipe failed ({}): the parent can no longer put the match \
                     down, so this host did, as at end of file",
                    crate::error::engine(&error)
                ))),
                Err(_) => Job::PipeFailed(Error::internal(
                    "the control pipe's reader panicked: the parent can no longer put the \
                     match down, so this host did, as at end of file",
                )),
            };
            // A surface thread that has already stopped has nobody to tell.
            let _told = jobs.send(job);
        })
        .map(drop)
        .map_err(|error| {
            Error::internal(said!(
                "starting the control pipe's reader: {}",
                crate::error::engine(&error)
            ))
        })
}

/// The rules table's hash, read off the text the binary handed over.
struct RulesHash(u64);

impl RulesHash {
    fn of(rules_json: &str) -> Result<RulesHash, Error> {
        pharmakos_sim::rules::RulesTable::from_canonical_json(rules_json)
            .map(|table| RulesHash(table.rules_hash()))
            .map_err(|error| {
                Error::invalid(said!(
                    "this is not a rules table this build reads: {}",
                    crate::error::engine(&error)
                ))
            })
    }
}

/// Read the one config line the parent writes.
fn read_config<C: Read>(lines: &mut BufReader<C>) -> Result<ConfigLine, Error> {
    let mut first = String::new();
    lines.read_line(&mut first).map_err(|error| {
        Error::internal(said!(
            "reading the config line: {}",
            crate::error::engine(&error)
        ))
    })?;
    if first.trim().is_empty() {
        return Err(Error::invalid(
            "this host is started by one config line on its standard input, and the line was empty",
        ));
    }
    ConfigLine::parse(&first)
}

/// The pristine match a config line describes, and its seats.
///
/// One function for both a new match and a resume, because a resume works
/// only if the world it restores into is generated exactly as the saved
/// match's was: two copies of this construction that drifted apart would have
/// every resume refused at [`Host::resume`]'s restore.
fn host_for(
    rules_json: &str,
    library: Option<PathBuf>,
    config: &Config,
) -> Result<(Host, Vec<SeatId>), Error> {
    let host = Host::open_from(
        rules_json,
        config.seed,
        config.seats,
        &Settings {
            segment_lengths_ms: config.segment_lengths_ms.clone(),
            round_limit: config.round_limit,
        },
        library,
    )?;
    let seats: Vec<SeatId> = (0..config.seats)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect();
    Ok((host, seats))
}

/// Open the match and the surface over it, in its opening Lull.
fn open_surface(
    rules_json: &str,
    library: Option<PathBuf>,
    config: &Config,
) -> Result<(Surface, Vec<SeatId>), Error> {
    let (host, seats) = host_for(rules_json, library, config)?;
    let rules = host.rules().clone();
    let mut surface = Surface::new(
        &config.match_id,
        config.seed,
        rules,
        // Fogged, always: the demo has no casual match, and a flag that could
        // turn the fog off from a config line would be a flag a bug could turn
        // off (spec section 12 makes it a per-match policy chosen when the
        // match is made, not something the transport carries).
        FogPolicy::fogged(),
        &seats,
    )?
    .with_preset(config.preset, config.human_seat.map(SeatId::new));
    surface.attach(host)?;
    surface.open_lull()?;
    Ok((surface, seats))
}

/// The largest a save of the match `config` names can be
/// ([`Save::max_bytes`]), under the rules text the binary handed over: what a
/// resume reads the save to, and refuses past.
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] for rules text this build does not
/// read; as [`Save::max_bytes`] otherwise.
fn largest_save(config: &Config, rules_json: &str) -> Result<u64, Error> {
    let rules =
        pharmakos_sim::rules::RulesTable::from_canonical_json(rules_json).map_err(|error| {
            Error::invalid(said!(
                "this is not a rules table this build reads: {}",
                crate::error::engine(&error)
            ))
        })?;
    Save::max_bytes(config, &rules)
}

/// Open a saved match, and the surface over it, where the save left it: in
/// the Lull it was quit in, or in the Push its seals began.
///
/// The pristine world is regenerated from the save's own config line --
/// [`Host::open_from`], exactly as a new match is opened -- and the surface
/// puts the save into it ([`Surface::resume`], which calls [`Host::resume`]).
fn resume_surface(
    rules_json: &str,
    library: Option<PathBuf>,
    config: &Config,
    save: &Save,
    continuation: crate::save::Continuation,
) -> Result<(Surface, Vec<SeatId>), Error> {
    let (host, seats) = host_for(rules_json, library, config)?;
    let rules = host.rules().clone();
    // The preset before the save goes in, so the first time synced is the
    // preset's (S3's `gwu`, fix pass 1).
    let surface = Surface::new(
        &config.match_id,
        config.seed,
        rules,
        // Fogged, always, as `open_surface` says.
        FogPolicy::fogged(),
        &seats,
    )?
    .with_preset(config.preset, config.human_seat.map(SeatId::new))
    .resume(host, &save.state, continuation)?;
    Ok((surface, seats))
}

/// The two tokens this host hands out, and the only time either is.
struct Minted {
    admin: Token,
    seat: Option<Token>,
}

/// The scopes a seat's own token holds: a socket's, and a built-in seat's.
fn seat_scopes() -> ScopeSet {
    ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs])
}

/// The scopes an advisor's token holds: a seat's, less `plan.submit`
/// (decisions-log item 111, decision C5). [`ADVISOR_METHODS`] narrows it
/// further.
fn advisor_scopes() -> ScopeSet {
    ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::Docs])
}

/// Mint the lobby's token and the human seat's.
///
/// The tokens of the seats this process plays or advises are minted by
/// [`InProcessSeats::open`] and never leave it.
///
/// **No spectator token is minted.** Spec section 5 gives the spectator camera
/// to built-in-only matches and unlocks an eliminated human by *policy* rather
/// than by token, and this demo has a human seat.
/// PLACEHOLDER: the spectator token — owner, at S5, with the built-in-only match flow.
fn mint_tokens(surface: &mut Surface, config: &Config) -> Result<Minted, Error> {
    let minted_at = surface.time().tick;
    // `observe` beside `admin`, because the lobby watches the match it
    // controls -- the phase and the timer ride the `_status` footer of every
    // answer, and the end screen reads the view. `admin` still reaches no
    // seat's private state: that gate is `Surface::seat_state`, and it is
    // about the subject asking rather than the scope held.
    let (admin, _) = surface.tokens().mint(
        Subject::Admin,
        &config.match_id,
        ScopeSet::of(&[Scope::Admin, Scope::Observe]),
        minted_at,
    )?;
    let seat = match config.human_seat {
        Some(raw) => {
            let (token, _) = surface.tokens().mint(
                Subject::Seat(SeatId::new(raw)),
                &config.match_id,
                seat_scopes(),
                minted_at,
            )?;
            Some(token)
        }
        None => None,
    };
    Ok(Minted { admin, seat })
}

/// One seat this process plays.
struct Played {
    seat: u8,
    token: Token,
    operator: Box<dyn BuiltInSeat>,
}

/// One seat this process advises.
struct Advised {
    seat: u8,
    token: Token,
    handle: Handle,
    advisor: Box<dyn Advisor>,
}

/// The seats this process plays or advises, with the tokens it minted for
/// them.
///
/// Built once, after the config line ([`InProcessSeats::open`]), and run once
/// per Lull ([`InProcessSeats::plan`]). Public so that a test can drive the
/// two seams against a surface it holds, without a socket, exactly as the host
/// loop does; its tokens are private fields and there is no accessor, so
/// nothing outside this module can render one.
pub struct InProcessSeats {
    played: Vec<Played>,
    advised: Vec<Advised>,
    /// The last round planned, so each Lull is planned once.
    planned: u32,
}

impl std::fmt::Debug for InProcessSeats {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InProcessSeats")
            .field("played", &self.played_seats())
            .field("advised", &self.advised_seats())
            .field("planned", &self.planned)
            .finish()
    }
}

impl InProcessSeats {
    /// Ask the factory for each seat, and mint and register a token for each
    /// answer.
    ///
    /// `seats` in any order; they are asked in ascending seat id. The human's
    /// seat is never offered a built-in operator. Every token minted here is
    /// registered with [`Surface::register_in_process`] at
    /// [`crate::limit::IN_PROCESS_LIMITS`]; an advisor's also reads a scratch
    /// view feed.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the operating system will not give
    /// entropy for a token.
    pub fn open(
        surface: &mut Surface,
        seats: &[SeatId],
        human_seat: Option<u8>,
        operators: &mut dyn Operators,
    ) -> Result<InProcessSeats, Error> {
        let mut ordered: Vec<u8> = seats.iter().map(|seat| seat.raw()).collect();
        ordered.sort_unstable();
        ordered.dedup();
        let minted_at = surface.time().tick;
        let match_id = surface.match_id().to_owned();
        let mut played: Vec<Played> = Vec::new();
        let mut advised: Vec<Advised> = Vec::new();
        for raw in ordered {
            let subject = Subject::Seat(SeatId::new(raw));
            if Some(raw) != human_seat
                && let Some(operator) = operators.built_in(raw)
            {
                let (token, handle) =
                    surface
                        .tokens()
                        .mint(subject, &match_id, seat_scopes(), minted_at)?;
                surface.register_in_process(
                    handle,
                    InProcess {
                        limits: crate::limit::IN_PROCESS_LIMITS,
                        scratch_view: false,
                    },
                );
                played.push(Played {
                    seat: raw,
                    token,
                    operator,
                });
                continue;
            }
            if let Some(advisor) = operators.advisor(raw) {
                let (token, handle) =
                    surface
                        .tokens()
                        .mint(subject, &match_id, advisor_scopes(), minted_at)?;
                surface.register_in_process(
                    handle,
                    InProcess {
                        limits: crate::limit::IN_PROCESS_LIMITS,
                        scratch_view: true,
                    },
                );
                advised.push(Advised {
                    seat: raw,
                    token,
                    handle,
                    advisor,
                });
            }
        }
        Ok(InProcessSeats {
            played,
            advised,
            planned: 0,
        })
    }

    /// The seats this process plays, ascending.
    #[must_use]
    pub fn played_seats(&self) -> Vec<u8> {
        self.played.iter().map(|played| played.seat).collect()
    }

    /// The seats this process advises, ascending.
    #[must_use]
    pub fn advised_seats(&self) -> Vec<u8> {
        self.advised.iter().map(|advised| advised.seat).collect()
    }

    /// Plan every played seat's round and advise every advised seat, once per
    /// Lull.
    ///
    /// Does nothing outside a Lull or in a Lull already planned. The played
    /// seats go first, in ascending seat id, then the advised ones: neither can
    /// see the other's work (an advisor writes nothing but the advice the host
    /// files, and a built-in seat's store is its own), so the order is a
    /// convention rather than a dependency
    /// (`a_built_in_seats_sealed_plan_is_the_same_with_and_without_an_advisor_running`).
    pub fn plan(&mut self, surface: &mut Surface) {
        let time = surface.time();
        if time.phase != pharmakos_proto::gp::api::v1::status::Phase::Lull
            || time.round == self.planned
        {
            return;
        }
        self.planned = time.round;
        for played in &mut self.played {
            let token = &played.token;
            let mut call = |request: &Request| -> Json {
                let vision = vision_of(surface);
                surface.call(Some(token), request, &vision)
            };
            played.operator.plan(played.seat, &mut call);
        }
        for advised in &mut self.advised {
            let (seat, token, handle) = (advised.seat, &advised.token, advised.handle);
            let mut call =
                |request: &Request| -> Json { advisor_call(surface, seat, token, handle, request) };
            let advice = advised.advisor.advise(seat, &mut call);
            if let Err(error) = surface.file_advice(SeatId::new(seat), advice) {
                // Only a host that asked outside a Lull, or for a seat the
                // match has not got, gets here: said in the log rather than
                // dropped.
                let tick = surface.time().tick;
                surface.audit().refused(
                    tick,
                    Some(Subject::Seat(SeatId::new(seat))),
                    Some(handle),
                    "file advice",
                    &error,
                );
            }
        }
    }
}

/// One call an advisor makes: through the allow-list, then the ordinary door.
fn advisor_call(
    surface: &mut Surface,
    seat: u8,
    token: &Token,
    handle: Handle,
    request: &Request,
) -> Json {
    let method = crate::scopes::method_from_wire(&request.method);
    let allowed = method.is_some_and(|method| ADVISOR_METHODS.contains(&method));
    if !allowed {
        let error = Error::forbidden(said!(
            "the built-in operator advising seat {} may not call `{}`: an advisor reads and \
             never writes, and a seat's notebook, drafts, submission and readiness are its own",
            seat,
            pharmakos_proto::json::grammar::quoted(&method.map_or_else(
                || String::from("<unknown>"),
                crate::scopes::method_wire_name
            ))
        ));
        let tick = surface.time().tick;
        surface.audit().refused(
            tick,
            Some(Subject::Seat(SeatId::new(seat))),
            Some(handle),
            crate::surface::call_action(&request.method),
            &error,
        );
        return rpc::failure(&request.id, &error);
    }
    let vision = vision_of(surface);
    surface.call(Some(token), request, &vision)
}

/// A match hosted for **one seat played from outside the game**, with no
/// socket: the external seat (decisions-log item 231; the module docs, "The
/// external seat").
///
/// The program outside never holds a token. This value holds the seat's, in
/// a private field with no accessor, and [`External::call`] is
/// [`Surface::call`] bound to it: the same authentication, scopes, phase
/// checks, fog and audit as a socket's call, at [`crate::limit::EXTERNAL_LIMITS`].
/// There is no admin token at all. The match moves only by the host's own
/// hand ([`External::advance`]), and only when the seat has said it is ready:
/// the seat's rights are a seat's, and "play the Push" is not one of them.
///
/// Every other seat is built in, planned by the factory in [`Setup`] exactly
/// as [`run`] plans them, and the outside seat is advised as a human's is, so
/// a round it seals nothing in is filed that seat's safe playbook.
///
/// The private cache, the saves and the replay's inputs are written as
/// [`run`] writes them, by the same [`Writer`] and under the same rule: a
/// boundary is on disk before anyone is told it was crossed. A match is
/// therefore put down and taken up between any two calls
/// ([`External::close`], then a resume line), which is what lets one command
/// make one call.
pub struct External {
    surface: Surface,
    writer: Writer,
    in_process: InProcessSeats,
    /// The seat's own token: minted here, used here, never rendered.
    token: Token,
    seat: SeatId,
    /// The write this host stopped on, once it has: every later call is
    /// answered with it, as a socket host's connections are.
    stopped: Option<Error>,
}

impl std::fmt::Debug for External {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("External")
            .field("seat", &self.seat.raw())
            .field("in_process", &self.in_process)
            .field("stopped", &self.stopped.is_some())
            .finish_non_exhaustive()
    }
}

impl External {
    /// Open the match `line` names, or resume it, with its private cache
    /// under `data_root`, and bring it to where the outside seat acts next:
    /// a Lull every built-in seat has planned, or the match's end.
    ///
    /// The outside seat is the line's `human seat`: the config line has one
    /// slot for a seat the built-in operator does not play, and a headless
    /// match fills it with the program outside (item 231). A resume that
    /// finds the match in a Push, because the last process ended in one,
    /// replays that Push first, from the save its seals left.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a line with no human seat,
    /// and as [`run`]'s for the match itself; [`crate::error::Code::Internal`]
    /// when a write the match needs fails.
    pub fn open(setup: Setup, data_root: &Path, line: &ConfigLine) -> Result<External, Error> {
        let Some(raw) = line.config.human_seat else {
            return Err(Error::invalid(
                "an external seat is the config line's human seat, and this line has none",
            ));
        };
        if u32::from(raw) >= line.config.seats {
            return Err(Error::invalid(said!(
                "this match has {} seats and the external seat is seat {}",
                line.config.seats,
                raw
            )));
        }
        let seat = SeatId::new(raw);
        let mut operators = setup.operators;
        let OpenedMatch {
            cache,
            mut surface,
            seats,
            rules_hash,
            largest,
        } = open_match(&setup.rules_json, setup.library, data_root, line)?;

        let minted_at = surface.time().tick;
        let match_id = surface.match_id().to_owned();
        let subject = Subject::Seat(seat);
        let (token, handle) =
            surface
                .tokens()
                .mint(subject, &match_id, seat_scopes(), minted_at)?;
        surface.register_in_process(
            handle,
            InProcess {
                limits: crate::limit::EXTERNAL_LIMITS,
                scratch_view: false,
            },
        );
        // Which handle is the program outside, said once per process: every
        // line this seat's calls leave after it carries that handle.
        surface.audit().record(
            minted_at,
            Some(subject),
            Some(handle),
            "external seat",
            crate::audit::Outcome::Ok,
        );
        let in_process = InProcessSeats::open(&mut surface, &seats, Some(raw), operators.as_mut())?;
        let mut external = External {
            surface,
            writer: Writer {
                cache,
                config: line.config.clone(),
                rules_hash,
                largest,
                drops: None,
                drops_recorded: Cell::new(false),
                audit_broken: Cell::new(false),
            },
            in_process,
            token,
            seat,
            stopped: None,
        };
        external.advance()?;
        Ok(external)
    }

    /// Answer one JSON-RPC request, as the outside seat.
    ///
    /// `text` is one request; the answer is one response object, always, for
    /// the caller to write in the layout its stream wants
    /// ([`pharmakos_proto::json::write`], or `write_line` for a stream of
    /// lines): a refusal is an answer, and a message that is not a request is
    /// answered under a null id, as JSON-RPC asks. What the call left pending
    /// (its audit line) is on disk before the answer is returned; when that
    /// write fails the answer is the typed error instead, and every later
    /// call's.
    ///
    /// The match does not move here. A `set_ready` is answered where the
    /// match stands; [`External::advance`] is what plays the Push.
    pub fn call(&mut self, text: &str) -> Json {
        self.call_bytes(text.as_bytes())
    }

    /// [`External::call`] over the bytes a stream handed in, which may not be
    /// text at all.
    ///
    /// Two refusals are made here, before anything is parsed, each answered
    /// under a null id and each one line in the audit log: a message past
    /// [`crate::frame::MAX_MESSAGE_BYTES`], refused unread (a caller that
    /// frames a stream may hand over only the first bytes of one, so long as
    /// they pass the cap), and a message that is not UTF-8, which is never
    /// repaired into text that might then parse.
    pub fn call_bytes(&mut self, bytes: &[u8]) -> Json {
        if let Some(error) = &self.stopped {
            let id = match std::str::from_utf8(bytes).map(rpc::parse) {
                Ok(Ok(request)) => request.id,
                Ok(Err(_)) | Err(_) => Json::Null,
            };
            return rpc::failure(&id, error);
        }
        let (id, response) = if crate::frame::message_fits(bytes.len()) {
            match std::str::from_utf8(bytes) {
                Ok(text) => match rpc::parse(text) {
                    Ok(request) => {
                        let vision = vision_of(&self.surface);
                        let response = self.surface.call(Some(&self.token), &request, &vision);
                        (request.id, response)
                    }
                    Err(malformed) => (Json::Null, self.unread("call <malformed>", &malformed)),
                },
                Err(_) => (
                    Json::Null,
                    self.unread(
                        "call <not utf-8>",
                        &rpc::Malformed::Parse(String::from("a request is UTF-8 text")),
                    ),
                ),
            }
        } else {
            (
                Json::Null,
                self.unread(
                    "call <oversize>",
                    &rpc::Malformed::Invalid(format!(
                        "a request is at most {} bytes, and this one is longer",
                        crate::frame::MAX_MESSAGE_BYTES
                    )),
                ),
            )
        };
        match self.writer.flush(&mut self.surface) {
            Ok(()) => response,
            Err(error) => rpc::failure(&id, &self.stop(error)),
        }
    }

    /// The answer to a message that was never a request, and its line in the
    /// audit log: `action` is one of this function's callers' fixed literals,
    /// never a byte the caller sent.
    fn unread(&mut self, action: &'static str, malformed: &rpc::Malformed) -> Json {
        let tick = self.surface.time().tick;
        self.surface.audit().refused(
            tick,
            Some(Subject::Seat(self.seat)),
            None,
            action,
            &Error::invalid(said!("a message that is not a request")),
        );
        rpc::malformed_response(malformed)
    }

    /// Bring the match to where the outside seat acts next.
    ///
    /// In a Lull the seat has not called ready in, nothing moves: the
    /// built-in seats plan it, once, and that is all. Once the seat is ready
    /// the host seals every seat's orders, plays the Push to its segment's
    /// end, closes the recap and opens the next Lull, which the built-in
    /// seats plan; or the match ends. A Push or a recap a resume landed in is
    /// played out the same way. Each boundary is on disk before the next is
    /// crossed.
    ///
    /// The Lull has no timer: a headless host reads no clock and reports
    /// none, so a Lull lasts until the seat says it may end.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when a write the match needs fails or
    /// the match will not move as its phase says it should, and the sim's own
    /// typed refusal of a tick. The host has then stopped: every later call
    /// is answered with the same error, and what the disk holds is what a
    /// resume reads.
    pub fn advance(&mut self) -> Result<(), Error> {
        if let Some(error) = &self.stopped {
            return Err(error.clone());
        }
        match self.advance_until_the_seat_acts() {
            Ok(()) => Ok(()),
            Err(error) => Err(self.stop(error)),
        }
    }

    fn advance_until_the_seat_acts(&mut self) -> Result<(), Error> {
        use pharmakos_proto::gp::api::v1::status::Phase;
        loop {
            match self.surface.time().phase {
                Phase::Lull => {
                    self.in_process.plan(&mut self.surface);
                    self.writer.flush(&mut self.surface)?;
                    // The driver holds the rule: a Lull the outside seat
                    // has not called ready in is left alone.
                    if !crate::headless::leave_lull(&mut self.surface, self.seat)? {
                        return Ok(());
                    }
                    self.writer.flush(&mut self.surface)?;
                }
                Phase::Push => {
                    crate::headless::play_push(&mut self.surface)?;
                    self.writer.flush(&mut self.surface)?;
                }
                Phase::Recap => {
                    crate::headless::leave_recap(&mut self.surface)?;
                    self.writer.flush(&mut self.surface)?;
                }
                Phase::Ended | Phase::Lobby | Phase::Unspecified => {
                    self.writer.flush(&mut self.surface)?;
                    return Ok(());
                }
            }
        }
    }

    /// Stop on `error`: write the audit log's last lines and keep the error
    /// for every later call.
    fn stop(&mut self, error: Error) -> Error {
        let error = self.writer.last_lines(&mut self.surface, error);
        self.stopped = Some(error.clone());
        error
    }

    /// Put the match down: write what is pending and, in a Lull, the `lull`
    /// save a resume lands in.
    ///
    /// # Errors
    ///
    /// The error this host stopped on, when it had; otherwise as
    /// [`Writer::close`]: a Lull lost on the way out is said, never dropped.
    pub fn close(mut self) -> Result<(), Error> {
        if let Some(error) = self.stopped.take() {
            return Err(error);
        }
        match self.writer.close(&mut self.surface) {
            Ok(()) => Ok(()),
            Err(error) => Err(self.writer.last_lines(&mut self.surface, error)),
        }
    }

    /// Whether the match has ended: there is then no Lull to act in, and
    /// every call a seat makes of a planning method is answered
    /// `PHASE_CLOSED`.
    #[must_use]
    pub fn ended(&self) -> bool {
        self.surface.time().phase == pharmakos_proto::gp::api::v1::status::Phase::Ended
    }
}

/// Accept connections, bounded by [`MAX_CONNECTIONS`], one reader thread each.
///
/// A connection thread parses every frame its socket sends, so it is made on
/// the stated stack like the surface thread (S3's `vfyq2`, fix pass 2, item
/// E). When the operating system will not make one, the connection is
/// hung up on and the refusal audited, as a connection over the cap is.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the dispatcher's own thread cannot
/// be made.
fn spawn_dispatcher(
    accepting: crate::net::Accepting,
    policy: crate::handshake::Policy,
    jobs: SyncSender<Job>,
    drops: Arc<Drops>,
    stopped: Stopped,
) -> Result<std::thread::JoinHandle<()>, Error> {
    let live = Arc::new(AtomicUsize::new(0));
    pharmakos_plan_core::stack::player_text_thread("gateway dispatcher")
        .spawn(move || {
            while let Some(connection) = accepting.recv() {
                let held = live.fetch_add(1, Ordering::SeqCst);
                if held >= MAX_CONNECTIONS {
                    live.fetch_sub(1, Ordering::SeqCst);
                    // Refused before the upgrade, and audited: a connection the
                    // host would not serve is still a connection somebody made.
                    drops.count(DropReason::OverCapacity);
                    let mut stream = connection.stream;
                    let _ = stream.write_all(OVER_CAPACITY.as_bytes());
                    // `try_send`, and not `send`: this is the accept loop, the
                    // queue in front of the surface is bounded, and the surface
                    // thread can be inside a whole `advance_push`. A blocking
                    // send here would let a flood of refusals stop the host
                    // accepting the connection that matters, which is the very
                    // thing the cap exists to prevent. The 503 has already gone
                    // out on the socket; the audit line is what is dropped when
                    // the queue is full, and a full queue is itself a fact the
                    // log shows as a gap.
                    let _ = jobs.try_send(Job::Refused {
                        reason: said!(
                            "this host serves at most {} connections at once",
                            MAX_CONNECTIONS
                        ),
                        status: 503,
                    });
                    continue;
                }
                let refusals = jobs.clone();
                let jobs = jobs.clone();
                let mine = Arc::clone(&live);
                let policy = policy.clone();
                let stopped = Arc::clone(&stopped);
                let started = pharmakos_plan_core::stack::player_text_thread("gateway connection")
                    .spawn(move || {
                        let mut door = RemoteDoor::new(jobs, stopped);
                        let _ = session::serve_connection_through(connection, &mut door, &policy);
                        mine.fetch_sub(1, Ordering::SeqCst);
                    });
                if let Err(error) = started {
                    drops.count(DropReason::NoConnectionThread);
                    // The closure, and the socket with it, was dropped unrun: the
                    // peer is hung up on. Its slot is given back, and the refusal
                    // is audited as an over-capacity one is (`try_send`, for the
                    // same reason).
                    live.fetch_sub(1, Ordering::SeqCst);
                    let _ = refusals.try_send(Job::Refused {
                        reason: said!(
                            "this host could not start a connection thread: {}",
                            crate::error::engine(&error)
                        ),
                        status: 503,
                    });
                }
            }
        })
        .map_err(|error| {
            Error::internal(said!(
                "starting the connection dispatcher: {}",
                crate::error::engine(&error)
            ))
        })
}

/// What the surface thread writes into the private match cache, and with.
///
/// # A boundary is durable before anyone is told it happened
///
/// The surface never touches the disk: a call that crosses a boundary (the
/// Push's start, a segment's end, the match's end) leaves what the disk needs
/// in the surface's pending slot, and the host loop writes it **before** it
/// answers that call ([`serve_surface`]). So no seat and no lobby is told
/// "the Push has begun" or "the match has ended" while the disk does not hold
/// it. When a write fails, the call is answered with the typed
/// [`crate::error::Code::Internal`] instead, whose words say what could not
/// be written, why, and what the disk holds, read back from the cache
/// ([`Writer::on_disk`]), never assumed ([`stop_words`]); every later call,
/// seat and lobby alike, is answered with the same error; and the host stops
/// ([`Writer::stop`], `run_in` returning it, `gamectl host` saying it on
/// standard error and exiting non-zero).
///
/// The boundary has happened in memory by then and is not undone: a tick
/// cannot be un-stepped. Nobody is served that memory again, so what a
/// resume reads is the disk, which the words describe.
///
/// # The save is the boundary's commit record: written last, or not at all
///
/// A boundary's writes go in one order, held by [`Writer::flush`]'s shape:
/// the replay inputs first (each seat's sealed playbook, each ended
/// segment's chain), then the audit log (whose last sequence number and tick
/// a resume continues from, `MatchCache::continuation`), and **the save
/// last, only when every write before it landed**. An earlier failure means
/// the save is not written, the previous save stands, and a resume of it
/// replays what was lost, byte for byte (the sim is a pure function of its
/// inputs). So the disk never holds a save whose replay inputs are missing,
/// which a resume could not rebuild (review 3's probe R3D: an `ended` save
/// over a lost final chain, which no resume can ever reach again). For each
/// boundary, after a failure at each write:
///
/// * **the Push's start** (`end_lull`): a seat's sealed playbook, or the
///   audit log: the `sealed` save is not written, and the save before it
///   stands (none in round 1; the round before's `sealed` one, or a `lull`
///   save made on a quit, after it), so a resume goes back to it and the
///   Lull is planned again; the `sealed` save itself: the same save before
///   it stands, the sealed playbooks written are overwritten byte for byte
///   when the Push is sealed again;
/// * **a segment's end** (the `advance_push` that ends it, no save): its
///   chain, or the audit log: the round's `sealed` save stands, and a resume
///   replays the Push and writes the chain again, the same bytes;
/// * **the match's end**: the final chain, or the audit log: the `ended`
///   save is not written, the final round's `sealed` save stands, and a
///   resume replays the final Push and writes the chain and the `ended`
///   save: the chain's bytes the first run would have written, and the
///   save's but for `lull_offset`, the gateway's clock offset, which a
///   resume moves past the dead process's last audited tick (tested); the
///   `ended` save itself: the same;
/// * **the close in a Lull** (end of file): any audit line still pending, or
///   the `lull` save: the save before it stands (the round's `sealed` one, or
///   none in round 1), and a resume replays from it; the close's own audit
///   lines, which follow the save they record ('save lull' and the drop
///   counts): the `lull` save is complete on disk, the words name it, the
///   host exits with the error, and a resume lands in that Lull
///   ([`Writer::close`]).
///
/// Every write is held to this, because a resume or the private replay
/// reads each: the save, the sealed playbooks and the chains (a format on
/// the player's disk, AGENTS.md section 5), and the audit log.
struct Writer {
    cache: MatchCache,
    config: Config,
    rules_hash: u64,
    /// The largest a save of this match can be ([`Save::max_bytes`]): what
    /// [`Writer::on_disk`] reads the save to.
    largest: u64,
    /// The dispatcher's counts of the connections it dropped, written into
    /// the audit log once, as the match closes or stops. None for a host
    /// with no socket ([`External`]), which has no connection to drop and
    /// writes no such line.
    drops: Option<Arc<Drops>>,
    /// Whether the drop counts have been recorded: once, whichever of the
    /// close and the stop comes first (review 3: probe C's log held them
    /// twice).
    drops_recorded: Cell<bool>,
    /// Whether an append to the audit log has failed: no later append is
    /// tried, and the words never cite the log after its own failure.
    audit_broken: Cell<bool>,
}

/// One write that did not land: what, and why. `cause` is none when `what`
/// already says it (the surface's own refusal to build a save), so no cause
/// is said twice.
struct Failure {
    what: Said,
    cause: Option<Said>,
}

/// How many of the other failures [`stop_words`] names when the audit log,
/// which would name them all, could not be written; past it, a count.
const NAMED_BESIDE: usize = 3;

/// The words a host stops with, built from what happened (review 3, item 2):
/// the first failure and its cause; the others, cited to the audit log only
/// when its append landed (`audit_written`), otherwise named here, at most
/// [`NAMED_BESIDE`] of them and a count of the rest; and what the disk holds.
fn stop_words(first: &Failure, others: &[Failure], audit_written: bool, on_disk: &Said) -> Said {
    let said_one = |failure: &Failure| match &failure.cause {
        Some(cause) => said!("{} ({})", failure.what.clone(), cause.clone()),
        None => said!("{}", failure.what.clone()),
    };
    let beside = match (others.len(), audit_written) {
        (0, _) => said!(""),
        (1, true) => said!(" (1 more write failed beside it: the audit log names it)"),
        (more, true) => said!(
            " ({} more writes failed beside it: the audit log names them)",
            more
        ),
        (more, false) => {
            let mut named = said!("");
            for (index, failure) in others.iter().take(NAMED_BESIDE).enumerate() {
                named = if index == 0 {
                    said_one(failure)
                } else {
                    said!("{}; {}", named, said_one(failure))
                };
            }
            let rest = more.saturating_sub(NAMED_BESIDE);
            match rest {
                0 => said!(" (beside it, {})", named),
                1 => said!(" (beside it, {}; and 1 more write)", named),
                _ => said!(" (beside it, {}; and {} more writes)", named, rest),
            }
        }
    };
    said!(
        "{}{}; on disk is {}; this host stops",
        said_one(first),
        beside,
        on_disk.clone()
    )
}

impl Writer {
    /// Write whatever the surface has pending, in the boundary's order (the
    /// type's doc): the sealed playbooks and the chains, then the audit log,
    /// then the save, which is written only when everything before it
    /// landed. Every replay input is still tried, so the disk holds as much
    /// as it can.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when a write the match needs could
    /// not be made, or the surface could not build the save it needed
    /// ([`crate::save::Persistence::unsaved`]), in words that say what the
    /// disk holds ([`stop_words`]).
    fn flush(&self, surface: &mut Surface) -> Result<(), Error> {
        let pending = surface.take_persistence();
        let mut failures: Vec<Failure> = Vec::new();
        if let Some(unsaved) = pending.unsaved {
            failures.push(Failure {
                what: unsaved.said(),
                cause: None,
            });
        }
        let tick = surface.time().tick;
        // 1. The replay inputs.
        for file in &pending.sealed {
            if let Err(error) = self
                .cache
                .write_sealed(file.seat, file.round, &file.playbook_jsonc)
            {
                surface
                    .audit()
                    .refused(tick, None, None, "write sealed playbook", &error);
                failures.push(Failure {
                    what: said!(
                        "seat {}'s sealed playbook of round {} could not be written",
                        file.seat.raw(),
                        file.round
                    ),
                    cause: Some(error.said()),
                });
            }
        }
        for chain in &pending.chains {
            if let Err(error) = self.cache.write_replay(chain.round, &chain.render()) {
                surface
                    .audit()
                    .refused(tick, None, None, "write replay chain", &error);
                failures.push(Failure {
                    what: said!("round {}'s replay chain could not be written", chain.round),
                    cause: Some(error.said()),
                });
            }
        }
        // 2. The audit log, which a resume continues from.
        self.append(surface, &mut failures);
        // 3. The save, last: only over a disk that holds everything it
        // depends on.
        if let Some(state) = pending.save
            && failures.is_empty()
        {
            let (boundary, round) = (state.boundary, state.round);
            if let Err(error) = self.write_save(state) {
                surface
                    .audit()
                    .refused(tick, None, None, "write save", &error);
                failures.push(Failure {
                    what: said!(
                        "the `{}` save of round {} could not be written",
                        boundary.name(),
                        round
                    ),
                    cause: Some(error.said()),
                });
                self.append(surface, &mut failures);
            }
        }
        self.failed(failures)
    }

    /// Append what the audit log holds pending, unless an append has failed
    /// before; a failure is added to `failures` and marks the log broken.
    fn append(&self, surface: &mut Surface, failures: &mut Vec<Failure>) {
        if self.audit_broken.get() {
            return;
        }
        if let Err(error) = self.cache.append_audit(&surface.audit().take()) {
            self.audit_broken.set(true);
            failures.push(Failure {
                what: said!("the audit log could not be written"),
                cause: Some(error.said()),
            });
        }
    }

    /// `Ok` when nothing failed; otherwise the typed error the host stops
    /// with, in [`stop_words`]' words over what the disk holds now.
    fn failed(&self, failures: Vec<Failure>) -> Result<(), Error> {
        let mut failures = failures.into_iter();
        let Some(first) = failures.next() else {
            return Ok(());
        };
        let others: Vec<Failure> = failures.collect();
        Err(Error::internal(stop_words(
            &first,
            &others,
            !self.audit_broken.get(),
            &self.on_disk(),
        )))
    }

    /// What the disk holds of this match's save, read back from the cache:
    /// `nothing: the match folder is gone`, `no save yet`, `the `<boundary>`
    /// save of round N`, or a save this build cannot read and why. What a resume of this match would read.
    fn on_disk(&self) -> Said {
        // A match folder that has gone holds no save, and saying "no save yet"
        // would say the match never saved (review 4, R4A [12] and [13]).
        if !self.cache.directory().is_dir() {
            return said!("nothing: the match folder is gone");
        }
        if !self.cache.has_save() {
            return said!("no save yet");
        }
        match self
            .cache
            .read_save(self.largest)
            .and_then(|text| Save::parse(&text))
        {
            Ok(save) => said!(
                "the `{}` save of round {}",
                save.state.boundary.name(),
                save.state.round
            ),
            Err(error) => said!("a save this build cannot read ({})", error.said()),
        }
    }

    fn write_save(&self, state: crate::save::SavedMatch) -> Result<(), Error> {
        let save = Save {
            stamp: Stamp::current(self.rules_hash),
            config: self.config.clone(),
            state,
        };
        self.cache.write_save(&save.render())
    }

    /// Record the dropped connections' counts in the audit log, once.
    fn record_drops(&self, surface: &mut Surface) {
        if !self.drops_recorded.replace(true)
            && let Some(drops) = &self.drops
        {
            drops.record(surface);
        }
    }

    /// The control pipe reached end of file: write what is pending, then save
    /// a Lull, and then the close's own audit lines.
    ///
    /// The order, and what a failure at each place leaves (the type's table,
    /// the close's row, in full):
    ///
    /// 1. **What is pending** ([`Writer::flush`]): nothing at a plain close,
    ///    since every job's own audit lines were written with its answer. A
    ///    failure leaves the save before it (no `lull` save is written).
    /// 2. **The `lull` save.** A failure to build or write it leaves the save
    ///    before it.
    /// 3. **The close's audit lines**, the "save lull" line and the dropped
    ///    connections' counts, which follow the save they record. A failure
    ///    here leaves the `lull` save complete on disk: the words, read back
    ///    from the disk, name it, the host exits with the typed error (`gamectl
    ///    host`: code 4), and a resume lands in that Lull; only those lines
    ///    are lost.
    ///
    /// Item 84's second boundary. A Push, a recap and an ended match are not
    /// saved here: the save made when the Push began stands, and resuming it
    /// replays that Push (the wave-6 notes, decision C8).
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when a pending write failed
    /// ([`Writer::flush`]; no Lull save is then written), or the Lull's save
    /// could not be built or written: a Lull lost on quit is said, never
    /// dropped, in words that say what the disk holds.
    fn close(&self, surface: &mut Surface) -> Result<(), Error> {
        self.flush(surface)?;
        if surface.time().phase != pharmakos_proto::gp::api::v1::status::Phase::Lull {
            self.record_drops(surface);
            let mut failures: Vec<Failure> = Vec::new();
            self.append(surface, &mut failures);
            return self.failed(failures);
        }
        let tick = surface.time().tick;
        let round = surface.time().round;
        let mut failures: Vec<Failure> = Vec::new();
        match surface.lull_save() {
            Ok(state) => match self.write_save(state) {
                Ok(()) => {
                    surface.audit().record(
                        tick,
                        None,
                        None,
                        "save lull",
                        crate::audit::Outcome::Ok,
                    );
                }
                Err(error) => {
                    surface
                        .audit()
                        .refused(tick, None, None, "save lull", &error);
                    failures.push(Failure {
                        what: said!("the `lull` save of round {} could not be written", round),
                        cause: Some(error.said()),
                    });
                }
            },
            Err(error) => {
                surface
                    .audit()
                    .refused(tick, None, None, "save lull", &error);
                failures.push(Failure {
                    what: said!("the `lull` save of round {} could not be built", round),
                    cause: Some(error.said()),
                });
            }
        }
        self.record_drops(surface);
        self.append(surface, &mut failures);
        self.failed(failures)
    }

    /// The audit log's last lines for a host stopping on `error`: the dropped
    /// connections' counts (once, where the host has connections) and
    /// whatever is pending, unless the log is already broken. Returns the
    /// error the host stops with, which names the audit log too when only now
    /// its last lines could not be written.
    fn last_lines(&self, surface: &mut Surface, error: Error) -> Error {
        self.record_drops(surface);
        let mut failures: Vec<Failure> = Vec::new();
        self.append(surface, &mut failures);
        match failures.first() {
            None => error,
            Some(lost) => Error::internal(said!(
                "{}; and the audit log's last lines could not be written{}",
                error.said(),
                match &lost.cause {
                    Some(cause) => said!(" ({})", cause),
                    None => said!(""),
                }
            )),
        }
    }

    /// Stop on `error`: count the dropped connections into the audit log
    /// (once) and write it, unless it is already broken, and leave the error
    /// where every connection's door answers with it
    /// ([`RemoteDoor::after_the_surface`]). Returns the error the host stops
    /// with, which names the audit log too when only now its last lines
    /// could not be written.
    fn stop(&self, surface: &mut Surface, error: Error, stopped: &OnceLock<HostStopped>) -> Error {
        let error = self.last_lines(surface, error);
        // Set once: only this thread sets it, and only on its way out. The
        // tokens go with it: the surface serves nothing more.
        let tick = surface.time().tick;
        let _first = stopped.set(HostStopped {
            error: error.clone(),
            tokens: std::mem::take(surface.tokens()),
            match_id: surface.match_id().to_owned(),
            tick,
        });
        error
    }
}

/// The answer a job is owed, held until what the job left pending is on disk.
enum Owed {
    /// A connection's upgrade.
    Open {
        reply: Sender<Opened>,
        granted: bool,
    },
    /// A call's answer, and the call's id, for the error that replaces the
    /// answer when the disk does not take what the call left.
    Call {
        reply: Sender<String>,
        id: Json,
        response: Json,
    },
}

impl Owed {
    /// Send the answer, or `failed` in its place.
    fn pay(self, failed: Option<&Error>) {
        // A connection that has gone away is owed nothing: its reply channel
        // is closed, and the send's error says only that.
        match (self, failed) {
            // A granted upgrade whose own flush failed crosses the stop: it is
            // told the stop's words (a 503), never refused as if its token
            // were not this match's (review 4's probe R4B).
            (Owed::Open { reply, granted }, failed) => {
                let opened = match (granted, failed) {
                    (false, _) => Opened::Refused,
                    (true, None) => Opened::Granted,
                    (true, Some(error)) => Opened::Stopped(stopped_words(error)),
                };
                let _gone = reply.send(opened);
            }
            (
                Owed::Call {
                    reply, response, ..
                },
                None,
            ) => {
                let _gone = reply.send(rpc::render(&response));
            }
            (Owed::Call { reply, id, .. }, Some(error)) => {
                let _gone = reply.send(rpc::render(&rpc::failure(&id, error)));
            }
        }
    }
}

/// The one thread that owns the surface.
///
/// Every job's answer waits until what the job left pending is written
/// ([`Writer`]'s rule): a write that fails answers that job with the typed
/// error instead, and the thread stops.
///
/// # Errors
///
/// As [`Writer::flush`] and [`Writer::close`]: a write the disk needed and
/// could not take stops the surface, which answers no further job; every
/// connection's door answers with the same error from then on.
fn serve_surface(
    surface: &mut Surface,
    writer: &Writer,
    in_process: &mut InProcessSeats,
    work: &Receiver<Job>,
    stopped: &OnceLock<HostStopped>,
    first: &Sender<Result<(), Error>>,
) -> Result<(), Error> {
    in_process.plan(surface);
    if let Err(error) = writer.flush(surface) {
        let error = writer.stop(surface, error, stopped);
        // `run_in` waits on this before it announces; a `run_in` that has
        // gone has nobody to tell.
        let _told = first.send(Err(error.clone()));
        return Err(error);
    }
    let _told = first.send(Ok(()));
    while let Ok(job) = work.recv() {
        let owed = match job {
            Job::Open { token, reply } => {
                let tick = surface.time().tick;
                let match_id = surface.match_id().to_owned();
                let granted = match surface.tokens().authenticate(&token, &match_id, tick) {
                    Ok(grant) => Some((grant.subject, grant.handle)),
                    Err(_) => None,
                };
                if let Some((subject, handle)) = granted {
                    surface.audit().record(
                        tick,
                        Some(subject),
                        Some(handle),
                        "upgrade",
                        crate::audit::Outcome::Ok,
                    );
                } else {
                    surface.audit().refused(
                        tick,
                        None,
                        None,
                        "upgrade",
                        &Error::unauthenticated("no such token"),
                    );
                }
                Some(Owed::Open {
                    reply,
                    granted: granted.is_some(),
                })
            }
            Job::Call { token, text, reply } => {
                let (id, response) = match rpc::parse(&text) {
                    Ok(request) => {
                        let vision = vision_of(surface);
                        let response = surface.call(Some(&token), &request, &vision);
                        (request.id, response)
                    }
                    Err(malformed) => (Json::Null, rpc::malformed_response(&malformed)),
                };
                Some(Owed::Call {
                    reply,
                    id,
                    response,
                })
            }
            Job::Stop => {
                return match writer.close(surface) {
                    Ok(()) => Ok(()),
                    Err(error) => Err(writer.stop(surface, error, stopped)),
                };
            }
            Job::PipeFailed(pipe) => {
                let error = match writer.close(surface) {
                    Ok(()) => pipe,
                    Err(closed) => {
                        Error::internal(said!("{}; and before it, {}", closed.said(), pipe.said()))
                    }
                };
                return Err(writer.stop(surface, error, stopped));
            }
            Job::Refused { reason, status } => {
                let tick = surface.time().tick;
                let error = match status {
                    401 => Error::unauthenticated(reason),
                    403 => Error::forbidden(reason),
                    503 => Error::rate_limited(reason),
                    _ => Error::invalid(reason),
                };
                surface.audit().refused(tick, None, None, "upgrade", &error);
                None
            }
        };
        in_process.plan(surface);
        match writer.flush(surface) {
            Ok(()) => {
                if let Some(owed) = owed {
                    owed.pay(None);
                }
            }
            Err(error) => {
                let error = writer.stop(surface, error, stopped);
                if let Some(owed) = owed {
                    owed.pay(Some(&error));
                }
                return Err(error);
            }
        }
    }
    // Every sender went away without a `Stop`: the process is going down
    // anyway, and what the surface holds is still worth the disk.
    match writer.close(surface) {
        Ok(()) => Ok(()),
        Err(error) => Err(writer.stop(surface, error, stopped)),
    }
}

/// What a seat can see right now, rebuilt before every call.
///
/// An owned snapshot rather than a long-lived object, because a sensor moves
/// whenever a unit walks, a beacon is placed or lost or a structure is
/// finished, and a stale one would be a fog leak with a delay on it. With no
/// match hosted there is no sensor at all, so the vision is blind: nothing
/// can be seen where nothing stands.
fn vision_of(surface: &Surface) -> LiveVision {
    surface.host().map_or_else(
        |_| LiveVision::default(),
        |host| LiveVision::of(host.world()),
    )
}

#[cfg(test)]
mod tests {
    use super::{Config, Drops, Writer, largest_save, open_surface};
    use crate::cache::{Header, MatchCache};
    use crate::error::Code;
    use crate::probation::Preset;
    use pharmakos_sim::math::quantity::Damage;
    use pharmakos_sim::tables::{SeatId, UnitId};
    use pharmakos_sim::world::{DamageOrder, DamageTarget};
    use std::sync::Arc;

    /// A save the surface could not build (`Persistence::unsaved`) stops the
    /// flush as a failed write does, in words that say what the disk holds
    /// (mutant N4a: a flush that ignored it would let the host serve a Push
    /// whose save is missing). Here a host order waits for a tick as the Push
    /// begins, so the `sealed` save cannot be built.
    #[test]
    fn a_save_the_surface_could_not_build_stops_the_flush() {
        let root = std::env::temp_dir()
            .join("pharmakos-gateway-tests")
            .join("serve")
            .join(format!("unsaved-{}", std::process::id()));
        let _fresh = std::fs::remove_dir_all(&root);
        let rules_json = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .expect("the shipped rules table");
        let config = Config {
            match_id: String::from("m-opsave-unsaved"),
            seed: 7,
            seats: 2,
            human_seat: None,
            segment_lengths_ms: vec![1_000],
            round_limit: 2,
            preset: Preset::Custom,
        };
        let cache = MatchCache::open(
            &root,
            &config.match_id,
            &Header {
                match_seed: config.seed,
                seats: config.seats,
                segment_lengths_ms: config.segment_lengths_ms.clone(),
                round_limit: config.round_limit,
                rules_hash: 0,
            },
        )
        .expect("a cache");
        let (mut surface, _) = open_surface(&rules_json, None, &config).expect("a match");
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
        assert!(surface.begin_push().expect("the Push begins"));
        let writer = Writer {
            cache,
            largest: largest_save(&config, &rules_json).expect("a bound"),
            config,
            rules_hash: 0,
            drops: Some(Arc::new(Drops::default())),
            drops_recorded: std::cell::Cell::new(false),
            audit_broken: std::cell::Cell::new(false),
        };
        let error = writer
            .flush(&mut surface)
            .expect_err("the save was not built");
        assert_eq!(error.code(), Code::Internal);
        assert!(
            error
                .message()
                .starts_with("the `sealed` save of round 1 could not be built ("),
            "{}",
            error.message()
        );
        assert!(
            error
                .message()
                .ends_with("; on disk is no save yet; this host stops"),
            "{}",
            error.message()
        );
        assert_eq!(
            error.message().matches("could not be built").count(),
            1,
            "the cause is said once (review 3, probe R3I): {}",
            error.message()
        );
        let _cleaned = std::fs::remove_dir_all(&root);
    }

    /// An external seat's host that dies in a Push leaves the `sealed` save
    /// its seals made, and the process that takes the match up replays that
    /// Push as it opens: the round's chain is byte for byte the chain of a
    /// host that never died, and the seat lands in the next Lull.
    #[test]
    fn an_external_match_that_died_in_a_push_replays_it_as_it_is_taken_up() {
        use super::{ConfigLine, External, NoOperators, Setup};
        let rules_json = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .expect("the shipped rules table");
        let config = Config {
            match_id: String::from("m-external-died"),
            seed: 7,
            seats: 2,
            human_seat: Some(0),
            segment_lengths_ms: vec![1_000],
            round_limit: 2,
            preset: Preset::Custom,
        };
        let setup = || Setup {
            rules_json: rules_json.clone(),
            library: None,
            operators: Box::new(NoOperators),
        };
        let ready = crate::rpc::request_text(
            &pharmakos_proto::json::Json::Number(String::from("1")),
            "set_ready",
            pharmakos_proto::json::Json::Object(Vec::new()),
        );
        let play = |name: &str, dies: bool| -> String {
            let root = std::env::temp_dir()
                .join("pharmakos-gateway-tests")
                .join("serve")
                .join(format!("{name}-{}", std::process::id()));
            let _fresh = std::fs::remove_dir_all(&root);
            let new = ConfigLine {
                config: config.clone(),
                resume: false,
            };
            let mut external = External::open(setup(), &root, &new).expect("the match opens");
            assert!(external.call(&ready).get("result").is_some());
            let external = if dies {
                // The seals are made and their save is on disk; then nothing:
                // no step, no close.
                assert!(external.surface.begin_push().expect("the Push begins"));
                external
                    .writer
                    .flush(&mut external.surface)
                    .expect("the sealed save lands");
                drop(external);
                let resume = ConfigLine {
                    config: config.clone(),
                    resume: true,
                };
                External::open(setup(), &root, &resume).expect("the match is taken up")
            } else {
                external.advance().expect("round 1 plays");
                external
            };
            let time = external.surface.time();
            assert_eq!(
                time.phase,
                pharmakos_proto::gp::api::v1::status::Phase::Lull
            );
            assert_eq!(time.round, 2);
            external.close().expect("put down in round 2's Lull");
            let chain = std::fs::read_to_string(
                root.join("matches")
                    .join(&config.match_id)
                    .join("replay")
                    .join("1.hashes.txt"),
            )
            .expect("round 1's chain");
            let _cleaned = std::fs::remove_dir_all(&root);
            chain
        };
        let whole = play("external-whole", false);
        let died = play("external-died", true);
        assert!(whole.lines().count() >= 20, "{whole}");
        assert_eq!(whole, died);
    }

    /// The stop's words over every combination (fix pass 3, item 2): each
    /// kind of first failure, none to five others, the audit log written or
    /// not. The log is cited only when it was written; otherwise the others
    /// are named, at most `NAMED_BESIDE`, with a count of the rest; singular
    /// and plural are right; no cause is said twice; the disk is said last.
    #[test]
    fn the_stop_words_say_what_happened_and_cite_no_unwritten_log() {
        use super::{Failure, NAMED_BESIDE, stop_words};
        use pharmakos_proto::said;
        let write = |what: pharmakos_proto::json::grammar::Said, cause: &'static str| Failure {
            what,
            cause: Some(pharmakos_proto::json::grammar::Said::from(cause)),
        };
        let firsts = [
            write(
                said!("the `sealed` save of round 1 could not be written"),
                "disk full",
            ),
            write(
                said!("seat 0's sealed playbook of round 1 could not be written"),
                "denied",
            ),
            write(
                said!("round 1's replay chain could not be written"),
                "denied",
            ),
            write(said!("the audit log could not be written"), "read-only"),
            Failure {
                what: said!("the `sealed` save of round 1 could not be built (an order waits)"),
                cause: None,
            },
        ];
        let other = |index: usize| write(said!("other write {} failed", index), "cause");
        for first in &firsts {
            for count in 0..=5 {
                for audit_written in [true, false] {
                    let others: Vec<Failure> = (0..count).map(other).collect();
                    let words = stop_words(first, &others, audit_written, &said!("no save yet"))
                        .to_string();
                    let case = format!("{count} others, log written {audit_written}: {words}");
                    assert!(words.starts_with(&first.what.to_string()), "{case}");
                    assert!(
                        words.ends_with("; on disk is no save yet; this host stops"),
                        "{case}"
                    );
                    if let Some(cause) = &first.cause {
                        assert_eq!(words.matches(&format!("({cause})")).count(), 1, "{case}");
                    }
                    assert_eq!(
                        words.matches(&first.what.to_string()).count(),
                        1,
                        "the first failure once: {case}"
                    );
                    let cites = words.contains("the audit log names");
                    assert_eq!(cites, audit_written && count > 0, "{case}");
                    match (count, audit_written) {
                        (0, _) => assert!(!words.contains("beside"), "{case}"),
                        (1, true) => assert!(
                            words.contains(
                                "(1 more write failed beside it: the audit log names it)"
                            ),
                            "{case}"
                        ),
                        (_, true) => assert!(
                            words.contains(&format!(
                                "({count} more writes failed beside it: the audit log names them)"
                            )),
                            "{case}"
                        ),
                        (_, false) => {
                            for index in 0..count.min(NAMED_BESIDE) {
                                assert!(
                                    words.contains(&format!("other write {index} failed (cause)")),
                                    "{case}"
                                );
                            }
                            for index in NAMED_BESIDE..count {
                                assert!(
                                    !words.contains(&format!("other write {index} failed")),
                                    "{case}"
                                );
                            }
                            match count.saturating_sub(NAMED_BESIDE) {
                                0 => assert!(!words.contains("more write"), "{case}"),
                                1 => assert!(words.contains("; and 1 more write)"), "{case}"),
                                rest => assert!(
                                    words.contains(&format!("; and {rest} more writes)")),
                                    "{case}"
                                ),
                            }
                        }
                    }
                }
            }
        }
    }
}
