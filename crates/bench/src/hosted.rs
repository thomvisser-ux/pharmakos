// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A bench match, hosted exactly as `gamectl scenario run` hosts one: the
//! gateway's [`Host`] behind a [`Surface`], every seat's playbook arriving
//! through `submit_plan` with that seat's own token and sealed by
//! [`Surface::begin_push`].
//!
//! P1's decision tick ([`crate::decision`]) and G3′-real ([`crate::g3`]) both
//! open their matches here, so the two figures are taken on the same door.
//! Nothing here seals around the verifier: a seat whose submission is refused,
//! or that is not exactly on the size budget, stops the measurement with the
//! reason.
//!
//! The one host-side seam it adds to that door is the lobby layout
//! ([`Host::seed_layout`], S2's plan, E30), placed before the Lull opens, so
//! the layout is an input of the match like its seed.

use pharmakos_gateway::fog::{Blind, FogPolicy};
use pharmakos_gateway::host::Host;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{Layout, MatchSettings};
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::{World, WorldConfig};

use crate::error::Error;

/// The match id every bench match is hosted under. Lower-case letters and
/// hyphens only, as `MatchCache::valid_match_id` asks; it reaches nothing
/// hashed.
const MATCH_ID: &str = "m-bench";

/// How a bench match is opened.
#[derive(Clone, Copy, Debug)]
pub struct Opening<'a> {
    /// The map seed.
    pub seed: u64,
    /// The seats, each sealing `playbook`.
    pub seats: u32,
    /// The one segment's length, in game milliseconds.
    pub segment_ms: i32,
    /// The rules table the match is played under.
    pub rules: &'a RulesTable,
    /// The playbook every seat submits, as canonical JSON.
    pub playbook: &'a str,
}

/// A bench match in its Push, and the size units each seat sealed.
#[derive(Debug)]
pub struct Opened {
    /// The match, its Push open.
    pub surface: Surface,
    /// The size units each seat's sealed playbook holds: the size budget.
    pub units_per_seat: u32,
}

/// The seats `0..seats`, as ids.
///
/// # Errors
///
/// [`Error::Overflow`] for a count past a seat id's range.
pub fn seat_ids(seats: u32) -> Result<Vec<SeatId>, Error> {
    (0..seats)
        .map(|seat| u8::try_from(seat).map(SeatId::new))
        .collect::<Result<_, _>>()
        .map_err(|_| Error::Overflow("the seat count"))
}

/// Open the host alone, in its lobby, so a caller can read the generated world
/// and lay it out before [`attach`] hands it to a surface.
///
/// # Errors
///
/// [`Error::Gateway`] when the host refuses the match.
pub fn open_host(opening: &Opening<'_>) -> Result<Host, Error> {
    Host::open(
        &WorldConfig {
            match_seed: opening.seed,
            seats: opening.seats,
            // The harness walkers stay at 0, as every committed chain has them;
            // the gate's load is the layout (S2's plan, E30 and decision 23).
            units_per_seat: 0,
            rules: opening.rules.clone(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![opening.segment_ms],
                round_limit: 1,
            },
        },
        None,
    )
    .map_err(|error| Error::gateway("opening the bench match", &error))
}

