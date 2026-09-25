// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The match host: the one place in this crate that steps a [`Runner`].
//!
//! Spec section 15 puts the match in the gateway layer, and the crate map says
//! the gateway "hosts the match". T9 built the surface with no match behind it;
//! T13 puts one there. The whole of the driving lives here, in four calls
//! ([`Host::seal_plans`], [`Host::begin_push`], [`Host::step`],
//! [`Host::end_recap`]), and **no method handler may reach any of them** —
//! `verify_plan`, `estimate_route`, `get_economy_forecast`, `render_plan`,
//! `patch_plan` and `instantiate_template` never step a runner and never clone
//! one to step the copy. That is AGENTS.md section 3 rule 2's principle, "no
//! dry runs", applied here: the rule names `plan-core` and the verifier and
//! does not mention the gateway at all, which is a gap the pull request raises
//! rather than a rule this crate is breaking. `tests/confinement.rs` asserts it
//! over this crate's own source text.
//!
//! # The seal is the fourth call, and it is new
//!
//! T13 shipped with three: the gateway sealed a verified submission in its own
//! private store ([`crate::surface::Sealed`]) and the hosted world never heard
//! of it, because the sim it was built against had only the interpreter seam.
//! T11 then landed `Plan::compile` and `World::seal_playbook`, and T13b wires
//! the two together (decisions-log item 103 (1)): a playbook is compiled at
//! `submit_plan`, held with the seal, and filed into the runner by
//! [`Host::seal_plans`] while the match is still in its Lull. Before that a
//! hosted match played out with every commander standing still.
//!
//! # A resume is here too (T17)
//!
//! [`Host::resume`] puts a save's planning snapshot into a freshly opened
//! match: the one restore this crate does, beside the four driving calls and
//! under the same rule (`tests/confinement.rs` holds `.restore(` to this
//! module). The saved orders are not sealed by it; they go back into the
//! seats' private store and reach the match through [`Host::seal_plans`] like
//! any other, at [`crate::surface::Surface::begin_push`].
//!
//! # The Lull and the recap end on the host's word, not on a clock
//!
//! The gateway is not a walled crate, so it reads no clock at all
//! (decisions-log item 99's closing note; [`crate::time`]). `rules.match.lull_ms`
//! is a number the *client* counts down, and what reaches this crate is the
//! client's answer — [`crate::surface::Surface::set_phase_remaining_ms`] — and
//! its decision, which is a call to [`Host::begin_push`]. The sim agrees: T10's
//! runner spends no tick in a Lull or a recap and both end when the host says
//! so.
//!
//! # A degenerate lobby setting is refused here
//!
//! Decisions-log item 102 (8): the sim **clamps** a zero or negative segment
//! length and a round limit of zero, because a default snapshot must stay
//! restorable and a restore has nobody to complain to. The gateway is where
//! there *is* somebody to complain to, so [`Host::check_settings`] refuses one
//! with [`crate::error::Code::InvalidArgument`] before a world is built —
//! before, rather than after, so the refusal names the setting the lobby typed
//! rather than the number the sim silently corrected it to.

use std::path::{Path, PathBuf};

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::math::quantity::MS_PER_TICK;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{MatchPhase, MatchSettings, Runner, TickReport};
use pharmakos_sim::sight::Spheres;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::voxels::VoxelEdit;
use pharmakos_sim::world::{DamageOrder, World, WorldConfig};

use crate::error::Error;
use crate::fog::Vision;
use crate::routes::RouteAdapter;

