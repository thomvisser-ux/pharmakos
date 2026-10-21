// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The view feed: derived, unhashed state behind `get_view`.
//!
//! Spec section 15 makes the Godot client a JSON-RPC client of this gateway,
//! and spec section 13 gives it a camera. Neither says how the terrain gets
//! there, and before T16a nothing did (decisions-log item 106 (1)): the sim
//! exposes a borrowable chunk view, and `client-gdext` may not depend on the
//! sim. So the terrain travels down the one wire that already exists, through
//! the one fog policy that already exists, and this module is what keeps that
//! affordable.
//!
//! # Nothing in here is hashed, and nothing in here is a tick
//!
//! Every field below is **derived** from the world and the fog policy. None of
//! it enters the state hash, none of it enters a snapshot, and a restore
//! rebuilds it from a pristine world (T17: [`ViewFeed::attach`] is what a
//! restore calls, every cursor then goes stale, and every entity handle is
//! re-minted).
//!
//! The stamp is the feed's own [`ViewFeed::seq`], bumped once per stepped
//! tick, once per attach, phase change and fog-policy change, and once more
//! inside [`ViewFeed::refresh`] whenever a viewer's own sight turns out to
//! have moved since it last looked -- so that the delivery rule cannot be
//! broken by a caller that forgot.
//!
//! It is deliberately **not** a sim tick: a Lull and a recap consume none, and
//! a seat's view changes in both -- a beacon it may see comes into a sphere, a
//! seat is eliminated and the map opens up. A tick-stamped feed would answer
//! "nothing changed" to every one of those.
//!
//! # The delta rule is about WIRE BYTES, and that is what makes it leak-proof
//!
//! For each viewer the feed keeps, **for modified chunks only**, the bytes
//! that viewer is entitled to -- the current byte where the viewer is unfogged
//! or its [`Vision`] reaches that voxel, and the **generated** byte everywhere
//! else -- and the stamp at which those bytes last changed. A chunk is in a
//! delta *if and only if its bytes for that viewer changed*.
//!
//! Three things follow, and they are the whole design:
//!
//! * an enemy edit one voxel outside a seat's spheres, inside a chunk the seat
//!   half sees, produces **no chunk at all** -- not an empty one, not a
//!   smaller one, nothing. A delta whose mere presence said "something
//!   happened over there" would be a fog leak by arithmetic, which is the same
//!   mistake [`crate::feed`]'s cursor exists to avoid;
//! * the full-map unlock is not a feature. A seat is eliminated or the match
//!   ends, [`crate::fog::FogPolicy`] says the seat is unfogged, every modified
//!   chunk's entitled bytes change at once and page out on the next call. Only
//!   the *modified* ones: the generated map was delivered at the keyframe;
//! * a chunk nobody has ever written is the generated chunk for everybody, so
//!   it is encoded once, at attach, and served from there.
//!
//! # The generated map is served whole (decisions-log item 107 (1))
//!
//! What is fogged is what *changes* the terrain and what *stands on* it, voxel
//! by voxel. The generated map itself is not: `main` already serves the match
//! seed to every seat as "not a secret", the map is a pure function of that
//! seed and the rules table, and the travel estimator already prices every leg
//! as known ground.
//!
//! PLACEHOLDER: real terrain fog in the view — owner, at S3, with the knowledge store.
//! With the knowledge store, real terrain
//! fog (never seen, last known, the x1.5 cost bound) arrives as **one** mask
//! that feeds both this view and `RouteAdapter::set_unknown_columns`, so the
//! camera and the editor's dashed legs cannot disagree. The wire is ready for
//! it: `ViewChunk`'s comment says a decoder draws nothing for a material value
//! it does not know.

use pharmakos_proto::chunk_rle;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::voxels::VoxelStore;

use crate::error::Error;
use crate::feed::ViewCursor;
use crate::fog::{Viewer, Vision};

/// How many bytes of encoded runs one page of the view carries.
///
/// The cut is on **bytes** rather than on a chunk count because a chunk's
/// encoded size is between three bytes and three times raw, and a count would
/// therefore bound nothing. The first chunk of a page always goes through, so
/// a chunk larger than the whole budget is delivered rather than being a hole
/// no page can ever fill.
///
/// PLACEHOLDER: a view page's 256 KiB — owner, at hardening, with the rate limits.
/// 256 KiB is a working number with two technical reasons behind
/// it and no measurement of its own. Godot's `WebSocketPeer` defaults to an
/// inbound buffer of about 64 KiB and the client has to raise it either way;
/// and one page is answered by the single thread that owns the surface, so a
/// larger page is a longer stretch in which no other connection is answered.
/// **OWNER**, at hardening, with `MAX_MESSAGE_BYTES`, `READ_TIMEOUT` and the
/// rate limits.
pub const VIEW_PAGE_BYTES: usize = 256 * 1024;

/// What decides, for one viewer, which voxels of a modified chunk it may see.
///
/// The feed takes it as a pair of plain numbers rather than reaching for a
/// world, because the day the sim answers "what changed about this seat's
/// sight" this is the value that stops being recomputed from scratch. Units
/// and structures see since S2 (the sim's one sensor interface, S2's plan
/// E10), so the key moves whenever one of the seat's sensors does; its cost
/// is the watch handler's marker (`Surface::sight_key`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sight {
    /// True when the fog policy has lifted for this viewer, so every voxel of
    /// every chunk is its own.
    pub unfogged: bool,
    /// A digest of whatever else decides what the viewer sees -- every one
    /// of its own sensors, as the sim's live view draws them. When this
    /// changes, every modified chunk is
    /// recomputed for the viewer rather than only the ones the world touched.
    pub key: u64,
}

/// One modified chunk's bytes as one viewer is entitled to them.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Entitled {
    chunk: u32,
    /// Run-length encoded, ready for the wire.
    bytes: Vec<u8>,
    /// The stamp at which **these** bytes last changed.
    changed_at: u64,
}

