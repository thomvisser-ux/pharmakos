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
//! how the match ended (`outcome()`), on a shared win the final audit's
//! tied seats (`audit::final_audit(host.world())`, a pure read), and after
//! every tick and at every Lull the Attack table's rows
//! (`host.world().attack().rows()`), for the hunts a nightly result counts
//! ([`Hunts`]), because the feed's `attack_launched` names no beacon. `Host::runner`
//! hands out a `&Runner` and `Host::world` a `&World`, and every stepping
//! method needs `&mut`, so the reach cannot become a second way to play a
//! match; if `Surface` gains accessors for these, this file should take them
//! and `.runner()` should join the needles.
//!
//! # The match settings a file may name
//!
//! Two inputs of a match beyond the triple are the scenario's own keys, each
//! optional: `round_limit` since S1's first contract pull request (the
//! register's S1-41) and `segment_lengths_ms` since decisions-log item 232.
//! `round_limit`'s sibling `units_per_seat`, the harness walkers, went with
//! them (S2-16, S2's plan decision 23).
//!
//! * `round_limit` is the sim's
//!   [`DEFAULT_ROUND_LIMIT`](pharmakos_sim::runner::DEFAULT_ROUND_LIMIT), the
//!   spec's default and a lobby setting rather than a tuning row
//!   (decisions-log item 102 (7)), when the file names none; a scenario that
//!   plays a match to its end names it, and both readers refuse a limit below
//!   the file's segment count.
//! * `segment_lengths_ms` is the match's segment length list, the lobby's
//!   setting, which the match state hashes. When the file names none it is
//!   the lengths of the file's own segments; a scenario that replays a match
//!   played elsewhere states that match's list whole
//!   (`Scenario::segment_lengths_ms`).
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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use pharmakos_gateway::fog::{Audience, Blind, FogPolicy};
use pharmakos_gateway::host::Host;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::serve;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::json::Json;
use pharmakos_sim::World;
use pharmakos_sim::attack::{Aim, AttackRow};
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{MatchPhase, MatchSettings};
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::WorldConfig;

use crate::cli::RunOptions;
use crate::exit::{Exit, Failure};
use crate::host::EasyOperators;
use crate::scenario::{Assertion, Scenario, SeatKind, sealed_file, ticks_of};
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
    /// The round the match was in when the event was read off the feed,
    /// counted from 1: the runner's own count of the segments it opened, held
    /// equal to the sim's round at every Lull (`play_until` refuses a run
    /// where the two disagree). A Push's events are its round's; what a recap
    /// and the next Lull's opening leave on the feed is read in the Lull it
    /// opened, so it carries that round.
    pub round: u32,
}

/// The hunts a run's seats filed, and the ones that launched (S3's plan, E14:
/// "the nightly result does not count hunts that never launched",
/// decisions-log item 150 (2)).
///
/// A **hunt** is an Attack beacon whose target is the enemy commander
/// ([`Aim::Commander`]); it is **filed** once its row has been seen in the
/// Attack table with that aim, and **launched** once the row has been seen
/// with a wave counted (`waves_used > 0`). That one condition is sufficient
/// because of when the table is read: after every tick and at every Lull. The
/// sim counts a wave on the tick the force launches (`attack.rs`, the tail of
/// `gather`) and clears the count only on a new segment's first tick
/// (`advance`), so the read after the launch tick always sees it, even when
/// the attack ends on that same tick: `end()` resets the phase and the launch
/// stamp, never the count. The phase and the stamp would each miss that case,
/// so neither is read (fix pass 1 of #219, item 6).
///
/// Read from the world rather than the feed because the feed's
/// `attack_launched` names no beacon (`crates/gateway/src/feed.rs`: a kind, a
/// time, an audience, a text and a memo), so the feed cannot say *which*
/// Attack beacon launched. The read is the Attack table's rows, through
/// `Host::world()`, whose every method that writes needs `&mut`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Hunts {
    /// Every hunt's beacon seen filed, by its id's raw value (row and
    /// generation, so a rebuilt beacon is a new hunt), and the seat that owns
    /// it, read from the beacon table when the hunt is first seen.
    pub filed: BTreeMap<u32, u8>,
    /// Every hunt's beacon seen launched.
    pub launched: BTreeSet<u32>,
}

impl Hunts {
    /// How many of `seat`'s filed hunts never launched.
    #[must_use]
    pub fn unlaunched_of(&self, seat: u8) -> usize {
        self.filed
            .iter()
            .filter(|(beacon, owner)| **owner == seat && !self.launched.contains(*beacon))
            .count()
    }

