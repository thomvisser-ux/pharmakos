// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `render_plan`: the playbook read back as English.
//!
//! Spec section 11 makes the plain-language rendering one of the three places
//! user-facing strings live, and spec section 13 puts it in front of the
//! player at submit. `gamectl` prints it, the editor shows it, and the
//! accessible names in the editor are generated from these sentences.
//!
//! # Deterministic, and a function of the file
//!
//! The prose is a pure function of the playbook, the rules table and the
//! seat's frozen snapshot. No wall clock, no iteration over a hash container,
//! no float — the same file renders the same bytes on Windows, Linux and
//! macOS, which is what lets `expected.prose.txt` be a golden at all.
//!
//! # What it may say
//!
//! Decisions-log item 97, on what wave 3 owes the scenario vocabulary:
//!
//! > T8 claims nothing in `render_plan` that a scenario would have to assert
//! > differently once they exist.
//!
//! So the rendering says three kinds of thing and no fourth:
//!
//! 1. **What the file says** — the steps, the guards, the rules, the blocks.
//!    A scenario asserting on a playbook's content asserts the same words.
//!    One narrowing, stated rather than left to be noticed: a
//!    `set_mandate_settings` row is rendered as *that* — the row is named, its
//!    individual settings are not listed. Each setting's player-facing wording
//!    belongs to the mandate contract, which is T14's and S1's, and inventing
//!    five vocabularies here would be exactly the "don't invent rules" AGENTS.md
//!    section 12 forbids. The interface time the row costs *is* stated, because
//!    that is arithmetic over the rate card. The settings themselves arrive
//!    with the mandates.
//! 2. **Interface-time arithmetic** — exact, from `gp.v1.RulesTable`'s
//!    section-5 rate card. A scenario asserting how long a visit took asserts
//!    the same number, because it is the same row.
//! 3. **The size meter** — `pharmakos_verifier::size_units` against the
//!    rules-table budget, so the editor's meter and the verifier's `E0110`
//!    count one thing (decisions-log item 94).
//!
//! It does **not** say how long the commander will take to walk anywhere, when
//! it will arrive, what the opponent will do, or how long the segment is. The
//! first is [`crate::travel`]'s, rendered beside the route as a bound the
//! editor draws. The last *is* in the frozen snapshot, since T10
//! ([`crate::context::segment_length_ms`]), and the prose still leaves it out:
//! whether the route fits the coming segment is the verifier's estimate stage's
//! to say (`W0701` and `I0001`, S1's plan's task `proj`), and a sentence here
//! about the segment would be a claim beyond the file, the rate card and the
//! meter, which item 97 rules out.
//!
//! # Targeting's descriptions read as what they are
//!
//! A `covering` or `on` site (docs/design/targeting.md) is rendered as the text
//! the author wrote — "a site covering the nearest heat vent you do not cover
//! yet" — never as what it would resolve to, because the sealed file holds the
//! text and a carried playbook reads it again every round. A name reads as the
//! feature it names, "Heat vent (120, 88)", from the name alone. Nothing here
//! ranks.
//!
//! # The chips (S3's plan, E8)
//!
//! [`render`] answers the prose and, beside it, each route step and each rule
//! with the lines of the prose that are its own and its **chips**: for each
//! editable value the sentence shows, its JSON Pointer into the playbook, the
//! span of its line it occupies (in Unicode code points), what may be written
//! there ([`crate::shape::Shape`]) and its value now, as JSON text in the
//! canonical spelling. The prose is exactly [`render_plan`]'s: a chip only
//! marks where a value already stands, so the prose goldens do not move, and
//! the chips have a golden of their own (`expected.chips.txt`).
//!
//! **A chip is a value the file writes.** Its pointer is the path of a JSON
//! Patch `replace`, which needs a member to replace, and a proto3 value at its
//! default may be absent from the file: so a value is chipped when it is not
//! at its default, or when its field carries presence (an `optional` field, a
//! message), and never otherwise. A value the prose does not show (a step's
//! label, a rule's id, a note) is no chip, and neither is a list, a wall's
//! box, an Attack's staging voxel or a selector's filter, which no one
//! [`crate::shape::Shape`] describes. The tail blocks (the death limit, the
//! fallback, the options) are no rule or step, so they carry none.
//!
//! Every chip is built where its words are written, through [`Text`]: a span
//! is counted, never searched for, so a value written twice in one sentence
//! is two chips in their own places.

use pharmakos_proto::gp::v1::feature_ref::{self, Coverage, Rank};
use pharmakos_proto::gp::v1::{
    AttackSettings, BeaconFilter, BeaconRef, Condition, DefendSettings, Fallback, FeatureRef,
    Freshness, Handler, InterfaceRow, Location, MandateSettings, NearPlace, OnDeath, Options,
    Playbook, Step, Voxel, attack_settings, attack_target, beacon_filter, beacon_ref,
    beacon_units_by_role, cmdr_hp_pct, condition, defend_settings, enemy_structure_near, fallback,
    freshness, handler, int_compare, interface_row, location, mandate_settings, meta, near_place,
    on_death, on_fail, playbook, protected_asset, step,
};

use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::rules::RulesTable;
use pharmakos_verifier::{Limits, size_units};

use crate::context::PlanContext;
use crate::error::{Error, push_pointer};
use crate::interface::{self, Rates};
use crate::shape::{
    ANCHOR_ARMS, BEACON_ARMS, NEAR_ARMS, ReferenceKind, Shape, UNIT_COUNT, UNIT_DOLLARS, UNIT_KW,
    UNIT_VOXELS,
};
use crate::strings as s;

/// The prose and, beside it, each rule and route step with its chips.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rendering {
    /// [`render_plan`]'s prose, unchanged.
    pub prose: String,
    /// Each route step, then each rule, in file order
    /// (`gp.api.v1.RenderPlanResponse.rules`).
    pub rules: Vec<RenderedRule>,
}

/// One route step or rule of a [`Rendering`] (`gp.api.v1.RenderedRule`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RenderedRule {
    /// Its JSON Pointer: `/declarative/route/0`, `/declarative/handlers/2`.
    pub pointer: String,
    /// The zero-based line of the prose that is its sentence.
    pub line: u32,
    /// How many lines it takes, its sentence and what is indented under it.
    pub line_count: u32,
    /// Its chips, in the order they stand in the prose.
    pub chips: Vec<Chip>,
}

/// One editable value in the prose (`gp.api.v1.Chip`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Chip {
    /// The value's JSON Pointer in the playbook: a `replace`'s path.
    pub pointer: String,
    /// The zero-based line of the prose it stands on.
    pub line: u32,
    /// Its first code point on that line.
    pub start: u32,
    /// One past its last code point on that line.
    pub end: u32,
    /// What may be written there.
    pub shape: Shape,
    /// The value now, as JSON text in the canonical spelling.
    pub value_json: String,
}

/// Renders a playbook as English prose.
///
/// # Errors
///
/// As [`render`].
pub fn render_plan(
    playbook: &Playbook,
    context: &PlanContext,
    rules: &RulesTable,
) -> Result<String, Error> {
    render(playbook, context, rules).map(|rendering| rendering.prose)
}

/// Renders a playbook as English prose, with each rule's and step's chips.
///
/// # Errors
///
/// When the rules table is missing a block the rendering reads: the
/// section-5 interface rates, or the verifier's size budget. A rendering that
/// silently priced a visit at nothing, or showed a budget of zero, would be
/// worse than no rendering. And, for the chips, a value whose canonical JSON
/// cannot be written, an enum the schema does not declare, or a span past a
/// `u32`: each is this crate disagreeing with the schema or the wire, never
/// a playbook's fault.
pub fn render(
    playbook: &Playbook,
    context: &PlanContext,
    rules: &RulesTable,
) -> Result<Rendering, Error> {
    let rates = Rates::from_rules(rules)?;
    let limits = Limits::from_rules(rules)
        .map_err(|gap| Error::at("", format!("the rules table is incomplete: {gap}")))?;

    let mut out = Out::default();
    // `context` is the frozen snapshot's half of the rendering. Nothing in the
    // prose reads it yet. The one fact the prose would take from it, the
    // coming segment's length, has been in the snapshot since T10
    // (`crate::context::segment_length_ms`); whether the route fits it is the
    // verifier's estimate stage's to say (S1's `proj`), so the prose does not
    // state it (the module doc says why). The argument stays so that a fact
    // the prose does come to read is a line in this function rather than a
    // signature change in every caller.
    let _ = context.segment_ms();
    let ctx = Ctx {
        rates: &rates,
        limits: &limits,
    };
    envelope(playbook, &mut out);
    let mut pending = route(playbook, &ctx, &mut out);
    pending.extend(rules_section(playbook, &ctx, &mut out));
    blocks(playbook, &mut out);
    budget(playbook, &rates, limits.size_budget_units(), &mut out);
    if let Some(why) = out.fault {
        return Err(Error::at("", why));
    }
    if let Some(stray) = out.marks.first() {
        return Err(Error::at(
            stray.pointer.clone(),
            "the rendering placed a chip outside every rule and step",
        ));
    }
    let rules = pending
        .into_iter()
        .map(Pending::finish)
        .collect::<Result<Vec<RenderedRule>, Error>>()?;
    Ok(Rendering {
        prose: out.prose,
        rules,
    })
}

/// What the sentences read besides the playbook.
struct Ctx<'a> {
    rates: &'a Rates,
    limits: &'a Limits,
}

// ---------------------------------------------------------------------------
// Text with chips
// ---------------------------------------------------------------------------

/// How a chip's shape is written down where the chip is made: ready, or an
/// enum the schema lists, read when the rendering is finished.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Spec {
    Ready(Shape),
    Choice(&'static str),
}

/// A chip's value, written as JSON text when the rendering is finished.
#[derive(Clone, PartialEq, Debug)]
enum Value {
    /// Already JSON text: a number, an enum name in quotes, `true`, a string.
    Json(String),
    Location(Location),
    Near(NearPlace),
    Beacon(BeaconRef),
}

impl Value {
    fn int(value: impl core::fmt::Display) -> Value {
        Value::Json(value.to_string())
    }

    fn name(name: &str) -> Value {
        Value::Json(format!("\"{name}\""))
    }

    fn text(text: &str) -> Value {
        Value::Json(crate::library::compact(
            &pharmakos_proto::json::Json::String(text.to_owned()),
        ))
    }

