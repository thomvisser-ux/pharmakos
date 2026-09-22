// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One connection's life: the upgrade, then JSON-RPC over text frames, then a
//! close.
//!
//! Generic over `Read + Write` rather than written against [`std::net::TcpStream`],
//! for one reason that is worth the generic: **the whole protocol is testable
//! without a socket**. `tests/websocket.rs` drives a real handshake, real masked
//! frames and real JSON-RPC through this function over a pair of byte buffers,
//! which means the transport's negative cases -- an unmasked frame, a binary
//! frame, an oversized message, a foreign `Origin` -- are ordinary unit tests
//! rather than something that needs a port and a timeout.
//!
//! # The order, and why authentication is first
//!
//! The token rides the upgrade ([`crate::handshake`]), so a connection is
//! authenticated before a single frame is read. That is deliberate: the rate
//! limiter counts per token, so an unauthenticated connection would be one
//! nothing counts, and a socket that can send frames for free is a socket that
//! can spend the host's memory for free.
//!
//! # `std::time::Duration` is not a clock
//!
//! [`READ_TIMEOUT`] is a socket option. Nothing in the gateway's behaviour
//! depends on its value, no result changes if the operating system honours it
//! late, and it is never read as a time. The gateway's clock is the host's tick
//! and there is no other (AGENTS.md section 4.5).
//!
//! # The transport half and the surface half (T16a)
//!
//! T9 wrote one function that did both: it parsed frames *and* held a
//! `&mut Surface` to answer them with. One surface and several connections
//! cannot both be true of that shape, because two connections cannot both
//! hold the one mutable borrow -- so [`crate::serve`] would have had to
//! rewrite the transport rather than use it.
//!
//! The split is a trait and nothing else. [`Door`] is "somewhere a token is
//! authenticated and a text message is answered"; everything above it in this
//! module is RFC 6455 and JSON-RPC and knows nothing about a match.
//! [`SurfaceDoor`] is the in-process door, which is what [`serve`] still
//! builds, so every T9 and T13 test drives the same code it always did.
//! [`crate::serve`]'s door hands the work to the one thread that owns the
//! surface.

use crate::error::Error;
use crate::frame::{self, Assembler, Message, Opcode};
use crate::handshake::{self, MAX_HANDSHAKE_BYTES, Policy, Refusal};
use crate::rpc;
use crate::surface::Surface;
use crate::token::Token;
use std::io::{Read, Write};
use std::time::Duration;

/// How long a connection may be silent before the socket read gives up.
///
/// PLACEHOLDER: thirty seconds is a working number. OWNER settles it at
/// hardening with the rate limits. A socket option, not a clock read.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// How much a single socket read asks for.
const READ_CHUNK: usize = 8 * 1024;

/// How a connection ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Ended {
    /// The peer sent a close frame, and the gateway echoed one.
    PeerClosed(u16),
    /// The gateway closed the connection with this RFC 6455 code.
    GatewayClosed(u16),
    /// The upgrade was refused; the HTTP response says why.
    UpgradeRefused(Refusal),
    /// The socket went away without a close frame.
    Disconnected,
    /// Reading or writing failed.
    Io(String),
}

/// Where a connection's token is authenticated and its messages are answered.
///
/// The transport knows nothing else about the gateway. One implementation
/// ([`SurfaceDoor`]) holds the surface directly, which is what a test and a
/// single-connection host do; [`crate::serve`]'s implementation hands each
/// message to the one thread that owns the surface, which is what lets several
/// connections share one match.
pub trait Door {
    /// Authenticate the token that rode the upgrade and record it.
    ///
    /// `false` ends the connection with a 401 before a single frame is read,
    /// which is what makes every frame this gateway parses one the rate
    /// limiter can count.
    fn open(&mut self, token: &Token) -> bool;

    /// Answer one JSON-RPC text message, as the text that goes back.
    fn answer(&mut self, token: &Token, text: &str) -> String;

    /// Record a refused upgrade. Every refusal is in the audit log, whatever
    /// it was.
    fn refused(&mut self, refusal: &Refusal);
}

/// The in-process door: the surface itself, with the vision the fog filter
/// asks.
pub struct SurfaceDoor<'a, V: crate::fog::Vision> {
    surface: &'a mut Surface,
    vision: &'a V,
}