    /// Read the Attack table once.
    ///
    /// # Errors
    ///
    /// [`Exit::Internal`] for an Attack row whose beacon the beacon table
    /// does not hold: the sim's two tables disagreeing, never a hunt to count
    /// as nobody's.
    fn read(&mut self, world: &World) -> Result<(), Failure> {
        for row in world.attack().rows() {
            if !matches!(row.settings.aim, Aim::Commander(_)) {
                continue;
            }
            let beacon = row.beacon.raw();
            if let std::collections::btree_map::Entry::Vacant(slot) = self.filed.entry(beacon) {
                let owner = world
                    .beacons()
                    .row_of(row.beacon)
                    .ok()
                    .and_then(|at| world.beacons().seats().get(at.index()).copied())
                    .ok_or_else(|| {
                        Failure::internal(format!(
                            "an Attack row names beacon {beacon}, which the beacon table does \
                             not hold"
                        ))
                    })?;
                slot.insert(owner);
            }
            if launched(row) {
                self.launched.insert(beacon);
            }
        }
        Ok(())
    }
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
    /// The hunts the seats filed and launched over the whole run.
    pub hunts: Hunts,
    /// The plan fingerprint of what each `playbook` or `sealed` seat sealed
    /// in the last round sealed, by seat, as the gateway's seal carries it
    /// (`Sealed::plan_fingerprint`): what an arena result names a playbook
    /// contestant's text by.
    pub fingerprints: BTreeMap<u8, u64>,
}

impl Played {
    /// How many ticks were played in total.
    ///
    /// # Errors
    ///
    /// [`Exit::Internal`] for a total past a `u32`: `play_push` counts each
    /// segment in a `u32`, and the sum of several is refused rather than
    /// saturated.
    pub fn ticks(&self) -> Result<u32, Failure> {
        self.segments.iter().try_fold(0_u32, |total, ticks| {
            total
                .checked_add(*ticks)
                .ok_or_else(|| Failure::internal("a run of more ticks than a u32 counts"))
        })
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
            return Err(Failure::input(strings::operator_unused(
                "scenario run",
                &scenario.source,
            )));
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
    /// The final audit's line for every seat it decided between, ascending by
    /// seat: what each held and what it destroyed, so a result says by how
    /// much a match was won and on which term, not only by whom.
    pub audit: Vec<Audited>,
}

/// One seat's line of the final audit ([`pharmakos_sim::audit::AuditLine`]),
/// as whole `$`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Audited {
    /// Whose line it is.
    pub seat: u8,
    /// Value held: `$` plus the seat's own assets at build cost by current
    /// hit points.
    pub held: i64,
    /// The build cost of the enemy assets the seat destroyed.
    pub destroyed: i64,
    /// How many of the seat's beacons died this match.
    pub beacons_lost: u32,
}

/// Play one scenario **to the match's end** and say how it ended, asserting
/// nothing: what `--seeds` does once per seed.
///
/// The match's segment length list is the file's, as it is for [`play`]
/// (`segment_lengths_ms` where the file names it, the lengths of its segments
/// where it does not), and rounds past the list's end take its last length, as
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
    play_to_end_for(root, scenario, ToEnd::Nightly)
}

/// Whose run a match played to its end belongs to, which is whose words a
/// refused playbook is said in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToEnd {
    /// A run over a seed file: the playbook is the nightly's arm.
    Nightly,
    /// The arena: the playbook is a contestant's.
    Arena,
}