/// One viewer's derived state. Per viewer, never per token and never per
/// connection: two connections on one seat's token share this, so neither can
/// corrupt the other's position, and a dropped connection resumes with the
/// cursor it already had.
#[derive(Clone, PartialEq, Eq, Debug)]
struct ViewerState {
    viewer: Viewer,
    entitled: Vec<Entitled>,
    refreshed_at: u64,
    sight: Sight,
    /// Opaque handles, in the order this viewer first saw each thing.
    units: Vec<(u64, String)>,
    structures: Vec<(u64, String)>,
    /// The wrecks this viewer has been shown, by wreck id.
    wrecks: Vec<(u64, String)>,
    /// The shells in flight this viewer has been shown, by
    /// [`ViewFeed::shell_handle`]'s key.
    shells: Vec<(u64, String)>,
    /// The viewer's `e_NN` handles for other seats' beacons, by beacon id,
    /// in the order each was named.
    beacons: Vec<(u32, String)>,
    /// The view's `v_N` handles for other seats' beacons a seat holds no
    /// `e_NN` for yet ([`ViewFeed::beacon_name`]).
    unnamed: Vec<(u32, String)>,
}

impl ViewerState {
    const fn new(viewer: Viewer) -> ViewerState {
        ViewerState {
            viewer,
            entitled: Vec::new(),
            refreshed_at: 0,
            sight: Sight {
                unfogged: false,
                key: 0,
            },
            units: Vec::new(),
            structures: Vec::new(),
            wrecks: Vec::new(),
            shells: Vec::new(),
            beacons: Vec::new(),
            unnamed: Vec::new(),
        }
    }
}

/// One page of a view, before it becomes JSON.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ViewPage {
    /// The chunks this page carries, ascending: origin and encoded runs.
    pub chunks: Vec<(Voxel, Vec<u8>)>,
    /// False while more pages of this listing are waiting.
    pub complete: bool,
    /// Where the next call resumes.
    pub next: ViewCursor,
}

/// The derived view state for one hosted match.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ViewFeed {
    /// Zero until a match is attached: a cursor can then never be mistaken for
    /// one of a view that exists.
    view_id: u64,
    /// How many times this feed has been attached. Part of the view's id, so
    /// that a second attach over the same world mints a different one --
    /// which is what makes T17's restore stale every outstanding cursor.
    attaches: u64,
    /// Set on a resumed match's feed, before its one attach: a value no
    /// earlier process of this match minted a view with
    /// ([`ViewFeed::resumed_after`]). `None` for a new match, whose identity
    /// bytes are exactly what they were before T17.
    resumed_after: Option<u64>,
    seq: u64,
    /// The generated map, encoded once, one entry per chunk in chunk order.
    generated: Vec<Vec<u8>>,
    /// The stamp at which each chunk's **world** bytes last changed.
    changed_at: Vec<u64>,
    /// Every chunk that has ever been written, ascending and unique.
    modified: Vec<u32>,
    modified_flag: Vec<bool>,
    viewers: Vec<ViewerState>,
}

impl ViewFeed {
    /// A feed with no match behind it.
    #[must_use]
    pub const fn new() -> ViewFeed {
        ViewFeed {
            view_id: 0,
            attaches: 0,
            resumed_after: None,
            seq: 0,
            generated: Vec::new(),
            changed_at: Vec::new(),
            modified: Vec::new(),
            modified_flag: Vec::new(),
            viewers: Vec::new(),
        }
    }

    /// Mark this feed as a **resumed** match's, before its attach.
    ///
    /// The attach count alone is not enough across processes: a resumed
    /// process builds a fresh feed and attaches it once, over the same match
    /// id, seed and map as the dead process's first attach, so without this
    /// the two views would have the same id and a cursor or entity handle from
    /// the dead process would be read as one of this view's. `generation` is
    /// a value no earlier process of this match minted a view under: the host
    /// loop passes the audit log's last sequence number, which every process
    /// that answered a single call has moved past, because the host loop
    /// flushes a resumed process's `resumed` line before it serves anything.
    /// It is still not a clock and not entropy, so a replay of the same inputs
    /// mints the same id.
    pub fn resumed_after(&mut self, generation: u64) {
        self.resumed_after = Some(generation);
    }

    /// Take the generated map and open a view over it.
    ///
    /// Called by [`crate::surface::Surface::attach`], and by nothing else.
    /// Every cursor issued before it is
    /// [`crate::error::Code::StaleSnapshot`] afterwards, because the view id
    /// is minted here.
    ///
    /// # What the id is made of, and why all four
    ///
    /// The match's **id** and **seed** are what make a cursor match-tied:
    /// hand one to another match and it is refused rather than read as a
    /// place in a world nobody meant. The chunk **count** is the map's own
    /// shape, so a cursor cannot survive into a view of a different size even
    /// if the first two ever repeated. And the **attach count** is what T17
    /// needs: a restore re-attaches the *same* match id, the same seed and
    /// the same map, and every cursor a client is holding still has to go
    /// stale, because [`ViewFeed::seq`] starts again at one and a cursor
    /// carrying a larger `from_seq` would match nothing for ever.
    ///
    /// It is a digest rather than a counter for the first reason, not the
    /// last: a counter would make the first view of every match the same view.
    /// Nothing here reads a clock or any entropy, so a match replayed from the
    /// same inputs mints the same id -- which is what lets a golden hold a
    /// rendered cursor.
    ///
    /// The whole map is encoded once: 288 chunks at the skeleton's size, a few
    /// hundred kilobytes of runs, and every keyframe of the match is served
    /// from it without touching the world again.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the store's own chunk count names
    /// a chunk it holds no bytes for, or does not fit this host's `usize`: a
    /// keyframe built over either would draw a hole as if it were air.
    pub fn attach(
        &mut self,
        voxels: &VoxelStore,
        match_id: &str,
        match_seed: u64,
    ) -> Result<(), Error> {
        let count = voxels.chunk_count();
        let slots = slot(count)?;
        let mut generated: Vec<Vec<u8>> = Vec::with_capacity(slots);
        for chunk in 0..count {
            let raw = voxels.chunk_bytes(chunk).ok_or_else(|| {
                Error::internal(format!(
                    "the voxel store counts {count} chunks and holds no bytes for chunk {chunk}"
                ))
            })?;
            generated.push(chunk_rle::encode(raw.as_slice()));
        }
        self.attaches = self.attaches.saturating_add(1);
        let mut identity: Vec<u8> = Vec::with_capacity(match_id.len().saturating_add(32));
        identity.extend_from_slice(match_id.as_bytes());
        // A separator, so that ("m-1", 2) and ("m-12", ...) cannot run into
        // one another: a match id is variable width and the rest is not.
        identity.push(0);
        identity.extend_from_slice(&match_seed.to_le_bytes());
        identity.extend_from_slice(&u64::from(count).to_le_bytes());
        identity.extend_from_slice(&self.attaches.to_le_bytes());
        // A resumed process's views are never the dead process's: see
        // `resumed_after`. Appended only then, so a new match's identity
        // bytes -- and every golden that holds a rendered cursor -- are
        // unchanged.
        if let Some(generation) = self.resumed_after {
            identity.push(1);
            identity.extend_from_slice(&generation.to_le_bytes());
        }
        self.view_id = pharmakos_sim::digest(&identity) | 1;
        self.seq = 1;
        self.generated = generated;
        self.changed_at = vec![0; slots];
        self.modified = Vec::new();
        self.modified_flag = vec![false; slots];
        self.viewers = Vec::new();
        Ok(())
    }

