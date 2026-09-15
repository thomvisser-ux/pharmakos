// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integer maths and the seeded RNG — the determinism core's arithmetic half.
//!
//! Decisions log item 70 settled where this lives: **there is no `math`
//! crate.** The newtypes, the RNG streams, the canonical encoding and the state
//! hash stay as modules inside `pharmakos-sim`, so "the determinism code" is
//! one nameable contract path for AGENTS.md §5 and for `cargo xtask ci`.
//!
//! * [`fixed`] — `Fx` (Q16.16), `Sq` (Q32.32), `Angle` (u16 + a 4 096-entry
//!   sine table).
//! * [`quantity`] — `Hp`, `Money`, `Kw`, `Tick`, `Ms`.
//! * [`random`] — the counter-based split streams.
//!
//! Every `#[allow(clippy::as_conversions)]` in the crate lives under this
//! module, one per audited widening cast, each with a comment saying why it
//! cannot truncate. `tests/confinement.rs` fails the build if one appears
//! anywhere else.

pub mod fixed;
pub mod quantity;
pub mod random;
