// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The RFC 6455 opening handshake, with the `Host` and `Origin` checks spec
//! section 12 requires.
//!
//! The gateway binds loopback only ([`crate::net`]), and that alone is not
//! enough: a page in a browser on the same machine can reach a loopback port,
//! and a name that resolves to `127.0.0.1` can carry an attacker's `Host`. So
//! the upgrade is refused unless
//!
//! * the request is a `GET` at HTTP/1.1 or better with the four headers RFC 6455
//!   section 4.2.1 requires;
//! * `Sec-WebSocket-Version` is exactly `13`;
//! * `Sec-WebSocket-Key` decodes to exactly sixteen bytes;
//! * `Host` names a loopback authority -- `127.0.0.1`, `[::1]` or `localhost` --
//!   at the port this listener actually bound, and nothing else. A `Host` of
//!   `pharmakos.example` that DNS happens to point at `127.0.0.1` is refused
//!   here, which is the whole reason the check exists;
//! * `Origin`, **if present at all**, is on the allow-list, which is empty in v1;
//! * and an `Authorization: Bearer <64 hex digits>` header carries the seat's
//!   token.
//!
//! # Why the token rides the upgrade
//!
//! Because a connection with no token would otherwise be a connection with no
//! rate limit: the limiter counts per token ([`crate::limit`]), so a peer that
//! never authenticates would be a peer nothing counts. Authenticating once, at
//! the upgrade, gives every open socket a handle, a budget and an audit trail,
//! and keeps the secret out of every message that follows.
//!
//! It is a **header** and never a query string. A `?token=` in a request target
//! is a secret in something everything logs; an `Authorization` header is where
//! the rest of the world puts one.
//!
//! The last rule deserves its sentence. Every v1 client is a native process --
//! the Godot editor, `gamectl`, the built-in operator -- and a native client
//! sends no `Origin`. A browser always sends one. So "no `Origin` header, or a
//! listed one" reads exactly as "not a web page", and with an empty list v1
//! refuses every browser. [`Policy::allowed_origins`] is not a widening knob for
//! convenience; it is where the roadmap's web spectator would be named, one
//! origin at a time, if it ever ships.
//!
//! There is no flag anywhere in this module that relaxes any of it (AGENTS.md
//! section 7: "no LAN convenience binding, not even behind a flag").

use crate::sha1;
use pharmakos_proto::json::base64;

/// The GUID RFC 6455 section 4.2.2 concatenates with the client's key. A
/// constant of the standard, not a secret and not a tuning value.
pub const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// The RFC 6455 version this server speaks. Version 13 is the only one there is.
pub const WEBSOCKET_VERSION: &str = "13";

/// The largest opening handshake the gateway will read, in bytes.
///
/// A handshake is a few hundred bytes; four kibibytes is generous. The cap is
/// here so a client that never sends `\r\n\r\n` cannot make the gateway buffer
/// without end.
pub const MAX_HANDSHAKE_BYTES: usize = 4 * 1024;

/// The most headers a handshake may carry, so a client cannot spend the
/// gateway's memory on a thousand of them.
pub const MAX_HANDSHAKE_HEADERS: usize = 64;

/// What this listener will accept an upgrade for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Policy {
    /// The TCP port the listener actually bound. A `Host` header naming a
    /// different port is refused: it is either a mistake or a cross-origin
    /// request aimed at some other service on the machine.
    pub port: u16,
    /// Origins allowed to open a WebSocket, exact match, scheme included.
    ///
    /// **Empty in v1, and that is the policy rather than an omission.** See the
    /// module docs.
    pub allowed_origins: Vec<String>,
}

impl Policy {
    /// The v1 policy: this port, and no origin at all.
    #[must_use]
    pub const fn loopback(port: u16) -> Policy {
        Policy {
            port,
            allowed_origins: Vec::new(),
        }
    }
}

