// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The audit log: what was asked, by whom, at which tick, and what the answer
//! was.
//!
//! Spec section 12 lists it in the security paragraph and AGENTS.md section 7
//! says it is "part of the feature, not a later hardening task". It is therefore
//! written at T9, with the surface, rather than bolted on when something goes
//! wrong.
//!
//! # What an entry is, and what it is deliberately not
//!
//! One line per attempt: a sequence number, the tick, the subject, the token's
//! handle, what was asked, and the outcome. It is an **access** log, not a data
//! log:
//!
//! * no token ever reaches it -- a token is named by its [`Handle`], which is a
//!   counter and not a fingerprint of the secret;
//! * no params and no results reach it. A playbook, a draft, a notes box and a
//!   seat's knowledge are exactly the things spec section 12 says never leave
//!   the gateway for another seat, and a log file is another seat's if anyone
//!   can read it -- which, on a local host, anyone can (see [`crate::cache`]);
//! * no wall-clock stamp. The tick is the clock (AGENTS.md section 4.5), and the
//!   sequence number orders entries inside a tick.
//!
//! # The format
//!
//! Tab-separated, one record per line, LF endings -- the project's golden
//! convention (`tests/golden/README.md` rule 3), because the log is compared
//! byte for byte by `tests/golden/gateway/audit_log/`.
//!
//! ```text
//! seq     tick    subject     handle  action              outcome
//! 1       0       -           -       upgrade             ok
//! 2       0       seat.0      t1      call get_status     ok
//! 3       2       seat.1      t2      call save_notes     FORBIDDEN_SCOPE
//! ```

use crate::error::{Code, Error};
use crate::token::{Handle, Subject};
use pharmakos_sim::math::quantity::Tick;

/// The most entries the in-memory log holds before it starts dropping the
/// oldest.
///
/// A bound rather than a policy: the host flushes to
/// [`crate::cache::MatchCache`] as it goes, and this cap is only there so a
/// gateway nobody is flushing cannot grow without end. [`AuditLog::dropped`]
/// counts what fell off, so a reader is never quietly short of entries.
pub const MAX_RETAINED: usize = 4096;

/// What happened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// The call was made and answered.
    Ok,
    /// It was refused with a code from the closed set.
    Refused(Code),
}

impl Outcome {
    /// The rendering: `ok`, or the code's name.
    #[must_use]
    pub fn render(self) -> String {
        match self {
            Outcome::Ok => String::from("ok"),
            Outcome::Refused(code) => String::from(code.as_str_name()),
        }
    }
}

/// One line of the log.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// Order within the log. Starts at 1, never reused, and orders entries that
    /// share a tick.
    pub seq: u64,
    /// The host's tick when the attempt was made.
    pub tick: Tick,
    /// Who asked, or `-` when the attempt never authenticated.
    pub subject: Option<Subject>,
    /// Which token, or `-` for the same reason.
    pub handle: Option<Handle>,
    /// What was asked: `upgrade`, `call get_status`, `mint seat.0`, `revoke t2`.
    pub action: String,
    /// What the gateway answered.
    pub outcome: Outcome,
}

impl Entry {
    /// The entry as one tab-separated line, without its newline.
    #[must_use]
    pub fn render(&self) -> String {
        let subject = self
            .subject
            .map_or_else(|| String::from("-"), Subject::render);
        let handle = self
            .handle
            .map_or_else(|| String::from("-"), |handle| handle.to_string());
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.seq,
            self.tick.raw(),
            subject,
            handle,
            self.action,
            self.outcome.render()
        )
    }
}

/// The log itself.
#[derive(Debug, Default)]
pub struct AuditLog {
    entries: Vec<Entry>,
    next_seq: u64,
    dropped: u64,
}

impl AuditLog {
    /// An empty log.
    #[must_use]
    pub const fn new() -> AuditLog {
        AuditLog {
            entries: Vec::new(),
            next_seq: 1,
            dropped: 0,
        }
    }

    /// Record an attempt. Returns the sequence number it was given.
    pub fn record(
        &mut self,
        tick: Tick,
        subject: Option<Subject>,
        handle: Option<Handle>,
        action: impl Into<String>,
        outcome: Outcome,
    ) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(Entry {
            seq,
            tick,
            subject,
            handle,
            action: action.into(),
            outcome,
        });
        while self.entries.len() > MAX_RETAINED {
            self.entries.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
        seq
    }