    /// True when a match has been attached.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.view_id != 0
    }

    /// The view's match-tied id.
    #[must_use]
    pub const fn view_id(&self) -> u64 {
        self.view_id
    }

    /// The feed's own monotonic stamp.
    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// Move the stamp on, because something a viewer can see may have changed.
    ///
    /// Once per stepped tick, and once per attach, phase change and fog-policy
    /// change. Bumping too often costs a recomputation; bumping too seldom
    /// loses a change, so this is called from the places that cannot be missed
    /// rather than from the places that are interesting -- and
    /// [`ViewFeed::refresh`] bumps once more on its own when a viewer's sight
    /// has moved, which is the backstop for a caller that changed the policy
    /// and did not.
    pub const fn bump(&mut self) {
        if self.view_id != 0 {
            self.seq = self.seq.saturating_add(1);
        }
    }

    /// Record that these chunks' world bytes changed at the current stamp.
    ///
    /// `settled` is [`VoxelStore::settled`], which is the sim's own answer to
    /// "which chunks did this tick write" -- ascending, unique, and refreshed
    /// inside the tick that wrote them.
    pub fn stamp(&mut self, settled: &[u32]) {
        for chunk in settled {
            let Ok(index) = usize::try_from(*chunk) else {
                continue;
            };
            if let Some(slot) = self.changed_at.get_mut(index) {
                *slot = self.seq;
            }
            if matches!(self.modified_flag.get(index), Some(false)) {
                if let Some(flag) = self.modified_flag.get_mut(index) {
                    *flag = true;
                }
                self.modified.push(*chunk);
                // Ascending, because every listing this feed produces is, and
                // because the key IS the chunk index so the order is total.
                self.modified.sort_unstable();
            }
        }
    }

    /// How many chunks have ever been written.
    #[must_use]
    pub fn modified(&self) -> usize {
        self.modified.len()
    }

    /// Bring one viewer's entitled bytes up to date.
    ///
    /// **This is the only place a [`Vision`] exists in the view's life**, and
    /// that is deliberate: `Surface::step` takes none -- its signature is
    /// T15's and does not move -- so the tick can stamp *which* chunks changed
    /// and nothing else. What a particular viewer may see of that change is
    /// worked out here, inside the `get_view` call that asked.
    ///
    /// Two reasons to recompute a chunk, and the second is the one that makes
    /// the unlock free: the world wrote it since this viewer last looked, or
    /// the viewer's own sight changed -- a beacon gained or lost, the fog
    /// policy lifted.
    ///
    /// # A sight change stamps the feed itself
    ///
    /// A recomputed chunk is stamped with the feed's **current** stamp, and
    /// [`ViewFeed::page`] delivers a chunk only when that stamp is strictly
    /// later than the cursor's. A viewer already caught up therefore carries
    /// `from_seq == seq`, and a sight change that moved no stamp would
    /// recompute its chunks into `changed_at == from_seq` and deliver them
    /// **never**. Every caller does bump before it changes the policy, but an
    /// invariant that lives in three call sites is an invariant with a hole
    /// in it, so the bump is here as well: a sight that differs from the one
    /// this viewer was last refreshed at moves the stamp before anything is
    /// stamped with it. The cost of the extra bump is nothing -- a stamp
    /// nobody's bytes carry changes nobody's delta.
    ///
    /// # Errors
    ///
    /// As [`entitled_bytes`], and [`crate::error::Code::Internal`] for a
    /// modified chunk the attached map does not hold.
    pub fn refresh<V: Vision>(
        &mut self,
        viewer: Viewer,
        sight: Sight,
        voxels: &VoxelStore,
        vision: &V,
    ) -> Result<(), Error> {
        let index = slot_of(&mut self.viewers, viewer);
        // A viewer seeing this feed for the first time needs no bump: it has
        // no cursor yet, and its keyframe is stamped with whatever the feed
        // is on. A viewer whose sight *moved* does: see the note above.
        let moved = self
            .viewers
            .get(index)
            .is_some_and(|state| state.refreshed_at != 0 && state.sight != sight);
        if moved {
            self.bump();
        }
        let ViewFeed {
            seq,
            generated,
            changed_at,
            modified,
            viewers,
            ..
        } = self;
        let state = viewers.get_mut(index).ok_or_else(|| {
            Error::internal("the view feed lost the viewer slot it had just made")
        })?;
        let whole = state.sight != sight || state.refreshed_at == 0;
        let seat = match viewer {
            Viewer::Seat(seat) => Some(seat),
            Viewer::Spectator { .. } | Viewer::Admin => None,
        };

        for chunk in modified.iter() {
            let index = slot(*chunk)?;
            let written = changed_at
                .get(index)
                .copied()
                .is_some_and(|stamp| stamp > state.refreshed_at);
            if !whole && !written {
                continue;
            }
            let base = generated
                .get(index)
                .map(Vec::as_slice)
                .ok_or_else(|| unmapped(*chunk))?;
            let bytes = entitled_bytes(*chunk, voxels, base, seat, sight.unfogged, vision)?;
            if let Some(at) = state.entitled.iter().position(|held| held.chunk == *chunk) {
                if let Some(held) = state.entitled.get_mut(at)
                    && held.bytes != bytes
                {
                    held.bytes = bytes;
                    held.changed_at = *seq;
                }
            } else {
                // A chunk entering this viewer's list for the FIRST time has
                // not necessarily changed for it. A world edit the viewer
                // cannot see leaves the entitled bytes exactly the generated
                // ones -- and stamping those with the current stamp would put
                // the chunk in the viewer's next delta, whose mere presence
                // would say "something happened over there". So the stamp is
                // zero unless the bytes really differ from the map everybody
                // already has.
                let changed_at = if bytes == base { 0 } else { *seq };
                state.entitled.push(Entitled {
                    chunk: *chunk,
                    bytes,
                    changed_at,
                });
                // Ascending, because every listing this feed produces is.
                state.entitled.sort_unstable_by_key(|held| held.chunk);
            }
        }
        state.sight = sight;
        state.refreshed_at = *seq;
        Ok(())
    }

    /// One page of the view for a viewer that has been refreshed.
    ///
    /// `cursor` is `None` for a keyframe. The listing is every chunk of the
    /// map for a keyframe and, for a delta, exactly the modified chunks whose
    /// bytes **for this viewer** changed after the cursor's `from_seq`.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for a listed chunk the attached map
    /// does not hold, or whose origin the store cannot place.
    pub fn page(
        &self,
        viewer: Viewer,
        cursor: Option<ViewCursor>,
        voxels: &VoxelStore,
    ) -> Result<ViewPage, Error> {
        let (from_seq, to_seq, start) = match cursor {
            None => (0, self.seq, 0),
            Some(held) if held.index == 0 => (held.from_seq, self.seq, 0),
            Some(held) => (held.from_seq, held.to_seq, held.index),
        };
        let state = self.viewers.iter().find(|state| state.viewer == viewer);

        let listing: Vec<u32> = if from_seq == 0 {
            (start..voxels.chunk_count()).collect()
        } else {
            // A viewer this feed has never refreshed holds no entitled bytes,
            // so nothing has changed for it: an empty delta, which is exact.
            match state {
                Some(state) => state
                    .entitled
                    .iter()
                    .filter(|held| held.changed_at > from_seq && held.chunk >= start)
                    .map(|held| held.chunk)
                    .collect(),
                None => Vec::new(),
            }
        };

        let mut chunks: Vec<(Voxel, Vec<u8>)> = Vec::new();
        let mut spent: usize = 0;
        let mut next: Option<u32> = None;
        for chunk in listing {
            let bytes = self.bytes_for(state, chunk)?;
            // The first chunk of a page always goes through: a chunk whose
            // encoding is larger than the whole budget would otherwise be a
            // hole no page could ever fill.
            if !chunks.is_empty() && spent.saturating_add(bytes.len()) > VIEW_PAGE_BYTES {
                next = Some(chunk);
                break;
            }
            spent = spent.saturating_add(bytes.len());
            let [x, y, z] = voxels.chunk_origin(chunk).ok_or_else(|| unmapped(chunk))?;
            chunks.push((Voxel { x, y, z }, bytes));
        }

        Ok(match next {
            Some(index) => ViewPage {
                chunks,
                complete: false,
                next: ViewCursor {
                    view_id: self.view_id,
                    from_seq,
                    to_seq,
                    index,
                },
            },
            None => ViewPage {
                chunks,
                complete: true,
                // The stamp the listing STARTED at, not the one the feed is on
                // now: see [`ViewCursor`]'s own doc for why that is the
                // difference between a chunk being re-sent and a chunk being
                // lost.
                next: ViewCursor {
                    view_id: self.view_id,
                    from_seq: to_seq,
                    to_seq,
                    index: 0,
                },
            },
        })
    }

    /// The bytes one chunk goes on the wire as for this viewer: its entitled
    /// bytes when the chunk has ever been written, and the generated chunk
    /// otherwise.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for a chunk the attached map does not
    /// hold.
    fn bytes_for(&self, state: Option<&ViewerState>, chunk: u32) -> Result<Vec<u8>, Error> {
        if let Some(held) = state.and_then(|state| {
            state
                .entitled
                .iter()
                .find(|entitled| entitled.chunk == chunk)
        }) {
            return Ok(held.bytes.clone());
        }
        self.generated
            .get(slot(chunk)?)
            .cloned()
            .ok_or_else(|| unmapped(chunk))
    }

    /// This viewer's opaque handle for a unit, minted on first sighting.
    ///
    /// **A dense table row id is a count.** One visible enemy drone carrying
    /// the id `u_812` would tell a seat that the match has made eight hundred
    /// units, which is a number no answer of this gateway reports and no seat
    /// may derive. So a handle counts what *this viewer* has seen, which is by
    /// construction something it already knows.
    ///
    /// # Errors
    ///
    /// As [`ViewFeed::foreign_beacon_name`].
    pub fn unit_handle(&mut self, viewer: Viewer, id: u32) -> Result<String, Error> {
        ViewFeed::handle(&mut self.viewers, viewer, u64::from(id), Space::Unit)
    }

    /// This viewer's opaque handle for a wreck, `w_N`, minted on first sight
    /// by the wreck's id. As [`ViewFeed::unit_handle`].
    ///
    /// # Errors
    ///
    /// As [`ViewFeed::unit_handle`].
    pub fn wreck_handle(&mut self, viewer: Viewer, id: u32) -> Result<String, Error> {
        ViewFeed::handle(&mut self.viewers, viewer, u64::from(id), Space::Wreck)
    }

    /// This viewer's opaque handle for a shell in flight, `p_N`, minted on
    /// first sight. A shell has no id of its own in the sim's table (its rows
    /// compact as shells land), so it is keyed by what makes it one shell: its
    /// firer, with the id's generation, and the tick it lands on. A weapon
    /// fires at most once a tick, so no two shells share the key.
    ///
    /// # Errors
    ///
    /// As [`ViewFeed::unit_handle`].
    pub fn shell_handle(
        &mut self,
        viewer: Viewer,
        firer: u32,
        lands_at: u32,
    ) -> Result<String, Error> {
        let key = (u64::from(firer) << 32) | u64::from(lands_at);
        ViewFeed::handle(&mut self.viewers, viewer, key, Space::Shell)
    }

    /// This viewer's opaque handle for a structure. As [`ViewFeed::unit_handle`].
    ///
    /// # Errors
    ///
    /// As [`ViewFeed::unit_handle`].
    pub fn structure_handle(&mut self, viewer: Viewer, id: u32) -> Result<String, Error> {
        ViewFeed::handle(&mut self.viewers, viewer, u64::from(id), Space::Structure)
    }

    /// This viewer's name for the beacon at `row` of `world`'s table.
    ///
    /// A seat's **own** beacon is its `b_NN`, the beacon's per-seat ordinal,
    /// so a seat's core is `b_00` (decisions-log item 127 (13)). **Anybody
    /// else's** is an `e_NN`, counting from `e_01` (`docs/design/targeting.md`,
    /// "Names"). A seat's `e_NN` is minted only where a Lull opens or a save
    /// resumes, from the sim's sighting table (`Surface::name_sightings`); a
    /// beacon it holds none for yet is drawn under a `v_N` view handle. A
    /// spectator's and the lobby's, which no report reads, are minted the
    /// first time each is shown. Either way the number counts what this viewer
    /// already knows, so no name tells a seat how many beacons another has
    /// placed. A spectator and the lobby own no beacon, so every beacon is an
    /// `e_NN` to them.
    ///
    /// Re-minted on a restore, with the unit and structure handles (the
    /// module docs): an `e_NN` reaches no hashed state, and the sim answers
    /// one with `no_target`. From S2 it does reach the verifier's scope, as
    /// the key of a sighting (`Surface::verifier_scope`), so a seat's report
    /// spells an enemy beacon as the seat's own listing does.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for a row the beacon table does not
    /// hold, and as [`ViewFeed::foreign_beacon_name`].
    pub fn beacon_name(
        &mut self,
        viewer: Viewer,
        world: &pharmakos_sim::world::World,
        row: usize,
    ) -> Result<String, Error> {
        let beacons = world.beacons();
        let missing = || Error::internal(format!("the beacon table has no row {row}"));
        let owner = beacons.seats().get(row).copied().ok_or_else(missing)?;
        if let Viewer::Seat(seat) = viewer
            && owner == seat.raw()
        {
            let ordinal = beacons.ordinals().get(row).copied().ok_or_else(missing)?;
            return Ok(pharmakos_sim::tables::own_beacon_name(ordinal));
        }
        let id = beacons.ids().get(row).copied().ok_or_else(missing)?;
        match viewer {
            // A seat's `e_NN` is minted only where a Lull opens
            // (`Surface::name_sightings`), so a call made in a Push cannot
            // change the names a report reads. A beacon it holds no name for
            // yet is drawn under a view handle, `v_N`, which reaches nothing
            // but the camera.
            Viewer::Seat(_) => match self.foreign_beacon_handle(viewer, id) {
                Some(name) => Ok(name),
                None => self.beacon_view_handle(viewer, id),
            },
            Viewer::Spectator { .. } | Viewer::Admin => self.foreign_beacon_name(viewer, id),
        }
    }

    /// This viewer's view handle for another seat's beacon it holds no
    /// `e_NN` for yet: `v_N`, minted the first time the view draws it, as a
    /// unit's `u_N` is. It names nothing to any method but `get_view`, and is
    /// never a report's: the beacon takes its `e_NN` at the next Lull.
    ///
    /// # Errors
    ///
    /// As [`ViewFeed::foreign_beacon_name`].
    fn beacon_view_handle(&mut self, viewer: Viewer, id: u32) -> Result<String, Error> {
        let index = slot_of(&mut self.viewers, viewer);
        let state = self.viewers.get_mut(index).ok_or_else(|| {
            Error::internal("the view feed lost the viewer slot it had just made")
        })?;
        if let Some((_, handle)) = state.unnamed.iter().find(|(held, _)| *held == id) {
            return Ok(handle.clone());
        }
        let number = state.unnamed.len().checked_add(1).ok_or_else(|| {
            Error::internal("this viewer holds more view handles than usize counts")
        })?;
        let handle = format!("v_{number}");
        state.unnamed.push((id, handle.clone()));
        Ok(handle)
    }

    /// This viewer's `e_NN` for another seat's beacon by its id, minted the
    /// first time it is asked, as [`ViewFeed::beacon_name`] mints one.
    ///
    /// By id rather than by row, because a beacon a seat **remembers** may
    /// have died and its row been given to another since (S2's plan, E1): the
    /// id, generation and all, names the beacon that was seen, and the handle
    /// the seat knows it by stays that beacon's.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when this viewer already holds more
    /// handles than a `u32` counts.
    pub fn foreign_beacon_name(&mut self, viewer: Viewer, id: u32) -> Result<String, Error> {
        let index = slot_of(&mut self.viewers, viewer);
        let state = self.viewers.get_mut(index).ok_or_else(|| {
            Error::internal("the view feed lost the viewer slot it had just made")
        })?;
        if let Some((_, name)) = state.beacons.iter().find(|(held, _)| *held == id) {
            return Ok(name.clone());
        }
        let number = u32::try_from(state.beacons.len())
            .ok()
            .and_then(|held| held.checked_add(1))
            .ok_or_else(|| {
                Error::internal("this viewer holds more beacon handles than u32 counts")
            })?;
        let name = pharmakos_sim::tables::foreign_beacon_name(number);
        state.beacons.push((id, name.clone()));
        Ok(name)
    }

    /// The handle this viewer already holds for another seat's beacon `id`,
    /// or `None` when it holds none. Read-only: it mints nothing.
    #[must_use]
    pub fn foreign_beacon_handle(&self, viewer: Viewer, id: u32) -> Option<String> {
        self.viewers
            .iter()
            .find(|state| state.viewer == viewer)?
            .beacons
            .iter()
            .find(|(held, _)| *held == id)
            .map(|(_, name)| name.clone())
    }

    /// The id of the beacon this viewer was shown under the handle `name`, or
    /// `None` when it was shown none by that handle. Read-only: it mints
    /// nothing.
    #[must_use]
    pub fn foreign_beacon_id(&self, viewer: Viewer, name: &str) -> Option<u32> {
        self.viewers
            .iter()
            .find(|state| state.viewer == viewer)?
            .beacons
            .iter()
            .find(|(_, held)| held == name)
            .map(|(id, _)| *id)
    }

    /// The row of `world`'s beacon table a name names **for this viewer**, or
    /// `None`.
    ///
    /// A `b_NN` names the viewer's own beacon with that ordinal, and only a
    /// seat owns beacons. An `e_NN` names the beacon this viewer was shown
    /// under that handle, and nothing else: a handle never shown to this
    /// viewer names nothing, so a guess answers exactly as an absent beacon
    /// does.
    #[must_use]
    pub fn beacon_row(
        &self,
        viewer: Viewer,
        world: &pharmakos_sim::world::World,
        name: &str,
    ) -> Option<usize> {
        let beacons = world.beacons();
        match pharmakos_sim::tables::parse_beacon_name(name)? {
            pharmakos_sim::tables::BeaconName::Own(ordinal) => match viewer {
                Viewer::Seat(seat) => beacons.row_of_ordinal(seat, ordinal),
                Viewer::Spectator { .. } | Viewer::Admin => None,
            },
            pharmakos_sim::tables::BeaconName::Foreign(number) => {
                let wanted = pharmakos_sim::tables::foreign_beacon_name(number);
                let state = self.viewers.iter().find(|state| state.viewer == viewer)?;
                let (id, _) = state.beacons.iter().find(|(_, held)| *held == wanted)?;
                // Through the id's own row, checked by generation (S2's plan,
                // E1): a beacon since freed, or its row reused, names nothing.
                beacons
                    .row_of(pharmakos_sim::tables::BeaconId::new(*id))
                    .ok()
                    .map(pharmakos_sim::ids::Row::index)
            }
        }
    }

    fn handle(
        viewers: &mut Vec<ViewerState>,
        viewer: Viewer,
        id: u64,
        space: Space,
    ) -> Result<String, Error> {
        let index = slot_of(viewers, viewer);
        let state = viewers.get_mut(index).ok_or_else(|| {
            Error::internal("the view feed lost the viewer slot it had just made")
        })?;
        let (prefix, table) = match space {
            Space::Unit => ("u_", &mut state.units),
            Space::Structure => ("s_", &mut state.structures),
            Space::Wreck => ("w_", &mut state.wrecks),
            Space::Shell => ("p_", &mut state.shells),
        };
        if let Some((_, handle)) = table.iter().find(|(held, _)| *held == id) {
            return Ok(handle.clone());
        }
        let number = table.len().checked_add(1).ok_or_else(|| {
            Error::internal("this viewer holds more entity handles than usize counts")
        })?;
        let handle = format!("{prefix}{number}");
        table.push((id, handle.clone()));
        Ok(handle)
    }
}

