// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Easy: the one difficulty the walking skeleton ships (spec section 14's
//! Easy column), as a built-in seat and as an advisor.
//!
//! [`Easy::play`] plays a seat nobody at this machine plays: it reads, scores,
//! composes, verifies and repairs, submits, and **ends by calling
//! `set_ready`** -- otherwise the lobby's Ready could never end a two-seat
//! Lull (T19 PR 1's finding 6, decisions-log item 112 (3)). When its own plan
//! fails its four repairs, or there is nothing worth doing, it submits its
//! own safe playbook.
//!
//! [`Easy::advise`] advises a seat a person plays: that seat's own safe
//! playbook, and one suggestion per template for the editor's wizard. It
//! writes nothing: the gateway's allow-list refuses an advisor every write
//! anyway, and the operator never tries one.
//!
//! # Easy's row, and what has no rules row
//!
//! Easy's evaluation units and thresholds are not tuning rows in
//! `rules/rules.v1.json` (the table has no operator block), so each is a
//! named constant below carrying a PLACEHOLDER that names its owner and stage
//! (AGENTS.md section 12). Everything the operator *scores* with -- costs,
//! kW ratings, yields, interface times, the sphere radius -- comes from the
//! rules text ([`crate::tuning`]).
//!
//! # The call budget, derived
//!
//! The budget is in **evaluation units** (spec section 14; decision C6), and
//! [`EASY_CALL_BUDGET`] is derived from them term by term rather than sized to
//! fit. Every call a round can make is in exactly one term:
//!
//! | term | calls |
//! |---|---|
//! | the fixed reads ([`FIXED_READS`]) | `get_status`, `get_briefing`, `get_economy_forecast`, `get_map_summary`, and at most [`PAGES_MAX`] pages each of `list_beacons`, `get_view` and `list_templates` |
//! | one estimate per candidate | [`EASY_CANDIDATES`] `estimate_route` |
//! | one instantiate per template | [`EASY_TEMPLATES`] `instantiate_template`, each with its own values, to read what it declares |
//! | the composition | [`COMPOSE_CALLS`]: one `patch_plan` filling the chosen template and inserting its goals |
//! | the verifies | 1 `verify_plan`, then [`EASY_REPAIRS`] repairs of one `patch_plan` and one `verify_plan` each |
//! | the safe playbook | [`SAFE_CALLS`]: at most [`SAFE_ESTIMATES`] `estimate_route`, one `instantiate_template` with the route, one `verify_plan` |
//! | the commit | [`COMMIT_CALLS`]: at most two `submit_plan` (its own plan, then the safe one if the first was refused) and one `set_ready` |
//!
//! An advisor makes the fixed reads, the estimates, the instantiates and the
//! safe playbook's calls, and nothing else ([`EASY_ADVISOR_CALL_BUDGET`]).
//! `easy_never_exceeds_its_derived_call_budget` asserts both on a worst-case
//! scripted round, and the hosted tests in `crates/gamectl` assert them on
//! every round they play.

use pharmakos_proto::json::Json;

use crate::candidates::{self, Candidate, Goal};
use crate::compose::{self, Composed};
use crate::playbook::{self, Declared};
use crate::safe::{self, SafePlan};
use crate::situation::Situation;
use crate::terrain::Feature;
use crate::tuning::{RulesError, Tuning};
use crate::wire::{Call, Wire, array_of, bool_of, compact, object, string, text_of};

// ---------------------------------------------------------------------------
// Easy's row (spec section 14), and the thresholds with no rules row
// ---------------------------------------------------------------------------

/// Easy's breadth: how many candidates are evaluated, one estimate each.
///
/// PLACEHOLDER: spec section 14's Easy column gives 30; it has no rules row
/// because the operator's rows are not in the table. Owner, at **S5**, with
/// Normal and Hard.
pub const EASY_CANDIDATES: u32 = 30;

/// Easy's k: how many of the candidates, best first by utility per second,
/// go into composition.
///
/// PLACEHOLDER: spec section 14's "top-3". Read as best-of-k with no draw
/// (decision C15). Owner, at **S5**.
pub const EASY_TOP_K: usize = 3;

/// How much of the segment Easy's route fills, per cent.
///
/// PLACEHOLDER: spec section 14's 70 %. Owner, at **S5**.
pub const EASY_ROUTE_FILL_PERCENT: i64 = 70;

