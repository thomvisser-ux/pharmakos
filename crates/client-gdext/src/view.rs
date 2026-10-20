// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The view feed, decoded: a `get_view` result in, chunks on the drain queue out.
//!
//! Terrain and entities reach the client from ONE gateway method, `get_view` (skeleton
//! plan T16a; decisions-log item 107 (5)). An empty cursor asks for a keyframe, which is the
//! whole generated map (item 107 (1), confirmed as item 108 (1)); any other cursor asks what
//! changed for this viewer since the view the cursor came from. This module is the client's
//! half of that contract, and it is **marshalling**:
//!
//! 1. the result text is read against the schema (`gp.api.v1.GetViewResponse`), with the
//!    `_status` footer split off first because the footer is an envelope concern and not a
//!    field of the message;
//! 2. each chunk's `voxels_rle` is decoded by `pharmakos_proto::chunk_rle` — the one codec,
//!    shared with the producer — and refused whole if its runs do not sum to a chunk;
//! 3. each material byte is looked up in the wire's table ([`palette_of`]) and a value the
//!    table does not know is drawn as **nothing**, which is what `ViewChunk`'s comment
//!    requires of every decoder;
//! 4. the chunk is transposed from the sim's voxel order into the mesher's by
//!    [`crate::chunks`], and stored under its **origin**, which is its key;
//! 5. the light is baked from the materials ([`LightField`], the mesher's own bake), and the
//!    chunks whose surfaces changed go on item 54's [`DrainQueue`].
//!
//! # What the client does not do
//!
//! **No fog logic.** A chunk the seat may not see change is simply never re-sent, and the
//! client keeps its copy; a chunk that is re-sent replaces the copy. There is no reveal
//! toggle and no "last known" state here, because the server decides what each viewer is
//! entitled to (item 107 (5)) and the full-map unlock at elimination or match end is the
//! server's policy change, arriving as ordinary deltas under the same token.
//!
//! **No entity bookkeeping.** `entities` on a completing page is the complete visible list,
//! so an id absent from it is gone; the model replaces its list rather than merging.
//!
//! # The chunk set and the dirty set
//!
//! `bridge.rs` promised (T12) that "the chunk set arrives with the watch rig". It is exactly
//! `GetViewResponse.chunks`: the keyframe establishes the grid and every chunk in it, and a
//! delta's chunks are the dirty set — rebaked where the light changed and queued for the
//! drain. The six neighbour borders T12 wrote and did not wire are wired here, through the
//! mesher's own [`ChunkBorders`], because a whole map is now resident.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use pharmakos_mesher::{
    AIR, CHUNK_EDGE, CHUNK_VOLUME, ChunkBorders, ChunkGrid, DrainBudget, DrainQueue, LightField,
    LightParams, MeshBuffers, Mesher, chunk_view, gpu_surface_bytes,
};
use pharmakos_proto::chunk_rle;
use pharmakos_proto::gp::api::v1::{GetViewResponse, Status, ViewEntity, view_entity};
use pharmakos_proto::json::{self, Json};

use crate::chunks;
use crate::enums;
use crate::error::BridgeError;

/// The key of the envelope footer every gateway result carries (spec section 12,
/// "Budgets"). A leading underscore is not a legal Protobuf identifier, which is exactly why
/// the footer is split off before the result is read against its message.
pub const STATUS_KEY: &str = "_status";

/// [`CHUNK_EDGE`] as an `i32`, for the sim-axes voxel arithmetic of an origin.
const CHUNK_EDGE_I32: i32 = 32;

// The two spellings of the chunk edge cannot drift apart.
const _: () = assert!(CHUNK_EDGE_I32 as usize == CHUNK_EDGE);

/// The mesher's palette id for a wire material byte.
///
/// The wire's table is `ViewChunk`'s comment in `gateway.proto`: 0 air, 1 dirt, 2 stone,
/// 3 to 5 the scrap seam lean, standard and rich, 6 to 8 the heat vent lean, standard and
/// rich; and S2's 9 to 33 (S2's plan, E14 and decisions 10 and 42): 9 to 16 a wall by the
/// seat that built it, whole then cracked, for seats 0 to 3; 17 to 32 each terrain
/// material cracked once and twice, ore and vent keeping their grade; and 33 bedrock. The
/// mesher's palette is its own art table (`pharmakos_mesher::PALETTE`, its ids named in
/// `pharmakos_mesher::palette`), so the two disagree about what 1 and 2 mean, and the
/// palette has no row for a seam's or a vent's grade at all: this lookup is where the
/// wire's meaning meets the art's colours, and it decides nothing about the world.
///
/// **A wall is drawn by the seat its material names, which is who built it.** Whose the
/// wall is now, a seat's or neutral, is the sim's reading of the spheres and is written into
/// no voxel (E14, decision 42), so the client may not make it (AGENTS.md section 3 rule 4)
/// and draws the builder's row. A cracked voxel is solid, so it draws as ground in a darker
/// row of its base, never as a hole.
///
/// **A value this table does not know draws nothing** ([`AIR`]), which is what
/// `ViewChunk`'s comment requires of every decoder and what lets a material be added to
/// the wire without breaking this client. `every_wire_material_draws` walks the wire's
/// table in `gateway.proto` and fails on any value it names that maps to air but air.
///
/// PLACEHOLDER: the seam and vent colours — OWNER, at S6's art pass. Art, like the
/// mesher's palette itself, and whether richness is visible at all; the palette grows the
/// rows these three share today then.
#[must_use]
pub const fn palette_of(wire: u8) -> u8 {
    use pharmakos_mesher::palette;
    match wire {
        1 => palette::DIRT,
        2 => palette::STONE,
        3..=5 => palette::ORE_A, // scrap seam, lean to rich
        6..=8 => palette::ORE_B, // heat vent, lean to rich
        9 => palette::WALL_0,
        10 => palette::WALL_0_CRACKED,
        11 => palette::WALL_1,
        12 => palette::WALL_1_CRACKED,
        13 => palette::WALL_2,
        14 => palette::WALL_2_CRACKED,
        15 => palette::WALL_3,
        16 => palette::WALL_3_CRACKED,
        17 => palette::DIRT_CRACKED_ONCE,
        18 => palette::DIRT_CRACKED_TWICE,
        19 => palette::STONE_CRACKED_ONCE,
        20 => palette::STONE_CRACKED_TWICE,
        21 | 23 | 25 => palette::ORE_A_CRACKED_ONCE, // a scrap seam of any grade
        22 | 24 | 26 => palette::ORE_A_CRACKED_TWICE,
        27 | 29 | 31 => palette::ORE_B_CRACKED_ONCE, // a heat vent of any grade
        28 | 30 | 32 => palette::ORE_B_CRACKED_TWICE,
        33 => palette::BEDROCK,
        _ => AIR, // air, and every value the wire's table does not name
    }
}

