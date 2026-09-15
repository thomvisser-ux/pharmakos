// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-seat 256-bit tokens: minted from OS entropy, compared in constant time,
//! revocable from the lobby.
//!
//! Spec section 12: "Per-seat 256-bit tokens tied to the match and seat,
//! revocable from the lobby. Scopes: observe, plan, plan.submit, docs,
//! spectate.nofog, admin", and then the sentence this module exists to make
//! literally true in code: "Seat tokens can never hold `spectate.nofog`; only
//! separate spectator tokens can. Admin covers lobby and match control and can
//! never read another seat's playbooks, drafts or knowledge."
//!
//! # The invariants, and where each one lives
//!
//! | Invariant | Enforced in |
//! |---|---|
//! | A seat token never holds `spectate.nofog` | [`TokenStore::mint`] |
//! | Only a spectator token holds `spectate.nofog` | [`TokenStore::mint`] |
//! | `plan` and `plan.submit` need a seat to plan for | [`TokenStore::mint`] |
//! | Only an admin token holds `admin` | [`TokenStore::mint`] |
//! | `admin` never reads another seat's private state | [`crate::surface`] |
//! | A token is tied to one match | [`TokenStore::authenticate`] |
//!
//! The first four are refused at the moment of minting rather than checked at
//! the moment of use, because a token that cannot exist cannot leak. The fifth
//! is a property of every read path and belongs where the reads are.
//!
//! # Entropy
//!
//! [`getrandom`] and nothing else, which is decisions-log item 99's whole
//! subject: the standard library has no stable cryptographic source in rustc
//! 1.98, `unsafe_code = "deny"` rules out calling the OS by hand, and
//! `RandomState` is a disallowed type. The sim's seeded RNG streams are the
//! wrong tool by design -- they are reproducible, which is exactly what a token
//! must not be -- and nothing in this module is hashed state.
//!
//! # No clock
//!
//! A token's life is measured in the host's ticks ([`crate::time`]). The gateway
//! reads no wall clock at all (AGENTS.md section 4.5), so "expired" means "the
//! match is further through than the grant allows" and not "some seconds have
//! passed".

use crate::error::Error;
use crate::scopes::{NEVER_ON_A_SEAT, Scope, ScopeSet};
use pharmakos_sim::math::quantity::Tick;
use pharmakos_sim::tables::SeatId;

/// A token's length in bytes. 256 bits, as spec section 12 says.
pub const TOKEN_BYTES: usize = 32;

/// How long a minted token stays valid, in ticks.
///
/// PLACEHOLDER: `u32::MAX` is "the whole match", which is the only lifetime the
/// skeleton can justify -- a token that expires mid-match would have to be
/// reissued, and spec section 12 says no token is reissued mid-match. OWNER sets
/// the real policy at hardening, alongside the rate limits; the likely shape is
/// "the match, plus a grace period for the recap", which needs a number nobody
/// has yet. The mechanism is built and tested either way
/// (`an_expired_token_is_refused`), so setting it is a one-line change.
pub const TOKEN_LIFETIME_TICKS: u32 = u32::MAX;

/// Who a token speaks for.
///
/// Three subjects and no fourth. A token that is not a seat, a spectator or the
/// lobby is a token with no story about what it may read.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Subject {
    /// One seat of the match. At most one of them is human (spec section 3).
    Seat(SeatId),
    /// A spectator: the only subject that may hold `spectate.nofog`, and one
    /// that never holds `plan`.
    Spectator,
    /// The lobby and match control. Never another seat's secrets.
    Admin,
}

impl Subject {
    /// The seat this subject is, if it is one.
    #[must_use]
    pub const fn seat(self) -> Option<SeatId> {
        match self {
            Subject::Seat(seat) => Some(seat),
            _ => None,
        }
    }

    /// A short name for the audit log: `seat.0`, `spectator`, `admin`.
    #[must_use]
    pub fn render(self) -> String {
        match self {
            Subject::Seat(seat) => format!("seat.{}", seat.raw()),
            Subject::Spectator => String::from("spectator"),
            Subject::Admin => String::from("admin"),
        }
    }
}

