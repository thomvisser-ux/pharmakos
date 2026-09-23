// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Why the bridge refused to marshal something.
//!
//! Every variant is a **marshalling** failure: a length that is not what the other side
//! declared, a value that does not fit the type it is being put into, a JSON text the
//! schema rejects. None of them is a game rule, because the bridge decides nothing
//! (AGENTS.md section 3 rule 4) — a rule the bridge could break is a rule that is in the
//! wrong crate.

use std::fmt;

use pharmakos_mesher::{ChunkInputError, MeshError};

/// Something the bridge could not marshal.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BridgeError {
    /// A byte array handed across the seam was not the length its type requires.
    Length {
        /// What was being marshalled — `"chunk materials"`, `"the -x border"`.
        what: &'static str,
        /// How many bytes the type requires.
        expected: usize,
        /// How many bytes arrived.
        got: usize,
    },
    /// A rules-table row the mesher needs is absent from the table that was handed over.
    MissingRules {
        /// The row, as `rules/rules.v1.json` spells it.
        row: &'static str,
    },
    /// A rules-table value does not fit the parameter it fills.
    RulesOutOfRange {
        /// The field, as `gp.v1.RulesTable` spells it.
        field: &'static str,
        /// The value the table carried.
        value: u32,
        /// Why it does not fit.
        because: &'static str,
    },
    /// The mesher refused the chunk it was handed.
    Chunk(ChunkInputError),
    /// The mesher refused to mesh.
    Mesh(MeshError),
    /// A JSON text was not valid against the schema, or named a type the schema does not
    /// declare. `pointer` is the RFC 6901 pointer the codec reported.
    Schema {
        /// Where in the document the problem is; `""` is the root.
        pointer: String,
        /// The codec's own words, passed through rather than reworded.
        message: String,
    },
    /// A `gp.api.v1` method name that the schema's `Method` enum does not declare.
    UnknownMethod {
        /// The name as it arrived.
        name: String,
    },
    /// A `get_view` page this client could not store: a chunk with no origin, an origin
    /// off the chunk grid, a run list `chunk_rle` refuses, or a map the light bake refuses.
    /// The page is refused whole, because a view drawn with a hole in it cannot be told
    /// from a map with one.
    View(String),
    /// A JSON-RPC answer the watch rig could not read: not an object, no id, or an id no
    /// request of this client's carried.
    Rpc(String),
    /// A surface carried more vertices than a 16-bit index buffer can address, or more
    /// than the engine-side buffer that was already allocated for it.
    SurfaceSize {
        /// What overflowed.
        what: &'static str,
        /// The count that did not fit.
        got: usize,
        /// The cap.
        cap: usize,
    },
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length {
                what,
                expected,
                got,
            } => write!(formatter, "{what} must be {expected} bytes, got {got}"),
            Self::MissingRules { row } => write!(
                formatter,
                "the rules table carries no `{row}` row; the bridge reads its parameters from \
                 the table and holds no constant of its own (AGENTS.md section 12)"
            ),
            Self::RulesOutOfRange {
                field,
                value,
                because,
            } => write!(formatter, "rules field `{field}` is {value}: {because}"),
            Self::Chunk(error) => write!(formatter, "{error}"),
            Self::Mesh(error) => write!(formatter, "{error}"),
            Self::Schema { pointer, message } => {
                if pointer.is_empty() {
                    write!(formatter, "{message}")
                } else {
                    write!(formatter, "at {pointer}: {message}")
                }
            }
            Self::UnknownMethod { name } => write!(
                formatter,
                "`{name}` is not a value of `gp.api.v1.Method`; the bridge looks methods up in \
                 the schema and keeps no table of its own"
            ),
            Self::View(why) => write!(formatter, "the view was not stored: {why}"),
            Self::Rpc(why) => write!(formatter, "the gateway's answer was not read: {why}"),
            Self::SurfaceSize { what, got, cap } => {
                write!(formatter, "{what} is {got}, which exceeds the cap of {cap}")
            }
        }
    }
}

impl std::error::Error for BridgeError {}

impl From<ChunkInputError> for BridgeError {
    fn from(error: ChunkInputError) -> Self {
        Self::Chunk(error)
    }
}

impl From<MeshError> for BridgeError {
    fn from(error: MeshError) -> Self {
        Self::Mesh(error)
    }
}

impl From<pharmakos_proto::json::Error> for BridgeError {
    fn from(error: pharmakos_proto::json::Error) -> Self {
        Self::Schema {
            pointer: error.pointer.clone(),
            message: error.message.clone(),
        }
    }
}