/// The gateway's **fallback** safe playbook: the library's Safe Playbook
/// template (`library/safe_playbook.jsonc`) with nothing raised.
///
/// Spec section 14 makes the safe playbook the operator's: "it files the safe
/// playbook when a seat submits nothing verified before the Lull ends", and
/// each seat's is its own, because whether power is short depends on the seat.
/// Decisions-log item 111 (decision C5) gives the operator that job through
/// [`crate::serve::Advisor`]: at every Lull's start it advises each seat it
/// does not play, and [`crate::surface::Surface::file_advice`] verifies and
/// files that seat's own. **This constant is what is filed when there is no
/// such advice** — a host with no advisor installed (`gamectl scenario run`,
/// the tests, a hosted match until T18 lands the operator) — and when an
/// advice does not qualify, which the audit log then says.
///
/// It is the spec's safe playbook in the one situation that needs no reading
/// of the seat's own economy (decision C16): move to the safest beacon, raise
/// nothing, then shadow the safest beacon, with the flee rule. It leaves every
/// beacon's configuration as it is. "Written once, tested twice" (item 81):
/// `the_gateways_fallback_is_the_safe_template_with_nothing_raised` holds it
/// canonical-equal, comments aside, to the template instantiated with no
/// parameters, so the file the editor renders and the constant a timeout
/// files cannot drift apart.
///
/// It is JSONC, comments and all, because that is what every other playbook a
/// client receives is and because the "why" note is the teaching half
/// (spec section 13).
///
/// PLACEHOLDER: the spec's rescue rule ("with the flee and rescue rules") is
/// absent, because nothing can be rescued before combat: flee only. **OWNER**,
/// at **S2/S5**.
pub const SAFE_PLAYBOOK: &str = concat!(
    "// The safe playbook: what is filed for you if the Lull ends with nothing\n",
    "// sealed and no built-in operator advised you. It spends nothing, places\n",
    "// nothing and changes no beacon.\n",
    "{\n",
    "  \"schema_version\": {\"major\": 1, \"minor\": 0},\n",
    "  \"meta\": {\n",
    "    \"title\": \"Safe playbook\",\n",
    "    \"author_kind\": \"BUILTIN\",\n",
    "    \"note\": \"Move to the safest beacon, raise the priority of any beacon about to brown out, then stay with the safest beacon.\"\n",
    "  },\n",
    "  \"kind\": \"PLAYBOOK\",\n",
    "  \"declarative\": {\n",
    "    \"route\": [\n",
    "      // Nothing is raised: the whole route is the walk to safety.\n",
    "      {\"label\": \"to_safety\", \"move\": {\"to\": {\"safest\": {}}, \"pace\": \"AVOID_KNOWN_THREATS\"}}\n",
    "    ],\n",
    "    \"handlers\": [\n",
    "      // Hurt and under fire, walk back to safety and wait.\n",
    "      {\"id\": \"flee\", \"when\": {\"all\": {\"items\": [{\"cmdr_hp_pct\": {\"cmp\": \"LE\", \"pct\": 40}}, {\"cmdr_took_damage_within\": {\"ms\": 3000}}]}},\n",
    "       \"body\": [{\"label\": \"flee_to_safety\", \"move\": {\"to\": {\"safest\": {}}}}, {\"label\": \"flee_wait\", \"hold\": {\"ms\": 8000}}],\n",
    "       \"resume\": \"CONTINUE\", \"cooldown_ms\": 30000, \"max_fires\": 3}\n",
    "    ]\n",
    "  },\n",
    "  \"on_death\": {\"on_respawn\": \"CONTINUE\"},\n",
    "  // Then stay with the safest beacon, wherever that turns out to be.\n",
    "  \"fallback\": {\"shadow\": {\"beacon\": {\"safest\": {}}}}\n",
    "}\n",
);

/// Where the gateway's own tests and `gamectl host` find the template
/// library, relative to the workspace root: the flat `library/` folder
/// (decisions-log item 111, decision C13).
///
/// PLACEHOLDER: where the library lives beside a **shipped** binary is
/// packaging's question, **T21** (`gamectl`'s `LIBRARY_PATH` names this same
/// folder for a checkout).
pub const LIBRARY_FOLDER: &str = "library";

/// The most seats a v1 match has.
///
/// Not a tuning value: spec section 1 is "up to three seats (at most one
/// human, the rest built-in)", and AGENTS.md section 11 puts "more than 3
/// seats" on the list of what v1 does not build. [`Host::open_from`] and
/// [`crate::serve::Config::parse`] refuse anything outside `1..=MAX_SEATS` as
/// [`crate::error::Code::InvalidArgument`] (decisions-log item 110 (5)):
/// before T17 `gamectl host` would host a 99-seat match when asked.
pub const MAX_SEATS: u32 = 3;