/// What kind of thing an entity is, as `ViewEntity.Kind` names it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum EntityKind {
    /// A unit; the commander is a unit whose subtype is `commander`.
    Unit,
    /// A beacon. A seat's own is `b_NN`, which `get_beacon` takes; another seat's carries
    /// the handle the gateway gave this viewer (`e_NN`, or `v_N` for one first seen in a
    /// Push until the next Lull names it), which the client never parses.
    Beacon,
    /// A structure.
    Structure,
    /// A mortar shell in flight (S2). Its owner is the seat that fired it.
    Shell,
    /// What a dead unit or structure leaves (S2). Owned by nobody.
    Wreck,
    /// A structure on a destroyed beacon, turned neutral (S2). Owned by nobody: the wire
    /// spells its owner `""`.
    Ruin,
    /// A kind the schema declares and this build has no marker for (today only
    /// `KIND_UNSPECIFIED`, or a number with no name). Drawn as a generic marker rather than
    /// dropped: the server decided the viewer may see it.
    ///
    /// A kind spelt as a string the schema does NOT declare never reaches here: the strict
    /// codec refuses the page, as it refuses any field it does not know, the watch rig asks
    /// for a keyframe again ([`crate::rig::Rig::view_refused`]) and the refusal is counted.
    /// A new kind is a `gateway.proto` change, and it ships with a client that knows it.
    Unknown,
}

impl EntityKind {
    /// The lower-case name GDScript reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Beacon => "beacon",
            Self::Structure => "structure",
            Self::Shell => "shell",
            Self::Wreck => "wreck",
            Self::Ruin => "ruin",
            Self::Unknown => "unknown",
        }
    }

    /// The kind a wire value names; [`EntityKind::Unknown`] for `KIND_UNSPECIFIED` and for
    /// a number the schema has no name for, which is still drawn.
    #[must_use]
    pub fn of_wire(kind: i32) -> Self {
        match view_entity::Kind::try_from(kind) {
            Ok(view_entity::Kind::Unit) => Self::Unit,
            Ok(view_entity::Kind::Beacon) => Self::Beacon,
            Ok(view_entity::Kind::Structure) => Self::Structure,
            Ok(view_entity::Kind::Shell) => Self::Shell,
            Ok(view_entity::Kind::Wreck) => Self::Wreck,
            Ok(view_entity::Kind::Ruin) => Self::Ruin,
            Ok(view_entity::Kind::Unspecified) | Err(_) => Self::Unknown,
        }
    }
}

/// One thing standing on the terrain, as the viewer may see it.
///
/// Every field is the gateway's, carried as it came: what a seat sees of another seat's
/// asset is the gateway's fog filter's (S2's plan, decision 5), so a field the gateway
/// left unset stays unset here and is not drawn, rather than drawn as a zero.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entity {
    /// Opaque and viewer-scoped. Never parsed here.
    pub id: String,
    /// Unit, beacon, structure, shell, wreck or ruin.
    pub kind: EntityKind,
    /// `commander`, `build_drone`, `generator`; empty when the build has no name for it.
    pub subtype: String,
    /// `seat.0`, or empty for nobody (a wreck, a ruin).
    pub owner: String,
    /// Whole voxels, in the SIM's axes: x east, y north, z up.
    pub at: [i32; 3],
    /// Its hit points and their maximum, whole HP, when the view carries them: on a unit,
    /// a structure or a beacon the viewer sees live, its own or another seat's. `None` on a
    /// shell, a wreck and a ruin, which have none. The schema sends the two together.
    pub hp: Option<Health>,
    /// Its heading in the sim's units, 65 536 to the full turn, 0 east (+x) and 16 384
    /// north (+y), when the view carries one: a unit's. `None` for anything without a
    /// heading of its own.
    pub heading: Option<u16>,
}

/// An entity's hit points as the view gave them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Health {
    /// Hit points now.
    pub hp: u32,
    /// Its maximum hit points, never zero.
    pub max_hp: NonZeroU32,
}
/// One chunk of a page, decoded and ready to store.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PageChunk {
    /// The chunk's lowest voxel, in the sim's axes — the chunk's key.
    pub origin: [i32; 3],
    /// The chunk's materials as palette ids, in the MESHER's voxel order.
    pub materials: Vec<u8>,
}

/// One `get_view` page, decoded.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Page {
    /// Game milliseconds since the start of the segment; zero in a Lull.
    pub at_ms: i32,
    /// The page's chunks, in the order the gateway sent them.
    pub chunks: Vec<PageChunk>,
    /// The complete visible list, on a completing page; empty on any other.
    pub entities: Vec<Entity>,
    /// Where to read from next.
    pub next_cursor: String,
    /// Whether this is the last page of the view.
    pub complete: bool,
}

/// Splits the `_status` footer off a result object.
///
/// Returns the result without its footer, and the footer when there was one. A value that
/// is not an object comes back unchanged with no footer, and the schema read that follows
/// refuses it with a pointer.
#[must_use]
pub fn split_footer(result: &Json) -> (Json, Option<Json>) {
    match result {
        Json::Object(entries) => {
            let mut body: Vec<(String, Json)> = Vec::with_capacity(entries.len());
            let mut footer = None;
            for (key, value) in entries {
                if key == STATUS_KEY {
                    footer = Some(value.clone());
                } else {
                    body.push((key.clone(), value.clone()));
                }
            }
            (Json::Object(body), footer)
        }
        other => (other.clone(), None),
    }
}

/// The `_status` footer, read against `gp.api.v1.Status`.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the footer is not a `Status`.
pub fn read_status(footer: &Json) -> Result<Status, BridgeError> {
    Ok(json::decode_json::<Status>(&enums::canonical(
        "gp.api.v1.Status",
        footer,
    ))?)
}

