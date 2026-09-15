// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Generated Protobuf types for `gp.v1` and `gp.api.v1`, and the canonical
//! proto-JSON codec that reads and writes them.
//!
//! Role from spec section 15 (Architecture), "Schema": **Protobuf is the
//! single source** — package [`gp::v1`] is the game types (playbooks,
//! templates, the rules table, the reserved script seams) and [`gp::api::v1`]
//! is the Seat Gateway surface.
//!
//! Licensed `MIT OR Apache-2.0`, unlike the rest of the workspace, so the
//! schema and the codec stay reusable outside the GPL'd game.
//!
//! # What is in here
//!
//! | Module | What it is |
//! |---|---|
//! | [`gp`] | The generated prost types, checked in under `src/generated` |
//! | [`json`] | Canonical proto3 JSON, encode **and** decode |
//! | [`descriptor`] | The schema, read back out of the checked-in descriptor set |
//! | [`scope`] | The method/scope table, and the JSON-RPC wire spellings |
//! | [`fingerprint`] | The plan fingerprint's rule (the arithmetic is the sim's) |
//!
//! # Regenerating the checked-in tree
//!
//! `src/generated` is **committed**, so a plain `cargo build` needs neither
//! `buf` nor `protoc`, and a schema change arrives as a reviewable diff, which
//! is what a contract file wants. Two commands regenerate it, both run from
//! the workspace root:
//!
//! ```sh
//! # once; a dev tool, not a dependency. The version is pinned because the
//! # check below is a byte-for-byte diff of generated source, and
//! # .github/workflows/ci.yml installs this exact version.
//! cargo install protoc-gen-prost --version 0.5.0 --locked
//! buf build proto --exclude-source-info --as-file-descriptor-set \
//!     -o crates/proto/src/generated/descriptor.binpb
//! buf generate proto --template proto/buf.gen.yaml -o .
//! ```
//!
//! `tests/generated.rs` runs both into a scratch directory and diffs the
//! result against what is committed, so the tree cannot drift. It skips with a
//! named reason when `buf` or the plugin is not installed — but **fails**
//! instead when `PHARMAKOS_REQUIRE_TOOLS` is set, which CI sets globally, so a
//! runner missing a tool reddens the build rather than going green over an
//! unchecked tree.
//!
//! # The file format, in one paragraph
//!
//! A playbook on disk is a **JSONC** file: canonical proto JSON for `gp.v1`
//! plus comments, which the editor round-trips byte for byte. This crate owns
//! the canonical JSON half — field names as the `.proto` spells them, enum
//! values as their bare value names, durations as bare numbers because every
//! duration field is `int32` (decisions-log item 46), fields written in
//! field-number order, and an unknown field **rejected** rather than stripped.
//! The comment-and-formatting layer on top is `plan-core`'s (item 74).
//!
//! # Reserved seams (spec sections 15 and 18)
//!
//! Oneofs with only the `builtin` arm implemented — `operator {builtin|script}`,
//! `mandate {builtin|script}`, `program {builtin|script}` — plus
//! `author_kind SCRIPT` and the plan fingerprint. Alongside them, reserved
//! field numbers: the v1 vocabulary's absentees (`set_flag`, `clear_flag`,
//! `branch`, `repeat`, the flag predicates), player Dispatches, `team_id`,
//! capture, logistics and the radio predicates, each naming the version or
//! stage it returns in. Nothing on the script side executes in v1; the script
//! arm is v1.1 and the live conduit v1.2.
//!
//! `Playbook.kind` was the one held field number that this crate's walking-
//! skeleton pass discharges: item 47 reserved field 7, item 76 defines it.

