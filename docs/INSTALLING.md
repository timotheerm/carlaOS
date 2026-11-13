# Installing games from the Steam library

## What it does

- The games of the Steam library that are not installed appear under
  **My Games, Not installed**, and in search. Nothing has to be set up and no
  account is asked for.
- A game's page offers **Install on this Ally** and, for each linked desktop
  that has the game in its library, **Install on HOME PC**. A game that
  already streams from a desktop can also be installed on the Ally from there.
- While Steam fetches it, the card and the page show how far it is, and Home
  has an **Installing now** row. When Steam has finished, the game becomes
  playable by itself.
- Updates Steam is fetching for installed games show the same way.

Steam does all the downloading, with the account that is signed in there.
CarlaOS never downloads a game itself, and Steam only installs what the
account owns.

## How the library is known

Steam keeps two files in `appcache`: `packageinfo.vdf` (the packages the
accounts on this machine have a licence for) and `appinfo.vdf` (what each app
is called, what kind it is, and which folder it installs into). CarlaOS reads
both; see `crates/core/src/library/owned.rs`. Only Windows games are listed.
The package every account has (free tools and servers) is left out.

The files exist once Steam has been opened and signed in on that machine.
`carla-companion status` on a desktop says how many games it found, or why
it found none.

## How an install is started

`crates/core/src/library/downloads.rs`.

- **Quietly**, when Steam's files name the game's folder: CarlaOS writes
  Steam's own record for the game (`steamapps/appmanifest_<id>.acf`) saying
  "update started", in the library folder with the most room, and has Steam
  start again in the background. Steam then fetches the game by itself. This
  is what makes installing on a desktop possible with nobody sitting at it.
- **By asking Steam**, otherwise: Steam is handed `steam://install/<id>` and
  shows its own install window. CarlaOS says so; on a desktop someone has to
  answer there.

Because Steam has to start again, an install is refused while a game is
running on that machine.

## How progress is known

Steam's record of each app has `StateFlags`, `BytesToDownload` and
`BytesDownloaded`. Steam writes it only now and then, so the files waiting in
`steamapps/downloading/<id>` are measured too, and the further of the two is
shown. The Ally looks every four seconds while something moves, every thirty
otherwise, and once a minute during a game.

A desktop answers three new signed requests: `GET /v1/library`,
`POST /v1/install` (only a game of its own library, by id) and
`GET /v1/downloads`. A desktop with an older companion offers nothing until
it is updated.

`downloads::steam` and `Download` (with `update` set for an installed game)
are meant to be shared with anything else that needs to know what Steam is
fetching.

## Not tried on real machines yet

All of this is tested on made-up Steam files, not on a real Steam install:

- reading `appinfo.vdf` and `packageinfo.vdf` (three and two known layouts);
- whether the current Steam client starts fetching from a record written
  this way. If it does not, the card stays on "Waiting for Steam"; deleting
  `appmanifest_<id>.acf` from `steamapps` takes it back;
- the library can include games of every account that has signed in on that
  machine, and family-shared ones.

Epic and GOG are not included yet: their launchers have no equivalent that
needs no sign-in.
