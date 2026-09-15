// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The method/scope table -- read from the schema, never written down twice.
//!
//! T1 put the scope a method needs on the method itself, as a custom
//! `EnumValueOptions` extension in `proto/gp/api/v1/gateway.proto`, and
//! [`pharmakos_proto::scope::required_scope`] reads it back out of the
//! checked-in descriptor set. This module is the gateway's thin layer over that:
//! the JSON-RPC method string in, the [`Method`] and its [`Scope`] out.
//!
//! **There is no second table here on purpose.** A `match` from method name to
//! scope in this file would be a copy of the annotation, and a copy of a
//! contract is a contract that drifts. The one test that matters is
//! `every_method_of_the_schema_resolves_and_needs_a_scope`: if T1 adds a method
//! and forgets its annotation, the gateway's own suite goes red rather than
//! quietly serving it under no scope at all.
//!
//! # Two scopes gate no method, and that is the design
//!
//! `spectate.nofog` and `admin` annotate nothing. They are not oversights:
//!
//! * **`spectate.nofog` gates tokens, not methods** (decisions-log item 26). Fog
//!   is a per-match server-side policy applied by [`crate::fog`], so the scope's
//!   whole job is to mark a *spectator* token as one that sees through it. That
//!   is what keeps "a seat token can never hold `spectate.nofog`" literally true
//!   and keeps every method callable by an ordinary seat.
//! * **`admin` covers lobby and match control**, which in v1 is minting and
//!   revoking tokens, starting and ending a match -- host operations, not
//!   JSON-RPC methods. `gamectl connect` does not exist in v1.
//!
//! So `no_method_is_gated_by_a_token_only_scope` is an assertion about the
//! design rather than a coincidence of today's method list.

pub use pharmakos_proto::gp::api::v1::{Method, Scope};
use pharmakos_proto::scope as annotation;

/// The full name of the method enum in the descriptor set.
const METHOD_ENUM: &str = "gp.api.v1.Method";

/// The full name of the scope enum in the descriptor set.
const SCOPE_ENUM: &str = "gp.api.v1.Scope";

/// The JSON-RPC method string for a method: `get_status`, `submit_plan`.
///
/// Mechanical, from the schema: drop the `METHOD_` prefix and lower-case what is
/// left. `gp.api.v1`'s transport note says the method strings are exactly the
/// `lower_snake_case` names, so this needs no table of its own.
#[must_use]
pub fn method_wire_name(method: Method) -> String {
    annotation::wire_name(METHOD_ENUM, method.as_str_name())
}

/// The method a JSON-RPC method string names, or `None` if the schema has no
/// such method.
#[must_use]
pub fn method_from_wire(name: &str) -> Option<Method> {
    if name.is_empty() {
        return None;
    }
    let value = annotation::from_wire_name(METHOD_ENUM, name)?;
    let method = Method::from_str_name(&value)?;
    if method == Method::Unspecified {
        return None;
    }
    Some(method)
}

/// The scope a token must hold to call `method`.
///
/// `None` only for `METHOD_UNSPECIFIED`, or for a method T1 left unannotated --
/// which the tests below refuse to let happen.
#[must_use]
pub fn required(method: Method) -> Option<Scope> {
    annotation::required_scope(method)
}

/// A scope's spelling on the wire: `observe`, `plan.submit`, `spectate.nofog`.
///
/// Lower case with a dot, which is how spec section 12 writes them and what
/// decisions-log item 80 settled for every enum-valued parameter.
#[must_use]
pub fn scope_wire_name(scope: Scope) -> String {
    annotation::wire_name(SCOPE_ENUM, scope.as_str_name())
}

/// The scope a wire spelling names.
#[must_use]
pub fn scope_from_wire(text: &str) -> Option<Scope> {
    let value = annotation::from_wire_name(SCOPE_ENUM, text)?;
    let scope = Scope::from_str_name(&value)?;
    if scope == Scope::Unspecified {
        return None;
    }
    Some(scope)
}

