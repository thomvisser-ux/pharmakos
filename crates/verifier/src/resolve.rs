// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stage 3, **resolve**: names to the things they name.
//!
//! "Every reference resolves against the seat's frozen knowledge snapshot, or
//! has a none-policy" (spec section 11). Three kinds of name are resolved here:
//!
//! * **Labels** — a jump's target and `step_reached`'s subject must be a step
//!   that exists. A route jump resolves inside the route; a jump inside a
//!   handler body resolves inside that body, because that is the list the body
//!   runs down.
//! * **Handler ids** — `rule_fired` must name a handler this playbook declares.
//! * **Beacon ids** — a fixed `beacon_id` must be a beacon the seat's snapshot
//!   holds, matched **exactly**. Bindings are per match (`gp.v1.BeaconRef`), so
//!   a near miss is a diagnostic rather than a guess, and a saved playbook
//!   carried into another match is told so rather than quietly pointed at
//!   somebody else's beacon.
//!
//! A **late-bound selector** resolves nothing here and is not meant to: it
//! resolves when the step starts and stays pinned for that step (spec section
//! 10). Resolving to nothing at run time is a step failure that `on_fail`
//! answers — never a silent skip, and never something this stage can promise
//! against.
//!
//! The stage also builds the [`Symbols`] table, because the forwardness of a
//! jump is a question about *positions* and belongs to [`crate::semantics`].

use pharmakos_proto::gp::v1::{BeaconRef, Condition, Playbook, Step, beacon_ref, condition};

use crate::pointer;
use crate::report::{Builder, Diag};
use crate::scope::Scope;
use crate::walk::{self, List, Visit};

/// Every name a playbook declares, in the order it declares them.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct Symbols {
    route_labels: Vec<String>,
    handler_ids: Vec<String>,
    body_labels: Vec<Vec<String>>,
}

impl Symbols {
    /// Read the tables out of a playbook.
    pub(crate) fn of(playbook: &Playbook) -> Symbols {
        let Some(declarative) = playbook.declarative.as_ref() else {
            return Symbols::default();
        };
        Symbols {
            route_labels: declarative
                .route
                .iter()
                .map(|entry| entry.label.clone())
                .collect(),
            handler_ids: declarative
                .handlers
                .iter()
                .map(|handler| handler.id.clone())
                .collect(),
            body_labels: declarative
                .handlers
                .iter()
                .map(|handler| {
                    handler
                        .body
                        .iter()
                        .map(|entry| entry.label.clone())
                        .collect()
                })
                .collect(),
        }
    }

    /// Where a label sits in the route, if it is there at all.
    pub(crate) fn route_index(&self, label: &str) -> Option<usize> {
        index_of(&self.route_labels, label)
    }

    /// Where a label sits in one handler's body.
    pub(crate) fn body_index(&self, handler: usize, label: &str) -> Option<usize> {
        self.body_labels
            .get(handler)
            .and_then(|labels| index_of(labels, label))
    }

    /// Whether the playbook declares a handler with this id.
    pub(crate) fn has_handler(&self, id: &str) -> bool {
        self.handler_ids.iter().any(|declared| declared == id)
    }

    /// Where a label sits in the list `list` names.
    pub(crate) fn index_in(&self, list: List, label: &str) -> Option<usize> {
        match list {
            List::Route => self.route_index(label),
            List::Body(handler) => self.body_index(handler, label),
        }
    }
}

/// The **first** position a label holds.
///
/// A repeated label is `E0105` from the structure stage; resolving to the first
/// one keeps this stage's answer defined while that error is on the report.
fn index_of(labels: &[String], label: &str) -> Option<usize> {
    labels.iter().position(|declared| declared == label)
}

/// Run the stage.
pub(crate) fn run(playbook: &Playbook, scope: &Scope, out: &mut Builder) -> Symbols {
    let symbols = Symbols::of(playbook);
    let mut visitor = Resolver {
        symbols: &symbols,
        scope,
        out,
    };
    walk::walk(playbook, &mut visitor);

    // `resume_at_label` hangs off the handler rather than off a step, so the
    // shared walk does not reach it.
    if let Some(declarative) = playbook.declarative.as_ref() {
        for (index, handler) in declarative.handlers.iter().enumerate() {
            if handler.resume_at_label.is_empty() {
                continue;
            }
            if symbols.route_index(&handler.resume_at_label).is_none() {
                let at = pointer::child(
                    &pointer::at("/declarative/handlers", index),
                    "resume_at_label",
                );
                out.emit(
                    Diag::new("E0305", at)
                        .arg("label", &handler.resume_at_label)
                        .arg("scope", "route"),
                );
            }
        }
    }
    symbols
}

struct Resolver<'a> {
    symbols: &'a Symbols,
    scope: &'a Scope,
    out: &'a mut Builder,
}

impl Visit for Resolver<'_> {
    fn entry(&mut self, at: &str, entry: &Step, list: List, _index: usize) {
        let Some(on_fail) = entry.on_fail.as_ref() else {
            return;
        };
        if on_fail.jump_to_label.is_empty() {
            return;
        }
        if self
            .symbols
            .index_in(list, &on_fail.jump_to_label)
            .is_none()
        {
            let scope = match list {
                List::Route => "route",
                List::Body(_) => "handler body",
            };
            self.out.emit(
                Diag::new(
                    "E0305",
                    pointer::child(&pointer::child(at, "on_fail"), "jump_to_label"),
                )
                .arg("label", &on_fail.jump_to_label)
                .arg("scope", scope),
            );
        }
    }

    fn beacon(&mut self, at: &str, reference: &BeaconRef) {
        let Some(beacon_ref::Ref::BeaconId(id)) = reference.r#ref.as_ref() else {
            return;
        };
        if self.scope.beacon(id).is_none() {
            self.out
                .emit(Diag::new("E0401", pointer::child(at, "beacon_id")).arg("id", id));
        }
    }

    fn condition(&mut self, at: &str, node: &Condition) {
        match node.node.as_ref() {
            Some(condition::Node::RuleFired(predicate))
                if !predicate.handler_id.is_empty()
                    && !self.symbols.has_handler(&predicate.handler_id) =>
            {
                self.out.emit(
                    Diag::new(
                        "E0207",
                        pointer::child(&pointer::child(at, "rule_fired"), "handler_id"),
                    )
                    .arg("id", &predicate.handler_id),
                );
            }
            Some(condition::Node::StepReached(predicate))
                if !predicate.label.is_empty()
                    && self.symbols.route_index(&predicate.label).is_none() =>
            {
                self.out.emit(
                    Diag::new(
                        "E0208",
                        pointer::child(&pointer::child(at, "step_reached"), "label"),
                    )
                    .arg("label", &predicate.label),
                );
            }
            _ => {}
        }
    }
}
