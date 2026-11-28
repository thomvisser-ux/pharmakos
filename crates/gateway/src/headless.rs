// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The headless host's driver: the three boundaries a match with no admin
//! token crosses (decisions-log item 231).
//!
//! A socket host never moves its match: the client does, through the
//! admin-scoped methods of `surface/control.rs`. The external seat's host
//! ([`crate::serve::External`]) mints no admin token, on purpose, so nothing a
//! program outside can send moves the match; the host moves it itself, and
//! this module is where. It is the second and last place outside
//! [`crate::host`] and [`crate::surface`] that may name a driving method, held
//! to the same terms as the first by `tests/confinement.rs`
//! (`the_match_is_stepped_in_one_module`): the surface's three driving
//! methods and nothing else of the stepping list, and no way to reach inside
//! the match. Its three functions are called from `serve.rs`'s external host
//! and from nowhere else (`the_headless_driver_is_the_external_hosts_alone`),
//! so no method handler can move a match by a name the stepping list does not
//! know.
//!
//! Each crossing is refused, with a typed error and nothing half done, when
//! the match is not where the crossing starts, and each leaves its own line in
//! the audit log under no subject: the log then says that the host, and no
//! seat, moved the match.

use crate::audit::Outcome;
use crate::error::Error;
use crate::surface::Surface;
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_sim::tables::SeatId;

/// Leave the Lull: seal every seat's orders and begin the Push, **only if
/// `seat` has said it is ready**.
///
/// Answers `false`, having touched nothing, when the seat has not: the seat
/// played from outside is the one that says a headless Lull may end, and this
/// function is where that is held, not its caller.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the match is not in a Lull or will
/// not leave it, and the surface's own refusal of the seal.
pub(crate) fn leave_lull(surface: &mut Surface, seat: SeatId) -> Result<bool, Error> {
    if surface.time().phase != Phase::Lull {
        return Err(Error::internal(
            "the host would leave a Lull the match is not in",
        ));
    }
    if !surface.seat_is_ready(seat) {
        return Ok(false);
    }
    host_line(surface, "end lull");
    if !surface.begin_push()? {
        return Err(Error::internal(
            "the outside seat is ready and the match would not leave its Lull",
        ));
    }
    Ok(true)
}

/// Play the Push to its segment's end, with nobody in control.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the match is not in a Push or the
/// Push will not reach its segment's end, and the sim's own typed refusal of
/// a tick.
pub(crate) fn play_push(surface: &mut Surface) -> Result<(), Error> {
    if surface.time().phase != Phase::Push {
        return Err(Error::internal(
            "the host would play a Push the match is not in",
        ));
    }
    while let Some(report) = surface.step()? {
        if report.segment_ended {
            break;
        }
    }
    if surface.time().phase == Phase::Push {
        return Err(Error::internal(
            "the Push would not step to its segment's end",
        ));
    }
    Ok(())
}

/// Leave the recap: open the next Lull, or end the match.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the match is not in a recap or will
/// not leave it, and the surface's own refusal of the next Lull.
pub(crate) fn leave_recap(surface: &mut Surface) -> Result<(), Error> {
    if surface.time().phase != Phase::Recap {
        return Err(Error::internal(
            "the host would leave a recap the match is not in",
        ));
    }
    host_line(surface, "end recap");
    if !surface.end_recap()? {
        return Err(Error::internal("the match would not leave its recap"));
    }
    if surface.time().phase == Phase::Lull {
        surface.open_lull()?;
    }
    Ok(())
}

/// One audit line for a crossing the host makes itself, under no subject.
fn host_line(surface: &mut Surface, action: &'static str) {
    let tick = surface.time().tick;
    surface
        .audit()
        .record(tick, None, None, action, Outcome::Ok);
}
