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
  player can see, with no game or stream running, stays silent for 30 seconds, a
  "Stopped responding" report is written once (and "responding again" goes into the
  notes if it recovers). Hidden or minimised windows (the page reports when it is hidden), running
  games and streams, and the time around the Ally sleeping are not judged, because
  a hidden page answers rarely.
- **Recent activity** (`breadcrumbs.log`, kept to about 100 KB): a session starting,
  running and ending, a stream cut off and picked up again, a desktop stopping or
  starting to answer, and errors the interface caught. Only plain words and ids: no
  keys, PINs, tokens or things typed by the player.

IPv4 addresses and the Windows account name in paths are replaced (`x.x.x.x`,
`<user>`) in every report. This is best effort: read a report before sending it.

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
