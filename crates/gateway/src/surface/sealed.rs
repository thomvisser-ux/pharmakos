// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A seat's seal and the only places it is made (S3's `fpstream`, part 2).
//!
//! A seal carries the plan fingerprint of its text and the `$` its route
//! commits, and a save and the forecast read those without reading the text.
//! Each pair must be of the one text, so the type holds them: every field
//! here is private to this module, no caller hands a text and a fingerprint
//! separately, and every maker reads both from what one decode of the text
//! produced.
//!
//! * [`Surface::qualify`]: a FULL verify of the text, its canonical form
//!   compiled and priced, and the qualifying report's fingerprint, which the
//!   verifier wrote from that canonical form's wire bytes. `submit_plan` and
//!   an advised safe playbook seal through it.
//! * [`Fallback::read`] and [`Surface::fallback_for`]: the gateway's own safe
//!   playbook, decoded once, compiled and fingerprinted from that decode, and
//!   priced for each seat it is filed for.
//! * [`Surface::restored_seal`]: a saved seal, decoded, its fingerprint
//!   computed and compared with the stamp beside it (a mismatch is refused,
//!   as it always was), compiled and priced.
//! * [`Surface::harness_seal`]: the scenario harness's door, a FULL verify
//!   of a text the verifier refuses, compiled, fingerprinted and priced from
//!   the same decode.
//!
//! What is made before it is filed is an [`Unfiled`], with no round: the round
//! a seal is for has one home, the caller that files it ([`Unfiled::file`]),
//! so an advised safe playbook and the advice it came with cannot name two
//! rounds. Nothing here is `pub` that makes a seal or changes one, and no
//! method takes `&mut self` (`tests/confinement.rs` holds both).

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json::Canonicalised;
use pharmakos_proto::said;
use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;

use crate::error::Error;
use crate::save::{SavedSeal, SavedSeat};

use super::Surface;
use super::knowledge::Unpriced;

/// What a seat has sealed: the latest verified submission, and the report that
/// verified it.
///
/// Decisions-log item 5: "the latest verified submission replaces the previous
/// one, any number of times until the timer ends", so there is one of these per
/// seat and never a history. The safe playbook is filed only if nothing
/// verified was ever submitted.
///
/// It carries the **compiled** [`Plan`] as well as the text, because the text
/// is what a seat reads back and the plan is what the match plays; the plan
/// fingerprint of the text's canonical form (decisions-log item 77), which a
/// save stamps beside it; and the `$` its route commits, which the forecast
/// reports. All of them were taken from one decode of the text, by one of the
/// makers this module names.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sealed {
    /// The playbook, as submitted.
    playbook_jsonc: String,
    /// `report_hash` of the FULL report that accepted it. Eight big-endian
    /// bytes; the same value the pre-check returned, which is what spec
    /// section 11's "byte-identical" promise means. Empty for the gateway's
    /// own fallback, which no report accepted.
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
    /// The `$` the route orders, by the verifier's projection over the same
    /// canonical form ([`Surface::seal_commitment_of`]). The spend is a function
    /// of the text and the rules table alone; the economy it is priced against
    /// decides only whether the projection overflows.
    committed_dollars: i32,
}

impl Sealed {
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

    /// The `$` the sealed route orders, as it was priced when the seal was
    /// made.
    #[must_use]
    pub const fn committed_dollars(&self) -> i32 {
        self.committed_dollars
    }
}

/// A seal made and not yet filed for a round: what one decode of one text
/// yields. An advised safe playbook waits as one of these until a Lull ends
/// with the seat having sealed nothing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Unfiled {
    /// The playbook, as submitted.
    playbook_jsonc: String,
    /// `report_hash` of the report that accepted it, or empty.
    report_hash: Vec<u8>,
    /// The playbook as the interpreter runs it.
    plan: Plan,
    /// The plan fingerprint of `playbook_jsonc`'s canonical form.
    plan_fingerprint: u64,
    /// The `$` the route orders.
    committed_dollars: i32,
}

impl Unfiled {
    /// The playbook, as submitted.
    #[must_use]
    pub fn playbook_jsonc(&self) -> &str {
        &self.playbook_jsonc
    }

    /// `report_hash` of the report that accepted it, or empty.
    #[must_use]
    pub fn report_hash(&self) -> &[u8] {
        &self.report_hash
    }

    /// The playbook as the interpreter runs it.
    #[must_use]
    pub const fn plan(&self) -> &Plan {
        &self.plan
    }

