// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **The rig's link handling under seeded faults** (S3's `fixq-c`, fix pass 5, item B).
//!
//! The rig is sans-IO: it takes events and time and returns calls. Here a seeded model host
//! plays both links. For every call it answers rightly, refuses, answers with something
//! unreadable, answers past the inbound buffer (a close on the message, 1009), closes with
//! any code (1000, 1001, 1003, 1007, 1008, 1009, 1011, none), drops the connection, fails the
//! send, or delays. It also refuses reconnects at times. The player presses Ready and saves
//! notes at generated times. Five invariants are checked at every step, each a named
//! assertion: no spin, no hang, gives up, true words, one door. A failing seed is a named
//! regression test below it; review 4's R4-P1 to R4-P6 are hand-written cases, red before the
//! fix.

#[cfg(test)]
mod tests {
    use crate::rig::{
        ADMIN, CLOSE_GRACE_US, CONNECT_DEADLINE_US, Close, KEEPALIVE_US, LinkAction, Phase,
        Purpose, RECONNECT_BACKOFF_US, RECONNECTS, Rig, SEAT, SEAT_CALLS_PER_REFILL, Transport,
    };
    use std::collections::BTreeMap;

    /// A small seeded generator (xorshift64*): no dependency, and a run replays from its seed.
    struct Seeded(u64);

    impl Seeded {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, bound: u64) -> u64 {
            self.next().checked_rem(bound).expect("a bound above zero")
        }

