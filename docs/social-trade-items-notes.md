# Social actions, chat, trading and interactors: Java behaviour to replicate

Notes from recording `traces/social-*.json`, `chat-filter`, `chat-commands`, `trade` and the interactor
traces (`flat-teleport`, `flat-roller`, `flat-postit`, `flat-gates`, `flat-present`) against the
Java server. They explain the traces; the traces stay the spec.

## Timing that shapes the scenarios
- Java pings a connection 60 s after it opens (`IdleStateHandler(60, 0, 0)`; in the recordings it
  fired for bots that kept sending, so it does not wait for 60 quiet seconds) and kicks it
  (`HOTEL_LOGOUT`, `LOGOUT` to the room) when the next one fires without a `PONG` in between.
  `PONG` runs `processAchievements(player, false)`, which hands out time-dependent achievements
  (`ACH_RegistrationDuration` one level per call, `ACH_TraderPass`) with pixel rewards. The bots
  never answer, so every scenario keeps each connection under 60 s.
- Look timer: chat makes every other user within 14 tiles look at the speaker (a shout: any
  distance; whispers: nobody). `Position.getDistanceSquared` returns the distance, not its square.
  Looking (and `LOOKTO`) sets `lookTimer = now + 6` in whole seconds; the 1 s `StatusTask` then resets
  the head rotation and sends one more `USER_STATUSES` 6-7 s later, even when nothing changed.
- Status changes (dance aside) go out through `needsUpdate` on the next 500 ms room tick.
- Teleporters, one-way doors and teleporter arrivals run on scheduler timers (see below), not on
  the room tick. Rollers run on their own 2 s task (`roller.tick.default`).

## Chat, whispers, filter
- `WHISPER` body: one string, `"<username> <message>"`. Sent to the sender and the named user only,
  as `CHAT_MESSAGE.WHISPER` (25) with the sender's instance id. Unknown or ignoring target: the
  sender alone. Gestures come from smileys as in chat. Nobody turns their head.
- The ignore list used by chat and whispers is filled only by `GET_IGNORE_LIST` (the real client
  sends it after login) or `IGNORE_USER`. An ignoring user gets the `TYPING_STATUS` and the head
  turns, never the chat message.
- Word filter (`wordfitler.enabled` [sic], replacement `bobba`): words with `is_filterable = 0`
  ('zaphotel', ...) are replaced for everyone before anything else; then each receiver with
  `users.wordfilter_enabled = 1` gets the filterable words replaced too. Any replacement lowercases
  the whole sentence (`"Ask Aaron"` -> `"ask bobba"`). Same for chat, shout and whisper; the speaker's
  own setting does not matter. A sentence that is only a mandatory word becomes `"bobba"`.
- Bannable words (`is_bannable = 1`) ban users who joined less than 30 minutes ago (also users whose
  join date is in the future, like tracebot3-6). Not covered: it writes a 20-year ban.

## Chat commands (`game/commands`)
- Checked after the filter, before the mute check: `:name args` whose name (any alias, case-
  insensitive) the user may run is executed and is never chat; no `TYPING_STATUS`. A command above
  the user's rank, or an unknown one, is ordinary chat (`:shutdown now` even gets the sad gesture,
  because it contains `:s`).
- Too few arguments: `ALERT "You have not supplied any arguments!"` (external text
  `player_commands_no_args`). Arguments are `message.replace(":" + name + " ", "").split(" ")`.
- `:about`/`:info`: fixed `ALERT`. `:help`/`:commands [filter] [page]`: commands the user has rights
  for, sorted by first alias, 10 per page, `"Page n out of m"`; a filter matches any alias substring.
- `:givedrink user`: `ALERT "Could not find user: x"` (not in the same room too), `ALERT "You are not
  carrying any food or drinks to give."`; target already carrying: whisper to the giver
  `"<name> is already enjoying a drink."`; success: target carries the drink and gets a whisper
  from itself `"<giver> handed you a <handitemN text>."`, the giver stops carrying (`USER_OBJECTS` +
  `USER_STATUSES` refresh), both statuses go out together on the next tick.
