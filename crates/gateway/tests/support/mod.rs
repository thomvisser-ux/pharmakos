// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the view, control and host-loop suites all need: a hosted match, the
//! production vision, and one way to make a call.
//!
//! A `tests/support/` directory rather than a `tests/support.rs`, because
//! cargo builds every `.rs` directly under `tests/` as its own test binary and
//! a helper module is not a suite.
#![allow(
    dead_code,
    unreachable_pub,
    reason = "three suites share this module and none of them uses all of it; a helper the next \
              suite needs is better here than copied a fourth time, and a helper of a test \
              binary reaches nothing outside it whatever it is spelled"
)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, SphereVision};
use pharmakos_gateway::rpc;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::{Hp, Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::MatchSettings;
use pharmakos_sim::tables::{BeaconId, SeatId};
use pharmakos_sim::voxels::{CHUNK_EDGE, Material, VoxelEdit};
use pharmakos_sim::world::{DamageOrder, DamageTarget, WorldConfig};
use std::path::{Path, PathBuf};

/// The match id every suite here uses.
pub const MATCH: &str = "m-0001";

/// The scenario file's seed, so a reader comparing a transcript with
/// `scenarios/skeleton/expand-east.scenario.jsonc` is looking at the same map.
pub const SEED: u64 = 0x0000_0000_ca5c_aded;

/// A short segment: twenty ticks a test can run to the end.
pub const SEGMENT_MS: i32 = 1_000;

/// `rules.match.lull_ms`, which is what a client counts down.
pub const LULL_MS: i32 = 180_000;

/// How much of the Lull a client reports having spent between two calls.
pub const CLIENT_FRAME_MS: i32 = 50;

/// The workspace root: this crate is `<root>/crates/gateway`.
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/gateway sits two levels below the workspace root"))
        .to_path_buf()
}

/// The cargo target directory, honouring `CARGO_TARGET_DIR`.
#[must_use]
pub fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap_or_else(|| panic!("CARGO_TARGET_TMPDIR always has a parent"))
        .to_path_buf()
}

/// The shipped rules table.
#[must_use]
pub fn rules() -> RulesTable {
    RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
        .expect("the shipped rules table")
}

/// The shipped rules table's canonical JSON, which is what `serve::run` takes.
#[must_use]
pub fn rules_json() -> String {
    rules().canonical_json().to_owned()
}

/// A hosted match sitting in its opening Lull.
#[must_use]
pub fn hosted(seats: u32, segment_ms: i32, round_limit: u32) -> Surface {
    hosted_as(MATCH, FogPolicy::fogged(), seats, segment_ms, round_limit)
}

/// A hosted match with a casual (no-fog) policy, made the way a casual match
/// is made: at [`Surface::new`], and never mid-match.
///
/// Not the same thing as a match that has *ended*, which reaches the same
/// answer through [`FogPolicy::end_match`]. A test that wants "both policies"
/// wants this one and [`hosted`], and a test that wants the unlock wants the
/// other.
#[must_use]
pub fn hosted_casual(seats: u32, segment_ms: i32, round_limit: u32) -> Surface {
    hosted_as(MATCH, FogPolicy::casual(), seats, segment_ms, round_limit)
}

/// A hosted match under a given match id and fog policy.
#[must_use]
pub fn hosted_as(
    match_id: &str,
    policy: FogPolicy,
    seats: u32,
    segment_ms: i32,
    round_limit: u32,
) -> Surface {
    let ids: Vec<SeatId> = (0..seats)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect();
    let mut surface = Surface::new(match_id, SEED, rules(), policy, &ids).expect("a match id");
    let host = Host::open(
        &WorldConfig {
            match_seed: SEED,
            seats,
            units_per_seat: 0,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![segment_ms],
                round_limit,
            },
        },
        None,
    )
    .expect("a match");
    surface.attach(host).expect("attached");
    surface.set_phase_remaining_ms(Ms::new(LULL_MS));
    surface.open_lull().expect("the opening Lull");
    surface
}