    /// The value as compact canonical JSON text, one line.
    fn json(&self) -> Result<String, pharmakos_proto::json::Error> {
        let message = match self {
            Value::Json(text) => return Ok(text.clone()),
            Value::Location(place) => pharmakos_proto::json::encode_json(place)?,
            Value::Near(place) => pharmakos_proto::json::encode_json(place)?,
            Value::Beacon(reference) => pharmakos_proto::json::encode_json(reference)?,
        };
        Ok(crate::library::compact(&message))
    }
}

/// One chip inside a [`Text`], by code point.
#[derive(Clone, PartialEq, Debug)]
struct Mark {
    start: usize,
    end: usize,
    pointer: String,
    spec: Spec,
    value: Value,
}

/// Words, with the chips among them.
///
/// `fault` is the first count this text could not hold (a character count or
/// a chip's offset past `usize`, a rules-table bound past a chip's `int32`):
/// the pieces are put without a `Result`, so the fault travels with the words
/// and [`render`] refuses the rendering with it (E22), rather than a count
/// that saturated and a span that reads as true.
#[derive(Clone, Default, PartialEq, Debug)]
struct Text {
    words: String,
    chars: usize,
    marks: Vec<Mark>,
    fault: Option<&'static str>,
}

impl Text {
    fn push(&mut self, piece: impl Piece) {
        piece.put(self);
    }

    fn push_str(&mut self, text: &str) {
        self.words.push_str(text);
        match self.chars.checked_add(text.chars().count()) {
            Some(chars) => self.chars = chars,
            None => self.fail(s::RENDER_COUNT_PAST_RANGE),
        }
    }

    /// Keeps the first fault.
    fn fail(&mut self, why: &'static str) {
        self.fault.get_or_insert(why);
    }
}

/// Something a [`Text`] can be built from.
trait Piece {
    fn put(self, text: &mut Text);
}

impl Piece for &str {
    fn put(self, text: &mut Text) {
        text.push_str(self);
    }
}

impl Piece for String {
    fn put(self, text: &mut Text) {
        text.push_str(&self);
    }
}

impl Piece for &String {
    fn put(self, text: &mut Text) {
        text.push_str(self);
    }
}

impl Piece for Text {
    fn put(self, text: &mut Text) {
        let offset = text.chars;
        if let Some(why) = self.fault {
            text.fail(why);
        }
        for mark in self.marks {
            match (mark.start.checked_add(offset), mark.end.checked_add(offset)) {
                (Some(start), Some(end)) => text.marks.push(Mark { start, end, ..mark }),
                _ => text.fail(s::RENDER_COUNT_PAST_RANGE),
            }
        }
        text.push_str(&self.words);
    }
}

/// A [`Text`] of its pieces, in order.
macro_rules! cat {
    ($($piece:expr),* $(,)?) => {{
        let mut text = Text::default();
        $( text.push($piece); )*
        text
    }};
}

/// `shown`, as one chip.
fn chip(shown: &str, pointer: String, spec: Spec, value: Value) -> Text {
    let chars = shown.chars().count();
    Text {
        words: shown.to_owned(),
        chars,
        marks: vec![Mark {
            start: 0,
            end: chars,
            pointer,
            spec,
            value,
        }],
        fault: None,
    }
}

/// A child of a pointer: a field name, or an index.
fn at(base: &str, token: impl core::fmt::Display) -> String {
    push_pointer(base, &token.to_string())
}

/// The prose as it is written, line by line, with the chips placed on it,
/// and the first fault of any text written onto it ([`Text`]'s).
#[derive(Default)]
struct Out {
    prose: String,
    line: u32,
    marks: Vec<Placed>,
    fault: Option<&'static str>,
}

impl Out {
    /// Keeps the first fault.
    fn fail(&mut self, why: &'static str) {
        self.fault.get_or_insert(why);
    }

    /// Moves to the next line.
    fn next_line(&mut self) {
        match self.line.checked_add(1) {
            Some(line) => self.line = line,
            None => self.fail(s::RENDER_COUNT_PAST_RANGE),
        }
    }

    /// A list position, counted from 1.
    fn ordinal(&mut self, index: usize) -> usize {
        // On a fault the number is never read: [`render`] refuses the
        // rendering with it.
        let Some(number) = index.checked_add(1) else {
            self.fail(s::RENDER_COUNT_PAST_RANGE);
            return index;
        };
        number
    }
}

/// A chip placed on a line of the prose.
struct Placed {
    line: u32,
    start: usize,
    end: usize,
    pointer: String,
    spec: Spec,
    value: Value,
}

/// A rule or step whose chips are placed and not yet written.
struct Pending {
    pointer: String,
    line: u32,
    line_count: u32,
    marks: Vec<Placed>,
}

impl Pending {
    /// Opens a rule or step at the line the prose is on.
    fn open(pointer: String, out: &Out) -> (Pending, usize) {
        (
            Pending {
                pointer,
                line: out.line,
                line_count: 0,
                marks: Vec::new(),
            },
            out.marks.len(),
        )
    }

    /// Closes it: every line since it opened, and every chip placed on them.
    fn close(mut self, from: usize, out: &mut Out) -> Pending {
        match out.line.checked_sub(self.line) {
            Some(count) => self.line_count = count,
            None => out.fail(s::RENDER_COUNT_PAST_RANGE),
        }
        self.marks = out.marks.split_off(from);
        self
    }

    fn finish(self) -> Result<RenderedRule, Error> {
        let pointer = self.pointer;
        let chips = self
            .marks
            .into_iter()
            .map(|placed| {
                let span = |at: usize| {
                    u32::try_from(at).map_err(|_| {
                        Error::at(placed.pointer.clone(), "a chip's span is past a u32")
                    })
                };
                let shape = match placed.spec {
                    Spec::Ready(shape) => shape,
                    Spec::Choice(name) => Shape::choice(name)
                        .map_err(|error| Error::at(placed.pointer.clone(), error.to_string()))?,
                };
                Ok(Chip {
                    line: placed.line,
                    start: span(placed.start)?,
                    end: span(placed.end)?,
                    shape,
                    value_json: placed.value.json()?,
                    pointer: placed.pointer,
                })
            })
            .collect::<Result<Vec<Chip>, Error>>()?;
        Ok(RenderedRule {
            pointer,
            line: self.line,
            line_count: self.line_count,
            chips,
        })
    }
}

// ---------------------------------------------------------------------------
// The envelope
// ---------------------------------------------------------------------------

fn envelope(book: &Playbook, out: &mut Out) {
    let meta = book.meta.as_ref();
    let title = meta
        .map(|meta| meta.title.trim())
        .filter(|title| !title.is_empty())
        .unwrap_or(s::UNTITLED);
    line(0, cat!(title), out);
    line(
        0,
        cat!(match meta.map(pharmakos_proto::gp::v1::Meta::author_kind) {
            Some(meta::AuthorKind::Human) => s::AUTHOR_HUMAN,
            Some(meta::AuthorKind::Builtin) => s::AUTHOR_BUILTIN,
            Some(meta::AuthorKind::Script) => s::AUTHOR_SCRIPT,
            Some(meta::AuthorKind::Unspecified) | None => s::AUTHOR_UNKNOWN,
        }),
        out,
    );
    match book.kind() {
        playbook::Kind::Template => line(0, cat!(s::KIND_TEMPLATE), out),
        playbook::Kind::Sample => line(0, cat!(s::KIND_SAMPLE), out),
        playbook::Kind::Playbook | playbook::Kind::Unspecified => {}
    }
    if let Some(note) = meta
        .map(|meta| meta.note.trim())
        .filter(|note| !note.is_empty())
    {
        for sentence in note.lines() {
            line(0, cat!(sentence.trim()), out);
        }
    }
}

// ---------------------------------------------------------------------------
// The route
// ---------------------------------------------------------------------------

fn route(book: &Playbook, ctx: &Ctx<'_>, out: &mut Out) -> Vec<Pending> {
    let mut pending = Vec::new();
    let Some(body) = book.declarative.as_ref() else {
        return pending;
    };
    if body.route.is_empty() {
        return pending;
    }
    heading(s::HEADING_ROUTE, out);
    for (index, entry) in body.route.iter().enumerate() {
        let here = at("/declarative/route", index);
        let (open, from) = Pending::open(here.clone(), out);
        let number = out.ordinal(index);
        line(
            1,
            cat!(number.to_string(), ". ", step_sentence(entry, &here, ctx)),
            out,
        );
        for guard in guards(entry, &here) {
            line(2, guard, out);
        }
        pending.push(open.close(from, out));
    }
    pending
}

fn step_sentence(entry: &Step, here: &str, ctx: &Ctx<'_>) -> Text {
    let label = entry.label.trim();
    let body = step_body(entry, here, ctx);
    if label.is_empty() {
        cat!(body, ".")
    } else {
        cat!(label, ": ", body, ".")
    }
}

fn step_body(entry: &Step, here: &str, ctx: &Ctx<'_>) -> Text {
    match entry.kind.as_ref() {
        None => cat!(s::STEP_NOTHING),
        Some(step::Kind::Move(walk)) => {
            let pace = match walk.pace() {
                pharmakos_proto::gp::v1::move_step::Pace::Direct => s::PACE_DIRECT,
                pharmakos_proto::gp::v1::move_step::Pace::AvoidKnownThreats => s::PACE_AVOID,
                pharmakos_proto::gp::v1::move_step::Pace::Unspecified => "",
            };
            let walk_at = at(here, "move");
            cat!(
                s::STEP_MOVE,
                " ",
                location_chip(walk.to.as_ref(), at(&walk_at, "to"), &NEAR_ARMS),
                pace
            )
        }
        Some(step::Kind::Interface(visit)) => {
            let visit_at = at(here, "interface");
            let rows_at = at(&visit_at, "rows");
            let rows: Vec<Text> = visit
                .rows
                .iter()
                .enumerate()
                .map(|(index, row)| row_sentence(row, &at(&rows_at, index)))
                .collect();
            let spent = interface::interface_ms(visit, ctx.rates).total;
            let what = if rows.is_empty() {
                Text::default()
            } else {
                cat!(" and ", join(rows))
            };
            cat!(
                s::STEP_INTERFACE,
                " ",
                beacon_chip(visit.beacon.as_ref(), at(&visit_at, "beacon"), &BEACON_ARMS),
                what,
                " (",
                duration(spent),
                " on site)"
            )
        }
        Some(step::Kind::PlaceBeacon(place)) => {
            let place_at = at(here, "place_beacon");
            let spent = interface::place_beacon_ms(place, ctx.rates).total;
            let mut sentence = cat!(
                s::STEP_PLACE_BEACON,
                " ",
                location_chip(
                    place.at.as_ref(),
                    at(&place_at, "at"),
                    &crate::shape::place_arms()
                )
            );
            if !place.tags.is_empty() {
                let tags: Vec<Text> = place.tags.iter().map(|tag| cat!("\"", tag, "\"")).collect();
                sentence.push(cat!(", ", s::TAGGED, " ", join(tags)));
            }
            if let Some(initial) = place.initial.as_ref() {
                let initial_at = at(&place_at, "initial");
                if let Some(settings) = initial.mandate.as_ref() {
                    sentence.push(cat!(
                        ", on ",
                        article(settings),
                        " ",
                        writ(settings),
                        " mandate",
                        settings_clause(settings, &at(&initial_at, "mandate"))
                    ));
                }
                if initial.priority != 0 {
                    sentence.push(cat!(
                        ", at ",
                        priority_chip(initial.priority, at(&initial_at, "priority")),
                        " Quartermaster priority"
                    ));
                }
            }
            sentence.push(cat!(" (", duration(spent), " on site)"));
            sentence
        }
        Some(step::Kind::WaitUntil(wait)) => {
            let wait_at = at(here, "wait_until");
            cat!(
                s::STEP_WAIT_UNTIL,
                " ",
                condition(wait.condition.as_ref(), &at(&wait_at, "condition"))
            )
        }
        Some(step::Kind::Hold(hold)) => {
            let ms_at = at(&at(here, "hold"), "ms");
            // The verifier refuses a hold of zero or less (`E0303`).
            cat!(s::STEP_HOLD, " ", duration_chip(hold.ms, ms_at, Some(1)))
        }
        Some(step::Kind::Broadcast(_)) => cat!(s::STEP_BROADCAST),
    }
}

