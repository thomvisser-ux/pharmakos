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
//! editor draws; the last is not in the frozen snapshot yet
//! ([`crate::context::segment_length_ms`]) and is left out rather than guessed.

use pharmakos_proto::gp::v1::{
    BeaconFilter, BeaconRef, Condition, Fallback, Handler, InterfaceRow, Location, MandateSettings,
    OnDeath, Options, Playbook, Step, Voxel, beacon_filter, beacon_ref, cmdr_hp_pct, condition,
    fallback, handler, int_compare, interface_row, location, mandate_settings, meta, on_death,
    on_fail, playbook, step,
};
use std::fmt::Write as _;

use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::rules::RulesTable;
use pharmakos_verifier::{Limits, size_units};

use crate::context::PlanContext;
use crate::error::Error;
use crate::interface::{self, Rates};
use crate::strings as s;

/// Renders a playbook as English prose.
///
/// # Errors
///
/// When the rules table is missing a block the rendering reads: the
/// section-5 interface rates, or the verifier's size budget. A rendering that
/// silently priced a visit at nothing, or showed a budget of zero, would be
/// worse than no rendering.
pub fn render_plan(
    playbook: &Playbook,
    context: &PlanContext,
    rules: &RulesTable,
) -> Result<String, Error> {
    let rates = Rates::from_rules(rules)?;
    let limits = Limits::from_rules(rules)
        .map_err(|gap| Error::at("", format!("the rules table is incomplete: {gap}")))?;

    let mut out = String::new();
    // `context` is the frozen snapshot's half of the rendering. Nothing in the
    // prose reads it yet, because the one fact the prose would take from it —
    // the coming segment's length — is not in the snapshot until T10
    // (`crate::context::segment_length_ms`), and a rendering that guessed it
    // would be the constant spec section 13 forbids. The argument is here from
    // the start so that filling it in is a line in this function rather than a
    // signature change in every caller.
    let _ = context.segment_ms();
    envelope(playbook, &mut out);
    route(playbook, &rates, &mut out);
    rules_section(playbook, &rates, &mut out);
    blocks(playbook, &mut out);
    budget(playbook, &rates, limits.size_budget_units(), &mut out);
    Ok(out)
}

// ---------------------------------------------------------------------------
// The envelope
// ---------------------------------------------------------------------------

fn envelope(book: &Playbook, out: &mut String) {
    let meta = book.meta.as_ref();
    let title = meta
        .map(|meta| meta.title.trim())
        .filter(|title| !title.is_empty())
        .unwrap_or(s::UNTITLED);
    line(0, title, out);
    line(
        0,
        match meta.map(pharmakos_proto::gp::v1::Meta::author_kind) {
            Some(meta::AuthorKind::Human) => s::AUTHOR_HUMAN,
            Some(meta::AuthorKind::Builtin) => s::AUTHOR_BUILTIN,
            Some(meta::AuthorKind::Script) => s::AUTHOR_SCRIPT,
            Some(meta::AuthorKind::Unspecified) | None => s::AUTHOR_UNKNOWN,
        },
        out,
    );
    match book.kind() {
        playbook::Kind::Template => line(0, s::KIND_TEMPLATE, out),
        playbook::Kind::Sample => line(0, s::KIND_SAMPLE, out),
        playbook::Kind::Playbook | playbook::Kind::Unspecified => {}
    }
    if let Some(note) = meta
        .map(|meta| meta.note.trim())
        .filter(|note| !note.is_empty())
    {
        for sentence in note.lines() {
            line(0, sentence.trim(), out);
        }
    }
}

// ---------------------------------------------------------------------------
// The route
// ---------------------------------------------------------------------------

fn route(book: &Playbook, rates: &Rates, out: &mut String) {
    let Some(body) = book.declarative.as_ref() else {
        return;
    };
    if body.route.is_empty() {
        return;
    }
    heading(s::HEADING_ROUTE, out);
    for (index, entry) in body.route.iter().enumerate() {
        let number = index.saturating_add(1);
        line(
            1,
            &format!("{number}. {}", step_sentence(entry, rates)),
            out,
        );
        for guard in guards(entry) {
            line(2, &guard, out);
        }
    }
}

