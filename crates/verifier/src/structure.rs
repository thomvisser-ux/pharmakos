// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stage 2, **structure**: shape and limits.
//!
//! Everything that can be decided by walking the tree once, with no symbol table
//! and no knowledge of the world: the blocks a playbook must have, the names its
//! parts must carry, the ranges its numbers must sit in, the shape of its
//! conditions, and the one size budget.
//!
//! # Limits and termination (spec section 10)
//!
//! "Every playbook halts. These limits are technical — bounded cost and
//! guaranteed termination — and in the story they are the Ledger's
//! seal-inspection rules." This stage owns three of the four:
//!
//! * **Size** — one budget, and `size_units` is on the report whether the
//!   playbook fits or not, so the editor's meter reads the verifier's number
//!   rather than counting for itself.
//! * **Rule firing** — a cooldown at or above the floor, and `max_fires` between
//!   one and the ceiling. Both numbers are rules-table rows.
//! * **Conditions** — four levels and twenty-four nodes, integer comparisons
//!   only.
//!
//! The fourth, "jumps go forward and every wait has a timeout", needs the label
//! table and belongs to [`crate::semantics`].
//!
//! # Durations are `int32` milliseconds and are never negative
//!
//! Decisions-log item 46: every duration in `gp.v1` is `int32` game
//! milliseconds, so the canonical JSON writes a bare number and spec section
//! 10's worked example loads as written — and a negative one is **rejected here,
//! with a JSON Pointer**, rather than at decode where there would be no pointer
//! to give. `E0109` is that rejection.

use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::v1::handler::Resume;
use pharmakos_proto::gp::v1::on_death::OnRespawn;
use pharmakos_proto::gp::v1::{
    Condition, Handler, IntCompare, Playbook, Step, condition, int_compare, interface_row, step,
};

use crate::limits::{CONDITION_MAX_DEPTH, CONDITION_MAX_NODES, Limits};
use crate::pointer;
use crate::report::{Builder, Diag, number, patch_add, patch_replace};
use crate::size;

/// Run the stage.
pub(crate) fn run(playbook: &Playbook, limits: &Limits, out: &mut Builder) {
    required_blocks(playbook, out);
    budget(playbook, limits, out);

    if let Some(declarative) = playbook.declarative.as_ref() {
        if declarative.route.is_empty() {
            out.emit(Diag::new("E0102", "/declarative/route"));
        }
        for (index, entry) in declarative.route.iter().enumerate() {
            walk_route_entry(entry, &pointer::at("/declarative/route", index), out);
        }
        unique_names(
            declarative.route.iter().map(|entry| entry.label.as_str()),
            "/declarative/route",
            "within the route",
            "label",
            out,
        );
        for (index, handler) in declarative.handlers.iter().enumerate() {
            walk_handler(
                handler,
                &pointer::at("/declarative/handlers", index),
                limits,
                out,
            );
        }
        unique_names(
            declarative
                .handlers
                .iter()
                .map(|handler| handler.id.as_str()),
            "/declarative/handlers",
            "within the playbook",
            "id",
            out,
        );
    }

    if let Some(on_death) = playbook.on_death.as_ref() {
        if on_death.on_respawn() == OnRespawn::Unspecified {
            out.emit(
                Diag::new("E0111", "/on_death/on_respawn")
                    .arg("field", "on_respawn")
                    .fix(
                        "Continue the route after a respawn",
                        patch_replace(
                            "/on_death/on_respawn",
                            &pharmakos_proto::json::Json::String("CONTINUE".to_owned()),
                        ),
                        Applicability::MaybeIncorrect,
                    ),
            );
        }
    }

    if let Some(tail) = playbook.fallback.as_ref() {
        if tail.posture.is_none() {
            out.emit(Diag::new("E0111", "/fallback").arg("field", "posture"));
        }
    }
}

/// The four blocks every playbook carries (`proto/gp/v1/playbook.proto`:
/// "Required" means the verifier rejects the playbook without it).
fn required_blocks(playbook: &Playbook, out: &mut Builder) {
    if playbook.meta.is_none() {
        out.emit(Diag::new("E0101", "/meta").arg("what", "`meta`"));
    }
    if playbook.declarative.is_none() {
        out.emit(Diag::new("E0101", "/declarative").arg("what", "`declarative`"));
    }
    if playbook.on_death.is_none() {
        out.emit(
            Diag::new("E0101", "/on_death")
                .arg("what", "the `on_death` block")
                .fix(
                    "Add an on_death block",
                    patch_add("/on_death", &on_death_stub()),
                    Applicability::HasPlaceholders,
                ),
        );
    }
    if playbook.fallback.is_none() {
        out.emit(
            Diag::new("E0101", "/fallback")
                .arg("what", "the `fallback` block")
                .fix(
                    "Add a fallback that holds at the safest beacon",
                    patch_add("/fallback", &fallback_stub()),
                    Applicability::HasPlaceholders,
                ),
        );
    }
}

