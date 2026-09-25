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
//! <match id>\t<seed>\t<seats>\t<human seat|->\t<segment lengths|->\t<round limit>[\tresume]
//! ```
//!
//! The seventh field is optional and has one value, `resume` (decisions-log
//! item 111, decision C9): go on with the match saved under this id rather
//! than start a new one. A resume line repeats **all six** of the saved
//! match's values, the round limit included, and one that differs is refused
//! as [`crate::error::Code::InvalidArgument`] (item 112 (8)); so is a rules
//! table, verifier or snapshot format that is not the save's. [`ConfigLine`]
//! reads it.
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
//! schema would be freezing the wrong thing. PLACEHOLDER: **OWNER**, at v1.1,
//! decides whether it becomes part of anything published.
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

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};

use pharmakos_proto::json::Json;
use pharmakos_sim::tables::SeatId;

use pharmakos_proto::gp::api::v1::Method;

use crate::advice::Advice;
use crate::cache::MatchCache;
use crate::error::Error;
use crate::fog::FogPolicy;
use crate::host::{Host, Settings, SphereVision};
use crate::net::Listener;
use crate::rpc::{self, Request};
use crate::save::{Save, Stamp};
use crate::scopes::{Scope, ScopeSet};
use crate::session::{self, Door};
use crate::surface::{InProcess, Surface};
use crate::token::{Handle, Subject, Token};

/// How many connections the host serves at once.
///
/// PLACEHOLDER: 8 is a working number with a technical reason and no
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
/// human's feed as it found it. Anything off it is answered `FORBIDDEN_SCOPE`
/// and audited (`an_advisor_is_refused_every_method_off_its_allow_list`).
pub const ADVISOR_METHODS: [Method; 18] = [
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
    Method::ListTemplates,
    Method::InstantiateTemplate,
    Method::VerifyPlan,
    Method::RenderPlan,
    Method::PatchPlan,
    Method::GetSafePlan,
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
}

impl Config {
    /// Read one config line's first six fields.
    ///
    /// A seventh field is not read here: [`ConfigLine::parse`] reads it, and
    /// is what the host loop uses.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a line with too few fields
    /// or a field this host cannot read, and for a seat count outside `1` to
    /// [`crate::host::MAX_SEATS`]. Nothing here is silently defaulted: the
    /// lobby is a program, and a host that guessed what it meant would start
    /// the wrong match.
    pub fn parse(line: &str) -> Result<Config, Error> {
        let fields: Vec<&str> = line.trim_end_matches(['\r', '\n']).split('\t').collect();
        let field = |index: usize, name: &str| -> Result<String, Error> {
            fields
                .get(index)
                .map(|text| (*text).to_owned())
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "the config line has no field {index} (`{name}`); it is six tab-separated \
                         fields"
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
                Error::invalid(format!(
                    "`{seed_text}` is not a match seed: `0x…` or decimal"
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
                    Error::invalid(format!(
                        "`{piece}` is not a segment length in game milliseconds"
                    ))
                })?);
            }
            found
        };
        let round_limit = field(5, "round limit")?
            .parse::<u32>()
            .map_err(|_| Error::invalid("`round limit` is how many rounds the match runs"))?;
        // Decisions-log item 110 (5): v1 plays one to three seats, and a lobby
        // that asked for more is told so here, before a map is generated.
        Host::check_seats(seats)?;
        if let Some(seat) = human_seat {
            if u32::from(seat) >= seats {
                return Err(Error::invalid(format!(
                    "this match has {seats} seats and the human is seat {seat}"
                )));
            }
        }
        Ok(Config {
            match_id,
            seed,
            seats,
            human_seat,
            segment_lengths_ms,
            round_limit,
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
            "{}\t{}\t{}\t{human}\t{lengths}\t{}\n",
            self.match_id,
            crate::view::seed_text(self.seed),
            self.seats,
            self.round_limit
        )
    }
}

/// A whole config line: the match's six values, and whether to resume it.
///
/// A type of its own rather than a seventh field on [`Config`], so that every
/// caller that builds a [`Config`] keeps building one (decisions-log item
/// 111, decision C9: the config line grows, its six values do not change).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConfigLine {
    /// The six values.
    pub config: Config,
    /// True for a line whose seventh field is `resume`: go on with the match
    /// saved under this id.
    pub resume: bool,
}