fn guards(entry: &Step, here: &str) -> Vec<Text> {
    let mut found = Vec::new();
    if let Some(skip) = entry.skip_if.as_ref() {
        found.push(cat!(
            s::GUARD_SKIP_IF,
            " ",
            condition(Some(skip), &at(here, "skip_if")),
            "."
        ));
    }
    if entry.timeout_ms != 0 {
        // A `wait_until` step's timeout must be above zero (`E0302`); any
        // other step's must not be below it.
        let least = i32::from(matches!(entry.kind, Some(step::Kind::WaitUntil(_))));
        found.push(cat!(
            s::GUARD_TIMEOUT,
            " ",
            duration_chip(entry.timeout_ms, at(here, "timeout_ms"), Some(least)),
            "."
        ));
    }
    if let Some(fail) = entry.on_fail.as_ref() {
        let fail_at = at(here, "on_fail");
        match fail.action() {
            on_fail::Action::Skip => found.push(cat!(s::ON_FAIL_SKIP)),
            on_fail::Action::AbortRoute => found.push(cat!(s::ON_FAIL_ABORT)),
            on_fail::Action::JumpForward => found.push(cat!(
                s::ON_FAIL_JUMP,
                " ",
                label_chip(
                    &fail.jump_to_label,
                    at(&fail_at, "jump_to_label"),
                    ReferenceKind::StepLabel
                ),
                "."
            )),
            on_fail::Action::OnFailUnspecified => {}
        }
    }
    found
}

/// A label or a rule id the file names, quoted, as a chip when it is written.
fn label_chip(label: &str, pointer: String, kind: ReferenceKind) -> Text {
    let shown = format!("\"{label}\"");
    if label.is_empty() {
        return cat!(shown);
    }
    chip(
        &shown,
        pointer,
        Spec::Ready(Shape::reference(kind, &[])),
        Value::text(label),
    )
}

// ---------------------------------------------------------------------------
// Interface rows
// ---------------------------------------------------------------------------

fn row_sentence(row: &InterfaceRow, here: &str) -> Text {
    match row.row.as_ref() {
        None => cat!(s::ROW_NOTHING),
        Some(interface_row::Row::SetMandate(settings)) => cat!(
            s::ROW_SET_MANDATE,
            " ",
            article(settings),
            " ",
            writ(settings),
            " mandate",
            settings_clause(settings, &at(here, "set_mandate"))
        ),
        Some(interface_row::Row::SetMandateSettings(settings)) => cat!(
            s::ROW_SET_SETTINGS,
            settings_clause(settings, &at(here, "set_mandate_settings"))
        ),
        Some(interface_row::Row::SetPriority(value)) => cat!(
            s::ROW_SET_PRIORITY,
            " ",
            priority_chip(*value, at(here, "set_priority"))
        ),
        Some(interface_row::Row::Recycle(_)) => cat!(s::ROW_RECYCLE),
        Some(interface_row::Row::AddBuildTarget(add)) => {
            let target = add.target.as_ref();
            let target_at = at(&at(here, "add_build_target"), "target");
            let blueprint = target.map_or_else(String::new, |target| target.blueprint_id.clone());
            let anchor = location_chip(
                target.and_then(|target| target.anchor.as_ref()),
                at(&target_at, "anchor"),
                &ANCHOR_ARMS,
            );
            // A wall's box is named as the author wrote it, corner to corner
            // (S2's plan, task `con4`); whether it is legal is the verifier's.
            let wall_box = target
                .and_then(|target| target.wall_box.as_ref())
                .map_or_else(String::new, |area| {
                    format!(
                        ", {} {} {} {}",
                        s::WALL_BOX_FROM,
                        corner(area.min.as_ref()),
                        s::WALL_BOX_TO,
                        corner(area.max.as_ref())
                    )
                });
            cat!(s::ROW_ADD_TARGET, " ", blueprint, " at ", anchor, wall_box)
        }
        Some(interface_row::Row::RemoveBuildTarget(remove)) => cat!(
            s::ROW_REMOVE_TARGET,
            " ",
            location_chip(
                remove.anchor.as_ref(),
                at(&at(here, "remove_build_target"), "anchor"),
                &["on"]
            )
        ),
        Some(interface_row::Row::QueueStructure(queue)) => {
            cat!(s::ROW_QUEUE, " ", &queue.blueprint_id)
        }
    }
}

/// What a Defend or an Attack arm writes, as a clause after its writ: empty
/// for every other writ and for an empty arm, so S1's prose is unchanged
/// (S2's plan, task `vfy2`). Every field the arm writes is said, in field
/// order, as the file wrote it; whether it is legal is the verifier's. `here`
/// is the `MandateSettings`' pointer.
fn settings_clause(settings: &MandateSettings, here: &str) -> Text {
    let parts = match settings.mandate.as_ref() {
        Some(mandate_settings::Mandate::Defend(defend)) => {
            defend_parts(defend, &at(here, "defend"))
        }
        Some(mandate_settings::Mandate::Attack(attack)) => {
            attack_parts(attack, &at(here, "attack"))
        }
        _ => Vec::new(),
    };
    if parts.is_empty() {
        Text::default()
    } else {
        cat!(": ", join(parts))
    }
}

/// A Defend arm's fields, in field order.
fn defend_parts(defend: &DefendSettings, here: &str) -> Vec<Text> {
    let mut parts = Vec::new();
    if !defend.protect.is_empty() {
        let protect_at = at(here, "protect");
        let assets: Vec<Text> = defend
            .protect
            .iter()
            .enumerate()
            .map(|(index, entry)| match entry.asset.as_ref() {
                Some(protected_asset::Asset::Beacon(reference)) => beacon_chip(
                    Some(reference),
                    at(&at(&protect_at, index), "beacon"),
                    &BEACON_ARMS,
                ),
                Some(protected_asset::Asset::MyCommander(_)) => cat!(s::DEFEND_MY_COMMANDER),
                None => cat!(s::DEFEND_NOTHING),
            })
            .collect();
        parts.push(cat!(s::DEFEND_PROTECTING, " ", join(assets)));
    }
    if defend.engagement_radius_pct != 0 {
        // 1 to 100: a written 0 is `E0101`'s, past 100 the range's.
        parts.push(cat!(
            s::DEFEND_ENGAGING,
            " ",
            int_chip(
                defend.engagement_radius_pct,
                at(here, "engagement_radius_pct"),
                Shape::integer(Some(1), Some(100), crate::shape::UNIT_PERCENT)
            ),
            "% ",
            s::DEFEND_OF_SPHERE
        ));
    }
    if defend.pursue != 0 {
        let words = match defend_settings::Pursue::try_from(defend.pursue) {
            Ok(defend_settings::Pursue::Never) => s::PURSUE_NEVER,
            Ok(defend_settings::Pursue::SphereEdge) => s::PURSUE_SPHERE_EDGE,
            Ok(defend_settings::Pursue::WhileInReach) => s::PURSUE_WHILE_IN_REACH,
            Ok(defend_settings::Pursue::Unspecified) | Err(_) => s::PURSUE_UNSET,
        };
        parts.push(enum_chip(
            words,
            defend.pursue,
            at(here, "pursue"),
            "gp.v1.DefendSettings.Pursue",
        ));
    }
    if let Some(pickets) = defend.picket_scouts {
        // At most a beacon's byte column (`E0508`).
        parts.push(cat!(
            s::DEFEND_PICKET,
            " ",
            int_chip(
                pickets,
                at(here, "picket_scouts"),
                Shape::integer(Some(0), Some(i32::from(u8::MAX)), UNIT_COUNT)
            ),
            " ",
            s::DEFEND_PICKET_UNIT
        ));
    }
    parts
}