/// `{"on_respawn":"CONTINUE","max_deaths_before_fallback":2}`.
///
/// `HAS_PLACEHOLDERS`, not `MACHINE_APPLICABLE`: how many deaths a playbook
/// tolerates is the author's judgement and this suggestion only makes the file
/// loadable.
fn on_death_stub() -> pharmakos_proto::json::Json {
    pharmakos_proto::json::Json::Object(vec![
        (
            "on_respawn".to_owned(),
            pharmakos_proto::json::Json::String("CONTINUE".to_owned()),
        ),
        ("max_deaths_before_fallback".to_owned(), number(2)),
    ])
}

/// `{"hold":{"at":{"beacon_anchor":{"safest":{}}}}}` — the guaranteed tail of
/// spec section 10, in its plainest form.
fn fallback_stub() -> pharmakos_proto::json::Json {
    use pharmakos_proto::json::Json;
    Json::Object(vec![(
        "hold".to_owned(),
        Json::Object(vec![(
            "at".to_owned(),
            Json::Object(vec![(
                "beacon_anchor".to_owned(),
                Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))]),
            )]),
        )]),
    )])
}

/// The one size budget (item 94).
fn budget(playbook: &Playbook, limits: &Limits, out: &mut Builder) {
    let units = size::size_units(playbook);
    if units > limits.size_budget_units() {
        out.emit(
            Diag::new("E0110", "/declarative")
                .arg("found", units)
                .arg("budget", limits.size_budget_units()),
        );
    }
}

/// One route step.
fn walk_route_entry(entry: &Step, at: &str, out: &mut Builder) {
    if entry.label.is_empty() {
        out.emit(Diag::new("E0104", at).arg("what", "the step"));
    }
    if entry.timeout_ms < 0 {
        negative(out, at, "timeout_ms", entry.timeout_ms);
    }
    if let Some(guard) = entry.skip_if.as_ref() {
        walk_condition_root(guard, &pointer::child(at, "skip_if"), out);
    }
    let Some(kind) = entry.kind.as_ref() else {
        out.emit(Diag::new("E0103", at));
        return;
    };
    walk_action(kind, at, out);
}

/// PLACEHOLDER — how an **omitted enum inside the body** reads.
///
/// `proto/gp/v1/playbook.proto`'s header says an unset enum is a verifier error
/// and names exactly one exception (`BeaconFilter`) and one open case
/// (`MineSettings.seam_choice`). Two more fields are in the same position and
/// cannot take the rule as written, because spec section 10's worked example —
/// which `examples/playbooks/expand_east.jsonc` copies verbatim and which the
/// schema is checked against — omits them: `MoveStep.pace` on its third step,
/// and `MineSettings.pillar_spacing` beside `seam_choice`. Checking them would
/// make the spec's own example fail to verify; defaulting them would invent a
/// default the section 6 mandate contract has not chosen.
///
/// So this stage checks **neither**, and records why here rather than silently.
/// **OWNER decides at S1**, with the rest of the section 6 mandate contract and
/// alongside `MineSettings`'s own PLACEHOLDER, which names the same stage; the
/// answer is one decision for all three, because an omitted setting reading two
/// ways in one file is the inconsistency the rule exists to prevent. The
/// envelope's own tags — `kind` and `author_kind` — are unaffected and are
/// checked at decode, because item 76 settles them by name.
fn walk_action(kind: &step::Kind, at: &str, out: &mut Builder) {
    match kind {
        step::Kind::Move(_) => {}
        step::Kind::Interface(interface) => {
            let here = pointer::child(at, "interface");
            if interface.beacon.is_none() {
                out.emit(
                    Diag::new("E0101", pointer::child(&here, "beacon")).arg("what", "`beacon`"),
                );
            }
            for (index, row) in interface.rows.iter().enumerate() {
                let row_at = pointer::at(&pointer::child(&here, "rows"), index);
                match row.row.as_ref() {
                    None => out.emit(Diag::new("E0101", row_at).arg("what", "the interface row")),
                    Some(interface_row::Row::QueueStructure(queue)) => {
                        if queue.blueprint_id.is_empty() {
                            out.emit(
                                Diag::new("E0101", pointer::child(&row_at, "queue_structure"))
                                    .arg("what", "`blueprint_id`"),
                            );
                        }
                    }
                    Some(_) => {}
                }
            }
        }
        step::Kind::PlaceBeacon(place) => {
            let here = pointer::child(at, "place_beacon");
            if place.at.is_none() {
                out.emit(Diag::new("E0101", pointer::child(&here, "at")).arg("what", "`at`"));
            }
        }
        step::Kind::WaitUntil(wait) => {
            let here = pointer::child(at, "wait_until");
            match wait.condition.as_ref() {
                None => out.emit(
                    Diag::new("E0101", pointer::child(&here, "condition"))
                        .arg("what", "`condition`"),
                ),
                Some(condition) => {
                    walk_condition_root(condition, &pointer::child(&here, "condition"), out);
                }
            }
        }
        step::Kind::Hold(hold) => {
            if hold.ms < 0 {
                negative(out, &pointer::child(at, "hold"), "ms", hold.ms);
            }
        }
        step::Kind::Broadcast(broadcast) => {
            if broadcast.payload.is_none() {
                out.emit(
                    Diag::new("E0101", pointer::child(at, "broadcast")).arg("what", "the payload"),
                );
            }
        }
    }
}

