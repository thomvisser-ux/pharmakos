// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A seat's seal, in a module of its own so that its fields are private to it
//! (S3's `fpstream`, part 2, fix pass 1).
//!
//! A seal carries the plan fingerprint of its text, and a save stamps that
//! value beside the text without reading it. The two must stay one pair, so
//! the type holds the pair: every field is private to this module, a seal is
//! made only by [`Sealed::new`], and nothing that holds one, a `&mut Sealed`
//! included, can change its text, its plan or its fingerprint afterwards. Only
//! its round moves, and only by [`Sealed::for_round`], which consumes it.

use pharmakos_sim::interpreter::Plan;

/// What a seat has sealed: the latest verified submission, and the report that
/// verified it.
///
/// Decisions-log item 5: "the latest verified submission replaces the previous
/// one, any number of times until the timer ends", so there is one of these per
/// seat and never a history. The safe playbook is filed only if nothing
/// verified was ever submitted.
///
/// It carries the **compiled** [`Plan`] as well as the text, because the text
/// is what a seat reads back and the plan is what the match plays. The compile
/// happens once, at the door that can still tell somebody it failed
/// ([`crate::surface::planning::compile_canonical`]); by the time
/// [`crate::surface::Surface::begin_push`] seals this into the runner there is
/// nothing left that can go wrong with it.
///
/// It carries the text's **plan fingerprint** too (decisions-log item 77),
/// taken once where the seal is made, from the decode that qualified it, so a
/// save stamps it beside the text and decodes nothing (S3's `fpstream`,
/// part 2). Every field is private to this module, so no seal can be built
/// without a fingerprint and none can be changed after: a field write does not
/// compile.
///
/// ```compile_fail
/// fn retext(sealed: &mut pharmakos_gateway::surface::Sealed) {
///     sealed.playbook_jsonc = String::from("{}");
/// }
/// ```
///
/// The getters read it:
///
/// ```
/// fn text(sealed: &pharmakos_gateway::surface::Sealed) -> (&str, u64) {
///     (sealed.playbook_jsonc(), sealed.plan_fingerprint())
/// }
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sealed {
    /// The playbook, as submitted.
    playbook_jsonc: String,
    /// `report_hash` of the FULL report that accepted it. Eight big-endian
    /// bytes; the same value the pre-check returned, which is what spec
    /// section 11's "byte-identical" promise means.
    report_hash: Vec<u8>,
    /// Which round it was sealed in.
    round: u32,
    /// True when the gateway filed this itself because the Lull ended with
    /// nothing sealed (spec section 14).
    filed_by_the_gateway: bool,
    /// The playbook as the interpreter runs it.
    plan: Plan,
    /// The plan fingerprint of `playbook_jsonc`'s canonical form: what a save
    /// stamps beside it.
    plan_fingerprint: u64,
}

impl Sealed {
    /// A seal of `playbook_jsonc`, compiled to `plan`, whose canonical form's
    /// plan fingerprint is `plan_fingerprint`.
    ///
    /// The fingerprint is the caller's to take from the decode it made for the
    /// verify or the compile (a qualifying report's `plan_fingerprint`, read
    /// with `carried_fingerprint`; or the fingerprint of the canonical form it
    /// compiled), never a value of its own making: what a save stamps is what a
    /// resume checks the text against.
    #[must_use]
    pub const fn new(
        playbook_jsonc: String,
        report_hash: Vec<u8>,
        round: u32,
        filed_by_the_gateway: bool,
        plan: Plan,
        plan_fingerprint: u64,
    ) -> Self {
        Self {
            playbook_jsonc,
            report_hash,
            round,
            filed_by_the_gateway,
            plan,
            plan_fingerprint,
        }
    }

    /// The same seal, filed for `round`: what the gateway files for a seat
    /// that sealed nothing this round. The text, the plan and the fingerprint
    /// go with it unchanged.
    #[must_use]
    pub fn for_round(self, round: u32) -> Self {
        Self { round, ..self }
    }

    /// The playbook, as submitted.
    #[must_use]
    pub fn playbook_jsonc(&self) -> &str {
        &self.playbook_jsonc
    }

    /// `report_hash` of the FULL report that accepted it; empty for the
    /// gateway's own fallback, which no report accepted.
    #[must_use]
    pub fn report_hash(&self) -> &[u8] {
        &self.report_hash
    }

    /// Which round it was sealed in.
    #[must_use]
    pub const fn round(&self) -> u32 {
        self.round
    }

    /// True when the gateway filed this itself (spec section 14).
    #[must_use]
    pub const fn filed_by_the_gateway(&self) -> bool {
        self.filed_by_the_gateway
    }

    /// The playbook as the interpreter runs it.
    #[must_use]
    pub const fn plan(&self) -> &Plan {
        &self.plan
    }

    /// The plan fingerprint of the sealed text's canonical form, as it was
    /// taken when the seal was made.
    #[must_use]
    pub const fn plan_fingerprint(&self) -> u64 {
        self.plan_fingerprint
    }
}
