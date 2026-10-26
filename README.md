# havana-rs

A Rust rewrite of Havana, built for fun: small enough to run on a Raspberry Pi, with an embedded
key-value store instead of MariaDB. It is in its early stages.

The Java server in this repository is the reference. Anything the Rust server does is judged by one
question: **does it answer the clients exactly like the Java server does?** The trace harness in
this directory answers that question automatically.

## Layout

| Path | What it is |
|---|---|
| `crates/protocol` | FUSE wire protocol: Base64/VL64 encodings, framing, body builders, message id tables |
| `crates/trace` | `havana-trace`: scripted bots that record the Java server's answers and check another server against them |
| `scenarios/` | Bot scripts (TOML): what the bots send, step by step |
| `traces/` | Recorded answers of the Java server (JSON). This is the conformance corpus |
| `oracle/` | Runs the Java server + MariaDB as the reference (`oracle.sh`) |
| `tools/gen_message_ids.py` | Regenerates `crates/protocol/src/ids.rs` from the Java sources |

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

The bots are the users in `oracle/seed.sql` (`tracebot1`/`tracebot2`, tickets `trace-sso-1`/`trace-sso-2`).

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
on wall-clock time beyond what the masks cover.

## Current corpus

| Trace | Covers |
|---|---|
| `login` | handshake, SSO login, user info, credits |
| `navigator` | public/guest categories, flat categories, own rooms, search |
| `public-room` | enter a public room, heightmap/objects/users, walk, chat, shout, wave, leave |
| `room-two-bots` | two users seeing each other enter, talk, walk and leave |
| `catalogue` | catalogue index and pages, buying furni, hand |
| `messenger` | console init, user search, friend request/accept, private message |

## Known gaps

- Bots talk to the unencrypted transport only: they skip `GENERATEKEY`, so the RC4 layer the real
  Shockwave client negotiates is not covered yet. The Flash port (12323) is not covered either.
- `OBJECTS_WORLD` (public room furniture) changes length between runs of the same scenario, so it is
  checked by header only. The likely cause is iteration order of a hash map in the Java server; a
  set-like comparison of its items would make it checkable.
- The oracle is built without `chesslib` (served only by jitpack.io, which this environment cannot
  reach): `oracle/no-jitpack.init.gradle` swaps `GameChess` for a stub, so chess does nothing.
- `tools/migrations/aeyra-settings-20261025.sql` is not applied: its string literals lost their
  quotes and it fails to parse.
