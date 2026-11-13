# Administering the Ally from your own computer

The Ally's player never has to set anything up. The administrator can look
after the Ally from a desktop it is linked to: see how it is doing, add and
remove games, set save folders, run the Windows clean-up, update CarlaOS.

## Turning it on (once, on the Ally)

Settings, Desktops, the desktop, **Administer this Ally from …**. It needs the
administrator PIN and is off for every desktop until it is turned on. Linking
a desktop again turns it off again.

## Using it (on the desktop)

```
carla-companion ally                  how the Ally is doing
carla-companion ally games            every game in its library, with ids
carla-companion ally rescan           look for installed games again
carla-companion ally add "NAME" "C:\path\on\the\ally\game.exe" [--args "…"] [--kind game|port|homebrew] [--about "…"]
carla-companion ally remove ID        take a game added by hand off its library
carla-companion ally saves ID "C:\path\on\the\ally\saves"
carla-companion ally saves ID --forget
carla-companion ally desktops         the desktops the Ally is linked to
carla-companion ally cleanup          what the Windows clean-up found there
carla-companion ally cleanup apply ID [ID ...]
carla-companion ally cleanup undo ID [ID ...]
carla-companion ally update           install the newest CarlaOS on the Ally
carla-companion ally restart          close CarlaOS on the Ally and start it again
carla-companion ally startup [on|off] whether CarlaOS opens when the Ally is turned on
```

With more than one Ally linked, name it first:
`carla-companion ally --device ID games` (ids from `carla-companion devices`).

Paths are paths **on the Ally**. A program or folder that does not exist there
is refused. A command normally answers within a second or two.

## How it works, and why it is safe

- **The Ally asks; nobody connects to the Ally.** The Ally already talks to
  its desktops. For an allowed desktop it also asks "anything for me?", and
  the desktop keeps that question open for twenty seconds, so a command typed
  in the meantime arrives at once. Nothing is opened in the Ally's firewall,
  and it works at home and through Tailscale alike.
- **Only the linked desktop can give commands.** The command travels in the
  desktop's signed reply. The Ally checks the signature with the key made
  when the two were linked, bound to the question it just asked, so a command
  cannot be forged or replayed by someone else on the network.
- **Only an allowed desktop is ever asked.** A desktop that was not allowed
  on the Ally has no way to hand it anything.
- **Only a fixed list of things can be asked**, the ones above. There is no
  "run this program". The clean-up keeps its own rules: what is protected
  stays protected, and everything it changes can be undone.
- **On the desktop**, a command can be given by whoever can use the Windows
  account the companion runs under, which is also who holds the link's key.
- The Ally keeps a short record of what was asked in `remote.log` in its data
  folder.

## Limits

- The Ally must be on with CarlaOS running, and able to reach the desktop.
  A sleeping Ally does not answer; the command says so after a minute.
- Clean-up items marked "Windows asks for approval" still show the Windows
  approval prompt on the Ally's own screen. That prompt cannot be answered
  from afar, by design of Windows.
- Update and restart are refused while a game is running on the Ally.
- Covers cannot be sent to the Ally yet; a game added from afar gets its
  picture on the Ally, under Manage games.
- Tested between an Ally stand-in and a companion over the real protocol on
  one machine. Not yet run between two Windows machines.
