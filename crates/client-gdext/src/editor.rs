// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The playbook editor's half of the seat connection: which planning call goes out next,
//! and what each answer means (skeleton plan T19, pull requests 1 and 2).
//!
//! The editor is **one more client of the Seat Gateway** (spec section 12), and a thin one.
//! Every verdict it shows is the verifier's, every travel time is the estimator's, every
//! edit is a JSON Patch the gateway applied, and every Fix is the verifier's own patch
//! (AGENTS.md section 3 rule 4: the editor "runs no validation or time maths of its own —
//! it asks the gateway"). What is left for this module is marshalling and ordering:
//!
//! * turning a click on the map into a JSON Patch — an **append** to the route, whose value
//!   is the step the click names ([`step_value`]); the patch is applied by `patch_plan`;
//! * choosing the next call, one at a time, on the seat connection the vista already holds
//!   (`docs/design/skeleton-plan-t16a-notes.md` section B, "T19" (5)), in an order that
//!   keeps what the player sees about the text they are looking at;
//! * reading each answer into rows, a route, a ghost, the notes, the drafts, the wizard's
//!   pages ([`crate::wizard`]) and the rule list.
//!
//! **When** a call may go out is not decided here: the watch rig ([`crate::rig`]) holds the
//! seat connection's rate budget and the phase, and the pacer's idle timer
//! ([`crate::pacer::IdleTimer`]) says when FULL is owed. This module holds no clock.
//!
//! # The order of the calls
//!
//! 1. an **edit** the player asked for — a Load, a map action, a Fix, an Undo, a placement
//!    preview — in the order they were asked, one at a time, each against the text the one
//!    before it produced;
//! 2. **QUICK** for the newest text, once no edit is waiting ("QUICK runs on every edit",
//!    spec section 13; an edit a newer edit replaced before its check went out is not
//!    checked separately, so the rows always describe the text on screen);
//! 3. the **route estimate** for the newest text;
//! 4. the **rule list**: `render_plan`'s prose for the newest text;
//! 5. everything else the player asked for, in order: submit, the notes, the drafts, a
//!    beacon's description, the template list and the wizard's instantiations;
//! 6. **FULL**, once the editor has been left alone for 600 ms.
//!
//! An edit is anything that changes the text on screen: a Load, a map action, a Fix, an
//! Undo, a placement preview, the wizard's playbook put into the editor, and the carried
//! draft opened through `get_draft`.
//!
//! # What the editor reads out of the playbook itself
//!
//! One thing: where the route goes, so it can be drawn and priced. The step targets are
//! read out of the player's text by a deliberately lax walk ([`route_waypoints`]) over the
//! JSON the comments were stripped from. It is lax because it decides nothing: the file's
//! verdict is the verifier's, a step this walk cannot read is simply not drawn, and the
//! times on the route are the gateway's answer to the waypoints it was sent.

use std::collections::VecDeque;

use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::api::v1::{
    GetBeaconResponse, GetBriefingResponse, GetDraftResponse, ListDraftsResponse,
    PatchPlanResponse, SaveNotesResponse, SubmitPlanResponse, VerifyPlanResponse, VerifyReport,
};
use pharmakos_proto::json::{self, Json};

use crate::enums;
use crate::error::BridgeError;
use crate::view::{Entity, EntityKind, split_footer};
use crate::wizard::{self, Instance, TemplateRow, Wizard};

/// The verifier codes on which Load refuses a file rather than opening it.
///
/// Spec section 13: "Load rejects any out-of-vocabulary construct with a verifier code and a
/// JSON Pointer to the offending node; it never silently strips what it cannot draw." Load
/// is a QUICK `verify_plan` of the file as it is on disk, and these are the decode stage's
/// codes for a file that carries something the v1 vocabulary does not have, or that does not
/// decode at all: `E0001` not a playbook, `E0002` a field the schema does not declare,
/// `E0003` a construct held back to a later version (flags, branch, repeat), `E0004` a
/// schema version this build does not speak, `E0006` a SCRIPT author. Every other
/// diagnostic opens the file and shows as a row, because the editor can draw it.
///
/// PLACEHOLDER: the list is the client's reading of the diagnostic catalogue's decode
/// family. A column in the catalogue saying which codes refuse a load would make it the
/// verifier's, and a new decode code would then need no change here. OWNER, S6, with the
/// editor's Load and Save as template.
pub const LOAD_REFUSALS: &[&str] = &["E0001", "E0002", "E0003", "E0004", "E0006"];

/// The draft id the editor's "Save draft" keeps its copy under, so repeated saves replace
/// one draft rather than filling the seat's store.
///
/// PLACEHOLDER: one editor draft per seat is the skeleton's; named drafts and a draft
/// browser are S6's (skeleton plan T19, PR 2 and S6).
pub const EDITOR_DRAFT_ID: &str = "editor";

/// The draft id the gateway pre-loads last round's playbook under
/// (`pharmakos_gateway::surface::CARRIED_DRAFT_ID`, spec section 13 "Draft continuity").
pub const CARRIED_DRAFT_ID: &str = "carried";

/// The route's JSON Pointer in a playbook.
const ROUTE_POINTER: &str = "/declarative/route";

/// A beacon selector: what Alt-click turns a fixed target into (spec section 13, "Alt-click
/// turns a fixed target into nearest, weakest, safest or most threatened").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selector {
    /// The nearest own beacon when the step starts.
    Nearest,
    /// The weakest own beacon when the step starts.
    Weakest,
    /// The safest own beacon when the step starts.
    Safest,
    /// The most threatened own beacon when the step starts.
    MostThreatened,
}

impl Selector {
    /// The selector a name stands for: `nearest`, `weakest`, `safest`, `most_threatened`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "nearest" => Some(Self::Nearest),
            "weakest" => Some(Self::Weakest),
            "safest" => Some(Self::Safest),
            "most_threatened" => Some(Self::MostThreatened),
            _ => None,
        }
    }

    /// The `gp.v1.BeaconRef` this selector is, over the seat's own beacons.
    ///
    /// Own beacons because the click was on one of the seat's own: an enemy beacon is a
    /// target for an Attack beacon, which is S2's.
    fn beacon_ref(self) -> Json {
        let own = || object(vec![("filter", object(vec![("side", text("OWN"))]))]);
        match self {
            Self::Nearest => object(vec![("nearest", own())]),
            Self::Weakest => object(vec![("weakest", own())]),
            Self::Safest => object(vec![("safest", object(Vec::new()))]),
            Self::MostThreatened => object(vec![("most_threatened", own())]),
        }
    }
}

/// What a click on the map points at.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Target {
    /// One of the seat's beacons, by its `b_NN` id.
    Beacon(String),
    /// A beacon chosen when the step starts.
    Selector(Selector),
    /// A voxel of ground, in the sim's axes (x east, y north, z up).
    Voxel([i32; 3]),
}

/// A Quartermaster priority, as a Visit & change row sets it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Priority {
    /// `LOW`.
    Low,
    /// `NORMAL`.
    Normal,
    /// `HIGH`.
    High,
}

impl Priority {
    const fn name(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Normal => "NORMAL",
            Self::High => "HIGH",
        }
    }
}

/// What the player chose from the map's menu (spec section 13, "Map route").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Go here: a move step.
    Go,
    /// Visit & change: an interface step with one row, the beacon's priority.
    ///
    /// PLACEHOLDER: a priority row is the one change the skeleton's menu offers; the
    /// mandate and build-target rows arrive with S3's pickers. OWNER, S3.
    Visit(Priority),
    /// Recycle: an interface step with the recycle row.
    Recycle,
    /// Place beacon: a place-beacon step.
    Place,
}

impl Action {
    /// The action a name stands for: `go`, `visit_low`, `visit_normal`, `visit_high`,
    /// `recycle`, `place`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "go" => Some(Self::Go),
            "visit_low" => Some(Self::Visit(Priority::Low)),
            "visit_normal" => Some(Self::Visit(Priority::Normal)),
            "visit_high" => Some(Self::Visit(Priority::High)),
            "recycle" => Some(Self::Recycle),
            "place" => Some(Self::Place),
            _ => None,
        }
    }

    /// The stem of the label the new step gets.
    const fn stem(self) -> &'static str {
        match self {
            Self::Go => "go",
            Self::Visit(_) => "visit",
            Self::Recycle => "recycle",
            Self::Place => "place",
        }
    }
}

/// A validation row's severity: the icon and the word beside it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowSeverity {
    /// The playbook cannot be sealed as it is.
    Error,
    /// Worth a look.
    Warning,
    /// For information.
    Info,
}

impl RowSeverity {
    /// The name GDScript looks the icon and the word up by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// A Fix button: the verifier's own machine-applicable patch and what the button says.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fix {
    /// What the button says, from the verifier.
    pub title: String,
    /// The RFC 6902 patch, as the verifier wrote it.
    pub patch: String,
}

/// One validation row: an icon, a plain sentence, the code and the pointer, and the fixes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// The severity.
    pub severity: RowSeverity,
    /// The diagnostic code, `E0002`.
    pub code: String,
    /// The JSON Pointer into the playbook, as the verifier wrote it.
    pub pointer: String,
    /// The plain-language sentence: the verifier's beginner text, or its precise message
    /// when it has none. The row's accessible name is exactly this (spec section 13,
    /// "accessible names generated from the rendered sentences").
    pub sentence: String,
    /// The precise message.
    pub message: String,
    /// The machine-applicable fixes only: those are the editor's Fix buttons (spec section
    /// 13; `gp.api.v1.PatchSuggestion.Applicability`).
    pub fixes: Vec<Fix>,
}

/// The rows of one report, in the verifier's order.
#[must_use]
pub fn rows_of(report: &VerifyReport) -> Vec<Row> {
    report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let severity = match diagnostic.severity() {
                Severity::Warning => RowSeverity::Warning,
                Severity::Info => RowSeverity::Info,
                Severity::Error | Severity::Unspecified => RowSeverity::Error,
            };
            let sentence = if diagnostic.beginner.is_empty() {
                diagnostic.message.clone()
            } else {
                diagnostic.beginner.clone()
            };
            let fixes = diagnostic
                .suggestions
                .iter()
                .filter(|suggestion| suggestion.applicability() == Applicability::MachineApplicable)
                .map(|suggestion| Fix {
                    title: suggestion.title.clone(),
                    patch: suggestion.json_patch.clone(),
                })
                .collect();
            Row {
                severity,
                code: diagnostic.code.clone(),
                pointer: diagnostic.path.clone(),
                sentence,
                message: diagnostic.message.clone(),
                fixes,
            }
        })
        .collect()
}

/// Which answer the rows on screen came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Nothing has been checked.
    None,
    /// Load refused the file; the rows say why, and the editor's text is unchanged.
    Refused,
    /// QUICK, for the text on screen.
    Quick,
    /// FULL, for the text on screen.
    Full,
    /// `submit_plan`'s report, which is FULL.
    Submitted,
}

impl Verdict {
    /// The name GDScript reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Refused => "refused",
            Self::Quick => "quick",
            Self::Full => "full",
            Self::Submitted => "submitted",
        }
    }
}