/// Which of a viewer's handle spaces a handle is minted in: one prefix each,
/// so no two kinds of thing share a name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Space {
    /// A unit, `u_N`.
    Unit,
    /// A structure, a ruin included, `s_N`: a structure keeps its handle when
    /// it turns into a ruin.
    Structure,
    /// A wreck, `w_N`.
    Wreck,
    /// A shell in flight, `p_N`.
    Shell,
}

/// This viewer's slot, made on first sight of it.
///
/// A `Vec` walked in order rather than a map: a handful of viewers, and no
/// hash container anywhere near this crate's state (AGENTS.md section 4.4).
fn slot_of(viewers: &mut Vec<ViewerState>, viewer: Viewer) -> usize {
    if let Some(found) = viewers.iter().position(|state| state.viewer == viewer) {
        return found;
    }
    viewers.push(ViewerState::new(viewer));
    viewers.len().saturating_sub(1)
}

/// One chunk's bytes as `seat` is entitled to them, run-length encoded.
///
/// Built by **inclusion**: the answer starts from the generated map -- which
/// every viewer is entitled to whole (decisions-log item 107 (1)) -- and a
/// voxel is replaced by the world's current material only where this viewer
/// may see it. Nothing is ever redacted out of a current byte, because a
/// redaction has to know what it is hiding and a mistake in one is a leak.
///
/// A voxel whose current material equals the generated one is copied without
/// asking the vision at all: identical bytes disclose nothing, and skipping
/// the question is what keeps a chunk with one crater in it cheap.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the store holds no bytes or no origin
/// for a chunk the feed lists as modified, or when the generated chunk the
/// feed encoded at attach does not decode to a whole chunk. Each was once a
/// silent fallback -- the generated map, or a chunk of zeros, which is air
/// (the carry of decisions-log item 145 (5)) -- and each would have drawn a
/// hole, or hidden an edit, as if it were the map.
fn entitled_bytes<V: Vision>(
    chunk: u32,
    voxels: &VoxelStore,
    generated: &[u8],
    seat: Option<SeatId>,
    unfogged: bool,
    vision: &V,
) -> Result<Vec<u8>, Error> {
    let world = voxels.chunk_bytes(chunk).ok_or_else(|| unmapped(chunk))?;
    if unfogged {
        return Ok(chunk_rle::encode(world.as_slice()));
    }
    let Some(seat) = seat else {
        // No seat, no sight, and the policy has not lifted: the generated map
        // and nothing of what happened to it.
        return Ok(generated.to_vec());
    };
    let base = chunk_rle::decode(generated).map_err(|error| {
        Error::internal(format!(
            "chunk {chunk}'s generated bytes, encoded at attach, do not decode: {error:?}"
        ))
    })?;
    let [ox, oy, oz] = voxels.chunk_origin(chunk).ok_or_else(|| unmapped(chunk))?;

    let mut out: Vec<u8> = Vec::with_capacity(chunk_rle::CHUNK_VOXELS);
    // The store's chunk and the decoded generated chunk are both
    // `CHUNK_VOXELS` long by construction (the store's array type, and the
    // codec's refusal of a short run list), so the zip walks every voxel.
    for (index, (current, pristine)) in world.iter().zip(base.iter()).enumerate() {
        let (current, pristine) = (*current, *pristine);
        if current == pristine {
            out.push(current);
            continue;
        }
        // The voxel order inside a chunk is x + 32y + 1024z (item 92), so the
        // address splits by a shift and a mask and never by a division. A
        // chunk's index is under 32 768, and a chunk's origin plus 31 is a
        // voxel of the map, so neither step can overflow.
        let offset = i32::try_from(index)
            .map_err(|_| Error::internal(format!("chunk {chunk} has a voxel past i32")))?;
        let at = Voxel {
            x: ox.saturating_add(offset & 31),
            y: oy.saturating_add((offset >> 5_i32) & 31),
            z: oz.saturating_add(offset >> 10_i32),
        };
        out.push(if vision.sees(seat, &at) {
            current
        } else {
            pristine
        });
    }
    Ok(chunk_rle::encode(&out))
}