/// A 256-bit token.
///
/// `Debug` is implemented by hand and prints nothing but the type's name: a
/// token that reaches a log or a panic message is a token that has leaked, and
/// the derive would do exactly that. For the same reason there is no `Display`,
/// no `Serialize` and no accessor returning the bytes -- the only ways out are
/// [`Token::render`], which the lobby calls once to hand the token to its owner,
/// and a constant-time comparison.
///
/// `PartialEq` is written by hand for the same reason: the derive's `eq` on a
/// `[u8; 32]` exits at the first differing byte, which is the timing side
/// channel [`Token::constant_time_eq`] exists to close. `==` on a `Token` is
/// therefore the constant-time comparison, and a later lane that reaches for the
/// operator gets the safe one rather than the fast one.
#[derive(Clone)]
pub struct Token([u8; TOKEN_BYTES]);

impl PartialEq for Token {
    fn eq(&self, other: &Token) -> bool {
        self.constant_time_eq(other)
    }
}

impl Eq for Token {}

impl std::fmt::Debug for Token {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Token(<redacted>)")
    }
}

impl Token {
    /// A fresh token from OS entropy.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the operating system will not give
    /// entropy. Nothing the caller did caused it, and there is no fallback: a
    /// token from a weaker source is worse than no match at all.
    pub fn mint() -> Result<Token, Error> {
        let mut bytes = [0_u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes).map_err(|error| {
            Error::internal(format!(
                "the operating system would not provide entropy for a seat token: {error}"
            ))
        })?;
        Ok(Token(bytes))
    }

    /// A token from its 64 lower-case hex digits, as [`Token::render`] wrote it.
    ///
    /// Returns `None` for anything that is not exactly 64 hex digits, so a
    /// truncated or padded token is refused rather than silently zero-extended.
    #[must_use]
    pub fn parse(text: &str) -> Option<Token> {
        if text.len() != TOKEN_BYTES.saturating_mul(2) {
            return None;
        }
        let mut bytes = [0_u8; TOKEN_BYTES];
        let digits = text.as_bytes();
        for (index, slot) in bytes.iter_mut().enumerate() {
            let high = hex_value(digits.get(index.saturating_mul(2)).copied()?)?;
            let low = hex_value(
                digits
                    .get(index.saturating_mul(2).saturating_add(1))
                    .copied()?,
            )?;
            *slot = high.saturating_mul(16).saturating_add(low);
        }
        Some(Token(bytes))
    }

    /// The token as 64 lower-case hex digits.
    ///
    /// The lobby calls this once, to hand the token to the client it minted it
    /// for. Nothing else should: a rendered token is a secret in a `String`.
    #[must_use]
    pub fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(TOKEN_BYTES.saturating_mul(2));
        for byte in &self.0 {
            // Writing into a `String` cannot fail.
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    /// Constant-time equality.
    ///
    /// Hand-written, deliberately: `subtle` is not on the approved dependency
    /// list (AGENTS.md section 3 rule 5) and this is nine lines. The loop never
    /// breaks early and never branches on a byte, so the time it takes depends
    /// on the token's length and on nothing else -- which is what stops a caller
    /// learning a token one byte at a time from how long a rejection took.
    ///
    /// `black_box` keeps an optimiser from noticing that the accumulator is only
    /// ever compared with zero and turning the loop back into an early exit.
    #[must_use]
    pub fn constant_time_eq(&self, other: &Token) -> bool {
        let mut difference: u8 = 0;
        for (left, right) in self.0.iter().zip(other.0.iter()) {
            difference |= left ^ right;
        }
        std::hint::black_box(difference) == 0
    }
}

/// One hex digit's value.
const fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(digit.wrapping_sub(b'a').wrapping_add(10)),
        _ => None,
    }
}

/// A grant's handle: how the audit log and the lobby name a token without
/// holding one.
///
/// A counter, not a fingerprint of the secret. A fingerprint would be a value
/// derived from the token that travels to places the token may not go, and the
/// only thing that buys is a shorter `revoke` call.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Handle(u32);

