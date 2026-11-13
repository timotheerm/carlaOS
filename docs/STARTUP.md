# Starting straight into CarlaOS

With this on, turning on the Ally shows CarlaOS, not the Windows desktop.

**Turn it on:** Settings, Starting up (administrator PIN), "Start CarlaOS when
the Ally turns on". From a desktop: `carla-companion ally startup on`.

For a start with no stops at all, Windows also has to sign in by itself. That
is a Windows setting of the account (no password prompt at start), set once
by the administrator. CarlaOS does not change how Windows signs in.

## What it changes, and how it is put back

| Change | Where | Turning it off |
| --- | --- | --- |
| CarlaOS starts at sign-in, for this account only | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `CarlaOS` | Removed |
| Windows no longer waits a few seconds before start-up programs | `HKCU\…\Explorer\Serialize`, `StartupDelayInMSec = 0`, only if nothing was set there | Removed, only if CarlaOS set it |

Nothing else. CarlaOS does **not** replace the Windows shell: Explorer, the
desktop and the taskbar are all still there underneath, which is what makes
the ways back below safe.

## Ways back to the Windows desktop

1. **Settings, Exit to Windows**, at any time.
2. **Hold View while the Ally starts** (the button with two squares, left of
   the middle). CarlaOS then does not open that one time.
3. **By itself.** If CarlaOS was started by Windows twice in a row and never
   got as far as showing the library (a broken update, a missing part), the
   third start stands down and leaves the desktop. The start after that tries
   again, and says what happened.
4. Turning the setting off, on the Ally or from a desktop.

A start by hand (the Start menu, the shortcut) is never held back by any of
this.

## Why not the Windows "full screen experience" home app

Windows 11 on handhelds can start into a "home app" chosen under Settings,
Gaming. Being offered in that list needs CarlaOS to be installed as a
packaged app (MSIX) signed with a certificate the Ally trusts, which is a
different way of building and installing than the current installer, and it
only exists on handhelds and recent Windows builds, not on the laptop that
stands in for the Ally today. Starting with Windows works everywhere now.
The home app remains the better end state on the real Ally (Windows then
skips loading parts of the desktop), and can be added next to this without
removing it.

## Tested

The guard (when to stand down, when not to) has unit tests. The page was
tried in the browser preview. The registry changes and reading the View
button at start are type-checked against Windows but **have not been run on
Windows yet**. To check on the laptop: turn it on, sign out and in, CarlaOS
should appear; hold View while signing in, it should not; turn it off, and
both registry values should be gone.
