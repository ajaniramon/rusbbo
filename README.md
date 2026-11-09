<p align="center">
  <img src="docs/logo.png" alt="rusbbo" width="600">
</p>

<p align="center">
  <b>A retro hotel server in one small Rust binary.</b><br>
  The Habbo Hotel v31 (Shockwave) and r39 (Flash) server <i>and</i> its website, rewritten from
  <a href="https://github.com/Quackster/Havana">Havana</a> and checked byte for byte against it.
</p>

<p align="center">
  <a href="https://github.com/ajaniramon/rusbbo/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/ajaniramon/rusbbo/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/ajaniramon/rusbbo/releases"><img alt="Release" src="https://img.shields.io/github/v/release/ajaniramon/rusbbo?include_prereleases&label=download&color=e8833a"></a>
  <a href="LICENSE"><img alt="License: AGPL-3.0" src="https://img.shields.io/badge/license-AGPL--3.0-1f3a68"></a>
  <img alt="Rust 1.85+" src="https://img.shields.io/badge/rust-1.85%2B-c0392b?logo=rust&logoColor=white">
  <br>
  <img alt="Shockwave v31" src="https://img.shields.io/badge/client-Shockwave%20v31-b03a2e">
  <img alt="Flash r39" src="https://img.shields.io/badge/client-Flash%20r39-e8833a">
  <img alt="Storage: one redb file" src="https://img.shields.io/badge/storage-one%20redb%20file-6c5ce7">
  <img alt="13 MiB idle" src="https://img.shields.io/badge/RAM-13%20MiB%20idle-2e8b57">
  <img alt="No MariaDB, no JVM" src="https://img.shields.io/badge/needs-no%20MariaDB%2C%20no%20JVM-2a9d8f">
</p>

---

**rusbbo** is a complete rewrite of Havana, the Java emulator for the classic 2009 hotel client,
in Rust. Everything Havana does, the game server, the Flash connection, the website with its
homes, groups and housekeeping, and the avatar imager, runs from **one process** with an
**embedded database file**. No MariaDB, no JVM, no RCON glue between a web server and a game
server: download the binary, point it at your data, and the hotel is open.

It is not "inspired by" Havana. The Java server is the specification: scripted bots recorded
how Havana answers, and rusbbo had to give **the same bytes back**, game and website, quirks
included.

> rusbbo is a fan project. It is not affiliated with, endorsed by or connected to Sulake or
> Habbo Hotel; "Habbo" is a trademark of Sulake. This repository contains no client, no client
> assets and no Sulake code: you need your own copy of the client to play.

## ✨ Highlights

- 🏨 **The whole hotel**: every feature of Java Havana is ported, nothing is left as a `TODO`.
- 🎯 **Byte-for-byte compatible**: 113 game traces and 17 website traces recorded from the Java
  server pass against rusbbo; the real v31 client has been played against it on a copy of a
  real hotel's database.
- 🪶 **Featherweight**: 13 MiB of RAM idle, 32 MiB with 500 bots online. Built to run on a
  Raspberry Pi.
- ⚡ **Fast**: logins 4x faster, chat round trips 2.7x faster and a third of the CPU of the Java
  server on the same machine.
- 💾 **One file of data**: an ACID, crash-safe embedded database (redb). Back it up by copying a
  file.
- 🌐 **Website included**: Havana-Web's pages, homes, groups, forums, minimail, e-mail, the
  imager and the housekeeping (in English or Spanish), served from the game's own process.

## 📊 Performance

Same data, same machine (12 cores, WSL), 500 bots logging in, entering public rooms, walking and
chatting (`rusbbo-trace load`):

| | Java Havana | rusbbo |
|---|---:|---:|
| Memory idle / with 500 bots | 150 / 302 MiB | **13 / 32 MiB** |
| CPU with 500 bots (% of one core) | 27.6 % | **8.7 %** |
| Login p50 / p99 | 7.7 / 10.1 ms | **1.7 / 2.5 ms** |
| Room entry p50 / p99 | 1.2 / 3.3 ms | **1.1 / 1.5 ms** |
| Chat round trip p50 / p99 | 0.70 / 1.26 ms | **0.26 / 0.46 ms** |

And that is without MariaDB, which Havana needs next to it.

## 🎮 Features

### The game (Shockwave v31)