/// The production vision, rebuilt from the world as the host loop rebuilds it
/// before every call.
#[must_use]
pub fn vision(surface: &Surface) -> SphereVision {
    surface.host().map_or_else(
        |_| SphereVision::default(),
        |host| SphereVision::of(host.world()),
    )
}

/// One JSON-RPC request.
#[must_use]
pub fn request(method: &str, params: &str) -> rpc::Request {
    rpc::parse(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#
    ))
    .expect("well formed")
}

/// One call, through the production vision.
#[must_use]
pub fn call(surface: &mut Surface, token: &Token, method: &str, params: &str) -> Json {
    let sight = vision(surface);
    surface.call(Some(token), &request(method, params), &sight)
}

/// One call in a Lull, reporting the client's own timer first.
///
/// A Lull consumes no sim tick, so the gateway's tick -- and with it every
/// token's rate budget -- comes from what the client says is left. A test that
/// never moved the timer would be a client that froze its own clock and then
/// complained about its budget.
#[must_use]
pub fn call_in_lull(
    surface: &mut Surface,
    token: &Token,
    remaining: &mut i32,
    method: &str,
    params: &str,
) -> Json {
    *remaining = remaining.saturating_sub(CLIENT_FRAME_MS).max(0);
    surface.set_phase_remaining_ms(Ms::new(*remaining));
    call(surface, token, method, params)
}

/// The `result` member, or a panic naming the refusal.
#[must_use]
pub fn result(response: &Json, what: &str) -> Json {
    response.get("result").cloned().unwrap_or_else(|| {
        panic!(
            "{what} was refused: {}",
            response.get("error").map_or_else(
                || String::from("<no error member>"),
                |error| format!("{error:?}")
            )
        )
    })
}

/// The closed-set code a refusal came back with.
#[must_use]
pub fn code(response: &Json) -> String {
    response
        .get("error")
        .and_then(|error| error.get("data"))
        .and_then(|data| data.get("code"))
        .map_or_else(
            || String::from("<no error>"),
            |value| match value {
                Json::String(text) => text.clone(),
                other => format!("{other:?}"),
            },
        )
}

/// One string member.
#[must_use]
pub fn text_of(value: &Json, key: &str) -> String {
    match value.get(key) {
        Some(Json::String(text)) => text.clone(),
        other => panic!("`{key}` is a string, and it is {other:?}"),
    }
}

/// One array member.
#[must_use]
pub fn array_of(value: &Json, key: &str) -> Vec<Json> {
    match value.get(key) {
        Some(Json::Array(items)) => items.clone(),
        other => panic!("`{key}` is an array, and it is {other:?}"),
    }
}

/// A seat token with the four scopes a seat holds.
///
/// Minted against the surface's **own** match id rather than the constant, so
/// that a test which builds a second match under a second id gets a token
/// that works on it -- a token is tied to a match, and that is a different
/// rule from the view's.
#[must_use]
pub fn seat_token(surface: &mut Surface, seat: u8) -> Token {
    let scopes = ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs]);
    let tick = surface.time().tick;
    let held = surface.match_id().to_owned();
    let (token, _) = surface
        .tokens()
        .mint(Subject::Seat(SeatId::new(seat)), &held, scopes, tick)
        .expect("minted");
    token
}

/// The lobby's token: `admin`, and `observe` so it can watch its own match.
#[must_use]
pub fn admin_token(surface: &mut Surface) -> Token {
    let tick = surface.time().tick;
    let held = surface.match_id().to_owned();
    let (token, _) = surface
        .tokens()
        .mint(
            Subject::Admin,
            &held,
            ScopeSet::of(&[Scope::Admin, Scope::Observe]),
            tick,
        )
        .expect("minted");
    token
}

