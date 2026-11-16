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

Only the Ally can time the connection: it asks, and times the answer. So it
tells the desktop, signed like every other request:

```
POST /v1/report
{ "version": "0.10.0", "latency_ms": 4, "jitter_ms": 1, "kbps": 310000,
  "away": false, "streaming": true,
  "picture": { "width": 1920, "height": 1080, "fps": 120, "kbps": 40000 },
  "level": "smooth", "automatic": true }
```

It does so when it looks at its desktops, and every five seconds during a
stream (three empty questions and one short answer, which is nothing beside
the stream). Every field after the first three is optional, and a desktop
ignores fields it does not know, so more measurements can be added without
breaking an older desktop. `carla_core::link::AllyReport` is the type and
`Link::report` sends it. A desktop from before this answers "not known" and
the Ally stops telling it.

An Ally counts as connected when it was heard from in the last 90 seconds.

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
