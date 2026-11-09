# rusbbo

A Rust rewrite of [Havana](https://github.com/Quackster/Havana), the Habbo Hotel v31 (Shockwave)
and r39 (Flash) server and its website (Havana-Web), formerly `havana-rs`. One binary with an
embedded key-value store (redb) instead of MariaDB, small enough to run on a Raspberry Pi.

The Java server is the reference. Anything the Rust server does is judged by one question: **does
it answer the clients exactly like the Java server does?** The trace harness in this repository
answers that automatically: scripted bots record how the Java server answers, and the Rust server
must give the same bytes back.

## Status

- **Coverage**: logging in (plain and encrypted), the navigator, public and private rooms,
  furniture and the item interactors, the catalogue and purchases, the messenger, trading,
  NPCs and pets, the room dimmer and the jukebox, moderation and bans, the Habbo Lido, the public
  room triggers (Wobble Squabble, Infobus, Space Cafe, Rooftop Rumble), the game hall tables,
  BattleBall and SnowStorm, groups, guides, user tags, referrals, room ads, the Ecotron recycler,
  club gifts, seasonal and collectable catalogue pages, the camera and its photos (the Multi User
  Server), the idol's songs and the Hire-A-Room teleporter; the Flash client on its own port
  ([docs/FLASH.md](docs/FLASH.md)); and the whole website, e-mail, housekeeping and imager
  included ([docs/WEB.md](docs/WEB.md)). Nothing of Java Havana is left to port; what is kept as
  Java does it on purpose is in [docs/BACKLOG.md](docs/BACKLOG.md).
- **Conformance**: the 113 game traces in `traces/` (`harness/ported.txt`) and the 17 web
  traces in `traces/web/` pass against the Java server and against `rusbbo-server`. The real v31 client (HabboLauncher) has been played
  against it on a copy of a real hotel's database, including the Lido, the game hall, BattleBall
  and SnowStorm.
- **Performance**: same data and machine (12 cores, WSL), 500 bots logging in, entering public
  rooms, walking and chatting (`harness/bench.sh`):

  | | Java Havana | rusbbo |
  |---|---|---|
  | Memory idle / with 500 bots | 150 / 302 MiB | 13 / 32 MiB |
  | CPU with 500 bots (% of one core) | 27.6 % | 8.7 % |
  | Login p50 / p99 | 7.7 / 10.1 ms | 1.7 / 2.5 ms |
  | Room entry p50 / p99 | 1.2 / 3.3 ms | 1.1 / 1.5 ms |
  | Chat round trip p50 / p99 | 0.70 / 1.26 ms | 0.26 / 0.46 ms |

## Quick start

```sh
cargo build --release
# Copy a Java Havana database into a redb file (once; the server never talks to MariaDB).
target/release/rusbbo-import --mysql mysql://user:pass@host:3306/havana --out rusbbo.redb
# Serve the Shockwave port, the Flash port and the website (templates and files from Havana's
# tools/ directory, see docs/WEB.md).
target/release/rusbbo-server --db rusbbo.redb --bind 0.0.0.0:12321 --mus 0.0.0.0:12322 \
  --flash-bind 0.0.0.0:12323 \
  --figuredata havana/tools/figuredata.xml \
  --web 0.0.0.0:80 --web-templates havana/tools/www-tpl --web-site havana/tools/www \
  --web-figuredata havana/tools/figuredata.xml --web-imaging imaging/
```

The trace harness, the load test and the id generators also need a checkout of the Java Havana
repository (`HAVANA_DIR`, by default a sibling directory named `aeyra-havana`); the server
itself does not. CI runs `cargo fmt`, `clippy` and the unit tests; the traces need the Java
oracle, so they run locally (see [Trace harness](#trace-harness)).

## Layout

| Path | What it is |
|---|---|
| `docs/DESIGN.md` | Architecture decisions: crates, concurrency model, redb data model, import, handshake |
| `docs/WEB.md`, `docs/FLASH.md` | The website port and the Flash client |
| `docs/BACKLOG.md` | Java features not ported yet |
| `crates/protocol` | FUSE wire protocol: Base64/VL64 encodings, framing, body builders, message id tables |
| `crates/storage` | redb storage: typed tables with their secondary indexes, schema version |
| `crates/server` | `rusbbo-server`: the game server (tokio). Mirrors the Java packages: `messages/incoming`, `messages/outgoing`, `game` |
| `crates/import` | `rusbbo-import`: copies a Java Havana MariaDB database into a redb file |
| `crates/trace` | `rusbbo-trace`: scripted bots that record the Java server's answers and check another server against them |
| `scenarios/` | Bot scripts (TOML): what the bots send, step by step |
| `traces/` | Recorded answers of the Java server (JSON). This is the conformance corpus |
| `oracle/` | Runs the Java server + MariaDB as the reference (`oracle.sh`) |
| `harness/target-rs.sh` | Runs the Rust server from the same data as the oracle, for `rusbbo-trace check` |
| `harness/bench.sh` | Load test of both servers on the same data (`rusbbo-trace load`): start time, memory, CPU, latency |
| `tools/gen_*.py` | Regenerate `ids.rs` (message ids) and `settings_defaults.rs` from the Java sources |

## Rust server

```sh
cargo run -p rusbbo-import -- --mysql mysql://user:pass@host:3306/havana --out rusbbo.redb
cargo run -p rusbbo-server -- --db rusbbo.redb --bind 127.0.0.1:12321
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

The website itself (`--web`, [docs/WEB.md](docs/WEB.md)) serves the same ticket routes and every
Havana-Web page from the game server's process, on the same store. The Flash client connects to
`--flash-bind` ([docs/FLASH.md](docs/FLASH.md)); `--figuredata` is the figure data its
`CHANGE_LOOKS` is checked against. `--mus` (the game port + 1 by default) is the Multi User Server
the Shockwave camera saves and loads photos through (`crates/protocol/src/mus.rs`,
`crates/server/src/mus.rs`); `rusbbo-trace` bots with `mus = true` talk to it.

To check it against the corpus (needs the oracle's MariaDB container for the fixture):

```sh
cargo run -p rusbbo-trace -- check traces/*.json --target 127.0.0.1:12331 --reset "harness/target-rs.sh reset"
```

Traces with `rcon` steps (the Infobus) send them to the target's port minus 12 (Java's 12321 and
12309; `target-rs.sh` starts the Rust server with `--rcon` there), or to `--rcon <addr>`.

Ported so far: the Shockwave handshake including the encrypted transport (`GENERATEKEY`), SSO
login, `GET_INFO`, `GET_CREDITS`, the daily credits handout from the game tick, public rooms
(entering, heightmap, furniture, users, walking with the Java pathfinder and the 500 ms room tick,
chat, shout, wave, typing, leaving, AFK), the navigator (`NAVIGATE`, `GETUSERFLATCATS`, `SUSERF`,
`SRCHF`, `GETSPACENODEUSERS`, favourite rooms), the catalogue (index, pages, buying, the hand,
engraved trophies), the messenger (console, search, friend requests, private messages, status
updates, following friends and invitations to rooms) and private rooms (room kiosk, settings,
doorbell and password, rights, kick, delete, ratings, room events) with their furniture (placing,
moving, rotating and picking up floor and wall items, the default on/off interactor, chairs,
solid furni in the pathfinder, wallpaper and floor), and the interactors for teleporters
(within a flat and between flats), rollers, post-its, gates and one-way doors, and presents
(gift purchases and opening). Social actions (whispers, `LOOKTO`, dancing, carrying drinks,
`GOAWAY`), the word filter, the ignore list in chat, the chat commands (`game/commands`; staff
ones in `staff.rs`, among them `:shutdown` with the maintenance countdown
(`game/maintenance.rs`: `INFO_HOTEL_CLOSING` to everyone, at login, instead of a purchase or a
trade), `:bot` and `:reload`; the player peaks are counted on the game tick for `:uptime` and
`:usersonline`) and trading are ported too. So are the other furniture
interactors (`game/item/interactors`): beds (lying on the pillow), dice, the wheel of fortune, vending
machines, the hockey light and scoreboard, the fortune teller, the love randomizer, totems, step
lights, multi-height furniture and the American Idol hotspot and vote chairs, with the per-room
furniture limits and credit furni exchange; the room-hire interactor and the idol's songs are
`TODO(...)` markers. Every trace in `harness/ported.txt` passes against the Rust server.

The game-hall tables as in Java (`game/gamehall`, `messages/incoming/rooms_games.rs`): sitting on a
table's chairs in the TicTacToe, Battleships, Chess and Poker halls starts or joins its game
(`OPENGAMEBOARD`), walking off, `CLOSE` or leaving the room leaves it (`CLOSEGAMEBOARD`), and `IIM`
carries the commands: five in a row on the 23 x 24 board, placing ships and shooting (a miss passes
the turn 2 s later), chess with chesslib's rules (legal moves, castling, en passant, promotion to a
queen, mate, the draws) and five-card draw poker (dealing, changing, the round's end, the
`poker.*` entry price and rewards), with Java's quirks and the places where its commands throw.
The game id is random, so scenarios write `$gameid` in `IIM` bodies (`crates/trace/src/scenario.rs`).

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

The other public room model triggers as in Java: Rooftop Rumble's Wobble Squabble
(`game/room/wobble_squabble.rs`: the join tiles that take a player with a ticket onto the bridge,
also right after buying tickets there, the queue, the game started by the second player on a start
tile, `PTM` moves and `PT_STATUS` every 200 ms, the fall, the timeout, the tickets and statistics
at the end, and a player who leaves mid-game, whom Java never takes out of the game) and its
camera (`rooftop_rumble_trigger`, Lido B's); the Space Cafe's lights and flipboard
(`game/room/space_cafe.rs`); and the Infobus (`game/infobus.rs`): the park's bus door
(`BUS_DOOR`, the door tile closed, `TRYBUS`, `CHANGEWORLD`), the polls (`infobus_polls`:
`POLL_QUESTION`, `VOTE`, `VOTE_RESULTS` 30 s later and on every new vote) and the end of the
event. The website's housekeeping drives the Infobus over RCON; `--rcon <addr>` serves Java's
RCON port (`rcon.rs`), with every header of Java's `RconHeader`: the hotel, user and room
alerts, the refreshes of looks, club, credits, the hand, the trade setting (a player who may trade
again enters the room again), friend requests, messenger categories, the catalogue, and the ones
that only reload a manager (those for groups, tags and photos do what Java does without them).
The trace harness has `rcon` steps (see `crates/trace/src/scenario.rs`).

The lobby games as in Java (`game/games`, `messages/incoming/games.rs`): the BattleBall and
SnowStorm lobbies (`LOUNGEINFO` and everyone's points and rank on `G_STAT`, `GETINSTANCELIST`,
`INITIATECREATEGAME` and `GAMEPARAMETERVALUES` with their checks, joining and switching teams,
observing and spectating, kicking, leaving, the ticket price, `REQUEST_GAME_LOBBY`, and the
lobbies' visitors in the navigator counting the started games), and BattleBall itself: `STARTGAME`
(the tickets, `GAMELOCATION`), the arena as a room of its own (`ROOM_DIRECTORY` -1 at the spawn,
`FULLGAMESTATUS` on `G_HMAP`, nothing on `G_STAT`, its game task instead of the room's), the
500 ms `GAMESTATUS` with the moves (`GAMEEVENT`), the tiles' colouring, sealing and filling and the
points, the power ups (random in Java, so unit tests only), `GAMEEND` and the scores, the history
(`games_played_history`, listed and observed in the lobby), the players' points, xp, wins and
`ACH_GamePlayed`, the monthly xp reset, `GAMERESTART` (restarting, or back to the lobby when the
countdown ends) and players leaving mid-game. SnowStorm (`game/games/snowstorm`, its engine behind
the same `GameKind` seam): the arenas from `tools/snowstorm_maps` (compiled in too), the random
spawns, the map's `HEIGHTMAP`, `FULLGAMESTATUS` and `OBJECTS_WORLD` on `G_HMAP`,
`REQUESTFULLGAMESTATUS`, the 300 ms `SNOWSTORM_GAMESTATUS` turns, walking, the four throws (long,
short, quick following its target, and Java's instant trajectory 3), hits, stuns and invincibility,
making snowballs, the snowball machines, the scores, `GAMERESET` and leaving; its unit tests check
bytes recorded from the oracle. The game scenarios shorten the games with a staff bot's
`:setconfig`, and every step during a game takes one of the arena's ticks (500 ms in BattleBall,
300 ms in SnowStorm; see the comments in `scenarios/battleball-game.toml` and
`scenarios/snowstorm-game.toml`).

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

The social side as in Java: favourite rooms (`users_room_favourites`: `ADD_FAVORITE_ROOM`,
`DEL_FAVORITE_ROOM`, `GETFVRF`, at most 30, newest room id first), following friends
(`FOLLOW_FRIEND` and its errors, the navigator's second forward after a public room follow,
`INVITE_FRIEND`, the friend's room in the console and in `FINDUSER`), room ratings
(`users_room_votes`: `RATEFLAT`, `UPDATE_VOTES`, voters matched by user, machine id and the IP
addresses of `users_ip_logs`, `ratingSantiyCheck` in the navigator), room events (`rooms_events`:
creating, editing and ending them, `ROOMEEVENT_INFO`, the navigator's lists by type, the event
ending with its room; `game/events.rs`), trophies engraved with the buyer's or the sender's name,
the date and a message (`game/item/trophy.rs`), and the room chat log (`room_chatlogs`, saved
every 60 s and at shutdown, purged at startup after `delete.chatlogs.after.x.age`, with the IP
logs after `delete.iplogs.after.x.age`; `game/chatlog.rs`).

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

Needs Docker, a JDK 25 (`ORACLE_JAVA_HOME`, or `/opt/jdk25`) and a checkout of the Java Havana
repository (`HAVANA_DIR`, by default `../aeyra-havana`): `setup` builds its jar and loads its
schema, migrations and `figuredata.xml`. The SnowStorm maps live in this repository
(`tools/snowstorm_maps`, also compiled into the server).

```sh
oracle/oracle.sh setup   # build the jar, start MariaDB, load schema + migrations + seed, snapshot it
oracle/oracle.sh start
```

`oracle.sh build` builds only the jar. Havana's chess needs chesslib, which only jitpack.io
serves: by default (`ORACLE_CHESSLIB=auto`) the jar gets it when jitpack.io answers, and
otherwise `oracle/no-jitpack.init.gradle` swaps `GameChess` for a stub that ignores every
command (the chess traces then fail); `ORACLE_CHESSLIB=1` or `0` forces either.

`oracle.sh reset` stops the server, restores the database snapshot, starts it again and re-issues the
bots' SSO tickets (Havana clears all tickets on startup and on each login). It takes ~2-3 s and runs
before every recording, so every scenario starts from the same state.

The bots are the users in `oracle/seed.sql` (`tracebot1`-`tracebot6`, tickets `trace-sso-1`-`trace-sso-6`;
the moderation users `tracestaff`, `tracemod`, `tracebanned`, `tracemachine` with tickets
`trace-sso-21`-`trace-sso-24`, and `tracepass`, who logs in with a password).

### 2. Record

```sh
cargo run -p rusbbo-trace -- record scenarios/*.toml --reset "oracle/oracle.sh reset"
```

Each scenario is recorded 3 times (`--runs`). The recordings are compared to tell stable output from
volatile output:

- identical in every run → **exact** match required later;
- same length but some bytes differ (random tokens, ids) → **mask**: the varying alphanumeric runs are
  ignored, the rest must match. Dates and times are always masked, and so are the random ids
  of public room furniture in `OBJECTS_WORLD` (Java's `PublicItemParser`: a letter and 0-998,
  e.g. `r697`), which become `<id>` before recordings and checked messages are compared;
- different lengths → **header** only;
- message order differs between runs → that bot's messages for that step are compared **as a set**,
  and messages that appeared only in some runs are **optional**.

### 3. Check another server

```sh
cargo run -p rusbbo-trace -- check traces/*.json --target 127.0.0.1:12321 --reset "<reset command>"
```

Prints `scenario: N/M steps ok` per trace and every difference; exits 1 if anything differs. Running
it against the Java server itself must always pass (that is how the corpus is validated).

A few traces hang on the server's timers landing in the right step (the SnowStorm and BattleBall
games, Wobble Squabble, the cameras), and in a full run on a busy machine one of them now and then
misses by a tick, against Java as well as Rust. Re-run a failing trace on its own before reading
anything into it.

`rusbbo-trace show traces/login.json` prints a trace readably.

### Load test

`harness/bench.sh [java|rust|both]` runs each server on a copy of the golden snapshot with one
extra user per bot, times its start, samples its memory and CPU every second, and runs
`rusbbo-trace load` at 100, 300 and 500 bots: they log in, enter ordinary public rooms (25 each),
then walk and chat every ~4 s. It reports login, room entry and chat round-trip percentiles, lost
replies and dropped connections, in `harness/run/bench/`. Build with `cargo build --release`
first; the knobs are environment variables listed at the top of the script.

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
| `navigator-favourites` | favourite rooms: `ADD_FAVORITE_ROOM` (flats and public rooms, twice, rooms that do not exist), `GETFVRF` (newest room id first, the owner shown or `-`, a gamehall room's door), `DEL_FAVORITE_ROOM`, the limit of 30 |
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
| `messenger-follow` | `FOLLOW_FRIEND` (not a friend, on the hotel view, into a public room and `NAVIGATE`'s second forward, into a flat, offline), the friend's room in `FINDUSER`, `INVITE_FRIEND`, friends' consoles on entering and leaving rooms |
| `client-flow` | what the real Shockwave client sends and the `login` macro does not: `VERSIONCHECK`, `GET_SESSION_PARAMETERS` before `SSO`, `RECOMMENDED_ROOMS`, `GETINTEREST` before entering, `GET_FLOORMAP` in a private and a public room |
| `moderation-login` | a user id ban and a machine id ban at login (kicked), expired and lifted bans that do not count, `TRY_LOGIN` with a wrong and a right password |
| `moderation-autoban` | the word filter's automatic ban (chat, shout, console), the word filter on console messages, a muted console sender, the stored ban seen by `:ban` |
| `staff-ops` | `:shutdown` (the countdown, `INFO_HOTEL_CLOSING` to everyone, at login and instead of a purchase, cancelling, a bad number), `:usersonline`, `:uptime` (by header: it shows the process), `:reload catalogue` (the heightmap and the catalogue index to everyone), `:reload games` |
| `staff-bot` | `:bot`: random bots seen by everyone in the room and the task's first run (random: by header), a bad amount, a normal user |
| `staff-commands` | the staff chat commands: `:help` for an administrator, `:ha`, `:ra`, `:roommute`, `:mute`/`:unmute`, credits, badges, `:tradeban`, `:teleport`, `:coords`, `:itemdebug`, `:talk`, `:packet`, `:setconfig`, `:reload`, `:resetpw`, `:dc`, `:ban`/`:unban`, and what a moderator may not do |
| `moderation-tool` | `MODERATORACTION`: alert, kick, room alert, room kick and ban, what they refuse, and a normal user who sends it |
| `call-for-help` | calls for help: submit (once), `REQUEST_CFH`, category, pick up, answer, withdraw, follow to the room |
| `flat-moderation` | the word filter on room names and descriptions, `BANUSER` and what it refuses, `:unacceptable` |
| `flat-votes` | room ratings: `RATEFLAT` (bad answers, twice, by the owner), `UPDATE_VOTES` on entry and after a vote, a voter matched by IP address, a new user counting as voted, a negative rating shown as 0, the rating ordering the navigator |
| `flat-events` | room events: `CAN_CREATE_ROOMEVENT`, `CREATE_ROOMEVENT` (out of a room, bad types, twice), `ROOMEEVENT_INFO` to the room and on entry, `GET_ROOMEVENTS_BY_TYPE`, `EDIT_ROOMEVENT`/`QUIT_ROOMEVENT` by the owner and a visitor, the event ending when its room unloads |
| `idle-timeout` | `PING` after 60 s idle, `HOTEL_LOGOUT` and a closed connection 60 s later without `PONG`; a `PONG` keeps the connection (takes ~2 minutes) |
| `flat-pet-gift` | a pet nest bought as a gift (unchecked), opened into the nest with its pet, and a gift with a bad pet that opens into nothing |
| `pool-booth` | the Habbo Lido's changing booths: walking in (`OPEN_UIMAKOPPI`, the curtain closes), `SWIMSUIT`, `CLOSE_UIMAKOPPI`, a visitor's `G_STAT` with the curtains' programs, leaving from inside a booth; the room's re-initialisation without `G_STAT` showing the programs again |
| `pool-ladders` | Lido B's ladders down into the water and up again (swimming, splashes), and swimming through the walkway into the Habbo Lido (arriving at z 1 swims) |
| `pool-tickets` | `BTCKS`: tickets as a gift to an online user, to an unknown user, for oneself; without a pool figure the ladder is out of reach and the diving queue sends you back |
| `pool-diving` | the diving queue to the lift (a ticket taken, `JUMPINGPLACE_OK`), signs and votes (`SIGN`), `DIVE`, `SPLASH_POSITION` (the splash, the lift opening, the score a second later) and out of the water |
| `pool-camera` | Lido B's diving camera: the lift's program on `G_STAT`, the first shot 3 s after the first `G_STAT`, the camera mode for a second user, the camera following the other user when its user leaves |
| `user-effects` | avatar effects: activating (copies, not owned), wearing in and out of a room, waving refused, a visitor's `G_STAT`, drinks and effects stopping each other, `:sit` re-sending it, switching it off, the effect kept into the next room |
| `catalogue-effects` | buying an effect: as a gift (refused), for pixels (stored, nothing sent: Java's `newEffect` bug), too expensive |
| `catalogue-trophy` | trophies: bought with an engraving (the `TrophyExtraData` JSON in the hand, the name, date and message in the room), and as a gift, engraved with the sender's name once opened |
| `gamehall-tictactoe` | the TicTacToe hall: a game started alone and joined, `CHOOSETYPE`, `SETSECTOR` (turns, sides, taken squares), five in a row (winning tokens, the chat line), `RESTART`, walking off and back, `CLOSE` (and a move Java still lets through), leaving the room |
| `gamehall-battleships` | the Battleships hall: `PLACESHIP` (bad type, too many), the start (`OPPONENTS`, `TURN`), shots out of turn, misses (the turn 2 s later), `HIT`/`HITTWICE`/`SINK` with the `SITUATION` grids, `GAMEEND`/`GAMEOVER`, `CLOSE`, leaving |
| `gamehall-chess` | the Chess hall: waiting for a second player, the game for both, walking off ends it for both, a new game, leaving the room, the king chairs |
| `gamehall-chess-mate` | chess moves (chesslib): before the sides are chosen, `CHOOSETYPE`, out of turn, illegal, the same square, a bad square, the scholar's mate, moves after the end, `RESTART`, a check and a move that ignores it |
| `gamehall-chess-draw` | the knights back and forth (no repetition draw: Havana's chesslib board keeps no history), `RESTART`, Sam Loyd's ten-move stalemate (the draw line) |
| `gamehall-poker` | the Poker hall: `OPEN`, `STARTOVER` (cards by header only), a second player in the round, `CHANGE` (`CHANGED`, once per round), walking off (`OPPONENT_LOGOUT` on the next command), the round's end (the winner's line, `REVEALCARDS`), a `CHANGE` without a hand, `CLOSE` |
| `wobble-squabble` | Rooftop Rumble's Wobble Squabble: onto the bridge through the join tile (walking, and in teleport mode), the queue tiles, `PT_PREPARE`/`PT_START`, `PTM` moves and `PT_STATUS` (walking, the bridge's ends, hits, rebalancing, `WALK` ignored), the fall (`PT_WIN`, `PT_END`, tickets, into the water) |
| `wobble-squabble-queue` | a player waiting on a queue tile moves on when the game ends and starts the next one against the winner; leaving mid-game: the game goes on, and the one who left still pays a ticket |
| `wobble-squabble-tickets` | the join tile does nothing while a game is on; buying tickets (`BTCKS`) on it afterwards takes the buyer onto the bridge and into the next game |
| `battleball-lobby` | the BattleBall lobby: `LOUNGEINFO`/`GAMEPLAYERINFO`, the instance list, creating a game (bad map, bad parameter), observing, joining and switching teams (bad team, full team), `STARTGAME` refused, kicking, spectating then joining, leaving, the creator leaving a waiting game, the ticket checks (`:setconfig`), `REQUEST_GAME_LOBBY` |
| `snowstorm-lobby` | the SnowStorm lobby up to the arena: its parameters, a bad map, too many teams, a one-team game (and its `STARTGAME` alert), the game length in the lists, joining, leaving, `STARTFAILED`, a missing parameter |
| `battleball-game` | a BattleBall game end to end on Barebones Classic: `STARTGAME`, the arena, `FULLGAMESTATUS`, `GAMESTATUS` every 500 ms, the lobby's visitors in the navigator, moves colouring tiles, chat and typing in the arena, `GAMEEND`, the observer's finished game, the history and the points |
| `battleball-restart` | a BattleBall game on Sky Peak with a spectator (`YOUARESPECTATOR`), unscored (the same-IP check), `GAMERESTART` by both (`GAMERESET`, the second game) and then by one only (both back to the lobby) |
| `battleball-reload` | `:reload games` in the BattleBall lobby: the waiting game drops out (not listed, observable, startable or joinable), a new game takes the next id, its player joins it from the lost game |
| `battleball-leave` | leaving a BattleBall game that is on: a player disconnecting, the last one going back to the lobby (the game is gone, also for the observer) |
| `snowstorm-game` | a SnowStorm game end to end on arena 4: `STARTGAME`, the arena (`HEIGHTMAP`, `FULLGAMESTATUS`, `OBJECTS_WORLD`), `SNOWSTORM_GAMESTATUS` every 300 ms, nothing before `GAMESTART`, walking (random spawns: header only), long, quick and short throws and their hits, making a snowball, `REQUESTFULLGAMESTATUS`, instant hits that stun, no throwing while stunned, `GAMEEND` with the scores, the finished game, the points and xp |
| `snowstorm-reward` | `snowstorm-game` with fixed credit rewards: the credits come on the game tick after `GAMEEND` |
| `snowstorm-leave` | a one-team SnowStorm game on arena 7: a player leaving mid-game (their avatar gone, the game goes on for the other), `GAMEEND` with the leaver at 0, the last player leaving |
| `rooftop-camera` | Rooftop Rumble's camera (`rooftop_rumble_trigger`): the first shot 3 s after the first `G_STAT`, the mode for a second user, following the other user when its user leaves |
| `space-cafe` | the Space Cafe's lights (`space_cafe_trigger`): six random `SHOWPROGRAM`s 4.5 s after the first `G_STAT` |
| `infobus-door` | the Infobus's door in the park: `BUS_DOOR`, `TRYBUS` and the door tile while closed, opening and closing it over RCON, walking into the bus and back out with `CHANGEWORLD` |
| `rcon-users` | RCON: hotel, user and room alerts, refreshing credits, looks, club, hand and tags, friend requests, messenger categories, group permissions (no group), the catalogue, the manager reloads, a bad number, an unknown header, `disconnect` |
| `rcon-trade` | RCON's `refreshtrade` in a Trade floor flat: on (`ACH_TraderPass`, the room entered again where the player stands, the trade sees it), off (in the room too), a missing value, an offline user |
| `groups-room` | groups in rooms: the favourite group in `USER_OBJECTS`, `GROUP_MEMBERSHIP_UPDATE` and `GROUP_BADGES`, `GET_GROUP_BADGES`, `GET_GROUP_INFO`, `GET_USER_TAGS`, the room entry badge, RCON's group and tag refreshes |
| `guides` | Habbo guides: asking for a guide, the invite task, accepting and following, `INVITING_COMPLETED`, the GLK badge, respect, `RESET_TUTORIAL` |
| `recycler` | the Ecotron: its status and prize table, `RECYCLE_ITEMS` (too few, repeated, five items), the box and `ITEM_DELIVERED` |
| `club-gifts` | `SCR_GIFT_APPROVAL` with gifts due, `GET_ALIAS_LIST`, `GET_FURNI_VERSIONS`, `GETUSERCREDITLOG`, `PURCHASE_AND_WEAR` |
| `flash-login` | a Flash client on the Flash port: no `HELLO`, `SESSION_PARAMETERS`, `HOME_ROOM`, `FLASH_FAVOURITES`, `FLASH_MODTOOL`, the Flash `AVATAR_EFFECTS` |
| `flash-navigator` | the Flash navigator: public and guest rooms, room info, icon and settings, search by name, owner and tag, popular tags, events, the home room, the wardrobe |
| `flash-room` | a Flash and a Shockwave client in the same rooms: event 390, each kind's `USER_OBJECTS`, `USERBADGE` and `ROOMEEVENT_INFO`, rights by id, favourites, the hand after buying, placing and deleting, event tags, `CHANGE_LOOKS`, `QUIT` |
| `welcome-alert` | the welcome message's line breaks and `%username%` (`StringUtil.replaceAlertMessage`), for Shockwave and Flash |
| `staff-rank` | a moderator's command above their rank: plain chat with the look timer |
| `trade-redeemable` | credit furniture to another account on the same IP: gifts and placing it (and a present holding it) trade-ban the buyer |
| `referrals` | `ACH_MGM` levels from the users a player referred |
| `seasonal-catalogue` | seasonal pages with `seasonal.items` on: the index for a user and for staff, a page in and out of season, Easter Monday's |
| `collectables` | collectables cycles: the store and admin pages list only the active item (its layout from the price), buying it without seeing the admin page |
| `camera-photos` | the MUS port: Logon, LOGIN (same IP, offline user shut out), PHOTOTXT, BINDATA (the photo in the hand, film), GETBINDATA, RCON `clear_photo` |
| `idol-song` | an idol performance with the performer's song disc: `START_PLAYING_SONG`/`STOP_PLAYING_SONG` and the sound machine |
| `room-hire` | the Hire-A-Room teleporter: in, `LOGOUT`, `ROOMFORWARD` to room 0, which cannot be entered |
| `follow-hidden` | following a friend into a hidden public room: the forward goes to its walkway's room |
| `teleport-public` | a teleporter from a flat to a hidden public room: `Room.forward`'s offset and redirect |
| `walkway-others` | a walkway seen from the room left behind: the walker's new status |
| `generatekey-parity` | a second `GENERATEKEY`, a zero shared secret, a key that is not a number, BigInteger's signs |
| `messenger-order` | friends in Java's `HashMap` order |
| `search-accents` | room search and name lookups without case or accents, as MariaDB's collation compares |
| `normalise-input` | `normalise.input.strings`: accents become `?` on the wire |
| `rooms-carry` | `:teleport`, `:itemdebug` and the flood mute kept from room to room |
| `pool-no-stat` | leaving Lido B before anyone sent `G_STAT`: Java's exception skips the rest of the leave |
| `infobus-poll` | an Infobus poll over RCON: `POLL_QUESTION`, `VOTE` (out of range, twice), `VOTE_RESULTS` 30 s later and on a new vote, a poll that does not exist, the end of the event (everyone walks out into the park and hears it again there) |

## Known gaps

- The bots' `login` macro is shorter than what the real client sends; `client-flow` covers the
  difference that was found by logging in with the real v31 client (HabboLauncher and dirplayer)
  against a copy of a real hotel's database. That session also showed what the client asks for and
  the server did not answer then: moodlight presets (`MSG_ROOMDIMMER_GET_PRESETS`) and the jukebox
  (`GET_PLAY_LIST`, `GET_JUKEBOX_DISCS`), ported since (`flat-moodlight`, `flat-trax`,
  `flat-jukebox`); ids 288, 301, 302, 315 and 321 have no handler in Java either.

- Only `login-encrypted` uses the RC4 layer (`encrypt = true` on a bot); every other scenario runs
  unencrypted.
- The chess traces need an oracle jar built with `chesslib` (jitpack.io, see
  [Start the reference server](#1-start-the-reference-server)); a jar built with the stub fails
  them. Promotion, castling and en passant are checked by unit tests only.
- The Java checkout's `tools/migrations/aeyra-settings-20261025.sql` is not applied: its string literals lost their
  quotes and it fails to parse.
- Games: no trace covers a spectator whose game id went stale (a restart takes the id the newest
  game was created with, Java's `getAndIncrement`), stopping a lobby walk on the first BattleBall
  start, or a Wobble Squabble player who left for another room when the game ends; they follow
  the Java source.
- What Java does that cannot be reproduced (identity-hash orders, ties in MariaDB's `ORDER BY`,
  thread races) is listed in [docs/BACKLOG.md](docs/BACKLOG.md).
