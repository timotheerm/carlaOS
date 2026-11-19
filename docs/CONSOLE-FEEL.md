# Badges, the pre-game page, achievements and saves

## Fewer tabs, badges instead

The sections are Home, My Library, My Desktops, Cloud and Settings. My
Library contains the games selected for this Ally: local Steam installs are
included automatically, while desktop games and other catalogue entries are
added by choice. Add games never installs anything by itself. Each card says
two things with small badges:

- where the game is from (Steam, Standalone, Switch, Native port and so on);
- **Offline**: it is installed on the Ally and plays without a desktop;
  **Stream**: it plays from a linked desktop and needs a connection to it.
  A game that is both shows Offline and a small stream mark.

"Offline" means CarlaOS needs no desktop for it. A game that itself insists on
being online still does.

Cloud is only for rented cloud gaming services.

My Library is a cover-led collection, rather than the full catalogue. Its filters
show how many games are in each view, and the collection can be sorted by last
played, name, date added or total playtime. The grid adapts to the available
screen width while keeping the artwork large enough to scan from the sofa.

**Add games** keeps the controller keyboard available but collapsed until it is
needed. A physical or touch keyboard can be used directly in the search field.
The filter counts update with the search, and adding a game returns focus to the
next result instead of sending the player back to the top of the page.

## The page of a game, before it starts

Opening a game shows, before anything starts: time played, when it was last
played, achievements earned out of how many, and when it was last saved. Below
are the achievements one by one, the CarlaOS milestones, and the saves.

### Achievements

- **Steam games** show their real achievements. CarlaOS reads the files Steam
  itself keeps on the machine (`appcache/stats`); nothing is asked of Steam's
  servers and no account details are needed. For a game that is only on a
  desktop, the desktop's companion reads them and the Ally remembers the last
  answer for when the desktop is asleep. Steam writes those files when the game
  has been started at least once.
- **Every game**, including ones added by hand, has CarlaOS milestones worked
  out from the play sessions CarlaOS recorded: first play, 1 to 100 hours,
  playing on 3, 7 and 30 different days, and three hours in one sitting.

CarlaOS never reads achievements out of a game's memory or protected files.

### Saves

For Steam games CarlaOS finds the save folder Steam uses for cloud saves. For
any other game the administrator says where the saves are: Settings, Manage
games, Game saves.

- After each play session on the Ally, if the saves changed, CarlaOS keeps a
  safe copy. The newest five are kept. A folder over 512 MB is not copied.
- Anyone can make a safe copy from the game's page. It only ever adds one.
- Putting a copy back is for the administrator, needs a second press, cannot
  be done while a game runs, and first makes a safe copy of the saves as they
  are, so it can be undone.

Safe copies are in CarlaOS's own data folder, under `saves`.

## Details for games added by hand

In "Add a game", "Find details online" looks the name up in Steam's public
store listing and fills in the description and a cover picture. Only public
store information is fetched; nothing is downloaded about or from the game
itself. A picture chosen by hand replaces the fetched one.

## Startup animation

A heart draws itself and the name appears, for under three seconds. Any key,
button or touch skips it. With "reduce motion" switched on in Windows it is a
still picture for a moment.
