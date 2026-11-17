# RoséOS Phase 1: Feasibility and Architecture

Nov 11, 2026 · @Timothée

## Summary

**Update, 11 Nov 2026:** the architecture is approved and the project is now called CarlaOS, with code at [timotheerm/carlaOS](https://github.com/timotheerm/carlaOS). The priorities are a player who only turns the Ally on and picks a game, an administrator who sets everything up beforehand or remotely, and the leanest possible Ally. This adds an admin lock on the Ally and remote administration over Tailscale to M4. The rest of this document still says RoséOS.

RoséOS is feasible on the RC73YA, and the recommendation is a custom frontend that plugs into Windows 11's own full screen experience, with Sunshine, Moonlight and Tailscale doing the streaming underneath.

- **Shell:** a custom app (Tauri 2, Rust core, React UI) registered as the Windows full screen experience home app. Windows then boots into RoséOS, skips the desktop shell, and keeps the Xbox button, Game Bar and Armoury Crate working.
- **Streaming:** stock Sunshine on each desktop, stock Moonlight on the Ally driven invisibly through its command line, Tailscale between locations, and a small RoséOS companion service on each desktop.
- **Library:** our own SQLite catalogue and store scanners instead of Playnite as a backend. Playnite 11 is a private rewrite with no date, so building on Playnite 10 internals would be building on something about to change.
- **Biggest risks:** AMD driver crashes with Sunshine on the RX 9070 XT (two open bugs since June 2026), remote wake of a sleeping PC, and the weak Z2 A CPU for local emulation.
- **Switch 2 emulation:** real but days old, desktop only, and unstable. It can only ever be a stream-from-desktop option, off by default.

### Decisions needed from you

- [ ] Approve the architecture in section 2 (custom Tauri frontend, Rust companion, stock Sunshine and Moonlight).
- [ ] Create an empty private GitHub repository (suggested name `roseos`) and tell me its name. I cannot create repositories myself.
- [ ] Confirm the MVP scope in section 5, which pulls a thin slice of desktop streaming forward into the first milestone.
- [ ] Tell me the second PC's GPU vendor (AMD, NVIDIA or Intel) and whether each desktop is on Ethernet.

## 1. Technical feasibility

Everything in the brief can be built on Windows 11 except live game migration between hosts and reliable remote wake without extra hardware. The table grades each area.

| Area | Verdict | What the research found |
| --- | --- | --- |
| Boot into RoséOS, hide the desktop | Feasible | Windows 11 lets a home app replace the Xbox app in the full screen experience. [AnyFSE](https://codeberg.org/ashpynov/AnyFSE) (MIT) and [OmniConsole](https://8bit2qubit.github.io/omniconsole-site/docs/faq) already register third-party home apps this way. Microsoft's [handheld launcher guidance](https://devdocs.xbox.com/build/gdk-and-engines/handheld/handheld-launchers) describes the behaviour: launch at power on, return to the home app after a game closes, taskbar and some startup apps suppressed. |
| Pink controller-first UI on a 7 inch 1080p screen | Feasible | A WebView2 UI gives full control over type, motion and artwork. Controller input is read natively in Rust so it works regardless of web focus. |
| Unified local library | Feasible | Installed games from Steam, Epic, GOG, Xbox, EA, Ubisoft, Battle.net and itch.io can be read from local manifests and registry keys with no account credentials. EA and Battle.net formats are undocumented and need maintenance. |
| Multi-desktop streaming | Feasible, with one hardware risk | Sunshine exposes a [REST API](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2api.html) for apps, pairing PINs and clients. Moonlight has a command line for pair, list, stream and quit. Together they allow zero-touch pairing and launching. The risk is the RX 9070 XT driver, see section 8. |
| Remote access between locations | Feasible | Tailscale gives each machine a stable private address with no public ports. |
| Wake a sleeping remote PC | Partly | Wake-on-LAN is a local broadcast and does not cross a VPN. It needs an always-on device at each location, or the PC must stay awake. |
| Local emulation on the Ally | Partly | The Z2 A is [four Zen 2 cores with RDNA 2 graphics at 6 to 20 W](https://www.pcgamesn.com/amd/ryzen-z2-a-cpu), close to a Steam Deck. Retro systems, GameCube, Wii and PS2 are realistic. Wii U and Switch are title dependent. Heavy titles should stream from the HOME PC instead. |
| Switch 1 emulation | Feasible | Eden, Ryubing and Citron Neo are alive and self-hosted after Nintendo's February 2026 GitHub takedowns, per a [3 Nov 2026 status check](https://aliteq.com/switch-emulators-survived-nintendo-2026-dmca-purge). |
| Switch 2 emulation | Experimental, desktop only | Working forks appeared in the last two weeks. See section 10. |
| Cloud gaming tiles | Feasible | GeForce NOW and Boosteroid have native Windows apps, and Xbox Cloud Gaming runs in the Xbox app or Edge. RoséOS launches them, it does not embed them. |
| ASUS power, fan and firmware features | Keep as is | Armoury Crate SE stays installed and reachable from its hardware button. ASUS has no public API, so RoséOS links to it at first. |

Windows 11 stays. SteamOS or Bazzite would drop Game Pass, most anti-cheat titles and Armoury Crate, and the full screen experience already removes most of the desktop overhead.

## 2. Recommended architecture

Two programs share one Rust core: the RoséOS shell on the Ally and the RoséOS companion on each desktop. Both run the same library scanners, so a desktop reports its games in exactly the format the Ally uses for its own.

&#91;embedded content: system layout · Ally, two desktops, two channels each\]

Control requests go from the Ally to the companion, and the picture comes back from Sunshine to a hidden Moonlight. Both channels use the same private address, local at home and Tailscale elsewhere.

### Components

| Component | Runs on | Built with | Job |
| --- | --- | --- | --- |
| RoséOS shell | Ally | Tauri 2, React, TypeScript | The interface. Registered as the full screen experience home app. |
| Core | Ally and desktops | Rust library | Catalogue database, store scanners, emulator and cloud plugins, launch and process tracking. |
| Input service | Ally | Rust, XInput first, GameInput later | Reads the built-in controller and sends navigation events to the UI. |
| Stream controller | Ally | Rust | Drives Moonlight's command line, runs pairing, picks a profile, watches the session. |
| Companion | Each desktop | Rust Windows service plus a small tray app | Status, hardware info, catalogue, game launch, sleep and shutdown, Sunshine setup. |
| Waker (optional) | Any always-on device per location | Rust, single binary | Sends Wake-on-LAN packets on request and remembers the PC's last state. |

### Why this stack

- **Tauri and React for the UI.** The design goal is a premium, animated, artwork-led interface, and web rendering is the fastest route to that level of polish. The UI can also be built and screenshot-tested before it ever touches the handheld. The cost is WebView2's memory, roughly 150 to 250 MB, which is acceptable on 16 GB once the desktop shell is suppressed. The shell unloads its views while a game or stream runs.
- **Rust for the core and companion.** One language for both ends means one protocol definition and one set of scanners. A Rust service idles at a few MB of RAM and near zero CPU, which meets the brief that the companion must not affect gaming performance.
- **Not C# and WinUI.** It would be a fine choice for a Windows-only app and gives easy access to Playnite's plugin ecosystem, but those plugins depend on a Playnite SDK that version 11 will replace. It also makes the custom visual design slower to reach.
- **SQLite** for the catalogue on every machine, with JSON over HTTPS between them.

### Plugin boundaries

Each of these is a small interface in the core, with one implementation per provider, so providers can be added or removed without touching the UI.

- **Library source:** scan installed games, return entries, build a launch command. One per storefront, plus manual executables and scanned folders.
- **Emulator:** executable, argument template, file extensions, ROM folders. Defined in a data file, so adding an emulator needs no code.
- **Cloud service:** name, artwork, launch target (app, protocol link or URL).
- **Stream host:** today Sunshine. The interface leaves room for Apollo or another fork.

### Catalogue database

| Table | Holds |
| --- | --- |
| `games` | One row per title: canonical id, name, description, artwork ids, favourite flag, section (game, emulated, native port, homebrew). |
| `installs` | One row per place a game exists: game id, location (this Ally or a host id), storefront and store id, install path, executable, arguments, emulator id, ROM path. |
| `hosts` | Each desktop: name, addresses (LAN, Tailscale), MAC, hardware, pairing keys reference, last seen, last state. |
| `play_sessions` | Start, end, location, used for playtime, recently played and continue playing. |
| `artwork` | Cached covers, heroes and logos with their source. |
| `saves` | Known save paths per install, for later save awareness. |
| `settings` | Profiles, appearance, plugin configuration. |

The split between `games` and `installs` is what produces "Play locally", "Stream from HOME PC" and "Stream from SECOND PC" on one card. Installs are matched to a game by storefront id first, then by normalised title, with a manual merge for the rest.

### Companion API

The API is versioned JSON over HTTPS on one port, bound only to the LAN and Tailscale interfaces. The Ally is enrolled once with a short code shown on the desktop. After that every request is signed with a per-device key, and the desktop's certificate is pinned on the Ally.

| Endpoint | Purpose |
| --- | --- |
| `GET /v1/status` | State, user session, Sunshine health, current stream, GPU and driver version. |
| `GET /v1/hardware` | CPU, GPU, RAM, displays, encoders available. |
| `GET /v1/catalogue?since=` | Installed games, incremental. |
| `GET /v1/artwork/{id}` | Cover art the desktop already has. |
| `POST /v1/sessions` | Prepare a game for the next stream and return when ready. |
| `DELETE /v1/sessions/current` | End the game and the stream cleanly. |
| `POST /v1/pairing` | Accept a Moonlight pairing PIN and pass it to Sunshine. |
| `POST /v1/power` | Sleep, shut down or restart, refused while someone is using the PC unless forced. |
| `GET` and `PUT /v1/config` | Streaming profiles and companion settings. |
| `POST /v1/update` | Fetch and apply a signed companion update. |

Secrets live in Windows Credential Manager on both ends. The Sunshine admin password is generated by the companion and never leaves the desktop.

## 3. Existing applications versus custom development

A fully custom frontend wins because the defining feature, browsing and launching games on two remote desktops as if they were local, does not exist in any launcher and cannot be added cleanly to Playnite.

| Option | Strengths | Weaknesses | Verdict |
| --- | --- | --- | --- |
| Playnite with a custom theme | Mature library with plugins for every store. Fullscreen mode with controller support. Least work. Latest stable is [10.63 from 19 Oct 2026](https://protagonistlabs.app/shelf/playnite-linux/). | The theme controls looks, not structure. No concept of a game installed on another machine, no host cards, no pairing flow. Fullscreen mode is XAML-themed and hard to make feel bespoke. | Good fallback, not the product. |
| Playnite as backend, custom frontend | Reuses store plugins and metadata. | Playnite has no headless mode or stable external API, so we would read its database files or write a bridge plugin. Version 11 is a rewrite in a private repository with no beta date, and its SDK will change. | Rejected: most fragile of the three. |
| Independent custom frontend | Full control of design, navigation and the multi-host catalogue. Same scanners run on the Ally and the desktops. | We write and maintain our own store scanners and metadata fetching. | Recommended. |

Reuse still does most of the heavy lifting. RoséOS writes no video, VPN, emulation or cloud code.

| Need | Reused | Custom |
| --- | --- | --- |
| Video streaming | Sunshine, Moonlight | Orchestration only |
| Remote networking | Tailscale | Address selection and health checks |
| Boot-to-shell | Windows full screen experience | Home app registration |
| Emulation | RetroArch, Dolphin, PCSX2, Cemu, Eden, Ryubing | Launch templates and ROM scanning |
| Artwork and metadata | SteamGridDB, IGDB, Steam's own art | Matching and cache |
| Hardware controls | Armoury Crate SE | Links at first |
| Library, UI, multi-host catalogue, companion | Nothing suitable exists | All custom |

An optional one-time importer from an existing Playnite library is cheap to add later if you already use Playnite.

## 4. Required software and dependencies

Nothing here costs money except optional cloud subscriptions. Tailscale's free personal plan covers three machines.

### On the Ally

| Software | Role | Notes |
| --- | --- | --- |
| Windows 11 25H2 with full screen experience | Base | Already on the device. |
| RoséOS shell | Interface | Ours. |
| WebView2 runtime | UI rendering | Ships with Windows 11. |
| Moonlight PC 6.x | Stream client | [moonlight-qt](https://github.com/moonlight-stream/moonlight-qt/releases), latest listed is 6.2.0. Launched hidden by RoséOS. |
| Tailscale | Remote link | Runs as a service. |
| Armoury Crate SE and ASUS services | Power, fans, firmware, button mapping | Kept untouched. |
| Store clients you use | Local games | Steam, Xbox app, Epic and so on. RoséOS starts games through them. |
| Emulators you choose | Emulation | Installed by you, registered in RoséOS. |
| GeForce NOW, Boosteroid apps | Cloud | Optional. |

### On each desktop

| Software | Role | Notes |
| --- | --- | --- |
| RoséOS companion | Service and tray app | Ours. Installs and configures the rest. |
| Sunshine | Stream host | Stock [LizardByte build](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2api.html). Actively released, including a November 2026 build. |
| Tailscale | Remote link | Access rules limit the streaming and companion ports to the Ally. |
| GPU driver | Hardware encoding | AMD AMF on the HOME PC. Version pinned by the companion, see section 8. |
| Virtual display driver | Stream at 1080p without touching the monitors | Optional, added in Phase 4 if needed. |

### Development

| Tool | Use |
| --- | --- |
| Rust stable, Tauri 2, Node, React, TypeScript, Vite | Build the shell and companion. |
| SQLite through `rusqlite` | Catalogue. |
| `windows` crate | XInput, GameInput, services, power events, package queries. |
| GitHub Actions Windows runners | Build signed installers on every change, so you only download and test. |
| SteamGridDB API key, optional IGDB key | Artwork and descriptions. Your own free keys, stored in Credential Manager. |

## 5. Development roadmap

The one change to your phase order is that a thin slice of desktop streaming moves into the MVP, because it is the defining feature and carries the most technical risk. Every milestone ends with a build you install and test on the Ally.

| Milestone | Delivers | You test on the Ally | Priority |
| --- | --- | --- | --- |
| M0 Foundations | Repository, build pipeline, installer that produces a runnable empty shell. | The app installs, opens fullscreen and closes. | Must |
| M1 UI prototype | Pink design system, light and dark themes, home screen, game cards, controller and touch navigation, sample data. | Readability at 7 inches, navigation feel, frame rate, memory use. | Must |
| M2 Local library | SQLite catalogue, Steam scanner, manual executable import with artwork and arguments, launch and return to shell, playtime. | Launch a Steam game and a standalone game, quit, land back in RoséOS. | Must |
| M3 Streaming slice (MVP complete) | Companion with status, hardware and catalogue. HOME PC card, zero-touch pairing, one-tap stream through hidden Moonlight on the home network. | Pick HOME PC, pick a game, play, quit, return. | Must |
| M4 Full desktops | Second PC, Tailscale, state detection, profiles, wake, sleep and shutdown, unified card with play and stream choices. | Stream from both PCs at home and away. Switch hosts. | Must |
| M5 Full library | Epic, GOG, Xbox and Game Pass, EA, Ubisoft, Battle.net, itch.io, folder scanning, SteamGridDB artwork, favourites and search. | Your real library looks right and launches. | Should |
| M6 Emulation and native ports | Emulator plugin format, RetroArch, Dolphin, PCSX2, Cemu, a Switch 1 emulator, ROM folder scanning, Native Ports section. | Launch a game per emulator, locally or streamed. | Should |
| M7 Console behaviour | Home app registration, boot to RoséOS, reversible Windows tuning with a restore point, settings pages. | Cold boot to RoséOS, sleep and resume, undo every tweak. | Must |
| M8 Extras | Cloud tiles, performance profiles, save awareness, experimental Switch 2 entry. | Each feature on its own. | Could |
| M9 Finalisation | Signed installer, auto-update for shell and companion, crash recovery, security review, rollback. | A clean install from scratch on the Ally and one desktop. | Must |

M0 to M3 is the MVP: a pink shell that launches local games and streams from the HOME PC with one tap.

## 6. Difficulty and time

The MVP is about three to four weeks and the full project about three to four months, assuming I write the code and you test a new build on the Ally a few times a week. Your testing time on real hardware sets the pace more than the coding does.

| Milestone | Difficulty | Estimate | Main uncertainty |
| --- | --- | --- | --- |
| M0 Foundations | Low | 2 to 3 days | Windows build pipeline. |
| M1 UI prototype | Medium | 1 to 2 weeks | Tuning focus movement and animation on the real screen. |
| M2 Local library | Medium | 1 week | Detecting when a game launched through a store client has really exited. |
| M3 Streaming slice | High | 1 to 2 weeks | AMD driver stability, hiding Moonlight's own window. |
| M4 Full desktops | High | 2 to 3 weeks | Remote network conditions, wake behaviour, state detection. |
| M5 Full library | Medium | 2 to 3 weeks | EA and Battle.net formats, Game Pass titles. |
| M6 Emulation and native ports | Medium | 1 to 2 weeks | Per-emulator arguments and controller profiles. |
| M7 Console behaviour | High | 1 to 2 weeks | Home app registration is thinly documented and must be verified on your device. |
| M8 Extras | Low to medium | 2 weeks | ASUS has no public API for power profiles. |
| M9 Finalisation | Medium | 2 weeks | Code signing and update safety. |

These are estimates from comparable projects, not measurements. The ranges on M3, M4 and M7 could double if the driver or registration problems prove stubborn.

## 7. Interface design and screen structure

The interface is seven sections on one horizontal rail, switched with the bumpers, with artwork doing the talking and text kept to titles and one status line. A clickable prototype in M1 is where the look gets decided; this section fixes the structure.

### Design tokens (starting point)

| Token | Light theme | Dark theme |
| --- | --- | --- |
| Background | Cream `#FBF5F3` | Deep plum `#1E1220` |
| Surface (cards, panels) | White `#FFFFFF` | Plum `#2C1B2F` |
| Primary accent | Dusty rose `#D98BA0` | Rose `#F2A7BB` |
| Soft accent | Pastel pink `#F6D5DD` | Mauve `#5A3550` |
| Text | Charcoal plum `#3A2A33` | Cream `#F8ECEF` |
| Secondary text | Soft grey `#8C7F86` | Grey mauve `#B9A6B3` |
| Status | Sage for online, amber for waking, grey for offline | Same hues, lifted |

- **Type:** one rounded sans family such as Nunito or Manrope, bundled with the app. Minimum 22 px for body and 28 px for titles at 1080p, which is about the smallest comfortable size on a 7 inch panel.
- **Shape:** 20 px card corners, 2:3 portrait covers in the library, wide hero art on Home.
- **Motion:** 150 to 200 ms eased transitions. The focused card scales to 1.06 with a rose glow. Nothing loops or bounces.
- **Targets:** every touch target at least 64 px high, with 16 px between cards.

### Controller map

| Input | Action |
| --- | --- |
| Left stick or D-pad | Move focus |
| A | Select or play |
| B | Back |
| X | Favourite |
| Y | Search |
| LB and RB | Previous and next section |
| Menu | Options for the focused item |
| View | Quick panel: battery, Wi-Fi, volume, brightness, active desktop |
| Xbox button | Left to Windows and Game Bar |

Touch does everything the controller does: tap to select, swipe to scroll, long press for options.

### Screens

| Section | Layout | Contents |
| --- | --- | --- |
| Home | Hero banner over horizontal rows | Continue playing as the hero. Rows for recently played, favourites and recently added. Two desktop tiles with live status. Status strip with clock, battery, Wi-Fi and VPN. |
| My Games | Cover grid, five across | Every title once. Filter chips for source, installed here, and streamable. A small badge marks local, remote or both. |
| My Desktops | Two large cards, then that desktop's games | Each card shows the state, hardware, latency, quality, game count and last connection. Selecting a card opens its game grid directly. |
| Emulation | Console tiles, then a cover grid per console | Switch and Switch 2 are separate consoles. Switch 2 carries an Experimental label. |
| Native Ports | Cover grid with a type badge | Official PC release, community port, or homebrew. Emulated games never appear here. |
| Cloud | Three large service tiles | GeForce NOW, Xbox Cloud Gaming, Boosteroid. Labelled as rented cloud, visually distinct from My Desktops. |
| Settings | Left list, right panel | Appearance, controller, performance, network, storage, emulators, desktops, updates, Windows and ASUS shortcuts. |

### Shared views

- **Game detail:** hero art, title, one-paragraph description, playtime and platform. One primary button per place the game exists: Play, Stream from HOME PC, Stream from SECOND PC. Unavailable places are hidden, never greyed with a guess.
- **Add game:** pick an executable with the built-in file browser, pick or search artwork, set arguments, choose a section.
- **Stream overlay:** a brief card while connecting that names the host, the profile and each step. On failure it says which step failed and offers one fix.
- **First run:** theme, library folders, add a desktop by entering the code its companion shows.

## 8. Multiple desktop streaming

RoséOS never shows Sunshine or Moonlight: the companion tells the Ally what each desktop has, the Ally asks the companion to prepare a game, and Moonlight is started hidden to carry the picture.

### One tap, step by step

1. **Enrol once.** The companion's tray app shows a six-digit code. You enter it on the Ally. The two exchange keys, and the companion completes Moonlight pairing by passing the PIN to Sunshine's API. You never see a Sunshine page.
2. **Stay in sync.** The Ally polls each desktop's status every few seconds while My Desktops is open, and pulls catalogue changes when they exist. Cards update live.
3. **Select a game.** The Ally sends `POST /v1/sessions` with the game id. The companion checks the PC is free, wakes the display or virtual display, and holds the game ready.
4. **Stream.** The Ally starts `Moonlight stream <host> "RoséOS"` with the chosen profile. Sunshine has a single app called RoséOS, whose command is the companion's launcher. It starts the prepared game and stays alive until the game exits.
5. **Finish.** When the game exits, the stream ends by itself and RoséOS returns to the front. Quitting from the Ally sends `DELETE /v1/sessions/current`, which closes the game and the stream.

Using one Sunshine app keeps hundreds of games out of Sunshine's own list and means any new game on a desktop is streamable the moment the companion sees it.

### Finding and reaching hosts

- **At home:** the companion announces itself on the local network with mDNS, so a new desktop appears as a card to enrol.
- **Away:** each host also has a Tailscale name. The Ally tries the LAN address first and falls back to Tailscale, then labels the card Local or Remote.
- **No open ports:** nothing is forwarded on either router. Tailscale access rules allow only the Ally to reach the streaming ports and the companion port.
- **Direct paths matter:** Tailscale normally connects peer to peer. If it falls back to a relay, latency rises sharply, so the card shows Relayed and RoséOS picks the low bandwidth profile.

### Host states

| State shown | How it is decided | What the card offers |
| --- | --- | --- |
| Ready | Companion answers, Sunshine healthy, nobody mid-game. | Browse and play. |
| Busy | Companion answers, a game or a local user session is active. | Browse. Play asks before taking over. |
| Online, not ready | Companion answers, but Sunshine is down, no user is signed in, or the GPU encoder failed its check. | The reason, and a Repair action that restarts Sunshine. |
| Asleep | No answer, and the companion's last message was a sleep notice. | Wake. |
| Off | No answer, and the last message was a shutdown notice. | Wake, with a note that it may not work from full shutdown. |
| Unreachable | No answer and no notice, or the Ally itself has no route. | Retry, plus what was checked. |

Asleep and Off are inferred from the last notice, because a PC that does not answer cannot say why. The card says "last seen" with a time so the inference is visible.

### Wake-on-LAN

- **At home** the Ally broadcasts the magic packet itself. This needs the desktop on Ethernet, with wake enabled in the BIOS and the network adapter, and Windows Fast Startup turned off.
- **Away** it cannot work directly. The magic packet is a local broadcast, and a sleeping PC has dropped off Tailscale. The options, best first:
  1. A small always-on device at that location running the RoséOS waker over Tailscale, such as a Raspberry Pi, a NAS or a capable router.
  2. Keep the desktop awake with the display off. Simple and reliable, at the cost of idle power.
  3. A smart plug with the BIOS set to power on when mains returns. It only works from fully off and risks an unclean shutdown, so it is a last resort.
- Desktops on Wi-Fi generally cannot be woken at all.

### Switching hosts

Selecting the other desktop while a stream is running ends the current session first: the game is closed on the first host, Moonlight exits, then the new stream starts. A running game is never moved between computers, and RoséOS warns before closing one with unsaved progress.

### Quality profiles

Moonlight and Sunshine use a fixed bitrate for the length of a session, so true mid-stream adaptation is not available. RoséOS measures latency and throughput to the companion before each stream and picks the profile, and offers to step down if the session reports heavy frame loss.

| Profile | Resolution and rate | Codec | Bitrate | Chosen when |
| --- | --- | --- | --- | --- |
| Local | 1080p at 60, optional 120 | HEVC | 30 to 50 Mbps | Same network, 5 or 6 GHz Wi-Fi. |
| High quality | 1080p at 120 | HEVC, AV1 if the Ally decodes it well | 60 to 80 Mbps | Same network, strong signal, chosen by you. |
| Remote | 1080p at 60 | HEVC | 10 to 20 Mbps | Direct Tailscale path with enough upload at the host. |
| Low bandwidth | 720p at 60 | HEVC | 4 to 8 Mbps | Relayed path, mobile hotspot or weak upload. |

These numbers are starting values to be tuned on your networks. Each desktop has its own copy, so the second PC can use a different encoder and limits. Whether the Z2 A decodes AV1 in hardware smoothly at 120 fps is to be measured on the device; HEVC is the safe default.

### The RX 9070 XT risk

Two open Sunshine bug reports describe AMD driver crashes on the RX 9070 XT. [Issue 5385](https://github.com/LizardByte/Sunshine/issues/5385) reports HEVC and AV1 encoder start-up failing on Adrenalin 26.6.4 and taking the display driver down until reboot, with 25.9.1 unaffected. [Issue 5321](https://github.com/LizardByte/Sunshine/issues/5321) reports driver crashes mid-stream on 26.6.2. Neither has a confirmed fix.

The plan is to test your current driver in M3 before building further, have the companion run an encoder self-test at start and report the driver version, fall back to H.264 if HEVC fails, and record a known-good driver version in settings. If stock Sunshine stays unstable, the stream host interface allows trying a fork without changing the Ally.

## 9. Hardware upgrade recommendation for the RC73YA

The only internal upgrade worth doing is the SSD, and for a streaming-first setup it is optional: spend on the network first.

| Upgrade | Recommendation | Evidence and notes |
| --- | --- | --- |
| SSD | Optional. Move to a 1 TB or 2 TB M.2 2280 NVMe drive if local installs outgrow 512 GB. | [iFixit's guide](https://www.ifixit.com/Guide/Xbox+ROG+Ally+SSD+Upgrade/203813) rates the swap easy, about 30 minutes, and lists any M.2 2280 SSD. [Kingston's guide](https://www.kingston.com/en/blog/gaming/upgrade-your-rog-xbox-ally-ssd) covers reinstalling Windows through ASUS Cloud Recovery in the BIOS. Drain the battery to 25% or lower first and take care with the battery connector. |
| SSD choice | A power-efficient PCIe 4.0 drive, single-sided where possible. | The handheld cannot use the speed of a flagship drive, and cooler drives suit the chassis. I found no official maximum capacity or thickness limit, so check clearance before buying a double-sided 4 TB drive. |
| microSD | Good home for ROMs, emulators and small games. | Avoids opening the device at all. |
| RAM | Not upgradable. | 16 GB LPDDR5X is soldered. The full screen experience and a lean shell are how memory gets freed. |
| Network at home | Highest value for this project. | Put both desktops on Ethernet. Use a Wi-Fi 6E access point near where you play so the Ally gets a clean 6 GHz link for 120 fps streams. |
| Always-on waker | Recommended for the second location. | A Raspberry Pi class device makes remote wake reliable, see section 8. |
| Dummy display plug or virtual display | Only if a desktop's monitor is off or absent while streaming. | Decided in M4 after testing. |

Opening the device may affect warranty terms in some regions, so check ASUS's policy for yours before the SSD swap.

## 10. Known limitations

These are the things RoséOS will not do, or can only do partly, stated now so nothing in the interface promises them.

### Cannot be delivered

- **Moving a running game between desktops.** A host switch ends one session and starts another.
- **Waking a remote PC with no always-on device at that location.**
- **Mid-stream automatic bitrate adaptation.** The protocol fixes the bitrate per session. RoséOS chooses before the stream and can restart at a lower profile.
- **Replacing Windows entirely.** The lock screen, Windows Update, driver prompts, launcher logins and some anti-cheat dialogs will still appear at times and may need touch input.
- **Bypassing DRM or anti-cheat, or supplying games, ROMs, firmware or keys.** RoséOS only launches what you installed and dumped yourself. It will not download, bundle or generate console keys or firmware.
- **Switch 2 emulation on the Ally itself.** The current forks need a desktop GPU with 8 GB of video memory and 16 GB of RAM.

### Partly deliverable

- **Switch 2 emulation.** As of early November 2026 the working options are Eden-based forks named Eglantine and Lichii. One fork was created and archived within three days in late October, and several repositories share the same names, according to [one tracking page](https://heldgames.com/guides/switch-2-emulators) that has corrected itself several times. A few commercial games boot. Nintendo firmware 23.5.0 on 9 Nov 2026 closed the method used to dump games. RoséOS will offer a Switch 2 console entry that is off by default, labelled Experimental, stream-from-desktop only, and pointed at an emulator you install. I would not build anything further on it until a project has survived a few months.
- **Switch 1 emulation on the Ally.** Light titles should run. Demanding ones belong on the HOME PC, streamed.
- **Storefront coverage.** Steam, Epic, GOG and Xbox are well understood. EA, Ubisoft and Battle.net rely on undocumented files that change, so those scanners will break occasionally. Games that are owned but not installed will not appear at first, since that needs account sign-in per store.
- **Playtime.** Accurate for games RoséOS launches. Not imported from stores at first.
- **Save-game integration.** Showing known save locations is realistic. Syncing saves between machines is not planned.
- **ASUS power and fan control inside RoséOS.** ASUS publishes no API. RoséOS links to Armoury Crate first. Direct control is possible later through the same unofficial interface open-source tools use, with the risk that a firmware update breaks it.
- **Cloud services.** RoséOS opens each service's own app. Per-game shortcuts into GeForce NOW are possible later. Sign-in and their interfaces stay theirs.
- **Boot-to-shell registration.** Third-party home apps work today, but Microsoft documents the feature mainly for OEMs, so a Windows update could change it. The fallback is launching RoséOS at sign-in.
- **120 fps streaming.** Depends on the Ally's decoder and your Wi-Fi. 60 fps is the commitment.

### Windows tuning rules

Every change is recorded with its previous value and undone from Settings or by an uninstaller, with a restore point taken first. ASUS services, drivers, Windows security, controller services and Xbox components are never removed.

## Sources

Pages opened on 11 Nov 2026. Emulator status came from secondary tracking sites, not the projects themselves, and should be rechecked before M6.

- [Xbox handheld launcher guidance](https://devdocs.xbox.com/build/gdk-and-engines/handheld/handheld-launchers), Microsoft
- [How to replace the Xbox full screen experience](https://www.windowscentral.com/hardware/handheld-gaming-pc/how-to-stop-xbox-full-screen-experience-or-replace-it-on-windows-handhelds), Windows Central
- [AnyFSE](https://codeberg.org/ashpynov/AnyFSE) and [OmniConsole FAQ](https://8bit2qubit.github.io/omniconsole-site/docs/faq)
- [Sunshine API documentation](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2api.html), LizardByte
- [Sunshine issue 5385](https://github.com/LizardByte/Sunshine/issues/5385) and [issue 5321](https://github.com/LizardByte/Sunshine/issues/5321), RX 9070 XT driver crashes
- [Apollo vs Sunshine in 2026](https://tech-insider.org/apollo-vs-sunshine-game-streaming-2026/), Tech Insider
- [Moonlight PC releases](https://github.com/moonlight-stream/moonlight-qt/releases)
- [Playnite status](https://protagonistlabs.app/shelf/playnite-linux/), Protagonist Labs
- [Ryzen Z2 A](https://www.pcgamesn.com/amd/ryzen-z2-a-cpu), PCGamesN
- [Xbox ROG Ally SSD upgrade](https://www.ifixit.com/Guide/Xbox+ROG+Ally+SSD+Upgrade/203813), iFixit, and [Kingston's upgrade guide](https://www.kingston.com/en/blog/gaming/upgrade-your-rog-xbox-ally-ssd)
- [Which Switch emulators still work](https://aliteq.com/switch-emulators-survived-nintendo-2026-dmca-purge), Aliteq, checked 3 Nov 2026
- [Switch 2 emulators in 2026](https://heldgames.com/guides/switch-2-emulators), Heldgames, updated 10 Nov 2026
- [Boosteroid ROG Ally support](https://rog.asus.com/articles/news/boosteroid-adds-official-rog-ally-series-support-for-optimized-handheld-cloud-gaming/), ASUS, 21 Jan 2026

Not from a page opened today: Moonlight's command-line verbs, store manifest locations and Tailscale's relay behaviour are from my own knowledge and will be verified in M2 to M4.
