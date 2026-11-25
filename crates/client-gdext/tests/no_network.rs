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

/// One Rust file read two ways, line for line: `code` with every comment and literal removed,
/// and `literals` with every comment removed and the literals kept. Both stop at the file's first
/// `#[cfg(test)]` in code: a unit-test module is the file's last item
/// (`tests/no_arithmetic.rs`'s `every_test_module_is_the_files_last_item`), and test drivers are
/// never compiled into the client.
struct Views {
    code: Vec<String>,
    literals: Vec<String>,
}

fn views(text: &str) -> Views {
    let chars: Vec<char> = text.chars().collect();
    let mut code = String::new();
    let mut literals = String::new();
    let mut at = 0_usize;
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at + 1).copied();
        let word_before = at
            .checked_sub(1)
            .and_then(|before| chars.get(before))
            .is_some_and(|before| before.is_alphanumeric() || *before == '_');
        if c == '/' && next == Some('/') {
            while chars.get(at).is_some_and(|c| *c != '\n') {
                at += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0_usize;
            loop {
                match (chars.get(at), chars.get(at + 1)) {
                    (None, _) => break,
                    (Some('/'), Some('*')) => {
                        depth += 1;
                        at += 2;
                    }
                    (Some('*'), Some('/')) => {
                        depth -= 1;
                        at += 2;
                        if depth == 0 {
                            break;
                        }
                    }
                    (Some('\n'), _) => {
                        code.push('\n');
                        literals.push('\n');
                        at += 1;
                    }
                    _ => at += 1,
                }
            }
        } else if let Some(end) = (!word_before).then(|| literal_end(&chars, at)).flatten() {
            code.push_str("\"\"");
            for index in at..end {
                let c = chars.get(index).copied().unwrap_or(' ');
                literals.push(c);
                if c == '\n' {
                    code.push('\n');
                }
            }
            at = end;
        } else {
            code.push(c);
            literals.push(c);
            at += 1;
        }
    }
    let code: Vec<String> = code.lines().map(str::to_owned).collect();
    let literals: Vec<String> = literals.lines().map(str::to_owned).collect();
    let cut = code
        .iter()
        .position(|line| line.contains("#[cfg(test)]"))
        .unwrap_or(code.len());
    Views {
        code: code.into_iter().take(cut).collect(),
        literals: literals.into_iter().take(cut).collect(),
    }
}

/// One past the end of the string, byte string, raw string or character literal at `at`, or
/// `None` when none starts there (a lifetime is not one).
fn literal_end(chars: &[char], at: usize) -> Option<usize> {
    let c = *chars.get(at)?;
    let next = chars.get(at + 1).copied();
    let (prefix, raw) = match (c, next) {
        ('b', Some('r')) => (2, true),
        ('r', Some('#' | '"')) => (1, true),
        ('b', Some('"' | '\'')) => (1, false),
        _ => (0, false),
    };
    if raw {
        let mut hashes = 0_usize;
        while chars.get(at + prefix + hashes) == Some(&'#') {
            hashes += 1;
        }
        let open = at + prefix + hashes;
        if chars.get(open) != Some(&'"') {
            return None;
        }
        let mut close = open + 1;
        while let Some(&c) = chars.get(close) {
            if c == '"' && (1..=hashes).all(|k| chars.get(close + k) == Some(&'#')) {
                return Some(close + 1 + hashes);
            }
            close += 1;
        }
        return Some(close);
    }
    let quote = at + prefix;
    match chars.get(quote) {
        Some('"') => {
            let mut close = quote + 1;
            while let Some(&c) = chars.get(close) {
                match c {
                    '\\' => close += 2,
                    '"' => return Some(close + 1),
                    _ => close += 1,
                }
            }
            Some(close)
        }
        Some('\'') => {
            if chars.get(quote + 1) == Some(&'\\') {
                (quote + 3..quote + 12)
                    .find(|i| chars.get(*i) == Some(&'\''))
                    .map(|i| i + 1)
            } else {
                (chars.get(quote + 2) == Some(&'\'')).then_some(quote + 3)
            }
        }
        _ => None,
    }
}

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

fn rust_offences(text: &str) -> Vec<String> {
    let views = views(text);
    let mut out = Vec::new();
    for (index, line) in views.code.iter().enumerate() {
        for name in RUST_NETWORK_NAMES {
            if line.contains(name) {
                out.push(format!("line {}: names `{name}`", index + 1));
            }
        }
    }
    for (index, line) in views.literals.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        for scheme in SCHEMES {
            if lower.contains(scheme) {
                out.push(format!("line {}: holds `{scheme}`", index + 1));
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

#[test]
fn the_rust_source_names_no_socket_and_no_url() {
    let mut files = Vec::new();
    collect(&crate_root().join("src"), &["rs"], &mut files);
    assert!(files.len() > 20, "the scan found the source: {files:?}");
    let mut found = Vec::new();
    for path in &files {
        // A module of tests (`src/rig/tests/`) is compiled only under `#[cfg(test)]`.
        if path.components().any(|part| part.as_os_str() == "tests") {
            continue;
        }
        let text = fs::read_to_string(path).expect("a source file");
        for offence in rust_offences(&text) {
            found.push(format!("{}: {offence}", path.display()));
        }
    }
    assert!(
        found.is_empty(),
        "the client names a network:\n{}",
        found.join("\n")
    );
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
        "#[cfg(test)]\nmod tests { use std::net::TcpStream; }",
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

/// Why the file `name`, holding `text`, breaks the no-network rule.
fn godot_offences(name: &str, text: &str) -> Vec<String> {
    let script = Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gd"));
    let comment = if script { '#' } else { ';' };
    let check = CHECKS.contains(&name);
    let mut out = Vec::new();
    for (index, line) in gd_code(text, comment).iter().enumerate() {
        let number = index + 1;
        let lower = line.to_ascii_lowercase();
        for banned in GODOT_BANNED {
            if line.contains(banned) {
                out.push(format!("line {number}: names `{banned}`"));
            }
        }
        for scheme in SCHEMES {
            if lower.contains(scheme) && !(line.contains("connect_to_url(\"ws://127.0.0.1:")) {
                out.push(format!("line {number}: holds `{scheme}`"));
            }
        }
        if line.contains("WebSocketPeer") && !(name == "host_link.gd" || check) {
            out.push(format!(
                "line {number}: a WebSocketPeer outside host_link.gd and the checks"
            ));
        }
        if line.contains("connect_to_url(")
            && !calls_with(line, "connect_to_url(", "\"ws://127.0.0.1:")
        {
            out.push(format!(
                "line {number}: connect_to_url to anything but a literal ws://127.0.0.1:"
            ));
        }
        for tcp in ["TCPServer", "StreamPeerTCP"] {
            if line.contains(tcp) && !check {
                out.push(format!("line {number}: a {tcp} outside the checks"));
            }
        }
        for method in [".listen(", "connect_to_host("] {
            if line.contains(method) && !lower.contains("\"127.0.0.1\"") {
                out.push(format!("line {number}: `{method}` off 127.0.0.1"));
            }
        }
        if line.contains("execute_with_pipe(") && name != "host_link.gd" {
            out.push(format!(
                "line {number}: a process spawned outside host_link.gd"
            ));
        }
        if line.contains("OS.execute(") && name != "watch_check.gd" {
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