/// How many times Easy repairs a plan that does not qualify, after its first
/// verify ("verify and repair up to 4 times").
///
/// PLACEHOLDER: spec section 14's 4, the same at every difficulty. Owner, at
/// **S5**.
pub const EASY_REPAIRS: u32 = 4;

/// How far an at-risk beacon may be for the safe playbook to raise it, game
/// milliseconds of travel from the commander.
///
/// PLACEHOLDER: spec section 14's "within 60 s travel", with "at risk"
/// itself undefined there. Owner, at **S1**, with the grid.
pub const SAFE_REACH_MS: i64 = 60_000;

/// How many beacons the safe playbook raises at most.
///
/// PLACEHOLDER: spec section 14's "up to 2". Owner, at **S1**, with the grid.
pub const SAFE_MAX_RAISED: usize = 2;

/// How many browned-out beacons the safe playbook estimates, nearest the
/// commander in the ground plane first: twice what it may raise, so the
/// nearest two by travel are found among the nearest four by distance.
///
/// PLACEHOLDER: a bound the spec does not state, needed so the safe
/// playbook's calls are a term of the budget. Owner, at **S1**, with the two
/// above.
pub const SAFE_ESTIMATES: u32 = 4;

/// How many pages of `list_beacons`, `get_view` and `list_templates` a round
/// reads at most. Each is one page today: the whole map's keyframe is one
/// 256 KiB page (201 558 bytes at the golden seed).
///
/// PLACEHOLDER: a bound on paging, with the gateway's page sizes. Owner, at
/// **hardening**, with the rate limits.
pub const PAGES_MAX: u32 = 4;

/// How far inside a sphere's radius a place must be for Easy to call it
/// inside: whole-voxel positions are floored, and the sim measures between
/// fixed-point positions.
///
/// PLACEHOLDER: a margin the spec does not state. Owner, at **S1**, when
/// placement legality is on the wire as an estimate.
pub const SPHERE_MARGIN_VOXELS: i64 = 2;

/// The templates Easy knows how to fill, in the order it instantiates them.
/// Which three ship is decision 10 (decisions-log item 81).
pub const EASY_KNOWN_TEMPLATES: [&str; 3] = [EXPAND_AND_MINE, HOLD_AND_BUILD, SAFE_PLAYBOOK];

/// How many templates Easy instantiates: one per template it knows.
pub const EASY_TEMPLATES: u32 = 3;

/// The Expand & Mine template's id in the library.
pub const EXPAND_AND_MINE: &str = "expand_and_mine";
/// The Hold & Build template's id in the library.
pub const HOLD_AND_BUILD: &str = "hold_and_build";
/// The Safe Playbook template's id in the library.
pub const SAFE_PLAYBOOK: &str = "safe_playbook";

// ---------------------------------------------------------------------------
// The Balanced weighting
// ---------------------------------------------------------------------------

/// The utility weights: defence + economy + expansion + capability + attack -
/// risk, each term multiplied by its weight.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Weights {
    /// Defence.
    pub defence: i64,
    /// Economy.
    pub economy: i64,
    /// Expansion.
    pub expansion: i64,
    /// Capability.
    pub capability: i64,
    /// Attack.
    pub attack: i64,
    /// Risk, subtracted.
    pub risk: i64,
}

/// The Balanced weighting, the only one v1 ships (spec section 14).
///
/// PLACEHOLDER: all ones, because the terms are already in one unit (points,
/// where a whole $ is a point). The Balanced weights are Tuning. Owner, at
/// **S5**.
pub const BALANCED: Weights = Weights {
    defence: 1,
    economy: 1,
    expansion: 1,
    capability: 1,
    attack: 1,
    risk: 1,
};

/// What a kW of supply is worth in points, so a Generator's output and a
/// grid left short can be scored against a seam's $.
///
/// PLACEHOLDER: a conversion the spec does not state. Owner, at **S5**, with
/// the Balanced weights.
pub const DOLLARS_PER_KW: i64 = 10;

/// What one more beacon, and so one more sphere, is worth in points.
///
/// PLACEHOLDER: Owner, at **S5**, with the Balanced weights.
pub const EXPANSION_POINTS_PER_BEACON: i64 = 100;