    /// The plan fingerprint of its text's canonical form.
    #[must_use]
    pub const fn plan_fingerprint(&self) -> u64 {
        self.plan_fingerprint
    }

    /// Filed for `round`: the seal a seat holds. The caller is the one that
    /// knows the round, so a seal is for the round it was filed in.
    pub(super) fn file(self, round: u32, filed_by_the_gateway: bool) -> Sealed {
        Sealed {
            playbook_jsonc: self.playbook_jsonc,
            report_hash: self.report_hash,
            round,
            filed_by_the_gateway,
            plan: self.plan,
            plan_fingerprint: self.plan_fingerprint,
            committed_dollars: self.committed_dollars,
        }
    }
}

/// What a FULL verify of a text answered for sealing it.
pub(super) enum Qualified {
    /// It qualifies, compiles and prices: the seal, and the report.
    Sealable {
        /// The FULL report.
        report: VerifyReport,
        /// The seal, ready to file.
        unfiled: Unfiled,
    },
    /// It does not qualify, or has no canonical form: the report.
    Refused {
        /// The FULL report.
        report: VerifyReport,
    },
}

/// The gateway's own safe playbook, read once for a filing: its text, its
/// canonical form, the plan compiled from it and the fingerprint taken from
/// it.
pub(super) struct Fallback {
    playbook_jsonc: String,
    canonical: Canonicalised<Playbook>,
    plan: Plan,
    plan_fingerprint: u64,
}

impl Fallback {
    /// Read `playbook_jsonc`, the gateway's fallback: decoded once, compiled
    /// and fingerprinted from that decode.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when it has no canonical form, will
    /// not compile or has no fingerprint: the one playbook that is supposed to
    /// be safe in every situation would be runnable in none.
    pub(super) fn read(playbook_jsonc: String, rules: &RulesTable) -> Result<Fallback, Error> {
        let unfiled = |error: Error| {
            Error::internal(said!(
                "the safe playbook this gateway files will not compile: {}",
                error.said()
            ))
        };
        let canonical = sealed_canonical(&playbook_jsonc).map_err(unfiled)?;
        let plan =
            crate::surface::planning::compile_canonical(&canonical, rules).map_err(unfiled)?;
        let plan_fingerprint = fingerprint_of(&canonical)?;
        Ok(Fallback {
            playbook_jsonc,
            canonical,
            plan,
            plan_fingerprint,
        })
    }
}

impl Surface {
    /// A FULL verify of `playbook_jsonc` for `seat`, and, when it qualifies,
    /// the seal it makes: its canonical form compiled and priced, and the
    /// fingerprint the qualifying report carries, which the verifier wrote
    /// from that canonical form's wire bytes.
    ///
    /// # Errors
    ///
    /// As `Surface::verified_for`; `INVALID_ARGUMENT` when the playbook
    /// qualifies and this build cannot execute it or the Ledger cannot price
    /// it; `INTERNAL` when a qualifying report carries no fingerprint.
    pub(super) fn qualify(&self, seat: SeatId, playbook_jsonc: String) -> Result<Qualified, Error> {
        let verified = self.verified_for(seat, &playbook_jsonc, Depth::Full)?;
        let (report, canonical) = match verified {
            pharmakos_plan_core::Verified::Canonical { report, canonical } if report.qualifies => {
                (report, canonical)
            }
            refused => {
                return Ok(Qualified::Refused {
                    report: refused.into_report(),
                });
            }
        };
        // The doors, in the order a submission meets them: the compile, the
        // price, then the fingerprint the report was written with.
        let plan = crate::surface::planning::compile_canonical(&canonical, self.host()?.rules())?;
        let committed_dollars = self
            .seal_commitment_of(seat, canonical.decoded().message())
            .map_err(|unpriced| match unpriced {
                Unpriced::Gateway(error) => error,
                Unpriced::Playbook(why) => Error::invalid(said!(
                    "this playbook qualifies and the Ledger cannot price its seal: {}. Order \
                     less in one step",
                    why
                )),
            })?;
        let plan_fingerprint = carried_fingerprint(&report)?;
        let unfiled = Unfiled {
            report_hash: report.report_hash.clone(),
            playbook_jsonc,
            plan,
            plan_fingerprint,
            committed_dollars,
        };
        Ok(Qualified::Sealable { report, unfiled })
    }