/// The generated prost types.
///
/// Everything under here is written by `protoc-gen-prost` from the `.proto`
/// files and committed; do not edit it by hand. The `#[allow]` list is the
/// price of including generated code in a workspace whose lint set is tuned
/// for code somebody wrote. What is allowed, exactly:
///
/// * `missing_docs` and `unreachable_pub` — prost carries the `.proto`
///   comments across, but not onto the `mod`s and `oneof` wrappers it
///   synthesises;
/// * `clippy::pedantic` and `clippy::all` — the generated names and shapes are
///   protoc's, not ours, and renaming them would defeat the point of
///   generating them.
///
/// **No determinism lint is allowed here**, and the two blanket allows above
/// would otherwise take seven of them with them: `clippy::all` contains the
/// `style` group, and with it `disallowed_types`, `disallowed_methods` and
/// `disallowed_macros`; `clippy::pedantic` contains `float_cmp` and the three
/// `cast_*` lints. So each generated module re-denies that set on the line
/// below the allow. AGENTS.md section 4.9 is explicit that an `#[allow]` on a
/// determinism lint outside a walled crate is a workaround wearing a disguise,
/// and the re-deny is what keeps that true of a `map<…>` field somebody adds
/// later. Belt and braces: `tests/proto.rs`'s
/// `the_generated_source_names_no_determinism_hazard` asserts over the
/// generated source text that no float, `as` cast, hash container or clock is
/// named in it.
pub mod gp {
    /// `gp.v1` — playbooks, templates, the rules table, the script seams.
    pub mod v1 {
        #![allow(
            missing_docs,
            unreachable_pub,
            clippy::pedantic,
            clippy::all,
            reason = "generated by protoc-gen-prost; see the module comment on `gp`"
        )]
        #![deny(
            clippy::disallowed_types,
            clippy::disallowed_methods,
            clippy::disallowed_macros,
            clippy::float_arithmetic,
            clippy::float_cmp,
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            clippy::cast_sign_loss,
            reason = "the determinism bans, put back after `clippy::all`/`pedantic` above \
                      switched them off; AGENTS.md §4.9"
        )]
        include!("generated/gp/v1/gp.v1.rs");
    }

    /// `gp.api.v1` — the Seat Gateway surface.
    pub mod api {
        /// `gp.api.v1`.
        pub mod v1 {
            #![allow(
                missing_docs,
                unreachable_pub,
                clippy::pedantic,
                clippy::all,
                reason = "generated by protoc-gen-prost; see the module comment on `gp`"
            )]
            #![deny(
                clippy::disallowed_types,
                clippy::disallowed_methods,
                clippy::disallowed_macros,
                clippy::float_arithmetic,
                clippy::float_cmp,
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                clippy::cast_precision_loss,
                clippy::cast_sign_loss,
                reason = "the determinism bans, put back after `clippy::all`/`pedantic` \
                          above switched them off; AGENTS.md §4.9"
            )]
            include!("generated/gp/api/v1/gp.api.v1.rs");
        }
    }
}

pub mod descriptor;
pub mod fingerprint;
pub mod json;
pub mod scope;

mod wire;

/// The compiled `FileDescriptorSet` for both packages, written by
/// `buf build --as-file-descriptor-set` and committed beside the generated
/// types.
///
/// It is the crate's single source for anything that has to agree with the
/// `.proto` files at run time: the canonical JSON codec's field and enum
/// tables, the reserved-number table, and the method/scope annotation. Source
/// info (the comments) is excluded — nothing reads it, and it trebles the
/// file.
pub const DESCRIPTOR_SET: &[u8] = include_bytes!("generated/descriptor.binpb");

/// The schema version this build of the crate speaks.
///
/// Minor versions add optional fields; a major version comes with a converter
/// that is kept forever (spec section 10, "Versioning and reserved seams").
pub const SCHEMA_VERSION: (u32, u32) = (1, 0);

/// The `gp.v1` schema version as the message a playbook carries.
#[must_use]
pub fn schema_version() -> gp::v1::SchemaVersion {
    let (major, minor) = SCHEMA_VERSION;
    gp::v1::SchemaVersion { major, minor }
}
