// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl scenario run <file>` — harness part 2's runner.
//!
//! AGENTS.md §9 item 10 and §10 item 4: a headless match from a scenario file,
//! asserting on events **and** on hashes, "not just 'it didn't crash'". This
//! module plays the match and checks the claims; `crate::scenario` reads the
//! file.
//!
//! # It hosts through the gateway, and that is the whole design
//!
//! Decisions-log item 106 (3) defines *the client path* for this task: a match
//! hosted by the gateway's [`Host`] behind a [`Surface`], with each seat's
//! playbook arriving through `submit_plan` and sealed by
//! [`Surface::begin_push`]. **This runner takes the same path**, in process and
//! without a socket in front of it — `Surface::call` is where a decoded
//! JSON-RPC request arrives, and it is the same call a live session makes after
//! the framing comes off.
//!
//! It matters because of what the alternative would have proved. A runner that
//! drove the sim's own runner directly would be a *second* way to play a match:
//! it would host without a Lull, seal without the verifier's door, and its hash
//! chain would be a claim about a code path no player ever takes. Spec §15 asks
//! this harness for "hash parity with the client path", and parity with
//! something only the harness runs is not parity. `tests/parity.rs` is the
//! assertion: the chain this module produces equals the chain of the same match
//! driven over a live WebSocket session, for the same seed, rules and
//! playbooks.
//!
//! The crate map gives `gamectl` no edge to the sim's `Runner`, and nothing
//! here *drives* one: the guard is against driving it, and that is the claim
//! `tests/confinement.rs` makes over the source text. Said exactly, because a
//! review found the earlier wording stronger than the test — this file does
//! **read** the runner, through `Host::runner()`, and the world, through
//! `Host::world()`, for what a report needs and nothing else: the tick a Lull
//! is standing on (`tick()`), the phase a refused `begin_push` was in and
//! whether a match played to its end has ended (`phase()`), the round a seal
//! belongs to, a refusal is named in and a nightly match reached (`round()`),
//! how the match ended (`outcome()`), and on a shared win the final audit's
//! tied seats (`audit::final_audit(host.world())`, a pure read). `Host::runner`
//! hands out a `&Runner` and `Host::world` a `&World`, and every stepping
//! method needs `&mut`, so the reach cannot become a second way to play a
//! match; if `Surface` gains accessors for these, this file should take them
//! and `.runner()` should join the needles.
//!
//! # The match settings a file may name
//!
//! One input of a match beyond the triple is the scenario's own key since S1's
//! first contract pull request (the register's S1-41), and optional. Its
//! sibling `units_per_seat`, the harness walkers, went with them (S2-16, S2's
//! plan decision 23).
//!
//! * `round_limit` is the sim's
//!   [`DEFAULT_ROUND_LIMIT`](pharmakos_sim::runner::DEFAULT_ROUND_LIMIT), the
//!   spec's default and a lobby setting rather than a tuning row
//!   (decisions-log item 102 (7)), when the file names none; a scenario that
//!   plays a match to its end names it, and both readers refuse a limit below
//!   the file's segment count.
//!
//! Absent, both are what every committed chain was produced with, so the keys
//! moved no chain. `scenarios/README.md` documents them.
//!
//! # A `builtin` seat plays Easy (T18b)
//!
//! Decisions-log item 113 (8): a `builtin` seat is seated exactly as
//! `gamectl host` seats a seat nobody at the machine plays — the built-in
//! operator's factory, [`EasyOperators`], built from the **text** of the rules
//! table the scenario names (decision C17), handed to
//! [`serve::InProcessSeats::open`] with the scenario's `builtin` seats and no
//! human seat, so no seat is advised and a `safe` seat still gets the
//! gateway's fallback. It then plans in every Lull through its seat's own
//! in-process token — the same door, audit and fog as a socket — and submits
//! and says ready like any client. The host opens the repository's template
//! library ([`crate::host::LIBRARY_PATH`]) only when the scenario has a
//! `builtin` seat, because Easy instantiates its templates there; a scenario
//! with none opens exactly as it did before, so its chain cannot move.
//!
//! A chain from a scenario with a `builtin` seat is therefore a claim about
//! the operator (`crates/operator`) and the templates (`library/`) as well as
//! about the map seed, the playbooks and the rules hash, and moves when
//! either does (`tests/golden/scenarios/README.md`).
//!
//! PLACEHOLDER: a `builtin` seat plays Easy — owner, S5, with the `operator`
//! scenario key that would name another difficulty.

use std::fmt::Write as _;
use std::path::Path;

use pharmakos_gateway::fog::{Audience, Blind, FogPolicy};
use pharmakos_gateway::host::Host;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::serve;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{MatchPhase, MatchSettings};
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::WorldConfig;

use crate::cli::RunOptions;
use crate::exit::{Exit, Failure};
use crate::host::EasyOperators;
use crate::scenario::{Assertion, Scenario, SeatKind, ticks_of};
use crate::strings;

/// The most of the hash chain this runner reserves up front, in bytes.
///
/// Sixteen megabytes is about seven hundred thousand ticks, which is nine and
/// a half hours of game time — past anything a scenario plays and short of
/// anything that hurts. It caps the *reservation* only.
///
/// The format caps each segment at eight minutes since S1's first contract
/// pull request (`crate::scenario::MAX_SEGMENT_MS`, the register's X-01, held
/// in `xtask/src/scenario.rs` too), so no single length asks for gigabytes
/// any more; this bound stays for a file that lists many segments, which the
/// format does not cap.
const CHAIN_RESERVE_CAP: u64 = 16 * 1024 * 1024;

