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
//! **read** the runner, through `Host::runner()`, for three values a report
//! needs: the tick a Lull is standing on, the phase a refused `begin_push` was
//! in, and the round a seal belongs to. `Host::runner` hands out a `&Runner`
//! and every stepping method on it needs `&mut`, so the reach cannot become a
//! second way to play a match; if T16a gives `Surface` accessors for those
//! three, this file should take them and `.runner()` should join the needles.
//!
//! # What is fixed here rather than in the file, and why
//!
//! Two inputs of a match are not in the scenario format, so they are written
//! down here where a reader can find them:
//!
//! * `units_per_seat` is **0**. Both existing producers of a committed chain
//!   use it (`crates/sim/tests/scenario.rs`, `crates/gateway/tests/methods.rs`),
//!   and a scenario's hash chain has to be reproducible by them.
//! * `round_limit` is [`DEFAULT_ROUND_LIMIT`], the spec's default and a lobby
//!   setting rather than a tuning row (decisions-log item 102 (7)).
//!
//! PLACEHOLDER: both belong in the scenario file the day a scenario wants to
//! vary them — `units_per_seat` when S1's units arrive and a scenario wants a
//! seat with a roster, `round_limit` when a scenario plays a match to its end.
//! Adding a key is additive and is not a format break (`scenarios/README.md`),
//! but it is still a contract path, so the **owner** decides, at the stage that
//! first needs it. Today neither is variable and a file that could set them
//! would be a knob nothing turns.

use std::fmt::Write as _;
use std::path::Path;

use pharmakos_gateway::fog::{Audience, Blind, FogPolicy};
use pharmakos_gateway::host::Host;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{DEFAULT_ROUND_LIMIT, MatchSettings};
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::WorldConfig;

use crate::exit::{Exit, Failure};
use crate::scenario::{Assertion, Scenario, SeatKind, ticks_of};
use crate::strings;

/// No harness walkers. See the module doc.
const UNITS_PER_SEAT: u32 = 0;

