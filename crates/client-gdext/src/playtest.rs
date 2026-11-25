// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playtest mode: the fun gate's local, opt-in record and the bundle a tester sends by hand
//! (S3's `prob`, part 2; the plan's E10, decisions 1 and 11, R6; AGENTS.md section 7, "No network
//! telemetry").
//!
//! # What it is
//!
//! A [`Recorder`] the bridge keeps for the whole launch. It is **off at every launch** and holds
//! nothing while off; switching it off drops everything it held. While on it keeps, **in memory
//! only**:
//! - per Push, the wall time the window had focus and the time it did not, or that the Push was
//!   skipped (a skipped Push carries no times); the count of Pushes and of skipped ones;
//! - per Lull, the wall time used: from its opening to this seat's Ready, or to its end;
//! - the speed changes by speed, the camera moves by kind, the verifier's error rows shown by
//!   code (each code once per text revision it was shown in), the gateway's refusals by code;
//! - the client's own diagnostics log (below);
//! - the tester's survey answers.
//!
//! Nothing is written anywhere until the tester presses Export and picks a file
//! ([`Recorder::export`]): then one zip, built here, is written whole to that path by the client's
//! one atomic writer ([`crate::write::write_whole`]), and nowhere else.
//!
//! # What it never holds (R6), by construction
//!
//! Its inputs are an allow-list of typed facts: phases ([`Phase`]), rounds, the served preset
//! ([`ServedPreset`]), speeds, focus, camera-move kinds ([`CameraMove`]), wall-clock readings, and
//! codes from closed sets ([`Code`]: the gateway's error codes, the verifier's code shape, the
//! write failures' names), anything else recorded as `UNNAMED`. The two readers that take a
//! rig's answer and the editor's rows ([`Recorder::answered`], [`Recorder::shown`]) read only
//! those facts from them. No function here takes a playbook, a draft, the notebook, a label, a
//! JSON pointer, a message, a path to record, a match id, a seed, a token or a port, so none can
//! reach the bundle. The one free text is the survey's third answer, the tester's own words,
//! which goes into `survey.json` as written and which the manifest and the survey screen say is
//! sent as written.
//!
//! The gateway's refusal message is dropped and only its code is kept, because a refusal can
//! quote a player's text (`MAX_QUOTED_CHARS`). The raw `godot.log` and `client-<pid>.log` are never
//! read: they hold absolute paths and with them the user name.
//!
//! # The diagnostics log
//!
//! One line per occurrence, in a closed grammar:
//! `<seq> <phase> <event>[ <code>] t=<ms>`, where `seq` counts every line asked for from 1
//! (a line dropped at the cap still takes its number, so a gap shows), `phase` is the phase
//! word the line was recorded in, `event` one of [`Event`]'s names, `code` a [`Code`], and `ms`
//! whole milliseconds of wall time since Playtest mode was switched on (the pacer's
//! [`PlaytestClock::stamp_ms`]).
//!
//! # Bounded
//!
//! Every list and string the bundle carries has a cap from a named constant below. Past a cap
//! the recorder counts what it could not keep under a [`DropReason`], and `manifest.json` lists
//! every reason with its count, zeros included, so nothing is cut silently. A full recorder's
//! bundle fits [`MAX_BUNDLE_BYTES`] (`tests/playtest_bundle.rs` fills every cap and measures it).
//!
//! # Thin client
//!
//! The recorder counts and keeps what the client already schedules or draws; it decides nothing,
//! asks the gateway nothing, and sends nothing to it. Its sums of durations are the pacer's
//! ([`PlaytestClock`]); this module adds no time arithmetic (`tests/no_arithmetic.rs`). No
//! threshold is written anywhere: the file records, the owner judges (decisions-log item
//! 153 (1)).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use pharmakos_proto::gp::api::v1::gateway_error;
use pharmakos_proto::json::{self, Json};

use crate::editor::{Editor, RowSeverity};
use crate::pacer::{ClockFault, LullTime, MAX_ATTENTION_STEP_US, PlaytestClock, PushTime, SPEEDS};
use crate::rig::{Answer, Phase, Rig, ServedPreset};
use crate::write::WriteFailure;

/// The bundle's format: `manifest.json`'s `bundle_format`. A reader of a later format may find
/// files, keys and reasons this one does not have; a key it does not know is to be skipped, and
/// a format it does not know is to be refused rather than read as this one.
pub const BUNDLE_FORMAT: u32 = 1;

/// The bundle's entries, in the zip's order, and no other.
pub const BUNDLE_ENTRIES: [&str; 4] = [
    "manifest.json",
    "survey.json",
    "metrics.json",
    "diagnostics.log",
];

/// The most Push records kept.
///
/// The caps below are guesses, held to one marker:
/// PLACEHOLDER: the Playtest bundle's caps (records, log lines, codes, survey text, bundle size) — OWNER, at S7
///
/// They are 512 Push and 512 Lull records, 4096 log lines, 256 codes a tally, 4000 characters of
/// survey text and a 1 MiB bundle, settled with the recording S7 brings.
pub const MAX_PUSH_RECORDS: usize = 512;
/// The most Lull records kept (the PLACEHOLDER on [`MAX_PUSH_RECORDS`]).
pub const MAX_LULL_RECORDS: usize = 512;
/// The most diagnostics lines kept (the PLACEHOLDER on [`MAX_PUSH_RECORDS`]).
pub const MAX_LOG_LINES: usize = 4096;
/// The most distinct codes a tally keeps, verifier rows and refusals each (the PLACEHOLDER on
/// [`MAX_PUSH_RECORDS`]).
pub const MAX_CODES: usize = 256;
/// The longest survey text taken, in characters; a longer one is refused, never cut (the
/// PLACEHOLDER on [`MAX_PUSH_RECORDS`]).
pub const MAX_SURVEY_TEXT_CHARS: usize = 4000;
/// The largest bundle: what a full recorder's zip fits, measured by `tests/playtest_bundle.rs`
/// (the PLACEHOLDER on [`MAX_PUSH_RECORDS`]).
pub const MAX_BUNDLE_BYTES: usize = 1_048_576;