/// An Attack arm's fields, in field order.
fn attack_parts(attack: &AttackSettings, here: &str) -> Vec<Text> {
    let mut parts = Vec::new();
    if let Some(target) = attack.target.as_ref() {
        let target_at = at(here, "target");
        let what = match target.target.as_ref() {
            // An Attack target's selector is read when the force launches
            // (S2's `atk2`), not when the step starts. `safest` is no Attack
            // target (`E0415`), so it is not offered.
            Some(attack_target::Target::Beacon(reference)) => {
                let shown = beacon_noted(Some(reference), s::ATTACK_SELECTOR_NOTE);
                chip(
                    &shown,
                    at(&target_at, "beacon"),
                    Spec::Ready(Shape::reference(
                        ReferenceKind::Beacon,
                        &["beacon_id", "nearest", "weakest", "most_threatened"],
                    )),
                    Value::Beacon(reference.clone()),
                )
            }
            Some(attack_target::Target::Voxel(spot)) => cat!(voxel(spot)),
            Some(attack_target::Target::EnemyCommander(_)) => cat!(s::ATTACK_ENEMY_COMMANDER),
            None => cat!(s::ATTACK_NOTHING),
        };
        parts.push(cat!(s::ATTACK_TARGETING, " ", what));
    }
    if attack.retarget != 0 {
        let words = match attack_settings::Retarget::try_from(attack.retarget) {
            Ok(attack_settings::Retarget::NearestKnown) => s::RETARGET_NEAREST,
            Ok(attack_settings::Retarget::None) => s::RETARGET_NONE,
            Ok(attack_settings::Retarget::Unspecified) | Err(_) => s::RETARGET_UNSET,
        };
        parts.push(enum_chip(
            words,
            attack.retarget,
            at(here, "retarget"),
            "gp.v1.AttackSettings.Retarget",
        ));
    }
    if let Some(staging) = attack.staging.as_ref() {
        parts.push(cat!(s::ATTACK_STAGING, " ", voxel(staging)));
    }
    if let Some(launch) = attack.launch.as_ref() {
        parts.push(launch_part(launch, &at(here, "launch")));
    }
    if let Some(retreat) = attack.retreat.as_ref()
        && let Some(part) = retreat_part(retreat, &at(here, "retreat"))
    {
        parts.push(part);
    }
    if attack.after_action != 0 {
        let words = match attack_settings::AfterAction::try_from(attack.after_action) {
            Ok(attack_settings::AfterAction::RegroupAtStaging) => s::AFTER_REGROUP,
            Ok(attack_settings::AfterAction::ReturnHome) => s::AFTER_HOME,
            Ok(attack_settings::AfterAction::Unspecified) | Err(_) => s::AFTER_UNSET,
        };
        parts.push(enum_chip(
            words,
            attack.after_action,
            at(here, "after_action"),
            "gp.v1.AttackSettings.AfterAction",
        ));
    }
    if !attack.avoid_areas.is_empty() {
        let count = attack.avoid_areas.len();
        let noun = if count == 1 {
            s::AREA_ONE
        } else {
            s::AREA_MANY
        };
        parts.push(cat!(s::ATTACK_AVOIDING, " ", count.to_string(), " ", noun));
    }
    if let Some(breach) = attack.breach_allowed {
        let words = if breach {
            s::BREACH_ALLOWED
        } else {
            s::BREACH_REFUSED
        };
        parts.push(chip(
            words,
            at(here, "breach_allowed"),
            Spec::Ready(Shape::boolean()),
            Value::int(breach),
        ));
    }
    if attack.waves != 0 {
        let noun = if attack.waves == 1 {
            s::WAVE_ONE
        } else {
            s::WAVE_MANY
        };
        // At least one in a deploy or a switch (the range's `E0109`).
        parts.push(cat!(
            s::WAVES_IN,
            " ",
            int_chip(
                attack.waves,
                at(here, "waves"),
                Shape::integer(Some(1), None, UNIT_COUNT)
            ),
            " ",
            noun
        ));
    }
    parts
}

/// An Attack's launch conditions; `here` is the `AttackLaunch`' pointer.
fn launch_part(launch: &pharmakos_proto::gp::v1::AttackLaunch, here: &str) -> Text {
    let mut when = Vec::new();
    if let Some(force) = launch.force_size {
        when.push(cat!(
            int_chip(force, at(here, "force_size"), Shape::count(UNIT_COUNT)),
            " ",
            s::LAUNCH_FORCE
        ));
    }
    if let Some(ms) = launch.segment_ms {
        when.push(cat!(
            duration_chip(ms, at(here, "segment_ms"), Some(0)),
            " ",
            s::LAUNCH_SEGMENT
        ));
    }
    let when = if when.is_empty() {
        cat!(s::LAUNCH_NEVER)
    } else {
        separated(when, s::OR)
    };
    cat!(s::ATTACK_LAUNCH, " ", when)
}

/// An Attack's retreat conditions, or nothing when it writes none; `here` is
/// the `AttackRetreat`'s pointer.
fn retreat_part(retreat: &pharmakos_proto::gp::v1::AttackRetreat, here: &str) -> Option<Text> {
    let mut when = Vec::new();
    if let Some(pct) = retreat.force_below_pct {
        when.push(cat!(
            s::RETREAT_FORCE,
            " ",
            int_chip(
                pct,
                at(here, "force_below_pct"),
                Shape::integer(Some(1), Some(100), crate::shape::UNIT_PERCENT)
            ),
            "% ",
            s::RETREAT_FORCE_UNIT
        ));
    }
    if let Some(ms) = retreat.after_launch_ms {
        when.push(cat!(
            duration_chip(ms, at(here, "after_launch_ms"), Some(0)),
            " ",
            s::RETREAT_TIME
        ));
    }
    (!when.is_empty()).then(|| cat!(s::ATTACK_RETREAT, " ", separated(when, s::OR)))
}

/// The article a writ's name takes: "an Attack", "a Build".
fn article(settings: &MandateSettings) -> &'static str {
    match settings.mandate.as_ref() {
        Some(mandate_settings::Mandate::Attack(_)) => s::ARTICLE_AN,
        _ => s::ARTICLE_A,
    }
}

fn writ(settings: &MandateSettings) -> &'static str {
    match settings.mandate.as_ref() {
        Some(mandate_settings::Mandate::Build(_)) => s::MANDATE_BUILD,
        Some(mandate_settings::Mandate::Defend(_)) => s::MANDATE_DEFEND,
        Some(mandate_settings::Mandate::Attack(_)) => s::MANDATE_ATTACK,
        Some(mandate_settings::Mandate::Survey(_)) => s::MANDATE_SURVEY,
        Some(mandate_settings::Mandate::Mine(_)) => s::MANDATE_MINE,
        None => s::MANDATE_NONE,
    }
}

fn priority(value: i32) -> &'static str {
    match interface_row::QuartermasterPriority::try_from(value) {
        Ok(interface_row::QuartermasterPriority::Low) => s::PRIORITY_LOW,
        Ok(interface_row::QuartermasterPriority::Normal) => s::PRIORITY_NORMAL,
        Ok(interface_row::QuartermasterPriority::High) => s::PRIORITY_HIGH,
        Ok(interface_row::QuartermasterPriority::Unspecified) | Err(_) => s::PRIORITY_UNSET,
    }
}

/// A Quartermaster priority's word, as a chip when the file writes one.
fn priority_chip(value: i32, pointer: String) -> Text {
    enum_chip(
        priority(value),
        value,
        pointer,
        "gp.v1.InterfaceRow.QuartermasterPriority",
    )
}

// ---------------------------------------------------------------------------
// Chips of one kind
// ---------------------------------------------------------------------------

/// An enum's words, as a chip of the schema's values when the file writes a
/// value the schema names; the words alone for the unspecified zero or a
/// number the schema does not name, which are the verifier's to refuse and
/// which no `replace` should keep.
fn enum_chip(words: &str, value: i32, pointer: String, full_name: &'static str) -> Text {
    let named = pharmakos_proto::descriptor::schema()
        .enumeration(full_name)
        .and_then(|declared| declared.value_by_number(value))
        .filter(|declared| declared.number != 0);
    if let Some(declared) = named {
        chip(
            words,
            pointer,
            Spec::Choice(full_name),
            Value::name(&declared.name),
        )
    } else {
        cat!(words)
    }
}

/// A whole number, as a chip.
fn int_chip(value: impl core::fmt::Display, pointer: String, shape: Shape) -> Text {
    let shown = value.to_string();
    chip(
        &shown,
        pointer,
        Spec::Ready(shape),
        Value::Json(shown.clone()),
    )
}

/// A duration in game milliseconds, shown as [`duration`] writes it, as a
/// chip whose value is the milliseconds; `least` is the verifier's floor.
fn duration_chip(ms: i32, pointer: String, least: Option<i32>) -> Text {
    chip(
        &duration(Ms::new(ms)),
        pointer,
        Spec::Ready(Shape::integer(least, None, crate::shape::UNIT_MS)),
        Value::int(ms),
    )
}

// ---------------------------------------------------------------------------
// Places
// ---------------------------------------------------------------------------

fn location(place: Option<&Location>) -> String {
    match place.and_then(|place| place.place.as_ref()) {
        None => s::PLACE_NOWHERE.to_owned(),
        Some(location::Place::Voxel(spot)) => voxel(spot),
        Some(location::Place::BeaconAnchor(reference)) => beacon(Some(reference)),
        Some(location::Place::Safest(_)) => s::BEACON_SAFEST.to_owned(),
        Some(location::Place::On(feature)) => format!("{} {}", s::SITE_ON, feature_ref(feature)),
        Some(location::Place::Covering(feature)) => {
            format!("{} {}", s::SITE_COVERING, feature_ref(feature))
        }
        Some(location::Place::Toward(toward)) => match toward.feature.as_ref() {
            Some(feature) => format!("{} {}", s::SITE_TOWARD, feature_ref(feature)),
            None => format!("{} {}", s::SITE_TOWARD, s::FEATURE_NONE),
        },
    }
}

/// A place, as a chip when the file writes one; `arms` are the arms legal at
/// its pointer.
fn location_chip(place: Option<&Location>, pointer: String, arms: &[&'static str]) -> Text {
    let shown = location(place);
    if let Some(place) = place.filter(|place| place.place.is_some()) {
        chip(
            &shown,
            pointer,
            Spec::Ready(Shape::reference(ReferenceKind::Location, arms)),
            Value::Location(place.clone()),
        )
    } else {
        cat!(shown)
    }
}

/// The place an enemy predicate measures from.
fn near_place(place: Option<&NearPlace>) -> String {
    match place.and_then(|place| place.place.as_ref()) {
        None => s::PLACE_NOWHERE.to_owned(),
        Some(near_place::Place::Voxel(spot)) => voxel(spot),
        Some(near_place::Place::BeaconAnchor(reference)) => beacon(Some(reference)),
        Some(near_place::Place::Safest(_)) => s::BEACON_SAFEST.to_owned(),
    }
}

/// [`near_place`], as a chip when the file writes one.
fn near_chip(place: Option<&NearPlace>, pointer: String) -> Text {
    let shown = near_place(place);
    if let Some(place) = place.filter(|place| place.place.is_some()) {
        chip(
            &shown,
            pointer,
            Spec::Ready(Shape::reference(ReferenceKind::Location, &NEAR_ARMS)),
            Value::Near(place.clone()),
        )
    } else {
        cat!(shown)
    }
}

/// A feature, named or described, as the author wrote it.
fn feature_ref(feature: &FeatureRef) -> String {
    match feature.r#ref.as_ref() {
        None => s::FEATURE_NONE.to_owned(),
        Some(feature_ref::Ref::FeatureId(id)) => feature_name(id),
        Some(feature_ref::Ref::Vent(pick)) => {
            pick_sentence(s::VENT_NOUN, pick.rank(), pick.coverage())
        }
        Some(feature_ref::Ref::Seam(pick)) => {
            pick_sentence(s::SEAM_NOUN, pick.rank(), pick.coverage())
        }
        Some(feature_ref::Ref::Covered(_)) => s::FEATURE_COVERED.to_owned(),
    }
}

