// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **The client opens no network but its localhost link to the host it spawned** (S3's `prob`,
//! part 2; the plan's E10 and R6; AGENTS.md section 7: "No network telemetry. Playtest mode is
//! opt-in and local", "do not add an HTTP client to any shipped crate"). The extension of
//! `crates/gamectl/tests/confinement.rs`'s no-HTTP rule to `crates/client-gdext` and `godot/`.
//!
//! - **Rust:** `Cargo.lock`'s closure of this crate holds no HTTP, TLS, socket or async-runtime
//!   crate, and the shipped source (comments, string literals and the trailing test module set
//!   aside) names no socket, no HTTP class and no Godot network class; no string literal holds a
//!   URL scheme.
//! - **Godot:** no script, scene, resource or project file names an HTTP, TLS, UDP, `ENet` or
//!   WebRTC class, `shell_open`, or a URL scheme. Exactly what is allowed is listed: the
//!   `WebSocketPeer` only in `host_link.gd` among shipped scripts (and in the two checks, test
//!   drivers that are never exported), every `connect_to_url` to a literal `ws://127.0.0.1:`,
//!   a `TCPServer` or `StreamPeerTCP` only in a check and only on `"127.0.0.1"`, and a process
//!   spawned only where one is today (`host_link.gd` spawns `gamectl`; `watch_check.gd` runs
//!   `gamectl verify`).
//!
//! The scanners are themselves tested on snippets that must and must not trip each rule, so a
//! scanner with a broken pattern cannot report "nothing found" for ever. A new allowance is a
//! change to this file, and so a change a reviewer reads.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

mod shipped;

use shipped::{scanned_lines, views};

const SELF: &str = "pharmakos-client-gdext";

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_root() -> PathBuf {
    crate_root().join("..").join("..")
}

// --- The resolved graph -----------------------------------------------------------------------

/// Crates that speak to a network, or exist to: HTTP clients and servers, TLS, sockets beyond
/// the standard library, async runtimes and WebSocket clients.
const NETWORK_CRATES: &[&str] = &[
    "actix-web",
    "async-io",
    "async-std",
    "attohttpc",
    "axum",
    "curl",
    "curl-sys",
    "ehttp",
    "h2",
    "h3",
    "hickory-resolver",
    "http",
    "http-body",
    "hyper",
    "isahc",
    "minreq",
    "mio",
    "native-tls",
    "openssl",
    "openssl-sys",
    "quinn",
    "reqwest",
    "rouille",
    "rustls",
    "smol",
    "socket2",
    "surf",
    "tiny_http",
    "tokio",
    "tokio-tungstenite",
    "trust-dns-resolver",
    "tungstenite",
    "ureq",
    "warp",
    "websocket",
];

/// `Cargo.lock`'s packages and their dependency names.
fn lock_graph(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut name: Option<String> = None;
    let mut dependencies: Vec<String> = Vec::new();
    let mut in_dependencies = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if let Some(package) = name.take() {
                graph.entry(package).or_default().append(&mut dependencies);
            }
            dependencies.clear();
            in_dependencies = false;
        } else if let Some(rest) = trimmed.strip_prefix("name = ") {
            name = Some(rest.trim_matches('"').to_owned());
            in_dependencies = false;
        } else if trimmed == "dependencies = [" {
            in_dependencies = true;
        } else if in_dependencies && trimmed == "]" {
            in_dependencies = false;
        } else if in_dependencies {
            let entry = trimmed.trim_end_matches(',').trim_matches('"');
            if let Some(first) = entry.split_whitespace().next() {
                dependencies.push(first.to_owned());
            }
        }
    }
    if let Some(package) = name {
        graph.entry(package).or_default().append(&mut dependencies);
    }
    graph
}

/// Every package `root` reaches, itself included. `Cargo.lock` does not tell normal from build
/// or dev edges, so this is the widest closure, which is the safe side for a ban.
fn closure(graph: &BTreeMap<String, Vec<String>>, root: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut queue = vec![root.to_owned()];
    while let Some(name) = queue.pop() {
        if seen.insert(name.clone())
            && let Some(edges) = graph.get(&name)
        {
            queue.extend(edges.iter().cloned());
        }
    }
    seen
}