/// A code the bundle may carry, from a closed set: a gateway refusal's code, one of
/// `gp.api.v1.GatewayError.Code`'s names (`PHASE_CLOSED`); a verifier code, a severity letter
/// and four digits (`E0104`); a write failure's name in capitals (`NOT_WRITABLE`); or a speed
/// (`SPEED_2`). Anything else is [`Code::UNNAMED`], so no other text can pass: not a word of a
/// message, a pointer, a label or a path, whatever shape it has.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Code(String);

impl Code {
    /// What a code that is not one is recorded as.
    pub const UNNAMED: &'static str = "UNNAMED";

    fn unnamed() -> Self {
        Self(Self::UNNAMED.to_owned())
    }

    /// The code of a refusal the rig said as `CODE: message` ([`crate::rig::Answer`]'s `error`):
    /// the text before the first `": "` when it is one of the gateway's error codes, else
    /// [`Code::UNNAMED`] (text not in that shape is not a refusal the rig said). The message is
    /// dropped here, because it can quote a player's text.
    #[must_use]
    pub fn of_refusal(said: &str) -> Self {
        let Some((code, _)) = said.split_once(": ") else {
            return Self::unnamed();
        };
        match gateway_error::Code::from_str_name(code) {
            Some(gateway_error::Code::ErrorCodeUnspecified) | None => Self::unnamed(),
            Some(named) => Self(named.as_str_name().to_owned()),
        }
    }

    /// A verifier code (`E0104`, `W0702`, `I0003`) as itself, else [`Code::UNNAMED`].
    #[must_use]
    pub fn of_verifier(code: &str) -> Self {
        let mut chars = code.chars();
        let letter = chars.next().is_some_and(|c| matches!(c, 'E' | 'W' | 'I'));
        let digits: Vec<char> = chars.collect();
        if letter && digits.len() == 4 && digits.iter().all(char::is_ascii_digit) {
            Self(code.to_owned())
        } else {
            Self::unnamed()
        }
    }

    /// A write failure's name, in capitals.
    fn of_failure(failure: &WriteFailure) -> Self {
        Self(failure.name().to_ascii_uppercase())
    }

    /// A speed of the pacer's set.
    fn of_speed(speed: u32) -> Self {
        Self(format!("SPEED_{speed}"))
    }

    /// The code's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A camera move the tester made, by kind (`camera_rig.gd`'s `moved` signal).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMove {
    /// A free-look key pressed down.
    Pan,
    /// A wheel notch.
    Zoom,
    /// A turn with the right mouse button held.
    Turn,
    /// Follow switched on or off.
    Follow,
}

impl CameraMove {
    /// Every kind, in the bundle's order.
    pub const ALL: [Self; 4] = [Self::Pan, Self::Zoom, Self::Turn, Self::Follow];

    /// The kind's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pan => "pan",
            Self::Zoom => "zoom",
            Self::Turn => "turn",
            Self::Follow => "follow",
        }
    }

    /// The kind `name` names, or `None`.
    #[must_use]
    pub fn of_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

/// The camera moves, one count a kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct CameraCounts {
    pan: u64,
    zoom: u64,
    turn: u64,
    follow: u64,
}

impl CameraCounts {
    const fn of(&self, kind: CameraMove) -> u64 {
        match kind {
            CameraMove::Pan => self.pan,
            CameraMove::Zoom => self.zoom,
            CameraMove::Turn => self.turn,
            CameraMove::Follow => self.follow,
        }
    }

    const fn of_mut(&mut self, kind: CameraMove) -> &mut u64 {
        match kind {
            CameraMove::Pan => &mut self.pan,
            CameraMove::Zoom => &mut self.zoom,
            CameraMove::Turn => &mut self.turn,
            CameraMove::Follow => &mut self.follow,
        }
    }
}

/// Why something was not kept: each is counted, and `manifest.json` lists every one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DropReason {
    /// A Push ended with [`MAX_PUSH_RECORDS`] kept: it is counted, not recorded.
    PushRecordsFull,
    /// A Lull ended with [`MAX_LULL_RECORDS`] kept.
    LullRecordsFull,
    /// A diagnostics line with [`MAX_LOG_LINES`] kept.
    LogFull,
    /// A code past [`MAX_CODES`] distinct ones in its tally.
    CodesFull,
    /// A count at `u64::MAX`.
    CounterFull,
    /// A clock reading before the last ([`ClockFault::Backwards`]).
    ClockBackwards,
    /// A sum of time past `u64` ([`ClockFault::Overflow`]).
    ClockOverflow,
}

impl DropReason {
    /// Every reason, in the manifest's order.
    pub const ALL: [Self; 7] = [
        Self::PushRecordsFull,
        Self::LullRecordsFull,
        Self::LogFull,
        Self::CodesFull,
        Self::CounterFull,
        Self::ClockBackwards,
        Self::ClockOverflow,
    ];

    /// The reason's name in the manifest.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PushRecordsFull => "push_records_full",
            Self::LullRecordsFull => "lull_records_full",
            Self::LogFull => "log_full",
            Self::CodesFull => "codes_full",
            Self::CounterFull => "counter_full",
            Self::ClockBackwards => "clock_backwards",
            Self::ClockOverflow => "clock_overflow",
        }
    }

    const fn of_fault(fault: ClockFault) -> Self {
        match fault {
            ClockFault::Backwards => Self::ClockBackwards,
            ClockFault::Overflow => Self::ClockOverflow,
        }
    }
}

/// How many times each [`DropReason`] was counted.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct DropCounts {
    push_records_full: u64,
    lull_records_full: u64,
    log_full: u64,
    codes_full: u64,
    counter_full: u64,
    clock_backwards: u64,
    clock_overflow: u64,
}