fn step_sentence(entry: &Step, rates: &Rates) -> String {
    let label = entry.label.trim();
    let body = step_body(entry, rates);
    if label.is_empty() {
        format!("{body}.")
    } else {
        format!("{label}: {body}.")
    }
}

fn step_body(entry: &Step, rates: &Rates) -> String {
    match entry.kind.as_ref() {
        None => s::STEP_NOTHING.to_owned(),
        Some(step::Kind::Move(walk)) => {
            let pace = match walk.pace() {
                pharmakos_proto::gp::v1::move_step::Pace::Direct => s::PACE_DIRECT,
                pharmakos_proto::gp::v1::move_step::Pace::AvoidKnownThreats => s::PACE_AVOID,
                pharmakos_proto::gp::v1::move_step::Pace::Unspecified => "",
            };
            format!("{} {}{pace}", s::STEP_MOVE, location(walk.to.as_ref()))
        }
        Some(step::Kind::Interface(visit)) => {
            let rows: Vec<String> = visit.rows.iter().map(row_sentence).collect();
            let spent = interface::interface_ms(visit, rates).total;
            let what = if rows.is_empty() {
                String::new()
            } else {
                format!(" and {}", join(&rows))
            };
            format!(
                "{} {}{what} ({} on site)",
                s::STEP_INTERFACE,
                beacon(visit.beacon.as_ref()),
                duration(spent)
            )
        }
        Some(step::Kind::PlaceBeacon(place)) => {
            let spent = interface::place_beacon_ms(place, rates).total;
            let mut sentence = format!("{} {}", s::STEP_PLACE_BEACON, location(place.at.as_ref()));
            if !place.tags.is_empty() {
                let tags: Vec<String> = place.tags.iter().map(|tag| format!("\"{tag}\"")).collect();
                let _ = write!(sentence, ", {} {}", s::TAGGED, join(&tags));
            }
            if let Some(initial) = place.initial.as_ref() {
                if let Some(settings) = initial.mandate.as_ref() {
                    let _ = write!(sentence, ", on a {} mandate", writ(settings));
                }
                if initial.priority != 0 {
                    let _ = write!(
                        sentence,
                        ", at {} Quartermaster priority",
                        priority(initial.priority)
                    );
                }
            }
            let _ = write!(sentence, " ({} on site)", duration(spent));
            sentence
        }
        Some(step::Kind::WaitUntil(wait)) => {
            format!(
                "{} {}",
                s::STEP_WAIT_UNTIL,
                condition(wait.condition.as_ref())
            )
        }
        Some(step::Kind::Hold(hold)) => {
            format!("{} {}", s::STEP_HOLD, duration(Ms::new(hold.ms)))
        }
        Some(step::Kind::Broadcast(_)) => s::STEP_BROADCAST.to_owned(),
    }
}

fn guards(entry: &Step) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(skip) = entry.skip_if.as_ref() {
        found.push(format!("{} {}.", s::GUARD_SKIP_IF, condition(Some(skip))));
    }
    if entry.timeout_ms != 0 {
        found.push(format!(
            "{} {}.",
            s::GUARD_TIMEOUT,
            duration(Ms::new(entry.timeout_ms))
        ));
    }
    if let Some(fail) = entry.on_fail.as_ref() {
        found.push(match fail.action() {
            on_fail::Action::Skip => s::ON_FAIL_SKIP.to_owned(),
            on_fail::Action::AbortRoute => s::ON_FAIL_ABORT.to_owned(),
            on_fail::Action::JumpForward => {
                format!("{} \"{}\".", s::ON_FAIL_JUMP, fail.jump_to_label)
            }
            on_fail::Action::OnFailUnspecified => String::new(),
        });
        found.retain(|sentence| !sentence.is_empty());
    }
    found
}