impl ConfigLine {
    /// Read one config line, with its optional seventh field.
    ///
    /// # Errors
    ///
    /// As [`Config::parse`], and [`crate::error::Code::InvalidArgument`] for a
    /// seventh field that is anything but `resume`, `-` or empty.
    pub fn parse(line: &str) -> Result<ConfigLine, Error> {
        let config = Config::parse(line)?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let seventh = trimmed.split('\t').nth(6).unwrap_or("");
        let resume = match seventh {
            "" | "-" => false,
            "resume" => true,
            other => {
                return Err(Error::invalid(format!(
                    "`{other}` is not a seventh config field this host reads: it is `resume`, or \
                     nothing for a new match"
                )));
            }
        };
        Ok(ConfigLine { config, resume })
    }

    /// The line a lobby writes: six fields for a new match, seven for a
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
        let seat = fields.get(3).copied().unwrap_or("-");
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
    Open { token: Token, reply: Sender<bool> },
    /// Answer one JSON-RPC text message.
    Call {
        token: Token,
        text: String,
        reply: Sender<String>,
    },
    /// Record a refused upgrade.
    Refused { reason: String, status: u16 },
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
}

/// A connection's door: everything goes to the one thread that owns the
/// surface, and the connection waits for the answer.
struct RemoteDoor {
    jobs: SyncSender<Job>,
    replies: (Sender<String>, Receiver<String>),
    opens: (Sender<bool>, Receiver<bool>),
}

impl RemoteDoor {
    fn new(jobs: SyncSender<Job>) -> RemoteDoor {
        RemoteDoor {
            jobs,
            replies: channel(),
            opens: channel(),
        }
    }
}

impl Door for RemoteDoor {
    fn open(&mut self, token: &Token) -> bool {
        let job = Job::Open {
            token: token.clone(),
            reply: self.opens.0.clone(),
        };
        if self.jobs.send(job).is_err() {
            return false;
        }
        self.opens.1.recv().unwrap_or(false)
    }

    fn answer(&mut self, token: &Token, text: &str) -> String {
        let job = Job::Call {
            token: token.clone(),
            text: text.to_owned(),
            reply: self.replies.0.clone(),
        };
        if self.jobs.send(job).is_err() {
            return rpc::render(&rpc::malformed_response(&rpc::Malformed::Invalid(
                String::from("this host is shutting down"),
            )));
        }
        self.replies.1.recv().unwrap_or_else(|_| {
            rpc::render(&rpc::malformed_response(&rpc::Malformed::Invalid(
                String::from("this host is shutting down"),
            )))
        })
    }

    fn refused(&mut self, refusal: &crate::handshake::Refusal) {
        let error = session::refusal_error(refusal);
        let _ = self.jobs.send(Job::Refused {
            reason: error.message,
            status: refusal.status(),
        });
    }
}