impl DropCounts {
    const fn of(&self, reason: DropReason) -> u64 {
        match reason {
            DropReason::PushRecordsFull => self.push_records_full,
            DropReason::LullRecordsFull => self.lull_records_full,
            DropReason::LogFull => self.log_full,
            DropReason::CodesFull => self.codes_full,
            DropReason::CounterFull => self.counter_full,
            DropReason::ClockBackwards => self.clock_backwards,
            DropReason::ClockOverflow => self.clock_overflow,
        }
    }

    const fn of_mut(&mut self, reason: DropReason) -> &mut u64 {
        match reason {
            DropReason::PushRecordsFull => &mut self.push_records_full,
            DropReason::LullRecordsFull => &mut self.lull_records_full,
            DropReason::LogFull => &mut self.log_full,
            DropReason::CodesFull => &mut self.codes_full,
            DropReason::CounterFull => &mut self.counter_full,
            DropReason::ClockBackwards => &mut self.clock_backwards,
            DropReason::ClockOverflow => &mut self.clock_overflow,
        }
    }

    fn any(&self) -> bool {
        DropReason::ALL.iter().any(|reason| self.of(*reason) != 0)
    }
}

/// What the recorder did with one thing it was told.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Recorded {
    /// Playtest mode is off: nothing was kept.
    Off,
    /// Kept.
    Taken,
    /// Not a thing this phase records (a Skip outside a Push, a Ready outside a Lull): kept as a
    /// diagnostics line only.
    NotInPhase,
    /// Not one the recorder counts (a speed outside the pacer's set): nothing kept.
    Refused,
    /// Not kept, and counted under this reason.
    Dropped(DropReason),
}

/// One kind of diagnostics line: the client's own occurrences, closed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// Playtest mode was switched on.
    PlaytestOn,
    /// A Lull opened.
    LullOpened,
    /// A Push began.
    PushBegan,
    /// A recap opened.
    RecapOpened,
    /// The match ended.
    MatchEnded,
    /// The phase became one the rig cannot name (before the first answer, or the lobby).
    PhaseOther,
    /// This seat pressed Ready.
    Ready,
    /// The tester pressed Skip.
    Skipped,
    /// The tester chose a speed (its code `SPEED_<n>`).
    Speed,
    /// The gateway refused a call (its code).
    Refused,
    /// The window gained focus.
    FocusIn,
    /// The window lost focus.
    FocusOut,
    /// The survey's answers were taken.
    SurveyAnswered,
    /// A bundle was written.
    ExportDone,
    /// A bundle could not be written (its code, the failure's name).
    ExportFailed,
}

impl Event {
    /// Every kind.
    pub const ALL: [Self; 15] = [
        Self::PlaytestOn,
        Self::LullOpened,
        Self::PushBegan,
        Self::RecapOpened,
        Self::MatchEnded,
        Self::PhaseOther,
        Self::Ready,
        Self::Skipped,
        Self::Speed,
        Self::Refused,
        Self::FocusIn,
        Self::FocusOut,
        Self::SurveyAnswered,
        Self::ExportDone,
        Self::ExportFailed,
    ];

    /// The event's word in a line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PlaytestOn => "playtest_on",
            Self::LullOpened => "lull_opened",
            Self::PushBegan => "push_began",
            Self::RecapOpened => "recap_opened",
            Self::MatchEnded => "match_ended",
            Self::PhaseOther => "phase_other",
            Self::Ready => "ready",
            Self::Skipped => "skipped",
            Self::Speed => "speed",
            Self::Refused => "refused",
            Self::FocusIn => "focus_in",
            Self::FocusOut => "focus_out",
            Self::SurveyAnswered => "survey_answered",
            Self::ExportDone => "export_done",
            Self::ExportFailed => "export_failed",
        }
    }

    /// The line a new phase is recorded with.
    const fn of_phase(phase: Phase) -> Self {
        match phase {
            Phase::Lull => Self::LullOpened,
            Phase::Push => Self::PushBegan,
            Phase::Recap => Self::RecapOpened,
            Phase::Ended => Self::MatchEnded,
            Phase::Unknown | Phase::Lobby => Self::PhaseOther,
        }
    }
}

/// One diagnostics line, typed until it is written.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Line {
    seq: u64,
    phase: Phase,
    event: Event,
    code: Option<Code>,
    stamp: u64,
}

impl Line {
    /// The line in the log's grammar.
    fn text(&self) -> String {
        let (seq, phase, event, stamp) =
            (self.seq, self.phase.name(), self.event.name(), self.stamp);
        match &self.code {
            Some(code) => format!("{seq} {phase} {event} {} t={stamp}", code.as_str()),
            None => format!("{seq} {phase} {event} t={stamp}"),
        }
    }
}

/// How one Push is recorded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PushOutcome {
    /// Its time, split by focus.
    Timed(PushTime),
    /// The tester pressed Skip in it: it carries no times.
    Skipped,
    /// Its time could not be taken (the fault is also counted).
    Untimed(ClockFault),
}

/// One Push, as recorded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PushRecord {
    /// The round it was in.
    pub round: u32,
    /// What it recorded.
    pub outcome: PushOutcome,
}

/// One Lull, as recorded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LullRecord {
    /// The round it was in.
    pub round: u32,
    /// Its used time.
    pub time: LullTime,
}

/// The survey's first question, "Play another round?".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Another {
    /// Yes.
    Yes,
    /// No.
    No,
    /// Not answered: said as such, never a default answer.
    #[default]
    Unanswered,
}

impl Another {
    /// Every answer.
    pub const ALL: [Self; 3] = [Self::Yes, Self::No, Self::Unanswered];

    /// The answer's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Unanswered => "unanswered",
        }
    }

    /// The answer `name` names, or `None`.
    #[must_use]
    pub fn of_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|answer| answer.name() == name)
    }
}

