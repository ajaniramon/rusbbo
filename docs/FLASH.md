# The Flash client

Havana serves the Flash client (r39) next to Shockwave, and so does rusbbo:
`rusbbo-server --flash-bind 0.0.0.0:12323` (Java: the game port plus 2). Both kinds of client
share the players, rooms and ids, and speak the same B64/VL64 protocol. What changes is in a few
places only, all of them the Java server's `isFlashConnection()` branches.

## The connection

- No `HELLO` and no RC4 layer. A read that does not start with `@` (the Flash player's
  `<policy-file-request/>`) gets the cross-domain policy XML (`net.rs`, `FLASH_POLICY`), as
  Java's `NetworkDecoder` does; the connection stays open.
- `INIT_CRYPTO` is answered with `SESSION_PARAMETERS` (Java's Flash login flow).
- `Session::is_flash()` marks the session. The encoder writes `<br>` as `\r` in `ALERT`, and byte
  149 as `?` in the chat headers (24, 25, 26, 134), as Java's `NetworkEncoder` does.

## Messages built for each client

Some messages Java composes per player read `isFlashConnection()` of the *receiver*. A room sends
them through `Room::send_with(|flash| ..)` (or `send_pair([shockwave, flash])`), which builds each
kind at most once:

- `USER_OBJECTS`: pet ids +1,000,000,000 and bot ids +1,500,000,000, the bots' Flash figures.
- `USER_STATUSES`: the picnic bench's seat height.
- `USERBADGE`: the user id as a number (Shockwave: as text).
- `ROOMEEVENT_INFO`: the event's tags (Shockwave: none).
- `AVATAR_EFFECTS`, `CATALOGUE_PAGE` (`frontpage3`) and the hand (`INVENTORY`: every item, not a
  page of nine).

## Flash-only messages

`messages/incoming/flash.rs` and `messages/outgoing/flash.rs`:

- Login: `HOME_ROOM`, `FLASH_FAVOURITES`, `FLASH_MODTOOL` for moderators; `GET_INFO` gives the
  badges and achievements.
- The navigator (380-382, 430-439): public rooms, guest rooms by mode, search by name, owner and
  tag, popular tags, events, room info, the home room, favourites with `FLASH_FAVOURITE_STATUS`.
- Room settings and info (384-388, 400, 401): `FLASH_FLATINFO`, `FLASH_EDITDATA`, icons,
  `FLASH_ROOM_UPDATED`.
- Entering a room: event 390 runs `GETROOMAD` and `G_HMAP` (Java registers both on it): the room's
  contents, users, `FLASH_ROOMENTRYINFO`, `FLOOR_MAP`, `HEIGHTMAP_UPDATE`.
- The wardrobe (375, 376), `CHANGE_LOOKS` (44, validated against the game's `figuredata.xml`,
  `--figuredata`), the moderation tool (456, 459), `USER_CARRY_OBJECT` (482).

## Other branches

Rights, kicks and bans name users by id (`REMOVERIGHTS` takes a list), and `FLASH_EDITDATA`
follows a rights change. `QUIT` sends the hotel view, `DELETEFLAT` the room list again,
`ROOMFORWARD` has no public room offset, `GETPETSTAT` takes the Flash pet id. The hand is sent
after buying, placing and deleting an item. The maintenance countdown is an `ALERT`. Flash
walkers do not enter the Lido's queue, booths and lift or the idol's scoreboard and vote chairs,
and a Flash client cannot follow a friend into a public room that has no Flash navigator style
(`navigator_styles`, after the walkway redirect).

## Traces

`flash-login`, `flash-navigator` and `flash-room` (bots with `flash = true` connect to the target
port plus 2). `flash-room` has a Flash and a Shockwave client in the same rooms, so each kind's
`USER_OBJECTS`, `USERBADGE` and `ROOMEEVENT_INFO` is compared.
