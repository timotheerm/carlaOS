# Cleaning up Windows

Settings → **Clean up Windows** scans the Ally for apps, start-up programs,
background services and Windows settings that cost performance, and lets the
administrator switch them off. It sits behind the administrator PIN.

## Rules

1. **Default-deny.** Only things listed in
   [`crates/core/src/debloat/catalogue.rs`](../crates/core/src/debloat/catalogue.rs)
   can be changed. Anything else is left alone, or pointed out for a person to
   deal with.
2. **Protected parts are never changed**, even if a rule would match them:
   ASUS and Armoury Crate, Xbox and Game Bar, Windows security, hardware
   drivers, controller, touch, Bluetooth and audio services, Windows Update, the
   Microsoft Store, Edge WebView (it draws CarlaOS), Tailscale, anti-cheat.
   They are shown under "Kept on purpose" with the reason.
3. **Every change can be undone.** Just before changing something, the helper
   reads what was there. The opposite operation goes into
   `cleanup/journal.json` in CarlaOS's data folder. If the previous state cannot
   be restored exactly, the change is not made.
4. **Nothing irreversible is automated.** Classic programs such as a trial
   antivirus cannot be reinstalled by CarlaOS, so they are listed "for you to do
   by hand" and never uninstalled automatically.

## What a change is

| Kind | What CarlaOS does | Undo |
| --- | --- | --- |
| App | Removes a Store app for the current user. | Re-registers the copy Windows kept, otherwise reinstalls it from the Microsoft Store. |
| Starts with Windows | Marks the entry as disabled, the way Task Manager does. The entry itself stays. | Puts the previous marker back. |
| Background service | Sets the start type to Disabled and stops it. | Restores the previous start type and running state. |
| Windows setting | Writes one or more registry values. | Restores each previous value, or removes the value (and the key, if CarlaOS created it). |

Items are sorted into **Recommended** (selected by default, no downside on a
handheld that boots into CarlaOS) and **Your call** (each line says what it
costs). Unrecognised start-up programs appear under "Your call" and are never
selected by default.

## Approval

Per-user changes (apps, the user's own start-up entries and settings) need no
approval. Machine-wide ones (services, policies) run in a second, elevated
helper, so Windows asks for administrator approval once per batch. Saying no
leaves those changes out and keeps the rest.

## Deliberately not done

- Memory integrity, virtualisation-based security and Defender are left on.
  Turning them off can raise frame rates, but they are Windows security.
- Game Bar, Game Mode and capture settings are Xbox components and stay as
  they are.
- Power plans, fan curves and TDP stay with Armoury Crate.

## The lock

The administrator PIN is four digits, stored salted and hashed in CarlaOS's
data folder. It keeps the player out; it is not a security boundary. Windows'
own approval prompt still guards everything machine-wide. The pages lock again
as soon as they are left.

## Adding to the catalogue

Add a rule to `catalogue.rs`. A removable app needs its Microsoft Store product
id so undo can reinstall it; without one it can only be listed for a person.
The tests in that file fail if a changeable rule overlaps a protected pattern.