// ---------------------------------------------------------------------------
// Interface rows
// ---------------------------------------------------------------------------

fn row_sentence(row: &InterfaceRow) -> String {
    match row.row.as_ref() {
        None => s::ROW_NOTHING.to_owned(),
        Some(interface_row::Row::SetMandate(settings)) => {
            format!("{} {} mandate", s::ROW_SET_MANDATE, writ(settings))
        }
        Some(interface_row::Row::SetMandateSettings(_)) => s::ROW_SET_SETTINGS.to_owned(),
        Some(interface_row::Row::SetPriority(value)) => {
            format!("{} {}", s::ROW_SET_PRIORITY, priority(*value))
        }
        Some(interface_row::Row::Recycle(_)) => s::ROW_RECYCLE.to_owned(),
        Some(interface_row::Row::AddBuildTarget(add)) => {
            let target = add.target.as_ref();
            let blueprint = target.map_or_else(String::new, |target| target.blueprint_id.clone());
            let anchor = location(target.and_then(|target| target.anchor.as_ref()));
            format!("{} {blueprint} at {anchor}", s::ROW_ADD_TARGET)
        }
        Some(interface_row::Row::RemoveBuildTarget(remove)) => {
            format!(
                "{} {}",
                s::ROW_REMOVE_TARGET,
                location(remove.anchor.as_ref())
            )
        }
        Some(interface_row::Row::QueueStructure(queue)) => {
            format!("{} {}", s::ROW_QUEUE, queue.blueprint_id)
        }
    }
}

fn writ(settings: &MandateSettings) -> &'static str {
    match settings.mandate.as_ref() {
        Some(mandate_settings::Mandate::Build(_)) => "Build",
        Some(mandate_settings::Mandate::Defend(_)) => "Defend",
        Some(mandate_settings::Mandate::Attack(_)) => "Attack",
        Some(mandate_settings::Mandate::Survey(_)) => "Survey",
        Some(mandate_settings::Mandate::Mine(_)) => "Mine",
        None => "writless",
    }
}

fn priority(value: i32) -> &'static str {
    match interface_row::QuartermasterPriority::try_from(value) {
        Ok(interface_row::QuartermasterPriority::Low) => "low",
        Ok(interface_row::QuartermasterPriority::Normal) => "normal",
        Ok(interface_row::QuartermasterPriority::High) => "high",
        Ok(interface_row::QuartermasterPriority::Unspecified) | Err(_) => "unset",
    }
}

// ---------------------------------------------------------------------------
// Places
// ---------------------------------------------------------------------------

fn location(place: Option<&Location>) -> String {
    match place.and_then(|place| place.place.as_ref()) {
        None => s::PLACE_NOWHERE.to_owned(),
        Some(location::Place::Voxel(at)) => voxel(at),
        Some(location::Place::BeaconAnchor(reference)) => beacon(Some(reference)),
        Some(location::Place::Safest(_)) => s::BEACON_SAFEST.to_owned(),
    }
}

fn voxel(at: &Voxel) -> String {
    format!("{} {}, {}, {}", s::PLACE_VOXEL, at.x, at.y, at.z)
}

fn beacon(reference: Option<&BeaconRef>) -> String {
    match reference.and_then(|reference| reference.r#ref.as_ref()) {
        None => s::BEACON_NONE.to_owned(),
        Some(beacon_ref::Ref::BeaconId(id)) => format!("{} \"{id}\"", s::BEACON_NOUN),
        Some(beacon_ref::Ref::Safest(_)) => s::BEACON_SAFEST.to_owned(),
        Some(beacon_ref::Ref::Nearest(pick)) => selector(s::BEACON_NEAREST, pick.filter.as_ref()),
        Some(beacon_ref::Ref::Weakest(pick)) => selector(s::BEACON_WEAKEST, pick.filter.as_ref()),
        Some(beacon_ref::Ref::MostThreatened(pick)) => {
            selector(s::BEACON_MOST_THREATENED, pick.filter.as_ref())
        }
    }
}