/// The survey's second question, "Did you understand why your playbook did what it did?".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Understood {
    /// Yes.
    Yes,
    /// Partly.
    Partly,
    /// No.
    No,
    /// Not answered.
    #[default]
    Unanswered,
}

impl Understood {
    /// Every answer.
    pub const ALL: [Self; 4] = [Self::Yes, Self::Partly, Self::No, Self::Unanswered];

    /// The answer's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::Partly => "partly",
            Self::No => "no",
            Self::Unanswered => "unanswered",
        }
    }

    /// The answer `name` names, or `None`.
    #[must_use]
    pub fn of_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|answer| answer.name() == name)
    }
}

/// The survey's answers (decision 11).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Survey {
    another: Another,
    understood: Understood,
    /// "What did you most want to change during a Push?", as the tester wrote it; `None` when
    /// it was left empty.
    wanted: Option<String>,
}

/// Why the survey's answers were not taken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SurveyRefusal {
    /// Playtest mode is off.
    Off,
    /// The text is longer than [`MAX_SURVEY_TEXT_CHARS`]; nothing was taken.
    TooLong,
}

impl SurveyRefusal {
    /// The string table's `playtest_survey_<name>` row.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::TooLong => "too_long",
        }
    }
}

/// Why no bundle was built.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BundleRefusal {
    /// Playtest mode is off: there is nothing to bundle.
    Off,
    /// The zip would be past [`MAX_BUNDLE_BYTES`], or past what a zip's 32-bit sizes hold.
    TooLarge,
}

impl BundleRefusal {
    /// The string table's `playtest_export_<name>` row.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::TooLarge => "too_large",
        }
    }
}

/// Why an export did not write a bundle.
#[derive(Debug)]
pub enum ExportFailure {
    /// No bundle was built.
    Refused(BundleRefusal),
    /// The bundle was built and the write failed (or, for the folder's sync, landed with words).
    Write(WriteFailure),
}

/// A bundle written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Exported {
    /// The zip's size.
    pub bytes: usize,
    /// A temporary file of ours from a stopped write, found and removed first.
    pub stale_removed: Option<PathBuf>,
}

/// What the lobby shows of the recorder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Summary {
    /// Pushes ended.
    pub pushes: u64,
    /// Of which skipped.
    pub skipped_pushes: u64,
    /// Lulls ended.
    pub lulls: usize,
    /// Diagnostics lines kept.
    pub lines: usize,
    /// Whether anything was dropped.
    pub dropped: bool,
}

/// Everything kept while Playtest mode is on.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Recording {
    clock: PlaytestClock,
    phase: Phase,
    round: u32,
    preset: Option<ServedPreset>,
    /// The open Push's round and whether it was skipped.
    push_round: u32,
    push_skipped: bool,
    /// The open Lull's round.
    lull_round: u32,
    push_records: Vec<PushRecord>,
    pushes: u64,
    skipped_pushes: u64,
    lull_records: Vec<LullRecord>,
    speed_changes: BTreeMap<u32, u64>,
    camera_moves: CameraCounts,
    verifier_rows: BTreeMap<Code, u64>,
    /// The text revision whose shown codes are [`Recording::shown`].
    shown_revision: Option<u64>,
    shown: BTreeSet<Code>,
    /// Whether [`Recording::shown`] reached [`MAX_CODES`] in this revision, and was counted so.
    shown_full: bool,
    refusals: BTreeMap<Code, u64>,
    log: Vec<Line>,
    seq: u64,
    dropped: DropCounts,
    survey: Survey,
}

/// One more, or the reason it cannot be.
fn bump(count: &mut u64) -> Result<(), DropReason> {
    *count = count.checked_add(1).ok_or(DropReason::CounterFull)?;
    Ok(())
}

impl Recording {
    fn new(clock: PlaytestClock, phase: Phase, round: u32, preset: Option<ServedPreset>) -> Self {
        Self {
            clock,
            phase,
            round,
            preset,
            push_round: round,
            push_skipped: false,
            lull_round: round,
            push_records: Vec::new(),
            pushes: 0,
            skipped_pushes: 0,
            lull_records: Vec::new(),
            speed_changes: SPEEDS.into_iter().map(|speed| (speed, 0)).collect(),
            camera_moves: CameraCounts::default(),
            verifier_rows: BTreeMap::new(),
            shown_revision: None,
            shown: BTreeSet::new(),
            shown_full: false,
            refusals: BTreeMap::new(),
            log: Vec::new(),
            seq: 0,
            dropped: DropCounts::default(),
            survey: Survey::default(),
        }
    }

    /// Counts `reason` once more. A drop count already at `u64::MAX` is left there: the manifest
    /// then says `u64::MAX`, which reads as "at least this many", and the reason is still
    /// answered.
    fn drop_for(&mut self, reason: DropReason) -> Recorded {
        let count = self.dropped.of_mut(reason);
        if let Some(next) = count.checked_add(1) {
            *count = next;
        }
        Recorded::Dropped(reason)
    }

    /// The result `result` as recorded: a fault counted under its reason.
    fn noted(&mut self, result: Result<(), DropReason>) -> Recorded {
        match result {
            Ok(()) => Recorded::Taken,
            Err(reason) => self.drop_for(reason),
        }
    }

    /// A diagnostics line for `event` at `now_us`, with `code`.
    fn line(&mut self, now_us: u64, event: Event, code: Option<Code>) -> Recorded {
        if let Err(reason) = bump(&mut self.seq) {
            return self.drop_for(reason);
        }
        let stamp = match self.clock.stamp_ms(now_us) {
            Ok(stamp) => stamp,
            Err(fault) => return self.drop_for(DropReason::of_fault(fault)),
        };
        if self.log.len() >= MAX_LOG_LINES {
            return self.drop_for(DropReason::LogFull);
        }
        self.log.push(Line {
            seq: self.seq,
            phase: self.phase,
            event,
            code,
            stamp,
        });
        Recorded::Taken
    }