    /// The gateway's fallback, priced for `seat`.
    ///
    /// # Errors
    ///
    /// `INTERNAL` when it does not price: the fallback orders nothing.
    pub(super) fn fallback_for(&self, fallback: &Fallback, seat: SeatId) -> Result<Unfiled, Error> {
        let committed_dollars = self
            .seal_commitment_of(seat, fallback.canonical.decoded().message())
            .map_err(|unpriced| match unpriced {
                Unpriced::Gateway(error) => error,
                Unpriced::Playbook(why) => Error::internal(said!(
                    "the safe playbook this gateway files does not price: {}",
                    why
                )),
            })?;
        Ok(Unfiled {
            playbook_jsonc: fallback.playbook_jsonc.clone(),
            // No report: this playbook did not come from a seat and was not
            // pre-checked by one. The Push's own verification is what accepts
            // it, and an invented hash here would be a hash nothing produced.
            report_hash: Vec::new(),
            plan: fallback.plan.clone(),
            plan_fingerprint: fallback.plan_fingerprint,
            committed_dollars,
        })
    }

    /// One saved seal, back in a seat's store: decoded once, its fingerprint
    /// computed and checked against the stamp beside it, then compiled and
    /// priced.
    ///
    /// The fingerprint first, because it is what says the text is the text
    /// that was saved; a text that compiles and is not is exactly what it
    /// catches. The fingerprint, the compile and the price all read the one
    /// canonical form (S3's `vfyq2`, fix pass 2, item F): what is checked is
    /// what is compiled. The refusals keep their codes and their words.
    ///
    /// # Errors
    ///
    /// `INVALID_ARGUMENT` for a seal with no canonical form, a stamp that is
    /// not its text's, a text this build cannot execute, or a seal that does
    /// not price: the file is damaged.
    pub(super) fn restored_seal(
        &self,
        held: &SavedSeat,
        seal: &SavedSeal,
    ) -> Result<Sealed, Error> {
        let damaged = |error: Error| {
            Error::invalid(said!(
                "seat {}'s sealed playbook in this save has no canonical form ({}): the file is \
                 damaged",
                held.seat,
                error.said()
            ))
        };
        let canonical = sealed_canonical(&seal.playbook_jsonc).map_err(damaged)?;
        let plan_fingerprint = fingerprint_of(&canonical).map_err(damaged)?;
        if plan_fingerprint != seal.plan_fingerprint {
            return Err(Error::invalid(said!(
                "seat {}'s sealed playbook in this save does not match the plan fingerprint \
                 saved beside it ({} against {}): the file is damaged, or this build writes a \
                 different canonical form than the one that saved it",
                held.seat,
                pharmakos_proto::json::grammar::quoted(&pharmakos_sim::hex(plan_fingerprint)),
                pharmakos_proto::json::grammar::quoted(&pharmakos_sim::hex(seal.plan_fingerprint))
            )));
        }
        let plan = crate::surface::planning::compile_canonical(&canonical, self.host()?.rules())
            .map_err(|error| {
                Error::invalid(said!(
                    "seat {}'s sealed playbook in this save will not compile in this build: {}",
                    held.seat,
                    error.said()
                ))
            })?;
        // A saved seal prices, as the door that sealed it required, so the
        // forecast's `committed_dollars` answers after a resume as before it,
        // and a resumed seal is the seal that was saved. A save holding one
        // that does not was never written by a build with that door.
        let committed_dollars = self
            .seal_commitment_of(SeatId::new(held.seat), canonical.decoded().message())
            .map_err(|unpriced| match unpriced {
                Unpriced::Gateway(error) => error,
                Unpriced::Playbook(why) => Error::invalid(said!(
                    "seat {}'s sealed playbook in this save does not price: {}: the \
                             file is damaged",
                    held.seat,
                    why
                )),
            })?;
        Ok(Sealed {
            playbook_jsonc: seal.playbook_jsonc.clone(),
            report_hash: seal.report_hash.clone(),
            round: seal.round,
            filed_by_the_gateway: seal.filed_by_the_gateway,
            plan,
            plan_fingerprint,
            committed_dollars,
        })
    }

