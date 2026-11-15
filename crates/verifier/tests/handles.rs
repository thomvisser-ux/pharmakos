// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An enemy beacon's handle, slot by slot, held to the sim's compile (S3's
//! `vfy6`; the review of S3's `gwr`, item 11).
//!
//! Since S3-04 a sighted `e_NN` resolves in the verifier's scope, so a handle
//! that names an enemy beacon is no longer `E0401` anywhere. The sim compiles
//! every `e_NN` to `BeaconSpec::Foreign`, which resolves to nothing, so in
//! every slot but an Attack target the step fails `no_target` at run time.
//! The verifier refuses each such slot (`E0421`), keeps `E0405` for an
//! `interface` step and `E0414` for a protect list, and lets an Attack target
//! stand. This file holds both halves: what the verifier says in each slot,
//! and that the sim's compile reads the same reference as `Foreign` and
//! resolves it to nothing.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::beacon_filter::MandateKind;
use pharmakos_proto::gp::v1::{BeaconRef, Playbook, Voxel, beacon_ref};
use pharmakos_proto::json;
use pharmakos_sim::interpreter::{BeaconSpec, Plan, Refusal, beacon_spec_of, resolve_beacon_in};
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{
    Input, KnownBeacon, Ownership, Scope, Sighted, Sighting, code_for_construct, verify,
};

fn rules() -> RulesTable {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/verifier sits two levels below the workspace root");
    RulesTable::load(&root.join("rules").join("rules.v1.json")).expect("the committed rules table")
}

fn snapshot() -> Snapshot {
    Snapshot {
        version: SNAPSHOT_VERSION,
        ..Snapshot::default()
    }
}

/// Seat 0's core `b_01`, and the enemy beacon `e_01`, seen just now.
fn scope() -> Scope {
    let beacon = |id: &str, side: Ownership, at: Voxel| KnownBeacon {
        beacon_id: id.to_owned(),
        owner: SeatId::new(u8::from(side != Ownership::Own)),
        side,
        mandate: if side == Ownership::Own {
            MandateKind::Build
        } else {
            MandateKind::Unspecified
        },
        tags: Vec::new(),
        at,
        is_core: side == Ownership::Own,
    };
    let far = Voxel {
        x: 300,
        y: 300,
        z: 40,
    };
    Scope::new(SeatId::new(0), SeatEconomy::default())
        .with_beacon(beacon(
            "b_01",
            Ownership::Own,
            Voxel {
                x: 80,
                y: 11,
                z: 55,
            },
        ))
        .with_beacon(beacon("e_01", Ownership::EnemyKnown, far))
        .with_sighting(Sighting {
            what: Sighted::Beacon("e_01".to_owned()),
            at: far,
            age_ms: 0,
        })
}

/// A playbook whose route is `step`, with `handlers` and `fallback`.
fn playbook(step: &str, handlers: &str, fallback: &str) -> String {
    format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"case","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{step}],"handlers":[{handlers}]}},"on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"fallback":{fallback}}}"#
    )
}

const HOLD: &str = r#"{"label":"s","hold":{"ms":1000}}"#;
const SAFEST: &str = r#"{"hold":{"at":{"safest":{}}}}"#;

/// The errors a QUICK check raises, with their pointers.
fn errors(text: &str, rules: &RulesTable) -> Vec<(String, String)> {
    let bytes = snapshot().to_bytes().expect("the snapshot encodes");
    let scope = scope();
    let input = Input::new(text.as_bytes(), &bytes, &scope, rules).expect("an input");
    verify(&input, Depth::Quick)
        .diagnostics
        .into_iter()
        .filter(|found| found.severity == i32::from(Severity::Error))
        .map(|found| (found.code, found.path))
        .collect()
}

/// One slot: a playbook naming `{beacon}` there (written in as `e_01` or as
/// `b_01`), and the pointer to its `beacon_id`.
struct Place {
    text: String,
    at: &'static str,
}

