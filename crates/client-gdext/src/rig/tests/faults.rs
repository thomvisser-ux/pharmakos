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
        ADMIN, Close, KEEPALIVE_US, LinkAction, Phase, Purpose, RECONNECT_BACKOFF_US, RECONNECTS,
        Rig, SEAT, SEAT_CALLS_PER_REFILL,
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
    const STEPS: u64 = 900;
    const STEP_US: u64 = 50_000;

    /// Invariant 1's bound: a purpose may be sent at most this many times on a link in any
    /// keep-alive interval with no readable answer on that link between them.
    const MAX_UNANSWERED_SENDS: usize = 8;

    /// Invariant 2's bound: how long a pressed Ready may wait, counting only time its link was
    /// open, before it is sent or the Ready line says why not.
    const READY_WAIT_US: u64 = 4 * KEEPALIVE_US;

    /// A JSON-RPC answer with a footer: `extra` is the result's fields before `_status`.
    fn wire(id: i64, extra: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{{extra}"_status":{{"phase":"lull","phase_remaining_ms":180000,"round":1}}}}}}"#
        )
    }

    /// The right answer to `method`.
    fn right(id: i64, method: &str) -> String {
        let extra = match method {
            "get_view" => r#""next_cursor":"c9","complete":true,"#,
            "get_segment_feed" => r#""events":[],"next_cursor":"f1","#,
            "report_host_clock" => r#""all_ready":false,"#,
            "advance_push" => r#""advanced_ms":0,"#,
            "get_recap" => r#""prose":"x","seat_tallies":[],"#,
            _ => "",
        };
        wire(id, extra)
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
    #[allow(
        clippy::too_many_lines,
        reason = "one step of the model host after another, read in order; not a determinism lint"
    )]
    fn run(seed: u64, temper: Temper) -> Option<u64> {
        let mut random = Seeded(seed | 1);
        let mut rig = Rig::new();
        let mut watch = Watch::default();
        let mut delayed: Vec<(u64, usize, String)> = Vec::new();
        let mut opened_at: BTreeMap<usize, u64> = BTreeMap::new();
        let mut answered_since_open: BTreeMap<usize, bool> = BTreeMap::new();
        let mut now = 0_u64;
        for step in 0..STEPS {
            now = now.saturating_add(STEP_US);
            let healthy = temper == Temper::Recovering && now > 10_000_000;
            // The links, as host_link.gd drives them: the rig's policy says when to connect.
            for link in [ADMIN, SEAT] {
                let open = rig.links.get(link).is_some_and(|state| state.open);
                if open {
                    // A flapping host closes a link a second or so after it answered once.
                    if temper == Temper::Flapping
                        && answered_since_open.get(&link) == Some(&true)
                        && opened_at
                            .get(&link)
                            .is_some_and(|at| now.saturating_sub(*at) > 1_000_000)
                    {
                        rig.closed(link, Close::Connection);
                    }
                    continue;
                }
                match rig.link_action(link) {
                    Some(LinkAction::Connect) => {
                        if !healthy && temper != Temper::Flapping && random.chance(150) {
                            // The host refuses the reconnect: it closes again at once.
                            rig.closed(link, Close::Connection);
                        } else {
                            rig.opened(link);
                            opened_at.insert(link, now);
                            answered_since_open.insert(link, false);
                        }
                    }
                    Some(LinkAction::GiveUp) => {
                        watch.gave_up.get_or_insert(step);
                    }
                    Some(LinkAction::Wait) | None => {}
                }
            }
            if watch.gave_up.is_some() {
                return watch.gave_up;
            }
            // The player.
            if random.chance(20) && rig.phase == Phase::Lull {
                rig.ready();
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
                if rig.links.get(link).is_some_and(|state| state.open) {
                    let _ = rig.receive(link, &text);
                    answered_since_open.insert(link, true);
                    watch.unanswered.retain(|(at, _), _| *at != link);
                }
            }
            // The calls, and what the host does with each.
            for frame in rig.poll(now) {
                let method = method_of(&frame.text);
                let id = id_of(&frame.text);
                let link = frame.link;
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
                let fault = if healthy || temper == Temper::Flapping || !random.chance(250) {
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
                        _ => Fault::Delay(random.below(800_000)),
                    }
                };
                let ledger_before = rig.ledger();
                match fault {
                    Fault::Answer => {
                        let _ = rig.receive(link, &right(id, &method));
                        answered_since_open.insert(link, true);
                        watch.unanswered.retain(|(at, _), _| *at != link);
                    }
                    Fault::Delay(by) => {
                        delayed.push((now.saturating_add(by), link, right(id, &method)));
                    }
                    Fault::Refuse => {
                        let _ = rig.receive(
                            link,
                            &format!(
                                r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32000,"message":"slow down","data":{{"code":"RATE_LIMITED"}}}}}}"#
                            ),
                        );
                        check_words(&rig, seed, step, &method, "refused");
                    }
                    Fault::Unreadable => {
                        let _ = rig.receive(link, &format!(r#"{{"jsonrpc":"2.0","id":{id}}}"#));
                        check_words(&rig, seed, step, &method, "unreadable");
                    }
                    Fault::Garbled => {
                        let _ = rig.receive(
                            link,
                            &wire(
                                id,
                                r#""next_cursor":5,"events":5,"advanced_ms":"x","all_ready":"x","seat_tallies":5,"prose":5,"#,
                            ),
                        );
                        if rig.ledger().unfinished > ledger_before.unfinished {
                            check_words(&rig, seed, step, &method, "unreadable");
                        }
                    }
                    Fault::PastBuffer => {
                        rig.closed(link, Close::ThisMessage(1009));
                        if rig.ledger().unfinished > ledger_before.unfinished {
                            check_words_closed(&rig, seed, step, &method, 1009);
                        }
                    }
                    Fault::Close(code) => {
                        rig.closed(link, Close::of_code(code));
                        if rig.ledger().unfinished > ledger_before.unfinished {
                            check_words_closed(&rig, seed, step, &method, code);
                        }
                    }
                    Fault::Drop => {
                        rig.closed(link, Close::Connection);
                    }
                    Fault::SendFailed => {
                        rig.send_failed(link, 6);
                        check_words(&rig, seed, step, &method, "send_failed");
                    }
                }
            }
            // Invariant 2: no hang. A pressed Ready is sent, or its line says why, within its bound
            // of link-open time; and the editor never waits on a call that is not in flight.
            if let Some(waited) = watch.ready_waiting.as_mut() {
                if rig.ready_failed().is_some() || rig.phase != Phase::Lull || !rig.wants.ready {
                    watch.ready_waiting = None;
                } else if rig.links.get(SEAT).is_some_and(|state| state.open) {
                    *waited = waited.saturating_add(STEP_US);
                    assert!(
                        *waited <= READY_WAIT_US,
                        "seed {seed:#x} {temper:?} step {step}: no hang: Ready pressed and neither \
                         sent nor said after {waited} us of an open seat link"
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
    fn check_words_closed(rig: &Rig, seed: u64, step: u64, method: &str, code: i64) {
        check_words_with(rig, seed, step, method, "closed_on", Some(code));
    }

    fn check_words(rig: &Rig, seed: u64, step: u64, method: &str, cause: &str) {
        check_words_with(rig, seed, step, method, cause, None);
    }

    fn check_words_with(
        rig: &Rig,
        seed: u64,
        step: u64,
        method: &str,
        cause: &str,
        code: Option<i64>,
    ) {
        let Some((purpose, named)) = rig.last_unfinished else {
            panic!(
                "seed {seed:#x} step {step}: true words: {method} {cause} was settled by no door"
            );
        };
        assert_eq!(
            named, cause,
            "seed {seed:#x} step {step}: true words: {method} settled as {named}, not {cause}"
        );
        match purpose {
            Purpose::Ready => assert_eq!(
                rig.ready_failed().map(|(name, _)| name),
                Some(cause),
                "seed {seed:#x} step {step}: true words: the Ready line"
            ),
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
            let gave_up = run(seed.wrapping_add(7), Temper::Flapping);
            assert!(
                gave_up.is_some_and(|step| step <= bound),
                "seed {seed}: gives up: a flapping host was given up at {gave_up:?}, past step {bound}"
            );
            assert_eq!(
                run(seed.wrapping_add(1000), Temper::Recovering),
                None,
                "seed {seed}: a host that recovers was given up"
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
                    let _ = rig.receive(SEAT, &right(id_of(&frame.text), &method_of(&frame.text)));
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
                    wire(id, r#""next_cursor":5,"complete":true,"#)
                } else {
                    right(id, &method)
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