/// What one thing of another seat in sight near a goal costs in points.
///
/// PLACEHOLDER: Owner, at **S5**, with the Balanced weights.
pub const RISK_POINTS_PER_ENEMY: i64 = 100;

// ---------------------------------------------------------------------------
// The budget
// ---------------------------------------------------------------------------

/// The fixed reads: four single calls and three paged reads.
pub const FIXED_READS: u32 = 4 + 3 * PAGES_MAX;

/// The composition: one `patch_plan`.
pub const COMPOSE_CALLS: u32 = 1;

/// The verifies: one, and one `patch_plan` and one `verify_plan` per repair.
pub const VERIFY_CALLS: u32 = 1 + EASY_REPAIRS * 2;

/// The safe playbook: its estimates, one instantiate, one verify.
pub const SAFE_CALLS: u32 = SAFE_ESTIMATES + 1 + 1;

/// The commit: two submits at most, and `set_ready`.
pub const COMMIT_CALLS: u32 = 2 + 1;

/// The most calls a built-in seat's round makes, derived term by term (see
/// the module docs).
pub const EASY_CALL_BUDGET: u32 = FIXED_READS
    + EASY_CANDIDATES
    + EASY_TEMPLATES
    + COMPOSE_CALLS
    + VERIFY_CALLS
    + SAFE_CALLS
    + COMMIT_CALLS;

/// The most calls an advisor's round makes.
pub const EASY_ADVISOR_CALL_BUDGET: u32 =
    FIXED_READS + EASY_CANDIDATES + EASY_TEMPLATES + SAFE_CALLS;

// ---------------------------------------------------------------------------
// What comes back
// ---------------------------------------------------------------------------

/// What a built-in seat's round ended with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Submitted {
    /// Its own composed plan was accepted.
    Own,
    /// Its safe playbook was accepted.
    Safe,
    /// Nothing was accepted: the gateway files its fallback at `begin_push`.
    Nothing,
}

/// One built-in seat's round, as the operator saw it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Played {
    /// The round it planned, 0 when it could not read one.
    pub round: u32,
    /// What it submitted.
    pub submitted: Submitted,
    /// The playbook it submitted, if one was accepted.
    pub playbook_jsonc: Option<String>,
    /// How many repairs its own plan took.
    pub repairs: u32,
    /// True when `set_ready` answered without a refusal.
    pub ready: bool,
    /// How many calls the round made.
    pub calls: u32,
    /// Why, in one line.
    pub why: String,
}

/// One suggested value for one declared template parameter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SuggestedValue {
    /// The declared pointer.
    pub pointer: String,
    /// The JSON to put there, as compact text.
    pub value: String,
}

/// The operator's suggestion for one template.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Suggestion {
    /// The library's id for the template.
    pub template_id: String,
    /// The values it would fill, in declaration order. Empty when it has none
    /// to offer, and `why` says so.
    pub parameters: Vec<SuggestedValue>,
    /// Why, in one sentence.
    pub why: String,
}

/// What the operator advises one seat for one round.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Advice {
    /// The seat's own safe playbook, JSONC. Empty when the library has no
    /// Safe Playbook template, in which case the gateway's fallback stands.
    pub safe_playbook_jsonc: String,
    /// One suggestion per template the operator knows and the library lists.
    pub suggestions: Vec<Suggestion>,
    /// How many calls the advice made.
    pub calls: u32,
}

// ---------------------------------------------------------------------------
// Easy
// ---------------------------------------------------------------------------

/// The Easy operator for one seat.
///
/// One instance per seat and nothing shared: it holds the rules text's rows
/// and nothing else, so an instance carries no state from one seat or one
/// round to the next.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Easy {
    tuning: Tuning,
}

impl Easy {
    /// An Easy operator scoring with the rows of this rules text.
    ///
    /// # Errors
    ///
    /// [`RulesError`] when the text is not a rules table or lacks a row the
    /// operator scores with.
    pub fn new(rules_json: &str) -> Result<Easy, RulesError> {
        Ok(Easy {
            tuning: Tuning::read(rules_json)?,
        })
    }

