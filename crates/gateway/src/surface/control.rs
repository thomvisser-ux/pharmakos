// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Match control: the four `admin` methods the local lobby drives a match
//! with.
//!
//! # This module changes the confinement rule, and says so
//!
//! T9's rule was "no handler steps the match". T13 changed the fact rather
//! than the rule -- the gateway hosts the match now -- and kept the rule by
//! putting every stepping call in `src/host.rs`. **T16a changes the rule
//! itself** (decisions-log item 107 (4)):
//!
//! > No handler may step or seal, **except** the admin-scoped control handlers
//! > in `surface/control.rs`, which may drive only the live match through the
//! > surface's three driving methods, and may name none of `Runner`, `Host`,
//! > `World`, `host_mut`, `seal_plans`, `seal_playbook`, `snapshot`,
//! > `.clone()`, `file_voxel_edit` or `file_damage`.
//!
//! `tests/confinement.rs` is rewritten to exactly that sentence, and
//! `every_control_handler_needs_the_admin_scope_in_the_schema` reads the scope
//! off the descriptor set rather than trusting this file. The rule had to
//! change because something has to move the match and the gateway reads no
//! clock: before this lane nothing outside a test called `Host::step` at all
//! (decisions-log item 106 (1)), so the Push had no owner.
//!
//! What has **not** changed is the reason the rule exists. A read method still
//! may not step: "no dry runs" is about answering *what would happen* by
//! making it happen (AGENTS.md section 3 rule 2), and none of the four below
//! answers a question. They drive the one live match forward, they need the
//! `admin` scope to do it, and every call is rate-limited, phase-checked and
//! audited like any other.
//!
//! # Game milliseconds, and no clock
//!
//! No tick and no state hash reaches the wire. [`Surface::serve_advance_push`]
//! takes game milliseconds, floors them to whole ticks *inside* the gateway,
//! stops at segment end and answers what it actually ran; the client carries
//! the remainder and never learns how long a tick is. Segment end and match
//! end are read from the `_status` footer every result already carries.
//!
//! [`Surface::serve_report_host_clock`] is the other half: the gateway's own
//! tick is the sim's, a Lull, a recap and an ended match spend none, and
//! without a reported clock every token would hold one tick's worth of calls
//! for the whole of each. The input is client-attested and therefore bounded
//! and refused rather than clamped -- see [`MAX_CLOCK_STEP_MS`].

use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::Ms;

use crate::error::Error;
use crate::rpc::Request;
use crate::surface::Surface;

/// The longest stretch of game time one `advance_push` may ask for.
///
/// PLACEHOLDER: 60 000 ms is a working number with a technical reason and no
/// measurement. **One thread owns the surface**, so a call that runs a
/// thousand ticks is a stretch in which no other connection is answered and no
/// other seat's editor gets a reply; sixty seconds of game time is three
/// seconds of wall clock at the skeleton's tick cost and is comfortably the
/// largest step a skip needs. **OWNER**, at hardening, with
/// `MAX_MESSAGE_BYTES`, `READ_TIMEOUT`, `VIEW_PAGE_BYTES` and the rate limits.
pub const MAX_ADVANCE_MS: i32 = 60_000;

/// The furthest one `report_host_clock` may carry the phase's clock forward.
///
/// PLACEHOLDER: 60 000 ms, and the reason is **not** the one above. This input
/// is attested by the client: the gateway cannot check it against a clock,
/// because it has none. A single unbounded report would therefore let the
/// local lobby hand every token an arbitrary number of refilled rate-limit
/// windows in one call. Bounding the step turns that into "a client that
/// stalled for five minutes reports in five steps", which costs the lobby five
/// calls and costs the limiter nothing. **OWNER**, at hardening, with the rate
/// limits: the honest statement is that outside a Push the limiter's clock is
/// whatever the machine's own user says it is, bounded per call.
pub const MAX_CLOCK_STEP_MS: i32 = 60_000;

impl Surface {
    /// `end_lull`: seal every seat's orders and open the Push.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::PhaseClosed`] outside a Lull, and as
    /// [`Surface::begin_push`].
    pub(super) fn serve_end_lull(&mut self) -> Result<Json, Error> {
        if self.time().phase != Phase::Lull {
            return Err(Error::phase_closed(
                "`end_lull` ends a Lull, and this match is not in one",
            ));
        }
        if !self.begin_push()? {
            return Err(Error::phase_closed("this match would not leave its Lull"));
        }
        Ok(Json::Object(Vec::new()))
    }

