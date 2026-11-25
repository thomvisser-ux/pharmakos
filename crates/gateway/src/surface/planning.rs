// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Docs, planning and commit: everything a seat does *to* a playbook, and the
//! one act it cannot take back.
//!
//! All of it is `plan-core`'s and the verifier's work, marshalled. The gateway
//! decides who may call, which snapshot the work is against and what comes
//! back; it decides nothing about a playbook, which is the same rule the editor
//! is held to one layer further out (AGENTS.md section 3 rule 4, "it asks the
//! gateway").
//!
//! # Nothing here steps the sim
//!
//! `verify_plan`, `render_plan`, `patch_plan` and `instantiate_template` are
//! the four methods a "no dry runs" violation would most naturally hide in --
//! each of them is a question about what *would* happen. None of them has a
//! [`pharmakos_sim::runner::Runner`] in its hand: they read the frozen
//! snapshot's bytes and the rules table, and that is all
//! (AGENTS.md section 3 rule 2; `tests/confinement.rs`).
//!
//! S3's `fixq` adds two more of the same kind, text in and text out:
//! `set_value_text` reads a typed value in its chip's display unit and writes
//! it, and `new_rule` adds an empty rule. Each renders or decodes the file
//! against the frozen snapshot and the rules table and patches it through
//! `patch_plan`'s own door; neither holds a runner.
//!
//! `resolve_refs`, S1's fifth, is the one that ranks: "nearest" is ranked where
//! the pathing graph is, and for a preview that is here. It reads the frozen
//! world through the sim's own resolver, borrowed read-only
//! ([`crate::targeting`]), and estimates travel over terrain the snapshot
//! already holds -- the estimate `estimate_route` answers with -- so it is a
//! preview and not a dry run: nothing moves, no mandate or program runs, and no
//! enemy is modelled.
//!
//! # `submit_plan` always runs FULL, and an invalid playbook is not an error
//!
//! Spec section 12 and decisions-log item 82: `verify_plan{depth}` and
//! `report_hash` ship complete, and submit runs FULL -- since S1 a FULL that
//! runs its estimate stage (`E0601`, the `W06xx` and `W07xx` warnings and
//! `I0001`, the `proj` lane's) and its lint stage (targeting's lints, the
//! `tgtv` lane's) as well as QUICK's. So a FULL pre-check and the check at submit
//! produce the **same `report_hash`** -- which is what the walkthrough asserts
//! and what makes the seal inspectable rather than a second opinion.
//!
//! An invalid playbook comes back as a full report with `qualifies: false` and
//! `accepted: false`, never as a method error: "a gateway error means the call
//! could not be made; a report means the call was made and the answer is no"
//! ([`crate::error`]).
//!
//! # There are two doors, and `submit_plan` opens both of them here
//!
//! The verifier answers *"may this be sealed"*; the sim's
//! [`pharmakos_sim::interpreter::Plan::compile`] answers *"can this build
//! execute it"*, and the two are deliberately not the same check (T11). A
//! playbook can pass the first and fail the second — a construct in the v1
//! vocabulary whose effect waits for a later stage, an enum value newer than
//! this build, a voxel the verifier's resolve stage does not yet bound.
//!
//! [`compile_canonical`] is where the second door is (its one `Plan::compile`
//! call is [`compile_decoded`], which `resolve_refs` shares, so a preview and
//! a seal go through the same door), and it is **at submit**
//! rather than at `begin_push` (decisions-log item 103 (1)): a refusal
//! discovered when the Lull ends is a refusal nobody is listening for, and the
//! seat would find out by watching a commander stand still. So the compile
//! happens while the caller is still on the line, its refusal is a method error
//! naming the construct, and the seat's previous seal is left exactly where it
//! was.
//!
//! Compiling is **not a dry run.** `Plan::compile` is a pure function of the
//! playbook and the rules table: it resolves labels, checks the two halting
//! properties and prices nothing against the world. It has no [`Runner`] and no
//! `World` in its hand, it steps nothing, and `tests/confinement.rs` asserts
//! that this module names no stepping call at all.
//!
//! [`Runner`]: pharmakos_sim::runner::Runner

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json::Json;
use pharmakos_proto::json::grammar::Said;
use pharmakos_proto::said;
use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::rules::RulesTable;

use crate::error::Error;
use crate::rpc::Request;
use crate::surface::{Draft, Surface};

/// The longest label a seat may hang on a draft.
///
/// PLACEHOLDER: a draft label's 120 characters — owner, at S6, with the editor.
/// 120 is a working number, chosen so a label fits one line of the editor's draft list.
/// It is a **character** count and not a byte count, for the same reason the
/// notebook's is (spec section 12 says 4,000 characters).
pub const MAX_DRAFT_LABEL_CHARS: usize = 120;

/// The most drafts one seat may hold at once.
///
/// PLACEHOLDER: 32 drafts a seat — owner, at hardening, with the rate limits.
/// 32 is a working number. It exists because a draft is stored in
/// the private match cache and an unbounded store is a way for one
/// authenticated seat to fill a disk (AGENTS.md section 7's audit-log argument,
/// applied to the thing beside it). OWNER settles it at hardening with the rate
/// limits.
pub const MAX_DRAFTS: usize = 32;

/// The most drafts one seat holds in all: [`MAX_DRAFTS`] of its own and the
/// one carried draft ([`crate::surface::CARRIED_DRAFT_ID`]), the draft rule's
/// whole ([`drafts_hold`]). What a save's read refuses past before it reads a
/// draft (`crate::save`).
pub const MAX_HELD_DRAFTS: usize = MAX_DRAFTS + 1;

/// How many numbered ids `save_draft` tries when it names a draft
/// (`d<round>-<n>`): one more than a seat's own drafts can hold, so one of
/// them is always free.
const DRAFT_NUMBERS: usize = MAX_DRAFTS + 1;

/// Why a seat's drafts break the draft rule ([`drafts_hold`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DraftsRefused {
    /// More than [`MAX_DRAFTS`] of the seat's own.
    TooMany {
        /// How many of its own it holds.
        own: usize,
    },
    /// The gateway's carried draft more than once.
    CarriedTwice,
    /// One id on two drafts.
    IdTwice {
        /// The id.
        id: String,
    },
}

impl DraftsRefused {
    /// The rule broken, in words.
    #[must_use]
    pub fn said(&self) -> Said {
        match self {
            DraftsRefused::TooMany { own } => said!(
                "it holds {} drafts of its own, past the {} a seat may beside the carried one",
                own,
                MAX_DRAFTS
            ),
            DraftsRefused::CarriedTwice => said!(
                "it holds the carried draft `{}` twice",
                crate::surface::CARRIED_DRAFT_ID
            ),
            DraftsRefused::IdTwice { id } => said!(
                "it holds two drafts named `{}`",
                pharmakos_proto::json::grammar::quoted(id)
            ),
        }
    }
}

/// How many of `drafts` are the seat's own: every draft but the gateway's
/// carried one ([`crate::surface::CARRIED_DRAFT_ID`]), which the gateway
/// writes, `save_draft` cannot, and no count of the seat's holds.
#[must_use]
pub fn own_drafts(drafts: &[Draft]) -> usize {
    drafts
        .iter()
        .filter(|draft| draft.draft_id != crate::surface::CARRIED_DRAFT_ID)
        .count()
}

/// The draft rule, stated once: a seat holds at most [`MAX_DRAFTS`] drafts of
/// its own **plus** the one carried draft, and no id twice. Held by
/// `save_draft` (which can neither write nor count the carried id), draft
/// continuity (which replaces the carried draft), a save's read
/// (`crate::save`) and the resume's restore.
///
/// # Errors
///
/// [`DraftsRefused`], naming the rule broken.
pub fn drafts_hold(drafts: &[Draft]) -> Result<(), DraftsRefused> {
    let own = own_drafts(drafts);
    if own > MAX_DRAFTS {
        return Err(DraftsRefused::TooMany { own });
    }
    let mut ids: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for draft in drafts {
        if !ids.insert(draft.draft_id.as_str()) {
            return Err(if draft.draft_id == crate::surface::CARRIED_DRAFT_ID {
                DraftsRefused::CarriedTwice
            } else {
                DraftsRefused::IdTwice {
                    id: draft.draft_id.clone(),
                }
            });
        }
    }
    Ok(())
}

/// The longest playbook text a method will take, in characters: the
/// verifier's [`pharmakos_verifier::MAX_PLAYBOOK_CHARS`], re-exported here, where it
/// was defined until S3's `vfy6`, so this path still names it (the walled
/// `pharmakos-bench` reads it here). The number has one definition.
pub use pharmakos_verifier::MAX_PLAYBOOK_CHARS;

impl Surface {
    /// `get_schema`: the playbook vocabulary, generated from the schema.
    ///
    /// Served from [`crate::schema`], which walks the checked-in descriptor
    /// set. Nothing is written down twice, which is the whole of what "so they
    /// can't drift" buys (spec section 12).
    pub(super) fn get_schema(request: &Request) -> Result<Json, Error> {
        let part = request.string_param("part")?.unwrap_or_default();
        Ok(Json::Object(vec![(
            String::from("json_schema"),
            Json::String(crate::schema::text(part)?),
        )]))
    }

