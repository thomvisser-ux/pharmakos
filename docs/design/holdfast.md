<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Holdfast: trust, rotes and the three towers

This is the design of how a seat's machines behave and how a seat comes to trust them: the rote
a machine runs, the grades of trust a Stake earns, the buried cable that joins Stakes into a
network, and what losing a match now means. It is the design that S4's first half, "the three
towers", builds (`s4-plan.md`), and the direction for what S4's second half adds.

Every rule here carries one of three marks:

- **Ruled**: the owner's answer, recorded in `decisions-log.md` item 229 (or the earlier item
  named) with its point number. The owner's words are quoted there, not here.
- **Decided**: the main session's engineering or design decision, made on those rulings. The
  owner can veto each one.
- **Open**: not decided. Section 20 lists every open question.

Nothing here is built. Every duration, price, range and depth is a rules-table row whose value is
set when the slice that needs it is built, and measured by the AI play harness before the slice
closes; none is given here. Where this document and the spec disagree, the decisions log, and so
this document's rulings, win (`README.md`, "Precedence"); the spec's body is brought in line by
the pull requests that build each part. The AI review that shaped this design is not independent
review.

## 1. Words used here

The spec's own words are used as the spec uses them.

- A **Stake** is a beacon. Its sealed heart is the **key-core**, whose lit **seal** shows that it
  holds power. Its sphere is its **holding**.
- The commander is a **Keybearer**. In the spec's words, a Keybearer "carries an air-gapped key
  and presses it to a Stake's key-core by hand".
- The **Ledger** pays, inspects every sealed playbook before the Push, and settles accounts at
  each **settlement** between rounds.
- A unit's or a structure's program is a **rote** (the spec's term table already maps "Program"
  to "rote"). A rote is a short numbered list of **cards**.
- **Holdfast** is how firmly a seat holds a place: the grade of trust a Stake has earned
  (ruled: item 229, point 56). A Stake's holdfast is one of the **grades** of section 6.
- A **press** is the Keybearer pressing the key to a key-core on site, which is the one way a
  seat raises a Stake's holdfast (section 3). A **vouch**, a **commission** and (with S5) an
  **oath** are kinds of press.
- A **mark** is a claim that one machine passes to others, such as "contact at this place,
  challenged and unanswered" (section 4).
- **Cable** is buried wire that joins two Stakes and carries marks (section 8).

The spec calls the air gap "the commander's legs": orders reach a Stake only by a body that walks
there. Holdfast lives inside that air gap. It never sends a key, a grade or an order over any
medium.

## 2. The form, in four sentences

*Decided, on the owner's points 20, 28 and 36 (items 226 and 227) and point 57 (item 229).*

1. Every machine runs a **rote**: a short numbered list of **cards**, each "when this reading, do
   that". The machine reads the list from the top on every beat, and the first card whose
   reading is true acts. Everything a machine does is on a card the player can open: there is no
   hidden behaviour behind the list, and a Sentry with no true card holds its fire.
2. A card reads only what that machine's own sensors read, and acts only through what that
   machine has. Nothing arrives clever: a card may use only the words that the grade in force at
   its Stake allows (section 6).
3. The commander's **playbook** is unchanged, and it is also how a rote travels: the commander
   installs a rote at a Stake by hand, as one more row of a visit, and the sealed playbook
   carries the rote inside it.
4. One Ledger, one recap: the Ledger inspects every card before the Push, and when anything dies
   the recap names the machine, the card and the reading it fired on.

A rote is data with a closed vocabulary, like a playbook: it cannot loop, read a file or run
code, it is bounded in size and cost, and it always finishes (ruled: item 227, point 28; AGENTS.md
section 2). If code ever comes (S6 "Library" names "the step from cards to code", item 227, point
37), it is another way to write the same tables.

## 3. The five rules

*The model is ruled as the option the owner picked described it: "Pressed by hand, in force by
links, a fall is authored, and surety with its price" (item 229, point 57). The rules' wording is
decided.*

1. **Pressed by hand.** Every rise in holdfast begins with a Keybearer's press at a Stake. There
   is no other source of trust during a match. Two exceptions are stated where they happen: one
   press may cover more than the Stake it is made at (surety, section 7), and a match setting may
   start the founding Stake with a vouch already on its record (section 10).