/// Decodes one `get_view` result: the object the JSON-RPC `result` member holds, footer
/// and all.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the result is not a `GetViewResponse`, and
/// [`BridgeError::View`] when a chunk has no origin, an origin that is not on the chunk
/// grid, or a run list `chunk_rle` refuses. A refused chunk refuses the page: a page drawn
/// with a hole in it is indistinguishable from a map with one.
pub fn decode_page(result: &Json) -> Result<Page, BridgeError> {
    let (body, _) = split_footer(result);
    let response: GetViewResponse =
        json::decode_json(&enums::canonical("gp.api.v1.GetViewResponse", &body))?;
    let mut decoded: Vec<PageChunk> = Vec::with_capacity(response.chunks.len());
    let mut sim_order: Vec<u8> = Vec::with_capacity(CHUNK_VOLUME);
    for chunk in &response.chunks {
        let origin = chunk
            .origin
            .as_ref()
            .map(|voxel| [voxel.x, voxel.y, voxel.z])
            .ok_or_else(|| BridgeError::View("a chunk arrived with no origin".to_owned()))?;
        if chunk_coords_of(origin).is_none() {
            return Err(BridgeError::View(format!(
                "a chunk's origin {origin:?} is not a corner of the 32-voxel chunk grid"
            )));
        }
        let wire = chunk_rle::decode(&chunk.voxels_rle).map_err(|error| {
            BridgeError::View(format!("the chunk at {origin:?} did not decode: {error}"))
        })?;
        sim_order.clear();
        sim_order.extend(wire.iter().map(|byte| palette_of(*byte)));
        let mut materials = Vec::with_capacity(CHUNK_VOLUME);
        chunks::sim_to_mesher_chunk("chunk materials", &sim_order, &mut materials)?;
        decoded.push(PageChunk { origin, materials });
    }
    let entities = response
        .entities
        .iter()
        .map(entity_of)
        .collect::<Result<Vec<Entity>, BridgeError>>()?;
    Ok(Page {
        at_ms: response.at_ms,
        chunks: decoded,
        entities,
        next_cursor: response.next_cursor,
        complete: response.complete,
    })
}

/// One `ViewEntity`, read as it came.
///
/// # Errors
///
/// [`BridgeError::View`] for an entity with no position, a hit-point count without its
/// maximum or the other way round, a maximum of zero, or a heading past the full turn.
/// A refused entity refuses its page, as a refused chunk does: an entity drawn somewhere
/// the gateway never said, or with a bar it never sent, is a guess.
fn entity_of(entity: &ViewEntity) -> Result<Entity, BridgeError> {
    let refused = |why: &str| BridgeError::View(format!("the entity {:?} {why}", entity.id));
    let at = entity
        .at
        .as_ref()
        .map(|voxel| [voxel.x, voxel.y, voxel.z])
        .ok_or_else(|| refused("arrived with no position"))?;
    let hp = match (entity.hp, entity.max_hp) {
        (None, None) => None,
        (Some(hp), Some(max_hp)) => Some(Health {
            hp,
            max_hp: NonZeroU32::new(max_hp)
                .ok_or_else(|| refused("has a maximum of zero hit points"))?,
        }),
        (Some(_), None) | (None, Some(_)) => {
            return Err(refused("carries its hit points or their maximum alone"));
        }
    };
    let heading = entity
        .heading
        .map(|heading| {
            u16::try_from(heading).map_err(|_| refused("has a heading past the full turn"))
        })
        .transpose()?;
    Ok(Entity {
        id: entity.id.clone(),
        kind: EntityKind::of_wire(entity.kind),
        subtype: entity.subtype.clone(),
        owner: entity.owner.clone(),
        at,
        hp,
        heading,
    })
}

/// Decodes one `get_view` result from its text.
///
/// # Errors
///
/// As [`decode_page`], plus [`BridgeError::Schema`] when the text is not JSON.
pub fn decode_page_text(text: &str) -> Result<Page, BridgeError> {
    decode_page(&json::read(text)?)
}

/// A sim-axes chunk origin as the mesher's chunk coordinates.
///
/// The sim's y is north and its z is up; the mesher's y is up and its z is north
/// ([`crate::chunks`]). `None` for an origin that is negative or not a multiple of the chunk
/// edge.
#[must_use]
pub fn chunk_coords_of(origin: [i32; 3]) -> Option<[i32; 3]> {
    let edge = CHUNK_EDGE_I32;
    let mut out = [0_i32; 3];
    for (slot, value) in out.iter_mut().zip(origin) {
        if value < 0 || value.checked_rem(edge)? != 0 {
            return None;
        }
        *slot = value.checked_div(edge)?;
    }
    let [east, north, up] = out;
    Some([east, up, north])
}

/// How many voxel boundaries a pick ray crosses before it gives up: longer than the
/// camera's farthest reach across the whole map.
///
/// PLACEHOLDER: the pick reach — OWNER, ruled at the demo to stand for now.
/// It is a presentation bound with no rule behind it, ruled with the camera's other numbers
/// (decisions-log item 126 (2) (b); register D-23).
pub const PICK_REACH: usize = 4096;

/// The whole map as this client holds it, once a keyframe has established it.
#[derive(Debug)]
struct MapState {
    grid: ChunkGrid,
    /// Chunk-major, mesher order, palette ids — the layout [`LightField`] and
    /// [`chunk_view`] borrow.
    materials: Vec<u8>,
    light: LightField,
    queue: DrainQueue,
    borders: ChunkBorders,
    /// The last surface byte size per chunk, for the drain's byte budget.
    sizes: Vec<usize>,
}

/// What one page did to the model.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Applied {
    /// Whether the view is complete; `false` while more pages are waiting.
    pub complete: bool,
    /// The cursor to read from next.
    pub next_cursor: String,
    /// The page's `at_ms`.
    pub at_ms: i32,
    /// How many chunks the page carried.
    pub chunks: usize,
    /// How many chunks went onto the drain queue because of it.
    pub queued: usize,
    /// Whether the chunk grid was established or re-established by it — the renderer is
    /// rebuilt when it is.
    pub grid_changed: bool,
}

/// The client's copy of the view: every chunk it was sent, the light baked from them, the
/// drain queue, and the latest entity list.
#[derive(Debug)]
pub struct ViewModel {
    params: LightParams,
    budget: DrainBudget,
    /// The map's extent in voxels, sim axes, from the rules table's `map` row: no chunk
    /// origin at or beyond it is taken.
    extent: [u32; 3],
    /// Every chunk ever received, keyed by its sim-axes origin, in mesher order.
    held: BTreeMap<[i32; 3], Vec<u8>>,
    /// Origins whose bytes changed since the last completing page.
    changed: BTreeSet<[i32; 3]>,
    map: Option<MapState>,
    entities: Vec<Entity>,
    /// Drain calls so far: the drain queue's frame counter.
    drains: u64,
}

impl ViewModel {
    /// An empty model that will bake with `params`, drain under `budget` and take chunks
    /// inside `extent` (voxels, sim axes) — all three read from the rules table by
    /// [`crate::rules`].
    #[must_use]
    pub fn new(params: LightParams, budget: DrainBudget, extent: [u32; 3]) -> Self {
        Self {
            params,
            budget,
            extent,
            held: BTreeMap::new(),
            changed: BTreeSet::new(),
            map: None,
            entities: Vec::new(),
            drains: 0,
        }
    }

