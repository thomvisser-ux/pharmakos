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
//!   projected future (AGENTS.md section 3 rule 2).
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
pub mod error;
pub mod jsonc;

pub use canonical::{Canonical, canonicalise, canonicalise_text};
pub use error::Error;
pub use jsonc::Document;
