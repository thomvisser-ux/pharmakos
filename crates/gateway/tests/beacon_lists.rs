// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A beacon's lists, as `get_beacon` tells a seat (S3's `edlists`): each writ's
//! one list (Build's protected areas, Defend's protect list, Attack's avoid
//! areas, Survey's probe areas), empty, with one entry, and at the sim's room,
//! each entry a sentence in plan-core's words in the order the sim reads it;
//! none on another seat's beacon, in sight or fogged; and the clear a client
//! composes from one, patched and rendered by the gateway.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

mod support;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::Host;
use pharmakos_gateway::surface::Surface;
use pharmakos_plan_core::render::{
    BeaconList, area_line, protected_beacon_line, protected_commander_line,
};
use pharmakos_plan_core::strings as s;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::runner::MatchSettings;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{AreaKind, BeaconId, ColumnBox, SeatId};
use pharmakos_sim::world::{World, WorldConfig};
use support::{
    FIRST_LULL_MS, MATCH, SEED, SEGMENT_MS, array_of, call_in_lull, quote, result, rules,
    seat_token, text_of,
};

/// A two-seat match under `policy` in its opening Lull, resumed from its own
/// frozen snapshot after `edit` has changed a copy of its world: how a test
/// gives a beacon a list only a played Push would, through the save's one
/// restore.
fn resumed(policy: FogPolicy, edit: impl FnOnce(&mut World)) -> Surface {
    let opening = support::hosted_as(MATCH, policy.clone(), 2, SEGMENT_MS, 3);
    let mut world = opening.host().expect("hosted").world().clone();
    edit(&mut world);
    let bytes = Snapshot::capture(&world)
        .expect("a world between ticks")
        .to_bytes()
        .expect("it encodes");
    let mut surface = Surface::new(
        MATCH,
        SEED,
        rules(),
        policy,
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a match id");
    let config = WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: 3,
        },
    };
    let mut host = Host::open(&config, None).expect("a match");
    host.resume(&bytes).expect("the edited snapshot resumes");
    surface.attach(host).expect("attached");
    surface
        .set_phase_remaining_ms(Ms::new(FIRST_LULL_MS))
        .expect("a custom match takes a countdown");
    surface.open_lull().expect("the Lull");
    surface
}

/// `seat`'s core: its row and id.
fn core(world: &World, seat: u8) -> (usize, BeaconId) {
    let beacons = world.beacons();
    let row = (0..beacons.ids().len())
        .find(|row| beacons.seats().get(*row) == Some(&seat))
        .expect("the seat's core");
    (row, BeaconId::new(beacons.ids()[row]))
}

/// The `i`th test area: distinct columns, some west and south of the origin.
fn area(i: i16) -> ColumnBox {
    ColumnBox::new([i * 3 - 4, -2], [i * 3 - 2, i + 1]).expect("a box")
}

/// The line `area(i)` is said as.
fn area_said(i: i16) -> String {
    let held = area(i);
    area_line(held.min_column(), held.max_column())
}