    /// The chunk grid, once a keyframe has established it.
    #[must_use]
    pub fn grid(&self) -> Option<ChunkGrid> {
        self.map.as_ref().map(|map| map.grid)
    }

    /// How many chunks are waiting on the drain queue.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.map.as_ref().map_or(0, |map| map.queue.len())
    }

    /// The latest complete entity list.
    #[must_use]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// How many chunks this client holds.
    #[must_use]
    pub fn held(&self) -> usize {
        self.held.len()
    }

    /// The voxel of ground a ray from the camera points at: the empty voxel in front of the
    /// first solid face the ray meets, in the SIM's axes (x east, y north, z up), which is
    /// where a commander would stand. `None` when the ray meets nothing within
    /// [`PICK_REACH`] voxels, or starts inside the ground.
    ///
    /// `origin` and `direction` are in the WORLD's axes (x east, y up, z north). This is
    /// picking — which voxel is under the cursor — and a presentation question: the walk
    /// reads only the chunks this client was sent, and whether the voxel is somewhere a
    /// beacon may go is the verifier's answer, asked for by the editor (T19's ghost).
    #[must_use]
    pub fn pick(&self, origin: [f32; 3], direction: [f32; 3]) -> Option<[i32; 3]> {
        let length = direction
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        if length.is_nan() || length <= 0.0 {
            return None;
        }
        let direction = direction.map(|value| value / length);
        let mut cell = origin.map(|value| value.floor() as i32);
        if self.solid_world(cell) {
            return None;
        }
        let setup = |d: f32, o: f32, c: i32| -> (i32, f32, f32) {
            if d > 0.0 {
                (1, ((c as f32) + 1.0 - o) / d, 1.0 / d)
            } else if d < 0.0 {
                (-1, (o - (c as f32)) / -d, 1.0 / -d)
            } else {
                (0, f32::INFINITY, f32::INFINITY)
            }
        };
        let [dx, dy, dz] = direction;
        let [ox, oy, oz] = origin;
        let [cx, cy, cz] = cell;
        let setups = [setup(dx, ox, cx), setup(dy, oy, cy), setup(dz, oz, cz)];
        let step = setups.map(|one| one.0);
        let mut next = setups.map(|one| one.1);
        let delta = setups.map(|one| one.2);
        for _ in 0..PICK_REACH {
            let previous = cell;
            let [east, up, north] = next;
            let axis = if east <= up && east <= north {
                0
            } else if up <= north {
                1
            } else {
                2
            };
            if let (Some(position), Some(time), Some(by), Some(stride)) = (
                cell.get_mut(axis),
                next.get_mut(axis),
                step.get(axis),
                delta.get(axis),
            ) {
                *position = position.saturating_add(*by);
                *time += stride;
            }
            if self.solid_world(cell) {
                let [x, up, north] = previous;
                return Some([x, north, up]);
            }
        }
        None
    }

    /// The ground at column `(x, y)` (sim axes): the empty voxel above the highest solid one
    /// the chunks held show there, as a sim-axes `z`, which is where a marker or a pick point
    /// for that column stands. `None` when this client holds no solid voxel in the column.
    ///
    /// Presentation, like [`ViewModel::pick`]: it reads only what this client was sent, and
    /// says where to draw, never whether anything may stand there.
    #[must_use]
    pub fn ground_at(&self, x: i32, y: i32) -> Option<i32> {
        let top = i32::try_from(self.extent.get(2).copied()?).ok()?;
        (0..top)
            .rev()
            .find(|up| self.solid_world([x, *up, y]))
            .and_then(|up| up.checked_add(1))
    }

    /// Whether the voxel at `world` (x east, y up, z north) is solid in the chunks held.
    fn solid_world(&self, world: [i32; 3]) -> bool {
        let edge = CHUNK_EDGE_I32;
        let [x, up, north] = world;
        if x < 0 || up < 0 || north < 0 {
            return false;
        }
        let local = |value: i32| value.rem_euclid(edge);
        let origin = [
            x.saturating_sub(local(x)),
            north.saturating_sub(local(north)),
            up.saturating_sub(local(up)),
        ];
        let Some(bytes) = self.held.get(&origin) else {
            return false;
        };
        let (Ok(mx), Ok(my), Ok(mz)) = (
            usize::try_from(local(x)),
            usize::try_from(local(up)),
            usize::try_from(local(north)),
        ) else {
            return false;
        };
        bytes
            .get(pharmakos_mesher::voxel_index(mx, my, mz))
            .is_some_and(|material| *material != AIR)
    }

    /// Stores one page, and on a completing page bakes and queues what changed.
    ///
    /// # Errors
    ///
    /// [`BridgeError::View`] when a chunk's origin lies outside the map's extent, when the
    /// grid cannot be built from the chunks held, or when the light bake refuses the map.
    /// A page with a chunk outside the map is refused whole, before anything is stored.
    pub fn apply(&mut self, page: Page) -> Result<Applied, BridgeError> {
        if let Some(outside) = page.chunks.iter().find(|chunk| !self.inside(chunk.origin)) {
            return Err(BridgeError::View(format!(
                "a chunk's origin {:?} is outside the map's extent {:?}",
                outside.origin, self.extent
            )));
        }
        let chunks = page.chunks.len();
        for chunk in page.chunks {
            if self.held.get(&chunk.origin) != Some(&chunk.materials) {
                self.changed.insert(chunk.origin);
                self.held.insert(chunk.origin, chunk.materials);
            }
        }
        let mut queued = 0;
        let mut grid_changed = false;
        if page.complete {
            self.entities = page.entities;
            let outside = self.map.as_ref().is_some_and(|map| {
                self.changed
                    .iter()
                    .any(|origin| index_of(map.grid, *origin).is_none())
            });
            if self.map.is_none() || outside {
                self.build()?;
                grid_changed = true;
                queued = self.pending();
            } else {
                queued = self.rebake_changed()?;
            }
            self.changed.clear();
        }
        Ok(Applied {
            complete: page.complete,
            next_cursor: page.next_cursor,
            at_ms: page.at_ms,
            chunks,
            queued,
            grid_changed,
        })
    }

    /// Whether a chunk at `origin` lies inside the map: every axis non-negative and below
    /// the extent. [`decode_page`] has already refused a negative or off-grid origin.
    fn inside(&self, origin: [i32; 3]) -> bool {
        origin
            .iter()
            .zip(self.extent)
            .all(|(value, limit)| u32::try_from(*value).is_ok_and(|value| value < limit))
    }

    /// Builds the grid from every chunk held, bakes the whole map and queues every chunk.
    fn build(&mut self) -> Result<(), BridgeError> {
        let mut extent = [0_i32; 3];
        for origin in self.held.keys() {
            let coords = chunk_coords_of(*origin)
                .ok_or_else(|| BridgeError::View(format!("{origin:?} is not a chunk origin")))?;
            for (slot, value) in extent.iter_mut().zip(coords) {
                *slot = (*slot).max(value.saturating_add(1));
            }
        }
        let dims = extent.map(|value| u32::try_from(value).unwrap_or(0));
        let grid = ChunkGrid::new(dims[0], dims[1], dims[2])
            .map_err(|error| BridgeError::View(format!("no chunk grid: {error}")))?;
        let mut materials = vec![AIR; grid.voxel_count()];
        for (origin, bytes) in &self.held {
            if let Some(slot) = slice_of(grid, &mut materials, *origin) {
                slot.copy_from_slice(bytes);
            }
        }
        let mut light = LightField::new(grid, self.params);
        light
            .bake_all(&materials)
            .map_err(|error| BridgeError::View(format!("the light bake refused: {error}")))?;
        let mut queue = DrainQueue::new(grid, self.budget);
        for chunk in 0..grid.chunk_count() {
            queue.push(chunk, self.drains);
        }
        let count = usize::try_from(grid.chunk_count()).unwrap_or(0);
        self.map = Some(MapState {
            grid,
            materials,
            light,
            queue,
            borders: ChunkBorders::new(),
            sizes: vec![0; count],
        });
        Ok(())
    }

    /// Writes the changed chunks into the map, relights what they changed and queues the
    /// chunks whose mesh moved.
    ///
    /// Each changed chunk is compared with the copy the map holds, voxel by voxel, and only
    /// the box of voxels that differ is handed to the mesher's bounded relight
    /// ([`LightField::relight`]), which re-bakes box by box, or in one whole bake when the
    /// boxes together are larger than the map, and names exactly the chunks whose mesh the
    /// edits changed. Before S2 a changed chunk re-baked and queued its whole padded box,
    /// and more than 16 changed chunks re-baked the whole map and queued every chunk of it,
    /// so a crater stream at speed queued the map (S2's plan, task `view`). A diff of two
    /// copies of what the gateway sent decides nothing: it is where to redraw.
    fn rebake_changed(&mut self) -> Result<usize, BridgeError> {
        let Some(map) = self.map.as_mut() else {
            return Ok(0);
        };
        if self.changed.is_empty() {
            return Ok(0);
        }
        let before = map.queue.len();
        let mut edits: Vec<([i32; 3], [i32; 3])> = Vec::with_capacity(self.changed.len());
        for origin in &self.changed {
            let bytes = self.held.get(origin).ok_or_else(|| {
                BridgeError::View(format!("the changed chunk at {origin:?} is not held"))
            })?;
            let corner = mesher_corner(*origin)
                .ok_or_else(|| BridgeError::View(format!("{origin:?} is not a chunk origin")))?;
            let slot = slice_of(map.grid, &mut map.materials, *origin).ok_or_else(|| {
                BridgeError::View(format!("the chunk at {origin:?} is outside the grid"))
            })?;
            if let Some(edit) = edited_box(slot, bytes, corner) {
                edits.push(edit);
            }
            slot.copy_from_slice(bytes);
        }
        let mut dirty: Vec<u32> = Vec::new();
        map.light
            .relight(&map.materials, &edits, &mut dirty)
            .map_err(|error| BridgeError::View(format!("the light bake refused: {error}")))?;
        for chunk in dirty {
            map.queue.push(chunk, self.drains);
        }
        Ok(map.queue.len().saturating_sub(before))
    }

    /// This drain's chunks, in item 54's order, under the rules table's K and B.
    ///
    /// `camera_chunk` is the camera's chunk in the MESHER's axes. Each call is one drain
    /// "frame" to the queue, so a chunk's age counts drains, which is what the ageing term
    /// was defined over.
    pub fn next_uploads(&mut self, camera_chunk: [i32; 3]) -> Vec<u32> {
        self.drains = self.drains.saturating_add(1);
        let drains = self.drains;
        let Some(map) = self.map.as_mut() else {
            return Vec::new();
        };
        let sizes = &map.sizes;
        map.queue.drain(drains, camera_chunk, |chunk| {
            usize::try_from(chunk)
                .ok()
                .and_then(|at| sizes.get(at).copied())
                .unwrap_or(0)
        })
    }

    /// Meshes one chunk of the map with its six real neighbours and its baked light.
    ///
    /// Returns the chunk's corner in the MESHER's axes, in voxels, which is where its
    /// instance is placed.
    ///
    /// # Errors
    ///
    /// [`BridgeError::View`] before a keyframe, [`BridgeError::Chunk`] for an index outside
    /// the grid, and whatever the mesher refuses.
    pub fn mesh(
        &mut self,
        chunk: u32,
        mesher: &mut Mesher,
        buffers: &mut MeshBuffers,
    ) -> Result<[i32; 3], BridgeError> {
        let Some(map) = self.map.as_mut() else {
            return Err(BridgeError::View("no keyframe has arrived yet".to_owned()));
        };
        let coords = map
            .grid
            .chunk_coords(chunk)
            .ok_or_else(|| BridgeError::View(format!("chunk {chunk} is outside the map")))?;
        map.borders.gather(
            map.grid,
            &map.materials,
            map.light.all(),
            self.params,
            chunk,
        );
        let view = chunk_view(&map.materials, &map.light, &map.borders, chunk)?;
        mesher.mesh_chunk_into(&view, buffers)?;
        if let Some(size) = usize::try_from(chunk)
            .ok()
            .and_then(|at| map.sizes.get_mut(at))
        {
            *size = gpu_surface_bytes(buffers);
        }
        let edge = CHUNK_EDGE_I32;
        Ok(coords.map(|value| value.saturating_mul(edge)))
    }
}

