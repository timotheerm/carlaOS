# Game artwork, shortcuts and ports

## Adding a game that is in no store

Settings, Manage games, Add a game. Choose the game's program **or a shortcut to it**.

- A shortcut (`.lnk`) is followed to the program behind it, with the options the shortcut passes. Windows cannot start a shortcut the way it starts a program, which is why a game added as a shortcut used not to start. Games added that way before are mended the next time CarlaOS looks through the library. The desktop companion does the same for `carla-companion add`.
- A link to an address (`.url`) is refused with a sentence saying so: CarlaOS could never tell when such a game has closed.
- A program that only starts the way a double click starts it (it asks for administrator rights, for example) is started that way when the plain way is refused.
- A well-known native port is recognised by its program's or folder's name (`crates/core/src/library/ports.rs`): it gets its real name, is filed as a native port, and remembers the console it first came out on. Hookshot and Ship of Harkinian are Ocarina of Time, for example. The list only holds names; nothing is downloaded.

"Find details online" now asks Wikipedia as well as Steam, so a game that never came out on Steam is found too. Each match says where it is from.

## Choosing pictures

Settings, Manage games, Game artwork lists every game, the ones without a picture first. Choosing a game looks for pictures and shows them to pick from: covers (the tall picture on the card) and wide pictures (the game's page and the top of Home). A newly added game goes straight to this page.

Where pictures come from, and what is sent there:

| Place | What it has | What is sent |
| --- | --- | --- |
| Steam | cover and wide pictures of games sold there | the name searched |
| Wikipedia | the box art on a game's page | the name searched |
| libretro box art | console games, by exact name | the game's name, or an emulated game's file name |
| SteamGridDB | thousands of covers made by players | the name searched and your key |

SteamGridDB is only asked when a key has been entered (it is free: steamgriddb.com, Preferences, API). The key is kept sealed on the Ally and never shown again.

Pictures are only ever fetched from those four places' own picture hosts, and only JPEG, PNG and WebP files are kept. A chosen picture wins over what a store or desktop offers and survives every look through the library; "Back to its own picture" gives it up. A picture from the Ally itself can be used too.

From a desktop that may administer the Ally:

```
carla-companion ally art ID [NAME]    look for pictures; they open, numbered, in the browser
carla-companion ally art use NUMBER   give the game that picture
carla-companion ally art clear ID     back to its own picture
carla-companion ally art key KEY      keep a SteamGridDB key on the Ally (none forgets it)
```

## The band on a game's picture

A game played through an emulator has a band along the bottom of its picture with its console's name, in that console's colour: red for Switch, indigo for GameCube, blue for PlayStation and so on (`apps/shell/src/ui/platform.ts`). A native port has the console it first came out on followed by "PC port"; homebrew says so. Games from a PC store have no band.

## Feel

- The whole screen takes on the colours of the game in focus, softly, and changes as focus moves.
- Starting a game shows its cover while it loads or while the desktop connects.
- Small sounds for moving, choosing, going back and starting a game. Settings, Sounds turns them off.

## Not checked on real hardware

The picture places could not be reached from where this was built, so reading their answers is tested against recorded examples of what they send, not against the live services.