- `:dropdrink`: stops carrying (refresh) or `ALERT "You are not carrying any food or drinks to drop."`.
- `:sit`: `USER_DANCE 0` and `sit 0.5` at the even rotation; ignored when already sitting.
- `:afk`: `USER_SLEEP true` to the room; walking wakes up (`USER_SLEEP false` first).
- `:guidestatus`: `ALERT` with the guide table (empty for the bots).
- `:pickall` (owner): `REMOVE_FLOORITEM`/`REMOVE_WALLITEM` per item (post-its stay), then the hand.
- Not covered: `:uptime`, `:usersonline` (time and peak dependent), `:rgb` (dimmer).

## Social actions
- `LOOKTO`: Shockwave sends `"x y"` as the raw body (Java also accepts two VL64s). Own tile, lying,
  or `NO_HEAD_TURN` furni: ignored. Sets body and head rotation; status on the next tick.
- `DANCE n`: 0 stops, 1 for everyone, 2-4 need `fuse_use_club_dance` (a non-club user's request is
  ignored entirely). Not while sitting, lying or swimming. `USER_DANCE (instance, n)` goes out at
  once to the room. Dancing while carrying drops the drink: `USER_DANCE`, then the refresh
  (`USER_OBJECTS`, `USER_STATUSES`, `USER_DANCE` again).
- `STOP` takes the raw body `Dance` or `CarryItem`.
- `CARRYDRINK`/`CARRYITEM n`: public rooms allow any id; flats only from an adjacent vending machine
  (that part of `canCarry` is commented out: flats refuse). Status `carryd n` on the next tick; it
  switches to `drink` after 12 s and expires after `carry.timer.seconds` (300). `0` (or `STOP
  CarryItem`) refreshes the user (`USER_OBJECTS` + `USER_STATUSES`). `USER_CARRY_OBJECT` goes to
  Flash clients only. 20 (camera) needs a camera in the hand.
- Typing: `TYPING_STATUS (instance, bool)` to the room, only on a change. Chat sends
  `TYPING_STATUS false` first.
- `GOAWAY`: walk to the door (kick with walking allowed). Every tick's `USER_STATUSES` to all; at the
  door the leaver gets `HOTEL_VIEW`, the others `LOGOUT` and a last `USER_STATUSES` for instance -1.

## Trading
- `TRADE_OPEN` (VL64 instance id) needs the room's category `allow_trading` (Trade floor, 113).
  Both users get `TRADEOPEN (own id, own trade_enabled, partner id, partner trade_enabled)`. The trade
  starts (status `trd` for both, next tick) unless *both* have trading disabled. With one side
  disabled, accept/add/confirm are all ignored; only closing works.
- Partner already trading: `TRADE_ALREADY_OPEN` to the opener.
- `TRADE_ADDITEM` (VL64 item id from the hand): ignored when already offered, not in the hand,
  in confirmation, or both accepted. Resets both accepts silently and sends `TRADE_ITEMS` to both,
  each from their own side: `own id, count, items, partner id, count, items`; an item is
  `id, "s"|"i", id, sprite id, -1, -1, custom data, -1, -1, -1[, -1 for floor items]`.
- `TRADE_ACCEPT`: `TRADEACCEPT (user id, true)` to both; when both accepted, `TRADECONFIRM` to both,
  accepts reset, offers locked. `TRADE_UNACCEPT` (68, and 403) before that: `TRADEACCEPT false`;
  otherwise it closes the trade.
- `TRADE_REMOVE_ITEM`: `TRADE_ITEMS` to both; a `TRADEACCEPT false` for each side that had accepted.
- `TRADE_CONFIRM_ACCEPT` from both: `TRADE_COMPLETED` to both, items move (same virtual ids),
  then each gets its hand (`INVENTORY`, view `new`), `trd` removed on the next tick.
