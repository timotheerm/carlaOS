# The desktop's dashboard

One pink page on each desktop that answers, in plain words: is the Ally
connected, how good is the connection, what is being streamed, and is this PC
ready. It is for whoever sits at the desktop; nothing on it needs explaining.

## Opening it

- **CarlaOS Dashboard** in the Start menu and on the desktop, or
  `carla-companion dashboard`. It starts the companion in the background when
  it is not running, and sets it up first when that was never done.
- It opens in a window of its own (Edge's app mode, which every Windows 11
  has) and in the usual browser otherwise. The address is
  `http://127.0.0.1:47600/`.
- It also opens after the set-up at the end of the installer, showing the
  same link code as the console window.

## What it shows

| Part | What it says |
| --- | --- |
| Headline | One sentence: streaming a game, an Ally connected, ready and waiting, ready for a first Ally, or what needs a hand. |
| Connection to the Ally | The delay in milliseconds, there and back, with a word for it (Excellent to Slow), a chart of the last minutes, how steady it is, the speed, and whether it is the home network or Tailscale. When it is not good, one line on what helps. |
| Stream | The game, the picture (size, pictures a second, megabits), how that was chosen, how long, to which Ally, and which screen is shown. "Close the game" closes it. |
| Linked Allies | Each one: streaming, connected or not, where it is, its CarlaOS version, whether Moonlight is paired. "Link an Ally" shows a code; "Forget" stops accepting one. |
| This PC | Sunshine, the home network, Tailscale, waking from sleep, the stream's screen, Moonlight pairing, processor and memory. When a screen of its own for each stream is switched on but its driver is missing, the check "Picture for the Ally" offers "Install the screen driver" (Windows asks you to allow it; it runs on a background thread, the page shows "Installing... allow it when Windows asks", and the check turns green when the driver is there). That is the only way the host installs it: never by itself. |
| Games | How many the Ally can stream from here and from where, what Steam is installing, room left, and "Look for games again". |
| Updates | A banner when a newer release exists, with "Update now". |

## Where the numbers come from

The Ally measures the connection (`carla_core::link::probe`, `health`) and
hands the result to the desktop with each pulse (`POST /v1/pulse`, a
`Pulse` with a `Health`). The desktop keeps the last measurement of each
Ally and this page shows it. A pulse sent under a running game carries no
measurement, so the last one is kept and the stream's picture comes from it.

The same data is what `carla-companion allies` prints, so the page and the
terminal agree. An Ally counts as connected when it was heard from in the
last 90 seconds (`CONNECTED_FOR`), or while it is being streamed to.

Each measurement is counted in the chart once, by the second it was made.

## Who may see it

The page is not part of the signed API, so it is guarded another way:

- only requests from this machine itself are answered;
- the `Host` they ask under must be `127.0.0.1` or `localhost`, so a web page
  elsewhere cannot reach it by pointing a name of its own here;
- everything but the page needs a token only the page holds, in a header no
  other site's page may send. The token is new each time the companion starts.

Everything shown is put on the page as text, never as markup.

## Log and console

The companion runs in the background with no window, so what it does is
written to `host.log` in its folder (`%LOCALAPPDATA%\CarlaOS\companion`, up to
256 KB; when it is full the oldest half is dropped). Each line starts with the
time to the millisecond, so "the call took 38 seconds" shows. It says:

- when the companion started (version, port) and when Sunshine is ready or not;
- every request of the Ally: method, route, status, how long it took and
  who asked, as the first six letters of the device id (`stranger` for an id
  that is not linked here, `unsigned` for none). A route that names a game or
  a save folder is written as its shape (`/v1/saves/:folder/write`). A request
  that takes a second or more is marked `SLOW`, and a refusal says why
  (403, 409, 503 and the rest, with the reason the Ally was given);
- changes only, never each pulse: an Ally heard from, not heard from for 90
  seconds (`CONNECTED_FOR`), a stream starting and ending, Moonlight being
  paired, a device linked or forgotten;
- what the Ally asked to start, the screen switch, the game started and
  closed (the lines of `launcher.log` are copied here, so one file tells the
  story), and a note when every request worker is busy.

Three ways to read it:

- **The dashboard**: the "Log" section shows the newest 100 lines, refreshes
  by itself and has a **Copy** button for pasting into a message.
- **The terminal**, the console of the companion: `carla-companion log`
  prints the newest 100 lines, `--lines N` changes that, `--launcher` shows
  `launcher.log` instead, and `carla-companion log --follow` keeps printing new
  lines every half second until you press Ctrl+C. It can run in a terminal
  next to the running companion. `carla-companion log path` prints where the
  file is.
- The file itself.

Never written: request bodies, headers or query strings, link keys, device
keys, tokens, signatures, PINs, the Sunshine password, the contents of a
save. The names a player typed (an Ally's name) are not written either, only
ids. IPv4 addresses and the Windows account name are hidden by the log itself.
The dashboard's own every-two-seconds requests are not logged unless they fail.

## Staying answerable

The API is served by eight worker threads, so one request that waits holds one
of them. To keep the Ally's status, games and pulse answered:

- a client that stops sending or reading mid-request is let go after 10
  seconds, a body that trickles in is given up on after 30, and the worker is
  free again (the `408` is in the log);
- at most four workers are on requests that can take long (saves, pairing,
  installs, a look for games, an Ally waiting for a request from the
  administrator); a fifth is answered `503` at once;
- a scan of the disks is shared by everyone asking at once, and the Ally's
  list of games is answered from the last scan while one runs; Sunshine's
  state is answered from the last answer while Sunshine is being asked.

## Not proven yet

- Nothing here has run on Windows or against a real Sunshine: the window
  opening through Edge, the Start menu entry, and the list of paired
  Moonlight devices (`/api/clients/list`, absent in older Sunshine releases,
  in which case that line is simply left out).
- The Ally's reports during a real Moonlight stream.