/// A chunk index as a slot of the feed's per-chunk columns.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for an index past this host's `usize`,
/// which no map this project makes has.
fn slot(chunk: u32) -> Result<usize, Error> {
    usize::try_from(chunk)
        .map_err(|_| Error::internal(format!("chunk {chunk} does not fit this host's usize")))
}

/// The refusal for a chunk the feed or the store does not hold.
fn unmapped(chunk: u32) -> Error {
    Error::internal(format!(
        "chunk {chunk} is listed by the view feed and not held by the map it was attached to"
    ))
}

#[cfg(test)]
mod tests {
    use super::{Sight, VIEW_PAGE_BYTES, ViewFeed};
    use crate::fog::{Blind, KnownVoxels, Viewer};
    use pharmakos_proto::chunk_rle;
    use pharmakos_proto::gp::v1::Voxel;
    use pharmakos_sim::tables::SeatId;
    use pharmakos_sim::voxels::{Material, VoxelStore};

    fn store() -> VoxelStore {
        let mut store = VoxelStore::new([64, 32, 32]).expect("a two-chunk store");
        // Something to see: a solid floor across both chunks.
        for y in 0..32_i32 {
            for x in 0..64_i32 {
                store.set([x, y, 0], Material::STONE);
            }
        }
        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.settle(&mut digests);
        store
    }

