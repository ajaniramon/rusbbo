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
solid furni in the pathfinder, wallpaper and floor). Social actions (whispers, `LOOKTO`, dancing, carrying drinks, `GOAWAY`), the word filter, the
ignore list in chat and the rank-1 chat commands (`game/commands`; staff commands are
`TODO(commands)`) are ported too. Interactors other than the default one
(teleporters, dice, rollers, post-its, games...) are `TODO(...)` markers. Every trace in
`harness/ported.txt` passes against the Rust server.

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

The bots are the users in `oracle/seed.sql` (`tracebot1`-`tracebot6`, tickets `trace-sso-1`-`trace-sso-6`).

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
| `flat-present` | buying a gift for another user (unknown user, too soon again), placing and opening it (`PRESENTOPEN`, `DELIVER_PRESENT`) |

## Known gaps

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