/// [`play_to_end`], for the run `whose` names.
///
/// # Errors
///
/// As [`play_to_end`].
pub fn play_to_end_for(
    root: &Path,
    scenario: &Scenario,
    whose: ToEnd,
) -> Result<(Played, Ending), Failure> {
    let mut ending = None;
    let played = play_until(root, scenario, Until::MatchEnd(&mut ending, whose))?;
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
    MatchEnd(&'a mut Option<Ending>, ToEnd),
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
    // `saturating_mul` and `min` are exact here: a product past `u64::MAX`
    // is past the cap, which is what the reservation is held to either way.
    let reserve = u64::from(scenario.ticks()?)
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
        hunts: Hunts::default(),
        fingerprints: BTreeMap::new(),
    };
    let mut seen = 0_usize;
    let mut index = 0_usize;

    loop {
        // The round this Lull opens: the runner's own count, held to the sim's.
        let opened = round_of(&surface, index)?;
        // Whatever the last phase left on the feed, dated at the tick the match
        // is standing on. A Lull consumes no sim tick, so that is the tick
        // every line of it belongs to.
        let standing = surface_tick(&surface)?;
        drain(&surface, &mut seen, standing, opened, &mut played)?;

        // The `builtin` seats plan first, then the playbook seats submit: the
        // order `gamectl host` has, whose surface thread plans its in-process
        // seats as a Lull opens and before any client's call is served. Neither
        // can see the other's work -- each seat's submission is its own, and
        // nothing is sealed until `begin_push` -- so the order is a convention
        // kept for parity rather than a dependency. No `builtin` seat, no call.
        if let Some(seats) = builtin.as_mut() {
            seats.plan(&mut surface);
        }

        let to_end = match &until {
            Until::Segments => None,
            Until::MatchEnd(_, whose) => Some(*whose),
        };
        seal_all(
            root,
            &mut surface,
            scenario,
            &tokens,
            to_end,
            &mut played.notes,
        )?;
        read_fingerprints(&surface, scenario, &mut played.fingerprints)?;

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
        drain(&surface, &mut seen, anchor, opened, &mut played)?;
        let ticks = play_push(&mut surface, &mut seen, opened, &mut played)?;
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
            Until::Segments => next_index(index)? < scenario.segments.len(),
            Until::MatchEnd(ending, _) => {
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
        surface
            .set_phase_remaining_ms(whole_lull(&surface)?)
            .map_err(internal)?;
        surface.open_lull().map_err(internal)?;
        index = next_index(index)?;
    }
    Ok(played)
}

/// Whether an Attack row, read after a tick, has launched this segment: a
/// wave counted, the one field `end()` does not reset ([`Hunts`]).
fn launched(row: &AttackRow) -> bool {
    row.waves_used > 0
}

/// The segment after `index`.
///
/// # Errors
///
/// [`Exit::Internal`] past a `usize`, which a run bounded by its round limit
/// never reaches.
fn next_index(index: usize) -> Result<usize, Failure> {
    index
        .checked_add(1)
        .ok_or_else(|| Failure::internal("a segment index past a usize"))
}

/// The round segment `index` plays, counted from 1, from the runner's own
/// count of the segments it has opened, and the sim's round held to it: an
/// event's round is never inferred from its tick, and the two counts cannot
/// drift apart in silence.
///
/// # Errors
///
/// [`Exit::Internal`] for an index past a `u32`'s rounds, which the sim's
/// `u32` round count would have refused first, and for a match standing in
/// another round than the one this runner opened.
fn round_of(surface: &Surface, index: usize) -> Result<u32, Failure> {
    held_round(index, round(surface)?)
}

/// [`round_of`]'s rule, as a pure function of the runner's segment index and
/// the sim's round (fix pass 1 of #219, item 7): the round the index opens,
/// counted from 1, when the sim stands in that round, and otherwise the
/// disagreement, named.
fn held_round(index: usize, sim_round: u32) -> Result<u32, Failure> {
    let opened = u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or_else(|| Failure::internal(format!("segment {index} is past a u32's rounds")))?;
    if sim_round == opened {
        Ok(opened)
    } else {
        Err(Failure::internal(format!(
            "this runner opened its segment {index} as round {opened} and the match is in round \
             {sim_round}"
        )))
    }
}

/// What a refusal of a seat's playbook by `submit_plan` means for the run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Refused {
    /// The harness door is taken, with a note ([`seal`]).
    HarnessDoor,
    /// The run fails: an arm that is broken on this seed and round.
    BrokenArm,
    /// The run fails: an arena contestant that is broken on this seed, seat
    /// and round.
    BrokenContestant,
    /// The run fails: a `sealed` seat's file that the door it was sealed
    /// through no longer takes, so the match it came from is not the one
    /// being played.
    BrokenReplay,
}

impl Refused {
    /// The run's failure for a refusal said as `why`, or `None` where the
    /// harness door is taken instead.
    fn failure(self, seat: u8, relative: &str, round: u32, why: &str) -> Option<Failure> {
        match self {
            Refused::HarnessDoor => None,
            Refused::BrokenArm => Some(Failure::new(
                Exit::Failed,
                strings::nightly_arm_refused(seat, relative, round, why),
            )),
            Refused::BrokenContestant => Some(Failure::new(
                Exit::Failed,
                strings::arena_playbook_refused(seat, relative, round, why),
            )),
            Refused::BrokenReplay => Some(Failure::new(
                Exit::Failed,
                strings::sealed_replay_refused(seat, relative, round, why),
            )),
        }
    }
}