/// Why an upgrade was refused.
///
/// Each variant is a different thing for the operator of the machine to read in
/// the audit log, which is why they are not collapsed into one string.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// The bytes are not an HTTP request the server can parse at all.
    Malformed,
    /// A method other than `GET`.
    NotGet,
    /// HTTP/1.0 or older; RFC 6455 requires 1.1 or better.
    OldHttp,
    /// No `Host` header. RFC 6455 section 4.2.1 requires one.
    MissingHost,
    /// A `Host` that is not a loopback authority at this listener's port.
    ForeignHost(String),
    /// `Upgrade: websocket` missing.
    MissingUpgrade,
    /// `Connection: Upgrade` missing.
    MissingConnectionUpgrade,
    /// No `Sec-WebSocket-Key`.
    MissingKey,
    /// A key that is not sixteen bytes of standard base64.
    MalformedKey,
    /// A `Sec-WebSocket-Version` other than `13`.
    UnsupportedVersion(String),
    /// An `Origin` header that is not on the allow-list -- in v1, any `Origin`
    /// header at all.
    ForeignOrigin(String),
    /// No `Authorization: Bearer` header, or one whose scheme is not `Bearer`.
    MissingAuthorization,
    /// A bearer token that is not 64 hex digits.
    MalformedToken,
    /// A well formed token this match did not mint, or one revoked or expired.
    Unauthenticated,
    /// A header offered twice where one value is the only sensible reading.
    DuplicateHeader(String),
    /// The handshake ran past [`MAX_HANDSHAKE_BYTES`] or
    /// [`MAX_HANDSHAKE_HEADERS`].
    TooLarge,
}

impl Refusal {
    /// The HTTP status to answer with.
    #[must_use]
    pub const fn status(&self) -> u16 {
        match self {
            Refusal::ForeignHost(_) | Refusal::ForeignOrigin(_) => 403,
            Refusal::MissingAuthorization | Refusal::MalformedToken | Refusal::Unauthenticated => {
                401
            }
            Refusal::UnsupportedVersion(_) => 426,
            Refusal::TooLarge => 431,
            _ => 400,
        }
    }

    /// A short English reason, safe to put on the wire and in the audit log.
    ///
    /// It never echoes the offending value back: a refusal tells the caller
    /// which rule it broke, not what the gateway thought it said.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Refusal::Malformed => "not an HTTP request",
            Refusal::NotGet => "the upgrade is a GET",
            Refusal::OldHttp => "HTTP/1.1 or better is required",
            Refusal::MissingHost => "no Host header",
            Refusal::ForeignHost(_) => "Host is not this loopback listener",
            Refusal::MissingUpgrade => "no Upgrade: websocket",
            Refusal::MissingConnectionUpgrade => "no Connection: Upgrade",
            Refusal::MissingKey => "no Sec-WebSocket-Key",
            Refusal::MalformedKey => "Sec-WebSocket-Key is not sixteen base64 bytes",
            Refusal::MissingAuthorization => "no Authorization: Bearer seat token",
            Refusal::MalformedToken => "the bearer token is not 64 hex digits",
            Refusal::Unauthenticated => "that token is not this match's",
            Refusal::UnsupportedVersion(_) => "only Sec-WebSocket-Version 13 is spoken",
            Refusal::ForeignOrigin(_) => "Origin is not allowed to open a seat",
            Refusal::DuplicateHeader(_) => "a header was sent twice",
            Refusal::TooLarge => "the handshake is too large",
        }
    }
}

/// An accepted upgrade: what the server has to say back.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Upgrade {
    /// The request target, kept for the audit log. The gateway serves one
    /// endpoint, so it is not routed on.
    pub target: String,
    /// The value of `Sec-WebSocket-Accept`.
    pub accept: String,
    /// The bearer token the client presented, still as text. The session turns
    /// it into a [`crate::token::Token`] and drops the string.
    pub bearer: String,
}

