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
- `crates/storage/src/web.rs`: the website's tables, as generic `Record` tables.

## Phases

| Phase | Java | Status |
|---|---|---|
| 1. Core | `HomepageController`, `AccountController`, `ClientController`, `TicketController`, `RegisterController`, `RecoveryController`, the `/me` page and what it shows (news, alerts, tags, minimail, friends, events, room selection, beginner gifts) | in progress |
| 2. Site | `ProfileController`, `CreditsController`, `ClubController`, `CollectablesController`, `CommunityController`, `NewsController`, `GamesController`, `TagController`, `FaqController`, `SiteController`, the habblets (`habblet/*`), `FriendManagementController`, `MinimailController`, `QuickmenuController` | |
| 3. Homes | `HomesController`, `StoreController`, `NoteEditorController`, `WidgetController`, `GuestbookController`, `RateController`, `BadgesController`, `TraxController`, the friends/members widgets | |
| 4. Groups | `GroupController`, `GroupHabbletController`, `GroupMemberController`, `GroupFavouriteController`, `GroupTagController`, `GroupDiscussionsController`, `DiscussionController`, `DiscussionActionsController`, `DiscussionPreviewController` | |
| 5. Housekeeping | `controllers/housekeeping/*` | |
| 6. Imager | `ImagerController` (`/habbo-imaging/*`: avatars and badges, Avatara4j) | |

## Differences from Java

- `/api/ticket` and `/api/login` are rate limited per IP (`--http-rate-limit`), as the standalone
  ticket API was.
- Static files are never served from outside the site directory (Java's `Paths.get` follows
  `..`).