/// A sim-axes chunk origin as the chunk's lowest voxel in the MESHER's axes.
fn mesher_corner(origin: [i32; 3]) -> Option<[i32; 3]> {
    let coords = chunk_coords_of(origin)?;
    Some(coords.map(|value| value.saturating_mul(CHUNK_EDGE_I32)))
}

/// The inclusive box, in map voxels and the mesher's axes, of the voxels whose material
/// differs between two copies of one chunk whose lowest voxel is `corner`; `None` when the
/// copies agree. Both are in the mesher's order, `x + 32 z + 1024 y`, which the loops
/// below walk in.
fn edited_box(old: &[u8], new: &[u8], corner: [i32; 3]) -> Option<([i32; 3], [i32; 3])> {
    let mut pairs = old.iter().zip(new);
    let mut bounds: Option<([i32; 3], [i32; 3])> = None;
    for y in 0..CHUNK_EDGE_I32 {
        for z in 0..CHUNK_EDGE_I32 {
            for x in 0..CHUNK_EDGE_I32 {
                if pairs.next().is_some_and(|(was, now)| was != now) {
                    let at = [
                        corner[0].saturating_add(x),
                        corner[1].saturating_add(y),
                        corner[2].saturating_add(z),
                    ];
                    bounds = Some(match bounds {
                        None => (at, at),
                        Some((lo, hi)) => (
                            [lo[0].min(at[0]), lo[1].min(at[1]), lo[2].min(at[2])],
                            [hi[0].max(at[0]), hi[1].max(at[1]), hi[2].max(at[2])],
                        ),
                    });
                }
            }
        }
    }
    bounds
}