fn network_crates_in(reachable: &BTreeSet<String>) -> Vec<&'static str> {
    NETWORK_CRATES
        .iter()
        .copied()
        .filter(|name| reachable.contains(*name))
        .collect()
}

#[test]
fn the_resolved_graph_holds_no_network_crate() {
    let text = fs::read_to_string(workspace_root().join("Cargo.lock")).expect("Cargo.lock");
    let graph = lock_graph(&text);
    let reachable = closure(&graph, SELF);
    assert!(
        reachable.contains("godot") && reachable.contains("pharmakos-proto"),
        "the reader found the closure: {reachable:?}"
    );
    let found = network_crates_in(&reachable);
    assert!(
        found.is_empty(),
        "`{SELF}` reaches {found:?}: the client opens no network but its localhost link, and \
         takes no HTTP client (AGENTS.md section 7). A new dependency is an owner decision \
         (AGENTS.md section 3 rule 5)."
    );
}

#[test]
fn the_lock_reader_finds_a_network_crate_two_hops_out() {
    let sample = "\
[[package]]\nname = \"pharmakos-client-gdext\"\nversion = \"0.1.0\"\ndependencies = [\n \"a\",\n]\n\n\
[[package]]\nname = \"a\"\nversion = \"1.0.0\"\ndependencies = [\n \"hyper 1.0.0\",\n]\n\n\
[[package]]\nname = \"hyper\"\nversion = \"1.0.0\"\n";
    let reachable = closure(&lock_graph(sample), SELF);
    assert_eq!(network_crates_in(&reachable), ["hyper"]);
}

// --- Rust source --------------------------------------------------------------------------------

/// Names in Rust code that open a socket or name a network class (gdext spells Godot's classes
/// in this case).
const RUST_NETWORK_NAMES: &[&str] = &[
    "std::net",
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "ToSocketAddrs",
    "SocketAddr",
    "HttpRequest",
    "HttpClient",
    "StreamPeerTcp",
    "StreamPeerTls",
    "TcpServer",
    "UdpServer",
    "PacketPeerUdp",
    "PacketPeerDtls",
    "DtlsServer",
    "WebSocketPeer",
    "WebSocketMultiplayerPeer",
    "WebRtc",
    "ENet",
    "Enet",
    "shell_open",
    "JavaScriptBridge",
];

/// URL schemes, in a string literal or in GDScript.
const SCHEMES: &[&str] = &[
    "http://", "https://", "ws://", "wss://", "ftp://", "file://",
];

/// Godot's network classes as their names are spelt in a string (`ClassDB::instantiate`'s
/// argument, a `call` by name): no Rust file of this crate names one in a literal. The one link
/// the client opens is `godot/scripts/host_link.gd`'s, in GDScript; no Rust file owns it.
const RUST_NETWORK_CLASSES: &[&str] = &[
    "HTTPRequest",
    "HTTPClient",
    "StreamPeerTCP",
    "StreamPeerTLS",
    "PacketPeerUDP",
    "UDPServer",
    "TCPServer",
    "WebSocketPeer",
    "WebRTC",
    "ENet",
];

/// A process spawned from Rust: the client spawns none (`host_link.gd` spawns `gamectl`).
/// `std::process::id()`, which names this process in a temporary file's name (`write.rs`), is
/// the one `std::process` read allowed.
const RUST_PROCESS_NAMES: &[&str] = &["Command::new", "process::Command", "Command;", "Command}"];

/// Why the Rust source `text` names a network or a process. A `#[cfg(test)]` the shared reader
/// cannot place is itself a finding: the scan never stops early at one.
fn rust_offences(text: &str) -> Vec<String> {
    let views = views(text);
    let mut out = Vec::new();
    let shipped = match scanned_lines(text) {
        Ok(shipped) => shipped,
        Err(why) => {
            out.push(why);
            views.code.len()
        }
    };
    for (index, line) in views.code.iter().enumerate().take(shipped) {
        let number = index + 1;
        for name in RUST_NETWORK_NAMES.iter().chain(RUST_PROCESS_NAMES) {
            if line.contains(name) {
                out.push(format!("line {number}: names `{name}`"));
            }
        }
        if line.contains("std::process") && !line.contains("std::process::id()") {
            out.push(format!(
                "line {number}: names `std::process` other than `id()`"
            ));
        }
    }
    for (index, line) in views.literals.iter().enumerate().take(shipped) {
        let number = index + 1;
        let lower = line.to_ascii_lowercase();
        for scheme in SCHEMES {
            if lower.contains(scheme) {
                out.push(format!("line {number}: holds `{scheme}`"));
            }
        }
        for class in RUST_NETWORK_CLASSES {
            if line.contains(class) {
                out.push(format!("line {number}: names `{class}` in a string"));
            }
        }
    }
    out
}

