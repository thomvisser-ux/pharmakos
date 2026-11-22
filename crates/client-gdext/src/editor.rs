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
//! # A shut door, and what S1's stage demo found (decisions-log item 139 (2))
//!
//! The rig tells the editor whether its calls can go out ([`Door`]): open in a Lull, shut
//! once this seat is ready and in a Push, a recap and an ended match. With the door shut an
//! ask is refused at once with the reason, what was queued is dropped rather than kept for a
//! Lull that may never come, and a check that will not come says why
//! ([`Editor::unchecked`]), as does a check the gateway refused, `PHASE_CLOSED` included
//! (finding K). "Ready to seal" is said only with the door open, and once the gateway has
//! filed this seat's safe playbook the editor says its text was not sealed ([`SealLine`];
//! finding J). The editor keeps the file its text came from, so Save as offers that name
//! and no other (finding G). Round 1's "This round" sentence comes with its first accepted
//! Submit, because the gateway says it only of a sealed or carried playbook (finding A: the
//! run sheet's timing, not a drop).
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
    GetBriefingResponse, GetDraftResponse, ListDraftsResponse, PatchPlanResponse,
    SaveNotesResponse, SubmitPlanResponse, VerifyPlanResponse, VerifyReport,
};
use pharmakos_proto::json::{self, Json};

use crate::enums;
use crate::error::BridgeError;
use crate::rig::proto3_text;
use crate::rule_list::{self, ChipEditor, NameKind, RuleList};
use crate::targeting::{self, Chip, Feature, FeatureKind};
use crate::variant::proto3_integer;
use crate::variant::text_or_empty;
use crate::view::{Entity, EntityKind, split_footer};
use crate::vocabulary::{self, BeaconDetail, PickError, Vocabulary};
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
/// PLACEHOLDER: the Load refusal codes — OWNER, S6, with the editor's Load and Save as
/// template. The list is the client's reading of the diagnostic catalogue's decode
/// family. A column in the catalogue saying which codes refuse a load would make it the
/// verifier's, and a new decode code would then need no change here.
pub const LOAD_REFUSALS: &[&str] = &["E0001", "E0002", "E0003", "E0004", "E0006"];

/// The draft id the editor's "Save draft" keeps its copy under, so repeated saves replace
/// one draft rather than filling the seat's store.
///
/// PLACEHOLDER: one editor draft per seat — OWNER, S6, with the draft browser. One draft
/// per seat is the skeleton's; named drafts and a draft browser are S6's (skeleton plan
/// T19, PR 2 and S6).
pub const EDITOR_DRAFT_ID: &str = "editor";

/// The draft id the gateway pre-loads last round's playbook under
/// (`pharmakos_gateway::surface::CARRIED_DRAFT_ID`, spec section 13 "Draft continuity").
pub const CARRIED_DRAFT_ID: &str = "carried";

/// The route's JSON Pointer in a playbook.
const ROUTE_POINTER: &str = "/declarative/route";

/// `options.allow_dormant_beacons`'s JSON Pointer: the option the E0601 checkbox sets.
pub const DORMANT_OPTION: &str = "/declarative/options/allow_dormant_beacons";

/// The playbook's `options` object, under which the checkbox's removal names the key in
/// the spelling the text uses.
const OPTIONS_POINTER: &str = "/declarative/options";

/// The option's two spellings the playbook codec reads (`crates/proto/src/json/codec.rs`):
/// the canonical `snake_case` one first, then proto JSON's `camelCase`.
const DORMANT_KEYS: &[&str] = &["allow_dormant_beacons", "allowDormantBeacons"];

/// The verifier codes that name the dormant-beacon option, and show its checkbox: E0601,
/// the route adds draw beyond supply and the option is not set; W0603, the same with the
/// option set, so the shortfall is accepted (`crates/verifier/src/estimate.rs`). The
/// checkbox's reading of the verifier's catalogue, as [`LOAD_REFUSALS`] is Load's.
///
/// PLACEHOLDER: the dormant checkbox's codes — OWNER, S6, with the catalogue column. A
/// column in the catalogue naming the codes an option answers would make the list the
/// verifier's, as it would [`LOAD_REFUSALS`].
pub const DORMANT_CODES: &[&str] = &["E0601", "W0603"];

/// The verifier's refusal of a playbook over the size budget (`crates/verifier/src/structure.rs`,
/// `budget`): the size meter says this row's sentence, not a sentence of its own (S3's plan,
/// E9). The meter's reading of the verifier's catalogue, as [`DORMANT_CODES`] is the checkbox's.
pub const SIZE_REFUSAL: &str = "E0110";

/// The size meter: what the verifier counted, from the last report on the text on screen
/// (S3's plan, E9). Nothing here is counted or compared by the client.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SizeMeter {
    /// `VerifyReport.size_units`, as it came.
    pub units: u32,
    /// `VerifyReport.size_budget`, as it came.
    pub budget: u32,
    /// The sentence of the verifier's own size refusal, when that report made one.
    pub refusal: Option<String>,
}

/// The size meter from one report's numbers and its rows; `None` when the report names no
/// budget, which proto3 writes as 0 and which no bar can be drawn against.
fn size_meter_of(report: &VerifyReport, rows: &[Row]) -> Option<SizeMeter> {
    (report.size_budget > 0).then(|| SizeMeter {
        units: report.size_units,
        budget: report.size_budget,
        refusal: rows
            .iter()
            .find(|row| row.code == SIZE_REFUSAL)
            .map(|row| row.sentence.clone()),
    })
}

/// The `allow_dormant_beacons` checkbox as the panel draws it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Dormant {
    /// Whether the checkbox is drawn: the rows name the option, or the text sets it.
    pub shown: bool,
    /// Whether the text on screen sets the option.
    pub checked: bool,
    /// Whether the box takes a click at all: only while the planning door is open. A click it
    /// cannot take otherwise reaches the bridge and is refused with its reason (S3's `fixp`, P6b
    /// finding 9: a press on a disabled box was dropped by Godot without a word).
    pub enabled: bool,
    /// Why the last click was refused, while the text it was refused for is on screen.
    pub refused: Option<DormantRefusal>,
}

/// Why a click on the E0601 checkbox was not taken: said on the status line and beside the box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DormantRefusal {
    /// The planning door is shut: said as the door's own line.
    Shut,
    /// The rows describe an older text than the one on screen: its checks are on their way.
    Stale,
    /// An edit is on its way.
    Busy,
    /// Ticked with no patch from the verifier, or cleared with nothing set: nothing to change.
    NothingToChange,
}

impl DormantRefusal {
    /// The name GDScript looks the words up by (`dormant_refused_<name>`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Shut => "shut",
            Self::Stale => "stale",
            Self::Busy => "busy",
            Self::NothingToChange => "nothing_to_change",
        }
    }
}

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
    /// A feature by its name, `vent_<x>_<y>`: what a click on a vent names
    /// (`docs/design/targeting.md`, "Surfaces").
    Feature(String),
    /// "The nearest <kind> you can cover": what Alt-click turns a clicked feature into, a
    /// description the sim ranks when the step starts.
    NearestUncovered(FeatureKind),
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
    /// Visit & change's priority rows: an interface step with one row, the beacon's
    /// Quartermaster priority. The menu's other rows (a setting, a mandate, a Build target, a
    /// queued structure) are composed from `get_beacon`'s detail and `get_vocabulary`
    /// ([`Editor::change_setting_number`] and its siblings), not as an `Action`.
    Visit(Priority),
    /// Recycle: an interface step with the recycle row.
    Recycle,
    /// Place beacon: a place-beacon step at a voxel.
    Place,
    /// Place beacon covering a feature: a place-beacon step whose site is `covering` the
    /// feature a click named, or the description Alt-click made of it.
    PlaceCovering,
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
            "place_covering" => Some(Self::PlaceCovering),
            _ => None,
        }
    }

    /// The stem of the label the new step gets.
    const fn stem(self) -> &'static str {
        match self {
            Self::Go => "go",
            Self::Visit(_) => "visit",
            Self::Recycle => "recycle",
            Self::Place | Self::PlaceCovering => "place",
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
    /// The diagnostic's other JSON Pointers, as the verifier wrote them (W0702's earlier
    /// rule, which the rule list's connector joins).
    pub related: Vec<String>,
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
                related: diagnostic.related_paths.clone(),
            }
        })
        .collect()
}

/// Which answer the rows on screen came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Verdict {
    /// Nothing has been checked.
    #[default]
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

/// What an answer to the editor's call was: an answer, or a refused call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Settled {
    /// A result, or an error the gateway answers a question with (an `estimate_route`
    /// whose waypoint resolves to nothing): drawn, not counted as a refusal.
    Answered,
    /// The gateway refused the call; [`Editor::refusals`] counts it.
    Refused,
}

/// Whether the editor's planning calls can reach the gateway now, as the watch rig reads it
/// off the admin connection's footer and its own Ready (S1's stage demo, finding K;
/// decisions-log item 139 (2)).
///
/// The gateway's planning door is open in a Lull and closed in a Push, a recap and an ended
/// match (`PHASE_CLOSED`), and the rig sends no planning call once this seat is ready. A
/// call the rig will not send never comes back, so the editor neither queues one while the
/// door is shut nor keeps one it queued before: an ask is refused at once with the reason,
/// and a check that will not come says why instead of "Checking...". Which phase it is
/// stays the gateway's word; this is scheduling, as the rig's own Lull gate is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Door {
    /// A Lull, or a phase not yet known: the editor's calls go out in the Lull.
    #[default]
    Open,
    /// A Lull in which this seat has said Ready: its orders are final until the next Lull.
    Ready,
    /// The Push plays.
    Push,
    /// The recap.
    Recap,
    /// The match is over.
    Ended,
}

impl Door {
    /// The name GDScript reads, and the suffix of the string-table keys that say why the
    /// door is shut (`closed_push`, `unchecked_push`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Ready => "ready",
            Self::Push => "push",
            Self::Recap => "recap",
            Self::Ended => "ended",
        }
    }
}

/// Why the checks of the text on screen will not come: what the panel shows in place of
/// "Checking...", "Estimating...", "Rendering..." and "Reading the map..." so none of them
/// waits for an answer that is not on its way (finding K).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Unchecked {
    /// The gateway refused a check of this text: `CODE: message`, as it came.
    Refused(String),
    /// The gateway answered, and this client could not read the answer (a rendering whose
    /// chips cannot be cut where it says, a report that is not a report): the reason, as the
    /// client's decode wrote it. Shown as such rather than as a check still on its way.
    Unreadable(String),
    /// The door is shut, so no check is sent.
    Closed(Door),
}

/// One check of the text on screen, by the panel line that waits for it: a refusal is
/// shown on its own check's line and nowhere else ([`Editor::unchecked`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    /// QUICK and FULL, the verdict line and the rows ("Checking...").
    Rows,
    /// `estimate_route`, the route line ("Estimating...").
    Route,
    /// `render_plan`, the rules' prose ("Rendering...").
    Prose,
    /// `resolve_refs`, the chips ("Reading the map...").
    Chips,
}

impl Check {
    /// Every check, in the order the panel draws their lines.
    pub const ALL: [Self; 4] = [Self::Rows, Self::Route, Self::Prose, Self::Chips];

    /// The name GDScript reads the check's line under.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rows => "rows",
            Self::Route => "route",
            Self::Prose => "prose",
            Self::Chips => "chips",
        }
    }
}

/// Why each check of a text will not come, with the revision it checked: the gateway's
/// refusal, or an answer this client could not read.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
struct CheckRefusals {
    rows: Option<(u64, Unchecked)>,
    route: Option<(u64, Unchecked)>,
    prose: Option<(u64, Unchecked)>,
    chips: Option<(u64, Unchecked)>,
}

impl CheckRefusals {
    const fn get(&self, check: Check) -> Option<&(u64, Unchecked)> {
        match check {
            Check::Rows => self.rows.as_ref(),
            Check::Route => self.route.as_ref(),
            Check::Prose => self.prose.as_ref(),
            Check::Chips => self.chips.as_ref(),
        }
    }

    const fn slot(&mut self, check: Check) -> &mut Option<(u64, Unchecked)> {
        match check {
            Check::Rows => &mut self.rows,
            Check::Route => &mut self.route,
            Check::Prose => &mut self.prose,
            Check::Chips => &mut self.chips,
        }
    }
}

/// What the verdict line says after the verifier's word, if anything (finding J).
///
/// "Ready to seal" and "Cannot be sealed yet" are said only while the door is open, because
/// only then can this text be sealed; outside a Lull a playbook the gateway filed for this
/// seat is said to be what was sealed, and otherwise nothing is added.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealLine {
    /// Nothing after the verdict.
    None,
    /// The last report qualifies the text on screen, and it can be sealed now.
    Ready,
    /// The last report does not qualify it.
    NotReady,
    /// The gateway filed this seat's safe playbook for the round: the text on screen was
    /// not sealed.
    Filed,
}

impl SealLine {
    /// The name GDScript looks the words up by; empty for [`SealLine::None`].
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Ready => "ready",
            Self::NotReady => "not_ready",
            Self::Filed => "filed",
        }
    }
}

/// The feed kind of the line a seat reads when the gateway filed its safe playbook
/// (`pharmakos_gateway::surface::SAFE_PLAYBOOK_FILED_KIND`; the gateway's act at the Push's
/// start, spec section 14). The client's reading of the gateway's feed vocabulary, as
/// [`ESTIMATE_FOUND_NOTHING`] is of its error codes; a unit test holds it to the gateway's
/// source.
pub const SAFE_PLAYBOOK_FILED_KIND: &str = "safe_playbook_filed";

/// The error code `estimate_route` answers a waypoint that resolves to nothing with
/// (`crates/gateway/src/surface/knowledge.rs`, `place_of`): a `covering` that covers
/// nothing, a selector that ranks nothing, a beacon this seat does not know (unknown, or
/// another seat's it has not seen, which the gateway answers alike so that a seat cannot map
/// the enemy's beacons), or a commander standing nowhere. Every one is the route's answer
/// and is drawn as the route (decisions-log item 136 (3) (k)).
///
/// **Not narrowed to the `covering` and selector causes** (item 136 (5) asked S2's first
/// client lane to decide; decided, the owner can veto). The client reads the code alone, and
/// telling the causes apart would mean classifying the waypoints itself or parsing the
/// gateway's sentence, which is the gateway's to word; and the gateway answers an unseen
/// beacon and a missing one alike on purpose. So a mistyped own beacon reads as no route,
/// with the gateway's own sentence saying why, rather than as a refused call.
const ESTIMATE_FOUND_NOTHING: &str = "NOT_FOUND";

/// The route as the gateway priced it: where each leg ends and what it costs.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Route {
    /// The commander's voxel, then where each leg ends; sim axes.
    pub points: Vec<[i32; 3]>,
    /// Each leg's travel, in game milliseconds, exactly as the gateway answered.
    pub legs: Vec<i64>,
    /// The whole route's travel, in game milliseconds, as the gateway answered.
    pub whole: i64,
    /// The whole route's travel as the player reads it ("1 min 24 s"), rendered by the
    /// gateway (the register's S3-18); empty when it sent none.
    pub whole_text: String,
    /// Each leg's travel as the player reads it, as the gateway rendered it, already labelled
    /// a bound for a fogged leg; empty for a leg it sent none for.
    pub leg_texts: Vec<String>,
    /// Whether the connectivity oracle found a route at all.
    pub reachable: bool,
    /// Whether this route describes the text on screen.
    pub current: bool,
    /// Whether every leg named where it ends. When one did not, `points` is empty and no
    /// polyline is drawn, rather than one to a point the gateway never named.
    pub readable: bool,
    /// The gateway's `NOT_FOUND` sentence when a waypoint resolves to nothing — a
    /// `covering` that covers nothing, a selector that ranks nothing, a beacon this seat
    /// does not know, a commander standing nowhere ([`ESTIMATE_FOUND_NOTHING`]) — as it
    /// came, and empty otherwise. That is the gateway's answer about the route, which the editor
    /// draws (no polyline, the sentence), not a refused call.
    pub found_nothing: String,
}

/// The placement ghost's verdict (spec section 13, "a live legality ghost").
///
/// PLACEHOLDER: the ghost per click, not live on hover — OWNER, S6. The skeleton's
/// ghost is per click (plan T19 amendment, `skeleton-plan-w6-notes.md` A4); a
/// live-on-hover legality ghost is S6's, with the full editor (A4 item 5; S3's plan's
/// register row S3-02: not on any P6 path, a ghost per click still answers).
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

/// Why the rule list took no edit: each said on the status line by its own words
/// (`rule_refused_<name>` in `godot/scripts/strings.gd`), never a silent `false`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleEditRefusal {
    /// The door is shut: said as the door's own line.
    Shut,
    /// There is no text on screen.
    NoText,
    /// The rule list describes an older text than the one on screen.
    Stale,
    /// An edit is on its way.
    Busy,
    /// The list was redrawn since the editor was opened on it: another rendering, or another
    /// chip at that index.
    Changed,
    /// No chip at that index.
    NoSuchChip,
    /// The chip takes another editor (a number field asked of an enum).
    NotThisEditor,
    /// The enum lists no value at that index.
    NoSuchValue,
    /// The enum's value text, the gateway's, is not JSON.
    NotJson,
    /// The value is the one the chip holds: nothing is sent.
    Unchanged,
    /// No rule at that index.
    NoSuchRule,
    /// A rule dropped on itself.
    OntoItself,
    /// A rule dropped on a rule of another array: a step onto a rule.
    OtherList,
    /// A map pick with no pick on.
    NotPicking,
    /// A map pick whose click named nothing: no beacon, feature or voxel. The pick stays on.
    NoTarget,
    /// The rendering id does not fit GDScript's `int`, so no control can name it back.
    RenderingOutOfRange,
    /// The click names something the chip does not take; the chip's kind, `beacon`,
    /// `location` or `feature`.
    NotThisKind(&'static str),
    /// The gateway lists no such arm at this pointer: the arm the click would write.
    ArmNotHere(&'static str),
    /// The text typed into a value field is not a whole number: an optional sign and digits,
    /// nothing else (no space, no point, no expression), within an `i64`. Nothing is sent.
    NotAnInteger,
    /// The vocabulary has not been read this Lull, or its read was refused.
    NoVocabulary,
    /// The condition slot is not inside the rule the picker was opened on.
    NotInRule,
    /// No family, predicate, mandate, blueprint, setting, target or name at that index.
    NoSuchEntry,
    /// The gateway locked the entry or its family: its reason is drawn beside it.
    Locked,
    /// No detail of that beacon is on hand: none was read, it was another beacon's, or its
    /// read was refused.
    NoDetail,
    /// A setting's pointer names no field.
    NoPointer,
    /// The entry carries no lock and no name: the gateway's answer named no word to write.
    Unnamed,
    /// No condition slot at that index.
    NoSuchSlot,
    /// The slot is a group (`all`, `any`, `not`): its items are the pick targets, not it.
    GroupSlot,
    /// The slot names no op this build knows (or none at all): no pick is guessed.
    NoPickOp,
}

impl RuleEditRefusal {
    /// The name GDScript looks the words up by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Shut => "shut",
            Self::NoText => "no_text",
            Self::Stale => "stale",
            Self::Busy => "busy",
            Self::Changed => "changed",
            Self::NoSuchChip => "no_such_chip",
            Self::NotThisEditor => "not_this_editor",
            Self::NoSuchValue => "no_such_value",
            Self::NotJson => "not_json",
            Self::Unchanged => "unchanged",
            Self::NoSuchRule => "no_such_rule",
            Self::OntoItself => "onto_itself",
            Self::OtherList => "other_list",
            Self::NotPicking => "not_picking",
            Self::NoTarget => "no_target",
            Self::RenderingOutOfRange => "rendering_out_of_range",
            Self::NotThisKind(_) => "not_this_kind",
            Self::ArmNotHere(_) => "arm_not_here",
            Self::NotAnInteger => "not_an_integer",
            Self::NoVocabulary => "no_vocabulary",
            Self::NotInRule => "not_in_rule",
            Self::NoSuchEntry => "no_such_entry",
            Self::Locked => "locked",
            Self::NoDetail => "no_detail",
            Self::NoPointer => "no_pointer",
            Self::Unnamed => "unnamed",
            Self::NoSuchSlot => "no_such_slot",
            Self::GroupSlot => "group_slot",
            Self::NoPickOp => "no_pick_op",
        }
    }

    /// What fills the words: the chip's kind or the arm, and nothing for the rest.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::NotThisKind(detail) | Self::ArmNotHere(detail) => detail,
            _ => "",
        }
    }
}

/// The refusal of a rule-list edit, the gateway's or this client's: shown on the rule that holds
/// `pointer` (S3's `fixp`: a refusal is drawn where the player is looking, not only on the
/// status line).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuleRefusal {
    /// What the edit was of: the chip's pointer, the moved rule's or the slot's.
    pub pointer: String,
    /// The string table's key for its words: [`GATEWAY_REFUSED_KEY`] for the gateway's refusal,
    /// the status line's own key (`status_rule_refused_<name>`, `status_closed_<door>`) for one
    /// of this client's.
    pub key: String,
    /// What fills the words: the gateway's `CODE: message`, as it came, or the refusal's detail.
    pub detail: String,
    /// The text it was asked of.
    revision: u64,
}

impl From<PickError> for RuleEditRefusal {
    fn from(error: PickError) -> Self {
        match error {
            PickError::NoSuchEntry => Self::NoSuchEntry,
            PickError::Locked => Self::Locked,
            PickError::NotJson => Self::NotJson,
            PickError::NoPointer => Self::NoPointer,
            PickError::Unnamed => Self::Unnamed,
        }
    }
}

/// The pick that is on: a rule-list chip's, as the control that began it named it, or a Build
/// target's anchor for the beacon and blueprint the Visit & change menu named.
#[derive(Clone, PartialEq, Eq, Debug)]
enum PickOn {
    Chip {
        chip: usize,
        rendering: u64,
        pointer: String,
    },
    Anchor {
        beacon: String,
        blueprint: usize,
    },
}

/// What one of the editor's reads of the gateway's words came to, when it did not come: the
/// vocabulary's or a beacon's detail (decisions-log item 160 (4)). Shown on its own line, and nothing
/// waits on it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ReadFailed {
    /// The gateway refused the read: `CODE: message`, as it came.
    Refused(String),
    /// The gateway answered, and this client could not read the answer: the decode's reason.
    Unreadable(String),
}

impl ReadFailed {
    /// The name GDScript looks the words up by.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Refused(_) => "refused",
            Self::Unreadable(_) => "unreadable",
        }
    }

    /// What fills the words.
    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            Self::Refused(detail) | Self::Unreadable(detail) => detail,
        }
    }
}

/// A rule as a drag names it: its index in the rule list, the rendering the drag began on and
/// its pointer, so a drop never moves another rule than the one dragged.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuleNamed {
    /// Its index in the rule list.
    pub index: usize,
    /// The rendering id the drag began on ([`Editor::rules_rendering`]).
    pub rendering: u64,
    /// Its JSON Pointer.
    pub pointer: String,
}

/// Two rules the conflict connector joins (W0702), by their index in the rule list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Conflict {
    /// The row that names them.
    pub row: usize,
    /// The earlier rule, which wins every time both are true.
    pub earlier: usize,
    /// The later rule, which never fires while the earlier does.
    pub later: usize,
    /// The row's sentence, the verifier's.
    pub sentence: String,
}

/// The string table's key for the gateway's refusal of an edit, drawn on its rule and in a value
/// field's popup.
pub const GATEWAY_REFUSED_KEY: &str = "rule_refused";

/// What became of the last value sent from a value field (the number chip's, or a Visit & change
/// number setting's), for the field's popup to say while it is open (S3's `fixp`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum FieldAsk {
    /// Nothing was sent since the field opened.
    #[default]
    Nothing,
    /// The edit is queued or on its way.
    Waiting,
    /// It was refused, by this client or by the gateway: the string table's key and its detail.
    Refused {
        /// The string table's key, as [`RuleRefusal::key`].
        key: String,
        /// What fills the words.
        detail: String,
    },
    /// The edit landed.
    Landed,
}

impl FieldAsk {
    /// The name GDScript reads.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Nothing => "nothing",
            Self::Waiting => "waiting",
            Self::Refused { .. } => "refused",
            Self::Landed => "landed",
        }
    }
}

/// A value field's text as the whole number it names: an optional `-` or `+`, then one or more
/// ASCII digits, and nothing else, within an `i64`. A decoding of what the player typed, not a
/// rule: whether the number is legal there is the gateway's (S3's `fixp`).
///
/// # Errors
///
/// [`RuleEditRefusal::NotAnInteger`] for anything else: an empty text, a space, a point, an
/// expression, or a number past an `i64`.
pub fn typed_integer(text: &str) -> Result<i64, RuleEditRefusal> {
    let digits = match text.as_bytes() {
        [b'-' | b'+', rest @ ..] => rest,
        all => all,
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(RuleEditRefusal::NotAnInteger);
    }
    text.parse::<i64>()
        .map_err(|_| RuleEditRefusal::NotAnInteger)
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
    /// What QUICK said about the patched draft, once answered.
    checked: Option<CheckedPreview>,
}

/// QUICK's report on a patched draft, kept until the placement is taken or dropped.
#[derive(Clone, PartialEq, Eq, Debug)]
struct CheckedPreview {
    rows: Vec<Row>,
    qualifies: bool,
    dormant: Option<String>,
    size: Option<SizeMeter>,
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
        /// The file the bytes were read from, when the player opened one.
        file: Option<String>,
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
    /// New: the shipped empty playbook, put into the editor as one edit.
    New {
        text: String,
    },
    /// A rule-list edit: a chip's `replace` or a drag's `move`, composed from the click, and
    /// the pointer of what it edits, for a refusal to be shown on that rule.
    RuleEdit {
        patch: String,
        pointer: String,
        /// Whether a value field sent it, so its popup hears what became of it.
        field: bool,
    },
    /// `get_draft`: open the draft `id` (the carried one) as the text.
    GetDraft {
        id: String,
    },
    /// `get_map_summary`: the features the vent click picks from, read every Lull.
    MapSummary,
    /// The `allow_dormant_beacons` checkbox: `on`, the verifier's own patch from E0601;
    /// off, the removal of the option the text sets.
    SetOption {
        patch: String,
        on: bool,
    },
    /// `get_vocabulary`: the words the pickers draw, read every Lull.
    Vocabulary,
    /// A Visit & change row: one route step visiting `beacon` with the one `row`, labelled
    /// against the text the patch is applied to, as a map action is.
    Visit {
        beacon: String,
        row: Json,
        /// Whether a value field sent it, as for a rule-list edit.
        field: bool,
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
                | Self::New { .. }
                | Self::RuleEdit { .. }
                | Self::GetDraft { .. }
                | Self::SetOption { .. }
                | Self::Visit { .. }
        )
    }
}

/// What Undo takes back.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Undo {
    /// The inverse patch `patch_plan` handed back for an edit.
    Patch(String),
    /// The text as it was before the wizard's playbook replaced it (`None`: there was no
    /// text), restored as it was, byte for byte, with no call, and the file it came from.
    Restore {
        text: Option<String>,
        file: Option<String>,
    },
}