        /// True `per_mille` times in a thousand.
        fn chance(&mut self, per_mille: u64) -> bool {
            self.below(1000) < per_mille
        }
    }

    /// What the model host does with one call.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Fault {
        Answer,
        Refuse,
        Unreadable,
        /// A result of the right shape with wrongly typed fields: a decode failure in `settle`.
        Garbled,
        PastBuffer,
        Close(i64),
        Drop,
        SendFailed,
        Delay(u64),
        /// No answer ever: the call's deadline settles it.
        Silent,
        /// An answer whose id is a string: dropped and counted; the deadline settles the call.
        NoId,
    }

    /// The host's temper over a run.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Temper {
        /// A fault on about one call in four.
        Mixed,
        /// Every link answers once and is closed within a second or so of opening, for ever.
        Flapping,
        /// Faulty for the first ten seconds, then answers every call rightly.
        Recovering,
    }

    /// The close codes a host may close with; -1 is a close with no code.
    const CODES: [i64; 8] = [1000, 1001, 1003, 1007, 1008, 1009, 1011, -1];

    /// Steps a run takes, and how far apart.
    const STEPS: u64 = 2400;
    const STEP_US: u64 = 50_000;

    /// Invariant 1's bound: a purpose may be sent at most this many times on a link in any
    /// keep-alive interval with no readable answer on that link between them.
    const MAX_UNANSWERED_SENDS: usize = 8;

    /// Invariant 2's bound: how long a pressed Ready may wait, counting only time its link was
    /// open, before it is sent or the Ready line says why not.
    const READY_WAIT_US: u64 = super::super::CALL_DEADLINE_US + 4 * KEEPALIVE_US;

    /// Invariant 2's bound on a connection's state: how long a link may stay connecting or
    /// closing before the rig has it aborted (fix pass 7: the real peer stayed closing for ever).
    const TRANSPORT_WAIT_US: u64 = if CONNECT_DEADLINE_US > CLOSE_GRACE_US {
        CONNECT_DEADLINE_US
    } else {
        CLOSE_GRACE_US
    } + 2 * STEP_US;

    /// One link's connection as the model plays Godot's `WebSocketPeer`: its state, whether it
    /// is stuck there until aborted (a host that accepts and never completes the handshake, or
    /// never answers a close), and the close code it reports when it does close.
    #[derive(Clone, Copy, Debug)]
    struct Peer {
        state: Transport,
        stuck: bool,
        code: i64,
        since: u64,
    }

    impl Peer {
        const fn closed(now: u64) -> Self {
            Self {
                state: Transport::Closed,
                stuck: false,
                code: -1,
                since: now,
            }
        }

        const fn now_in(state: Transport, stuck: bool, code: i64, now: u64) -> Self {
            Self {
                state,
                stuck,
                code,
                since: now,
            }
        }
    }

    /// Invariant 4's bound against a host that recovers: how long a "may have been taken" line
    /// may stand once the host answers rightly again.
    const RESOLVE_US: u64 = 6 * KEEPALIVE_US;

    /// The footer the model host writes: its phase and round.
    #[derive(Clone, Copy)]
    struct Footer {
        phase: &'static str,
        round: u32,
    }

    /// The footer the hand-written cases answer with.
    const LULL: Footer = Footer {
        phase: "lull",
        round: 1,
    };

    /// Steps in one round of the model host, and the last step of its Lull: a Lull longer than
    /// [`READY_WAIT_US`], so a Ready that hangs is seen before the phase moves on.
    const ROUND_STEPS: u64 = 1100;
    const LULL_LAST_STEP: u64 = 899;
    const _: () = assert!(LULL_LAST_STEP * STEP_US > READY_WAIT_US);

    /// The model host's phase at `step` of a run: a Lull, a Push and a recap a round, round after
    /// round, and the match ended for the last stretch.
    fn footer_at(step: u64) -> Footer {
        if step >= STEPS.saturating_sub(200) {
            return Footer {
                phase: "ended",
                round: 3,
            };
        }
        let round = u32::try_from(step.checked_div(ROUND_STEPS).expect("a span")).expect("fits");
        let phase = match step.checked_rem(ROUND_STEPS).expect("a span") {
            0..=LULL_LAST_STEP => "lull",
            900..=999 => "push",
            _ => "recap",
        };
        Footer {
            phase,
            round: round.saturating_add(1),
        }
    }

    /// A JSON-RPC answer with a footer: `extra` is the result's fields before `_status`.
    fn wire(id: i64, extra: &str, footer: Footer) -> String {
        let Footer { phase, round } = footer;
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{{extra}"_status":{{"phase":"{phase}","phase_remaining_ms":180000,"round":{round}}}}}}}"#
        )
    }

    /// The right answer to `method`.
    fn right(id: i64, method: &str, footer: Footer) -> String {
        let extra = match method {
            "get_view" => r#""next_cursor":"c9","complete":true,"#,
            "get_segment_feed" => r#""events":[],"next_cursor":"f1","#,
            "report_host_clock" => r#""all_ready":false,"#,
            "advance_push" => r#""advanced_ms":0,"#,
            "get_recap" => r#""prose":"x","seat_tallies":[],"#,
            _ => "",
        };
        wire(id, extra, footer)
    }

    fn id_of(text: &str) -> i64 {
        let frame = pharmakos_proto::json::read(text).expect("a frame the rig wrote");
        match frame.get("id") {
            Some(pharmakos_proto::json::Json::Number(id)) => id.parse().expect("an id"),
            _ => panic!("a frame with no id"),
        }
    }

    fn method_of(text: &str) -> String {
        let frame = pharmakos_proto::json::read(text).expect("a frame the rig wrote");
        match frame.get("method") {
            Some(pharmakos_proto::json::Json::String(method)) => method.clone(),
            _ => panic!("a frame with no method"),
        }
    }

    /// The call methods that change something at the gateway.
    const WRITES: [&str; 3] = ["submit_plan", "save_notes", "save_draft"];

    /// One run's account of what it saw.
    #[derive(Default)]
    struct Watch {
        /// Per link and method, the times it was sent since the link's last readable answer.
        unanswered: BTreeMap<(usize, String), Vec<u64>>,
        /// When Ready was pressed and not yet sent or said, and the link-open time since.
        ready_waiting: Option<u64>,
        /// Whether a link was given up, and when.
        gave_up: Option<u64>,
    }

    /// One run of the model host against a rig, checking every invariant at every step. Answers
    /// the step at which the rig gave a link up, if it did.
    fn run(seed: u64, temper: Temper) -> Option<u64> {
        run_on(&mut Honest(Rig::new()), seed, temper)
    }

    /// [`run`] against `rig`: the real one ([`Honest`]) or a deliberately bad one (the
    /// self-test's).
    #[allow(
        clippy::too_many_lines,
        reason = "one step of the model host after another, read in order; not a determinism lint"
    )]
    fn run_on<R: Harnessed>(rig: &mut R, seed: u64, temper: Temper) -> Option<u64> {
        let mut random = Seeded(seed | 1);
        let mut watch = Watch::default();
        let mut delayed: Vec<(u64, usize, String)> = Vec::new();
        let mut opened_at: BTreeMap<usize, u64> = BTreeMap::new();
        let mut answered_since_open: BTreeMap<usize, bool> = BTreeMap::new();
        let mut now = 0_u64;
        let mut maybe_since: Option<u64> = None;
        let mut peers = [Peer::closed(0), Peer::closed(0)];
        for step in 0..STEPS {
            now = now.saturating_add(STEP_US);
            let footer = footer_at(step);
            let healthy = temper == Temper::Recovering && now > 10_000_000;
            // The links, as host_link.gd drives them (fix pass 7): each peer's state goes to the
            // rig every frame, the rig says when to abort it, and its policy says when to connect.
            for link in [ADMIN, SEAT] {
                let Some(peer) = peers.get_mut(link) else {
                    continue;
                };
                // A close the harness made through a fault: the peer is closed too.
                if peer.state == Transport::Open
                    && !rig.links.get(link).is_some_and(|state| state.open)
                {
                    *peer = Peer::closed(now);
                }
                // Invariant 2: no hang. No connection stays connecting or closing past its bound.
                if matches!(peer.state, Transport::Connecting | Transport::Closing) {
                    assert!(
                        now.saturating_sub(peer.since) <= TRANSPORT_WAIT_US,
                        "seed {seed:#x} {temper:?} step {step}: no hang: link {link} {:?} for {} us",
                        peer.state,
                        now.saturating_sub(peer.since)
                    );
                }
                if rig.transport(link, peer.state) {
                    // An abort closes at once (measured: 32 ms), reported with no code.
                    *peer = Peer::closed(now);
                    let _ = rig.transport(link, Transport::Closed);
                    rig.closed(link, Close::of_code(-1));
                    continue;
                }
                match peer.state {
                    Transport::Open => {
                        // A flapping host closes a link a second or so after it answered once.
                        if temper == Temper::Flapping
                            && answered_since_open.get(&link) == Some(&true)
                            && opened_at
                                .get(&link)
                                .is_some_and(|at| now.saturating_sub(*at) > 1_000_000)
                        {
                            *peer = Peer::closed(now);
                            rig.closed(link, Close::Connection);
                        }
                    }
                    Transport::Connecting if !peer.stuck => {
                        if !healthy && temper != Temper::Flapping && random.chance(150) {
                            // The host refuses the connection: it closes at once.
                            *peer = Peer::closed(now);
                            rig.closed(link, Close::Connection);
                        } else {
                            *peer = Peer::now_in(Transport::Open, false, -1, now);
                            rig.opened(link);
                            opened_at.insert(link, now);
                            answered_since_open.insert(link, false);
                        }
                    }
                    Transport::Closing if !peer.stuck => {
                        let code = peer.code;
                        *peer = Peer::closed(now);
                        rig.closed(link, Close::of_code(code));
                    }
                    Transport::Connecting | Transport::Closing => {}
                    Transport::Closed => match rig.link_action(link) {
                        Some(LinkAction::Connect) => {
                            // A host that accepts the connection and never completes the
                            // handshake (review 6's TCPServer probe): stuck until aborted.
                            let stuck = !healthy && temper != Temper::Flapping && random.chance(40);
                            *peer = Peer::now_in(Transport::Connecting, stuck, -1, now);
                        }
                        Some(LinkAction::GiveUp) => {
                            watch.gave_up.get_or_insert(step);
                        }
                        Some(LinkAction::Wait) | None => {}
                    },
                }
            }
            if watch.gave_up.is_some() {
                return watch.gave_up;
            }
            // The player.
            if random.chance(20) && rig.phase == Phase::Lull {
                rig.press_ready();
                watch.ready_waiting.get_or_insert(0);
            }
            if random.chance(15) {
                rig.editor_mut().save_notes("notes");
            }
            if random.chance(10) {
                let _ = rig.editor_mut().load(br#"{"schema_version":{"major":1}}"#);
            }
            if random.chance(8) {
                let _ = rig.editor_mut().submit();
            }
            if random.chance(8) {
                let _ = rig.editor_mut().save_draft("draft");
            }
            if step.checked_rem(20) == Some(0) {
                rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            }
            // Delayed answers that are due.
            let (due, later): (Vec<_>, Vec<_>) =
                delayed.into_iter().partition(|(at, _, _)| *at <= now);
            delayed = later;
            for (_, link, text) in due {
                let peer_open = peers
                    .get(link)
                    .is_some_and(|peer| peer.state == Transport::Open);
                if peer_open && rig.links.get(link).is_some_and(|state| state.open) {
                    rig.receive(link, &text);
                    answered_since_open.insert(link, true);
                    watch.unanswered.retain(|(at, _), _| *at != link);
                }
            }
            // The calls, and what the host does with each.
            for frame in rig.poll(now) {
                let method = method_of(&frame.text);
                let id = id_of(&frame.text);
                let link = frame.link;
                // host_link.gd sends only on an open peer: a frame for a peer stuck closing is
                // never sent, and its call waits for the abort.
                if peers
                    .get(link)
                    .is_none_or(|peer| peer.state != Transport::Open)
                {
                    continue;
                }
                let sends = watch.unanswered.entry((link, method.clone())).or_default();
                sends.push(now);
                sends.retain(|at| now.saturating_sub(*at) <= KEEPALIVE_US);
                // Invariant 1: no spin.
                assert!(
                    sends.len() <= MAX_UNANSWERED_SENDS,
                    "seed {seed:#x} {temper:?} step {step}: no spin: {method} sent {} times on link \
                     {link} in a keep-alive interval with no readable answer",
                    sends.len()
                );
                if method == "set_ready" {
                    watch.ready_waiting = None;
                }
                // A frame the rig never tracks (the self-test's spinner): the host never
                // answers it, so only the spin invariant can see it.
                let fault = if id >= 900_000 {
                    Fault::Silent
                } else if healthy || temper == Temper::Flapping || !random.chance(250) {
                    Fault::Answer
                } else {
                    match random.below(8) {
                        0 => Fault::Refuse,
                        1 if random.chance(500) => Fault::Garbled,
                        1 => Fault::Unreadable,
                        2 => Fault::PastBuffer,
                        3 => Fault::Close(
                            *CODES
                                .get(usize::try_from(random.below(8)).expect("fits"))
                                .expect("a code"),
                        ),
                        4 => Fault::Drop,
                        5 => Fault::SendFailed,
                        6 if random.chance(300) => Fault::Silent,
                        6 if random.chance(300) => Fault::NoId,
                        _ => Fault::Delay(random.below(800_000)),
                    }
                };
                let ledger_before = rig.ledger();
                match fault {
                    Fault::Answer => {
                        rig.receive(link, &right(id, &method, footer));
                        answered_since_open.insert(link, true);
                        watch.unanswered.retain(|(at, _), _| *at != link);
                    }
                    Fault::Silent => {}
                    Fault::NoId => {
                        rig.receive(
                            link,
                            &format!(r#"{{"jsonrpc":"2.0","id":"{id}","result":{{}}}}"#),
                        );
                    }
                    Fault::Delay(by) => {
                        delayed.push((now.saturating_add(by), link, right(id, &method, footer)));
                    }
                    Fault::Refuse => {
                        rig.receive(
                            link,
                            &format!(
                                r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32000,"message":"slow down","data":{{"code":"RATE_LIMITED"}}}}}}"#
                            ),
                        );
                        if rig.ledger().unfinished > ledger_before.unfinished {
                            check_words(&*rig, seed, step, &method, "refused");
                        }
                    }
                    Fault::Unreadable => {
                        rig.receive(link, &format!(r#"{{"jsonrpc":"2.0","id":{id}}}"#));
                        if rig.ledger().unfinished > ledger_before.unfinished {
                            check_words(&*rig, seed, step, &method, "unreadable");
                        }
                    }
                    Fault::Garbled => {
                        rig.receive(
                            link,
                            &wire(
                                id,
                                r#""next_cursor":5,"events":5,"advanced_ms":"x","all_ready":"x","seat_tallies":5,"prose":5,"#,
                                    footer,
                            ),
                        );
                        if rig.ledger().unfinished > ledger_before.unfinished {
                            check_words(&*rig, seed, step, &method, "unreadable");
                        }
                    }
                    Fault::PastBuffer => {
                        rig.closed(link, Close::ThisMessage(1009));
                        if rig.ledger().unfinished > ledger_before.unfinished {
                            check_words_closed(&*rig, seed, step, &method, 1009);
                        }
                    }
                    // A host that starts a close and never finishes it: the peer is closing, and
                    // the rig hears of it only once the grace aborts it (fix pass 7).
                    Fault::Close(code) if random.chance(250) => {
                        if let Some(peer) = peers.get_mut(link) {
                            *peer = Peer::now_in(Transport::Closing, true, code, now);
                        }
                    }
                    Fault::Close(code) => {
                        rig.closed(link, Close::of_code(code));
                        if rig.ledger().unfinished > ledger_before.unfinished {
                            check_words_closed(&*rig, seed, step, &method, code);
                        }
                    }
                    Fault::Drop => {
                        rig.closed(link, Close::Connection);
                    }
                    Fault::SendFailed => {
                        rig.send_failed(link, 6);
                        if rig.ledger().unfinished > ledger_before.unfinished {
                            check_words(&*rig, seed, step, &method, "send_failed");
                        }
                    }
                }
            }
            // Invariant 4, resolved: against a host that recovers, every "may have been taken"
            // line (the editor's, the Ready line's) gives way within its bound once the host
            // answers rightly.
            let maybe = rig.editor().status().key == "call_maybe_taken"
                || rig.ready_failed().map(|(name, _)| name) == Some("lost");
            if maybe && healthy {
                let since = *maybe_since.get_or_insert(now);
                assert!(
                    now.saturating_sub(since) <= RESOLVE_US,
                    "seed {seed:#x} {temper:?} step {step}: true words: a \"may have been \
                     taken\" line stood {} us against a host that recovered",
                    now.saturating_sub(since)
                );
            } else {
                maybe_since = None;
            }
            // Invariant 2: no hang. No call stays in flight past its deadline; a pressed Ready is
            // sent, or its line says why, within its bound of time the seat link was open with
            // nothing in flight (a call in flight is held to its own deadline: fix pass 7); and
            // the editor never waits on a call that is not in flight.
            for link in [ADMIN, SEAT] {
                if let Some(state) = rig.links.get(link)
                    && state.in_flight.is_some()
                {
                    let waited = now.saturating_sub(state.in_flight_since);
                    assert!(
                        waited <= super::super::CALL_DEADLINE_US.saturating_add(2 * STEP_US),
                        "seed {seed:#x} {temper:?} step {step}: no hang: a call on link {link} in \
                         flight for {waited} us"
                    );
                }
            }
            if let Some(waited) = watch.ready_waiting.as_mut() {
                if rig.ready_failed().is_some() || rig.phase != Phase::Lull || !rig.wants_ready() {
                    watch.ready_waiting = None;
                } else if rig
                    .links
                    .get(SEAT)
                    .is_some_and(|state| state.open && state.in_flight.is_none())
                {
                    *waited = waited.saturating_add(STEP_US);
                    assert!(
                        *waited <= READY_WAIT_US,
                        "seed {seed:#x} {temper:?} step {step}: no hang: Ready pressed and neither \
                         sent nor said after {waited} us of an open seat link (editor jobs {}, \
                         status owed {}, seat in flight {:?}, calls left {}, paging {:?}, \
                         keyframe {})",
                        rig.editor().has_pending_jobs(),
                        rig.reads.status,
                        rig.links.get(SEAT).map(|state| state.in_flight),
                        rig.seat_calls_left,
                        rig.view_paging,
                        rig.due.keyframe
                    );
                }
            }
            let plan_in_flight = rig
                .links
                .get(SEAT)
                .is_some_and(|state| matches!(state.in_flight, Some((_, Purpose::Plan))));
            assert!(
                !rig.editor().call_in_flight() || plan_in_flight,
                "seed {seed:#x} {temper:?} step {step}: no hang: the editor waits on a call that is \
                 not in flight"
            );
            // Invariant 5: one door. Every call handed out ended one way, or is in flight.
            let ledger = rig.ledger();
            assert_eq!(
                ledger.sent,
                ledger
                    .answered
                    .saturating_add(ledger.unfinished)
                    .saturating_add(ledger.carried)
                    .saturating_add(u64::try_from(rig.in_flight()).expect("fits")),
                "seed {seed:#x} {temper:?} step {step}: one door: {ledger:?}, {} in flight",
                rig.in_flight()
            );
        }
        watch.gave_up
    }

    /// Invariant 4: true words. The last settlement names the cause that happened, and the words
    /// the rig hands the bridge for it are that cause's; a lost write says it may have been taken
    /// and forgets the seal.
    /// [`check_words`] for a close on the message with `code`: a 1009 on an editor call is the
    /// client's own close on an answer too big for it, said as what to do.
    fn check_words_closed<R: Harnessed>(rig: &R, seed: u64, step: u64, method: &str, code: i64) {
        check_words_with(rig, seed, step, method, "closed_on", Some(code));
    }

    fn check_words<R: Harnessed>(rig: &R, seed: u64, step: u64, method: &str, cause: &str) {
        check_words_with(rig, seed, step, method, cause, None);
    }

    fn check_words_with<R: Harnessed>(
        rig: &R,
        seed: u64,
        step: u64,
        method: &str,
        cause: &str,
        code: Option<i64>,
    ) {
        let Some((purpose, named)) = rig.named_unfinished() else {
            panic!(
                "seed {seed:#x} step {step}: true words: {method} {cause} was settled by no door"
            );
        };
        assert_eq!(
            named, cause,
            "seed {seed:#x} step {step}: true words: {method} settled as {named}, not {cause}"
        );
        match purpose {
            Purpose::Ready => {
                // A lost answer (unreadable, closed on, timed out) is "may have been taken" while
                // Ready is sent again, then "lost each time"; any other cause is said as itself.
                let lost = matches!(cause, "unreadable" | "closed_on" | "timed_out");
                let said = rig.ready_failed().map(|(name, _)| name);
                assert!(
                    if lost {
                        matches!(said, Some("lost" | "lost_spent"))
                    } else {
                        said == Some(cause)
                    },
                    "seed {seed:#x} step {step}: true words: the Ready line says {said:?} for {cause}"
                );
            }
            Purpose::Status => assert_eq!(
                rig.status_facts
                    .failed
                    .as_ref()
                    .map(crate::editor::ReadFailed::name),
                Some(cause),
                "seed {seed:#x} step {step}: true words: the status line"
            ),
            Purpose::Recap => assert_eq!(
                rig.recap().refusal_cause,
                cause,
                "seed {seed:#x} step {step}: true words: the recap column"
            ),
            Purpose::Plan if cause != "refused" => {
                let key = rig.editor().status().key.clone();
                let lost = matches!(cause, "closed_on" | "unreadable") && WRITES.contains(&method);
                let expected = if lost {
                    "call_maybe_taken"
                } else {
                    match cause {
                        "too_large" => "call_too_large",
                        "send_failed" => "call_send_failed",
                        "unreadable" => "call_unreadable",
                        _ if code == Some(1009) => "call_answer_too_large",
                        _ => "call_closed_on",
                    }
                };
                // A check of the text is said on its own line, not the status line; any other call's
                // failure is said on the status line in its cause's words.
                if ![
                    "verify_plan",
                    "render_plan",
                    "estimate_route",
                    "resolve_refs",
                ]
                .contains(&method)
                {
                    assert_eq!(
                        key, expected,
                        "seed {seed:#x} step {step}: true words: {method} {cause}"
                    );
                }
                if lost {
                    assert!(
                        rig.status_facts.sealed.is_none() && rig.reads.status,
                        "seed {seed:#x} step {step}: true words: a lost {method} did not forget the seal"
                    );
                }
            }
            _ => {}
        }
    }

    /// **The property:** mixed faults on a range of seeds; every invariant holds at every step, and
    /// a host that faults about one call in four is not given up for good while it keeps answering.
    #[test]
    fn every_invariant_holds_under_mixed_faults() {
        for seed in 0..120_u64 {
            let _ = run(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), Temper::Mixed);
        }
    }

    /// **Gives up (invariant 3):** against a host that never lets a link stay up, whatever mix of
    /// closes and answers it uses, the link is given up within [`RECONNECTS`] attempts in a row and
    /// the back-off's sum plus a second an attempt; against a host that recovers, never.
    #[test]
    fn a_host_that_never_stays_up_is_given_up_and_one_that_recovers_is_not() {
        let backoff: u64 = RECONNECT_BACKOFF_US.iter().sum();
        let attempts = u64::from(RECONNECTS).saturating_add(1);
        let bound = backoff
            .saturating_add(attempts.saturating_mul(2_000_000))
            .checked_div(STEP_US)
            .expect("a step");
        for seed in 0..30_u64 {
            assert_gives_up(&mut Honest(Rig::new()), seed.wrapping_add(7), bound);
            assert_eq!(
                run(seed.wrapping_add(1000), Temper::Recovering),
                None,
                "seed {seed}: a host that recovers was given up"
            );
        }
    }

    /// Invariant 3, as an assertion the self-test can trip: against a flapping host, `rig` gives
    /// the link up within `bound` steps.
    fn assert_gives_up<R: Harnessed>(rig: &mut R, seed: u64, bound: u64) {
        let gave_up = run_on(rig, seed, Temper::Flapping);
        assert!(
            gave_up.is_some_and(|step| step <= bound),
            "seed {seed}: gives up: a flapping host was given up at {gave_up:?}, past step {bound}"
        );
    }

    /// What the harness drives: the calls a host and a player make, which a bad rig overrides;
    /// everything else is the rig's, through `Deref`.
    trait Harnessed: std::ops::DerefMut<Target = Rig> {
        fn poll(&mut self, now: u64) -> Vec<super::super::Outgoing> {
            (**self).poll(now)
        }
        fn receive(&mut self, link: usize, text: &str) {
            let _ = (**self).receive(link, text);
        }
        fn link_action(&self, link: usize) -> Option<LinkAction> {
            (**self).link_action(link)
        }
        fn transport(&mut self, link: usize, state: Transport) -> bool {
            (**self).transport(link, state)
        }
        fn press_ready(&mut self) {
            (**self).ready();
        }
        fn wants_ready(&self) -> bool {
            self.wants.ready
        }
        fn named_unfinished(&self) -> Option<(Purpose, &'static str)> {
            self.last_unfinished
        }
    }

    /// The real rig, as the harness drives it.
    struct Honest(Rig);

    impl std::ops::Deref for Honest {
        type Target = Rig;
        fn deref(&self) -> &Rig {
            &self.0
        }
    }

    impl std::ops::DerefMut for Honest {
        fn deref_mut(&mut self) -> &mut Rig {
            &mut self.0
        }
    }

    impl Harnessed for Honest {}

    /// A deliberately bad rig, one fault per invariant, for the self-test.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Bad {
        /// Sends a status read on every poll, answered or not (invariant 1).
        Spinner,
        /// Takes Ready and never sends it, nor says why (invariant 2).
        Hanger,
        /// Opens a link again however often it fails (invariant 3).
        NeverGivesUp,
        /// Names every failure as the gateway's refusal (invariant 4).
        FalseCause,
        /// Settles a refused call by clearing it, past the one door (invariant 5).
        PrivateSettle,
        /// Never has a connection aborted, so a peer stuck connecting or closing stays so
        /// (invariant 2; fix pass 7, review 6's silent host).
        NeverAborts,
    }

    struct Sabotaged {
        rig: Rig,
        bad: Bad,
        spun: i64,
        wants: bool,
    }

    impl std::ops::Deref for Sabotaged {
        type Target = Rig;
        fn deref(&self) -> &Rig {
            &self.rig
        }
    }

    impl std::ops::DerefMut for Sabotaged {
        fn deref_mut(&mut self) -> &mut Rig {
            &mut self.rig
        }
    }

    impl Harnessed for Sabotaged {
        fn poll(&mut self, now: u64) -> Vec<super::super::Outgoing> {
            let mut frames = self.rig.poll(now);
            if self.bad == Bad::Spinner && self.rig.links.get(SEAT).is_some_and(|link| link.open) {
                self.spun = self.spun.saturating_add(1);
                frames.push(super::super::Outgoing {
                    link: SEAT,
                    text: super::super::frame_text(
                        900_000_i64.saturating_add(self.spun),
                        "get_status",
                        super::super::object(Vec::new()),
                    ),
                });
            }
            frames
        }
        fn receive(&mut self, link: usize, text: &str) {
            if self.bad == Bad::PrivateSettle && text.contains("\"error\"") {
                if let Some(state) = self.rig.links.get_mut(link) {
                    state.in_flight = None;
                }
                return;
            }
            let _ = self.rig.receive(link, text);
        }
        fn link_action(&self, link: usize) -> Option<LinkAction> {
            match (self.bad, self.rig.link_action(link)) {
                (Bad::NeverGivesUp, Some(LinkAction::GiveUp)) => Some(LinkAction::Connect),
                (_, action) => action,
            }
        }
        fn transport(&mut self, link: usize, state: Transport) -> bool {
            let abort = self.rig.transport(link, state);
            abort && self.bad != Bad::NeverAborts
        }
        fn press_ready(&mut self) {
            if self.bad == Bad::Hanger {
                self.wants = true;
            } else {
                self.rig.ready();
            }
        }
        fn wants_ready(&self) -> bool {
            if self.bad == Bad::Hanger {
                self.wants
            } else {
                self.rig.wants.ready
            }
        }
        fn named_unfinished(&self) -> Option<(Purpose, &'static str)> {
            match (self.bad, self.rig.last_unfinished) {
                (Bad::FalseCause, Some((purpose, _))) => Some((purpose, "refused")),
                (_, named) => named,
            }
        }
    }

    /// **The fault test holds its own strength** (fix pass 6, item 6; review 5's MT1 and MT2):
    /// against each deliberately bad rig, the harness's own named assertion fires. Raising a bound
    /// past what the spinner does, or deleting an assertion, turns this red.
    #[test]
    fn each_invariant_catches_a_rig_that_breaks_it() {
        let bound = RECONNECT_BACKOFF_US
            .iter()
            .sum::<u64>()
            .saturating_add(
                u64::from(RECONNECTS)
                    .saturating_add(1)
                    .saturating_mul(2_000_000),
            )
            .checked_div(STEP_US)
            .expect("a step");
        for (bad, temper, named) in [
            (Bad::Spinner, Temper::Recovering, "no spin"),
            (Bad::Hanger, Temper::Recovering, "no hang"),
            (Bad::NeverGivesUp, Temper::Flapping, "gives up"),
            (Bad::FalseCause, Temper::Mixed, "true words"),
            (Bad::PrivateSettle, Temper::Mixed, "one door"),
            (Bad::NeverAborts, Temper::Mixed, "no hang"),
        ] {
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut rig = Sabotaged {
                    rig: Rig::new(),
                    bad,
                    spun: 0,
                    wants: false,
                };
                if temper == Temper::Flapping {
                    assert_gives_up(&mut rig, 7, bound);
                } else {
                    for seed in 0..12_u64 {
                        let _ = run_on(&mut rig, seed.wrapping_mul(0x9E37_79B9), temper);
                        rig = Sabotaged {
                            rig: Rig::new(),
                            bad,
                            spun: 0,
                            wants: false,
                        };
                    }
                }
            }));
            let said = match &caught {
                Ok(()) => String::new(),
                Err(payload) => payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| {
                        payload
                            .downcast_ref::<&str>()
                            .map(|said| (*said).to_owned())
                    })
                    .expect("a message"),
            };
            assert!(
                said.contains(named),
                "a {bad:?} rig was not caught by \"{named}\": {said}"
            );
        }
    }

    /// R4-P1, written out: a host that answers once and closes, for ever. The budget returned on
    /// every answer, so the link was reopened a thousand times and never given up.
    #[test]
    fn r4_p1_a_flapping_host_is_given_up() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        let mut now = 0_u64;
        for _ in 0..1000 {
            now = now.saturating_add(STEP_US);
            rig.poll(now);
            match rig.link_action(SEAT) {
                Some(LinkAction::Connect) => rig.opened(SEAT),
                Some(LinkAction::GiveUp) => return,
                Some(LinkAction::Wait) | None => continue,
            }
            for frame in rig.poll(now) {
                if frame.link == SEAT {
                    let _ = rig.receive(
                        SEAT,
                        &right(id_of(&frame.text), &method_of(&frame.text), LULL),
                    );
                }
            }
            rig.closed(SEAT, Close::Connection);
        }
        panic!("a host that answers once and closes was never given up");
    }

    /// R4-P2, written out: every call closed on its message. A close on a message spent no budget,
    /// so the admin link's clock report was reopened and resent 239 times a minute.
    #[test]
    fn r4_p2_every_call_closed_on_its_message_is_given_up() {
        let mut rig = Rig::new();
        let mut now = 0_u64;
        for _ in 0..1200 {
            now = now.saturating_add(STEP_US);
            rig.poll(now);
            for link in [ADMIN, SEAT] {
                match rig.link_action(link) {
                    Some(LinkAction::Connect) => rig.opened(link),
                    Some(LinkAction::GiveUp) => return,
                    Some(LinkAction::Wait) | None => {}
                }
            }
            for frame in rig.poll(now) {
                rig.closed(frame.link, Close::ThisMessage(1009));
            }
        }
        panic!("a host that closes every call on its message was never given up");
    }

    /// R4-P5, written out: an unreadable view answer was re-asked on every poll (1 999 times in two
    /// seconds) and starved the status read.
    #[test]
    fn r4_p5_an_unreadable_view_answer_is_spaced() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        rig.opened(SEAT);
        let mut views = 0;
        for step in 1..2000_u64 {
            rig.seat_calls_left = SEAT_CALLS_PER_REFILL;
            for frame in rig.poll(step.saturating_mul(1000)) {
                let method = method_of(&frame.text);
                let id = id_of(&frame.text);
                let text = if method == "get_view" {
                    views += 1;
                    // A wrongly typed cursor: readable JSON, an answer this build cannot take.
                    wire(id, r#""next_cursor":5,"complete":true,"#, LULL)
                } else {
                    right(id, &method, LULL)
                };
                let _ = rig.receive(frame.link, &text);
            }
        }
        assert!(
            views <= 2,
            "an unreadable view answer was asked {views} times in two seconds"
        );
    }

    /// R4-P4, written out: the clock's and the recap's reasons were bare numbers; each now carries
    /// its cause for the string table.
    #[test]
    fn r4_p4_the_recap_says_its_cause() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        rig.opened(SEAT);
        rig.unfinished(SEAT, Purpose::Recap, &crate::rig::Unfinished::SendFailed(6));
        assert_eq!(rig.recap().refusal_cause, "send_failed");
        rig.unfinished(
            SEAT,
            Purpose::Recap,
            &crate::rig::Unfinished::TooLarge(5_000_000),
        );
        assert_eq!(rig.recap().refusal_cause, "too_large");
    }
}