2. **In force by links.** A Stake's **pressed record** is what hands have pressed there, and it
   is never lost while the Stake stands. Its grade **in force** is what that record, and its
   neighbours' records, support right now. A link can lower the grade in force below the pressed
   record (a cable is cut) and one link can raise it above (a neighbour's surety).
3. **A fall is authored, never a surprise.** A card that uses a word above the grade in force is
   skipped, and the machine reads on down its list. The cards below are the rote's fallback,
   written by the player and shown by the editor as "what this rote does at each lower grade".
   The Ledger warns before the Push when a fall would leave a machine silent, or raw near
   friends.
4. **Every step up is the remedy and the poison.** Each grade gives the seat a power and the
   enemy a target: a guarantor to kill, a cable to cut and, once radio arrives, a broadcast to
   overhear. This rule is weak until radio arrives; section 18 says so.
5. **Everything is on the record.** Every rise and fall, and every skipped card that would
   otherwise have acted, is a named event with its cause, written once when it starts, shown on
   the machine during the Push and in the recap.

## 4. Keys, challenges and marks

*Decided.*

- **The press key** is the Keybearer's. It never leaves a hand and is never sent. Pressing it to a
  key-core is the one act by which a seat raises a Stake's pressed record. The key dies with its
  bearer: an enemy cannot take it.
- **A challenge** is a short sealed question and answer between a machine and what it sees, by
  line of sight. It uses no network and cannot be overheard. Every unit of a seat, and its
  Keybearer, answers that seat's challenges: a unit is given the answer where it is made. Units
  hold no grade of their own.
- **A mark** is data, never an order: a card may weigh it. Marks travel by buried cable
  (section 8), and nothing else carries them until radio arrives. A mark will carry the medium
  that brought it, so that once radio exists a card can trust cable and doubt radio.

## 5. Who holds what

*Decided.*

| Holder | Holds (all hashed and saved) |
|---|---|
| **A Stake** | Its pressed record: vouched by hand; standing surety (after the three towers); commissioned, that is, on the seat's roll (section 8); later, sworn to which commander, and bound. Also the rotes installed there by hand for each kind of machine; the last installed is the one that runs. |
| **A cable** | Its two ends, its run, and whether it was whole when last reported, so that a cut is reported once. |
| **A seat** | The mark up to which the Ledger has **entered** its Stakes: one number, because Stakes are numbered as they are driven. |
| **A Keybearer** (S5) | A rank: what that commander may press. |
| **A rote** | Nothing: it is data. The editor shows the grade its highest card needs. |
| **A machine** | Nothing. It runs at the grade in force at its Stake, with the rote last installed there for its kind, or its kind's basic rote if none was. |

The grade in force is **not stored**. The sim works it out from the pressed records, the cables
and the map once on every tick, so it can never be stale: a Stake lost in one tick's fighting
changes its neighbours' grades from the next tick.

## 6. The ladder

*The ladder's shape and steps are ruled (item 229, point 49), and so are its names (point 53).
What each grade unlocks and how it is earned is decided.*

Five grades, and two fallen states that arrive with radio:

| Grade | Unlocks | Earned | Built |
|---|---|---|---|
| **Unkeyed** | Raw words only: motion in the arc, range. | By deploying. Never lost. | The three towers, slice 1 |
| **Vouched** | **Challenge**. | A vouch press on an entered Stake, or surety (section 7). | The three towers, slice 2 |
| **Commissioned** | **Mark**. | A commission press on a Stake vouched by hand. | The three towers, slice 3 |
| **Sworn** | A commander's style may choose among the rotes installed there. | An oath press. | S5 "Commanders" |
| **Bound** | A live mind acts there. | Designed with S5. | S5 "Commanders" |

**Unkeyed.** A newly driven Stake. Its machines can sense and shoot, and the Ledger lets no
advanced logic run there. A raw Sentry fires at once at anything that moves in its arc, its own
seat's drones and units included. It is the best stopper of a fast raider and the most dangerous
thing near a seat's own traffic. *The decision:* where each Sentry faces. Whether a raw Sentry
fires on its own seat's Keybearer is open (section 9).

**Vouched.** The Keybearer has come back and vouched for the place. *Earned* by hand: the Ledger
must have **entered** the Stake at a settlement (a Stake driven this round is not yet in its
books, except under the Warm start setting of section 10), and then a vouch press on site takes a
few seconds. Or on surety (section 7). *Lost* only if it rested on a surety that fell. *The
decision* is in the cards, not the grade: a challenge takes time to answer, so a fast mover can
cross the arc before the answer is read. "Fire on motion" stays right where nothing friendly
passes; "challenge, then fire" is right where friends work.

**Commissioned.** The Stake is on the seat's network. A machine there may post a contact it has
challenged without an answer as a mark, and may act on other Stakes' marks: a Sentry may fire on
a marked contact inside its gun's range and outside its own arc. *Earned* by a commission press on
a Stake vouched by hand, which puts the Stake on the seat's roll (section 8). *In force* while
Vouched is in force there and whole cable joins the Stake to at least one other Stake where
Commissioned is in force. A Commissioned Stake joined to nobody works as Vouched until it is
joined, and the Ledger warns of it before the Push. *The decision:* which Stakes are worth a
commission and a cable, and which stay simple.

**Sworn** (S5). The Stake is sworn to one named commander. While the oath is in force, that
commander's style may choose which of the rotes installed at that Stake by hand is the one that
runs, without a visit. It chooses; it installs nothing new. *Earned* by an oath press from a
commander of enough rank, on a Commissioned Stake. *In force* while Commissioned is in force there
and the commander is on the roster. An oath that is out of force stays on the record, and the
Stake works as Vouched until Commissioned is in force again.

**Bound** (S5). Full integration: the step at which a live AI acts. The owner's point 17 (item
226) places active AI involvement as a late-game upgrade. Its form is open (section 16).