    /// `get_vocabulary`: every condition family, predicate, mandate, route step
    /// and blueprint a playbook may name, each with its parameters and, where
    /// the calling seat cannot use it yet, why (S3's plan, E8; spec section
    /// 13). The content is plan-core's ([`pharmakos_plan_core::vocabulary`]),
    /// built from the schema, the verifier's refusal table and the rules
    /// table; the one thing this handler adds is the seat's own licences, read
    /// off the hosted world's licence table, which is what locks or opens a
    /// licensed blueprint. That read is about the calling seat alone, as
    /// `SCOPE_DOCS`' comment allows. A caller that is no seat -- the lobby, a
    /// spectator -- holds no licence, so it is answered every licensed entry
    /// locked, which is true of it. It takes no parameters.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the licence table holds a licence
    /// this build does not define or the vocabulary disagrees with the schema.
    pub(super) fn get_vocabulary(&self, subject: crate::token::Subject) -> Result<Json, Error> {
        let held: Vec<pharmakos_proto::gp::v1::Licence> = match subject.seat() {
            Some(seat) => self
                .host()?
                .world()
                .licences()
                .rows()
                .iter()
                .filter(|row| row.seat == seat.raw())
                .map(|row| {
                    pharmakos_sim::licence::licence_of(row.licence).ok_or_else(|| {
                        Error::internal(said!(
                            "the licence table holds licence {} for seat {}, which this build \
                             does not define",
                            row.licence,
                            row.seat
                        ))
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?,
            None => Vec::new(),
        };
        let words = pharmakos_plan_core::vocabulary::vocabulary(&held).map_err(|error| {
            Error::internal(said!(
                "the vocabulary: {}",
                pharmakos_proto::json::grammar::Said::engine_error(&error)
            ))
        })?;
        let entries = |entries: &[pharmakos_plan_core::vocabulary::Entry]| {
            Json::Array(entries.iter().map(vocabulary_entry).collect())
        };
        Ok(Json::Object(vec![
            (
                String::from("families"),
                Json::Array(
                    words
                        .families
                        .iter()
                        .map(|family| {
                            let mut fields = vec![
                                (String::from("family"), Json::String(family.id.to_owned())),
                                (String::from("title"), Json::String(family.title.to_owned())),
                                (String::from("predicates"), entries(&family.predicates)),
                            ];
                            if let Some(locked) = &family.locked {
                                fields.push((String::from("locked"), Json::String(locked.clone())));
                            }
                            Json::Object(fields)
                        })
                        .collect(),
                ),
            ),
            (String::from("mandates"), entries(&words.mandates)),
            (String::from("steps"), entries(&words.steps)),
            (String::from("blueprints"), entries(&words.blueprints)),
            (
                String::from("queue_blueprints"),
                entries(&words.queue_blueprints),
            ),
            (String::from("groups"), entries(&words.groups)),
        ]))
    }

    /// `list_templates`: the local library, read and never stored.
    ///
    /// Spec section 13: "The gateway reads that folder to list and instantiate
    /// templates, but stores none of it". Every call re-reads the folder, which
    /// is what "stores none of it" means in code, and nothing in it executes.
    ///
    /// A host with **no** library configured lists nothing rather than failing:
    /// a wizard that could not open because a path was unset would be worse
    /// than one with no templates in it. The shipped host is configured with
    /// the zip's `library/`, beside `gamectl` (decisions-log item 117 (3)),
    /// whose paths the zip's manifest golden pins. So from a packaged build a
    /// missing folder is an INTERNAL error, and an empty list means its
    /// templates were removed or no longer parse.
    pub(super) fn list_templates(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let tag = request.string_param("tag")?.unwrap_or_default();
        let guided = self.guided_pick(subject)?;
        let Some(folder) = self.host()?.library() else {
            return Ok(Json::Object(vec![
                (String::from("templates"), Json::Array(Vec::new())),
                (String::from("next_cursor"), Json::String(String::new())),
            ]));
        };
        let found = pharmakos_plan_core::library::list(folder).map_err(|error| {
            Error::internal(said!(
                "the template folder could not be read: {}",
                error.said()
            ))
        })?;
        let templates: Vec<Json> = found
            .iter()
            // A worked playbook in the library is a sample and not a template,
            // and the wizard does not offer it (`plan-core`'s `Summary::kind`).
            .filter(|summary| summary.kind == pharmakos_proto::gp::v1::playbook::Kind::Template)
            .filter(|summary| tag.is_empty() || summary.tags.iter().any(|held| held == tag))
            .map(|summary| {
                let mut entries = vec![
                    (
                        String::from("template_id"),
                        Json::String(summary.template_id.clone()),
                    ),
                    (String::from("title"), Json::String(summary.title.clone())),
                    (
                        String::from("tags"),
                        Json::Array(
                            summary
                                .tags
                                .iter()
                                .map(|tag| Json::String(tag.clone()))
                                .collect(),
                        ),
                    ),
                    (String::from("summary"), Json::String(summary.blurb.clone())),
                ];
                // E25: the template the advisor says to start with, with its
                // reason, and on no other entry (proto3's `false` and empty
                // string are left out).
                if let Some((template, reason)) = guided
                    && *template == summary.template_id
                {
                    entries.push((String::from("guided_pick"), Json::Bool(true)));
                    entries.push((
                        String::from("guided_pick_reason"),
                        Json::String(reason.to_owned()),
                    ));
                }
                Json::Object(entries)
            })
            .collect();
        Ok(Json::Object(vec![
            (String::from("templates"), Json::Array(templates)),
            // The listing is complete: a folder of templates is a folder, and
            // paging one would be pagination for its own sake.
            (String::from("next_cursor"), Json::String(String::new())),
        ]))
    }

    /// The guided first pick for `subject`'s template list: the template its
    /// advisor said to start with and the advisor's sentence for it, in a Probation
    /// match's first Lull, for the human seat it was filed for (S3's plan,
    /// decision 5 and E25). `None` anywhere else: a `custom` match, a later
    /// round, outside a Lull, a caller that is not that seat, or advice that
    /// names none. Read from the seat's own store, so no seat is told
    /// another's.
    ///
    /// # Errors
    ///
    /// As [`Surface::host`].
    fn guided_pick(&self, subject: crate::token::Subject) -> Result<Option<(&str, &str)>, Error> {
        let runner = self.host()?.runner();
        let first_lull =
            runner.round() == 1 && runner.phase() == pharmakos_sim::runner::MatchPhase::Lull;
        if self.preset() != crate::probation::Preset::Probation || !first_lull {
            return Ok(None);
        }
        let Some(seat) = subject
            .seat()
            .filter(|seat| Some(*seat) == self.human_seat())
        else {
            return Ok(None);
        };
        Ok(self.current_advice(seat).and_then(|advised| {
            let start = advised.advice.start_here.as_ref()?;
            Some((start.template_id.as_str(), start.why.as_str()))
        }))
    }

    /// `instantiate_template`: a template plus parameters, as a playbook, and
    /// the list of what was filled in.
    ///
    /// The template id is decided on **characters** and never on
    /// `std::path::Component` (`plan-core`'s `library::resolve`, and the lesson
    /// decisions-log item 100's closing note records): a backslash is a
    /// separator on Windows and an ordinary filename byte elsewhere, and the
    /// same hostile id must get the same answer on all three platforms.
    ///
    /// # `suggested`
    ///
    /// Decisions-log item 111, decision C2. With `suggested: true`, every
    /// parameter the template declares and the caller did not name is
    /// pre-filled with the built-in operator's suggestion **for the calling
    /// seat**, where it made one for this template this round, and `why`
    /// carries its reason. The suggestion is read from the seat's own private
    /// store through [`Surface::current_advice`], and a subject that is not a
    /// seat has none, so a seat is only ever told its own
    /// (`advice_for_one_seat_never_reaches_another`). An explicit value beats
    /// a suggestion, always.
    ///
    /// The reply lists every **declared** parameter, in declaration order,
    /// with the value applied and whether it was the suggestion
    /// (`gp.api.v1.FilledParameter`). The arithmetic is `plan-core`'s; this
    /// handler decides only whose suggestion it may use.
    pub(super) fn instantiate_template(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let template_id = request
            .string_param("template_id")?
            .ok_or_else(|| Error::invalid("`template_id` names a template"))?;
        if !is_template_id(template_id) {
            return Err(Error::invalid(
                "`template_id` is a plain file stem: no separators, no control characters, no \
                 percent escapes",
            ));
        }
        let suggested = request.bool_param("suggested")?.unwrap_or(false);
        let folder = self.host()?.library().ok_or_else(|| {
            Error::not_found(
                "this gateway has no template folder, so there is nothing to \
                              instantiate",
            )
        })?;
        // The failure names the **template**, never the path it tried: where
        // the library lives is the host's business, and a refusal that spelled
        // out an absolute directory would hand every seat the layout of the
        // machine it is playing on for the price of one bad id.
        let text = pharmakos_plan_core::library::read(folder, template_id)
            .map_err(|refused| library_refusal(template_id, &refused))?;

        let mut parameters: Vec<pharmakos_plan_core::library::Parameter> = Vec::new();
        if let Some(value) = request.param("parameters") {
            let Json::Array(items) = value else {
                return Err(Error::invalid("`parameters` is an array of {name, value}"));
            };
            for (index, item) in items.iter().enumerate() {
                let name = match item.get("name") {
                    Some(Json::String(name)) => name.clone(),
                    _ => {
                        return Err(Error::invalid(said!("parameter {} has no `name`", index)));
                    }
                };
                let value = match item.get("value") {
                    Some(Json::String(value)) => value.clone(),
                    _ => {
                        return Err(Error::invalid(said!("parameter {} has no `value`", index)));
                    }
                };
                parameters.push(pharmakos_plan_core::library::Parameter { name, value });
            }
        }

        // Whose suggestion: the calling seat's own, for this template, this
        // round, and only when asked for.
        let suggestion = if suggested {
            subject
                .seat()
                .and_then(|seat| self.current_advice(seat))
                .and_then(|advised| advised.advice.suggestion(template_id))
        } else {
            None
        };
        let offered: Vec<pharmakos_plan_core::library::Parameter> =
            suggestion.map_or_else(Vec::new, |suggestion| {
                suggestion
                    .parameters
                    .iter()
                    .map(|value| pharmakos_plan_core::library::Parameter {
                        name: value.pointer.clone(),
                        value: value.value.clone(),
                    })
                    .collect()
            });
        let why = suggestion.map_or_else(String::new, |suggestion| suggestion.why.clone());

        let made = pharmakos_plan_core::instantiate(&text, &parameters, &offered)
            .map_err(|error| Error::invalid(error.shown()))?;
        let filled: Vec<Json> = made
            .parameters
            .iter()
            .map(|parameter| {
                Json::Object(vec![
                    (
                        String::from("pointer"),
                        Json::String(parameter.pointer.clone()),
                    ),
                    (String::from("label"), Json::String(parameter.label.clone())),
                    (String::from("value"), Json::String(parameter.value.clone())),
                    (String::from("suggested"), Json::Bool(parameter.suggested)),
                ])
            })
            .collect();
        Ok(Json::Object(vec![
            (
                String::from("playbook_jsonc"),
                Json::String(made.playbook_jsonc),
            ),
            (String::from("parameters"), Json::Array(filled)),
            (String::from("why"), Json::String(why)),
        ]))
    }

    /// `verify_plan{depth}`: seal inspection, at the depth the caller asks for.
    ///
    /// The depth travels as a lower-case string (`"quick"`, `"full"`) --
    /// decisions-log item 80, and spec section 12's own worked example -- and
    /// defaults to FULL when it is absent, which is what `gateway.proto` says
    /// and what `submit_plan` always runs.
    pub(super) fn verify_plan(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "a playbook to verify")?;
        let playbook = playbook_param(request)?;
        let depth = depth_of(request)?;
        let report = self.verify_for(seat, playbook, depth)?;
        Ok(Json::Object(vec![(
            String::from("report"),
            report_json(&report),
        )]))
    }

    /// `render_plan`: the playbook as English prose.
    ///
    /// Deterministic template prose, and it reads the coming segment's length
    /// from the frozen snapshot rather than from a constant (spec section 13,
    /// and `plan-core`'s `context::segment_length_ms`, whose PLACEHOLDER this
    /// lane discharged).
    ///
    /// From S3, `rules` beside it (S3's plan, E8): each route step and rule,
    /// in file order, with the lines of the prose that are its own and its
    /// chips, each an editable value's pointer, its span on its line, its
    /// shape and its value now, all of plan-core's
    /// ([`pharmakos_plan_core::render`]), so the editor draws them and parses
    /// no sentence (AGENTS.md section 3 rule 4). Left out, as proto3 leaves an
    /// empty list, when the file has no route step and no rule, or no
    /// canonical form.
    pub(super) fn render_plan(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "a playbook to render")?;
        let playbook = playbook_param(request)?;
        let host = self.host()?;
        let context = pharmakos_plan_core::PlanContext::from_snapshot(
            host.runner().frozen().snapshot(),
            seat.raw(),
        )
        .map_err(|error| Error::internal(error.shown()))?;

        // A file with no canonical form has no rendering. That is not an error
        // -- the editor asks for a rendering on every keystroke and a half-typed
        // file is the normal case -- so it comes back as the sentence that says
        // to verify it, with no rules, and `verify_plan` is where the
        // diagnostics live.
        //
        // The prose and the rules' chips read the decoded playbook and nothing
        // else (`render` takes a `Playbook`; a chip is a pointer into the
        // canonical tree, not into the file's comments), so the read is the
        // one decode `verify_plan` takes its report over, `decode_jsonc`, and
        // no JSONC tree is built (S3's `vfyq2`, fix pass 1, item 3).
        let rendering = match pharmakos_plan_core::decode_jsonc(playbook) {
            Err(_no_canonical_form) => pharmakos_plan_core::Rendering {
                prose: String::from(crate::strings::UNRENDERABLE),
                rules: Vec::new(),
            },
            Ok(canonical) => {
                pharmakos_plan_core::render(canonical.decoded().message(), &context, host.rules())
                    .map_err(|error| Error::internal(error.shown()))?
            }
        };
        let mut entries = vec![(String::from("prose"), Json::String(rendering.prose))];
        if !rendering.rules.is_empty() {
            entries.push((
                String::from("rules"),
                Json::Array(rendering.rules.iter().map(rendered_rule).collect()),
            ));
        }
        Ok(Json::Object(entries))
    }

    /// `patch_plan`: apply a JSON Patch, comments and formatting intact: RFC
    /// 6902's six operations read more strictly (a member an operation does
    /// not have is refused), or an undo this method returned (`add`, `remove`
    /// and `replace`, each with its `undo` member; `pharmakos_plan_core::patch`).
    ///
    /// Returns the inverse patch as well, which is the editor's undo stack
    /// (spec section 13, "Edits are JSON Patches with inverse-patch undo").
    /// Takes no seat: a patch is text in and text out and touches no snapshot,
    /// so there is nothing seat-shaped to check beyond the scope the method
    /// already carries.
    pub(super) fn patch_plan(request: &Request) -> Result<Json, Error> {
        let playbook = playbook_param(request)?;
        let patch = request.string_param("json_patch")?.ok_or_else(|| {
            Error::invalid(said!(
                "`json_patch` is a JSON Patch, as text: RFC 6902's operations with no member \
                     an operation does not have, or an undo `patch_plan` returned"
            ))
        })?;
        let (patched, inverse) = pharmakos_plan_core::patch_text(playbook, patch)
            .map_err(|error| Error::invalid(error.shown()))?;
        undo_travels(&patched, &inverse)?;
        Ok(Json::Object(vec![
            (String::from("playbook_jsonc"), Json::String(patched)),
            (String::from("inverse_json_patch"), Json::String(inverse)),
        ]))
    }

    /// `set_value_text`: a value as the player typed it, in its shape's
    /// display unit, written at a chip's pointer and answered as `patch_plan`
    /// answers (S3's `fixq`, item 7). The reading, the conversion from
    /// seconds and the patch are plan-core's
    /// ([`pharmakos_plan_core::edit::set_value_text`]); the shape it reads is
    /// the one `render_plan` answers for the same file, so the frozen snapshot
    /// is read as `render_plan` reads it, and nothing is stepped.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::PhaseClosed`] outside a Lull, before anything is
    /// parsed; [`crate::error::Code::InvalidArgument`] for every other refusal,
    /// in plan-core's words with the typed text quoted.
    pub(super) fn set_value_text(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        // Its own gate, before anything is parsed, beside the dispatcher's
        // planning door: the shape it reads is the frozen snapshot's.
        if !self.time.planning_open() {
            return Err(Error::phase_closed(said!(
                "`set_value_text` reads the frozen planning snapshot, open only in a Lull: ask in a Lull"
            )));
        }
        let seat = Surface::seat_of(subject, "a value to type")?;
        let playbook = playbook_param(request)?;
        let pointer = request
            .string_param("pointer")?
            .ok_or_else(|| Error::invalid("`pointer` names the value: a chip's pointer"))?;
        let typed = request
            .string_param("typed")?
            .ok_or_else(|| Error::invalid("`typed` is the value as the player typed it"))?;
        let host = self.host()?;
        let context = pharmakos_plan_core::PlanContext::from_snapshot(
            host.runner().frozen().snapshot(),
            seat.raw(),
        )
        .map_err(|error| Error::internal(error.shown()))?;
        let (patched, inverse) = pharmakos_plan_core::edit::set_value_text(
            playbook,
            pointer,
            typed,
            &context,
            host.rules(),
        )
        .map_err(|error| {
            Error::invalid(pharmakos_plan_core::edit::set_value_words(&error, typed))
        })?;
        undo_travels(&patched, &inverse)?;
        Ok(Json::Object(vec![
            (String::from("playbook_jsonc"), Json::String(patched)),
            (String::from("inverse_json_patch"), Json::String(inverse)),
        ]))
    }

    /// `new_rule`: an empty rule added at the end of the playbook's rules,
    /// with a fresh id, answered as `patch_plan` answers, and the new rule's
    /// pointer and id beside it (S3's `fixq`, item 8). The rule is
    /// plan-core's ([`pharmakos_plan_core::edit::new_rule`]), its cooldown the
    /// rules table's least; nothing is read from the world and nothing is
    /// stepped.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::PhaseClosed`] outside a Lull, before anything is
    /// parsed; [`crate::error::Code::InvalidArgument`] for a file with no canonical form
    /// and for one already holding the most rules a playbook may, in
    /// plan-core's words.
    pub(super) fn new_rule(&self, request: &Request) -> Result<Json, Error> {
        // Its own gate, before anything is parsed, beside the dispatcher's
        // planning door: a planning edit, open only in a Lull.
        if !self.time.planning_open() {
            return Err(Error::phase_closed(said!(
                "`new_rule` is a planning edit, open only in a Lull: ask in a Lull"
            )));
        }
        let playbook = playbook_param(request)?;
        let made = pharmakos_plan_core::edit::new_rule(playbook, self.host()?.rules())
            .map_err(|error| Error::invalid(pharmakos_plan_core::edit::new_rule_words(&error)))?;
        undo_travels(&made.playbook_jsonc, &made.inverse_json_patch)?;
        Ok(Json::Object(vec![
            (
                String::from("playbook_jsonc"),
                Json::String(made.playbook_jsonc),
            ),
            (
                String::from("inverse_json_patch"),
                Json::String(made.inverse_json_patch),
            ),
            (String::from("rule_pointer"), Json::String(made.pointer)),
            (String::from("rule_id"), Json::String(made.id)),
        ]))
    }

    /// `compose_wall_box`: a wall's Build target from two map clicks (S3's
    /// `fixq`, item 4). The box spans the two corners' columns; its floor is
    /// the lowest ground under walls among them and its top the highest plus
    /// the rules' `materials.wall_min_height_voxels`, read with the sim's own
    /// [`pharmakos_sim::site::ground_under_walls`], and the box is then judged
    /// by the sim's own [`pharmakos_sim::site::check_wall`] against the
    /// beacon's sphere, so the answer is a box the row commits, or a refusal
    /// in words. Its anchor is the box's lowest corner.
    ///
    /// **Only in a Lull**, as `estimate_route`: the hosted world is the frozen
    /// planning world only then, and the read is the TRUE ground there (fog
    /// over terrain is S4's, decisions-log item 168 (10)), as
    /// `get_map_summary`'s travel is. It reveals no ground the client's own
    /// view could not show. **Own beacons only**: another seat's beacon, a
    /// foreign handle and an unknown name answer one `NOT_FOUND` alike. Nothing
    /// is stepped.
    ///
    /// # Errors
    ///
    /// `PHASE_CLOSED` outside a Lull; `NOT_FOUND` for a beacon that is not the
    /// seat's own; `INVALID_ARGUMENT` for a blueprint that fills no box, a
    /// corner past the map's coordinates, no ground under a column, or a box
    /// outside the beacon's sphere.
    pub(super) fn compose_wall_box<V: crate::fog::Vision>(
        &self,
        subject: crate::token::Subject,
        request: &Request,
        vision: &V,
    ) -> Result<Json, Error> {
        if !self.time.planning_open() {
            return Err(Error::phase_closed(said!(
                "`compose_wall_box` reads the frozen planning world, which is open only in a \
                 Lull: ask in a Lull"
            )));
        }
        let seat = Surface::seat_of(subject, "a wall to compose")?;
        let wanted = request
            .string_param("beacon_id")?
            .ok_or_else(|| Error::invalid("`beacon_id` names one of your beacons, as in `b_04`"))?;
        let blueprint = request.string_param("blueprint_id")?.ok_or_else(|| {
            Error::invalid("`blueprint_id` names the blueprint that fills the box: `wall`")
        })?;
        let boxed = pharmakos_plan_core::vocabulary::blueprints_by_slot()
            .into_iter()
            .find(|kind| kind.id == blueprint)
            .is_some_and(|kind| kind.anchor == pharmakos_plan_core::vocabulary::AnchorShape::Box);
        if !boxed {
            return Err(Error::invalid(said!(
                "`{}` fills no box: only a wall does",
                pharmakos_proto::json::grammar::quoted(blueprint)
            )));
        }
        let corner_a = corner_of(request, "corner_a")?;
        let corner_b = corner_of(request, "corner_b")?;
        let host = self.host()?;
        let world = host.world();
        let absent = || {
            Error::not_found(said!(
                "no beacon `{}` of yours here",
                pharmakos_proto::json::grammar::quoted(wanted)
            ))
        };
        // Own beacons only, and one refusal for every name that is not one.
        // Read as `get_beacon` reads a name, and standing: one refusal for
        // every name that is not one of the seat's own living beacons.
        let row = self
            .own_standing_beacon(seat, vision, wanted)?
            .ok_or_else(absent)?;
        let beacon = pharmakos_sim::tables::BeaconId::new(
            world.beacons().ids().get(row).copied().ok_or_else(|| {
                Error::internal(said!("the beacon table has no id in row {}", row))
            })?,
        );
        let centre = world
            .beacons()
            .positions()
            .get(row)
            .copied()
            .ok_or_else(|| {
                Error::internal(said!("the beacon table has no position in row {}", row))
            })?;
        let height = i32::from(host.rules().site().wall_min_height_voxels);
        let wall = sized_wall(world, beacon, centre, corner_a, corner_b, height)?;
        Ok(wall_target_json(blueprint, &wall))
    }

    /// `save_draft`: keep a playbook, privately, for this seat.
    ///
    /// A draft never leaves the gateway for anybody but its seat (spec section
    /// 12). Saving one under an id the seat already has **replaces** it, which
    /// is what an editor's Save does.
    pub(super) fn save_draft(
        &mut self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "drafts")?;
        let playbook = playbook_param(request)?.to_owned();
        // An absent label is proto3's empty string: a draft with no label.
        let label = match request.string_param("label")? {
            Some(label) => label.to_owned(),
            None => String::new(),
        };
        if label.chars().count() > MAX_DRAFT_LABEL_CHARS {
            return Err(Error::invalid(said!(
                "a draft's label is at most {} characters",
                MAX_DRAFT_LABEL_CHARS
            )));
        }
        let round = self.host()?.runner().round();
        // An absent id is minted, the next free `d<round>-<n>` (`gateway.proto`).
        let draft_id = match request.string_param("draft_id")? {
            Some(asked) => asked.to_owned(),
            None => format!("d{round}-{}", self.next_draft_number(seat, round)?),
        };
        if !is_draft_id(&draft_id) {
            return Err(Error::invalid(said!(
                "`{}` is not a draft id: lower-case ASCII letters, digits, `-` and `_`, \
                 starting with a letter",
                pharmakos_proto::json::grammar::quoted(&draft_id)
            )));
        }

        // The carried draft is the gateway's (draft continuity): a seat reads
        // it and saves its edit under an id of its own.
        if draft_id == crate::surface::CARRIED_DRAFT_ID {
            return Err(Error::invalid(said!(
                "`{}` is the draft the gateway carries last round's seal in; save your edit \
                 under another id",
                crate::surface::CARRIED_DRAFT_ID
            )));
        }

        let state = self.seat_state_mut(subject, seat)?;
        let replacing = state.draft(&draft_id).is_some();
        if !replacing && own_drafts(&state.drafts) >= MAX_DRAFTS {
            return Err(Error::invalid(said!(
                "this seat holds {} drafts of its own, as many as it may, and a draft is \
                 never deleted: save under one of their ids to replace that draft",
                MAX_DRAFTS
            )));
        }
        state.drafts.retain(|draft| draft.draft_id != draft_id);
        state.drafts.push(Draft {
            draft_id: draft_id.clone(),
            label,
            round,
            playbook_jsonc: playbook,
        });
        Ok(Json::Object(vec![(
            String::from("draft_id"),
            Json::String(draft_id),
        )]))
    }

    /// `list_drafts`: the seat's own, and nobody else's.
    ///
    /// Summaries only. A draft's **body** is not in `gp.api.v1.DraftSummary`
    /// and does not travel here: a listing is what the editor's sidebar reads,
    /// and a listing that carried every playbook in full would be the one call
    /// that hands a seat's whole round's work to anybody who got hold of one
    /// response.
    pub(super) fn list_drafts(&self, subject: crate::token::Subject) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "drafts")?;
        let state = self.seat_state(subject, seat)?;
        let drafts: Vec<Json> = state
            .drafts
            .iter()
            .map(|draft| {
                Json::Object(vec![
                    (
                        String::from("draft_id"),
                        Json::String(draft.draft_id.clone()),
                    ),
                    (String::from("label"), Json::String(draft.label.clone())),
                    (String::from("round"), Json::Number(draft.round.to_string())),
                ])
            })
            .collect();
        Ok(Json::Object(vec![
            (String::from("drafts"), Json::Array(drafts)),
            (String::from("next_cursor"), Json::String(String::new())),
        ]))
    }

    /// `get_draft`: one of the seat's own drafts, body and all.
    ///
    /// Decisions-log item 112 (5). A listing carries summaries only
    /// ([`Surface::list_drafts`]); this is the one method that returns a
    /// draft's playbook, and it is how a client that has restarted -- after a
    /// resume, a new process with no copy of anything -- opens last round's
    /// carried draft (spec section 13, "Draft continuity").
    ///
    /// **The caller's own drafts and nobody else's.** The lookup is in the
    /// calling seat's own store, through [`Surface::seat_state`], so another
    /// seat's draft is simply not there to find: an id that belongs to
    /// another seat and an id nobody saved get the same `NOT_FOUND`, word for
    /// word, and the refusal does not echo the id back. Phase-gated like every
    /// `plan` method, by the scope the schema annotates it with.
    pub(super) fn get_draft(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "drafts")?;
        let draft_id = request
            .string_param("draft_id")?
            .ok_or_else(|| Error::invalid("`draft_id` names one of this seat's drafts"))?;
        let state = self.seat_state(subject, seat)?;
        let draft = state
            .draft(draft_id)
            .ok_or_else(|| Error::not_found("this seat has no draft by that id"))?;
        Ok(Json::Object(vec![
            (
                String::from("playbook_jsonc"),
                Json::String(draft.playbook_jsonc.clone()),
            ),
            (String::from("label"), Json::String(draft.label.clone())),
            (String::from("round"), Json::Number(draft.round.to_string())),
        ]))
    }

    /// `get_safe_plan`: what would be filed for this seat on a timeout.
    ///
    /// Spec section 14, and item 81's reason for shipping it as a template:
    /// the editor can render it, so **the cost of a timeout is visible**.
    ///
    /// **The seat's own** (decisions-log item 111, decision C5): the built-in
    /// operator's safe playbook for the calling seat, as
    /// [`Surface::file_advice`] verified and filed it this round. A seat no
    /// operator advises, or whose advice did not qualify, is shown the
    /// gateway's fallback, [`crate::host::safe_playbook`] — which is exactly
    /// what [`Surface::begin_push`] would file for it, so what this answers
    /// and what a timeout costs are the same playbook.
    pub(super) fn get_safe_plan(&self, subject: crate::token::Subject) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "a safe playbook")?;
        let own = self
            .current_advice(seat)
            .and_then(|advised| advised.safe.as_ref())
            .map(|safe| safe.playbook_jsonc().to_owned());
        let playbook = match own {
            Some(playbook) => playbook,
            None => self.host()?.safe_playbook().to_owned(),
        };
        Ok(Json::Object(vec![(
            String::from("playbook_jsonc"),
            Json::String(playbook),
        )]))
    }

    /// `resolve_refs`: what each feature reference in a playbook reads now
    /// (`docs/design/targeting.md`, "Surfaces"; the register's S1-48 lists it
    /// beside `get_map_summary.features`).
    ///
    /// One `ResolvedRef` per `covering` and per `on`, in file order, each read
    /// over the frozen world exactly as the sim reads it when its step starts
    /// ([`crate::targeting::resolve_playbook`] says how, site by site), with
    /// the pick taken from the sim's own resolver. **Nothing is stepped**:
    /// "nearest" is an estimate over terrain the snapshot already holds, the
    /// same estimate `estimate_route` answers with, and ranking it is a preview
    /// rather than a dry run (AGENTS.md section 3 rule 2; `gateway.proto`'s
    /// own comment on the method).
    ///
    /// The playbook goes through the sim's door first, as a submission does
    /// ([`compile_decoded`]): a file this build cannot execute reads nothing,
    /// and is refused with the door's own reason rather than half-answered. A
    /// file with no canonical form names nothing yet, and is refused too:
    /// `verify_plan` is where its diagnostics are.
    ///
    /// **Outside a Push only**, as `estimate_route`'s `covering` is: in a Push
    /// the hosted world is the live one, so a preview there would rank over
    /// the live map and is `PHASE_CLOSED` (decisions-log item 133 (3) (f)).
    ///
    /// Internal in S1 -- it is on the advisor's allow-list
    /// ([`crate::serve::ADVISOR_METHODS`]) because the operator, like the
    /// editor, cannot reach the sim -- and published with v1.1.
    pub(super) fn resolve_refs(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "a playbook to resolve")?;
        let host = self.host()?;
        // Its own gate, beside the door's, and before anything is parsed, so
        // a Push answers PHASE_CLOSED whatever the request carries: a preview
        // reads the hosted world, which is the frozen planning world only
        // outside a Push. In a Push the world has moved on, and a ranking over
        // it would answer about the live map -- which `estimate_route`'s
        // `covering` already refuses the same way (decisions-log item 133 (3)
        // (f)). The dispatcher's planning door already closes every
        // `plan`-scoped method in a Push and a recap, so through `call` this
        // gate is defence in depth: it holds the rule where the read is,
        // should the method's scope ever change, and
        // `surface::tests::resolve_refs_closes_its_own_door_in_a_push`
        // reaches it without the dispatcher.
        if host.runner().phase() == pharmakos_sim::runner::MatchPhase::Push {
            return Err(Error::phase_closed(
                "`resolve_refs` reads the frozen world, and in a Push the world has moved on \
                 from it: ask outside a Push",
            ));
        }
        let playbook = playbook_param(request)?;
        let canonical = pharmakos_plan_core::decode_jsonc(playbook).map_err(|error| {
            Error::invalid(said!(
                "this file has no canonical form yet, so it names nothing to resolve: {}",
                error.said()
            ))
        })?;
        let playbook = canonical.decoded().message();
        let plan = compile_decoded(playbook, host.rules()).map_err(|why| {
            Error::invalid(said!(
                "this build cannot execute this playbook, so none of it reads anything: {}",
                why
            ))
        })?;
        let resolved = crate::targeting::resolve_playbook(
            host.world(),
            host.runner().frozen().snapshot(),
            seat,
            playbook,
            &plan,
        )?;
        Ok(Json::Object(vec![(
            String::from("refs"),
            Json::Array(
                resolved
                    .iter()
                    .map(resolved_json)
                    .collect::<Result<Vec<Json>, Error>>()?,
            ),
        )]))
    }

    /// The Lull's "this round" sentence for `seat`: what the playbook it will
    /// play this round does wherever its route reads the map
    /// (`docs/design/targeting.md`, "Surfaces": "On re-seal the Lull says what
    /// this round will do"). `get_briefing` carries it in its prose, to a
    /// caller that holds `plan.submit` only.
    ///
    /// **It reads the seat's seal and nothing else** -- gateway-owned state a
    /// caller cannot write except by sealing. The playbook is the seal when
    /// the seat sealed this round, so a re-seal changes the sentence at once;
    /// otherwise it is last round's seal, if the seat sealed it itself, which
    /// is byte for byte the playbook this Lull carried as its draft (spec
    /// section 13, "Draft continuity"; `Surface::carry_draft_forward` copies
    /// it from there): the demo's round 2 reads "no vent you can cover" off a
    /// carried "cover the nearest vent" before anybody has touched it. The
    /// draft list itself is never read: a seat may save anything under the
    /// carried draft's id, and the advisor, which reads briefings, never reads
    /// a seat's drafts (decisions-log item 111). A playbook the gateway filed
    /// is not the seat's and is not carried, so it says nothing.
    ///
    /// Read exactly as `resolve_refs` reads it, over the frozen world, with
    /// the plan the seal compiled to. `None` outside a Lull, when there is no
    /// such seal, and when its route reads the map nowhere.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when a sealed playbook has no
    /// canonical form, when the rules table prices no beacon, or when a
    /// `covering` read answers neither a site nor a failure: each is the
    /// gateway contradicting itself, since a seal was verified and compiled
    /// once already.
    pub(super) fn this_round(
        &self,
        seat: pharmakos_sim::tables::SeatId,
    ) -> Result<Option<String>, Error> {
        use crate::strings::RoundRead;
        use crate::targeting::Site;
        use pharmakos_sim::interpreter::StepFailure;
        let host = self.host()?;
        if host.runner().phase() != pharmakos_sim::runner::MatchPhase::Lull {
            return Ok(None);
        }
        let round = host.runner().round();
        let state = self.seat_state(crate::token::Subject::Seat(seat), seat)?;
        let Some(sealed) = state.sealed.as_ref().filter(|sealed| {
            sealed.round() == round
                || (sealed.round().checked_add(1) == Some(round) && !sealed.filed_by_the_gateway())
        }) else {
            return Ok(None);
        };
        let canonical =
            pharmakos_plan_core::decode_jsonc(sealed.playbook_jsonc()).map_err(|error| {
                Error::internal(said!(
                    "a playbook this seat sealed has no canonical form: {}",
                    error.said()
                ))
            })?;
        let reads = crate::targeting::resolve_playbook(
            host.world(),
            host.runner().frozen().snapshot(),
            seat,
            canonical.decoded().message(),
            sealed.plan(),
        )?;
        let dollars = host
            .rules()
            .message()
            .structures
            .as_ref()
            .and_then(|block| block.beacon)
            .map(|beacon| beacon.cost_dollars)
            .ok_or_else(|| Error::internal("the rules table prices no beacon"))?;
        let mut lines: Vec<RoundRead<'_>> = Vec::new();
        // S3's `pred`: a `toward` placement reads the map as a `covering` does,
        // through the same placement reader, and says so in its own words.
        for read in reads
            .iter()
            .filter(|read| read.site == Site::Covering || read.site == Site::Toward)
        {
            let toward = read.site == Site::Toward;
            // A handler body may never run, so only the route speaks.
            let Some(step) = read.route_step else {
                continue;
            };
            let kind = read
                .kind
                .map_or("feature", pharmakos_sim::features::FeatureKind::name);
            lines.push(match (read.failure, read.site_voxel) {
                (None, Some(site)) => RoundRead::Places {
                    step,
                    x: site.x,
                    y: site.y,
                    dollars,
                },
                (Some(StepFailure::NoTarget), _) if toward && read.matched == 0 => {
                    RoundRead::NothingToward { step, kind }
                }
                (Some(StepFailure::NoTarget), _) if toward => RoundRead::NoneReachableToward {
                    step,
                    matched: read.matched,
                },
                (Some(StepFailure::IllegalSite), _) if toward => RoundRead::NoStone {
                    step,
                    kind,
                    weighed: read.weighed.ok_or_else(|| {
                        Error::internal("a `toward` that found no site counted no sites")
                    })?,
                },
                (Some(StepFailure::NoTarget), _) if read.matched == 0 => {
                    RoundRead::NothingMatches { step, kind }
                }
                (Some(StepFailure::NoTarget), _) => RoundRead::NoneReachable {
                    step,
                    matched: read.matched,
                },
                (Some(StepFailure::IllegalSite), _) => RoundRead::NoneCoverable { step, kind },
                (Some(failure), _) => RoundRead::Fails {
                    step,
                    failure: failure.name(),
                },
                (None, None) => {
                    return Err(Error::internal(
                        "a `covering` or `toward` read answered neither a site nor a failure",
                    ));
                }
            });
        }
        Ok(crate::strings::this_round(&lines))
    }

    /// `submit_plan`: seal it.
    ///
    /// Always FULL. A report with `qualifies: false` comes back with
    /// `accepted: false` and the seat's previous seal untouched -- a failed
    /// submission must not unseal what was already sealed. A report that
    /// qualifies **replaces** the previous seal, any number of times until the
    /// timer ends (decisions-log item 5).
    ///
    /// Two doors stand after the verifier, and a refusal at either is a method
    /// error, `INVALID_ARGUMENT`, with the previous seal untouched: the sim's
    /// compile ([`compile_canonical`]), and the price of the seal
    /// (`Surface::seal_commitment_of`) -- a playbook whose spend or draw does
    /// not fit the sim's `$` and kW types, or whose spend is past the
    /// forecast's `int32`, is refused rather than sealed into a forecast that
    /// could not answer. The seal carries the report's plan fingerprint, and a
    /// qualifying report that carries none is `INTERNAL`, the previous seal
    /// untouched again: the verifier and the gateway disagreeing, never a
    /// seal under a fingerprint nobody took.
    pub(super) fn submit_plan(
        &mut self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "a playbook to submit")?;
        let playbook = playbook_param(request)?.to_owned();
        let round = self.host()?.runner().round();
        // One decode from the socket to the seal (S3's `vfyq2`, fix pass 1,
        // item 3): the report, the compile, the price, the fingerprint and the
        // seal all come from the canonical form `verified_for` decoded, made
        // into a seal by its one maker (`Surface::qualify`), so the playbook
        // sealed is the one the report qualified, by construction, and the
        // file written is the text that canonical form was decoded from.
        //
        // The doors after the verifier: the sim's compile, the price of the
        // seal (a seal always prices, so the seat's own forecast can always
        // answer its `committed_dollars`, review A of `econ`; from S2 the
        // verifier refuses one that does not, `E0603`, so the door stays as
        // defence in depth), and a qualifying report's fingerprint. A refusal
        // at any of them is a method error, and the previous seal is left
        // exactly where it is: the gateway does not say `qualifies: false` in
        // the same breath as handing out the hash of a report that qualified.
        let (report, accepted) = match self.qualify(seat, playbook)? {
            super::sealed::Qualified::Sealable { report, unfiled } => {
                self.seat_state_mut(subject, seat)?.sealed = Some(unfiled.file(round, false));
                (report, true)
            }
            super::sealed::Qualified::Refused { report } => (report, false),
        };
        Ok(Json::Object(vec![
            (String::from("report"), report_json(&report)),
            (String::from("accepted"), Json::Bool(accepted)),
        ]))
    }

    /// The next unused number for an auto-named draft of this seat.
    ///
    /// The lowest `d{round}-{n}` the seat does not already hold, and **not**
    /// `len() + 1`: a seat that named a draft `d1-2` itself, or that carries
    /// last round's draft under [`crate::surface::CARRIED_DRAFT_ID`], would
    /// otherwise have its next auto-named save land on an id it already held
    /// and silently replace it. `save_draft` replacing an id on purpose is what
    /// an editor's Save is; replacing one the client never named is data loss
    /// with no error to notice it by.
    ///
    /// # Errors
    ///
    /// As [`Surface::seat_state`], and [`crate::error::Code::Internal`] when
    /// none of the first [`DRAFT_NUMBERS`] numbers is free, which holding at
    /// most `MAX_DRAFTS` drafts makes impossible.
    fn next_draft_number(
        &self,
        seat: pharmakos_sim::tables::SeatId,
        round: u32,
    ) -> Result<usize, Error> {
        let state = self.seat_state(crate::token::Subject::Seat(seat), seat)?;
        // Bounded by DRAFT_NUMBERS by construction: at most MAX_DRAFTS ids are
        // held, so one of the first DRAFT_NUMBERS candidates is free.
        (1..=DRAFT_NUMBERS)
            .find(|number| state.draft(&format!("d{round}-{number}")).is_none())
            .ok_or_else(|| {
                Error::internal(said!(
                    "seat {} holds more than {} drafts of round {}",
                    seat.raw(),
                    MAX_DRAFTS,
                    round
                ))
            })
    }
}

