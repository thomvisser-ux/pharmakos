// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The economy's surfaces (S1's plan, task `econ`): the recap's settlement
//! and shortfall lines and its round count (the register's X-16;
//! decisions-log item 130 (4)), the briefing's standing (X-03), the
//! forecast's committed spend (S1-46) and `resolve_refs`' phase gate
//! (decisions-log item 133 (3) (f)); and, at the `fog` / `grid` merge, the
//! settlement's band, the shortfall's shed kW and the forecast's next BMI,
//! each the sim's own read (decisions-log item 134 (2) (c)).
//!
//! Every figure is read back against the sim's own answer for the same
//! world -- the audit line, the treasury the Ledger credited, the dormant
//! column -- never against a number restated here.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::Host;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Token;
use pharmakos_proto::json::Json;
use pharmakos_sim::economy::{band_percent, bmi_for, ladder_place};
use pharmakos_sim::math::quantity::{Money, Ms};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::MatchSettings;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::WorldConfig;

use support::{
    FIRST_LULL_MS, MATCH, SEED, SEGMENT_MS, admin_token, array_of, call, call_in_lull, quote,
    result, seat_token, text_of,
};

/// One integer field.
fn number(value: &Json, key: &str) -> i64 {
    match value.get(key) {
        Some(Json::Number(lexeme)) => lexeme.parse().expect("an integer"),
        other => panic!("`{key}` is a number, and it is {other:?}"),
    }
}

/// Step the Push to its segment's end.
fn run_the_push(surface: &mut Surface) {
    assert!(surface.begin_push().expect("the Push begins"));
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
}

/// The ticks one `SEGMENT_MS` segment runs: twenty at 20 Hz.
const SEGMENT_TICKS: u32 = 20;

/// A two-seat match whose grid cannot carry its starting force: the rules
/// table's `power.core_surplus_kw` is 0, so the two units each seat fields
/// draw more than its core supplies, and the brownout order darkens the core
/// at the first settle. Everything else is the shipped table.
fn short_of_power(round_limit: u32) -> Surface {
    let json = support::rules_json().replace("\"core_surplus_kw\": 10", "\"core_surplus_kw\": 0");
    assert!(
        json.contains("\"core_surplus_kw\": 0"),
        "the row was rewritten"
    );
    let rules = RulesTable::from_canonical_json(&json).expect("a rules table");
    let seats = [SeatId::new(0), SeatId::new(1)];
    let mut surface =
        Surface::new(MATCH, SEED, rules.clone(), FogPolicy::fogged(), &seats).expect("a match");
    let host = Host::open(
        &WorldConfig {
            match_seed: SEED,
            seats: 2,
            units_per_seat: 2,
            rules,
            match_settings: MatchSettings {
                segment_lengths_ms: vec![SEGMENT_MS],
                round_limit,
            },
        },
        None,
    )
    .expect("a match");
    surface.attach(host).expect("attached");
    surface.set_phase_remaining_ms(Ms::new(FIRST_LULL_MS));
    surface.open_lull().expect("the opening Lull");
    surface
}

/// The seat's treasury, as its own forecast reads it.
fn treasury(surface: &mut Surface, token: &Token) -> i64 {
    number(
        &result(
            &call(surface, token, "get_economy_forecast", "{}"),
            "get_economy_forecast",
        ),
        "treasury_now",
    )
}

