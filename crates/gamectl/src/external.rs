// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl match`: the external seat, one seat of a headless match played by
//! a program outside the game (decisions-log item 231).
//!
//! **Experimental.** These commands, their output, the bridge's framing and
//! the files of this module's own in a match's folder ([`LINE_FILE`],
//! [`LOCK_FILE`], [`RESULT_FILE`]) may change in any release until v1.1
//! publishes the gateway (AGENTS.md section 2); the gateway's files in the
//! folder (the save, the sealed playbooks, the chains, the audit log) keep
//! the standing AGENTS.md section 5 gives them.
//!
//! # What a program outside gets, and what it does not
//!
//! It gets a seat: `gp.api.v1`'s methods under a seat's scopes (`observe`,
//! `plan`, `plan.submit`, `docs`), fog-filtered, counted against a call
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
//! token. One folder holds one match, [`MATCH_ID`] under the `custom` preset,
//! and a command takes up no other: a line that names another match (one the
//! lobby keeps, say, in a folder laid out the same way) is refused, so these
//! commands reach only the matches they started.
//!
//! **The folder is not secret from the program that plays it.** After every
//! command it holds the match's save, which is the whole world with no fog
//! and every seat's playbook for the round being planned, the built-in
//! seats' among them (they plan as the Lull opens); and it holds every
//! seat's sealed playbooks of the rounds played, the chains and the audit
//! log. All in plain files (AGENTS.md section 7: enforcement, not
//! cryptography). The lobby's cache holds the same kinds of file, but the
//! lobby saves a Lull only as it closes; here every command does. A program
//! that reads the folder has read what its seat is not told, this round's
//! orders included; results from such a run are development evidence and
//! never proof.
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
//! next Lull (or the match's end). Each process plans the built-in seats
//! again as it takes the Lull up (to the same playbooks: the operator is a
//! function of the match, the seat and the round), so a command adds their
//! calls to the audit log beside its own.
//!
//! An answer is printed even when the command then fails: a call that was
//! answered and whose host stopped afterwards prints the answer on standard
//! output, says the stop on standard error and exits 4, so a caller knows
//! its call landed.
//!
//! # The call budget is one process's
//!
//! The gateway counts the seat's calls in the host's process
//! (`pharmakos_gateway::limit::EXTERNAL_LIMITS`), and a headless Lull's tick
//! stands still, so the budget is a Lull's **for one process**: `serve` is
//! held to it, and the one-call commands, one process each, are bounded by
//! the cost of starting a process and not by the number. A process that has
//! spent it is refused every call for the rest of that Lull, `set_ready`
//! included; the next process has its own. After
//! `pharmakos_gateway::limit::EXTERNAL_REFUSALS_IN_A_ROW` refusals in a row
//! (lines that are not requests, and calls past the budget) the host closes
//! its door and `serve` ends, exit 3, with the match put down: a program
//! stuck in a loop is stopped, and the audit log stops growing.
//!
//! # The match's end
//!
//! The gateway takes up no match that has ended (its saves land in a Lull),
//! and **the gateway, not a file, says a match has ended**: every command
//! asks it. The process that plays the last Push (by its `set_ready`, or as
//! it takes up a match whose last process died in that Push) asks the seat's
//! last `get_recap` and writes the answer to [`RESULT_FILE`] in the match's
//! folder, whole or not at all: the outcome, the seat's settlement and, the
//! fog having lifted, every seat's tally. From then on `brief` prints that
//! file, under a heading that says it is the kept file and not the gateway's
//! answer now, and every other command is answered that the match has ended
//! (exit 3). A file of that name in the folder of a match that has not ended
//! is not read at all. Said plainly: a process that dies after the last Push
//! is on disk and before that file is, leaves an ended match with no recap
//! file, which the commands say; the match's sealed playbooks and chains are
//! still in the folder.
//!
//! # Game text is data
//!
//! What these commands print is the game's text: template and library words,
//! a notebook, a recap. So is every file in the match's folder, the kept
//! recap among them: whoever can write the folder can write those. A program
//! that plays a seat reads all of it as data and never as instructions
//! (AGENTS.md section 7).

use std::io::{BufRead, Read, Write};
use std::path::Path;

use pharmakos_gateway::Code;
use pharmakos_gateway::probation::Preset;
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

/// What [`RESULT_FILE`] is written as before it is renamed into place, so the
/// file under its own name is whole or absent.
const RESULT_PARTIAL: &str = "result.json.partial";

