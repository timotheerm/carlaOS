# Cloud games

GeForce NOW, Xbox Cloud Gaming, Boosteroid and Amazon Luna run games in a data
centre and play them in a web page, with the controller, full screen. CarlaOS
makes them feel like any other game on the Ally.

## What the player sees

- The **Cloud** tab shows the four services as tiles. Choosing one opens that
  service full screen, to browse it. Below them, the cloud games that were
  added have their own cards.
- A cloud game has a cloud badge, says "In the cloud" and opens a page that says
  "Play on Xbox Cloud Gaming" (or its service). Pressing Play opens the game's
  page full screen, with no browser bars.
- The quick menu ends it like any other game.
- Games, under the **Cloud** filter, lists only cloud games. **Streamed** no
  longer includes them.

## Adding one (administrator)

Manage games, **Add a cloud game**. Paste the game's link from its page on the
service, for example
`https://www.xbox.com/en-US/play/games/forza-horizon-5/9NKX70BBCDRN`. CarlaOS
says which service it is and, for Xbox, suggests the name. Saving goes straight
on to the artwork picker, so the card has a picture.

From the desktop: `carla-companion ally cloud LINK [NAME]`.

## Signing in, once

The first time a service is opened, it asks to sign in. The administrator does
that once, from the service's tile. The window uses a browser profile kept only
for cloud gaming (`cloud-browser` in CarlaOS's data folder), so the sign-in
stays and the player never sees it again.

## How it works

- Playing opens Microsoft Edge (it comes with Windows; Chrome is used if Edge is
  missing) with `--app=LINK --start-fullscreen` and that profile. CarlaOS
  watches the browser; when it closes, the game is over, and ending the game
  closes it.
- A link is checked twice, when it is added and again before it is opened: it
  must be `https`, on the service's own host, with no port, no user name, no
  spaces or quotes, and a path the service uses for games. Anything else is
  refused, so a link can never turn into a browser option or another program.
- CarlaOS buys nothing, signs in to nothing and sends nothing to the services.
  Saves are kept by the service, in the account there; save sync is skipped.

## Untested

- Nothing ran on Windows yet: whether Edge stays one process with its own
  profile (CarlaOS relies on it to know when a game ends), full screen on the
  Ally, and the controller on each service.
- The services could not be reached from the build machine.
