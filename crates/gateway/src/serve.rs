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
//! <match id>\t<seed>\t<seats>\t<human seat|->\t<segment lengths|->\t<round limit>
//! ```
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
//! # Bounds
//!
//! One request in flight per connection ([`crate::session`] waits for each
//! answer before reading the next frame), a bounded queue in front of the
//! surface thread, and at most [`MAX_CONNECTIONS`] live connections with an
//! audited 503 beyond that. A flood of silent sockets therefore fills the
//! pending slots, times out and disturbs nothing that has authenticated.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};

use pharmakos_proto::json::Json;
use pharmakos_sim::tables::SeatId;

use crate::error::Error;
use crate::fog::FogPolicy;
use crate::host::{Host, Settings, SphereVision};
use crate::net::Listener;
use crate::rpc::{self, Request};
use crate::scopes::{Scope, ScopeSet};
use crate::session::{self, Door};
use crate::surface::Surface;
use crate::token::{Subject, Token};

/// How many connections the host serves at once.
///
/// PLACEHOLDER: 8 is a working number with a technical reason and no
/// measurement. **A connection costs a thread**, because the transport is
/// blocking threads and `std::sync` channels (decisions-log item 99) and not
/// an async runtime; eight covers three seats, an editor, a lobby, a spectator
/// and two spare, which is more than the whole of v1 has clients. **OWNER**,
/// at hardening, with `READ_TIMEOUT`, `MAX_MESSAGE_BYTES` and the rate limits.
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
/// so the operator is rate-limited, audited and fog-filtered exactly like a
/// socket, and it never sees a token, the surface or a `pharmakos-sim` type.
///
/// A seat that files nothing gets the safe playbook at `begin_push`, which the
/// surface already does -- so an empty list, which is what the skeleton ships,
/// is a match two built-in seats play safely rather than a match that will not
/// start. An implementation that wants the Lull to end on "all ready" must end
/// by calling `set_ready`.
pub trait BuiltInSeat: Send {
    /// Plan this seat's round.
    fn plan(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json);
}

/// What the binary hands the host loop.
pub struct Setup {
    /// The rules table's canonical JSON, as the binary read it off disk. Text,
    /// not a path and not a parsed table: see the module docs.
    pub rules_json: String,
    /// The template library folder, when the binary was given one.
    pub library: Option<PathBuf>,
    /// The seats this process plays itself. Empty in the skeleton.
    pub built_in: Vec<Box<dyn BuiltInSeat>>,
}

impl std::fmt::Debug for Setup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Setup")
            .field("rules_json", &self.rules_json.len())
            .field("library", &self.library)
            .field("built_in", &self.built_in.len())
            .finish()
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
    /// Read one config line.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a line with too few fields
    /// or a field this host cannot read. Nothing here is silently defaulted:
    /// the lobby is a program, and a host that guessed what it meant would
    /// start the wrong match.
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