    /// `code` counted once more in its tally, or dropped past [`MAX_CODES`] distinct codes.
    fn tally(&mut self, which: Tally, code: Code) -> Recorded {
        let tally = match which {
            Tally::Verifier => &mut self.verifier_rows,
            Tally::Refusal => &mut self.refusals,
        };
        let counted = if let Some(count) = tally.get_mut(&code) {
            bump(count)
        } else if tally.len() >= MAX_CODES {
            Err(DropReason::CodesFull)
        } else {
            tally.insert(code, 1);
            Ok(())
        };
        self.noted(counted)
    }

    /// The open Push closed at `now_us`.
    fn close_push(&mut self, now_us: u64) {
        let outcome = match self.clock.push_ended(now_us) {
            Ok(Some(_)) if self.push_skipped => PushOutcome::Skipped,
            Ok(Some(time)) => PushOutcome::Timed(time),
            // No Push was open: one under way when Playtest mode was switched on is not recorded.
            Ok(None) => return,
            Err(fault) => {
                self.drop_for(DropReason::of_fault(fault));
                PushOutcome::Untimed(fault)
            }
        };
        let counted = bump(&mut self.pushes);
        self.noted(counted);
        if outcome == PushOutcome::Skipped {
            let counted = bump(&mut self.skipped_pushes);
            self.noted(counted);
        }
        if self.push_records.len() >= MAX_PUSH_RECORDS {
            self.drop_for(DropReason::PushRecordsFull);
        } else {
            self.push_records.push(PushRecord {
                round: self.push_round,
                outcome,
            });
        }
    }

    /// The open Lull closed at `now_us`.
    fn close_lull(&mut self, now_us: u64) {
        let time = match self.clock.lull_ended(now_us) {
            Ok(Some(time)) => time,
            // No Lull was open: one under way when Playtest mode was switched on is not recorded.
            Ok(None) => return,
            Err(fault) => {
                self.drop_for(DropReason::of_fault(fault));
                return;
            }
        };
        if self.lull_records.len() >= MAX_LULL_RECORDS {
            self.drop_for(DropReason::LullRecordsFull);
        } else {
            self.lull_records.push(LullRecord {
                round: self.lull_round,
                time,
            });
        }
    }

    /// A clock fault counted, or nothing.
    fn clocked(&mut self, result: Result<(), ClockFault>) {
        if let Err(fault) = result {
            self.drop_for(DropReason::of_fault(fault));
        }
    }
}

/// Which code tally.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tally {
    Verifier,
    Refusal,
}

/// Playtest mode's recorder: off, or on with what it has kept.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Recorder {
    on: Option<Box<Recording>>,
}

impl Recorder {
    /// A recorder that is off: every launch starts here.
    #[must_use]
    pub const fn new() -> Self {
        Self { on: None }
    }

    /// Whether Playtest mode is on.
    #[must_use]
    pub const fn is_on(&self) -> bool {
        self.on.is_some()
    }

    /// Switches Playtest mode on or off at `now_us`. On, it starts from nothing, in `phase` of
    /// `round` (a phase already under way is not recorded), with the window's focus as
    /// `focused`; already on, nothing changes. Off, everything kept is dropped.
    pub fn switch(
        &mut self,
        on: bool,
        now_us: u64,
        focused: bool,
        phase: Phase,
        round: u32,
    ) -> Recorded {
        if !on {
            self.on = None;
            return Recorded::Off;
        }
        if self.on.is_some() {
            return Recorded::Taken;
        }
        let mut recording =
            Recording::new(PlaytestClock::start(now_us, focused), phase, round, None);
        recording.line(now_us, Event::PlaytestOn, None);
        self.on = Some(Box::new(recording));
        Recorded::Taken
    }

    /// The match is in `phase` of `round` at `now_us`, under `preset` as last read (`None`
    /// before a readable `get_status`). A change of phase closes the open Push or Lull and
    /// opens the next, with its line.
    pub fn phase(
        &mut self,
        now_us: u64,
        phase: Phase,
        round: u32,
        preset: Option<ServedPreset>,
    ) -> Recorded {
        let Some(recording) = self.on.as_mut() else {
            return Recorded::Off;
        };
        if preset.is_some() {
            recording.preset = preset;
        }
        recording.round = round;
        if phase == recording.phase {
            return Recorded::Taken;
        }
        match recording.phase {
            Phase::Push => recording.close_push(now_us),
            Phase::Lull => recording.close_lull(now_us),
            Phase::Unknown | Phase::Lobby | Phase::Recap | Phase::Ended => {}
        }
        recording.phase = phase;
        match phase {
            Phase::Push => {
                recording.push_round = round;
                recording.push_skipped = false;
                let began = recording.clock.push_began(now_us);
                recording.clocked(began);
            }
            Phase::Lull => {
                recording.lull_round = round;
                let began = recording.clock.lull_began(now_us);
                recording.clocked(began);
            }
            Phase::Unknown | Phase::Lobby | Phase::Recap | Phase::Ended => {}
        }
        recording.line(now_us, Event::of_phase(phase), None)
    }

    /// This seat's Ready was taken at `now_us`.
    pub fn ready(&mut self, now_us: u64) -> Recorded {
        let Some(recording) = self.on.as_mut() else {
            return Recorded::Off;
        };
        if recording.phase != Phase::Lull {
            recording.line(now_us, Event::Ready, None);
            return Recorded::NotInPhase;
        }
        let ready = recording.clock.lull_ready(now_us);
        recording.clocked(ready);
        recording.line(now_us, Event::Ready, None)
    }

