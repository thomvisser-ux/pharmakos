// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bench's one error type: every way a measurement can fail to measure,
//! enumerated (S2's plan, E7, task `g3h`).
//!
//! Each variant names a class of failure, and its text names the instance, so
//! a `::warning::` says both what went wrong and where. Nothing in this crate
//! falls back to a default instead: a figure that could not be taken is an
//! error, never a zero (`tests/fallbacks.rs` holds the crate to it).

use std::fmt;
use std::path::PathBuf;

/// Why a measurement could not be taken.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Error {
    /// A committed input could not be read.
    Read {
        /// The file or directory.
        path: PathBuf,
        /// What the OS said.
        reason: String,
    },
    /// A committed input, or something the bench wrote, would not decode or
    /// encode.
    Codec {
        /// What was being decoded or encoded.
        what: String,
        /// What the codec said.
        reason: String,
    },
    /// The rules table, or the bench's variant of it, lacks or refuses a row.
    Rules(String),
    /// The gateway refused a call the bench made.
    Gateway {
        /// What the bench was doing.
        during: &'static str,
        /// What the gateway said.
        message: String,
    },
    /// The sim refused a call the bench made from outside (the fourth seat's
    /// line, the only measurement that drives the sim's own runner).
    Sim {
        /// What the bench was doing.
        during: &'static str,
        /// What the sim said.
        reason: String,
    },
    /// A seat's playbook was refused, or sealed off the size budget.
    Submission(String),
    /// The gate's layout, the crater stream or the support construct could not
    /// be built on this map.
    Layout(String),
    /// Two runs of the same input disagreed, or an input drifted from what the
    /// module documents.
    Drift(String),
    /// The measurement was asked for nothing, or found nothing to measure.
    Empty(String),
    /// A count or a sum would not fit its type.
    Overflow(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Read { path, reason } => write!(f, "reading {}: {reason}", path.display()),
            Error::Codec { what, reason } => write!(f, "{what}: {reason}"),
            Error::Rules(reason) => write!(f, "the rules table: {reason}"),
            Error::Gateway { during, message } => {
                write!(f, "the gateway refused while {during}: {message}")
            }
            Error::Sim { during, reason } => write!(f, "the sim refused while {during}: {reason}"),
            Error::Submission(reason)
            | Error::Layout(reason)
            | Error::Drift(reason)
            | Error::Empty(reason) => f.write_str(reason),
            Error::Overflow(what) => write!(f, "{what} does not fit its type"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// The gateway's refusal, while doing `during`.
    #[must_use]
    pub fn gateway(during: &'static str, error: &pharmakos_gateway::Error) -> Error {
        Error::Gateway {
            during,
            message: error.message.clone(),
        }
    }
}
