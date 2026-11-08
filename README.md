# havana-rs

A Rust rewrite of Havana, built for fun: small enough to run on a Raspberry Pi, with an embedded
key-value store instead of MariaDB. It is in its early stages.

The Java server in this repository is the reference. Anything the Rust server does is judged by one
question: **does it answer the clients exactly like the Java server does?** The trace harness in
this directory answers that question automatically.

## Layout

| Path | What it is |
|---|---|
| `docs/DESIGN.md` | Architecture decisions: crates, concurrency model, redb data model, import, handshake |
| `crates/protocol` | FUSE wire protocol: Base64/VL64 encodings, framing, body builders, message id tables |
| `crates/storage` | redb storage: typed tables with their secondary indexes, schema version |
| `crates/server` | `havana-server`: the game server (tokio). Mirrors the Java packages: `messages/incoming`, `messages/outgoing`, `game` |
| `crates/import` | `havana-import`: copies a Java Havana MariaDB database into a redb file |
| `crates/trace` | `havana-trace`: scripted bots that record the Java server's answers and check another server against them |
| `scenarios/` | Bot scripts (TOML): what the bots send, step by step |
| `traces/` | Recorded answers of the Java server (JSON). This is the conformance corpus |
| `oracle/` | Runs the Java server + MariaDB as the reference (`oracle.sh`) |
| `harness/target-rs.sh` | Runs the Rust server from the same data as the oracle, for `havana-trace check` |
| `tools/gen_*.py` | Regenerate `ids.rs` (message ids) and `settings_defaults.rs` from the Java sources |

## Rust server

```sh
cargo run -p havana-import -- --mysql mysql://user:pass@host:3306/havana --out havana.redb
cargo run -p havana-server -- --db havana.redb --bind 127.0.0.1:12321
```

Real users log in through the ticket API, as with Havana-Web: start the server with
`--http 127.0.0.1:8095` and the launcher's `GET /api/ticket?username=..&password=..` returns the
same JSON as Havana-Web's `TicketController` (host, ports and client URLs from the `loader.*` and
`site.path` settings, and an SSO ticket); `/api/login` returns the bare ticket. Passwords are the
Argon2 hashes the Java web wrote (`users.password`). Each client IP may make
`--http-rate-limit` requests per minute (default 10; more get `429`), and password checks run one
at a time (each needs 64 MiB). Behind a reverse proxy every request has the proxy's IP, so the
limit is shared. A bad login is a `404` with an empty body (Java sends its 404 page).
`--dev-ticket` stays for the trace harness.

To check it against the corpus (needs the oracle's MariaDB container for the fixture):

```sh
cargo run -p havana-trace -- check traces/*.json --target 127.0.0.1:12331 --reset "harness/target-rs.sh reset"
```

Ported so far: the Shockwave handshake including the encrypted transport (`GENERATEKEY`), SSO
login, `GET_INFO`, `GET_CREDITS`, the daily credits handout from the game tick, public rooms
(entering, heightmap, furniture, users, walking with the Java pathfinder and the 500 ms room tick,
chat, shout, wave, typing, leaving, AFK), the navigator (`NAVIGATE`, `GETUSERFLATCATS`, `SUSERF`,
`SRCHF`, `GETSPACENODEUSERS`), the catalogue (index, pages, buying, the hand), the messenger
(console, search, friend requests, private messages, status updates) and private rooms (room
kiosk, settings, doorbell and password, rights, kick, delete) with their furniture (placing,
moving, rotating and picking up floor and wall items, the default on/off interactor, chairs,
solid furni in the pathfinder, wallpaper and floor), and the interactors for teleporters
(within a flat and between flats), rollers, post-its, gates and one-way doors, and presents
(gift purchases and opening). Social actions (whispers, `LOOKTO`, dancing, carrying drinks,
`GOAWAY`), the word filter, the ignore list in chat, the chat commands (`game/commands`; staff
ones in `staff.rs`, `:bot`/`:shutdown` are `TODO(commands)`) and trading are ported too. So are the other furniture
interactors (`game/item/interactors`): beds (lying on the pillow), dice, the wheel of fortune, vending
machines, the hockey light and scoreboard, the fortune teller, the love randomizer, totems, step
lights, multi-height furniture and the American Idol hotspot and vote chairs, with the per-room
furniture limits and credit furni exchange; the game and room-hire interactors and the idol's songs
are `TODO(...)` markers. Every trace in `harness/ported.txt` passes against the Rust server.

The Habbo Lido as in Java (`game/room/pool.rs`, `messages/incoming/rooms_pool.rs`): the changing
booths (curtains, `OPEN_UIMAKOPPI`, `SWIMSUIT`, `CLOSE_UIMAKOPPI`), the ladders in and out of the
water (swimming, `SHOWPROGRAM` splashes), Lido B's diving queue, lift and dive (`DIVE`,
`SPLASH_POSITION`, the votes of `SIGN` and the score), Lido tickets (`BTCKS`), the
`habbo_lido_trigger` and `diving_deck_trigger` (swimming in from the other pool, the diving
camera), and the items' programs on room entry. Avatar effects as in Java (`game/effects.rs`):
`ACTIVATE_AVATAR_EFFECT`, `USE_AVATAR_EFFECT` (`USER_AVATAR_EFFECT` to the room, kept from room to
room, shown on `G_STAT`), expiry on the game tick, drinks and effects stopping each other, the
effect shown again after sitting or lying down, Abigail.Ryan's ghost, and effects bought from the
catalogue or given by a totem. Java's `EffectDao.newEffect` stores those last two but fails to read
the new row's id with the MariaDB driver, so the player only gets them at the next login; the port
does the same (`traces/catalogue-effects.json`).