/// The call in flight, with what the answer is needed for.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Sent {
    Load {
        candidate: String,
        file: Option<String>,
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
    /// `resolve_refs` for the text at `revision`: the chips.
    Resolve {
        revision: u64,
    },
    MapSummary,
    /// `get_vocabulary`, asked in `round`.
    Vocabulary {
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
    verdict: Verdict,
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
    /// The round of the last submission the gateway accepted, as this process saw it: what the
    /// Ready line says Ready will do (S3's `fixp`, P6b finding 4).
    sealed_round: Option<u32>,
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
    /// The rule list: `render_plan`'s lines, rules and chips for the text at
    /// `prose_revision`.
    rule_list: RuleList,
    /// The refusal of the last rule-list edit, the gateway's or this client's, for the text it
    /// was asked of.
    rule_refusal: Option<RuleRefusal>,
    /// What became of the last value a value field sent ([`FieldAsk`]).
    field_ask: FieldAsk,
    /// Why the last click on the E0601 checkbox was refused, and the revision it was refused at.
    dormant_refused: Option<(u64, DormantRefusal)>,
    /// The chip whose map pick the next map click is, by its index in the rule list.
    picking: Option<PickOn>,
    /// The rendering id: bumped on every rule-list answer the editor accepts, a re-render of
    /// the same text included (a new round's Lull, a door opened again), so any re-render
    /// invalidates every control opened on the list before it.
    rendering: u64,
    /// The rendering the rule list on screen is, once one was accepted for the text.
    rules_rendering: Option<u64>,
    prose_revision: Option<u64>,
    /// The rule list (`render_plan`) is owed for the newest text.
    render_owed: bool,
    /// The chips: what each feature reference in the text reads now, for the text at
    /// `chips_revision`.
    chips: Vec<Chip>,
    /// Every Attack beacon's target in the text on screen, read once per text
    /// ([`targeting::attack_targets`]), not once per frame.
    attack: Vec<targeting::AttackMark>,
    /// Every Build target in the text on screen, read once per text
    /// ([`targeting::build_targets`]).
    build_marks: Vec<targeting::BuildMark>,
    chips_revision: Option<u64>,
    /// The chips (`resolve_refs`) are owed for the newest text.
    resolve_owed: bool,
    /// The features the map lists, as the last `get_map_summary` this Lull answered.
    features: Vec<Feature>,
    /// The Lull's "this round" sentence, from the last briefing; `None` when it had none.
    this_round: Option<String>,
    /// The verifier's own patch that sets `allow_dormant_beacons`, from E0601's suggestion
    /// in the report the rows came from; `None` when that report had no E0601.
    dormant_patch: Option<String>,
    /// Whether the editor's calls can reach the gateway now, as the rig reads it.
    door: Door,
    /// The gateway's refusal of each check, `CODE: message`, and the revision it checked.
    check_refusals: CheckRefusals,
    /// The file the text on screen was opened from or last saved to; `None` when it came
    /// from a template, a carried draft or nowhere (finding G: Save as offers this name, and
    /// no other file's).
    file: Option<String>,
    /// The last file a Load opened or a Save wrote this session, whatever the text on screen is
    /// now: where Load opens when the text has no file of its own (S3's `fixp`).
    last_file: Option<String>,
    /// The gateway's `safe_playbook_filed` line, when it filed this seat's safe playbook for
    /// the round under way (the line comes in the seat's own feed; finding J).
    filed: Option<String>,
    /// What the verifier counted, from the last report on the text on screen (E9).
    size: Option<SizeMeter>,
    /// The words the pickers draw, as `get_vocabulary` answered this Lull; `None` before.
    vocabulary: Option<Vocabulary>,
    /// Why this Lull's vocabulary did not come, when it did not.
    vocabulary_failed: Option<ReadFailed>,
    /// The detail of the beacon last described, as `get_beacon` answered.
    beacon: Option<BeaconDetail>,
    /// Why the beacon's detail did not come, when it did not.
    beacon_failed: Option<ReadFailed>,
    /// The route's step labels in the text on screen, read once per text.
    step_labels: Vec<String>,
    /// The rules' ids in the text on screen, read once per text.
    rule_ids: Vec<String>,
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

    /// Load a playbook's bytes that came from no file of the player's (a check's fixture,
    /// a pasted text): [`Editor::open`] with no file.
    pub fn load(&mut self, bytes: &[u8]) -> bool {
        self.open(bytes, None)
    }

    /// Load a file's bytes, read from `file` when the player opened one. Returns `false`
    /// when the bytes are not UTF-8 text, which no playbook can be, or when the door is
    /// shut; otherwise the file is checked by QUICK before the editor takes it, and once it
    /// is taken, `file` is the file the text came from ([`Editor::file`]).
    pub fn open(&mut self, bytes: &[u8], file: Option<&str>) -> bool {
        if self.shut_to_asks() {
            return false;
        }
        let Ok(candidate) = String::from_utf8(bytes.to_vec()) else {
            self.say("load_not_text", "");
            return false;
        };
        self.queue.push_back(Job::Load {
            candidate,
            file: file.map(str::to_owned),
        });
        self.touch();
        true
    }

    /// The text on screen was written to `file`, byte for byte: from now on it is the file
    /// the text belongs to, which Save writes and Save as offers.
    pub fn saved_to(&mut self, file: &str) {
        if self.text.is_some() {
            self.file = Some(file.to_owned());
            self.last_file = Some(file.to_owned());
            self.say("saved_file", file);
        }
    }

    /// The last file a Load opened or a Save wrote this session, if any: where Load opens when the
    /// text on screen came from no file.
    #[must_use]
    pub fn last_file(&self) -> Option<&str> {
        self.last_file.as_deref()
    }

    /// The file the text on screen was opened from or last saved to, if it came from one:
    /// not after a template's playbook, a carried draft, or a text loaded from no file.
    #[must_use]
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// The text the editor holds, as bytes: what Save writes. Empty with no text.
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        text_or_empty(self.text.as_deref()).as_bytes().to_vec()
    }

    /// The text the last accepted submission sealed, as bytes; empty before one.
    #[must_use]
    pub fn sealed_bytes(&self) -> Vec<u8> {
        text_or_empty(self.sealed.as_deref()).as_bytes().to_vec()
    }

    /// A map action. Returns whether it was taken: an action needs text to act on, and a
    /// target it can name (a visit or a recycle names a beacon, a placement names ground).
    pub fn act(&mut self, action: Action, target: &Target) -> bool {
        if self.shut_to_asks() {
            return false;
        }
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
        if self.shut_to_asks() {
            return false;
        }
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
        if ready && let Some(preview) = self.preview.take() {
            self.undo.push(Undo::Patch(preview.inverse));
            self.accept_text(preview.patched);
            if let Some(checked) = preview.checked {
                self.rows = checked.rows;
                self.dormant_patch = checked.dormant;
                self.size = checked.size;
                self.verdict = Verdict::Quick;
                self.rows_revision = self.revision;
                self.qualifies = checked.qualifies;
                self.owed.quick = false;
            }
            self.say("", "");
            return true;
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
    ///
    /// A Fix taken says so at once ("fixing"), and again when the patched text lands
    /// ("fixed"): the answer takes a round trip, and a click with nothing to show for it
    /// until then reads as a click that was lost (F1, decisions-log item 126 (3)).
    pub fn fix(&mut self, row: usize, fix: usize) -> bool {
        if self.shut_to_asks() {
            return false;
        }
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
        self.say("fixing", "");
        true
    }

    /// Undo the last edit, with the inverse patch `patch_plan` handed back for it.
    pub fn undo(&mut self) -> bool {
        if self.shut_to_asks() {
            return false;
        }
        if self.undo.is_empty() {
            return false;
        }
        self.queue.push_back(Job::Undo);
        self.touch();
        true
    }

    /// Submit the text on screen (spec section 12: `submit_plan` always runs FULL). A
    /// submission taken says so at once ("submitting"), before its answer, for the reason
    /// [`Editor::fix`] gives.
    pub fn submit(&mut self) -> bool {
        if self.shut_to_asks() {
            return false;
        }
        if self.text.is_none() {
            self.say("no_playbook", "");
            return false;
        }
        self.queue.push_back(Job::Submit);
        self.say("submitting", "");
        true
    }

    /// Save the notes box through `save_notes`.
    pub fn save_notes(&mut self, notes: &str) {
        if self.shut_to_asks() {
            return;
        }
        self.queue.push_back(Job::Notes {
            notes: notes.to_owned(),
        });
        self.touch();
    }

    /// Keep the text on screen as the seat's editor draft, through `save_draft`.
    pub fn save_draft(&mut self, label: &str) -> bool {
        if self.shut_to_asks() {
            return false;
        }
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
        if self.shut_to_asks() {
            return;
        }
        self.beacon_prose.clear();
        self.beacon = None;
        self.beacon_failed = None;
        self.queue.retain(|job| !matches!(job, Job::Beacon { .. }));
        self.queue.push_back(Job::Beacon { id: id.to_owned() });
        self.touch();
    }

    // --- The wizard (T19, pull request 2) -------------------------------------------------

    /// Ask for the template list again (it is read once, at the first Lull, on its own).
    pub fn list_templates(&mut self) {
        if self.shut_to_asks() {
            return;
        }
        self.queue.retain(|job| !matches!(job, Job::Templates));
        self.queue.push_back(Job::Templates);
        self.touch();
    }

    /// Open the wizard on `template_id`: `instantiate_template{suggested: true}` with no
    /// explicit value, whose answer's parameters are the pages.
    pub fn wizard_open(&mut self, template_id: &str) {
        if self.shut_to_asks() {
            return;
        }
        self.wizard_asks = self.wizard_asks.wrapping_add(1);
        self.wizard = Some(Wizard::new(template_id, self.wizard_asks));
        self.ask_wizard();
    }

    /// The player typed `text` on the page at `pointer`: it is sent exactly as typed, as an
    /// explicit value, with `suggested` still true, so every other page keeps the
    /// operator's value and mark. Returns whether that page is on screen.
    pub fn wizard_set(&mut self, pointer: &str, text: &str) -> bool {
        if self.shut_to_asks() {
            return false;
        }
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
        if self.shut_to_asks() {
            return false;
        }
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

    /// **New** (the P6a finding): put `bytes`, the empty playbook the client ships
    /// (`godot/playbooks/new.jsonc`), on screen as one edit Undo takes back, as a template's
    /// playbook is. The client claims nothing about it: its checks follow as for any edit, and
    /// they say what it still needs. It is no file's, so Save as offers no name. Returns
    /// `false` with the door shut or for bytes that are not text.
    pub fn start_new(&mut self, bytes: &[u8]) -> bool {
        if self.shut_to_asks() {
            return false;
        }
        let Ok(text) = String::from_utf8(bytes.to_vec()) else {
            self.say("load_not_text", "");
            return false;
        };
        self.queue.push_back(Job::New { text });
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

    // --- The dormant-beacon option (S1's plan, task `ui`) --------------------------------

    /// The `allow_dormant_beacons` checkbox, ticked (`on`) or cleared.
    ///
    /// Ticking it applies the verifier's own patch, the one E0601 suggests for the text on
    /// screen, so the option is written where and how the verifier says; clearing it removes
    /// the option the text sets, a patch composed from the click (AGENTS.md section 3 rule
    /// 4: "the client may compose a JSON Patch from a click"). Whether the playbook may
    /// leave beacons dark is the player's call, and what it costs is the verifier's next
    /// report: E0601 becomes W0603, or back. Refused while the rows describe an older text
    /// or an edit is on its way, as a Fix is, and when there is nothing to change.
    ///
    /// # Errors
    ///
    /// A [`DormantRefusal`], said on the status line and beside the box: the door is shut, the
    /// rows describe an older text, an edit is on its way, or there is nothing to change. Every
    /// click reaches here, so none is lost without a word (S3's `fixp`, P6b finding 9).
    pub fn set_allow_dormant(&mut self, on: bool) -> Result<(), DormantRefusal> {
        let asked = self.dormant_ask(on);
        match asked {
            Ok(()) => self.dormant_refused = None,
            Err(DormantRefusal::Shut) => {
                self.dormant_refused = Some((self.revision, DormantRefusal::Shut));
            }
            Err(refusal) => {
                self.dormant_refused = Some((self.revision, refusal));
                let key = format!("dormant_refused_{}", refusal.name());
                self.say(&key, "");
            }
        }
        self.touch();
        asked
    }

    fn dormant_ask(&mut self, on: bool) -> Result<(), DormantRefusal> {
        if self.shut_to_asks() {
            return Err(DormantRefusal::Shut);
        }
        if self.busy() {
            return Err(DormantRefusal::Busy);
        }
        if !self.rows_current() {
            return Err(DormantRefusal::Stale);
        }
        let patch = if on {
            match self.dormant_patch.clone() {
                Some(patch) => patch,
                None => return Err(DormantRefusal::NothingToChange),
            }
        } else if let Some(key) = self.text.as_deref().and_then(dormant_key) {
            // The key as the text spells it, so a hand-written camelCase option is removed
            // rather than refused by `patch_plan` for naming a key that is not there.
            json::write(&Json::Array(vec![object(vec![
                ("op", text("remove")),
                ("path", text(&format!("{OPTIONS_POINTER}/{key}"))),
            ])]))
        } else {
            return Err(DormantRefusal::NothingToChange);
        };
        self.queue.push_back(Job::SetOption { patch, on });
        self.say(
            if on {
                "dormant_allowing"
            } else {
                "dormant_disallowing"
            },
            "",
        );
        Ok(())
    }

    /// The checkbox as the panel draws it.
    #[must_use]
    pub fn dormant(&self) -> Dormant {
        let checked = self.text.as_deref().and_then(dormant_key).is_some();
        let named = self
            .rows
            .iter()
            .any(|row| DORMANT_CODES.contains(&row.code.as_str()));
        Dormant {
            shown: checked || (self.rows_current() && named),
            checked,
            enabled: self.door == Door::Open,
            refused: self
                .dormant_refused
                .filter(|(revision, _)| *revision == self.revision)
                .map(|(_, refusal)| refusal),
        }
    }

    // --- Targeting (S1's plan, task `ui`) -------------------------------------------------

    /// Every Attack beacon's target in the text on screen, in the file's order, for the map
    /// to draw ([`targeting::attack_targets`]); none without a text.
    #[must_use]
    pub fn attack_targets(&self) -> &[targeting::AttackMark] {
        &self.attack
    }

    /// Every Build target in the text on screen, in the file's order, for the map to mark in the
    /// Lull ([`targeting::build_targets`]); none without a text.
    #[must_use]
    pub fn build_targets(&self) -> &[targeting::BuildMark] {
        &self.build_marks
    }

    /// The chips: what each feature reference in the text reads now, in the file's order.
    #[must_use]
    pub fn chips(&self) -> &[Chip] {
        &self.chips
    }

    /// Whether the chips describe the text on screen.
    #[must_use]
    pub fn chips_current(&self) -> bool {
        self.text.is_some() && self.chips_revision == Some(self.revision)
    }

    /// The features the map lists, as `get_map_summary` last answered this Lull.
    #[must_use]
    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    /// The Lull's "this round" sentence, as the gateway wrote it; `None` when the last
    /// briefing carried none.
    #[must_use]
    pub fn this_round(&self) -> Option<&str> {
        self.this_round.as_deref()
    }

    // --- The rule list (S3's `ed3b`) -------------------------------------------------------

    /// Why the rule list takes no edit now, if it takes none: the door is shut, there is no
    /// text, the list describes an older text than the one on screen, or an edit is on its
    /// way. `None` when its controls take a click, as for a Fix button.
    #[must_use]
    pub fn rules_blocked(&self) -> Option<RuleEditRefusal> {
        if self.door != Door::Open {
            Some(RuleEditRefusal::Shut)
        } else if self.text.is_none() {
            Some(RuleEditRefusal::NoText)
        } else if !self.prose_current() {
            Some(RuleEditRefusal::Stale)
        } else if self.busy() {
            Some(RuleEditRefusal::Busy)
        } else {
            None
        }
    }

    /// Whether the rule list's controls take a click now ([`Editor::rules_blocked`]).
    #[must_use]
    pub fn rules_editable(&self) -> bool {
        self.rules_blocked().is_none()
    }

    /// The rendering id of the rule list on screen: what an editor opened on one of its chips
    /// or rules names back, so an edit never lands on a chip of another rendering (`set_number`,
    /// `choose`, `begin_pick`, `move_rule`). Bumped on every rule-list answer accepted, a
    /// re-render of the same text included. `None` before a rendering.
    #[must_use]
    pub const fn rules_rendering(&self) -> Option<u64> {
        self.rules_rendering
    }

    /// Chip `chip`'s number field sent `value`: one `replace` of it at the chip's pointer.
    /// `revision` and `pointer` are the rendering and the chip the field was opened on. No
    /// bound is applied here: the bounds are the gateway's, and whether the value is legal is
    /// its next answer.
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line and on the chip's rule: the list takes
    /// no edit now, it was redrawn since the field opened, there is no such chip or it is not a
    /// number, or the value is the one it holds.
    pub fn set_number(
        &mut self,
        chip: usize,
        revision: u64,
        pointer: &str,
        value: i64,
    ) -> Result<(), RuleEditRefusal> {
        let outcome = self.number_patch(chip, revision, pointer, value);
        self.ask_rule_edit(outcome, pointer, false)
    }

    /// Chip `chip`'s value field sent `text`, exactly as typed: read as a whole number
    /// ([`typed_integer`]) and sent as [`Editor::set_number`] sends one. What becomes of it is
    /// [`Editor::field_ask`], for the field's popup.
    ///
    /// # Errors
    ///
    /// [`RuleEditRefusal::NotAnInteger`] for a text that names no whole number, and every
    /// refusal [`Editor::set_number`] makes; each is said on the status line, on the chip's
    /// rule and to the field.
    pub fn set_number_text(
        &mut self,
        chip: usize,
        revision: u64,
        pointer: &str,
        text: &str,
    ) -> Result<(), RuleEditRefusal> {
        let outcome =
            typed_integer(text).and_then(|value| self.number_patch(chip, revision, pointer, value));
        let asked = self.ask_rule_edit(outcome, pointer, true);
        self.field_heard(asked)
    }

    /// A value field opened: nothing it sent before is its to say any more.
    pub fn field_opened(&mut self) {
        if self.field_ask != FieldAsk::Nothing {
            self.field_ask = FieldAsk::Nothing;
            self.touch();
        }
    }

    /// What became of the last value a value field sent.
    #[must_use]
    pub const fn field_ask(&self) -> &FieldAsk {
        &self.field_ask
    }

    /// A value field's ask was taken (it waits) or refused (with the words the status line
    /// says), and hands the outcome back.
    fn field_heard(&mut self, asked: Result<(), RuleEditRefusal>) -> Result<(), RuleEditRefusal> {
        self.field_ask = match asked {
            Ok(()) => FieldAsk::Waiting,
            Err(_) => FieldAsk::Refused {
                key: format!("status_{}", self.status.key),
                detail: self.status.detail.clone(),
            },
        };
        self.touch();
        asked
    }

    fn number_patch(
        &self,
        chip: usize,
        revision: u64,
        pointer: &str,
        value: i64,
    ) -> Result<(String, String), RuleEditRefusal> {
        let found = self.chip_drawn(chip, revision, pointer)?;
        let ChipEditor::Number { now, .. } = &found.editor else {
            return Err(RuleEditRefusal::NotThisEditor);
        };
        if *now == value {
            return Err(RuleEditRefusal::Unchanged);
        }
        Ok((
            rule_list::replace_patch(&found.pointer, Json::Number(value.to_string())),
            found.pointer.clone(),
        ))
    }

    /// Chip `chip`'s enum took its `which`th value as sent: one `replace` writing that value's
    /// JSON text, on the rendering and chip named as for [`Editor::set_number`].
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line, as for [`Editor::set_number`], and for
    /// a value the chip does not list or a value text that is not JSON.
    pub fn choose(
        &mut self,
        chip: usize,
        revision: u64,
        pointer: &str,
        which: usize,
    ) -> Result<(), RuleEditRefusal> {
        let outcome = self.choice_patch(chip, revision, pointer, which);
        self.ask_rule_edit(outcome, pointer, false)
    }

    fn choice_patch(
        &self,
        chip: usize,
        revision: u64,
        pointer: &str,
        which: usize,
    ) -> Result<(String, String), RuleEditRefusal> {
        let found = self.chip_drawn(chip, revision, pointer)?;
        let ChipEditor::Choice { values } = &found.editor else {
            return Err(RuleEditRefusal::NotThisEditor);
        };
        let text = values.get(which).ok_or(RuleEditRefusal::NoSuchValue)?;
        if *text == found.value_json {
            return Err(RuleEditRefusal::Unchanged);
        }
        // The gateway's own value text, which its contract says is JSON.
        let value = json::read(text).map_err(|_| RuleEditRefusal::NotJson)?;
        Ok((
            rule_list::replace_patch(&found.pointer, value),
            found.pointer.clone(),
        ))
    }

    /// The next map click is chip `chip`'s pick, on the rendering and chip named as for
    /// [`Editor::set_number`].
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line, as for [`Editor::set_number`], and for
    /// a chip that is not picked on the map.
    pub fn begin_pick(
        &mut self,
        chip: usize,
        revision: u64,
        pointer: &str,
    ) -> Result<(), RuleEditRefusal> {
        let outcome = self.chip_drawn(chip, revision, pointer).and_then(|found| {
            if matches!(found.editor, ChipEditor::Pick { .. }) {
                Ok(())
            } else {
                Err(RuleEditRefusal::NotThisEditor)
            }
        });
        match outcome {
            Ok(()) => {
                self.picking = Some(PickOn::Chip {
                    chip,
                    rendering: revision,
                    pointer: pointer.to_owned(),
                });
                self.say("picking", "");
                Ok(())
            }
            Err(refusal) => Err(self.refuse_rule_edit(refusal)),
        }
    }

    /// The pick is called off: the next map click opens the map's menus again.
    pub fn cancel_pick(&mut self) {
        if self.picking.take().is_some() {
            self.say("", "");
        }
    }

    /// The chip whose pick the next map click is, if one is.
    #[must_use]
    pub fn picking(&self) -> Option<usize> {
        match self.picking.as_ref() {
            Some(PickOn::Chip { chip, .. }) => Some(*chip),
            Some(PickOn::Anchor { .. }) | None => None,
        }
    }

    /// Whether the next map click is a Build target's anchor (Visit & change's "Add a build
    /// target", decisions-log item 160 (4)).
    #[must_use]
    pub const fn anchoring(&self) -> bool {
        matches!(self.picking, Some(PickOn::Anchor { .. }))
    }

    /// A map click on `target` while a pick is on: one `replace` of the picked chip with the
    /// arm the click names, when the gateway lists that arm there. A click on something the
    /// chip does not take keeps the pick on, as a click on nothing does
    /// ([`Editor::pick_missed`]): the player missed and clicks again. Any other refusal ends it.
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line: no pick is on, the list takes no edit
    /// now or was redrawn since the pick began ([`RuleEditRefusal::Changed`]), or the click
    /// names nothing this chip takes ([`RuleEditRefusal::NotThisKind`],
    /// [`RuleEditRefusal::ArmNotHere`]).
    pub fn pick(&mut self, target: &Target) -> Result<(), RuleEditRefusal> {
        if let Some(PickOn::Anchor { beacon, blueprint }) = self.picking.clone() {
            return self.pick_anchor(&beacon, blueprint, target);
        }
        let outcome = self.pick_patch(target);
        let on = match self.picking.as_ref() {
            Some(PickOn::Chip { pointer, .. }) => pointer.clone(),
            Some(PickOn::Anchor { .. }) | None => String::new(),
        };
        if !matches!(
            outcome,
            Err(RuleEditRefusal::NotThisKind(_) | RuleEditRefusal::ArmNotHere(_))
        ) {
            self.picking = None;
        }
        self.ask_rule_edit(outcome, &on, false)
    }

    /// A map click on nothing (no beacon, feature or voxel) while a pick is on: refused as
    /// [`RuleEditRefusal::NoTarget`], and the pick stays on, as for a click on something the
    /// chip does not take.
    ///
    /// # Errors
    ///
    /// Always: [`RuleEditRefusal::NoTarget`], or [`RuleEditRefusal::NotPicking`] with no pick
    /// on.
    pub fn pick_missed(&mut self) -> Result<(), RuleEditRefusal> {
        let refusal = if self.picking.is_some() {
            RuleEditRefusal::NoTarget
        } else {
            RuleEditRefusal::NotPicking
        };
        Err(self.refuse_rule_edit(refusal))
    }

    fn pick_patch(&self, target: &Target) -> Result<(String, String), RuleEditRefusal> {
        let Some(PickOn::Chip {
            chip,
            rendering,
            pointer,
        }) = self.picking.as_ref()
        else {
            return Err(RuleEditRefusal::NotPicking);
        };
        let found = self.chip_drawn(*chip, *rendering, pointer)?;
        let ChipEditor::Pick { kind, arms } = &found.editor else {
            return Err(RuleEditRefusal::NotThisEditor);
        };
        let value =
            rule_list::picked_value(*kind, arms, target).map_err(|refusal| match refusal {
                rule_list::PickRefusal::ArmNotHere(arm) => RuleEditRefusal::ArmNotHere(arm),
                rule_list::PickRefusal::NotThisKind => RuleEditRefusal::NotThisKind(kind.name()),
            })?;
        Ok((
            rule_list::replace_patch(&found.pointer, value),
            found.pointer.clone(),
        ))
    }

    /// A drag of rule `from` onto rule `to` of the rendering `revision`: one `move` of it to
    /// that rule's place, within their own array.
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line: the list takes no edit now or was
    /// redrawn since the drag began, a rule is not there, the drop is on the dragged rule
    /// itself, or on a rule of another array (a step onto a rule).
    pub fn move_rule(&mut self, from: &RuleNamed, to: &RuleNamed) -> Result<(), RuleEditRefusal> {
        let outcome = self.move_patch(from, to);
        self.ask_rule_edit(outcome, &from.pointer, false)
    }

    fn move_patch(
        &self,
        from: &RuleNamed,
        to: &RuleNamed,
    ) -> Result<(String, String), RuleEditRefusal> {
        if let Some(blocked) = self.rules_blocked() {
            return Err(blocked);
        }
        let moved = self.rule_drawn(from)?;
        let onto = self.rule_drawn(to)?;
        if from.index == to.index {
            return Err(RuleEditRefusal::OntoItself);
        }
        if moved.parent() != onto.parent() {
            return Err(RuleEditRefusal::OtherList);
        }
        Ok((
            rule_list::move_patch(&moved.pointer, &onto.pointer),
            moved.pointer.clone(),
        ))
    }

    /// Says `refusal` on the status line, and hands it back: a rule-list ask the bridge could
    /// not even name (an index GDScript sent that no `usize` holds) is refused through this,
    /// as every other is. A shut door is said as the door's own line.
    pub fn refuse_rule_edit(&mut self, refusal: RuleEditRefusal) -> RuleEditRefusal {
        if refusal == RuleEditRefusal::Shut {
            let key = format!("closed_{}", self.door.name());
            self.say(&key, "");
        } else {
            let key = format!("rule_refused_{}", refusal.name());
            self.say(&key, refusal.detail());
        }
        refusal
    }

    /// The gateway's refusal of the last rule-list edit, while the text it was asked of is on
    /// screen.
    #[must_use]
    pub fn rule_refusal(&self) -> Option<&RuleRefusal> {
        self.rule_refusal
            .as_ref()
            .filter(|refusal| refusal.revision == self.revision)
    }

    /// The rule pairs the conflict connector joins: for each W0702 row
    /// ([`rule_list::CONFLICT_CODE`]), the rule holding its path (the later rule) and the
    /// rule holding its related path (the earlier one that wins). Only while the rows and the
    /// rule list both describe the text on screen; a row whose paths no rendered rule holds
    /// joins nothing.
    #[must_use]
    pub fn conflicts(&self) -> Vec<Conflict> {
        if !self.rows_current() || !self.prose_current() {
            return Vec::new();
        }
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.code == rule_list::CONFLICT_CODE)
            .filter_map(|(index, row)| {
                let later = self.rule_list.rule_holding(&row.pointer)?;
                let earlier = row
                    .related
                    .iter()
                    .find_map(|related| self.rule_list.rule_holding(related))?;
                Some(Conflict {
                    row: index,
                    earlier,
                    later,
                    sentence: row.sentence.clone(),
                })
            })
            .collect()
    }

    /// Rule `named.index` of the rule list on screen, when it is the rule a drag named: the
    /// same rendering and the same pointer.
    fn rule_drawn(&self, named: &RuleNamed) -> Result<&rule_list::RuleRow, RuleEditRefusal> {
        if self.rules_rendering != Some(named.rendering) {
            return Err(RuleEditRefusal::Changed);
        }
        let found = self
            .rule_list
            .rules
            .get(named.index)
            .ok_or(RuleEditRefusal::NoSuchRule)?;
        if found.pointer != named.pointer {
            return Err(RuleEditRefusal::Changed);
        }
        Ok(found)
    }

    /// Chip `chip` of the rule list on screen, when the list takes an edit now and the chip
    /// is the one an editor was opened on: the same rendering and the same pointer.
    fn chip_drawn(
        &self,
        chip: usize,
        revision: u64,
        pointer: &str,
    ) -> Result<&rule_list::RuleChip, RuleEditRefusal> {
        if let Some(blocked) = self.rules_blocked() {
            return Err(blocked);
        }
        if self.rules_rendering != Some(revision) {
            return Err(RuleEditRefusal::Changed);
        }
        let found = self
            .rule_list
            .chips
            .get(chip)
            .ok_or(RuleEditRefusal::NoSuchChip)?;
        if found.pointer != pointer {
            return Err(RuleEditRefusal::Changed);
        }
        Ok(found)
    }

    /// Queues a composed rule-list edit, or says why there is none: on the status line, and on
    /// the rule holding `on`, the pointer the control named (S3's `fixp`). `field` says whether a
    /// value field sent it.
    fn ask_rule_edit(
        &mut self,
        outcome: Result<(String, String), RuleEditRefusal>,
        on: &str,
        field: bool,
    ) -> Result<(), RuleEditRefusal> {
        match outcome {
            Ok((patch, pointer)) => {
                self.rule_refusal = None;
                self.queue.push_back(Job::RuleEdit {
                    patch,
                    pointer,
                    field,
                });
                self.say("rule_editing", "");
                Ok(())
            }
            Err(refusal) => {
                let refusal = self.refuse_rule_edit(refusal);
                if !on.is_empty() {
                    self.rule_refusal = Some(RuleRefusal {
                        pointer: on.to_owned(),
                        key: format!("status_{}", self.status.key),
                        detail: self.status.detail.clone(),
                        revision: self.revision,
                    });
                }
                Err(refusal)
            }
        }
    }

    // --- The condition picker and the name picks (S3's `ed3b`, part 2) -------------------

    /// The condition picker took predicate `entry` of family `family`, as `get_vocabulary`
    /// sent them, for condition slot `slot` of the rendering `rendering` (named, as a chip is, by
    /// its index, the rendering the picker opened on and its `pointer`). One JSON Patch: the
    /// predicate's skeleton wrapped under its name, sent with exactly the op the slot names
    /// (`add` where nothing stands, `replace` where something does, an array item in place): the
    /// client never chooses one. The parameters then edit as chips once the new rendering
    /// carries them.
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line: the list takes no edit now, it was
    /// redrawn since the picker opened, there is no such slot, the slot is a group or names no op
    /// this build knows, it is not inside the rule it was sent under, the vocabulary was not
    /// read this Lull, there is no such entry, or the gateway locked it or its family.
    pub fn choose_predicate(
        &mut self,
        slot: usize,
        rendering: u64,
        pointer: &str,
        family: usize,
        entry: usize,
    ) -> Result<(), RuleEditRefusal> {
        let outcome = self.predicate_patch(slot, rendering, pointer, family, entry);
        self.ask_rule_edit(outcome, pointer, false)
    }

    fn predicate_patch(
        &self,
        slot: usize,
        rendering: u64,
        pointer: &str,
        family: usize,
        entry: usize,
    ) -> Result<(String, String), RuleEditRefusal> {
        if let Some(blocked) = self.rules_blocked() {
            return Err(blocked);
        }
        if self.rules_rendering != Some(rendering) {
            return Err(RuleEditRefusal::Changed);
        }
        let found = self
            .rule_list
            .slots
            .get(slot)
            .ok_or(RuleEditRefusal::NoSuchSlot)?;
        if found.pointer != pointer {
            return Err(RuleEditRefusal::Changed);
        }
        let present = match (found.state, found.op) {
            (rule_list::SlotState::Group, _) => return Err(RuleEditRefusal::GroupSlot),
            (rule_list::SlotState::Empty | rule_list::SlotState::Leaf, rule_list::SlotOp::Add) => {
                false
            }
            (
                rule_list::SlotState::Empty | rule_list::SlotState::Leaf,
                rule_list::SlotOp::Replace,
            ) => true,
            (_, rule_list::SlotOp::None) | (rule_list::SlotState::Unknown, _) => {
                return Err(RuleEditRefusal::NoPickOp);
            }
        };
        let inside = found
            .rule
            .and_then(|index| self.rule_list.rules.get(index))
            .is_some_and(|rule| rule.holds(&found.pointer) && rule.pointer != found.pointer);
        if !inside {
            return Err(RuleEditRefusal::NotInRule);
        }
        let words = self
            .vocabulary
            .as_ref()
            .ok_or(RuleEditRefusal::NoVocabulary)?;
        let value = vocabulary::predicate_value(words, family, entry)?;
        Ok((
            vocabulary::slot_patch(&found.pointer, value, present),
            found.pointer.clone(),
        ))
    }

    /// Name chip `chip` took its `which`th choice, from the labels or rule ids the text on
    /// screen holds in file order ([`Editor::name_choices`]): one `replace` writing it, on the
    /// rendering and chip named as for [`Editor::set_number`]. Whether the name is legal there (a
    /// later step, a rule that exists) is the gateway's check.
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line, as for [`Editor::choose`].
    pub fn choose_name(
        &mut self,
        chip: usize,
        revision: u64,
        pointer: &str,
        which: usize,
    ) -> Result<(), RuleEditRefusal> {
        let outcome = self.name_patch(chip, revision, pointer, which);
        self.ask_rule_edit(outcome, pointer, false)
    }

    fn name_patch(
        &self,
        chip: usize,
        revision: u64,
        pointer: &str,
        which: usize,
    ) -> Result<(String, String), RuleEditRefusal> {
        let found = self.chip_drawn(chip, revision, pointer)?;
        let ChipEditor::Name(kind) = &found.editor else {
            return Err(RuleEditRefusal::NotThisEditor);
        };
        let name = self
            .name_choices(*kind)
            .get(which)
            .ok_or(RuleEditRefusal::NoSuchValue)?;
        let value = Json::String(name.clone());
        if json::read(&found.value_json).is_ok_and(|now| now == value) {
            return Err(RuleEditRefusal::Unchanged);
        }
        Ok((
            rule_list::replace_patch(&found.pointer, value),
            found.pointer.clone(),
        ))
    }

    /// The names a name chip of `kind` is picked from: the text on screen's step labels or rule
    /// ids, in file order, each once.
    #[must_use]
    pub fn name_choices(&self, kind: NameKind) -> &[String] {
        match kind {
            NameKind::StepLabel => &self.step_labels,
            NameKind::RuleId => &self.rule_ids,
        }
    }

    /// Ask for the vocabulary once more (the picker's "Ask again", offered when its read was
    /// refused or could not be read). Returns `false`, asking nothing, with the door shut or
    /// with a read already queued or on its way: one more call within the seat's budget.
    pub fn ask_vocabulary_again(&mut self) -> bool {
        if self.shut_to_asks() {
            return false;
        }
        let waiting = self.queue.iter().any(|job| matches!(job, Job::Vocabulary))
            || matches!(self.in_flight, Some(Sent::Vocabulary { .. }));
        if waiting {
            return false;
        }
        self.vocabulary_failed = None;
        self.queue.push_back(Job::Vocabulary);
        self.touch();
        true
    }

    /// The words the pickers draw, as `get_vocabulary` answered this Lull.
    #[must_use]
    pub const fn vocabulary(&self) -> Option<&Vocabulary> {
        self.vocabulary.as_ref()
    }

    /// Why this Lull's vocabulary did not come, when it did not.
    #[must_use]
    pub const fn vocabulary_failed(&self) -> Option<&ReadFailed> {
        self.vocabulary_failed.as_ref()
    }

    /// The detail of the beacon last described, as `get_beacon` answered.
    #[must_use]
    pub const fn beacon_detail(&self) -> Option<&BeaconDetail> {
        self.beacon.as_ref()
    }

    /// Why the beacon's detail did not come, when it did not.
    #[must_use]
    pub const fn beacon_failed(&self) -> Option<&ReadFailed> {
        self.beacon_failed.as_ref()
    }

    // --- Visit & change rows (S3's `ed3b`, part 2; register row S3-01) ------------------

    /// Setting `setting` of beacon `beacon`'s detail (named by its `pointer` too) to the number
    /// `value`: one route step visiting the beacon with one `set_mandate_settings` row, nested
    /// from the setting's pointer.
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line: the door is shut, there is no text, no
    /// detail of that beacon is on hand, no such setting, it is not a number, the value cannot
    /// be held exactly, or it is the value now.
    pub fn change_setting_number(
        &mut self,
        beacon: &str,
        setting: usize,
        pointer: &str,
        value: i64,
    ) -> Result<(), RuleEditRefusal> {
        let outcome = self.setting_number_row(beacon, setting, pointer, value);
        self.ask_visit(beacon, outcome, false)
    }

    /// Setting `setting` of beacon `beacon`'s detail took `text` from its value field, exactly
    /// as typed: read as a whole number ([`typed_integer`]) and sent as
    /// [`Editor::change_setting_number`] sends one. What becomes of it is
    /// [`Editor::field_ask`].
    ///
    /// # Errors
    ///
    /// [`RuleEditRefusal::NotAnInteger`] for a text that names no whole number, and every
    /// refusal [`Editor::change_setting_number`] makes; each is said on the status line and to
    /// the field.
    pub fn change_setting_number_text(
        &mut self,
        beacon: &str,
        setting: usize,
        pointer: &str,
        text: &str,
    ) -> Result<(), RuleEditRefusal> {
        let outcome = typed_integer(text)
            .and_then(|value| self.setting_number_row(beacon, setting, pointer, value));
        let asked = self.ask_visit(beacon, outcome, true);
        self.field_heard(asked)
    }

    fn setting_number_row(
        &self,
        beacon: &str,
        setting: usize,
        pointer: &str,
        value: i64,
    ) -> Result<Json, RuleEditRefusal> {
        let found = self.setting_drawn(beacon, setting, pointer)?;
        let ChipEditor::Number { .. } = &found.editor else {
            return Err(RuleEditRefusal::NotThisEditor);
        };
        // No `Unchanged` here: the detail is the beacon as the Lull sees it, and an earlier
        // step may change it, so setting it back is a row the verifier judges.
        let settings = vocabulary::nested_setting(&found.pointer, Json::Number(value.to_string()))?;
        Ok(vocabulary::settings_row(settings))
    }

    /// Setting `setting` of beacon `beacon`'s detail took its `which`th value as sent: one route
    /// step with one `set_mandate_settings` row.
    ///
    /// # Errors
    ///
    /// As [`Editor::change_setting_number`], and for a value the setting does not list or one
    /// that is not JSON.
    pub fn change_setting_choice(
        &mut self,
        beacon: &str,
        setting: usize,
        pointer: &str,
        which: usize,
    ) -> Result<(), RuleEditRefusal> {
        let outcome = self.setting_choice_row(beacon, setting, pointer, which);
        self.ask_visit(beacon, outcome, false)
    }

    fn setting_choice_row(
        &self,
        beacon: &str,
        setting: usize,
        pointer: &str,
        which: usize,
    ) -> Result<Json, RuleEditRefusal> {
        let found = self.setting_drawn(beacon, setting, pointer)?;
        let ChipEditor::Choice { values } = &found.editor else {
            return Err(RuleEditRefusal::NotThisEditor);
        };
        // No `Unchanged`, as for a number: the verifier says what the row does.
        let text = values.get(which).ok_or(RuleEditRefusal::NoSuchValue)?;
        let value = json::read(text).map_err(|_| RuleEditRefusal::NotJson)?;
        let settings = vocabulary::nested_setting(&found.pointer, value)?;
        Ok(vocabulary::settings_row(settings))
    }

    /// Switch beacon `beacon` to mandate `which`, as `get_vocabulary` sent it: one route step
    /// with one `set_mandate` row, the mandate's skeleton under its name.
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line: the door is shut, there is no text, no
    /// detail of that beacon is on hand, the vocabulary was not read, no such mandate, or the
    /// gateway locked it.
    pub fn switch_mandate(&mut self, beacon: &str, which: usize) -> Result<(), RuleEditRefusal> {
        let outcome = self.visit_gate(beacon).and_then(|()| {
            let mandate = self
                .words()?
                .mandates
                .get(which)
                .ok_or(RuleEditRefusal::NoSuchEntry)?;
            Ok(vocabulary::mandate_row(mandate)?)
        });
        self.ask_visit(beacon, outcome, false)
    }

    /// Add a Build target of blueprint `which` (as `get_vocabulary` sent it) at beacon
    /// `beacon`: the next map click on ground is its anchor ([`Editor::pick`]). Offered on every
    /// own beacon: whether the beacon carries a mandate that takes a target is the verifier's
    /// to say.
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line: the door is shut, there is no text, no
    /// detail of that beacon is on hand, the vocabulary was not read, no such blueprint, or the
    /// gateway locked it.
    pub fn begin_build_target(
        &mut self,
        beacon: &str,
        which: usize,
    ) -> Result<(), RuleEditRefusal> {
        let outcome = self.visit_gate(beacon).and_then(|()| {
            let blueprint = self
                .words()?
                .blueprints
                .get(which)
                .ok_or(RuleEditRefusal::NoSuchEntry)?;
            vocabulary::usable_name(blueprint)?;
            Ok(())
        });
        match outcome {
            Ok(()) => {
                self.picking = Some(PickOn::Anchor {
                    beacon: beacon.to_owned(),
                    blueprint: which,
                });
                self.say("anchoring", "");
                Ok(())
            }
            Err(refusal) => Err(self.refuse_rule_edit(refusal)),
        }
    }

    /// The anchor pick's click: ground gives the target's anchor as a voxel, a vent gives it
    /// `on` the feature by its name, and either makes one route step with one
    /// `add_build_target` row. A click on a beacon or a selector is refused with its words
    /// (`NotThisKind("location")`) and the pick stays on, for the player to click again.
    fn pick_anchor(
        &mut self,
        beacon: &str,
        blueprint: usize,
        target: &Target,
    ) -> Result<(), RuleEditRefusal> {
        let anchor = match target {
            Target::Voxel(at) => voxel_location(*at),
            Target::Feature(id) => vocabulary::on_feature(id),
            Target::Beacon(_) | Target::Selector(_) | Target::NearestUncovered(_) => {
                return Err(self.refuse_rule_edit(RuleEditRefusal::NotThisKind("location")));
            }
        };
        self.picking = None;
        let outcome = self.visit_gate(beacon).and_then(|()| {
            let entry = self
                .words()?
                .blueprints
                .get(blueprint)
                .ok_or(RuleEditRefusal::NoSuchEntry)?;
            Ok(vocabulary::add_target_row(entry, anchor)?)
        });
        self.ask_visit(beacon, outcome, false)
    }

    /// Remove beacon `beacon`'s Build target `which`, as its detail lists it: one route step
    /// with one `remove_build_target` row naming the target's anchor as sent.
    ///
    /// # Errors
    ///
    /// A [`RuleEditRefusal`], said on the status line: the door is shut, there is no text, no
    /// detail of that beacon is on hand, or it lists no such target.
    pub fn remove_build_target(
        &mut self,
        beacon: &str,
        which: usize,
    ) -> Result<(), RuleEditRefusal> {
        let outcome = self.visit_gate(beacon).and_then(|()| {
            let target = self
                .beacon
                .as_ref()
                .and_then(|detail| detail.targets.get(which))
                .ok_or(RuleEditRefusal::NoSuchEntry)?;
            Ok(vocabulary::remove_target_row(target))
        });
        self.ask_visit(beacon, outcome, false)
    }

    /// Queue blueprint `which`, as `get_vocabulary` sent it, at beacon `beacon`: one route step
    /// with one `queue_structure` row.
    ///
    /// # Errors
    ///
    /// As [`Editor::switch_mandate`], for a blueprint.
    pub fn queue_structure(&mut self, beacon: &str, which: usize) -> Result<(), RuleEditRefusal> {
        let outcome = self.visit_gate(beacon).and_then(|()| {
            let blueprint = self
                .words()?
                .blueprints
                .get(which)
                .ok_or(RuleEditRefusal::NoSuchEntry)?;
            Ok(vocabulary::queue_row(blueprint)?)
        });
        self.ask_visit(beacon, outcome, false)
    }

    /// Whether a Visit & change row for `beacon` can be composed now: the door is open, there is
    /// a text to append to, and the detail on hand is that beacon's.
    fn visit_gate(&self, beacon: &str) -> Result<(), RuleEditRefusal> {
        if self.door != Door::Open {
            return Err(RuleEditRefusal::Shut);
        }
        if self.text.is_none() {
            return Err(RuleEditRefusal::NoText);
        }
        match self.beacon.as_ref() {
            Some(detail) if detail.id == beacon => Ok(()),
            _ => Err(RuleEditRefusal::NoDetail),
        }
    }

    /// Setting `setting` of beacon `beacon`'s detail, when it is the one a menu named.
    fn setting_drawn(
        &self,
        beacon: &str,
        setting: usize,
        pointer: &str,
    ) -> Result<&vocabulary::Setting, RuleEditRefusal> {
        self.visit_gate(beacon)?;
        let found = self
            .beacon
            .as_ref()
            .and_then(|detail| detail.settings.get(setting))
            .ok_or(RuleEditRefusal::NoSuchEntry)?;
        if found.pointer != pointer {
            return Err(RuleEditRefusal::Changed);
        }
        Ok(found)
    }

    fn words(&self) -> Result<&Vocabulary, RuleEditRefusal> {
        self.vocabulary
            .as_ref()
            .ok_or(RuleEditRefusal::NoVocabulary)
    }

    /// Queues a composed Visit & change row, or says why there is none.
    fn ask_visit(
        &mut self,
        beacon: &str,
        outcome: Result<Json, RuleEditRefusal>,
        field: bool,
    ) -> Result<(), RuleEditRefusal> {
        match outcome {
            Ok(row) => {
                self.queue.push_back(Job::Visit {
                    beacon: beacon.to_owned(),
                    row,
                    field,
                });
                self.say("visit_changing", "");
                Ok(())
            }
            Err(refusal) => Err(self.refuse_rule_edit(refusal)),
        }
    }

    // --- What the rig tells the editor --------------------------------------------------

    /// A Lull opened, in `round`. The notes and the drafts are read again, the template list
    /// the first time, and a text the editor already holds is checked and rendered again
    /// against the new snapshot; an open wizard asks again, because the operator's
    /// suggestion is made afresh at each Lull's start.
    pub fn lull_opened(&mut self, round: u32) {
        let new_round = round != self.round;
        self.round = round;
        if new_round {
            // Last round's sentence and features describe last round's map, and a filing
            // was last round's: so is its line, which no carried draft replaces, because the
            // gateway carries nothing after a round it filed (item 143 (3)).
            self.this_round = None;
            self.features.clear();
            self.filed = None;
            if self.status.key == "safe_filed" {
                self.say("", "");
            }
            // A licence is earned in a Push, so last Lull's words are stale, and a beacon's
            // detail described the world before it.
            self.vocabulary = None;
            self.vocabulary_failed = None;
            self.beacon = None;
            self.beacon_failed = None;
        }
        // The vocabulary every Lull, never carried across one (decisions-log item 160 (4)), and again in
        // the same Lull only when it is not on hand.
        if self.vocabulary.is_none() {
            self.queue.retain(|job| !matches!(job, Job::Vocabulary));
            self.queue.push_back(Job::Vocabulary);
        }
        // The briefing every Lull, not only the first: besides the notebook it carries the
        // Lull's "this round" sentence (item 133 (3) (i)), which the new snapshot changes.
        self.queue
            .retain(|job| !matches!(job, Job::Briefing | Job::MapSummary));
        self.queue.push_back(Job::Briefing);
        if !self.known.templates {
            self.queue.push_back(Job::Templates);
        }
        self.queue.push_back(Job::Drafts);
        // The features the vent click picks from: their coverage moves every round.
        self.queue.push_back(Job::MapSummary);
        if new_round && self.text.is_some() {
            self.owed.quick = true;
            self.owed.estimate = true;
            self.resolve_owed = true;
            self.render_owed = true;
            self.full_done = None;
            self.owed.edited = true;
            self.check_refusals = CheckRefusals::default();
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

    /// Whether the editor's calls can reach the gateway now ([`Door`]). Shutting the door
    /// drops what was queued and will never go out: the player's asks (the status line says
    /// so when there were any), the checks owed (each then says why it will not come), a
    /// ghost still waiting for its check, and a wizard still waiting for its answer. What is
    /// in flight is answered as usual. Opening it again within the same Lull (a refused
    /// Ready) owes the text's checks again; a new Lull's are owed by
    /// [`Editor::lull_opened`].
    pub fn set_door(&mut self, door: Door) {
        if door == self.door {
            return;
        }
        let was_open = self.door == Door::Open;
        self.door = door;
        // A line about the door that just changed is not true any more, shut to shut as much as
        // shut to open: "You pressed Ready" in the Push, "closed during the Push" at the end.
        if self.status.key.starts_with("closed_") {
            self.say("", "");
        }
        if door == Door::Open {
            if self.text.is_some() {
                self.owe_checks();
            }
        } else if was_open && self.shut() {
            self.say(&format!("closed_{}", door.name()), "");
        }
        self.touch();
    }

    /// What the verifier counted, from the last report on the text on screen, or `None`
    /// with no text or before the first report (S3's plan, E9).
    #[must_use]
    pub const fn size_meter(&self) -> Option<&SizeMeter> {
        self.size.as_ref()
    }

    /// The door, as the rig last set it.
    #[must_use]
    pub const fn door(&self) -> Door {
        self.door
    }

    /// The gateway filed this seat's safe playbook for the round: its `safe_playbook_filed`
    /// line, `said`, came in the seat's own feed. The status line says so with the
    /// gateway's words and that the text on screen was not sealed; the "this round" sentence,
    /// which described orders that were not sealed, goes. The filed playbook is not shown:
    /// no read the seat's token holds returns it once the Push has begun (`get_safe_plan` is
    /// a planning method, closed in a Push, and answers what would be filed, not what was).
    pub fn safe_playbook_filed(&mut self, said: &str) {
        self.filed = Some(said.to_owned());
        self.this_round = None;
        self.say("safe_filed", said);
    }

    /// Whether the gateway filed this seat's safe playbook for the round under way.
    #[must_use]
    pub const fn filed(&self) -> bool {
        self.filed.is_some()
    }

    /// Why `check` of the text on screen will not come, if it will not: the gateway refused
    /// that check of this text, or the door is shut. Read only where that check is not
    /// current; another check's refusal is not this one's.
    #[must_use]
    pub fn unchecked(&self, check: Check) -> Option<Unchecked> {
        if let Some((revision, why)) = self.check_refusals.get(check)
            && *revision == self.revision
        {
            return Some(why.clone());
        }
        (self.door != Door::Open).then_some(Unchecked::Closed(self.door))
    }

    /// What the verdict line says after the verifier's word ([`SealLine`]).
    #[must_use]
    pub fn seal_line(&self) -> SealLine {
        if self.door != Door::Open {
            return if self.filed.is_some() {
                SealLine::Filed
            } else {
                SealLine::None
            };
        }
        match self.verdict() {
            Verdict::None | Verdict::Refused => SealLine::None,
            Verdict::Quick | Verdict::Full | Verdict::Submitted => {
                if self.qualifies() {
                    SealLine::Ready
                } else {
                    SealLine::NotReady
                }
            }
        }
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
        // The rig asks only with the door open; a shut door sends nothing whoever asks.
        if self.in_flight.is_some() || self.door != Door::Open {
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
        // 3. The route, priced; then the chips, what each feature reference reads now.
        if self.owed.estimate {
            self.owed.estimate = false;
            if let Some(call) = self.estimate_call() {
                return Some(call);
            }
        }
        if self.resolve_owed {
            self.resolve_owed = false;
            if let Some(call) = self.resolve_call() {
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
        if full_due
            && self.full_done != Some(self.revision)
            && let Some(text) = self.text.clone()
        {
            let revision = self.revision;
            let mut call = self.send(
                Sent::Full { revision },
                "verify_plan",
                verify_params(&text, "full"),
            );
            call.full = true;
            return Some(call);
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
            && !self.resolve_owed
            && !self.render_owed
            && self.in_flight.is_none()
    }

    /// Whether an edit landed since the last call: the rig restarts the idle timer.
    pub fn take_edit_mark(&mut self) -> bool {
        std::mem::take(&mut self.owed.edited)
    }

    /// The answer to the call in flight: a result, or the gateway's refusal as
    /// `(code, message)`. Says whether it was an answer or a refused call: an
    /// `estimate_route` whose waypoint resolves to nothing is the gateway's answer about
    /// the route, drawn as such ([`Route::found_nothing`]), and not a refusal.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Schema`] when a result is not the message its method answers with;
    /// the call is settled either way and nothing waits on it.
    pub fn answered(
        &mut self,
        answer: Result<&Json, (&str, &str)>,
    ) -> Result<Settled, BridgeError> {
        let Some(sent) = self.in_flight.take() else {
            return Ok(Settled::Answered);
        };
        self.touch();
        let check = check_of(&sent);
        let settled = match answer {
            Ok(result) => self.settle(sent, result).map(|()| Settled::Answered),
            Err((code, message)) => Ok(self.refused(&sent, code, message)),
        };
        // An answer to a check of the text on screen that this client could not read will not
        // be asked again by itself: its line says so rather than wait for ever (M4).
        if let (Err(error), Some((check, revision))) = (&settled, check)
            && revision == self.revision
        {
            *self.check_refusals.slot(check) =
                Some((revision, Unchecked::Unreadable(error.to_string())));
        }
        // An answer that lands after the door shut (sent in the Lull's last moment) may owe
        // more: a submission's briefing, a preview's check, a new text's checks. None of it
        // will go out, so none of it is kept waiting.
        if self.door != Door::Open {
            self.shut();
        }
        settled
    }

    /// The seat connection dropped with a call in flight: an edit or a request the player
    /// made is asked again once it is back; a check is owed again. With the door shut
    /// (the call went out in the Lull's last moment), nothing is kept for a Lull that may
    /// never come: what would be asked again is dropped as [`Editor::set_door`] drops it,
    /// and the status line says the ask was not sent (finding K).
    ///
    /// Asking again is safe for every one of them: `patch_plan` is stateless on the gateway
    /// (it hands back the patched text and keeps nothing), the text on screen is still the
    /// one the lost edit was patched against, and `save_notes` replaces the notebook whole.
    pub fn dropped(&mut self) {
        let Some(sent) = self.in_flight.take() else {
            return;
        };
        match sent {
            Sent::Load { candidate, file } => self.queue.push_front(Job::Load { candidate, file }),
            Sent::Edit { base, retry } => {
                if base == self.revision {
                    self.queue.push_front(*retry);
                } else if field_sent(&retry) {
                    self.field_ask = FieldAsk::Refused {
                        key: "status_rule_refused_changed".to_owned(),
                        detail: String::new(),
                    };
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
            Sent::Resolve { .. } => self.resolve_owed = true,
            Sent::MapSummary => self.queue.push_front(Job::MapSummary),
            Sent::Vocabulary { round } => {
                if round == self.round {
                    self.queue.push_front(Job::Vocabulary);
                }
            }
        }
        if self.door != Door::Open && self.shut() {
            let key = format!("closed_{}", self.door.name());
            self.say(&key, "");
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
        self.verdict
    }

    /// Whether the rows describe the text on screen.
    #[must_use]
    pub fn rows_current(&self) -> bool {
        !matches!(self.verdict, Verdict::None | Verdict::Refused)
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

    /// Whether the gateway accepted a submission from this editor in the round under way, so
    /// Ready keeps this round's seal rather than file the safe playbook. What this process saw;
    /// a resumed process learns it from the gateway's own word on `get_status`, which the wire
    /// does not carry yet (lane `probw`; S3's `fixp`, TODO 1).
    #[must_use]
    pub fn sealed_this_round(&self) -> bool {
        self.sealed_round == Some(self.round)
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
        &self.rule_list.lines
    }

    /// The rule list: lines, rules and chips, as `render_plan` last answered.
    #[must_use]
    pub const fn rule_list(&self) -> &RuleList {
        &self.rule_list
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

    /// An ask made with the door shut: refused at once, with the reason on the status line,
    /// rather than queued behind a door the rig will not open (finding K).
    fn shut_to_asks(&mut self) -> bool {
        if self.door == Door::Open {
            return false;
        }
        let key = format!("closed_{}", self.door.name());
        self.say(&key, "");
        true
    }

    /// Drops what the shut door will never let out. Returns whether a player's ask was among
    /// it, so the status line can say it was not sent.
    fn shut(&mut self) -> bool {
        let asked = self.queue.iter().any(|job| {
            !matches!(
                job,
                Job::Briefing | Job::Drafts | Job::Templates | Job::MapSummary | Job::Vocabulary
            )
        });
        let waiting_ghost = self
            .ghost
            .as_ref()
            .is_some_and(|ghost| ghost.state == GhostState::Waiting);
        // A value field's edit still queued will not go out: its popup says why.
        if self.queue.iter().any(field_sent) {
            self.field_ask = FieldAsk::Refused {
                key: format!("status_closed_{}", self.door.name()),
                detail: String::new(),
            };
        }
        self.queue.clear();
        self.picking = None;
        self.owed.quick = false;
        self.owed.estimate = false;
        self.resolve_owed = false;
        self.render_owed = false;
        if waiting_ghost
            && !matches!(
                self.in_flight,
                Some(Sent::Preview { .. } | Sent::PreviewCheck)
            )
        {
            self.ghost = None;
            self.preview = None;
        }
        if self
            .wizard
            .as_ref()
            .is_some_and(|wizard| wizard.answered != wizard.asked && wizard.refusal.is_none())
            && !matches!(self.in_flight, Some(Sent::Instantiate { .. }))
        {
            self.wizard = None;
        }
        asked || waiting_ghost
    }

    /// Every check of the text on screen is owed again.
    fn owe_checks(&mut self) {
        self.check_refusals = CheckRefusals::default();
        self.owed.quick = true;
        self.owed.estimate = true;
        self.resolve_owed = true;
        self.render_owed = true;
        self.owed.edited = true;
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
            Job::Load { candidate, file } => {
                let params = verify_params(&candidate, "quick");
                self.send(Sent::Load { candidate, file }, "verify_plan", params)
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
            job @ (Job::Beacon { .. }
            | Job::Templates
            | Job::Instantiate
            | Job::UseWizard { .. }
            | Job::New { .. }
            | Job::RuleEdit { .. }
            | Job::GetDraft { .. }
            | Job::MapSummary
            | Job::SetOption { .. }
            | Job::Vocabulary
            | Job::Visit { .. }) => return self.render_pr2(job),
        })
    }

    /// Undo: the inverse patch through `patch_plan`, or, for the wizard's playbook, the text
    /// before it put back as it was, with no call.
    fn render_undo(&mut self) -> Option<Call> {
        let patch = match self.undo.last()? {
            Undo::Patch(patch) => patch.clone(),
            Undo::Restore { .. } => {
                if let Some(Undo::Restore { text, file }) = self.undo.pop() {
                    self.drop_settled_ghost();
                    self.restore(text, file);
                    self.say("undone", "");
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
                // The template's playbook is no file's: Save as offers no name for it, and
                // Undo gives the text before it back with the file that text came from.
                self.undo.push(Undo::Restore {
                    text: self.text.clone(),
                    file: self.file.take(),
                });
                self.drop_settled_ghost();
                self.accept_text(text);
                self.say("wizard_used", "");
                None
            }
            Job::RuleEdit {
                patch,
                pointer,
                field,
            } => {
                let params = patch_params(self.text.as_deref()?, &patch);
                let retry = Job::RuleEdit {
                    patch,
                    pointer,
                    field,
                };
                Some(self.send_patch(params, retry))
            }
            Job::New { text } => {
                // As a template's playbook: no file's, and Undo gives the text before it back
                // with its file. Another text is on screen, so an open wizard's pages go.
                self.undo.push(Undo::Restore {
                    text: self.text.clone(),
                    file: self.file.take(),
                });
                self.drop_settled_ghost();
                self.wizard_close();
                self.accept_text(text);
                self.say("new_opened", "");
                None
            }
            Job::GetDraft { id } => {
                let round = self.round;
                let params = object(vec![("draft_id", Json::String(id.clone()))]);
                Some(self.send(Sent::GetDraft { id, round }, "get_draft", params))
            }
            Job::MapSummary => {
                Some(self.send(Sent::MapSummary, "get_map_summary", object(Vec::new())))
            }
            Job::SetOption { patch, on } => {
                let playbook = self.text.clone()?;
                let revision = self.revision;
                let params = patch_params(&playbook, &patch);
                Some(self.send(
                    Sent::Edit {
                        base: revision,
                        retry: Box::new(Job::SetOption { patch, on }),
                    },
                    "patch_plan",
                    params,
                ))
            }
            Job::Beacon { id } => {
                let params = object(vec![("beacon_id", Json::String(id.clone()))]);
                Some(self.send(Sent::Beacon { id }, "get_beacon", params))
            }
            Job::Vocabulary => {
                let round = self.round;
                Some(self.send(
                    Sent::Vocabulary { round },
                    "get_vocabulary",
                    object(Vec::new()),
                ))
            }
            Job::Visit { beacon, row, field } => {
                let playbook = self.text.clone()?;
                let revision = self.revision;
                let label = fresh_label(&playbook, Action::Visit(Priority::Normal).stem());
                let step = vocabulary::visit_step(&beacon, row.clone(), &label);
                let params = patch_params(&playbook, &append_patch(step));
                Some(self.send(
                    Sent::Edit {
                        base: revision,
                        retry: Box::new(Job::Visit { beacon, row, field }),
                    },
                    "patch_plan",
                    params,
                ))
            }
            other => self.render(other),
        }
    }

    /// An edit's `patch_plan` call with `params`, against the text as it is now, with `retry`
    /// kept for a connection that drops before its answer.
    fn send_patch(&mut self, params: Json, retry: Job) -> Call {
        let base = self.revision;
        self.send(
            Sent::Edit {
                base,
                retry: Box::new(retry),
            },
            "patch_plan",
            params,
        )
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

    /// `resolve_refs` for the text on screen, when it holds a feature reference; a text
    /// with none has no chips, and nothing is asked.
    fn resolve_call(&mut self) -> Option<Call> {
        let text = self.text.clone()?;
        let revision = self.revision;
        if !targeting::has_feature_refs(&text) {
            self.chips.clear();
            self.chips_revision = Some(revision);
            self.touch();
            return None;
        }
        Some(self.send(
            Sent::Resolve { revision },
            "resolve_refs",
            object(vec![("playbook_jsonc", Json::String(text))]),
        ))
    }

    fn settle(&mut self, sent: Sent, result: &Json) -> Result<(), BridgeError> {
        match sent {
            Sent::Load { candidate, file } => self.settle_load(candidate, file, result)?,
            Sent::Edit { base, retry } => self.settle_edit(base, &retry, result)?,
            Sent::Undo { base } => {
                let answer = patch_of(result)?;
                if base == self.revision {
                    self.undo.pop();
                    self.drop_settled_ghost();
                    self.accept_text(answer.playbook_jsonc);
                    self.say("undone", "");
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
            Sent::PreviewCheck => self.settle_preview_check(&report_of(result)?),
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
                    self.take_route(result)?;
                }
            }
            Sent::Submit { text, revision } => {
                let response: SubmitPlanResponse =
                    json::decode_json(&body(result, "gp.api.v1.SubmitPlanResponse"))?;
                self.settle_submit(text, revision, response)?;
            }
            Sent::Briefing => {
                let response: GetBriefingResponse =
                    json::decode_json(&body(result, "gp.api.v1.GetBriefingResponse"))?;
                self.notes = response.notes;
                self.known.notes = true;
                self.this_round = targeting::this_round_of(&response.prose);
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
            sent @ (Sent::Render { .. }
            | Sent::Beacon { .. }
            | Sent::Vocabulary { .. }
            | Sent::Templates
            | Sent::Instantiate { .. }
            | Sent::GetDraft { .. }
            | Sent::Resolve { .. }
            | Sent::MapSummary) => self.settle_pr2(sent, result)?,
        }
        Ok(())
    }

    /// QUICK's report on the draft with a beacon patched in: the ghost's verdict, and what is
    /// taken at once if the placement is.
    fn settle_preview_check(&mut self, report: &VerifyReport) {
        if let Some(preview) = self.preview.as_mut() {
            let (state, sentence) = placement_verdict(report, preview.step);
            self.ghost = Some(Ghost {
                at: preview.at,
                state,
                sentence,
            });
            let rows = rows_of(report);
            preview.checked = Some(CheckedPreview {
                size: size_meter_of(report, &rows),
                rows,
                qualifies: report.qualifies,
                dormant: dormant_patch_of(report),
            });
        }
    }

    /// Pull request 2's answers: the rule list, the template list, the wizard's
    /// instantiation and the carried draft.
    fn settle_pr2(&mut self, sent: Sent, result: &Json) -> Result<(), BridgeError> {
        match sent {
            Sent::Render { revision } => {
                let list = rule_list::rule_list_of(result)?;
                if revision == self.revision {
                    self.rule_list = list;
                    self.prose_revision = Some(revision);
                    self.rendering = self.rendering.wrapping_add(1);
                    self.rules_rendering = Some(self.rendering);
                }
            }
            Sent::Templates => {
                self.templates = wizard::templates_of(result)?;
                self.known.templates = true;
            }
            Sent::Instantiate { asked } => {
                let instance: Instance = wizard::instance_of(result)?;
                if let Some(wizard) = self.wizard.as_mut()
                    && wizard.asked == asked
                {
                    wizard.instance = Some(instance);
                    wizard.answered = asked;
                    wizard.refusal = None;
                }
            }
            Sent::GetDraft { id: _, round } => {
                let response: GetDraftResponse =
                    json::decode_json(&body(result, "gp.api.v1.GetDraftResponse"))?;
                if round == self.round {
                    self.undo.clear();
                    self.preview = None;
                    self.ghost = None;
                    // Last round's seal, from the gateway: no file of the player's.
                    self.file = None;
                    self.accept_text(response.playbook_jsonc);
                    self.say("carried", &response.label);
                }
            }
            Sent::Resolve { revision } => {
                if revision == self.revision
                    && let Some(text) = self.text.as_deref()
                {
                    self.chips = targeting::chips_of(result, text)?;
                    self.chips_revision = Some(revision);
                }
            }
            Sent::MapSummary => self.features = targeting::features_of(result)?,
            Sent::Beacon { id } => {
                // The detail beside the prose: another seat's beacon carries none, and so its
                // detail lists nothing (decisions-log item 160 (4)).
                // The prose first, alone, so an answer whose detail cannot be read still heads
                // the menu with the gateway's sentence.
                let (answer, _) = split_footer(result);
                proto3_text(&answer, "prose")?.clone_into(&mut self.beacon_prose);
                match vocabulary::beacon_detail_of(&id, result) {
                    Ok((prose, detail)) => {
                        self.beacon_prose = prose;
                        self.beacon = Some(detail);
                        self.beacon_failed = None;
                    }
                    Err(error) => {
                        self.beacon = None;
                        self.beacon_failed = Some(ReadFailed::Unreadable(error.to_string()));
                        return Err(error);
                    }
                }
            }
            Sent::Vocabulary { round } => match vocabulary::vocabulary_of(result) {
                Ok(words) => {
                    if round == self.round {
                        self.vocabulary = Some(words);
                        self.vocabulary_failed = None;
                    }
                }
                Err(error) => {
                    if round == self.round {
                        self.vocabulary_failed = Some(ReadFailed::Unreadable(error.to_string()));
                    }
                    return Err(error);
                }
            },
            other => return self.settle(other, result),
        }
        Ok(())
    }

    /// An edit's patched text: taken when the text it was patched against is still the one on
    /// screen. A Fix's lands with "fixed" on the status line, after the "fixing" its click
    /// put there.
    fn settle_edit(&mut self, base: u64, job: &Job, result: &Json) -> Result<(), BridgeError> {
        let answer = patch_of(result)?;
        // A value field's edit landed, or another landed first and it was not taken: its popup
        // hears which.
        if field_sent(job) {
            self.field_ask = if base == self.revision {
                FieldAsk::Landed
            } else {
                FieldAsk::Refused {
                    key: "status_rule_refused_changed".to_owned(),
                    detail: String::new(),
                }
            };
        }
        if base == self.revision {
            self.undo.push(Undo::Patch(answer.inverse_json_patch));
            self.drop_settled_ghost();
            self.accept_text(answer.playbook_jsonc);
            match job {
                Job::Edit { .. } => self.say("fixed", ""),
                Job::SetOption { on: true, .. } => self.say("dormant_allowed", ""),
                Job::SetOption { on: false, .. } => self.say("dormant_disallowed", ""),
                // A map action has no line of its own, and the last one (a submission, a
                // Fix, a template's playbook) no longer describes the text on screen.
                _ => self.say("", ""),
            }
        }
        Ok(())
    }

    /// Load's QUICK answer: refuse the file with the code and the pointer, or open it.
    fn settle_load(
        &mut self,
        candidate: String,
        file: Option<String>,
        result: &Json,
    ) -> Result<(), BridgeError> {
        let report = &report_of(result)?;
        let refusal = report.diagnostics.iter().find(|diagnostic| {
            diagnostic.severity() == Severity::Error
                && LOAD_REFUSALS.contains(&diagnostic.code.as_str())
        });
        if let Some(refusal) = refusal {
            let detail = format!("{} {}", refusal.code, refusal.path);
            self.rows = rows_of(report);
            self.verdict = Verdict::Refused;
            self.say("load_refused", &detail);
        } else {
            self.undo.clear();
            self.preview = None;
            self.ghost = None;
            if file.is_some() {
                self.last_file.clone_from(&file);
            }
            self.file = file;
            // Another file is on screen: a template's pages describe a different playbook
            // (item 155 (3) 3).
            self.wizard_close();
            self.accept_text(candidate);
            self.take_rows(report, Verdict::Quick);
            self.owed.quick = false;
            self.say("loaded", "");
        }
        Ok(())
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

    fn settle_submit(
        &mut self,
        text: String,
        revision: u64,
        response: SubmitPlanResponse,
    ) -> Result<(), BridgeError> {
        let report = response.report.ok_or_else(|| no_report("submit_plan"))?;
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
            self.sealed_round = Some(self.round);
            // A re-seal changes the Lull's "this round" sentence at once (targeting.md,
            // "Messages"), so the briefing is read again.
            self.queue.retain(|job| !matches!(job, Job::Briefing));
            self.queue.push_back(Job::Briefing);
            self.say("submitted", "");
        } else {
            self.say("submit_refused", "");
        }
        Ok(())
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
    /// PLACEHOLDER: a resumed Lull opens `carried` — OWNER, S6, with the draft browser.
    /// A resumed Lull opens `carried`, as spec section 13's continuity says,
    /// and the seat's own `editor` draft of this round is listed, not opened. Opening it when
    /// present is a preference the spec does not state; a click that opens any listed draft
    /// is a small draft browser ahead of S6's.
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

    /// Undo of the wizard's playbook: the text before it, as it was (`None`: no text), and
    /// the file it came from.
    fn restore(&mut self, before: Option<String>, file: Option<String>) {
        self.file = file;
        if let Some(text) = before {
            self.accept_text(text);
            return;
        }
        self.text = None;
        self.size = None;
        self.attack.clear();
        self.build_marks.clear();
        self.step_labels.clear();
        self.rule_ids.clear();
        self.revision = self.revision.wrapping_add(1);
        self.rows.clear();
        self.verdict = Verdict::None;
        self.qualifies = false;
        self.route = Route::default();
        self.rule_list = RuleList::default();
        self.prose_revision = None;
        self.chips.clear();
        self.chips_revision = None;
        self.dormant_patch = None;
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
        self.attack = targeting::attack_targets(&text);
        self.build_marks = targeting::build_targets(&text);
        self.step_labels = vocabulary::step_labels(&text);
        self.rule_ids = vocabulary::rule_ids(&text);
        self.rule_refusal = None;
        self.picking = None;
        self.text = Some(text);
        self.revision = self.revision.wrapping_add(1);
        self.owed.quick = true;
        self.owed.estimate = true;
        self.resolve_owed = true;
        self.render_owed = true;
        self.owed.edited = true;
        self.route.current = false;
        self.touch();
    }

    fn take_rows(&mut self, report: &VerifyReport, verdict: Verdict) {
        self.rows = rows_of(report);
        self.size = size_meter_of(report, &self.rows);
        self.dormant_patch = dormant_patch_of(report);
        self.verdict = verdict;
        self.rows_revision = self.revision;
        self.qualifies = report.qualifies;
        self.touch();
    }

    /// Takes in an `estimate_route` answer.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Rpc`] for a time or an axis that is present and not an integer (an
    /// absent one is proto3's zero), or an axis outside an `i32`.
    fn take_route(&mut self, result: &Json) -> Result<(), BridgeError> {
        let (answer, _) = split_footer(result);
        let reachable = matches!(answer.get("reachable"), Some(Json::Bool(true)));
        let mut route = Route {
            reachable,
            whole: proto3_integer(&answer, "ms")?,
            whole_text: proto3_text(&answer, "travel_text")?.to_owned(),
            current: true,
            readable: true,
            ..Route::default()
        };
        if let Some(commander) = self.commander {
            route.points.push(commander);
        }
        if let Some(Json::Array(legs)) = answer.get("legs") {
            for leg in legs {
                route.legs.push(proto3_integer(leg, "ms")?);
                route
                    .leg_texts
                    .push(proto3_text(leg, "travel_text")?.to_owned());
                // `gp.api.v1.Leg.to` is a `gp.v1.Location`, `{"voxel":{..}}`; the gateway
                // at `main` writes the bare voxel `{"x":..}` there instead. Both spellings
                // name the same voxel, so both are read; the disagreement is the gateway's
                // to settle and is reported in this lane's pull request.
                let to = leg.get("to");
                let voxel = to.and_then(|to| to.get("voxel")).or(to);
                match voxel {
                    Some(voxel @ Json::Object(_)) => route.points.push(voxel_of(voxel)?),
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
        Ok(())
    }

    fn refused(&mut self, sent: &Sent, code: &str, message: &str) -> Settled {
        if let (Sent::Estimate { revision }, ESTIMATE_FOUND_NOTHING) = (sent, code) {
            if *revision == self.revision {
                self.route = Route {
                    current: true,
                    found_nothing: message.to_owned(),
                    ..Route::default()
                };
                self.touch();
            }
            return Settled::Answered;
        }
        self.refusals = self.refusals.saturating_add(1);
        let detail = format!("{code}: {message}");
        // A refused check of the text on screen will not come back by itself: the panel
        // says so, with the gateway's words, rather than "Checking..." for ever (finding K).
        // Each check's refusal is its own line's: a refused estimate leaves the chips'
        // "Reading the map..." alone while their own answer is on its way.
        if let Some((check, revision)) = check_of(sent)
            && revision == self.revision
        {
            *self.check_refusals.slot(check) = Some((revision, Unchecked::Refused(detail.clone())));
        }
        // A refused read of the gateway's words is said on its own line, and nothing waits on it.
        match sent {
            Sent::Vocabulary { round } if *round == self.round => {
                self.vocabulary_failed = Some(ReadFailed::Refused(detail.clone()));
            }
            Sent::Beacon { .. } => {
                self.beacon = None;
                self.beacon_failed = Some(ReadFailed::Refused(detail.clone()));
            }
            _ => {}
        }
        if let Sent::Edit { retry, .. } = sent
            && field_sent(retry)
        {
            self.field_ask = FieldAsk::Refused {
                key: GATEWAY_REFUSED_KEY.to_owned(),
                detail: detail.clone(),
            };
        }
        if let Sent::Edit { base, retry } = sent
            && let Job::RuleEdit { pointer, .. } = retry.as_ref()
        {
            self.rule_refusal = Some(RuleRefusal {
                pointer: pointer.clone(),
                key: GATEWAY_REFUSED_KEY.to_owned(),
                detail: detail.clone(),
                revision: *base,
            });
        }
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
                if let Some(wizard) = self.wizard.as_mut()
                    && wizard.asked == *asked
                {
                    wizard.refusal = Some(detail.clone());
                }
                self.say("wizard_refused", &detail);
            }
            _ => self.say("gateway_refused", &detail),
        }
        Settled::Refused
    }
}

/// Whether a value field sent this job ([`FieldAsk`]).
const fn field_sent(job: &Job) -> bool {
    matches!(
        job,
        Job::RuleEdit { field: true, .. } | Job::Visit { field: true, .. }
    )
}

/// The check of the text a call is, and the revision it checks; `None` for a call that checks
/// nothing.
const fn check_of(sent: &Sent) -> Option<(Check, u64)> {
    match sent {
        Sent::Quick { revision } | Sent::Full { revision } => Some((Check::Rows, *revision)),
        Sent::Estimate { revision } => Some((Check::Route, *revision)),
        Sent::Render { revision } => Some((Check::Prose, *revision)),
        Sent::Resolve { revision } => Some((Check::Chips, *revision)),
        _ => None,
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
/// name that target: a visit or a recycle names a beacon, a placement names ground, and a
/// placement covering a feature names a feature or Alt-click's description of one.
#[must_use]
pub fn step_value(action: Action, target: &Target, label: &str) -> Option<Json> {
    // A feature is a site only under `covering` (`gp.v1.Location.covering`, legal only in
    // a place_beacon's `at`), so it is the one action a feature target can take.
    let covering = match target {
        Target::Feature(id) => Some(object(vec![("feature_id", Json::String(id.clone()))])),
        Target::NearestUncovered(kind) => Some(object(vec![(
            kind.name(),
            object(vec![
                ("rank", text("NEAREST")),
                ("coverage", text("UNCOVERED")),
            ]),
        )])),
        Target::Beacon(_) | Target::Selector(_) | Target::Voxel(_) => None,
    };
    match (action, covering) {
        (Action::PlaceCovering, Some(reference)) => {
            return Some(object(vec![
                ("label", Json::String(label.to_owned())),
                (
                    "place_beacon",
                    object(vec![("at", object(vec![("covering", reference)]))]),
                ),
            ]));
        }
        (Action::PlaceCovering, None) | (_, Some(_)) => return None,
        (_, None) => {}
    }
    let beacon_ref = match target {
        Target::Beacon(id) => Some(object(vec![("beacon_id", Json::String(id.clone()))])),
        Target::Selector(selector) => Some(selector.beacon_ref()),
        Target::Voxel(_) | Target::Feature(_) | Target::NearestUncovered(_) => None,
    };
    let location = match target {
        Target::Voxel(at) => voxel_location(*at),
        Target::Beacon(_) | Target::Selector(_) => {
            object(vec![("beacon_anchor", beacon_ref.clone()?)])
        }
        Target::Feature(_) | Target::NearestUncovered(_) => return None,
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
        Action::PlaceCovering => return None,
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

/// The player's JSONC text as JSON, for the lax readers of the client's own file: the
/// route's targets ([`route_waypoints`]), the E0601 option ([`dormant_key`]) and
/// targeting's walks (`has_feature_refs`, `attack_targets`, `chips_of`'s form read).
///
/// They decide nothing, and the verdict on the file is the verifier's (AGENTS.md section 3
/// rule 4), so a text that is not JSON once its comments are gone is no document here, and
/// each reader answers what it documents for one: no route, no option set, no reference.
/// The verifier, asked about the same text, names what is wrong with it.
///
/// Its depth is bounded (decisions-log item 167 (9), carried to S3's `fixp`): `json::read` is the
/// one grammar every reader shares, which refuses a value inside more than
/// [`json::grammar::MAX_DEPTH`] containers with its typed error before it nests further, so a
/// hostile file cannot exhaust this reader's stack, nor the recursive walks over what it returns
/// (`targeting`'s and `vocabulary`'s), which never go deeper than the tree it built.
pub(crate) fn lax_document(text: &str) -> Option<Json> {
    json::read(&strip_comments(text)).ok()
}

/// The route's steps, read laxly out of a JSONC text.
fn route_steps(text: &str) -> Option<Vec<Json>> {
    let document = lax_document(text)?;
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

/// The verifier's own patch for E0601, the dormant-beacon option, when `report` carries
/// one: its first suggestion, whatever its applicability. The verifier marks it
/// `MAYBE_INCORRECT` because leaving beacons dark is a choice, not a fix, which is why it
/// is a checkbox the player ticks rather than a Fix button.
fn dormant_patch_of(report: &VerifyReport) -> Option<String> {
    report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0601")
        .and_then(|diagnostic| diagnostic.suggestions.first())
        .map(|suggestion| suggestion.json_patch.clone())
}

/// The key under which the playbook `text` sets `options.allow_dormant_beacons` to true,
/// in the spelling the text uses ([`DORMANT_KEYS`]), read laxly as the route is: a text
/// this walk cannot read sets nothing, and the verifier has the last word.
fn dormant_key(text: &str) -> Option<&'static str> {
    let document = lax_document(text)?;
    let options = document.get("declarative")?.get("options")?;
    DORMANT_KEYS
        .iter()
        .copied()
        .find(|key| matches!(options.get(key), Some(Json::Bool(true))))
}

/// A `verify_plan` answer's report.
fn report_of(result: &Json) -> Result<VerifyReport, BridgeError> {
    let response: VerifyPlanResponse =
        json::decode_json(&body(result, "gp.api.v1.VerifyPlanResponse"))?;
    response.report.ok_or_else(|| no_report("verify_plan"))
}

/// An answer that carries no report, which the gateway always writes: refused, never
/// read as an empty report, which would show the text as clean.
fn no_report(method: &str) -> BridgeError {
    BridgeError::Rpc(format!("a {method} answer with no report"))
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
pub(crate) fn voxel_location(at: [i32; 3]) -> Json {
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
///
/// # Errors
///
/// [`BridgeError::Rpc`] for an axis that is not an integer or does not fit an `i32`.
fn voxel_of(value: &Json) -> Result<[i32; 3], BridgeError> {
    let axis = |name: &str| {
        let found = proto3_integer(value, name)?;
        i32::try_from(found).map_err(|_| BridgeError::Rpc(format!("a voxel's `{name}` is {found}")))
    };
    Ok([axis("x")?, axis("y")?, axis("z")?])
}

/// The integer field `name` of an answer: proto3's canonical JSON leaves a zero out, so
/// an absent field is 0, and anything but an integer is refused.
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

    /// **The lax reader's depth is bounded** (decisions-log item 167 (9)): a text nested a million
    /// deep is refused as no document, without exhausting the stack, and so is one container past
    /// the shared grammar's bound; one at the bound is read, and the walks over it return.
    #[test]
    fn the_lax_reader_is_bounded_in_depth() {
        let deep = |depth: usize| {
            let mut text = "[".repeat(depth);
            text.push_str(&"]".repeat(depth));
            text
        };
        assert!(lax_document(&deep(1_000_000)).is_none());
        // A value may stand inside at most MAX_DEPTH (64) containers: 65 nested arrays put the
        // innermost inside 64, which reads; 66 put it inside 65, which is refused.
        assert_eq!(json::grammar::MAX_DEPTH, 64);
        assert!(lax_document(&deep(64)).is_some());
        assert!(lax_document(&deep(65)).is_some());
        assert!(lax_document(&deep(66)).is_none());
        let at_bound = deep(65);
        assert!(targeting::attack_targets(&at_bound).is_empty());
        assert!(targeting::build_targets(&deep(1_000_000)).is_empty());
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
            hp: None,
            heading: None,
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
        assert_eq!(editor.status().key, "fixing", "a Fix taken says so at once");
        let call = editor.next_call(false).expect("the fix");
        assert_eq!(call.method, "patch_plan");
        assert!(json::write(&call.params).contains("cooldown_ms"));
        assert!(!editor.fix(0, 0), "not while the edit is in flight");
        assert!(!editor.fix(0, 1), "no such fix");
        editor
            .answered(Ok(&patched("{\"fixed\": 1}", "[]")))
            .expect("reads");
        assert_eq!(editor.status().key, "fixed", "and again when it lands");
    }

    #[test]
    fn an_accepted_seal_is_this_rounds_until_the_next_round() {
        let submit = |accepted: bool| {
            let mut editor = loaded(EXPAND_EAST);
            editor.lull_opened(2);
            assert!(!editor.sealed_this_round());
            assert!(editor.submit());
            for _ in 0..12 {
                match editor.next_call(false) {
                    Some(call) if call.method == "submit_plan" => break,
                    Some(_) => {
                        editor
                            .answered(Err(("INTERNAL", "not this test's call")))
                            .expect("reads");
                    }
                    None => panic!("no submit_plan"),
                }
            }
            editor
                .answered(Ok(&read(&format!(
                    r#"{{"report":{{"qualifies":{accepted},"depth":"full"}},"accepted":{accepted}}}"#
                ))))
                .expect("reads");
            editor
        };
        let mut editor = submit(true);
        assert!(
            editor.sealed_this_round(),
            "an accepted seal is this round's"
        );
        editor.lull_opened(2);
        assert!(
            editor.sealed_this_round(),
            "the same Lull opened again keeps it"
        );
        editor.lull_opened(3);
        assert!(!editor.sealed_this_round(), "a new round holds no seal yet");
        assert!(!submit(false).sealed_this_round(), "a refused seal is none");
    }

    #[test]
    fn a_submission_says_it_was_taken_before_its_answer() {
        let mut editor = loaded(EXPAND_EAST);
        let changes = editor.changes();
        assert!(editor.submit());
        assert_eq!(editor.status().key, "submitting");
        assert!(
            editor.changes() > changes,
            "the status line is redrawn at once"
        );
        // The checks the loaded text still owes go first; the submission follows them.
        let mut method = method_of(editor.next_call(false));
        for _ in 0..8 {
            if method == "submit_plan" {
                break;
            }
            editor
                .answered(Err(("INTERNAL", "not this test's call")))
                .expect("reads");
            method = method_of(editor.next_call(false));
        }
        assert_eq!(method, "submit_plan");
        editor
            .answered(Ok(&read(
                r#"{"report":{"qualifies":true,"depth":"full"},"accepted":true}"#,
            )))
            .expect("reads");
        assert_eq!(editor.status().key, "submitted");
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
                "get_vocabulary" => read(r#"{"families":[]}"#),
                "get_map_summary" => read(r#"{"features":[]}"#),
                "resolve_refs" => read(r#"{"refs":[]}"#),
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

    /// A click on a vent names it; Alt-click turns it into "the nearest vent you can cover"
    /// (`docs/design/targeting.md`, "Surfaces"). Either is a place-beacon step whose site is
    /// `covering`, and a feature is a target for nothing else.
    #[test]
    fn a_vent_click_places_a_beacon_covering_it_and_alt_click_describes_it() {
        // Both are offered on every vent click; Alt only puts the description first.
        let named = step_value(
            Action::PlaceCovering,
            &Target::Feature("vent_120_88".to_owned()),
            "place_1",
        )
        .expect("a name");
        assert_eq!(
            compact(&named),
            r#"{"label":"place_1","place_beacon":{"at":{"covering":{"feature_id":"vent_120_88"}}}}"#
        );
        let described = step_value(
            Action::PlaceCovering,
            &Target::NearestUncovered(FeatureKind::Vent),
            "place_2",
        )
        .expect("a description");
        assert_eq!(
            compact(&described),
            r#"{"label":"place_2","place_beacon":{"at":{"covering":{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}}}}"#
        );
        let vent = Target::Feature("vent_120_88".to_owned());
        for action in [
            Action::Go,
            Action::Visit(Priority::Low),
            Action::Recycle,
            Action::Place,
        ] {
            assert!(step_value(action, &vent, "x").is_none(), "{action:?}");
        }
        assert!(
            step_value(Action::PlaceCovering, &Target::Voxel([1, 2, 3]), "x").is_none(),
            "covering names a feature, not a voxel"
        );
        // The route reader sends a covering site to the estimator as it is written.
        let text = format!(
            r#"{{"declarative":{{"route":[{}]}}}}"#,
            json::write(&described)
        );
        let waypoints = route_waypoints(&text).expect("reads");
        assert!(
            waypoints
                .first()
                .map(compact)
                .unwrap_or_default()
                .contains("\"covering\""),
            "{waypoints:?}"
        );
    }

    /// **S3-18**: the route carries the gateway's rendered travel figures, whole and per leg,
    /// as they came, beside the raw milliseconds; a leg the gateway sent no text for keeps an
    /// empty one, said as such and never filled in by the client.
    #[test]
    fn the_route_carries_the_gateways_rendered_travel_text() {
        let mut editor = loaded(EXPAND_EAST);
        editor.set_entities(&[Entity {
            id: "u_1".to_owned(),
            kind: EntityKind::Unit,
            subtype: "commander".to_owned(),
            owner: "seat.0".to_owned(),
            at: [358, 22, 36],
            hp: None,
            heading: None,
        }]);
        assert_eq!(method_of(editor.next_call(false)), "estimate_route");
        editor
            .answered(Ok(&read(
                r#"{"reachable":true,"ms":84000,"travel_text":"1 min 24 s","legs":[{"to":{"voxel":{"x":1,"y":2,"z":3}},"ms":80000,"travel_text":"1 min 20 s"},{"to":{"voxel":{"x":1,"y":2,"z":4}},"ms":4000}]}"#,
            )))
            .expect("reads");
        let route = editor.route();
        assert_eq!(route.whole_text, "1 min 24 s");
        assert_eq!(
            route.leg_texts,
            vec!["1 min 20 s".to_owned(), String::new()]
        );
        assert_eq!(
            route.legs,
            vec![80_000, 4_000],
            "the raw figures stay as they came"
        );
    }

    /// The gateway's committed `render_plan` answer for the Expand & Mine demo: the rule list
    /// the chip tests edit.
    const EXPAND_AND_MINE_RENDER: &str = include_str!(
        "../../../tests/golden/gateway/demo_expand_and_mine_render/expected.response.json"
    );

    /// An editor holding `text`, its rows QUICK-clean and its rule list the golden's.
    fn rendered(text: &str) -> Editor {
        let mut editor = loaded(text);
        for _ in 0..16 {
            let Some(call) = editor.next_call(false) else {
                break;
            };
            let answer = match call.method {
                "render_plan" => read(EXPAND_AND_MINE_RENDER)
                    .get("result")
                    .cloned()
                    .expect("a result"),
                "estimate_route" => read(r#"{"reachable":true}"#),
                "resolve_refs" => read(r#"{"refs":[]}"#),
                _ => quick_report("", true),
            };
            editor.answered(Ok(&answer)).expect("reads");
        }
        assert!(editor.prose_current(), "the rule list is the text's");
        editor
    }

    fn chip_index(editor: &Editor, suffix: &str) -> usize {
        editor
            .rule_list()
            .chips
            .iter()
            .position(|chip| chip.pointer.ends_with(suffix))
            .unwrap_or_else(|| panic!("no chip {suffix}"))
    }

    fn patch_of_call(call: &Call) -> String {
        match call.params.get("json_patch") {
            Some(Json::String(patch)) => compact(&read(patch)),
            other => panic!("no patch: {other:?}"),
        }
    }

    /// The chip at `suffix` as an editor opened on it names it: its index, the rendering's
    /// revision and its pointer.
    fn drawn(editor: &Editor, suffix: &str) -> (usize, u64, String) {
        let index = chip_index(editor, suffix);
        let pointer = editor
            .rule_list()
            .chips
            .get(index)
            .map(|chip| chip.pointer.clone())
            .expect("a chip");
        (
            index,
            editor.rules_rendering().expect("a rendering"),
            pointer,
        )
    }

    /// Rule `index` as a drag begun now names it.
    fn rule_named(editor: &Editor, index: usize) -> RuleNamed {
        RuleNamed {
            index,
            rendering: editor.rules_rendering().expect("a rendering"),
            pointer: editor
                .rule_list()
                .rules
                .get(index)
                .map(|rule| rule.pointer.clone())
                .unwrap_or_default(),
        }
    }

    /// The golden render answer with its rules and every rule's chips rotated by one: the
    /// same text rendered again, where every old index names another chip and another rule.
    fn reordered_render() -> Json {
        let mut result = read(EXPAND_AND_MINE_RENDER)
            .get("result")
            .cloned()
            .expect("a result");
        // Rotated, not reversed: a reversal keeps a middle item at its index.
        let reverse = |value: &mut Json| {
            if let Json::Array(items) = value
                && !items.is_empty()
            {
                items.rotate_left(1);
            }
        };
        if let Json::Object(fields) = &mut result {
            for (key, rules) in fields.iter_mut() {
                if key != "rules" {
                    continue;
                }
                reverse(rules);
                if let Json::Array(rules) = rules {
                    for rule in rules.iter_mut() {
                        if let Json::Object(rule) = rule {
                            for (key, chips) in rule.iter_mut() {
                                if key == "chips" {
                                    reverse(chips);
                                }
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// Answers the editor's calls until it is quiet, with `render` for the rule list.
    fn rerender(editor: &mut Editor, render: &Json) {
        for _ in 0..24 {
            let Some(call) = editor.next_call(false) else {
                return;
            };
            let answer = match call.method {
                "render_plan" => render.clone(),
                "estimate_route" => read(r#"{"reachable":true}"#),
                "resolve_refs" => read(r#"{"refs":[]}"#),
                "get_briefing" => read(r#"{"notes":""}"#),
                "get_vocabulary" => read(r#"{"families":[]}"#),
                "get_map_summary" => read(r#"{"features":[]}"#),
                "list_drafts" => read(r#"{"drafts":[]}"#),
                "list_templates" => read(r#"{"templates":[]}"#),
                _ => quick_report("", true),
            };
            editor.answered(Ok(&answer)).expect("reads");
        }
        panic!("the editor never went quiet");
    }

    /// **A chip's edit is one `replace`** (S3's `ed3b`): a number within the bounds sent, an
    /// enum's value as sent, each at the chip's pointer; the patched text is `patch_plan`'s.
    #[test]
    fn a_chip_edit_is_one_replace_at_its_pointer() {
        let mut editor = rendered(EXPAND_EAST);
        let (pct, revision, pointer) = drawn(&editor, "/cmdr_hp_pct/pct");
        assert!(editor.rules_editable());
        assert_eq!(editor.set_number(pct, revision, &pointer, 35), Ok(()));
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(call.method, "patch_plan");
        assert_eq!(
            patch_of_call(&call),
            r#"[{"op":"replace","path":"/declarative/handlers/0/when/all/items/0/cmdr_hp_pct/pct","value":35}]"#
        );
        assert_eq!(
            editor.set_number(pct, revision, &pointer, 36),
            Err(RuleEditRefusal::Busy),
            "not while the edit is in flight"
        );
        editor
            .answered(Ok(&patched("{\"edited\": 1}", "[]")))
            .expect("reads");
        assert_eq!(editor.bytes(), b"{\"edited\": 1}");
        assert_eq!(editor.undo_depth(), 1);
        assert_eq!(
            editor.rules_blocked(),
            Some(RuleEditRefusal::Stale),
            "the rule list describes the text before the edit"
        );

        let mut editor = rendered(EXPAND_EAST);
        let (priority, revision, at) = drawn(&editor, "/initial/priority");
        assert_eq!(editor.choose(priority, revision, &at, 2), Ok(()));
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(
            patch_of_call(&call),
            r#"[{"op":"replace","path":"/declarative/route/0/place_beacon/initial/priority","value":"HIGH"}]"#
        );
    }

    /// **Setting a number to 0** writes the 0, as any other value (M1's second test).
    #[test]
    fn setting_a_number_to_zero_writes_the_zero() {
        let mut editor = rendered(EXPAND_EAST);
        let (pct, revision, pointer) = drawn(&editor, "/cmdr_hp_pct/pct");
        assert_eq!(editor.set_number(pct, revision, &pointer, 0), Ok(()));
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(
            patch_of_call(&call),
            r#"[{"op":"replace","path":"/declarative/handlers/0/when/all/items/0/cmdr_hp_pct/pct","value":0}]"#
        );
    }

    /// **An edit never lands on another chip** (M1). A drag is in flight; a click on a chip of
    /// the list drawn before it is refused while it is on its way, refused again once its text
    /// lands (the list is stale), and refused once the list is redrawn for the new text, because
    /// the click names the old rendering; a pointer that is not the chip's at that index is
    /// refused too. Every refusal is said, by its reason.
    #[test]
    fn an_edit_never_lands_on_a_chip_of_another_rendering() {
        let mut editor = rendered(EXPAND_EAST);
        let (pct, revision, pointer) = drawn(&editor, "/cmdr_hp_pct/pct");
        let (from, onto) = (rule_named(&editor, 1), rule_named(&editor, 0));
        assert_eq!(editor.move_rule(&from, &onto), Ok(()));
        assert_eq!(
            editor.set_number(pct, revision, &pointer, 35),
            Err(RuleEditRefusal::Busy)
        );
        assert_eq!(editor.status().key, "rule_refused_busy");
        let _ = editor.next_call(false).expect("the move");
        editor
            .answered(Ok(&patched("{\"moved\": 1}", "[]")))
            .expect("reads");
        assert_eq!(
            editor.set_number(pct, revision, &pointer, 35),
            Err(RuleEditRefusal::Stale)
        );
        assert_eq!(editor.status().key, "rule_refused_stale");
        // The new text is checked and rendered, its chips in another order: the old index
        // names another chip now.
        rerender(&mut editor, &reordered_render());
        assert!(editor.rules_editable());
        assert_ne!(editor.rules_rendering(), Some(revision));
        assert_ne!(
            editor
                .rule_list()
                .chips
                .get(pct)
                .map(|chip| chip.pointer.as_str()),
            Some(pointer.as_str()),
            "the old index lands on another chip now"
        );
        assert_eq!(
            editor.set_number(pct, revision, &pointer, 35),
            Err(RuleEditRefusal::Changed),
            "the click names the rendering before the drag"
        );
        assert_eq!(editor.status().key, "rule_refused_changed");
        let (pct, revision, _) = drawn(&editor, "/cmdr_hp_pct/pct");
        assert_eq!(
            editor.set_number(pct, revision, "/declarative/route/0/timeout_ms", 35),
            Err(RuleEditRefusal::Changed),
            "another chip's pointer at this index"
        );
        assert!(editor.next_call(false).is_none(), "nothing was sent");
    }

    /// **A re-render of the same text invalidates every open control** (fix pass 2, item 2). A
    /// new round's Lull renders the same text again, here with its rules and chips in another
    /// order, so every old index names another chip or rule. A pick begun on the old list and
    /// a drag begun on it are both refused as `Changed`, and nothing is sent.
    #[test]
    fn a_re_render_of_the_same_text_refuses_an_old_pick_and_an_old_drag() {
        let mut editor = rendered(EXPAND_EAST);
        let (to, rendering, to_at) = drawn(&editor, "/route/1/move/to");
        let (from, onto) = (rule_named(&editor, 1), rule_named(&editor, 0));
        assert_eq!(editor.begin_pick(to, rendering, &to_at), Ok(()));
        let revision = editor.revision();
        editor.lull_opened(2);
        rerender(&mut editor, &reordered_render());
        assert_eq!(editor.revision(), revision, "the same text");
        assert_ne!(editor.rules_rendering(), Some(rendering), "a new rendering");
        assert_ne!(
            editor
                .rule_list()
                .rules
                .get(from.index)
                .map(|rule| rule.pointer.as_str()),
            Some(from.pointer.as_str()),
            "the old index names another rule now"
        );
        assert_eq!(
            editor.pick(&Target::Beacon("b_01".to_owned())),
            Err(RuleEditRefusal::Changed)
        );
        assert_eq!(editor.picking(), None, "a pick on an old list ends");
        assert_eq!(
            editor.move_rule(&from, &onto),
            Err(RuleEditRefusal::Changed)
        );
        assert_eq!(editor.status().key, "rule_refused_changed");
        assert!(editor.next_call(false).is_none(), "nothing was sent");
    }

    /// **A pick on nothing keeps the pick on** (fix pass 2, item 4): the player missed, says
    /// so, and clicks again.
    #[test]
    fn a_pick_on_nothing_keeps_the_pick_on() {
        let mut editor = rendered(EXPAND_EAST);
        assert_eq!(editor.pick_missed(), Err(RuleEditRefusal::NotPicking));
        let (to, rendering, to_at) = drawn(&editor, "/route/1/move/to");
        assert_eq!(editor.begin_pick(to, rendering, &to_at), Ok(()));
        assert_eq!(editor.pick_missed(), Err(RuleEditRefusal::NoTarget));
        assert_eq!(editor.status().key, "rule_refused_no_target");
        assert_eq!(editor.picking(), Some(to), "the pick stays on");
        assert_eq!(editor.pick(&Target::Beacon("b_01".to_owned())), Ok(()));
    }

    /// **No rule-list edit is refused in silence** (M2): each wrong ask comes back as its
    /// reason and says it on the status line.
    #[test]
    fn every_refused_rule_edit_says_its_reason() {
        let mut editor = rendered(EXPAND_EAST);
        let (priority, revision, at) = drawn(&editor, "/initial/priority");
        let (pct, _, pct_at) = drawn(&editor, "/cmdr_hp_pct/pct");
        let (timeout, _, timeout_at) = drawn(&editor, "/route/0/timeout_ms");
        let cases: Vec<(Result<(), RuleEditRefusal>, RuleEditRefusal)> = vec![
            (
                editor.choose(priority, revision, &at, 3),
                RuleEditRefusal::NoSuchValue,
            ),
            (
                editor.choose(priority, revision, &at, 1),
                RuleEditRefusal::Unchanged,
            ),
            (
                editor.set_number(pct, revision, &pct_at, 40),
                RuleEditRefusal::Unchanged,
            ),
            (
                editor.set_number(priority, revision, &at, 3),
                RuleEditRefusal::NotThisEditor,
            ),
            (
                editor.set_number(timeout, revision, &timeout_at, 60_000),
                RuleEditRefusal::NotThisEditor,
            ),
            (
                editor.begin_pick(priority, revision, &at),
                RuleEditRefusal::NotThisEditor,
            ),
            (
                editor.set_number(999, revision, &at, 1),
                RuleEditRefusal::NoSuchChip,
            ),
            (
                editor.move_rule(&rule_named(&editor, 0), &rule_named(&editor, 0)),
                RuleEditRefusal::OntoItself,
            ),
            (
                editor.move_rule(&rule_named(&editor, 0), &rule_named(&editor, 99)),
                RuleEditRefusal::NoSuchRule,
            ),
            (
                editor.pick(&Target::Voxel([1, 2, 3])),
                RuleEditRefusal::NotPicking,
            ),
        ];
        for (got, want) in cases {
            assert_eq!(got, Err(want));
        }
        assert_eq!(
            editor.choose(priority, revision, &at, 3),
            Err(RuleEditRefusal::NoSuchValue)
        );
        assert_eq!(editor.status().key, "rule_refused_no_such_value");
        assert_eq!(
            editor.refuse_rule_edit(RuleEditRefusal::NoSuchChip),
            RuleEditRefusal::NoSuchChip
        );
        assert_eq!(editor.status().key, "rule_refused_no_such_chip");
        assert!(editor.next_call(false).is_none(), "nothing was sent");
    }

    /// **The door gates every rule-list control** (contract LOW-5): with the Push playing each
    /// is refused at once with the door's own line, a pick that was on included.
    #[test]
    fn the_door_gates_every_rule_list_control() {
        let mut editor = rendered(EXPAND_EAST);
        let (pct, revision, pct_at) = drawn(&editor, "/cmdr_hp_pct/pct");
        let (priority, _, at) = drawn(&editor, "/initial/priority");
        let (to, _, to_at) = drawn(&editor, "/route/1/move/to");
        assert_eq!(editor.begin_pick(to, revision, &to_at), Ok(()));
        editor.set_door(Door::Push);
        assert_eq!(editor.picking(), None, "a shut door ends the pick");
        assert_eq!(
            editor.pick(&Target::Beacon("b_01".to_owned())),
            Err(RuleEditRefusal::NotPicking)
        );
        for got in [
            editor.set_number(pct, revision, &pct_at, 35),
            editor.choose(priority, revision, &at, 0),
            editor.begin_pick(to, revision, &to_at),
            editor.move_rule(&rule_named(&editor, 1), &rule_named(&editor, 0)),
        ] {
            assert_eq!(got, Err(RuleEditRefusal::Shut));
            assert_eq!(editor.status().key, "closed_push");
        }
        assert_eq!(editor.rules_blocked(), Some(RuleEditRefusal::Shut));
        // A pick begun before the door shuts in the same Lull (no shut() between): the pick
        // itself goes through the door as well.
        let mut editor = rendered(EXPAND_EAST);
        let (to, revision, to_at) = drawn(&editor, "/route/1/move/to");
        assert_eq!(editor.begin_pick(to, revision, &to_at), Ok(()));
        editor.door = Door::Push;
        assert_eq!(
            editor.pick(&Target::Beacon("b_01".to_owned())),
            Err(RuleEditRefusal::Shut)
        );
        assert_eq!(editor.status().key, "closed_push");
    }

    /// **A map pick is one `replace`**: the chip's pick mode takes the next map click, writes
    /// the arm the click names when the gateway lists it, and refuses one it does not.
    #[test]
    fn a_map_pick_replaces_a_reference_chip_with_the_arm_the_click_names() {
        let mut editor = rendered(EXPAND_EAST);
        let (to, revision, to_at) = drawn(&editor, "/route/1/move/to");
        assert_eq!(editor.begin_pick(to, revision, &to_at), Ok(()));
        assert_eq!(editor.picking(), Some(to));
        assert_eq!(
            editor.pick(&Target::Feature("vent_1_2".to_owned())),
            Err(RuleEditRefusal::ArmNotHere("covering")),
            "this move takes no `covering`"
        );
        assert_eq!(editor.status().key, "rule_refused_arm_not_here");
        assert_eq!(editor.status().detail, "covering");
        assert_eq!(editor.picking(), Some(to), "a refused pick keeps the mode");
        assert_eq!(editor.pick(&Target::Beacon("b_01".to_owned())), Ok(()));
        assert_eq!(editor.picking(), None);
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(
            patch_of_call(&call),
            r#"[{"op":"replace","path":"/declarative/route/1/move/to","value":{"beacon_anchor":{"beacon_id":"b_01"}}}]"#
        );
        let mut editor = rendered(EXPAND_EAST);
        assert_eq!(editor.begin_pick(to, revision, &to_at), Ok(()));
        editor.cancel_pick();
        assert_eq!(editor.picking(), None);
    }

    /// **A drag is one `move`** within the rule's own array, onto the other rule's place.
    #[test]
    fn a_drag_is_one_move_within_the_rules_own_array() {
        let mut editor = rendered(EXPAND_EAST);
        let revision = editor.rules_rendering().expect("a rendering");
        let rules = editor.rule_list().rules.clone();
        let route_1 = rules
            .iter()
            .position(|rule| rule.pointer == "/declarative/route/1")
            .expect("step 2");
        let handler = rules
            .iter()
            .position(|rule| rule.pointer == "/declarative/handlers/0")
            .expect("rule 1");
        assert_eq!(
            editor.move_rule(&rule_named(&editor, route_1), &rule_named(&editor, handler)),
            Err(RuleEditRefusal::OtherList),
            "a step is not a rule"
        );
        assert_eq!(editor.status().key, "rule_refused_other_list");
        assert_eq!(
            editor.move_rule(
                &RuleNamed {
                    rendering: revision.wrapping_add(7),
                    ..rule_named(&editor, route_1)
                },
                &rule_named(&editor, 0)
            ),
            Err(RuleEditRefusal::Changed)
        );
        assert_eq!(
            editor.move_rule(&rule_named(&editor, route_1), &rule_named(&editor, 0)),
            Ok(())
        );
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(
            patch_of_call(&call),
            r#"[{"op":"move","from":"/declarative/route/1","path":"/declarative/route/0"}]"#
        );
    }

    /// **The gateway's refusal of a rule-list edit shows on that rule**, in its words.
    #[test]
    fn a_refused_chip_edit_shows_the_gateways_words_on_its_rule() {
        let mut editor = rendered(EXPAND_EAST);
        let (pct, revision, pointer) = drawn(&editor, "/cmdr_hp_pct/pct");
        assert_eq!(editor.set_number(pct, revision, &pointer, 35), Ok(()));
        let _ = editor.next_call(false);
        editor
            .answered(Err(("INVALID_ARGUMENT", "no such member: pct")))
            .expect("reads");
        let refusal = editor.rule_refusal().expect("the refusal");
        assert_eq!(refusal.detail, "INVALID_ARGUMENT: no such member: pct");
        let rule = editor
            .rule_list()
            .rule_holding(&refusal.pointer)
            .and_then(|index| editor.rule_list().rules.get(index))
            .expect("its rule");
        assert_eq!(rule.pointer, "/declarative/handlers/0");
        assert_eq!(editor.status().key, "gateway_refused");
    }

    /// **A rendering this client cannot read is said, not waited on** (M4): a `render_plan`
    /// answer whose chip runs past its line is refused by the client, and the rule list's
    /// line says so from then on instead of "Rendering...".
    #[test]
    fn an_unreadable_rendering_is_said_and_not_waited_on() {
        let mut editor = loaded(EXPAND_EAST);
        for _ in 0..16 {
            let Some(call) = editor.next_call(false) else {
                break;
            };
            if call.method == "render_plan" {
                let bad = read(
                    r#"{"prose":"Route\n  1. go\n","rules":[{"pointer":"/declarative/route/0","line":1,"line_count":1,"chips":[{"pointer":"/a","line":1,"start":2,"end":99,"value_json":"1"}]}]}"#,
                );
                assert!(editor.answered(Ok(&bad)).is_err(), "the client refuses it");
                continue;
            }
            let answer = match call.method {
                "estimate_route" => read(r#"{"reachable":true}"#),
                "resolve_refs" => read(r#"{"refs":[]}"#),
                _ => quick_report("", true),
            };
            editor.answered(Ok(&answer)).expect("reads");
        }
        assert!(!editor.prose_current());
        match editor.unchecked(Check::Prose) {
            Some(Unchecked::Unreadable(said)) => assert!(said.contains("not inside"), "{said}"),
            other => panic!("the rule list's line waits: {other:?}"),
        }
        assert_eq!(
            editor.unchecked(Check::Rows),
            None,
            "one check's refusal is its own"
        );
    }

    /// **The conflict connector's two rules** are the rules holding a W0702 row's path and its
    /// related path, read only while the rows and the rule list describe the text on screen.
    #[test]
    fn a_w0702_row_names_the_two_rules_the_connector_joins() {
        let mut editor = rendered(EXPAND_EAST);
        let later = compact_row("/declarative/handlers/0/when", "/declarative/route/0");
        let diagnostic = format!(
            r#"{{"code":"W0702","severity":"warning","path":"/declarative/handlers/0/when","related_paths":["/declarative/route/1"],"beginner":"{later}"}}"#
        );
        // A fresh QUICK of the same text: the rows change, the rule list stays the text's.
        editor.owe_checks();
        let _ = editor.next_call(false);
        editor
            .answered(Ok(&quick_report(&diagnostic, true)))
            .expect("reads");
        let conflicts = editor.conflicts();
        let [conflict] = conflicts.as_slice() else {
            panic!("one conflict: {conflicts:?}");
        };
        let pointer = |index: usize| {
            editor
                .rule_list()
                .rules
                .get(index)
                .map(|rule| rule.pointer.clone())
        };
        assert_eq!(
            pointer(conflict.later).as_deref(),
            Some("/declarative/handlers/0")
        );
        assert_eq!(
            pointer(conflict.earlier).as_deref(),
            Some("/declarative/route/1")
        );
        assert_eq!(conflict.sentence, later);
    }

    fn compact_row(a: &str, b: &str) -> String {
        format!("{a} is always beaten by {b}")
    }

    /// **New** (the P6a finding): the shipped empty playbook's bytes go on screen as one edit,
    /// with no call of its own and no file, the wizard closed; its checks are owed like any
    /// edit's, and Undo gives back the text and the file before it. With the door shut it is
    /// refused at once, and bytes that are not text are refused before anything is queued.
    #[test]
    fn new_puts_the_empty_playbook_on_screen_as_one_edit_undo_takes_back() {
        const NEW: &str = include_str!("../../../godot/playbooks/new.jsonc");
        let mut editor = Editor::new("seat.0");
        assert!(editor.open(EXPAND_EAST.as_bytes(), Some("C:/mine.jsonc")));
        let _ = editor.next_call(false);
        editor.answered(Ok(&quick_report("", true))).expect("reads");
        drain(&mut editor, r#"{"drafts":[]}"#);
        editor.wizard_open("safe_playbook");
        assert_eq!(method_of(editor.next_call(false)), "instantiate_template");
        editor
            .answered(Err(("NOT_FOUND", "no such template")))
            .expect("reads");
        assert!(editor.wizard().is_some(), "a refused wizard stays open");
        let before = editor.revision();
        assert!(editor.start_new(NEW.as_bytes()));
        assert_eq!(
            method_of(editor.next_call(false)),
            "verify_plan",
            "New makes no call of its own: the next is QUICK of the new text"
        );
        assert_eq!(
            editor.bytes(),
            NEW.as_bytes(),
            "the shipped bytes, byte for byte"
        );
        assert_eq!(editor.revision(), before.wrapping_add(1));
        assert_eq!(editor.file(), None, "a new playbook is no file's");
        assert!(
            editor.wizard().is_none(),
            "the template's pages describe another text"
        );
        assert_eq!(editor.status().key, "new_opened");
        editor
            .answered(Ok(&quick_report("", false)))
            .expect("reads");
        assert_eq!(editor.undo_depth(), 1);
        assert!(editor.undo());
        let _ = editor.next_call(false);
        assert_eq!(editor.bytes(), EXPAND_EAST.as_bytes(), "Undo takes it back");
        assert_eq!(editor.file(), Some("C:/mine.jsonc"));

        assert!(!editor.start_new(&[0xff, 0xfe]));
        assert_eq!(editor.status().key, "load_not_text");
        editor.set_door(Door::Push);
        assert!(!editor.start_new(NEW.as_bytes()));
        assert_eq!(editor.status().key, "closed_push");
    }

    /// **The P6a finding (item 139 (2) C)**: the vent menu's "the nearest vent you can cover",
    /// chosen with Alt held or picked from the menu without it, is one `patch_plan` whose
    /// patch appends the description, `covering {vent {NEAREST, UNCOVERED}}`, exactly.
    #[test]
    fn the_nearest_vent_you_can_cover_composes_the_description_patch() {
        let mut editor = loaded(r#"{"declarative":{"route":[]}}"#);
        assert!(editor.act(
            Action::PlaceCovering,
            &Target::NearestUncovered(FeatureKind::Vent)
        ));
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(call.method, "patch_plan");
        let patch = match call.params.get("json_patch") {
            Some(Json::String(patch)) => patch.clone(),
            other => panic!("no patch: {other:?}"),
        };
        assert_eq!(
            compact(&read(&patch)),
            r#"[{"op":"add","path":"/declarative/route/-","value":{"label":"place_1","place_beacon":{"at":{"covering":{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}}}}}]"#
        );
    }

    /// The chips are `resolve_refs`' answer for the text on screen, asked after the route is
    /// priced and only for a text that holds a feature reference.
    #[test]
    fn the_chips_are_asked_for_a_text_with_a_feature_reference_and_only_then() {
        let mut editor = loaded(EXPAND_EAST);
        drain(&mut editor, r#"{"drafts":[]}"#);
        assert!(
            editor.chips_current(),
            "no reference: no call, and no chips"
        );
        assert!(editor.chips().is_empty());

        assert!(editor.act(
            Action::PlaceCovering,
            &Target::Feature("vent_120_88".to_owned())
        ));
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(call.method, "patch_plan");
        let covering = r#"{"declarative":{"route":[{"label":"place_1","place_beacon":{"at":{"covering":{"feature_id":"vent_120_88"}}}}]}}"#;
        editor
            .answered(Ok(&patched(covering, "[]")))
            .expect("reads");
        assert_eq!(method_of(editor.next_call(false)), "verify_plan");
        editor.answered(Ok(&quick_report("", true))).expect("reads");
        // No commander in this test, so no route to price: the chips come next.
        let call = editor.next_call(false).expect("the chips");
        assert_eq!(call.method, "resolve_refs");
        assert!(json::write(&call.params).contains("vent_120_88"));
        assert!(!editor.chips_current());
        editor
            .answered(Ok(&read(
                r#"{"refs":[{"pointer":"/declarative/route/0/place_beacon/at/covering","feature_id":"vent_120_88","travel_ms":14000,"candidates":[{"feature_id":"vent_120_88","travel_ms":14000}],"matched":1,"failure":""}]}"#,
            )))
            .expect("reads");
        assert!(editor.chips_current());
        let chip = editor.chips().first().expect("one chip");
        assert_eq!(chip.form, crate::targeting::ChipForm::Name);
        assert_eq!(chip.now.as_ref().map(|now| now.travel_ms), Some(14_000));
        // A dropped connection asks for the chips again.
        assert!(editor.undo());
        let _ = editor.next_call(false);
        editor
            .answered(Ok(&patched(covering.replace("88", "89").as_str(), "[]")))
            .expect("reads");
        let _ = editor.next_call(false);
        editor.answered(Ok(&quick_report("", true))).expect("reads");
        assert_eq!(method_of(editor.next_call(false)), "resolve_refs");
        editor.dropped();
        assert_eq!(method_of(editor.next_call(false)), "resolve_refs");
    }

    /// Every Lull reads the briefing (for the "this round" sentence, item 133 (3) (i)) and the
    /// map's features; an accepted seal reads the briefing again, because a re-seal changes
    /// the sentence at once.
    #[test]
    fn every_lull_and_every_seal_reads_the_this_round_sentence() {
        let mut editor = loaded(EXPAND_EAST);
        editor.lull_opened(1);
        let mut asked: Vec<&str> = Vec::new();
        for _ in 0..16 {
            let Some(call) = editor.next_call(false) else {
                break;
            };
            asked.push(call.method);
            let answer = match call.method {
                "get_briefing" => read(
                    r#"{"notes":"n","prose":"Round 1 of 3, the Lull. This round: step 1 finds no vent you can cover."}"#,
                ),
                "get_vocabulary" => read(r#"{"families":[]}"#),
                "get_map_summary" => read(
                    r#"{"features":[{"feature_id":"vent_120_88","kind":"vent","grade":"rich","x":120,"y":88,"live":true},{"feature_id":"seam_3_4","kind":"seam","x":3,"y":4,"live":true,"covered":true}]}"#,
                ),
                "list_drafts" => read(r#"{"drafts":[]}"#),
                "list_templates" => read(r#"{"templates":[]}"#),
                "render_plan" => read(r#"{"prose":"Playbook\n"}"#),
                "resolve_refs" => read(r#"{"refs":[]}"#),
                _ => quick_report("", true),
            };
            editor.answered(Ok(&answer)).expect("reads");
        }
        assert!(
            asked.contains(&"get_briefing") && asked.contains(&"get_map_summary"),
            "{asked:?}"
        );
        assert_eq!(
            editor.this_round(),
            Some("This round: step 1 finds no vent you can cover.")
        );
        let features = editor.features();
        assert_eq!(features.len(), 2);
        assert_eq!(
            features
                .first()
                .map(|feature| (feature.kind, feature.x, feature.y)),
            Some((FeatureKind::Vent, 120, 88))
        );
        assert!(features.get(1).is_some_and(|feature| feature.covered));

        assert!(editor.submit());
        assert_eq!(method_of(editor.next_call(false)), "submit_plan");
        editor
            .answered(Ok(&read(
                r#"{"report":{"qualifies":true,"depth":"full"},"accepted":true}"#,
            )))
            .expect("reads");
        assert_eq!(
            method_of(editor.next_call(false)),
            "get_briefing",
            "the sentence is read again after the seal"
        );
        editor
            .answered(Ok(&read(
                r#"{"notes":"n","prose":"Round 1 of 3, the Lull."}"#,
            )))
            .expect("reads");
        assert_eq!(
            editor.this_round(),
            None,
            "a briefing with no sentence has none"
        );

        // A new round forgets last round's sentence and features until they are read again.
        editor.lull_opened(2);
        assert_eq!(editor.this_round(), None);
        assert!(editor.features().is_empty());
    }

    /// E0601's checkbox: ticking it applies the verifier's own patch, clearing it removes
    /// the option the text sets, and either is refused while the rows are stale.
    #[test]
    fn the_dormant_checkbox_is_the_verifiers_patch_and_its_removal() {
        let mut editor = Editor::new("seat.0");
        assert!(editor.load(br#"{"declarative":{"route":[]}}"#));
        let _ = editor.next_call(false);
        let e0601 = r#"{"code":"E0601","severity":"error","path":"/declarative/route/0","beginner":"Short of power.","suggestions":[{"title":"Allow dormant beacons","json_patch":"[{\"op\":\"add\",\"path\":\"/declarative/options\",\"value\":{\"allow_dormant_beacons\":true}}]","applicability":"maybe_incorrect"}]}"#;
        editor
            .answered(Ok(&quick_report(e0601, false)))
            .expect("reads");
        let shown = editor.dormant();
        assert!(shown.shown && shown.enabled && !shown.checked, "{shown:?}");
        assert!(
            editor
                .rows()
                .first()
                .is_some_and(|row| row.fixes.is_empty()),
            "a choice, not a Fix button"
        );
        assert_eq!(
            editor.set_allow_dormant(false),
            Err(DormantRefusal::NothingToChange),
            "nothing to clear"
        );
        assert_eq!(editor.status().key, "dormant_refused_nothing_to_change");
        assert_eq!(
            editor.dormant().refused,
            Some(DormantRefusal::NothingToChange),
            "said beside the box"
        );
        assert_eq!(editor.set_allow_dormant(true), Ok(()));
        assert_eq!(editor.status().key, "dormant_allowing");
        assert_eq!(editor.dormant().refused, None);
        assert!(
            editor.dormant().enabled,
            "the box still takes a click while the edit is on its way"
        );
        // A click while the edit is on its way reaches the bridge, and is refused by its reason.
        assert_eq!(editor.set_allow_dormant(false), Err(DormantRefusal::Busy));
        assert_eq!(editor.status().key, "dormant_refused_busy");
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(call.method, "patch_plan");
        assert!(json::write(&call.params).contains("allow_dormant_beacons"));
        let allowed = r#"{"declarative":{"route":[],"options":{"allow_dormant_beacons":true}}}"#;
        editor.answered(Ok(&patched(allowed, "[]"))).expect("reads");
        assert_eq!(editor.status().key, "dormant_allowed");
        assert!(editor.dormant().checked);
        assert_eq!(
            editor.set_allow_dormant(false),
            Err(DormantRefusal::Stale),
            "the rows are stale: its checks are on their way"
        );
        assert_eq!(editor.status().key, "dormant_refused_stale");
        assert_eq!(method_of(editor.next_call(false)), "verify_plan");
        let w0603 = r#"{"code":"W0603","severity":"warning","path":"/declarative/route/0","beginner":"Some beacons will go dark."}"#;
        editor
            .answered(Ok(&quick_report(w0603, true)))
            .expect("reads");
        assert!(editor.dormant().enabled);
        assert_eq!(editor.set_allow_dormant(false), Ok(()));
        let call = editor.next_call(false).expect("the removal");
        editor.set_door(Door::Push);
        assert!(
            !editor.dormant().enabled,
            "the shut door is the one disabled state"
        );
        assert_eq!(editor.set_allow_dormant(true), Err(DormantRefusal::Shut));
        assert_eq!(editor.status().key, "closed_push");
        let written = json::write(&call.params);
        assert!(
            written.contains("remove") && written.contains("allow_dormant_beacons"),
            "{written}"
        );
    }

    /// A hand-written option in proto JSON's `camelCase` shows ticked, and clearing it
    /// removes that key as the text spells it (review of `ui`: the removal used to name
    /// the `snake_case` key only, which `patch_plan` refuses for a key that is not there).
    #[test]
    fn the_dormant_checkbox_clears_the_key_the_text_spells() {
        let mut editor = Editor::new("seat.0");
        let camel = r#"{"declarative":{"route":[],"options":{"allowDormantBeacons":true}}}"#;
        assert!(editor.load(camel.as_bytes()));
        let _ = editor.next_call(false);
        let w0603 = r#"{"code":"W0603","severity":"warning","path":"/declarative/route/0","beginner":"Some beacons will go dark."}"#;
        editor
            .answered(Ok(&quick_report(w0603, true)))
            .expect("reads");
        let shown = editor.dormant();
        assert!(shown.shown && shown.checked && shown.enabled, "{shown:?}");
        assert_eq!(editor.set_allow_dormant(false), Ok(()));
        let call = editor.next_call(false).expect("the removal");
        let written = json::write(&call.params);
        assert!(
            written.contains("/declarative/options/allowDormantBeacons")
                && !written.contains("allow_dormant_beacons"),
            "{written}"
        );
        assert_eq!(dormant_key(camel), Some("allowDormantBeacons"));
        assert_eq!(
            dormant_key(r#"{"declarative":{"options":{"allow_dormant_beacons":false}}}"#),
            None
        );
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
        // The seal changes the Lull's "this round" sentence, so the briefing is read again.
        assert_eq!(method_of(editor.next_call(false)), "get_briefing");
        editor
            .answered(Ok(&read(r#"{"notes":"remember the vent"}"#)))
            .expect("reads");
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
                "get_vocabulary" => read(r#"{"families":[]}"#),
                "get_map_summary" => read(r#"{"features":[]}"#),
                "resolve_refs" => read(r#"{"refs":[]}"#),
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

    /// Finding A of S1's stage demo: round 1's Lull drew no "This round" sentence. The
    /// gateway says it only for a sealed or carried playbook (`Surface::this_round`), so in
    /// round 1 there is none before the first Submit; after an accepted Submit the briefing
    /// is asked again and its sentence is the one the panel draws. This passed on the code
    /// the demo played: the sheet's step 8 read the panel before step 11's Submit.
    #[test]
    fn round_ones_sentence_comes_with_its_first_accepted_submit() {
        let sentence = "This round: step 1 places a new beacon near (324, 16), $ 60.";
        let mut editor = loaded(EXPAND_EAST);
        editor.lull_opened(1);
        let mut briefings = 0;
        for _ in 0..16 {
            let Some(call) = editor.next_call(false) else {
                break;
            };
            let answer = match call.method {
                // Nothing is sealed or carried in round 1, so the gateway's prose has no
                // "This round" sentence.
                "get_briefing" => {
                    briefings += 1;
                    read(r#"{"notes":"","prose":"Round 1 of 3, the Lull. You hold 1 beacon."}"#)
                }
                "get_vocabulary" => read(r#"{"families":[]}"#),
                "get_map_summary" => read(r#"{"features":[]}"#),
                "list_drafts" => read(r#"{"drafts":[]}"#),
                "list_templates" => read(r#"{"templates":[]}"#),
                "render_plan" => read(r#"{"prose":"Playbook\n"}"#),
                "resolve_refs" => read(r#"{"refs":[]}"#),
                _ => quick_report("", true),
            };
            editor.answered(Ok(&answer)).expect("reads");
        }
        assert_eq!(briefings, 1, "the Lull's briefing was read");
        assert_eq!(
            editor.this_round(),
            None,
            "no sentence before the first Submit"
        );

        assert!(editor.submit());
        assert_eq!(method_of(editor.next_call(false)), "submit_plan");
        editor
            .answered(Ok(&read(
                r#"{"report":{"qualifies":true,"depth":"full"},"accepted":true}"#,
            )))
            .expect("reads");
        let call = editor.next_call(false).expect("the briefing again");
        assert_eq!(call.method, "get_briefing", "asked again after the seal");
        let changes = editor.changes();
        editor
            .answered(Ok(&read(&format!(
                r#"{{"notes":"","prose":"Round 1 of 3, the Lull. You hold 1 beacon. {sentence}"}}"#
            ))))
            .expect("reads");
        assert_eq!(
            editor.this_round(),
            Some(sentence),
            "and its sentence is drawn"
        );
        assert!(editor.changes() > changes, "the panel is told to redraw");
    }

    /// Finding G: Save as offers the name of the file the text on screen came from, and no
    /// name when it came from a template, a carried seal or no file at all, so one Enter
    /// cannot overwrite another round's file unasked (the dialog confirms an overwrite).
    #[test]
    fn the_texts_file_is_the_one_it_was_opened_from_or_saved_to_and_no_other() {
        let mut editor = Editor::new("seat.0");
        assert!(editor.open(EXPAND_EAST.as_bytes(), Some("C:/plays/round1.jsonc")));
        assert_eq!(
            editor.file(),
            None,
            "not until QUICK has seen it and it opened"
        );
        let _ = editor.next_call(false);
        editor.answered(Ok(&quick_report("", true))).expect("reads");
        assert_eq!(editor.file(), Some("C:/plays/round1.jsonc"));
        // An edit keeps the file: it is still that file's text.
        assert!(editor.act(Action::Go, &Target::Beacon("b_00".to_owned())));
        let _ = editor.next_call(false);
        editor
            .answered(Ok(&patched(r#"{"edited":1}"#, "[]")))
            .expect("reads");
        assert_eq!(editor.file(), Some("C:/plays/round1.jsonc"));
        drain(&mut editor, r#"{"drafts":[]}"#);
        // A refused Load changes nothing, the file included.
        assert!(editor.open(b"{}", Some("C:/plays/bad.json")));
        let _ = editor.next_call(false);
        let e0003 =
            r#"{"code":"E0003","severity":"error","path":"/declarative","message":"held back"}"#;
        editor
            .answered(Ok(&quick_report(e0003, false)))
            .expect("reads");
        assert_eq!(editor.file(), Some("C:/plays/round1.jsonc"));
        // The wizard's playbook is no file's; Undo gives the text and its file back.
        editor.queue.push_back(Job::UseWizard {
            text: r#"{"wizard":1}"#.to_owned(),
        });
        drain(&mut editor, r#"{"drafts":[]}"#);
        assert_eq!(editor.bytes(), br#"{"wizard":1}"#);
        assert_eq!(editor.file(), None, "a template's playbook offers no name");
        assert!(editor.undo());
        assert_eq!(
            method_of(editor.next_call(false)),
            "verify_plan",
            "the text before the wizard's comes back with no call, and is checked again"
        );
        editor.answered(Ok(&quick_report("", true))).expect("reads");
        assert_eq!(editor.bytes(), br#"{"edited":1}"#);
        assert_eq!(editor.file(), Some("C:/plays/round1.jsonc"));
        drain(&mut editor, r#"{"drafts":[]}"#);
        // Saving names the file from then on.
        editor.saved_to("C:/plays/round1b.jsonc");
        assert_eq!(editor.file(), Some("C:/plays/round1b.jsonc"));
        // A carried draft is last round's seal, from the gateway: no file of the player's.
        editor.lull_opened(2);
        drain(
            &mut editor,
            r#"{"drafts":[{"draft_id":"carried","label":"carried from round 1","round":2}]}"#,
        );
        assert_eq!(editor.status().key, "carried");
        assert_eq!(editor.file(), None, "the carried draft offers no name");
        // And a text loaded from no file has none.
        let bare = loaded(EXPAND_EAST);
        assert_eq!(bare.file(), None);
    }

    /// Finding K, in a Lull: a check the gateway refuses says so, with the gateway's words,
    /// where the panel would otherwise say "Checking..." until the next edit.
    #[test]
    fn a_refused_check_says_why_instead_of_checking() {
        let mut editor = loaded(EXPAND_EAST);
        assert!(editor.act(Action::Go, &Target::Beacon("b_00".to_owned())));
        let _ = editor.next_call(false);
        editor
            .answered(Ok(&patched(r#"{"edited":1}"#, "[]")))
            .expect("reads");
        assert_eq!(
            editor.unchecked(Check::Rows),
            None,
            "the check is on its way"
        );
        assert_eq!(method_of(editor.next_call(false)), "verify_plan");
        let settled = editor
            .answered(Err(("INTERNAL", "the verifier could not run")))
            .expect("settled");
        assert_eq!(settled, Settled::Refused);
        assert!(!editor.rows_current());
        assert_eq!(
            editor.unchecked(Check::Rows),
            Some(Unchecked::Refused(
                "INTERNAL: the verifier could not run".to_owned()
            ))
        );
        // QUICK's refusal is the verdict line's alone: the route, the prose and the chips,
        // whose own checks are still owed, keep their waiting words.
        for other in [Check::Route, Check::Prose, Check::Chips] {
            assert_eq!(editor.unchecked(other), None, "{other:?}");
        }
        // A new edit is a new text, whose checks are owed again.
        drain(&mut editor, r#"{"drafts":[]}"#);
        assert!(editor.act(Action::Go, &Target::Beacon("b_01".to_owned())));
        let _ = editor.next_call(false);
        editor
            .answered(Ok(&patched(r#"{"edited":2}"#, "[]")))
            .expect("reads");
        assert_eq!(editor.unchecked(Check::Rows), None);
    }

    /// A refused estimate says so on the route's line only; the chips, whose
    /// `resolve_refs` is still on its way, keep "Reading the map..." (review of `fixd`).
    #[test]
    fn a_refusal_is_shown_on_its_own_checks_line_only() {
        let mut editor = loaded(EXPAND_EAST);
        editor.set_entities(&[Entity {
            id: "u_1".to_owned(),
            kind: EntityKind::Unit,
            subtype: "commander".to_owned(),
            owner: "seat.0".to_owned(),
            at: [358, 22, 36],
            hp: None,
            heading: None,
        }]);
        assert_eq!(method_of(editor.next_call(false)), "estimate_route");
        editor
            .answered(Err(("INTERNAL", "no estimate")))
            .expect("settled");
        assert!(!editor.chips_current(), "the chips' own read is still owed");
        assert_eq!(
            editor.unchecked(Check::Route),
            Some(Unchecked::Refused("INTERNAL: no estimate".to_owned()))
        );
        for other in [Check::Rows, Check::Prose, Check::Chips] {
            assert_eq!(editor.unchecked(other), None, "{other:?}");
        }
    }

    /// A call lost on a seat-connection drop with the door shut is not asked again behind a
    /// door that may never open: nothing waits, and the status line says the ask was not
    /// sent (review of `fixd`; before, a dropped edit was queued for the next Lull and the
    /// panel said "Checking..." until then, or for ever after the match ended).
    #[test]
    fn a_call_dropped_behind_a_shut_door_is_not_kept() {
        let mut editor = loaded(EXPAND_EAST);
        drain(&mut editor, r#"{"drafts":[]}"#);
        assert!(editor.act(Action::Go, &Target::Beacon("b_00".to_owned())));
        assert_eq!(method_of(editor.next_call(false)), "patch_plan");
        editor.set_door(Door::Ended);
        editor.dropped();
        assert!(!editor.busy(), "nothing is in flight or queued");
        assert!(!editor.has_pending_jobs());
        assert_eq!(editor.status().key, "closed_ended");
        assert_eq!(
            editor.unchecked(Check::Rows),
            Some(Unchecked::Closed(Door::Ended))
        );
        assert!(editor.next_call(true).is_none());
    }

    /// Finding J: "Ready to seal" and "Cannot be sealed yet" are said only while the door is
    /// open; outside a Lull nothing is added, unless the gateway filed this seat's safe
    /// playbook, when the line says the text on screen was not sealed. A new round forgets
    /// the filing.
    #[test]
    fn ready_to_seal_is_said_only_in_an_open_lull() {
        let mut editor = loaded(EXPAND_EAST);
        editor.lull_opened(3);
        drain(&mut editor, r#"{"drafts":[]}"#);
        let full = editor.next_call(true).expect("FULL");
        assert!(full.full);
        editor
            .answered(Ok(&read(
                r#"{"report":{"qualifies":true,"depth":"full","diagnostics":[]}}"#,
            )))
            .expect("reads");
        assert_eq!(editor.verdict(), Verdict::Full);
        assert_eq!(editor.seal_line(), SealLine::Ready);
        editor.set_door(Door::Push);
        assert_eq!(
            editor.seal_line(),
            SealLine::None,
            "nothing is ready to seal in a Push"
        );
        editor.this_round = Some("This round: step 1 places a new beacon.".to_owned());
        editor.safe_playbook_filed("The Lull ran out, so the safe playbook was filed for you.");
        assert_eq!(editor.seal_line(), SealLine::Filed);
        assert_eq!(editor.status().key, "safe_filed");
        assert_eq!(
            editor.this_round(),
            None,
            "the sentence described orders that were not sealed"
        );
        assert_eq!(editor.verdict(), Verdict::Full, "the verifier's word stays");
        editor.set_door(Door::Open);
        editor.lull_opened(4);
        assert!(!editor.filed(), "a filing is its round's");
    }

    /// Finding K: shutting the door drops what will never go out, says so when the player
    /// had asked for something, refuses asks at once, and opening it again in the same Lull
    /// owes the text's checks again.
    #[test]
    fn a_shut_door_drops_what_waited_and_refuses_asks() {
        let mut editor = loaded(EXPAND_EAST);
        drain(&mut editor, r#"{"drafts":[]}"#);
        let changes = editor.changes();
        editor.set_door(Door::Recap);
        assert!(editor.changes() > changes);
        assert_eq!(
            editor.status().key,
            "loaded",
            "with nothing asked, the status line keeps its word"
        );
        assert!(!editor.save_draft("x"));
        assert_eq!(editor.status().key, "closed_recap");
        editor.save_notes("kept in the box");
        editor.describe_beacon("b_00");
        editor.list_templates();
        editor.wizard_open("hold_and_build");
        assert!(editor.wizard().is_none());
        assert!(!editor.has_pending_jobs());
        assert!(editor.next_call(true).is_none());

        // Ready, then a refused set_ready: the door opens again in the same Lull.
        let mut editor = loaded(EXPAND_EAST);
        assert!(editor.act(Action::Go, &Target::Beacon("b_00".to_owned())));
        editor.set_door(Door::Ready);
        assert_eq!(editor.status().key, "closed_ready", "the edit was not sent");
        assert!(!editor.has_pending_jobs());
        assert_eq!(
            editor.unchecked(Check::Rows),
            Some(Unchecked::Closed(Door::Ready)),
            "the loaded text's checks will not come"
        );
        editor.set_door(Door::Open);
        assert_eq!(editor.unchecked(Check::Rows), None);
        assert_eq!(
            method_of(editor.next_call(false)),
            "verify_plan",
            "its checks are owed again"
        );
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
            hp: None,
            heading: None,
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

    /// Answers every call the editor owes, so a test can go on with an idle editor.
    fn drain_checks(editor: &mut Editor) {
        while let Some(call) = editor.next_call(false) {
            let answer = match call.method {
                "verify_plan" => quick_report("", true),
                "render_plan" => read(r#"{"prose":"","_status":{"phase":"lull"}}"#),
                _ => read(r#"{"_status":{"phase":"lull"}}"#),
            };
            editor.answered(Ok(&answer)).expect("an answer");
        }
    }

    /// Finding H (item 139 (2); item 155 (3) 2): after Undo took the wizard's playbook back the
    /// status line still said "Undo takes it back", with nothing left to undo. Both ways of
    /// undoing say so, and a map action clears a line that no longer describes the text.
    #[test]
    fn undo_and_a_map_action_leave_no_stale_status_line() {
        let mut editor = loaded("{}");
        drain_checks(&mut editor);
        editor.queue.push_back(Job::UseWizard {
            text: "{\"a\":1}".to_owned(),
        });
        drain_checks(&mut editor);
        assert_eq!(editor.status().key, "wizard_used");
        assert_eq!(editor.undo_depth(), 1);
        assert!(editor.undo());
        drain_checks(&mut editor);
        assert_eq!(
            editor.undo_depth(),
            0,
            "the wizard's playbook is taken back"
        );
        assert_eq!(editor.status().key, "undone");
        assert_eq!(editor.text.as_deref(), Some("{}"));

        // A patched edit's Undo says the same, and the map action before it cleared the line.
        editor.say("submitted", "");
        assert!(editor.act(Action::Go, &Target::Voxel([1, 2, 3])));
        let call = editor.next_call(false).expect("the map action's patch");
        assert_eq!(call.method, "patch_plan");
        editor
            .answered(Ok(&patched("{\"b\":1}", "[]")))
            .expect("the patched text");
        assert_eq!(
            editor.status().key,
            "",
            "the submission's line is stale now"
        );
        drain_checks(&mut editor);
        assert!(editor.undo());
        let call = editor.next_call(false).expect("the inverse patch");
        assert_eq!(call.method, "patch_plan");
        editor
            .answered(Ok(&patched("{}", "[]")))
            .expect("the patched text");
        assert_eq!(editor.status().key, "undone");
    }

    /// Item 155 (3) 3: Load opening a different file closes the wizard, whose pages describe
    /// another playbook; a Load the verifier refuses opens nothing and leaves it.
    #[test]
    fn load_opening_a_file_closes_the_wizard_and_a_refused_load_does_not() {
        let mut editor = loaded("{}");
        drain_checks(&mut editor);
        editor.wizard = Some(Wizard::new("hold_and_build", 1));
        assert!(editor.load(b"not a playbook"));
        let call = editor.next_call(false).expect("Load's QUICK");
        assert_eq!(call.method, "verify_plan");
        editor
            .answered(Ok(&quick_report(
                r#"{"code":"E0002","severity":"error","path":"/x","message":"m"}"#,
                false,
            )))
            .expect("the refusal");
        assert_eq!(editor.verdict(), Verdict::Refused);
        assert!(editor.wizard().is_some(), "nothing was opened");

        assert!(editor.load(b"{\"other\":1}"));
        let call = editor.next_call(false).expect("Load's QUICK");
        assert_eq!(call.method, "verify_plan");
        editor
            .answered(Ok(&quick_report("", true)))
            .expect("the file opens");
        assert_eq!(editor.text.as_deref(), Some("{\"other\":1}"));
        assert!(editor.wizard().is_none(), "the wizard closes with the file");
        assert_eq!(editor.status().key, "loaded");
    }

    /// Item 143 (3): round 2's safe-playbook line stayed into round 3's Lull. A line about a
    /// filing goes at the new round, and a line about a shut door when the door opens.
    #[test]
    fn a_filing_line_and_a_shut_door_line_do_not_outlive_them() {
        let mut editor = loaded("{}");
        editor.lull_opened(2);
        editor.set_door(Door::Push);
        editor.safe_playbook_filed("The Lull ran out.");
        assert_eq!(editor.status().key, "safe_filed");
        editor.set_door(Door::Open);
        assert_eq!(editor.status().key, "safe_filed", "still round 2's");
        editor.lull_opened(3);
        assert_eq!(editor.status().key, "", "round 3 has no filing");
        assert!(!editor.filed());

        editor.set_door(Door::Push);
        assert!(!editor.submit());
        assert_eq!(editor.status().key, "closed_push");
        editor.set_door(Door::Open);
        assert_eq!(editor.status().key, "", "the door is open again");
    }

    /// A shut door's line goes when the door moves on to another shut door: an ask after Ready
    /// says nothing about the Push, nor one in the Push about the match's end.
    #[test]
    fn a_shut_doors_line_does_not_outlive_it_at_another_shut_door() {
        let mut editor = loaded("{}");
        editor.lull_opened(1);
        editor.set_door(Door::Ready);
        assert!(!editor.submit());
        assert_eq!(editor.status().key, "closed_ready");
        editor.set_door(Door::Push);
        assert_eq!(
            editor.status().key,
            "",
            "Ready's line does not survive into the Push"
        );

        assert!(!editor.submit());
        assert_eq!(editor.status().key, "closed_push");
        editor.set_door(Door::Recap);
        assert_eq!(editor.status().key, "", "nor the Push's into the recap");

        assert!(!editor.submit());
        assert_eq!(editor.status().key, "closed_recap");
        editor.set_door(Door::Ended);
        assert_eq!(
            editor.status().key,
            "",
            "nor the recap's past the match's end"
        );

        // Any other line stays: the filing is still what happened this round.
        editor.safe_playbook_filed("The Lull ran out.");
        editor.set_door(Door::Recap);
        assert_eq!(editor.status().key, "safe_filed");
    }

    /// Saving says where, from the editor's own status (so a redraw cannot wipe it).
    #[test]
    fn load_opens_where_the_last_file_was() {
        let mut editor = Editor::new("seat.0");
        assert_eq!(editor.last_file(), None);
        assert!(editor.open(br#"{"declarative":{"route":[]}}"#, Some("C:/plays/a.jsonc")));
        let _ = editor.next_call(false);
        editor.answered(Ok(&quick_report("", true))).expect("reads");
        assert_eq!(editor.last_file(), Some("C:/plays/a.jsonc"));
        // A text from no file keeps the last file Loaded or Saved, and a Save moves it.
        assert!(editor.load(br#"{"declarative":{"route":[]}}"#));
        let _ = editor.next_call(false);
        editor.answered(Ok(&quick_report("", true))).expect("reads");
        assert_eq!(editor.file(), None);
        assert_eq!(editor.last_file(), Some("C:/plays/a.jsonc"));
        editor.saved_to("D:/other/b.jsonc");
        assert_eq!(editor.last_file(), Some("D:/other/b.jsonc"));
    }

    #[test]
    fn a_save_says_where_on_the_status_line() {
        let mut editor = loaded("{}");
        editor.saved_to("round1.jsonc");
        assert_eq!(editor.status().key, "saved_file");
        assert_eq!(editor.status().detail, "round1.jsonc");
    }

    fn sized_report(units: u32, budget: u32, diagnostics: &str) -> Json {
        read(&format!(
            r#"{{"report":{{"qualifies":false,"depth":"quick","size_units":{units},"size_budget":{budget},"diagnostics":[{diagnostics}]}},"_status":{{"phase":"lull"}}}}"#
        ))
    }

    /// The size refusal the meter looks for is the one the verifier writes, read out of its
    /// source because the client may not depend on the verifier (`CLIENT_WALL`).
    #[test]
    fn the_size_refusal_is_the_one_the_verifier_writes() {
        let read = |file: &str| {
            let path = format!("{}/../verifier/src/{file}", env!("CARGO_MANIFEST_DIR"));
            std::fs::read_to_string(path)
                .expect("the verifier's source")
                .replace("\r\n", "\n")
        };
        assert!(
            read("strings.rs").contains(&format!(
                "code: \"{SIZE_REFUSAL}\",\n        message: \"the playbook is {{found}} size units;"
            )),
            "the verifier's size refusal is not `{SIZE_REFUSAL}`"
        );
        assert!(
            read("structure.rs")
                .contains(&format!("Diag::new(\"{SIZE_REFUSAL}\", \"/declarative\")")),
            "the size budget's check does not emit `{SIZE_REFUSAL}`"
        );
    }

    /// E9: the meter draws what the verifier counted. Over budget it holds the verifier's own
    /// refusal sentence, the client compares nothing, and a report with no budget draws no
    /// meter at all.
    #[test]
    fn the_size_meter_holds_what_the_verifier_counted() {
        let mut editor = loaded("{}");
        assert_eq!(editor.size_meter(), None, "no report has named a budget");
        let report = |result: &Json| report_of(result).expect("a report");
        editor.take_rows(&report(&sized_report(40, 128, "")), Verdict::Quick);
        assert_eq!(
            editor.size_meter(),
            Some(&SizeMeter {
                units: 40,
                budget: 128,
                refusal: None,
            })
        );
        let over = sized_report(
            130,
            128,
            r#"{"code":"E0110","severity":"error","path":"/declarative","message":"the playbook is 130 size units; the budget is 128.","beginner":"This playbook is larger than the seal allows."}"#,
        );
        editor.take_rows(&report(&over), Verdict::Quick);
        assert_eq!(
            editor.size_meter(),
            Some(&SizeMeter {
                units: 130,
                budget: 128,
                refusal: Some("This playbook is larger than the seal allows.".to_owned()),
            })
        );
        editor.take_rows(&report(&quick_report("", true)), Verdict::Quick);
        assert_eq!(
            editor.size_meter(),
            None,
            "a report with no budget names none"
        );
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

    #[test]
    fn a_waypoint_that_resolves_to_nothing_is_drawn_not_refused() {
        let mut editor = loaded(EXPAND_EAST);
        editor.set_entities(&[Entity {
            id: "u_1".to_owned(),
            kind: EntityKind::Unit,
            subtype: "commander".to_owned(),
            owner: "seat.0".to_owned(),
            at: [358, 22, 36],
            hp: None,
            heading: None,
        }]);
        assert_eq!(method_of(editor.next_call(false)), "estimate_route");
        let said = "waypoint 1 covers nothing: its step would fail `illegal_site`";
        let settled = editor.answered(Err(("NOT_FOUND", said))).expect("settled");
        assert_eq!(
            settled,
            Settled::Answered,
            "the gateway's answer about the route"
        );
        assert_eq!(editor.refusals(), 0, "not a refused call");
        let route = editor.route();
        assert!(route.current, "it describes the text on screen");
        assert!(!route.reachable);
        assert!(
            route.points.is_empty(),
            "no polyline to a site the gateway never named"
        );
        assert_eq!(
            route.found_nothing, said,
            "the gateway's sentence, as it came"
        );
    }

    #[test]
    fn any_other_refusal_of_the_estimate_is_a_refused_call() {
        let mut editor = loaded(EXPAND_EAST);
        editor.set_entities(&[Entity {
            id: "u_1".to_owned(),
            kind: EntityKind::Unit,
            subtype: "commander".to_owned(),
            owner: "seat.0".to_owned(),
            at: [358, 22, 36],
            hp: None,
            heading: None,
        }]);
        assert_eq!(method_of(editor.next_call(false)), "estimate_route");
        let settled = editor
            .answered(Err(("INVALID_ARGUMENT", "waypoint 1: off the map")))
            .expect("settled");
        assert_eq!(settled, Settled::Refused);
        assert_eq!(editor.refusals(), 1);
        assert!(editor.route().found_nothing.is_empty());
    }

    /// **`estimate_route`'s `PHASE_CLOSED`** (decisions-log item 169, carried to S3's `fixp`): the
    /// editor never asks for an estimate outside an open Lull, its door; and one asked in the
    /// Lull's last moment and refused `PHASE_CLOSED` is a refused call (AGENTS.md section 3 rule 4:
    /// only `NOT_FOUND` is the route's answer), counted, and shown on the route's own line.
    #[test]
    fn a_phase_closed_estimate_is_never_asked_and_is_a_refusal_when_it_comes() {
        let commander = Entity {
            id: "u_1".to_owned(),
            kind: EntityKind::Unit,
            subtype: "commander".to_owned(),
            owner: "seat.0".to_owned(),
            at: [358, 22, 36],
            hp: None,
            heading: None,
        };
        for door in [Door::Push, Door::Recap, Door::Ended, Door::Ready] {
            let mut editor = loaded(EXPAND_EAST);
            editor.set_door(door);
            editor.set_entities(std::slice::from_ref(&commander));
            for _ in 0..12 {
                assert_ne!(
                    method_of(editor.next_call(false)),
                    "estimate_route",
                    "no estimate behind a shut door ({door:?})"
                );
            }
        }
        let mut editor = loaded(EXPAND_EAST);
        editor.set_entities(std::slice::from_ref(&commander));
        assert_eq!(method_of(editor.next_call(false)), "estimate_route");
        editor.set_door(Door::Push);
        let settled = editor
            .answered(Err(("PHASE_CLOSED", "planning is closed in a Push")))
            .expect("settled");
        assert_eq!(settled, Settled::Refused);
        assert_eq!(editor.refusals(), 1, "counted as a refused call");
        assert!(
            editor.route().found_nothing.is_empty(),
            "not the route's answer"
        );
        assert_eq!(
            editor.unchecked(Check::Route),
            Some(Unchecked::Refused(
                "PHASE_CLOSED: planning is closed in a Push".to_owned()
            )),
            "shown on the route's own line"
        );
    }

    // --- Part 2: the reads, the picker, the name picks and the Visit & change rows ---------

    const VOCABULARY: &str = include_str!("../../../godot/fixtures/vocabulary.json");
    const BEACON: &str = include_str!("../../../godot/fixtures/beacon_core.json");

    fn with_words(editor: &mut Editor) {
        editor.vocabulary = Some(vocabulary::vocabulary_of_text(VOCABULARY).expect("reads"));
    }

    fn predicate_at(editor: &Editor, family: &str, name: &str) -> (usize, usize) {
        let words = editor.vocabulary().expect("the words");
        let at = words
            .families
            .iter()
            .position(|one| one.id == family)
            .expect("the family");
        let entry = words
            .families
            .get(at)
            .and_then(|one| one.predicates.iter().position(|entry| entry.name == name))
            .expect("the entry");
        (at, entry)
    }

    fn rule_index(editor: &Editor, pointer: &str) -> usize {
        editor
            .rule_list()
            .rules
            .iter()
            .position(|rule| rule.pointer == pointer)
            .expect("the rule")
    }

    /// Answers the Lull's own reads plainly, the vocabulary with `vocabulary`.
    fn lull_reads(editor: &mut Editor, vocabulary: &dyn Fn(&mut Editor)) -> u32 {
        let mut asked = 0;
        for _ in 0..12 {
            let Some(call) = editor.next_call(false) else {
                break;
            };
            if call.method == "get_vocabulary" {
                asked += 1;
                vocabulary(editor);
                continue;
            }
            let answer = match call.method {
                "get_briefing" => read(r#"{"notes":""}"#),
                "list_drafts" => read(r#"{"drafts":[]}"#),
                "list_templates" => read(r#"{"templates":[]}"#),
                _ => read(r#"{"features":[]}"#),
            };
            editor.answered(Ok(&answer)).expect("reads");
        }
        asked
    }

    /// **Item 1: the vocabulary is asked every Lull and never carried across one**; its answer
    /// is held as typed data; a refused or unreadable answer is said on its own and nothing
    /// waits on it.
    #[test]
    fn the_vocabulary_is_read_every_lull_and_a_failed_read_is_said() {
        let mut editor = Editor::new("seat.0");
        editor.lull_opened(1);
        let answer = |editor: &mut Editor| {
            let words = read(VOCABULARY).get("result").cloned().expect("a result");
            editor.answered(Ok(&words)).expect("reads");
        };
        assert_eq!(lull_reads(&mut editor, &answer), 1);
        assert!(
            editor
                .vocabulary()
                .is_some_and(|words| !words.families.is_empty())
        );
        // The same Lull again (a door reopened) asks nothing more.
        editor.lull_opened(1);
        assert!(
            !editor
                .queue
                .iter()
                .any(|job| matches!(job, Job::Vocabulary))
        );
        // A new Lull drops the words and asks again; a refusal is said and nothing waits.
        editor.lull_opened(2);
        assert!(editor.vocabulary().is_none());
        let refuse = |editor: &mut Editor| {
            let settled = editor
                .answered(Err(("PHASE_CLOSED", "not now")))
                .expect("reads");
            assert_eq!(settled, Settled::Refused);
        };
        assert_eq!(lull_reads(&mut editor, &refuse), 1);
        assert_eq!(
            editor.vocabulary_failed(),
            Some(&ReadFailed::Refused("PHASE_CLOSED: not now".to_owned()))
        );
        assert!(
            editor.next_call(false).is_none(),
            "nothing is asked again by itself"
        );
        // An answer this client cannot read is said as such.
        editor.lull_opened(3);
        let garble = |editor: &mut Editor| {
            assert!(editor.answered(Ok(&read(r#"{"families":7}"#))).is_err());
        };
        assert_eq!(lull_reads(&mut editor, &garble), 1);
        assert!(matches!(
            editor.vocabulary_failed(),
            Some(ReadFailed::Unreadable(_))
        ));
        assert!(editor.vocabulary().is_none());
    }

    /// **Item 1: a beacon's detail is read beside its prose**; another seat's beacon yields no
    /// detail rows; a refused read is said.
    #[test]
    fn a_beacons_detail_is_read_and_another_seats_has_none() {
        let mut editor = loaded(EXPAND_EAST);
        drain_checks(&mut editor);
        editor.describe_beacon("b_00");
        let call = editor.next_call(false).expect("get_beacon");
        assert_eq!(call.method, "get_beacon");
        editor
            .answered(Ok(&read(BEACON).get("result").cloned().expect("a result")))
            .expect("reads");
        let detail = editor.beacon_detail().expect("the detail");
        assert_eq!(detail.id, "b_00");
        assert!(!detail.settings.is_empty());
        assert!(!editor.beacon_prose().is_empty());
        editor.describe_beacon("e_00");
        assert!(
            editor.beacon_detail().is_none(),
            "the old detail goes at once"
        );
        let _ = editor.next_call(false).expect("get_beacon");
        editor
            .answered(Ok(&read(
                r#"{"summary":{"beacon_id":"e_00"},"prose":"Theirs."}"#,
            )))
            .expect("reads");
        let theirs = editor.beacon_detail().expect("an empty detail");
        assert!(theirs.settings.is_empty() && theirs.targets.is_empty());
        assert!(theirs.mandate.is_empty());
        editor.describe_beacon("b_00");
        let _ = editor.next_call(false).expect("get_beacon");
        let _ = editor.answered(Err(("NOT_FOUND", "no such beacon")));
        assert_eq!(
            editor.beacon_failed(),
            Some(&ReadFailed::Refused("NOT_FOUND: no such beacon".to_owned()))
        );
    }

    /// A condition slot of the rule list on screen, as the picker opened on it names it: its
    /// index, the rendering and its pointer.
    fn slot_named(editor: &Editor, pointer: &str) -> (usize, u64, String) {
        let index = editor
            .rule_list()
            .slots
            .iter()
            .position(|slot| slot.pointer == pointer)
            .unwrap_or_else(|| panic!("no slot {pointer}"));
        (
            index,
            editor.rules_rendering().expect("a rendering"),
            pointer.to_owned(),
        )
    }

    fn pick_slot(editor: &mut Editor, pointer: &str) -> Result<(), RuleEditRefusal> {
        let (family, entry) = predicate_at(editor, "commander", "cmdr_hp_pct");
        let (slot, rendering, at) = slot_named(editor, pointer);
        editor.choose_predicate(slot, rendering, &at, family, entry)
    }

    /// **A pick sends the op the slot names, at its pointer** (decisions-log item 160 (4)): an
    /// item of an `all` of two leaves is replaced in place, never inserted beside them (an
    /// RFC 6902 `add` at a list index inserts); an empty slot takes an `add`.
    #[test]
    fn a_predicate_pick_sends_the_op_the_slot_names() {
        let mut editor = rendered(EXPAND_EAST);
        with_words(&mut editor);
        assert_eq!(
            pick_slot(&mut editor, "/declarative/handlers/0/when/all/items/0"),
            Ok(())
        );
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(call.method, "patch_plan");
        assert_eq!(
            patch_of_call(&call),
            r#"[{"op":"replace","path":"/declarative/handlers/0/when/all/items/0","value":{"cmdr_hp_pct":{"cmp":"LE","pct":40}}}]"#,
            "one item replaced, so the all still holds two"
        );
        let mut editor = rendered(EXPAND_EAST);
        with_words(&mut editor);
        assert_eq!(
            pick_slot(&mut editor, "/declarative/route/0/skip_if"),
            Ok(())
        );
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(
            patch_of_call(&call),
            r#"[{"op":"add","path":"/declarative/route/0/skip_if","value":{"cmdr_hp_pct":{"cmp":"LE","pct":40}}}]"#
        );
    }

    /// **A group slot is no pick target, and an op the client does not know is no guess**:
    /// each refused with its words, nothing sent.
    #[test]
    fn a_group_slot_and_an_unknown_op_are_refused() {
        let mut editor = rendered(EXPAND_EAST);
        with_words(&mut editor);
        assert_eq!(
            pick_slot(&mut editor, "/declarative/handlers/0/when"),
            Err(RuleEditRefusal::GroupSlot)
        );
        assert_eq!(editor.status().key, "rule_refused_group_slot");
        let mut editor = loaded(EXPAND_EAST);
        let render = read(
            r#"{"prose":"Rules\n  r1: when x.\n","rules":[{"pointer":"/declarative/handlers/0","line":1,"line_count":1,"slots":[{"pointer":"/declarative/handlers/0/when","title":"when","line":1,"state":"leaf","predicate":"cmdr_hp_pct","pick_op":"unspecified"},{"pointer":"/declarative/handlers/0/body/0/skip_if","title":"step 1, skip if","line":1,"state":"empty","predicate":"","pick_op":7}]}],"_status":{"phase":"lull"}}"#,
        );
        rerender(&mut editor, &render);
        with_words(&mut editor);
        for pointer in [
            "/declarative/handlers/0/when",
            "/declarative/handlers/0/body/0/skip_if",
        ] {
            assert_eq!(
                pick_slot(&mut editor, pointer),
                Err(RuleEditRefusal::NoPickOp)
            );
            assert_eq!(editor.status().key, "rule_refused_no_pick_op");
        }
        assert!(!editor.has_pending_jobs(), "nothing was sent");
    }

    /// **An op string the gateway never sends is the decode's refusal**: the rendering is
    /// refused whole, so the list stays stale and a pick is refused with that reason, never sent.
    /// (`NoPickOp` is reached only by `unspecified` on a slot that is not a group.)
    #[test]
    fn an_unknown_op_string_leaves_the_list_stale_and_sends_nothing() {
        let mut editor = loaded(EXPAND_EAST);
        let render = read(
            r#"{"prose":"Rules\n  r1: when x.\n","rules":[{"pointer":"/declarative/handlers/0","line":1,"line_count":1,"slots":[{"pointer":"/declarative/handlers/0/when","title":"when","line":1,"state":"leaf","predicate":"cmdr_hp_pct","pick_op":"insert"}]}],"_status":{"phase":"lull"}}"#,
        );
        for _ in 0..24 {
            let Some(call) = editor.next_call(false) else {
                break;
            };
            let answer = match call.method {
                "render_plan" => render.clone(),
                "estimate_route" => read(r#"{"reachable":true}"#),
                "resolve_refs" => read(r#"{"refs":[]}"#),
                _ => quick_report("", true),
            };
            let _ = editor.answered(Ok(&answer));
        }
        assert!(!editor.prose_current());
        assert!(matches!(
            editor.unchecked(Check::Prose),
            Some(Unchecked::Unreadable(_))
        ));
        assert_eq!(editor.rules_blocked(), Some(RuleEditRefusal::Stale));
        with_words(&mut editor);
        assert_eq!(
            editor.choose_predicate(0, 0, "/declarative/handlers/0/when", 1, 0),
            Err(RuleEditRefusal::Stale)
        );
        assert!(!editor.has_pending_jobs(), "nothing was sent");
    }

    /// **What else the picker refuses**, each by its reason and never sent: no vocabulary, a
    /// locked entry, a locked family, a slot of another rendering or another pointer, a slot
    /// outside the rule it belongs to.
    #[test]
    fn the_picker_refuses_a_locked_entry_and_a_stale_rendering() {
        let mut editor = rendered(EXPAND_EAST);
        let (slot, rendering, at) = slot_named(&editor, "/declarative/handlers/0/when/all/items/0");
        assert_eq!(
            editor.choose_predicate(slot, rendering, &at, 0, 0),
            Err(RuleEditRefusal::NoVocabulary)
        );
        with_words(&mut editor);
        let words = editor.vocabulary().cloned().expect("the words");
        let clock = words
            .families
            .iter()
            .position(|family| family.id == "clock")
            .expect("clock");
        let locked = words
            .families
            .get(clock)
            .and_then(|family| {
                family
                    .predicates
                    .iter()
                    .position(|entry| entry.locked.is_some())
            })
            .expect("a locked entry");
        assert_eq!(
            editor.choose_predicate(slot, rendering, &at, clock, locked),
            Err(RuleEditRefusal::Locked)
        );
        assert_eq!(editor.status().key, "rule_refused_locked");
        let locked_family = words
            .families
            .iter()
            .position(|family| family.locked.is_some())
            .expect("a locked family");
        assert_eq!(
            editor.choose_predicate(slot, rendering, &at, locked_family, 0),
            Err(RuleEditRefusal::Locked)
        );
        assert_eq!(
            editor.choose_predicate(slot, rendering.wrapping_add(1), &at, clock, 0),
            Err(RuleEditRefusal::Changed)
        );
        assert_eq!(
            editor.choose_predicate(slot, rendering, "/declarative/handlers/1/when", clock, 0),
            Err(RuleEditRefusal::Changed)
        );
        assert_eq!(
            editor.choose_predicate(usize::MAX, rendering, &at, clock, 0),
            Err(RuleEditRefusal::NoSuchSlot)
        );
        if let Some(found) = editor.rule_list.slots.get_mut(slot) {
            found.rule = Some(0);
        }
        assert_eq!(
            editor.choose_predicate(slot, rendering, &at, clock, 0),
            Err(RuleEditRefusal::NotInRule)
        );
        assert!(!editor.has_pending_jobs(), "nothing was sent");
    }

    fn beacon_detail() -> BeaconDetail {
        vocabulary::beacon_detail_of(
            "b_00",
            &read(BEACON).get("result").cloned().expect("a result"),
        )
        .expect("reads")
        .1
    }

    fn setting_named(editor: &Editor, pointer: &str) -> (usize, String) {
        let at = editor
            .beacon_detail()
            .and_then(|detail| {
                detail
                    .settings
                    .iter()
                    .position(|setting| setting.pointer == pointer)
            })
            .expect("the setting");
        (at, pointer.to_owned())
    }

    fn visit_editor() -> Editor {
        let mut editor = rendered(EXPAND_EAST);
        with_words(&mut editor);
        editor.beacon = Some(beacon_detail());
        editor
    }

    /// **Items 2, 3 and 6: the door gates every new control**, with the shut door's own line:
    /// the picker, a name pick, each Visit & change row and the anchor pick.
    #[test]
    fn the_door_gates_every_part_2_control() {
        let mut editor = visit_editor();
        let rule = rule_named(&editor, rule_index(&editor, "/declarative/handlers/0"));
        let setting = setting_named(&editor, "/build/rebuild_destroyed");
        let number = setting_named(&editor, "/retreat_hp_pct");
        assert_eq!(editor.begin_build_target("b_00", 0), Ok(()));
        // The anchor pick's click goes through the door as well, and ends the pick.
        editor.door = Door::Push;
        assert_eq!(
            editor.pick(&Target::Voxel([1, 2, 3])),
            Err(RuleEditRefusal::Shut)
        );
        assert!(!editor.anchoring());
        for door in [Door::Push, Door::Ready, Door::Recap, Door::Ended] {
            // Set as the rig sets it in the Lull's last moment, with no `shut()` between: each
            // control goes through the door itself.
            editor.door = door;
            for got in [
                editor.choose_predicate(0, rule.rendering, "/declarative/route/0/skip_if", 1, 0),
                editor.choose_name(0, rule.rendering, "/x", 0),
                editor.change_setting_choice("b_00", setting.0, &setting.1, 0),
                editor.change_setting_number("b_00", number.0, &number.1, 30),
                editor.switch_mandate("b_00", 0),
                editor.begin_build_target("b_00", 0),
                editor.remove_build_target("b_00", 0),
                editor.queue_structure("b_00", 0),
            ] {
                assert_eq!(got, Err(RuleEditRefusal::Shut), "{door:?}");
                assert_eq!(editor.status().key, format!("closed_{}", door.name()));
            }
            assert!(!editor.has_pending_jobs(), "{door:?}: nothing was sent");
        }
        // Shutting the door the usual way ends an anchor pick, as it ends a chip's.
        let mut editor = visit_editor();
        assert_eq!(editor.begin_build_target("b_00", 0), Ok(()));
        assert!(editor.anchoring());
        editor.set_door(Door::Push);
        assert!(!editor.anchoring());
    }

    /// A text with labelled steps (one label twice, one step with none) and two rules, rendered
    /// with one step-label chip and one rule-id chip.
    fn named_editor() -> Editor {
        let text = r#"{"declarative":{"route":[{"label":"go_1","hold_ms":1},{"hold_ms":2},{"label":"go_2","hold_ms":3},{"label":"go_1","hold_ms":4}],"handlers":[{"id":"r1","when":{"step_reached":{"label":"go_1"}}},{"id":"r2"}]}}"#;
        let mut editor = loaded(text);
        let render = read(
            r#"{"prose":"Rules\n  r1: when step go_1 is reached.\n","rules":[{"pointer":"/declarative/handlers/0","line":1,"line_count":1,"chips":[{"pointer":"/declarative/handlers/0/when/step_reached/label","line":1,"start":16,"end":20,"shape":{"reference":{"kind":"step_label"}},"value_json":"\"go_1\""},{"pointer":"/declarative/handlers/0/id","line":1,"start":2,"end":4,"shape":{"reference":{"kind":"handler_id"}},"value_json":"\"r1\""}]}],"_status":{"phase":"lull"}}"#,
        );
        rerender(&mut editor, &render);
        assert!(editor.prose_current());
        editor
    }

    /// **Item 3: a step label or a rule id is picked from the file's own**, listed in file
    /// order once each, and the pick is one exact `replace`; legality is the gateway's.
    #[test]
    fn a_name_chip_is_picked_from_the_files_own_names() {
        let mut editor = named_editor();
        assert_eq!(editor.name_choices(NameKind::StepLabel), ["go_1", "go_2"]);
        assert_eq!(editor.name_choices(NameKind::RuleId), ["r1", "r2"]);
        let (label, revision, at) = drawn(&editor, "/step_reached/label");
        assert_eq!(
            editor
                .rule_list()
                .chips
                .get(label)
                .map(|chip| chip.editor.clone()),
            Some(ChipEditor::Name(NameKind::StepLabel))
        );
        assert_eq!(
            editor.choose_name(label, revision, &at, 0),
            Err(RuleEditRefusal::Unchanged)
        );
        assert_eq!(
            editor.choose_name(label, revision, &at, 2),
            Err(RuleEditRefusal::NoSuchValue)
        );
        assert_eq!(
            editor.set_number(label, revision, &at, 1),
            Err(RuleEditRefusal::NotThisEditor)
        );
        assert_eq!(editor.choose_name(label, revision, &at, 1), Ok(()));
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(
            patch_of_call(&call),
            r#"[{"op":"replace","path":"/declarative/handlers/0/when/step_reached/label","value":"go_2"}]"#
        );
        // The client judges nothing: an id another rule holds is composed, and the gateway's
        // check is what answers it.
        let mut editor = named_editor();
        let (id, revision, at) = drawn(&editor, "/handlers/0/id");
        assert_eq!(editor.choose_name(id, revision, &at, 1), Ok(()));
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(
            patch_of_call(&call),
            r#"[{"op":"replace","path":"/declarative/handlers/0/id","value":"r2"}]"#
        );
    }

    /// **A value field's text is read as a whole number, or refused by its reason** (S3's
    /// `fixp`, fix 1): an optional sign and digits within an `i64`, nothing else. A number past
    /// 2^53 is exact, because no float is in the path.
    #[test]
    fn a_typed_value_is_a_whole_number_or_refused() {
        assert_eq!(typed_integer("30"), Ok(30));
        assert_eq!(typed_integer("-5"), Ok(-5));
        assert_eq!(typed_integer("+7"), Ok(7));
        assert_eq!(typed_integer("9007199254740993"), Ok(9_007_199_254_740_993));
        assert_eq!(typed_integer("-9223372036854775808"), Ok(i64::MIN));
        for refused in [
            " 3",
            "3 ",
            "3.0",
            "5*2",
            "",
            "-",
            "+",
            "--3",
            "0x10",
            "\u{0663}",
            "99999999999999999999",
            "9223372036854775808",
        ] {
            assert_eq!(
                typed_integer(refused),
                Err(RuleEditRefusal::NotAnInteger),
                "{refused:?}"
            );
        }
    }

    /// **The text goes to its patch exactly, or is refused where the player looks**: on the
    /// status line, on the chip's rule, and to the field's popup.
    #[test]
    fn a_typed_value_reaches_its_patch_or_its_refusal() {
        let mut editor = rendered(EXPAND_EAST);
        let (pct, revision, at) = drawn(&editor, "/cmdr_hp_pct/pct");
        let rule = editor.rule_list().rule_holding(&at);
        assert_eq!(
            editor.set_number_text(pct, revision, &at, "abc"),
            Err(RuleEditRefusal::NotAnInteger)
        );
        assert_eq!(editor.status().key, "rule_refused_not_an_integer");
        assert!(!editor.has_pending_jobs(), "nothing is sent");
        let refused = editor.rule_refusal().expect("on the chip's rule");
        assert_eq!(refused.key, "status_rule_refused_not_an_integer");
        assert_eq!(editor.rule_list().rule_holding(&refused.pointer), rule);
        assert_eq!(
            editor.field_ask(),
            &FieldAsk::Refused {
                key: "status_rule_refused_not_an_integer".to_owned(),
                detail: String::new()
            },
            "the popup hears it while it is open"
        );
        // The value the chip holds now, typed again: Unchanged, on the rule and to the field.
        let now = match editor.rule_list().chips.get(pct).map(|chip| &chip.editor) {
            Some(ChipEditor::Number { now, .. }) => *now,
            other => panic!("not a number chip: {other:?}"),
        };
        assert_eq!(
            editor.set_number_text(pct, revision, &at, &now.to_string()),
            Err(RuleEditRefusal::Unchanged)
        );
        assert_eq!(
            editor.rule_refusal().map(|refusal| refusal.key.as_str()),
            Some("status_rule_refused_unchanged")
        );
        // A new field forgets what the last one heard.
        editor.field_opened();
        assert_eq!(editor.field_ask(), &FieldAsk::Nothing);
        // Past 2^53, exactly; past the gateway's bounds too: the gateway judges it.
        assert_eq!(
            editor.set_number_text(pct, revision, &at, "9007199254740993"),
            Ok(())
        );
        assert_eq!(editor.field_ask(), &FieldAsk::Waiting);
        assert!(
            editor.rule_refusal().is_none(),
            "a sent edit clears the rule's line"
        );
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(
            patch_of_call(&call),
            format!(r#"[{{"op":"replace","path":"{at}","value":9007199254740993}}]"#)
        );
        // The gateway refuses it: the popup and the rule both say so in its words.
        editor
            .answered(Err(("INVALID_ARGUMENT", "out of range")))
            .expect("settled");
        assert_eq!(
            editor.field_ask(),
            &FieldAsk::Refused {
                key: GATEWAY_REFUSED_KEY.to_owned(),
                detail: "INVALID_ARGUMENT: out of range".to_owned()
            }
        );
        assert_eq!(
            editor.rule_refusal().map(|refusal| refusal.key.as_str()),
            Some(GATEWAY_REFUSED_KEY)
        );
        // A value that lands: the field hears it landed.
        assert_eq!(editor.set_number_text(pct, revision, &at, "-5"), Ok(()));
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(
            patch_of_call(&call),
            format!(r#"[{{"op":"replace","path":"{at}","value":-5}}]"#)
        );
        let text = editor.text.clone().expect("a text");
        editor.answered(Ok(&patched(&text, "[]"))).expect("settled");
        assert_eq!(editor.field_ask(), &FieldAsk::Landed);
    }

    /// **A Visit & change number setting reads its text the same way**, and sends a value past
    /// 2^53 exactly.
    #[test]
    fn a_typed_setting_value_is_read_the_same_way() {
        let mut editor = visit_editor();
        let (at, pointer) = setting_named(&editor, "/retreat_hp_pct");
        assert_eq!(
            editor.change_setting_number_text("b_00", at, &pointer, "5*2"),
            Err(RuleEditRefusal::NotAnInteger)
        );
        assert_eq!(editor.status().key, "rule_refused_not_an_integer");
        assert_eq!(editor.field_ask().name(), "refused");
        assert!(!editor.has_pending_jobs());
        assert_eq!(
            editor.change_setting_number_text("b_00", at, &pointer, "9007199254740993"),
            Ok(())
        );
        assert_eq!(editor.field_ask(), &FieldAsk::Waiting);
        assert_eq!(
            visit_patch(&mut editor),
            visit_step_patch(r#"{"set_mandate_settings":{"retreat_hp_pct":9007199254740993}}"#)
        );
        // A shut door drops a field's edit that waited, and the popup says why.
        let mut editor = visit_editor();
        assert_eq!(
            editor.change_setting_number_text("b_00", at, &pointer, "3"),
            Ok(())
        );
        editor.set_door(Door::Push);
        assert_eq!(
            editor.field_ask(),
            &FieldAsk::Refused {
                key: "status_closed_push".to_owned(),
                detail: String::new()
            }
        );
    }

    /// The patch a Visit & change row became, after the editor's next call.
    fn visit_patch(editor: &mut Editor) -> String {
        let call = editor.next_call(false).expect("the patch");
        assert_eq!(call.method, "patch_plan");
        patch_of_call(&call)
    }

    fn entry_named(entries: &[vocabulary::Entry], name: &str) -> usize {
        entries
            .iter()
            .position(|entry| entry.name == name)
            .expect("the entry")
    }

    /// One route step visiting `b_00` with `row`, as the first visit's patch.
    fn visit_step_patch(row: &str) -> String {
        format!(
            r#"[{{"op":"add","path":"/declarative/route/-","value":{{"label":"visit_1","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{row}]}}}}}}]"#
        )
    }

    /// **Item 6: each Visit & change row is one route step with one row**, each exact.
    #[test]
    fn each_visit_and_change_row_is_one_exact_step() {
        let mut editor = visit_editor();
        let (at, pointer) = setting_named(&editor, "/build/rebuild_destroyed");
        assert_eq!(
            editor.change_setting_choice("b_00", at, &pointer, 0),
            Ok(())
        );
        assert_eq!(
            visit_patch(&mut editor),
            visit_step_patch(r#"{"set_mandate_settings":{"build":{"rebuild_destroyed":true}}}"#)
        );
        let mut editor = visit_editor();
        let (at, pointer) = setting_named(&editor, "/retreat_hp_pct");
        assert_eq!(
            editor.change_setting_number("b_00", at, &pointer, 30),
            Ok(())
        );
        assert_eq!(
            visit_patch(&mut editor),
            visit_step_patch(r#"{"set_mandate_settings":{"retreat_hp_pct":30}}"#)
        );
        let mut editor = visit_editor();
        let mandates = editor
            .vocabulary()
            .map(|words| words.mandates.clone())
            .expect("words");
        let defend = entry_named(&mandates, "defend");
        let skeleton = mandates
            .get(defend)
            .map(|entry| compact(&read(&entry.skeleton_json)))
            .expect("one");
        assert_eq!(editor.switch_mandate("b_00", defend), Ok(()));
        assert_eq!(
            visit_patch(&mut editor),
            visit_step_patch(&format!(r#"{{"set_mandate":{{"defend":{skeleton}}}}}"#))
        );
        let mut editor = visit_editor();
        let blueprints = editor
            .vocabulary()
            .map(|words| words.blueprints.clone())
            .expect("words");
        let generator = entry_named(&blueprints, "generator");
        assert_eq!(editor.begin_build_target("b_00", generator), Ok(()));
        assert!(editor.anchoring());
        assert_eq!(editor.picking(), None, "an anchor pick is no chip's");
        assert_eq!(
            editor.pick(&Target::Beacon("b_01".to_owned())),
            Err(RuleEditRefusal::NotThisKind("location"))
        );
        assert!(
            editor.anchoring(),
            "a click on something else keeps the pick on"
        );
        assert_eq!(editor.pick(&Target::Voxel([360, 30, 36])), Ok(()));
        assert!(!editor.anchoring());
        assert_eq!(
            visit_patch(&mut editor),
            visit_step_patch(
                r#"{"add_build_target":{"target":{"blueprint_id":"generator","anchor":{"voxel":{"x":360,"y":30,"z":36}}}}}"#
            )
        );
        let mut editor = visit_editor();
        if let Some(detail) = editor.beacon.as_mut() {
            detail.targets.push(vocabulary::HeldTarget {
                blueprint_id: "generator".to_owned(),
                anchor: read(r#"{"on":{"feature_id":"vent_120_88"}}"#),
                anchor_text: String::new(),
            });
        }
        assert_eq!(editor.remove_build_target("b_00", 0), Ok(()));
        assert_eq!(
            visit_patch(&mut editor),
            visit_step_patch(
                r#"{"remove_build_target":{"anchor":{"on":{"feature_id":"vent_120_88"}}}}"#
            )
        );
        let mut editor = visit_editor();
        let usable = blueprints
            .iter()
            .position(|entry| entry.locked.is_none() && !entry.name.is_empty())
            .expect("a usable blueprint");
        let name = blueprints
            .get(usable)
            .map(|entry| entry.name.clone())
            .expect("a name");
        assert_eq!(editor.queue_structure("b_00", usable), Ok(()));
        assert_eq!(
            visit_patch(&mut editor),
            visit_step_patch(&format!(
                r#"{{"queue_structure":{{"blueprint_id":"{name}"}}}}"#
            ))
        );
    }

    /// **Item 6: what a Visit & change row refuses**: a locked blueprint, another beacon's
    /// detail, no detail, a setting named by a stale pointer; and a gateway refusal of the patch is said in
    /// the gateway's words.
    #[test]
    fn a_visit_and_change_row_refuses_by_its_reason() {
        let mut editor = visit_editor();
        let blueprints = editor
            .vocabulary()
            .map(|words| words.blueprints.clone())
            .expect("words");
        let locked = blueprints
            .iter()
            .position(|entry| entry.locked.is_some())
            .expect("a locked blueprint");
        assert_eq!(
            editor.queue_structure("b_00", locked),
            Err(RuleEditRefusal::Locked)
        );
        assert_eq!(
            editor.begin_build_target("b_00", locked),
            Err(RuleEditRefusal::Locked)
        );
        assert_eq!(
            editor.switch_mandate("b_01", 0),
            Err(RuleEditRefusal::NoDetail)
        );
        let (at, pointer) = setting_named(&editor, "/build/rebuild_destroyed");
        assert_eq!(
            editor.change_setting_choice("b_00", at, "/roe", 0),
            Err(RuleEditRefusal::Changed)
        );
        assert_eq!(
            editor.change_setting_number("b_00", at, &pointer, 1),
            Err(RuleEditRefusal::NotThisEditor)
        );
        assert!(!editor.has_pending_jobs(), "nothing was sent");
        editor.beacon = None;
        assert_eq!(
            editor.switch_mandate("b_00", 0),
            Err(RuleEditRefusal::NoDetail)
        );
        let mut editor = visit_editor();
        assert_eq!(editor.switch_mandate("b_00", 0), Ok(()));
        let _ = editor.next_call(false).expect("the patch");
        let settled = editor
            .answered(Err(("INVALID_ARGUMENT", "the patch does not apply")))
            .expect("reads");
        assert_eq!(settled, Settled::Refused);
        assert_eq!(editor.status().key, "gateway_refused");
        assert_eq!(
            editor.status().detail,
            "INVALID_ARGUMENT: the patch does not apply"
        );
    }

    // --- The review of the pull request ---------------------------------------------

    /// **a Visit row may set a setting to the value the Lull sees**: an earlier step may
    /// have changed it, so the client composes the row and the verifier says what it does.
    #[test]
    fn a_visit_row_may_set_the_value_the_lull_sees() {
        let mut editor = visit_editor();
        let (at, pointer) = setting_named(&editor, "/build/rebuild_destroyed");
        assert_eq!(
            editor.change_setting_choice("b_00", at, &pointer, 1),
            Ok(())
        );
        assert_eq!(
            visit_patch(&mut editor),
            visit_step_patch(r#"{"set_mandate_settings":{"build":{"rebuild_destroyed":false}}}"#)
        );
        let mut editor = visit_editor();
        let (at, pointer) = setting_named(&editor, "/retreat_hp_pct");
        assert_eq!(
            editor.change_setting_number("b_00", at, &pointer, 0),
            Ok(())
        );
        assert_eq!(
            visit_patch(&mut editor),
            visit_step_patch(r#"{"set_mandate_settings":{"retreat_hp_pct":0}}"#)
        );
    }

    /// **an entry the answer names no word for is refused as such**, not as the gateway's
    /// lock.
    #[test]
    fn a_nameless_entry_with_no_lock_is_refused_as_unnamed() {
        let mut editor = visit_editor();
        if let Some(words) = editor.vocabulary.as_mut() {
            words.blueprints.push(vocabulary::Entry {
                name: String::new(),
                title: "Nameless".to_owned(),
                parameters: Vec::new(),
                skeleton_json: String::new(),
                locked: None,
            });
        }
        let nameless = editor
            .vocabulary()
            .map(|words| words.blueprints.len().saturating_sub(1))
            .expect("words");
        assert_eq!(
            editor.begin_build_target("b_00", nameless),
            Err(RuleEditRefusal::Unnamed)
        );
        assert_eq!(editor.status().key, "rule_refused_unnamed");
        assert_eq!(
            editor.queue_structure("b_00", nameless),
            Err(RuleEditRefusal::Unnamed)
        );
    }

    /// **a Build target anchors on a feature as well as on ground**: a vent click names
    /// the feature under `on`.
    #[test]
    fn a_build_target_anchors_on_a_feature() {
        let mut editor = visit_editor();
        let blueprints = editor
            .vocabulary()
            .map(|words| words.blueprints.clone())
            .expect("words");
        let generator = entry_named(&blueprints, "generator");
        assert_eq!(editor.begin_build_target("b_00", generator), Ok(()));
        assert_eq!(
            editor.pick(&Target::Feature("vent_120_88".to_owned())),
            Ok(())
        );
        assert_eq!(
            visit_patch(&mut editor),
            visit_step_patch(
                r#"{"add_build_target":{"target":{"blueprint_id":"generator","anchor":{"on":{"feature_id":"vent_120_88"}}}}}"#
            )
        );
    }

    /// **No number is refused for its size** (S3's `fixp`): with no float in the path, a value
    /// past `i32` and past 2^53 is sent exactly, and the gateway's bounds are its to refuse.
    #[test]
    fn no_number_is_refused_for_its_size() {
        for value in [
            2_147_483_648,
            9_007_199_254_740_993,
            -9_007_199_254_740_993,
            i64::MAX,
        ] {
            let mut editor = rendered(EXPAND_EAST);
            let (pct, revision, at) = drawn(&editor, "/cmdr_hp_pct/pct");
            assert_eq!(
                editor.set_number(pct, revision, &at, value),
                Ok(()),
                "{value}"
            );
        }
    }

    /// **a setting this client cannot read keeps the rest**: the prose and the other
    /// settings are drawn, and that setting is a read-only row with its reason.
    #[test]
    fn an_unreadable_setting_keeps_the_prose_and_the_rest() {
        let mut editor = loaded(EXPAND_EAST);
        drain_checks(&mut editor);
        editor.describe_beacon("b_00");
        let _ = editor.next_call(false).expect("get_beacon");
        let answer = read(
            r#"{"prose":"Yours.","mandate":"build","settings":[{"pointer":"/retreat_hp_pct","title":"Retreat below HP","value_json":"\"many\"","shape":{"integer":{"min":0,"max":100,"unit":"%"}}},{"pointer":"/roe","title":"Rules of engagement","value_json":"\"RETURN_FIRE\"","shape":{"choice":{"values":["\"HOLD_FIRE\"","\"RETURN_FIRE\""]}}}]}"#,
        );
        editor.answered(Ok(&answer)).expect("reads");
        assert_eq!(editor.beacon_prose(), "Yours.");
        let detail = editor.beacon_detail().expect("the detail");
        assert_eq!(
            detail
                .settings
                .first()
                .map(|setting| setting.editor.clone()),
            Some(ChipEditor::ReadOnly(rule_list::ReadOnly::Unreadable))
        );
        assert!(matches!(
            detail.settings.get(1).map(|setting| &setting.editor),
            Some(ChipEditor::Choice { .. })
        ));
        // The cause is kept, in the decode's words, for the row to say.
        let why = detail
            .settings
            .first()
            .and_then(|setting| setting.unreadable.clone())
            .expect("the cause");
        assert!(
            why.contains("/retreat_hp_pct") && why.contains("many"),
            "{why}"
        );
        assert_eq!(
            detail
                .settings
                .get(1)
                .and_then(|setting| setting.unreadable.clone()),
            None
        );
    }

    /// **a refused vocabulary can be asked again**, once more, within the seat's budget.
    #[test]
    fn a_refused_vocabulary_can_be_asked_again() {
        let mut editor = Editor::new("seat.0");
        editor.lull_opened(1);
        let refuse = |editor: &mut Editor| {
            let _ = editor.answered(Err(("RATE_LIMITED", "slow down")));
        };
        assert_eq!(lull_reads(&mut editor, &refuse), 1);
        assert!(editor.vocabulary_failed().is_some());
        assert!(editor.ask_vocabulary_again());
        assert!(
            !editor.ask_vocabulary_again(),
            "one ask on its way is enough"
        );
        assert!(
            editor.vocabulary_failed().is_none(),
            "the line says it is asked"
        );
        let answer = |editor: &mut Editor| {
            let words = read(VOCABULARY).get("result").cloned().expect("a result");
            editor.answered(Ok(&words)).expect("reads");
        };
        assert_eq!(lull_reads(&mut editor, &answer), 1);
        assert!(editor.vocabulary().is_some());
    }

    /// **The Quartermaster priority's value now is the detail's**, as `get_beacon`'s summary
    /// sends it.
    #[test]
    fn the_detail_carries_the_priority_as_sent() {
        assert_eq!(beacon_detail().priority, "normal");
    }
}