// By hand, because a `Vision` is not required to be `Debug` and requiring it
// would put a derive of this crate's in the way of the host's own vision type.
impl<V: crate::fog::Vision> std::fmt::Debug for SurfaceDoor<'_, V> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurfaceDoor")
            .field("match_id", &self.surface.match_id())
            .finish_non_exhaustive()
    }
}

impl<'a, V: crate::fog::Vision> SurfaceDoor<'a, V> {
    /// A door onto this surface.
    pub fn new(surface: &'a mut Surface, vision: &'a V) -> SurfaceDoor<'a, V> {
        SurfaceDoor { surface, vision }
    }
}

impl<V: crate::fog::Vision> Door for SurfaceDoor<'_, V> {
    fn open(&mut self, token: &Token) -> bool {
        let tick = self.surface.time().tick;
        let match_id = self.surface.match_id().to_owned();
        let Ok(grant) = self.surface.tokens().authenticate(token, &match_id, tick) else {
            return false;
        };
        let (subject, handle) = (grant.subject, grant.handle);
        self.surface.audit().record(
            tick,
            Some(subject),
            Some(handle),
            "upgrade",
            crate::audit::Outcome::Ok,
        );
        true
    }

    fn answer(&mut self, token: &Token, text: &str) -> String {
        let response = match rpc::parse(text) {
            Ok(request) => self.surface.call(Some(token), &request, self.vision),
            Err(malformed) => rpc::malformed_response(&malformed),
        };
        rpc::render(&response)
    }

    fn refused(&mut self, refusal: &Refusal) {
        let tick = self.surface.time().tick;
        let error = refusal_error(refusal);
        self.surface
            .audit()
            .refused(tick, None, None, "upgrade", &error);
    }
}

/// The error a refused upgrade is logged as.
#[must_use]
pub fn refusal_error(refusal: &Refusal) -> Error {
    match refusal.status() {
        401 => Error::unauthenticated(refusal.reason()),
        403 => Error::forbidden(refusal.reason()),
        _ => Error::invalid(refusal.reason()),
    }
}

/// Serve one connection from the first byte to the last.
///
/// Every outcome is an [`Ended`] rather than an error: a refused upgrade and a
/// polite goodbye are both ordinary ends to a connection, and the caller wants
/// to log them the same way.
pub fn serve<S: Read + Write, V: crate::fog::Vision>(
    stream: &mut S,
    surface: &mut Surface,
    vision: &V,
    policy: &Policy,
) -> Ended {
    let mut door = SurfaceDoor::new(surface, vision);
    serve_through(stream, &mut door, policy)
}

/// Serve one connection through any [`Door`].
pub fn serve_through<S: Read + Write, D: Door + ?Sized>(
    stream: &mut S,
    door: &mut D,
    policy: &Policy,
) -> Ended {
    let (request, leftover) = match read_handshake(stream) {
        Ok(pair) => pair,
        Err(ended) => return ended,
    };

    let upgrade = match handshake::review(&request, policy) {
        Ok(upgrade) => upgrade,
        Err(refusal) => return refuse(stream, door, &refusal),
    };

    // Authenticate before a single frame is read. The token is parsed into a
    // `Token` here and the text is dropped with the `Upgrade`.
    let Some(token) = Token::parse(&upgrade.bearer) else {
        return refuse(stream, door, &Refusal::MalformedToken);
    };
    if !door.open(&token) {
        return refuse(stream, door, &Refusal::Unauthenticated);
    }

    if let Err(error) = stream.write_all(handshake::response(&upgrade).as_bytes()) {
        return Ended::Io(error.to_string());
    }

    pump(stream, door, &token, leftover)
}

/// Serve one accepted TCP connection, with the socket's read timeout set.
pub fn serve_connection<V: crate::fog::Vision>(
    connection: crate::net::Connection,
    surface: &mut Surface,
    vision: &V,
    policy: &Policy,
) -> Ended {
    let mut door = SurfaceDoor::new(surface, vision);
    serve_connection_through(connection, &mut door, policy)
}