impl Handle {
    /// The raw number, for rendering.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// A handle from its raw number, for a lobby that has stored one and for the
    /// tests that render a log line.
    ///
    /// Safe to expose because a handle is not a secret and grants nothing: it
    /// names a grant to [`TokenStore::revoke`] and [`TokenStore::grant`], both of
    /// which only the host can call, and neither of which hands out a token.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Handle {
        Handle(raw)
    }
}

impl std::fmt::Display for Handle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "t{}", self.0)
    }
}

/// What a token grants: who, in which match, with which scopes, for how long.
#[derive(Clone, Debug)]
pub struct Grant {
    /// How the log names it.
    pub handle: Handle,
    /// Who it speaks for.
    pub subject: Subject,
    /// The match it is tied to. A token from another match is not a token here.
    pub match_id: String,
    /// What it may do.
    pub scopes: ScopeSet,
    /// The tick it was minted at.
    pub issued: Tick,
    /// The last tick it is valid on.
    pub expires: Tick,
    /// Revoked from the lobby. A revoked grant is kept rather than removed, so
    /// the audit log can still name the handle that was refused.
    pub revoked: bool,
    /// The secret. Never leaves this struct except through a constant-time
    /// comparison.
    secret: Token,
}

impl Grant {
    /// True when this grant is usable at `tick` in `match_id`.
    #[must_use]
    pub fn live(&self, match_id: &str, tick: Tick) -> bool {
        !self.revoked && self.match_id == match_id && tick <= self.expires
    }
}

/// Every token a host has minted for a match.
///
/// Ordered storage, walked in mint order: no hash map (AGENTS.md section 4.4),
/// and the order a token is found in cannot depend on the process.
#[derive(Debug)]
pub struct TokenStore {
    grants: Vec<Grant>,
    next: u32,
    lifetime: u32,
}

impl Default for TokenStore {
    fn default() -> TokenStore {
        TokenStore::new()
    }
}

impl TokenStore {
    /// An empty store, minting tokens that live [`TOKEN_LIFETIME_TICKS`].
    #[must_use]
    pub const fn new() -> TokenStore {
        TokenStore::with_lifetime(TOKEN_LIFETIME_TICKS)
    }

    /// An empty store whose tokens live `lifetime_ticks`.
    ///
    /// The lifetime is a policy the owner has not set yet
    /// ([`TOKEN_LIFETIME_TICKS`] is the PLACEHOLDER), so it is a parameter of
    /// the store rather than a constant baked into the mint: when the number
    /// arrives it is the lobby that passes it, and the expiry machinery is
    /// already built and tested.
    #[must_use]
    pub const fn with_lifetime(lifetime_ticks: u32) -> TokenStore {
        TokenStore {
            grants: Vec::new(),
            next: 1,
            lifetime: lifetime_ticks,
        }
    }

    /// How long a token minted by this store lives, in ticks.
    #[must_use]
    pub const fn lifetime(&self) -> u32 {
        self.lifetime
    }

    /// Mint a token for `subject` in `match_id` with `scopes`.
    ///
    /// Returns the token -- the only time it is ever handed out -- and the
    /// handle the lobby revokes it by.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::ForbiddenScope`] when the scope set breaks one of
    /// the four mint-time invariants in the module docs, and
    /// [`crate::error::Code::Internal`] when the operating system will not give
    /// entropy. A refused mint is not a partial mint: nothing is stored.
    pub fn mint(
        &mut self,
        subject: Subject,
        match_id: &str,
        scopes: ScopeSet,
        now: Tick,
    ) -> Result<(Token, Handle), Error> {
        check_scopes(subject, scopes)?;

        let secret = Token::mint()?;
        let handle = Handle(self.next);
        self.next = self.next.saturating_add(1);
        let expires = Tick::new(now.raw().saturating_add(self.lifetime));
        self.grants.push(Grant {
            handle,
            subject,
            match_id: match_id.to_owned(),
            scopes,
            issued: now,
            expires,
            revoked: false,
            secret: secret.clone(),
        });
        Ok((secret, handle))
    }