fn collect(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.expect("an entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if name != ".godot" {
                collect(&path, extensions, out);
            }
        } else if extensions
            .iter()
            .any(|extension| name.ends_with(&format!(".{extension}")) || name == *extension)
        {
            out.push(path);
        }
    }
}

/// The files under `src/` that are a trailing test module's children (`mod faults;` inside
/// `rig.rs`'s `#[cfg(test)] mod tests`), compiled only under `#[cfg(test)]`: each named, and
/// each held to its parent's test module by `the_rust_scan_reads_every_shipped_file`.
const TEST_CHILDREN: &[(&str, &str, &str)] = &[("rig/tests/faults.rs", "rig.rs", "mod faults;")];

/// A path under `src/`, with forward slashes.
fn src_relative(path: &Path) -> String {
    path.strip_prefix(crate_root().join("src"))
        .expect("under src")
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[test]
fn the_rust_source_names_no_socket_and_no_url() {
    let mut files = Vec::new();
    collect(&crate_root().join("src"), &["rs"], &mut files);
    let mut found = Vec::new();
    let mut scanned = Vec::new();
    for path in &files {
        let relative = src_relative(path);
        if TEST_CHILDREN.iter().any(|(child, _, _)| *child == relative) {
            continue;
        }
        let text = fs::read_to_string(path).expect("a source file");
        for offence in rust_offences(&text) {
            found.push(format!("{}: {offence}", path.display()));
        }
        scanned.push(relative);
    }
    assert!(
        found.is_empty(),
        "the client names a network:\n{}",
        found.join("\n")
    );
    // Every file under `src/` is scanned, but the named test children; and every module
    // `lib.rs` declares is one of the scanned files.
    assert_eq!(
        scanned.len() + TEST_CHILDREN.len(),
        files.len(),
        "a file under src/ was neither scanned nor a named test child: {scanned:?}"
    );
    let lib = fs::read_to_string(crate_root().join("src").join("lib.rs")).expect("lib.rs");
    let declared: Vec<String> = lib
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("pub mod ")
                .or_else(|| line.strip_prefix("mod "))
                .and_then(|rest| rest.strip_suffix(';'))
                .map(|name| format!("{name}.rs"))
        })
        .collect();
    assert!(
        declared.len() > 20,
        "the reader found lib.rs's modules: {declared:?}"
    );
    for module in &declared {
        assert!(
            scanned.contains(module),
            "lib.rs declares `{module}`, and the scan did not read it"
        );
    }
}

/// Each named test child is declared inside its parent's trailing test module, so skipping it
/// skips only test code.
#[test]
fn the_rust_scan_reads_every_shipped_file() {
    for (child, parent, declaration) in TEST_CHILDREN {
        let text = fs::read_to_string(crate_root().join("src").join(parent)).expect("the parent");
        let shipped = scanned_lines(&text).expect("the parent ends in its test module");
        let module: Vec<&str> = text.lines().skip(shipped).collect();
        assert!(
            module.iter().any(|line| line.trim() == *declaration),
            "{child} is not declared in {parent}'s trailing test module"
        );
        assert!(
            !text
                .lines()
                .take(shipped)
                .any(|line| line.trim() == *declaration),
            "{parent} declares {child} in its shipped code"
        );
    }
}