/// The match id every scenario run hosts under.
///
/// Fixed, and lower-case letters and hyphens only because
/// `MatchCache::valid_match_id` says so. It reaches nothing hashed — the sim is
/// a function of (map seed, playbooks, rules hash) and this is none of the
/// three — so a scenario does not name it.
const MATCH_ID: &str = "m-scenario";

/// One thing that happened, as an assertion reads it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fired {
    /// The sim tick it was reported on.
    pub tick: u32,
    /// The event kind's name.
    pub kind: String,
    /// Whose it was, when it was anybody's.
    pub seat: Option<u8>,
}

/// What one run produced.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Played {
    /// The per-tick xxh3 chain, `<tick>\t<16 hex digits>` per line, LF, with a
    /// trailing newline — the format `tests/golden/determinism/README.md`
    /// fixes and `tests/golden/scenarios/` shares.
    pub chain: String,
    /// Every event, in the order the bus reported them.
    pub events: Vec<Fired>,
    /// How many ticks each segment actually played.
    pub segments: Vec<u32>,
    /// Lines the run owes the reader whatever its assertions did: today, one
    /// per seat whose playbook could not go through `submit_plan` (see
    /// [`Door`]). Printed at the head of the report, above the assertions,
    /// because a chain produced behind the verifier's door is a different
    /// claim from one produced in front of it.
    pub notes: Vec<String>,
    /// The rules table the match ran under.
    pub rules_hash: u64,
}

impl Played {
    /// How many ticks were played in total.
    #[must_use]
    pub fn ticks(&self) -> u32 {
        self.segments
            .iter()
            .copied()
            .fold(0_u32, u32::saturating_add)
    }
}

/// `scenario run`, whichever options it was given: over a seed file
/// ([`crate::scenario::nightly`]) when `--seeds` names one, and otherwise
/// [`run`], exactly as before the options existed.
///
/// # Errors
///
/// As [`run`] and [`crate::scenario::nightly::run`], and [`Exit::Input`] for
/// an `--operator` given for a scenario that seats no `builtin` seat, which is
/// refused rather than ignored.
pub fn command(root: &Path, source: &Path, options: &RunOptions) -> Result<String, Failure> {
    if options.seeds.is_some() {
        return crate::scenario::nightly::run(root, source, options);
    }
    if options.operator.is_some() {
        let scenario = crate::scenario::load(root, source)?;
        if builtin_seats(&scenario).is_empty() {
            return Err(Failure::input(strings::operator_unused(&scenario.source)));
        }
    }
    run(root, source)
}

/// Play one scenario, check its assertions and render the report.
///
/// # Errors
///
/// [`Exit::Input`] for a file that is not a valid scenario, [`Exit::Failed`]
/// when an assertion did not hold, [`Exit::Internal`] when the gateway refuses
/// something no scenario could have caused.
pub fn run(root: &Path, source: &Path) -> Result<String, Failure> {
    let scenario = crate::scenario::load(root, source)?;
    let played = play(root, &scenario)?;
    let (report, failed) = check(root, &scenario, &played)?;
    if failed == 0 {
        Ok(report)
    } else {
        Err(Failure::new(Exit::Failed, report))
    }
}

/// Play one scenario and return what happened, asserting nothing.
///
/// Public because `tests/parity.rs` compares this chain with the one a live
/// WebSocket session produces, and because `tests/scenarios.rs` writes the
/// fresh `actual.hashes.txt` the `golden` step compares.
///
/// # Errors
///
/// [`Exit::Input`] for an input the scenario names and this build cannot use
/// (a playbook the verifier refuses and cannot compile, a rules table Easy
/// cannot score with for a `builtin` seat), and [`Exit::Internal`] for
/// anything the gateway refuses that no scenario could have caused.
pub fn play(root: &Path, scenario: &Scenario) -> Result<Played, Failure> {
    play_until(root, scenario, Until::Segments)
}

/// How a match ended, read from the sim's own outcome once it has (the
/// nightly result's per-seed half; S2's plan, task `night`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ending {
    /// The winners, ascending: the last seat standing or the final audit's
    /// single winner, or every seat that tied on the audit's every term on a
    /// shared win ([`pharmakos_sim::audit::final_audit`]).
    pub winners: Vec<u8>,
    /// Why it ended: `last_seat_standing`, `no_survivor` or `round_limit`, the
    /// sim's own names.
    pub reason: &'static str,
    /// The tick it ended on.
    pub end_tick: u32,
    /// The rounds played, the last one included.
    pub rounds: u32,
}

/// Play one scenario **to the match's end** and say how it ended, asserting
/// nothing: what `--seeds` does once per seed.
///
/// The file's segments are the match's segment lengths, as they are for
/// [`play`], and rounds past the last listed segment take the last length, as
/// the sim's ladder does (`MatchSettings::segment_lengths_ms`); the match
/// plays until the sim decides it, which its round limit bounds. Every
/// `playbook` seat goes through `submit_plan` every round: a refusal is a
/// broken arm, [`Exit::Failed`] naming the round and the codes, and never the
/// harness door, because a nightly result is a claim about a match a player
/// could have played.
///
/// # Errors
///
/// As [`play`], and [`Exit::Failed`] for a playbook `submit_plan` refused.
pub fn play_to_end(root: &Path, scenario: &Scenario) -> Result<(Played, Ending), Failure> {
    let mut ending = None;
    let played = play_until(root, scenario, Until::MatchEnd(&mut ending))?;
    let ending = ending.ok_or_else(|| {
        Failure::internal("a match played to its end did not report how it ended")
    })?;
    Ok((played, ending))
}