/// `vent_120_88` as "Heat vent (120, 88)" (targeting.md, "Names"). A name that
/// is not of that shape is quoted as it was written: the verifier says whether
/// it names anything, and the rendering does not guess. "That shape" is the
/// canonical spelling exactly: `vent_+120_088` parses to the same numbers but
/// is a different name, which the verifier matches by string and refuses, so
/// it is quoted rather than read as the vent it is not.
fn feature_name(id: &str) -> String {
    let parse = |prefix: &str| {
        let rest = id.strip_prefix(prefix)?;
        let (x, y) = rest.split_once('_')?;
        let parsed = (x.parse::<i32>().ok()?, y.parse::<i32>().ok()?);
        (format!("{prefix}{}_{}", parsed.0, parsed.1) == id).then_some(parsed)
    };
    if let Some((x, y)) = parse("vent_") {
        return format!("{} ({x}, {y})", s::VENT_NAME);
    }
    if let Some((x, y)) = parse("seam_") {
        return format!("{} ({x}, {y})", s::SEAM_NAME);
    }
    format!("{} \"{id}\"", s::FEATURE_NOUN)
}

/// A pick, with the chip's reminder of when and how it reads.
fn pick_sentence(noun: &str, rank: Rank, coverage: Coverage) -> String {
    let rank = match rank {
        Rank::Nearest => s::BEACON_NEAREST,
        Rank::Unspecified => s::RANK_UNSET,
    };
    let filter = match coverage {
        Coverage::Uncovered => s::NOT_COVERED,
        Coverage::Any | Coverage::Unspecified => "",
    };
    format!("{rank} {noun}{filter} ({})", s::PICK_NOTE)
}

fn voxel(spot: &Voxel) -> String {
    format!("{} {}, {}, {}", s::PLACE_VOXEL, spot.x, spot.y, spot.z)
}

/// One corner of an area, or the words for a corner the file leaves out.
fn corner(spot: Option<&Voxel>) -> String {
    spot.map_or_else(|| s::PLACE_NOWHERE.to_owned(), voxel)
}

fn beacon(reference: Option<&BeaconRef>) -> String {
    beacon_noted(reference, s::SELECTOR_NOTE)
}

/// [`beacon`], as a chip when the file writes a reference; `arms` are the
/// arms legal at its pointer.
fn beacon_chip(reference: Option<&BeaconRef>, pointer: String, arms: &[&'static str]) -> Text {
    let shown = beacon(reference);
    if let Some(reference) = reference.filter(|reference| reference.r#ref.is_some()) {
        chip(
            &shown,
            pointer,
            Spec::Ready(Shape::reference(ReferenceKind::Beacon, arms)),
            Value::Beacon(reference.clone()),
        )
    } else {
        cat!(shown)
    }
}

/// [`beacon`], with `note` saying when a selector is read.
fn beacon_noted(reference: Option<&BeaconRef>, note: &str) -> String {
    match reference.and_then(|reference| reference.r#ref.as_ref()) {
        None => s::BEACON_NONE.to_owned(),
        Some(beacon_ref::Ref::BeaconId(id)) => format!("{} \"{id}\"", s::BEACON_NOUN),
        Some(beacon_ref::Ref::Safest(_)) => s::BEACON_SAFEST.to_owned(),
        Some(beacon_ref::Ref::Nearest(pick)) => {
            selector(s::BEACON_NEAREST, pick.filter.as_ref(), note)
        }
        Some(beacon_ref::Ref::Weakest(pick)) => {
            selector(s::BEACON_WEAKEST, pick.filter.as_ref(), note)
        }
        Some(beacon_ref::Ref::MostThreatened(pick)) => {
            selector(s::BEACON_MOST_THREATENED, pick.filter.as_ref(), note)
        }
    }
}

/// A selector, with the chip text spec section 13 asks for.
fn selector(superlative: &str, filter: Option<&BeaconFilter>, note: &str) -> String {
    let mut sentence = superlative.to_owned();
    let side = match filter.map(BeaconFilter::side) {
        Some(beacon_filter::Side::EnemyKnown) => s::SIDE_ENEMY,
        _ => s::SIDE_OWN,
    };
    sentence.push(' ');
    sentence.push_str(side);
    if let Some(writ) = filter.map(BeaconFilter::mandate)
        && writ != beacon_filter::MandateKind::Unspecified
    {
        sentence.push(' ');
        sentence.push_str(writ_word(writ));
    }
    sentence.push(' ');
    sentence.push_str(s::BEACON_NOUN);
    if let Some(tags) = filter.map(|filter| &filter.tags)
        && !tags.is_empty()
    {
        let quoted: Vec<String> = tags.iter().map(|tag| format!("\"{tag}\"")).collect();
        sentence.push(' ');
        sentence.push_str(s::TAGGED);
        sentence.push(' ');
        sentence.push_str(&join_words(&quoted));
    }
    format!("{sentence} ({note})")
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

/// A condition, with its chips; `here` is the `Condition`'s pointer.
fn condition(node: Option<&Condition>, here: &str) -> Text {
    let Some(node) = node.and_then(|node| node.node.as_ref()) else {
        return cat!(s::CONDITION_NOTHING);
    };
    match node {
        condition::Node::All(group) => {
            group_sentence(&group.items, &at(&at(here, "all"), "items"), s::AND)
        }
        condition::Node::Any(group) => {
            group_sentence(&group.items, &at(&at(here, "any"), "items"), s::OR)
        }
        condition::Node::Not(inner) => cat!(
            s::NOT,
            " ",
            condition(inner.item.as_deref(), &at(&at(here, "not"), "item"))
        ),
        condition::Node::CmdrHpPct(test) => cmdr_hp(*test, &at(here, "cmdr_hp_pct")),
        condition::Node::CmdrTookDamageWithin(test) => cat!(
            "the commander took damage in the last ",
            nonzero_duration(test.ms, at(&at(here, "cmdr_took_damage_within"), "ms"))
        ),
        condition::Node::CmdrDeaths(test) => cat!(
            "the commander has died ",
            compare(
                test.deaths,
                &at(&at(here, "cmdr_deaths"), "deaths"),
                s::DEATHS_UNIT,
                Shape::integer(None, None, UNIT_COUNT)
            )
        ),
        condition::Node::SegmentElapsed(test) => cat!(
            "the segment has run ",
            compare_ms(test.ms, &at(&at(here, "segment_elapsed"), "ms"))
        ),
        condition::Node::BeaconHpPct(test) => {
            let test_at = at(here, "beacon_hp_pct");
            cat!(
                beacon_chip(test.beacon.as_ref(), at(&test_at, "beacon"), &BEACON_ARMS),
                " is ",
                compare(test.pct, &at(&test_at, "pct"), "% HP", Shape::percent())
            )
        }
        condition::Node::BeaconUnderAttack(test) => {
            under_attack(test, &at(here, "beacon_under_attack"))
        }
        condition::Node::BeaconPowered(test) => beacon_powered(test, &at(here, "beacon_powered")),
        condition::Node::Treasury(test) => cat!(
            "the treasury is ",
            compare_money(test.dollars, &at(&at(here, "treasury"), "dollars"))
        ),
        condition::Node::KwHeadroom(test) => grid(
            "the grid's headroom is ",
            test.kw,
            &at(&at(here, "kw_headroom"), "kw"),
        ),
        condition::Node::KwSupply(test) => grid(
            "the grid's supply is ",
            test.kw,
            &at(&at(here, "kw_supply"), "kw"),
        ),
        condition::Node::KwDraw(test) => grid(
            "the grid's draw is ",
            test.kw,
            &at(&at(here, "kw_draw"), "kw"),
        ),
        condition::Node::StepReached(test) => cat!(
            "the route has reached ",
            label_chip(
                &test.label,
                at(&at(here, "step_reached"), "label"),
                ReferenceKind::StepLabel
            )
        ),
        condition::Node::RuleFired(test) => rule_fired(test, &at(here, "rule_fired")),
        condition::Node::BeaconUnitsByRole(test) => {
            units_by_role(test, &at(here, "beacon_units_by_role"))
        }
        condition::Node::BeaconMandate(test) => beacon_mandate(test, &at(here, "beacon_mandate")),
        condition::Node::BeaconHasStructure(test) => cat!(
            beacon_chip(
                test.beacon.as_ref(),
                at(&at(here, "beacon_has_structure"), "beacon"),
                &BEACON_ARMS
            ),
            " has a finished ",
            &test.blueprint_id
        ),
        condition::Node::EnemyUnitsNear(test) => enemy_units(test, &at(here, "enemy_units_near")),
        condition::Node::EnemyBeaconKnown(test) => cat!(
            "the seat knows of an enemy beacon, ",
            fresh_words(
                test.freshness.as_ref(),
                &at(&at(here, "enemy_beacon_known"), "freshness")
            )
        ),
        condition::Node::EnemyStructureNear(test) => {
            enemy_structure(test, &at(here, "enemy_structure_near"))
        }
    }
}

/// `beacon_under_attack`; `here` is the predicate's pointer.
fn under_attack(test: &pharmakos_proto::gp::v1::BeaconUnderAttack, here: &str) -> Text {
    cat!(
        beacon_chip(test.beacon.as_ref(), at(here, "beacon"), &BEACON_ARMS),
        " took damage in the last ",
        nonzero_duration(test.within_ms, at(here, "within_ms"))
    )
}

/// `rule_fired`; `here` is the predicate's pointer.
fn rule_fired(test: &pharmakos_proto::gp::v1::RuleFired, here: &str) -> Text {
    cat!(
        "rule ",
        label_chip(
            &test.handler_id,
            at(here, "handler_id"),
            ReferenceKind::HandlerId
        ),
        " has fired"
    )
}

/// `beacon_mandate`; `here` is the predicate's pointer.
fn beacon_mandate(test: &pharmakos_proto::gp::v1::BeaconMandate, here: &str) -> Text {
    cat!(
        beacon_chip(test.beacon.as_ref(), at(here, "beacon"), &BEACON_ARMS),
        " carries ",
        enum_chip(
            writ_word(test.mandate()),
            test.mandate,
            at(here, "mandate"),
            "gp.v1.BeaconFilter.MandateKind"
        )
    )
}

/// `cmdr_hp_pct`; `here` is the predicate's pointer.
fn cmdr_hp(test: pharmakos_proto::gp::v1::CmdrHpPct, here: &str) -> Text {
    cat!(
        "the commander is ",
        enum_chip(
            cmp_hp(test.cmp()),
            test.cmp,
            at(here, "cmp"),
            "gp.v1.CmdrHpPct.Cmp"
        ),
        " ",
        if test.pct == 0 {
            cat!(test.pct.to_string())
        } else {
            int_chip(test.pct, at(here, "pct"), Shape::percent())
        },
        "% HP"
    )
}

/// `beacon_powered`; `here` is the predicate's pointer.
fn beacon_powered(test: &pharmakos_proto::gp::v1::BeaconPowered, here: &str) -> Text {
    let state = if test.powered {
        chip(
            s::POWERED,
            at(here, "powered"),
            Spec::Ready(Shape::boolean()),
            Value::int(true),
        )
    } else {
        cat!(s::DORMANT)
    };
    cat!(
        beacon_chip(test.beacon.as_ref(), at(here, "beacon"), &BEACON_ARMS),
        " is ",
        state
    )
}

/// `beacon_units_by_role`; `here` is the predicate's pointer.
fn units_by_role(test: &pharmakos_proto::gp::v1::BeaconUnitsByRole, here: &str) -> Text {
    cat!(
        beacon_chip(test.beacon.as_ref(), at(here, "beacon"), &BEACON_ARMS),
        " has ",
        compare_role(
            test.count,
            &at(here, "count"),
            enum_chip(
                role_noun(test.role()),
                test.role,
                at(here, "role"),
                "gp.v1.BeaconUnitsByRole.Role"
            )
        )
    )
}

/// `enemy_units_near`; `here` is the predicate's pointer.
fn enemy_units(test: &pharmakos_proto::gp::v1::EnemyUnitsNear, here: &str) -> Text {
    cat!(
        "the seat knows of ",
        compare(
            test.count,
            &at(here, "count"),
            s::ENEMY_UNITS_UNIT,
            Shape::integer(None, None, UNIT_COUNT)
        ),
        " within ",
        radius_chip(test.radius_voxels, at(here, "radius_voxels")),
        " voxels of ",
        near_chip(test.near.as_ref(), at(here, "near")),
        ", ",
        fresh_words(test.freshness.as_ref(), &at(here, "freshness"))
    )
}

/// `enemy_structure_near`; `here` is the predicate's pointer.
fn enemy_structure(test: &pharmakos_proto::gp::v1::EnemyStructureNear, here: &str) -> Text {
    cat!(
        "the seat knows of an enemy ",
        match test.structure.as_ref() {
            Some(enemy_structure_near::Structure::BlueprintId(id)) => id.as_str(),
            Some(enemy_structure_near::Structure::AnyKind(_)) => s::ANY_STRUCTURE,
            None => s::STRUCTURE_NONE,
        },
        " within ",
        radius_chip(test.radius_voxels, at(here, "radius_voxels")),
        " voxels of ",
        near_chip(test.near.as_ref(), at(here, "near")),
        ", ",
        fresh_words(test.freshness.as_ref(), &at(here, "freshness"))
    )
}

/// A grid predicate: `lead`, then its kW comparison at `here`.
fn grid(lead: &str, test: Option<pharmakos_proto::gp::v1::IntCompare>, here: &str) -> Text {
    cat!(
        lead,
        compare(test, here, "kW", Shape::integer(None, None, UNIT_KW))
    )
}

/// A `uint32` radius in voxels, as a chip when it is written.
fn radius_chip(radius: u32, pointer: String) -> Text {
    if radius == 0 {
        cat!(radius.to_string())
    } else {
        int_chip(radius, pointer, Shape::count(UNIT_VOXELS))
    }
}

/// A duration the verifier refuses below zero, as a chip when it is written.
fn nonzero_duration(ms: i32, pointer: String) -> Text {
    if ms == 0 {
        cat!(duration(Ms::new(ms)))
    } else {
        duration_chip(ms, pointer, Some(0))
    }
}

fn writ_word(writ: beacon_filter::MandateKind) -> &'static str {
    match writ {
        beacon_filter::MandateKind::Build => s::MANDATE_BUILD,
        beacon_filter::MandateKind::Defend => s::MANDATE_DEFEND,
        beacon_filter::MandateKind::Attack => s::MANDATE_ATTACK,
        beacon_filter::MandateKind::Survey => s::MANDATE_SURVEY,
        beacon_filter::MandateKind::Mine => s::MANDATE_MINE,
        beacon_filter::MandateKind::Unspecified => s::MANDATE_NONE,
    }
}

fn role_noun(role: beacon_units_by_role::Role) -> &'static str {
    match role {
        beacon_units_by_role::Role::BuildDrone => s::ROLE_BUILD_DRONE,
        beacon_units_by_role::Role::MiningDrone => s::ROLE_MINING_DRONE,
        beacon_units_by_role::Role::RepairDrone => s::ROLE_REPAIR_DRONE,
        beacon_units_by_role::Role::Raider => s::ROLE_RAIDER,
        beacon_units_by_role::Role::Scout => s::ROLE_SCOUT,
        beacon_units_by_role::Role::Unspecified => s::ROLE_NONE,
    }
}

