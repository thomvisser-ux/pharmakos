// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl match`: the external seat, one seat of a headless match played by
//! a program outside the game (decisions-log item 231).
//!
//! **Experimental.** These commands, their output and the bridge's framing
//! may change in any release until v1.1 publishes the gateway (AGENTS.md
//! section 2).
//!
//! # What a program outside gets, and what it does not
//!
//! It gets a seat: `gp.api.v1`'s methods under a seat's scopes (`observe`,
//! `plan`, `plan.submit`, `docs`), fog-filtered, counted against a Lull's call
//! budget and written to the audit log, exactly as a socket's calls are. It
//! gets **no token**: the token is minted inside the gateway
//! (`pharmakos_gateway::serve::External`), which has no accessor for it, so
//! none is in this crate, on a command line, on a pipe or on the disk. It gets
//! no socket either; nothing here listens. And it gets no `admin`: the match
//! moves by the host's own hand, after the seat's `set_ready`, and by nothing
//! the seat can call.
//!
//! The game never starts, calls or pays for the program. The program starts
//! `gamectl`.
//!
//! # A match is a folder
//!
//! `match new --dir <dir>` makes the folder and starts the match in it: the
//! gateway's private match cache (the saves, the sealed playbooks, the replay
//! chains, the audit log) and one file of this crate's, [`LINE_FILE`], the
//! config line the match was started with, which every later command reads to
//! take the match up again. The line holds the match's seven values and no
//! token. One folder holds one match, and a harness that keeps its matches in
//! folders of its own never touches the matches the lobby keeps.
//!
//! **The folder is not secret from the program that plays it.** It holds the
//! other seats' sealed playbooks and the match's saves in plain files, as the
//! lobby's cache does (AGENTS.md section 7: enforcement, not cryptography). A
//! program that reads the folder has read what its seat is not told; results
//! from such a run are development evidence and never proof.
//!
//! **One command at a time.** A command that takes a match up makes
//! [`LOCK_FILE`] in its folder and removes it as it puts the match down, and
//! a command that finds the file there is refused: two processes over one
//! save would each write what the other had not read. A process that is
//! killed leaves the file behind; the refusal names it, and removing it by
//! hand is safe once no `gamectl` is running on the folder (the saves are
//! written whole, at boundaries, so what is on disk is a match to take up).
//!
//! # The commands
//!
//! * `new` starts the match and prints where it stands (`get_status`).
//! * `call <method> [--params <file>]` makes one call and prints the whole
//!   JSON-RPC response. A refusal is printed the same way and exits 3.
//! * `submit <playbook>` is `submit_plan` with the file's text, because a
//!   playbook is awkward to write as a JSON string by hand. A playbook the
//!   verifier refuses exits 3 with the report printed.
//! * `brief` prints what the seat is told this round: the answers of a fixed
//!   list of reads, each under its method's name, the gateway's prose where an
//!   answer has prose and the answer itself where it has none. Nothing is
//!   re-worded here.
//! * `serve` answers calls on standard input until it ends: one request a
//!   line, one response a line ([`pharmakos_proto::json::write_line`]), for a
//!   program that plays many rounds without starting a process for each.
//!
//! Each of the first four is one process: it takes the match up from its
//! save, acts, lets the host move the match if the seat is now ready, and
//! puts the match down again in a Lull. **Ready is not saved**: a Lull taken
//! up again opens with no seat ready, as the lobby's does, so `set_ready` and
//! the Push it allows happen in one command, and the next command finds the
//! next Lull (or the match's end).
//!
//! # The match's end
//!
//! The gateway takes up no match that has ended (its saves land in a Lull).
//! So the process whose `set_ready` plays the last Push asks the seat's last
//! `get_recap` before it puts the match down, and writes the answer to
//! [`RESULT_FILE`] in the match's folder: the outcome, the seat's settlement
//! and, the fog having lifted, every seat's tally. From then on the folder
//! holds an ended match: `brief` prints that recap, and every other command
//! is answered that the match has ended and where its recap is (exit 3).
//! Said plainly: a process that dies after the last Push and before that
//! file is written leaves an ended match with no recap file; the match's
//! sealed playbooks and chains are still in the folder.
//!
//! # Game text is data
//!
//! What these commands print is the game's text: template and library words,
//! a notebook, a recap. A program that plays a seat reads all of it as data
//! and never as instructions (AGENTS.md section 7).