Public room NPCs (`rooms_bots`) as in Java: added on a room's first entry, wandering over their
walkspace, talking now and then, and handing out drinks when a user nearby asks for one
(`game/bot.rs`, `game/room/bots.rs`). Bots are room entities with a detached session and
`EntityKind::Bot`; they never count as players.

The room dimmer (`items_moodlight_presets`) as in Java: one per room, its three presets created
on the owner's first look (`MSG_ROOMDIMMER_GET_PRESETS`), saved (`SET_PRESET`, only the client's
palette) and switched on and off (`CHANGE_STATE`), and `:rainbow` cycling its colour
(`game/room/sound.rs`, `messages/incoming/rooms_dimmer.rs`). The sound machine and the jukebox
(TRAX, the `soundmachine_*` tables) as in Java: one per room, sound sets in the machine's slots,
songs made, edited, deleted and burnt to discs, the playlist, and discs loaded into a jukebox
(`game/song.rs`, `messages/incoming/songs.rs`, `messages/incoming/jukebox.rs`).

Pets (`items_pets`) as in Java: bought with their nest (name, race, colour), in the room while
the nest is placed, acting on their own every 10-25 s (walking, sitting, lying down, now and then
a line), eating, drinking and playing at food, water bowls and toys, and obeying their owner
(`Lacoste sit`, `come`, `jump`, `play dead`...) (`game/pet.rs`, `game/room/pets.rs`). A pet can be
a gift: it travels in the present and is created when the present is opened.

Purchases, gifts, club subscriptions, finished trades and opened presents are each one redb
transaction (Java commits step by step), with their `users_transactions` audit rows; memory
changes only after the commit (`docs/DESIGN.md`, `crates/server/src/txn_tests.rs`).

Moderation as in Java (`game/moderation`): hotel bans (`users_bans`: a user id or machine id
ban kicks the user at login; IP bans are stored but, as in Java, not enforced), the word filter's
automatic 20-year ban for a bannable sentence (chat, shout, whisper, console), mutes
(`MUTE_EXPIRES_AT`) and room mutes, the word filter on console messages and room names and
descriptions, `TRY_LOGIN` with a password, room bans (`BANUSER`), the moderation tool
(`MODERATORACTION`: alert, kick, ban, room alert, room kick, written to `housekeeping_audit_log`)
and calls for help (`SUBMIT_CFH`, `PICK_CALLFORHELP`, `MESSAGETOCALLER`...).

Server housekeeping as in Java: idle connections get `PING` after 60 s and are logged out 60 s
later without a `PONG`; startup resets SSO tickets, `players.online`, room visitor counts, trade
states and online flags (one transaction); Ctrl-C/SIGTERM sends `INFO_HOTEL_CLOSED`, logs everyone
out and closes the database cleanly. A panic in a message handler or a tick is logged and
contained (Java catches the exception and carries on); all text on the wire is ISO-8859-1
(`ServerBody::text`).

## Trace harness

### 1. Start the reference server

Needs Docker and a JDK 25 (`ORACLE_JAVA_HOME`, or `/opt/jdk25`).

```sh
oracle/oracle.sh setup   # build the jar, start MariaDB, load schema + migrations + seed, snapshot it
oracle/oracle.sh start
```