/// How far [`play_until`] plays.
enum Until<'a> {
    /// The file's segments, each to its full length, or the run fails.
    Segments,
    /// The match's end, wherever the sim puts it; the ending goes here.
    MatchEnd(&'a mut Option<Ending>),
}

fn play_until(root: &Path, scenario: &Scenario, mut until: Until<'_>) -> Result<Played, Failure> {
    let rules = load_rules(root, scenario)?;
    let rules_hash = rules.rules_hash();
    // Before the match is opened, so a rules table Easy cannot score with is
    // refused without generating a map.
    let mut factory = easy_for_builtin(root, scenario)?;
    let mut surface = open(root, scenario, rules)?;
    let tokens = mint(&mut surface, scenario)?;
    // After the runner's own tokens and before the first `begin_push`: Easy,
    // for the `builtin` seats alone (see the module docs).
    let mut builtin = match factory.as_mut() {
        Some(factory) => Some(
            serve::InProcessSeats::open(&mut surface, &builtin_seats(scenario), None, factory)
                .map_err(internal)?,
        ),
        None => None,
    };

    // Reserved, and capped. A line is twenty-four bytes, so the reservation is
    // the chain's real size for every scenario anybody writes, and a file that
    // lists many segments is held to [`CHAIN_RESERVE_CAP`]. The cap is a
    // reservation, not a limit on the run: a longer chain grows the ordinary
    // way. Reviewed finding, wave 5. Computed in `u64` and capped there, so the
    // conversion is from a number below the cap and the sweep of S2's E7
    // leaves no fallback in it.
    let reserve = u64::from(scenario.ticks())
        .saturating_mul(24)
        .min(CHAIN_RESERVE_CAP);
    let mut played = Played {
        chain: String::with_capacity(
            usize::try_from(reserve)
                .map_err(|_| Failure::internal("a chain reservation this platform cannot hold"))?,
        ),
        events: Vec::new(),
        segments: Vec::with_capacity(scenario.segments.len()),
        notes: Vec::new(),
        rules_hash,
    };
    let mut seen = 0_usize;
    let mut index = 0_usize;

    loop {
        // Whatever the last phase left on the feed, dated at the tick the match
        // is standing on. A Lull consumes no sim tick, so that is the tick
        // every line of it belongs to.
        let standing = surface_tick(&surface)?;
        drain(&surface, &mut seen, standing, &mut played.events)?;

        // The `builtin` seats plan first, then the playbook seats submit: the
        // order `gamectl host` has, whose surface thread plans its in-process
        // seats as a Lull opens and before any client's call is served. Neither
        // can see the other's work -- each seat's submission is its own, and
        // nothing is sealed until `begin_push` -- so the order is a convention
        // kept for parity rather than a dependency. No `builtin` seat, no call.
        if let Some(seats) = builtin.as_mut() {
            seats.plan(&mut surface);
        }

        let nightly = matches!(until, Until::MatchEnd(_));
        seal_all(
            root,
            &mut surface,
            scenario,
            &tokens,
            nightly,
            &mut played.notes,
        )?;

        // `begin_push` opens a fresh segment feed, so the count starts again.
        let anchor = surface_tick(&surface)?;
        let started = surface.begin_push().map_err(internal)?;
        if !started {
            return Err(Failure::internal(format!(
                "the match would not open segment {index}: it is in its {}",
                phase_name(&surface)?
            )));
        }
        seen = 0;
        drain(&surface, &mut seen, anchor, &mut played.events)?;
        let ticks = play_push(&mut surface, &mut seen, &mut played)?;
        if let Until::Segments = until {
            let wanted = scenario
                .segments
                .get(index)
                .map(|segment| ticks_of(segment.length_ms))
                .ok_or_else(|| Failure::internal("a segment played past the file's list"))?;
            if ticks != wanted {
                return Err(Failure::new(
                    Exit::Failed,
                    strings::scenario_segment_short(index, ticks, wanted),
                ));
            }
        }

        surface.end_recap().map_err(internal)?;
        let more = match &mut until {
            Until::Segments => index.saturating_add(1) < scenario.segments.len(),
            Until::MatchEnd(ending) => {
                if phase(&surface)? == MatchPhase::Ended {
                    **ending = Some(ending_of(&surface)?);
                    false
                } else if round(&surface)? > scenario.round_limit {
                    // The sim ends a match at its round limit, so this is the
                    // sim and this runner disagreeing, not an arm's doing.
                    return Err(Failure::internal(format!(
                        "the match played past its round limit of {} without ending",
                        scenario.round_limit
                    )));
                } else {
                    true
                }
            }
        };
        if !more {
            break;
        }
        surface.set_phase_remaining_ms(whole_lull(&surface)?);
        surface.open_lull().map_err(internal)?;
        index = index.saturating_add(1);
    }
    Ok(played)
}