/// The route as the gateway priced it: where each leg ends and what it costs.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Route {
    /// The commander's voxel, then where each leg ends; sim axes.
    pub points: Vec<[i32; 3]>,
    /// Each leg's travel, in game milliseconds, exactly as the gateway answered.
    pub legs: Vec<i64>,
    /// The whole route's travel, in game milliseconds, as the gateway answered.
    pub whole: i64,
    /// Whether the connectivity oracle found a route at all.
    pub reachable: bool,
    /// Whether this route describes the text on screen.
    pub current: bool,
    /// Whether every leg named where it ends. When one did not, `points` is empty and no
    /// polyline is drawn, rather than one to a point the gateway never named.
    pub readable: bool,
}

/// The placement ghost's verdict (spec section 13, "a live legality ghost").
///
/// PLACEHOLDER: the ghost is per click at the skeleton, not live on hover (plan T19
/// amendment, `skeleton-plan-w6-notes.md` A4); a live-on-hover legality ghost is S3/S6's
/// (A4 item 5). OWNER, S3/S6.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GhostState {
    /// The patched draft is being checked.
    Waiting,
    /// QUICK found nothing wrong with the placed step.
    Legal,
    /// QUICK found an error in the placed step.
    Illegal,
}

impl GhostState {
    /// The name GDScript reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Legal => "legal",
            Self::Illegal => "illegal",
        }
    }
}

/// Where a beacon would go, and what QUICK said about the draft with it placed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ghost {
    /// The voxel, sim axes.
    pub at: [i32; 3],
    /// The verdict.
    pub state: GhostState,
    /// The verifier's sentence for the first error in the placed step, when there is one.
    pub sentence: String,
}

/// One of the seat's drafts, as `list_drafts` summarises it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DraftRow {
    /// The draft's id.
    pub id: String,
    /// Its label.
    pub label: String,
    /// The round it was saved in.
    pub round: u32,
}

/// What the last `submit_plan` said.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Submitted {
    /// Whether the submission is now the seat's sealed order.
    pub accepted: bool,
    /// Whether FULL qualified it.
    pub qualifies: bool,
}

/// What the editor last had to tell the player that is not a row: a key GDScript looks up
/// in the string table, and the detail that goes into it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Status {
    /// The string table's key; empty when there is nothing to say.
    pub key: String,
    /// What fills the sentence: a code and pointer, a gateway refusal, a count.
    pub detail: String,
}

/// A placement that has been patched into a copy of the draft and not yet applied.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Preview {
    at: [i32; 3],
    base: u64,
    step: Option<usize>,
    patched: String,
    inverse: String,
    /// QUICK's rows for the patched draft, and whether it qualified, once answered.
    checked: Option<(Vec<Row>, bool)>,
}

/// One planning call the editor owes, before it is sent.
///
/// A map action and a placement preview are kept as what the player asked for, not as a
/// patch: the new step's label and the route index it lands at are read off the text the
/// patch is applied to, which is the text when the job is rendered, after every edit queued
/// ahead of it has landed. Two quick clicks therefore get two different labels.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Job {
    Load {
        candidate: String,
    },
    /// A Fix button's patch, the verifier's own.
    Edit {
        patch: String,
    },
    Map {
        action: Action,
        target: Target,
    },
    Undo,
    Preview {
        at: [i32; 3],
    },
    PreviewCheck,
    Submit,
    Briefing,
    Drafts,
    Notes {
        notes: String,
    },
    SaveDraft {
        label: String,
    },
    Beacon {
        id: String,
    },
    /// `list_templates`.
    Templates,
    /// `instantiate_template` for the wizard's newest ask, rendered from the wizard as it
    /// is when the job goes out.
    Instantiate,
    /// The wizard's playbook, put into the editor as one edit.
    UseWizard {
        text: String,
    },
    /// `get_draft`: open the draft `id` (the carried one) as the text.
    GetDraft {
        id: String,
    },
}

impl Job {
    /// Whether this job changes the text, so it goes before the checks of the text.
    const fn is_edit(&self) -> bool {
        matches!(
            self,
            Self::Load { .. }
                | Self::Edit { .. }
                | Self::Map { .. }
                | Self::Undo
                | Self::Preview { .. }
                | Self::PreviewCheck
                | Self::UseWizard { .. }
                | Self::GetDraft { .. }
        )
    }
}

/// What Undo takes back.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Undo {
    /// The inverse patch `patch_plan` handed back for an edit.
    Patch(String),
    /// The text as it was before the wizard's playbook replaced it (`None`: there was no
    /// text), restored as it was, byte for byte, with no call.
    Restore(Option<String>),
}

/// The call in flight, with what the answer is needed for.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Sent {
    Load {
        candidate: String,
    },
    Edit {
        base: u64,
        /// The job again, for a connection that drops before the answer.
        retry: Box<Job>,
    },
    Undo {
        base: u64,
    },
    Preview {
        at: [i32; 3],
        base: u64,
        step: Option<usize>,
    },
    PreviewCheck,
    Quick {
        revision: u64,
    },
    Full {
        revision: u64,
    },
    Estimate {
        revision: u64,
    },
    Submit {
        text: String,
        revision: u64,
    },
    Briefing,
    Drafts,
    Notes {
        notes: String,
    },
    SaveDraft,
    Beacon {
        id: String,
    },
    Render {
        revision: u64,
    },
    Templates,
    Instantiate {
        asked: u64,
    },
    GetDraft {
        id: String,
        round: u32,
    },
}

/// One call to send: the method and its params.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Call {
    /// The gateway's wire spelling.
    pub method: &'static str,
    /// The params object.
    pub params: Json,
    /// Whether this call is the FULL the idle timer owed, so the rig can clear it.
    pub full: bool,
}

/// The checks the editor owes the text on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Owed {
    /// QUICK for the newest text.
    quick: bool,
    /// The route estimate for the newest text.
    estimate: bool,
    /// An edit landed, so the rig restarts the idle timer that owes FULL.
    edited: bool,
}

/// What the editor has read from the gateway at least once.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Known {
    notes: bool,
    drafts: bool,
    templates: bool,
}

/// The editor: the draft's text, its undo stack, the queue of calls it owes, and what the
/// gateway last said about it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Editor {
    seat: String,
    text: Option<String>,
    revision: u64,
    undo: Vec<Undo>,
    queue: VecDeque<Job>,
    in_flight: Option<Sent>,
    owed: Owed,
    full_done: Option<u64>,
    rows: Vec<Row>,
    verdict: Option<Verdict>,
    rows_revision: u64,
    qualifies: bool,
    route: Route,
    commander: Option<[i32; 3]>,
    preview: Option<Preview>,
    ghost: Option<Ghost>,
    notes: String,
    notes_saved: Option<u32>,
    drafts: Vec<DraftRow>,
    known: Known,
    sealed: Option<String>,
    submitted: Option<Submitted>,
    round: u32,
    carried_round: u32,
    beacon_prose: String,
    status: Status,
    refusals: u32,
    changes: u64,
    templates: Vec<TemplateRow>,
    wizard: Option<Wizard>,
    /// Every wizard ask so far, so an answer to an older ask (of any template) is known.
    wizard_asks: u64,
    /// The rule list: `render_plan`'s lines for the text at `prose_revision`.
    prose: Vec<String>,
    prose_revision: Option<u64>,
    /// The rule list (`render_plan`) is owed for the newest text.
    render_owed: bool,
}

impl Editor {
    /// An editor with no text, playing `seat` (spelt as the gateway spells a seat).
    #[must_use]
    pub fn new(seat: &str) -> Self {
        Self {
            seat: seat.to_owned(),
            ..Self::default()
        }
    }

    /// The seat this editor plays.
    pub fn set_seat(&mut self, seat: &str) {
        seat.clone_into(&mut self.seat);
        self.touch();
    }

    // --- What the player asks for -------------------------------------------------------

    /// Load a file's bytes. Returns `false` when the bytes are not UTF-8 text, which no
    /// playbook can be; otherwise the file is checked by QUICK before the editor takes it.
    pub fn load(&mut self, bytes: &[u8]) -> bool {
        let Ok(candidate) = String::from_utf8(bytes.to_vec()) else {
            self.say("load_not_text", "");
            return false;
        };
        self.queue.push_back(Job::Load { candidate });
        self.touch();
        true
    }