/// A freshness window's words; `here` is the `Freshness`' pointer.
fn fresh_words(window: Option<&Freshness>, here: &str) -> Text {
    let Some(window) = window else {
        return cat!(s::FRESH_NONE);
    };
    let kind_at = at(here, "kind");
    match window.kind() {
        freshness::Kind::VisibleNow => {
            enum_chip(s::FRESH_NOW, window.kind, kind_at, "gp.v1.Freshness.Kind")
        }
        freshness::Kind::KnownWithin => cat!(
            enum_chip(
                s::FRESH_WITHIN,
                window.kind,
                kind_at,
                "gp.v1.Freshness.Kind"
            ),
            " ",
            // No least: the verifier refuses an enemy predicate whole until S3's
            // `vfy6` checks its window, so nothing bounds it yet.
            if window.known_within_ms == 0 {
                cat!(duration(Ms::new(0)))
            } else {
                duration_chip(window.known_within_ms, at(here, "known_within_ms"), None)
            }
        ),
        freshness::Kind::FreshnessUnspecified => cat!(s::FRESH_NONE),
    }
}

fn group_sentence(items: &[Condition], here: &str, joiner: &str) -> Text {
    if items.is_empty() {
        return cat!(s::EMPTY_GROUP);
    }
    let rendered: Vec<Text> = items
        .iter()
        .enumerate()
        .map(|(index, item)| condition(Some(item), &at(here, index)))
        .collect();
    separated(rendered, joiner)
}

fn cmp_hp(cmp: cmdr_hp_pct::Cmp) -> &'static str {
    match cmp {
        cmdr_hp_pct::Cmp::Lt => s::CMP_LT,
        cmdr_hp_pct::Cmp::Le => s::CMP_LE,
        cmdr_hp_pct::Cmp::Eq => s::CMP_EQ,
        cmdr_hp_pct::Cmp::Ne => s::CMP_NE,
        cmdr_hp_pct::Cmp::Ge => s::CMP_GE,
        cmdr_hp_pct::Cmp::Gt => s::CMP_GT,
        cmdr_hp_pct::Cmp::Unspecified => s::CMP_NONE,
    }
}

fn cmp_word(op: int_compare::Op) -> &'static str {
    match op {
        int_compare::Op::Lt => s::CMP_LT,
        int_compare::Op::Le => s::CMP_LE,
        int_compare::Op::Eq => s::CMP_EQ,
        int_compare::Op::Ne => s::CMP_NE,
        int_compare::Op::Ge => s::CMP_GE,
        int_compare::Op::Gt => s::CMP_GT,
        int_compare::Op::Unspecified => s::CMP_NONE,
    }
}

/// An `IntCompare`'s operator, as a chip; `here` is the comparison's pointer.
fn op_chip(test: pharmakos_proto::gp::v1::IntCompare, here: &str) -> Text {
    enum_chip(
        cmp_word(test.op()),
        test.op,
        at(here, "op"),
        "gp.v1.IntCompare.Op",
    )
}

/// An `IntCompare`'s value, as a chip when it is written.
fn value_chip(test: pharmakos_proto::gp::v1::IntCompare, here: &str, shape: Shape) -> Text {
    if test.value == 0 {
        cat!(test.value.to_string())
    } else {
        int_chip(test.value, at(here, "value"), shape)
    }
}

fn compare(
    test: Option<pharmakos_proto::gp::v1::IntCompare>,
    here: &str,
    unit: &str,
    shape: Shape,
) -> Text {
    let Some(test) = test else {
        return cat!(s::CONDITION_NOTHING);
    };
    let spacer = if unit.starts_with('%') { "" } else { " " };
    cat!(
        op_chip(test, here),
        " ",
        value_chip(test, here, shape),
        spacer,
        unit
    )
}

/// A role count: "at or above 2 scouts", with the role's noun, `noun`, a chip
/// of its own.
fn compare_role(test: Option<pharmakos_proto::gp::v1::IntCompare>, here: &str, noun: Text) -> Text {
    let Some(test) = test else {
        return cat!(s::CONDITION_NOTHING);
    };
    cat!(
        op_chip(test, here),
        " ",
        value_chip(test, here, Shape::integer(None, None, UNIT_COUNT)),
        " ",
        noun
    )
}

fn compare_money(test: Option<pharmakos_proto::gp::v1::IntCompare>, here: &str) -> Text {
    let Some(test) = test else {
        return cat!(s::CONDITION_NOTHING);
    };
    cat!(
        op_chip(test, here),
        " $",
        value_chip(test, here, Shape::integer(None, None, UNIT_DOLLARS))
    )
}

fn compare_ms(test: Option<pharmakos_proto::gp::v1::IntCompare>, here: &str) -> Text {
    let Some(test) = test else {
        return cat!(s::CONDITION_NOTHING);
    };
    let shown = if test.value == 0 {
        cat!(duration(Ms::new(test.value)))
    } else {
        chip(
            &duration(Ms::new(test.value)),
            at(here, "value"),
            Spec::Ready(Shape::integer(None, None, crate::shape::UNIT_MS)),
            Value::int(test.value),
        )
    };
    cat!(op_chip(test, here), " ", shown)
}

// ---------------------------------------------------------------------------
// Rules, blocks and the budget
// ---------------------------------------------------------------------------