/// The sim's door: a playbook's canonical form, already decoded, to a
/// compiled [`Plan`], or the refusal a caller is told.
///
/// The one path from a submitted file to something the interpreter can run, so
/// `submit_plan`, [`crate::surface::Surface`]'s filing of the safe playbook
/// and of an advised one, and a resume's restore of a saved seal cannot
/// compile it two different ways. Each compiles the canonical form its report
/// or its fingerprint was taken over (S3's `vfyq2`, fix pass 1, item 3): what
/// is verified is what is compiled.
///
/// # The code, and why it is this one
///
/// [`crate::error::Code::InvalidArgument`]. The closed set
/// (`gp.api.v1.GatewayError.Code`) has no word for "valid and not executable by
/// this build", and AGENTS.md section 5 says adding one is a contract change,
/// so the question is which of the ten existing codes is least wrong:
///
/// * `INTERNAL` is defined as "the gateway failed; never used to report
///   anything the caller could have avoided", and the caller **could** have
///   avoided this one by writing a different playbook;
/// * `NO_QUALIFYING_PLAN` already means "this seat has sealed nothing"
///   ([`crate::surface::Surface::sealed_plan`]) and answering a submission with
///   it would say something true of the seat's *store* rather than of the file
///   it just sent;
/// * `UNSUPPORTED_SCHEMA_VERSION` fits [`PlanError::UnknownEnum`] and nothing
///   else here, and one door answering with two codes is a door a client has to
///   learn twice;
/// * `INVALID_ARGUMENT` is "a param out of range" — the `playbook_jsonc`
///   parameter carries something this build cannot run — and its JSON-RPC
///   number is -32602, "invalid params", which tells a generic client the truth:
///   the request, not the server.
///
/// The message is the whole of the value here, and it names the construct and
/// the stage that gives it an effect ([`PlanError`]'s `Display`). A client that
/// branches on the code learns "fix the file"; a person reading the message
/// learns which line to fix.
///
/// [`PlanError`]: pharmakos_sim::interpreter::PlanError
/// [`PlanError::UnknownEnum`]: pharmakos_sim::interpreter::PlanError::UnknownEnum
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] for a [`PlanError`].
pub(crate) fn compile_canonical(
    canonical: &pharmakos_proto::json::Canonicalised<pharmakos_proto::gp::v1::Playbook>,
    rules: &RulesTable,
) -> Result<Plan, Error> {
    compile_decoded(canonical.decoded().message(), rules).map_err(|why| {
        Error::invalid(said!(
            "this playbook qualifies and this build cannot execute it: {}",
            why
        ))
    })
}