    /// Record a refusal, taking the code from the error.
    pub fn refused(
        &mut self,
        tick: Tick,
        subject: Option<Subject>,
        handle: Option<Handle>,
        action: impl Into<String>,
        error: &Error,
    ) -> u64 {
        self.record(tick, subject, handle, action, Outcome::Refused(error.code))
    }

    /// Everything retained, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// How many entries fell off the front of the log.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// The retained log as text: one record per line, LF endings, a trailing
    /// newline. The header is part of the format, so a file found on disk in a
    /// year says what its columns are.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::from("seq\ttick\tsubject\thandle\taction\toutcome\n");
        for entry in &self.entries {
            out.push_str(&entry.render());
            out.push('\n');
        }
        out
    }

    /// The retained lines without the header, for appending to a file that
    /// already has one.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.entries.iter().map(Entry::render).collect()
    }

    /// Forget the retained entries, keeping the sequence number.
    ///
    /// Called by a host that has just flushed them to disk: the log is a buffer
    /// in front of [`crate::cache`], and a sequence number that restarted would
    /// make the file unreadable.
    pub fn take(&mut self) -> Vec<Entry> {
        std::mem::take(&mut self.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::{AuditLog, MAX_RETAINED, Outcome};
    use crate::error::{Code, Error};
    use crate::token::{Handle, Subject};
    use pharmakos_sim::math::quantity::Tick;
    use pharmakos_sim::tables::SeatId;

    #[test]
    fn an_entry_names_the_tick_the_subject_and_the_outcome() {
        let mut log = AuditLog::new();
        log.record(Tick::ZERO, None, None, "upgrade", Outcome::Ok);
        log.refused(
            Tick::new(2),
            Some(Subject::Seat(SeatId::new(1))),
            None,
            "call save_notes",
            &Error::forbidden("no plan scope"),
        );
        let text = log.render();
        assert!(text.starts_with("seq\ttick\tsubject\thandle\taction\toutcome\n"));
        assert!(text.contains("1\t0\t-\t-\tupgrade\tok\n"), "{text}");
        assert!(
            text.contains("2\t2\tseat.1\t-\tcall save_notes\tFORBIDDEN_SCOPE\n"),
            "{text}"
        );
        assert!(text.ends_with('\n'));
        assert!(
            !text.contains('\r'),
            "LF endings, tests/golden/README.md rule 3"
        );
    }

    #[test]
    fn sequence_numbers_order_entries_that_share_a_tick() {
        let mut log = AuditLog::new();
        for _ in 0..3 {
            log.record(Tick::new(7), None, None, "call get_status", Outcome::Ok);
        }
        let seqs: Vec<u64> = log.entries().iter().map(|entry| entry.seq).collect();
        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[test]
    fn a_flushed_log_keeps_counting_where_it_left_off() {
        let mut log = AuditLog::new();
        log.record(Tick::ZERO, None, None, "upgrade", Outcome::Ok);
        assert_eq!(log.take().len(), 1);
        let seq = log.record(Tick::ZERO, None, None, "upgrade", Outcome::Ok);
        assert_eq!(
            seq, 2,
            "a restarted sequence would make the file unreadable"
        );
    }

    #[test]
    fn the_log_is_bounded_and_says_what_it_dropped() {
        let mut log = AuditLog::new();
        for _ in 0..MAX_RETAINED.saturating_add(5) {
            log.record(Tick::ZERO, None, None, "call get_status", Outcome::Ok);
        }
        assert_eq!(log.entries().len(), MAX_RETAINED);
        assert_eq!(log.dropped(), 5);
    }

    #[test]
    fn a_handle_is_named_and_a_token_is_not() {
        let mut log = AuditLog::new();
        log.record(
            Tick::ZERO,
            Some(Subject::Spectator),
            Some(Handle::from_raw(4)),
            "call get_segment_feed",
            Outcome::Refused(Code::StaleSnapshot),
        );
        let line = log.lines().pop().expect("one line");
        assert_eq!(
            line,
            "1\t0\tspectator\tt4\tcall get_segment_feed\tSTALE_SNAPSHOT"
        );
    }
}
