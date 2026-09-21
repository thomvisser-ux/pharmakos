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

use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::math::quantity::MS_PER_TICK;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{MatchPhase, MatchSettings, Runner, TickReport};
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::{World, WorldConfig};

use crate::error::Error;
use crate::routes::RouteAdapter;

/// The safe playbook the gateway hands back until the built-in operator
/// exists.
///
/// Spec section 14 makes this the operator's: "it files the safe playbook when
/// a seat submits nothing verified before the Lull ends", and item 81 ships it
/// as one of the skeleton's three templates precisely so the editor can render
/// "the cost of a timeout". The operator is **T18**, and `get_safe_plan` is a
/// method the skeleton's clients call *now*, so the gateway ships the one
/// playbook that needs no situation to be safe: hold where you are, and fall
/// back to the safest beacon you own.
///
/// It is JSONC, comments and all, because that is what every other playbook a
/// client receives is and because the "why" note is the teaching half
/// (spec section 13).
///
/// PLACEHOLDER: the real safe playbook is the built-in operator's, generated
/// against the seat's own snapshot. **T18** replaces this constant with a call
/// to it; the method, its scope, its shape and its tests do not move when that
/// happens. `always_qualifies` keeps this one honest in the meantime.
pub const SAFE_PLAYBOOK: &str = concat!(
    "// The safe playbook: what is filed for you if the Lull ends with nothing\n",
    "// sealed. It spends nothing and risks nothing.\n",
    "{\n",
    "  \"schema_version\": {\"major\": 1},\n",
    "  \"meta\": {\n",
    "    \"title\": \"Safe playbook\",\n",
    "    \"author_kind\": \"BUILTIN\",\n",
    "    \"note\": \"Hold position for the segment, then fall back to the safest beacon you own.\"\n",
    "  },\n",
    "  \"declarative\": {\n",
    "    \"route\": [\n",
    "      // One step, and it is the whole plan: stand still.\n",
    "      {\"label\": \"hold\", \"hold\": {\"ms\": 1000}}\n",
    "    ]\n",
    "  },\n",
    "  \"on_death\": {\"on_respawn\": \"CONTINUE\"},\n",
    "  \"fallback\": {\"hold\": {\"at\": {\"beacon_anchor\": {\"safest\": {}}}}},\n",
    "  \"kind\": \"PLAYBOOK\"\n",
    "}\n",
);

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

    /// The safe playbook, as JSONC.
    #[must_use]
    pub fn safe_playbook(&self) -> &str {
        &self.safe_playbook
    }

    /// Replace the safe playbook, which is what **T18** does once the built-in
    /// operator can generate one.
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