**Lapsed and Outcast** are reserved names. Both describe what happens to a Stake whose radio
traffic an enemy has turned against it, so both arrive with radio (section 11).

## 7. Surety: one press that covers neighbours (after the three towers)

*Surety is ruled, with its price, and is built after the three towers (item 229, point 57); its
price was settled by network access control (point 69). Its mechanics are decided.*

A Keybearer may **underwrite** a Stake it has vouched by hand: one more press there. The pledge
is a **surety**; the Stake that carries it is a **guarantor**; a Stake it covers is its **ward**.
From then on, any entered Stake of the seat that stands inside the guarantor's holding, and has no
hand vouch of its own, has Vouched in force with no walk. This is rule 1's one extension: the hand
pressed a pledge at the guarantor that covers its holding, and the Ledger accepts it.

- **It is worked out live.** A ward has Vouched in force while it stands inside the holding of any
  standing guarantor. Under two guarantors, it falls only when the last is gone, and a guarantor
  that comes to cover it restores it on the next tick. If the guarantor's holding shrinks (its
  Resonance Spire is lost), wards left outside fall, and the Ledger's warning names the Spire.
- **One step only.** A ward cannot itself be underwritten, because it has no hand vouch. Every
  Stake is at most one step from a hand.
- **When a guarantor falls,** its wards drop to Unkeyed on the next tick. Their Challenge cards
  are skipped, and each rote does whatever its lower cards say (rule 3). The Ledger warned before
  the Push, for example "B3's Challenge rests on one guarantor, B1. If B1 falls, B3's Sentry fires
  on motion (card 3)".
- **The price.** Surety never admits a Stake to the network (ruled: point 69): a ward cannot be
  commissioned until a hand vouches for it, and a ward later vouched by hand stops being a ward.
  Surety buys Challenge for a whole cluster with one walk; the network is only for Stakes a hand
  has vouched one by one. Whether the guarantor itself may be commissioned is open (section 20).

## 8. The network: buried cable