- `TRADE_CLOSE`: `TRADE_CLOSE (closer's id)` and the hand to both, `trd` removed. Bug kept by the
  trace: the hands are sent before the offered items leave the trade, so items still on offer are
  missing from that `INVENTORY` (the next `GETSTRIP` shows them again).

## Teleporters
- Each purchase creates two linked items (`items_teleporter_links`), virtual ids n and n+1. `door`
  is always placed with rotation 2 (`redirect_rotation_2`).
- `USEFURNITURE` on a teleporter not touching the user: walk to the square in front. In front:
  open (custom data 1), walk in, then on scheduler timers: 1 s effects (2), 1.5 s `LOGOUT` in the
  room and close (0), then same room: warp onto the partner (`USER_OBJECTS`, `USER_STATUSES` twice),
  partner effects (2); 3 s partner open (1) and walk out to its front; 4 s partner closes (0).
  `STUFFDATAUPDATE` uses the computed header 88 with `"<id>\x02<custom data>\x02"`.
- The partner is found through the database (`room_id` is written at once on placing) and must be in
  a loaded room, else the user walks in and back out.
- Other flat: at 1.5 s `ROOMFORWARD (false, room id)`; the client sends `TRYFLAT` + `GOTOFLAT`, and
  the user is put on the partner teleporter instead of the door: effects at once, open + walk out at
  1 s, close at 1.5 s (timers start at `GOTOFLAT`, so they arrive before the room load requests).

## Rollers
- Every 2 s (phase 250 ms after the room loads) each roller moves the items on it and the first
  user on it one square in its rotation's direction: `SLIDEOBJECTBUNDLE` = roller x, y, front x, y,
  item count, per item (id, from z string, to z string), roller id, then 2 + instance id + from z +
  to z for a user or 0 for none. A user then gets `USER_STATUSES` at the new square when the roll
  completes (800 ms later, `RollerCompleteTask`). The heights are strings (`0.45` on the roller).
  Only private rooms run rollers; a roller with nothing to move sends nothing.

## Post-its
- The catalogue pad has custom data 20. `PLACESTUFF` on the wall creates a new item (new virtual
  id, colour `FFFF33`, owned by the room owner) and decrements the pad (deleted at 0).
- `G_IDATA`: `IDATA "<id>\x02<colour> <text>\x02"`. `SETITEMDATA` (VL64 id, string `"<colour>
  <text>"`, the separator char is skipped): colour must be one of the five; without rights the new
  text must start with the old one; only owners may change the colour. Sends `STUFFDATAUPDATE`
  (computed header 85) `"<id>\x02<wall position>\x02<colour>\x02"` to the room.
- `USEWALLITEM` does nothing to a post-it; `REMOVEITEM` is owner-only and deletes it
  (`REMOVE_WALLITEM`).

## Gates and one-way doors
- Gates (`gate` behaviour, default interactor, rights needed): walkable only while open (custom
  data `1`). `USEFURNITURE` toggles with `STUFFDATAUPDATE` (88); while a user stands in the gate it
  does nothing at all (no message). Walking onto a closed gate: no path, nothing sent.
- One-way doors (`one_way_gate`): the One Way Gates page is club-only and its doors cost 15 credits
  and 30 pixels (`NO_CREDITS (false, true)` without pixels; a purchase sends `CREDIT_BALANCE` and
  `ActivityPointNotification` with the pixels left). `ENTER_ONEWAY_DOOR` (VL64 id) only from the
  square in front, with a free square behind: `CHANGESTATUS (id, 1)` to the room, the user walks
  through (two ticks), `CHANGESTATUS (id, 0)` 1 s later on a scheduler timer. From anywhere else it
  is ignored, and the closed door is solid, so walking back goes around it.