`oracle.sh reset` stops the server, restores the database snapshot, starts it again and re-issues the
bots' SSO tickets (Havana clears all tickets on startup and on each login). It takes ~2-3 s and runs
before every recording, so every scenario starts from the same state.

The bots are the users in `oracle/seed.sql` (`tracebot1`-`tracebot6`, tickets `trace-sso-1`-`trace-sso-6`;
the moderation users `tracestaff`, `tracemod`, `tracebanned`, `tracemachine` with tickets
`trace-sso-21`-`trace-sso-24`, and `tracepass`, who logs in with a password).

### 2. Record

```sh
cargo run -p havana-trace -- record scenarios/*.toml --reset "oracle/oracle.sh reset"
```

Each scenario is recorded 3 times (`--runs`). The recordings are compared to tell stable output from
volatile output:

- identical in every run → **exact** match required later;
- same length but some bytes differ (random tokens, ids) → **mask**: the varying alphanumeric runs are
  ignored, the rest must match. Dates and times are always masked;
- different lengths → **header** only;
- message order differs between runs → that bot's messages for that step are compared **as a set**,
  and messages that appeared only in some runs are **optional**.

### 3. Check another server

```sh
cargo run -p havana-trace -- check traces/*.json --target 127.0.0.1:12321 --reset "<reset command>"
```

Prints `scenario: N/M steps ok` per trace and every difference; exits 1 if anything differs. Running
it against the Java server itself must always pass (that is how the corpus is validated).

`havana-trace show traces/login.json` prints a trace readably.

### Writing scenarios

See the doc comment in `crates/trace/src/scenario.rs`. In short: a list of bots with SSO tickets
and a list of steps; each step is a `macro` (`login`), `connect`, `disconnect` or `send` with typed
`args` (ints/bools → VL64, strings → client strings, `{ raw = "..." }` → raw bytes). Message names
are the Java class names (`ROOM_DIRECTORY`, `CHAT`...). Use `settle_ms` when the server keeps sending
after a request (walking, anything driven by the 1 s game tick).

Keep scenarios deterministic: avoid rooms with NPCs (they wander randomly) and anything that depends
on wall-clock time beyond what the masks cover. In particular:

- Java sends `PING` 60 s after a connection opens and drops it 60 s later without a `PONG`; the
  bots do not answer (a `PONG` runs the achievement check, whose output depends on the clock), so
  keep each bot's part of a scenario well under 60 s.
- Chat and `LOOKTO` start a look timer (6 s, in whole seconds): one more `USER_STATUSES` follows
  6-7 s later. Give the step after which it falls a long `settle_ms` (8 s) so it always lands there.