/// Seal every `playbook` seat's file for the Lull the match stands in.
///
/// A seat the verifier refuses goes through the harness door with a note, as
/// [`seal`] says, except in a run to the match's end (`nightly`), where
/// [`seal`] fails the run on a refusal instead: a broken arm.
fn seal_all(
    root: &Path,
    surface: &mut Surface,
    scenario: &Scenario,
    tokens: &[Token],
    nightly: bool,
    notes: &mut Vec<String>,
) -> Result<(), Failure> {
    for (seat, token) in scenario.seats.iter().zip(tokens) {
        let SeatKind::Playbook(relative) = &seat.kind else {
            continue;
        };
        let Door::Harness(codes) = seal(root, surface, token, seat.seat, relative, nightly)? else {
            continue;
        };
        let note = strings::scenario_harness_seal(seat.seat, relative, &codes);
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    Ok(())
}

/// Step the open Push to its segment's end, writing the chain and the
/// events, and say how many ticks it played.
fn play_push(surface: &mut Surface, seen: &mut usize, played: &mut Played) -> Result<u32, Failure> {
    let mut ticks = 0_u32;
    while let Some(report) = surface.step().map_err(internal)? {
        let tick = report.tick.raw();
        let _ = writeln!(played.chain, "{tick}\t{}", pharmakos_sim::hex(report.hash));
        drain(surface, seen, tick, &mut played.events)?;
        ticks = ticks
            .checked_add(1)
            .ok_or_else(|| Failure::internal("a segment of more ticks than a u32 counts"))?;
        if report.segment_ended {
            break;
        }
    }
    played.segments.push(ticks);
    Ok(ticks)
}

/// How the match ended, from the sim's own outcome: the winner it recorded,
/// or on a shared win (no single winner) the final audit's tied seats, which
/// the sim reads again from the world rather than storing
/// ([`pharmakos_sim::audit`]).
fn ending_of(surface: &Surface) -> Result<Ending, Failure> {
    let host = surface.host().map_err(internal)?;
    let outcome = host
        .runner()
        .outcome()
        .ok_or_else(|| Failure::internal("the match ended with no outcome"))?;
    let winners = match outcome.winner {
        Some(seat) => vec![seat.raw()],
        None => pharmakos_sim::audit::final_audit(host.world())
            .winners
            .iter()
            .map(|seat| seat.raw())
            .collect(),
    };
    Ok(Ending {
        winners,
        reason: outcome.reason.name(),
        end_tick: outcome.at.raw(),
        rounds: host.runner().round(),
    })
}

fn phase(surface: &Surface) -> Result<MatchPhase, Failure> {
    Ok(surface.host().map_err(internal)?.runner().phase())
}

fn round(surface: &Surface) -> Result<u32, Failure> {
    Ok(surface.host().map_err(internal)?.runner().round())
}

// ---------------------------------------------------------------------------
// Opening the match
// ---------------------------------------------------------------------------

fn load_rules(root: &Path, scenario: &Scenario) -> Result<RulesTable, Failure> {
    let path = root.join(&scenario.rules);
    RulesTable::load(&path).map_err(|error| {
        Failure::input(strings::unreadable(
            "rules table",
            &crate::display(&path),
            &error.to_string(),
        ))
    })
}

/// The seats the scenario gives the built-in operator, ascending.
fn builtin_seats(scenario: &Scenario) -> Vec<SeatId> {
    scenario
        .seats
        .iter()
        .filter(|seat| seat.kind == SeatKind::Builtin)
        .map(|seat| SeatId::new(seat.seat))
        .collect()
}

/// Easy's factory for the `builtin` seats, which [`play`] seats through the
/// gateway's in-process seats with no human seat, so nothing is advised;
/// `None` when the scenario has no `builtin` seat.
///
/// # Errors
///
/// [`Exit::Input`] when the rules text is unreadable or lacks a row Easy
/// scores with.
fn easy_for_builtin(root: &Path, scenario: &Scenario) -> Result<Option<EasyOperators>, Failure> {
    if builtin_seats(scenario).is_empty() {
        return Ok(None);
    }
    // The rules table's own text, the one `load_rules` read: the operator
    // reads its rows from the public rules text (decision C17), as it does
    // under `gamectl host`.
    let path = root.join(&scenario.rules);
    let text = std::fs::read_to_string(&path).map_err(|error| {
        Failure::input(strings::unreadable(
            "rules table",
            &crate::display(&path),
            &error.to_string(),
        ))
    })?;
    // The first `builtin` seat's index, for the pointer: there is one,
    // because the caller found the list non-empty above.
    let index = scenario
        .seats
        .iter()
        .position(|seat| seat.kind == SeatKind::Builtin)
        .ok_or_else(|| Failure::internal("a scenario with a builtin seat has none"))?;
    let factory = EasyOperators::new(&text).map_err(|error| {
        Failure::input(strings::scenario_builtin_unseated(
            &format!("/seats/{index}/kind"),
            &scenario.rules,
            &error.to_string(),
        ))
    })?;
    Ok(Some(factory))
}

/// A hosted match in its opening Lull, at the gateway's own rate limits.
fn open(root: &Path, scenario: &Scenario, rules: RulesTable) -> Result<Surface, Failure> {
    let seats: Vec<SeatId> = scenario
        .seats
        .iter()
        .map(|seat| SeatId::new(seat.seat))
        .collect();
    // The template library only for a scenario with a `builtin` seat, whose
    // operator instantiates its templates there. A scenario that names its
    // playbooks as files never instantiates one, and a host handed a folder
    // it does not read is a path in a message that means nothing -- so such
    // a scenario opens exactly as it did before T18b.
    let library =
        (!builtin_seats(scenario).is_empty()).then(|| root.join(crate::host::LIBRARY_PATH));
    let count = u32::try_from(scenario.seats.len())
        .map_err(|_| Failure::internal("a match of more seats than a u32 can count"))?;
    let host = Host::open(
        &WorldConfig {
            match_seed: scenario.seed,
            seats: count,
            rules: rules.clone(),
            match_settings: MatchSettings {
                segment_lengths_ms: scenario
                    .segments
                    .iter()
                    .map(|segment| segment.length_ms)
                    .collect(),
                round_limit: scenario.round_limit,
            },
        },
        library,
    )
    .map_err(|error| {
        Failure::input(format!(
            "this scenario's match would not open: {}",
            error.message
        ))
    })?;

    let mut surface = Surface::new(
        MATCH_ID,
        scenario.seed,
        rules,
        // Fogged, which is the default a real match runs under (spec §12). It
        // filters what a *client* is shown and nothing the sim does, so it
        // reaches no hash; the runner reads the unfiltered feed, because a
        // scenario is the harness's view of the match and not a seat's.
        FogPolicy::fogged(),
        &seats,
    )
    .map_err(internal)?;
    surface.attach(host).map_err(internal)?;
    surface.set_phase_remaining_ms(whole_lull(&surface)?);
    surface.open_lull().map_err(internal)?;
    // The limits are left at the gateway's own defaults, deliberately. An
    // earlier draft raised them "because a scenario submits every seat's
    // playbook in one burst"; that reason was wrong and the review found it.
    // `Surface::admit` counts per *token* (`crates/gateway/src/surface.rs`),
    // `mint` gives every seat its own, and this module makes exactly one
    // `Surface::call` per seat per segment — one of the default eight. Copying
    // the window numbers here would also have frozen a copy of two gateway
    // constants that carry PLACEHOLDERs of their own, so that when the owner
    // moves them at hardening this would silently not follow.
    Ok(surface)
}

/// One token per seat, with the scopes a planning client holds.
fn mint(surface: &mut Surface, scenario: &Scenario) -> Result<Vec<Token>, Failure> {
    let scopes = ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit]);
    let mut tokens = Vec::with_capacity(scenario.seats.len());
    for seat in &scenario.seats {
        let (token, _) = surface
            .tokens()
            .mint(
                Subject::Seat(SeatId::new(seat.seat)),
                MATCH_ID,
                scopes,
                Tick::ZERO,
            )
            .map_err(internal)?;
        tokens.push(token);
    }
    Ok(tokens)
}