/// Seat 0's `b_00` as `get_beacon` answers it.
fn beacon_answer(surface: &mut Surface, beacon: &str) -> Json {
    let token = seat_token(surface, 0);
    let mut left = FIRST_LULL_MS;
    call_in_lull(
        surface,
        &token,
        &mut left,
        "get_beacon",
        &format!(r#"{{"beacon_id":"{beacon}"}}"#),
    )
}

/// The `lists` rows of an answer.
fn lists(answer: &Json) -> Vec<Json> {
    array_of(&result(answer, "get_beacon"), "lists")
}

/// The one row of `b_00`'s lists, checked against `list`'s facts, and its lines.
fn the_row(surface: &mut Surface, list: BeaconList) -> Vec<String> {
    let rows = lists(&beacon_answer(surface, "b_00"));
    assert_eq!(rows.len(), 1, "one list for the writ: {rows:?}");
    let row = &rows[0];
    assert_eq!(text_of(row, "pointer"), list.pointer());
    assert_eq!(text_of(row, "title"), list.title());
    assert_eq!(text_of(row, "clear_pointer"), list.clear_pointer());
    assert_eq!(text_of(row, "clear_title"), list.clear_title());
    assert_eq!(text_of(row, "empty_title"), s::LIST_EMPTY);
    match row.get("entry_lines") {
        None => Vec::new(),
        Some(Json::Array(lines)) => lines
            .iter()
            .map(|line| match line {
                Json::String(line) => line.clone(),
                other => panic!("a line is a string: {other:?}"),
            })
            .collect(),
        Some(other) => panic!("entry_lines is an array: {other:?}"),
    }
}

/// Seat 0's core switched to `writ`.
fn switched(world: &mut World, writ: MandateKind) -> BeaconId {
    let (row, id) = core(world, 0);
    world.switch_mandate_directly(row, writ);
    id
}

/// Each writ's list, empty: the row is there, with no lines, and Mine has none.
#[test]
fn each_writ_tells_its_list_empty() {
    for (writ, list) in [
        (MandateKind::Build, Some(BeaconList::ProtectedAreas)),
        (MandateKind::Defend, Some(BeaconList::Protect)),
        (MandateKind::Attack, Some(BeaconList::AvoidAreas)),
        (MandateKind::Survey, Some(BeaconList::ProbeAreas)),
        (MandateKind::Mine, None),
    ] {
        let mut surface = resumed(FogPolicy::casual(), |world| {
            let _ = switched(world, writ);
        });
        if let Some(list) = list {
            assert!(the_row(&mut surface, list).is_empty(), "{writ:?}");
        } else {
            let told = result(&beacon_answer(&mut surface, "b_00"), "get_beacon");
            assert_eq!(told.get("lists"), None, "{writ:?} holds no list: {told:?}");
        }
    }
}

/// The areas `count` of the given kind on seat 0's core, as a Build or Survey
/// beacon: answers how many the table took.
fn with_areas(world: &mut World, writ: MandateKind, kind: AreaKind, count: i16) -> usize {
    let id = switched(world, writ);
    let mut taken = 0;
    for i in 0..count {
        let centre = [Fx::from_voxels(i), Fx::from_voxels(0), Fx::from_voxels(0)];
        if world.add_area(id, kind, centre, area(i)).is_err() {
            break;
        }
        taken += 1;
    }
    taken
}

/// Build's protected areas and Survey's probe areas: one entry, then as many as
/// the target table holds for the beacon (the sim's room), each said by its
/// columns, in the order written.
#[test]
fn build_and_survey_tell_their_areas_up_to_the_room() {
    for (writ, kind, list) in [
        (
            MandateKind::Build,
            AreaKind::Protected,
            BeaconList::ProtectedAreas,
        ),
        (MandateKind::Survey, AreaKind::Probe, BeaconList::ProbeAreas),
    ] {
        let mut surface = resumed(FogPolicy::casual(), |world| {
            assert_eq!(with_areas(world, writ, kind, 1), 1);
        });
        assert_eq!(the_row(&mut surface, list), vec![area_said(0)], "{writ:?}");
        let mut room = 0;
        let mut surface = resumed(FogPolicy::casual(), |world| {
            room = with_areas(world, writ, kind, 64);
        });
        assert!(room >= 8, "{writ:?}: the room is {room}");
        let expected: Vec<String> = (0..i16::try_from(room).unwrap()).map(area_said).collect();
        assert_eq!(the_row(&mut surface, list), expected, "{writ:?}");
    }
}

/// Attack's avoid areas: one, then a size budget's worth (128, each costing one
/// unit), in list order.
#[test]
fn attack_tells_its_avoid_areas_in_order() {
    for count in [1_i16, 128] {
        let mut surface = resumed(FogPolicy::casual(), |world| {
            let id = switched(world, MandateKind::Attack);
            let settings = world.attack().row_of(id).expect("an Attack row").settings;
            let avoid: Vec<ColumnBox> = (0..count).map(area).collect();
            assert!(world.set_attack(id, settings, &avoid));
        });
        let expected: Vec<String> = (0..count).map(area_said).collect();
        assert_eq!(the_row(&mut surface, BeaconList::AvoidAreas), expected);
    }
}

/// Defend's protect list: the commander first, then each protected beacon by
/// its name in list order; at the room, every beacon the seat can hold.
#[test]
fn defend_tells_its_protect_list_commander_first() {
    let mut surface = resumed(FogPolicy::casual(), |world| {
        let id = switched(world, MandateKind::Defend);
        let mut settings = world.defend().row_of(id).expect("a Defend row").settings;
        settings.escort = true;
        assert!(world.set_defend(id, settings, &[id]));
    });
    assert_eq!(
        the_row(&mut surface, BeaconList::Protect),
        vec![protected_commander_line(), protected_beacon_line("b_00")]
    );
    // The room: every beacon seat 0 may hold, each protected, newest first so
    // the list's order is not the table's.
    let mut held = 0;
    let mut surface = resumed(FogPolicy::casual(), |world| {
        let id = switched(world, MandateKind::Defend);
        let at = world.beacons().positions()[core(world, 0).0];
        let mut placed = vec![id];
        while let Some(more) =
            world.place_beacon_directly(SeatId::new(0), at, MandateKind::Build, 2)
        {
            placed.push(more);
        }
        placed.reverse();
        held = placed.len();
        let settings = world.defend().row_of(id).expect("a Defend row").settings;
        assert!(world.set_defend(id, settings, &placed));
    });
    assert!(held > 2, "the seat holds {held} beacons");
    let lines = the_row(&mut surface, BeaconList::Protect);
    assert_eq!(lines.len(), held);
    assert_eq!(lines.last(), Some(&protected_beacon_line("b_00")));
    let first = lines.first().expect("a line");
    assert!(
        first.starts_with(s::BEACON_NOUN) && !first.contains("b_00\""),
        "{first}"
    );
}

/// Another seat's beacon carries no list, in its owner's sight or not, casual
/// or fogged, though it holds one.
#[test]
fn another_seats_beacon_carries_no_list() {
    for policy in [FogPolicy::casual(), FogPolicy::fogged()] {
        let mut surface = resumed(policy.clone(), |world| {
            let (row, id) = core(world, 1);
            world.switch_mandate_directly(row, MandateKind::Defend);
            let mut settings = world.defend().row_of(id).expect("a Defend row").settings;
            settings.escort = true;
            assert!(world.set_defend(id, settings, &[id]));
        });
        let token = seat_token(&mut surface, 0);
        let mut left = FIRST_LULL_MS;
        let listed = result(
            &call_in_lull(&mut surface, &token, &mut left, "list_beacons", "{}"),
            "list_beacons",
        );
        let theirs: Vec<String> = array_of(&listed, "beacons")
            .iter()
            .map(|row| text_of(row, "beacon_id"))
            .filter(|id| id.starts_with("e_"))
            .collect();
        for name in &theirs {
            let answer = beacon_answer(&mut surface, name);
            let told = result(&answer, "get_beacon");
            assert_eq!(told.get("lists"), None, "{policy:?} {name}: {told:?}");
        }
        // Their own seat sees it.
        let token = seat_token(&mut surface, 1);
        let answer = call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "get_beacon",
            r#"{"beacon_id":"b_00"}"#,
        );
        assert_eq!(lists(&answer).len(), 1, "{policy:?}");
        if policy == FogPolicy::casual() {
            assert!(!theirs.is_empty(), "a casual match lists the other core");
        }
    }
}