*Ruled: buried cable is the network for the three towers' third slice, with almost no
counterplay until radio arrives (item 229, point 65, on the owner's points 59 and 64); network
access control is ruled as below (point 69). The engineering is decided.*

**What a cable is.** A run of cable voxels, one per column, a set depth below the surface, between
two of a seat's Commissioned Stakes. It is its own kind of build target and its own material, not
a wall: a wall is an upright box at least two voxels high, and enemy raiders look for walls to
breach. Build drones dig the trench, lay the cable and cover it, at a price and a time per voxel
of length, so only a Stake with a Build writ lays or re-lays cable, and only inside its own
holding. The orders name the two ends, and the run follows the ground between them.

**What it carries.** Marks, between the Commissioned Stakes it joins and onward through every
Commissioned Stake cabled to those. It carries no key, no grade and no order.

**It cannot be overheard or spoken into.** Nothing on a cable is on the air, and nothing in this
design taps one.

**It is hard to cut, and cutting it is deliberate.** Units walk over it. Light fire does nothing
to it. It is cut only when the ground is removed down to its depth: a demolition charge, a crater
deep enough, or digging. A seat's own mining and terraforming leave its own cable alone. A cut is
a named event that says where ("the cable B1 to B3 was cut at 4:12"); the Stakes beyond the cut
leave that part of the network on the next tick; and build drones re-lay the missing length if
the Build writ is set to rebuild.

**It cannot be seen** by an enemy: it is under the ground. An enemy can guess which Stakes are
joined; it cannot read the network off the map.

**One network.** A seat's network is its Commissioned Stakes and the whole cables between them. A
mark posted at one Stake reaches every Stake joined to it by any chain of whole cables. The sim
works the joins out once on every tick from the cables' voxels.

**What the player decides.** Which Stakes are worth joining; where the runs go (inside which Build
Stake's holding, across which ground an enemy might dig or shell); and whether a second run is
worth its price, so that one cut parts nothing.

**Honestly:** buried cable is very safe. In the three towers the network has almost no weakness
short of killing the Stakes it joins, or a deliberate charge on a run the enemy has guessed. Risk
comes with radio.

### Network access control

*Ruled (item 229, point 69): default deny; admission by hand, Stake by Stake; a continuous check
with automatic quarantine; inherited trust never admits; allow-lists in cards; a stricter,
separate admission for radio when it is designed.*

- **The network refuses by default.** Being cabled is not enough, and laying cable admits nobody:
  a cable to a Stake that is not on the roll carries nothing.
- **Admission is by hand, Stake by Stake.** The commission press is the admission: the Keybearer,
  on site, at a Stake it has vouched by hand, has the Ledger put that Stake on the seat's
  **roll**. Inherited trust never admits: a ward, vouched only on a neighbour's surety, cannot be
  put on the roll until a hand vouches for it.
- **Admission is checked all the time.** A Stake on the roll is on the network only while Vouched
  is in force there and whole cable joins it to another Stake on the roll. A Stake that fails the
  check is **quarantined** at once: it sends no marks, receives none, and works as Vouched. It
  returns by itself when the check passes again, and nothing has to be pressed twice.
- **Allow-lists in cards.** What an admitted Stake may do is set by its grade and its cards. A
  card may also name whose marks it will act on ("marks from B2 or B3"), so a seat can keep parts
  of its network from trusting each other. That costs nothing while cable is the only medium; it
  is the habit that limits the damage of one bad Stake when radio arrives.
- **Striking a Stake from the roll** is a press on site by its own Keybearer (decided).
- **Radio's admission is stricter and separate**: being on the roll by cable does not put a Stake
  on the air. It is designed with radio.

## 9. Losing: the last Keybearer

*Ruled, for now: a seat whose last Keybearer is eliminated is defeated (item 229, point 68). The
rest of this section is the main session's reading and additions (decided; the owner can veto).*

With one Keybearer per seat today, that is its Keybearer's death; when the roster arrives (S5),
it is the death of the last one. There is no respawn, so the respawn delay, the death counter and
the playbook's on-death options go. The existing defeat rule stays beside it: a seat with no
Stakes left is also defeated.

What follows from it:

- **The Keybearer is the piece a seat cannot lose.** Every walk to press a key is a risk taken
  with the whole match, and hunting the enemy's Keybearer is a way to win.
- **It changes how a match ends,** so it moves every recorded match, and it is built as its own
  reviewed change, early in the three towers (`s4-plan.md`, step 1).
- **Whether a raw Sentry fires on its own seat's Keybearer is open**: the owner deferred it (item
  229, point 70). The build keeps the choice as one rules-table value, so either answer is a change
  of data. The first build carries "does not fire" (the main session's choice, told to the owner):
  the spec says the key-core "only cares that it is physically present", so it senses the key, and
  even an Unkeyed Stake's machines stay quiet for the bearer of their seat's key. Without that,
  walking into one's own arc would lose the match on the spot, sometimes with no warning, because
  the Ledger's arc warnings are rough. A seat's drones and units stay in danger from its raw
  Sentries either way.

## 10. The first round is a match setting

*Ruled (item 229, point 58): the first round is a match setting with three values; the newcomers'
Probation match plays Home vouched and a standard match plays Cold start; the lobby can change it
per match.*

| Setting | The founding Stake | Other Stakes | Round 1 |
|---|---|---|---|
| **Cold start** | Unkeyed | Entered at a settlement, then vouched | Raw everywhere, at home too. |
| **Home vouched** | Starts with a hand vouch on its record | Entered at a settlement, then vouched | Safe at home; raw at each outpost. |
| **Warm start** | Starts with a hand vouch | Entered the moment they are driven; may be vouched on any later visit | Trust costs seconds. For short or casual matches. |

*Decided:* a rules row names the default and the match setting overrides it. It is the first
lobby option that changes the match itself, so it is part of the match's hashed settings: written
with the seed to the match's file (`match.json`), to the config line the lobby remembers and to
the save, and given to the Ledger, the built-in opponents and the wizard. These are contract
formats (AGENTS.md section 5), and one contract pull request names all of them. Home vouched and
Warm start are the setting's two admitted exceptions to rule 1: a vouch that no press in the match
made.

## 11. Keeping it, now and when radio arrives

**Now.** The enemy strikes trust and the network with the weapons the game already has: kill a
guarantor and its wards drop; kill a Stake and the network loses it; cut a cable, if it can find
one; hunt the Keybearer, and every press waits (and, by section 9, the match may end).

**When radio arrives.** *Ruled: radio is deferred out of the three towers and arrives in S4's
second half, when emissions matter and each machine knows only what its own sensors pick up and
what is sent to it as a mark (item 229, points 60 to 63).* What follows is direction only, the
main session's, to be designed in detail then and brought to the owner as its own batch:

- Radio is reach without cable: across open ground, and to anything that moves. It needs the
  spec's Signal spar (the Radio Mast), which stays where the spec has it and is not in the three
  towers. Radio is the only medium that can be overheard.
- A Stake that broadcasts can be overheard by an enemy listener. The Ledger then declares its key
  **Lapsed**: the Stake is off the radio, and keeps its cables, until a Keybearer re-keys it on
  site. A Stake may listen without broadcasting, which cannot be overheard.
- A Lapsed Stake, and only a Lapsed Stake, can be tampered with by an enemy Keybearer who walks in
  and stands at it. If that finishes, the Ledger casts the Stake out: it is **Outcast**, back to
  raw cards, until its own Keybearer purges it. This is the title's rite: the compromised node is
  driven out so that the network stays clean.
- Marks carry the medium that brought them, so a card can trust cable and doubt radio.
- Kept for that batch: what radio is for when cable reaches everywhere a seat holds (one answer
  proposed in review: anything that moves hears marks only by radio); forged marks; and whether a
  seat may cast out its own Stake.

## 12. The Sentry, raw cards, install and the arc

*Ruled: the three towers adds only the Sentry; every gun moves onto cards in S4's second half
(item 229, point 51). The Sentry's design is decided.*

- **The Sentry** is a new tower: a gun and a motion sensor that covers a quarter-circle in front
  of it. It faces north, east, south or west, set when it is placed. It cannot tell friend from
  foe by itself: it uses whatever its Stake's grade in force allows.
- **Its basic rote**, `sentry.basic`, is one card: "when something moves in the arc, fire". A
  Sentry's rest state is to hold its fire, so this one card is the simple behaviour, in plain
  sight, and no hidden default ever hands "fire" back.
- **Its friend-or-foe rote**, `sentry.iff`, needs Vouched: "when a contact answers, hold fire;
  when something moves in the arc and has not answered, fire". A raw Sentry fires at once and hits
  anything; a challenging Sentry is safe for friends and slower. Neither is simply better.
- **Its marking rote**, `sentry.mark`, needs Commissioned: it posts unanswered contacts as marks,
  and may fire on a marked contact inside its gun's range but outside its own arc. Two or more
  cards may then act on the same beat (mark it and shoot it), so the rule "the first true card
  wins each of fire and signal" arrives with Mark, not before.
- **Install.** The player opens a rote in the editor's rule list, edits or reorders its cards, and
  the commander installs it at the Stake by hand, as an interface row. A rote the player wrote can
  be saved to the player's local library and loaded in a later match.
- **The arc** is drawn on the map in the editor and during the Push. The Ledger warns when an arc
  may cover the seat's own Stakes, seams or route, with a Fix that turns the Sentry. Turning a
  standing Sentry is an interface row, not a rebuild.
- **The recap line**: "Sentry 2 fired on card 1 (motion in arc) and destroyed your Scout." A
  Ledger memo explains a seat's first friendly fire.

## 13. What the player sees

*Decided.*

- **On the map,** each Stake carries one holdfast mark beside the seal (which still means power):
  the grade in force. A second, hollow mark appears only when the pressed record is higher: "B2:
  pressed Commissioned, in force Vouched. The cable to B1 was cut at 3:40." A pip shows "entered:
  may be vouched". A surety is drawn as a dashed line from guarantor to ward, and a seat sees its
  own cables drawn on its own map.
- **Through fog,** an enemy that sees a Stake sees its grade in force, a lit mark like the seal.
  It does not see cables, sureties or holdfast events: those are private to their owner.
- **In the editor,** a rote is shown as it runs at each grade: "At Vouched: cards 1, 2, 3. At
  Unkeyed: card 3 only." A card above the Stake's grade is marked, with a Fix that adds the vouch
  to a visit the route already makes.
- **Before the Push,** the Ledger *refuses* only what it is certain can never work: a vouch on a
  Stake it has not entered; a commission on a Stake where no hand vouch can come first; a cable
  whose ends are not both the seat's Stakes, or whose run on the map leaves the holding of the
  Build Stake that is to lay it. Where a vouch may or may not come first (it sits behind a
  condition, say), the Ledger warns, and the press succeeds or fails in the Push with its recorded
  reason. The Ledger *warns* ("may") about: a rote installed above its Stake's grade ("cards 1 and
  2 are skipped until B4 is Vouched"); a grade resting on one guarantor or one cable; a
  Commissioned Stake joined to nothing; a raw arc, now or after a fall, that covers one of the
  seat's own structures, a writ's working area or a cable run; and a straight leg of the
  Keybearer's route, between two places the orders name, that crosses one of its seat's raw arcs.
  These are map geometry: the Ledger never works out a real path, reads the ground, steps the sim
  or predicts what will walk where (AGENTS.md section 3, rule 2). What it cannot know (a visit
  aimed by a description such as "the nearest"; a run that dips out of the holding where the
  ground falls away) succeeds or fails in the Push with a recorded reason.
- **During the Push and in the recap:** "B1 was vouched by hand." "Guarantor B1 fell at 4:12: B3
  and B4 dropped to Unkeyed." "The cable B1 to B3 was cut at 4:12: B3 is off the network." And
  for the tower that did nothing, on the tower and in the recap: "Sentry 4 at B3 saw a Raider at
  4:20 and stayed quiet: card 2 needs Vouched; B3 is Unkeyed since B1 fell."
- **A newcomer's first match** (Home vouched): the core's Sentry runs the friend-or-foe rote from
  the start. The first outpost's Sentry runs the basic rote, its arc drawn on the ground, with the
  Ledger warning if the arc covers the home Stake's working area. A memo explains the first
  friendly fire. The recap says the outpost was entered. Round 2's wizard offers to add the vouch
  and the friend-or-foe rote to the visit the route already makes. Two or three new ideas a round.

## 14. Questions a player can ask

| Question | Answer |
|---|---|
| What makes a unit answer a challenge? | It belongs to the seat. Units are given the answer where they are made. |
| Does a raw Sentry fire on its own Keybearer? | Open: the owner deferred it (item 229, point 70). The first build is set to "no", as one rules-table value (section 9). A raw Sentry does fire on its seat's drones and units. |
| What happens when my Keybearer dies? | If it was the seat's last, the seat is defeated (section 9). Nobody comes back. |
| What rote does a new machine run? | The one last installed at its Stake for its kind; otherwise its kind's basic rote. |
| A Stake under two guarantors loses one. | Nothing changes until the last is gone. |
| The guarantor's Spire is lost and its holding shrinks. | Wards left outside drop to Unkeyed on the next tick. |
| Can a ward join the network? | Not until a hand vouches for it (item 229, point 69). Whether a guarantor can is open. |
| A ward is later vouched by hand. | It stops being a ward. It keeps Vouched if its guarantor falls, and may be commissioned. |
| A cable is cut. | The Stakes beyond the cut leave that part of the network on the next tick. A Stake with no join left works as Vouched. Build drones re-lay the length if the writ is set to rebuild. |
| Can a cable be seen, tapped or cut by accident? | No. It is buried: an enemy cannot see it, nothing taps it, and only removing the ground to its depth cuts it. A seat's own mining and terraforming leave it alone. |
| Can a cable cross ground I do not hold? | No. Its run must lie inside the holding of the Build Stake that lays it. Open country between two clusters is crossed by driving Stakes across it, or later by radio. |
| Can a Stake with no Build writ lay or mend cable? | No. A Build Stake whose holding contains the run does it. |
| The Stake at one end of a cable falls. | The cable joins nothing and is left in the ground. A new Stake driven there must be vouched, commissioned and named as the end of a run again. |
| A Stake is browned out. | It keeps its record (its key-core stays lit, as today) and its machines are parked, as today. Its cables still carry marks through it, and a dark guarantor still stands surety. |
| Can the enemy take the key from a dead Keybearer? | No. The key dies with its bearer. |
| The founding Stake falls. | The seat lives while it has a Stake and a Keybearer. Holdfast needs nothing from the core: the key is the Keybearer's. |
| Is a cabled Stake automatically on the network? | No. It must be on the roll (the commission press), with Vouched in force. A cable to a Stake that is not on the roll carries nothing. |
| A seat has only one Commissioned Stake. | It works as Vouched, and the Ledger says so before the Push: a network needs two. |
| A Sworn Stake loses every join. | The oath stays on the record and is out of force: the Stake works as Vouched until Commissioned is in force again. |
| Can the Ledger refuse to enter a Stake? | No. Every Stake standing at the settlement is entered, lit or dark, attacked or not. |
| Can the enemy see which Stake is my guarantor? | Not directly. It sees grades in force on Stakes it can see, and can guess from the map. |
| Does trust carry between matches? | No. Rotes do: a player's rotes are files in their library. Whether anything else carries over is open (section 20). |
| Does a commander's rank carry between matches? | Open, for S5 (section 20). |

## 15. Engineering decisions

*Each is decided by the main session; the owner can veto any of them.*

- **The grade in force and the network's joins are derived** once a tick, after the world
  settles, and never stored. Surety's reach is one more named reader of a holding's radius. What
  is stored per cable is only what a once-only report needs (whether it was whole when last
  reported), so each change is one event.
- **An installed rote is match state**: stored per Stake, hashed, saved and restored, so a resumed
  match runs the same Sentries. New tables follow the rule that a column added to a table may be
  hashed as a block after its rows (AGENTS.md section 4.8, decisions-log item 136), so a match that
  never fills them keeps its chain.
- **Readings.** A Sentry reads contacts as they stand on this beat after movement, as every gun
  does; it reads challenge answers and marks only from earlier beats, so the order machines fire
  in can never change a result.
- **No hidden default.** A Sentry's rest state is to hold its fire. There is no compulsory last
  card that hands "fire" back to a built-in default.
- **The cost of a card is counted honestly.** Today a rule costs one size unit however large its
  condition. A card's conditions count, a card-heavy case joins the verifier's timing gate before
  any claim is made about it, and the number of Sentries a seat can stand is bounded by the
  structure table's room.
- **The Ledger works grades out with the same shared function the sim uses**, over the frozen
  snapshot, and every plan-time refusal has a run-time twin with its own recorded reason.
- **The Ledger's arc warnings say "may"**: it checks straight legs between the places a playbook
  names, holds no pathfinder and never predicts what will cross an arc (section 13).
- **A press row is safe to repeat**: a visit interrupted and resumed pays again and presses
  nothing twice. Install and press take the last two short interface-row numbers.
- **The settlement's new act (enter) runs after the licence check.**
- **Grade and cable events are private to their owner.** Shipped event names keep their names; new
  kinds and reasons are added at the end (AGENTS.md section 5, the event vocabulary).
- **A cable is its own build target kind and its own material**, not a wall, with free material
  ids added; its name in code is `cable`, because "wire" already means the protocol format there.
  Only a Build Stake lays it, inside its own holding; a seat's own Build and Mine digs skip its
  cable voxels.
- **The reserved proto seam is left alone.** Installing a rote is a new interface row; the
  `program {builtin|script}` seam carries nothing (AGENTS.md section 2).
- **Every duration, price and depth is a rules row**, stamped into the rules hash.
- **Easy uses the same rows as a player**, uses only the shipped rotes (the same files a player can
  open), places Sentries facing away from its own holdings, and is recorded, not tuned, while
  balance is paused (item 227, point 35).

## 16. Later stages

**S4's second half.** Surety (section 7); every gun onto cards (item 229, point 51); each machine
sensing for itself; then radio with the Signal spar, and the security phase of section 11. It is
planned when the three towers closes (`s4-plan.md`, section 8).

**S5 "Commanders".** A commander holds rank, not holdfast. Rank decides what a commander may
press: any Keybearer vouches; a veteran underwrites, commissions and takes oaths (decided, for
S5's plan to confirm). Each Stake records who pressed it. The roster becomes who walks where, and
the defeat rule of section 9 counts the last of them.

**Bound (S5).** Open. Two candidates, both keeping touch to change:

- (a) The live mind takes residence in one key-core (the spec: "A human or a mind may pilot one;
  the key-core only cares that it is physically present") and may choose and rewrite rotes at
  that Stake only. It is the seat's most valuable target.
- (b) The live mind is attached to a Keybearer and makes that commander's on-site choices; its
  words reach a key-core only through a body that walks there.

An outside program at a seat is already approved, as the external seat (item 229, point 44). What
Bound adds is acting during a Push, which changes the rule "no manual control" (AGENTS.md section
11) and is the owner's when S5 is planned.

**Units.** When units run cards, a later stage, a unit runs at the grade in force at its home
Stake.

## 17. How it is built

The three towers, S4's first half, is built in three playable slices, after the external seat and
the AI play harness and the defeat rule (`s4-plan.md`):

- **Slice 1:** the Sentry, raw cards, the install row, friendly fire, the recap line. Every Stake
  behaves as Unkeyed.
- **Slice 2:** the pressed record (the hand vouch), the Ledger's entry at the settlement, the
  vouch press, the match setting, Challenge, the skipped-card rule and its events, the Ledger's
  refusals, warnings and Fixes, the holdfast mark.
- **Slice 3:** buried cable (a new build target and material), the commission press and the roll,
  the network's joins and quarantine, Mark.

## 18. What is true only later, said plainly

- **Until every gun runs cards (S4's second half), holdfast governs only the Sentry.** The
  autocannon still identifies friends perfectly and for free, so a player can win without
  vouching anything, and often should. The owner accepted this (item 229, point 51). The three
  towers proves the mechanics (cards, friendly fire, press, Challenge, cable, Mark, the recap's
  trace) on one tower; it does not yet make trust decide matches. What a Sentry buys in the
  milestone is its price (a cheaper tower, a rules row) and the player's own cards.
- **Until surety and radio, a higher grade is an upgrade, not a trade.** In the three towers
  Vouched costs seconds and exposes nothing, so a player vouches every Stake the route passes. The
  decisions are in facing, in the cards (fire at once, or challenge and wait) and in where cable is
  worth laying.
- **Mark shares nothing new until each machine senses for itself.** Today a seat's units already
  share one map. In the three towers Mark's one use is firing outside a Sentry's own arc.
- **Buried cable has almost no counterplay.** That is the owner's default secure option (item 229,
  point 65 accepted it). The risk side of the network arrives with radio.

## 19. Words, and known weak points

**Words.** *Holdfast is ruled (item 229, point 56); the grade names are ruled (point 53); every
other coined word is decided and the owner's to keep or change.*

- **Holdfast** is the noun for the trust: "B1's holdfast rose to Vouched." It sits in a crowded
  family (the holding, the Hold writ), so text never uses "hold", "held" or "holding" for a grade
  or for a quiet machine: a grade is **pressed** or **in force**, and a machine that does not act
  **stays quiet**.
- **Grades and states:** Unkeyed, Vouched, Commissioned, Sworn, Bound; reserved for radio: Lapsed,
  Outcast. "Probation" is the newcomers' lobby match only.
- **Coined here:** press (the act; vouch, underwrite, commission and oath are its kinds), entered,
  surety, underwrite, guarantor, ward, roll, quarantine, cable, and the settings Cold start, Home
  vouched and Warm start. "Cable", not "line", because the Handfast Line is a faction; not
  "conduit", because the live conduit is a planned feature; not "wire" in code.
- **"Licence"** stays, for now, the Ledger's unlock of a structure for a seat (the Resonance
  Spire). It is a different mechanism from holdfast, and it no longer unlocks any card.

**Lore checks owed at the lore pass** (open): "Holdfast: Nations At War" is, from memory and
unchecked, a published game's title; here holdfast is a term inside the game, not a title.
"Sentry" is to be checked against the spec's banned "Sentinels". "Mark" already names the
commander's own broadcast of a target; a Stake's mark is the same idea from a machine, and the
lore pass decides whether they share the word.

**Known weak points.** Each is measured by the AI play harness before its slice closes
(`s4-plan.md`, section 7).

1. **A Sentry stands beside guns that still identify perfectly** (section 18), so a player may
   never build one, and "never vouch" may be the best line in the milestone. The cure is that
   every gun runs a rote, which is S4's second half.
2. **Round 1 could be nothing but friendly fire.** Only a seat's own drones move in round 1. The
   arc drawing, the Ledger's warning with its Fix, and the memo are there for this.
3. **One rote may dominate.** If "challenge, then fire" is always right once unlocked, the cards
   are an upgrade path, not a choice. The reply delay is the first counterweight.
4. **A fall can still fail open.** A rote whose only card needs Vouched goes quiet when its grade
   falls; a rote with a raw card below it brings friendly fire back at the moment of crisis. Rule
   3 and the Ledger's warnings make both visible before the Push; they do not stop a player
   choosing either.
5. **Buried cable has almost no counterplay**, so in the three towers the network is nearly free
   to keep once laid (section 18).
6. **The arc warnings are rough.** They are straight-line geometry between named places and can
   miss a real path (section 13).

## 20. Open questions

Each is the owner's, and is held in `decisions-log.md` item 229 (12) unless it says otherwise.

1. **Whether a raw Sentry fires on its own seat's Keybearer** (item 229, point 70; deferred by the
   owner). The first build carries "does not fire" as one rules-table value.
2. **Whether a guarantor may be commissioned.** The ruling (point 69) names the wards; the
   design's earlier wording ("a guarantor and its wards cannot be commissioned") is wider. It
   matters only once surety is built, in S4's second half.
3. **The radio batch:** Lapsed, Outcast, tamper, forged marks, whether a seat may cast out its own
   Stake, and what radio is for when cable reaches everywhere a seat holds.
4. **Bound's form** (section 16), for S5.
5. **Whether a commander's rank, and anything else, carries between matches.** The owner's points
   22 and 23 (item 226) leave it open.
6. **The lore pass's checks** (section 19).
7. **The form of the starter bots** (item 229, point 46).
8. **Smaller calls made here,** each the owner's to overrule: an enemy sees a Stake's grade in
   force and nothing else; "licence" stays the word for a structure's unlock; slice 3's Mark is
   built although, until each machine senses for itself, its only use is firing outside a
   Sentry's own arc.