#[test]
fn the_recap_names_the_seats_own_settlement_and_shortfall_and_nobody_elses() {
    let mut surface = short_of_power(3);
    let seat = seat_token(&mut surface, 0);
    let admin = admin_token(&mut surface);

    // Before any segment has ended there is nothing to settle or report.
    let opening = result(&call(&mut surface, &seat, "get_recap", "{}"), "get_recap");
    assert_eq!(
        text_of(&opening, "prose"),
        "No round has ended yet. The match continues."
    );
    assert!(opening.get("settlement").is_none());
    assert!(opening.get("shortfall").is_none());

    let before = treasury(&mut surface, &seat);
    run_the_push(&mut surface);
    let recap = result(&call(&mut surface, &seat, "get_recap", "{}"), "get_recap");
    let prose = text_of(&recap, "prose");
    assert!(
        prose.starts_with(&format!(
            "Round 1 ran {SEGMENT_TICKS} ticks. The match continues."
        )),
        "{prose}"
    );

    // The settlement is what the Ledger credited: the treasury moved by it and
    // by nothing else in a Push that ordered nothing.
    let settlement = recap
        .get("settlement")
        .expect("a seat is told its settlement");
    let credited = treasury(&mut surface, &seat).saturating_sub(before);
    assert!(credited > 0, "the Ledger pays a BMI at every recap");
    assert_eq!(number(settlement, "bmi_dollars"), credited);
    assert_eq!(
        number(settlement, "award_dollars"),
        0,
        "the award fund pays nobody until S4"
    );
    assert_eq!(
        number(settlement, "band_rank"),
        1,
        "two seats tied on held value: the lower seat index leads, and the first band reads 1"
    );
    assert!(
        prose.contains(&format!("credited you $ {credited}")),
        "{prose}"
    );

    // The shortfall names the seat's own dark beacons, as the world has them.
    let shortfall = recap.get("shortfall").expect("the core went dark");
    let dark: Vec<String> = array_of(shortfall, "beacon_ids")
        .iter()
        .map(|id| match id {
            Json::String(text) => text.clone(),
            other => panic!("a beacon id is a string, and it is {other:?}"),
        })
        .collect();
    assert_eq!(dark, vec![String::from("b_00")]);
    let world_says = {
        let world = surface.host().expect("hosted").world();
        let beacons = world.beacons();
        (0..beacons.ids().len()).any(|row| {
            beacons.seats().get(row).copied() == Some(0)
                && beacons.ordinals().get(row).copied() == Some(0)
                && beacons.dormant().get(row).copied() == Some(true)
        })
    };
    assert!(
        world_says,
        "the world has seat 0's core dark at segment end"
    );
    assert!(
        prose.ends_with(&format!(
            "When the segment ended, b_00 was dark: draw outran supply, and the brownout shed {} \
             kW.",
            number(shortfall, "kw")
        )),
        "{prose}"
    );

    // A caller that is no seat is told the match-wide prose and no seat's
    // figures.
    let lobby = result(&call(&mut surface, &admin, "get_recap", "{}"), "get_recap");
    assert!(lobby.get("settlement").is_none() && lobby.get("shortfall").is_none());
    assert_eq!(
        text_of(&lobby, "prose"),
        format!(
            "Round {} ran {SEGMENT_TICKS} ticks. The match continues.",
            1
        )
    );

    // Round 2's Lull still reads round 1's recap.
    surface.end_recap().expect("the recap ends");
    surface.set_phase_remaining_ms(surface.lull_length(2));
    surface.open_lull().expect("round 2's Lull");
    let later = result(&call(&mut surface, &seat, "get_recap", "{}"), "get_recap");
    assert!(
        text_of(&later, "prose").starts_with(&format!("Round 1 ran {SEGMENT_TICKS} ticks.")),
        "{later:?}"
    );
}

#[test]
fn a_recap_after_the_match_ended_counts_the_ticks_its_last_round_ran() {
    // `check`'s finding (decisions-log item 130 (4)): once the match had
    // ended, the recap read "Round N ran 0 ticks" for a round that ran in
    // full, because the sim moves the segment's start to its closing tick.
    let mut surface = support::hosted(2, SEGMENT_MS, 1);
    let seat = seat_token(&mut surface, 0);
    run_the_push(&mut surface);
    let in_recap = text_of(
        &result(&call(&mut surface, &seat, "get_recap", "{}"), "get_recap"),
        "prose",
    );
    assert!(
        in_recap.starts_with(&format!("Round 1 ran {SEGMENT_TICKS} ticks.")),
        "{in_recap}"
    );
    surface.end_recap().expect("the last recap ends");
    let forecast = result(
        &call(&mut surface, &seat, "get_economy_forecast", "{}"),
        "get_economy_forecast",
    );
    assert!(
        forecast.get("bmi_next_dollars").is_none(),
        "an ended match settles nothing more: {forecast:?}"
    );
    let ended = text_of(
        &result(&call(&mut surface, &seat, "get_recap", "{}"), "get_recap"),
        "prose",
    );
    assert!(
        ended.starts_with(&format!(
            "Round 1 ran {SEGMENT_TICKS} ticks. The match ended:"
        )),
        "{ended}"
    );
}

