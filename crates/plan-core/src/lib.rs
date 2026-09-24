// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plan core: everything done *to* a playbook that is not simulating it. Role
//! from spec section 15 (Architecture), gateway layer — it runs in process
//! with the Seat Gateway.
//!
//! # What lives here
//!
//! | Module | What it is |
//! |---|---|
//! | [`jsonc`] | The lossless JSONC reader and writer: comments and formatting survive byte for byte |
//! | [`canonical`] | The canonical form, **authoritative for the file format** |
//! | [`patch`] | RFC 6902 JSON Patch with inverse-patch undo |
//! | [`render`] | `render_plan`'s deterministic English prose |
//! | [`interface`] | Interface-time arithmetic at spec section 5's rates |
//! | [`projection`] | The `$` and `kW` projection the Quartermaster's rules imply |
//! | [`travel`] | The pass-through to the sim's estimator, and the two rendering rules |
//! | [`library`] | The local template folder the gateway reads and stores nothing from |
//! | [`context`] | What the seat's frozen snapshot says |
//! | [`verify`] | JSONC in, `pharmakos-verifier`'s report out |
//! | [`strings`] | This crate's section of the one English string table |
//!
//! # The one property everything else rests on
//!
//! A playbook on disk is a **JSONC** file: canonical proto JSON for `gp.v1`
//! plus comments (decisions-log item 74). Spec section 13 calls the round trip
//! "Exact":
//!
//! > The editor's model is the playbook JSON. Edits are JSON Patches with
//! > inverse-patch undo, and comments survive a load-and-save unchanged. The
//! > plan core's canonical form is authoritative, and golden tests byte-compare
//! > the output.
//!
//! Both halves of that live here and are pinned by
//! `tests/golden/plan-core/**`: loading and saving a file gives the bytes back,
//! and canonicalising one gives the same canonical bytes every time, with the
//! comments carried to the nodes they were written against.
//!
//! ```no_run
//! use pharmakos_plan_core::{canonical, jsonc::Document};
//!
//! let text = std::fs::read_to_string("examples/playbooks/expand_east.jsonc")?;
//!
//! // Save what was loaded: the same bytes, always.
//! assert_eq!(Document::parse(&text)?.to_text(), text);
//!
//! // Or save canonically, comments included.
//! let canonical = canonical::canonicalise_text(&text)?;
//! assert!(canonical.relocated.is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Rules this crate is held to
//!
//! * **No dry runs.** Estimates may use the pathfinder, interface-time
//!   arithmetic, Quartermaster projection and placement legality. They may
//!   never step or fork the simulation, run mandates, programs, combat or
//!   construction, model an opponent, or evaluate rule conditions over a
//!   projected future (AGENTS.md section 3 rule 2). `tests/confinement.rs`
//!   asserts it over this crate's own source text, because a lint can be
//!   switched off by an `#[allow]` somebody adds in a hurry and a test cannot.
//! * Consequently this crate **may never reach the `research` feature** of
//!   `pharmakos-sim`, which is what gates `fork`. It depends on the sim with
//!   `default-features = false`, for its snapshot, rules-table and integer
//!   newtypes and nothing else; `cargo xtask ci`'s research-guard walks the
//!   feature graph and fails the build otherwise.
//! * **The editor decides nothing.** Spec section 12: the editor "runs no
//!   validation or time maths of its own — it asks the gateway", and the
//!   gateway asks this crate. So every number the player reads is computed
//!   here, once, from `gp.v1.RulesTable` rows rather than constants
//!   (AGENTS.md section 12).
//! * Determinism: integer maths in the sim's own newtypes, ordered
//!   collections, no wall clock, no `HashMap`/`HashSet`, no float arithmetic.
//!   A prose golden that differed between Windows, Linux and macOS would be a
//!   determinism hole in the editor rather than a wording change.

pub mod canonical;
pub mod context;
pub mod error;
pub mod interface;
pub mod jsonc;
pub mod library;
pub mod patch;
pub mod projection;
pub mod render;
pub mod strings;
pub mod travel;
pub mod verify;

pub use canonical::{Canonical, canonicalise, canonicalise_text};
pub use context::PlanContext;
pub use error::Error;
pub use jsonc::Document;
pub use library::{instantiate, instantiate_template};
pub use patch::{Patch, patch_text};
pub use projection::{Projection, project};
pub use render::render_plan;
pub use travel::{Estimate, TravelEstimator};
pub use verify::verify_jsonc;
