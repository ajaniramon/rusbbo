# Private rooms and furniture: Java behaviour to replicate

Notes from recording `traces/flat-*.json` against the Java server. They explain the traces; the
traces stay the spec.

## Ids and timing
- The golden snapshot's `rooms` AUTO_INCREMENT is 1000, so the first created room is 1000. Items start at 1.
- Item ids on the wire are virtual ids from a global counter starting at 0 on every server start:
  purchases are numbered 1..n and keep their id while placed. Reloading a room
  (`ItemDao.getRoomItems`) or a hand (`Inventory.reload`) assigns new virtual ids.
- `Room.tryDispose` waits `room.dispose.timer.seconds` (30 s) before unloading; until then the room
  stays in `RoomManager` and the database is not re-read. After `DELETEFLAT`, `GETFLATINFO 1000`
  still returns the deleted room and `TRYFLAT` still answers `FLAT_LETIN` (flat-delete records this).
  flat-decor's re-entry also relies on the room still being loaded.
- `Item.save()`/`delete()` go through a GameScheduler queue flushed every 5-10 s.
- Walking and kicking emit one `USER_STATUSES` per 500 ms room tick; rights statuses after
  `ASSIGNRIGHTS`/`REMOVERIGHTS` follow the same tick.

## Navigator and settings
- `CREATEFLAT` answers only `GOTO_FLAT(1000, name)`. Silent on success: `SETFLATINFO`,
  `SETFLATCAT`, `UPDATEFLAT`, `QUIT` (to the quitter), `MOVESTUFF` without rights, `PLACESTUFF` of
  an unknown item.
- `TRYFLAT`, `GETFLATINFO` and `SETFLATINFO` load rooms that are not in memory straight from the DB
  (`RoomDao.getRoomById`).
- `FLATINFO` sends door mode 0 to someone already authenticated for the room.

## Room entry
- Owner entry: `ROOM_READY`, `FLATPROPERTY landscape`, wallpaper/floor `FLATPROPERTY` only if > 0,
  `YOUAROWNER`, `YOUARECONTROLLER`, `UPDATE_VOTES` (owner 0 → `H`; a visitor who has not voted -1),
  `ROOMEEVENT_INFO`.
- On a fresh room load the first `USER_STATUSES` in the `GOTOFLAT` step has no `flatctrl`;
  `flatctrl useradmin` first appears in `G_STAT`. Re-entering a still-loaded room: already there.
- Others see a newcomer's `USER_STATUSES` during `GOTOFLAT`, but `USER_OBJECTS` only when the
  newcomer sends `G_STAT`.
- Floor furni goes in `ACTIVE_OBJECTS` (answer to `G_USRS`), wall furni in `ITEMS` (answer to
  `G_ITEMS`). `G_OBJS` sends the hard-coded `DuHH`/`DiHH`. `GETROOMAD` on a flat: empty `ROOMAD`.

## Furniture
- Each `PLACESTUFF`/`MOVESTUFF`/floor pickup sends `HEIGHTMAP_UPDATE` before
  `PLACE_FLOORITEM`/`MOVE_FLOORITEM`/`REMOVE_FLOORITEM`; the heightmap in it stays all `0` for furniture.
  Wall items: `PLACE_WALLITEM`/`REMOVE_WALLITEM`, no heightmap.
- A no-op or invalid `MOVESTUFF` sends `MOVE_FLOORITEM` with the unchanged position, to the mover only.
- `STUFFDATAUPDATE` has a computed header: 88 floor, 85 wall. The default interactor cycles custom
  data `0..max_status-1`; `USEFURNITURE`'s status argument is ignored.
- Walking onto a solid item sends nothing. Paths go diagonally around solid items.
- Sitting sends `USER_DANCE "HH"` before the `sit 1.0` status. Picking up the chair you sit on:
  `REMOVE_FLOORITEM`, then an immediate `USER_STATUSES` without `sit`, then `INVENTORY`.
- `ADDSTRIPITEM` reads two VL64 ints (first ignored) and answers the full hand `INVENTORY` (view `update`).
- Wallpaper/floor custom data comes from the purchase's extra data, not the catalogue special sprite
  id; `""` applies 0, which does not show on re-entry. `FLATPROPBYITEM` consumes the item and sends
  `FLATPROPERTY` to the room plus `INVENTORY` to the owner.

## Doorbell, rights, kick, delete
- Doorbell: the visitor gets an empty `DOORBELL_WAIT`; everyone with rights in the room gets
  `DOORBELL_WAIT` with the visitor's name. Refusing sends `FLATNOTALLOWEDTOENTER`. Wrong password:
  `LOCALISED_ERROR -100002`.
- Shockwave `ASSIGNRIGHTS`, `REMOVERIGHTS` and `KICK` take the bare username as the raw body.
- `ASSIGNRIGHTS`: target gets `YOUARECONTROLLER`, then `flatctrl` (empty value) on the next tick.
- `REMOVERIGHTS` bug: `refreshRights` runs before the user is removed from the list, so the target
  gets `YOUARECONTROLLER` again and keeps `flatctrl/` in its status, but its rights are gone (its
  next `MOVESTUFF` is ignored).
- Kick: the target gets `HOTEL_VIEW`, walks to the door tick by tick (both see it), leaves, gets a
  second `HOTEL_VIEW`; the owner gets `LOGOUT` then a final `USER_STATUSES` with instance id -1 at the door.
- A visitor's `QUIT` sends `LOGOUT` (leaver's instance id) to the others. `DELETEFLAT` with users
  inside: the owner gets `HOTEL_VIEW`; the visitor `LOGOUT` for the owner, then `HOTEL_VIEW`. Furni
  in a deleted room is deleted, not returned to the hand.