/// What the lobby chose, in plain values.
///
/// The point of this struct is what is **not** in it: no `pharmakos-sim`
/// type. AGENTS.md section 3's crate map gives `gamectl` no edge to the sim,
/// and [`Host::open`] takes the sim's
/// [`WorldConfig`](pharmakos_sim::world::WorldConfig) — so a binary that
/// wanted to host a match had to name a sim type to do it (decisions-log item
/// 107 (11)). [`Host::open_from`] takes this instead, with the rules table as
/// **text** that this crate parses, so `gamectl host` needs nothing but the
/// gateway.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Settings {
    /// The Push lengths, in game milliseconds, one per round. Empty means the
    /// rules table's own ladder (decisions-log item 40).
    pub segment_lengths_ms: Vec<i32>,
    /// How many rounds the match runs at most.
    pub round_limit: u32,
    /// How many units each seat starts with beyond its commander.
    pub units_per_seat: u32,
}

/// The match, its search graph and the folder the templates are read from.
#[derive(Debug)]
pub struct Host {
    runner: Runner,
    routes: RouteAdapter,
    library: Option<PathBuf>,
    safe_playbook: String,
}

impl Host {
    /// Open a match: check what the lobby chose, build the world, and build the
    /// estimator's own graph over it.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a lobby setting
    /// [`Host::check_settings`] refuses, and [`crate::error::Code::Internal`]
    /// when the map generator or the search graph will not build — neither is
    /// something a caller did.
    pub fn open(config: &WorldConfig, library: Option<PathBuf>) -> Result<Host, Error> {
        Host::check_settings(&config.match_settings)?;
        let world = World::new(config).map_err(|error| {
            Error::internal(format!("this match's map could not be generated: {error}"))
        })?;
        let routes = RouteAdapter::new(world.voxels(), world.rules())?;
        Ok(Host {
            runner: Runner::new(world),
            routes,
            library,
            safe_playbook: String::from(SAFE_PLAYBOOK),
        })
    }

    /// Open a match from **text and plain values**: the rules table as the
    /// canonical JSON a file holds, and the lobby's own numbers.
    ///
    /// [`Host::open`] is unchanged and is what a caller that already has a
    /// [`RulesTable`] uses. This one exists so that a binary can host a match
    /// without naming a `pharmakos-sim` type at all — see [`Settings`] and
    /// decisions-log item 107 (11). It is also what
    /// [`crate::serve::run`] uses itself, which is what keeps the two paths
    /// from drifting: the host loop takes the same door a caller does.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a rules table this build
    /// will not read and for a lobby setting [`Host::check_settings`] refuses,
    /// and [`crate::error::Code::Internal`] as [`Host::open`].
    pub fn open_from(
        rules_json: &str,
        seed: u64,
        seats: u32,
        settings: &Settings,
        library: Option<PathBuf>,
    ) -> Result<Host, Error> {
        Host::check_seats(seats)?;
        let rules = RulesTable::from_canonical_json(rules_json).map_err(|error| {
            Error::invalid(format!(
                "this is not a rules table this build reads: {error}"
            ))
        })?;
        let config = WorldConfig {
            match_seed: seed,
            seats,
            units_per_seat: settings.units_per_seat,
            rules,
            match_settings: MatchSettings {
                segment_lengths_ms: settings.segment_lengths_ms.clone(),
                round_limit: settings.round_limit,
            },
        };
        Host::open(&config, library)
    }