    /// `advance_push`: run the Push for a stretch of game time.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a missing `ms` or one
    /// outside 1 to [`MAX_ADVANCE_MS`] -- **refused, never clamped** -- and
    /// [`crate::error::Code::PhaseClosed`] outside a Push.
    pub(super) fn serve_advance_push(&mut self, request: &Request) -> Result<Json, Error> {
        let asked = request
            .integer_param("ms")?
            .ok_or_else(|| Error::invalid("`ms` is how much game time to run, in milliseconds"))?;
        if asked < 1 || asked > i64::from(MAX_ADVANCE_MS) {
            return Err(Error::invalid(format!(
                "`ms` is between 1 and {MAX_ADVANCE_MS} game milliseconds and this is {asked}; a \
                 request out of range is refused rather than clamped, because a client told \
                 `fine` would read the answer as the time it asked for"
            )));
        }
        if self.time().phase != Phase::Push {
            return Err(Error::phase_closed(
                "`advance_push` runs a Push, and this match is not in one",
            ));
        }
        // Floored: a request too small to fill a tick runs nothing and answers
        // zero, which is a normal answer and not an error. The client carries
        // `ms - advanced_ms` and never learns how long a tick is.
        let ticks = Ms::new(i32::try_from(asked).unwrap_or(MAX_ADVANCE_MS)).to_ticks_floor();
        let mut ran: u32 = 0;
        for _ in 0..ticks {
            // `None` is the segment ending: the match left the Push under us,
            // and the footer this result carries says so.
            if self.step()?.is_none() {
                break;
            }
            ran = ran.saturating_add(1);
        }
        Ok(Json::Object(vec![(
            String::from("advanced_ms"),
            Json::Number(Ms::from_ticks(ran).raw().to_string()),
        )]))
    }

    /// `end_recap`: close the recap and open the next Lull.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::PhaseClosed`] outside a recap, and as
    /// [`Surface::end_recap`].
    pub(super) fn serve_end_recap(&mut self) -> Result<Json, Error> {
        if self.time().phase != Phase::Recap {
            return Err(Error::phase_closed(
                "`end_recap` ends a recap, and this match is not in one",
            ));
        }
        if !self.end_recap()? {
            return Err(Error::phase_closed("this match would not leave its recap"));
        }
        // A recap that ended the last round leaves the match ENDED rather than
        // in a Lull, and there is then no draft to carry forward.
        if self.time().phase == Phase::Lull {
            self.open_lull()?;
        }
        Ok(Json::Object(Vec::new()))
    }

    /// `report_host_clock`: the host's own clock, reported to a gateway that
    /// has none.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::PhaseClosed`] during a Push and in the lobby, and
    /// [`crate::error::Code::InvalidArgument`] for a clock that runs backwards
    /// within a phase, jumps further than [`MAX_CLOCK_STEP_MS`], is negative,
    /// or reports a countdown in a phase that has none -- all **refused, never
    /// clamped**.
    pub(super) fn serve_report_host_clock(&mut self, request: &Request) -> Result<Json, Error> {
        let phase = self.time().phase;
        if !matches!(phase, Phase::Lull | Phase::Recap | Phase::Ended) {
            return Err(Error::phase_closed(
                "`report_host_clock` is for a phase that spends no game time: during a Push the \
                 sim's own tick is the clock, and in the lobby there is no match",
            ));
        }
        let elapsed = request.integer_param("elapsed_ms")?.ok_or_else(|| {
            Error::invalid("`elapsed_ms` is host time spent in this phase, in milliseconds")
        })?;
        let remaining = request.integer_param("remaining_ms")?.unwrap_or(0);
        let elapsed = i32::try_from(elapsed).map_err(|_| {
            Error::invalid(format!(
                "`elapsed_ms` is game milliseconds and this is {elapsed}"
            ))
        })?;
        let remaining = i32::try_from(remaining).map_err(|_| {
            Error::invalid(format!(
                "`remaining_ms` is game milliseconds and this is {remaining}"
            ))
        })?;
        if remaining != 0 && phase != Phase::Lull {
            return Err(Error::invalid(
                "`remaining_ms` is what the status footer shows as left in a Lull, and no other \
                 phase has a declared length to count down",
            ));
        }
        self.set_host_clock(Ms::new(elapsed), Ms::new(remaining))?;
        Ok(Json::Object(vec![(
            String::from("all_ready"),
            Json::Bool(self.all_ready()),
        )]))
    }
}