- When the Java server picks something at random (a gift's wrapping), list the affected server
  messages in the step's `header_only`; they are then recorded header-only even when the
  recordings happen to agree.

The bots are `tracebot1`-`tracebot6` in `oracle/seed.sql`; its comments say what each one is for.
`docs/flats-notes.md` and `docs/social-trade-items-notes.md` collect the Java behaviour the traces show.

## Current corpus

| Trace | Covers |
|---|---|
| `login` | handshake, SSO login, user info, credits |
| `login-encrypted` | the same with `GENERATEKEY`: Diffie-Hellman, then every message over the RC4 layer |
| `navigator` | public/guest categories, flat categories, own rooms, search |
| `public-room` | enter a public room, heightmap/objects/users, walk, chat, shout, wave, leave |
| `room-bots` | public room NPCs: Harry joins the Welcome Lounge with its first visitor (`USER_OBJECTS` type 3), and counts neither as a visitor nor in the room's user list |
| `room-two-bots` | two users seeing each other enter, talk, walk and leave |
| `catalogue` | catalogue index and pages, buying furni, hand |
| `messenger` | console init, user search, friend request/accept, private message |
| `flat-create` | room kiosk (`CREATEFLAT`, `SETFLATINFO`, `SETFLATCAT`), own rooms and search, room info, entering a flat as the owner (`TRYFLAT`/`GOTOFLAT` + room load), walking, settings changed from inside (`UPDATEFLAT`) |
| `flat-furni` | buying floor/wall furni, placing it from the hand, moving, rotating, an invalid move, toggling floor and wall items, walking blocked by solid furni, sitting on a chair, picking up (also from under a sitting user) |
| `flat-visit` | two users: doorbell (refused, then let in), live furni updates for both, rights given/removed, password room (wrong, right), kick |
| `flat-delete` | deleting a room with a visitor and furni inside: both sent to the hotel view, furni gone, navigator no longer lists it |
| `flat-decor` | buying wallpaper and floor, applying them (`FLATPROPBYITEM`), seeing them on re-entry |
| `social-actions` | whispers (to a user, to someone ignoring you, to nobody), chat and the ignore list, `LOOKTO`, typing, `GOAWAY` |
| `social-dance-carry` | dancing (club styles refused to non-club users), `STOP`, `CARRYDRINK`/`CARRYITEM`, dancing drops the drink |
| `chat-filter` | word filter per receiver (filterable words) and for everyone (mandatory words) on chat, shout and whisper |
| `chat-commands` | the rank-1 chat commands (`:about`, `:commands`/`:help`, `:givedrink`, `:dropdrink`, `:sit`, `:afk`, `:guidestatus`), a command above one's rank and an unknown one |
| `trade` | trading in a Trade floor flat: a partner with trading disabled, `TRADE_ALREADY_OPEN`, offering/removing items, accept/unaccept, confirm and the swap, a cancelled trade |
| `flat-teleport` | a teleporter pair inside one flat, a second pair between two flats (`ROOMFORWARD`, arriving on the far teleporter), seen by a second user |
| `flat-roller` | a roller moving a user and an item (`SLIDEOBJECTBUNDLE`), `:pickall` |
| `flat-postit` | post-it pad placed on the wall, read and written by the owner and a visitor without rights, deleted |
| `flat-gates` | a gate (solid while closed) and a one-way door (`ENTER_ONEWAY_DOOR`) |
| `flat-pets` | pet names (`APPROVE_PET_NAME`), buying a crocodile (and an invalid colour that buys nothing), its nest placed so the pet comes in (`USER_OBJECTS` type 2), `GETPETSTAT`, the nest picked up so it leaves |
| `flat-moodlight` | the room dimmer: one per room, presets created by the owner's first look (none for a visitor before), saving a preset (palette colours only, a bad preset id), on/off, what a visitor sees on entry, `:rainbow` (started, stopped by a preset or a switch, without a dimmer), picked up switched off |
| `flat-trax` | the sound machine: one per room (error 23, a jukebox too), sound sets in and out, a song saved, listed, read and edited, the playlist seen by a visitor, burning a disc (then `SONG_LOCKED`, or a copy under another title), deleting songs, the machine picked up |
| `flat-jukebox` | the jukebox: discs burnt, listed in the hand and in the jukebox (needs rights), loaded into slots (bad and taken slots), queueing songs (no rights needed), a disc taken out, `RESET_JUKEBOX`, the jukebox picked up with a disc inside |
| `flat-present` | buying a gift for another user (unknown user, too soon again), placing and opening it (`PRESENTOPEN`, `DELIVER_PRESENT`) |
| `flat-dice` | dice (`THROW_DICE` only next to it, `DICE_VALUE` while rolling and the random result, `DICE_OFF`, no moving or toggling while rolling), the wheel of fortune (`SPIN_WHEEL_OF_FORTUNE`), a visitor's `G_STAT` mid-roll |
| `flat-vending` | a vending machine (walk to it, turn, open, the drink after 1 s, closed after 2 s), the hockey light, the scoreboard (`USEFURNITURE` status 0/1/2, rights only), the fortune teller, `SET_RANDOM_STATE` |
| `flat-bed` | beds: any tile leads to a pillow, lying down with the bed's rotation (dancing stops), standing up off it |
| `flat-idol` | American Idol: the hotspot (`OPEN_PERFORMER_GUI`, one per room), the vote chair (`JUDGE_GUI_STATUS`), `START_PERFORMANCE`, `VOTE_PERFORMANCE`, stepping off and leaving |
| `flat-limits` | per-room limits: 35 rollers without club, one sound machine or jukebox (`CANNOT_PLACE_STUFF_FROM_STRIP`), one room dimmer (`ALERT`); `CONVERT_FURNI_TO_CREDITS` |
| `text-latin1` | non-ASCII text (ñ, accents, ¡¿) in a room name, a post-it, chat and a console message, as ISO-8859-1 |
| `messenger-search` | `FINDUSER` with SQL wildcards, including a pattern with 40 `%` |
| `client-flow` | what the real Shockwave client sends and the `login` macro does not: `VERSIONCHECK`, `GET_SESSION_PARAMETERS` before `SSO`, `RECOMMENDED_ROOMS`, `GETINTEREST` before entering, `GET_FLOORMAP` in a private and a public room |
| `moderation-login` | a user id ban and a machine id ban at login (kicked), expired and lifted bans that do not count, `TRY_LOGIN` with a wrong and a right password |
| `moderation-autoban` | the word filter's automatic ban (chat, shout, console), the word filter on console messages, a muted console sender, the stored ban seen by `:ban` |
| `staff-commands` | the staff chat commands: `:help` for an administrator, `:ha`, `:ra`, `:roommute`, `:mute`/`:unmute`, credits, badges, `:tradeban`, `:teleport`, `:coords`, `:itemdebug`, `:talk`, `:packet`, `:setconfig`, `:reload`, `:resetpw`, `:dc`, `:ban`/`:unban`, and what a moderator may not do |
| `moderation-tool` | `MODERATORACTION`: alert, kick, room alert, room kick and ban, what they refuse, and a normal user who sends it |
| `call-for-help` | calls for help: submit (once), `REQUEST_CFH`, category, pick up, answer, withdraw, follow to the room |
| `flat-moderation` | the word filter on room names and descriptions, `BANUSER` and what it refuses, `:unacceptable` |
| `idle-timeout` | `PING` after 60 s idle, `HOTEL_LOGOUT` and a closed connection 60 s later without `PONG`; a `PONG` keeps the connection (takes ~2 minutes) |
| `flat-pet-gift` | a pet nest bought as a gift (unchecked), opened into the nest with its pet, and a gift with a bad pet that opens into nothing |
| `pool-booth` | the Habbo Lido's changing booths: walking in (`OPEN_UIMAKOPPI`, the curtain closes), `SWIMSUIT`, `CLOSE_UIMAKOPPI`, a visitor's `G_STAT` with the curtains' programs, leaving from inside a booth; the room's re-initialisation without `G_STAT` showing the programs again |
| `pool-ladders` | Lido B's ladders down into the water and up again (swimming, splashes), and swimming through the walkway into the Habbo Lido (arriving at z 1 swims) |
| `pool-tickets` | `BTCKS`: tickets as a gift to an online user, to an unknown user, for oneself; without a pool figure the ladder is out of reach and the diving queue sends you back |
| `pool-diving` | the diving queue to the lift (a ticket taken, `JUMPINGPLACE_OK`), signs and votes (`SIGN`), `DIVE`, `SPLASH_POSITION` (the splash, the lift opening, the score a second later) and out of the water |
| `pool-camera` | Lido B's diving camera: the lift's program on `G_STAT`, the first shot 3 s after the first `G_STAT`, the camera mode for a second user, the camera following the other user when its user leaves |
| `user-effects` | avatar effects: activating (copies, not owned), wearing in and out of a room, waving refused, a visitor's `G_STAT`, drinks and effects stopping each other, `:sit` re-sending it, switching it off, the effect kept into the next room |
| `catalogue-effects` | buying an effect: as a gift (refused), for pixels (stored, nothing sent: Java's `newEffect` bug), too expensive |

## Known gaps

- The bots' `login` macro is shorter than what the real client sends; `client-flow` covers the
  difference that was found by logging in with the real v31 client (HabboLauncher and dirplayer)
  against a copy of a real hotel's database. That session also showed what the client asks for and
  the server did not answer then: moodlight presets (`MSG_ROOMDIMMER_GET_PRESETS`) and the jukebox
  (`GET_PLAY_LIST`, `GET_JUKEBOX_DISCS`), ported since (`flat-moodlight`, `flat-trax`,
  `flat-jukebox`); ids 288, 301, 302, 315 and 321 have no handler in Java either.
- The website itself is not ported; real users get their tickets from the ticket API
  (`--http`, see above).

- Only `login-encrypted` uses the RC4 layer (`encrypt = true` on a bot); every other scenario runs
  unencrypted. The Flash port (12323) is not covered.
- `OBJECTS_WORLD` (public room furniture) changes length between runs of the same scenario, so it is
  checked by header only. The cause is the random id each item gets on room load
  (`PublicItemParser`: a letter and 0-998, e.g. `r697` or `o2`); masking those ids would make the
  rest (sprites, positions, order) checkable.
- The oracle is built without `chesslib` (served only by jitpack.io, which this environment cannot
  reach): `oracle/no-jitpack.init.gradle` swaps `GameChess` for a stub, so chess does nothing.
- `tools/migrations/aeyra-settings-20261025.sql` is not applied: its string literals lost their
  quotes and it fails to parse.
