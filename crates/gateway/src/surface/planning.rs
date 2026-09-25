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
//! # `submit_plan` always runs FULL, and an invalid playbook is not an error
//!
//! Spec section 12 and decisions-log item 82: `verify_plan{depth}` and
//! `report_hash` ship complete, FULL's estimate and lint stages are present and
//! empty, and submit runs FULL. So a FULL pre-check and the check at submit
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
//! [`compile_playbook`] is where the second door is, and it is **at submit**
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
use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::rules::RulesTable;

use crate::error::Error;
use crate::rpc::Request;
use crate::surface::{Draft, Sealed, Surface};

/// The longest label a seat may hang on a draft.
///
/// PLACEHOLDER: 120 characters is a working number, chosen so a label fits one
/// line of the editor's draft list. OWNER settles it with the editor at **S6**.
/// It is a **character** count and not a byte count, for the same reason the
/// notebook's is (spec section 12 says 4,000 characters).
pub const MAX_DRAFT_LABEL_CHARS: usize = 120;

/// The most drafts one seat may hold at once.
///
/// PLACEHOLDER: 32 is a working number. It exists because a draft is stored in
/// the private match cache and an unbounded store is a way for one
/// authenticated seat to fill a disk (AGENTS.md section 7's audit-log argument,
/// applied to the thing beside it). OWNER settles it at hardening with the rate
/// limits.
pub const MAX_DRAFTS: usize = 32;