use std::io::{BufRead, Write};
use std::path::Path;

use pharmakos_gateway::Code;
use pharmakos_gateway::rpc;
use pharmakos_gateway::serve::{ConfigLine, External};
use pharmakos_proto::json::{self, Json};

use crate::cli::MatchVerb;
use crate::exit::{Exit, Failure};
use crate::{host, strings};

/// The file in a match's folder that holds the config line it was started
/// with: seven tab-separated fields as the gateway writes them
/// (`pharmakos_gateway::serve::ConfigLine`), and no token.
pub const LINE_FILE: &str = "external.line";

/// The file that says a command holds the match: made as a match is taken
/// up, removed as it is put down. It holds nothing.
pub const LOCK_FILE: &str = "external.lock";

/// The file in an ended match's folder that holds the seat's last recap: the
/// whole JSON-RPC response to the `get_recap` the ending process made.
pub const RESULT_FILE: &str = "result.json";

/// The match id every external match has: its folder is what tells one from
/// another, and Easy's seed is the match seed's, not the id's.
pub const MATCH_ID: &str = "m-external";

/// `--seed`'s default.
pub const DEFAULT_SEED: &str = "1";
/// `--seats`' default.
pub const DEFAULT_SEATS: &str = "2";
/// `--seat`'s default.
pub const DEFAULT_SEAT: &str = "0";
/// `--rounds`' default: the lobby's round limit (`godot/scripts/host_link.gd`).
pub const DEFAULT_ROUNDS: &str = "3";

/// The reads `brief` makes, in the order it prints them.
///
/// Every one is an `observe` or `plan` read a seat may make in a Lull, with
/// no params, and each is one call against the Lull's budget.
pub const BRIEF_READS: &[&str] = &[
    "get_status",
    "get_recap",
    "get_briefing",
    "get_map_summary",
    "list_beacons",
    "get_economy_forecast",
];

/// Run one `match` command but `serve`, which owns the process's standard
/// input and output ([`serve`]).
///
/// # Errors
///
/// [`Exit::Input`] for a folder that holds no match (or, for `new`, one that
/// already does), a file that cannot be read and a match the gateway refuses;
/// [`Exit::Failed`], with the answer on standard output, for a call the
/// gateway refuses and a playbook the verifier does; [`Exit::Internal`] when
/// the host stops on a write it could not make.
pub fn run(root: &Path, dir: &Path, verb: &MatchVerb) -> Result<String, Failure> {
    match verb {
        MatchVerb::New {
            seed,
            seats,
            seat,
            rounds,
        } => {
            let line = start_line(
                seed.as_deref(),
                seats.as_deref(),
                seat.as_deref(),
                rounds.as_deref(),
            )?;
            let mut session = Session::start(root, dir, &line)?;
            let answer = session.ask("get_status", empty_params());
            session.put_down()?;
            answered(&answer)
        }
        MatchVerb::Call { method, params } => {
            let params = match params {
                Some(path) => read_params(path)?,
                None => empty_params(),
            };
            let mut session = Session::take_up(root, dir)?;
            let answer = session.ask(method, params);
            session.advance()?;
            session.put_down()?;
            answered(&answer)
        }
        MatchVerb::Submit { path } => {
            let text = std::fs::read_to_string(path).map_err(|error| {
                Failure::input(strings::unreadable(
                    "playbook",
                    &crate::display(path),
                    &error.to_string(),
                ))
            })?;
            let mut session = Session::take_up(root, dir)?;
            let answer = session.ask(
                "submit_plan",
                Json::Object(vec![(String::from("playbook_jsonc"), Json::String(text))]),
            );
            session.put_down()?;
            let accepted = matches!(
                answer
                    .get("result")
                    .and_then(|result| result.get("accepted")),
                Some(Json::Bool(true))
            );
            if accepted {
                Ok(json::write(&answer))
            } else {
                Err(Failure::answered(Exit::Failed, json::write(&answer)))
            }
        }
        MatchVerb::Brief => {
            if let Some(recap) = ended_recap(dir)? {
                let mut text = strings::match_brief_heading("ended");
                text.push('\n');
                text.push_str(&strings::match_ended(&crate::display(
                    &dir.join(RESULT_FILE),
                )));
                text.push_str("\n\n");
                text.push_str(&strings::match_brief_heading("get_recap"));
                text.push('\n');
                text.push_str(brief_body(&recap).trim_end_matches('\n'));
                text.push('\n');
                return Ok(text);
            }
            let mut session = Session::take_up(root, dir)?;
            let mut text = String::new();
            for method in BRIEF_READS {
                let answer = session.ask(method, empty_params());
                text.push_str(&strings::match_brief_heading(method));
                text.push('\n');
                text.push_str(brief_body(&answer).trim_end_matches('\n'));
                text.push_str("\n\n");
            }
            session.put_down()?;
            Ok(text)
        }
        MatchVerb::Serve => Err(Failure::internal(
            "`match serve` owns the process's standard input and output and is not rendered",
        )),
    }
}