/// Host one match until `control` reaches end of file.
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] for a config line this host cannot
/// read or a lobby setting it refuses, and
/// [`crate::error::Code::Internal`] when the map will not generate, no
/// loopback address can be bound, the operating system will not give entropy
/// for a token, or there is nowhere standard to keep the private match cache
/// (decisions-log item 98: a missing variable is an error the lobby shows,
/// never a fallback beside the executable).
pub fn run<C: Read, A: Write>(setup: Setup, control: C, mut announce: A) -> Result<(), Error> {
    let mut lines = BufReader::new(control);
    let config = read_config(&mut lines)?;
    let mut built_in = setup.built_in;
    let (mut surface, seats) = open_surface(&setup.rules_json, setup.library, &config)?;

    let listener = Listener::bind(0)?;
    let port = listener.port();
    let policy = listener.policy();

    let minted = mint_tokens(&mut surface, &config, &seats, built_in.len())?;
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

    // The private match cache. The audit log is flushed into it as the match
    // runs, which is also what makes
    // `a_token_appears_on_the_announce_line_and_nowhere_else_the_host_writes`
    // a test of something rather than of nothing.
    let cache =
        crate::cache::MatchCache::open(&crate::cache::locate()?, &config.match_id, config.seed)?;
    let tokens = minted.built_in;

    let (jobs, work) = sync_channel::<Job>(QUEUE_DEPTH);
    let surface_thread = std::thread::spawn(move || {
        serve_surface(&mut surface, &cache, &mut built_in, &tokens, &work);
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

/// Read the one config line the parent writes.
fn read_config<C: Read>(lines: &mut BufReader<C>) -> Result<Config, Error> {
    let mut first = String::new();
    lines
        .read_line(&mut first)
        .map_err(|error| Error::internal(format!("reading the config line: {error}")))?;
    if first.trim().is_empty() {
        return Err(Error::invalid(
            "this host is started by one config line on its standard input, and the line was empty",
        ));
    }
    Config::parse(&first)
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

/// Every token this host mints, and the only time any of them is handed out.
struct Minted {
    admin: Token,
    seat: Option<Token>,
    built_in: Vec<(u8, Token)>,
}

/// Mint the lobby's token, the human seat's, and one per built-in seat.
///
/// **No spectator token is minted.** Spec section 5 gives the spectator camera
/// to built-in-only matches and unlocks an eliminated human by *policy* rather
/// than by token, and this demo has a human seat. PLACEHOLDER: **OWNER**, at
/// **S5**, with the built-in-only match flow.
fn mint_tokens(
    surface: &mut Surface,
    config: &Config,
    seats: &[SeatId],
    built_in: usize,
) -> Result<Minted, Error> {
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
    let seat_scopes = ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs]);
    let seat = match config.human_seat {
        Some(raw) => {
            let (token, _) = surface.tokens().mint(
                Subject::Seat(SeatId::new(raw)),
                &config.match_id,
                seat_scopes,
                minted_at,
            )?;
            Some(token)
        }
        None => None,
    };

    // A token per built-in seat, minted here and never leaving this process:
    // the operator comes through `Surface::call` like a socket client.
    let spare: Vec<SeatId> = seats
        .iter()
        .copied()
        .filter(|held| Some(held.raw()) != config.human_seat)
        .collect();
    if built_in > spare.len() {
        return Err(Error::invalid(format!(
            "this match has {} seats for a built-in operator and {built_in} were given",
            spare.len()
        )));
    }
    let mut played: Vec<(u8, Token)> = Vec::with_capacity(built_in);
    for held in spare.into_iter().take(built_in) {
        let (token, _) = surface.tokens().mint(
            Subject::Seat(held),
            &config.match_id,
            seat_scopes,
            minted_at,
        )?;
        played.push((held.raw(), token));
    }
    Ok(Minted {
        admin,
        seat,
        built_in: played,
    })
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
                let _ = jobs.send(Job::Refused {
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

/// The one thread that owns the surface.
fn serve_surface(
    surface: &mut Surface,
    cache: &crate::cache::MatchCache,
    built_in: &mut [Box<dyn BuiltInSeat>],
    tokens: &[(u8, Token)],
    work: &Receiver<Job>,
) {
    let mut planned: u32 = 0;
    plan_built_in(surface, built_in, tokens, &mut planned);
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
            Job::Stop => break,
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
        plan_built_in(surface, built_in, tokens, &mut planned);
        let _ = cache.append_audit(&surface.audit().take());
    }
    let _ = cache.append_audit(&surface.audit().take());
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

/// Hand each built-in seat its round, once per Lull.
///
/// The closure it is given **is** `Surface::call` bound to that seat's own
/// token, so a built-in seat goes through the same seven steps a socket does.
fn plan_built_in(
    surface: &mut Surface,
    built_in: &mut [Box<dyn BuiltInSeat>],
    tokens: &[(u8, Token)],
    planned: &mut u32,
) {
    if built_in.is_empty() {
        return;
    }
    let time = surface.time();
    if time.phase != pharmakos_proto::gp::api::v1::status::Phase::Lull || time.round == *planned {
        return;
    }
    *planned = time.round;
    for (index, seat) in built_in.iter_mut().enumerate() {
        let Some((raw, token)) = tokens.get(index) else {
            continue;
        };
        let mut call = |request: &Request| -> Json {
            let vision = vision_of(surface);
            surface.call(Some(token), request, &vision)
        };
        seat.plan(*raw, &mut call);
    }
}
