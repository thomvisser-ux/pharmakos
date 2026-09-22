// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The **reference seat view**: the snapshot and the seat's-eye view of it that
//! `gamectl verify` inspects a playbook against.
//!
//! # Why a verifier that is not attached to a match needs one at all
//!
//! A report is a pure function of exactly five inputs — playbook bytes,
//! snapshot, rules hash, verifier version and depth (`pharmakos_verifier`'s
//! crate doc) — and two of those five are a *match*. `gamectl verify` has no
//! match: it is run from a shell against a file, which is the whole point of it
//! existing beside the editor. So it has to name a snapshot and a seat view,
//! and the only honest choices are "an empty one" or "a written-down one".
//!
//! An empty one is the wrong choice, and it is worth saying why rather than
//! only which was taken: with no beacons in view, every `beacon_id` in every
//! real playbook resolves to nothing and the report fills with `E0401`. The
//! tool would refuse the spec's own worked example. What a person wants from
//! `gamectl verify` is the answer the *editor* would give, and the editor is
//! looking at a seat with a core.
//!
//! # So it is written down, once, here — and pinned to the verifier's own
//!
//! This is **the same view `crates/verifier/tests/verifier.rs` verifies every
//! committed case against**, and `tests/golden/verifier/README.md` prints its
//! table for a reader: one seat, a core `Build` beacon `b_01`, a `Mine` beacon
//! `b_02` tagged `east`, one known enemy beacon `e_01`, a treasury of 200 `$`
//! and 10 `kW` of supply against 4 of draw.
//!
//! Two copies of a fixture normally drift. This one cannot drift in silence,
//! and that is the deal: `tests/verify.rs` reads `report_hash` **out of the
//! committed golden** `tests/golden/verifier/expand_east/expected.report.json`
//! and asserts that `gamectl verify examples/playbooks/expand_east.jsonc`
//! reproduces it. The day T6's fixture moves, that test goes red naming this
//! file — which is the skeleton plan's T15 acceptance line, and is why it is
//! written that way rather than against a hash literal.
//!
//! PLACEHOLDER: verifying against a **real** match's frozen snapshot — the one
//! the gateway hands the editor — needs a running match to verify against, so
//! it arrives with the lobby and packaging work (`gamectl` shipped beside the
//! client, **T21**) or with the editor's own integration (**T19**), whichever
//! first gives a shell command a match to point at. The owner decides which,
//! and until then the reference view is what a shell answer is *about* and
//! [`crate::strings::verify_footer`] says so in the output.

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::gp::v1::beacon_filter::MandateKind;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{KnownBeacon, Ownership, Scope};

/// The match seed the reference snapshot carries.
///
/// Not a map that was generated: the verifier treats the snapshot as opaque
/// bytes and hashes them, so what matters is that this is *the same* number
/// `crates/verifier/tests/verifier.rs` uses. It is a counting pattern for
/// exactly that reason — a reader comparing the two files sees it at a glance.
const REFERENCE_SEED: u64 = 0x0102_0304_0506_0708;

/// The tick the reference snapshot is frozen at: the end of a three-minute
/// segment at 20 Hz, which is the first rung of item 68's ladder.
const REFERENCE_TICK: u32 = 3_600;

/// The seat whose view this is.
const REFERENCE_SEAT: u8 = 0;

/// The reference snapshot's postcard bytes.
///
/// # Errors
///
/// [`pharmakos_sim::snapshot::SnapshotError`] when the encoder refuses, which
/// would mean this build's snapshot format and its own encoder disagree.
pub fn snapshot() -> Result<Vec<u8>, pharmakos_sim::snapshot::SnapshotError> {
    Snapshot {
        version: SNAPSHOT_VERSION,
        match_seed: REFERENCE_SEED,
        tick: REFERENCE_TICK,
        ..Snapshot::default()
    }
    .to_bytes()
}

/// One beacon of the reference view.
fn beacon(
    id: &str,
    side: Ownership,
    mandate: MandateKind,
    at: (i32, i32, i32),
    is_core: bool,
    tags: &[&str],
) -> KnownBeacon {
    KnownBeacon {
        beacon_id: id.to_owned(),
        owner: if side == Ownership::EnemyKnown {
            SeatId::new(1)
        } else {
            SeatId::new(REFERENCE_SEAT)
        },
        side,
        mandate,
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        at: Voxel {
            x: at.0,
            y: at.1,
            z: at.2,
        },
        is_core,
    }
}

/// The seat's view of the reference snapshot.
#[must_use]
pub fn scope() -> Scope {
    Scope::new(
        SeatId::new(REFERENCE_SEAT),
        SeatEconomy {
            treasury: Money::new(200),
            supply: Kw::new(10),
            draw: Kw::new(4),
        },
    )
    .with_beacon(beacon(
        "b_01",
        Ownership::Own,
        MandateKind::Build,
        (80, 11, 55),
        true,
        &[],
    ))
    .with_beacon(beacon(
        "b_02",
        Ownership::Own,
        MandateKind::Mine,
        (100, 20, 58),
        false,
        &["east"],
    ))
    .with_beacon(beacon(
        "e_01",
        Ownership::EnemyKnown,
        MandateKind::Unspecified,
        (300, 300, 40),
        false,
        &[],
    ))
}

/// The reference view as the lines `seat doctor` prints, so the one place a
/// person can read what `verify` is answering about is the tool itself.
#[must_use]
pub fn described() -> Vec<String> {
    let scope = scope();
    let mut lines = vec![format!(
        "seat {}, snapshot version {SNAPSHOT_VERSION} at tick {REFERENCE_TICK}",
        scope.seat().raw()
    )];
    for known in scope.beacons() {
        let side = match known.side {
            Ownership::Own => "own",
            Ownership::EnemyKnown => "enemy, known",
        };
        lines.push(format!(
            "{} ({side}{}) at {}, {}, {}",
            known.beacon_id,
            if known.is_core { ", core" } else { "" },
            known.at.x,
            known.at.y,
            known.at.z
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{described, scope, snapshot};

    #[test]
    fn the_reference_view_is_the_one_the_footer_describes() {
        let view = scope();
        let ids: Vec<&str> = view
            .beacons()
            .iter()
            .map(|known| known.beacon_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["b_01", "b_02", "e_01"],
            "tests/golden/verifier/README.md prints this table; the two are one fixture"
        );
        assert!(snapshot().is_ok(), "the reference snapshot encodes");
        assert_eq!(described().len(), 4, "a heading and one line per beacon");
    }
}