#[test]
fn the_displayed_rank_agrees_with_the_final_audit() {
    let mut surface = support::hosted(2, SEGMENT_MS, 3);
    let mut left = FIRST_LULL_MS;
    let world_audit = pharmakos_sim::audit::final_audit(surface.host().expect("hosted").world());
    let mut first: Vec<SeatId> = Vec::new();
    for raw in [0_u8, 1] {
        let token = seat_token(&mut surface, raw);
        let briefing = result(
            &call_in_lull(&mut surface, &token, &mut left, "get_briefing", "{}"),
            "get_briefing",
        );
        let standing = briefing.get("standing").expect("a standing");
        let line = pharmakos_sim::audit::line_of(
            surface.host().expect("hosted").world(),
            SeatId::new(raw),
        );
        assert_eq!(
            number(standing, "score"),
            line.score.raw(),
            "the score is the seat's own audit line"
        );
        let rank = number(standing, "rank");
        assert!((1..=2).contains(&rank), "{standing:?}");
        if rank == 1 {
            first.push(SeatId::new(raw));
        }
        let prose = text_of(&briefing, "prose");
        assert!(
            prose.contains(&format!(
                "Your score is $ {}, which ranks you {rank} of 2",
                line.score.raw()
            )),
            "{prose}"
        );
    }
    assert_eq!(
        first, world_audit.winners,
        "the seats ranked first are the seats the final audit names"
    );
}

#[test]
fn the_forecast_commits_what_this_rounds_seal_orders_in_the_lull_only() {
    let mut surface = support::hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let forecast = |surface: &mut Surface, left: &mut i32| {
        result(
            &call_in_lull(surface, &token, left, "get_economy_forecast", "{}"),
            "get_economy_forecast",
        )
    };
    assert_eq!(
        number(&forecast(&mut surface, &mut left), "committed_dollars"),
        0,
        "nothing sealed commits nothing"
    );
    let playbook = std::fs::read_to_string(
        support::workspace_root()
            .join("scenarios")
            .join("s1")
            .join("cover-nearest-vent.playbook.jsonc"),
    )
    .expect("the committed scenario playbook");
    let sealed = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&playbook)),
    );
    assert_eq!(
        result(&sealed, "submit_plan").get("accepted"),
        Some(&Json::Bool(true))
    );
    // A beacon and the Generator in its initial settings, at the rules
    // table's prices.
    let structures = support::rules()
        .message()
        .structures
        .expect("a structures block");
    let beacon = structures.beacon.expect("a beacon row").cost_dollars;
    let generator = structures.generator.expect("a generator row").cost_dollars;
    let answered = forecast(&mut surface, &mut left);
    assert_eq!(
        number(&answered, "committed_dollars"),
        i64::from(beacon).saturating_add(i64::from(generator))
    );
    assert!(
        answered.get("bmi_next_dollars").is_some(),
        "the next BMI is answered in a Lull: {answered:?}"
    );

    // In a Push the seal is being paid as it goes: the field is left out.
    assert!(surface.begin_push().expect("the Push begins"));
    let in_push = result(
        &call(&mut surface, &token, "get_economy_forecast", "{}"),
        "get_economy_forecast",
    );
    assert!(in_push.get("committed_dollars").is_none(), "{in_push:?}");
    assert!(
        in_push.get("treasury_now").is_some(),
        "the live answer stays"
    );
}

/// One `place_beacon` whose Survey mandate fields `scouts` scouts, with
/// dormant beacons allowed: legal, one size unit, and it qualifies FULL with
/// warnings only (`W0602`, `W0603`), because the verifier's estimate sums
/// wide. Review A of `econ` found 3 000 000 000 of them made the seat's own
/// forecast answer INTERNAL for the rest of the Lull.
fn many_scouts(scouts: u64) -> String {
    format!(
        r#"{{
  "schema_version": {{"major": 1, "minor": 0}},
  "meta": {{"title": "Many scouts", "author_kind": "HUMAN"}},
  "kind": "PLAYBOOK",
  "declarative": {{
    "route": [
      {{"label": "first", "place_beacon": {{"at": {{"covering": {{"vent": {{"rank": "NEAREST", "coverage": "UNCOVERED"}}}}}},
        "initial": {{"mandate": {{"roe": "RETURN_FIRE", "survey": {{"scout_count": {scouts}}}}}}}}},
        "timeout_ms": 30000, "on_fail": {{"action": "SKIP"}}}}
    ],
    "handlers": [],
    "options": {{"allow_dormant_beacons": true}}
  }},
  "on_death": {{"on_respawn": "CONTINUE"}},
  "fallback": {{"hold": {{"at": {{"safest": {{}}}}}}}}
}}
"#
    )
}

