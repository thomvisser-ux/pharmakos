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
//!
//! # Features and the commander (S1's targeting)
//!
//! From S1 the view also carries the map's vents and seams as the seat sees
//! them, and the commander's position (docs/design/targeting.md, "Surfaces":
//! "`Scope` gains the features (id, kind, grade, a live bit) and the
//! commander's position"). The verifier **checks** against them — that a name
//! exists, that a description has something to match, that a fixed site is not
//! beside a vent the seat already covers — and **never ranks** them: "nearest"
//! is the sim's and the gateway's, where the pathing graph is. A
//! [`KnownFeature`] therefore carries no travel and no cost. It does carry two
//! things beyond targeting.md's list, both of them facts the gateway already
//! computes for `get_map_summary.features` with the sim's own rules rather than
//! anything the verifier works out for itself: the anchor column (the name's
//! `<x>_<y>`, held as numbers so nothing here parses a name for geometry) and
//! which of the seat's own beacons covers it, which two of the three lints read
//! (S1's plan, task `tgtv`; the gateway fills both in task `tgtw`).
//!
//! A view with no features is legal and is what every gateway builds until
//! `tgtw`: it encodes as an empty list, a named feature then resolves to
//! nothing (`E0412`), and the lints that read the map have nothing to say.

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::gp::v1::beacon_filter::MandateKind;
use pharmakos_proto::gp::v1::by_richness::Richness;
use pharmakos_sim::encoding::Enc;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::tables::SeatId;

/// Whose a beacon is, from this seat's point of view.
///
/// Two values, and no third. `gp.v1.BeaconFilter.Side` would fit the shape and
/// is deliberately **not** used here: its zero value is documented to read as
/// `OWN`, and `proto/gp/v1/playbook.proto`'s header scopes that exception to
/// `BeaconFilter` by name because "both of its enums are filter terms rather
/// than settings". A [`Scope`] is the gateway's *view*, not a filter, so the
/// filter enum's reading has no business leaking into it — a gateway that
/// forgot to set the field would otherwise have every enemy beacon counted as
/// the seat's own, silently, which is exactly the default the schema's "an
/// unset enum is a verifier error" rule exists to prevent.
///
/// The encoding in [`Scope::encode`] deliberately writes the same numbers
/// `BeaconFilter.Side` uses — `OWN = 1`, `ENEMY_KNOWN = 2` — so the view's
/// canonical bytes, and therefore every `report_hash`, are unchanged by holding
/// the distinction in a type of our own.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Ownership {
    /// The seat's own beacon. It can be interfaced with and it anchors a
    /// placement sphere.
    Own = 1,
    /// An enemy beacon the seat has seen. Seen is not readable: the seat may
    /// name it in a filter, and may not walk up and change it.
    EnemyKnown = 2,
}

impl Ownership {
    /// The number [`Scope::encode`] writes, which is the one
    /// `gp.v1.BeaconFilter.Side` uses for the same word.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        match self {
            Ownership::Own => 1,
            Ownership::EnemyKnown => 2,
        }
    }
}

/// One beacon the seat knows about, as the seat knows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KnownBeacon {
    /// The beacon's string id. A playbook's `beacon_id` is matched against this
    /// **exactly**: bindings are per match, and a near miss is a diagnostic
    /// rather than a guess.
    pub beacon_id: String,
    /// Which seat owns it.
    pub owner: SeatId,
    /// Which side it is on from this seat's point of view. There is no third
    /// reading: see [`Ownership`] for why this is not the schema's filter enum.
    pub side: Ownership,
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

/// Which kind of map feature a [`KnownFeature`] is.
///
/// The numbers are `gp.api.v1.MapFeature.Kind`'s for the same two words, so
/// the gateway's view and its `get_map_summary` answer spell one fact one way.
/// There is no unspecified value: a feature is always one or the other.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum FeatureKind {
    /// A heat vent: what a Generator stands on.
    Vent = 1,
    /// A scrap seam: what a Mine beacon digs.
    Seam = 2,
}

impl FeatureKind {
    /// The number [`Scope::encode`] writes, `gp.api.v1.MapFeature.Kind`'s.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        match self {
            FeatureKind::Vent => 1,
            FeatureKind::Seam => 2,
        }
    }
}