/// One handler: its name, its condition, its firing limits and its body.
fn walk_handler(handler: &Handler, at: &str, limits: &Limits, out: &mut Builder) {
    if handler.id.is_empty() {
        out.emit(Diag::new("E0104", at).arg("what", "the handler"));
    }
    match handler.when.as_ref() {
        None => out.emit(Diag::new("E0106", at)),
        Some(when) => walk_condition_root(when, &pointer::child(at, "when"), out),
    }
    firing_limits(handler, at, limits, out);
    if handler.resume() == Resume::Unspecified {
        out.emit(
            Diag::new("E0111", pointer::child(at, "resume"))
                .arg("field", "resume")
                .fix(
                    "Continue the route after the body",
                    patch_replace(
                        &pointer::child(at, "resume"),
                        &pharmakos_proto::json::Json::String("CONTINUE".to_owned()),
                    ),
                    Applicability::MaybeIncorrect,
                ),
        );
    }
    let body_at = pointer::child(at, "body");
    for (index, entry) in handler.body.iter().enumerate() {
        walk_route_entry(entry, &pointer::at(&body_at, index), out);
    }
    unique_names(
        handler.body.iter().map(|entry| entry.label.as_str()),
        &body_at,
        "within a handler body",
        "label",
        out,
    );
}

/// "Each rule has a cooldown >= 5 s and fires at most 1-8 times" (spec section
/// 10), with both numbers read from the rules table.
fn firing_limits(handler: &Handler, at: &str, limits: &Limits, out: &mut Builder) {
    let ceiling = limits.max_fires_max();
    if handler.max_fires < 1 || handler.max_fires > ceiling {
        let clamped = handler.max_fires.clamp(1, ceiling.max(1));
        out.emit(
            Diag::new("E0107", pointer::child(at, "max_fires"))
                .arg("found", handler.max_fires)
                .arg("max", ceiling)
                .fix(
                    format!("Set max_fires to {clamped}"),
                    patch_replace(&pointer::child(at, "max_fires"), &number(clamped)),
                    Applicability::MaybeIncorrect,
                ),
        );
    }
    let floor = limits.handler_cooldown_min_ms();
    if handler.cooldown_ms < 0 {
        negative(out, at, "cooldown_ms", handler.cooldown_ms);
    } else if handler.cooldown_ms < floor {
        out.emit(
            Diag::new("E0108", pointer::child(at, "cooldown_ms"))
                .arg("found", handler.cooldown_ms)
                .arg("min", floor)
                .fix(
                    format!("Set the cooldown to {floor} ms"),
                    patch_replace(&pointer::child(at, "cooldown_ms"), &number(floor)),
                    Applicability::MachineApplicable,
                ),
        );
    }
}

/// A duration that ran backwards (item 46).
fn negative(out: &mut Builder, at: &str, field: &'static str, found: i32) {
    out.emit(
        Diag::new("E0109", pointer::child(at, field))
            .arg("field", field)
            .arg("found", found)
            .fix(
                format!("Set {field} to 0"),
                patch_replace(&pointer::child(at, field), &number(0)),
                Applicability::MaybeIncorrect,
            ),
    );
}