/// [`compile_canonical`] for the scenario harness's door
/// ([`crate::surface::Surface::seal_by_harness`]), whose playbook the verifier
/// refused: the same door, with words that do not say it qualifies.
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] for a [`PlanError`].
///
/// [`PlanError`]: pharmakos_sim::interpreter::PlanError
pub(crate) fn compile_harnessed(
    canonical: &pharmakos_proto::json::Canonicalised<pharmakos_proto::gp::v1::Playbook>,
    rules: &RulesTable,
) -> Result<Plan, Error> {
    compile_decoded(canonical.decoded().message(), rules)
        .map_err(|why| Error::invalid(said!("this build cannot execute it: {}", why)))
}

/// The sim's own door, over a playbook already decoded: the **one** call of
/// `Plan::compile` in the handler modules (`tests/confinement.rs`), which
/// [`compile_canonical`] and `resolve_refs` both go through, so the playbook
/// `resolve_refs` reads is the one a submission would seal. A refusal comes
/// back as the gateway's own sentence for it ([`plan_refusal`]), never as
/// the sim's error, so no handler can say a `PlanError` any other way.
fn compile_decoded(
    playbook: &pharmakos_proto::gp::v1::Playbook,
    rules: &RulesTable,
) -> Result<Plan, Said> {
    Plan::compile(playbook, rules).map_err(|error| super::plan_words::plan_refusal(&error))
}