/// The match id every external match has: its folder is what tells one from
/// another, and Easy's seed is the match seed's, not the id's.
pub const MATCH_ID: &str = "m-external";

/// The most of [`LINE_FILE`] that is read: a config line of these commands'
/// is under two hundred bytes, and a longer file is not one.
const LINE_CAP: usize = 1024;

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
/// no params, and each is one call against the process's budget.
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
/// already does), a folder another command holds, a file that cannot be read
/// and a match the gateway refuses to open; [`Exit::Failed`], with the answer
/// on standard output, for a call the gateway refuses and a playbook the
/// verifier does, and, with its reason on standard error, for a match that
/// has ended; [`Exit::Internal`] when the host stops on a write it could not
/// make, with the call's answer on standard output when it had one.
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
            // A match whose first save could not be written is no match: its
            // line goes with it, so the folder can be started again.
            session
                .put_down()
                .map_err(|failure| unstarted(dir, failure))?;
            answered(&answer)
        }
        MatchVerb::Call { method, params } => {
            let params = match params {
                Some(path) => read_params(path)?,
                None => empty_params(),
            };
            let mut session = Session::take_up(root, dir)?;
            let answer = session.ask(method, params);
            let down = match session.advance() {
                Ok(()) => session.put_down(),
                Err(failure) => Err(failure),
            };
            beside(answered(&answer), down)
        }
        MatchVerb::Submit { path } => {
            let text = read_capped(path, pharmakos_gateway::frame::MAX_MESSAGE_BYTES).map_err(
                |unread| {
                    Failure::input(strings::unreadable(
                        "playbook",
                        &crate::display(path),
                        &unread.why,
                    ))
                },
            )?;
            let mut session = Session::take_up(root, dir)?;
            let answer = session.ask(
                "submit_plan",
                Json::Object(vec![(String::from("playbook_jsonc"), Json::String(text))]),
            );
            let down = session.put_down();
            let accepted = matches!(
                answer
                    .get("result")
                    .and_then(|result| result.get("accepted")),
                Some(Json::Bool(true))
            );
            let said = if accepted {
                Ok(json::write(&answer))
            } else {
                Err(Failure::answered(Exit::Failed, json::write(&answer)))
            };
            beside(said, down)
        }
        MatchVerb::Brief => {
            let Some(mut session) = Session::find(root, dir)? else {
                return ended_brief(dir);
            };
            let mut text = String::new();
            for method in BRIEF_READS {
                let answer = session.ask(method, empty_params());
                text.push_str(&strings::match_brief_heading(method));
                text.push('\n');
                text.push_str(brief_body(&answer).trim_end_matches('\n'));
                text.push_str("\n\n");
            }
            let down = session.put_down();
            beside(Ok(text), down)
        }
        MatchVerb::Serve => Err(Failure::internal(
            "`match serve` owns the process's standard input and output and is not rendered",
        )),
    }
}