    /// The tester pressed Skip at `now_us`: the open Push is recorded as skipped.
    pub fn skip(&mut self, now_us: u64) -> Recorded {
        let Some(recording) = self.on.as_mut() else {
            return Recorded::Off;
        };
        if recording.phase != Phase::Push {
            recording.line(now_us, Event::Skipped, None);
            return Recorded::NotInPhase;
        }
        recording.push_skipped = true;
        recording.line(now_us, Event::Skipped, None)
    }

    /// The tester chose `speed` at `now_us`; a speed outside the pacer's set is refused.
    pub fn speed(&mut self, now_us: u64, speed: u32) -> Recorded {
        let Some(recording) = self.on.as_mut() else {
            return Recorded::Off;
        };
        let Some(count) = recording.speed_changes.get_mut(&speed) else {
            return Recorded::Refused;
        };
        let counted = bump(count);
        let noted = recording.noted(counted);
        let code = Code::of_speed(speed);
        recording.line(now_us, Event::Speed, Some(code));
        noted
    }

    /// The rig read `answer` at `now_us`: what it records of it, and nothing else. A change of
    /// phase is recorded with the rig's phase, round and served preset ([`Recorder::phase`]);
    /// a refusal by its code alone ([`Code::of_refusal`]), its message dropped. The view, the
    /// feed's rows and every other word of the answer are not read.
    pub fn answered(&mut self, now_us: u64, rig: &Rig, answer: &Answer) -> Recorded {
        if !self.is_on() {
            return Recorded::Off;
        }
        let mut recorded = Recorded::Taken;
        if answer.phase_changed {
            recorded = self.phase(now_us, rig.phase(), rig.round(), rig.status_facts().preset);
        }
        if let Some(said) = answer.error.as_deref() {
            recorded = self.refused(now_us, &Code::of_refusal(said));
        }
        recorded
    }

    /// The editor's rows as shown now: each error row's code, once per text revision it is
    /// shown in ([`Recorder::rows`]). Rows that do not describe the text on screen (none yet, or
    /// a refused Load's) are not counted. The row's pointer, sentence and message are not read.
    pub fn shown(&mut self, editor: &Editor) -> Recorded {
        if !self.is_on() {
            return Recorded::Off;
        }
        if !editor.rows_current() {
            return Recorded::NotInPhase;
        }
        let codes = editor
            .rows()
            .iter()
            .filter(|row| row.severity == RowSeverity::Error)
            .map(|row| Code::of_verifier(&row.code));
        self.rows(editor.revision(), codes)
    }

    /// The gateway refused a call at `now_us` with `code` (its message is never given here).
    pub fn refused(&mut self, now_us: u64, code: &Code) -> Recorded {
        let Some(recording) = self.on.as_mut() else {
            return Recorded::Off;
        };
        let code = code.clone();
        let tallied = recording.tally(Tally::Refusal, code.clone());
        recording.line(now_us, Event::Refused, Some(code));
        tallied
    }

    /// The client drew a frame at `now_us`: the open Push's and Lull's time is taken up to it, so
    /// a step longer than [`MAX_ATTENTION_STEP_US`] is told apart as a frame gap. No line.
    pub fn seen(&mut self, now_us: u64) -> Recorded {
        let Some(recording) = self.on.as_mut() else {
            return Recorded::Off;
        };
        let taken = recording.clock.seen(now_us);
        recording.clocked(taken);
        Recorded::Taken
    }

    /// The window gained (`true`) or lost focus at `now_us`.
    pub fn focus(&mut self, now_us: u64, focused: bool) -> Recorded {
        let Some(recording) = self.on.as_mut() else {
            return Recorded::Off;
        };
        let taken = recording.clock.focus(now_us, focused);
        recording.clocked(taken);
        let event = if focused {
            Event::FocusIn
        } else {
            Event::FocusOut
        };
        recording.line(now_us, event, None)
    }

    /// The tester moved the camera.
    pub fn camera(&mut self, kind: CameraMove) -> Recorded {
        let Some(recording) = self.on.as_mut() else {
            return Recorded::Off;
        };
        let counted = bump(recording.camera_moves.of_mut(kind));
        recording.noted(counted)
    }

    /// The editor shows the verifier's error rows `codes` for text revision `revision`: each
    /// code is counted once per revision it is shown in. The rows are told again every frame, so
    /// what is dropped is dropped once a revision, never once a frame: a code past the tally's
    /// cap is remembered as shown, and the revision's own set, once at [`MAX_CODES`], is counted
    /// full once.
    pub fn rows<I>(&mut self, revision: u64, codes: I) -> Recorded
    where
        I: IntoIterator<Item = Code>,
    {
        let Some(recording) = self.on.as_mut() else {
            return Recorded::Off;
        };
        if recording.shown_revision != Some(revision) {
            recording.shown_revision = Some(revision);
            recording.shown.clear();
            recording.shown_full = false;
        }
        let mut answer = Recorded::Taken;
        for code in codes {
            if recording.shown.contains(&code) {
                continue;
            }
            if recording.shown.len() >= MAX_CODES {
                if !recording.shown_full {
                    recording.shown_full = true;
                    answer = recording.drop_for(DropReason::CodesFull);
                }
                continue;
            }
            recording.shown.insert(code.clone());
            let tallied = recording.tally(Tally::Verifier, code);
            if tallied != Recorded::Taken {
                answer = tallied;
            }
        }
        answer
    }

    /// The survey's answers at `now_us`: the third is `text` as the tester wrote it, none when
    /// empty.
    ///
    /// # Errors
    ///
    /// [`SurveyRefusal`]: off, or a text past [`MAX_SURVEY_TEXT_CHARS`], which is refused whole
    /// and never cut.
    pub fn survey(
        &mut self,
        now_us: u64,
        another: Another,
        understood: Understood,
        text: &str,
    ) -> Result<(), SurveyRefusal> {
        let Some(recording) = self.on.as_mut() else {
            return Err(SurveyRefusal::Off);
        };
        if text.chars().count() > MAX_SURVEY_TEXT_CHARS {
            return Err(SurveyRefusal::TooLong);
        }
        recording.survey = Survey {
            another,
            understood,
            wanted: (!text.is_empty()).then(|| text.to_owned()),
        };
        recording.line(now_us, Event::SurveyAnswered, None);
        Ok(())
    }