    /// The text the editor holds, as bytes: what Save writes. Empty with no text.
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        self.text
            .as_ref()
            .map(|text| text.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// The text the last accepted submission sealed, as bytes; empty before one.
    #[must_use]
    pub fn sealed_bytes(&self) -> Vec<u8> {
        self.sealed
            .as_ref()
            .map(|text| text.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// A map action. Returns whether it was taken: an action needs text to act on, and a
    /// target it can name (a visit or a recycle names a beacon, a placement names ground).
    pub fn act(&mut self, action: Action, target: &Target) -> bool {
        if self.text.is_none() {
            self.say("no_playbook", "");
            return false;
        }
        if action == Action::Place {
            if let Target::Voxel(at) = target {
                return self.place(*at);
            }
            return false;
        }
        // Whether the action can name this target at all. The label is chosen later,
        // against the text the patch is applied to (see [`Job`]).
        if step_value(action, target, "").is_none() {
            return false;
        }
        self.queue.push_back(Job::Map {
            action,
            target: target.clone(),
        });
        self.touch();
        true
    }

    /// Show the placement ghost at `at`: patch a beacon into a copy of the draft and ask
    /// QUICK about it, without applying it. Per click, not live on hover (T19 amendment;
    /// see [`GhostState`]'s PLACEHOLDER). The copy is patched against the text as it is
    /// when the preview goes out, after every edit queued ahead of it.
    pub fn preview_place(&mut self, at: [i32; 3]) -> bool {
        if self.text.is_none() {
            self.say("no_playbook", "");
            return false;
        }
        self.queue
            .retain(|job| !matches!(job, Job::Preview { .. } | Job::PreviewCheck));
        self.queue.push_back(Job::Preview { at });
        self.preview = None;
        self.ghost = Some(Ghost {
            at,
            state: GhostState::Waiting,
            sentence: String::new(),
        });
        self.touch();
        true
    }

    /// Place a beacon at `at`. When the ghost at `at` was checked against the text on
    /// screen and no edit is waiting ahead of it, its patched draft is taken as it is — no
    /// second call — and its QUICK report becomes the rows; otherwise the placement is
    /// patched like any other map action, in its turn.
    fn place(&mut self, at: [i32; 3]) -> bool {
        let ready = !self.busy()
            && self.preview.as_ref().is_some_and(|preview| {
                preview.at == at && preview.base == self.revision && preview.checked.is_some()
            });
        if ready {
            if let Some(preview) = self.preview.take() {
                self.undo.push(Undo::Patch(preview.inverse));
                self.accept_text(preview.patched);
                if let Some((rows, qualifies)) = preview.checked {
                    self.rows = rows;
                    self.verdict = Some(Verdict::Quick);
                    self.rows_revision = self.revision;
                    self.qualifies = qualifies;
                    self.owed.quick = false;
                }
                return true;
            }
        }
        if self.text.is_none() {
            return false;
        }
        self.queue.push_back(Job::Map {
            action: Action::Place,
            target: Target::Voxel(at),
        });
        self.touch();
        true
    }

    /// Apply the `fix`th Fix of row `row`. Refused while the rows describe an older text
    /// than the one on screen, because the verifier's patch points into the text it saw.
    pub fn fix(&mut self, row: usize, fix: usize) -> bool {
        if !self.rows_current() || self.busy() {
            return false;
        }
        let Some(patch) = self
            .rows
            .get(row)
            .and_then(|row| row.fixes.get(fix))
            .map(|fix| fix.patch.clone())
        else {
            return false;
        };
        self.queue.push_back(Job::Edit { patch });
        self.touch();
        true
    }

    /// Undo the last edit, with the inverse patch `patch_plan` handed back for it.
    pub fn undo(&mut self) -> bool {
        if self.undo.is_empty() {
            return false;
        }
        self.queue.push_back(Job::Undo);
        self.touch();
        true
    }

    /// Submit the text on screen (spec section 12: `submit_plan` always runs FULL).
    pub fn submit(&mut self) -> bool {
        if self.text.is_none() {
            self.say("no_playbook", "");
            return false;
        }
        self.queue.push_back(Job::Submit);
        self.touch();
        true
    }

    /// Save the notes box through `save_notes`.
    pub fn save_notes(&mut self, notes: &str) {
        self.queue.push_back(Job::Notes {
            notes: notes.to_owned(),
        });
        self.touch();
    }

    /// Keep the text on screen as the seat's editor draft, through `save_draft`.
    pub fn save_draft(&mut self, label: &str) -> bool {
        if self.text.is_none() {
            self.say("no_playbook", "");
            return false;
        }
        self.queue.push_back(Job::SaveDraft {
            label: label.to_owned(),
        });
        self.touch();
        true
    }

    /// Ask the gateway about one beacon, for the map menu's heading
    /// (`skeleton-plan-t16a-notes.md` section B, "T19" (4): a click maps onto `get_beacon`
    /// through the beacon's `b_NN` id).
    pub fn describe_beacon(&mut self, id: &str) {
        self.beacon_prose.clear();
        self.queue.retain(|job| !matches!(job, Job::Beacon { .. }));
        self.queue.push_back(Job::Beacon { id: id.to_owned() });
        self.touch();
    }

    // --- The wizard (T19, pull request 2) -------------------------------------------------

    /// Ask for the template list again (it is read once, at the first Lull, on its own).
    pub fn list_templates(&mut self) {
        self.queue.retain(|job| !matches!(job, Job::Templates));
        self.queue.push_back(Job::Templates);
        self.touch();
    }

    /// Open the wizard on `template_id`: `instantiate_template{suggested: true}` with no
    /// explicit value, whose answer's parameters are the pages.
    pub fn wizard_open(&mut self, template_id: &str) {
        self.wizard_asks = self.wizard_asks.wrapping_add(1);
        self.wizard = Some(Wizard::new(template_id, self.wizard_asks));
        self.ask_wizard();
    }

    /// The player typed `text` on the page at `pointer`: it is sent exactly as typed, as an
    /// explicit value, with `suggested` still true, so every other page keeps the
    /// operator's value and mark. Returns whether that page is on screen.
    pub fn wizard_set(&mut self, pointer: &str, text: &str) -> bool {
        let asks = self.wizard_asks.wrapping_add(1);
        let Some(wizard) = self.wizard.as_mut() else {
            return false;
        };
        if !wizard.set(pointer, text) {
            return false;
        }
        wizard.asked = asks;
        wizard.refusal = None;
        self.wizard_asks = asks;
        self.ask_wizard();
        true
    }

    /// Put the wizard's playbook into the editor, byte for byte, as one edit Undo takes
    /// back. Refused while the pages on screen do not answer the newest ask.
    pub fn wizard_use(&mut self) -> bool {
        let text = match self.wizard.as_ref() {
            Some(wizard) if wizard.current() => wizard
                .instance
                .as_ref()
                .map(|instance| instance.playbook_jsonc.clone()),
            _ => None,
        };
        let waiting = self.queue.iter().any(|job| matches!(job, Job::Instantiate))
            || matches!(self.in_flight, Some(Sent::Instantiate { .. }));
        let Some(text) = text.filter(|_| !waiting) else {
            return false;
        };
        self.queue.push_back(Job::UseWizard { text });
        self.touch();
        true
    }

    /// Close the wizard; nothing it showed is kept.
    pub fn wizard_close(&mut self) {
        self.wizard = None;
        self.queue.retain(|job| !matches!(job, Job::Instantiate));
        self.touch();
    }

    fn ask_wizard(&mut self) {
        self.queue.retain(|job| !matches!(job, Job::Instantiate));
        self.queue.push_back(Job::Instantiate);
        self.touch();
    }

    // --- What the rig tells the editor --------------------------------------------------

    /// A Lull opened, in `round`. The notes and the drafts are read again, the template list
    /// the first time, and a text the editor already holds is checked and rendered again
    /// against the new snapshot; an open wizard asks again, because the operator's
    /// suggestion is made afresh at each Lull's start.
    pub fn lull_opened(&mut self, round: u32) {
        let new_round = round != self.round;
        self.round = round;
        if !self.known.notes {
            self.queue.push_back(Job::Briefing);
        }
        if !self.known.templates {
            self.queue.push_back(Job::Templates);
        }
        self.queue.push_back(Job::Drafts);
        if new_round && self.text.is_some() {
            self.owed.quick = true;
            self.owed.estimate = true;
            self.render_owed = true;
            self.full_done = None;
            self.owed.edited = true;
        }
        if new_round && self.wizard.is_some() {
            let asks = self.wizard_asks.wrapping_add(1);
            self.wizard_asks = asks;
            if let Some(wizard) = self.wizard.as_mut() {
                wizard.asked = asks;
                wizard.refusal = None;
            }
            self.ask_wizard();
        }
        self.touch();
    }

    /// The view's latest complete entity list: the editor finds its own commander in it,
    /// because every route starts where the commander stands.
    pub fn set_entities(&mut self, entities: &[Entity]) {
        let commander = entities
            .iter()
            .find(|entity| {
                entity.kind == EntityKind::Unit
                    && entity.subtype == "commander"
                    && entity.owner == self.seat
            })
            .map(|entity| entity.at);
        if commander != self.commander {
            self.commander = commander;
            if self.text.is_some() {
                self.owed.estimate = true;
            }
            self.touch();
        }
    }

    /// The call to send next, if any. `full_due` is the pacer's idle timer.
    ///
    /// The rig calls this only in a Lull, with the seat connection idle and budget left.
    pub fn next_call(&mut self, full_due: bool) -> Option<Call> {
        if self.in_flight.is_some() {
            return None;
        }
        // 1. The edits, in order.
        while self.queue.front().is_some_and(Job::is_edit) {
            let job = self.queue.pop_front()?;
            if let Some(call) = self.render(job) {
                return Some(call);
            }
        }
        // 2. QUICK for the newest text.
        if self.owed.quick {
            self.owed.quick = false;
            if let Some(text) = self.text.clone() {
                let revision = self.revision;
                return Some(self.send(
                    Sent::Quick { revision },
                    "verify_plan",
                    verify_params(&text, "quick"),
                ));
            }
        }
        // 3. The route, priced.
        if self.owed.estimate {
            self.owed.estimate = false;
            if let Some(call) = self.estimate_call() {
                return Some(call);
            }
        }
        // 4. The rule list, for the newest text.
        if self.render_owed {
            self.render_owed = false;
            if let Some(text) = self.text.clone() {
                let revision = self.revision;
                return Some(self.send(
                    Sent::Render { revision },
                    "render_plan",
                    object(vec![("playbook_jsonc", Json::String(text))]),
                ));
            }
        }
        // 5. Everything else, in order.
        while let Some(job) = self.queue.pop_front() {
            if let Some(call) = self.render(job) {
                return Some(call);
            }
        }
        // 6. FULL, once the editor has been left alone.
        if full_due && self.full_done != Some(self.revision) {
            if let Some(text) = self.text.clone() {
                let revision = self.revision;
                let mut call = self.send(
                    Sent::Full { revision },
                    "verify_plan",
                    verify_params(&text, "full"),
                );
                call.full = true;
                return Some(call);
            }
        }
        None
    }

    /// Whether anything the player asked for is still waiting or in flight — the thing Ready
    /// waits behind, so a Ready pressed after Submit reaches the gateway after the submit.
    #[must_use]
    pub fn has_pending_jobs(&self) -> bool {
        !self.queue.is_empty() || self.in_flight.is_some()
    }

    /// Whether an edit is waiting or in flight.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.queue.iter().any(Job::is_edit)
            || matches!(
                self.in_flight,
                Some(
                    Sent::Load { .. }
                        | Sent::Edit { .. }
                        | Sent::Undo { .. }
                        | Sent::Preview { .. }
                        | Sent::PreviewCheck
                        | Sent::GetDraft { .. }
                )
            )
    }

    /// Whether the checks of the text on screen are all answered: no edit, QUICK, route
    /// estimate or rule list waiting.
    #[must_use]
    pub fn settled(&self) -> bool {
        !self.busy()
            && !self.owed.quick
            && !self.owed.estimate
            && !self.render_owed
            && self.in_flight.is_none()
    }

    /// Whether an edit landed since the last call: the rig restarts the idle timer.
    pub fn take_edit_mark(&mut self) -> bool {
        std::mem::take(&mut self.owed.edited)
    }

    /// The answer to the call in flight: a result, or the gateway's refusal as
    /// `(code, message)`.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Schema`] when a result is not the message its method answers with;
    /// the call is settled either way and nothing waits on it.
    pub fn answered(&mut self, answer: Result<&Json, (&str, &str)>) -> Result<(), BridgeError> {
        let Some(sent) = self.in_flight.take() else {
            return Ok(());
        };
        self.touch();
        match answer {
            Ok(result) => self.settle(sent, result),
            Err((code, message)) => {
                self.refused(&sent, code, message);
                Ok(())
            }
        }
    }

    /// The seat connection dropped with a call in flight: an edit or a request the player
    /// made is asked again once it is back; a check is owed again.
    ///
    /// Asking again is safe for every one of them: `patch_plan` is stateless on the gateway
    /// (it hands back the patched text and keeps nothing), the text on screen is still the
    /// one the lost edit was patched against, and `save_notes` replaces the notebook whole.
    pub fn dropped(&mut self) {
        let Some(sent) = self.in_flight.take() else {
            return;
        };
        match sent {
            Sent::Load { candidate } => self.queue.push_front(Job::Load { candidate }),
            Sent::Edit { base, retry } => {
                if base == self.revision {
                    self.queue.push_front(*retry);
                }
            }
            Sent::Undo { base } => {
                if base == self.revision {
                    self.queue.push_front(Job::Undo);
                }
            }
            Sent::Quick { .. } => self.owed.quick = true,
            Sent::Estimate { .. } => self.owed.estimate = true,
            Sent::Full { .. } => self.owed.edited = true,
            Sent::Submit { .. } => self.queue.push_front(Job::Submit),
            Sent::Briefing => self.queue.push_front(Job::Briefing),
            Sent::Drafts | Sent::SaveDraft => self.queue.push_front(Job::Drafts),
            Sent::Preview { at, .. } => {
                if self.ghost.as_ref().is_some_and(|ghost| ghost.at == at) {
                    self.queue.push_front(Job::Preview { at });
                }
            }
            Sent::PreviewCheck => {
                if self.preview.is_some() {
                    self.queue.push_front(Job::PreviewCheck);
                }
            }
            Sent::Notes { notes } => self.queue.push_front(Job::Notes { notes }),
            Sent::Beacon { id } => self.queue.push_front(Job::Beacon { id }),
            Sent::Render { .. } => self.render_owed = true,
            Sent::Templates => self.queue.push_front(Job::Templates),
            Sent::Instantiate { asked } => {
                if self
                    .wizard
                    .as_ref()
                    .is_some_and(|wizard| wizard.asked == asked)
                {
                    self.queue.push_front(Job::Instantiate);
                }
            }
            Sent::GetDraft { id, round } => {
                if round == self.round {
                    self.queue.push_front(Job::GetDraft { id });
                }
            }
        }
        self.touch();
    }

    // --- What GDScript reads ------------------------------------------------------------

    /// Whether the editor holds a playbook.
    #[must_use]
    pub const fn has_text(&self) -> bool {
        self.text.is_some()
    }

    /// The text's revision: one more for every edit that landed.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// How many edits Undo can take back.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// The rows on screen.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Which answer the rows came from.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        self.verdict.unwrap_or(Verdict::None)
    }

    /// Whether the rows describe the text on screen.
    #[must_use]
    pub fn rows_current(&self) -> bool {
        self.verdict
            .is_some_and(|verdict| verdict != Verdict::Refused)
            && self.rows_revision == self.revision
    }

    /// Whether the last report for the text on screen qualified it.
    #[must_use]
    pub fn qualifies(&self) -> bool {
        self.rows_current() && self.qualifies
    }

    /// The route, as last priced.
    #[must_use]
    pub const fn route(&self) -> &Route {
        &self.route
    }

    /// The placement ghost, if one is showing.
    #[must_use]
    pub const fn ghost(&self) -> Option<&Ghost> {
        self.ghost.as_ref()
    }

    /// The notebook as the gateway last returned it.
    #[must_use]
    pub fn notes(&self) -> &str {
        &self.notes
    }

    /// Whether the notebook has been read.
    #[must_use]
    pub const fn notes_known(&self) -> bool {
        self.known.notes
    }

    /// The characters the gateway stored at the last `save_notes`, in its own count.
    #[must_use]
    pub const fn notes_saved(&self) -> Option<u32> {
        self.notes_saved
    }

    /// The seat's drafts, as `list_drafts` last listed them.
    #[must_use]
    pub fn drafts(&self) -> &[DraftRow] {
        &self.drafts
    }

    /// Whether the drafts have been listed.
    #[must_use]
    pub const fn drafts_known(&self) -> bool {
        self.known.drafts
    }

    /// What the last submission said.
    #[must_use]
    pub const fn submitted(&self) -> Option<Submitted> {
        self.submitted
    }

    /// What `get_beacon` said about the beacon last clicked.
    #[must_use]
    pub fn beacon_prose(&self) -> &str {
        &self.beacon_prose
    }

    /// The status line.
    #[must_use]
    pub const fn status(&self) -> &Status {
        &self.status
    }

    /// How many of the editor's calls the gateway refused.
    #[must_use]
    pub const fn refusals(&self) -> u32 {
        self.refusals
    }

    /// A counter that moves whenever anything above changed, so a view redraws only then.
    #[must_use]
    pub const fn changes(&self) -> u64 {
        self.changes
    }

    /// The templates `list_templates` offered, in its order.
    #[must_use]
    pub fn templates(&self) -> &[TemplateRow] {
        &self.templates
    }

    /// Whether the template list has been read.
    #[must_use]
    pub const fn templates_known(&self) -> bool {
        self.known.templates
    }

    /// The wizard, when it is open.
    #[must_use]
    pub const fn wizard(&self) -> Option<&Wizard> {
        self.wizard.as_ref()
    }

    /// The rule list: `render_plan`'s lines, as they came.
    #[must_use]
    pub fn prose(&self) -> &[String] {
        &self.prose
    }

    /// Whether the rule list describes the text on screen.
    #[must_use]
    pub fn prose_current(&self) -> bool {
        self.text.is_some() && self.prose_revision == Some(self.revision)
    }

    // --- Inside ----------------------------------------------------------------------

    fn touch(&mut self) {
        self.changes = self.changes.wrapping_add(1);
    }

    fn say(&mut self, key: &str, detail: &str) {
        self.status = Status {
            key: key.to_owned(),
            detail: detail.to_owned(),
        };
        self.touch();
    }

    fn send(&mut self, sent: Sent, method: &'static str, params: Json) -> Call {
        self.in_flight = Some(sent);
        Call {
            method,
            params,
            full: false,
        }
    }

    /// One queued job as the call it becomes, against the text as it is now. `None` for a
    /// job with nothing to act on any more (an Undo with an empty stack, a preview whose
    /// patch never came back).
    fn render(&mut self, job: Job) -> Option<Call> {
        let current = self.text.clone();
        let revision = self.revision;
        Some(match job {
            Job::Load { candidate } => {
                let params = verify_params(&candidate, "quick");
                self.send(Sent::Load { candidate }, "verify_plan", params)
            }
            Job::Edit { patch } => {
                let playbook = current?;
                let params = patch_params(&playbook, &patch);
                self.send(
                    Sent::Edit {
                        base: revision,
                        retry: Box::new(Job::Edit { patch }),
                    },
                    "patch_plan",
                    params,
                )
            }
            Job::Map { action, target } => {
                let playbook = current?;
                let label = fresh_label(&playbook, action.stem());
                let step = step_value(action, &target, &label)?;
                let params = patch_params(&playbook, &append_patch(step));
                self.send(
                    Sent::Edit {
                        base: revision,
                        retry: Box::new(Job::Map { action, target }),
                    },
                    "patch_plan",
                    params,
                )
            }
            Job::Undo => return self.render_undo(),
            Job::Preview { at } => {
                let playbook = current?;
                let label = fresh_label(&playbook, Action::Place.stem());
                let patch = append_patch(step_value(Action::Place, &Target::Voxel(at), &label)?);
                let step = route_len(&playbook);
                self.send(
                    Sent::Preview {
                        at,
                        base: revision,
                        step,
                    },
                    "patch_plan",
                    patch_params(&playbook, &patch),
                )
            }
            Job::PreviewCheck => {
                let patched = self.preview.as_ref()?.patched.clone();
                self.send(
                    Sent::PreviewCheck,
                    "verify_plan",
                    verify_params(&patched, "quick"),
                )
            }
            Job::Submit => {
                let playbook = current?;
                let params = object(vec![("playbook_jsonc", Json::String(playbook.clone()))]);
                self.send(
                    Sent::Submit {
                        text: playbook,
                        revision,
                    },
                    "submit_plan",
                    params,
                )
            }
            Job::Briefing => self.send(Sent::Briefing, "get_briefing", object(Vec::new())),
            Job::Drafts => self.send(Sent::Drafts, "list_drafts", object(Vec::new())),
            Job::Notes { notes } => {
                let params = object(vec![("notes", Json::String(notes.clone()))]);
                self.send(Sent::Notes { notes }, "save_notes", params)
            }
            Job::SaveDraft { label } => {
                let playbook = current?;
                self.send(
                    Sent::SaveDraft,
                    "save_draft",
                    object(vec![
                        ("playbook_jsonc", Json::String(playbook)),
                        ("label", Json::String(label)),
                        ("draft_id", text(EDITOR_DRAFT_ID)),
                    ]),
                )
            }
            Job::Beacon { id } => {
                let params = object(vec![("beacon_id", Json::String(id.clone()))]);
                self.send(Sent::Beacon { id }, "get_beacon", params)
            }
            job @ (Job::Templates
            | Job::Instantiate
            | Job::UseWizard { .. }
            | Job::GetDraft { .. }) => return self.render_pr2(job),
        })
    }

    /// Undo: the inverse patch through `patch_plan`, or, for the wizard's playbook, the text
    /// before it put back as it was, with no call.
    fn render_undo(&mut self) -> Option<Call> {
        let patch = match self.undo.last()? {
            Undo::Patch(patch) => patch.clone(),
            Undo::Restore(_) => {
                if let Some(Undo::Restore(before)) = self.undo.pop() {
                    self.drop_settled_ghost();
                    self.restore(before);
                }
                return None;
            }
        };
        let playbook = self.text.clone()?;
        let revision = self.revision;
        Some(self.send(
            Sent::Undo { base: revision },
            "patch_plan",
            patch_params(&playbook, &patch),
        ))
    }

    /// Pull request 2's jobs: the template list, the wizard's instantiation, its playbook
    /// put into the editor, and the carried draft fetched.
    fn render_pr2(&mut self, job: Job) -> Option<Call> {
        match job {
            Job::Templates => {
                Some(self.send(Sent::Templates, "list_templates", object(Vec::new())))
            }
            Job::Instantiate => {
                let wizard = self.wizard.as_ref()?;
                let (asked, params) = (wizard.asked, wizard.params());
                Some(self.send(Sent::Instantiate { asked }, "instantiate_template", params))
            }
            Job::UseWizard { text } => {
                let before = self.text.clone();
                self.undo.push(Undo::Restore(before));
                self.drop_settled_ghost();
                self.accept_text(text);
                self.say("wizard_used", "");
                None
            }
            Job::GetDraft { id } => {
                let round = self.round;
                let params = object(vec![("draft_id", Json::String(id.clone()))]);
                Some(self.send(Sent::GetDraft { id, round }, "get_draft", params))
            }
            other => self.render(other),
        }
    }

    /// The route estimate for the text on screen, when there is a route to price and a
    /// commander to start it from.
    fn estimate_call(&mut self) -> Option<Call> {
        let text = self.text.as_ref()?;
        let commander = self.commander?;
        let targets = route_waypoints(text)?;
        if targets.is_empty() {
            self.route = Route {
                points: vec![commander],
                current: true,
                reachable: true,
                readable: true,
                ..Route::default()
            };
            self.touch();
            return None;
        }
        let mut waypoints = Vec::with_capacity(targets.len().saturating_add(1));
        waypoints.push(voxel_location(commander));
        waypoints.extend(targets);
        let revision = self.revision;
        Some(self.send(
            Sent::Estimate { revision },
            "estimate_route",
            object(vec![("waypoints", Json::Array(waypoints))]),
        ))
    }

    fn settle(&mut self, sent: Sent, result: &Json) -> Result<(), BridgeError> {
        match sent {
            Sent::Load { candidate } => self.settle_load(candidate, &report_of(result)?),
            Sent::Edit { base, retry: _ } => {
                let answer = patch_of(result)?;
                if base == self.revision {
                    self.undo.push(Undo::Patch(answer.inverse_json_patch));
                    self.drop_settled_ghost();
                    self.accept_text(answer.playbook_jsonc);
                }
            }
            Sent::Undo { base } => {
                let answer = patch_of(result)?;
                if base == self.revision {
                    self.undo.pop();
                    self.drop_settled_ghost();
                    self.accept_text(answer.playbook_jsonc);
                }
            }
            Sent::Preview { at, base, step } => {
                let answer = patch_of(result)?;
                if self.ghost.as_ref().is_some_and(|ghost| ghost.at == at) {
                    self.preview = Some(Preview {
                        at,
                        base,
                        step,
                        patched: answer.playbook_jsonc,
                        inverse: answer.inverse_json_patch,
                        checked: None,
                    });
                    self.queue.push_front(Job::PreviewCheck);
                }
            }
            Sent::PreviewCheck => {
                let report = report_of(result)?;
                if let Some(preview) = self.preview.as_mut() {
                    let (state, sentence) = placement_verdict(&report, preview.step);
                    self.ghost = Some(Ghost {
                        at: preview.at,
                        state,
                        sentence,
                    });
                    preview.checked = Some((rows_of(&report), report.qualifies));
                }
            }
            Sent::Quick { revision } => {
                let report = report_of(result)?;
                if revision == self.revision {
                    self.take_rows(&report, Verdict::Quick);
                }
            }
            Sent::Full { revision } => {
                let report = report_of(result)?;
                if revision == self.revision {
                    self.take_rows(&report, Verdict::Full);
                    self.full_done = Some(revision);
                }
            }
            Sent::Estimate { revision } => {
                if revision == self.revision {
                    self.take_route(result);
                }
            }
            Sent::Submit { text, revision } => {
                let response: SubmitPlanResponse =
                    json::decode_json(&body(result, "gp.api.v1.SubmitPlanResponse"))?;
                self.settle_submit(text, revision, response);
            }
            Sent::Briefing => {
                let response: GetBriefingResponse =
                    json::decode_json(&body(result, "gp.api.v1.GetBriefingResponse"))?;
                self.notes = response.notes;
                self.known.notes = true;
            }
            Sent::Notes { .. } => {
                let response: SaveNotesResponse =
                    json::decode_json(&body(result, "gp.api.v1.SaveNotesResponse"))?;
                self.notes_saved = Some(response.characters);
                self.say("notes_saved", &response.characters.to_string());
            }
            Sent::Drafts => {
                let response: ListDraftsResponse =
                    json::decode_json(&body(result, "gp.api.v1.ListDraftsResponse"))?;
                self.settle_drafts(response);
            }
            Sent::SaveDraft => {
                self.queue.push_back(Job::Drafts);
                self.say("draft_saved", "");
            }
            Sent::Beacon { .. } => {
                let response: GetBeaconResponse =
                    json::decode_json(&body(result, "gp.api.v1.GetBeaconResponse"))?;
                self.beacon_prose = response.prose;
            }
            sent @ (Sent::Render { .. }
            | Sent::Templates
            | Sent::Instantiate { .. }
            | Sent::GetDraft { .. }) => self.settle_pr2(sent, result)?,
        }
        Ok(())
    }

    /// Pull request 2's answers: the rule list, the template list, the wizard's
    /// instantiation and the carried draft.
    fn settle_pr2(&mut self, sent: Sent, result: &Json) -> Result<(), BridgeError> {
        match sent {
            Sent::Render { revision } => {
                let prose = wizard::prose_of(result)?;
                if revision == self.revision {
                    self.prose = prose;
                    self.prose_revision = Some(revision);
                }
            }
            Sent::Templates => {
                self.templates = wizard::templates_of(result)?;
                self.known.templates = true;
            }
            Sent::Instantiate { asked } => {
                let instance: Instance = wizard::instance_of(result)?;
                if let Some(wizard) = self.wizard.as_mut() {
                    if wizard.asked == asked {
                        wizard.instance = Some(instance);
                        wizard.answered = asked;
                        wizard.refusal = None;
                    }
                }
            }
            Sent::GetDraft { id: _, round } => {
                let response: GetDraftResponse =
                    json::decode_json(&body(result, "gp.api.v1.GetDraftResponse"))?;
                if round == self.round {
                    self.undo.clear();
                    self.preview = None;
                    self.ghost = None;
                    self.accept_text(response.playbook_jsonc);
                    self.say("carried", &response.label);
                }
            }
            other => return self.settle(other, result),
        }
        Ok(())
    }

    /// Load's QUICK answer: refuse the file with the code and the pointer, or open it.
    fn settle_load(&mut self, candidate: String, report: &VerifyReport) {
        let refusal = report.diagnostics.iter().find(|diagnostic| {
            diagnostic.severity() == Severity::Error
                && LOAD_REFUSALS.contains(&diagnostic.code.as_str())
        });
        if let Some(refusal) = refusal {
            let detail = format!("{} {}", refusal.code, refusal.path);
            self.rows = rows_of(report);
            self.verdict = Some(Verdict::Refused);
            self.say("load_refused", &detail);
        } else {
            self.undo.clear();
            self.preview = None;
            self.ghost = None;
            self.accept_text(candidate);
            self.take_rows(report, Verdict::Quick);
            self.owed.quick = false;
            self.say("loaded", "");
        }
    }

    fn settle_drafts(&mut self, response: ListDraftsResponse) {
        self.drafts = response
            .drafts
            .into_iter()
            .map(|draft| DraftRow {
                id: draft.draft_id,
                label: draft.label,
                round: draft.round,
            })
            .collect();
        self.known.drafts = true;
        self.carry_forward();
    }

    fn settle_submit(&mut self, text: String, revision: u64, response: SubmitPlanResponse) {
        let report = response.report.unwrap_or_default();
        self.submitted = Some(Submitted {
            accepted: response.accepted,
            qualifies: report.qualifies,
        });
        if revision == self.revision {
            self.take_rows(&report, Verdict::Submitted);
            self.full_done = Some(revision);
        }
        if response.accepted {
            self.sealed = Some(text);
            self.say("submitted", "");
        } else {
            self.say("submit_refused", "");
        }
    }

    /// Draft continuity (spec section 13): "each Lull opens with last round's playbook
    /// pre-loaded as an editable draft, re-verified against the new snapshot".
    ///
    /// The gateway pre-loads the sealed playbook as the `carried` draft when the Lull opens,
    /// and `list_drafts` says it is there. The editor **fetches it with `get_draft` and opens
    /// it, every time**: one path, with the gateway's copy as the authority, whether this
    /// client submitted it or is a new process that resumed the match and holds no copy of
    /// anything (decisions-log item 112 (5); T19 pull request 2, closing pull request 1's
    /// PLACEHOLDER). It costs one more seat call per Lull. The checks run again at once
    /// against the new snapshot, because opening it is an edit.
    ///
    /// Only a `carried` draft saved in **this** round counts. The gateway carries nothing
    /// forward after a round whose playbook it filed itself (the safe playbook on a miss),
    /// and an older `carried` draft can still be listed then; opening it would call a
    /// playbook from two rounds ago "last round's". The status line names the draft by the
    /// gateway's own label.
    ///
    /// PLACEHOLDER: a resumed Lull opens `carried`, as spec section 13's continuity says,
    /// and the seat's own `editor` draft of this round is listed, not opened. Opening it when
    /// present is a preference the spec does not state; a click that opens any listed draft
    /// is a small draft browser ahead of S6's. OWNER, at S6, with the draft browser.
    fn carry_forward(&mut self) {
        let carried = self
            .drafts
            .iter()
            .any(|draft| draft.id == CARRIED_DRAFT_ID && draft.round == self.round);
        if !carried || self.carried_round == self.round {
            return;
        }
        self.carried_round = self.round;
        self.queue
            .retain(|job| !matches!(job, Job::GetDraft { .. }));
        self.queue.push_back(Job::GetDraft {
            id: CARRIED_DRAFT_ID.to_owned(),
        });
        self.touch();
    }

    /// Undo of the wizard's playbook: the text before it, as it was (`None`: no text).
    fn restore(&mut self, before: Option<String>) {
        if let Some(text) = before {
            self.accept_text(text);
            return;
        }
        self.text = None;
        self.revision = self.revision.wrapping_add(1);
        self.rows.clear();
        self.verdict = None;
        self.qualifies = false;
        self.route = Route::default();
        self.prose.clear();
        self.prose_revision = None;
        self.owed = Owed {
            edited: true,
            ..Owed::default()
        };
        self.touch();
    }

    /// An edit landed: a ghost already answered describes the text before it and goes. A
    /// ghost still waiting stays, because its preview is patched against the text as it is
    /// when the preview goes out, which is after this edit.
    fn drop_settled_ghost(&mut self) {
        if self
            .ghost
            .as_ref()
            .is_some_and(|ghost| ghost.state != GhostState::Waiting)
        {
            self.ghost = None;
            self.preview = None;
        }
    }

    /// A new text is on screen: every check of it is owed.
    fn accept_text(&mut self, text: String) {
        self.text = Some(text);
        self.revision = self.revision.wrapping_add(1);
        self.owed.quick = true;
        self.owed.estimate = true;
        self.render_owed = true;
        self.owed.edited = true;
        self.route.current = false;
        self.touch();
    }

    fn take_rows(&mut self, report: &VerifyReport, verdict: Verdict) {
        self.rows = rows_of(report);
        self.verdict = Some(verdict);
        self.rows_revision = self.revision;
        self.qualifies = report.qualifies;
        self.touch();
    }

    fn take_route(&mut self, result: &Json) {
        let (answer, _) = split_footer(result);
        let reachable = matches!(answer.get("reachable"), Some(Json::Bool(true)));
        let mut route = Route {
            reachable,
            whole: answer.get("ms").and_then(integer).unwrap_or(0),
            current: true,
            readable: true,
            ..Route::default()
        };
        if let Some(commander) = self.commander {
            route.points.push(commander);
        }
        if let Some(Json::Array(legs)) = answer.get("legs") {
            for leg in legs {
                route
                    .legs
                    .push(leg.get("ms").and_then(integer).unwrap_or(0));
                // `gp.api.v1.Leg.to` is a `gp.v1.Location`, `{"voxel":{..}}`; the gateway
                // at `main` writes the bare voxel `{"x":..}` there instead. Both spellings
                // name the same voxel, so both are read; the disagreement is the gateway's
                // to settle and is reported in this lane's pull request.
                let to = leg.get("to");
                let voxel = to.and_then(|to| to.get("voxel")).or(to);
                match voxel {
                    Some(voxel @ Json::Object(_)) => route.points.push(voxel_of(voxel)),
                    // A leg that names no end is not drawn to a point the gateway never
                    // named: the polyline goes, and the times stay.
                    _ => route.readable = false,
                }
            }
        }
        if !route.readable {
            route.points.clear();
        }
        self.route = route;
        self.touch();
    }

    fn refused(&mut self, sent: &Sent, code: &str, message: &str) {
        self.refusals = self.refusals.saturating_add(1);
        let detail = format!("{code}: {message}");
        match sent {
            Sent::Load { .. } => self.say("load_refused_by_gateway", &detail),
            Sent::Preview { .. } | Sent::PreviewCheck => {
                self.ghost = None;
                self.preview = None;
                self.say("gateway_refused", &detail);
            }
            Sent::Instantiate { asked } => {
                // Shown as the gateway wrote it; the value stays as typed, for the player
                // to change. Nothing is retried with another value.
                if let Some(wizard) = self.wizard.as_mut() {
                    if wizard.asked == *asked {
                        wizard.refusal = Some(detail.clone());
                    }
                }
                self.say("wizard_refused", &detail);
            }
            _ => self.say("gateway_refused", &detail),
        }
    }
}

/// The ghost's verdict from the patched draft's QUICK report: illegal when an error points
/// into the placed step, legal otherwise. `step` is the placed step's index in the route;
/// when the route could not be read, the report's own `qualifies` is the verdict.
fn placement_verdict(report: &VerifyReport, step: Option<usize>) -> (GhostState, String) {
    let inside = |path: &str| match step {
        Some(index) => {
            let pointer = format!("{ROUTE_POINTER}/{index}");
            path == pointer || path.starts_with(&format!("{pointer}/"))
        }
        None => true,
    };
    let found = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity() == Severity::Error && inside(&diagnostic.path));
    match found {
        Some(diagnostic) => {
            let sentence = if diagnostic.beginner.is_empty() {
                diagnostic.message.clone()
            } else {
                diagnostic.beginner.clone()
            };
            (GhostState::Illegal, sentence)
        }
        None if step.is_none() && !report.qualifies => (GhostState::Illegal, String::new()),
        None => (GhostState::Legal, String::new()),
    }
}

