# havana-rs design

Status: block 2, first draft. Decisions marked **(review)** are the ones worth a second opinion
before many agents build on them.

## Goals and non-goals

- Same behaviour on the wire as the Java Havana in this repository, for the Shockwave (v31) and
  Flash (r39) clients. "Same" is defined by the trace corpus (`traces/`), not by reading the code.
- One small binary that runs on a Raspberry Pi: target under 50 MB RSS with a few dozen users.
- No external database: the data lives in one embedded file.
- Not a goal: being faster than the Java server on big hardware, horizontal scaling, keeping the
  Java code structure.

## Crates

| Crate | Role | Depends on |
|---|---|---|
| `protocol` | Wire format: encodings, framing, body readers/writers, message ids, (later) RC4 | — |
| `storage` | redb file: typed tables, secondary indexes, schema version | — |
| `server` | Game server: connections, sessions, handlers, game state, ticks | protocol, storage |
| `import` | One-off tool: MariaDB (Java schema) → redb file | storage |
| `trace` | Conformance harness (block 1) | protocol |
| `web` (later) | Website + housekeeping (axum + minijinja), **in the same process** as the server | storage |

Handlers are grouped by the Java packages (`handshake`, `user`, `navigator`, `rooms`,
`catalogue`, `messenger`...) as modules of `server`, so an agent can port one Java package at a
time and own a separate set of files.

## Concurrency model **(review)**

The Java server shares mutable state across threads (Netty event loops, a 1 s `GameScheduler`, a
500 ms task per room). The Rust server keeps the same shape, because porting it 1:1 is what lets
many agents work in parallel against the traces:

- **tokio**, multi-threaded runtime. One task per connection reads frames and runs handlers; one
  writer task per connection drains an unbounded channel of encoded frames. `Session::send` never
  blocks and never awaits.
- **Shared state behind synchronous locks** (`parking_lot::Mutex`/`RwLock`): a `World` with the
  player registry, the room registry and loaded catalogue/navigator data. Each room and each
  player has its own lock.
- **Rules**, enforced in review: never `.await` while holding a lock; lock order is player before
  room, and never two rooms at once; handlers are synchronous functions
  (`fn(&Ctx, &mut Session, ClientReader)`), not async.