    fn open() -> (ViewFeed, VoxelStore) {
        let store = store();
        let mut feed = ViewFeed::new();
        feed.attach(&store, "m-0001", 7).expect("attached");
        (feed, store)
    }

    fn sight(unfogged: bool) -> Sight {
        Sight { unfogged, key: 0 }
    }

    #[test]
    fn a_keyframe_carries_every_chunk_of_the_map() {
        let (mut feed, store) = open();
        let viewer = Viewer::Seat(SeatId::new(0));
        feed.refresh(viewer, sight(false), &store, &Blind)
            .expect("refreshed");
        let page = feed.page(viewer, None, &store).expect("a page");
        assert!(page.complete);
        assert_eq!(page.chunks.len(), 2, "a 64x32x32 map is two chunks");
        let origins: Vec<(i32, i32, i32)> = page
            .chunks
            .iter()
            .map(|(at, _)| (at.x, at.y, at.z))
            .collect();
        assert_eq!(origins, vec![(0, 0, 0), (32, 0, 0)]);
        for (_, bytes) in &page.chunks {
            assert_eq!(
                chunk_rle::decode(bytes).expect("a chunk").len(),
                chunk_rle::CHUNK_VOXELS
            );
        }
        assert!(feed.is_open());
        assert_eq!(feed.modified(), 0, "the generator's writes are pristine");
    }