fn rules_section(book: &Playbook, ctx: &Ctx<'_>, out: &mut Out) -> Vec<Pending> {
    let mut pending = Vec::new();
    let Some(body) = book.declarative.as_ref() else {
        return pending;
    };
    if body.handlers.is_empty() {
        return pending;
    }
    heading(s::HEADING_RULES, out);
    for (index, rule) in body.handlers.iter().enumerate() {
        let here = at("/declarative/handlers", index);
        let (open, from) = Pending::open(here.clone(), out);
        let number = out.ordinal(index);
        line(
            1,
            cat!(
                number.to_string(),
                ". \"",
                &rule.id,
                "\", when ",
                condition(rule.when.as_ref(), &at(&here, "when")),
                ":"
            ),
            out,
        );
        let body_at = at(&here, "body");
        for (position, entry) in rule.body.iter().enumerate() {
            let entry_at = at(&body_at, position);
            let step_number = out.ordinal(position);
            line(
                2,
                cat!(
                    step_number.to_string(),
                    ". ",
                    step_sentence(entry, &entry_at, ctx)
                ),
                out,
            );
            for guard in guards(entry, &entry_at) {
                line(3, guard, out);
            }
        }
        line(2, resume(rule, &here), out);
        line(2, firing(rule, &here, ctx.limits), out);
        let note = rule.note.trim();
        if !note.is_empty() {
            line(2, cat!(note), out);
        }
        pending.push(open.close(from, out));
    }
    pending
}

fn resume(rule: &Handler, here: &str) -> Text {
    match rule.resume() {
        handler::Resume::Continue => cat!("Then carry on with the route."),
        handler::Resume::SkipStep => cat!("Then skip the step the route was on."),
        handler::Resume::JumpForward => cat!(
            "Then jump forward to ",
            label_chip(
                &rule.resume_at_label,
                at(here, "resume_at_label"),
                ReferenceKind::StepLabel
            ),
            "."
        ),
        handler::Resume::EndRoute => cat!("Then end the route."),
        handler::Resume::Unspecified => cat!("Then do what the file does not say."),
    }
}

/// "Fires at most N times, and no sooner than every D.", with `max_fires`
/// between 1 and the rules table's ceiling (`E0107`) and the cooldown at or
/// above its floor (`E0108`).
fn firing(rule: &Handler, here: &str, limits: &Limits) -> Text {
    let fires = if rule.max_fires == 0 {
        cat!(rule.max_fires.to_string())
    } else {
        // The rules table's ceiling as the chip's `int32` bound; one the
        // bound cannot hold is the table disagreeing with the wire, refused
        // with the rendering (E22) rather than read as no ceiling.
        if let Ok(most) = i32::try_from(limits.max_fires_max()) {
            int_chip(
                rule.max_fires,
                at(here, "max_fires"),
                Shape::integer(Some(1), Some(most), UNIT_COUNT),
            )
        } else {
            let mut text = cat!(rule.max_fires.to_string());
            text.fail(s::RENDER_BOUND_PAST_RANGE);
            text
        }
    };
    let cooldown = if rule.cooldown_ms == 0 {
        cat!(duration(Ms::new(rule.cooldown_ms)))
    } else {
        duration_chip(
            rule.cooldown_ms,
            at(here, "cooldown_ms"),
            Some(limits.handler_cooldown_min_ms()),
        )
    };
    cat!(
        "Fires at most ",
        fires,
        " time",
        if rule.max_fires == 1 { "" } else { "s" },
        ", and no sooner than every ",
        cooldown,
        "."
    )
}

fn blocks(book: &Playbook, out: &mut Out) {
    heading(s::HEADING_ON_DEATH, out);
    let death = book.on_death.as_ref();
    line(
        1,
        cat!(match death.map(OnDeath::on_respawn) {
            Some(on_death::OnRespawn::Continue) => s::RESPAWN_CONTINUE,
            _ => s::RESPAWN_UNSET,
        }),
        out,
    );
    if let Some(limit) = death.map(|death| death.max_deaths_before_fallback)
        && limit != 0
    {
        line(
            1,
            cat!(format!("After {limit} deaths the fallback takes over.")),
            out,
        );
    }

    heading(s::HEADING_FALLBACK, out);
    line(1, cat!(fallback(book.fallback.as_ref())), out);

    if let Some(options) = book
        .declarative
        .as_ref()
        .and_then(|body| body.options.as_ref())
        && options_said(*options)
    {
        heading(s::HEADING_OPTIONS, out);
        if options.allow_dormant_beacons {
            line(1, cat!(s::ALLOW_DORMANT), out);
        }
    }
}

const fn options_said(options: Options) -> bool {
    options.allow_dormant_beacons
}

fn fallback(posture: Option<&Fallback>) -> String {
    match posture.and_then(|posture| posture.posture.as_ref()) {
        None => s::FALLBACK_NONE.to_owned(),
        Some(fallback::Posture::Hold(hold)) => {
            format!("{} {}.", s::FALLBACK_HOLD, location(hold.at.as_ref()))
        }
        Some(fallback::Posture::Shadow(shadow)) => {
            format!("{} {}.", s::FALLBACK_SHADOW, beacon(shadow.beacon.as_ref()))
        }
        Some(fallback::Posture::Patrol(patrol)) => {
            let stops: Vec<String> = patrol
                .waypoints
                .iter()
                .map(|stop| location(Some(stop)))
                .collect();
            format!("{} {}.", s::FALLBACK_PATROL, join_words(&stops))
        }
    }
}