/// Every scope of the schema, in declared order.
///
/// Written out rather than iterated from the descriptor because a *set* of
/// scopes has to have a fixed iteration order to be rendered deterministically
/// into an audit log, and the schema's own order is that order. The test
/// `the_scope_list_matches_the_schema` keeps the two in step.
pub const ALL: &[Scope] = &[
    Scope::Observe,
    Scope::Plan,
    Scope::PlanSubmit,
    Scope::Docs,
    Scope::SpectateNofog,
    Scope::Admin,
];

/// Scopes a **seat** token may never hold.
///
/// One entry, and it is spec section 12's own sentence: "Seat tokens can never
/// hold `spectate.nofog`; only separate spectator tokens can."
pub const NEVER_ON_A_SEAT: &[Scope] = &[Scope::SpectateNofog];

/// A set of scopes, ordered and cheap.
///
/// A bitset rather than a collection, because the gateway checks one of these on
/// every single call and because an ordered iteration is what the audit log
/// needs. No hash set anywhere near it (AGENTS.md section 4.4).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ScopeSet(u32);

impl ScopeSet {
    /// The empty set.
    pub const EMPTY: ScopeSet = ScopeSet(0);

    /// The set holding exactly the listed scopes.
    #[must_use]
    pub fn of(scopes: &[Scope]) -> ScopeSet {
        let mut set = ScopeSet::EMPTY;
        for scope in scopes {
            set = set.with(*scope);
        }
        set
    }

    /// This set plus `scope`.
    #[must_use]
    pub fn with(self, scope: Scope) -> ScopeSet {
        match bit(scope) {
            Some(bit) => ScopeSet(self.0 | bit),
            None => self,
        }
    }

    /// This set without `scope`.
    #[must_use]
    pub fn without(self, scope: Scope) -> ScopeSet {
        match bit(scope) {
            Some(bit) => ScopeSet(self.0 & !bit),
            None => self,
        }
    }

    /// True when the set holds `scope`.
    #[must_use]
    pub fn holds(self, scope: Scope) -> bool {
        bit(scope).is_some_and(|bit| self.0 & bit != 0)
    }

    /// True when the set holds nothing.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The scopes in the set, in the schema's declared order.
    #[must_use]
    pub fn list(self) -> Vec<Scope> {
        ALL.iter()
            .copied()
            .filter(|scope| self.holds(*scope))
            .collect()
    }

    /// The set as wire names joined by a space: `observe plan plan.submit`.
    /// Deterministic, and what the audit log records.
    #[must_use]
    pub fn render(self) -> String {
        self.list()
            .into_iter()
            .map(scope_wire_name)
            .collect::<Vec<String>>()
            .join(" ")
    }
}

/// The bit a scope occupies. `None` for `SCOPE_UNSPECIFIED`, which is not a
/// scope and cannot be held.
fn bit(scope: Scope) -> Option<u32> {
    if scope == Scope::Unspecified {
        return None;
    }
    let number = u32::try_from(i32::from(scope)).ok()?;
    if number >= 32 {
        return None;
    }
    Some(1_u32 << number)
}

#[cfg(test)]
mod tests {
    use super::{
        ALL, METHOD_ENUM, Method, NEVER_ON_A_SEAT, Scope, ScopeSet, method_from_wire,
        method_wire_name, required, scope_from_wire, scope_wire_name,
    };
    use pharmakos_proto::descriptor::schema;

    /// Every method the schema declares, `METHOD_UNSPECIFIED` aside.
    fn methods() -> Vec<Method> {
        schema()
            .enumeration(METHOD_ENUM)
            .expect("the method enum is in the checked-in descriptor set")
            .values
            .iter()
            .filter_map(|value| Method::from_str_name(&value.name))
            .filter(|method| *method != Method::Unspecified)
            .collect()
    }

    #[test]
    fn every_method_of_the_schema_resolves_and_needs_a_scope() {
        let methods = methods();
        assert!(methods.len() > 20, "found only {}", methods.len());
        for method in methods {
            let name = method_wire_name(method);
            assert_eq!(
                method_from_wire(&name),
                Some(method),
                "`{name}` does not round-trip"
            );
            assert!(
                required(method).is_some(),
                "`{name}` has no required_scope annotation: the gateway would serve it \
                 under no scope at all"
            );
        }
    }