    /// What the lobby shows; `None` while off.
    #[must_use]
    pub fn summary(&self) -> Option<Summary> {
        self.on.as_ref().map(|recording| Summary {
            pushes: recording.pushes,
            skipped_pushes: recording.skipped_pushes,
            lulls: recording.lull_records.len(),
            lines: recording.log.len(),
            dropped: recording.dropped.any(),
        })
    }

    /// The Push records kept, in order; `None` while off.
    #[must_use]
    pub fn push_records(&self) -> Option<&[PushRecord]> {
        self.on
            .as_ref()
            .map(|recording| recording.push_records.as_slice())
    }

    /// The Lull records kept, in order; `None` while off.
    #[must_use]
    pub fn lull_records(&self) -> Option<&[LullRecord]> {
        self.on
            .as_ref()
            .map(|recording| recording.lull_records.as_slice())
    }

    /// How many times `reason` was counted; `None` while off.
    #[must_use]
    pub fn dropped(&self, reason: DropReason) -> Option<u64> {
        self.on
            .as_ref()
            .map(|recording| recording.dropped.of(reason))
    }

    /// The bundle's four files, in [`BUNDLE_ENTRIES`]' order, as `(name, bytes)`.
    ///
    /// # Errors
    ///
    /// [`BundleRefusal::Off`] while off.
    pub fn files(&self) -> Result<Vec<(&'static str, Vec<u8>)>, BundleRefusal> {
        let recording = self.on.as_ref().ok_or(BundleRefusal::Off)?;
        let [manifest, survey, metrics, log] = BUNDLE_ENTRIES;
        Ok(vec![
            (
                manifest,
                json::write(&manifest_json(recording)).into_bytes(),
            ),
            (
                survey,
                json::write(&survey_json(&recording.survey)).into_bytes(),
            ),
            (metrics, json::write(&metrics_json(recording)).into_bytes()),
            (log, log_text(&recording.log).into_bytes()),
        ])
    }

    /// The bundle: one zip of [`Recorder::files`].
    ///
    /// # Errors
    ///
    /// [`BundleRefusal`]: off, or past [`MAX_BUNDLE_BYTES`].
    pub fn bundle(&self) -> Result<Vec<u8>, BundleRefusal> {
        let zip = stored_zip(&self.files()?)?;
        if zip.len() > MAX_BUNDLE_BYTES {
            return Err(BundleRefusal::TooLarge);
        }
        Ok(zip)
    }

    /// Export playtest bundle: the bundle written whole to `path`, the file the tester picked,
    /// and nowhere else; then a line saying it was (`export_done`) or why not (`export_failed`,
    /// the failure's name as its code), for the next export.
    ///
    /// # Errors
    ///
    /// [`ExportFailure`]: no bundle, or the write's failure.
    pub fn export(&mut self, now_us: u64, path: &Path) -> Result<Exported, ExportFailure> {
        let zip = self.bundle().map_err(ExportFailure::Refused)?;
        let written = crate::write::write_whole(path, &zip);
        if let Some(recording) = self.on.as_mut() {
            match &written {
                Ok(_) => recording.line(now_us, Event::ExportDone, None),
                Err(failure) => {
                    recording.line(now_us, Event::ExportFailed, Some(Code::of_failure(failure)))
                }
            };
        }
        written
            .map(|written| Exported {
                bytes: zip.len(),
                stale_removed: written.stale_removed,
            })
            .map_err(ExportFailure::Write)
    }
}

/// A count as a JSON number.
fn number(value: u64) -> Json {
    Json::Number(value.to_string())
}

/// A count of things kept as a JSON number.
fn count_number(value: usize) -> Json {
    Json::Number(value.to_string())
}

/// A string as JSON.
fn string(text: &str) -> Json {
    Json::String(text.to_owned())
}