    /// Play one seat's round: plan, submit, and say ready.
    pub fn play(&mut self, seat: u8, call: &mut Call<'_>) -> Played {
        let mut wire = Wire::new(call);
        let mut round = Played {
            round: 0,
            submitted: Submitted::Nothing,
            playbook_jsonc: None,
            repairs: 0,
            ready: false,
            calls: 0,
            why: String::new(),
        };
        if let Ok(situation) = Situation::read(&mut wire, seat) {
            round.round = situation.round;
            self.plan(&mut wire, &situation, &mut round);
        }
        // Always, whatever happened above: a built-in seat that never says it
        // is ready holds the Lull open until the timer ends.
        round.ready = wire
            .call("set_ready", object(vec![("ready", Json::Bool(true))]))
            .is_ok();
        round.calls = wire.calls();
        round
    }

    /// Plan and submit.
    fn plan(&self, wire: &mut Wire<'_, '_>, situation: &Situation, round: &mut Played) {
        let goals = candidates::enumerate(situation, &self.tuning);
        let evaluated = candidates::evaluate(wire, situation, &self.tuning, goals);
        let top = candidates::top_k(&evaluated);
        let declared = playbook::declare(wire, &situation.templates, &EASY_KNOWN_TEMPLATES);

        if let Some(composed) = compose::compose(situation, &top, &declared) {
            if let Some(text) = verified(wire, &declared, &composed, &mut round.repairs) {
                if submit(wire, &text) {
                    round.submitted = Submitted::Own;
                    round.why = composed
                        .goals
                        .first()
                        .map(|first| {
                            compose::why_for(
                                &first.goal,
                                first,
                                composed.goals.len(),
                                compose::fill_ms(situation),
                            )
                        })
                        .unwrap_or_default();
                    round.playbook_jsonc = Some(text);
                    return;
                }
            }
        }
        // Nothing worth doing, or a plan that would not qualify: the seat's
        // own safe playbook, submitted on its own token.
        if let Some(safe) = safe::safe_playbook(wire, situation, &declared) {
            if submit(wire, &safe.jsonc) {
                round.submitted = Submitted::Safe;
                round.why = compose::why_safe(situation, &safe.raised);
                round.playbook_jsonc = Some(safe.jsonc);
            }
        }
    }

    /// Advise one seat for the round its Lull has just opened.
    pub fn advise(&mut self, seat: u8, call: &mut Call<'_>) -> Advice {
        let mut wire = Wire::new(call);
        let mut advice = Advice::default();
        if let Ok(situation) = Situation::read(&mut wire, seat) {
            let goals = candidates::enumerate(&situation, &self.tuning);
            let evaluated = candidates::evaluate(&mut wire, &situation, &self.tuning, goals);
            let declared =
                playbook::declare(&mut wire, &situation.templates, &EASY_KNOWN_TEMPLATES);
            let safe = safe::safe_playbook(&mut wire, &situation, &declared);
            advice.suggestions = suggestions(&situation, &evaluated, &declared, safe.as_ref());
            advice.safe_playbook_jsonc = safe.map(|plan| plan.jsonc).unwrap_or_default();
        }
        advice.calls = wire.calls();
        advice
    }
}

/// Fill, verify FULL, and repair up to [`EASY_REPAIRS`] times.
///
/// Returns the qualifying text, or `None` when it never qualified, and counts
/// the repairs it spent into `repairs` either way. A repair is the verifier's own machine-applicable fix
/// for its first error when there is one; else taking out the last goal the
/// greedy insertion added; else there is nothing left to try.
fn verified(
    wire: &mut Wire<'_, '_>,
    declared: &[Declared],
    composed: &Composed,
    repairs: &mut u32,
) -> Option<String> {
    let base = declared
        .iter()
        .find(|held| held.template_id == composed.template_id)?;
    let mut text = patched(wire, &base.jsonc, &composed.patch)?;
    let mut removals = composed.removals.clone();
    loop {
        let answer = wire
            .call(
                "verify_plan",
                object(vec![
                    ("playbook_jsonc", string(&text)),
                    ("depth", string("full")),
                ]),
            )
            .ok()?;
        let report = answer.get("report")?;
        if bool_of(report, "qualifies") {
            return Some(text);
        }
        if *repairs >= EASY_REPAIRS {
            return None;
        }
        *repairs = repairs.saturating_add(1);
        let next = if let Some(json_patch) = machine_fix(report) {
            patched_text(wire, &text, &json_patch)
        } else {
            let last = removals.pop()?;
            patched(wire, &text, &last)
        };
        text = next?;
    }
}