/// Seal every `playbook` seat's file, and every `sealed` seat's file of this
/// round, for the Lull the match stands in.
///
/// A `playbook` seat the verifier refuses goes through the harness door with
/// a note, as [`seal`] says, except in a run to the match's end (`nightly`),
/// where [`seal`] fails the run on a refusal instead: a broken arm. A `sealed`
/// seat's refusal always fails the run.
fn seal_all(
    root: &Path,
    surface: &mut Surface,
    scenario: &Scenario,
    tokens: &[Token],
    to_end: Option<ToEnd>,
    notes: &mut Vec<String>,
) -> Result<(), Failure> {
    for (seat, token) in scenario.seats.iter().zip(tokens) {
        let (relative, refused) = match &seat.kind {
            SeatKind::Playbook(relative) => (
                relative.clone(),
                match to_end {
                    None => Refused::HarnessDoor,
                    Some(ToEnd::Nightly) => Refused::BrokenArm,
                    Some(ToEnd::Arena) => Refused::BrokenContestant,
                },
            ),
            SeatKind::Sealed(folder) => {
                (sealed_file(folder, round(surface)?), Refused::BrokenReplay)
            }
            SeatKind::Safe | SeatKind::Builtin => continue,
        };
        let relative = relative.as_str();
        let Door::Harness(codes) = seal(root, surface, token, seat.seat, relative, refused)? else {
            continue;
        };
        let note = strings::scenario_harness_seal(seat.seat, relative, &codes);
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    Ok(())
}

/// Read the fingerprint of what each `playbook` and `sealed` seat has sealed
/// this round, off the gateway's own seal.
///
/// A `playbook` seat seals one file every round, so a round whose
/// fingerprint differs from an earlier round's is the file changing while the
/// match played, and the run is refused: a run is a claim about one text for
/// each such seat. A `sealed` seat seals a file a round, and its last is
/// kept.
fn read_fingerprints(
    surface: &Surface,
    scenario: &Scenario,
    fingerprints: &mut BTreeMap<u8, u64>,
) -> Result<(), Failure> {
    for seat in &scenario.seats {
        if matches!(seat.kind, SeatKind::Playbook(_) | SeatKind::Sealed(_)) {
            let id = SeatId::new(seat.seat);
            let sealed = surface
                .sealed_plan(Subject::Seat(id), id)
                .map_err(internal)?;
            let now = sealed.plan_fingerprint();
            let before = fingerprints.insert(seat.seat, now);
            if let (SeatKind::Playbook(relative), Some(before)) = (&seat.kind, before)
                && before != now
            {
                return Err(Failure::input(strings::scenario_playbook_changed(
                    seat.seat,
                    relative,
                    round(surface)?,
                )));
            }
        }
    }
    Ok(())
}

/// Step the open Push to its segment's end, writing the chain and the
/// events, and say how many ticks it played.
fn play_push(
    surface: &mut Surface,
    seen: &mut usize,
    round: u32,
    played: &mut Played,
) -> Result<u32, Failure> {
    let mut ticks = 0_u32;
    while let Some(report) = surface.step().map_err(internal)? {
        let tick = report.tick.raw();
        let _ = writeln!(played.chain, "{tick}\t{}", pharmakos_sim::hex(report.hash));
        drain(surface, seen, tick, round, played)?;
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
    let final_audit = pharmakos_sim::audit::final_audit(host.world());
    let winners = match outcome.winner {
        Some(seat) => vec![seat.raw()],
        None => final_audit.winners.iter().map(|seat| seat.raw()).collect(),
    };
    let audit = final_audit
        .lines
        .iter()
        .map(|line| Audited {
            seat: line.seat.raw(),
            held: line.held.raw(),
            destroyed: line.destroyed.raw(),
            beacons_lost: line.beacons_lost,
        })
        .collect();
    Ok(Ending {
        winners,
        reason: outcome.reason.name(),
        end_tick: outcome.at.raw(),
        rounds: host.runner().round(),
        audit,
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
                segment_lengths_ms: scenario.segment_lengths_ms.clone(),
                round_limit: scenario.round_limit,
            },
        },
        library,
    )
    .map_err(|error| {
        Failure::input(format!(
            "this scenario's match would not open: {}",
            error.message()
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
    surface
        .set_phase_remaining_ms(whole_lull(&surface)?)
        .map_err(internal)?;
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
/// `compile_canonical` does, `plan-core`'s canonical form into `Plan::compile`,
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
    refused: Refused,
) -> Result<Door, Failure> {
    // Read no further than the size cap needs, as `gamectl verify` reads one
    // (S3's `vfy6`, the review of PR #179).
    let playbook = crate::verify::read_bounded(&root.join(relative))
        .map_err(|error| {
            Failure::input(strings::unreadable(
                "playbook",
                relative,
                &error.to_string(),
            ))
        })?
        .ok_or_else(|| {
            Failure::input(strings::unreadable(
                "playbook",
                relative,
                &format!(
                    "it is past the {} characters a playbook may be",
                    pharmakos_verifier::MAX_PLAYBOOK_CHARS
                ),
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
        // on this seed and round, as the module docs promise; and a `sealed`
        // seat's is a broken replay.
        if let Some(failure) = refused.failure(seat, relative, round(surface)?, &message_of(error))
        {
            return Err(failure);
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
    // A nightly run and a `sealed` seat never take the harness door: the
    // refusal fails the run, naming the round, before anything is sealed.
    if let Some(failure) = refused.failure(seat, relative, round(surface)?, &codes) {
        return Err(failure);
    }
    // The second door, the gateway's own: it verifies the text once more,
    // compiles, prices and fingerprints that one decode, and files the seal
    // (`Surface::seal_by_harness`), so the harness's seal carries its own
    // text's fingerprint and price like every other (S3's `fpstream`, part 2,
    // fix pass 2). A refusal of the playbook itself is the scenario's input
    // to fix; anything else is this build disagreeing with itself.
    surface
        .seal_by_harness(SeatId::new(seat), &playbook)
        .map_err(|error| {
            if error.code() == pharmakos_gateway::error::Code::InvalidArgument {
                Failure::input(strings::scenario_playbook_refused(
                    seat,
                    relative,
                    error.message(),
                ))
            } else {
                internal(error)
            }
        })?;
    Ok(Door::Harness(codes))
}

/// The refused report's codes and pointers, `CODE at /pointer`, joined.
fn codes_of(result: Option<&Json>) -> Result<String, Failure> {
    // The codes, not the count. "2 diagnostics" sends the reader back to the
    // tool; the codes and their pointers say what is wrong on their own, and
    // this line is read by somebody who has just watched a build go red.
    let mut codes: Vec<String> = Vec::new();
    let report = result.and_then(|result| result.get("report"));
    if let Some(Json::Array(items)) = report.and_then(|report| report.get("diagnostics")) {
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
            // An `I0007` is the player's words for one `left_out` entry (S3's
            // `leftout`), which is said below from the field itself.
            if code != "I0007" {
                codes.push(format!("{code} at {path}"));
            }
        }
    }
    // What the report counted and did not list, from its typed field, so a
    // reader of this line does not take the listed codes for all there were.
    // proto3 JSON leaves an empty list out; anything else is the wire's shape
    // having moved.
    let gaps = match report.and_then(|report| report.get("left_out")) {
        None => &[][..],
        Some(Json::Array(gaps)) => gaps.as_slice(),
        Some(other) => {
            return Err(Failure::internal(format!(
                "`submit_plan` answered with a left_out that is not a list ({other:?})"
            )));
        }
    };
    for gap in gaps {
        codes.push(left_out_said(gap, report)?);
    }
    Ok(codes.join("; "))
}

/// One `left_out` entry of a refused report, worded from the field
/// (`strings::scenario_left_out`): its code, count, `max` and severity, and
/// whether any of the code is listed. An entry missing any of them, or with a
/// severity the schema does not name, is the wire's shape having moved.
fn left_out_said(gap: &Json, report: Option<&Json>) -> Result<String, Failure> {
    let malformed = || {
        Failure::internal(format!(
            "`submit_plan` answered with a left-out entry that is not a code, a count, a most \
             and a severity ({gap:?})"
        ))
    };
    let (Some(Json::String(code)), Some(Json::Number(count)), Some(Json::Number(max))) =
        (gap.get("code"), gap.get("count"), gap.get("max"))
    else {
        return Err(malformed());
    };
    let severity = match gap.get("severity") {
        Some(Json::String(severity))
            if ["error", "warning", "info"].contains(&severity.as_str()) =>
        {
            severity.as_str()
        }
        _ => return Err(malformed()),
    };
    let listed_any = match report.and_then(|report| report.get("diagnostics")) {
        Some(Json::Array(items)) => items
            .iter()
            .any(|item| matches!(item.get("code"), Some(Json::String(listed)) if listed == code)),
        _ => false,
    };
    Ok(strings::scenario_left_out(
        code, count, max, severity, listed_any,
    ))
}

// ---------------------------------------------------------------------------
// Reading what happened
// ---------------------------------------------------------------------------

/// Copy the feed's new lines into the record, dated at `tick` and in
/// `round`, and read the hunts ([`Hunts`]) from the world as it stands.
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
    round: u32,
    played: &mut Played,
) -> Result<(), Failure> {
    played
        .hunts
        .read(surface.host().map_err(internal)?.world())?;
    let into = &mut played.events;
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
            round,
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

fn internal(error: pharmakos_gateway::Error) -> Failure {
    // The gateway's own sentence, with this runner's frame around it: a host
    // that drove its own surface wrongly is not something a scenario author
    // did, and the message has to say whose mistake it is. Composed through
    // `said!` from a literal and the gateway's `Said` (fix pass 4b): every
    // gateway refusal is made from a `Said`, so player text in it is already
    // quoted, and nothing else reaches this sentence.
    Failure::new(
        Exit::Internal,
        pharmakos_proto::said!(
            "the gateway refused a host's own call: {}",
            error.into_said()
        )
        .into_string(),
    )
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
        played.ticks()?,
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
                failed = failed.checked_add(1).ok_or_else(|| {
                    Failure::internal("more failed assertions than a usize counts")
                })?;
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
    // A golden under `tests/golden/` has its fresh chain written beside the
    // target directory's copy, where the `golden` step compares it; with no
    // target directory the run cannot be checked there, and says so. A
    // golden outside `tests/golden/` has no fresh path by the convention
    // (`crate::actual_for`), which the scenario format refuses and the
    // harness's own tests use to compare a doctored chain without writing it
    // where the `golden` step reads (T20's items 11 and 12): compared, and
    // not written.
    let written = match crate::actual_for(golden) {
        Ok(path) => {
            write_actual(&path, &played.chain)?;
            crate::display(&path)
        }
        Err(crate::ActualError::NotAGolden) => String::from("<nowhere: not under tests/golden/>"),
        Err(error @ crate::ActualError::NoTargetDir(_)) => {
            return Err(Failure::internal(strings::scenario_chain_unwritable(
                golden,
                &error.to_string(),
            )));
        }
    };

    // Not there is the assertion's answer; any other failure to read the
    // committed chain is the file, said with its error.
    let committed = match std::fs::read_to_string(root.join(golden)) {
        Ok(committed) => committed,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Err(strings::scenario_chain_missing(golden, &written)));
        }
        Err(error) => {
            return Err(Failure::input(strings::unreadable(
                "committed chain",
                golden,
                &error.to_string(),
            )));
        }
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
    // The line number shown, from 1. `at` is at most the shorter chain's
    // length, a count of lines held in memory, so one past it is refused only
    // if a usize could not count them.
    let line = at
        .checked_add(1)
        .ok_or_else(|| Failure::internal("a chain of more lines than a usize counts"))?;
    Ok(Err(strings::scenario_chain_moved(
        golden,
        line,
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
    use super::{Hunts, Played, hash_chain_equals, next_index, open, whole_lull};
    use crate::exit::Exit;
    use std::path::Path;

    fn played(segments: Vec<u32>) -> Played {
        Played {
            chain: String::new(),
            events: Vec::new(),
            segments,
            notes: Vec::new(),
            rules_hash: 0,
            hunts: Hunts::default(),
            fingerprints: std::collections::BTreeMap::new(),
        }
    }

    /// Fix pass 1 of #219, item 7: the runner's round is its segment index
    /// plus one, and a sim standing in any other round is refused, naming both.
    #[test]
    fn the_runner_s_round_is_held_to_the_sim_s() {
        assert_eq!(super::held_round(0, 1), Ok(1));
        assert_eq!(super::held_round(5, 6), Ok(6));
        for (index, sim) in [(0, 0), (0, 2), (1, 1), (5, 7)] {
            let refused = super::held_round(index, sim).expect_err("a disagreement");
            assert_eq!(refused.code, Exit::Internal);
            assert!(
                refused.message.contains(&format!("in round {sim}")),
                "{}",
                refused.message
            );
        }
    }

    /// Fix pass 1 of #219, item 6: a hunt that launched and ended on the same
    /// tick, read after that tick, is regathered with no launch stamp and one
    /// wave counted; it launched. A row that never launched has no wave.
    #[test]
    fn a_launch_is_its_wave_count_even_after_a_same_tick_end() {
        use pharmakos_sim::attack::{AttackRow, NEVER, Phase};
        use pharmakos_sim::tables::BeaconId;
        let mut row = AttackRow::fresh(BeaconId::new(3));
        assert!(!super::launched(&row), "a fresh row has not launched");
        row.waves_used = 1;
        row.phase = Phase::Gathering;
        row.launched_at = NEVER;
        assert!(
            super::launched(&row),
            "ended on its launch tick, it launched"
        );
        row.waves_used = 0;
        row.phase = Phase::Launched;
        row.launched_at = 120;
        assert!(
            !super::launched(&row),
            "a force still out from an earlier segment was counted when it launched there; the count alone decides"
        );
    }

    /// E22: the total of a run's segments is refused past a `u32`, never
    /// saturated, and so is a segment index past a `usize`.
    #[test]
    fn a_count_past_its_type_is_refused() {
        assert_eq!(played(vec![3_600, 6_000]).ticks(), Ok(9_600));
        let refused = played(vec![u32::MAX, 1]).ticks().expect_err("past a u32");
        assert_eq!(refused.code, Exit::Internal);
        assert_eq!(next_index(4), Ok(5));
        assert_eq!(
            next_index(usize::MAX).map_err(|failure| failure.code),
            Err(Exit::Internal)
        );
    }

    /// A committed chain that is there and cannot be read is the file's
    /// fault, said with its error, not a chain that is missing (E22). The
    /// goldens are outside `tests/golden/`, so the run writes no fresh
    /// chain into the tree the `golden` step and the cross-OS artefact read
    /// (fix pass 1 of #219, item 8).
    #[test]
    fn a_committed_chain_that_cannot_be_read_is_an_input_error() {
        let folder = crate::target_dir()
            .expect("a target directory")
            .join("scratch")
            .join("nite3-unreadable-chain");
        let golden = "scratch/nite3-unreadable/expected.hashes.txt";
        // A folder where the chain's file should be: there, and not a file.
        std::fs::create_dir_all(folder.join(golden)).expect("a scratch folder");
        let refused = hash_chain_equals(&folder, golden, &played(vec![1])).expect_err("refused");
        assert_eq!(refused.code, Exit::Input, "{}", refused.message);
        // And one that is not there is the assertion's own answer.
        let missing = "scratch/nite3-missing/expected.hashes.txt";
        let answer = hash_chain_equals(&folder, missing, &played(vec![1])).expect("an answer");
        assert!(
            answer.is_err_and(|why| why.contains("nothing is committed")),
            "a missing chain is a failed assertion"
        );
        for written in ["nite3-unreadable", "nite3-missing"] {
            let fresh = crate::target_dir()
                .expect("a target directory")
                .join("golden")
                .join("scenarios")
                .join(written);
            assert!(!fresh.exists(), "{} was written", fresh.display());
        }
    }

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

    /// The harness door's seal carries the fingerprint the gateway's report
    /// was written with, which is its own text's (S3's `fpstream`, part 2),
    /// and a save of the match stamps it beside the text.
    #[test]
    fn the_harness_door_s_seal_carries_its_text_s_fingerprint() {
        use pharmakos_gateway::token::Subject;
        use pharmakos_sim::tables::SeatId;
        let source = root().join("scenarios/s2/mortar-breaks-wall.scenario.jsonc");
        let scenario = crate::scenario::load(root(), &source).expect("a committed scenario");
        let rules = super::load_rules(root(), &scenario).expect("its rules table");
        let mut surface = open(root(), &scenario, rules).expect("the match opens");
        let tokens = super::mint(&mut surface, &scenario).expect("tokens");
        let mut harnessed = 0_usize;
        for (seat, token) in scenario.seats.iter().zip(&tokens) {
            let super::SeatKind::Playbook(relative) = &seat.kind else {
                continue;
            };
            let door = super::seal(
                root(),
                &mut surface,
                token,
                seat.seat,
                relative,
                super::Refused::HarnessDoor,
            )
            .expect("sealed");
            if matches!(door, super::Door::Harness(_)) {
                harnessed += 1;
            }
            let id = SeatId::new(seat.seat);
            let sealed = surface.sealed_plan(Subject::Seat(id), id).expect("a seal");
            let canonical = pharmakos_plan_core::decode_jsonc(sealed.playbook_jsonc())
                .expect("a canonical form");
            assert_eq!(
                sealed.plan_fingerprint(),
                {
                    // The rule written out whole: the reference, test-only.
                    let mut bytes = pharmakos_proto::fingerprint::PLAN_FINGERPRINT_DOMAIN.to_vec();
                    bytes.extend_from_slice(
                        &pharmakos_proto::fingerprint::canonical_input(
                            canonical.decoded().message(),
                        )
                        .expect("canonical"),
                    );
                    pharmakos_sim::encoding::digest(&bytes)
                },
                "seat {}: the carried fingerprint is the text's",
                seat.seat
            );
            // Filed for the round it is sealed in, by the harness and not by
            // the gateway for a seat that sealed nothing.
            assert_eq!(
                sealed.round(),
                surface.time().round,
                "seat {}: the seal is for this round",
                seat.seat
            );
            assert!(!sealed.filed_by_the_gateway(), "seat {}", seat.seat);
        }
        assert_eq!(harnessed, 1, "this scenario is the harness door's one");
        let saved = surface.lull_save().expect("a Lull saves");
        for held in &saved.seats {
            let Some(seal) = held.seal.as_ref() else {
                continue;
            };
            let id = SeatId::new(held.seat);
            assert_eq!(
                seal.plan_fingerprint,
                surface
                    .sealed_plan(Subject::Seat(id), id)
                    .expect("a seal")
                    .plan_fingerprint()
            );
        }
    }

    /// The harness door reads its text twice and takes no fingerprint of its
    /// own: the codec's work over `seal` is the request it writes and parses,
    /// the gateway's `submit_plan` of the text (which refuses it), and the
    /// gateway's `seal_by_harness`, whose one FULL verify reads the text as the
    /// submission did and whose compile, price and fingerprint read that
    /// decode, each measured on its own here, and nothing more.
    #[test]
    fn the_harness_door_reads_its_text_as_two_full_verifies_do() {
        use pharmakos_gateway::fog::Blind;
        use pharmakos_proto::json::{Json, work};
        let source = root().join("scenarios/s2/mortar-breaks-wall.scenario.jsonc");
        let scenario = crate::scenario::load(root(), &source).expect("a committed scenario");
        let rules = super::load_rules(root(), &scenario).expect("its rules table");
        let (seat, relative) = scenario
            .seats
            .iter()
            .find_map(|seat| match &seat.kind {
                super::SeatKind::Playbook(relative) => Some((seat.seat, relative.clone())),
                _ => None,
            })
            .expect("a playbook seat");
        let index = scenario
            .seats
            .iter()
            .position(|held| held.seat == seat)
            .expect("its index");

        // The door, measured whole.
        let mut surface = open(root(), &scenario, rules.clone()).expect("the match opens");
        let tokens = super::mint(&mut surface, &scenario).expect("tokens");
        let token = tokens.get(index).expect("its token");
        let _ = work::take();
        let door = super::seal(
            root(),
            &mut surface,
            token,
            seat,
            &relative,
            super::Refused::HarnessDoor,
        )
        .expect("sealed");
        let whole = work::take();
        assert!(matches!(door, super::Door::Harness(_)), "{door:?}");

        // Its parts, each on its own, on a second match opened the same way.
        let playbook = crate::verify::read_bounded(&root().join(&relative))
            .expect("readable")
            .expect("under the cap");
        let mut twin = open(root(), &scenario, rules).expect("the match opens");
        let twins = super::mint(&mut twin, &scenario).expect("tokens");
        let twin_token = twins.get(index).expect("its token");
        let _ = work::take();
        let params = pharmakos_proto::json::write(&Json::Object(vec![(
            String::from("playbook_jsonc"),
            Json::String(playbook.clone()),
        )]));
        let request = pharmakos_gateway::rpc::parse(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"submit_plan","params":{params}}}"#
        ))
        .expect("a request");
        let written = work::take();
        let _ = twin.call(Some(twin_token), &request, &Blind);
        let submitted = work::take();
        assert!(submitted > 0, "a submission reads the text");
        // The harness's verify is the submission's, of the same text against
        // the same snapshot, and its compile, price and fingerprint read that
        // decode and write no text.
        assert_eq!(
            whole,
            written + 2 * submitted,
            "the harness door read or wrote the playbook's text beyond its request, its \
             submission and its one verify"
        );
    }

    /// **The refused seal's line words each `left_out` entry from the field**
    /// (S3's `leftout`, fix pass 1): one entry of a code at the cap names the
    /// cap as sent; one of a code with none listed (an `I0003` past the size
    /// budget) names no cap; an `I0007` row is not shown beside them; and an
    /// entry missing its count, or with a severity the schema does not name,
    /// is the wire's shape having moved.
    #[test]
    fn codes_of_words_each_left_out_entry_from_the_field() {
        use pharmakos_proto::json::read;
        let result = |left_out: &str| {
            read(&format!(
                r#"{{"report":{{"diagnostics":[{{"code":"E0303","path":"/declarative/route/0"}},{{"code":"E0110","path":"/declarative"}},{{"code":"I0007"}}],"left_out":[{left_out}]}}}}"#
            ))
            .expect("json")
        };
        let at_cap = result(r#"{"code":"E0303","count":3,"max":7,"severity":"error"}"#);
        assert_eq!(
            super::codes_of(Some(&at_cap)).expect("worded"),
            "E0303 at /declarative/route/0; E0110 at /declarative; 3 more E0303 (error) not \
             listed: a report lists at most 7 of one code"
        );
        let past_budget = result(r#"{"code":"I0003","count":5,"max":7,"severity":"info"}"#);
        let words = super::codes_of(Some(&past_budget)).expect("worded");
        assert_eq!(
            words,
            "E0303 at /declarative/route/0; E0110 at /declarative; 5 I0003 (info) left out with \
             none of it listed"
        );
        assert!(!words.contains("at most"), "{words}");
        for malformed in [
            r#"{"code":"E0303","max":7,"severity":"error"}"#,
            r#"{"code":"E0303","count":3,"max":7}"#,
            r#"{"code":"E0303","count":3,"max":7,"severity":"severity_unspecified"}"#,
        ] {
            let refused = super::codes_of(Some(&result(malformed))).expect_err(malformed);
            assert_eq!(refused.code, Exit::Internal, "{malformed}");
        }
        let not_a_list = read(r#"{"report":{"diagnostics":[],"left_out":5}}"#).expect("json");
        assert_eq!(
            super::codes_of(Some(&not_a_list))
                .expect_err("a number")
                .code,
            Exit::Internal
        );
    }
}
