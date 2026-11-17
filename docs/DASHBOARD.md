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
| This PC | Sunshine, the home network, Tailscale, waking from sleep, the stream's screen, Moonlight pairing, processor and memory. |
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

## Not proven yet

- Nothing here has run on Windows or against a real Sunshine: the window
  opening through Edge, the Start menu entry, and the list of paired
  Moonlight devices (`/api/clients/list`, absent in older Sunshine releases,
  in which case that line is simply left out).
- The Ally's reports during a real Moonlight stream.