    /// Refuse a seat count v1 does not play: none, or more than
    /// [`MAX_SEATS`].
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`], naming the count asked for.
    pub fn check_seats(seats: u32) -> Result<(), Error> {
        if (1..=MAX_SEATS).contains(&seats) {
            return Ok(());
        }
        Err(Error::invalid(format!(
            "a match has between 1 and {MAX_SEATS} seats, and this asks for {seats}"
        )))
    }

    /// Refuse a lobby setting the sim would silently correct.
    ///
    /// Item 102 (8). Three refusals, each naming what was asked for:
    ///
    /// * a **round limit of zero** — a match with no rounds is not a match, and
    ///   the sim floors it at one;
    /// * a **segment length at or below zero** — a Push that closes on its
    ///   first tick;
    /// * a **segment length under one tick** (50 ms) — the same thing said
    ///   less obviously, and the case the clamp exists for. A lobby that asks
    ///   for a 10 ms round gets told, rather than getting a 50 ms one.
    ///
    /// An **empty** length list is not degenerate and is not refused: item 40
    /// says an empty list means the rules table's own ladder.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`], always — there is a user at the
    /// lobby to tell.
    pub fn check_settings(settings: &MatchSettings) -> Result<(), Error> {
        if settings.round_limit == 0 {
            return Err(Error::invalid(
                "a match runs at least one round: the round limit is 0",
            ));
        }
        for (index, length) in settings.segment_lengths_ms.iter().enumerate() {
            if *length <= 0 {
                return Err(Error::invalid(format!(
                    "segment length {index} is {length} ms: a Push runs for a positive length of \
                     game time"
                )));
            }
            if *length < MS_PER_TICK {
                return Err(Error::invalid(format!(
                    "segment length {index} is {length} ms and a tick is {MS_PER_TICK} ms: a \
                     segment shorter than one tick is a segment nothing happens in"
                )));
            }
        }
        Ok(())
    }

    /// The runner, to read. Every read method goes through this and none of
    /// them may step it.
    #[must_use]
    pub const fn runner(&self) -> &Runner {
        &self.runner
    }

    /// The world the match is in, to read.
    #[must_use]
    pub const fn world(&self) -> &World {
        self.runner.world()
    }

    /// The rules table the match runs under.
    #[must_use]
    pub const fn rules(&self) -> &RulesTable {
        self.runner.world().rules()
    }

    /// The travel estimator's adapter (item 100 (1)).
    #[must_use]
    pub const fn routes(&self) -> &RouteAdapter {
        &self.routes
    }

    /// The template folder, when the host was given one.
    ///
    /// PLACEHOLDER: **where** that folder is on each platform is the owner's,
    /// with packaging at **T21** — the skeleton plan's own T13 PLACEHOLDER. The
    /// gateway takes the path it is handed and reads it; it never writes to it
    /// and never stores anything from it (spec section 13, "Local-first
    /// library").
    #[must_use]
    pub fn library(&self) -> Option<&Path> {
        self.library.as_deref()
    }

    /// The fallback safe playbook, as JSONC: what a seat with no advice of its
    /// own is shown and filed ([`SAFE_PLAYBOOK`] unless a host replaced it).
    #[must_use]
    pub fn safe_playbook(&self) -> &str {
        &self.safe_playbook
    }

    /// Replace the fallback safe playbook, for a host or a test that wants a
    /// different one. A seat's **own** safe playbook is not set here: it comes
    /// from the built-in operator's advice ([`crate::serve::Advisor`],
    /// decisions-log item 111), per seat and per round.
    pub fn set_safe_playbook(&mut self, playbook_jsonc: &str) {
        playbook_jsonc.clone_into(&mut self.safe_playbook);
    }

    /// Rebuild the estimator's graph from the world, because a Push moved the
    /// terrain.
    ///
    /// # Errors
    ///
    /// As [`RouteAdapter::refresh`].
    pub fn refresh_routes(&mut self) -> Result<(), Error> {
        let (voxels, rules) = {
            let world = self.runner.world();
            (world.voxels().clone(), world.rules().clone())
        };
        self.routes.refresh(&voxels, &rules)
    }

    /// Seal each seat's compiled plan into the match, in the order given.
    ///
    /// **The fourth of this crate's four ways into the runner**, and the one
    /// that gives the Push anything to play: before it existed the gateway
    /// sealed a submission in its own private store and the hosted world never
    /// heard of it, so every commander stood still (decisions-log item
    /// 103 (1)).
    ///
    /// It belongs here for the same reason the other three do: the runner is
    /// this module's, and a handler that could seal could rewrite a seat's
    /// orders from inside a read. `tests/confinement.rs` asserts that neither
    /// this method's name nor the runner's (`seal_playbook`) is written
    /// anywhere else — the review found that the second needle alone left the
    /// first spelling, which is the one a handler would actually reach for,
    /// unguarded.
    ///
    /// The caller orders the list — [`crate::surface::Surface::begin_push`]
    /// sorts it by seat id — and this preserves that order rather than choosing
    /// one of its own, because the order is the caller's claim to make and the
    /// events come out in it.
    ///
    /// # It is all of them or none of them
    ///
    /// Both of [`pharmakos_sim::runner::SealRefused`]'s reasons can be read off
    /// the runner without sealing anything, so they are, before the first seal
    /// goes in. A loop that sealed as it went would leave a refusal half
    /// applied — some seats holding this round's orders with their execution
    /// state reset and a `plan_sealed` on the bus, the rest holding last
    /// round's — and a host that retried would seal the first group twice. That
    /// is close to the "answer that looks like success" the runner's own doc
    /// refuses to give.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`], always. Every refusal
    /// [`pharmakos_sim::runner::Runner::seal_playbook`] can give — the match is
    /// not in a Lull, the seat is not a seat of this match — is a mistake by
    /// whoever drove the host, and nothing a caller of a gateway method did
    /// could have caused or avoided it.
    pub fn seal_plans(&mut self, plans: Vec<(SeatId, Plan)>) -> Result<(), Error> {
        let phase = self.runner.phase();
        if phase != MatchPhase::Lull && !plans.is_empty() {
            return Err(Error::internal(format!(
                "this match would take no sealed playbook: a playbook is sealed during a Lull and \
                 this match is in its {}",
                phase.name()
            )));
        }
        let in_world = self.runner.world().seats().len();
        for (seat, _) in &plans {
            if u32::from(seat.raw()) >= in_world {
                return Err(Error::internal(format!(
                    "this match would not take seat {}'s sealed playbook: this match has no seat \
                     {}",
                    seat.raw(),
                    seat.raw()
                )));
            }
        }
        for (seat, plan) in plans {
            self.runner.seal_playbook(seat, plan).map_err(|refused| {
                Error::internal(format!(
                    "this match would not take seat {}'s sealed playbook: {refused}",
                    seat.raw()
                ))
            })?;
        }
        Ok(())
    }

    /// Open the Push. Returns false when the runner was not in a Lull.
    ///
    /// **One of the three places this crate steps a match**, and the only one
    /// that starts one.
    pub fn begin_push(&mut self) -> bool {
        self.runner.begin_push()
    }

    /// One tick of the Push, or `None` outside one.
    ///
    /// **One of the three places this crate steps a match.**
    pub fn step(&mut self) -> Option<TickReport> {
        self.runner.step()
    }

    /// End the recap and open the next Lull. Returns false when the runner was
    /// not in a recap.
    ///
    /// **One of the three places this crate steps a match.**
    pub fn end_recap(&mut self) -> bool {
        self.runner.end_recap()
    }

    /// Drain the events the sim has produced since the last drain.
    ///
    /// Cloned out rather than borrowed, because the caller is about to write
    /// them onto the feed and clear the bus, and it holds `&mut Host` for both.
    pub fn drain_events(&mut self) -> Vec<pharmakos_sim::events::Event> {
        let drained: Vec<pharmakos_sim::events::Event> = self.runner.events().to_vec();
        self.runner.clear_events();
        drained
    }

    /// Resume a saved match: put the saved planning snapshot into this
    /// freshly opened one.
    ///
    /// **The one place a restore happens, and it is here for the reason the
    /// four driving calls are** (`tests/confinement.rs` holds `.restore(` and
    /// `.resume(` to this module and the surface's own resume path). The
    /// caller has just opened this host with [`Host::open_from`] from the
    /// save's own config line, which regenerates the pristine world -- the map
    /// is a pure function of seed, rules and seats, so the snapshot carries
    /// only what an edit changed -- and this restores the snapshot into it.
    ///
    /// # Where it lands
    ///
    /// A save holds the **frozen** planning snapshot (spec section 3: "from the
    /// frozen segment-end snapshot"), because that is what every verifier
    /// report is taken over. After round 1 it was frozen at the tick the
    /// segment ended, in the recap, so the recap is closed here exactly as the
    /// host closed it, and the match stands in the same Lull the save was made
    /// in. Round 1's snapshot was frozen in the opening Lull and needs nothing.
    ///
    /// # What it checks
    ///
    /// That the snapshot decodes and restores (the sim checks its version, and
    /// every modified chunk's digest against the store it rebuilds), that it
    /// lands in a Lull, and that the restored planning snapshot encodes to
    /// **the saved bytes exactly** -- the verifier hashes those bytes into
    /// every `report_hash`, so a resume whose planning snapshot moved by a
    /// byte would give every seat a report different from the one it was
    /// shown before the restart.
    ///
    /// The seats' orders are **not** sealed here. A plan is an input and is not
    /// in a snapshot; the surface restores every saved seal into the seats'
    /// private store and seals them from there at
    /// [`crate::surface::Surface::begin_push`], through
    /// [`Host::seal_plans`], which is the one path orders take into a match.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a snapshot that will not
    /// decode, restore or land in a Lull, or that does not re-encode to itself
    /// -- a damaged or foreign save -- and [`crate::error::Code::Internal`] for
    /// a host that is not freshly opened.
    pub fn resume(&mut self, snapshot_bytes: &[u8]) -> Result<(), Error> {
        let fresh = self.runner.phase() == MatchPhase::Lull
            && self.runner.round() == 1
            && self.runner.tick().raw() == 0;
        if !fresh {
            return Err(Error::internal(
                "a saved match resumes into a host that has just been opened, and this one has \
                 already been played",
            ));
        }
        let snapshot = Snapshot::from_bytes(snapshot_bytes).map_err(|error| {
            Error::invalid(format!(
                "this save's planning snapshot will not decode: {error}"
            ))
        })?;
        self.runner.restore(&snapshot).map_err(|error| {
            Error::invalid(format!(
                "this save's planning snapshot will not restore into the match its config line \
                 describes: {error}"
            ))
        })?;
        if self.runner.phase() == MatchPhase::Recap {
            let _ = self.runner.end_recap();
        }
        if self.runner.phase() != MatchPhase::Lull {
            return Err(Error::invalid(format!(
                "a save is made in a Lull, and this one's planning snapshot lands in the match's \
                 {}",
                self.runner.phase().name()
            )));
        }
        let again = self
            .runner
            .frozen()
            .snapshot()
            .to_bytes()
            .map_err(|error| {
                Error::internal(format!(
                    "the restored planning snapshot would not encode: {error}"
                ))
            })?;
        if again != snapshot_bytes {
            return Err(Error::invalid(
                "this save's planning snapshot does not restore to itself, so every report a \
                 seat was shown before the restart would change after it: the file is damaged, \
                 or it was written by a build whose snapshot differs",
            ));
        }
        // A resumed match's feed starts empty (the wave-6 notes, A2): the
        // restore's own events are the host's business, not a seat's.
        self.runner.clear_events();
        self.refresh_routes()
    }

    // -----------------------------------------------------------------------
    // The test seam: a host may file this tick's orders, and no wire method may
    // -----------------------------------------------------------------------

    /// File a voxel edit for this tick's voxel phase. `false` when the queue
    /// is full.
    ///
    /// **A host-side seam, reached by no wire method, and that is the whole of
    /// its design.** Three of T16a's acceptance lines are about what a seat
    /// may see of an edit and of an elimination, and on `main` nothing in a
    /// hosted match edits a voxel or kills a seat: combat's craters are S2's
    /// and construction's sets are T14's. `Runner::world_mut`'s own doc names
    /// exactly this — "what a host files this tick's orders through — a voxel
    /// edit, a damage order" — so the seam is that sentence and nothing more.
    ///
    /// `tests/confinement.rs` bans this name and
    /// [`Host::file_damage`] from every handler module and from
    /// `surface/control.rs`, and
    /// `no_wire_method_files_a_voxel_edit_or_a_damage_order` asserts that no
    /// dispatch arm reaches either. **`gamectl`'s scenario runner must not
    /// grow a dependency on them without the owner's word**: a scenario that
    /// edits voxels is a scenario-format question, which is T3's.
    pub fn file_voxel_edit(&mut self, edit: VoxelEdit) -> bool {
        self.runner.world_mut().request_voxel_edit(edit)
    }

    /// File a damage order for this tick's combat phase. `false` when the
    /// queue is full.
    ///
    /// As [`Host::file_voxel_edit`], and for the same reason: elimination is a
    /// consequence of something dying, and nothing on `main` kills anything in
    /// a hosted match.
    pub fn file_damage(&mut self, order: DamageOrder) -> bool {
        self.runner.world_mut().request_damage(order)
    }
}