    #[test]
    fn the_wire_names_are_the_ones_the_spec_writes() {
        assert_eq!(method_wire_name(Method::GetStatus), "get_status");
        assert_eq!(method_wire_name(Method::SubmitPlan), "submit_plan");
        assert_eq!(method_wire_name(Method::GetSegmentFeed), "get_segment_feed");
        assert_eq!(scope_wire_name(Scope::PlanSubmit), "plan.submit");
        assert_eq!(scope_wire_name(Scope::SpectateNofog), "spectate.nofog");
        assert_eq!(scope_from_wire("plan.submit"), Some(Scope::PlanSubmit));
        assert_eq!(scope_from_wire("PLAN_SUBMIT"), None, "item 80: lower case");
    }

    #[test]
    fn an_unknown_method_name_resolves_to_nothing() {
        assert_eq!(
            method_from_wire("connect"),
            None,
            "there is no connect in v1"
        );
        assert_eq!(method_from_wire(""), None);
        assert_eq!(method_from_wire("METHOD_GET_STATUS"), None);
        assert_eq!(method_from_wire("get_ballot"), None, "voting is deferred");
    }

    #[test]
    fn the_scopes_of_the_spec_are_the_scopes_of_the_schema() {
        let declared: Vec<String> = schema()
            .enumeration("gp.api.v1.Scope")
            .expect("the scope enum")
            .values
            .iter()
            .filter(|value| value.name != "SCOPE_UNSPECIFIED")
            .map(|value| super::annotation::wire_name("gp.api.v1.Scope", &value.name))
            .collect();
        let ours: Vec<String> = ALL.iter().copied().map(scope_wire_name).collect();
        assert_eq!(declared, ours, "ALL is out of step with the schema");
        assert_eq!(
            ours,
            vec![
                "observe",
                "plan",
                "plan.submit",
                "docs",
                "spectate.nofog",
                "admin"
            ]
        );
    }

    /// The two token-only scopes. See the module docs.
    #[test]
    fn no_method_is_gated_by_a_token_only_scope() {
        for method in methods() {
            let scope = required(method).expect("annotated");
            assert_ne!(
                scope,
                Scope::SpectateNofog,
                "{} is gated by spectate.nofog, which would make it uncallable by any seat \
                 and would break the token invariant instead of the fog policy",
                method_wire_name(method)
            );
            assert_ne!(
                scope,
                Scope::Admin,
                "{} is gated by admin: lobby and match control are host operations, not \
                 JSON-RPC methods, and an admin-gated method is a way for admin to read a \
                 seat's private state",
                method_wire_name(method)
            );
        }
    }

    #[test]
    fn a_scope_set_iterates_in_the_schemas_order_whatever_order_it_was_built_in() {
        let forwards = ScopeSet::of(&[Scope::Observe, Scope::Docs, Scope::PlanSubmit]);
        let backwards = ScopeSet::of(&[Scope::PlanSubmit, Scope::Docs, Scope::Observe]);
        assert_eq!(forwards, backwards);
        assert_eq!(forwards.render(), "observe plan.submit docs");
    }

    #[test]
    fn a_scope_set_holds_what_it_was_given_and_nothing_else() {
        let set = ScopeSet::of(&[Scope::Observe, Scope::Plan]);
        assert!(set.holds(Scope::Observe));
        assert!(set.holds(Scope::Plan));
        assert!(!set.holds(Scope::PlanSubmit));
        assert!(!set.holds(Scope::Unspecified), "unspecified is not a scope");
        assert!(ScopeSet::EMPTY.is_empty());
        assert!(
            !set.with(Scope::Admin)
                .without(Scope::Admin)
                .holds(Scope::Admin)
        );
    }

    #[test]
    fn the_seat_ban_list_is_the_specs_sentence() {
        assert_eq!(NEVER_ON_A_SEAT, &[Scope::SpectateNofog]);
    }
}