#[test]
fn a_seal_that_does_not_price_is_refused_at_the_door_and_the_forecast_still_answers() {
    let mut surface = support::hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    // 3e9 scouts: the draw leaves the sim's kW type. 3e8: the spend fits `$`
    // and is past the forecast's int32.
    for scouts in [3_000_000_000_u64, 300_000_000] {
        let playbook = many_scouts(scouts);
        let verified = result(
            &call_in_lull(
                &mut surface,
                &token,
                &mut left,
                "verify_plan",
                &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&playbook)),
            ),
            "verify_plan",
        );
        assert_eq!(
            verified
                .get("report")
                .and_then(|report| report.get("qualifies")),
            Some(&Json::Bool(true)),
            "{scouts} scouts qualify FULL: {verified:?}"
        );
        let refused = call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "submit_plan",
            &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&playbook)),
        );
        assert_eq!(
            support::code(&refused),
            "INVALID_ARGUMENT",
            "{scouts} scouts: {refused:?}"
        );
        let message = refused
            .get("error")
            .and_then(|error| error.get("message"))
            .map(|message| format!("{message:?}"))
            .unwrap_or_default();
        assert!(
            message.contains("cannot price its seal"),
            "the price door refused it, not the compile: {message}"
        );
        let forecast = result(
            &call_in_lull(
                &mut surface,
                &token,
                &mut left,
                "get_economy_forecast",
                "{}",
            ),
            "get_economy_forecast",
        );
        assert_eq!(
            number(&forecast, "committed_dollars"),
            0,
            "nothing was sealed, so nothing is committed"
        );
        assert!(forecast.get("treasury_now").is_some());
    }
}

/// Write the fresh answer and compare it with the committed golden, as
/// `tests/targeting.rs` does for its own cases.
fn write_golden(case: &str, rendered: &str) {
    let file = "response.json";
    assert!(!rendered.contains('\r') && rendered.ends_with('\n'));
    let fresh = support::target_dir()
        .join("golden")
        .join("gateway")
        .join(case);
    std::fs::create_dir_all(&fresh).expect("the fresh output's folder");
    std::fs::write(fresh.join(format!("actual.{file}")), rendered).expect("the fresh output");
    let committed = support::workspace_root()
        .join("tests")
        .join("golden")
        .join("gateway")
        .join(case)
        .join(format!("expected.{file}"));
    let Ok(expected) = std::fs::read_to_string(&committed) else {
        panic!(
            "no committed golden at {}. A new case is committed by copying the fresh output \
             beside it, byte for byte, once it is right; `cargo xtask golden --bless` rewrites \
             only goldens that exist.",
            committed.display()
        );
    };
    assert_eq!(
        expected,
        rendered,
        "the golden at {} moved. tests/golden/gateway/README.md says what a diff means; the \
         pull request has to say which input moved it.",
        committed.display()
    );
}

/// The economy's three method answers on the golden seed, as a seat's
/// client is handed them: the briefing in the opening Lull, the forecast
/// once the seat has sealed the cover-and-build playbook, and the recap of
/// round 1.
#[test]
fn the_economy_surfaces_are_goldened_on_the_golden_seed() {
    let mut surface = support::hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    write_golden(
        "briefing",
        &pharmakos_gateway::rpc::render(&call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "get_briefing",
            "{}",
        )),
    );
    let playbook = std::fs::read_to_string(
        support::workspace_root()
            .join("scenarios")
            .join("s1")
            .join("cover-nearest-vent.playbook.jsonc"),
    )
    .expect("the committed scenario playbook");
    let sealed = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&playbook)),
    );
    let _ = result(&sealed, "submit_plan");
    write_golden(
        "economy_forecast",
        &pharmakos_gateway::rpc::render(&call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "get_economy_forecast",
            "{}",
        )),
    );
    run_the_push(&mut surface);
    write_golden(
        "recap",
        &pharmakos_gateway::rpc::render(&call(&mut surface, &token, "get_recap", "{}")),
    );
}

/// The ladder as the Ledger read it at the segment end just played, by seat
/// index: each living seat's held value less the credit it was paid, as
/// `crates/sim/tests/settlement.rs` recovers it.
fn ladder_at_settlement(surface: &Surface, credits: &[(u8, i64)]) -> Vec<Option<Money>> {
    let world = surface.host().expect("hosted").world();
    let seats = world.seats();
    seats
        .seats()
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            seats.is_alive(index).then(|| {
                let paid = credits
                    .iter()
                    .find(|(seat, _)| seat == raw)
                    .map(|(_, credit)| *credit)
                    .expect("a living seat was settled");
                Money::new(world.held_value(SeatId::new(*raw)).raw() - paid)
            })
        })
        .collect()
}