/// The route step one map action adds, labelled `label`. `None` when the action cannot
/// name that target: a visit or a recycle names a beacon, a placement names ground.
#[must_use]
pub fn step_value(action: Action, target: &Target, label: &str) -> Option<Json> {
    let beacon_ref = match target {
        Target::Beacon(id) => Some(object(vec![("beacon_id", Json::String(id.clone()))])),
        Target::Selector(selector) => Some(selector.beacon_ref()),
        Target::Voxel(_) => None,
    };
    let location = match target {
        Target::Voxel(at) => voxel_location(*at),
        Target::Beacon(_) | Target::Selector(_) => {
            object(vec![("beacon_anchor", beacon_ref.clone()?)])
        }
    };
    let kind = match action {
        Action::Go => ("move", object(vec![("to", location)])),
        Action::Visit(priority) => (
            "interface",
            object(vec![
                ("beacon", beacon_ref?),
                (
                    "rows",
                    Json::Array(vec![object(vec![("set_priority", text(priority.name()))])]),
                ),
            ]),
        ),
        Action::Recycle => (
            "interface",
            object(vec![
                ("beacon", beacon_ref?),
                (
                    "rows",
                    Json::Array(vec![object(vec![("recycle", object(Vec::new()))])]),
                ),
            ]),
        ),
        Action::Place => {
            let Target::Voxel(_) = target else {
                return None;
            };
            ("place_beacon", object(vec![("at", location)]))
        }
    };
    Some(object(vec![
        ("label", Json::String(label.to_owned())),
        kind,
    ]))
}