    /// The id is the match's, and a second attach over the same world is a
    /// different view: T17's restore is what calls that, and every cursor a
    /// client is still holding has to go stale rather than page into a feed
    /// whose stamp has started again at one.
    #[test]
    fn a_re_attach_mints_a_different_view_than_the_attach_before_it() {
        let store = store();
        let mut feed = ViewFeed::new();
        feed.attach(&store, "m-0001", 7).expect("attached");
        let first = feed.view_id();
        feed.attach(&store, "m-0001", 7).expect("attached");
        let second = feed.view_id();
        assert_ne!(
            first, second,
            "a restore re-attaches the same match over the same map, and the cursors from \
             before it are still STALE_SNAPSHOT"
        );
        assert_eq!(feed.seq(), 1, "and the new view's stamp starts again");

        // And the same match attached from scratch elsewhere is the same
        // view, because nothing in the id is read from a clock: a replay of
        // the same inputs renders the same cursor.
        let mut again = ViewFeed::new();
        again.attach(&store, "m-0001", 7).expect("attached");
        assert_eq!(again.view_id(), first);

        // Another match is another view, on the id alone and on the seed
        // alone.
        let mut other = ViewFeed::new();
        other.attach(&store, "m-0002", 7).expect("attached");
        assert_ne!(other.view_id(), first);
        let mut seeded = ViewFeed::new();
        seeded.attach(&store, "m-0001", 8).expect("attached");
        assert_ne!(seeded.view_id(), first);
    }

    #[test]
    fn a_resumed_process_never_mints_the_dead_processs_view() {
        let store = store();
        // The dead process: a new match, attached once.
        let mut dead = ViewFeed::new();
        dead.attach(&store, "m-0001", 7).expect("attached");
        // The resumed process: a fresh feed, attached once over the same
        // match, seed and map -- the case the attach count cannot tell apart.
        let mut resumed = ViewFeed::new();
        resumed.resumed_after(3);
        resumed.attach(&store, "m-0001", 7).expect("attached");
        assert_ne!(
            resumed.view_id(),
            dead.view_id(),
            "a cursor from the dead process is stale in the resumed one"
        );
        // A second resume, after the first process wrote more of the log, is
        // a third view; the same resume replayed is the same view.
        let mut later = ViewFeed::new();
        later.resumed_after(9);
        later.attach(&store, "m-0001", 7).expect("attached");
        assert_ne!(later.view_id(), resumed.view_id());
        let mut replayed = ViewFeed::new();
        replayed.resumed_after(3);
        replayed.attach(&store, "m-0001", 7).expect("attached");
        assert_eq!(replayed.view_id(), resumed.view_id());
        // Even a resume at generation zero is not the new match's view.
        let mut zero = ViewFeed::new();
        zero.resumed_after(0);
        zero.attach(&store, "m-0001", 7).expect("attached");
        assert_ne!(zero.view_id(), dead.view_id());
    }

    #[test]
    fn an_edit_a_seat_cannot_see_never_reaches_it_as_a_delta() {
        let (mut feed, mut store) = open();
        let seat = SeatId::new(0);
        let viewer = Viewer::Seat(seat);
        feed.refresh(viewer, sight(false), &store, &Blind)
            .expect("refreshed");
        let caught_up = feed.page(viewer, None, &store).expect("a page").next;

        // A tick writes one voxel of chunk 0 that this blind seat cannot see.
        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.set([4, 4, 0], Material::AIR);
        store.settle(&mut digests);
        feed.bump();
        feed.stamp(store.settled());
        assert_eq!(feed.modified(), 1);

        feed.refresh(viewer, sight(false), &store, &Blind)
            .expect("refreshed");
        let page = feed.page(viewer, Some(caught_up), &store).expect("a page");
        assert!(page.complete);
        assert!(
            page.chunks.is_empty(),
            "a delta whose presence said `something happened over there` is a leak"
        );
    }

    #[test]
    fn an_edit_a_seat_can_see_arrives_with_its_current_bytes() {
        let (mut feed, mut store) = open();
        let seat = SeatId::new(0);
        let viewer = Viewer::Seat(seat);
        let place = Voxel { x: 4, y: 4, z: 0 };
        let mut vision = KnownVoxels::new();
        vision.see(seat, &place);

        feed.refresh(viewer, sight(false), &store, &vision)
            .expect("refreshed");
        let caught_up = feed.page(viewer, None, &store).expect("a page").next;

        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.set([4, 4, 0], Material::AIR);
        store.settle(&mut digests);
        feed.bump();
        feed.stamp(store.settled());

        feed.refresh(viewer, sight(false), &store, &vision)
            .expect("refreshed");
        let page = feed.page(viewer, Some(caught_up), &store).expect("a page");
        assert_eq!(page.chunks.len(), 1);
        let (_, bytes) = page.chunks.first().expect("the chunk");
        let voxels = chunk_rle::decode(bytes).expect("a chunk");
        let index = 4_usize
            .saturating_add(4_usize.saturating_mul(32))
            .saturating_add(0);
        assert_eq!(
            voxels.get(index).copied(),
            Some(Material::AIR.raw()),
            "the seat saw it happen"
        );
    }