/// RFC 6455 section 4.2.2 step 5.4: base64(SHA-1(key + GUID)).
///
/// Pinned to the standard's own worked example in the tests, which is the only
/// thing that makes a hand-written SHA-1 trustworthy.
#[must_use]
pub fn accept_key(key: &str) -> String {
    let mut message = String::with_capacity(key.len().saturating_add(WEBSOCKET_GUID.len()));
    message.push_str(key);
    message.push_str(WEBSOCKET_GUID);
    base64::encode(&sha1::digest(message.as_bytes()))
}

/// True for exactly the text RFC 6455 section 4.1 requires of a
/// `Sec-WebSocket-Key`: sixteen random bytes as **standard**, padded base64,
/// which is twenty-two alphabet symbols and two `=`.
///
/// The shape is checked here rather than left to the decoder, and that is the
/// point of the function. `pharmakos_proto::json::base64::decode` is the
/// proto3 JSON mapping's reader (decisions-log item 100 (10) deleted this
/// crate's forty-line copy in favour of it), and the mapping *requires* a
/// reader to be lenient: it accepts the URL-safe alphabet and unpadded text as
/// well. That is correct for a `bytes` field and wrong here.
/// `NGC+MSAeaf7aoO7ouZl/XA` (the padding dropped) and
/// `NGC-MSAeaf7aoO7ouZl_XA==` (the URL-safe alphabet) both decode to the same
/// sixteen bytes and neither is a key a conforming client sends, so the only
/// thing that turns them away is this check. The server side of the handshake
/// is allowed to be strict (section 4.2.1), and a security surface should be.
fn is_sixteen_base64_bytes(key: &str) -> bool {
    // 16 bytes is five whole quanta plus one byte: 22 symbols and "==".
    let Some(symbols) = key.strip_suffix("==") else {
        return false;
    };
    if symbols.len() != 22
        || !symbols
            .bytes()
            .all(|symbol| matches!(symbol, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/'))
    {
        return false;
    }
    base64::decode(key).is_some_and(|bytes| bytes.len() == 16)
}

/// Read one header value, case-insensitively, refusing a duplicate.
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Result<Option<&'a str>, Refusal> {
    let mut found: Option<&str> = None;
    for (key, value) in headers {
        if key.eq_ignore_ascii_case(name) {
            if found.is_some() {
                return Err(Refusal::DuplicateHeader(name.to_ascii_lowercase()));
            }
            found = Some(value.as_str());
        }
    }
    Ok(found)
}

/// True when a comma-separated header list carries `token`, case-insensitively.
fn lists(value: &str, token: &str) -> bool {
    value
        .split(',')
        .any(|part| part.trim().eq_ignore_ascii_case(token))
}

/// True when `authority` is a loopback name at `port`.
///
/// The three spellings are the ones a loopback listener can legitimately be
/// reached by. Anything else -- including a name that resolves to `127.0.0.1` --
/// is refused, which is the point of checking the header rather than the socket.
fn loopback_authority(authority: &str, port: u16) -> bool {
    let (host, host_port) = match authority.strip_prefix('[') {
        // An IPv6 literal: `[::1]` or `[::1]:9500`.
        Some(rest) => match rest.split_once(']') {
            Some((inner, tail)) => {
                let mut bracketed = String::with_capacity(inner.len().saturating_add(2));
                bracketed.push('[');
                bracketed.push_str(inner);
                bracketed.push(']');
                (bracketed, tail.strip_prefix(':'))
            }
            None => return false,
        },
        None => match authority.split_once(':') {
            Some((name, tail)) => (name.to_owned(), Some(tail)),
            None => (authority.to_owned(), None),
        },
    };

    let named = matches!(host.as_str(), "127.0.0.1" | "[::1]" | "localhost");
    let right_port = match host_port {
        // Compared as text against the port's own spelling rather than parsed:
        // `u16::from_str` also accepts `09500` and `+9500` for 9500, and "this
        // listener's port" should mean the digits this listener would write.
        Some(text) => text == port.to_string(),
        // A `Host` with no port means 80, which this listener never binds.
        None => false,
    };
    named && right_port
}

/// Review a complete opening handshake.
///
/// `text` is everything up to and including the blank line that ends the
/// request. [`crate::session`] reads it under [`MAX_HANDSHAKE_BYTES`].
///
/// # Errors
///
/// A [`Refusal`] naming the one rule the request broke. The checks run in the
/// order the module docs list them, so the reason a client sees is the first
/// thing wrong rather than the last.
pub fn review(text: &str, policy: &Policy) -> Result<Upgrade, Refusal> {
    if text.len() > MAX_HANDSHAKE_BYTES {
        return Err(Refusal::TooLarge);
    }

    let mut lines = text.split("\r\n");
    let request_line = lines.next().ok_or(Refusal::Malformed)?;
    let mut parts = request_line.split(' ');
    let method = parts.next().ok_or(Refusal::Malformed)?;
    let target = parts.next().ok_or(Refusal::Malformed)?;
    let version = parts.next().ok_or(Refusal::Malformed)?;
    if parts.next().is_some() {
        return Err(Refusal::Malformed);
    }
    if method != "GET" {
        return Err(Refusal::NotGet);
    }
    if target.is_empty() {
        return Err(Refusal::Malformed);
    }
    match version {
        "HTTP/1.1" => {}
        "HTTP/1.0" | "HTTP/0.9" => return Err(Refusal::OldHttp),
        _ => return Err(Refusal::Malformed),
    }

    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if headers.len() >= MAX_HANDSHAKE_HEADERS {
            return Err(Refusal::TooLarge);
        }
        let (name, value) = line.split_once(':').ok_or(Refusal::Malformed)?;
        if name.is_empty() || name.trim() != name {
            // A space before the colon is how a request smuggles a header past
            // a lenient parser. This one is not lenient.
            return Err(Refusal::Malformed);
        }
        headers.push((name.to_owned(), value.trim().to_owned()));
    }

    let host = header(&headers, "Host")?.ok_or(Refusal::MissingHost)?;
    if !loopback_authority(host, policy.port) {
        return Err(Refusal::ForeignHost(host.to_owned()));
    }

    let upgrade = header(&headers, "Upgrade")?.unwrap_or_default();
    if !lists(upgrade, "websocket") {
        return Err(Refusal::MissingUpgrade);
    }
    let connection = header(&headers, "Connection")?.unwrap_or_default();
    if !lists(connection, "upgrade") {
        return Err(Refusal::MissingConnectionUpgrade);
    }

    let version = header(&headers, "Sec-WebSocket-Version")?.unwrap_or_default();
    if version != WEBSOCKET_VERSION {
        return Err(Refusal::UnsupportedVersion(version.to_owned()));
    }

    if let Some(origin) = header(&headers, "Origin")? {
        if !policy
            .allowed_origins
            .iter()
            .any(|allowed| allowed == origin)
        {
            return Err(Refusal::ForeignOrigin(origin.to_owned()));
        }
    }

    let key = header(&headers, "Sec-WebSocket-Key")?.ok_or(Refusal::MissingKey)?;
    if !is_sixteen_base64_bytes(key) {
        return Err(Refusal::MalformedKey);
    }

    let authorization = header(&headers, "Authorization")?.ok_or(Refusal::MissingAuthorization)?;
    let bearer = authorization
        .strip_prefix("Bearer ")
        .ok_or(Refusal::MissingAuthorization)?
        .trim();
    if crate::token::Token::parse(bearer).is_none() {
        return Err(Refusal::MalformedToken);
    }

    Ok(Upgrade {
        target: target.to_owned(),
        accept: accept_key(key),
        bearer: bearer.to_owned(),
    })
}

