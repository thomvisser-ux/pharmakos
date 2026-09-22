// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fog, as a **per-match server-side policy** -- not a scope, and not a flag a
//! client sets.
//!
//! Decisions-log item 26 settles the shape and spec section 12 states it:
//!
//! > Fog is a per-match policy, applied server-side by the gateway's fog filter:
//! > fogged by default, no-fog in casual matches, unlocked on elimination and at
//! > match end. Scopes gate spectator tokens only, so the token invariant holds
//! > literally and no token is reissued mid-match.
//!
//! Two consequences run through this whole module. **No token is reissued
//! mid-match**: when a seat is eliminated, or the match ends, the *policy*
//! changes and the same token starts seeing more. And **`spectate.nofog` is a
//! property of a spectator token, never of a seat**, which is what keeps
//! "a seat token can never hold `spectate.nofog`" literally true while still
//! letting a casual match be played with the fog off.
//!
//! # The filter, in one table
//!
//! | Audience | Seat `s` | Spectator with `spectate.nofog` | Spectator without | Admin |
//! |---|---|---|---|---|
//! | [`Audience::Private`] to `s` | yes | **no** | no | **no** |
//! | [`Audience::Private`] to another seat | no | **no** | no | **no** |
//! | [`Audience::Public`] | yes | yes | yes | yes |
//! | [`Audience::World`] owned by `s` | yes | yes | no | no |
//! | [`Audience::World`] elsewhere | only if seen | yes | no | no |
//!
//! The two bold columns are the ones worth arguing about, so here is the
//! argument. A private event is a seat's own playbook, draft, notebook or
//! private replay reaching it, and spec section 12's secrecy sentence has no
//! exception in it: those four "never leave the gateway for another seat", and a
//! spectator is not the seat. `spectate.nofog` lifts **fog**, which is about the
//! world, and not **secrecy**, which is about a seat's private state. `admin`
//! is the same sentence from the other end: "Admin covers lobby and match
//! control and can never read another seat's playbooks, drafts or knowledge" --
//! and world events are knowledge, so admin sees announcements and nothing else.
//!
//! A spectator without `spectate.nofog` has no seat, and therefore no fogged
//! view to compute: there is no seat whose sightings would decide what it sees.
//! Public announcements are what is left.
//!
//! # What this module does not do
//!
//! It does not decide **what a seat can see** -- that is the sim's vision, and it
//! arrives here through the [`Vision`] trait, which the host implements from the
//! world (T10, T13). The gateway asks; it never computes a line of sight, and it
//! certainly never steps the sim to find one (AGENTS.md section 3 rule 2).

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_sim::tables::SeatId;
use std::collections::BTreeSet;

/// Who is asking.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Viewer {
    /// One seat of the match.
    Seat(SeatId),
    /// A spectator. `nofog` is true when its token carries `spectate.nofog`,
    /// which a seat token can never carry.
    Spectator {
        /// Whether the spectator token holds `spectate.nofog`.
        nofog: bool,
    },
    /// The lobby. Sees announcements and nothing else.
    Admin,
}

/// Who an event is for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Audience {
    /// One seat's own private business: its draft, its notebook, its sealed
    /// playbook, its own commander's orders. Never leaves that seat, for
    /// anybody, under any policy.
    Private(SeatId),
    /// A match-wide announcement: the phase changed, the round ended, a seat was
    /// eliminated. Everyone, including the lobby.
    Public,
    /// Something that happened in the world, at a place. Fog decides.
    World {
        /// The seat whose asset it was, if it was anybody's.
        owner: Option<SeatId>,
        /// Where it happened.
        at: Voxel,
    },
}

/// The match's fog setting.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// Fogged: a seat sees what it has seen. The default (spec section 12).
    #[default]
    Fogged,
    /// No fog, for every seat. A casual match, chosen when the match is made and
    /// never mid-match.
    NoFog,
}

/// The per-match policy.
///
/// Held by the gateway, never by a token, and never sent to a client as
/// something it could set.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FogPolicy {
    mode: Mode,
    /// Seats whose fog has been lifted -- eliminated seats. Ordered and unique,
    /// so iteration is deterministic (AGENTS.md sections 4.4 and 4.6).
    unlocked: BTreeSet<u8>,
    ended: bool,
}

impl FogPolicy {
    /// The default policy: fogged, nobody eliminated, the match still running.
    #[must_use]
    pub fn fogged() -> FogPolicy {
        FogPolicy {
            mode: Mode::Fogged,
            unlocked: BTreeSet::new(),
            ended: false,
        }
    }

    /// A casual match: no fog for any seat, from the start.
    #[must_use]
    pub fn casual() -> FogPolicy {
        FogPolicy {
            mode: Mode::NoFog,
            unlocked: BTreeSet::new(),
            ended: false,
        }
    }