/// The undo of a `patch_plan` answer travels back (S3's `vfyq2`, fix pass 6,
/// item 2): the request that sends the inverse back, `patch_plan` with the
/// patched file and the inverse, measured as the client writes it
/// ([`crate::rpc::request_text`], with the widest numeric id a client
/// counts to, a `u64`'s twenty digits), must be a text the gateway reads
/// ([`crate::frame::travels`]). The inverse holds the text it takes out as
/// JSON, and the request writes it as JSON again, so it can be far larger
/// than the request that made it. Otherwise the forward patch is refused up
/// front, as an undo that could not be issued, rather than answered with an
/// undo whose request would close the seat's connection (1009).
fn undo_travels(patched: &str, inverse: &str) -> Result<(), Error> {
    let request = crate::rpc::request_text(
        &Json::Number(u64::MAX.to_string()),
        "patch_plan",
        Json::Object(vec![
            (
                String::from("playbook_jsonc"),
                Json::String(patched.to_owned()),
            ),
            (String::from("json_patch"), Json::String(inverse.to_owned())),
        ]),
    );
    if crate::frame::travels(request.len()) {
        return Ok(());
    }
    Err(Error::invalid(said!(
        "this patch is not applied: the patch that would undo it could not be issued (its \
         request would be {} bytes, past the {} one message carries)",
        request.len(),
        crate::frame::MAX_MESSAGE_BYTES
    )))
}