/// Lay `host` out (when a layout is given), put it behind a surface, open the
/// Lull, submit `opening.playbook` for every seat and open the Push.
///
/// # Errors
///
/// [`Error::Gateway`] for a refused step of the match's opening, and
/// [`Error::Submission`] for a refused or off-budget playbook.
pub fn attach(
    mut host: Host,
    layout: Option<&Layout>,
    opening: &Opening<'_>,
) -> Result<Opened, Error> {
    if let Some(layout) = layout {
        host.seed_layout(layout)
            .map_err(|error| Error::gateway("placing the gate's layout", &error))?;
    }
    let seats = seat_ids(opening.seats)?;
    let mut surface = Surface::new(
        MATCH_ID,
        opening.seed,
        opening.rules.clone(),
        FogPolicy::fogged(),
        &seats,
    )
    .map_err(|error| Error::gateway("building the surface", &error))?;
    surface
        .attach(host)
        .map_err(|error| Error::gateway("attaching the host", &error))?;
    let lull = opening
        .rules
        .message()
        .r#match
        .as_ref()
        .map(|block| block.lull_ms)
        .ok_or_else(|| {
            Error::Rules("no `match` block, so nothing says how long a Lull is".to_owned())
        })?;
    surface.set_phase_remaining_ms(Ms::new(lull));
    surface
        .open_lull()
        .map_err(|error| Error::gateway("opening the Lull", &error))?;

    let mut units_per_seat = None;
    for seat in &seats {
        let token = mint(&mut surface, *seat)?;
        let units = submit(&mut surface, &token, *seat, opening.playbook)?;
        if units_per_seat.is_some_and(|kept| kept != units) {
            return Err(Error::Submission(format!(
                "seat {} sealed {units} size units and an earlier seat a different count, from \
                 the same playbook",
                seat.raw()
            )));
        }
        units_per_seat = Some(units);
    }
    let units_per_seat = units_per_seat
        .ok_or_else(|| Error::Empty("a bench match of no seats seals nothing".to_owned()))?;
    if !surface
        .begin_push()
        .map_err(|error| Error::gateway("opening the Push", &error))?
    {
        return Err(Error::Gateway {
            during: "opening the Push",
            message: "the match was not in its Lull".to_owned(),
        });
    }
    Ok(Opened {
        surface,
        units_per_seat,
    })
}

/// The hosted world, read only.
///
/// # Errors
///
/// [`Error::Gateway`] when the surface hosts no match.
pub fn world(surface: &Surface) -> Result<&World, Error> {
    surface
        .host()
        .map(Host::world)
        .map_err(|error| Error::gateway("reading the hosted world", &error))
}

fn mint(surface: &mut Surface, seat: SeatId) -> Result<Token, Error> {
    let scopes = ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit]);
    surface
        .tokens()
        .mint(Subject::Seat(seat), MATCH_ID, scopes, Tick::ZERO)
        .map(|(token, _)| token)
        .map_err(|error| Error::gateway("minting a seat token", &error))
}

/// Submit through the door every player's playbook takes, and answer the
/// size units the report says were sealed. Refused, or off the budget, is an
/// error naming why.
fn submit(
    surface: &mut Surface,
    token: &Token,
    seat: SeatId,
    playbook: &str,
) -> Result<u32, Error> {
    let params = json::write(&Json::Object(vec![(
        String::from("playbook_jsonc"),
        Json::String(playbook.to_owned()),
    )]));
    let request = pharmakos_gateway::rpc::parse(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"submit_plan","params":{params}}}"#
    ))
    .map_err(|error| Error::Codec {
        what: "the bench's submit_plan request".to_owned(),
        reason: error.title().to_string(),
    })?;
    let answer = surface.call(Some(token), &request, &Blind);
    let result = answer.get("result");
    if result.and_then(|result| result.get("accepted")) != Some(&Json::Bool(true)) {
        return Err(Error::Submission(format!(
            "seat {} could not submit the budget playbook: {}",
            seat.raw(),
            json::write(&answer)
        )));
    }
    let report = result.and_then(|result| result.get("report"));
    let units = number(report.and_then(|report| report.get("size_units")));
    let budget = number(report.and_then(|report| report.get("size_budget")));
    match (units, budget) {
        (Some(units), Some(budget)) if units == budget => Ok(units),
        _ => Err(Error::Submission(format!(
            "seat {}'s sealed playbook is not exactly on the size budget ({units:?} of \
             {budget:?} units), so a cost per unit at the budget cannot be read from it",
            seat.raw()
        ))),
    }
}

/// A JSON number as a `u32`, or `None` for anything else: the caller names
/// what it expected.
fn number(value: Option<&Json>) -> Option<u32> {
    match value {
        Some(Json::Number(lexeme)) => lexeme.parse().ok(),
        _ => None,
    }
}