/// The first error's first machine-applicable patch, as text.
fn machine_fix(report: &Json) -> Option<String> {
    for diagnostic in array_of(report, "diagnostics") {
        let severity = Wire::enum_name("gp.api.v1.Diagnostic.Severity", diagnostic.get("severity"));
        if severity.as_deref() != Some("ERROR") {
            continue;
        }
        for suggestion in array_of(diagnostic, "suggestions") {
            let applicability = Wire::enum_name(
                "gp.api.v1.PatchSuggestion.Applicability",
                suggestion.get("applicability"),
            );
            let patch = text_of(suggestion, "json_patch");
            if applicability.as_deref() == Some("MACHINE_APPLICABLE") && !patch.is_empty() {
                return Some(patch.to_owned());
            }
        }
    }
    None
}

/// Apply a patch of operations through `patch_plan`.
fn patched(wire: &mut Wire<'_, '_>, text: &str, ops: &[Json]) -> Option<String> {
    patched_text(wire, text, &compact(&Json::Array(ops.to_vec())))
}

/// Apply a JSON Patch text through `patch_plan`.
fn patched_text(wire: &mut Wire<'_, '_>, text: &str, json_patch: &str) -> Option<String> {
    let answer = wire
        .call(
            "patch_plan",
            object(vec![
                ("playbook_jsonc", string(text)),
                ("json_patch", string(json_patch)),
            ]),
        )
        .ok()?;
    let out = text_of(&answer, "playbook_jsonc");
    (!out.is_empty()).then(|| out.to_owned())
}

/// Submit, and say whether it was accepted.
fn submit(wire: &mut Wire<'_, '_>, text: &str) -> bool {
    wire.call(
        "submit_plan",
        object(vec![("playbook_jsonc", string(text))]),
    )
    .is_ok_and(|answer| bool_of(&answer, "accepted"))
}

/// One suggestion per template Easy knows and the library lists, in
/// declaration order: own beacons and fixed targets only.
fn suggestions(
    situation: &Situation,
    evaluated: &[Candidate],
    declared: &[Declared],
    safe: Option<&SafePlan>,
) -> Vec<Suggestion> {
    let fill = compose::fill_ms(situation);
    let mut out: Vec<Suggestion> = Vec::new();
    for template in declared {
        let mut parameters: Vec<SuggestedValue> = Vec::new();
        let mut put = |pointer: Option<&str>, value: String| {
            if let Some(pointer) = pointer {
                parameters.push(SuggestedValue {
                    pointer: pointer.to_owned(),
                    value,
                });
            }
        };
        let why = match template.template_id.as_str() {
            EXPAND_AND_MINE => match candidates::best_for(evaluated, EXPAND_AND_MINE) {
                Some(best) => {
                    if let Goal::Mine { site, .. } = best.goal {
                        put(
                            template.pointer_ending("/move/to"),
                            compose::location_text(site),
                        );
                        put(
                            template.pointer_ending("/place_beacon/at"),
                            compose::location_text(site),
                        );
                    }
                    compose::why_for(&best.goal, best, 1, fill)
                }
                None => compose::why_none(Feature::Seam),
            },
            HOLD_AND_BUILD => match candidates::best_for(evaluated, HOLD_AND_BUILD) {
                Some(best) => {
                    if let Goal::Generator { anchor, .. } = best.goal {
                        put(
                            template.pointer_ending("/anchor/voxel"),
                            compose::voxel_text(anchor),
                        );
                    }
                    if let Some(hold) = compose::hold_left(best, fill) {
                        put(template.pointer_ending("/hold/ms"), hold.to_string());
                    }
                    compose::why_for(&best.goal, best, 1, fill)
                }
                None => compose::why_none(Feature::Vent),
            },
            SAFE_PLAYBOOK => {
                let raised: &[String] = safe.map_or(&[], |plan| plan.raised.as_slice());
                if let Some(plan) = safe.filter(|plan| !plan.raised.is_empty()) {
                    put(
                        template.pointer_ending("/declarative/route"),
                        plan.route.clone(),
                    );
                }
                compose::why_safe(situation, raised)
            }
            _ => continue,
        };
        out.push(Suggestion {
            template_id: template.template_id.clone(),
            parameters,
            why,
        });
    }
    out
}
