// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the built-in operator advises a seat it does not play, in plain
//! gateway types.
//!
//! Spec section 14 gives the built-in operator three jobs, and two of them
//! reach a seat a person plays: it files **the safe playbook** when that seat
//! seals nothing verified before the Lull ends, and its suggestion pre-fills
//! the editor's **template wizard** (spec section 13). Decisions-log item 111
//! (decisions C2 and C5) puts both behind one seam, [`crate::serve::Advisor`]:
//! at the start of every Lull the host asks the operator for an [`Advice`] for
//! each seat it does not play, through that seat's own in-process token, and
//! files what comes back with
//! [`crate::surface::Surface::file_advice`] -- a **host-side** call that no
//! method handler reaches (`tests/confinement.rs`).
//!
//! The types here are the whole of what crosses: text and plain values, no
//! token, no surface and no `pharmakos-sim` type, so the operator's crate
//! never names one (AGENTS.md section 3 rule 3).
//!
//! # What the gateway does with it
//!
//! * The safe playbook is **verified FULL and compiled when it is filed**,
//!   against the frozen snapshot the whole Lull plans against. One that does
//!   not qualify, or will not compile, is replaced by the gateway's fallback
//!   ([`crate::host::SAFE_PLAYBOOK`]) with an audit line saying so -- never
//!   silently. `get_safe_plan` answers what would be filed, and
//!   [`crate::surface::Surface::begin_push`] files it.
//! * The suggestions are stored per seat and answered only to that seat, by
//!   `instantiate_template{suggested: true}`.
//! * All of it is the **round's**: a Lull opening forgets the last one.

/// One suggested value for one declared template parameter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SuggestedValue {
    /// The `gp.v1.TemplateParameter` pointer it fills.
    pub pointer: String,
    /// The JSON to put there, as text.
    pub value: String,
}

/// The operator's suggestion for one template.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Suggestion {
    /// Which template: a file stem in the library, as `list_templates`
    /// answers it.
    pub template_id: String,
    /// The values it would fill in. A pointer the template does not declare
    /// is not applied (`pharmakos_plan_core::library::instantiate`).
    pub parameters: Vec<SuggestedValue>,
    /// Why, in one sentence of English (spec section 13's "why" note).
    pub why: String,
}

/// What the operator advises one seat for one round.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Advice {
    /// The seat's own safe playbook, JSONC (spec section 14).
    pub safe_playbook_jsonc: String,
    /// One suggestion per template it has one for, in any order.
    pub suggestions: Vec<Suggestion>,
}

impl Advice {
    /// The suggestion for one template, if there is one. The last one wins
    /// when an advice names a template twice, as the last explicit value does.
    #[must_use]
    pub fn suggestion(&self, template_id: &str) -> Option<&Suggestion> {
        self.suggestions
            .iter()
            .rev()
            .find(|suggestion| suggestion.template_id == template_id)
    }
}