/// The production [`Vision`]: a seat sees inside the spheres of its own living
/// beacons.
///
/// **A call to the sim's own rule**, [`World::in_own_sphere`], and no longer a
/// second copy of it (decisions-log items 107 (6) and 110 (5); T17). It holds
/// the owned [`Spheres`] value the sim builds for exactly this -- a host asks
/// "may this seat see that voxel" many times a call, from code that holds the
/// surface mutably, so it cannot hold a borrow of the world -- and asks it
/// through [`Spheres::contains`], which is the one place the rule is written
/// and which `World::in_own_sphere` asks too. The arithmetic is the
/// interpreter's `within`: integer squared distance in the sim's fixed point,
/// no square root (AGENTS.md section 4.2).
/// `the_view_uses_the_sims_own_sphere_rule` pins the boundary voxel through
/// both.
///
/// What it does **not** model, because the owner's answer keeps it out
/// (decisions-log item 108 (1): enemy units, beacons, structures and edits
/// "appear only inside own beacon spheres until the match-end unlock"): a
/// scout's own vision, Survey-lite's recorded sightings, a Sensor Spire's
/// reveal. A commander that walks out of its own spheres is still drawn to its
/// owner -- that is ownership, not sight -- and sees no enemy entity or edit
/// around it. PLACEHOLDER: **OWNER**, now, whether scouts join the live view
/// (the wave-6 notes, section D, question 4); a sight-radius row per unit and
/// structure at **S1** at the latest; Survey-lite's sightings at **S3**.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SphereVision {
    /// The sim's own snapshot of every living beacon's sphere, rebuilt before
    /// each call, so that nothing here can be read after the world has moved
    /// on.
    spheres: Spheres,
}