/// How a seat's playbook reached the match.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Door {
    /// Through `submit_plan`, which is the door every player's playbook goes
    /// through. What a scenario should use, and what almost every scenario
    /// will.
    Submitted,
    /// Through the harness's own door, because the seat's verifier refused it
    /// **against that seat's own frozen snapshot**. The string is the codes.
    Harness(String),
}

/// Seal one seat's playbook, and say which door it went through.
///
/// # The first door is `submit_plan`, and it is tried every time
///
/// A playbook a player writes reaches a match one way: verified against that
/// seat's frozen snapshot, then compiled, then sealed at the end of the Lull.
/// A scenario that seals any other way would be pinning a hash chain for a
/// match no player could have caused, so this tries the real door first and
/// says in the run's report when it worked.
///
/// # The second door exists because a committed scenario needs it, and it is
/// never quiet
///
/// `expand-east-segment.scenario.jsonc` seals the spec's worked `expand_east`
/// with its first site moved near seat 0's core (S2's `base`;
/// `scenarios/skeleton/expand-east-segment.playbook.jsonc`), and **that
/// playbook does not qualify there**: it names `b_01`, each seat's own first
/// placed beacon, which neither seat has when it seals (`E0401`), and for seat
/// 1 its site is far outside every sphere (`E0403`). That is decisions-log item 103 (3)'s finding
/// one step further on than item 103 (3) took it: the scenario does not merely
/// record a playbook that gets nowhere, it records one **no seat could ever
/// have submitted**. Its chain was produced by `crates/sim/tests/scenario.rs`,
/// which compiles a plan directly and never meets a verifier.
///
/// So a scenario is allowed to give a match orders no seat could give — a
/// harness is for pinning what the sim does, including with orders the door
/// would refuse — and every run that uses this door prints a line naming the
/// seat, the file and the codes (`Played::notes`, and the report `check`
/// renders). It compiles by exactly the route the gateway's own
/// `compile_playbook` does, `plan-core`'s canonical form into `Plan::compile`,
/// so the plan the match plays is the plan a submission would have produced.
///
/// **This is the one thing in this task the owner is asked to rule on**, and
/// the pull request puts it with its alternatives: refuse instead and the
/// `scenario` step is red on a committed file this task does not own; change
/// that scenario and T11's honest record and T14's re-bless target both move.
///
/// # What the door is *not*, since a review asked
///
/// It is not a fall-through. Only an explicit `"accepted": false` opens it: an
/// answer with no `accepted` flag at all is this build's gateway and this
/// build's runner disagreeing about the shape of a result, and that is
/// [`Exit::Internal`] rather than a quiet move onto the second door.
///
/// It is still *unscoped*, and the review is right that it should not be: a
/// scenario broken later by a rules or a map change keeps playing and announces
/// it only in a `note`. The fix is an opt-in key in the file — `"seal":
/// "harness"` beside `"kind": "playbook"` — with an unrequested refusal an
/// [`Exit::Failed`] naming the codes. That key has to be known to
/// `xtask/src/scenario.rs`'s reader as well, which rejects an unknown key
/// rather than ignoring it, and `xtask` is a contract path (AGENTS.md §5). So
/// it rides with the owner's ruling on the door itself rather than being taken
/// here.
fn seal(
    root: &Path,
    surface: &mut Surface,
    token: &Token,
    seat: u8,
    relative: &str,
    nightly: bool,
) -> Result<Door, Failure> {
    let playbook = std::fs::read_to_string(root.join(relative)).map_err(|error| {
        Failure::input(strings::unreadable(
            "playbook",
            relative,
            &error.to_string(),
        ))
    })?;
    let params = pharmakos_proto::json::write(&Json::Object(vec![(
        String::from("playbook_jsonc"),
        Json::String(playbook.clone()),
    )]));
    let request = pharmakos_gateway::rpc::parse(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"submit_plan","params":{params}}}"#
    ))
    .map_err(|error| {
        Failure::internal(format!(
            "this runner wrote a request the gateway will not parse: {} ({})",
            error.title(),
            error.number()
        ))
    })?;
    let answer = surface.call(Some(token), &request, &Blind);

    if let Some(error) = answer.get("error") {
        // In a run to the match's end, a refusal of any kind is a broken arm
        // on this seed and round, as the module docs promise.
        if nightly {
            return Err(Failure::new(
                Exit::Failed,
                strings::nightly_arm_refused(seat, relative, round(surface)?, &message_of(error)),
            ));
        }
        return Err(Failure::input(strings::scenario_playbook_refused(
            seat,
            relative,
            &message_of(error),
        )));
    }
    let result = answer.get("result");
    match result.and_then(|answer| answer.get("accepted")) {
        Some(Json::Bool(true)) => return Ok(Door::Submitted),
        // An explicit refusal, which is the only thing the second door is for.
        Some(Json::Bool(false)) => {}
        // Anything else is the *shape* of the answer having moved, not a
        // playbook having been refused — and a runner that treated a missing
        // `accepted` as a refusal would quietly move every scenario onto the
        // harness door the day `submit_plan`'s result changed. Reviewed
        // finding, wave 5: the fall-through was unscoped.
        other => {
            return Err(Failure::internal(format!(
                "`submit_plan` answered with no `accepted` flag ({other:?}). That is this \
                 build's gateway and this build's runner disagreeing about the shape of an \
                 answer, not a playbook a scenario got wrong, and the harness door is not \
                 the answer to it."
            )));
        }
    }

    let codes = codes_of(result)?;
    // A nightly run never takes the harness door: the refusal fails the run,
    // naming the round, before anything is sealed.
    if nightly {
        return Err(Failure::new(
            Exit::Failed,
            strings::nightly_arm_refused(seat, relative, round(surface)?, &codes),
        ));
    }
    let report_hash = report_hash_of(result)?;

    // The second door. Compiled by the route `compile_playbook` takes, so the
    // plan is the one a submission would have produced.
    let rules = surface.host().map_err(internal)?.rules().clone();
    let round = surface.host().map_err(internal)?.runner().round();
    let canonical = pharmakos_plan_core::canonicalise_text(&playbook).map_err(|error| {
        Failure::input(strings::scenario_playbook_refused(
            seat,
            relative,
            &format!("it has no canonical form: {}", error.message),
        ))
    })?;
    let plan = pharmakos_sim::Plan::compile(&canonical.playbook, &rules).map_err(|error| {
        Failure::input(strings::scenario_playbook_refused(
            seat,
            relative,
            &format!("this build cannot execute it: {error}"),
        ))
    })?;
    let state = surface
        .seat_state_mut(Subject::Seat(SeatId::new(seat)), SeatId::new(seat))
        .map_err(internal)?;
    state.sealed = Some(pharmakos_gateway::surface::Sealed {
        playbook_jsonc: playbook,
        report_hash,
        round,
        // False, and deliberately: `true` means the gateway filed the safe
        // playbook for a seat that sealed nothing (spec §14), and saying so
        // here would be this runner claiming to be the gateway.
        filed_by_the_gateway: false,
        plan,
    });
    Ok(Door::Harness(codes))
}