/// `match serve`: answer the seat's calls from `input` on `output`, one line
/// each, until `input` ends, and then put the match down.
///
/// A line is one JSON-RPC request, at most
/// `pharmakos_gateway::frame::MAX_MESSAGE_BYTES` long; its line ending, `\n`
/// or `\r\n`, is not part of it. Every line is answered, in order, by one
/// line: a line that is not a request is answered under a null id, a line too
/// long is answered so without being held in memory past the cap, and neither
/// ends the bridge. After each answer is written the host moves the match if
/// the seat is now ready, so the line after a `set_ready` is answered in the
/// next Lull, or at the match's end.
///
/// # Errors
///
/// As [`run`]'s for the match; [`Exit::Internal`] when standard input cannot
/// be read or standard output cannot be written, after the match has been put
/// down as far as it can be.
pub fn serve<R: BufRead, W: Write>(
    root: &Path,
    dir: &Path,
    mut input: R,
    mut output: W,
) -> Result<String, Failure> {
    let mut session = Session::take_up(root, dir)?;
    let cap = pharmakos_gateway::frame::MAX_MESSAGE_BYTES;
    let pipe = loop {
        let line = match read_line(&mut input, cap) {
            Ok(Some(line)) => line,
            Ok(None) => break Ok(()),
            Err(error) => break Err(strings::match_pipe("reading standard input", &error)),
        };
        let answer = session.external.call_bytes(&line);
        let mut text = json::write_line(&answer);
        text.push('\n');
        if let Err(error) = output
            .write_all(text.as_bytes())
            .and_then(|()| output.flush())
        {
            break Err(strings::match_pipe("writing standard output", &error));
        }
        session.advance()?;
    };
    session.put_down()?;
    pipe.map_err(Failure::internal)?;
    Ok(String::new())
}