/// The RFC 6902 patch that appends `step` to the route.
#[must_use]
pub fn append_patch(step: Json) -> String {
    json::write(&Json::Array(vec![object(vec![
        ("op", text("add")),
        ("path", text(&format!("{ROUTE_POINTER}/-"))),
        ("value", step),
    ])]))
}

/// The first label `stem_1`, `stem_2`, ... that the text does not already contain as a
/// quoted string anywhere.
///
/// A text search rather than a read of the labels: a label is unique in the file if its
/// quoted spelling appears nowhere in it, so this never names a label twice — it may skip a
/// free one that a comment happens to quote, which costs nothing. The verifier still has
/// the last word (`E0105`, a name used twice).
#[must_use]
pub fn fresh_label(text: &str, stem: &str) -> String {
    let mut number: u32 = 1;
    loop {
        let label = format!("{stem}_{number}");
        if !text.contains(&format!("\"{label}\"")) {
            return label;
        }
        number = number.saturating_add(1);
        if number == u32::MAX {
            return label;
        }
    }
}

/// A JSONC text with its comments blanked out, so a plain JSON reader can read it.
///
/// Line comments (`//` to the end of the line) and block comments (`/* ... */`) outside
/// string literals become spaces, line breaks inside them are kept, and everything else —
/// string contents and escapes included — is copied as it is. Used only to see where the
/// route goes ([`route_waypoints`]); the verdict on the file is the verifier's.
#[must_use]
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    let mut in_string = false;
    while let Some(character) = characters.next() {
        if in_string {
            out.push(character);
            if character == '\\' {
                if let Some(escaped) = characters.next() {
                    out.push(escaped);
                }
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match (character, characters.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(character);
            }
            ('/', Some('/')) => {
                out.push(' ');
                for rest in characters.by_ref() {
                    if rest == '\n' {
                        out.push('\n');
                        break;
                    }
                    out.push(' ');
                }
            }
            ('/', Some('*')) => {
                out.push(' ');
                let mut last = ' ';
                for rest in characters.by_ref() {
                    out.push(if rest == '\n' { '\n' } else { ' ' });
                    if last == '*' && rest == '/' {
                        break;
                    }
                    last = rest;
                }
            }
            _ => out.push(character),
        }
    }
    out
}

