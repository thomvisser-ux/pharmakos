// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The built-in operator (spec section 14): Easy, and the safe playbook.
//!
//! Every commander is run by the built-in operator. It does three jobs: it
//! **generates** a playbook with the rudimentary tool (templates plus utility
//! scoring), it has the **safe playbook** filed when a seat submits nothing
//! verified before the Lull ends, and it **executes** whatever playbook the
//! seat sealed. This crate is the first job and the making of each seat's
//! own safe playbook (a built-in seat submits its own; an advisor returns a
//! human seat's). The filing itself is the gateway's, at `begin_push`, and
//! the executing is the sim's playbook interpreter (AGENTS.md section 3's
//! operator row).
//!
//! # An ordinary client, by construction
//!
//! "Same snapshot, same verifier, same submit path, no privileged reads"
//! (spec section 14; AGENTS.md section 3 rule 3). This crate depends on
//! `pharmakos-proto` **alone** (decisions-log item 111, decision C4): it names
//! no gateway type and no sim type, because it cannot. It reaches the match
//! through one thing, a **call closure** ([`Call`]) -- a JSON-RPC method name
//! and its params in, the response out -- which `gamectl host` binds to the
//! seat's own in-process token and adapts twice: as a built-in seat for each
//! seat the operator plays ([`Easy::play`]) and as an advisor for each seat a
//! person plays ([`Easy::advise`]). Every read, estimate, verify and submit is
//! a wire call through the gateway's door, its audit and its fog.
//!
//! The one input that is not a call is the **public rules text** (decision
//! C17): [`Easy::new`] reads its costs, kW ratings, yields and interface times
//! from the same `rules/rules.v1.json` the host and every client pin.
//!
//! # Deterministic, and budgeted in evaluation units
//!
//! Its seed is (match, seat, round), and at Easy it draws nothing with it
//! (decision C15): candidates are enumerated in a total order and ties go to
//! the lowest id, so the seed is recorded in the playbook's note and unused
//! until S5. It reads no clock, never the `_status` footer's timer, and keys
//! nothing on a viewer-scoped handle. Its budget is Easy's evaluation units --
//! 30 candidates, one estimate each, 1 + 4 verifies -- and the call budget is
//! derived from them ([`easy::EASY_CALL_BUDGET`]).
//!
//! # Names, not places (S1's targeting)
//!
//! Easy's row says "fixed targets only", and since S1 a fixed target is a
//! name (decisions-log item 127 (12); `docs/design/targeting.md`): Easy plans
//! from `get_map_summary`'s named vents and seams, asks `estimate_route` for
//! the site the sim's own `covering` rule would choose, and writes
//! `{"feature_id": "vent_324_16"}` where the templates' descriptions stand.
//! It derives no id and keeps no site algorithm of its own.
//!
//! **No lookahead at any level**: barred by "no dry runs" (AGENTS.md section 3
//! rule 2), which is also why this crate has no `[features]` and can never
//! reach the sim's `research` feature.
//!
//! # What Easy does not do yet
//!
//! Spec section 14's Survey post, Defend guard, chatter and target spread are
//! not built at Easy (decisions-log item 111, section A6).
//!
//! PLACEHOLDER: Easy's Survey post, Defend guard, chatter and target spread — owner, S5, with Normal and Hard

pub mod easy;
pub mod wire;

mod candidates;
mod compose;
mod playbook;
mod safe;
mod situation;
mod tuning;

pub use easy::{Advice, Easy, Played, Submitted, SuggestedValue, Suggestion};
pub use tuning::RulesError;
pub use wire::{Call, Refused};