/// The `playbook_jsonc` parameter, bounded.
fn playbook_param(request: &Request) -> Result<&str, Error> {
    let text = request
        .string_param("playbook_jsonc")?
        .ok_or_else(|| Error::invalid("`playbook_jsonc` is the playbook, as JSONC text"))?;
    if pharmakos_verifier::is_too_long(text) {
        return Err(Error::invalid(said!(
            "a playbook is at most {} characters and this is {}",
            MAX_PLAYBOOK_CHARS,
            text.chars().count()
        )));
    }
    Ok(text)
}

/// The `depth` parameter: `"quick"`, `"full"`, or absent for FULL.
fn depth_of(request: &Request) -> Result<Depth, Error> {
    match request.string_param("depth")? {
        // An absent depth and an explicit "full" are the same depth on purpose:
        // `gateway.proto` says `verify_plan` defaults to FULL, which is also
        // what `submit_plan` always runs, so a client that omits the parameter
        // gets the report a submit would produce.
        None | Some("full") => Ok(Depth::Full),
        Some("quick") => Ok(Depth::Quick),
        Some(other) => Err(Error::invalid(said!(
            "`depth` is `quick` or `full`, and this is `{}`",
            pharmakos_proto::json::grammar::quoted(other)
        ))),
    }
}

/// The refusal of a library read, naming the template and never the path
/// (S3's `outcap`): a file past the playbook's text cap is the host's library
/// at fault, said as such (`INTERNAL`: nothing the caller did caused it, and
/// the words name the cap); any other read refusal is that the template is not
/// there (`NOT_FOUND`).
pub(crate) fn library_refusal(template_id: &str, refused: &pharmakos_plan_core::Error) -> Error {
    if refused.bound() == Some(pharmakos_plan_core::error::Bound::LibraryFile) {
        return Error::internal(said!(
            "the template `{}` in this gateway's library is past the {} characters a library \
             file may hold, so it is not read",
            pharmakos_proto::json::grammar::quoted(template_id),
            pharmakos_verifier::MAX_PLAYBOOK_CHARS
        ));
    }
    Error::not_found(said!(
        "no template `{}` in this gateway's library",
        pharmakos_proto::json::grammar::quoted(template_id)
    ))
}

/// True for a template id this gateway will look up.
///
/// The gate in front of `plan-core`'s own character rule, and it is here rather
/// than there for a reason the two reviews found between them: `plan-core`'s
/// rule refuses a separator, a device name and a relative step, which is what
/// stops a traversal, but `..%2Fsecret` and `ok\0` are neither -- they are
/// refused by the **filesystem**, one level further down, and the message that
/// comes back from there names the absolute path it tried to open. A percent
/// escape and a control byte are not part of a plain file stem in the first
/// place, so they are refused by name before a path exists at all.
///
/// Deliberately the same shape as [`is_draft_id`]: characters, never
/// `std::path::Component`, so the same hostile id gets the same answer on all
/// three platforms (decisions-log item 100's closing note).
fn is_template_id(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= 128
        && !text.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '%' | '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
        })
}

/// True for a draft id a seat may name.
///
/// Decided on characters, like a template id and a beacon id, and for the same
/// reason: a draft id reaches the private match cache's filesystem layout, and
/// a rule that leaned on the host's path parsing would answer differently on
/// Windows (decisions-log item 100's closing note).
pub(crate) fn is_draft_id(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= 64
        && text
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase())
        && text.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '_'
        })
}