    /// The mode this policy was made with.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Lift the fog for one seat, because it has been eliminated.
    ///
    /// Spec section 12: unlocked on elimination. The seat keeps the token it
    /// already has.
    pub fn eliminate(&mut self, seat: SeatId) {
        self.unlocked.insert(seat.raw());
    }

    /// Lift the fog for everyone, because the match is over.
    pub const fn end_match(&mut self) {
        self.ended = true;
    }

    /// True when the match has ended.
    #[must_use]
    pub const fn ended(&self) -> bool {
        self.ended
    }

    /// True when this seat sees the world without fog.
    #[must_use]
    pub fn unfogged(&self, seat: SeatId) -> bool {
        self.mode == Mode::NoFog || self.ended || self.unlocked.contains(&seat.raw())
    }

    /// The seats whose fog has been lifted individually, in seat order.
    #[must_use]
    pub fn eliminated(&self) -> Vec<SeatId> {
        self.unlocked.iter().copied().map(SeatId::new).collect()
    }

    /// How many seats' fog has been lifted individually.
    ///
    /// The same question as `eliminated().len()` and the answer a tick asks:
    /// [`crate::surface::Surface::step`] compares it before and after, up to
    /// 1 200 times inside one `advance_push`, and a `Vec` built to be
    /// counted and dropped is an allocation per tick on the one thread that
    /// owns the surface.
    #[must_use]
    pub fn unlocked_count(&self) -> usize {
        self.unlocked.len()
    }
}

/// What a seat can see, as the host knows it.
///
/// The gateway asks and never computes. A host wires this to the sim's vision at
/// T13; until then [`KnownVoxels`] is the implementation the tests and the
/// skeleton's host use.
pub trait Vision {
    /// True when `seat` can see `at` right now.
    fn sees(&self, seat: SeatId, at: &Voxel) -> bool;
}

/// A plain set of voxels per seat: the simplest thing that satisfies [`Vision`].
///
/// A `BTreeSet`, never a `HashSet` (AGENTS.md section 4.4) -- not because this
/// is hashed state, but because an unordered container in a crate whose outputs
/// are byte-compared across three operating systems is a trap that only springs
/// once somebody iterates it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct KnownVoxels {
    seen: BTreeSet<(u8, i32, i32, i32)>,
}

impl KnownVoxels {
    /// Nothing seen by anybody.
    #[must_use]
    pub fn new() -> KnownVoxels {
        KnownVoxels {
            seen: BTreeSet::new(),
        }
    }

    /// Record that `seat` can see `at`.
    pub fn see(&mut self, seat: SeatId, at: &Voxel) {
        self.seen.insert((seat.raw(), at.x, at.y, at.z));
    }

    /// How many (seat, voxel) pairs are recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    /// True when nothing is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

impl Vision for KnownVoxels {
    fn sees(&self, seat: SeatId, at: &Voxel) -> bool {
        self.seen.contains(&(seat.raw(), at.x, at.y, at.z))
    }
}

/// Nothing is visible to anybody. Useful as a host's starting state and as the
/// strictest case in a test.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Blind;

impl Vision for Blind {
    fn sees(&self, _seat: SeatId, _at: &Voxel) -> bool {
        false
    }
}

/// The filter itself: policy plus vision.
#[derive(Clone, Debug)]
pub struct FogFilter<'a, V: Vision> {
    policy: &'a FogPolicy,
    vision: &'a V,
}

