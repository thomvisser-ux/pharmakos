// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The seat's view of its own frozen planning snapshot.
//!
//! The gateway builds one of these per verification and hands it in with the
//! snapshot bytes. It is deliberately **small and flat**: everything the QUICK
//! stages need to resolve a reference and to judge placement, and nothing else.
//! It is not a second world model, it cannot be stepped, and it holds no other
//! seat's secrets — fog is the gateway's filter and this is what survives it
//! (AGENTS.md section 7).
//!
//! # Why this type exists at all
//!
//! `report_hash` is a pure function of five inputs, and the second of them is
//! "the snapshot". The verifier reads the seat's *view* of that snapshot rather
//! than the snapshot itself, so the view has to be hashed too or two different
//! views of the same bytes would produce the same hash for two different
//! reports. [`Scope::encode`] is that view's canonical encoding, and
//! `crate::report_hash` appends it directly after the snapshot bytes for exactly
//! this reason.
//!
//! # Voxels, not positions
//!
//! A beacon's place is a [`Voxel`] — whole numbers — and not the sim's Q16.16
//! [`Position`](pharmakos_sim::knowledge::Position). A playbook names voxels
//! (spec section 15, "Maths"), the checks here compare a playbook's voxel with a
//! beacon's, and the conversion from the sim's fixed point belongs at the
//! gateway's edge where the view is built. The economy comes straight across as
//! [`SeatEconomy`], which is already the seat's own view of its own money and
//! power.

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::gp::v1::beacon_filter::{MandateKind, Side};
use pharmakos_sim::encoding::Enc;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::tables::SeatId;

/// One beacon the seat knows about, as the seat knows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KnownBeacon {
    /// The beacon's string id. A playbook's `beacon_id` is matched against this
    /// **exactly**: bindings are per match, and a near miss is a diagnostic
    /// rather than a guess.
    pub beacon_id: String,
    /// Which seat owns it.
    pub owner: SeatId,
    /// Which side it is on from this seat's point of view. `ENEMY_KNOWN` means
    /// the seat has seen it, not that the seat can read it.
    pub side: Side,
    /// The writ it is on. `MANDATE_KIND_UNSPECIFIED` where the seat cannot see
    /// one, which is the ordinary case for an enemy beacon.
    pub mandate: MandateKind,
    /// The author labels it carries (`place_beacon.tags`). Own beacons only: an
    /// enemy beacon carries none of this seat's tags.
    pub tags: Vec<String>,
    /// Where it stands.
    pub at: Voxel,
    /// Whether it is the seat's pre-placed core. The core cannot be recycled
    /// (spec section 5), so the verifier has to be able to tell.
    pub is_core: bool,
}

/// What one seat can see of its own frozen snapshot, for one verification.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scope {
    seat: SeatId,
    economy: SeatEconomy,
    beacons: Vec<KnownBeacon>,
}

impl Scope {
    /// An empty view for `seat`.
    #[must_use]
    pub const fn new(seat: SeatId, economy: SeatEconomy) -> Scope {
        Scope {
            seat,
            economy,
            beacons: Vec::new(),
        }
    }

    /// Add a beacon, keeping the list in ascending `beacon_id` order.
    ///
    /// A beacon already in the list is **replaced**, which is what keeps the id
    /// unique and therefore makes the sort key total (AGENTS.md section 4.6,
    /// item 62). The order is part of [`Scope::encode`], so it is part of
    /// `report_hash`: two gateways that add the same beacons in different
    /// orders must produce the same bytes.
    pub fn push_beacon(&mut self, beacon: KnownBeacon) {
        match self
            .beacons
            .binary_search_by(|existing| existing.beacon_id.cmp(&beacon.beacon_id))
        {
            Ok(index) => {
                if let Some(slot) = self.beacons.get_mut(index) {
                    *slot = beacon;
                }
            }
            Err(index) => self.beacons.insert(index, beacon),
        }
    }

    /// The same, as a builder, so a fixture reads as one expression.
    #[must_use]
    pub fn with_beacon(mut self, beacon: KnownBeacon) -> Scope {
        self.push_beacon(beacon);
        self
    }

    /// Whose view this is.
    #[must_use]
    pub const fn seat(&self) -> SeatId {
        self.seat
    }

    /// The seat's own treasury and power.
    #[must_use]
    pub const fn economy(&self) -> SeatEconomy {
        self.economy
    }

    /// Every beacon the seat knows, in ascending `beacon_id` order.
    #[must_use]
    pub fn beacons(&self) -> &[KnownBeacon] {
        &self.beacons
    }