    /// The scenario harness's seal of a playbook the verifier refuses: a FULL
    /// verify of it, its canonical form compiled and priced, and the refused
    /// report's fingerprint, which the verifier wrote from that canonical
    /// form. What [`Surface::seal_by_harness`] files.
    ///
    /// # Errors
    ///
    /// `INVALID_ARGUMENT` when the text has no canonical form, this build
    /// cannot execute it or it does not price; `INTERNAL` when the report
    /// carries no fingerprint.
    pub(super) fn harness_seal(
        &self,
        seat: SeatId,
        playbook_jsonc: &str,
    ) -> Result<Unfiled, Error> {
        let verified = self.verified_for(seat, playbook_jsonc, Depth::Full)?;
        let pharmakos_plan_core::Verified::Canonical { report, canonical } = verified else {
            return Err(Error::invalid(
                "it has no canonical form, so there is nothing to compile",
            ));
        };
        let plan = crate::surface::planning::compile_harnessed(&canonical, self.host()?.rules())?;
        let committed_dollars = self
            .seal_commitment_of(seat, canonical.decoded().message())
            .map_err(|unpriced| match unpriced {
                Unpriced::Gateway(error) => error,
                Unpriced::Playbook(why) => Error::invalid(said!("it does not price: {}", why)),
            })?;
        let plan_fingerprint = carried_fingerprint(&report)?;
        Ok(Unfiled {
            playbook_jsonc: playbook_jsonc.to_owned(),
            report_hash: report.report_hash,
            plan,
            plan_fingerprint,
            committed_dollars,
        })
    }
}

/// The plan fingerprint a report carries, for the seal its decode makes.
///
/// The verifier writes it from the wire bytes of the decode it verified
/// (`pharmakos_sim::hash::plan_fingerprint_of`), the same canonical form a
/// resume checks a saved seal against, and reports a fingerprint it could not
/// take as `E0030`, an error. A report with no fingerprint, or one not eight
/// bytes long, is this build's verifier and its gateway disagreeing, and it is
/// refused rather than sealed under a value nobody computed.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the report carries no fingerprint,
/// or one of the wrong length.
fn carried_fingerprint(report: &VerifyReport) -> Result<u64, Error> {
    pharmakos_proto::fingerprint::from_field(&report.plan_fingerprint).ok_or_else(|| {
        Error::internal(said!(
            "the verifier wrote no plan fingerprint for this playbook ({} bytes where eight \
             belong), so it is not sealed",
            report.plan_fingerprint.len()
        ))
    })
}

/// A sealed playbook's canonical form: the one decode a saved seal, and the
/// gateway's fallback, are read through.
fn sealed_canonical(playbook_jsonc: &str) -> Result<Canonicalised<Playbook>, Error> {
    pharmakos_plan_core::decode_jsonc(playbook_jsonc).map_err(|error| {
        Error::internal(said!(
            "a sealed playbook has no canonical form: {}",
            error.said()
        ))
    })
}

/// The plan fingerprint of a canonical form already decoded (decisions-log
/// item 77): what a resume checks a saved seal's stamp against, and what the
/// gateway's fallback is sealed with. The arithmetic is the sim's, from the
/// wire bytes the form was decoded from
/// ([`pharmakos_sim::hash::plan_fingerprint_of`], S3's `fpstream`), so the
/// message is neither cloned nor encoded again and the text is never held.
fn fingerprint_of(canonical: &Canonicalised<Playbook>) -> Result<u64, Error> {
    pharmakos_sim::hash::plan_fingerprint_of(canonical.decoded()).map_err(|error| {
        Error::internal(said!(
            "a sealed playbook's canonical form will not encode for its fingerprint: {}",
            crate::error::engine(&error)
        ))
    })
}

#[cfg(test)]
mod tests {
    use crate::error::Code;

    /// A report's fingerprint is what a seal carries, read as the eight bytes
    /// the field holds; a report with none, or with one of another length, is
    /// refused `INTERNAL` and never sealed under a default (S3's `fpstream`,
    /// part 2; the main session's ruling 5).
    #[test]
    fn a_seal_takes_the_reports_fingerprint_or_refuses() {
        use pharmakos_proto::gp::api::v1::VerifyReport;
        let report = |bytes: Vec<u8>| VerifyReport {
            qualifies: true,
            plan_fingerprint: bytes,
            ..VerifyReport::default()
        };
        assert_eq!(
            super::carried_fingerprint(&report(pharmakos_proto::fingerprint::to_field(
                0x0123_4567_89ab_cdef
            )))
            .expect("eight bytes"),
            0x0123_4567_89ab_cdef
        );
        for wrong in [Vec::new(), vec![0; 7], vec![0; 9]] {
            let length = wrong.len();
            let refused = super::carried_fingerprint(&report(wrong)).expect_err("no fingerprint");
            assert_eq!(refused.code(), Code::Internal, "{length} bytes");
            assert!(
                refused
                    .message()
                    .contains(&format!("({length} bytes where eight belong)")),
                "{}",
                refused.message()
            );
        }
    }
}