/// One line of `input`, without its line ending, or `None` at end of input.
///
/// At most `cap + 1` bytes of a line are kept: a longer line is read to its
/// end and thrown away past that, so what is handed on is longer than any
/// request may be and is refused as such, at a bounded cost in memory.
fn read_line<R: BufRead>(input: &mut R, cap: usize) -> std::io::Result<Option<Vec<u8>>> {
    let keep = cap.saturating_add(1);
    let mut line: Vec<u8> = Vec::new();
    let mut any = false;
    loop {
        let buffer = match input.fill_buf() {
            Ok(buffer) => buffer,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if buffer.is_empty() {
            return Ok(any.then_some(without_carriage_return(line)));
        }
        any = true;
        let ended = buffer.iter().position(|byte| *byte == b'\n');
        // The line's bytes in this buffer: up to its line feed, or all of it.
        let (upto, consumed) = match ended {
            Some(at) => (at, at.saturating_add(1)),
            None => (buffer.len(), buffer.len()),
        };
        let room = keep.saturating_sub(line.len());
        line.extend(buffer.iter().take(upto).take(room).copied());
        input.consume(consumed);
        if ended.is_some() {
            return Ok(Some(without_carriage_return(line)));
        }
    }
}

fn without_carriage_return(mut line: Vec<u8>) -> Vec<u8> {
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    line
}

/// The lock on a match's folder, held for as long as this value lives.
struct Held {
    path: std::path::PathBuf,
}

impl Held {
    /// Make the lock file, or refuse because it is there.
    fn take(dir: &Path) -> Result<Held, Failure> {
        let path = dir.join(LOCK_FILE);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => Ok(Held { path }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(Failure::input(
                strings::match_in_use(&crate::display(&path)),
            )),
            Err(error) => Err(Failure::input(strings::match_folder(
                &crate::display(dir),
                &error.to_string(),
            ))),
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // A lock that will not go is said, on the one stream a drop has: the
        // next command would otherwise be refused with no reason on record.
        if let Err(error) = std::fs::remove_file(&self.path) {
            let _unsaid = writeln!(
                std::io::stderr(),
                "{}",
                strings::match_lock_left(&crate::display(&self.path), &error.to_string())
            );
        }
    }
}

/// One process's hold on a match: taken up from its folder, and put down
/// again.
struct Session {
    external: External,
    /// The match's folder.
    dir: std::path::PathBuf,
    unsaid: std::sync::Arc<host::Unsaid>,
    /// The folder's lock. Fields drop in the order they are declared, so the
    /// match (above) is gone before the lock is.
    _held: Held,
    /// The id of the next call this process makes for the caller.
    next_id: u64,
}

impl Session {
    /// Start a new match in `dir`, which must hold none.
    fn start(root: &Path, dir: &Path, line: &ConfigLine) -> Result<Session, Failure> {
        let path = dir.join(LINE_FILE);
        if path.exists() {
            return Err(Failure::input(strings::match_exists(&crate::display(dir))));
        }
        let (setup, unsaid) = host::setup(root)?;
        std::fs::create_dir_all(dir).map_err(|error| {
            Failure::input(strings::match_folder(
                &crate::display(dir),
                &error.to_string(),
            ))
        })?;
        let held = Held::take(dir)?;
        // The line first, and never over one that is there: a folder with a
        // line is a match, and a second `new` racing this one loses here.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                Failure::input(strings::match_folder(
                    &crate::display(dir),
                    &error.to_string(),
                ))
            })?;
        let written = file
            .write_all(line.render().as_bytes())
            .and_then(|()| file.sync_all());
        drop(file);
        let opened = match written {
            Ok(()) => External::open(setup, dir, line).map_err(|error| refused(&error, &unsaid)),
            Err(error) => Err(Failure::input(strings::match_folder(
                &crate::display(dir),
                &error.to_string(),
            ))),
        };
        match opened {
            Ok(external) => Ok(Session {
                external,
                dir: dir.to_path_buf(),
                unsaid,
                _held: held,
                next_id: 1,
            }),
            Err(failure) => {
                // No match was started: the line goes, so the folder is not
                // read as one. A line that will not go is said with the
                // failure, never left unsaid.
                match std::fs::remove_file(&path) {
                    Ok(()) => Err(failure),
                    Err(error) => Err(Failure::new(
                        failure.code,
                        strings::match_line_left(
                            &failure.message,
                            &crate::display(&path),
                            &error.to_string(),
                        ),
                    )),
                }
            }
        }
    }

    /// Take up the match `dir` holds.
    fn take_up(root: &Path, dir: &Path) -> Result<Session, Failure> {
        if ended_recap(dir)?.is_some() {
            return Err(Failure::failed(strings::match_ended(&crate::display(
                &dir.join(RESULT_FILE),
            ))));
        }
        let path = dir.join(LINE_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Failure::input(strings::match_missing(&crate::display(dir))));
            }
            Err(error) => {
                return Err(Failure::input(strings::unreadable(
                    "match's config line",
                    &crate::display(&path),
                    &error.to_string(),
                )));
            }
        };
        let mut line = ConfigLine::parse(&text).map_err(|error| {
            Failure::input(strings::unreadable(
                "match's config line",
                &crate::display(&path),
                &error.to_string(),
            ))
        })?;
        line.resume = true;
        let (setup, unsaid) = host::setup(root)?;
        let held = Held::take(dir)?;
        let external =
            External::open(setup, dir, &line).map_err(|error| refused(&error, &unsaid))?;
        Ok(Session {
            external,
            dir: dir.to_path_buf(),
            unsaid,
            _held: held,
            next_id: 1,
        })
    }

    /// One call for the caller, under this process's own running id.
    fn ask(&mut self, method: &str, params: Json) -> Json {
        let id = Json::Number(self.next_id.to_string());
        self.next_id = self.next_id.saturating_add(1);
        self.external.call(&rpc::request_text(&id, method, params))
    }

    /// Let the host move the match, if the seat is ready; and when that ends
    /// the match, keep the seat's last recap (the module docs, "The match's
    /// end").
    fn advance(&mut self) -> Result<(), Failure> {
        self.external
            .advance()
            .map_err(|error| stopped(&error, &self.unsaid))?;
        let path = self.dir.join(RESULT_FILE);
        if !self.external.ended() || path.exists() {
            return Ok(());
        }
        let recap = self.ask("get_recap", empty_params());
        std::fs::write(&path, json::write(&recap)).map_err(|error| {
            Failure::internal(strings::match_result_unwritten(
                &crate::display(&path),
                &error.to_string(),
            ))
        })
    }

    /// Put the match down, and say what Easy's seats could not.
    fn put_down(self) -> Result<(), Failure> {
        let Session {
            external,
            unsaid,
            _held: held,
            ..
        } = self;
        let closed = external.close().map_err(|error| stopped(&error, &unsaid));
        // The match is down, as far as it could be: the lock goes now.
        drop(held);
        closed?;
        host::say_unsaid_at_close(&mut std::io::stderr(), &unsaid);
        Ok(())
    }
}

