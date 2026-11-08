# The website

rusbbo serves Havana-Web (Java: duckHTTPD + Pebble, `Havana-Web/` in the Java checkout) from the
game server's process: `rusbbo-server --web 0.0.0.0:80 --web-templates <tools/www-tpl>
--web-site <tools/www>`. Pages read and write the same store as the game, so where the Java web
writes MariaDB and then sends the game server an RCON `REFRESH_*` message, the port changes the
store and the live game state directly.

The templates and static files are Havana's own (`tools/www-tpl`, `tools/www`): rusbbo has no
copy of them. `crates/pebble` renders the templates exactly like Pebble 3.1 as Havana-Web
configures it.

## How it is checked

Web scenarios (`scenarios/web/*.toml`) are HTTP request scripts. `rusbbo-trace web-record`
runs them against the Java Havana-Web (`oracle/web.sh`, port 12380, on the oracle's database and
next to the Java game server) and fails if two runs differ after masking, so everything volatile
is masked explicitly. `rusbbo-trace web-check` replays a trace against the Rust server
(`harness/target-rs.sh` with `TARGET_WEB_PORT`) and compares status, headers and body byte for
byte.

```
flock ~/oracle.lock -c "rusbbo-trace web-record scenarios/web/x.toml --reset 'oracle/web.sh reset'; oracle/web.sh stop; oracle/oracle.sh stop"
TARGET_WEB_PORT=12380 rusbbo-trace web-check traces/web/x.json --target 127.0.0.1:12380 --reset "harness/target-rs.sh reset"
```

## Layout

- `crates/pebble`: the template engine.
- `crates/server/src/web/`: `server.rs` (duckHTTPD's request pipeline), `http.rs` (parameters,
  cookies, sessions), `router.rs` + `routes_table.rs` (generated from `Routes.java` by
  `tools/gen_web_routes.py`), `tpl.rs` (templates and their binders), `beans.rs` (what Java
  objects look like to templates), `data.rs` (the DAOs), `controllers/` (one module per Java
  controller group).
- `homes.rs` (`Widget`, the sticker catalogue, guestbooks, ratings, edit sessions) and
  `forum.rs` (`GroupDiscussionDao`, topics and replies).
- `crates/storage/src/web.rs`: the website's tables, as generic `Record` tables.

## Phases

| Phase | Java | Status |
|---|---|---|
| 1. Core | `HomepageController`, `AccountController`, `ClientController`, `TicketController`, `RegisterController`, `RecoveryController`, the `/me` page and what it shows (news, alerts, tags, minimail, friends, events, room selection, beginner gifts) | done (TODO(email): the SMTP mails) |
| 2. Site | `ProfileController`, `CreditsController`, `ClubController`, `CollectablesController`, `CommunityController`, `NewsController`, `GamesController`, `TagController`, `FaqController`, `SiteController`, the habblets (`habblet/*`), `FriendManagementController`, `MinimailController`, `QuickmenuController`, `TagController`, `VoucherController`, `CollectablesController` | done |
| 3. Homes | `HomesController`, `StoreController`, `NoteEditorController`, `WidgetController`, `GuestbookController`, `RateController`, `BadgesController`, `TraxController`, the friends/members widgets | done |
| 4. Groups | `GroupController`, `GroupHabbletController`, `GroupMemberController`, `GroupFavouriteController`, `GroupTagController`, `GroupDiscussionsController`, `DiscussionController`, `DiscussionActionsController`, `DiscussionPreviewController` | done (creating topics and replies needs the captcha, so the traces only cover its errors) |
| 5. Housekeeping | `controllers/housekeeping/*` | |
| 6. Imager | `ImagerController` (`/habbo-imaging/*`: avatars and badges, Avatara4j) | |

## Data the traces need

`oracle/web-seed.sql` holds the web traces' own data (news, tags, scores, friends, groups,
minimail, forums, vouchers), loaded on top of `golden.sql` by `oracle/web.sh reset`; the Rust
fixture for web checks is built from both (`harness/target-rs.sh` with `TARGET_WEB_PORT`). Dates
are fixed times of the current day, so a recording and a check on the same day agree. Wherever
MariaDB or Java orders ties in no fixed way (GROUP BY, ORDER BY with LIMIT, identity-hash maps),
the seed avoids ties.

## Caching

Havana-Web shows several lists from memory: the `Watchdog`'s (refreshed every 30 seconds:
events, recommended and staff-pick groups and rooms, popular rooms, recent discussions, the tag
clouds, the news, the users online and the visits) and the `WebCache` regions under them (tag
cloud 5 minutes, news 2 minutes, recommendations, popular rooms, high scores, active habbos and
visits 60 seconds). `web/cache.rs` keeps the same lists with the same lifetimes, so a page is as
stale as Java's.

## Java's quirks kept

Pages show what Java shows, so these Havana-Web bugs are reproduced on purpose:

- `GuestbookDao.create` reads the new entry's key as `"id"`: the entry the page gets back has id
  -1 and creation time -1.
- `GroupDiscussionDao.getFirstReply`, `countReplies` and `countDiscussions` call
  `executeUpdate()` on a `SELECT`; they always return 0 (every post is shown as a reply, every
  list has one page).
- `RoomDao.saveGroupId` and `GroupDiscussionDao.deleteDiscussion` have their parameters
  swapped.
- `updatepost` refuses a *matching* captcha.
- `cms_stickers_catalogue` is a MyISAM table read without `ORDER BY`: products keep the
  table's storage order (`StickerProduct.position`, set by the importer), not their ids.
- Forum "has read" flags: Java's subquery counts every read row for replies older than seven
  days.

## Differences from Java

- `/api/ticket` and `/api/login` are rate limited per IP (`--http-rate-limit`), as the standalone
  ticket API was.
- Static files are never served from outside the site directory (Java's `Paths.get` follows
  `..`).
- The housekeeping audit log shows the panel's and the game's actions. Java's query fails on
  the shipped schema (`tools/migrations/hk_audit_log.sql` creates the table with another
  collation than the rest, and the `UNION` is refused), so the Java panel shows none;
  `oracle/web-seed.sql` converts the table so the traces compare the logs.