/// Every slot that names one of the seat's own beacons.
#[allow(
    clippy::too_many_lines,
    reason = "one table of slots; split, a slot could be left out of both halves"
)]
fn own_slots(beacon: &str) -> Vec<Place> {
    let anchor = format!(r#"{{"beacon_anchor":{{"beacon_id":"{beacon}"}}}}"#);
    let rule = |when: String| {
        format!(
            r#"{{"id":"h","when":{when},"body":[{{"label":"b","hold":{{"ms":1000}}}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":1}}"#
        )
    };
    let predicate = |name: &str, rest: &str| {
        rule(format!(
            r#"{{"{name}":{{"beacon":{{"beacon_id":"{beacon}"}}{rest}}}}}"#
        ))
    };
    vec![
        Place {
            text: playbook(
                &format!(r#"{{"label":"m","move":{{"to":{anchor},"pace":"DIRECT"}}}}"#),
                "",
                SAFEST,
            ),
            at: "/declarative/route/0/move/to/beacon_anchor/beacon_id",
        },
        Place {
            text: playbook(HOLD, "", &format!(r#"{{"hold":{{"at":{anchor}}}}}"#)),
            at: "/fallback/hold/at/beacon_anchor/beacon_id",
        },
        Place {
            text: playbook(
                HOLD,
                "",
                &format!(r#"{{"shadow":{{"beacon":{{"beacon_id":"{beacon}"}}}}}}"#),
            ),
            at: "/fallback/shadow/beacon/beacon_id",
        },
        Place {
            text: playbook(
                HOLD,
                "",
                &format!(r#"{{"patrol":{{"waypoints":[{{"safest":{{}}}},{anchor}]}}}}"#),
            ),
            at: "/fallback/patrol/waypoints/1/beacon_anchor/beacon_id",
        },
        Place {
            text: playbook(
                HOLD,
                &predicate("beacon_hp_pct", r#","pct":{"op":"LT","value":50}"#),
                SAFEST,
            ),
            at: "/declarative/handlers/0/when/beacon_hp_pct/beacon/beacon_id",
        },
        Place {
            text: playbook(
                HOLD,
                &predicate("beacon_under_attack", r#","within_ms":5000"#),
                SAFEST,
            ),
            at: "/declarative/handlers/0/when/beacon_under_attack/beacon/beacon_id",
        },
        Place {
            text: playbook(
                HOLD,
                &predicate("beacon_powered", r#","powered":true"#),
                SAFEST,
            ),
            at: "/declarative/handlers/0/when/beacon_powered/beacon/beacon_id",
        },
        Place {
            text: playbook(
                HOLD,
                &predicate(
                    "beacon_units_by_role",
                    r#","role":"SCOUT","count":{"op":"GE","value":1}"#,
                ),
                SAFEST,
            ),
            at: "/declarative/handlers/0/when/beacon_units_by_role/beacon/beacon_id",
        },
        Place {
            text: playbook(
                HOLD,
                &predicate("beacon_mandate", r#","mandate":"BUILD""#),
                SAFEST,
            ),
            at: "/declarative/handlers/0/when/beacon_mandate/beacon/beacon_id",
        },
        Place {
            text: playbook(
                HOLD,
                &predicate("beacon_has_structure", r#","blueprint_id":"generator""#),
                SAFEST,
            ),
            at: "/declarative/handlers/0/when/beacon_has_structure/beacon/beacon_id",
        },
        Place {
            text: playbook(
                HOLD,
                &rule(format!(
                    r#"{{"enemy_units_near":{{"near":{anchor},"radius_voxels":24,"count":{{"op":"GE","value":1}},"freshness":{{"kind":"VISIBLE_NOW"}}}}}}"#
                )),
                SAFEST,
            ),
            at: "/declarative/handlers/0/when/enemy_units_near/near/beacon_anchor/beacon_id",
        },
        Place {
            text: playbook(
                &format!(r#"{{"label":"p","place_beacon":{{"at":{anchor}}}}}"#),
                "",
                SAFEST,
            ),
            at: "/declarative/route/0/place_beacon/at/beacon_anchor/beacon_id",
        },
        Place {
            text: playbook(
                HOLD,
                &rule(format!(
                    r#"{{"enemy_structure_near":{{"near":{anchor},"radius_voxels":24,"any_kind":{{}},"freshness":{{"kind":"VISIBLE_NOW"}}}}}}"#
                )),
                SAFEST,
            ),
            at: "/declarative/handlers/0/when/enemy_structure_near/near/beacon_anchor/beacon_id",
        },
        Place {
            text: playbook(
                &format!(
                    r#"{{"label":"v","interface":{{"beacon":{{"beacon_id":"b_01"}},"rows":[{{"add_build_target":{{"target":{{"blueprint_id":"generator","anchor":{anchor}}}}}}}]}},"timeout_ms":30000}}"#
                ),
                "",
                SAFEST,
            ),
            at: "/declarative/route/0/interface/rows/0/add_build_target/target/anchor/beacon_anchor/beacon_id",
        },
        Place {
            text: playbook(
                &format!(
                    r#"{{"label":"v","interface":{{"beacon":{{"beacon_id":"b_01"}},"rows":[{{"remove_build_target":{{"anchor":{anchor}}}}}]}},"timeout_ms":30000}}"#
                ),
                "",
                SAFEST,
            ),
            at: "/declarative/route/0/interface/rows/0/remove_build_target/anchor/beacon_anchor/beacon_id",
        },
    ]
}

/// What the sim's compile makes of a playbook: accepted, or refused by an
/// entry of its refusal table (an S3 construct `pred` lifts), and nothing
/// else.
fn compiled_or_refused_for_its_stage(text: &str, rules: &RulesTable, at: &str) {
    let decoded: Playbook = json::decode(text).expect("the case decodes");
    if let Err(error) = Plan::compile(&decoded, rules) {
        let refused = Refusal::of(&error)
            .unwrap_or_else(|| panic!("{at}: the sim refuses it for {error}, not its stage"));
        assert!(
            code_for_construct(refused.construct).is_some(),
            "{at}: {error}"
        );
    }
}

#[test]
fn an_enemy_handle_in_a_slot_for_the_seats_own_beacons_is_refused() {
    let rules = rules();
    for place in own_slots("e_01") {
        let found = errors(&place.text, &rules);
        assert!(
            found.contains(&("E0421".to_owned(), place.at.to_owned())),
            "{}: {found:?}",
            place.at
        );
        // The verdict the sim has no door for: it compiles the handle and
        // fails the step at run time.
        compiled_or_refused_for_its_stage(&place.text, &rules, place.at);
    }
    // The same slots naming the seat's own core raise no E0421.
    for place in own_slots("b_01") {
        let found = errors(&place.text, &rules);
        assert!(
            !found.iter().any(|(code, _)| code == "E0421"),
            "{}: {found:?}",
            place.at
        );
    }
}

/// The sim's half: an `e_NN` compiles to a foreign reference, and a foreign
/// reference resolves to nothing for the seat, which is the `no_target` the
/// verifier says ahead of time.
#[test]
fn the_sim_reads_an_enemy_handle_as_foreign_and_resolves_it_to_nothing() {
    let rules = rules();
    let handle = BeaconRef {
        r#ref: Some(beacon_ref::Ref::BeaconId("e_01".to_owned())),
    };
    let spec = beacon_spec_of(&handle, &rules).expect("an e_NN compiles");
    assert_eq!(spec, BeaconSpec::Foreign(1));
    assert_eq!(
        resolve_beacon_in(&snapshot(), &rules, SeatId::new(0), spec, None),
        None
    );
}

#[test]
fn an_attack_target_takes_an_enemy_handle_and_the_other_slots_keep_their_codes() {
    let rules = rules();
    let visit = |row: &str| {
        format!(
            r#"{{"label":"v","interface":{{"beacon":{{"beacon_id":"b_01"}},"rows":[{row}]}},"timeout_ms":30000}}"#
        )
    };
    // An Attack target: no E0421, and the sim compiles it.
    let attack = playbook(
        &visit(
            r#"{"set_mandate":{"attack":{"target":{"beacon":{"beacon_id":"e_01"}},"staging":{"x":90,"y":20,"z":55},"launch":{"force_size":2},"retarget":"NONE","after_action":"RETURN_HOME","waves":1}}}"#,
        ),
        "",
        SAFEST,
    );
    let found = errors(&attack, &rules);
    assert!(!found.iter().any(|(code, _)| code == "E0421"), "{found:?}");
    let decoded: Playbook = json::decode(&attack).expect("the case decodes");
    Plan::compile(&decoded, &rules).expect("the sim compiles an Attack at a handle");
    // An interface step at an enemy beacon is E0405, as before.
    let interface = playbook(
        r#"{"label":"v","interface":{"beacon":{"beacon_id":"e_01"},"rows":[]},"timeout_ms":30000}"#,
        "",
        SAFEST,
    );
    let found = errors(&interface, &rules);
    assert!(
        found.contains(&(
            "E0405".to_owned(),
            "/declarative/route/0/interface/beacon/beacon_id".to_owned()
        )) && !found.iter().any(|(code, _)| code == "E0421"),
        "{found:?}"
    );
    // A protect list naming one is E0414, as before.
    let defend = playbook(
        &visit(
            r#"{"set_mandate":{"defend":{"protect":[{"beacon":{"beacon_id":"e_01"}}],"engagement_radius_pct":50,"pursue":"NEVER"}}}"#,
        ),
        "",
        SAFEST,
    );
    let found = errors(&defend, &rules);
    assert!(
        found.iter().any(|(code, _)| code == "E0414")
            && !found.iter().any(|(code, _)| code == "E0421"),
        "{found:?}"
    );
}

/// A broadcast's places (`mark_target`, `threat_at`, `rally`), which the sim
/// never compiles: an enemy handle there is not `E0421`'s, because no step
/// fails on it (S3's `vfy6`, the review). The file's shape rules still hold
/// there, as `E0408` does for `on`: `toward` in a broadcast is `E0420`.
#[test]
fn a_broadcasts_places_take_any_handle_and_keep_the_shape_rules() {
    let rules = rules();
    for payload in ["mark_target", "threat_at", "rally"] {
        for beacon in ["e_01", "b_01"] {
            let text = playbook(
                &format!(
                    r#"{{"label":"b","broadcast":{{"{payload}":{{"at":{{"beacon_anchor":{{"beacon_id":"{beacon}"}}}}}}}}}}"#
                ),
                "",
                SAFEST,
            );
            let found = errors(&text, &rules);
            assert!(
                !found.iter().any(|(code, _)| code == "E0421"),
                "{payload} at {beacon}: {found:?}"
            );
        }
        let toward = playbook(
            &format!(
                r#"{{"label":"b","broadcast":{{"{payload}":{{"at":{{"toward":{{"feature":{{"feature_id":"vent_0_0"}}}}}}}}}}}}"#
            ),
            "",
            SAFEST,
        );
        let found = errors(&toward, &rules);
        assert!(
            found.contains(&(
                "E0420".to_owned(),
                format!("/declarative/route/0/broadcast/{payload}/at/toward")
            )),
            "{payload}: {found:?}"
        );
    }
}
