# Crash reports

When CarlaOS on the Ally dies, it leaves a report that can be sent for diagnosis.
Nothing is sent by itself: a report is a text file on the Ally until the player or
administrator copies it.

## Where

Settings, **Crash reports**: the list, **Copy report** (to the clipboard) and
**Open folder**. The files are in `%LOCALAPPDATA%\<app id>\crash\`. The newest ten
are kept.

## What is recorded

- **A panic** (the program's own checks failing): the message, the place, a
  backtrace, a line saying what was running ("session: Hades at HOME PC, phase
  running") and the recent activity. The player is told on the next start.
- **A run that did not end** (a crash outside those checks, a forced close, Windows
  shutting down, a power cut): found at the next start because the marker saying
  "running" was never removed. These wait in Settings without a message, because a
  normal Windows shutdown with CarlaOS open looks the same.
- **A frozen window**: the interface says it is alive every 3 seconds. When a window the
  player can see stays silent for 30 seconds, also while a game or stream is marked as
  running (a session that never ended looks like that), a
  "Stopped responding" report is written once (and "responding again" goes into the
  notes if it recovers). Hidden or minimised windows (the page reports when it is hidden) and the time
  around the Ally sleeping are not judged, because a hidden page answers rarely.
  The end of a session writes each of its steps into the notes before it runs, so a step
  that never returns shows up as the last "session end:" line.
- **Recent activity** (`breadcrumbs.log`, kept to about 100 KB): a session starting,
  running and ending, a stream cut off and picked up again, a desktop stopping or
  starting to answer, and errors the interface caught. Only plain words and ids: no
  keys, PINs, tokens or things typed by the player.

IPv4 addresses and the Windows account name in paths are replaced (`x.x.x.x`,
`<user>`) in every report. This is best effort: read a report before sending it.

## Logs

When the Ally shows a desktop as connected that is not, or the app stops responding after a
game, the report is not enough: nothing crashed. The **connection log** says what the link
was doing, to the millisecond.

- **Where**: `%LOCALAPPDATA%\<app id>\logs\connection.log`, next to the `crash` folder, cut
  to its newest half at 256 KB. Settings, **Activity log** shows the newest 200 lines (the
  connection log and the crash trail's notes, merged by time, refreshed every 3 seconds),
  **Copy log** puts everything kept on the clipboard (a text box to select from appears when
  the clipboard is not available), **Open folder** shows the files in Explorer.
- **What is logged**:
  - every request to a desktop that failed (no answer, refused, answer not trusted, status
    400 or more) and every one that took 1.5 seconds or more: the method, the path without
    its query, the status or the kind of failure, the time it took and whether it went over
    the home network (`local`) or Tailscale (`remote`);
  - a desktop becoming reachable or unreachable, and how long it had been the other;
  - a look at all desktops that took 1.5 seconds or more or was cut at its 10-second limit
    (a desktop that has not answered by then is shown as unreachable, "did not answer in
    time"), and each background bringing-over of a desktop's games and covers (20-second
    budget, one at a time per desktop, never part of that look);
  - the steps of starting a stream (the desktop answering, the saves synced before the
    stream, the desktop prepared, pairing checked, Moonlight started, with the time since
    the start), when Moonlight exited and after how long, and the steps of a session's end
    with the time each took;
  - a save sync that waited a long time for the one before it, or gave up.
- **Never logged**: addresses (IPv4 addresses are also replaced in the file), request or
  answer bodies, headers, signatures, keys, PINs, tokens, save file names or the handle of a
  save folder, or anything the player typed. As with reports, read before sending.
- **Reading it**: a stuck stream shows as the last `start_stream` step; a desktop that
  vanished shows as `failed (unreachable)` calls followed by `reachable -> unreachable`;
  a look cut at its limit shows as `hosts_refresh: ... did not answer`.

## Normal ends

Quit, Exit to Windows, an update restart, a remote Restart and the side menu's
Restart or Off all count as normal and write no report.

## Not covered yet

- Fetching a report from the desktop (`carla-companion ally ...`) needs a new
  request in the companion protocol, and is not built.
- Crash dumps of native faults (access violations) are not written; only the
  "did not end" report and the activity before it exist for those.
- Not tried on a real Ally. Tested: the report files, the marker logic, scrubbing
  and the real panic hook in a child process.