/// The route's steps, read laxly out of a JSONC text.
fn route_steps(text: &str) -> Option<Vec<Json>> {
    let document = json::read(&strip_comments(text)).ok()?;
    match document
        .get("declarative")
        .and_then(|value| value.get("route"))
    {
        Some(Json::Array(steps)) => Some(steps.clone()),
        Some(_) => None,
        None => Some(Vec::new()),
    }
}

/// How many steps the route has, when the text can be read.
#[must_use]
pub fn route_len(text: &str) -> Option<usize> {
    route_steps(text).map(|steps| steps.len())
}

/// Where each step of the route sends the commander, as `gp.v1.Location`s in route order:
/// a move's `to`, a visit's beacon, a placement's `at`. Steps that go nowhere (a hold, a
/// wait, a broadcast) and steps this lax walk cannot read are left out. `None` when the
/// text is not JSON once its comments are gone.
#[must_use]
pub fn route_waypoints(text: &str) -> Option<Vec<Json>> {
    let steps = route_steps(text)?;
    Some(
        steps
            .iter()
            .filter_map(|step| {
                if let Some(to) = step.get("move").and_then(|value| value.get("to")) {
                    return Some(to.clone());
                }
                if let Some(beacon) = step.get("interface").and_then(|value| value.get("beacon")) {
                    return Some(object(vec![("beacon_anchor", beacon.clone())]));
                }
                step.get("place_beacon")
                    .and_then(|value| value.get("at"))
                    .cloned()
            })
            .collect(),
    )
}

/// A `verify_plan` answer's report.
fn report_of(result: &Json) -> Result<VerifyReport, BridgeError> {
    let response: VerifyPlanResponse =
        json::decode_json(&body(result, "gp.api.v1.VerifyPlanResponse"))?;
    Ok(response.report.unwrap_or_default())
}

/// A `patch_plan` answer.
fn patch_of(result: &Json) -> Result<PatchPlanResponse, BridgeError> {
    Ok(json::decode_json(&body(
        result,
        "gp.api.v1.PatchPlanResponse",
    ))?)
}

/// The result with its footer set aside and its enum values spelt as the schema spells
/// them, ready for the typed decode.
fn body(result: &Json, full_name: &str) -> Json {
    let (answer, _) = split_footer(result);
    enums::canonical(full_name, &answer)
}

fn verify_params(playbook: &str, depth: &str) -> Json {
    object(vec![
        ("depth", text(depth)),
        ("playbook_jsonc", Json::String(playbook.to_owned())),
    ])
}

fn patch_params(playbook: &str, patch: &str) -> Json {
    object(vec![
        ("playbook_jsonc", Json::String(playbook.to_owned())),
        ("json_patch", Json::String(patch.to_owned())),
    ])
}

/// A voxel as a `gp.v1.Location`.
fn voxel_location(at: [i32; 3]) -> Json {
    let [x, y, z] = at;
    object(vec![(
        "voxel",
        object(vec![
            ("x", Json::Number(x.to_string())),
            ("y", Json::Number(y.to_string())),
            ("z", Json::Number(z.to_string())),
        ]),
    )])
}

/// A `gp.v1.Voxel` object's three axes; an absent axis is zero, as proto3 JSON writes it.
fn voxel_of(value: &Json) -> [i32; 3] {
    let axis = |name: &str| {
        value
            .get(name)
            .and_then(integer)
            .and_then(|found| i32::try_from(found).ok())
            .unwrap_or(0)
    };
    [axis("x"), axis("y"), axis("z")]
}

fn integer(value: &Json) -> Option<i64> {
    match value {
        Json::Number(lexeme) | Json::String(lexeme) => lexeme.parse::<i64>().ok(),
        _ => None,
    }
}

