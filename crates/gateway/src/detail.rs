// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The read-method detail budgets, in the fixed salience order.
//!
//! Spec section 12, "Budgets", in full: "read methods take a `detail` of brief
//! (<=1.5k model tokens), standard (<=4k) or full (<=10k), truncated in a fixed
//! salience order. Every result carries a `_status` footer with the phase and
//! timer. **No v1 client is a model, so nothing actually spends those tokens;
//! the budgets are kept because they are the shape the v1.1 API needs.**"
//!
//! That last sentence is why this module exists at T9 rather than at T13. The
//! budgets buy this build nothing: the editor, `gamectl` and the built-in
//! operator are all native processes that read JSON. They are here because T9's
//! job is to freeze the shape v1.1 publishes (skeleton plan T9, risk R8), and a
//! `detail` parameter added after clients exist is a breaking change to every
//! one of them.
//!
//! # What a budget is, and what it deliberately is not
//!
//! It is **a ceiling on how much of a result comes back**, named by the caller
//! and clamped by the gateway. It is not a rate limit ([`crate::limit`] counts
//! calls), it is not a fog rule ([`crate::fog`] decides what may be seen at all,
//! and it runs first and independently), and it is not a rules-table row: a
//! rules row is stamped into `rules_hash` and a transport budget has no business
//! moving a hash chain (AGENTS.md section 12, and the same argument as
//! [`crate::limit::CALLS_PER_TICK`]).
//!
//! # The fixed salience order
//!
//! Truncation is not "drop the tail of the JSON". Two rules hold for every read
//! method, and the per-method order is part of each method's own definition at
//! T13:
//!
//! 1. **The `_status` footer is never truncated.** Spec section 12 says every
//!    result carries it, and a result that dropped the phase and the timer to
//!    fit a budget would be a result a client cannot act on at all.
//! 2. **Summaries outrank details.** What is dropped first is the most numerous
//!    and least individually meaningful part of the answer, and what survives is
//!    the part that says what happened. For [`crate::feed`] that means the
//!    digests are the summary and the events are the detail, so the events are
//!    what a budget cuts -- and the client is told exactly where the cut fell,
//!    because the cursor comes back pointing at the next unread event.
//!
//! PLACEHOLDER: the per-method salience order for the T13 method slice
//! (`get_briefing`, `get_recap`, `list_beacons`, `query_area` and the rest).
//! OWNER settles it with T13, whose methods are the things being ordered; T9
//! fixes the two rules above and the parameter's shape.

use crate::error::Error;
use crate::rpc::Request;
pub use pharmakos_proto::gp::api::v1::read_options::Detail;
use pharmakos_proto::scope as annotation;

/// The full name of the detail enum in the descriptor set.
const DETAIL_ENUM: &str = "gp.api.v1.ReadOptions.Detail";

/// The detail a read method uses when the caller names none.
///
/// PLACEHOLDER: `standard` is spec section 12's own middle rung and the one its
/// worked example spells out (`get_briefing{detail:"standard"}`), but the spec
/// does not say which rung an omitted `detail` means. OWNER settles it with the
/// budgets at hardening; the shape does not depend on the answer.
pub const DEFAULT: Detail = Detail::Standard;

/// The parameter's name on the wire.
pub const PARAM: &str = "detail";

/// A detail's spelling on the wire: `brief`, `standard`, `full`.
///
/// Lower case, decisions-log item 80's rule for every enum-valued parameter,
/// applied through the same translation [`crate::scopes`] uses so there is no
/// second table to drift.
#[must_use]
pub fn wire_name(detail: Detail) -> String {
    annotation::wire_name(DETAIL_ENUM, detail.as_str_name())
}

/// The detail a wire spelling names, or `None` for anything else.
///
/// `DETAIL_UNSPECIFIED` is not a spelling a caller may send: "unspecified" is
/// what an absent parameter means, and a client that sends it as a word is
/// telling the gateway something it cannot act on.
#[must_use]
pub fn from_wire(text: &str) -> Option<Detail> {
    let value = annotation::from_wire_name(DETAIL_ENUM, text)?;
    let detail = Detail::from_str_name(&value)?;
    if detail == Detail::Unspecified {
        return None;
    }
    Some(detail)
}