impl SphereVision {
    /// The spheres of every living beacon in `world`.
    #[must_use]
    pub fn of(world: &World) -> SphereVision {
        SphereVision {
            spheres: world.spheres(),
        }
    }

    /// How many living beacons the snapshot holds.
    #[must_use]
    pub fn spheres(&self) -> usize {
        self.spheres.len()
    }
}

impl Vision for SphereVision {
    fn sees(&self, seat: SeatId, at: &Voxel) -> bool {
        self.spheres.contains(seat, [at.x, at.y, at.z])
    }
}

#[cfg(test)]
mod tests {
    use super::{Host, SAFE_PLAYBOOK};
    use crate::error::Code;
    use pharmakos_sim::rules::RulesTable;
    use pharmakos_sim::runner::{MatchPhase, MatchSettings};
    use pharmakos_sim::tables::SeatId;
    use pharmakos_sim::world::WorldConfig;

    fn rules() -> RulesTable {
        RulesTable::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .expect("the shipped rules table")
    }

    fn config(settings: MatchSettings) -> WorldConfig {
        WorldConfig {
            match_seed: 0x0000_0000_ca5c_aded,
            seats: 2,
            units_per_seat: 0,
            rules: rules(),
            match_settings: settings,
        }
    }

    #[test]
    fn a_degenerate_lobby_setting_is_refused_here_rather_than_clamped() {
        // Item 102 (8): the sim clamps, because a restore has nobody to tell.
        // The lobby has somebody to tell.
        for (settings, expected) in [
            (
                MatchSettings {
                    segment_lengths_ms: vec![180_000],
                    round_limit: 0,
                },
                "round limit is 0",
            ),
            (
                MatchSettings {
                    segment_lengths_ms: vec![180_000, 0],
                    round_limit: 3,
                },
                "segment length 1 is 0 ms",
            ),
            (
                MatchSettings {
                    segment_lengths_ms: vec![-5],
                    round_limit: 3,
                },
                "segment length 0 is -5 ms",
            ),
            (
                MatchSettings {
                    segment_lengths_ms: vec![10],
                    round_limit: 3,
                },
                "shorter than one tick",
            ),
        ] {
            let error = Host::check_settings(&settings).expect_err("refused");
            assert_eq!(error.code, Code::InvalidArgument);
            assert!(error.message.contains(expected), "{}", error.message);
        }
    }