    /// The beacon with exactly this id, if the seat knows one.
    #[must_use]
    pub fn beacon(&self, beacon_id: &str) -> Option<&KnownBeacon> {
        self.beacons
            .binary_search_by(|existing| existing.beacon_id.as_str().cmp(beacon_id))
            .ok()
            .and_then(|index| self.beacons.get(index))
    }

    /// The seat's own beacons, in the same order.
    pub fn own_beacons(&self) -> impl Iterator<Item = &KnownBeacon> {
        self.beacons
            .iter()
            .filter(|beacon| matches!(beacon.side, Side::Own | Side::Unspecified))
    }

    /// Append the view to the canonical encoding.
    ///
    /// Fixed stride and length prefixes throughout, exactly as
    /// [`Enc`](pharmakos_sim::encoding::Enc) asks: nothing here can be confused
    /// with its neighbour, and no `usize` reaches the bytes.
    pub(crate) fn encode(&self, enc: &mut Enc) {
        enc.u8(self.seat.raw());
        enc.i64(self.economy.treasury.raw());
        enc.i32(self.economy.supply.raw());
        enc.i32(self.economy.draw.raw());
        enc.len(count(self.beacons.len()));
        for beacon in &self.beacons {
            encode_str(enc, &beacon.beacon_id);
            enc.u8(beacon.owner.raw());
            enc.i32(i32::from(beacon.side));
            enc.i32(i32::from(beacon.mandate));
            enc.len(count(beacon.tags.len()));
            for tag in &beacon.tags {
                encode_str(enc, tag);
            }
            enc.i32(beacon.at.x);
            enc.i32(beacon.at.y);
            enc.i32(beacon.at.z);
            enc.bool(beacon.is_core);
        }
    }
}

/// A length prefix for a string or a list.
///
/// Saturating rather than fallible, and for the same reason
/// `pharmakos_sim::rules::RulesTable::encode` saturates: a scope with more than
/// `u32::MAX` beacons in it is not a scope, and a `Result` here would put a
/// failure path into a function that cannot fail in this game.
fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// One length-prefixed UTF-8 string.
fn encode_str(enc: &mut Enc, text: &str) {
    let bytes = text.as_bytes();
    enc.len(count(bytes.len()));
    enc.bytes(bytes);
}

#[cfg(test)]
mod tests {
    use super::{KnownBeacon, Scope};
    use pharmakos_proto::gp::v1::Voxel;
    use pharmakos_proto::gp::v1::beacon_filter::{MandateKind, Side};
    use pharmakos_sim::encoding::Enc;
    use pharmakos_sim::knowledge::SeatEconomy;
    use pharmakos_sim::tables::SeatId;

    fn beacon(id: &str) -> KnownBeacon {
        KnownBeacon {
            beacon_id: id.to_owned(),
            owner: SeatId::new(0),
            side: Side::Own,
            mandate: MandateKind::Build,
            tags: Vec::new(),
            at: Voxel { x: 1, y: 2, z: 3 },
            is_core: false,
        }
    }

    fn digest(scope: &Scope) -> u64 {
        let mut enc = Enc::with_capacity(64);
        scope.encode(&mut enc);
        enc.finish()
    }

    #[test]
    fn the_insertion_order_does_not_reach_the_bytes() {
        let economy = SeatEconomy::default();
        let one = Scope::new(SeatId::new(0), economy)
            .with_beacon(beacon("b_01"))
            .with_beacon(beacon("b_02"));
        let other = Scope::new(SeatId::new(0), economy)
            .with_beacon(beacon("b_02"))
            .with_beacon(beacon("b_01"));
        assert_eq!(digest(&one), digest(&other));
    }

    #[test]
    fn a_repeated_id_replaces_rather_than_joins() {
        let mut scope = Scope::new(SeatId::new(0), SeatEconomy::default());
        scope.push_beacon(beacon("b_01"));
        let mut second = beacon("b_01");
        second.is_core = true;
        scope.push_beacon(second);
        assert_eq!(scope.beacons().len(), 1);
        assert_eq!(scope.beacon("b_01").map(|found| found.is_core), Some(true));
    }

    #[test]
    fn a_beacon_id_is_matched_exactly() {
        let scope = Scope::new(SeatId::new(0), SeatEconomy::default()).with_beacon(beacon("b_01"));
        assert!(scope.beacon("b_01").is_some());
        assert!(scope.beacon("b_0").is_none());
        assert!(scope.beacon("B_01").is_none());
        assert!(scope.beacon("b_01 ").is_none());
    }
}