/// Serve one accepted TCP connection through any [`Door`].
pub fn serve_connection_through<D: Door + ?Sized>(
    connection: crate::net::Connection,
    door: &mut D,
    policy: &Policy,
) -> Ended {
    let mut stream = connection.stream;
    if let Err(error) = stream.set_read_timeout(Some(READ_TIMEOUT)) {
        return Ended::Io(error.to_string());
    }
    serve_through(&mut stream, door, policy)
}

/// Read the opening handshake, up to and including the blank line.
///
/// Returns the handshake's text **and whatever came after it in the same read**.
/// A client that sends its first frame in the same packet as the upgrade is
/// ordinary and conforming, and a reader that swallowed those bytes -- or that
/// tried to read them as UTF-8 along with the request -- would lose the first
/// call of every fast client.
fn read_handshake<S: Read>(stream: &mut S) -> Result<(String, Vec<u8>), Ended> {
    let mut buffer: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0_u8; READ_CHUNK];
    let end = loop {
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index.saturating_add(4);
        }
        if buffer.len() > MAX_HANDSHAKE_BYTES {
            return Err(Ended::UpgradeRefused(Refusal::TooLarge));
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err(Ended::Disconnected),
            Ok(read) => match chunk.get(..read) {
                Some(bytes) => buffer.extend_from_slice(bytes),
                None => return Err(Ended::Disconnected),
            },
            Err(error) => return Err(Ended::Io(error.to_string())),
        }
    };
    let leftover = buffer.get(end..).unwrap_or_default().to_vec();
    let head = buffer.get(..end).unwrap_or_default().to_vec();
    let text = String::from_utf8(head).map_err(|_| Ended::UpgradeRefused(Refusal::Malformed))?;
    Ok((text, leftover))
}

/// Write an HTTP refusal, log it, and end.
fn refuse<S: Write, D: Door + ?Sized>(stream: &mut S, door: &mut D, refusal: &Refusal) -> Ended {
    door.refused(refusal);
    let _ = stream.write_all(handshake::refusal_response(refusal).as_bytes());
    Ended::UpgradeRefused(refusal.clone())
}

/// The message loop.
fn pump<S: Read + Write, D: Door + ?Sized>(
    stream: &mut S,
    door: &mut D,
    token: &Token,
    leftover: Vec<u8>,
) -> Ended {
    let mut buffer: Vec<u8> = leftover;
    let mut assembler = Assembler::new();
    let mut chunk = [0_u8; READ_CHUNK];

    loop {
        // Serve everything already buffered before asking for more.
        loop {
            let decoded = match frame::decode(&buffer) {
                Ok(Some(pair)) => pair,
                Ok(None) => break,
                Err(error) => {
                    return close(stream, error.close_code(), error.reason());
                }
            };
            let (parsed, used) = decoded;
            buffer.drain(..used);

            let message = match assembler.accept(parsed) {
                Ok(Some(message)) => message,
                Ok(None) => continue,
                Err(error) => {
                    return close(stream, error.close_code(), error.reason());
                }
            };

            match message {
                Message::Text(text) => {
                    // One request in flight per connection: the answer is
                    // waited for before the next frame is read.
                    let response = door.answer(token, &text);
                    let bytes = frame::encode(Opcode::Text, response.as_bytes());
                    if let Err(error) = stream.write_all(&bytes) {
                        return Ended::Io(error.to_string());
                    }
                }
                Message::Ping(payload) => {
                    let bytes = frame::encode(Opcode::Pong, &payload);
                    if let Err(error) = stream.write_all(&bytes) {
                        return Ended::Io(error.to_string());
                    }
                }
                Message::Pong(_) => {}
                Message::Close(code, _) => {
                    let _ = stream.write_all(&frame::close(1000, "goodbye"));
                    return Ended::PeerClosed(code);
                }
            }
        }

        match stream.read(&mut chunk) {
            Ok(0) => return Ended::Disconnected,
            Ok(read) => match chunk.get(..read) {
                Some(bytes) => buffer.extend_from_slice(bytes),
                None => return Ended::Disconnected,
            },
            Err(error) => return Ended::Io(error.to_string()),
        }
    }
}

/// Close the connection with a code and a reason.
fn close<S: Write>(stream: &mut S, code: u16, reason: &str) -> Ended {
    let _ = stream.write_all(&frame::close(code, reason));
    Ended::GatewayClosed(code)
}
