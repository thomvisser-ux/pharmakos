// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The listener. Loopback, and nothing else, ever.
//!
//! Spec section 12 and AGENTS.md section 7: "Localhost only. The Seat Gateway
//! binds `127.0.0.1` and `::1` and nothing else, with `Host` and `Origin` checks
//! on the WebSocket upgrade. No `0.0.0.0`, no LAN convenience binding, not even
//! behind a flag."
//!
//! **The type is the guarantee.** [`Listener::bind`] takes a port and nothing
//! else: there is no parameter, no builder and no environment variable through
//! which an address could reach it, so a LAN binding is not something this crate
//! refuses at run time -- it is something it cannot express.
//! `tests/confinement.rs` asserts that no wildcard address is written anywhere
//! in the crate's source, which is the half a type cannot check.
//!
//! # Both families, and what happens when one is missing
//!
//! `127.0.0.1` and `::1` are two addresses and need two sockets. A machine with
//! IPv6 switched off has no `::1` to bind, so the second bind is allowed to
//! fail and is recorded rather than fatal; binding *neither* is an error.
//! [`Listener::addresses`] is what actually got bound, and every address in it
//! is loopback or the bind was a bug.
//!
//! # Threads
//!
//! One accept thread per socket, handing connections down a `std::sync::mpsc`
//! channel (decisions-log item 99: "blocking threads and `std::sync` channels").
//! No async runtime: at most three seats, one editor and a CLI ever connect, and
//! a runtime would be a dependency and a scheduler on the surface v1.1
//! publishes.

use crate::error::Error;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

/// The port the lobby binds when it has no reason to choose another.
///
/// PLACEHOLDER: no port is registered and the spec names none. OWNER picks one
/// at packaging, or the lobby passes `0` and lets the operating system choose --
/// which is what a single-machine game with a lobby that already knows the port
/// should probably do, and what the tests here use.
pub const DEFAULT_PORT: u16 = 9500;

/// A loopback listener: one socket per address family.
#[derive(Debug)]
pub struct Listener {
    sockets: Vec<TcpListener>,
    port: u16,
}

impl Listener {
    /// Bind `127.0.0.1` and `::1` at `port`.
    ///
    /// Pass `0` for an ephemeral port; both families are then bound to the port
    /// the first bind was given, so the gateway still has exactly one port.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when neither loopback address could be
    /// bound -- the port is taken, or the machine has no loopback at all.
    pub fn bind(port: u16) -> Result<Listener, Error> {
        let mut sockets: Vec<TcpListener> = Vec::new();

        let first = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
        let mut chosen = port;
        match first {
            Ok(socket) => {
                chosen = socket.local_addr().map_or(port, |address| address.port());
                sockets.push(socket);
            }
            Err(error) => {
                // Recorded, not fatal: a machine without IPv4 is strange but the
                // same rule applies to both families.
                let _ = error;
            }
        }

        match TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, chosen))) {
            Ok(socket) => {
                if sockets.is_empty() {
                    chosen = socket.local_addr().map_or(port, |address| address.port());
                }
                sockets.push(socket);
            }
            Err(error) => {
                let _ = error;
            }
        }

        if sockets.is_empty() {
            return Err(Error::internal(format!(
                "neither 127.0.0.1:{port} nor [::1]:{port} could be bound. The Seat Gateway \
                 binds loopback only and has no other address to try."
            )));
        }
        Ok(Listener {
            sockets,
            port: chosen,
        })
    }

    /// The port every bound socket is on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// What actually got bound. Every entry is a loopback address.
    #[must_use]
    pub fn addresses(&self) -> Vec<SocketAddr> {
        self.sockets
            .iter()
            .filter_map(|socket| socket.local_addr().ok())
            .collect()
    }

    /// The handshake policy for this listener: its port, and no origin.
    #[must_use]
    pub fn policy(&self) -> crate::handshake::Policy {
        crate::handshake::Policy::loopback(self.port)
    }

    /// Start accepting, one thread per socket.
    ///
    /// The receiver yields connections until every accept thread has stopped,
    /// which happens when its socket is closed. A peer that is somehow not
    /// loopback is dropped before it reaches the channel: it cannot happen on a
    /// loopback bind, and if it ever does, the gateway's answer is to hang up.
    #[must_use]
    pub fn accept(self) -> Accepting {
        let (sender, receiver): (Sender<Connection>, Receiver<Connection>) = channel();
        let mut threads: Vec<JoinHandle<()>> = Vec::new();
        for socket in self.sockets {
            let sender = sender.clone();
            threads.push(std::thread::spawn(move || {
                accept_loop(&socket, &sender);
            }));
        }
        drop(sender);
        Accepting { receiver, threads }
    }
}

/// One accepted connection.
#[derive(Debug)]
pub struct Connection {
    /// The socket.
    pub stream: TcpStream,
    /// Who connected. Always loopback.
    pub peer: SocketAddr,
}

/// A running listener's connections.
#[derive(Debug)]
pub struct Accepting {
    receiver: Receiver<Connection>,
    threads: Vec<JoinHandle<()>>,
}

impl Accepting {
    /// The next connection, blocking until one arrives.
    ///
    /// `None` when every accept thread has stopped. Deliberately not called
    /// `next`: this is not an iterator, and a type that looks like one invites a
    /// `for` loop that swallows the difference between "no connection yet" and
    /// "the listener is closed".
    #[must_use]
    pub fn recv(&self) -> Option<Connection> {
        self.receiver.recv().ok()
    }

    /// How many accept threads are running.
    #[must_use]
    pub fn threads(&self) -> usize {
        self.threads.len()
    }
}

/// Accept until the socket is closed.
fn accept_loop(socket: &TcpListener, sender: &Sender<Connection>) {
    loop {
        match socket.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    // Unreachable on a loopback bind. Hanging up is the only
                    // answer that cannot become a foothold.
                    drop(stream);
                    continue;
                }
                if sender.send(Connection { stream, peer }).is_err() {
                    return;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Listener;
    use std::io::Write as _;
    use std::net::TcpStream;

    #[test]
    fn binding_is_localhost_only() {
        let listener = Listener::bind(0).expect("an ephemeral loopback port");
        let addresses = listener.addresses();
        assert!(!addresses.is_empty(), "at least one loopback family binds");
        for address in &addresses {
            assert!(
                address.ip().is_loopback(),
                "{address} is not a loopback address"
            );
            assert_eq!(address.port(), listener.port(), "one gateway, one port");
        }
        assert_eq!(listener.policy().port, listener.port());
        assert!(
            listener.policy().allowed_origins.is_empty(),
            "v1 allows no browser origin"
        );
    }

    #[test]
    fn a_loopback_client_is_accepted() {
        let listener = Listener::bind(0).expect("bound");
        let port = listener.port();
        let accepting = listener.accept();
        let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connected");
        client.write_all(b"GET / HTTP/1.1\r\n\r\n").expect("wrote");
        let connection = accepting.recv().expect("a connection");
        assert!(connection.peer.ip().is_loopback());
        assert!(accepting.threads() >= 1);
    }
}