/// The refused report's codes and pointers, `CODE at /pointer`, joined.
fn codes_of(result: Option<&Json>) -> Result<String, Failure> {
    // The codes, not the count. "2 diagnostics" sends the reader back to the
    // tool; the codes and their pointers say what is wrong on their own, and
    // this line is read by somebody who has just watched a build go red.
    let mut codes: Vec<String> = Vec::new();
    if let Some(Json::Array(items)) = result
        .and_then(|result| result.get("report"))
        .and_then(|report| report.get("diagnostics"))
    {
        for item in items {
            // A diagnostic always carries its code, so one without is the
            // wire's shape having moved, refused rather than shown as `?`.
            let Some(Json::String(code)) = item.get("code") else {
                return Err(Failure::internal(format!(
                    "`submit_plan` answered with a diagnostic that has no code ({item:?})"
                )));
            };
            // An absent `path` is proto3 JSON's omitted empty string: the
            // document's root pointer, which is spelled as nothing. Present
            // and not a string is the wire's shape having moved.
            let path = match item.get("path") {
                Some(Json::String(path)) => path.as_str(),
                None => "",
                Some(other) => {
                    return Err(Failure::internal(format!(
                        "`submit_plan` answered with a diagnostic path that is not a string \
                         ({other:?})"
                    )));
                }
            };
            codes.push(format!("{code} at {path}"));
        }
    }
    Ok(codes.join("; "))
}