/// A wall's box sized over the columns between `corner_a` and `corner_b`
/// (`compose_wall_box`): floor the lowest ground under walls, top the highest
/// plus `height`, then judged by the sim's own wall check against `beacon`'s
/// sphere, whose centre is `centre`. The work is bounded by the sphere before
/// a column is read.
///
/// # Errors
///
/// `INVALID_ARGUMENT` in the gateway's words: outside the sphere, or no
/// ground here.
fn sized_wall(
    world: &pharmakos_sim::world::World,
    beacon: pharmakos_sim::tables::BeaconId,
    centre: [pharmakos_sim::math::fixed::Fx; 3],
    corner_a: [i32; 3],
    corner_b: [i32; 3],
    height: i32,
) -> Result<SizedWall, Error> {
    let outside = || Error::invalid(crate::strings::WALL_BOX_OUTSIDE_SPHERE);
    // Before any column is read: a column further from the beacon than its
    // sphere's radius, and a voxel more, cannot hold the wall, so the work
    // is bounded by the sphere and never by the box the client sent.
    let reach = world
        .ground()
        .radius_of(Some(beacon))
        .floor_voxels()
        .checked_add(1)
        .ok_or_else(outside)?;
    let near = |corner: [i32; 3]| {
        let off = |at: i32, middle: i32| {
            at.checked_sub(middle)
                .and_then(i32::checked_abs)
                .is_some_and(|off| off <= reach)
        };
        off(corner[0], centre[0].floor_voxels()) && off(corner[1], centre[1].floor_voxels())
    };
    if !near(corner_a) || !near(corner_b) {
        return Err(outside());
    }
    let low = [corner_a[0].min(corner_b[0]), corner_a[1].min(corner_b[1])];
    let high = [corner_a[0].max(corner_b[0]), corner_a[1].max(corner_b[1])];
    let column = |at: i32| {
        i16::try_from(at).map_err(|_| Error::invalid(crate::strings::WALL_BOX_OFF_THE_MAP))
    };
    let columns = pharmakos_sim::tables::ColumnBox::new(
        [column(low[0])?, column(low[1])?],
        [column(high[0])?, column(high[1])?],
    )
    .ok_or_else(|| Error::internal(said!("an ordered box of columns was refused")))?;
    let mut grounds = Vec::new();
    for y in low[1]..=high[1] {
        for x in low[0]..=high[0] {
            grounds.push(
                pharmakos_sim::site::ground_under_walls(world, x, y).ok_or_else(|| {
                    Error::invalid(said!(
                        "no ground here: column {}, {} is off the map or holds no ground",
                        x,
                        y
                    ))
                })?,
            );
        }
    }
    let (floor, summit) = ground_span(&grounds)
        .ok_or_else(|| Error::internal(said!("a box of columns held no column")))?;
    let top = summit
        .checked_add(height)
        .ok_or_else(|| Error::invalid(crate::strings::WALL_BOX_OFF_THE_MAP))?;
    let wall = pharmakos_sim::site::WallBox {
        columns,
        floor: i16::try_from(floor)
            .map_err(|_| Error::invalid(crate::strings::WALL_BOX_OFF_THE_MAP))?,
        top: i16::try_from(top)
            .map_err(|_| Error::invalid(crate::strings::WALL_BOX_OFF_THE_MAP))?,
    };
    pharmakos_sim::site::check_wall(world, beacon, wall, &()).map_err(|refused| match refused {
        pharmakos_sim::site::WallRefused::OutsideSphere => outside(),
        pharmakos_sim::site::WallRefused::OffTheGround
        | pharmakos_sim::site::WallRefused::TooLow => Error::internal(said!(
            "a box sized to the ground was refused by the ground's own check"
        )),
    })?;
    Ok(SizedWall {
        low,
        high,
        floor,
        top,
    })
}

/// A box [`sized_wall`] sized: its lowest and highest columns, its floor and
/// its top.
struct SizedWall {
    low: [i32; 2],
    high: [i32; 2],
    floor: i32,
    top: i32,
}

/// The `ComposeWallBoxResponse` for `wall`: its Build target, anchored on the
/// box's lowest corner.
fn wall_target_json(blueprint: &str, wall: &SizedWall) -> Json {
    let SizedWall {
        low,
        high,
        floor,
        top,
    } = *wall;
    let voxel = |x: i32, y: i32, z: i32| {
        Json::Object(vec![
            (String::from("x"), Json::Number(x.to_string())),
            (String::from("y"), Json::Number(y.to_string())),
            (String::from("z"), Json::Number(z.to_string())),
        ])
    };
    Json::Object(vec![(
        String::from("target"),
        Json::Object(vec![
            (
                String::from("blueprint_id"),
                Json::String(blueprint.to_owned()),
            ),
            (
                String::from("anchor"),
                Json::Object(vec![(String::from("voxel"), voxel(low[0], low[1], floor))]),
            ),
            (
                String::from("wall_box"),
                Json::Object(vec![
                    (String::from("min"), voxel(low[0], low[1], floor)),
                    (String::from("max"), voxel(high[0], high[1], top)),
                ]),
            ),
        ]),
    )])
}

/// The lowest and the highest of `grounds`, a wall box's floor and the
/// ground its top stands over; `None` for no column.
fn ground_span(grounds: &[i32]) -> Option<(i32, i32)> {
    let mut span: Option<(i32, i32)> = None;
    for &ground in grounds {
        span = Some(match span {
            None => (ground, ground),
            Some((lowest, highest)) => (
                if ground < lowest { ground } else { lowest },
                if ground > highest { ground } else { highest },
            ),
        });
    }
    span
}

/// The `gp.v1.Voxel` parameter `name` as `[x, y, z]`, each a whole number
/// in `int32`.
///
/// # Errors
///
/// `INVALID_ARGUMENT` for a parameter that is absent, not an object of the
/// three, or holds a coordinate that is no `int32`.
fn corner_of(request: &Request, name: &str) -> Result<[i32; 3], Error> {
    let refused = || {
        Error::invalid(said!(
            "`{}` is a voxel: an object of exactly `x`, `y` and `z`, each a whole number",
            pharmakos_proto::json::grammar::quoted(name)
        ))
    };
    let Some(Json::Object(members)) = request.param(name) else {
        return Err(refused());
    };
    let mut out = [0_i32; 3];
    for (slot, axis) in out.iter_mut().zip(["x", "y", "z"]) {
        let Some((_, Json::Number(text))) = members.iter().find(|(key, _)| key == axis) else {
            return Err(refused());
        };
        *slot = text.parse::<i32>().map_err(|_| refused())?;
    }
    if members.len() != 3 {
        return Err(refused());
    }
    Ok(out)
}

/// One `gp.api.v1.ResolvedRef` as JSON: every field, the defaults included,
/// as `estimate_route`'s legs are written, except `travel_ms`. `failure` is
/// the step failure's name as the segment feed spells it, and empty when the
/// reference reads a feature.
///
/// **`reachable` says whether a route reaches what the reference reads**
/// (decisions-log item 133 (3) (i); `con3`'s field): true exactly when there
/// is a travel to write, and then `travel_ms` is written beside it. A
/// reference that reads nothing, or reads a named pick the origin cannot
/// reach, is `reachable: false` **with no `travel_ms` at all**, so no reader
/// can take the 0 a magic value once stood for as a feature at the origin.
///
/// Every travel written carries its `travel_text` beside it, plan-core's
/// rendering of the figure as `estimate_route` renders a leg's (S3's `fixq`,
/// item 12), so the chip does no time maths.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a travel the rendering refuses (one
/// below zero), never drawn as a clamped figure.
fn resolved_json(resolved: &crate::targeting::Resolved) -> Result<Json, Error> {
    let text = |ms: pharmakos_sim::math::quantity::Ms| {
        pharmakos_plan_core::travel::whole_seconds(ms).map_err(|error| {
            Error::internal(said!(
                "a reference's travel has no rendering: {}",
                Said::engine_error(&error)
            ))
        })
    };
    let candidates: Vec<Json> = resolved
        .candidates
        .iter()
        .map(|candidate| {
            Ok(Json::Object(vec![
                (
                    String::from("feature_id"),
                    Json::String(candidate.feature_id.clone()),
                ),
                (
                    String::from("travel_ms"),
                    Json::Number(candidate.travel_ms.raw().to_string()),
                ),
                (
                    String::from("travel_text"),
                    Json::String(text(candidate.travel_ms)?),
                ),
            ]))
        })
        .collect::<Result<Vec<Json>, Error>>()?;
    let mut entries = vec![
        (
            String::from("pointer"),
            Json::String(resolved.pointer.clone()),
        ),
        (
            String::from("feature_id"),
            Json::String(resolved.feature_id.clone()),
        ),
    ];
    if let Some(travel) = resolved.travel_ms {
        entries.push((
            String::from("travel_ms"),
            Json::Number(travel.raw().to_string()),
        ));
    }
    entries.extend([
        (String::from("candidates"), Json::Array(candidates)),
        (
            String::from("matched"),
            Json::Number(resolved.matched.to_string()),
        ),
        (
            String::from("failure"),
            Json::String(match resolved.failure {
                Some(failure) => failure.name().to_owned(),
                None => String::new(),
            }),
        ),
        (
            String::from("reachable"),
            Json::Bool(resolved.travel_ms.is_some()),
        ),
    ]);
    if let Some(travel) = resolved.travel_ms {
        entries.push((String::from("travel_text"), Json::String(text(travel)?)));
    }
    Ok(Json::Object(entries))
}

/// One `gp.api.v1.VerifyReport` as JSON.
///
/// Field for field from the message, with the two `bytes` fields in the
/// standard base64 the proto3 JSON mapping writes -- through the proto crate's
/// own module, which is the project's one base64 (decisions-log item 100 (10)).
fn report_json(report: &VerifyReport) -> Json {
    let diagnostics: Vec<Json> = report.diagnostics.iter().map(diagnostic_json).collect();
    Json::Object(vec![
        (String::from("qualifies"), Json::Bool(report.qualifies)),
        (
            String::from("depth"),
            Json::String(String::from(match report.depth() {
                Depth::Quick => "quick",
                Depth::Unspecified | Depth::Full => "full",
            })),
        ),
        (String::from("diagnostics"), Json::Array(diagnostics)),
        (
            String::from("report_hash"),
            Json::String(pharmakos_proto::json::base64::encode(&report.report_hash)),
        ),
        (
            String::from("rules_hash"),
            Json::String(pharmakos_proto::json::base64::encode(&report.rules_hash)),
        ),
        (
            String::from("plan_fingerprint"),
            Json::String(pharmakos_proto::json::base64::encode(
                &report.plan_fingerprint,
            )),
        ),
        (
            String::from("verifier_version"),
            Json::String(report.verifier_version.clone()),
        ),
        (
            String::from("size_units"),
            Json::Number(report.size_units.to_string()),
        ),
        (
            String::from("size_budget"),
            Json::Number(report.size_budget.to_string()),
        ),
    ])
}