/// Names must be unique in the list that addresses them.
///
/// The route's labels are what a jump and `step_reached` name; a handler body's
/// labels are what a jump inside that body names; a handler's id is what
/// `rule_fired` names. Each list is checked against itself, which is the scope
/// the references actually use — a body label may repeat a route label without
/// ambiguity, and nothing in the vocabulary can confuse the two.
fn unique_names<'a>(
    names: impl Iterator<Item = &'a str>,
    at: &str,
    scope: &'static str,
    field: &'static str,
    out: &mut Builder,
) {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for (index, name) in names.enumerate() {
        if name.is_empty() {
            continue;
        }
        match seen.iter().find(|(earlier, _)| *earlier == name) {
            Some((_, first)) => out.emit(
                Diag::new("E0105", pointer::child(&pointer::at(at, index), field))
                    .arg("name", name)
                    .arg("scope", scope)
                    .related(pointer::child(&pointer::at(at, *first), field)),
            ),
            None => seen.push((name, index)),
        }
    }
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

/// One whole condition: its two limits, then its shape.
fn walk_condition_root(condition: &Condition, at: &str, out: &mut Builder) {
    let depth = tree_depth(condition);
    if depth > CONDITION_MAX_DEPTH {
        out.emit(
            Diag::new("E0204", at)
                .arg("found", depth)
                .arg("max", CONDITION_MAX_DEPTH),
        );
    }
    let nodes = tree_nodes(condition);
    if nodes > CONDITION_MAX_NODES {
        out.emit(
            Diag::new("E0205", at)
                .arg("found", nodes)
                .arg("max", CONDITION_MAX_NODES),
        );
    }
    walk_condition(condition, at, out);
}

/// How deeply a condition nests. A bare predicate is one level.
fn tree_depth(condition: &Condition) -> u32 {
    let inner = match condition.node.as_ref() {
        Some(condition::Node::All(all)) => all.items.iter().map(tree_depth).max().unwrap_or(0),
        Some(condition::Node::Any(any)) => any.items.iter().map(tree_depth).max().unwrap_or(0),
        Some(condition::Node::Not(not)) => not.item.as_deref().map_or(0, tree_depth),
        _ => 0,
    };
    inner.saturating_add(1)
}

/// How many nodes a condition holds, itself included.
fn tree_nodes(condition: &Condition) -> u32 {
    let inner: u32 = match condition.node.as_ref() {
        Some(condition::Node::All(all)) => all
            .items
            .iter()
            .map(tree_nodes)
            .fold(0, u32::saturating_add),
        Some(condition::Node::Any(any)) => any
            .items
            .iter()
            .map(tree_nodes)
            .fold(0, u32::saturating_add),
        Some(condition::Node::Not(not)) => not.item.as_deref().map_or(0, tree_nodes),
        _ => 0,
    };
    inner.saturating_add(1)
}

/// One condition node's shape, and then its children.
fn walk_condition(condition: &Condition, at: &str, out: &mut Builder) {
    let Some(node) = condition.node.as_ref() else {
        out.emit(Diag::new("E0206", at));
        return;
    };
    match node {
        condition::Node::All(all) => {
            let here = pointer::child(at, "all");
            if all.items.is_empty() {
                out.emit(Diag::new("E0203", here.clone()).arg("node", "all"));
            }
            walk_items(&all.items, &pointer::child(&here, "items"), out);
        }
        condition::Node::Any(any) => {
            let here = pointer::child(at, "any");
            if any.items.is_empty() {
                out.emit(Diag::new("E0203", here.clone()).arg("node", "any"));
            }
            walk_items(&any.items, &pointer::child(&here, "items"), out);
        }
        condition::Node::Not(not) => {
            let here = pointer::child(at, "not");
            match not.item.as_ref() {
                None => out.emit(Diag::new("E0203", here).arg("node", "not")),
                Some(inner) => walk_condition(inner, &pointer::child(&here, "item"), out),
            }
        }
        other => walk_predicate(other, at, out),
    }
}

fn walk_items(items: &[Condition], at: &str, out: &mut Builder) {
    for (index, item) in items.iter().enumerate() {
        walk_condition(item, &pointer::at(at, index), out);
    }
}

/// One leaf predicate: its comparison, its ranges, its durations.
fn walk_predicate(node: &condition::Node, at: &str, out: &mut Builder) {
    match node {
        condition::Node::CmdrHpPct(predicate) => {
            let here = pointer::child(at, "cmdr_hp_pct");
            if predicate.cmp() == pharmakos_proto::gp::v1::cmdr_hp_pct::Cmp::Unspecified {
                out.emit(Diag::new("E0201", pointer::child(&here, "cmp")));
            }
            percent(out, &pointer::child(&here, "pct"), i64::from(predicate.pct));
        }
        condition::Node::CmdrTookDamageWithin(predicate) => {
            let here = pointer::child(at, "cmdr_took_damage_within");
            if predicate.ms < 0 {
                negative(out, &here, "ms", predicate.ms);
            }
        }
        condition::Node::CmdrDeaths(predicate) => {
            compare(
                predicate.deaths.as_ref(),
                &pointer::child(at, "cmdr_deaths"),
                "deaths",
                false,
                out,
            );
        }
        condition::Node::SegmentElapsed(predicate) => {
            compare(
                predicate.ms.as_ref(),
                &pointer::child(at, "segment_elapsed"),
                "ms",
                false,
                out,
            );
        }
        condition::Node::BeaconHpPct(predicate) => {
            let here = pointer::child(at, "beacon_hp_pct");
            beacon_ref(predicate.beacon.is_some(), &here, out);
            compare(predicate.pct.as_ref(), &here, "pct", true, out);
        }
        condition::Node::BeaconUnderAttack(predicate) => {
            let here = pointer::child(at, "beacon_under_attack");
            beacon_ref(predicate.beacon.is_some(), &here, out);
            if predicate.within_ms < 0 {
                negative(out, &here, "within_ms", predicate.within_ms);
            }
        }
        condition::Node::BeaconPowered(predicate) => {
            beacon_ref(
                predicate.beacon.is_some(),
                &pointer::child(at, "beacon_powered"),
                out,
            );
        }
        condition::Node::Treasury(predicate) => {
            compare(
                predicate.dollars.as_ref(),
                &pointer::child(at, "treasury"),
                "dollars",
                false,
                out,
            );
        }
        condition::Node::KwHeadroom(predicate) => {
            compare(
                predicate.kw.as_ref(),
                &pointer::child(at, "kw_headroom"),
                "kw",
                false,
                out,
            );
        }
        condition::Node::StepReached(predicate) => {
            if predicate.label.is_empty() {
                out.emit(
                    Diag::new(
                        "E0101",
                        pointer::child(&pointer::child(at, "step_reached"), "label"),
                    )
                    .arg("what", "`label`"),
                );
            }
        }
        condition::Node::RuleFired(predicate) => {
            if predicate.handler_id.is_empty() {
                out.emit(
                    Diag::new(
                        "E0101",
                        pointer::child(&pointer::child(at, "rule_fired"), "handler_id"),
                    )
                    .arg("what", "`handler_id`"),
                );
            }
        }
        condition::Node::All(_) | condition::Node::Any(_) | condition::Node::Not(_) => {}
    }
}

/// A predicate's `BeaconRef`, which is required wherever it appears.
fn beacon_ref(present: bool, at: &str, out: &mut Builder) {
    if !present {
        out.emit(Diag::new("E0101", pointer::child(at, "beacon")).arg("what", "`beacon`"));
    }
}

/// One `IntCompare`: present, with an operator, and inside its range if it is a
/// percentage.
fn compare(
    value: Option<&IntCompare>,
    at: &str,
    field: &'static str,
    is_percent: bool,
    out: &mut Builder,
) {
    let here = pointer::child(at, field);
    let Some(comparison) = value else {
        out.emit(Diag::new("E0101", here).arg("what", format!("`{field}`")));
        return;
    };
    if comparison.op() == int_compare::Op::Unspecified {
        out.emit(Diag::new("E0201", pointer::child(&here, "op")));
    }
    if is_percent {
        percent(
            out,
            &pointer::child(&here, "value"),
            i64::from(comparison.value),
        );
    }
}

/// "Percent, integer. 0-100."
fn percent(out: &mut Builder, at: &str, found: i64) {
    if (0..=100).contains(&found) {
        return;
    }
    let clamped = found.clamp(0, 100);
    out.emit(Diag::new("E0202", at).arg("found", found).fix(
        format!("Clamp it to {clamped}"),
        patch_replace(at, &number(clamped)),
        Applicability::MaybeIncorrect,
    ));
}