#[test]
fn the_band_and_the_next_bmi_are_the_sims_reads_and_the_next_bmi_is_what_is_settled() {
    let mut surface = short_of_power(3);
    let tokens = [seat_token(&mut surface, 0), seat_token(&mut surface, 1)];
    let rules = surface.host().expect("hosted").world().rules().clone();
    for round in 1..=2_u32 {
        // In the Lull: what each seat's forecast says the next settlement pays,
        // and its treasury before it.
        let mut promised: Vec<(i64, i64)> = Vec::new();
        for token in &tokens {
            let forecast = result(
                &call(&mut surface, token, "get_economy_forecast", "{}"),
                "get_economy_forecast",
            );
            promised.push((
                number(&forecast, "bmi_next_dollars"),
                number(&forecast, "treasury_now"),
            ));
        }
        run_the_push(&mut surface);
        let mut credits: Vec<(u8, i64)> = Vec::new();
        let mut settlements: Vec<Json> = Vec::new();
        for (raw, token) in [0_u8, 1].into_iter().zip(&tokens) {
            let recap = result(&call(&mut surface, token, "get_recap", "{}"), "get_recap");
            let settlement = recap.get("settlement").expect("a settlement").clone();
            credits.push((raw, number(&settlement, "bmi_dollars")));
            settlements.push(settlement);
        }
        let ladder = ladder_at_settlement(&surface, &credits);
        let mut ranks: Vec<i64> = Vec::new();
        for (index, ((raw, credited), settlement)) in credits.iter().zip(&settlements).enumerate() {
            let (next, before) = promised.get(index).copied().expect("a forecast");
            // The forecast's next BMI is what the Ledger then credited, and
            // the treasury moved by exactly that in a Push that ordered
            // nothing.
            assert_eq!(
                next, *credited,
                "round {round}, seat {raw}: the next BMI is the next `settled` value"
            );
            let after = treasury(&mut surface, tokens.get(index).expect("a token"));
            assert_eq!(after - before, *credited, "round {round}, seat {raw}");
            // The band is the sim's own read of the ladder the Ledger read.
            let place = ladder_place(&ladder, index).expect("a settled seat is on the ladder");
            assert_eq!(
                number(settlement, "band_rank"),
                i64::from(place.rank) + 1,
                "round {round}, seat {raw}: band_rank counts from 1"
            );
            assert_eq!(
                number(settlement, "band_percent"),
                i64::from(band_percent(&rules, place.rank, place.living).expect("a band")),
                "round {round}, seat {raw}"
            );
            assert_eq!(
                bmi_for(&rules, place.rank, place.living).raw(),
                *credited,
                "round {round}, seat {raw}: the band read places it where the Ledger paid it"
            );
            ranks.push(number(settlement, "band_rank"));
        }
        ranks.sort_unstable();
        assert_eq!(
            ranks,
            vec![1, 2],
            "round {round}: one leader, one last place"
        );
        assert_ne!(
            credits.first().map(|(_, credit)| credit),
            credits.get(1).map(|(_, credit)| credit),
            "round {round}: the band moves the BMI, so the reads are under test"
        );
        surface.end_recap().expect("the recap ends");
        surface.set_phase_remaining_ms(surface.lull_length(round + 1));
        surface.open_lull().expect("the next Lull");
    }
}

#[test]
fn the_shortfalls_kw_is_the_grids_shed_read_at_segment_end() {
    let mut surface = short_of_power(3);
    let seat = seat_token(&mut surface, 0);
    run_the_push(&mut surface);
    let recap = result(&call(&mut surface, &seat, "get_recap", "{}"), "get_recap");
    let shortfall = recap.get("shortfall").expect("the core went dark");
    // The recap phase consumes no tick, so the world is the segment end's.
    let shed =
        pharmakos_sim::power::dark_load(surface.host().expect("hosted").world(), SeatId::new(0))
            .expect("the grid's read")
            .shed()
            .expect("a shed that fits");
    assert!(shed.raw() > 0, "the dark core relieved the grid: {shed:?}");
    assert_eq!(number(shortfall, "kw"), i64::from(shed.raw()));
    assert!(
        text_of(&recap, "prose").contains(&format!("the brownout shed {} kW.", shed.raw())),
        "{recap:?}"
    );
}