/// The `detail` a request asks for, or [`DEFAULT`] when it names none.
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] when `detail` is present and is not
/// a string, or is a string that is not one of the three rungs. A budget a
/// client meant and the gateway silently ignored is worse than a refusal: the
/// client would read a truncated answer as a complete one.
pub fn of(request: &Request) -> Result<Detail, Error> {
    let Some(text) = request.string_param(PARAM)? else {
        return Ok(DEFAULT);
    };
    from_wire(text).ok_or_else(|| {
        Error::invalid(format!(
            "`{PARAM}` is `{}`, `{}` or `{}`, and this is `{text}`",
            wire_name(Detail::Brief),
            wire_name(Detail::Standard),
            wire_name(Detail::Full)
        ))
    })
}

/// The budget in model tokens: spec section 12's own three numbers.
///
/// Nothing in v1 spends them -- no v1 client is a model -- so this is the number
/// the v1.1 API documents and the thing [`events`] is derived from, not a
/// measurement anybody takes.
#[must_use]
pub const fn model_tokens(detail: Detail) -> u32 {
    match detail {
        Detail::Brief => 1_500,
        // An omitted `detail` is `DEFAULT`, so `Unspecified` only reaches here
        // from a caller inside this crate that has one in its hand.
        Detail::Unspecified | Detail::Standard => 4_000,
        Detail::Full => 10_000,
    }
}

/// How many feed events one page carries at this detail.
///
/// PLACEHOLDER: 16 / 64 / 256 are working numbers, chosen so the three rungs
/// step by a factor of four as spec section 12's token budgets roughly do, and
/// so `full` meets [`crate::feed::MAX_PAGE_EVENTS`] exactly. OWNER sets the real
/// ladder at hardening, alongside T13's read-method budgets.
#[must_use]
pub const fn events(detail: Detail) -> usize {
    match detail {
        Detail::Brief => 16,
        Detail::Unspecified | Detail::Standard => 64,
        Detail::Full => crate::feed::MAX_PAGE_EVENTS,
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT, Detail, events, from_wire, model_tokens, of, wire_name};
    use crate::error::Code;
    use crate::rpc;

    fn request(params: &str) -> rpc::Request {
        rpc::parse(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"get_segment_feed","params":{params}}}"#
        ))
        .expect("well formed")
    }

    #[test]
    fn a_detail_is_spelled_lower_case_on_the_wire() {
        assert_eq!(wire_name(Detail::Brief), "brief");
        assert_eq!(wire_name(Detail::Standard), "standard");
        assert_eq!(wire_name(Detail::Full), "full");
        assert_eq!(from_wire("standard"), Some(Detail::Standard));
        assert_eq!(from_wire("full"), Some(Detail::Full));
        assert_eq!(
            from_wire("FULL"),
            None,
            "item 80 is lower case, and the enum value name is not the wire spelling"
        );
        assert_eq!(
            from_wire("unspecified"),
            None,
            "an absent parameter is what unspecified means"
        );
        assert_eq!(from_wire("verbose"), None);
    }

    #[test]
    fn an_absent_detail_is_the_default_and_a_wrong_one_is_refused() {
        assert_eq!(of(&request("{}")).expect("absent"), DEFAULT);
        assert_eq!(
            of(&request(r#"{"detail":"brief"}"#)).expect("named"),
            Detail::Brief
        );
        assert_eq!(
            of(&request(r#"{"detail":"verbose"}"#))
                .expect_err("refused")
                .code,
            Code::InvalidArgument,
            "a budget the gateway ignored would be read as a complete answer"
        );
        assert_eq!(
            of(&request(r#"{"detail":3}"#)).expect_err("refused").code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn the_ladder_climbs() {
        assert!(model_tokens(Detail::Brief) < model_tokens(Detail::Standard));
        assert!(model_tokens(Detail::Standard) < model_tokens(Detail::Full));
        assert!(events(Detail::Brief) < events(Detail::Standard));
        assert!(events(Detail::Standard) < events(Detail::Full));
        assert_eq!(
            events(Detail::Full),
            crate::feed::MAX_PAGE_EVENTS,
            "the widest budget a caller can name is the cap a caller cannot raise"
        );
    }
}
