# Backlog

Java Havana features that rusbbo does not port yet and that the hotel it was tested against
(Aeyra) does not use today: no rows in their tables, or switched off in `settings`. Each one is a
`TODO(<tag>)` in the code; grep the tag for the exact places. The row counts are from Aeyra's
database on 2026-11-08.

When one of these is picked up, it follows the usual rule: a scenario recorded against the Java
oracle first, passing against it twice, then the port.

## Features

| Feature | Tag | What is missing | Aeyra today |
|---|---|---|---|
| Groups | `groups` | Loading a player's groups at login, `GROUP_BADGES` with the favourite group, the member rank on `USER_OBJECTS`, the `groups.ids.permission.*` command permissions, the XXX and UK176 group badges, `GroupDao.deleteHomeRoom` when a room is deleted | `groups_details` and `groups_memberships` empty |
| Guides | `guides` | The guide manager: `INIT_TUTOR_SERVICE_STATUS`, guides skipping the tutorial and starting to guide, `PLAYERS_GUIDED` and the GLK badge, `GuideDao.getGuidedBy` in its command | no guides |
| User tags | `tags` | `users_tags` and its achievement (5 tags), `TagDao.removeTags` when a room is deleted | `users_tags` empty |
| Referrals | `referred` | `users_referred` and its achievement | empty |
| Room ads | `ads` | With `room.ads` on, public rooms show a random `rooms_ads` row (`ROOMAD`) | 28 rows, `room.ads` = false |
| Seasonal catalogue | `seasonal` | With `seasonal.items` on, pages with `seasonal_start`/`seasonal_end` only show in their season | `seasonal.items` = false |
| Collectables | `collectables` | Collectable pages list only the active collectable, its price and page layout follow it | 3 `catalogue_collectables` rows |
| Sale and entry badges | `badges` | `catalogue_sale_badges` (a badge with a purchase), `rooms_entry_badges` (a badge for entering a room) | 1 and 0 rows |
| Ecotron | `ecotron` | `ECO_BOX` boxes and their `FLAG:` prizes | not used |
| Room-hire teleporter | `room-hire` | `TeleportRoomHireInteractor` | no item definition uses it |
| Club gifts | `club-gifts` | The `CLUB_GIFT_DUE` statistic and the monthly club gift | 1 `users_club_gifts` row |
| Maintenance mode | `maintenance`, `alerts` | `INFO_HOTEL_CLOSING` while the hotel is in maintenance (login, catalogue, trade), the other placeholders of `StringUtil.replaceAlertMessage` | `maintenance` = false |
| Hidden public rooms | `publics` | Hidden public rooms in the navigator and teleporters in public rooms | not used |
| Custom rooms | `custom-rooms` | Custom rooms keeping their item states and getting game tasks like arenas | not used |

## Small parity differences

Behaviour that differs from Java only in corner cases. Each one needs a scenario that shows it
before it is changed.

| Tag | Difference |
|---|---|
| `parity` | A second `GENERATEKEY` on an encrypted connection is ignored (Java answers it); Java's `BigInteger(String)` also accepts a leading `+`/`-` in the Diffie-Hellman key; messenger lists follow Java's `HashMap` order only for small ids; Unicode normalisation with `normalise.input.strings` on (off in Aeyra) |
| `settings` | `normalise.input.strings` for post-its and room text |
| `collation` | MariaDB's accent-insensitive search (`e` = `é`) in room search |
| `ip-log` | Room votes only count when the voter has logged IPs (Java joins `users_ip_logs`) |
| `trade` | A redeemable gift or placed item from the same IP as its receiver trade-bans the buyer |
| `transactions` | The "Exchanged ... into ... credits" transaction row when furni is exchanged for credits |
| `inventory` | Dropping hand items that are placed in the player's current room |
| `rooms` | Java keeps the spam counters and the `:teleport` flag on the player across rooms |
| `walkways` | Which room's users get a user's status when they cross a walkway |
| `effects` | Effects bought or given by a totem arrive at the next login, as in Java (its `newEffect` bug); give them at once if Java is fixed |
| `pool` | Java throws and skips the rest of `leaveRoom` when nobody ever sent `G_STAT` in a pool room |
| `bans` | IP bans are stored but not enforced, as in Java (its check is commented out) |

## Bigger tracks, pending a decision

- **The Flash client**: the Flash port (12323) and every `TODO(flash)` (about 20).
- **The website**: Havana-Web reads and writes MariaDB, while rusbbo keeps its own redb file, so
  both cannot run on the same hotel as they are. Rusbbo serves the ticket API (`--http`) today.