    #[test]
    fn the_full_map_unlock_is_the_viewer_becoming_unfogged() {
        let (mut feed, mut store) = open();
        let viewer = Viewer::Seat(SeatId::new(0));
        feed.refresh(viewer, sight(false), &store, &Blind)
            .expect("refreshed");
        let caught_up = feed.page(viewer, None, &store).expect("a page").next;

        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.set([40, 4, 0], Material::AIR);
        store.settle(&mut digests);
        feed.bump();
        feed.stamp(store.settled());
        feed.refresh(viewer, sight(false), &store, &Blind)
            .expect("refreshed");
        assert!(
            feed.page(viewer, Some(caught_up), &store)
                .expect("a page")
                .chunks
                .is_empty(),
            "fogged: nothing"
        );

        // The policy lifts. Nothing about the world changed.
        feed.bump();
        feed.refresh(viewer, sight(true), &store, &Blind)
            .expect("refreshed");
        let page = feed.page(viewer, Some(caught_up), &store).expect("a page");
        assert_eq!(page.chunks.len(), 1, "only the modified chunk pages out");
    }

    /// The same unlock, with the caller's bump left out: the feed stamps
    /// itself when a viewer's sight moves, so the delivery rule does not
    /// depend on three call sites remembering.
    #[test]
    fn a_sight_change_moves_the_stamp_even_when_the_caller_did_not() {
        let (mut feed, mut store) = open();
        let viewer = Viewer::Seat(SeatId::new(0));
        feed.refresh(viewer, sight(false), &store, &Blind)
            .expect("refreshed");
        let caught_up = feed.page(viewer, None, &store).expect("a page").next;

        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.set([40, 4, 0], Material::AIR);
        store.settle(&mut digests);
        feed.bump();
        feed.stamp(store.settled());
        feed.refresh(viewer, sight(false), &store, &Blind)
            .expect("refreshed");
        let caught_up = feed
            .page(viewer, Some(caught_up), &store)
            .expect("a page")
            .next;
        assert_eq!(caught_up.from_seq, feed.seq(), "the viewer is caught up");

        // No bump here, which is the whole point of the test.
        feed.refresh(viewer, sight(true), &store, &Blind)
            .expect("refreshed");
        let page = feed.page(viewer, Some(caught_up), &store).expect("a page");
        assert_eq!(
            page.chunks.len(),
            1,
            "a caught-up cursor and a stamp that had not moved would have delivered nothing"
        );
    }

    #[test]
    fn a_handle_counts_what_this_viewer_has_seen_and_nothing_else() {
        let (mut feed, _store) = open();
        let viewer = Viewer::Seat(SeatId::new(0));
        assert_eq!(feed.unit_handle(viewer, 812).expect("a handle"), "u_1");
        assert_eq!(feed.unit_handle(viewer, 9).expect("a handle"), "u_2");
        assert_eq!(
            feed.unit_handle(viewer, 812).expect("a handle"),
            "u_1",
            "stable"
        );
        assert_eq!(feed.structure_handle(viewer, 400).expect("a handle"), "s_1");
        let other = Viewer::Seat(SeatId::new(1));
        assert_eq!(
            feed.unit_handle(other, 812).expect("a handle"),
            "u_1",
            "another viewer's numbering is its own"
        );
    }

    /// A keyframe wider than one page is paged, and the pages between them
    /// carry the stamp the listing started at.
    ///
    /// The golden seed's own map does **not** exercise this: 288 chunks of
    /// generated heightmap encode to about 200 KiB of runs, which is one page.
    /// So the case is made here, with the worst chunk the encoding has -- no
    /// two neighbours alike, three bytes a voxel, 96 KiB a chunk -- which is
    /// also the shape the page budget is counted in bytes for.
    #[test]
    fn a_keyframe_wider_than_a_page_is_paged_in_ascending_chunk_order() {
        let mut store = VoxelStore::new([128, 32, 32]).expect("a four-chunk store");
        for z in 0..32_i32 {
            for y in 0..32_i32 {
                for x in 0..128_i32 {
                    let alternate = x.saturating_add(y).saturating_add(z) & 1;
                    if alternate == 1 {
                        store.set([x, y, z], Material::STONE);
                    }
                }
            }
        }
        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.settle(&mut digests);

        let mut feed = ViewFeed::new();
        feed.attach(&store, "m-0001", 11).expect("attached");
        let viewer = Viewer::Seat(SeatId::new(0));
        feed.refresh(viewer, sight(false), &store, &Blind)
            .expect("refreshed");

        let mut pages = 0_u32;
        let mut delivered: Vec<i32> = Vec::new();
        let mut cursor = None;
        loop {
            let page = feed.page(viewer, cursor, &store).expect("a page");
            pages = pages.saturating_add(1);
            let spent: usize = page.chunks.iter().map(|(_, bytes)| bytes.len()).sum();
            assert!(
                spent <= VIEW_PAGE_BYTES || page.chunks.len() == 1,
                "a page is cut at {VIEW_PAGE_BYTES} bytes of runs, and only a single chunk \
                 larger than the whole budget goes over"
            );
            for (origin, _) in &page.chunks {
                delivered.push(origin.x);
            }
            cursor = Some(page.next);
            if page.complete {
                break;
            }
            assert!(pages < 16, "a keyframe that never completes");
        }
        assert!(pages > 1, "four worst-case chunks do not fit one page");
        assert_eq!(
            delivered,
            vec![0, 32, 64, 96],
            "every chunk, once, in ascending chunk order"
        );
        let last = cursor.expect("a completing cursor");
        assert_eq!(last.from_seq, last.to_seq, "and the client is caught up");
        assert!(!last.is_keyframe());
    }

    #[test]
    fn a_keyframe_pages_and_the_completing_cursor_is_the_stamp_it_started_at() {
        let (mut feed, store) = open();
        let viewer = Viewer::Seat(SeatId::new(0));
        feed.refresh(viewer, sight(false), &store, &Blind)
            .expect("refreshed");
        let started = feed.seq();
        let page = feed.page(viewer, None, &store).expect("a page");
        assert!(page.complete, "two small chunks fit one page");
        assert_eq!(page.next.from_seq, started);
        assert_eq!(page.next.to_seq, started);
        assert_eq!(page.next.index, 0);
        assert!(!page.next.is_keyframe());
    }
}