    #[test]
    fn an_empty_ladder_is_the_rules_tables_own_and_is_not_degenerate() {
        assert!(
            Host::check_settings(&MatchSettings {
                segment_lengths_ms: Vec::new(),
                round_limit: 3,
            })
            .is_ok(),
            "item 40: an empty list means the ladder"
        );
        assert!(Host::check_settings(&MatchSettings::default()).is_ok());
    }

    #[test]
    fn a_host_opens_in_a_lull_with_a_graph_and_the_safe_playbook() {
        let host = Host::open(
            &config(MatchSettings {
                segment_lengths_ms: vec![1_000],
                round_limit: 2,
            }),
            None,
        )
        .expect("a match");
        assert_eq!(host.runner().phase(), MatchPhase::Lull);
        assert_eq!(host.runner().round(), 1);
        assert!(host.routes().columns() > 0);
        assert_eq!(host.safe_playbook(), SAFE_PLAYBOOK);
        assert_eq!(host.library(), None);
    }

    /// The safe playbook is what a seat that sealed nothing plays, so "safe"
    /// has to mean runnable as well as harmless. It never goes through
    /// `submit_plan`'s door, so nothing else would catch a constant this crate
    /// broke — and a playbook that will not compile is a Push the seat spends
    /// standing still.
    #[test]
    fn the_safe_playbook_this_gateway_files_compiles() {
        let plan = crate::surface::planning::compile_playbook(SAFE_PLAYBOOK, &rules())
            .expect("the safe playbook is runnable");
        assert_eq!(plan.route().len(), 1, "one step, and it is stand still");
        assert_eq!(
            plan.max_deaths_before_fallback(),
            0,
            "and it names no death limit of its own"
        );
    }