    /// Find the live grant a token speaks for.
    ///
    /// The scan does **not** stop at the first match: every grant is compared,
    /// with a constant-time byte comparison, so neither the answer nor the time
    /// it took says how many tokens exist or how far down the list one sat.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Unauthenticated`] for a token this store never
    /// minted, one minted for another match, one that has been revoked, and one
    /// past its expiry. The message says which, because the client is the
    /// gateway's own editor and a wrong-match token is a bug rather than an
    /// attack; the *code* is the same in every case, so nothing downstream can
    /// branch on the difference.
    pub fn authenticate(&self, token: &Token, match_id: &str, now: Tick) -> Result<&Grant, Error> {
        let mut found: Option<usize> = None;
        for (index, grant) in self.grants.iter().enumerate() {
            if grant.secret.constant_time_eq(token) {
                found = Some(index);
            }
        }
        let Some(grant) = found.and_then(|index| self.grants.get(index)) else {
            return Err(Error::unauthenticated("no such token"));
        };
        if grant.revoked {
            return Err(Error::unauthenticated(
                "this token was revoked from the lobby",
            ));
        }
        if grant.match_id != match_id {
            return Err(Error::unauthenticated(
                "this token belongs to a different match",
            ));
        }
        if now > grant.expires {
            return Err(Error::unauthenticated("this token has expired"));
        }
        Ok(grant)
    }

    /// Revoke a grant from the lobby. `false` when no such handle exists.
    pub fn revoke(&mut self, handle: Handle) -> bool {
        for grant in &mut self.grants {
            if grant.handle == handle {
                grant.revoked = true;
                return true;
            }
        }
        false
    }

    /// The grant a handle names, for the lobby's own listing.
    #[must_use]
    pub fn grant(&self, handle: Handle) -> Option<&Grant> {
        self.grants.iter().find(|grant| grant.handle == handle)
    }

    /// How many grants have been minted, revoked ones included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.grants.len()
    }

    /// True when nothing has been minted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }
}