/// The config line `match new` starts a match with, read by the gateway's own
/// reader: a value it refuses is refused in its words.
fn start_line(
    seed: Option<&str>,
    seats: Option<&str>,
    seat: Option<&str>,
    rounds: Option<&str>,
) -> Result<ConfigLine, Failure> {
    let fields = [
        ("--seed", seed.unwrap_or(DEFAULT_SEED)),
        ("--seats", seats.unwrap_or(DEFAULT_SEATS)),
        ("--seat", seat.unwrap_or(DEFAULT_SEAT)),
        ("--rounds", rounds.unwrap_or(DEFAULT_ROUNDS)),
    ];
    // A tab or a line ending in a value would be another field or another
    // line of the config line, and `-` is how the line writes "no seat".
    for (flag, value) in fields {
        if value.is_empty() || value == "-" || value.contains(['\t', '\r', '\n']) {
            return Err(Failure::usage(strings::match_bad_value(flag, value)));
        }
    }
    let [(_, seed), (_, seats), (_, seat), (_, rounds)] = fields;
    let text = format!("{MATCH_ID}\t{seed}\t{seats}\t{seat}\t-\t{rounds}\tcustom\n");
    ConfigLine::parse(&text)
        .map_err(|error| Failure::usage(strings::match_refused(&error.to_string())))
}

/// The seat's last recap, when `dir` holds a match that has ended.
fn ended_recap(dir: &Path) -> Result<Option<Json>, Failure> {
    let path = dir.join(RESULT_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Failure::input(strings::unreadable(
                "match's last recap",
                &crate::display(&path),
                &error.to_string(),
            )));
        }
    };
    json::read(&text).map(Some).map_err(|error| {
        Failure::input(strings::unreadable(
            "match's last recap",
            &crate::display(&path),
            &error.to_string(),
        ))
    })
}