- **Logging in**: the full handshake, including the encrypted transport (Diffie-Hellman key
  exchange and the RC4-like stream cipher), SSO tickets from the launcher and username/password
  logins, Argon2 password hashes, the daily credits and pixels handouts, idle `PING`s and AFK.
- **Navigator**: public and private room categories, searches, own rooms, favourite rooms,
  recommended rooms, room events and visitor counts.
- **Rooms**: the Java pathfinder and the 500 ms room tick, chat, shout and whisper, dancing,
  waving, carrying drinks, `LOOKTO`, the word filter, the ignore list, room ratings, rights,
  doorbell and passwords, kicks and room bans, room settings and deleting rooms.
- **Furniture**: placing, moving, rotating and picking up floor and wall items, wallpaper and
  floors, and every interactor: teleporters (within and between rooms), rollers, post-its,
  gates and one-way doors, beds, chairs, dice, the wheel of fortune, vending machines, the hockey
  light and scoreboard, the fortune teller, the love randomizer, totems, step lights,
  multi-height furniture, presents, trophies engraved with name, date and message, the room
  dimmer and its presets, and credit furni exchange.
- **Catalogue and economy**: pages, purchases (one transaction each), gifts, Habbo Club with its
  monthly gifts, seasonal and collectable pages, vouchers, the Ecotron recycler, the hand,
  trading, and the `users_transactions` audit trail.
- **Social**: the messenger (friends, requests, categories, private messages, following friends,
  invitations), groups and their badges, guides, user tags, referrals, badges and achievements,
  avatar effects.
- **Public rooms**: the Habbo Lido (changing booths, swimming, the diving board, the score
  votes), Rooftop Rumble's Wobble Squabble, the Space Cafe, the Infobus with its polls, public
  room NPCs handing out drinks.
- **Games**: BattleBall and SnowStorm (lobbies, teams, spectators, the arenas, power ups,
  throws, scores, history, xp and monthly rankings), and the game hall tables: TicTacToe,
  Battleships, Chess and Poker.
- **Pets**: bought with their nest, living in the room, eating, drinking, playing, obeying
  their owner (`sit`, `come`, `jump`, `play dead`...), and gifted inside presents.
- **Music**: the sound machine and TRAX songs (making, editing, burning to discs), playlists and
  the jukebox, and American Idol's stage with its songs and vote chairs.
- **Camera**: photos taken, saved and shown through the Multi User Server (MUS).
- **Moderation and staff**: user and machine bans, mutes and room mutes, the automatic ban
  for bannable sentences, the moderation tool (alerts, kicks, bans), calls for help, the staff
  chat commands (`:shutdown` with the maintenance countdown, `:bot`, `:reload`, `:uptime`,
  `:usersonline`, `:rainbow`...), and every RCON message the website can send.

### The Flash client (r39)

On its own port, sharing players, rooms and ids with Shockwave clients, with Havana's
Flash-only messages and per-client message variants. See [docs/FLASH.md](docs/FLASH.md).

### The website (Havana-Web)