- **Ticks**: one task for the 1 s game scheduler (credits/pixels handout, AFK), one interval per
  loaded room for the 500 ms room tick (walking, statuses). Same periods as Java, because traces
  depend on them (see the login trace's credit handout).

Actors per room were considered. They avoid lock-order rules but make every cross-room or
cross-player action a message protocol, which is much harder to port from Java mechanically.
If lock contention ever shows up on a Pi with a few dozen users, it would be a surprise.

## Storage **(review)**

**redb** (single file, ACID, B-trees, one writer at a time). fjall (LSM) was the alternative.
redb wins here: the workload is read-heavy with small writes, and a single file is simpler to
back up on a Pi. Chat logs, if ever stored, can go to a second table or file without changing that.

- **Records**: one `serde` struct per entity, encoded with `postcard`. Primary tables are keyed by
  numeric id (`u32`), e.g. `users: u32 -> User`.
- **Secondary indexes are explicit tables** maintained in the same write transaction:
  `users_by_name: lowercase name -> id`, `users_by_sso: ticket -> id`. All writes go through the
  `storage` API (`Store::put_user` and so on), never raw table access from the server, so indexes
  cannot drift.
- **Schema version** in a `meta` table. A record struct changes → bump the version and add a
  migration that rewrites the table. postcard has no field tags, so this is mandatory, not optional.
- **Durability**: `Durability::Immediate` (fsync) by default. Hot, losable writes (last online,
  statistics) may later use `Eventual` in batched transactions. SD cards fsync slowly, so this will
  matter.
- **Multi-step changes are one transaction** (Java commits each statement on its own): a purchase
  (items, pets, teleporter links, film, the charge and its `users_transactions` row), a gift, a club
  subscription, a finished trade (every ownership change and its rows) and opening a present (what
  it holds created, the present deleted) each use one `Writer`. The handler builds the change in
  the transaction, commits, and only then changes memory (hand, balances) and tells the client;
  on any error it drops the `Writer` (nothing is written) and leaves the player as it was. Tests
  prove it with `Store::fail_next_commits` (cargo feature `test-faults`, on for the server's
  tests): see `crates/server/src/txn_tests.rs`.
- **Blocking**: redb calls are blocking but short. Handlers call them directly on the runtime
  threads at first, and move them to `spawn_blocking` or a writer thread only if measurements on
  a Pi say so.
- **One process**: redb locks the file, so the website/housekeeping must live in the same process
  as the game server. That is one binary with an optional `web` feature. Admin actions that
  the Java side did through SQL or RCON become in-process calls.

### Mapping the 87 MariaDB tables

Not every table becomes a redb table:

- **Static game data** (`rooms_models`, `items_definitions`, `catalogue_*`, `navigator`
  categories, `external_texts`, `wordfilter`, public room items/walkways): stored in redb too (so
  housekeeping edits persist), loaded into memory at startup as the Java server does.
- **Per-user data** (`users`, `users_statistics`, badges, effects, messenger friends/requests,
  items, rooms): redb tables with the indexes the Java DAO queries need, derived from the
  `WHERE` clauses in `dao/mysql/*.java`.
- **Code, not data**: fuserights are a Java enum (`Fuseright.java`); they stay code (`fuserights.rs`).
- `settings` (138 rows): a `settings: name -> value` table, cached in memory like
  `GameConfiguration`.

Each table is ported when the first handler that needs it is ported, not all upfront.

## Import

`havana-import --mysql mysql://user:pass@host:port/db --out havana.redb`. It reads the Java schema
and writes a fresh redb file. Uses: migrating an existing hotel, and building the harness
fixture from the same snapshot the Java oracle uses, so both servers start from identical data.

## Handshake and encryption

1. On connect (Shockwave port) the server sends `HELLO`.
2. `INIT_CRYPTO` → `CRYPTO_PARAMETERS(token, true)`, with the token 24 random digits.
3. Optional `GENERATEKEY` (Diffie-Hellman) → `SECRET_KEY`, then the RC4-like stream cipher
   (`Cryptography`, `HugeInt15`) wraps both directions, with random padding.
4. `UNIQUEID`, then `SSO` (ticket) or `TRY_LOGIN` (user/password) → `UniqueIDMessageEvent`,
   `RIGHTS`, `LOGIN`, `AVATAR_EFFECTS`.

Step 3 lives in `protocol::crypto` (pure, no I/O): `Cipher` (Java `Cryptography`), `DiffieHellman`
(with `HugeInt15`'s limb decoding), `FrameEncoder`/`FrameDecoder` (the 6-byte encrypted header, the
padding driven by the tx/rx counters seeded from the token) and `Crypto`, which has a server end and
the mirror client end used by the bots. The server queues `SECRET_KEY` and then an `Encrypt` marker
in the session's outgoing queue, so the writer task encrypts exactly the frames after it; the
reader task switches to decrypting after the handler of `GENERATEKEY` returns.

**`SECRET_KEY` is sent in plain text.** `GENERATEKEY` runs on the Netty event loop, where
`writeAndFlush` passes the encoder pipeline synchronously, before `registerHandler` adds the cipher
handlers (confirmed against the oracle with a raw socket: `@A<digits>\x01`). The first encrypted
bytes are the server's answer to the next client message.

Test vectors come from the real Java classes (`tools/crypto-vectors`, including the unmodified
`EncryptionEncoder`/`EncryptionDecoder` in an `EmbeddedChannel`), and `login-encrypted` checks the
whole path against the oracle with a bot that does what the client does. One deliberate deviation:
Java's decoder re-decrypts a header when the body has not fully arrived, which garbles frames split
across TCP reads; the Rust decoder remembers the header.

SSO tickets: like Java, the server clears all tickets at startup and each ticket on use. Tickets
are issued by the ticket API (`--http`, `src/ticket_api.rs`, Havana-Web's `/api/ticket`), a small
hyper service in the same process; `--dev-ticket <user_id>=<ticket>` sets one at startup (used by
the harness). `TRY_LOGIN` and the ticket API check the Argon2 hashes the Java web wrote
(`game/password.rs`).

## Testing

- Unit tests per crate (encodings, composers against literal byte strings from traces).
- `havana-trace check` against the Rust server for every ported area. A trace that passes is never
  allowed to regress (CI runs the full corpus).
- `harness/target-rs.sh reset` builds the fixture (import of the oracle's golden snapshot), restarts
  the Rust server and issues the bot tickets, the same way `oracle.sh reset` does for Java.