/// The `101 Switching Protocols` response for an accepted upgrade.
///
/// No `Sec-WebSocket-Protocol` and no `Sec-WebSocket-Extensions`: v1 negotiates
/// neither, so it names neither (RFC 6455 section 4.2.2 -- a server that echoes
/// an extension it does not implement is worse than one that offers none).
#[must_use]
pub fn response(upgrade: &Upgrade) -> String {
    let accept = &upgrade.accept;
    format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\
         \r\n"
    )
}

/// The response for a refused upgrade: the status, the reason, and a body a
/// person reading a terminal can act on.
#[must_use]
pub fn refusal_response(refusal: &Refusal) -> String {
    let status = refusal.status();
    let reason = refusal.reason();
    let phrase = match status {
        401 => "Unauthorized",
        403 => "Forbidden",
        426 => "Upgrade Required",
        431 => "Request Header Fields Too Large",
        _ => "Bad Request",
    };
    let version_header = if status == 426 {
        format!("Sec-WebSocket-Version: {WEBSOCKET_VERSION}\r\n")
    } else {
        String::new()
    };
    let body = format!("{reason}\n");
    let length = body.len();
    format!(
        "HTTP/1.1 {status} {phrase}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {length}\r\n\
         {version_header}\
         Connection: close\r\n\
         \r\n\
         {body}"
    )
}