    /// The fourth call, and the one T13 did not have. A match that is given no
    /// plans plays a Push in which nothing was ordered.
    #[test]
    fn a_plan_is_sealed_into_the_match_during_the_lull_and_refused_after_it() {
        let mut host = Host::open(
            &config(MatchSettings {
                segment_lengths_ms: vec![1_000],
                round_limit: 2,
            }),
            None,
        )
        .expect("a match");
        let plan = crate::surface::planning::compile_playbook(SAFE_PLAYBOOK, &rules())
            .expect("the safe playbook");
        host.seal_plans(vec![
            (SeatId::new(0), plan.clone()),
            (SeatId::new(1), plan.clone()),
        ])
        .expect("a Lull takes a seal");
        assert!(host.world().interpreter().plan(0).is_some());
        assert!(host.world().interpreter().plan(1).is_some());

        assert!(host.begin_push());
        let error = host
            .seal_plans(vec![(SeatId::new(0), plan.clone())])
            .expect_err("a Push does not");
        assert_eq!(
            error.code,
            Code::Internal,
            "a host that seals late is a bug"
        );
        assert!(error.message.contains("its push"), "{}", error.message);
    }

    /// A refusal is all of them or none of them.
    ///
    /// The review's finding: a loop that sealed as it went would leave seat 0
    /// holding this round's orders and seat 1 holding last round's, and a host
    /// that retried would seal seat 0 twice. Both of the runner's refusals are
    /// readable before anything is sealed, so both are read first.
    #[test]
    fn a_refused_list_seals_none_of_it() {
        let mut host = Host::open(
            &config(MatchSettings {
                segment_lengths_ms: vec![1_000],
                round_limit: 2,
            }),
            None,
        )
        .expect("a match");
        let plan = crate::surface::planning::compile_playbook(SAFE_PLAYBOOK, &rules())
            .expect("the safe playbook");
        // Seat 2 is not a seat of this two-seat match, and it is last in the
        // list -- so a loop would have sealed seats 0 and 1 before finding out.
        let error = host
            .seal_plans(vec![
                (SeatId::new(0), plan.clone()),
                (SeatId::new(1), plan.clone()),
                (SeatId::new(2), plan),
            ])
            .expect_err("a seat this match has not got");
        assert_eq!(error.code, Code::Internal);
        assert!(error.message.contains("no seat 2"), "{}", error.message);
        assert!(
            host.world().interpreter().plan(0).is_none(),
            "seat 0 came before the refusal in the list and is still unsealed"
        );
        assert!(host.world().interpreter().plan(1).is_none());
        assert!(
            host.drain_events()
                .iter()
                .all(|event| event.kind != pharmakos_sim::events::EventKind::PlanSealed),
            "and no seat was told it had sealed"
        );
    }

    #[test]
    fn the_three_stepping_calls_are_the_only_way_a_match_moves() {
        let mut host = Host::open(
            &config(MatchSettings {
                segment_lengths_ms: vec![1_000],
                round_limit: 2,
            }),
            None,
        )
        .expect("a match");
        assert!(!host.drain_events().is_empty(), "the match opened");
        assert!(host.drain_events().is_empty(), "and the bus was drained");

        assert!(host.begin_push());
        assert_eq!(host.runner().phase(), MatchPhase::Push);
        let mut ticks = 0_u32;
        while let Some(report) = host.step() {
            ticks = ticks.saturating_add(1);
            if report.segment_ended {
                break;
            }
        }
        assert_eq!(ticks, 20, "1 000 ms at 50 ms a tick");
        assert_eq!(host.runner().phase(), MatchPhase::Recap);
        assert!(host.end_recap());
        assert_eq!(host.runner().phase(), MatchPhase::Lull);
        assert_eq!(host.runner().round(), 2);
        host.refresh_routes().expect("the graph rebuilds");
    }
}