/// The four mint-time invariants.
fn check_scopes(subject: Subject, scopes: ScopeSet) -> Result<(), Error> {
    if subject.seat().is_some() {
        for banned in NEVER_ON_A_SEAT {
            if scopes.holds(*banned) {
                return Err(Error::forbidden(format!(
                    "a seat token can never hold `{}` (spec section 12); only a separate \
                     spectator token can",
                    crate::scopes::scope_wire_name(*banned)
                )));
            }
        }
    } else if scopes.holds(Scope::SpectateNofog) && subject != Subject::Spectator {
        return Err(Error::forbidden(
            "only a spectator token can hold `spectate.nofog`",
        ));
    }

    if subject.seat().is_none() && (scopes.holds(Scope::Plan) || scopes.holds(Scope::PlanSubmit)) {
        return Err(Error::forbidden(
            "`plan` and `plan.submit` author a playbook for a seat, so only a seat token \
             may hold them",
        ));
    }

    // `admin` is lobby and match control (spec section 12, and
    // `crate::scopes`'s note on the two scopes that gate no method). It gates no
    // JSON-RPC method today, so a spectator holding it could reach nothing --
    // which is exactly why the rule belongs here, at the mint, before the day a
    // method is gated by it and the asymmetry has become a hole.
    if scopes.holds(Scope::Admin) && subject != Subject::Admin {
        return Err(Error::forbidden(
            "`admin` is lobby and match control, so only an admin token may hold it",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Handle, Scope, ScopeSet, Subject, Token, TokenStore};
    use crate::error::Code;
    use pharmakos_sim::math::quantity::Tick;
    use pharmakos_sim::tables::SeatId;

    const MATCH: &str = "m0000000000000001";

    fn seat(raw: u8) -> Subject {
        Subject::Seat(SeatId::new(raw))
    }

    fn store() -> TokenStore {
        TokenStore::new()
    }

    #[test]
    fn a_minted_token_is_two_hundred_and_fifty_six_bits_and_never_repeats() {
        let mut store = store();
        let scopes = ScopeSet::of(&[Scope::Observe]);
        let (first, _) = store
            .mint(seat(0), MATCH, scopes, Tick::ZERO)
            .expect("minted");
        let (second, _) = store
            .mint(seat(1), MATCH, scopes, Tick::ZERO)
            .expect("minted");
        assert_eq!(first.render().len(), 64);
        assert_ne!(first.render(), second.render());
        assert!(!first.constant_time_eq(&second));
    }

    #[test]
    fn a_token_round_trips_through_its_rendering() {
        let mut store = store();
        let (token, _) = store
            .mint(seat(0), MATCH, ScopeSet::of(&[Scope::Observe]), Tick::ZERO)
            .expect("minted");
        let parsed = Token::parse(&token.render()).expect("64 hex digits");
        assert!(token.constant_time_eq(&parsed));
    }

    #[test]
    fn a_malformed_token_is_refused_rather_than_padded() {
        assert!(Token::parse("").is_none());
        assert!(Token::parse(&"0".repeat(63)).is_none());
        assert!(Token::parse(&"0".repeat(65)).is_none());
        assert!(Token::parse(&"Z".repeat(64)).is_none());
        assert!(Token::parse(&"A".repeat(64)).is_none(), "lower case only");
    }

    #[test]
    fn a_debug_print_of_a_token_says_nothing() {
        let mut store = store();
        let (token, _) = store
            .mint(seat(0), MATCH, ScopeSet::of(&[Scope::Observe]), Tick::ZERO)
            .expect("minted");
        let printed = format!("{token:?}");
        assert_eq!(printed, "Token(<redacted>)");
        assert!(!printed.contains(token.render().get(..8).expect("prefix")));
    }

    #[test]
    fn a_grant_is_found_and_carries_its_subject_and_scopes() {
        let mut store = store();
        let scopes = ScopeSet::of(&[Scope::Observe, Scope::Plan]);
        let (token, handle) = store
            .mint(seat(2), MATCH, scopes, Tick::ZERO)
            .expect("minted");
        let grant = store
            .authenticate(&token, MATCH, Tick::new(100))
            .expect("live");
        assert_eq!(grant.handle, handle);
        assert_eq!(grant.subject, seat(2));
        assert_eq!(grant.scopes.render(), "observe plan");
    }

    #[test]
    fn a_token_from_another_match_is_not_a_token_here() {
        let mut store = store();
        let (token, _) = store
            .mint(seat(0), MATCH, ScopeSet::of(&[Scope::Observe]), Tick::ZERO)
            .expect("minted");
        let error = store
            .authenticate(&token, "m0000000000000002", Tick::ZERO)
            .expect_err("refused");
        assert_eq!(error.code, Code::Unauthenticated);
    }

    #[test]
    fn a_revoked_token_stops_working_and_keeps_its_handle() {
        let mut store = store();
        let (token, handle) = store
            .mint(seat(0), MATCH, ScopeSet::of(&[Scope::Observe]), Tick::ZERO)
            .expect("minted");
        assert!(store.revoke(handle));
        let error = store
            .authenticate(&token, MATCH, Tick::ZERO)
            .expect_err("refused");
        assert_eq!(error.code, Code::Unauthenticated);
        assert!(store.grant(handle).is_some(), "the log can still name it");
        assert!(!store.revoke(Handle(9_999)), "no such handle");
    }

    #[test]
    fn an_expired_token_is_refused() {
        // A short-lived store, because the default lifetime is "the whole
        // match" and a test must not wait for one. The gateway has no clock, so
        // "expired" is a tick comparison and nothing else.
        let mut store = TokenStore::with_lifetime(100);
        assert_eq!(store.lifetime(), 100);
        let (token, handle) = store
            .mint(seat(0), MATCH, ScopeSet::of(&[Scope::Observe]), Tick::ZERO)
            .expect("minted");
        let expires = store.grant(handle).expect("minted").expires;
        assert_eq!(expires, Tick::new(100));
        store
            .authenticate(&token, MATCH, expires)
            .expect("live on its last tick");
        let error = store
            .authenticate(&token, MATCH, Tick::new(expires.raw().saturating_add(1)))
            .expect_err("refused");
        assert_eq!(error.code, Code::Unauthenticated);
    }

    #[test]
    fn the_default_lifetime_is_the_whole_match() {
        // The PLACEHOLDER's shape: nothing expires mid-match, because spec
        // section 12 says no token is reissued mid-match.
        assert_eq!(TokenStore::new().lifetime(), super::TOKEN_LIFETIME_TICKS);
    }

    #[test]
    fn an_unknown_token_is_refused() {
        let store = store();
        let unknown = Token::parse(&"ab".repeat(32)).expect("64 hex digits");
        assert_eq!(
            store
                .authenticate(&unknown, MATCH, Tick::ZERO)
                .expect_err("refused")
                .code,
            Code::Unauthenticated
        );
    }

    /// Spec section 12, and one of T9's five named negative tests. The
    /// integration suite asserts it through the whole surface as well; this is
    /// the same rule at the only place it can be enforced once.
    #[test]
    fn a_seat_token_can_never_hold_spectate_nofog() {
        let mut store = store();
        let error = store
            .mint(
                seat(0),
                MATCH,
                ScopeSet::of(&[Scope::Observe, Scope::SpectateNofog]),
                Tick::ZERO,
            )
            .expect_err("refused");
        assert_eq!(error.code, Code::ForbiddenScope);
        assert!(store.is_empty(), "a refused mint stores nothing");
    }

    #[test]
    fn only_a_spectator_token_holds_spectate_nofog() {
        let mut store = store();
        store
            .mint(
                Subject::Spectator,
                MATCH,
                ScopeSet::of(&[Scope::Observe, Scope::SpectateNofog]),
                Tick::ZERO,
            )
            .expect("a spectator may");
        let error = store
            .mint(
                Subject::Admin,
                MATCH,
                ScopeSet::of(&[Scope::SpectateNofog]),
                Tick::ZERO,
            )
            .expect_err("refused");
        assert_eq!(error.code, Code::ForbiddenScope);
    }

    /// `admin` is lobby and match control, and it gates no method today -- which
    /// is why the rule is at the mint, before the day one is gated by it.
    #[test]
    fn only_an_admin_token_holds_admin() {
        let mut store = store();
        for subject in [seat(0), Subject::Spectator] {
            let error = store
                .mint(
                    subject,
                    MATCH,
                    ScopeSet::of(&[Scope::Observe, Scope::Admin]),
                    Tick::ZERO,
                )
                .expect_err("refused");
            assert_eq!(error.code, Code::ForbiddenScope, "{subject:?}");
        }
        store
            .mint(
                Subject::Admin,
                MATCH,
                ScopeSet::of(&[Scope::Admin]),
                Tick::ZERO,
            )
            .expect("the lobby may");
    }

    #[test]
    fn planning_scopes_need_a_seat_to_plan_for() {
        let mut store = store();
        for subject in [Subject::Spectator, Subject::Admin] {
            for scope in [Scope::Plan, Scope::PlanSubmit] {
                let error = store
                    .mint(subject, MATCH, ScopeSet::of(&[scope]), Tick::ZERO)
                    .expect_err("refused");
                assert_eq!(error.code, Code::ForbiddenScope, "{subject:?} {scope:?}");
            }
        }
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn an_admin_token_is_minted_for_the_lobby_and_holds_no_seat() {
        let mut store = store();
        let (token, _) = store
            .mint(
                Subject::Admin,
                MATCH,
                ScopeSet::of(&[Scope::Admin]),
                Tick::ZERO,
            )
            .expect("minted");
        let grant = store.authenticate(&token, MATCH, Tick::ZERO).expect("live");
        assert_eq!(grant.subject.seat(), None);
        assert_eq!(grant.subject.render(), "admin");
    }
}