/// The most of the hash chain this runner reserves up front, in bytes.
///
/// Sixteen megabytes is about seven hundred thousand ticks, which is nine and
/// a half hours of game time — past anything a scenario plays and short of
/// anything that hurts. It caps the *reservation* only.
///
/// PLACEHOLDER: the real fix is a cap on `length_ms` in the scenario format,
/// reported with a pointer the way `MAX_SEATS` is. That cap has to hold in
/// `xtask/src/scenario.rs` as well or the two readers of the format stop
/// agreeing, and `xtask` is a contract path (AGENTS.md §5). **OWNER**, with the
/// format's other open question in this task's pull request.
const CHAIN_RESERVE_CAP: usize = 16 * 1024 * 1024;

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
/// (a `builtin` seat, a playbook the verifier refuses), and [`Exit::Internal`]
/// for anything the gateway refuses that no scenario could have caused.
pub fn play(root: &Path, scenario: &Scenario) -> Result<Played, Failure> {
    let rules = load_rules(root, scenario)?;
    let rules_hash = rules.rules_hash();
    let mut surface = open(root, scenario, rules)?;
    let tokens = mint(&mut surface, scenario)?;

    let mut played = Played {
        // Reserved, and capped. A line is twenty-four bytes, so the reservation
        // is the chain's real size for every scenario anybody writes — but
        // `length_ms` is any positive int32, and two segments of `i32::MAX`
        // would ask for two gigabytes before the first tick. The cap is a
        // reservation, not a limit on the run: a chain longer than
        // [`CHAIN_RESERVE_CAP`] grows the ordinary way. Reviewed finding,
        // wave 5.
        chain: String::with_capacity(
            usize::try_from(scenario.ticks().saturating_mul(24))
                .unwrap_or(CHAIN_RESERVE_CAP)
                .min(CHAIN_RESERVE_CAP),
        ),
        events: Vec::new(),
        segments: Vec::with_capacity(scenario.segments.len()),
        notes: Vec::new(),
        rules_hash,
    };
    let mut seen = 0_usize;

    for segment in &scenario.segments {
        // Whatever the last phase left on the feed, dated at the tick the match
        // is standing on. A Lull consumes no sim tick, so that is the tick
        // every line of it belongs to.
        let standing = surface_tick(&surface)?;
        drain(&surface, &mut seen, standing, &mut played.events);

        for (seat, token) in scenario.seats.iter().zip(&tokens) {
            if let SeatKind::Playbook(relative) = &seat.kind {
                if let Door::Harness(codes) = seal(root, &mut surface, token, seat.seat, relative)?
                {
                    let note = strings::scenario_harness_seal(seat.seat, relative, &codes);
                    if !played.notes.contains(&note) {
                        played.notes.push(note);
                    }
                }
            }
        }

        // `begin_push` opens a fresh segment feed, so the count starts again.
        let anchor = surface_tick(&surface)?;
        let started = surface.begin_push().map_err(internal)?;
        if !started {
            return Err(Failure::internal(format!(
                "the match would not open segment {}: it is in its {}",
                segment.index,
                phase_name(&surface)?
            )));
        }
        seen = 0;
        drain(&surface, &mut seen, anchor, &mut played.events);

        let wanted = ticks_of(segment.length_ms);
        let mut ticks = 0_u32;
        while let Some(report) = surface.step().map_err(internal)? {
            let tick = report.tick.raw();
            let _ = writeln!(played.chain, "{tick}\t{}", pharmakos_sim::hex(report.hash));
            drain(&surface, &mut seen, tick, &mut played.events);
            ticks = ticks.saturating_add(1);
            if report.segment_ended {
                break;
            }
        }
        played.segments.push(ticks);
        if ticks != wanted {
            return Err(Failure::new(
                Exit::Failed,
                strings::scenario_segment_short(segment.index, ticks, wanted),
            ));
        }

        surface.end_recap().map_err(internal)?;
        if segment.index.saturating_add(1) < scenario.segments.len() {
            surface.set_phase_remaining_ms(lull_ms(&surface)?);
            surface.open_lull().map_err(internal)?;
        }
    }
    Ok(played)
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

/// A hosted match in its opening Lull, with the rate limits a batch client
/// needs.
fn open(root: &Path, scenario: &Scenario, rules: RulesTable) -> Result<Surface, Failure> {
    let seats: Vec<SeatId> = scenario
        .seats
        .iter()
        .map(|seat| SeatId::new(seat.seat))
        .collect();
    for (index, seat) in scenario.seats.iter().enumerate() {
        if seat.kind == SeatKind::Builtin {
            return Err(Failure::input(strings::scenario_builtin_seat(&format!(
                "/seats/{index}/kind"
            ))));
        }
    }
    let count = u32::try_from(scenario.seats.len())
        .map_err(|_| Failure::internal("a match of more seats than a u32 can count"))?;
    let host = Host::open(
        &WorldConfig {
            match_seed: scenario.seed,
            seats: count,
            units_per_seat: UNITS_PER_SEAT,
            rules: rules.clone(),
            match_settings: MatchSettings {
                segment_lengths_ms: scenario
                    .segments
                    .iter()
                    .map(|segment| segment.length_ms)
                    .collect(),
                round_limit: DEFAULT_ROUND_LIMIT,
            },
        },
        // No template library: a scenario names its playbooks as files and
        // never instantiates one, and a host handed a folder it does not read
        // is a path in a message that means nothing.
        None,
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
    surface.set_phase_remaining_ms(lull_ms(&surface)?);
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
    let _ = root;
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
/// `expand-east-segment.scenario.jsonc` seals the spec's own worked
/// `expand_east` on the skeleton's map, and **that playbook does not qualify
/// there**: it names `b_01`, which on this map is seat 1's core and is not in
/// seat 0's view at all (`E0401`), and its site is two hundred and sixty voxels
/// from seat 0's sphere (`E0403`). That is decisions-log item 103 (3)'s finding
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

    // The codes, not the count. "2 diagnostics" sends the reader back to the
    // tool; the codes and their pointers say what is wrong on their own, and
    // this line is read by somebody who has just watched a build go red.
    let mut codes: Vec<String> = Vec::new();
    if let Some(Json::Array(items)) = result
        .and_then(|result| result.get("report"))
        .and_then(|report| report.get("diagnostics"))
    {
        for item in items {
            let code = match item.get("code") {
                Some(Json::String(code)) => code.clone(),
                _ => String::from("?"),
            };
            let path = match item.get("path") {
                Some(Json::String(path)) => path.clone(),
                _ => String::new(),
            };
            codes.push(format!("{code} at {path}"));
        }
    }
    // Decoded, not stored as its own text. `Sealed::report_hash` is documented
    // as "eight big-endian bytes" and the wire spells it base64
    // (`surface/planning.rs`), so `text.into_bytes()` would have put twelve
    // ASCII characters where the eight bytes go — latent today because nothing
    // outside tests reads the field, and T17's restore path is named as the
    // first reader. Reviewed finding, wave 5.
    let report_hash = match result
        .and_then(|result| result.get("report"))
        .and_then(|report| report.get("report_hash"))
    {
        Some(Json::String(text)) => pharmakos_proto::json::base64::decode(text).unwrap_or_default(),
        _ => Vec::new(),
    };

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
    Ok(Door::Harness(codes.join("; ")))
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
fn drain(surface: &Surface, seen: &mut usize, tick: u32, into: &mut Vec<Fired>) {
    let events = surface.feed().events();
    let fresh = events.get(*seen..).unwrap_or_default();
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
}

fn surface_tick(surface: &Surface) -> Result<u32, Failure> {
    Ok(surface.host().map_err(internal)?.runner().tick().raw())
}

fn phase_name(surface: &Surface) -> Result<&'static str, Failure> {
    Ok(surface.host().map_err(internal)?.runner().phase().name())
}

/// `rules.match.lull_ms`, which is what a client counts down and what the
/// gateway derives its Lull tick from. Read from the table, never written here
/// (AGENTS.md §12).
fn lull_ms(surface: &Surface) -> Result<Ms, Failure> {
    let host = surface.host().map_err(internal)?;
    let lull = host
        .rules()
        .message()
        .r#match
        .as_ref()
        .map(|block| block.lull_ms)
        .ok_or_else(|| {
            Failure::internal(
                "the rules table carries no `match` block, so nothing says how long a Lull is",
            )
        })?;
    Ok(Ms::new(lull))
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
        expected.get(at).copied().unwrap_or("<end of file>"),
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