/// What a command says when it had an answer and then put the match down.
///
/// The answer is printed whatever the match then did: a call that landed is
/// never left unsaid because the host stopped after it. When the match went
/// down cleanly the answer's own outcome stands; when it did not, the answer
/// is on standard output, the stop on standard error, and the code is the
/// stop's.
fn beside(said: Result<String, Failure>, down: Result<(), Failure>) -> Result<String, Failure> {
    match down {
        Ok(()) => said,
        Err(stop) => {
            let output = match said {
                Ok(text) => text,
                Err(refusal) => refusal.output,
            };
            Err(Failure {
                code: stop.code,
                message: stop.message,
                output,
            })
        }
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
/// ends the bridge by itself. After each answer is written the host moves the
/// match if the seat is now ready, so the line after a `set_ready` is
/// answered in the next Lull, or at the match's end.
///
/// The bridge ends before `input` does when the host closes its door (the
/// module docs, "The call budget is one process's"): the line that closed it
/// is answered, the match is put down, and the command exits 3.
///
/// # Errors
///
/// As [`run`]'s for the match; [`Exit::Failed`] when the door closed;
/// [`Exit::Internal`] when standard input cannot be read or standard output
/// cannot be written. The match is put down in each case, and a failure to
/// put it down is said beside what ended the bridge, never in its place.
pub fn serve<R: BufRead, W: Write>(
    root: &Path,
    dir: &Path,
    mut input: R,
    mut output: W,
) -> Result<String, Failure> {
    let mut session = Session::take_up(root, dir)?;
    let cap = pharmakos_gateway::frame::MAX_MESSAGE_BYTES;
    let ended: Result<(), Failure> = loop {
        let line = match read_line(&mut input, cap) {
            Ok(Some(line)) => line,
            Ok(None) => break Ok(()),
            Err(error) => {
                break Err(Failure::internal(strings::match_pipe(
                    "reading standard input",
                    &error,
                )));
            }
        };
        let answer = session.external.call_bytes(&line);
        let mut text = json::write_line(&answer);
        text.push('\n');
        if let Err(error) = output
            .write_all(text.as_bytes())
            .and_then(|()| output.flush())
        {
            break Err(Failure::internal(strings::match_pipe(
                "writing standard output",
                &error,
            )));
        }
        if session.external.door_closed() {
            break Err(Failure::failed(strings::match_door_closed(
                pharmakos_gateway::limit::EXTERNAL_REFUSALS_IN_A_ROW,
            )));
        }
        // A host that stopped has said why and is not put down again: its
        // lock goes as the session is dropped.
        session.advance()?;
    };
    match (ended, session.put_down()) {
        (Ok(()), Ok(())) => Ok(String::new()),
        (Err(first), Ok(())) => Err(first),
        (Ok(()), Err(down)) => Err(down),
        (Err(first), Err(down)) => Err(Failure::new(
            down.code,
            strings::match_and(&first.message, &down.message),
        )),
    }
}

/// One line of `input`, without its line ending, or `None` at end of input.
///
/// At most `cap + 1` bytes of a line are kept: a longer line is read to its
/// end and thrown away past that, so what is handed on is longer than any
/// request may be and is refused as such, at a bounded cost in memory. A
/// trailing carriage return is the line ending's and is taken off, **but
/// only from a line that was kept whole**: taken off a line that was cut, it
/// would bring `cap + 1` kept bytes back to `cap` and pass a line that is too
/// long, less its tail, as one that fits.
fn read_line<R: BufRead>(input: &mut R, cap: usize) -> std::io::Result<Option<Vec<u8>>> {
    let keep = cap.saturating_add(1);
    let mut line: Vec<u8> = Vec::new();
    let mut any = false;
    let mut cut = false;
    loop {
        let buffer = match input.fill_buf() {
            Ok(buffer) => buffer,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if buffer.is_empty() {
            return Ok(any.then_some(without_line_ending(line, cut)));
        }
        any = true;
        let ended = buffer.iter().position(|byte| *byte == b'\n');
        // The line's bytes in this buffer: up to its line feed, or all of it.
        let (upto, consumed) = match ended {
            Some(at) => (at, at.saturating_add(1)),
            None => (buffer.len(), buffer.len()),
        };
        let room = keep.saturating_sub(line.len());
        if upto > room {
            cut = true;
        }
        line.extend(buffer.iter().take(upto).take(room).copied());
        input.consume(consumed);
        if ended.is_some() {
            return Ok(Some(without_line_ending(line, cut)));
        }
    }
}

/// `line` without the carriage return of a `\r\n` ending, when the line was
/// kept whole ([`read_line`]).
fn without_line_ending(mut line: Vec<u8>, cut: bool) -> Vec<u8> {
    if !cut && line.last() == Some(&b'\r') {
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
            // No folder to make a lock in is no match.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(Failure::input(strings::match_missing(&crate::display(dir))))
            }
            Err(error) => Err(Failure::input(strings::match_lock_unmade(
                &crate::display(&path),
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
    /// The seat's last recap is on disk: this process ended the match and
    /// wrote it.
    kept: bool,
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
                kept: false,
            }),
            // No match was started: the line goes, so the folder is not read
            // as one.
            Err(failure) => Err(unstarted(dir, failure)),
        }
    }

    /// Take up the match `dir` holds; `None` when the gateway says it has
    /// ended.
    ///
    /// The gateway is asked, whatever the folder holds: a recap file there
    /// does not end a match, and no recap file does not keep one going. A
    /// match that ends as it is taken up (its last process died in the last
    /// Push, which the gateway replays as it opens) has its recap kept here,
    /// by the one process that will ever hold it ended and open, and is put
    /// down.
    fn find(root: &Path, dir: &Path) -> Result<Option<Session>, Failure> {
        let held = Held::take(dir)?;
        let line = kept_line(dir)?;
        let (setup, unsaid) = host::setup(root)?;
        match External::open(setup, dir, &line) {
            Ok(external) => {
                let mut session = Session {
                    external,
                    dir: dir.to_path_buf(),
                    unsaid,
                    _held: held,
                    next_id: 1,
                    kept: false,
                };
                if session.external.ended() {
                    session.keep_recap()?;
                    session.put_down()?;
                    return Ok(None);
                }
                Ok(Some(session))
            }
            Err(error) if error == pharmakos_gateway::save::match_ended() => Ok(None),
            Err(error) => Err(refused(&error, &unsaid)),
        }
    }

    /// Take up the match `dir` holds, for a command that needs a match that
    /// has not ended.
    fn take_up(root: &Path, dir: &Path) -> Result<Session, Failure> {
        match Session::find(root, dir)? {
            Some(session) => Ok(session),
            None => Err(ended(dir)),
        }
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
        if self.external.ended() && !self.kept {
            self.keep_recap()?;
        }
        Ok(())
    }

    /// Ask the seat's last `get_recap` and write it to [`RESULT_FILE`], whole
    /// or not at all, over whatever file of that name is there: this process
    /// holds the ended match, and what it is told now is the recap.
    fn keep_recap(&mut self) -> Result<(), Failure> {
        let path = self.dir.join(RESULT_FILE);
        let unwritten = |why: &str| {
            Failure::internal(strings::match_result_unwritten(&crate::display(&path), why))
        };
        let recap = self.ask("get_recap", empty_params());
        if recap.get("result").is_none() {
            let message = match recap.get("error").and_then(|error| error.get("message")) {
                Some(Json::String(message)) => message.clone(),
                _ => json::write_line(&recap),
            };
            return Err(unwritten(&strings::match_recap_refused(&message)));
        }
        let partial = self.dir.join(RESULT_PARTIAL);
        write_whole(&partial, &path, json::write(&recap).as_bytes())
            .map_err(|error| unwritten(&error.to_string()))?;
        self.kept = true;
        Ok(())
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

/// Write `bytes` to `partial`, all of them and to the disk, and then rename
/// it to `path`: `path` holds the whole of `bytes` or what it held before.
fn write_whole(partial: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(partial)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(partial, path)
}

/// `failure`, for a `match new` that started no match: the line it wrote
/// goes, so the folder is not read as a match and can be started again. A
/// folder whose match does have a save keeps its line: that match can be
/// taken up. A line that will not go is said with the failure, never left
/// unsaid.
fn unstarted(dir: &Path, failure: Failure) -> Failure {
    if pharmakos_gateway::cache::MatchCache::reopen(dir, MATCH_ID).is_ok() {
        return failure;
    }
    let path = dir.join(LINE_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => failure,
        Err(error) => Failure {
            code: failure.code,
            message: strings::match_line_left(
                &failure.message,
                &crate::display(&path),
                &error.to_string(),
            ),
            output: failure.output,
        },
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

/// The config line `dir`'s match was started with, as a line to resume by.
///
/// Only a line `match new` could have written is taken: [`MATCH_ID`], under
/// the `custom` preset. A folder laid out like a match cache can hold other
/// matches (the lobby's data folder does), and a line that named one would
/// otherwise put a command in a seat of a match these commands did not start.
fn kept_line(dir: &Path) -> Result<ConfigLine, Failure> {
    let path = dir.join(LINE_FILE);
    let unreadable = |why: &str| {
        Failure::input(strings::unreadable(
            "match's config line",
            &crate::display(&path),
            why,
        ))
    };
    let text = match read_capped(&path, LINE_CAP) {
        Ok(text) => text,
        Err(unread) if unread.missing => {
            return Err(Failure::input(strings::match_missing(&crate::display(dir))));
        }
        Err(unread) => return Err(unreadable(&unread.why)),
    };
    let mut line = ConfigLine::parse(&text).map_err(|error| unreadable(&error.to_string()))?;
    if line.config.match_id != MATCH_ID || line.config.preset != Preset::Custom {
        return Err(Failure::input(strings::match_line_foreign(
            &crate::display(&path),
            MATCH_ID,
        )));
    }
    line.resume = true;
    Ok(line)
}

/// What a command but `brief` says in a folder whose match has ended: where
/// the seat's last recap is, or that it was not kept.
fn ended(dir: &Path) -> Failure {
    let path = dir.join(RESULT_FILE);
    match path.try_exists() {
        Ok(true) => Failure::failed(strings::match_ended(&crate::display(&path))),
        Ok(false) => Failure::failed(strings::match_ended_unkept(&crate::display(&path))),
        Err(error) => Failure::input(strings::unreadable(
            "match's last recap",
            &crate::display(&path),
            &error.to_string(),
        )),
    }
}

/// `brief`, in a folder whose match the gateway says has ended: the seat's
/// last recap as [`RESULT_FILE`] keeps it, under a heading that says where
/// the text came from.
fn ended_brief(dir: &Path) -> Result<String, Failure> {
    let path = dir.join(RESULT_FILE);
    let unreadable = |why: &str| {
        Failure::input(strings::unreadable(
            "match's last recap",
            &crate::display(&path),
            why,
        ))
    };
    let kept = match read_capped(&path, pharmakos_proto::transport::MAX_ANSWER_BYTES) {
        Ok(kept) => kept,
        Err(unread) if unread.missing => {
            return Err(Failure::failed(strings::match_ended_unkept(
                &crate::display(&path),
            )));
        }
        Err(unread) => return Err(unreadable(&unread.why)),
    };
    let recap = json::read(&kept).map_err(|error| unreadable(&error.to_string()))?;
    let mut text = strings::match_brief_heading("ended");
    text.push('\n');
    text.push_str(&strings::match_ended(&crate::display(&path)));
    text.push_str("\n\n");
    text.push_str(&strings::match_brief_kept(&crate::display(&path)));
    text.push('\n');
    text.push_str(brief_body(&recap).trim_end_matches('\n'));
    text.push('\n');
    Ok(text)
}

/// Why a file was not read.
struct Unread {
    /// There is no such file.
    missing: bool,
    /// The reason, in the operating system's words or this crate's.
    why: String,
}

/// The text of the file at `path`, when it is at most `cap` bytes of UTF-8.
///
/// Every file these commands read is read to a stated length: a file is a
/// caller's input, and a longer one is refused, never read into memory whole.
fn read_capped(path: &Path, cap: usize) -> Result<String, Unread> {
    let other = |why: String| Unread {
        missing: false,
        why,
    };
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            return Err(Unread {
                missing: error.kind() == std::io::ErrorKind::NotFound,
                why: error.to_string(),
            });
        }
    };
    let limit = u64::try_from(cap)
        .map_err(|error| other(error.to_string()))?
        .saturating_add(1);
    let mut text = String::new();
    file.take(limit)
        .read_to_string(&mut text)
        .map_err(|error| other(error.to_string()))?;
    if text.len() > cap {
        return Err(other(strings::too_long(cap)));
    }
    Ok(text)
}

/// A call's params, read from a file: one JSON object, at most a request
/// long (a request that holds them has to fit one).
fn read_params(path: &Path) -> Result<Json, Failure> {
    let unreadable = |why: &str| {
        Failure::input(strings::unreadable(
            "params file",
            &crate::display(path),
            why,
        ))
    };
    let text = read_capped(path, pharmakos_gateway::frame::MAX_MESSAGE_BYTES)
        .map_err(|unread| unreadable(&unread.why))?;
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

    /// The carriage return of a line ending is taken off a line that was kept
    /// whole, at the cap exactly as below it, and never off one that was cut:
    /// a line too long whose byte past the cap is a carriage return stays one
    /// byte past the cap, and is refused as too long (review 2, finding 3).
    #[test]
    fn a_carriage_return_is_the_endings_only_on_a_line_kept_whole() {
        // Exactly the cap, ended `\r\n`: whole, and its ending is taken off.
        assert_eq!(lines(b"abcd\r\n", 4), vec![b"abcd".to_vec()]);
        // Past the cap, with a carriage return where the kept bytes end.
        assert_eq!(lines(b"abcd\rLONG TAIL\n", 4), vec![b"abcd\r".to_vec()]);
        assert_eq!(lines(b"abcd\rLONG TAIL\r\n", 4), vec![b"abcd\r".to_vec()]);
        // Past the cap by one byte, ended `\r\n`: cut, so nothing is taken off.
        assert_eq!(lines(b"abcde\r\n", 4), vec![b"abcde".to_vec()]);
        // And with no line feed at all before the input ends.
        assert_eq!(lines(b"abcd\rLONG TAIL", 4), vec![b"abcd\r".to_vec()]);
        assert_eq!(lines(b"abcd\r", 4), vec![b"abcd".to_vec()]);
    }
}