/// One `gp.api.v1.Diagnostic` as JSON.
fn diagnostic_json(diagnostic: &pharmakos_proto::gp::api::v1::Diagnostic) -> Json {
    let strings = |values: &[String]| {
        Json::Array(
            values
                .iter()
                .map(|text| Json::String(text.clone()))
                .collect(),
        )
    };
    let suggestions: Vec<Json> = diagnostic
        .suggestions
        .iter()
        .map(|suggestion| {
            Json::Object(vec![
                (
                    String::from("title"),
                    Json::String(suggestion.title.clone()),
                ),
                (
                    String::from("json_patch"),
                    Json::String(suggestion.json_patch.clone()),
                ),
                (
                    String::from("applicability"),
                    Json::String(pharmakos_proto::scope::wire_name(
                        "gp.api.v1.PatchSuggestion.Applicability",
                        suggestion.applicability().as_str_name(),
                    )),
                ),
            ])
        })
        .collect();
    let map_refs: Vec<Json> = diagnostic
        .map_refs
        .iter()
        .map(|voxel| {
            Json::Object(vec![
                (String::from("x"), Json::Number(voxel.x.to_string())),
                (String::from("y"), Json::Number(voxel.y.to_string())),
                (String::from("z"), Json::Number(voxel.z.to_string())),
            ])
        })
        .collect();
    Json::Object(vec![
        (String::from("code"), Json::String(diagnostic.code.clone())),
        (
            String::from("severity"),
            Json::String(pharmakos_proto::scope::wire_name(
                "gp.api.v1.Diagnostic.Severity",
                diagnostic.severity().as_str_name(),
            )),
        ),
        // A `/byte/<offset>` path is the one named exception to "an RFC 6901
        // pointer" and is forwarded exactly as the verifier wrote it
        // (decisions-log item 96 (2); `gateway.proto` now says so at the field).
        (String::from("path"), Json::String(diagnostic.path.clone())),
        (
            String::from("related_paths"),
            strings(&diagnostic.related_paths),
        ),
        (
            String::from("message"),
            Json::String(diagnostic.message.clone()),
        ),
        (
            String::from("beginner"),
            Json::String(diagnostic.beginner.clone()),
        ),
        (String::from("suggestions"), Json::Array(suggestions)),
        (String::from("map_refs"), Json::Array(map_refs)),
    ])
}

/// One `gp.api.v1.VocabularyEntry`: `locked` only when it is set, every other
/// field always written.
fn vocabulary_entry(entry: &pharmakos_plan_core::vocabulary::Entry) -> Json {
    let mut fields = vec![
        (String::from("name"), Json::String(entry.name.to_owned())),
        (String::from("title"), Json::String(entry.title.to_owned())),
        (
            String::from("parameters"),
            Json::Array(
                entry
                    .parameters
                    .iter()
                    .map(|parameter| {
                        Json::Object(vec![
                            (
                                String::from("pointer"),
                                Json::String(parameter.pointer.to_owned()),
                            ),
                            (
                                String::from("title"),
                                Json::String(parameter.title.to_owned()),
                            ),
                            (
                                String::from("shape"),
                                crate::view::value_shape(&parameter.shape),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            String::from("skeleton_json"),
            Json::String(entry.skeleton_json.clone()),
        ),
    ];
    if let Some(locked) = &entry.locked {
        fields.push((String::from("locked"), Json::String(locked.clone())));
    }
    // Only on a Build target's blueprint entry, lower case (decisions-log item
    // 80); proto3 leaves the unspecified zero out everywhere else.
    if let Some(anchor) = entry.anchor_shape {
        fields.push((
            String::from("anchor_shape"),
            Json::String(pharmakos_proto::scope::wire_name(
                "gp.api.v1.VocabularyEntry.AnchorShape",
                anchor.proto_name(),
            )),
        ));
    }
    Json::Object(fields)
}

/// One `gp.api.v1.RenderedRule`, every field written, the zeros included.
fn rendered_rule(rule: &pharmakos_plan_core::RenderedRule) -> Json {
    let number = |value: u32| Json::Number(value.to_string());
    Json::Object(vec![
        (String::from("pointer"), Json::String(rule.pointer.clone())),
        (String::from("line"), number(rule.line)),
        (String::from("line_count"), number(rule.line_count)),
        (
            String::from("chips"),
            Json::Array(
                rule.chips
                    .iter()
                    .map(|chip| {
                        let mut fields = vec![
                            (String::from("pointer"), Json::String(chip.pointer.clone())),
                            (String::from("line"), number(chip.line)),
                            (String::from("start"), number(chip.start)),
                            (String::from("end"), number(chip.end)),
                            (String::from("shape"), crate::view::value_shape(&chip.shape)),
                            (
                                String::from("value_json"),
                                Json::String(chip.value_json.clone()),
                            ),
                            // S3's `fixq`: every field written, the empty text
                            // of a choice or a reference and `false` included.
                            (
                                String::from("value_text"),
                                Json::String(chip.value_text.clone()),
                            ),
                            (String::from("outside"), Json::Bool(chip.outside)),
                            (
                                String::from("pick_op"),
                                Json::String(pharmakos_proto::scope::wire_name(
                                    "gp.api.v1.ConditionSlot.PickOp",
                                    chip.pick.proto_name(),
                                )),
                            ),
                        ];
                        // S3's `zerotitle`: written only where the value has
                        // words of its own; proto3's empty string is the
                        // absence.
                        if let Some(title) = chip.value_title {
                            fields.push((
                                String::from("value_title"),
                                Json::String(title.to_owned()),
                            ));
                        }
                        Json::Object(fields)
                    })
                    .collect(),
            ),
        ),
        (
            String::from("slots"),
            Json::Array(rule.slots.iter().map(condition_slot).collect()),
        ),
    ])
}

/// One `gp.api.v1.ConditionSlot`, every field written, the empty predicate
/// of an empty slot and a group's `unspecified` op included, its state and
/// its op lower case (decisions-log item 80).
fn condition_slot(slot: &pharmakos_plan_core::ConditionSlot) -> Json {
    Json::Object(vec![
        (String::from("pointer"), Json::String(slot.pointer.clone())),
        (String::from("title"), Json::String(slot.title.clone())),
        (String::from("line"), Json::Number(slot.line.to_string())),
        (
            String::from("state"),
            Json::String(pharmakos_proto::scope::wire_name(
                "gp.api.v1.ConditionSlot.State",
                slot.state.proto_name(),
            )),
        ),
        (
            String::from("predicate"),
            Json::String(slot.predicate.to_owned()),
        ),
        (
            String::from("pick_op"),
            Json::String(pharmakos_proto::scope::wire_name(
                "gp.api.v1.ConditionSlot.PickOp",
                // A group is no pick target, and says so with the zero value.
                match slot.pick {
                    Some(op) => op.proto_name(),
                    None => "PICK_OP_UNSPECIFIED",
                },
            )),
        ),
    ])
}

/// The tests' compile of a JSONC text, where `host.rs`'s tests name it.
#[cfg(test)]
pub(crate) use tests::compile_playbook;

#[cfg(test)]
mod tests {
    use super::resolved_json;
    use crate::error::Error;
    use pharmakos_proto::said;
    use pharmakos_sim::interpreter::Plan;
    use pharmakos_sim::rules::RulesTable;

    /// [`super::compile_canonical`] over a JSONC text, decoded here by
    /// `plan-core`'s `decode_jsonc`: for this crate's tests, which compile a text
    /// with no decode in hand. Every shipped caller holds the decode it verified
    /// or fingerprinted and compiles that (S3's `fpstream`, part 2: the
    /// gateway's filing of its fallback, the last shipped caller, now decodes
    /// once for the compile and the fingerprint both).
    ///
    /// # Errors
    ///
    /// As [`super::compile_canonical`], and [`crate::error::Code::Internal`]
    /// when the text has no canonical form at all.
    pub(crate) fn compile_playbook(
        playbook_jsonc: &str,
        rules: &RulesTable,
    ) -> Result<Plan, Error> {
        let canonical = pharmakos_plan_core::decode_jsonc(playbook_jsonc).map_err(|error| {
            Error::internal(said!(
                "a playbook the verifier read has no canonical form: {}",
                error.said()
            ))
        })?;
        super::compile_canonical(&canonical, rules)
    }
    use crate::targeting::{Candidate, Resolved, Site};
    use pharmakos_proto::json::Json;
    use pharmakos_sim::math::quantity::Ms;

    fn named(travel: Option<Ms>, candidates: Vec<Candidate>) -> Resolved {
        Resolved {
            pointer: String::from("/declarative/route/0/place_beacon/at/covering"),
            route_step: Some(0),
            site: Site::Covering,
            kind: None,
            feature_id: String::from("vent_120_88"),
            travel_ms: travel,
            candidates,
            matched: 1,
            failure: None,
            weighed: None,
            site_voxel: None,
        }
    }

    /// A named pick the origin cannot reach says so with `reachable: false`
    /// and writes no `travel_ms` at all, rather than the 0 a magic value once
    /// stood for (decisions-log item 133 (3) (i)); a reachable one writes its
    /// travel beside `reachable: true`, a 0 included.
    /// `compose_wall_box`'s floor is the lowest ground and its top stands
    /// over the highest, whatever the order the columns come in.
    #[test]
    fn a_wall_box_spans_the_lowest_and_the_highest_ground() {
        assert_eq!(super::ground_span(&[]), None);
        assert_eq!(super::ground_span(&[7]), Some((7, 7)));
        assert_eq!(super::ground_span(&[9, 4, 12, 4, 6]), Some((4, 12)));
        assert_eq!(super::ground_span(&[-3, 0]), Some((-3, 0)));
    }

    #[test]
    fn an_unreachable_pick_says_so() {
        let unreachable = resolved_json(&named(None, Vec::new())).expect("written");
        assert_eq!(unreachable.get("reachable"), Some(&Json::Bool(false)));
        assert!(
            unreachable.get("travel_ms").is_none(),
            "no travel is written for a pick nothing reaches: {unreachable:?}"
        );
        assert!(
            unreachable.get("travel_text").is_none(),
            "and no text for one (S3's `fixq`): {unreachable:?}"
        );
        assert_eq!(
            unreachable.get("feature_id"),
            Some(&Json::String(String::from("vent_120_88"))),
            "the name is still the pick"
        );

        let underfoot = resolved_json(&named(
            Some(Ms::ZERO),
            vec![Candidate {
                feature_id: String::from("vent_120_88"),
                travel_ms: Ms::ZERO,
            }],
        ))
        .expect("written");
        assert_eq!(underfoot.get("reachable"), Some(&Json::Bool(true)));
        assert_eq!(
            underfoot.get("travel_ms"),
            Some(&Json::Number(String::from("0"))),
            "a feature at the origin is reached in no time, and says so"
        );
        // S3's `fixq`, item 12: the figure as the player reads it, beside the
        // pick's travel and each candidate's.
        assert_eq!(
            underfoot.get("travel_text"),
            Some(&Json::String(String::from("0 s")))
        );
        let Some(Json::Array(candidates)) = underfoot.get("candidates") else {
            panic!("candidates: {underfoot:?}");
        };
        assert_eq!(
            candidates
                .first()
                .and_then(|candidate| candidate.get("travel_text")),
            Some(&Json::String(String::from("0 s")))
        );
    }
}