## Presents
- `GRPC` as a gift: `page, item, extra data, true, receiver name, note`. Unknown receiver:
  `NO_USER_FOUND name`. One gift per minute per buyer (`ALERT "Not so fast! ..."`). Success:
  `ALERT "Successfully purchased gift for user x!"`, `CREDIT_BALANCE`; an online receiver gets the
  wrapped present in its hand at once (`INVENTORY`).
- The wrapping is random (`present_gen`, `present_gen1`..`6`), so the present's sprite id varies:
  those messages are recorded header-only. The note shows as the present's custom data when placed.
- `PRESENTOPEN` (VL64 id, owner only): ordinary furni keep the item and its virtual id, changing its
  definition: `REMOVE_FLOORITEM`, `DELIVER_PRESENT ("s"|"i", sprite id, sprite)`, the hand.
  Teleporters, trophies, post-its, decorations and packages are bought anew instead.

## Porting checklist
Three groups that can be ported independently; each only needs what is already ported (rooms,
flats, furniture, the hand, the catalogue).

1. **Social and chat** (`social-actions`, `social-dance-carry`, `chat-filter`, `chat-commands`):
   handlers `WHISPER`, `LOOKTO`, `DANCE`, `STOP`, `CARRYDRINK`, `CARRYITEM`, `GOAWAY` (typing,
   chat, shout are ported); the word filter (`wordfilter` table, `users.wordfilter_enabled`) in chat,
   shout and whisper; the ignore list feeding chat and whispers; the look timer in the status task;
   the carry status with its timers; `CommandManager` and the rank-1 commands. Composers:
   `CHAT_MESSAGE.WHISPER`/`SHOUT`, `USER_DANCE`, `USER_SLEEP`, `ALERT`, `IGNORED_LIST`, `HOTEL_VIEW`
   (all exist in Rust).
2. **Trading** (`trade`): `TRADE_OPEN`, `TRADE_ADDITEM`, `TRADE_REMOVE_ITEM`, `TRADE_ACCEPT`,
   `TRADE_UNACCEPT` (68 and 403), `TRADE_CONFIRM_ACCEPT`, `TRADE_CLOSE`; `RoomTradeManager`; the
   category's `allow_trading` and `users.trade_enabled`. Composers: `TRADEOPEN`,
   `TRADE_ALREADY_OPEN`, `TRADE_ITEMS`, `TRADEACCEPT`, `TRADECONFIRM`, `TRADE_COMPLETED`,
   `TRADE_CLOSE` (new); `INVENTORY` view `new` (exists).
3. **Interactors** (`flat-teleport`, `flat-roller`, `flat-postit`, `flat-gates`, `flat-present`):
   teleporter pairs at purchase and `TeleportInteractor` with its timers, the arrival in `GOTOFLAT`
   and `TRYFLAT` keeping the teleporter target; `RollerTask` + `RollerCompleteTask`; post-it
   placing, `G_IDATA`, `SETITEMDATA`, `REMOVEITEM`; gates in the default interactor and the
   pathfinder, `ENTER_ONEWAY_DOOR`; pixel prices and gift purchases in `GRPC`, `PRESENTOPEN`.
   `flat-roller` ends with `:pickall`, which needs the command framework of group 1 (or a stub for
   that one command). Composers: `ROOMFORWARD`, `SLIDEOBJECTBUNDLE`, `IDATA`, `CHANGESTATUS`,
   `DELIVER_PRESENT`, `NO_USER_FOUND` (new); `STUFFDATAUPDATE` 88/85, `PLACE_WALLITEM`,
   `REMOVE_WALLITEM`, `NO_CREDITS`, `ActivityPointNotification` (exist).

Seed users for these traces: tracebot3 (ignore list), tracebot4 (club dances), tracebot5 (trading,
club, 100 pixels) and tracebot6 (trading, word filter off); see `oracle/seed.sql`.