/// The report's `report_hash`, decoded.
fn report_hash_of(result: Option<&Json>) -> Result<Vec<u8>, Failure> {
    // Decoded, not stored as its own text. `Sealed::report_hash` is documented
    // as "eight big-endian bytes" and the wire spells it base64
    // (`surface/planning.rs`), so `text.into_bytes()` would have put twelve
    // ASCII characters where the eight bytes go — latent today because nothing
    // outside tests reads the field, and T17's restore path is named as the
    // first reader. Reviewed finding, wave 5.
    match result
        .and_then(|result| result.get("report"))
        .and_then(|report| report.get("report_hash"))
    {
        // A `report_hash` that is not base64 is this build's gateway and this
        // runner disagreeing about the wire, not a playbook a scenario got
        // wrong, so it is refused rather than sealed as no bytes (S2's sweep,
        // E7; the site vfy1 found).
        Some(Json::String(text)) => pharmakos_proto::json::base64::decode(text).ok_or_else(|| {
            Failure::internal(format!(
                "`submit_plan` answered with a `report_hash` that is not base64 ({text:?})"
            ))
        }),
        // Every report carries its hash (spec section 11), so a refused
        // playbook's answer without one is the wire's shape having moved, and
        // it is refused rather than sealed as empty bytes (S2's sweep, E7).
        other => Err(Failure::internal(format!(
            "`submit_plan` answered with no `report_hash` string ({other:?})"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Reading what happened
// ---------------------------------------------------------------------------

/// Copy the feed's new lines into the record, dated at `tick`.
///
/// The **feed**, not the sim's bus: `Surface::step` drains the bus onto the
/// feed as part of stepping, which is the point of hosting through the surface
/// rather than beside it. `SegmentFeed::events` is the unfiltered list, "for
/// the host and for tests -- never for a client"; the fog filter is what a
/// client's `get_segment_feed` goes through, and a scenario is not a client.
fn drain(
    surface: &Surface,
    seen: &mut usize,
    tick: u32,
    into: &mut Vec<Fired>,
) -> Result<(), Failure> {
    let events = surface.feed().events();
    // The feed only grows within a segment, and `seen` restarts at zero when
    // `begin_push` opens a fresh one, so a feed shorter than what was read is
    // the gateway and this runner disagreeing, not a quiet empty read.
    let fresh = events.get(*seen..).ok_or_else(|| {
        Failure::internal(format!(
            "the segment feed holds {} events after {} were read from it",
            events.len(),
            *seen
        ))
    })?;
    for event in fresh {
        into.push(Fired {
            tick,
            kind: event.kind.name().to_owned(),
            seat: match event.audience {
                Audience::Private(seat) => Some(seat.raw()),
                Audience::World { owner, .. } => owner.map(SeatId::raw),
                Audience::Public => None,
            },
        });
    }
    *seen = events.len();
    Ok(())
}

fn surface_tick(surface: &Surface) -> Result<u32, Failure> {
    Ok(surface.host().map_err(internal)?.runner().tick().raw())
}

fn phase_name(surface: &Surface) -> Result<&'static str, Failure> {
    Ok(surface.host().map_err(internal)?.runner().phase().name())
}

/// The whole of the Lull the match is about to open, which a scenario
/// spends no host time in and so hands the gateway entire (AGENTS.md section
/// 4.5): round 1's `rules.match.first_lull_ms` and every later round's
/// `rules.match.lull_ms`, through the gateway's own typed read
/// ([`Surface::lull_length`]), so the runner and the gateway cannot disagree
/// about which row a round's Lull is (S1's plan, task `econ`). Read from the
/// table, never written here (AGENTS.md section 12).
fn whole_lull(surface: &Surface) -> Result<Ms, Failure> {
    let round = surface.host().map_err(internal)?.runner().round();
    Ok(surface.lull_length(round))
}

fn internal(mut error: pharmakos_gateway::Error) -> Failure {
    // The gateway's own sentence, with this runner's frame around it: a host
    // that drove its own surface wrongly is not something a scenario author
    // did, and the message has to say whose mistake it is.
    error
        .message
        .insert_str(0, "the gateway refused a host's own call: ");
    Failure::new(Exit::Internal, error.message)
}

fn message_of(error: &Json) -> String {
    match error.get("message") {
        Some(Json::String(text)) => text.clone(),
        _ => format!("{error:?}"),
    }
}

// ---------------------------------------------------------------------------
// The assertions
// ---------------------------------------------------------------------------

/// Check every assertion and render the report. Returns how many did not hold.
///
/// # Errors
///
/// [`Exit::Internal`] when a chain that was played cannot be written down.
pub fn check(
    root: &Path,
    scenario: &Scenario,
    played: &Played,
) -> Result<(String, usize), Failure> {
    let mut text = strings::scenario_heading(
        &scenario.name,
        scenario.seats.len(),
        played.ticks(),
        &crate::verify::hex(&played.rules_hash.to_be_bytes()),
    );
    text.push('\n');
    // Above the assertions, deliberately: a chain produced behind the
    // verifier's door is a different claim from one produced in front of it,
    // and the reader has to meet that before the ticks.
    for note in &played.notes {
        let _ = writeln!(text, "{note}");
    }
    let mut failed = 0_usize;

    for (index, assertion) in scenario.assertions.iter().enumerate() {
        let (what, outcome) = match assertion {
            Assertion::EventFired {
                event,
                seat,
                by_tick,
            } => (
                strings::scenario_event_fired(event, *seat, *by_tick),
                event_fired(played, event, *seat, *by_tick),
            ),
            Assertion::HashChainEquals { golden } => (
                strings::scenario_hash_chain(golden),
                hash_chain_equals(root, golden, played)?,
            ),
        };
        match outcome {
            Ok(()) => {
                let _ = writeln!(text, "{}", strings::scenario_assertion_ok(index, &what));
            }
            Err(why) => {
                failed = failed.saturating_add(1);
                let _ = writeln!(
                    text,
                    "{}",
                    strings::scenario_assertion_failed(index, &what, &why)
                );
            }
        }
    }

    let _ = writeln!(
        text,
        "{}",
        if failed == 0 {
            strings::scenario_passed(&scenario.name, scenario.assertions.len())
        } else {
            strings::scenario_failed(&scenario.name, failed, scenario.assertions.len())
        }
    );
    Ok((text, failed))
}

fn event_fired(played: &Played, event: &str, seat: Option<u8>, by_tick: u32) -> Result<(), String> {
    let first = played
        .events
        .iter()
        .filter(|fired| fired.kind == event && (seat.is_none() || fired.seat == seat))
        .map(|fired| fired.tick)
        .min();
    match first {
        Some(tick) if tick <= by_tick => Ok(()),
        Some(tick) => Err(strings::scenario_event_late(event, seat, tick, by_tick)),
        None => {
            // Naming what *did* happen is the difference between a red build
            // somebody can act on and one they have to reproduce first.
            let mut kinds: Vec<String> = Vec::new();
            for fired in &played.events {
                if !kinds.contains(&fired.kind) {
                    kinds.push(fired.kind.clone());
                }
            }
            Err(strings::scenario_event_missing(event, seat, &kinds))
        }
    }
}

fn hash_chain_equals(
    root: &Path,
    golden: &str,
    played: &Played,
) -> Result<Result<(), String>, Failure> {
    let actual_path = crate::actual_for(golden);
    let written = match &actual_path {
        Some(path) => {
            write_actual(path, &played.chain)?;
            crate::display(path)
        }
        None => String::from("<no target directory to write it to>"),
    };

    let Ok(committed) = std::fs::read_to_string(root.join(golden)) else {
        return Ok(Err(strings::scenario_chain_missing(golden, &written)));
    };
    // The committed file is byte-compared, and `.gitattributes` marks
    // tests/golden/** as `-text` so git will not normalise it. A CRLF copy is
    // a golden that is identical on screen and unequal here, so it is
    // normalised for the comparison and the `golden` step is what refuses it.
    let committed = committed.replace("\r\n", "\n");
    if committed == played.chain {
        return Ok(Ok(()));
    }

    let expected: Vec<&str> = committed.lines().collect();
    let actual: Vec<&str> = played.chain.lines().collect();
    let at = expected
        .iter()
        .zip(&actual)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    Ok(Err(strings::scenario_chain_moved(
        golden,
        at.saturating_add(1),
        // A chain shorter than the other has no line at `at`, and
        // the message says which side ended there.
        expected.get(at).copied().unwrap_or("<end of file>"),
        // As above, for the run's side.
        actual.get(at).copied().unwrap_or("<end of run>"),
        expected.len(),
        actual.len(),
    )))
}

fn write_actual(path: &Path, chain: &str) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            Failure::internal(format!("creating {}: {error}", parent.display()))
        })?;
    }
    // `write` and not a formatting writer: the chain is already the bytes, LF
    // endings and all, and a platform-flavoured writer is how a golden
    // produced on Windows grows carriage returns (tests/golden/README.md rule
    // 3 -- "--bless is not the fix for this one: fix the producer").
    std::fs::write(path, chain.as_bytes())
        .map_err(|error| Failure::internal(format!("writing {}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::{open, whole_lull};
    use std::path::Path;

    /// The workspace root: this crate is `<root>/crates/gamectl`.
    fn root() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    #[test]
    fn a_scenario_opens_round_one_with_the_first_lulls_whole_length() {
        let source = root().join("scenarios/skeleton/expand-east-segment.scenario.jsonc");
        let scenario = crate::scenario::load(root(), &source).expect("a committed scenario");
        let rules = super::load_rules(root(), &scenario).expect("its rules table");
        let first = rules
            .message()
            .r#match
            .as_ref()
            .map(|block| block.first_lull_ms)
            .expect("a match block");
        let surface = open(root(), &scenario, rules).expect("the match opens");
        assert_eq!(surface.time().round, 1);
        assert_eq!(
            surface.time().phase_remaining_ms.raw(),
            first,
            "round 1's Lull is handed over as `first_lull_ms`"
        );
        assert_eq!(whole_lull(&surface).expect("a Lull").raw(), first);
        assert!(
            !surface.time().untimed,
            "a Lull handed its whole length is timed"
        );
    }
}
