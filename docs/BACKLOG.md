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
| Room-hire teleporter | `room-hire` | `TeleportRoomHireInteractor` | no item definition uses it |
| Idol songs | `sound`, `idol` | The performer's song disc in `START_PERFORMANCE`, `START_PLAYING_SONG`/`STOP_PLAYING_SONG` and the listeners | not used |
| Hidden public rooms | `publics` | Hidden public rooms in the navigator and teleporters in public rooms | not used |
| Custom rooms | `custom-rooms` | Custom rooms keeping their item states and getting game tasks like arenas | not used |
| E-mail | `email` | Havana-Web's SMTP mails: password recovery, activation codes | SMTP off |

## Small parity differences

Behaviour that differs from Java only in corner cases. Each one needs a scenario that shows it
before it is changed.

| Tag | Difference |
|---|---|
| `parity` | A second `GENERATEKEY` on an encrypted connection is ignored (Java answers it); Java's `BigInteger(String)` also accepts a leading `+`/`-` in the Diffie-Hellman key; messenger lists follow Java's `HashMap` order only for small ids; Unicode normalisation with `normalise.input.strings` on (off in Aeyra) |
| `settings` | `normalise.input.strings` for post-its and room text |
| `collation` | MariaDB's accent-insensitive search (`e` = `é`) in room search |
| `inventory` | Dropping hand items that are placed in the player's current room |
| `rooms` | Java keeps the spam counters and the `:teleport` flag on the player across rooms |
| `walkways` | Which room's users get a user's status when they cross a walkway |
| `effects` | Effects bought or given by a totem arrive at the next login, as in Java (its `newEffect` bug); give them at once if Java is fixed |
| `pool` | Java throws and skips the rest of `leaveRoom` when nobody ever sent `G_STAT` in a pool room |
| `bans` | IP bans are stored but not enforced, as in Java (its check is commented out) |
| `groups` | The website's most active groups: Java sorts a `HashMap` keyed by identity hashes, so ties come out in no fixed order; the port keeps id order |