fn budget(book: &Playbook, rates: &Rates, size_budget: u32, out: &mut Out) {
    heading(s::HEADING_BUDGET, out);
    line(
        1,
        cat!(format!("{} of {size_budget} size units.", size_units(book))),
        out,
    );
    line(
        1,
        cat!(format!(
            "{} standing at a beacon along the route.",
            duration(interface::route_ms(book, rates))
        )),
        out,
    );
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

fn heading(title: &str, out: &mut Out) {
    out.prose.push('\n');
    out.next_line();
    line(0, cat!(title), out);
}

/// One line of the prose at `depth`, its chips placed on it.
fn line(depth: usize, text: Text, out: &mut Out) {
    // On a fault the indent is never read: [`render`] refuses the
    // rendering with it.
    let indent = if let Some(indent) = depth.checked_mul(2) {
        indent
    } else {
        out.fail(s::RENDER_COUNT_PAST_RANGE);
        0
    };
    if let Some(why) = text.fault {
        out.fail(why);
    }
    for _ in 0..depth {
        out.prose.push_str("  ");
    }
    out.prose.push_str(&text.words);
    out.prose.push('\n');
    let number = out.line;
    for mark in text.marks {
        match (mark.start.checked_add(indent), mark.end.checked_add(indent)) {
            (Some(start), Some(end)) => out.marks.push(Placed {
                line: number,
                start,
                end,
                pointer: mark.pointer,
                spec: mark.spec,
                value: mark.value,
            }),
            _ => out.fail(s::RENDER_COUNT_PAST_RANGE),
        }
    }
    out.next_line();
}

/// `a`, `a and b`, `a, b and c` — the Oxford-free reading a sentence wants.
fn join(parts: Vec<Text>) -> Text {
    let mut parts = parts;
    let Some(last) = parts.pop() else {
        return Text::default();
    };
    if parts.is_empty() {
        return last;
    }
    let mut out = separated(parts, ", ");
    out.push(" and ");
    out.push(last);
    out
}

/// [`join`] over plain words.
fn join_words(parts: &[String]) -> String {
    join(parts.iter().map(|part| cat!(part)).collect()).words
}

/// The parts with `separator` between each two.
fn separated(parts: Vec<Text>, separator: &str) -> Text {
    let mut out = Text::default();
    for (index, part) in parts.into_iter().enumerate() {
        if index > 0 {
            out.push(separator);
        }
        out.push(part);
    }
    out
}

/// An exact duration, as the file wrote it: `8 s`, `1.5 s`, `2:00`.
///
/// Exact rather than rounded, unlike [`crate::travel::whole_seconds`]: these
/// are authored numbers and rules-table arithmetic, not estimates, and
/// rounding one would be the rendering inventing a fact. In `i64`, so the
/// least `i32`, negated, is still the number it is.
#[must_use]
pub fn duration(ms: Ms) -> String {
    let total = i64::from(ms.raw());
    if total < 0 {
        return format!("minus {}", magnitude(-total));
    }
    magnitude(total)
}

/// [`duration`] of a time at or above zero.
fn magnitude(total: i64) -> String {
    let seconds = total.div_euclid(1000);
    let rest = total.rem_euclid(1000);
    if rest == 0 && seconds >= 60 {
        let minutes = seconds.div_euclid(60);
        let spare = seconds.rem_euclid(60);
        return format!("{minutes}:{spare:02}");
    }
    if rest == 0 {
        return format!("{seconds} s");
    }
    if rest.rem_euclid(100) == 0 {
        let tenths = rest.div_euclid(100);
        return format!("{seconds}.{tenths} s");
    }
    format!("{seconds}.{rest:03} s")
}

#[cfg(test)]
mod tests {
    use super::{Limits, Text, duration, join_words, render_plan};
    use crate::canonical::canonicalise_text;
    use crate::context::PlanContext;
    use crate::strings as s;
    use pharmakos_sim::math::quantity::Ms;
    use pharmakos_sim::rules::RulesTable;
    use pharmakos_sim::snapshot::Snapshot;

    const MINIMAL: &str = concat!(
        "{\"schema_version\":{\"major\":1},",
        "\"meta\":{\"title\":\"Hold\",\"author_kind\":\"BUILTIN\"},",
        "\"declarative\":{\"route\":[{\"label\":\"wait\",\"hold\":{\"ms\":90000}}]},",
        "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
        "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
        "\"kind\":\"PLAYBOOK\"}"
    );

    fn rules() -> RulesTable {
        RulesTable::load(std::path::Path::new("../../rules/rules.v1.json"))
            .expect("the committed rules table")
    }

    fn context() -> PlanContext {
        let snapshot = Snapshot {
            seat_id: vec![0],
            seat_treasury: vec![200],
            seat_supply: vec![10],
            seat_draw: vec![2],
            ..Snapshot::default()
        };
        PlanContext::from_snapshot(&snapshot, 0).expect("seat 0")
    }

    fn prose(text: &str) -> String {
        prose_with(text, &rules())
    }

    fn prose_with(text: &str, rules: &RulesTable) -> String {
        let canonical = canonicalise_text(text).expect("a playbook");
        render_plan(&canonical.playbook, &context(), rules).expect("the rules table is complete")
    }

    #[test]
    fn durations_read_as_the_file_wrote_them() {
        assert_eq!(duration(Ms::new(0)), "0 s");
        assert_eq!(duration(Ms::new(1500)), "1.5 s");
        assert_eq!(duration(Ms::new(8000)), "8 s");
        assert_eq!(duration(Ms::new(120_000)), "2:00");
        assert_eq!(duration(Ms::new(16_500)), "16.5 s");
        assert_eq!(duration(Ms::new(1234)), "1.234 s");
        assert_eq!(duration(Ms::new(-500)), "minus 0.5 s");
        // E22 (S3's `gwr`): the ends of `i32` read as the numbers they
        // are; the least one used to saturate on its negation and lose 1 ms.
        assert_eq!(duration(Ms::new(i32::MAX)), "2147483.647 s");
        assert_eq!(duration(Ms::new(i32::MIN)), "minus 2147483.648 s");
    }

    /// E22 (S3's `gwr` review): a count the rendering cannot hold is a fault
    /// that travels with the words to `render`, which refuses with it, and
    /// never a count that saturated into one that reads as true.
    #[test]
    fn a_count_past_its_range_is_a_fault_and_not_a_saturated_count() {
        let mut out = super::Out {
            line: u32::MAX,
            ..super::Out::default()
        };
        out.next_line();
        assert_eq!(out.fault, Some(s::RENDER_COUNT_PAST_RANGE));
        let mut out = super::Out::default();
        assert_eq!(out.ordinal(3), 4);
        assert_eq!(out.fault, None);
        let _ = out.ordinal(usize::MAX);
        assert_eq!(out.fault, Some(s::RENDER_COUNT_PAST_RANGE));
        let mut faulty = cat!("a");
        faulty.fail(s::RENDER_BOUND_PAST_RANGE);
        let mut out = super::Out::default();
        super::line(1, cat!("b", faulty), &mut out);
        assert_eq!(out.fault, Some(s::RENDER_BOUND_PAST_RANGE));
        assert_eq!(out.line, 1, "the line is still written");
    }

    #[test]
    fn lists_read_as_sentences() {
        assert_eq!(join_words(&[]), "");
        assert_eq!(join_words(&["a".to_owned()]), "a");
        assert_eq!(join_words(&["a".to_owned(), "b".to_owned()]), "a and b");
        assert_eq!(
            join_words(&["a".to_owned(), "b".to_owned(), "c".to_owned()]),
            "a, b and c"
        );
    }

    #[test]
    fn the_rendering_is_deterministic() {
        assert_eq!(prose(MINIMAL), prose(MINIMAL));
    }

    #[test]
    fn the_envelope_and_the_blocks_are_rendered() {
        let text = prose(MINIMAL);
        assert!(
            text.starts_with("Hold\nWritten from a built-in template.\n"),
            "{text}"
        );
        assert!(text.contains("1. wait: hold for 1:30."), "{text}");
        assert!(text.contains("Hold at the safest own beacon."), "{text}");
    }

    /// The number in the meter has to **follow the row**, not happen to equal
    /// it: item 94 put the budget in the rules table and says P1 moves it at
    /// S3's exit, so a rendering that had baked today's 128 in would read
    /// correctly today and lie the day the row changes. Rendering twice, once
    /// against the committed table and once against a table whose row has been
    /// edited, is the only assertion a constant cannot pass.
    #[test]
    fn the_size_meter_reads_the_rules_table_budget() {
        let committed = rules();
        let row = Limits::from_rules(&committed)
            .expect("the committed table carries the verifier block")
            .size_budget_units();
        let text = prose_with(MINIMAL, &committed);
        assert!(text.contains(&format!("of {row} size units.")), "{text}");

        let mut edited = committed.message().clone();
        let verifier = edited
            .verifier
            .as_mut()
            .expect("the committed table carries a verifier block");
        verifier.size_budget_units = row.saturating_add(7);
        let moved = RulesTable::from_message(&edited).expect("the edited table still decodes");
        let text = prose_with(MINIMAL, &moved);
        assert!(
            text.contains(&format!("of {} size units.", row.saturating_add(7))),
            "the meter printed the old number after the row moved:\n{text}"
        );
    }

    /// Item 97: claim nothing a scenario assertion would later have to state
    /// differently. The snapshot carries the coming segment's length (T10),
    /// but whether the route fits it is the verifier's estimate stage's to
    /// say, so the prose must not mention a segment **at all** — not in any
    /// wording, which is why this looks for the word rather than for one
    /// sentence built from it.
    #[test]
    fn nothing_is_claimed_about_the_segments_length() {
        let text = prose(MINIMAL).to_lowercase();
        assert!(
            !text.contains("segment"),
            "the prose claims something about the segment:\n{text}"
        );
    }

    /// Targeting's arms read as the text the author wrote (S1's plan, task
    /// `tgtv`), with no "not in this build yet" left over from the proto
    /// pull request that added them, and every phrase from the string table.
    #[test]
    fn targetings_sites_read_as_the_text_the_author_wrote() {
        let cover = concat!(
            "{\"schema_version\":{\"major\":1},",
            "\"meta\":{\"title\":\"Cover\",\"author_kind\":\"HUMAN\"},",
            "\"declarative\":{\"route\":[",
            "{\"label\":\"cover\",\"place_beacon\":{\"at\":{\"covering\":",
            "{\"vent\":{\"rank\":\"NEAREST\",\"coverage\":\"UNCOVERED\"}}},",
            "\"initial\":{\"mandate\":{\"build\":{\"targets\":[{\"blueprint_id\":\"generator\",",
            "\"anchor\":{\"on\":{\"covered\":{}}},\"order\":1}]}}}}},",
            "{\"label\":\"tap\",\"interface\":{\"beacon\":{\"beacon_id\":\"b_00\"},\"rows\":[",
            "{\"add_build_target\":{\"target\":{\"blueprint_id\":\"generator\",",
            "\"anchor\":{\"on\":{\"vent\":{\"rank\":\"NEAREST\"}}}}}},",
            "{\"remove_build_target\":{\"anchor\":{\"on\":{\"feature_id\":\"vent_120_88\"}}}}]}},",
            "{\"label\":\"dig\",\"place_beacon\":{\"at\":{\"covering\":",
            "{\"feature_id\":\"seam_14_9\"}}}}]},",
            "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
            "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
            "\"kind\":\"PLAYBOOK\"}"
        );
        let text = prose(cover);
        for (line, phrase) in [
            (
                "covering",
                format!(
                    "{} {} {}{} ({})",
                    s::SITE_COVERING,
                    s::BEACON_NEAREST,
                    s::VENT_NOUN,
                    s::NOT_COVERED,
                    s::PICK_NOTE
                ),
            ),
            (
                "on",
                format!(
                    "{} {} {} ({})",
                    s::SITE_ON,
                    s::BEACON_NEAREST,
                    s::VENT_NOUN,
                    s::PICK_NOTE
                ),
            ),
            (
                "a vent's name",
                format!("{} {} (120, 88)", s::SITE_ON, s::VENT_NAME),
            ),
            (
                "a seam's name",
                format!("{} {} (14, 9)", s::SITE_COVERING, s::SEAM_NAME),
            ),
        ] {
            assert!(
                text.contains(&phrase),
                "{line}: `{phrase}` is not in\n{text}"
            );
        }
        assert!(
            !text.contains("not in this build"),
            "the proto lane's placeholder wording survived:\n{text}"
        );
        // `covered` sits in the deploy's initial settings, which the prose
        // names as a mandate without listing (the module doc's narrowing), so
        // its phrase is checked where it is built.
        let covered = pharmakos_proto::gp::v1::FeatureRef {
            r#ref: Some(super::feature_ref::Ref::Covered(
                pharmakos_proto::gp::v1::Covered {},
            )),
        };
        assert_eq!(super::feature_ref(&covered), s::FEATURE_COVERED);
        assert_eq!(super::feature_name("vent_1"), "feature \"vent_1\"");
        assert_eq!(super::feature_name("seam_-3_4"), "Scrap seam (-3, 4)");
        // A name the verifier would refuse, because it is not the canonical
        // spelling, is quoted rather than read as the vent it is not.
        for odd in ["vent_+1_2", "vent_01_2", "vent_1_02", "seam_-0_4"] {
            assert_eq!(super::feature_name(odd), format!("feature \"{odd}\""));
        }
    }

    /// S2's base kit (S2's plan, task `con4`): a `wall` target reads as its
    /// blueprint, its anchor and the box it fills, corner to corner, which
    /// the sim and the verifier refuse until S2's `site` lane; the prose says
    /// what the file says and never whether it is legal.
    #[test]
    fn a_wall_target_names_its_box() {
        let wall = concat!(
            "{\"schema_version\":{\"major\":1},",
            "\"meta\":{\"title\":\"Wall\",\"author_kind\":\"HUMAN\"},",
            "\"declarative\":{\"route\":[",
            "{\"label\":\"wall\",\"interface\":{\"beacon\":{\"beacon_id\":\"b_00\"},\"rows\":[",
            "{\"add_build_target\":{\"target\":{\"blueprint_id\":\"wall\",",
            "\"anchor\":{\"voxel\":{\"x\":30,\"y\":40,\"z\":5}},",
            "\"wall_box\":{\"min\":{\"x\":30,\"y\":40,\"z\":5},",
            "\"max\":{\"x\":38,\"y\":40,\"z\":7}}}}}]}}]},",
            "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
            "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
            "\"kind\":\"PLAYBOOK\"}"
        );
        let text = prose(wall);
        let phrase = format!(
            "{} wall at {} 30, 40, 5, {} {} 30, 40, 5 {} {} 38, 40, 7",
            s::ROW_ADD_TARGET,
            s::PLACE_VOXEL,
            s::WALL_BOX_FROM,
            s::PLACE_VOXEL,
            s::WALL_BOX_TO,
            s::PLACE_VOXEL
        );
        assert!(
            text.contains(&phrase),
            "`{phrase}` is not in
{text}"
        );

        // A box missing a corner says so rather than inventing one.
        let area = pharmakos_proto::gp::v1::Area {
            min: None,
            max: Some(pharmakos_proto::gp::v1::Voxel { x: 1, y: 2, z: 3 }),
        };
        assert_eq!(super::corner(area.min.as_ref()), s::PLACE_NOWHERE);
        assert_eq!(
            super::corner(area.max.as_ref()),
            format!("{} 1, 2, 3", s::PLACE_VOXEL)
        );
    }
}