/// The worst `get_beacon` answer a list can make: an Attack beacon with a size
/// budget's worth of avoid areas at the map's widest columns. Its size is said,
/// so a reader can set it beside the largest answer the gateway sends.
#[test]
fn the_largest_list_answer_is_measured() {
    let mut surface = resumed(FogPolicy::casual(), |world| {
        let id = switched(world, MandateKind::Attack);
        let settings = world.attack().row_of(id).expect("an Attack row").settings;
        let widest = ColumnBox::new([i16::MIN, i16::MIN], [i16::MAX, i16::MAX]).expect("a box");
        assert!(world.set_attack(id, settings, &[widest; 128]));
    });
    let answer = beacon_answer(&mut surface, "b_00");
    let bytes = pharmakos_gateway::rpc::render(&answer).len();
    let lines = the_row(&mut surface, BeaconList::AvoidAreas);
    assert_eq!(lines.len(), 128);
    println!("[edlists] the widest Attack list's get_beacon answer: {bytes} bytes");
    assert!(bytes < 64 * 1024, "{bytes} bytes");
}

/// The clear a client composes from a list (one visit, one row writing the
/// clear pointer true), patched by the gateway: the text carries the flag, and
/// the rendered rule says the clear in its words.
#[test]
fn the_clear_is_patched_and_rendered_in_its_words() {
    let mut surface = resumed(FogPolicy::casual(), |world| {
        let _ = with_areas(world, MandateKind::Build, AreaKind::Protected, 1);
    });
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let rows = lists(&beacon_answer(&mut surface, "b_00"));
    let clear = text_of(&rows[0], "clear_pointer");
    // The client's composition: the pointer's segments, outermost first.
    let segments: Vec<&str> = clear.trim_start_matches('/').split('/').collect();
    let settings = segments
        .iter()
        .rev()
        .fold(String::from("true"), |inner, key| {
            format!(r#"{{"{key}":{inner}}}"#)
        });
    let patch = format!(
        r#"[{{"op":"add","path":"/declarative/route/-","value":{{"label":"visit_1","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"set_mandate_settings":{settings}}}]}}}}}}]"#
    );
    let text = r#"{"schema_version":{"major":1},"kind":"PLAYBOOK","meta":{"title":"clear","author_kind":"HUMAN"},"declarative":{"route":[]},"on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":3},"fallback":{"hold":{"at":{"safest":{}}}}}"#;
    let patched = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "patch_plan",
            &format!(
                r#"{{"playbook_jsonc":{},"json_patch":{}}}"#,
                quote(text),
                quote(&patch)
            ),
        ),
        "patch_plan",
    );
    let patched = text_of(&patched, "playbook_jsonc");
    assert!(
        patched.contains(r#""clear_protected_areas":true"#),
        "{patched}"
    );
    let rendered = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "render_plan",
            &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&patched)),
        ),
        "render_plan",
    );
    let prose = text_of(&rendered, "prose");
    assert!(
        prose.contains(&format!("{}: {}", s::ROW_SET_SETTINGS, s::CLEAR_PROTECTED)),
        "{prose}"
    );
}

/// `get_beacon` of a Build core holding two protected areas, as a golden: the
/// wire shape of a list with entries, which the client's fixture
/// `godot/fixtures/beacon_lists.json` copies byte for byte.
#[test]
fn get_beacon_of_a_beacon_with_a_list_answers_its_golden() {
    let mut surface = resumed(FogPolicy::casual(), |world| {
        assert_eq!(
            with_areas(world, MandateKind::Build, AreaKind::Protected, 2),
            2
        );
    });
    let answer = beacon_answer(&mut surface, "b_00");
    let rendered = pharmakos_gateway::rpc::render(&answer);
    assert!(rendered.contains(r#""entry_lines""#), "{rendered}");
    support::write_golden("get_beacon_lists", &rendered);
}