/// A call's params, read from a file: one JSON object.
fn read_params(path: &Path) -> Result<Json, Failure> {
    let unreadable = |why: &str| {
        Failure::input(strings::unreadable(
            "params file",
            &crate::display(path),
            why,
        ))
    };
    let text = std::fs::read_to_string(path).map_err(|error| unreadable(&error.to_string()))?;
    match json::read(&text) {
        Ok(Json::Object(members)) => Ok(Json::Object(members)),
        Ok(_) => Err(unreadable("a call's params are one JSON object")),
        Err(error) => Err(unreadable(&error.to_string())),
    }
}

fn empty_params() -> Json {
    Json::Object(Vec::new())
}

/// A response as a command prints it: the whole of it, and exit 3 when it is
/// a refusal, so a script branches on the code and reads the reason.
fn answered(answer: &Json) -> Result<String, Failure> {
    if answer.get("error").is_some() {
        Err(Failure::answered(Exit::Failed, json::write(answer)))
    } else {
        Ok(json::write(answer))
    }
}

/// What `brief` prints under a read's heading: the refusal's own message
/// where the read was refused; otherwise the answer's own `prose`, where it
/// has one, and then the rest of the answer as the gateway wrote it.
///
/// Two members are left out of "the rest": `prose`, which was just printed,
/// and the `_status` footer every answer repeats, which `get_status` (the
/// first read) prints as its own `status`. Nothing else is left out, and
/// nothing is re-worded.
fn brief_body(answer: &Json) -> String {
    if let Some(error) = answer.get("error") {
        return match error.get("message") {
            Some(Json::String(message)) => strings::match_brief_refused(message),
            _ => json::write(error),
        };
    }
    let Some(Json::Object(members)) = answer.get("result") else {
        return json::write(answer);
    };
    let mut text = String::new();
    if let Some((_, Json::String(prose))) = members.iter().find(|(key, _)| key == "prose") {
        text.push_str(prose);
        text.push('\n');
    }
    let rest: Vec<(String, Json)> = members
        .iter()
        .filter(|(key, _)| key != "prose" && key != "_status")
        .cloned()
        .collect();
    if !rest.is_empty() {
        text.push_str(&json::write(&Json::Object(rest)));
    }
    text
}

/// A match the gateway would not open.
fn refused(error: &pharmakos_gateway::Error, unsaid: &host::Unsaid) -> Failure {
    let message = host::with_unsaid(strings::match_refused(&error.to_string()), unsaid);
    if error.code() == Code::InvalidArgument {
        Failure::input(message)
    } else {
        Failure::internal(message)
    }
}

/// A host that stopped on a write the match needed.
fn stopped(error: &pharmakos_gateway::Error, unsaid: &host::Unsaid) -> Failure {
    Failure::internal(host::with_unsaid(
        strings::match_stopped(&error.to_string()),
        unsaid,
    ))
}

#[cfg(test)]
mod tests {
    use super::read_line;

    fn lines(text: &[u8], cap: usize) -> Vec<Vec<u8>> {
        // A reader that hands over three bytes at a time, so a line and its
        // ending arrive in pieces.
        let mut input = std::io::BufReader::with_capacity(3, text);
        let mut read = Vec::new();
        while let Some(line) = read_line(&mut input, cap).expect("a slice reads") {
            read.push(line);
        }
        read
    }

    #[test]
    fn a_line_is_read_without_its_ending_and_the_last_needs_none() {
        assert_eq!(
            lines(b"one\ntwo\r\n\nlast", 64),
            vec![
                b"one".to_vec(),
                b"two".to_vec(),
                Vec::new(),
                b"last".to_vec()
            ]
        );
        assert_eq!(lines(b"", 64), Vec::<Vec<u8>>::new());
        assert_eq!(lines(b"\n", 64), vec![Vec::new()]);
    }

    #[test]
    fn a_line_past_the_cap_is_kept_one_byte_past_it_and_the_next_line_is_whole() {
        let read = lines(b"0123456789abcdef\nnext\n", 4);
        assert_eq!(read, vec![b"01234".to_vec(), b"next".to_vec()]);
    }
}