impl<'a, V: Vision> FogFilter<'a, V> {
    /// A filter over this policy and this vision.
    #[must_use]
    pub const fn new(policy: &'a FogPolicy, vision: &'a V) -> FogFilter<'a, V> {
        FogFilter { policy, vision }
    }

    /// True when `viewer` may be told about something addressed to `audience`.
    ///
    /// The whole table in the module docs, in one function, because a rule split
    /// across call sites is a rule with a hole in it.
    #[must_use]
    pub fn visible(&self, viewer: Viewer, audience: &Audience) -> bool {
        match (viewer, audience) {
            // Secrecy is absolute and has no policy that lifts it.
            (Viewer::Seat(seat), Audience::Private(owner)) => seat == *owner,

            // Announcements are for everyone.
            (_, Audience::Public) => true,

            (Viewer::Seat(seat), Audience::World { owner, at }) => {
                *owner == Some(seat) || self.policy.unfogged(seat) || self.vision.sees(seat, at)
            }
            (Viewer::Spectator { nofog }, Audience::World { .. }) => nofog,

            // A spectator's `spectate.nofog` lifts fog and not secrecy, and
            // `admin` reads no seat's knowledge at all: the two remaining
            // answers are both no, for the two different reasons the module
            // docs give.
            (_, Audience::Private(_) | Audience::World { .. }) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Audience, Blind, FogFilter, FogPolicy, KnownVoxels, Mode, Viewer, Vision};
    use pharmakos_proto::gp::v1::Voxel;
    use pharmakos_sim::tables::SeatId;

    fn voxel(x: i32, y: i32, z: i32) -> Voxel {
        Voxel { x, y, z }
    }

    fn seat(raw: u8) -> SeatId {
        SeatId::new(raw)
    }

    #[test]
    fn a_fogged_seat_never_sees_an_unseen_voxel() {
        let policy = FogPolicy::fogged();
        let filter = FogFilter::new(&policy, &Blind);
        let elsewhere = Audience::World {
            owner: Some(seat(1)),
            at: voxel(200, 200, 40),
        };
        assert!(!filter.visible(Viewer::Seat(seat(0)), &elsewhere));
        assert!(
            filter.visible(Viewer::Seat(seat(1)), &elsewhere),
            "its own asset, which is not fog"
        );
    }

    #[test]
    fn a_seat_sees_a_voxel_it_has_seen() {
        let policy = FogPolicy::fogged();
        let mut vision = KnownVoxels::new();
        let place = voxel(10, 11, 12);
        vision.see(seat(0), &place);
        let filter = FogFilter::new(&policy, &vision);
        let event = Audience::World {
            owner: Some(seat(1)),
            at: place,
        };
        assert!(filter.visible(Viewer::Seat(seat(0)), &event));
        assert!(!filter.visible(Viewer::Seat(seat(2)), &event));
        assert!(!vision.is_empty());
        assert_eq!(vision.len(), 1);
        assert!(!vision.sees(seat(0), &voxel(10, 11, 13)));
    }

    #[test]
    fn a_casual_match_has_no_fog_for_any_seat() {
        let policy = FogPolicy::casual();
        assert_eq!(policy.mode(), Mode::NoFog);
        let filter = FogFilter::new(&policy, &Blind);
        let event = Audience::World {
            owner: Some(seat(1)),
            at: voxel(300, 1, 1),
        };
        assert!(filter.visible(Viewer::Seat(seat(0)), &event));
    }

    #[test]
    fn elimination_lifts_one_seats_fog_without_reissuing_a_token() {
        let mut policy = FogPolicy::fogged();
        policy.eliminate(seat(2));
        let filter = FogFilter::new(&policy, &Blind);
        let event = Audience::World {
            owner: Some(seat(0)),
            at: voxel(5, 5, 5),
        };
        assert!(filter.visible(Viewer::Seat(seat(2)), &event));
        assert!(!filter.visible(Viewer::Seat(seat(1)), &event));
        assert_eq!(policy.eliminated(), vec![seat(2)]);
    }

    #[test]
    fn match_end_lifts_every_seats_fog() {
        let mut policy = FogPolicy::fogged();
        policy.end_match();
        assert!(policy.ended());
        let filter = FogFilter::new(&policy, &Blind);
        let event = Audience::World {
            owner: Some(seat(0)),
            at: voxel(5, 5, 5),
        };
        for raw in 0..3 {
            assert!(filter.visible(Viewer::Seat(seat(raw)), &event));
        }
    }

    #[test]
    fn a_private_event_never_leaves_its_seat_for_anybody() {
        let mut policy = FogPolicy::casual();
        policy.end_match();
        let filter = FogFilter::new(&policy, &Blind);
        let draft = Audience::Private(seat(0));
        assert!(filter.visible(Viewer::Seat(seat(0)), &draft));
        assert!(!filter.visible(Viewer::Seat(seat(1)), &draft));
        assert!(
            !filter.visible(Viewer::Spectator { nofog: true }, &draft),
            "spectate.nofog lifts fog, not secrecy"
        );
        assert!(
            !filter.visible(Viewer::Admin, &draft),
            "admin never reads another seat's drafts (spec section 12)"
        );
    }

    #[test]
    fn admin_sees_announcements_and_nothing_else() {
        let policy = FogPolicy::casual();
        let filter = FogFilter::new(&policy, &Blind);
        assert!(filter.visible(Viewer::Admin, &Audience::Public));
        assert!(!filter.visible(
            Viewer::Admin,
            &Audience::World {
                owner: None,
                at: voxel(1, 2, 3),
            }
        ));
    }

    #[test]
    fn a_spectator_without_the_scope_sees_announcements_only() {
        let policy = FogPolicy::fogged();
        let filter = FogFilter::new(&policy, &Blind);
        let world = Audience::World {
            owner: None,
            at: voxel(1, 2, 3),
        };
        assert!(!filter.visible(Viewer::Spectator { nofog: false }, &world));
        assert!(filter.visible(Viewer::Spectator { nofog: true }, &world));
        assert!(filter.visible(Viewer::Spectator { nofog: false }, &Audience::Public));
    }
}
