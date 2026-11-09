# Backlog

Every Java Havana feature, game and website, is ported: the code has no `TODO(...)` markers left.
New work follows the usual rule: a scenario recorded against the Java oracle first, passing
against it in three runs, then the port.

## Kept as Java does it

Behaviour that looks wrong but is Java's, reproduced on purpose. Each one is noted where it is
implemented.

| Where | Behaviour |
|---|---|
| Effects | Effects bought or given by a totem arrive at the next login (Java's `newEffect` returns null right after storing the row) |
| Bans | IP bans are stored but not enforced (Java's check is commented out) |
| Hire-A-Room | The teleporter forwards to a temporary room with id 0 that Java never registers, so nobody gets in and the user stays put, unable to walk |
| E-mail | Havana's mail templates include `base/email_header.tpl`, which Pebble looks for at the template root and cannot find: with SMTP on, every request that renders a mail fails there, and no mail is sent |
| Camera rooms | Leaving Lido B or Rooftop Rumble before anyone sent `G_STAT` throws in Java and skips the rest of `leaveRoom` |
| `normalise.input.strings` | In the game, the combining marks of Java's NFD cannot be written in ISO-8859-1: `é` reaches the client as `e?` |

## What cannot be reproduced

| Where | Why |
|---|---|
| The website's most active groups | Java sorts a `HashMap` keyed by `Group` objects, whose hash is their identity: ties come out in an order that changes from run to run. The port keeps id order, and the web traces' seed has no ties |
| Timers | Java's scheduler threads race the connection's event loop; the traces mark those steps (`dedupe`, `header_only`, `ignore_status`) where the order is not fixed in Java either |
| Ties in `ORDER BY` | MariaDB returns rows with equal keys in no fixed order; the port keeps insertion order, and the seeds avoid ties |