/// The longest playbook text a method will take, in characters.
///
/// PLACEHOLDER: 512 KiB of characters is far above item 94's 128-unit size
/// budget -- the worked example is 6 units and a few hundred bytes -- and far
/// below anything that costs the process memory. It is a transport bound and
/// not a game rule, so it is not a rules-table row, for the same reason
/// [`crate::limit::CALLS_PER_TICK`] is not. OWNER settles it at hardening.
pub const MAX_PLAYBOOK_CHARS: usize = 512 * 1024;

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

    /// `list_templates`: the local library, read and never stored.
    ///
    /// Spec section 13: "The gateway reads that folder to list and instantiate
    /// templates, but stores none of it". Every call re-reads the folder, which
    /// is what "stores none of it" means in code, and nothing in it executes.
    ///
    /// A host with **no** library configured lists nothing rather than failing:
    /// where the folder lives on each platform is the owner's, with packaging
    /// at T21 (the plan's own T13 PLACEHOLDER), and a wizard that could not
    /// open because a path was unset would be worse than one with no templates
    /// in it.
    pub(super) fn list_templates(&self, request: &Request) -> Result<Json, Error> {
        let tag = request.string_param("tag")?.unwrap_or_default();
        let Some(folder) = self.host()?.library() else {
            return Ok(Json::Object(vec![
                (String::from("templates"), Json::Array(Vec::new())),
                (String::from("next_cursor"), Json::String(String::new())),
            ]));
        };
        let found = pharmakos_plan_core::library::list(folder).map_err(|error| {
            Error::internal(format!(
                "the template folder could not be read: {}",
                error.message
            ))
        })?;
        let templates: Vec<Json> = found
            .iter()
            // A worked playbook in the library is a sample and not a template,
            // and the wizard does not offer it (`plan-core`'s `Summary::kind`).
            .filter(|summary| summary.kind == pharmakos_proto::gp::v1::playbook::Kind::Template)
            .filter(|summary| tag.is_empty() || summary.tags.iter().any(|held| held == tag))
            .map(|summary| {
                Json::Object(vec![
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
                ])
            })
            .collect();
        Ok(Json::Object(vec![
            (String::from("templates"), Json::Array(templates)),
            // The listing is complete: a folder of templates is a folder, and
            // paging one would be pagination for its own sake.
            (String::from("next_cursor"), Json::String(String::new())),
        ]))
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
        let text = pharmakos_plan_core::library::read(folder, template_id).map_err(|_| {
            Error::not_found(format!(
                "no template `{template_id}` in this gateway's library"
            ))
        })?;

        let mut parameters: Vec<pharmakos_plan_core::library::Parameter> = Vec::new();
        if let Some(value) = request.param("parameters") {
            let Json::Array(items) = value else {
                return Err(Error::invalid("`parameters` is an array of {name, value}"));
            };
            for (index, item) in items.iter().enumerate() {
                let name = match item.get("name") {
                    Some(Json::String(name)) => name.clone(),
                    _ => {
                        return Err(Error::invalid(format!("parameter {index} has no `name`")));
                    }
                };
                let value = match item.get("value") {
                    Some(Json::String(value)) => value.clone(),
                    _ => {
                        return Err(Error::invalid(format!("parameter {index} has no `value`")));
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
            .map_err(|error| Error::invalid(error.message))?;
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
        .map_err(|error| Error::internal(error.message))?;

        // A file with no canonical form has no rendering. That is not an error
        // -- the editor asks for a rendering on every keystroke and a half-typed
        // file is the normal case -- so it comes back as the sentence that says
        // to verify it, and `verify_plan` is where the diagnostics live.
        let prose = match pharmakos_plan_core::canonicalise_text(playbook) {
            Err(_no_canonical_form) => String::from(crate::strings::UNRENDERABLE),
            Ok(canonical) => {
                pharmakos_plan_core::render_plan(&canonical.playbook, &context, host.rules())
                    .map_err(|error| Error::internal(error.message))?
            }
        };
        Ok(Json::Object(vec![(
            String::from("prose"),
            Json::String(prose),
        )]))
    }

    /// `patch_plan`: apply an RFC 6902 patch, comments and formatting intact.
    ///
    /// Returns the inverse patch as well, which is the editor's undo stack
    /// (spec section 13, "Edits are JSON Patches with inverse-patch undo").
    /// Takes no seat: a patch is text in and text out and touches no snapshot,
    /// so there is nothing seat-shaped to check beyond the scope the method
    /// already carries.
    pub(super) fn patch_plan(request: &Request) -> Result<Json, Error> {
        let playbook = playbook_param(request)?;
        let patch = request
            .string_param("json_patch")?
            .ok_or_else(|| Error::invalid("`json_patch` is an RFC 6902 patch, as text"))?;
        let (patched, inverse) = pharmakos_plan_core::patch_text(playbook, patch)
            .map_err(|error| Error::invalid(error.message))?;
        Ok(Json::Object(vec![
            (String::from("playbook_jsonc"), Json::String(patched)),
            (String::from("inverse_json_patch"), Json::String(inverse)),
        ]))
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
        let label = request
            .string_param("label")?
            .unwrap_or_default()
            .to_owned();
        if label.chars().count() > MAX_DRAFT_LABEL_CHARS {
            return Err(Error::invalid(format!(
                "a draft's label is at most {MAX_DRAFT_LABEL_CHARS} characters"
            )));
        }
        let round = self.host()?.runner().round();
        let draft_id = request.string_param("draft_id")?.map_or_else(
            || format!("d{round}-{}", self.next_draft_number(seat, round)),
            str::to_owned,
        );
        if !is_draft_id(&draft_id) {
            return Err(Error::invalid(format!(
                "`{draft_id}` is not a draft id: lower-case ASCII letters, digits, `-` and `_`, \
                 starting with a letter"
            )));
        }

        let state = self.seat_state_mut(subject, seat)?;
        let replacing = state.draft(&draft_id).is_some();
        if !replacing && state.drafts.len() >= MAX_DRAFTS {
            return Err(Error::invalid(format!(
                "this seat holds {MAX_DRAFTS} drafts, which is as many as it may; delete one by \
                 saving over it"
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
    /// gateway's fallback, [`crate::host::SAFE_PLAYBOOK`] — which is exactly
    /// what [`Surface::begin_push`] would file for it, so what this answers
    /// and what a timeout costs are the same playbook.
    pub(super) fn get_safe_plan(&self, subject: crate::token::Subject) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "a safe playbook")?;
        let own = self
            .current_advice(seat)
            .and_then(|advised| advised.safe.as_ref())
            .map(|safe| safe.playbook_jsonc.clone());
        let playbook = match own {
            Some(playbook) => playbook,
            None => self.host()?.safe_playbook().to_owned(),
        };
        Ok(Json::Object(vec![(
            String::from("playbook_jsonc"),
            Json::String(playbook),
        )]))
    }

    /// `submit_plan`: seal it.
    ///
    /// Always FULL. A report with `qualifies: false` comes back with
    /// `accepted: false` and the seat's previous seal untouched -- a failed
    /// submission must not unseal what was already sealed. A report that
    /// qualifies **replaces** the previous seal, any number of times until the
    /// timer ends (decisions-log item 5).
    pub(super) fn submit_plan(
        &mut self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "a playbook to submit")?;
        let playbook = playbook_param(request)?.to_owned();
        let round = self.host()?.runner().round();
        let report = self.verify_for(seat, &playbook, Depth::Full)?;
        let accepted = report.qualifies;
        if accepted {
            // The second door. A refusal here leaves the previous seal exactly
            // where it is -- `self.seat_state_mut` is not reached at all -- and
            // comes back as a method error rather than as a report, because the
            // verifier has already said this playbook may be sealed and saying
            // `qualifies: false` on the same breath would be the gateway
            // contradicting a hash it has just handed out.
            let plan = compile_playbook(&playbook, self.host()?.rules())?;
            let sealed = Sealed {
                playbook_jsonc: playbook,
                report_hash: report.report_hash.clone(),
                round,
                filed_by_the_gateway: false,
                plan,
            };
            self.seat_state_mut(subject, seat)?.sealed = Some(sealed);
        }
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
    fn next_draft_number(&self, seat: pharmakos_sim::tables::SeatId, round: u32) -> usize {
        let Ok(state) = self.seat_state(crate::token::Subject::Seat(seat), seat) else {
            return 1;
        };
        // Bounded by MAX_DRAFTS + 1 by construction: at most MAX_DRAFTS ids are
        // held, so one of the first MAX_DRAFTS + 1 candidates is free.
        (1..=MAX_DRAFTS.saturating_add(1))
            .find(|number| state.draft(&format!("d{round}-{number}")).is_none())
            .unwrap_or(1)
    }
}

/// The sim's door: a JSONC playbook to a compiled [`Plan`], or the refusal a
/// caller is told.
///
/// The one path from a submitted file to something the interpreter can run, so
/// `submit_plan` and [`crate::surface::Surface`]'s filing of the safe playbook
/// cannot compile it two different ways. The decode is `plan-core`'s canonical
/// form, which is the same decode the verifier's report was taken over.
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
/// [`crate::error::Code::InvalidArgument`] for a [`PlanError`], and
/// [`crate::error::Code::Internal`] when the text has no canonical form at all
/// — which for a submission cannot happen, because the report that qualified it
/// was taken over that same canonical form, and so would mean the two had
/// drifted apart.
pub(crate) fn compile_playbook(playbook_jsonc: &str, rules: &RulesTable) -> Result<Plan, Error> {
    let canonical = pharmakos_plan_core::canonicalise_text(playbook_jsonc).map_err(|error| {
        Error::internal(format!(
            "a playbook the verifier read has no canonical form: {}",
            error.message
        ))
    })?;
    Plan::compile(&canonical.playbook, rules).map_err(|error| {
        Error::invalid(format!(
            "this playbook qualifies and this build cannot execute it: {error}"
        ))
    })
}

/// The `playbook_jsonc` parameter, bounded.
fn playbook_param(request: &Request) -> Result<&str, Error> {
    let text = request
        .string_param("playbook_jsonc")?
        .ok_or_else(|| Error::invalid("`playbook_jsonc` is the playbook, as JSONC text"))?;
    if text.chars().count() > MAX_PLAYBOOK_CHARS {
        return Err(Error::invalid(format!(
            "a playbook is at most {MAX_PLAYBOOK_CHARS} characters and this is {}",
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
        Some(other) => Err(Error::invalid(format!(
            "`depth` is `quick` or `full`, and this is `{other}`"
        ))),
    }
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
fn is_draft_id(text: &str) -> bool {
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