/// The mesher chunk index of a sim-axes origin, when it is inside `grid`.
fn index_of(grid: ChunkGrid, origin: [i32; 3]) -> Option<u32> {
    grid.chunk_index(chunk_coords_of(origin)?)
}

/// The slice of a chunk-major map that holds the chunk at `origin`.
fn slice_of(grid: ChunkGrid, materials: &mut [u8], origin: [i32; 3]) -> Option<&mut [u8]> {
    let index = usize::try_from(index_of(grid, origin)?).ok()?;
    let from = index.checked_mul(CHUNK_VOLUME)?;
    materials.get_mut(from..from.checked_add(CHUNK_VOLUME)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pharmakos_proto::gp::api::v1::{ViewChunk, ViewEntity};
    use pharmakos_proto::gp::v1::Voxel;

    /// The committed map's extent, `map.size_*` in rules/rules.v1.json.
    const EXTENT: [u32; 3] = [384, 384, 64];

    fn params() -> LightParams {
        LightParams::new(15, 1).expect("the committed pair")
    }

    fn budget() -> DrainBudget {
        DrainBudget {
            surfaces_per_frame: 4,
            bytes_per_frame: 512 * 1024,
            age_frames: 2,
        }
    }

    /// A chunk in SIM order: a floor `depth` voxels deep of `material`.
    fn floor(depth: usize, material: u8) -> Vec<u8> {
        let mut out = vec![0_u8; CHUNK_VOLUME];
        for (index, slot) in out.iter_mut().enumerate() {
            if index.checked_div(1024).unwrap_or(0) < depth {
                *slot = material;
            }
        }
        out
    }

    fn result(chunks: &[([i32; 3], Vec<u8>)], complete: bool) -> Json {
        let response = GetViewResponse {
            at_ms: 0,
            chunks: chunks
                .iter()
                .map(|(origin, sim)| ViewChunk {
                    origin: Some(Voxel {
                        x: origin[0],
                        y: origin[1],
                        z: origin[2],
                    }),
                    voxels_rle: chunk_rle::encode(sim),
                })
                .collect(),
            entities: vec![ViewEntity {
                id: "u_1".to_owned(),
                kind: view_entity::Kind::Unit.into(),
                subtype: "commander".to_owned(),
                owner: "seat.0".to_owned(),
                at: Some(Voxel { x: 3, y: 4, z: 5 }),
                hp: None,
                max_hp: None,
                heading: None,
            }],
            next_cursor: "c".to_owned(),
            complete,
        };
        let mut value = json::encode_json(&response).expect("encodes");
        if let Json::Object(entries) = &mut value {
            entries.push((
                STATUS_KEY.to_owned(),
                json::read(r#"{"phase":"lull","round":1}"#).expect("json"),
            ));
        }
        value
    }

    /// The wire's material table, read out of `ViewChunk`'s comment in `gateway.proto`:
    /// every value the table names, from "THE MATERIAL BYTE VALUES" to "ADDITIVE ONLY".
    fn wire_table() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../proto/gp/api/v1/gateway.proto");
        let text = std::fs::read_to_string(&path).expect("gateway.proto reads");
        let from = text
            .find("THE MATERIAL BYTE VALUES")
            .expect("ViewChunk's comment names its table");
        let to = from
            + text
                .get(from..)
                .and_then(|rest| rest.find("ADDITIVE ONLY"))
                .expect("the table ends where the comment says it is additive");
        let mut values: Vec<u8> = Vec::new();
        for line in text.get(from..to).expect("the table's text").lines() {
            let row = line.trim_start_matches(|c: char| c == '/' || c.is_whitespace());
            // A table line starts with a value; a prose line never does.
            if !row.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            let words: Vec<&str> = row.split_whitespace().collect();
            // A row is `<value>  <name ...>`, one or two to a line.
            for pair in words.windows(2) {
                if let [value, name] = pair
                    && let Ok(value) = value.parse::<u8>()
                    && name.starts_with(|c: char| c.is_ascii_alphabetic())
                {
                    values.push(value);
                }
            }
        }
        values.sort_unstable();
        values.dedup();
        values
    }

    /// No value the wire's table names maps to air but air itself, so no material the sim
    /// writes is drawn as a hole (S2's plan, task `view`; E14). The table runs without a
    /// gap from 0, and a value past it still draws nothing.
    #[test]
    fn every_wire_material_draws() {
        let table = wire_table();
        let expected: Vec<u8> = (0..=33).collect();
        assert_eq!(
            table, expected,
            "the wire's table as gateway.proto names it"
        );
        for value in &table {
            if *value == 0 {
                assert_eq!(palette_of(0), AIR, "air draws nothing");
            } else {
                assert_ne!(palette_of(*value), AIR, "wire material {value} draws");
                let row = usize::from(palette_of(*value));
                assert!(
                    row < pharmakos_mesher::PALETTE.len(),
                    "wire material {value} names palette row {row}, which the palette has"
                );
            }
        }
        let past = table.last().copied().expect("a table") + 1;
        for unknown in past..=255_u8 {
            assert_eq!(palette_of(unknown), AIR, "{unknown} is not in the table");
        }
    }

    /// A wall draws by the seat that built it, whole and cracked apart, and a cracked
    /// terrain voxel draws in its base's cracked row, never as its base or as air.
    #[test]
    fn a_wall_draws_by_its_builder_and_a_cracked_voxel_by_its_base() {
        use pharmakos_mesher::palette;
        let walls: Vec<u8> = (9..=16).map(palette_of).collect();
        let mut distinct = walls.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            8,
            "four builders, whole and cracked: {walls:?}"
        );
        assert_eq!(palette_of(19), palette::STONE_CRACKED_ONCE);
        assert_eq!(palette_of(20), palette::STONE_CRACKED_TWICE);
        assert_eq!(palette_of(17), palette::DIRT_CRACKED_ONCE);
        assert_eq!(
            palette_of(21),
            palette_of(25),
            "a seam keeps one row whatever its grade"
        );
        assert_eq!(palette_of(28), palette_of(32), "and a vent");
        assert_eq!(palette_of(33), palette::BEDROCK);
    }

    #[test]
    fn the_footer_is_split_off_and_read_as_a_status() {
        let value = result(&[([0, 0, 0], floor(4, 2))], true);
        let (body, footer) = split_footer(&value);
        assert!(body.get(STATUS_KEY).is_none());
        let status = read_status(&footer.expect("a footer")).expect("a Status");
        assert_eq!(status.round, 1);
    }

    #[test]
    fn a_page_decodes_with_the_unknown_value_drawn_as_air() {
        let mut sim = floor(4, 2);
        if let Some(slot) = sim.get_mut(0) {
            *slot = 200;
        }
        let page = decode_page(&result(&[([32, 0, 0], sim)], true)).expect("decodes");
        let chunk = page.chunks.first().expect("one chunk");
        assert_eq!(chunk.origin, [32, 0, 0]);
        assert_eq!(
            chunk.materials.first().copied(),
            Some(AIR),
            "200 draws nothing"
        );
        assert_eq!(chunk.materials.get(1).copied(), Some(palette_of(2)));
        assert_eq!(page.entities.len(), 1);
        assert_eq!(
            page.entities.first().map(|entity| entity.kind),
            Some(EntityKind::Unit)
        );
    }

    #[test]
    fn a_chunk_off_the_grid_or_short_is_refused_not_drawn() {
        let error = decode_page(&result(&[([5, 0, 0], floor(1, 1))], true))
            .expect_err("5 is not a chunk corner");
        assert!(matches!(error, BridgeError::View(_)), "{error}");
        let error = decode_page(&result(&[([0, 0, 0], vec![1_u8; 100])], true))
            .expect_err("a short run list");
        assert!(matches!(error, BridgeError::View(_)), "{error}");
    }

    #[test]
    fn a_pick_finds_the_empty_voxel_on_top_of_the_ground() {
        let mut model = ViewModel::new(params(), budget(), EXTENT);
        model
            .apply(decode_page(&result(&[([0, 0, 0], floor(4, 2))], true)).expect("keyframe"))
            .expect("applies");
        // World axes: x east, y up, z north. Straight down over sim (10, 20): the ground's
        // top voxel is z = 3, so the voxel a commander would stand in is z = 4.
        assert_eq!(
            model.pick([10.5, 40.0, 20.5], [0.0, -1.0, 0.0]),
            Some([10, 20, 4])
        );
        let slanted = model.pick([2.5, 30.0, 2.5], [0.3, -1.0, 0.2]);
        assert_eq!(slanted.map(|[_, _, up]| up), Some(4), "{slanted:?}");
        assert_eq!(
            model.pick([10.5, 40.0, 20.5], [0.0, 1.0, 0.0]),
            None,
            "looking up meets nothing"
        );
        assert_eq!(
            model.pick([10.5, 1.5, 20.5], [0.0, -1.0, 0.0]),
            None,
            "a ray that starts inside the ground picks nothing"
        );
        assert_eq!(
            model.pick([10.5, 40.0, 20.5], [0.0, 0.0, 0.0]),
            None,
            "no direction"
        );
    }

    #[test]
    fn the_ground_at_a_column_is_the_empty_voxel_on_top_of_it() {
        let mut model = ViewModel::new(params(), budget(), EXTENT);
        assert_eq!(model.ground_at(10, 20), None, "nothing held yet");
        model
            .apply(decode_page(&result(&[([0, 0, 0], floor(4, 2))], true)).expect("keyframe"))
            .expect("applies");
        assert_eq!(
            model.ground_at(10, 20),
            Some(4),
            "where the pick stands, too"
        );
        assert_eq!(model.ground_at(40, 20), None, "a column with no chunk held");
    }

    #[test]
    fn a_keyframe_builds_the_grid_and_queues_every_chunk() {
        let mut model = ViewModel::new(params(), budget(), EXTENT);
        let first = decode_page(&result(&[([0, 0, 0], floor(4, 2))], false)).expect("page 1");
        let applied = model.apply(first).expect("applies");
        assert!(!applied.complete);
        assert_eq!(
            model.grid(),
            None,
            "nothing is built before the view completes"
        );
        let second = decode_page(&result(&[([32, 32, 0], floor(4, 2))], true)).expect("page 2");
        let applied = model.apply(second).expect("applies");
        assert!(applied.grid_changed);
        let grid = model.grid().expect("a grid");
        assert_eq!(
            (grid.chunks_x(), grid.chunks_y(), grid.chunks_z()),
            (2, 1, 2),
            "sim north is the mesher's z"
        );
        assert_eq!(model.pending(), 4, "every chunk of the grid is queued");
        assert_eq!(model.entities().len(), 1);
    }

    #[test]
    fn a_delta_that_changes_nothing_queues_nothing_and_one_that_does_queues_its_fan_out() {
        let mut model = ViewModel::new(params(), budget(), EXTENT);
        let keyframe = [([0, 0, 0], floor(4, 2)), ([32, 0, 0], floor(4, 2))];
        model
            .apply(decode_page(&result(&keyframe, true)).expect("keyframe"))
            .expect("applies");
        while model.pending() > 0 {
            let _ = model.next_uploads([0, 0, 0]);
        }
        let same = decode_page(&result(&keyframe, true)).expect("the same view again");
        assert_eq!(model.apply(same).expect("applies").queued, 0);

        let edited = decode_page(&result(&[([32, 0, 0], floor(3, 2))], true)).expect("delta");
        let applied = model.apply(edited).expect("applies");
        assert!(!applied.grid_changed);
        assert!(
            applied.queued >= 1,
            "the edited chunk is queued: {applied:?}"
        );
    }

    /// A completing page of no chunks carrying exactly `entities`.
    fn with_entities(entities: Vec<ViewEntity>) -> Json {
        let response = GetViewResponse {
            at_ms: 0,
            chunks: Vec::new(),
            entities,
            next_cursor: "c".to_owned(),
            complete: true,
        };
        json::encode_json(&response).expect("encodes")
    }

    fn entity(id: &str, kind: view_entity::Kind, owner: &str) -> ViewEntity {
        ViewEntity {
            id: id.to_owned(),
            kind: kind.into(),
            subtype: String::new(),
            owner: owner.to_owned(),
            at: Some(Voxel { x: 1, y: 2, z: 3 }),
            hp: None,
            max_hp: None,
            heading: None,
        }
    }

    /// S2's entities come through as the gateway sent them: HP with its maximum, a heading,
    /// and shells, wrecks and ruins as their own kinds, a ruin owned by nobody.
    #[test]
    fn hit_points_headings_and_s2_kinds_are_carried_as_sent() {
        let mut raider = entity("u_4", view_entity::Kind::Unit, "seat.1");
        raider.hp = Some(30);
        raider.max_hp = Some(120);
        raider.heading = Some(16_384);
        let page = decode_page(&with_entities(vec![
            raider,
            entity("s_1", view_entity::Kind::Shell, "seat.0"),
            entity("w_1", view_entity::Kind::Wreck, ""),
            entity("r_1", view_entity::Kind::Ruin, ""),
            entity("v_1", view_entity::Kind::Beacon, "seat.1"),
        ]))
        .expect("decodes");
        let kinds: Vec<&str> = page.entities.iter().map(|one| one.kind.name()).collect();
        assert_eq!(kinds, ["unit", "shell", "wreck", "ruin", "beacon"]);
        let first = page.entities.first().expect("the raider");
        assert_eq!(
            first.hp.map(|health| (health.hp, health.max_hp.get())),
            Some((30, 120))
        );
        assert_eq!(first.heading, Some(16_384));
        let ruin = page.entities.get(3).expect("the ruin");
        assert_eq!(
            (ruin.owner.as_str(), ruin.hp, ruin.heading),
            ("", None, None)
        );
        assert_eq!(
            page.entities.get(4).map(|beacon| beacon.id.as_str()),
            Some("v_1"),
            "a Push's temporary handle is carried as an opaque id"
        );
    }

    /// An entity the client would have to guess about refuses its page.
    #[test]
    fn an_entity_with_no_position_or_half_its_hit_points_is_refused() {
        let mut nowhere = entity("u_1", view_entity::Kind::Unit, "seat.0");
        nowhere.at = None;
        let mut no_maximum = entity("u_2", view_entity::Kind::Unit, "seat.0");
        no_maximum.hp = Some(5);
        let mut no_count = entity("u_3", view_entity::Kind::Unit, "seat.0");
        no_count.max_hp = Some(5);
        let mut zero = entity("u_4", view_entity::Kind::Unit, "seat.0");
        zero.hp = Some(0);
        zero.max_hp = Some(0);
        let mut spun = entity("u_5", view_entity::Kind::Unit, "seat.0");
        spun.heading = Some(65_536);
        for refused in [nowhere, no_maximum, no_count, zero, spun] {
            let id = refused.id.clone();
            let error = decode_page(&with_entities(vec![refused])).expect_err(&id);
            assert!(matches!(error, BridgeError::View(_)), "{id}: {error}");
        }
    }

    /// One crater in a delta queues the chunks its relight reaches, not the map: before S2
    /// a changed chunk queued its whole padded box, and more than 16 queued every chunk.
    #[test]
    fn a_crater_queues_the_chunks_it_reaches_and_not_the_map() {
        let mut model = ViewModel::new(params(), budget(), EXTENT);
        let mut keyframe: Vec<([i32; 3], Vec<u8>)> = Vec::new();
        for x in (0..256).step_by(32) {
            for y in (0..256).step_by(32) {
                keyframe.push(([x, y, 0], floor(24, 2)));
            }
        }
        model
            .apply(decode_page(&result(&keyframe, true)).expect("keyframe"))
            .expect("applies");
        while model.pending() > 0 {
            let _ = model.next_uploads([0, 0, 0]);
        }
        // A crater three voxels deep in the middle of chunk (128, 128, 0).
        let mut cratered = floor(24, 2);
        for z in 21..24_usize {
            for y in 14..18_usize {
                for x in 14..18_usize {
                    if let Some(slot) = cratered.get_mut(x + 32 * y + 1024 * z) {
                        *slot = 0;
                    }
                }
            }
        }
        let applied = model
            .apply(decode_page(&result(&[([128, 128, 0], cratered)], true)).expect("delta"))
            .expect("applies");
        assert!(
            (1..=8).contains(&applied.queued),
            "a crater in the middle of one chunk queues it and at most its neighbours, not \
             the 64 chunks of the map: {applied:?}"
        );
    }

    #[test]
    fn a_chunk_outside_the_map_is_refused_before_anything_is_stored() {
        let mut model = ViewModel::new(params(), budget(), EXTENT);
        // 360 chunks east and north: a grid of 4.27e9 voxels if it were ever sized from it.
        let far = decode_page(&result(
            &[([0, 0, 0], floor(4, 2)), ([11_520, 11_520, 0], floor(4, 2))],
            true,
        ))
        .expect("the page itself decodes");
        let error = model
            .apply(far)
            .expect_err("outside the 384 x 384 x 64 map");
        assert!(matches!(error, BridgeError::View(_)), "{error}");
        assert_eq!(model.held(), 0, "the page is refused whole");
        assert_eq!(model.grid(), None);
        let edge = decode_page(&result(&[([352, 352, 32], floor(4, 2))], true)).expect("decodes");
        model
            .apply(edge)
            .expect("the last chunk inside the map is taken");
        let grid = model.grid().expect("a grid");
        assert_eq!(
            (grid.chunks_x(), grid.chunks_y(), grid.chunks_z()),
            (12, 2, 12)
        );
    }

    #[test]
    fn a_box_rebake_and_a_whole_bake_agree() {
        let keyframe = [([0, 0, 0], floor(6, 2)), ([32, 0, 0], floor(6, 2))];
        let mut boxed = ViewModel::new(params(), budget(), EXTENT);
        boxed
            .apply(decode_page(&result(&keyframe, true)).expect("keyframe"))
            .expect("applies");
        boxed
            .apply(decode_page(&result(&[([32, 0, 0], floor(2, 1))], true)).expect("delta"))
            .expect("applies");

        let mut whole = ViewModel::new(params(), budget(), EXTENT);
        whole
            .apply(
                decode_page(&result(
                    &[([0, 0, 0], floor(6, 2)), ([32, 0, 0], floor(2, 1))],
                    true,
                ))
                .expect("keyframe"),
            )
            .expect("applies");

        let light = |model: &ViewModel| {
            model
                .map
                .as_ref()
                .map(|map| map.light.all().to_vec())
                .unwrap_or_default()
        };
        assert_eq!(light(&boxed), light(&whole));
    }
}