/// A selector, with the chip text spec section 13 asks for.
fn selector(superlative: &str, filter: Option<&BeaconFilter>) -> String {
    let mut sentence = superlative.to_owned();
    let side = match filter.map(BeaconFilter::side) {
        Some(beacon_filter::Side::EnemyKnown) => s::SIDE_ENEMY,
        _ => s::SIDE_OWN,
    };
    sentence.push(' ');
    sentence.push_str(side);
    if let Some(writ) = filter.map(BeaconFilter::mandate) {
        if writ != beacon_filter::MandateKind::Unspecified {
            sentence.push(' ');
            sentence.push_str(match writ {
                beacon_filter::MandateKind::Build => "Build",
                beacon_filter::MandateKind::Defend => "Defend",
                beacon_filter::MandateKind::Attack => "Attack",
                beacon_filter::MandateKind::Survey => "Survey",
                beacon_filter::MandateKind::Mine => "Mine",
                beacon_filter::MandateKind::Unspecified => "",
            });
        }
    }
    sentence.push(' ');
    sentence.push_str(s::BEACON_NOUN);
    if let Some(tags) = filter.map(|filter| &filter.tags) {
        if !tags.is_empty() {
            let quoted: Vec<String> = tags.iter().map(|tag| format!("\"{tag}\"")).collect();
            let _ = write!(sentence, " {} {}", s::TAGGED, join(&quoted));
        }
    }
    format!("{sentence} ({})", s::SELECTOR_NOTE)
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

fn condition(node: Option<&Condition>) -> String {
    let Some(node) = node.and_then(|node| node.node.as_ref()) else {
        return s::CONDITION_NOTHING.to_owned();
    };
    match node {
        condition::Node::All(group) => group_sentence(&group.items, s::AND),
        condition::Node::Any(group) => group_sentence(&group.items, s::OR),
        condition::Node::Not(inner) => {
            format!("{} {}", s::NOT, condition(inner.item.as_deref()))
        }
        condition::Node::CmdrHpPct(test) => {
            format!("the commander is {} {}% HP", cmp_hp(test.cmp()), test.pct)
        }
        condition::Node::CmdrTookDamageWithin(test) => format!(
            "the commander took damage in the last {}",
            duration(Ms::new(test.ms))
        ),
        condition::Node::CmdrDeaths(test) => format!(
            "the commander has died {}",
            compare(test.deaths.as_ref(), "times")
        ),
        condition::Node::SegmentElapsed(test) => {
            format!("the segment has run {}", compare_ms(test.ms.as_ref()))
        }
        condition::Node::BeaconHpPct(test) => format!(
            "{} is {}",
            beacon(test.beacon.as_ref()),
            compare(test.pct.as_ref(), "% HP")
        ),
        condition::Node::BeaconUnderAttack(test) => format!(
            "{} took damage in the last {}",
            beacon(test.beacon.as_ref()),
            duration(Ms::new(test.within_ms))
        ),
        condition::Node::BeaconPowered(test) => format!(
            "{} is {}",
            beacon(test.beacon.as_ref()),
            if test.powered { "powered" } else { "dormant" }
        ),
        condition::Node::Treasury(test) => {
            format!("the treasury is {}", compare_money(test.dollars.as_ref()))
        }
        condition::Node::KwHeadroom(test) => {
            format!("the grid's headroom is {}", compare(test.kw.as_ref(), "kW"))
        }
        condition::Node::StepReached(test) => {
            format!("the route has reached \"{}\"", test.label)
        }
        condition::Node::RuleFired(test) => {
            format!("rule \"{}\" has fired", test.handler_id)
        }
    }
}

fn group_sentence(items: &[Condition], joiner: &str) -> String {
    if items.is_empty() {
        return s::EMPTY_GROUP.to_owned();
    }
    let rendered: Vec<String> = items.iter().map(|item| condition(Some(item))).collect();
    rendered.join(joiner)
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

fn compare(test: Option<&pharmakos_proto::gp::v1::IntCompare>, unit: &str) -> String {
    test.map_or_else(
        || s::CONDITION_NOTHING.to_owned(),
        |test| {
            let spacer = if unit.starts_with('%') { "" } else { " " };
            format!("{} {}{spacer}{unit}", cmp_word(test.op()), test.value)
        },
    )
}

fn compare_money(test: Option<&pharmakos_proto::gp::v1::IntCompare>) -> String {
    test.map_or_else(
        || s::CONDITION_NOTHING.to_owned(),
        |test| format!("{} ${}", cmp_word(test.op()), test.value),
    )
}

fn compare_ms(test: Option<&pharmakos_proto::gp::v1::IntCompare>) -> String {
    test.map_or_else(
        || s::CONDITION_NOTHING.to_owned(),
        |test| format!("{} {}", cmp_word(test.op()), duration(Ms::new(test.value))),
    )
}

// ---------------------------------------------------------------------------
// Rules, blocks and the budget
// ---------------------------------------------------------------------------

fn rules_section(book: &Playbook, rates: &Rates, out: &mut String) {
    let Some(body) = book.declarative.as_ref() else {
        return;
    };
    if body.handlers.is_empty() {
        return;
    }
    heading(s::HEADING_RULES, out);
    for (index, rule) in body.handlers.iter().enumerate() {
        let number = index.saturating_add(1);
        line(
            1,
            &format!(
                "{number}. \"{}\", when {}:",
                rule.id,
                condition(rule.when.as_ref())
            ),
            out,
        );
        for (position, entry) in rule.body.iter().enumerate() {
            let step_number = position.saturating_add(1);
            line(
                2,
                &format!("{step_number}. {}", step_sentence(entry, rates)),
                out,
            );
            for guard in guards(entry) {
                line(3, &guard, out);
            }
        }
        line(2, &resume(rule), out);
        line(2, &firing(rule), out);
        let note = rule.note.trim();
        if !note.is_empty() {
            line(2, note, out);
        }
    }
}

fn resume(rule: &Handler) -> String {
    match rule.resume() {
        handler::Resume::Continue => "Then carry on with the route.".to_owned(),
        handler::Resume::SkipStep => "Then skip the step the route was on.".to_owned(),
        handler::Resume::JumpForward => {
            format!("Then jump forward to \"{}\".", rule.resume_at_label)
        }
        handler::Resume::EndRoute => "Then end the route.".to_owned(),
        handler::Resume::Unspecified => "Then do what the file does not say.".to_owned(),
    }
}

fn firing(rule: &Handler) -> String {
    format!(
        "Fires at most {} time{}, and no sooner than every {}.",
        rule.max_fires,
        if rule.max_fires == 1 { "" } else { "s" },
        duration(Ms::new(rule.cooldown_ms))
    )
}

fn blocks(book: &Playbook, out: &mut String) {
    heading(s::HEADING_ON_DEATH, out);
    let death = book.on_death.as_ref();
    line(
        1,
        match death.map(OnDeath::on_respawn) {
            Some(on_death::OnRespawn::Continue) => s::RESPAWN_CONTINUE,
            _ => s::RESPAWN_UNSET,
        },
        out,
    );
    if let Some(limit) = death.map(|death| death.max_deaths_before_fallback) {
        if limit != 0 {
            line(
                1,
                &format!("After {limit} deaths the fallback takes over."),
                out,
            );
        }
    }

    heading(s::HEADING_FALLBACK, out);
    line(1, &fallback(book.fallback.as_ref()), out);

    if let Some(options) = book
        .declarative
        .as_ref()
        .and_then(|body| body.options.as_ref())
    {
        if options_said(*options) {
            heading(s::HEADING_OPTIONS, out);
            if options.allow_dormant_beacons {
                line(1, s::ALLOW_DORMANT, out);
            }
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
            format!("{} {}.", s::FALLBACK_PATROL, join(&stops))
        }
    }
}

fn budget(book: &Playbook, rates: &Rates, size_budget: u32, out: &mut String) {
    heading(s::HEADING_BUDGET, out);
    line(
        1,
        &format!("{} of {size_budget} size units.", size_units(book)),
        out,
    );
    line(
        1,
        &format!(
            "{} standing at a beacon along the route.",
            duration(interface::route_ms(book, rates))
        ),
        out,
    );
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

fn heading(title: &str, out: &mut String) {
    out.push('\n');
    line(0, title, out);
}

fn line(depth: usize, text: &str, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
    out.push_str(text);
    out.push('\n');
}

/// `a`, `a and b`, `a, b and c` — the Oxford-free reading a sentence wants.
fn join(parts: &[String]) -> String {
    match parts.len() {
        0 => String::new(),
        1 => parts.first().cloned().unwrap_or_default(),
        _ => {
            let last = parts.last().cloned().unwrap_or_default();
            let head: Vec<String> = parts
                .get(..parts.len().saturating_sub(1))
                .unwrap_or_default()
                .to_vec();
            format!("{} and {last}", head.join(", "))
        }
    }
}

/// An exact duration, as the file wrote it: `8 s`, `1.5 s`, `2:00`.
///
/// Exact rather than rounded, unlike [`crate::travel::whole_seconds`]: these
/// are authored numbers and rules-table arithmetic, not estimates, and
/// rounding one would be the rendering inventing a fact.
#[must_use]
pub fn duration(ms: Ms) -> String {
    let total = ms.raw();
    if total < 0 {
        return format!("minus {}", duration(Ms::new(total.saturating_neg())));
    }
    let seconds = total.checked_div(1000).unwrap_or(0);
    let rest = total.saturating_sub(seconds.saturating_mul(1000));
    if rest == 0 && seconds >= 60 {
        let minutes = seconds.checked_div(60).unwrap_or(0);
        let spare = seconds.saturating_sub(minutes.saturating_mul(60));
        return format!("{minutes}:{spare:02}");
    }
    if rest == 0 {
        return format!("{seconds} s");
    }
    if rest.checked_rem(100) == Some(0) {
        let tenths = rest.checked_div(100).unwrap_or(0);
        return format!("{seconds}.{tenths} s");
    }
    format!("{seconds}.{rest:03} s")
}

#[cfg(test)]
mod tests {
    use super::{duration, join, render_plan};
    use crate::canonical::canonicalise_text;
    use crate::context::PlanContext;
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
        let canonical = canonicalise_text(text).expect("a playbook");
        render_plan(&canonical.playbook, &context(), &rules()).expect("the rules table is complete")
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
    }

    #[test]
    fn lists_read_as_sentences() {
        assert_eq!(join(&[]), "");
        assert_eq!(join(&["a".to_owned()]), "a");
        assert_eq!(join(&["a".to_owned(), "b".to_owned()]), "a and b");
        assert_eq!(
            join(&["a".to_owned(), "b".to_owned(), "c".to_owned()]),
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
            text.starts_with("Hold\nFiled by the built-in operator.\n"),
            "{text}"
        );
        assert!(text.contains("1. wait: hold for 1:30."), "{text}");
        assert!(text.contains("Hold at the safest own beacon."), "{text}");
    }

    #[test]
    fn the_size_meter_reads_the_rules_table_budget() {
        let text = prose(MINIMAL);
        assert!(text.contains("1 of 128 size units."), "{text}");
    }

    #[test]
    fn nothing_is_claimed_about_the_segments_length() {
        let text = prose(MINIMAL);
        assert!(
            !text.contains("segment is"),
            "the snapshot does not carry the segment length yet:\n{text}"
        );
    }
}
