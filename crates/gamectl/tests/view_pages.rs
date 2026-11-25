// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **The built-in operator's page budget holds the whole view at the gate's
//! load** (S3's `outcap`, fix pass 1, item H). Easy reads at most
//! `pharmakos_operator::easy::PAGES_MAX` pages of `get_view`, and a view that
//! does not complete within them is no view. Since `outcap` a page carries
//! chunks and entities up to `VIEW_PAGE_BYTES`, so this hosts the gate's load
//! (two seats, 150 raiders each on top of their starting force, a casual
//! match so every seat sees every entity) and counts the pages of a seat's
//! keyframe: within `PAGES_MAX`, with the figure printed.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, which does not reach an integration \
              test's helpers; a panic is this file's failure report"
)]

use std::path::{Path, PathBuf};

use pharmakos_gateway::fog::{Blind, FogPolicy};
use pharmakos_gateway::host::Host;
use pharmakos_gateway::rpc;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Subject;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{Layout, LayoutBeacon, MatchSettings};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{SeatId, UnitKind};
use pharmakos_sim::world::WorldConfig;

const SEED: u64 = 0x0000_0000_ca5c_aded;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root")
        .to_path_buf()
}

fn rules() -> RulesTable {
    RulesTable::load(&root().join("rules").join("rules.v1.json")).expect("the rules table")
}

#[test]
fn easys_page_budget_holds_the_view_at_the_gate_load() {
    const UNITS_PER_SEAT: u32 = 150;
    const BEACONS_PER_SEAT: u32 = 5;
    let ids = [SeatId::new(0), SeatId::new(1)];
    let mut surface =
        Surface::new("m-pages", SEED, rules(), FogPolicy::casual(), &ids).expect("a match id");
    let mut host = Host::open(
        &WorldConfig {
            match_seed: SEED,
            seats: 2,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![60_000],
                round_limit: 3,
            },
        },
        None,
    )
    .expect("a match");
    let beacons = ids
        .iter()
        .flat_map(|seat| {
            (0..BEACONS_PER_SEAT).map(move |index| LayoutBeacon {
                seat: *seat,
                column: [
                    40_i32.saturating_add(
                        60_i32.saturating_mul(i32::try_from(index).expect("five beacons")),
                    ),
                    60_i32.saturating_add(200_i32.saturating_mul(i32::from(seat.raw()))),
                ],
                mandate: MandateKind::Build,
                units: vec![(
                    UnitKind::Raider,
                    UNITS_PER_SEAT
                        .checked_div(BEACONS_PER_SEAT)
                        .expect("five beacons"),
                )],
            })
        })
        .collect();
    host.seed_layout(&Layout { beacons })
        .expect("the gate load fits each seat's share");
    surface.attach(host).expect("attached");
    surface
        .set_phase_remaining_ms(Ms::new(600_000))
        .expect("a countdown");
    surface.open_lull().expect("the opening Lull");
    let tick = surface.time().tick;
    let (token, _) = surface
        .tokens()
        .mint(
            Subject::Seat(SeatId::new(0)),
            "m-pages",
            ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs]),
            tick,
        )
        .expect("minted");

    let mut cursor = String::new();
    let mut pages = 0_u32;
    let mut entities = 0_usize;
    loop {
        let request = rpc::parse(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"get_view","params":{{"cursor":"{cursor}"}}}}"#
        ))
        .expect("a request");
        let response = surface.call(Some(&token), &request, &Blind);
        let page = response.get("result").expect("a page").clone();
        pages = pages.saturating_add(1);
        if let Some(Json::Array(listed)) = page.get("entities") {
            entities = entities.saturating_add(listed.len());
        }
        match page.get("next_cursor") {
            Some(Json::String(next)) => next.clone_into(&mut cursor),
            other => panic!("a cursor, and it is {other:?}"),
        }
        if matches!(page.get("complete"), Some(Json::Bool(true))) {
            break;
        }
        assert!(pages < 64, "a view that never completes");
    }
    println!(
        "the gate load's keyframe: {pages} page(s) of {} bytes, {entities} entities, against \
         Easy's {}",
        pharmakos_gateway::viewfeed::VIEW_PAGE_BYTES,
        pharmakos_operator::easy::PAGES_MAX
    );
    assert!(
        entities >= 300,
        "the gate load lists every unit: {entities}"
    );
    assert!(
        pages <= pharmakos_operator::easy::PAGES_MAX,
        "{pages} pages, past Easy's {}",
        pharmakos_operator::easy::PAGES_MAX
    );
}
