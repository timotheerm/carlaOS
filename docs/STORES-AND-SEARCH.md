# Other stores, and search

## Epic and GOG games appear by themselves

Next to Steam, CarlaOS now looks through two more stores, on the Ally and on
every linked desktop. Nothing needs setting up and no account is asked for.

| Store | How it is found | How it is started |
| --- | --- | --- |
| Steam | Steam's own files | Through Steam |
| Epic Games | The launcher's manifest files under `ProgramData` | Through the Epic Games Launcher, which must be signed in |
| GOG | The registry key each GOG game writes | The game's own program, directly |

Add-ons, half-finished installs and games whose folder or program has gone are
left out. This is the catalogue discovery layer, not the player's My Library:
only Steam games installed locally on the Ally are included automatically.
Epic and GOG games, like desktop games, must be selected through **My Library,
Add games** before they appear in the player's collection.

Not included: Game Pass and Microsoft Store games (they start differently and
come later), and achievements for Epic and GOG, which those stores keep only on
their servers. Such games still get the CarlaOS milestones. Epic and GOG games
have no cover picture yet; they show the coloured card with their name.

`carla-companion status` on a desktop now says how many games each store has.

## Search

Press **Y** on the controller (or `/` on a keyboard) anywhere in CarlaOS.
Letters are chosen with the controller; a keyboard or the Windows touch
keyboard types straight into the field. Search shows the games in My Library.
To find something outside it, open **My Library, Add games**, where the full
catalogue can be filtered by Ally, desktop or installation state. Every word
typed has to begin a word of the name or of the store; accents and punctuation
do not matter. Going into a game and back keeps what was typed.

## Not tried on real machines yet

The Epic and GOG scanners are tested on made-up manifest files and registry
entries. Starting an Epic game hands the launcher's own
`com.epicgames.launcher://` address to Windows; nothing else is ever opened
that way.