/// Host one match until `control` reaches end of file, keeping its private
/// cache in the OS-standard per-user data directory ([`crate::cache::locate`]).
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
/// shows, never a fallback beside the executable).
pub fn run<C: Read, A: Write>(setup: Setup, control: C, announce: A) -> Result<(), Error> {
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
pub fn run_in<C: Read, A: Write>(
    setup: Setup,
    data_root: &Path,
    control: C,
    mut announce: A,
) -> Result<(), Error> {
    let mut lines = BufReader::new(control);
    let line = read_config(&mut lines)?;
    let config = line.config.clone();
    let rules_hash = RulesHash::of(&setup.rules_json)?;
    let mut operators = setup.operators;

    // The save is read before the surface is opened (the wave-6 notes, H16),
    // and a new match is refused before a map is generated if its id already
    // has one.
    let (cache, mut surface, seats) = if line.resume {
        let cache = MatchCache::reopen(data_root, &config.match_id)?;
        let save = Save::parse(&cache.read_save()?)?;
        save.check(&config, rules_hash.0)?;
        let (surface, seats) = resume_surface(&setup.rules_json, setup.library, &config, &save)?;
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
        let (surface, seats) = open_surface(&setup.rules_json, setup.library, &config)?;
        (cache, surface, seats)
    };
    surface.audit().continue_after(cache.last_audit_seq());
    if line.resume {
        let tick = surface.time().tick;
        surface
            .audit()
            .record(tick, None, None, "resumed", crate::audit::Outcome::Ok);
    }

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
    announce
        .write_all(written.as_bytes())
        .and_then(|()| announce.flush())
        .map_err(|error| Error::internal(format!("writing the announce line: {error}")))?;

    // The private match cache was opened above. The audit log, the saves and
    // the replay's inputs are flushed into it as the match runs, which is also
    // what makes
    // `a_token_appears_on_the_announce_line_and_nowhere_else_the_host_writes`
    // a test of something rather than of nothing.
    let (jobs, work) = sync_channel::<Job>(QUEUE_DEPTH);
    let writer = Writer {
        cache,
        config,
        rules_hash: rules_hash.0,
    };
    let surface_thread = std::thread::spawn(move || {
        serve_surface(&mut surface, &writer, &mut in_process, &work);
    });
    let dispatcher = spawn_dispatcher(listener.accept(), policy, jobs.clone());

    // The parent holds this pipe open for the life of the match. Reading it to
    // end of file is the whole of the shutdown protocol -- no heartbeat, no
    // clock, nothing to time out.
    let mut rest: Vec<u8> = Vec::new();
    let _ = lines.read_to_end(&mut rest);

    // The surface thread flushes the audit log on its way out, so it is joined
    // rather than abandoned. The accept threads are not: they are blocked in
    // `accept`, and `gamectl host` exiting is what ends them.
    let _ = jobs.send(Job::Stop);
    drop(jobs);
    let _ = surface_thread.join();
    drop(dispatcher);
    Ok(())
}

/// The rules table's hash, read off the text the binary handed over.
struct RulesHash(u64);

impl RulesHash {
    fn of(rules_json: &str) -> Result<RulesHash, Error> {
        pharmakos_sim::rules::RulesTable::from_canonical_json(rules_json)
            .map(|table| RulesHash(table.rules_hash()))
            .map_err(|error| {
                Error::invalid(format!(
                    "this is not a rules table this build reads: {error}"
                ))
            })
    }
}

/// Read the one config line the parent writes.
fn read_config<C: Read>(lines: &mut BufReader<C>) -> Result<ConfigLine, Error> {
    let mut first = String::new();
    lines
        .read_line(&mut first)
        .map_err(|error| Error::internal(format!("reading the config line: {error}")))?;
    if first.trim().is_empty() {
        return Err(Error::invalid(
            "this host is started by one config line on its standard input, and the line was empty",
        ));
    }
    ConfigLine::parse(&first)
}

/// Open the match and the surface over it, in its opening Lull.
fn open_surface(
    rules_json: &str,
    library: Option<PathBuf>,
    config: &Config,
) -> Result<(Surface, Vec<SeatId>), Error> {
    let host = Host::open_from(
        rules_json,
        config.seed,
        config.seats,
        &Settings {
            segment_lengths_ms: config.segment_lengths_ms.clone(),
            round_limit: config.round_limit,
            units_per_seat: 0,
        },
        library,
    )?;
    let rules = host.rules().clone();
    let seats: Vec<SeatId> = (0..config.seats)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect();
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
    )?;
    surface.attach(host)?;
    surface.open_lull()?;
    Ok((surface, seats))
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
) -> Result<(Surface, Vec<SeatId>), Error> {
    let host = Host::open_from(
        rules_json,
        config.seed,
        config.seats,
        &Settings {
            segment_lengths_ms: config.segment_lengths_ms.clone(),
            round_limit: config.round_limit,
            units_per_seat: 0,
        },
        library,
    )?;
    let rules = host.rules().clone();
    let seats: Vec<SeatId> = (0..config.seats)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect();
    let surface = Surface::resume(
        &config.match_id,
        config.seed,
        rules,
        // Fogged, always, as `open_surface` says.
        FogPolicy::fogged(),
        &seats,
        host,
        &save.state,
    )?;
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
/// than by token, and this demo has a human seat. PLACEHOLDER: **OWNER**, at
/// **S5**, with the built-in-only match flow.
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
            if Some(raw) != human_seat {
                if let Some(operator) = operators.built_in(raw) {
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
        let error = Error::forbidden(format!(
            "the built-in operator advising seat {seat} may not call `{}`: an advisor reads and \
             never writes, and a seat's notebook, drafts, submission and readiness are its own",
            method.map_or_else(
                || String::from("<unknown>"),
                crate::scopes::method_wire_name
            )
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

/// Accept connections, bounded by [`MAX_CONNECTIONS`], one reader thread each.
fn spawn_dispatcher(
    accepting: crate::net::Accepting,
    policy: crate::handshake::Policy,
    jobs: SyncSender<Job>,
) -> std::thread::JoinHandle<()> {
    let live = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        while let Some(connection) = accepting.recv() {
            let held = live.fetch_add(1, Ordering::SeqCst);
            if held >= MAX_CONNECTIONS {
                live.fetch_sub(1, Ordering::SeqCst);
                // Refused before the upgrade, and audited: a connection the
                // host would not serve is still a connection somebody made.
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
                    reason: format!(
                        "this host serves at most {MAX_CONNECTIONS} connections at once"
                    ),
                    status: 503,
                });
                continue;
            }
            let jobs = jobs.clone();
            let live = Arc::clone(&live);
            let policy = policy.clone();
            std::thread::spawn(move || {
                let mut door = RemoteDoor::new(jobs);
                let _ = session::serve_connection_through(connection, &mut door, &policy);
                live.fetch_sub(1, Ordering::SeqCst);
            });
        }
    })
}

/// What the surface thread writes into the private match cache, and with.
struct Writer {
    cache: MatchCache,
    config: Config,
    rules_hash: u64,
}

impl Writer {
    /// Write whatever the surface has pending -- the seals and the save a
    /// Push's beginning left, the chain a segment's end left -- and then the
    /// audit log, so that a write that failed is itself in the log.
    fn flush(&self, surface: &mut Surface) {
        let pending = surface.take_persistence();
        let mut refused: Vec<(&'static str, Error)> = Vec::new();
        for file in &pending.sealed {
            if let Err(error) = self
                .cache
                .write_sealed(file.seat, file.round, &file.playbook_jsonc)
            {
                refused.push(("write sealed playbook", error));
            }
        }
        for chain in &pending.chains {
            if let Err(error) = self.cache.write_replay(chain.round, &chain.render()) {
                refused.push(("write replay chain", error));
            }
        }
        if let Some(state) = pending.save {
            if let Err(error) = self.write_save(state) {
                refused.push(("write save", error));
            }
        }
        let tick = surface.time().tick;
        for (action, error) in &refused {
            surface.audit().refused(tick, None, None, *action, error);
        }
        let _ = self.cache.append_audit(&surface.audit().take());
    }

    fn write_save(&self, state: crate::save::SavedMatch) -> Result<(), Error> {
        let save = Save {
            stamp: Stamp::current(self.rules_hash),
            config: self.config.clone(),
            state,
        };
        self.cache.write_save(&save.render())
    }

    /// The control pipe reached end of file: save a Lull, and nothing else.
    ///
    /// Item 84's second boundary. A Push, a recap and an ended match are not
    /// saved here: the save made when the Push began stands, and resuming it
    /// replays that Push (the wave-6 notes, decision C8).
    fn close(&self, surface: &mut Surface) {
        if surface.time().phase == pharmakos_proto::gp::api::v1::status::Phase::Lull {
            let tick = surface.time().tick;
            match surface.lull_save() {
                Ok(state) => match self.write_save(state) {
                    Ok(()) => surface.audit().record(
                        tick,
                        None,
                        None,
                        "save lull",
                        crate::audit::Outcome::Ok,
                    ),
                    Err(error) => surface
                        .audit()
                        .refused(tick, None, None, "save lull", &error),
                },
                Err(error) => surface
                    .audit()
                    .refused(tick, None, None, "save lull", &error),
            };
        }
        self.flush(surface);
    }
}

/// The one thread that owns the surface.
fn serve_surface(
    surface: &mut Surface,
    writer: &Writer,
    in_process: &mut InProcessSeats,
    work: &Receiver<Job>,
) {
    in_process.plan(surface);
    writer.flush(surface);
    while let Ok(job) = work.recv() {
        match job {
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
                    let _ = reply.send(true);
                } else {
                    surface.audit().refused(
                        tick,
                        None,
                        None,
                        "upgrade",
                        &Error::unauthenticated("no such token"),
                    );
                    let _ = reply.send(false);
                }
            }
            Job::Call { token, text, reply } => {
                let response = match rpc::parse(&text) {
                    Ok(request) => {
                        let vision = vision_of(surface);
                        surface.call(Some(&token), &request, &vision)
                    }
                    Err(malformed) => rpc::malformed_response(&malformed),
                };
                let _ = reply.send(rpc::render(&response));
            }
            Job::Stop => {
                writer.close(surface);
                return;
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
            }
        }
        in_process.plan(surface);
        writer.flush(surface);
    }
    // Every sender went away without a `Stop`: the process is going down
    // anyway, and what the surface holds is still worth the disk.
    writer.close(surface);
}

/// What a seat can see right now, rebuilt before every call.
///
/// An owned snapshot rather than a long-lived object, because a sphere moves
/// when a beacon is placed or lost and a stale one would be a fog leak with a
/// delay on it.
fn vision_of(surface: &Surface) -> SphereVision {
    surface.host().map_or_else(
        |_| SphereVision::default(),
        |host| SphereVision::of(host.world()),
    )
}