- The home page, registration, login, password recovery and account activation by e-mail
  (SMTP/SMTPS, with Havana's mail templates; see [docs/BACKLOG.md](docs/BACKLOG.md)), the `/me` page with news, alerts, tags, friends, events and room selection.
- Profiles, credits, Habbo Club, collectables, community, news, games, tags, FAQ, minimail and
  the habblets.
- **Homes**: widgets, stickers, notes, guestbooks, ratings, badges and TRAX, with the sticker
  store and edit mode.
- **Groups**: group pages, members, tags, favourites and discussion forums.
- **The imager**: avatars and badges drawn pixel for pixel like Havana's avatara4j.
- **The housekeeping** (`/allseeingeye/hk`): dashboard, users (edit, credits, pixels, club,
  badges, bans, alerts, log in as), rooms, groups, the catalogue editor, item definitions, room
  models and categories, vouchers, the word filter, the recycler, news, room ads and badges,
  Infobus polls, chat logs, the transaction lookup, hotel and room alerts, the settings, and an
  audit log of everything staff did. **In English or Spanish**: it follows the browser's
  language, with an EN/ES switch in its header.
- The launcher's ticket API (`/api/ticket`, `/api/login`), rate limited per IP, and
  `--build-launcher`, which compiles the Windows launcher for your hotel.
- A first administrator created on the first start, with its password printed to the console.

What Havana does that looks odd but is kept on purpose is listed in
[docs/BACKLOG.md](docs/BACKLOG.md).

## 🚀 Getting started

### 1. Get the binaries

Download `rusbbo-server` and `rusbbo-import` for your platform from the
[releases page](https://github.com/ajaniramon/rusbbo/releases) (Linux x86_64 and ARM64,
Windows, macOS), or build them (Rust 1.85 or newer):

```sh
cargo build --release
# target/release/rusbbo-server and target/release/rusbbo-import
```

### 2. Run it

In an empty folder:

```sh
rusbbo-server --bind 0.0.0.0:12321 --web 0.0.0.0:80
```

That is all. On the first start (no `rusbbo.redb` yet) the server sets up a new hotel by itself,
in about ten seconds:

1. It downloads [Havana](https://github.com/Quackster/Havana) v1.5.4's own data (rusbbo ships
   none): `havana.sql` and its migrations, the website's templates and files, the figure data and
   the avatar imager's resources. The SQL files and the release are checked against their
   SHA-256.
2. It writes the files to `tools/` (`tools/www-tpl`, `tools/www`, `tools/figuredata.xml`,
   `tools/imaging`), the paths the server uses by default.
3. It imports `havana.sql` into `rusbbo.redb`. No MariaDB is involved: the dump is read directly.
4. It creates an administrator and prints its credentials:

```text
========================================================================
  No administrator yet, so one was created:

    username  admin
    password  yw28eQHywykNym
  ...
```

Later starts just open `rusbbo.redb`. To start over, stop the server and delete it.

| Port | What | Flag |
|---|---|---|
| 12321 | Shockwave game port | `--bind` |
| 12322 | MUS, the Shockwave camera | `--mus` (default: game port + 1) |
| 12323 | Flash game port | `--flash-bind` (default: game port + 2) |
| 80 | Website, housekeeping, imager and ticket API | `--web` |

Log in with the admin to the housekeeping at `/allseeingeye/hk` (and change the password in the
website's settings). Set the hotel's public addresses in its **Configuration** page
(`site.path`, `site.name`, `loader.game.ip`, `loader.game.port`...). To make another user an
administrator, restart with `--set-rank theirname=8`. (The admin is also created on any start
where no user has rank 8.)

### 3. Add the client files

Neither Havana nor rusbbo includes the game clients or the website's images, which are Sulake's.
Havana's README links to an archive of them (`havana_www_*.7z`). Extract it into `tools/www`.
That folder is where the clients (`dcr/v31`, `flash`...) and images (`web-gallery`,
`c_images`...) are served from.

### 4. Build the launcher for your players

Players log in with a small Windows launcher
([rusbbo-launcher](https://github.com/ajaniramon/rusbbo-launcher)) that starts the Shockwave or
Flash client. Its hotel address is compiled in, so rusbbo builds it for you (it needs `curl`,
`tar` and the [.NET 9 SDK](https://dotnet.microsoft.com/download/dotnet/9.0), and works on
Linux, macOS and Windows):

```sh
rusbbo-server --build-launcher dist --launcher-url http://hotel.example.com --launcher-name "My Hotel"
# dist/My Hotel Launcher.exe: one self-contained .exe to hand out
```

Without `--launcher-url` and `--launcher-name` it uses the `site.path` and `site.name`
settings from `--db` (stop the server first, or pass both).

### Moving an existing Havana hotel

`rusbbo-import` copies a Havana database, users and all, into a rusbbo database file. It reads
either a MariaDB/MySQL server or a `.sql` dump of one:

```sh
rusbbo-import --sql backup.sql --out rusbbo.redb
rusbbo-import --mysql mysql://havana:password@127.0.0.1:3306/havana --out rusbbo.redb
```

Then point the server at your Havana's files with `--figuredata`, `--web-templates`,
`--web-site` and `--web-imaging`, or copy them into `tools/`.

### Command line

Every option is explained by `rusbbo-server --help` and `rusbbo-import --help`. Logging is set
with `RUST_LOG` (`info` by default; `RUST_LOG=debug` for everything). To back up the hotel,
stop it and copy the `.redb` file.

## 🔧 How it works

### Architecture

One tokio process serves every port. Each connection has a reader task, which runs the message
handlers, and a writer task fed by a channel, so sending never blocks. Game state (players,
rooms, the catalogue, the navigator) lives in memory behind fine-grained locks, one per room and
per player, with a fixed lock order. Ticks run at Java's own periods: a 1 s game scheduler
(handouts, AFK, effects) and a 500 ms task per loaded room (walking, statuses), because clients
and traces depend on that timing.

The website runs in the same process on the same store. Where Havana-Web writes to MariaDB and
then pokes the game server over RCON, rusbbo changes the store and the live game state directly:
a ban kicks at once, a catalogue edit reloads at once.

### Persistence

All data lives in **one redb file**: an embedded, ACID, crash-safe key-value store of
copy-on-write B-trees, with no server process and nothing to configure.

- **Typed tables**: 120 tables cover Havana's 87 MariaDB tables. Each record is a Rust struct
  encoded with `postcard`; primary tables are keyed by numeric id.
- **Explicit indexes**: every query the Java DAOs make has its own index table (users by name,
  by SSO ticket...), written in the same transaction as the record, through one storage API, so
  indexes never drift.
- **Real transactions**: a purchase, a gift, a club subscription, a finished trade or an opened
  present is **one** transaction (Java commits statement by statement). The server builds the
  change, commits, and only then updates memory and tells the client; if anything fails, nothing
  is written and the player is left as they were. Tests inject commit failures to prove it.
- **Durable**: every commit is synced to disk before it is acknowledged.
- **Versioned**: the schema version is stored in the file, so a build never opens data it does
  not understand.
- **Fast reads**: static game data (furniture definitions, the catalogue, room models, the
  navigator, settings) is loaded into memory at startup, as Havana does, and written back when
  the housekeeping edits it.

### How it was ported

rusbbo was ported test-first against the Java server:

1. A **scenario** scripts what bots send (TOML): log in, walk here, buy that, open the present.
2. `rusbbo-trace record` runs it against Java Havana and saves every byte it answers (twice,
   to make sure the recording is deterministic; anything volatile is masked).
3. The feature is ported until `rusbbo-trace check` replays the trace against rusbbo with **no
   difference**. Website traces compare status, headers and HTML byte for byte.

The result is about 97,000 lines of Rust with 358 unit tests, plus pieces Havana takes from
libraries, rewritten to match them exactly: a **Pebble 3.1 template engine**
(`crates/pebble`) and the **avatara4j imager**, which reproduces Java2D's own blending
arithmetic so every pixel matches. Havana's bugs that a client could notice are kept, and listed
in [docs/BACKLOG.md](docs/BACKLOG.md).

The traces and the Java reference setup are kept out of this repository; `rusbbo-trace` is
here, and its `load` command is the load test above.

## 📁 Layout

| Path | What it is |
|---|---|
| `crates/server` | `rusbbo-server`: the game server, the Flash port, the MUS, RCON and the website (`src/web`) |
| `crates/protocol` | The FUSE wire protocol: Base64/VL64 encodings, framing, encryption, message ids |
| `crates/storage` | The redb storage: typed tables, indexes, transactions, schema version |
| `crates/import` | `rusbbo-import`: a Havana database (MariaDB, or a `.sql` dump read through SQLite) to a redb file; also the server's first-start import |
| `crates/pebble` | The Pebble template engine for Havana-Web's templates |
| `crates/trace` | `rusbbo-trace`: scripted bots to record, check and load test a server |
| `docs/` | [Design](docs/DESIGN.md), [website](docs/WEB.md), [Flash](docs/FLASH.md), [kept quirks](docs/BACKLOG.md) |

## 🙏 Credits

- [Havana](https://github.com/Quackster/Havana) by Quackster and its contributors: the server,
  the website and the years of work rusbbo is a translation of.
- [avatara4j](https://github.com/Quackster/Avatara4j) and [Pebble](https://pebbletemplates.io/),
  whose behaviour rusbbo reproduces.
- [redb](https://github.com/cberner/redb), [tokio](https://tokio.rs) and the rest of the Rust
  ecosystem.

## 📜 License

[GNU AGPL-3.0](LICENSE), the same license as Havana, which rusbbo is derived from. If you run a
modified rusbbo for others over a network, you must offer them its source.