fn object(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn text(value: &str) -> Json {
    Json::String(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPAND_EAST: &str = include_str!("../../../examples/playbooks/expand_east.jsonc");

    fn read(text: &str) -> Json {
        json::read(text).expect("json")
    }

    fn quick_report(diagnostics: &str, qualifies: bool) -> Json {
        read(&format!(
            r#"{{"report":{{"qualifies":{qualifies},"depth":"quick","diagnostics":[{diagnostics}]}},"_status":{{"phase":"lull"}}}}"#
        ))
    }

    fn patched(text: &str, inverse: &str) -> Json {
        object(vec![
            ("playbook_jsonc", Json::String(text.to_owned())),
            ("inverse_json_patch", Json::String(inverse.to_owned())),
        ])
    }

    /// An editor holding `text`, loaded and QUICK-checked clean.
    fn loaded(text: &str) -> Editor {
        let mut editor = Editor::new("seat.0");
        assert!(editor.load(text.as_bytes()));
        let call = editor.next_call(false).expect("the load check");
        assert_eq!(call.method, "verify_plan");
        editor.answered(Ok(&quick_report("", true))).expect("reads");
        assert!(editor.has_text());
        editor
    }

    fn method_of(call: Option<Call>) -> &'static str {
        call.map_or("none", |call| call.method)
    }

    fn compact(value: &Json) -> String {
        json::write(value).split_whitespace().collect()
    }

    #[test]
    fn a_click_becomes_one_appended_step() {
        let go = step_value(Action::Go, &Target::Beacon("b_00".to_owned()), "go_1").expect("go");
        assert_eq!(
            compact(&go),
            r#"{"label":"go_1","move":{"to":{"beacon_anchor":{"beacon_id":"b_00"}}}}"#
        );
        let near = step_value(Action::Go, &Target::Selector(Selector::Nearest), "go_2")
            .expect("a selector");
        assert!(compact(&near).contains(r#""nearest":{"filter":{"side":"OWN"}}"#));
        let visit = step_value(
            Action::Visit(Priority::High),
            &Target::Selector(Selector::Safest),
            "visit_1",
        )
        .expect("a visit");
        let written = compact(&visit);
        assert!(
            written.contains(
                r#""interface":{"beacon":{"safest":{}},"rows":[{"set_priority":"HIGH"}]}"#
            ),
            "{written}"
        );
        let place = step_value(Action::Place, &Target::Voxel([1, 2, 3]), "place_1").expect("place");
        assert!(compact(&place).contains(r#""place_beacon":{"at":{"voxel":{"x":1,"y":2,"z":3}}}"#));
        assert!(step_value(Action::Visit(Priority::Low), &Target::Voxel([1, 2, 3]), "v").is_none());
        assert!(step_value(Action::Recycle, &Target::Voxel([1, 2, 3]), "r").is_none());
        assert!(step_value(Action::Place, &Target::Beacon("b_00".to_owned()), "p").is_none());
        let patch = append_patch(go);
        assert!(patch.contains("/declarative/route/-"), "{patch}");
    }

    #[test]
    fn a_fresh_label_is_one_the_file_does_not_quote() {
        assert_eq!(fresh_label(EXPAND_EAST, "go"), "go_1");
        let text = r#"{"route":[{"label":"go_1"},{"label":"go_2"}]}"#;
        assert_eq!(fresh_label(text, "go"), "go_3");
    }

    #[test]
    fn comments_go_and_strings_stay() {
        let text = "{ // a comment\n \"a\": \"x // y /* z */\", /* block\n two */ \"b\": 1 }";
        let stripped = strip_comments(text);
        let value = read(&stripped);
        assert_eq!(
            value.get("a"),
            Some(&Json::String("x // y /* z */".to_owned()))
        );
        assert_eq!(value.get("b"), Some(&Json::Number("1".to_owned())));
        assert_eq!(
            stripped.matches('\n').count(),
            text.matches('\n').count(),
            "line breaks are kept"
        );
    }

    #[test]
    fn the_route_is_read_out_of_the_committed_example() {
        let waypoints = route_waypoints(EXPAND_EAST).expect("the example reads");
        assert_eq!(
            waypoints.len(),
            3,
            "move, place, move; the handler is not the route"
        );
        let first = waypoints.first().map(compact).unwrap_or_default();
        assert!(first.contains("\"voxel\""), "{first}");
        let last = waypoints.last().map(compact).unwrap_or_default();
        assert!(last.contains("b_01"), "{last}");
        assert_eq!(route_len(EXPAND_EAST), Some(3));
        assert_eq!(route_waypoints("not json"), None);
    }

    #[test]
    fn open_and_save_give_the_same_bytes() {
        for text in [
            EXPAND_EAST.to_owned(),
            EXPAND_EAST.replace('\n', "\r\n"),
            format!("\u{feff}{EXPAND_EAST}"),
        ] {
            let editor = loaded(&text);
            assert_eq!(
                editor.bytes(),
                text.as_bytes(),
                "a load and a save are byte-identical"
            );
        }
    }

    #[test]
    fn bytes_that_are_not_text_are_refused_before_any_call() {
        let mut editor = Editor::new("seat.0");
        assert!(!editor.load(&[0xff, 0xfe, 0x00]));
        assert_eq!(editor.status().key, "load_not_text");
        assert!(editor.next_call(false).is_none());
    }

    #[test]
    fn load_refuses_an_out_of_vocabulary_construct_with_its_code_and_pointer() {
        let mut editor = Editor::new("seat.0");
        assert!(editor.load(br#"{"declarative":{"route":[{"label":"a","branch":{}}]}}"#));
        assert_eq!(method_of(editor.next_call(false)), "verify_plan");
        let diagnostic = r#"{"code":"E0003","severity":"error","path":"/declarative/route/0/branch","message":"held back","beginner":"This is a word the game keeps for later."}"#;
        editor
            .answered(Ok(&quick_report(diagnostic, false)))
            .expect("reads");
        assert!(
            !editor.has_text(),
            "a refused file is not opened, and nothing is stripped"
        );
        assert_eq!(editor.verdict(), Verdict::Refused);
        assert_eq!(editor.status().key, "load_refused");
        assert_eq!(editor.status().detail, "E0003 /declarative/route/0/branch");
        let row = editor.rows().first().expect("the refusal is a row");
        assert_eq!(row.sentence, "This is a word the game keeps for later.");
        assert_eq!(row.pointer, "/declarative/route/0/branch");
    }

    #[test]
    fn an_edit_is_patched_then_quick_then_priced_then_full_after_idle() {
        let mut editor = loaded(EXPAND_EAST);
        editor.set_entities(&[Entity {
            id: "u_1".to_owned(),
            kind: EntityKind::Unit,
            subtype: "commander".to_owned(),
            owner: "seat.0".to_owned(),
            at: [358, 22, 36],
        }]);
        // The load itself owes a route estimate.
        let call = editor.next_call(false).expect("the estimate");
        assert_eq!(call.method, "estimate_route");
        let waypoints = json::write(&call.params);
        assert!(
            waypoints.contains("358"),
            "the route starts at the commander: {waypoints}"
        );
        editor
            .answered(Ok(&read(
                r#"{"reachable":true,"ms":9000,"legs":[{"to":{"x":96,"y":11,"z":62},"ms":8000},{"to":{"x":96,"y":11,"z":62},"ms":0},{"to":{"voxel":{"x":358,"y":24,"z":36}},"ms":1000}]}"#,
            )))
            .expect("reads");
        assert_eq!(editor.route().legs, vec![8000, 0, 1000]);
        assert_eq!(editor.route().points.len(), 4);
        assert_eq!(editor.route().points.last(), Some(&[358, 24, 36]));

        assert!(editor.act(Action::Go, &Target::Selector(Selector::Nearest)));
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(call.method, "patch_plan");
        assert!(json::write(&call.params).contains("go_1"));
        editor
            .answered(Ok(&patched(r#"{"declarative":{"route":[]}}"#, "[]")))
            .expect("reads");
        assert_eq!(editor.revision(), 2);
        assert!(
            !editor.rows_current(),
            "the rows describe the text before the edit"
        );
        assert!(editor.take_edit_mark(), "the idle timer restarts");
        assert_eq!(method_of(editor.next_call(false)), "verify_plan");
        editor.answered(Ok(&quick_report("", true))).expect("reads");
        assert_eq!(editor.verdict(), Verdict::Quick);
        assert!(editor.rows_current());
        // The patched text has no route, so there is nothing to price; the rule list is
        // rendered for it.
        assert_eq!(method_of(editor.next_call(false)), "render_plan");
        editor
            .answered(Ok(&read(r#"{"prose":"Playbook\n\nRoute\n"}"#)))
            .expect("reads");
        assert_eq!(editor.prose(), ["Playbook", "", "Route"]);
        assert!(editor.prose_current());
        assert!(
            editor.next_call(false).is_none(),
            "FULL waits for the idle timer"
        );
        let full = editor.next_call(true).expect("FULL once idle");
        assert!(full.full);
        assert!(json::write(&full.params).contains("full"));
        editor
            .answered(Ok(&read(
                r#"{"report":{"qualifies":true,"depth":"full","diagnostics":[]}}"#,
            )))
            .expect("reads");
        assert_eq!(editor.verdict(), Verdict::Full);
        assert!(editor.next_call(true).is_none(), "FULL once per text");
    }

    #[test]
    fn a_fix_button_is_the_verifiers_own_machine_applicable_patch() {
        let mut editor = Editor::new("seat.0");
        assert!(editor.load(b"{}"));
        let _ = editor.next_call(false);
        let diagnostic = r#"{"code":"E0108","severity":"error","path":"/declarative/handlers/0/cooldown_ms","beginner":"A rule waits a while.","suggestions":[{"title":"Set the cooldown to 1000 ms","json_patch":"[{\"op\":\"add\",\"path\":\"/declarative/handlers/0/cooldown_ms\",\"value\":1000}]","applicability":"machine_applicable"},{"title":"Look at it","json_patch":"[]","applicability":"maybe_incorrect"}]}"#;
        editor
            .answered(Ok(&quick_report(diagnostic, false)))
            .expect("reads");
        let row = editor.rows().first().expect("a row");
        assert_eq!(
            row.fixes.len(),
            1,
            "only the machine-applicable one is a Fix button"
        );
        assert!(editor.fix(0, 0));
        let call = editor.next_call(false).expect("the fix");
        assert_eq!(call.method, "patch_plan");
        assert!(json::write(&call.params).contains("cooldown_ms"));
        assert!(!editor.fix(0, 0), "not while the edit is in flight");
        assert!(!editor.fix(0, 1), "no such fix");
    }

    #[test]
    fn a_checked_placement_is_taken_without_a_second_call() {
        let mut editor = loaded(EXPAND_EAST);
        assert!(editor.preview_place([350, 22, 36]));
        assert_eq!(
            editor.ghost().map(|ghost| ghost.state),
            Some(GhostState::Waiting)
        );
        let call = editor.next_call(false).expect("the preview patch");
        assert_eq!(call.method, "patch_plan");
        assert_eq!(editor.revision(), 1, "a preview applies nothing");
        editor
            .answered(Ok(&patched(
                r#"{"placed":1}"#,
                r#"[{"op":"remove","path":"/declarative/route/3"}]"#,
            )))
            .expect("reads");
        let call = editor.next_call(false).expect("the preview's QUICK");
        assert!(json::write(&call.params).contains("placed"));
        let diagnostic = r#"{"code":"E0501","severity":"error","path":"/declarative/route/3/place_beacon/at","beginner":"Too far from anything of yours."}"#;
        editor
            .answered(Ok(&quick_report(diagnostic, false)))
            .expect("reads");
        let ghost = editor.ghost().expect("the ghost");
        assert_eq!(ghost.state, GhostState::Illegal);
        assert_eq!(ghost.sentence, "Too far from anything of yours.");

        assert!(editor.act(Action::Place, &Target::Voxel([350, 22, 36])));
        assert_eq!(editor.revision(), 2, "taken as checked");
        assert_eq!(editor.bytes(), br#"{"placed":1}"#);
        assert!(editor.rows_current(), "and its QUICK report is the rows");
        assert_eq!(editor.undo_depth(), 1);
        assert!(editor.undo());
        let call = editor.next_call(false).expect("the undo");
        assert!(
            json::write(&call.params).contains("remove"),
            "the inverse patch"
        );
    }

    #[test]
    fn a_ghost_over_a_clean_step_is_legal() {
        let report = VerifyReport {
            qualifies: true,
            ..VerifyReport::default()
        };
        assert_eq!(placement_verdict(&report, Some(3)).0, GhostState::Legal);
    }

    /// Answers every call the editor makes with something unremarkable, until it is quiet.
    /// `get_draft` answers with `carried` as its body.
    fn drain(editor: &mut Editor, drafts: &str) {
        drain_with(editor, drafts, EXPAND_EAST);
    }

    fn drain_with(editor: &mut Editor, drafts: &str, carried: &str) {
        for _ in 0..32 {
            let Some(call) = editor.next_call(false) else {
                return;
            };
            let answer = match call.method {
                "get_briefing" => read(r#"{"notes":"remember the vent"}"#),
                "list_drafts" => read(drafts),
                "list_templates" => read(r#"{"templates":[]}"#),
                "estimate_route" => read(r#"{"reachable":true}"#),
                "render_plan" => read(r#"{"prose":"Playbook\n"}"#),
                "get_draft" => {
                    assert!(
                        json::write(&call.params).contains("\"carried\""),
                        "only the carried draft is opened: {}",
                        json::write(&call.params)
                    );
                    object(vec![
                        ("playbook_jsonc", Json::String(carried.to_owned())),
                        ("label", text("carried from round 1")),
                        ("round", Json::Number("2".to_owned())),
                    ])
                }
                _ => quick_report("", true),
            };
            editor.answered(Ok(&answer)).expect("reads");
        }
        panic!("the editor never went quiet");
    }

    #[test]
    fn the_carried_draft_is_last_rounds_sealed_playbook_checked_again() {
        let mut editor = loaded(EXPAND_EAST);
        editor.lull_opened(1);
        drain(&mut editor, r#"{"drafts":[]}"#);
        assert_eq!(editor.notes(), "remember the vent");
        assert!(editor.submit());
        assert_eq!(method_of(editor.next_call(false)), "submit_plan");
        editor
            .answered(Ok(&read(
                r#"{"report":{"qualifies":true,"depth":"full"},"accepted":true}"#,
            )))
            .expect("reads");
        assert_eq!(editor.sealed_bytes(), EXPAND_EAST.as_bytes());
        // An edit after the submission, never submitted.
        assert!(editor.act(Action::Go, &Target::Beacon("b_00".to_owned())));
        let _ = editor.next_call(false);
        editor
            .answered(Ok(&patched(r#"{"unsubmitted":1}"#, "[]")))
            .expect("reads");

        editor.lull_opened(2);
        drain(
            &mut editor,
            r#"{"drafts":[{"draft_id":"carried","label":"carried from round 1","round":2}]}"#,
        );
        assert_eq!(
            editor.bytes(),
            EXPAND_EAST.as_bytes(),
            "last round's sealed playbook, as get_draft returned it"
        );
        assert_eq!(editor.status().key, "carried");
        assert_eq!(editor.status().detail, "carried from round 1");
        assert!(
            editor.rows_current(),
            "and checked against the new snapshot at once"
        );
        assert!(editor.prose_current(), "and rendered again");
    }

    #[test]
    fn a_restarted_editor_opens_the_carried_draft_through_get_draft() {
        // A new process after a resume: no text, nothing submitted, nothing sealed.
        let mut editor = Editor::new("seat.0");
        editor.lull_opened(2);
        let mut methods: Vec<&'static str> = Vec::new();
        let mut asked_for = String::new();
        for _ in 0..32 {
            let Some(call) = editor.next_call(false) else {
                break;
            };
            methods.push(call.method);
            let answer = match call.method {
                "get_briefing" => read(r#"{"notes":"remember the vent"}"#),
                "list_templates" => read(r#"{"templates":[]}"#),
                "list_drafts" => read(
                    r#"{"drafts":[{"draft_id":"editor","label":"Saved from the editor","round":2},{"draft_id":"carried","label":"carried from round 1","round":2}]}"#,
                ),
                "get_draft" => {
                    asked_for = json::write(&call.params);
                    object(vec![
                        ("playbook_jsonc", Json::String(EXPAND_EAST.to_owned())),
                        ("label", text("carried from round 1")),
                        ("round", Json::Number("2".to_owned())),
                    ])
                }
                "render_plan" => read(r#"{"prose":"Playbook\n"}"#),
                "estimate_route" => read(r#"{"reachable":true}"#),
                _ => quick_report("", true),
            };
            editor.answered(Ok(&answer)).expect("reads");
        }
        assert_eq!(
            methods
                .iter()
                .filter(|method| **method == "get_draft")
                .count(),
            1,
            "one get_draft: {methods:?}"
        );
        assert!(
            asked_for.contains("\"carried\"") && !asked_for.contains("\"editor\""),
            "the carried draft is fetched, and the seat's own editor draft is listed, not opened: {asked_for}"
        );
        assert_eq!(
            editor.bytes(),
            EXPAND_EAST.as_bytes(),
            "the gateway's copy, byte for byte"
        );
        assert_eq!(editor.status().key, "carried");
        assert!(editor.rows_current(), "checked against the new snapshot");
        assert_eq!(editor.drafts().len(), 2, "both drafts are listed");
        assert_eq!(
            editor.undo_depth(),
            0,
            "opening the carried draft is not an undoable edit"
        );
    }

    /// A playbook whose route is one move step per label.
    fn with_route(labels: &[&str]) -> String {
        let steps: Vec<String> = labels
            .iter()
            .map(|label| {
                format!(
                    r#"{{"label":"{label}","move":{{"to":{{"voxel":{{"x":1,"y":2,"z":3}}}}}}}}"#
                )
            })
            .collect();
        format!(r#"{{"declarative":{{"route":[{}]}}}}"#, steps.join(","))
    }

    #[test]
    fn quick_clicks_get_their_labels_and_indices_from_the_text_they_are_applied_to() {
        let mut editor = loaded(&with_route(&["start"]));
        assert!(editor.act(Action::Go, &Target::Beacon("b_00".to_owned())));
        assert!(editor.act(Action::Go, &Target::Beacon("b_02".to_owned())));
        assert!(editor.preview_place([350, 22, 36]));

        let first = editor.next_call(false).expect("the first edit");
        assert_eq!(first.method, "patch_plan");
        assert!(json::write(&first.params).contains("go_1"));
        editor
            .answered(Ok(&patched(&with_route(&["start", "go_1"]), "[]")))
            .expect("reads");

        let second = editor.next_call(false).expect("the second edit");
        assert_eq!(second.method, "patch_plan");
        let written = json::write(&second.params);
        assert!(
            written.contains("go_2"),
            "the second click is labelled against the text the first produced: {written}"
        );
        editor
            .answered(Ok(&patched(&with_route(&["start", "go_1", "go_2"]), "[]")))
            .expect("reads");

        let preview = editor
            .next_call(false)
            .expect("the preview, behind both edits");
        assert_eq!(preview.method, "patch_plan");
        assert!(json::write(&preview.params).contains("place_1"));
        editor
            .answered(Ok(&patched(
                &with_route(&["start", "go_1", "go_2", "place_1"]),
                r#"[{"op":"remove","path":"/declarative/route/3"}]"#,
            )))
            .expect("reads");
        let check = editor.next_call(false).expect("the preview's QUICK");
        assert_eq!(check.method, "verify_plan");
        // The placed step is route index 3, after both edits; an error there is illegal.
        let diagnostic = r#"{"code":"E0403","severity":"error","path":"/declarative/route/3/place_beacon/at/voxel","beginner":"Outside every sphere of yours."}"#;
        editor
            .answered(Ok(&quick_report(diagnostic, false)))
            .expect("reads");
        assert_eq!(
            editor.ghost().map(|ghost| ghost.state),
            Some(GhostState::Illegal)
        );
    }

    #[test]
    fn a_checked_ghost_is_not_taken_ahead_of_an_edit_still_waiting() {
        let mut editor = loaded(&with_route(&["start"]));
        assert!(editor.preview_place([350, 22, 36]));
        let _ = editor.next_call(false);
        editor
            .answered(Ok(&patched(&with_route(&["start", "place_1"]), "[]")))
            .expect("reads");
        let _ = editor.next_call(false);
        editor.answered(Ok(&quick_report("", true))).expect("reads");
        assert_eq!(
            editor.ghost().map(|ghost| ghost.state),
            Some(GhostState::Legal)
        );
        // A Go clicked before Place: the placement waits its turn instead of jumping it.
        assert!(editor.act(Action::Go, &Target::Beacon("b_00".to_owned())));
        assert!(editor.act(Action::Place, &Target::Voxel([350, 22, 36])));
        assert_eq!(editor.revision(), 1, "nothing was taken out of turn");
        let go = editor.next_call(false).expect("the go");
        assert!(json::write(&go.params).contains("go_1"));
    }

    #[test]
    fn an_edit_or_a_note_lost_to_a_dropped_connection_is_asked_again() {
        let mut editor = loaded(&with_route(&["start"]));
        assert!(editor.act(Action::Go, &Target::Beacon("b_00".to_owned())));
        let first = editor.next_call(false).expect("the edit");
        editor.dropped();
        let again = editor.next_call(false).expect("the edit, again");
        assert_eq!(again.method, "patch_plan");
        assert_eq!(json::write(&again.params), json::write(&first.params));
        editor
            .answered(Ok(&patched(&with_route(&["start", "go_1"]), "[]")))
            .expect("reads");
        assert_eq!(
            editor.revision(),
            2,
            "the edit the player asked for happened"
        );

        drain(&mut editor, r#"{"drafts":[]}"#);
        editor.save_notes("remember the vent");
        let notes = editor.next_call(false).expect("the notes");
        assert_eq!(notes.method, "save_notes");
        editor.dropped();
        let again = editor.next_call(false).expect("the notes, again");
        assert_eq!(again.method, "save_notes");
        assert!(json::write(&again.params).contains("remember the vent"));
    }

    #[test]
    fn a_missed_round_does_not_reopen_an_older_playbook_as_last_rounds() {
        let mut editor = loaded(EXPAND_EAST);
        editor.lull_opened(1);
        drain(&mut editor, r#"{"drafts":[]}"#);
        assert!(editor.submit());
        assert_eq!(method_of(editor.next_call(false)), "submit_plan");
        editor
            .answered(Ok(&read(
                r#"{"report":{"qualifies":true,"depth":"full"},"accepted":true}"#,
            )))
            .expect("reads");
        // Round 2: the carried draft is round 1's playbook, and the player edits it
        // but submits nothing, so the gateway files the safe playbook.
        editor.lull_opened(2);
        drain(
            &mut editor,
            r#"{"drafts":[{"draft_id":"carried","label":"carried from round 1","round":2}]}"#,
        );
        assert!(editor.act(Action::Go, &Target::Beacon("b_00".to_owned())));
        let _ = editor.next_call(false);
        editor
            .answered(Ok(&patched(r#"{"round_two_work":1}"#, "[]")))
            .expect("reads");
        // Round 3: nothing was carried, and round 2's `carried` draft is still listed.
        let before = editor.revision();
        editor.lull_opened(3);
        drain(
            &mut editor,
            r#"{"drafts":[{"draft_id":"carried","label":"carried from round 1","round":2}]}"#,
        );
        assert_eq!(
            editor.bytes(),
            br#"{"round_two_work":1}"#,
            "the text on screen is kept; round 1's playbook is not last round's"
        );
        assert_eq!(editor.revision(), before, "nothing was opened over it");
    }

    #[test]
    fn a_leg_that_names_no_end_draws_no_polyline() {
        let mut editor = loaded(EXPAND_EAST);
        editor.set_entities(&[Entity {
            id: "u_1".to_owned(),
            kind: EntityKind::Unit,
            subtype: "commander".to_owned(),
            owner: "seat.0".to_owned(),
            at: [358, 22, 36],
        }]);
        assert_eq!(method_of(editor.next_call(false)), "estimate_route");
        editor
            .answered(Ok(&read(
                r#"{"reachable":true,"ms":9000,"legs":[{"to":{"x":96,"y":11,"z":62},"ms":8000},{"ms":1000}]}"#,
            )))
            .expect("reads");
        assert!(!editor.route().readable);
        assert!(editor.route().points.is_empty(), "no point is invented");
        assert_eq!(editor.route().legs, vec![8000, 1000], "the times stay");
    }

    #[test]
    fn a_refusal_settles_the_call_and_says_so() {
        let mut editor = loaded(EXPAND_EAST);
        assert!(editor.act(Action::Go, &Target::Beacon("b_00".to_owned())));
        let _ = editor.next_call(false);
        editor
            .answered(Err(("INVALID_ARGUMENT", "no such path")))
            .expect("settled");
        assert_eq!(editor.revision(), 1, "a refused patch changes nothing");
        assert_eq!(editor.refusals(), 1);
        assert_eq!(editor.status().key, "gateway_refused");
        assert!(!editor.busy());
    }
}