#[test]
fn the_rust_scanner_catches_what_it_is_for() {
    for snippet in [
        "use std::net::TcpStream;",
        "let s = std::net::TcpStream::connect(a);",
        "let l = TcpListener::bind(a);",
        "let u = UdpSocket::bind(a);",
        "let r = godot::classes::HttpRequest::new_alloc();",
        "let p = WebSocketPeer::new_gd();",
        "let c = StreamPeerTcp::new_gd();",
        "godot::classes::Os::singleton().shell_open(u);",
        "const AT: &str = \"https://example.invalid/upload\";",
        "let u = format!(\"http://{host}/\");",
        "let r = r#\"wss://x\"#;",
        "let b = b\"ftp://x\";",
        "/* a */ let s = std::net::TcpStream::connect(a);",
        // Review 1, M12c: an attribute on one item hides nothing after it.
        "#[cfg(test)] const _PROBE: () = ();
fn _probe() { let _ = std::net::TcpStream::connect(\"127.0.0.1:1\"); }",
        "fn a() {}
#[cfg(test)]
fn helper() {}
fn b() { let _ = std::net::UdpSocket::bind(x); }",
        // Review 2's R7: a test module ending at an indented brace hides nothing after it.
        "fn a() {}\n#[cfg(test)]\nmod tests {\n    #![rustfmt::skip]\n    }\nfn evil() {\n    let _ = std::net::TcpStream::connect(\"x\");\n}\n",
        "let child = std::process::Command::new(\"gamectl\");",
        "use std::process::{self, Command};",
        "let c = Command::new(path);",
        "let node = ClassDB::singleton().instantiate(\"HTTPRequest\");",
        "let peer = object.call(\"x\", &[\"WebSocketPeer\".to_variant()]);",
        "let name = \"PacketPeerUDP\";",
    ] {
        assert!(
            !rust_offences(snippet).is_empty(),
            "the scanner let through: {snippet}"
        );
    }
    for snippet in [
        "// The gateway listens on TcpListener; this client never does.",
        "/// `WebSocketPeer.send_text` refuses a frame past its buffer.",
        "/* http://example.invalid */ let x = 1;",
        "let words = \"the TcpStream is the host's\";",
        "fn f<'a>(x: &'a str) -> &'a str { x }",
        "let quote = '\"'; let net = 1;",
        "fn a() {}\n#[cfg(test)]\nmod tests {\n    use std::net::TcpStream;\n}\n",
        "let id = std::process::id();",
        "let words = \"the generated engine\";",
    ] {
        assert!(
            rust_offences(snippet).is_empty(),
            "the scanner tripped on: {snippet}: {:?}",
            rust_offences(snippet)
        );
    }
}

// --- Godot --------------------------------------------------------------------------------------

/// Names no file under `godot/` may hold, in code or in a string: HTTP, TLS, UDP, `ENet`, `WebRTC`,
/// the multiplayer peers, the browser bridge and the shell.
const GODOT_BANNED: &[&str] = &[
    "HTTPRequest",
    "HTTPClient",
    "StreamPeerTLS",
    "PacketPeerUDP",
    "PacketPeerDTLS",
    "DTLSServer",
    "UDPServer",
    "ENetConnection",
    "ENetMultiplayerPeer",
    "ENetPacketPeer",
    "WebRTC",
    "WebSocketMultiplayerPeer",
    "MultiplayerPeer",
    "JavaScriptBridge",
    "shell_open",
    "create_process",
];

/// The two checks: test drivers, never exported (`export_presets.cfg`'s `exclude_filter`).
const CHECKS: &[&str] = &["client_check.gd", "watch_check.gd"];

/// GDScript, or a scene, resource or project file, with each `#` or `;` comment removed outside
/// a string, line for line.
fn gd_code(text: &str, comment: char) -> Vec<String> {
    text.lines()
        .map(|line| {
            let mut quote: Option<char> = None;
            let mut escaped = false;
            let mut out = String::new();
            for c in line.chars() {
                match quote {
                    Some(_) if escaped => escaped = false,
                    Some(_) if c == '\\' => escaped = true,
                    Some(open) if c == open => quote = None,
                    None if c == '"' || c == '\'' => quote = Some(c),
                    None if c == comment => break,
                    Some(_) | None => {}
                }
                out.push(c);
            }
            out
        })
        .collect()
}

/// Whether `line`'s call of `method` carries `wanted` as its first argument's opening text.
fn calls_with(line: &str, method: &str, wanted: &str) -> bool {
    line.match_indices(method).all(|(at, _)| {
        line.get(at + method.len()..)
            .is_some_and(|rest| rest.trim_start().starts_with(wanted))
    })
}

/// Methods whose names, said as a string, call a network or a process by name (`call`,
/// `callv`, `call_deferred`, a `Callable`): no file under `godot/` says one in a string.
const NAMED_CALLS: &[&str] = &[
    "\"connect_to_",
    "\"listen\"",
    "\"execute\"",
    "\"execute_with_pipe\"",
    "\"shell_open\"",
    "\"create_process\"",
    "\"instantiate\"",
    "\"put_packet\"",
];

/// Why the file `name`, holding `text`, breaks the no-network rule. Each line is read with its
/// comment removed and with every space and tab removed, so no spacing before a parenthesis or
/// around a dot hides a call.
fn godot_offences(name: &str, text: &str) -> Vec<String> {
    let script = Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gd"));
    let comment = if script { '#' } else { ';' };
    let check = CHECKS.contains(&name);
    let mut out = Vec::new();
    for (index, line) in gd_code(text, comment).iter().enumerate() {
        let number = index + 1;
        let squeezed: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        let lower = squeezed.to_ascii_lowercase();
        for banned in GODOT_BANNED.iter().chain(["ClassDB.instantiate"].iter()) {
            if squeezed.contains(banned) {
                out.push(format!("line {number}: names `{banned}`"));
            }
        }
        for named in NAMED_CALLS {
            if squeezed.contains(named) {
                out.push(format!("line {number}: calls `{named}` by name"));
            }
        }
        for scheme in SCHEMES {
            if lower.contains(scheme) && !squeezed.contains("connect_to_url(\"ws://127.0.0.1:") {
                out.push(format!("line {number}: holds `{scheme}`"));
            }
        }
        if squeezed.contains("WebSocketPeer") && !(name == "host_link.gd" || check) {
            out.push(format!(
                "line {number}: a WebSocketPeer outside host_link.gd and the checks"
            ));
        }
        if squeezed.contains("connect_to_url")
            && !(squeezed.contains("connect_to_url(")
                && calls_with(&squeezed, "connect_to_url(", "\"ws://127.0.0.1:"))
        {
            out.push(format!(
                "line {number}: connect_to_url to anything but a literal ws://127.0.0.1:"
            ));
        }
        for tcp in ["TCPServer", "StreamPeerTCP"] {
            if squeezed.contains(tcp) && !check {
                out.push(format!("line {number}: a {tcp} outside the checks"));
            }
        }
        for method in [".listen(", "connect_to_host"] {
            if squeezed.contains(method) && !lower.contains("\"127.0.0.1\"") {
                out.push(format!("line {number}: `{method}` off 127.0.0.1"));
            }
        }
        if squeezed.contains("execute_with_pipe") && name != "host_link.gd" {
            out.push(format!(
                "line {number}: a process spawned outside host_link.gd"
            ));
        }
        if squeezed.contains("OS.execute(") && name != "watch_check.gd" {
            out.push(format!(
                "line {number}: a process run outside watch_check.gd"
            ));
        }
    }
    out
}

#[test]
fn the_godot_project_opens_no_network_but_the_hosts_link() {
    let mut files = Vec::new();
    collect(
        &workspace_root().join("godot"),
        &["gd", "tscn", "tres", "cfg", "godot", "gdextension"],
        &mut files,
    );
    let scripts = files
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "gd"))
        .count();
    assert!(scripts > 20, "the scan found the scripts: {files:?}");
    let mut found = Vec::new();
    for path in &files {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let text = fs::read_to_string(path).expect("a file");
        for offence in godot_offences(name, &text) {
            found.push(format!("{}: {offence}", path.display()));
        }
    }
    assert!(
        found.is_empty(),
        "godot/ names a network beyond the host's localhost link:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_godot_scanner_catches_what_it_is_for() {
    for (name, snippet) in [
        ("playtest.gd", "var request := HTTPRequest.new()"),
        ("playtest.gd", "var client := HTTPClient.new()"),
        ("lobby.gd", "var peer := PacketPeerUDP.new()"),
        ("lobby.gd", "var tls := StreamPeerTLS.new()"),
        ("lobby.gd", "var server := UDPServer.new()"),
        ("lobby.gd", "var e := ENetMultiplayerPeer.new()"),
        ("lobby.gd", "var r := WebRTCPeerConnection.new()"),
        ("lobby.gd", "OS.shell_open(\"x\")"),
        ("lobby.gd", "var at := \"https://example.invalid\""),
        ("lobby.gd", "var at := \"http://127.0.0.1:80\""),
        ("lobby.gd", "var peer := WebSocketPeer.new()"),
        (
            "host_link.gd",
            "peer.connect_to_url(\"ws://10.0.0.1:%d/\" % port)",
        ),
        ("host_link.gd", "peer.connect_to_url(url)"),
        (
            "host_link.gd",
            "peer.connect_to_url(\"wss://127.0.0.1:1/\")",
        ),
        ("lobby.gd", "var server := TCPServer.new()"),
        ("lobby.gd", "var tcp := StreamPeerTCP.new()"),
        ("watch_check.gd", "server.listen(0, \"0.0.0.0\")"),
        ("watch_check.gd", "server.listen(0)"),
        ("watch_check.gd", "tcp.connect_to_host(\"192.168.0.1\", 1)"),
        ("lobby.gd", "OS.execute_with_pipe(path, [])"),
        ("lobby.gd", "OS.execute(path, [])"),
        ("lobby.gd", "OS.create_process(path, [])"),
        (
            "lobby.tscn",
            "[node name=\"Http\" type=\"HTTPRequest\" parent=\".\"]",
        ),
        // Review 1's follow-up: spacing, a name built in a string, a call by name.
        ("lobby.gd", "OS.execute (path, [])"),
        ("lobby.gd", "OS . shell_open (\"x\")"),
        ("lobby.gd", "OS.create_process\t(p, [])"),
        (
            "lobby.gd",
            "var r: Node = ClassDB.instantiate(\"HTTP\" + \"Request\")",
        ),
        ("lobby.gd", "var r: Variant = ClassDB . instantiate (kind)"),
        ("host_link.gd", "peer.call(\"connect_to_url\", url)"),
        (
            "host_link.gd",
            "peer.callv(\"connect_to_host\", [host, port])",
        ),
        (
            "host_link.gd",
            "var go := Callable(peer, \"connect_to_url\")",
        ),
        ("host_link.gd", "peer.connect_to_url (\"ws://10.0.0.1:1/\")"),
        ("host_link.gd", "var go := peer.connect_to_url"),
        ("watch_check.gd", "server.listen (0, \"0.0.0.0\")"),
        ("lobby.gd", "node.call_deferred(\"execute\", path)"),
    ] {
        assert!(
            !godot_offences(name, snippet).is_empty(),
            "the scanner let through, in {name}: {snippet}"
        );
    }
    for (name, snippet) in [
        (
            "host_link.gd",
            "var error := peer.connect_to_url(\"ws://127.0.0.1:%d/\" % _port)",
        ),
        ("host_link.gd", "var peer := WebSocketPeer.new()"),
        ("host_link.gd", "OS.execute_with_pipe(gamectl, args, false)"),
        (
            "host_link.gd",
            "var error := peer.connect_to_url (\"ws://127.0.0.1:%d/\" % _port)",
        ),
        (
            "client_check.gd",
            "if not ClassDB.class_exists(\"PharmakosBridge\"):",
        ),
        (
            "watch_check.gd",
            "if server.listen(0, \"127.0.0.1\") != OK:",
        ),
        ("watch_check.gd", "var held: Array[StreamPeerTCP] = []"),
        (
            "watch_check.gd",
            "var code := OS.execute(_gamectl, args, output, true)",
        ),
        ("client_check.gd", "var server := TCPServer.new()"),
        ("lobby.gd", "# No HTTPRequest here, and no https:// either."),
        ("lobby.gd", "var hash := \"#\" # a HTTPClient in a comment"),
        ("project.godot", "; HTTPRequest in a comment"),
    ] {
        assert!(
            godot_offences(name, snippet).is_empty(),
            "the scanner tripped, in {name}: {snippet}: {:?}",
            godot_offences(name, snippet)
        );
    }
}