/// One vent or seam, as the seat's view holds it (docs/design/targeting.md,
/// "Names" and "Surfaces").
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KnownFeature {
    /// Its name, as a playbook writes it: `vent_<x>_<y>` or `seam_<x>_<y>`. A
    /// playbook's `feature_id` is matched against this **exactly**.
    pub feature_id: String,
    /// Vent or seam.
    pub kind: FeatureKind,
    /// Its grade, the map generator's richness.
    pub grade: Richness,
    /// The generation anchor column's x, the name's `<x>`.
    pub x: i32,
    /// The generation anchor column's y, the name's `<y>`.
    pub y: i32,
    /// False once it is lost: no exposed vent material left in a vent's
    /// footprint, no ore left in a seam's.
    pub live: bool,
    /// The seat's own living beacon whose sphere covers it, the lowest id when
    /// several do, as the sim decides coverage; `None` when it is UNCOVERED.
    /// Own beacons only: another seat's coverage is never in a seat's view.
    pub covered_by: Option<String>,
}

/// What one seat can see of its own frozen snapshot, for one verification.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scope {
    seat: SeatId,
    economy: SeatEconomy,
    beacons: Vec<KnownBeacon>,
    features: Vec<KnownFeature>,
    commander: Option<Voxel>,
}

impl Scope {
    /// An empty view for `seat`: no beacons, no features, and no commander
    /// position.
    #[must_use]
    pub const fn new(seat: SeatId, economy: SeatEconomy) -> Scope {
        Scope {
            seat,
            economy,
            beacons: Vec::new(),
            features: Vec::new(),
            commander: None,
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
            .filter(|beacon| beacon.side == Ownership::Own)
    }

    /// Add a feature, keeping the list in ascending `feature_id` order.
    ///
    /// A feature already in the list is **replaced**, for the reason
    /// [`Scope::push_beacon`] gives: the id stays unique, so the order is
    /// total and the encoding does not depend on the order the gateway added
    /// them in.
    pub fn push_feature(&mut self, feature: KnownFeature) {
        match self
            .features
            .binary_search_by(|existing| existing.feature_id.cmp(&feature.feature_id))
        {
            Ok(index) => {
                if let Some(slot) = self.features.get_mut(index) {
                    *slot = feature;
                }
            }
            Err(index) => self.features.insert(index, feature),
        }
    }

    /// The same, as a builder.
    #[must_use]
    pub fn with_feature(mut self, feature: KnownFeature) -> Scope {
        self.push_feature(feature);
        self
    }

    /// Every feature the seat knows, in ascending `feature_id` order.
    #[must_use]
    pub fn features(&self) -> &[KnownFeature] {
        &self.features
    }

    /// The feature with exactly this id, if the seat knows one.
    #[must_use]
    pub fn feature(&self, feature_id: &str) -> Option<&KnownFeature> {
        self.features
            .binary_search_by(|existing| existing.feature_id.as_str().cmp(feature_id))
            .ok()
            .and_then(|index| self.features.get(index))
    }

    /// Set where the commander stands in the frozen snapshot, as a voxel.
    pub fn set_commander(&mut self, at: Option<Voxel>) {
        self.commander = at;
    }

    /// The same, as a builder.
    #[must_use]
    pub fn with_commander(mut self, at: Voxel) -> Scope {
        self.commander = Some(at);
        self
    }

    /// Where the commander stands, if the view carries it. `covering`'s
    /// "nearest" measures from here at step start; the verifier carries it so
    /// that the hash covers it and never ranks from it.
    #[must_use]
    pub const fn commander(&self) -> Option<Voxel> {
        self.commander
    }

    /// Append the view to the canonical encoding.
    ///
    /// Fixed stride and length prefixes throughout, exactly as
    /// [`Enc`](pharmakos_sim::encoding::Enc) asks: nothing here can be confused
    /// with its neighbour, and no `usize` reaches the bytes.
    ///
    /// The beacons, then the features, then the commander. An absent
    /// `covered_by` or commander is a `false` presence byte; a present one is
    /// `true` followed by the value, so "none" and "the empty string" or "the
    /// origin" never share bytes. The features and the commander widened this
    /// encoding in S1 (`REPORT_HASH_DOMAIN` moved with it), so every
    /// `report_hash` moved once, the view's content unchanged.
    pub(crate) fn encode(&self, enc: &mut Enc) {
        enc.u8(self.seat.raw());
        enc.i64(self.economy.treasury.raw());
        enc.i32(self.economy.supply.raw());
        enc.i32(self.economy.draw.raw());
        enc.len(count(self.beacons.len()));
        for beacon in &self.beacons {
            encode_str(enc, &beacon.beacon_id);
            enc.u8(beacon.owner.raw());
            enc.i32(beacon.side.as_i32());
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
        enc.len(count(self.features.len()));
        for feature in &self.features {
            encode_str(enc, &feature.feature_id);
            enc.i32(feature.kind.as_i32());
            enc.i32(i32::from(feature.grade));
            enc.i32(feature.x);
            enc.i32(feature.y);
            enc.bool(feature.live);
            match feature.covered_by.as_deref() {
                None => enc.bool(false),
                Some(beacon_id) => {
                    enc.bool(true);
                    encode_str(enc, beacon_id);
                }
            }
        }
        match self.commander {
            None => enc.bool(false),
            Some(at) => {
                enc.bool(true);
                enc.i32(at.x);
                enc.i32(at.y);
                enc.i32(at.z);
            }
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
    use super::{FeatureKind, KnownBeacon, KnownFeature, Ownership, Scope};
    use pharmakos_proto::gp::v1::Voxel;
    use pharmakos_proto::gp::v1::beacon_filter::{MandateKind, Side};
    use pharmakos_proto::gp::v1::by_richness::Richness;
    use pharmakos_sim::encoding::Enc;
    use pharmakos_sim::knowledge::SeatEconomy;
    use pharmakos_sim::tables::SeatId;

    fn beacon(id: &str) -> KnownBeacon {
        KnownBeacon {
            beacon_id: id.to_owned(),
            owner: SeatId::new(0),
            side: Ownership::Own,
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
    fn the_view_writes_the_filter_enum_s_own_numbers() {
        // Holding ownership in a type of this crate's own must not move a single
        // `report_hash`, and this is what proves it: the bytes are the numbers
        // `gp.v1.BeaconFilter.Side` uses for the same two words.
        assert_eq!(Ownership::Own.as_i32(), i32::from(Side::Own));
        assert_eq!(Ownership::EnemyKnown.as_i32(), i32::from(Side::EnemyKnown));
    }

    #[test]
    fn an_enemy_beacon_is_never_counted_among_the_seat_s_own() {
        let mut enemy = beacon("e_01");
        enemy.side = Ownership::EnemyKnown;
        let scope = Scope::new(SeatId::new(0), SeatEconomy::default())
            .with_beacon(beacon("b_01"))
            .with_beacon(enemy);
        let own: Vec<&str> = scope
            .own_beacons()
            .map(|found| found.beacon_id.as_str())
            .collect();
        assert_eq!(own, ["b_01"]);
    }

    fn feature(id: &str, covered_by: Option<&str>) -> KnownFeature {
        KnownFeature {
            feature_id: id.to_owned(),
            kind: FeatureKind::Vent,
            grade: Richness::Standard,
            x: 10,
            y: 20,
            live: true,
            covered_by: covered_by.map(str::to_owned),
        }
    }

    #[test]
    fn the_feature_order_does_not_reach_the_bytes() {
        let economy = SeatEconomy::default();
        let one = Scope::new(SeatId::new(0), economy)
            .with_feature(feature("vent_10_20", None))
            .with_feature(feature("seam_30_40", None));
        let other = Scope::new(SeatId::new(0), economy)
            .with_feature(feature("seam_30_40", None))
            .with_feature(feature("vent_10_20", None));
        assert_eq!(digest(&one), digest(&other));
        assert_eq!(
            one.features()
                .iter()
                .map(|found| found.feature_id.as_str())
                .collect::<Vec<_>>(),
            ["seam_30_40", "vent_10_20"]
        );
    }

    #[test]
    fn every_part_of_the_s1_view_moves_the_bytes() {
        let economy = SeatEconomy::default();
        let base = Scope::new(SeatId::new(0), economy);
        let with_feature = base.clone().with_feature(feature("vent_10_20", None));
        let covered = base
            .clone()
            .with_feature(feature("vent_10_20", Some("b_01")));
        let at_origin = base.clone().with_commander(Voxel { x: 0, y: 0, z: 0 });
        let elsewhere = base.clone().with_commander(Voxel { x: 0, y: 0, z: 1 });
        let all = [
            digest(&base),
            digest(&with_feature),
            digest(&covered),
            digest(&at_origin),
            digest(&elsewhere),
        ];
        for (index, one) in all.iter().enumerate() {
            for other in all.iter().skip(index.saturating_add(1)) {
                assert_ne!(one, other, "two different views share bytes");
            }
        }
    }

    #[test]
    fn a_feature_id_is_matched_exactly() {
        let scope = Scope::new(SeatId::new(0), SeatEconomy::default())
            .with_feature(feature("vent_10_20", None));
        assert!(scope.feature("vent_10_20").is_some());
        assert!(scope.feature("vent_10_2").is_none());
        assert!(scope.feature("VENT_10_20").is_none());
    }

    #[test]
    fn the_kind_writes_the_map_summary_s_own_numbers() {
        use pharmakos_proto::gp::api::v1::map_feature::Kind;
        assert_eq!(FeatureKind::Vent.as_i32(), i32::from(Kind::Vent));
        assert_eq!(FeatureKind::Seam.as_i32(), i32::from(Kind::Seam));
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