#[cfg(test)]
mod tests {
    use super::{Policy, Refusal, accept_key, loopback_authority, response, review};

    const PORT: u16 = 9500;

    fn request(lines: &[&str]) -> String {
        let mut text = String::from("GET /seat HTTP/1.1\r\n");
        for line in lines {
            text.push_str(line);
            text.push_str("\r\n");
        }
        text.push_str("\r\n");
        text
    }

    fn good() -> Vec<&'static str> {
        vec![
            "Host: 127.0.0.1:9500",
            "Upgrade: websocket",
            "Connection: Upgrade",
            "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
            "Sec-WebSocket-Version: 13",
            "Authorization: Bearer              0000000000000000000000000000000000000000000000000000000000000001",
        ]
    }

    /// RFC 6455 section 1.3, verbatim. If this fails, every WebSocket client in
    /// the world disagrees with this server and nothing else matters.
    #[test]
    fn the_rfc_6455_worked_example() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn a_conforming_handshake_is_accepted_and_answered() {
        let upgrade = review(&request(&good()), &Policy::loopback(PORT)).expect("conforming");
        assert_eq!(upgrade.accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
        assert_eq!(upgrade.target, "/seat");
        let text = response(&upgrade);
        assert!(
            text.starts_with("HTTP/1.1 101 Switching Protocols\r\n"),
            "{text}"
        );
        assert!(text.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
        assert!(
            !text.contains("Sec-WebSocket-Extensions"),
            "v1 negotiates none"
        );
    }

    #[test]
    fn the_ipv6_loopback_literal_is_a_loopback_authority() {
        assert!(loopback_authority("[::1]:9500", PORT));
        assert!(loopback_authority("127.0.0.1:9500", PORT));
        assert!(loopback_authority("localhost:9500", PORT));
        assert!(
            !loopback_authority("127.0.0.1:9501", PORT),
            "another service"
        );
        assert!(
            !loopback_authority("127.0.0.1", PORT),
            "port 80 is not ours"
        );
        assert!(!loopback_authority("pharmakos.example:9500", PORT));
        assert!(!loopback_authority("127.0.0.1.evil.example:9500", PORT));
        assert!(!loopback_authority("[::1", PORT), "an unterminated literal");
        // The port is compared as the digits this listener would write, not
        // parsed: `u16::from_str` also reads `09500` and `+9500` as 9500, and
        // a `Host` this gateway would never write is not this gateway's host.
        assert!(
            !loopback_authority("127.0.0.1:09500", PORT),
            "a leading zero is not the port's spelling"
        );
        assert!(!loopback_authority("127.0.0.1:+9500", PORT));
        assert!(!loopback_authority("127.0.0.1: 9500", PORT));
    }

    #[test]
    fn a_host_that_merely_resolves_to_loopback_is_refused() {
        let mut lines = good();
        *lines.first_mut().expect("Host is first") = "Host: pharmakos.example:9500";
        let refusal = review(&request(&lines), &Policy::loopback(PORT)).expect_err("refused");
        assert!(matches!(refusal, Refusal::ForeignHost(_)), "{refusal:?}");
        assert_eq!(refusal.status(), 403);
    }

    #[test]
    fn a_version_other_than_13_is_refused_with_426() {
        let mut lines = good();
        *lines.get_mut(4).expect("version line") = "Sec-WebSocket-Version: 8";
        let refusal = review(&request(&lines), &Policy::loopback(PORT)).expect_err("refused");
        assert!(
            matches!(refusal, Refusal::UnsupportedVersion(_)),
            "{refusal:?}"
        );
        assert_eq!(refusal.status(), 426);
        assert!(super::refusal_response(&refusal).contains("Sec-WebSocket-Version: 13"));
    }

    #[test]
    fn a_short_key_is_refused() {
        let mut lines = good();
        *lines.get_mut(3).expect("key line") = "Sec-WebSocket-Key: Zm9v";
        let refusal = review(&request(&lines), &Policy::loopback(PORT)).expect_err("refused");
        assert_eq!(refusal, Refusal::MalformedKey);
    }

    #[test]
    fn a_key_the_lenient_decoder_would_take_is_still_refused() {
        // Both decode to sixteen bytes -- the proto3 JSON mapping's reader is
        // *required* to accept unpadded text and the URL-safe alphabet -- and
        // neither is a key RFC 6455 section 4.1 lets a client send. The shape
        // check in `is_sixteen_base64_bytes` is the only thing that turns them
        // away, which is what its own doc claims and what this asserts.
        for key in ["NGC+MSAeaf7aoO7ouZl/XA", "NGC-MSAeaf7aoO7ouZl_XA=="] {
            assert_eq!(
                pharmakos_proto::json::base64::decode(key).map(|bytes| bytes.len()),
                Some(16),
                "`{key}` is meant to be one the lenient decoder takes"
            );
            let line = format!("Sec-WebSocket-Key: {key}");
            let mut lines = good();
            *lines.get_mut(3).expect("key line") = &line;
            let refusal = review(&request(&lines), &Policy::loopback(PORT)).expect_err("refused");
            assert_eq!(refusal, Refusal::MalformedKey, "`{key}`");
        }
    }

    #[test]
    fn a_post_is_not_an_upgrade() {
        let text = "POST /seat HTTP/1.1\r\nHost: 127.0.0.1:9500\r\n\r\n";
        assert_eq!(
            review(text, &Policy::loopback(PORT)).expect_err("refused"),
            Refusal::NotGet
        );
    }

    #[test]
    fn a_duplicated_key_is_refused_rather_than_resolved() {
        let mut lines = good();
        lines.push("Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==");
        let refusal = review(&request(&lines), &Policy::loopback(PORT)).expect_err("refused");
        assert!(
            matches!(refusal, Refusal::DuplicateHeader(_)),
            "{refusal:?}"
        );
    }

    #[test]
    fn a_header_with_a_space_before_the_colon_is_refused() {
        let mut lines = good();
        lines.push("X-Smuggled : 1");
        assert_eq!(
            review(&request(&lines), &Policy::loopback(PORT)).expect_err("refused"),
            Refusal::Malformed
        );
    }

    #[test]
    fn connection_keep_alive_upgrade_is_read_as_a_list() {
        let mut lines = good();
        *lines.get_mut(2).expect("connection line") = "Connection: keep-alive, Upgrade";
        review(&request(&lines), &Policy::loopback(PORT)).expect("a list, not a word");
    }
}