/// An object from `(key, value)` pairs.
fn object<const N: usize>(entries: [(&str, Json); N]) -> Json {
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

/// A tally of codes as a JSON object, in code order.
fn tally_json(tally: &BTreeMap<Code, u64>) -> Json {
    Json::Object(
        tally
            .iter()
            .map(|(code, count)| (code.as_str().to_owned(), number(*count)))
            .collect(),
    )
}

/// `manifest.json`: what the bundle is, its caps, and every reason something was not kept.
fn manifest_json(recording: &Recording) -> Json {
    // No readable `get_status` yet is its own word, not a preset.
    let preset = match recording.preset {
        Some(served) => served.name(),
        None => "not_read",
    };
    object([
        ("bundle_format", number(u64::from(BUNDLE_FORMAT))),
        (
            "files",
            Json::Array(BUNDLE_ENTRIES.iter().map(|name| string(name)).collect()),
        ),
        ("preset", string(preset)),
        ("last_round", number(u64::from(recording.round))),
        (
            "skip_rate",
            string("skipped_pushes / pushes, both in metrics.json; this file does not divide"),
        ),
        ("recording", string("absent until S7")),
        (
            "survey_free_text",
            string(
                "survey.json's wanted_to_change is the tester's own words, sent as written: read \
                 it before sending",
            ),
        ),
        (
            "times",
            string(
                "metrics.json's times are wall microseconds on the tester's computer; a step \
                 between two frames longer than max_attention_step_us (a stalled window, a \
                 sleeping machine) is no attention: it is summed in its Push's or Lull's gap_us \
                 and counted in its gaps, never in focused_us, unfocused_us or used_us; \
                 diagnostics.log's t= is whole milliseconds since Playtest mode was switched on",
            ),
        ),
        (
            "caps",
            object([
                ("push_records", count_number(MAX_PUSH_RECORDS)),
                ("lull_records", count_number(MAX_LULL_RECORDS)),
                ("log_lines", count_number(MAX_LOG_LINES)),
                ("codes_per_tally", count_number(MAX_CODES)),
                ("survey_text_chars", count_number(MAX_SURVEY_TEXT_CHARS)),
                ("bundle_bytes", count_number(MAX_BUNDLE_BYTES)),
                ("max_attention_step_us", number(MAX_ATTENTION_STEP_US)),
            ]),
        ),
        (
            "dropped",
            Json::Object(
                DropReason::ALL
                    .iter()
                    .map(|reason| {
                        (
                            reason.name().to_owned(),
                            number(recording.dropped.of(*reason)),
                        )
                    })
                    .collect(),
            ),
        ),
    ])
}

/// `survey.json`: the three answers; the third as written, or said unanswered.
fn survey_json(survey: &Survey) -> Json {
    let wanted = match survey.wanted.as_ref() {
        Some(text) => object([("answered", Json::Bool(true)), ("text", string(text))]),
        None => object([("answered", Json::Bool(false))]),
    };
    object([
        ("play_another_round", string(survey.another.name())),
        ("understood_why", string(survey.understood.name())),
        ("wanted_to_change", wanted),
    ])
}

/// `metrics.json`: the counts and the records.
fn metrics_json(recording: &Recording) -> Json {
    let push_time = recording
        .push_records
        .iter()
        .map(|record| {
            let round = ("round", number(u64::from(record.round)));
            match record.outcome {
                PushOutcome::Timed(time) => object([
                    round,
                    ("skipped", Json::Bool(false)),
                    ("focused_us", number(time.focused_us)),
                    ("unfocused_us", number(time.unfocused_us)),
                    ("gap_us", number(time.gap_us)),
                    ("gaps", number(time.gaps)),
                ]),
                PushOutcome::Skipped => object([round, ("skipped", Json::Bool(true))]),
                PushOutcome::Untimed(fault) => object([
                    round,
                    ("skipped", Json::Bool(false)),
                    ("untimed", string(fault.name())),
                ]),
            }
        })
        .collect();
    let lull_time = recording
        .lull_records
        .iter()
        .map(|record| {
            object([
                ("round", number(u64::from(record.round))),
                ("used_us", number(record.time.used_us)),
                ("gap_us", number(record.time.gap_us)),
                ("gaps", number(record.time.gaps)),
                ("ended_by", string(record.time.ended_by.name())),
            ])
        })
        .collect();
    let speeds = recording
        .speed_changes
        .iter()
        .map(|(speed, count)| (speed.to_string(), number(*count)))
        .collect();
    let camera = CameraMove::ALL
        .iter()
        .map(|kind| {
            (
                kind.name().to_owned(),
                number(recording.camera_moves.of(*kind)),
            )
        })
        .collect();
    object([
        ("pushes", number(recording.pushes)),
        ("skipped_pushes", number(recording.skipped_pushes)),
        ("push_time", Json::Array(push_time)),
        ("lull_time", Json::Array(lull_time)),
        ("speed_changes", Json::Object(speeds)),
        ("camera_moves", Json::Object(camera)),
        ("verifier_error_rows", tally_json(&recording.verifier_rows)),
        ("refusals", tally_json(&recording.refusals)),
    ])
}

/// `diagnostics.log`: one line each, each ending in a newline.
fn log_text(log: &[Line]) -> String {
    let mut text = String::new();
    for line in log {
        text.push_str(&line.text());
        text.push('\n');
    }
    text
}

/// The CRC-32 of `bytes`, as zip stores it (IEEE 802.3, reflected polynomial `0xEDB88320`), a
/// bit at a time: `xtask/src/zip.rs`'s, which this walled crate cannot depend on (`xtask` is
/// dev-only and never shipped). A checksum the zip format requires, not a hash of state.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// "Version made by": Unix (3) in the high byte, specification 2.0 in the low.
const MADE_BY: u16 = 0x0314;
/// "Version needed to extract" for a stored file.
const NEEDED: u16 = 10;
/// 1980-01-01 at midnight, the MS-DOS epoch, on every entry: the bundle carries no time.
const DOS_DATE: u16 = 0x0021;
const DOS_TIME: u16 = 0x0000;
/// A plain file's Unix mode, in the external attributes' high half.
const FILE_ATTRIBUTES: u32 = 0o100_644 << 16;

/// `files` as one zip: each entry stored (no compression), with no extra field, no comment and
/// no zip64; [`BundleRefusal::TooLarge`] when a size or an offset does not fit the classic
/// format's 32 bits or the entry count its 16.
fn stored_zip(files: &[(&str, Vec<u8>)]) -> Result<Vec<u8>, BundleRefusal> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, data) in files {
        let offset = u32::try_from(out.len()).map_err(|_| BundleRefusal::TooLarge)?;
        let size = u32::try_from(data.len()).map_err(|_| BundleRefusal::TooLarge)?;
        let name_len = u16::try_from(name.len()).map_err(|_| BundleRefusal::TooLarge)?;
        let crc = crc32(data);
        out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        out.extend_from_slice(&NEEDED.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&DOS_TIME.to_le_bytes());
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
        central.extend_from_slice(&MADE_BY.to_le_bytes());
        central.extend_from_slice(&NEEDED.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&DOS_TIME.to_le_bytes());
        central.extend_from_slice(&DOS_DATE.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&FILE_ATTRIBUTES.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let entries = u16::try_from(files.len()).map_err(|_| BundleRefusal::TooLarge)?;
    let central_offset = u32::try_from(out.len()).map_err(|_| BundleRefusal::TooLarge)?;
    let central_size = u32::try_from(central.len()).map_err(|_| BundleRefusal::TooLarge)?;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&entries.to_le_bytes());
    out.extend_from_slice(&entries.to_le_bytes());
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    Ok(out)
}