/// A spectator token that sees through the fog. A seat token can never hold
/// this scope (spec section 12).
#[must_use]
pub fn spectator_token(surface: &mut Surface, nofog: bool) -> Token {
    let mut scopes = ScopeSet::of(&[Scope::Observe, Scope::Docs]);
    if nofog {
        scopes = scopes.with(Scope::SpectateNofog);
    }
    let tick = surface.time().tick;
    let held = surface.match_id().to_owned();
    let (token, _) = surface
        .tokens()
        .mint(Subject::Spectator, &held, scopes, tick)
        .expect("minted");
    token
}

/// Every page of one `get_view`, following the cursor until it completes.
///
/// Returns the pages in order. The last one is the completing page and is the
/// only one that carries the entity list.
#[must_use]
pub fn view_pages(surface: &mut Surface, token: &Token, cursor: &str) -> Vec<Json> {
    let mut pages: Vec<Json> = Vec::new();
    let mut at = cursor.to_owned();
    loop {
        let response = call(
            surface,
            token,
            "get_view",
            &format!(r#"{{"cursor":"{at}"}}"#),
        );
        let page = result(&response, "get_view");
        at = text_of(&page, "next_cursor");
        let complete = matches!(page.get("complete"), Some(Json::Bool(true)));
        pages.push(page);
        if complete {
            return pages;
        }
        assert!(
            pages.len() < 512,
            "a view that never completes is a view that pages for ever"
        );
    }
}

/// The completing page of one `get_view`, and the cursor it leaves behind.
#[must_use]
pub fn view(surface: &mut Surface, token: &Token, cursor: &str) -> (Json, String) {
    let pages = view_pages(surface, token, cursor);
    let last = pages.last().cloned().expect("at least one page");
    let next = text_of(&last, "next_cursor");
    (last, next)
}

/// Every chunk of every page, as `(origin, decoded materials)`.
#[must_use]
pub fn chunks_of(pages: &[Json]) -> Vec<((i32, i32, i32), Vec<u8>)> {
    let mut out: Vec<((i32, i32, i32), Vec<u8>)> = Vec::new();
    for page in pages {
        for chunk in array_of(page, "chunks") {
            let origin = chunk.get("origin").cloned().expect("an origin");
            let axis = |name: &str| match origin.get(name) {
                Some(Json::Number(lexeme)) => lexeme.parse::<i32>().expect("a whole number"),
                other => panic!("`{name}` is a number, and it is {other:?}"),
            };
            let encoded = pharmakos_proto::json::base64::decode(&text_of(&chunk, "voxels_rle"))
                .expect("standard base64");
            let voxels = pharmakos_proto::chunk_rle::decode(&encoded).expect("a whole chunk");
            out.push(((axis("x"), axis("y"), axis("z")), voxels));
        }
    }
    out
}

/// One seat's core beacon: its wire id and where it stands, in whole voxels.
#[must_use]
pub fn core_beacon(surface: &Surface, seat: u8) -> (BeaconId, [i32; 3]) {
    let host = surface.host().expect("a hosted match");
    let beacons = host.world().beacons();
    for row in 0..beacons.ids().len() {
        if beacons.seats().get(row).copied() != Some(seat) {
            continue;
        }
        let id = BeaconId::new(beacons.ids().get(row).copied().expect("an id"));
        let at = beacons.positions().get(row).copied().expect("a position");
        let voxel = pharmakos_gateway::view::voxel_of(at);
        return (id, [voxel.x, voxel.y, voxel.z]);
    }
    panic!("seat {seat} has no beacon");
}

/// Every beacon of one seat, by wire id.
#[must_use]
pub fn beacons_of(surface: &Surface, seat: u8) -> Vec<BeaconId> {
    let host = surface.host().expect("a hosted match");
    let beacons = host.world().beacons();
    (0..beacons.ids().len())
        .filter(|row| beacons.seats().get(*row).copied() == Some(seat))
        .filter_map(|row| beacons.ids().get(row).copied().map(BeaconId::new))
        .collect()
}

/// The sphere radius the rules table gives a beacon.
#[must_use]
pub fn sphere_radius(surface: &Surface) -> i32 {
    let rules = surface.host().expect("a hosted match").rules();
    let raw = rules
        .message()
        .beacon
        .as_ref()
        .map_or(0, |beacon| beacon.sphere_radius_voxels);
    i32::try_from(raw).expect("a sphere radius that fits")
}

/// A solid voxel of `seat`'s core chunk **inside** its sphere, and one
/// **outside** it -- so that one chunk is half seen, which is the case the fog
/// rule is actually about.
///
/// Both are chosen by walking the chunk the core stands in, so the pair is a
/// fact of the generated map rather than an arithmetic guess about it.
#[must_use]
pub fn seen_and_unseen_in_one_chunk(surface: &Surface, seat: u8) -> ([i32; 3], [i32; 3]) {
    let (_, centre) = core_beacon(surface, seat);
    let radius = sphere_radius(surface);
    let edge = i32::try_from(CHUNK_EDGE).expect("32");
    let host = surface.host().expect("a hosted match");
    let voxels = host.world().voxels();
    let origin = [
        centre[0].div_euclid(edge).saturating_mul(edge),
        centre[1].div_euclid(edge).saturating_mul(edge),
        centre[2].div_euclid(edge).saturating_mul(edge),
    ];

    let mut seen: Option<[i32; 3]> = None;
    let mut unseen: Option<[i32; 3]> = None;
    for dz in 0..edge {
        for dy in 0..edge {
            for dx in 0..edge {
                let at = [
                    origin[0].saturating_add(dx),
                    origin[1].saturating_add(dy),
                    origin[2].saturating_add(dz),
                ];
                if voxels.get(at).is_none_or(|material| !material.is_solid()) {
                    continue;
                }
                let d2 = at
                    .iter()
                    .zip(centre.iter())
                    .map(|(here, there)| {
                        let delta = i64::from(*here).saturating_sub(i64::from(*there));
                        delta.saturating_mul(delta)
                    })
                    .sum::<i64>();
                let r2 = i64::from(radius).saturating_mul(i64::from(radius));
                if d2 <= r2 {
                    seen = seen.or(Some(at));
                } else {
                    unseen = unseen.or(Some(at));
                }
            }
        }
    }
    (
        seen.expect("a solid voxel inside the core's own sphere"),
        unseen.expect("a solid voxel of the core's chunk outside its sphere"),
    )
}

/// Turn one voxel to air, through the host-side seam, and settle it.
pub fn erase(surface: &mut Surface, at: [i32; 3]) {
    let host = surface.host_mut().expect("a hosted match");
    assert!(
        host.file_voxel_edit(VoxelEdit::Set {
            at,
            material: Material::AIR,
        }),
        "the edit queue took the edit"
    );
}

/// Knock every one of a seat's beacons down, through the host-side seam.
pub fn fell(surface: &mut Surface, seat: u8) {
    let ids = beacons_of(surface, seat);
    let host = surface.host_mut().expect("a hosted match");
    for id in ids {
        assert!(
            host.file_damage(DamageOrder {
                target: DamageTarget::Beacon(id),
                amount: Hp::new(i32::MAX),
                by: SeatId::NEUTRAL,
            }),
            "the damage queue took the order"
        );
    }
}

/// Step the match `ticks` times, or until it leaves the Push.
pub fn step(surface: &mut Surface, ticks: u32) -> u32 {
    let mut ran: u32 = 0;
    for _ in 0..ticks {
        if surface.step().expect("a hosted match").is_none() {
            break;
        }
        ran = ran.saturating_add(1);
    }
    ran
}

/// The tick a test can mint against before anything has moved.
#[must_use]
pub const fn opening_tick() -> Tick {
    Tick::ZERO
}
