//! What CarlaOS knows about Windows: what must stay, what can go, and why.
//!
//! The rule is default-deny. Something can only be changed when it is listed
//! here as changeable, and anything matching a `KEEP_*` pattern is never
//! changeable even if another rule also matches it. The tests at the bottom
//! hold that line.
//!
//! Patterns compare case-insensitively and `*` stands for any run of characters.

use super::model::{RegValue, StartupLocation};

/// How confident CarlaOS is that a change is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// No downside on a handheld that boots into CarlaOS. Selected by default.
    Recommended,
    /// Helps, but costs something the administrator should weigh.
    Optional,
    /// Worth doing, but cannot be undone by CarlaOS, so it is left to a person.
    Manual,
    /// Needed by the hardware, Xbox, security or CarlaOS. Never changed.
    Kept,
}

pub fn matches(pattern: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some((b'*', rest)) => (0..=t.len()).any(|i| go(rest, &t[i..])),
            Some((c, rest)) => t
                .split_first()
                .is_some_and(|(d, more)| c.eq_ignore_ascii_case(d) && go(rest, more)),
        }
    }
    go(pattern.as_bytes(), text.as_bytes())
}

pub struct Keep {
    pub pattern: &'static str,
    pub why: &'static str,
}

const ASUS: &str = "ASUS hardware control: fans, power profiles, buttons and firmware updates.";
const XBOX: &str = "Xbox and Game Bar: sign-in, saves, the Xbox button and the full screen experience.";
const SECURITY: &str = "Windows security.";
const DRIVER: &str = "Part of a hardware driver.";
const STORE: &str = "The Microsoft Store and app installer, which CarlaOS uses to bring removed apps back.";
const WEBVIEW: &str = "Edge WebView, which draws CarlaOS itself.";
const INPUT: &str = "Controllers, touch, Bluetooth or audio.";
const UPDATE: &str = "Windows and firmware updates.";
const NETWORK: &str = "Networking and remote access.";
const GAMES: &str = "Game launchers' own services and anti-cheat.";
const CARLA: &str = "Part of CarlaOS or its streaming.";

/// Store packages that are never offered for removal.
pub const KEEP_APPS: &[Keep] = &[
    Keep { pattern: "B9ECED6F.*", why: ASUS },
    Keep { pattern: "*ASUS*", why: ASUS },
    Keep { pattern: "*ArmouryCrate*", why: ASUS },
    Keep { pattern: "*ROG*", why: ASUS },
    Keep { pattern: "Microsoft.Xbox*", why: XBOX },
    Keep { pattern: "Microsoft.GamingApp", why: XBOX },
    Keep { pattern: "Microsoft.GamingServices", why: XBOX },
    Keep { pattern: "Microsoft.Edge.GameAssist", why: XBOX },
    Keep { pattern: "*SecHealth*", why: SECURITY },
    Keep { pattern: "Microsoft.WindowsStore", why: STORE },
    Keep { pattern: "Microsoft.StorePurchaseApp", why: STORE },
    Keep { pattern: "Microsoft.DesktopAppInstaller", why: STORE },
    Keep { pattern: "Microsoft.MicrosoftEdge*", why: WEBVIEW },
    Keep { pattern: "*WebView*", why: WEBVIEW },
    Keep { pattern: "AdvancedMicroDevicesInc*", why: DRIVER },
    Keep { pattern: "RealtekSemiconductorCorp*", why: DRIVER },
    Keep { pattern: "DolbyLaboratories*", why: DRIVER },
    Keep { pattern: "*Tailscale*", why: NETWORK },
    Keep { pattern: "*CarlaOS*", why: CARLA },
];

/// Services that are never touched.
pub const KEEP_SERVICES: &[Keep] = &[
    Keep { pattern: "*ASUS*", why: ASUS },
    Keep { pattern: "*Armoury*", why: ASUS },
    Keep { pattern: "*ROG*", why: ASUS },
    Keep { pattern: "asComSvc", why: ASUS },
    Keep { pattern: "LightingService", why: ASUS },
    Keep { pattern: "Xbl*", why: XBOX },
    Keep { pattern: "Xbox*", why: XBOX },
    Keep { pattern: "GamingServices*", why: XBOX },
    Keep { pattern: "GameInput*", why: XBOX },
    Keep { pattern: "WinDefend", why: SECURITY },
    Keep { pattern: "WdNisSvc", why: SECURITY },
    Keep { pattern: "MDCoreSvc", why: SECURITY },
    Keep { pattern: "Sense", why: SECURITY },
    Keep { pattern: "SecurityHealthService", why: SECURITY },
    Keep { pattern: "wscsvc", why: SECURITY },
    Keep { pattern: "mpssvc", why: SECURITY },
    Keep { pattern: "BFE", why: SECURITY },
    Keep { pattern: "SgrmBroker", why: SECURITY },
    Keep { pattern: "webthreatdefsvc", why: SECURITY },
    Keep { pattern: "hidserv", why: INPUT },
    Keep { pattern: "bthserv", why: INPUT },
    Keep { pattern: "BTAGService", why: INPUT },
    Keep { pattern: "BthAvctpSvc", why: INPUT },
    Keep { pattern: "DeviceAssociationService", why: INPUT },
    Keep { pattern: "TabletInputService", why: INPUT },
    Keep { pattern: "TextInputManagementService", why: INPUT },
    Keep { pattern: "Audiosrv", why: INPUT },
    Keep { pattern: "AudioEndpointBuilder", why: INPUT },
    Keep { pattern: "wuauserv", why: UPDATE },
    Keep { pattern: "UsoSvc", why: UPDATE },
    Keep { pattern: "BITS", why: UPDATE },
    Keep { pattern: "DoSvc", why: UPDATE },
    Keep { pattern: "TrustedInstaller", why: UPDATE },
    Keep { pattern: "edgeupdate*", why: WEBVIEW },
    Keep { pattern: "AMD*", why: DRIVER },
    Keep { pattern: "Rtk*", why: DRIVER },
    Keep { pattern: "Dolby*", why: DRIVER },
    Keep { pattern: "Tailscale", why: NETWORK },
    Keep { pattern: "WlanSvc", why: NETWORK },
    Keep { pattern: "Dhcp", why: NETWORK },
    Keep { pattern: "Dnscache", why: NETWORK },
    Keep { pattern: "Steam Client Service", why: GAMES },
    Keep { pattern: "EasyAntiCheat*", why: GAMES },
    Keep { pattern: "BEService", why: GAMES },
    Keep { pattern: "vgc", why: GAMES },
    Keep { pattern: "Sunshine*", why: CARLA },
    Keep { pattern: "*CarlaOS*", why: CARLA },
];

/// Programs that start with Windows and must keep doing so. Checked against
/// both the entry's name and its command line, so the patterns avoid short
/// words that also occur in ordinary paths ("Program Files" contains "rog").
pub const KEEP_STARTUP: &[Keep] = &[
    Keep { pattern: "*ASUS*", why: ASUS },
    Keep { pattern: "*Armoury*", why: ASUS },
    Keep { pattern: "ROG*", why: ASUS },
    Keep { pattern: "*\\ROG*", why: ASUS },
    Keep { pattern: "*Xbox*", why: XBOX },
    Keep { pattern: "*GamingServices*", why: XBOX },
    Keep { pattern: "*SecurityHealth*", why: SECURITY },
    Keep { pattern: "*Windows Defender*", why: SECURITY },
    Keep { pattern: "AMD*", why: DRIVER },
    Keep { pattern: "*\\AMD\\*", why: DRIVER },
    Keep { pattern: "*Radeon*", why: DRIVER },
    Keep { pattern: "Rtk*", why: DRIVER },
    Keep { pattern: "*\\Rtk*", why: DRIVER },
    Keep { pattern: "*Realtek*", why: DRIVER },
    Keep { pattern: "*Dolby*", why: DRIVER },
    Keep { pattern: "*Tailscale*", why: NETWORK },
    Keep { pattern: "*Sunshine*", why: CARLA },
    Keep { pattern: "*Moonlight*", why: CARLA },
    Keep { pattern: "*CarlaOS*", why: CARLA },
];

pub fn kept<'a>(rules: &'a [Keep], texts: &[&str]) -> Option<&'a Keep> {
    rules.iter().find(|k| texts.iter().any(|t| matches(k.pattern, t)))
}

/// A Store app that can be removed. `store_id` is the Microsoft Store product
/// it is reinstalled from on undo; without one the app is listed for a person
/// to uninstall, because CarlaOS could not promise to bring it back.
pub struct App {
    pub pattern: &'static str,
    pub title: &'static str,
    pub tier: Tier,
    pub why: &'static str,
    pub store_id: Option<&'static str>,
}

const UNUSED: &str = "Not used on a handheld that boots into CarlaOS.";
const PROMO: &str = "Preinstalled promotion.";

const fn app(pattern: &'static str, title: &'static str, store_id: &'static str) -> App {
    App { pattern, title, tier: Tier::Recommended, why: UNUSED, store_id: Some(store_id) }
}

pub const APPS: &[App] = &[
    app("Microsoft.BingNews", "Microsoft News", "9WZDNCRFHVFW"),
    app("Microsoft.BingWeather", "Weather", "9WZDNCRFJ3Q2"),
    app("Microsoft.MicrosoftSolitaireCollection", "Solitaire Collection", "9WZDNCRFHWD2"),
    app("Microsoft.MicrosoftOfficeHub", "Microsoft 365 app", "9WZDNCRD29V9"),
    app("Microsoft.OutlookForWindows", "Outlook", "9NRX63209R7B"),
    app("microsoft.windowscommunicationsapps", "Mail and Calendar", "9WZDNCRFHVQM"),
    app("Microsoft.Todos", "Microsoft To Do", "9NBLGGH5R558"),
    app("Clipchamp.Clipchamp", "Clipchamp video editor", "9P1J8S7CCWWT"),
    app("Microsoft.Copilot", "Copilot", "9NHT9RB2F4HD"),
    app("Microsoft.Windows.DevHome", "Dev Home", "9N8MHTPHNGVV"),
    app("Microsoft.GetHelp", "Get Help", "9PKDZBMV1H3T"),
    app("Microsoft.Getstarted", "Tips", "9WZDNCRDTBJJ"),
    app("Microsoft.WindowsFeedbackHub", "Feedback Hub", "9NBLGGH4R32N"),
    app("Microsoft.WindowsMaps", "Maps", "9WZDNCRDTBVB"),
    app("Microsoft.YourPhone", "Phone Link", "9NMPJ99VJBWV"),
    app("Microsoft.MicrosoftStickyNotes", "Sticky Notes", "9NBLGGH4QGHW"),
    app("Microsoft.WindowsSoundRecorder", "Sound Recorder", "9WZDNCRFHWKN"),
    app("Microsoft.WindowsAlarms", "Clock", "9WZDNCRFJ3PR"),
    app("Microsoft.549981C3F5F10", "Cortana", "9NFFX4SZZ23L"),
    app("Microsoft.Whiteboard", "Whiteboard", "9MSPC6MP8FM4"),
    app("MicrosoftCorporationII.MicrosoftFamily", "Family Safety", "9PDJDJS743XF"),
    App {
        pattern: "Microsoft.WindowsCamera",
        title: "Camera",
        tier: Tier::Recommended,
        why: "The Ally has no camera.",
        store_id: Some("9WZDNCRFJBBG"),
    },
    App {
        pattern: "7EE7776C.LinkedInforWindows",
        title: "LinkedIn",
        tier: Tier::Recommended,
        why: PROMO,
        store_id: Some("9WZDNCRFJ4Q7"),
    },
    App {
        pattern: "Microsoft.ZuneMusic",
        title: "Media Player",
        tier: Tier::Optional,
        why: "Plays music and video files. Remove it if nobody will open media files from the desktop.",
        store_id: Some("9WZDNCRFJ3PT"),
    },
    App {
        pattern: "Microsoft.ZuneVideo",
        title: "Films & TV",
        tier: Tier::Optional,
        why: "Plays video files. Remove it if nobody will open them from the desktop.",
        store_id: Some("9WZDNCRFJ3P2"),
    },
    App {
        pattern: "MicrosoftCorporationII.QuickAssist",
        title: "Quick Assist",
        tier: Tier::Optional,
        why: "Microsoft's remote help tool. Remove it unless you use it to help with this Ally from afar.",
        store_id: Some("9P7BP5VNWKX5"),
    },
    App {
        pattern: "SpotifyAB.SpotifyMusic",
        title: "Spotify",
        tier: Tier::Optional,
        why: "Often preinstalled. Remove it unless someone listens to Spotify on the Ally.",
        store_id: Some("9NCBCSZSJRSB"),
    },
    // Known clutter with no Store product CarlaOS could reinstall it from.
    App { pattern: "MSTeams", title: "Microsoft Teams", tier: Tier::Manual, why: UNUSED, store_id: None },
    App { pattern: "MicrosoftTeams", title: "Microsoft Teams", tier: Tier::Manual, why: UNUSED, store_id: None },
    App { pattern: "Microsoft.People", title: "People", tier: Tier::Manual, why: UNUSED, store_id: None },
    App { pattern: "Microsoft.PowerAutomateDesktop", title: "Power Automate", tier: Tier::Manual, why: UNUSED, store_id: None },
    App { pattern: "Microsoft.BingSearch", title: "Bing Search", tier: Tier::Manual, why: UNUSED, store_id: None },
    App { pattern: "king.com.*", title: "Candy Crush and friends", tier: Tier::Manual, why: PROMO, store_id: None },
    App { pattern: "*McAfee*", title: "McAfee", tier: Tier::Manual, why: "Trial antivirus. Windows Security takes over by itself once it is uninstalled.", store_id: None },
];

/// A background service that can be switched off.
pub struct ServiceRule {
    pub name: &'static str,
    pub title: &'static str,
    pub tier: Tier,
    pub why: &'static str,
}

pub const SERVICES: &[ServiceRule] = &[
    ServiceRule {
        name: "DiagTrack",
        title: "Usage data collection",
        tier: Tier::Recommended,
        why: "Collects and uploads usage data in the background. Nothing on the Ally needs it.",
    },
    ServiceRule {
        name: "dmwappushservice",
        title: "Usage data routing",
        tier: Tier::Recommended,
        why: "Carries the same usage data. Nothing on the Ally needs it.",
    },
    ServiceRule {
        name: "MapsBroker",
        title: "Offline maps updater",
        tier: Tier::Recommended,
        why: "Downloads map updates for the Maps app.",
    },
    ServiceRule {
        name: "RetailDemo",
        title: "Shop demo mode",
        tier: Tier::Recommended,
        why: "Only used on display units in shops.",
    },
    ServiceRule {
        name: "Fax",
        title: "Fax",
        tier: Tier::Recommended,
        why: "Sends and receives faxes.",
    },
    ServiceRule {
        name: "WMPNetworkSvc",
        title: "Media Player network sharing",
        tier: Tier::Recommended,
        why: "Shares a music library with other devices on the network.",
    },
    ServiceRule {
        name: "SysMain",
        title: "App preloading (SysMain)",
        tier: Tier::Optional,
        why: "Preloads often-used programs into memory. Off frees memory and disk activity for games; desktop programs may open a little slower.",
    },
    ServiceRule {
        name: "WSearch",
        title: "File search indexing",
        tier: Tier::Optional,
        why: "Indexes files in the background. Off saves processor and disk work; searching for files in Windows gets slower. CarlaOS does not use it.",
    },
    ServiceRule {
        name: "Spooler",
        title: "Printing",
        tier: Tier::Optional,
        why: "Turn off if nobody will ever print from the Ally.",
    },
    ServiceRule {
        name: "WerSvc",
        title: "Crash report upload",
        tier: Tier::Optional,
        why: "Sends crash reports to Microsoft. Off means less work after a game crashes, and fewer details if you need to investigate one.",
    },
];

/// Where startup entries live, and where Windows records whether each is
/// allowed to run. Writing that record is what Task Manager's "Disable" does,
/// so the entry itself is never deleted.
pub struct StartupSource {
    pub location: StartupLocation,
    /// Registry key listing the entries, if this source is a registry key.
    pub run: Option<&'static str>,
    /// .NET special folder name, if this source is a folder of shortcuts.
    pub folder: Option<&'static str>,
    pub approved: &'static str,
}

pub const STARTUP_SOURCES: &[StartupSource] = &[
    StartupSource {
        location: StartupLocation::HkcuRun,
        run: Some("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
        folder: None,
        approved: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run",
    },
    StartupSource {
        location: StartupLocation::HklmRun,
        run: Some("HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run"),
        folder: None,
        approved: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run",
    },
    StartupSource {
        location: StartupLocation::HklmRun32,
        run: Some("HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run"),
        folder: None,
        approved: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run32",
    },
    StartupSource {
        location: StartupLocation::UserFolder,
        run: None,
        folder: Some("Startup"),
        approved: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\StartupFolder",
    },
    StartupSource {
        location: StartupLocation::CommonFolder,
        run: None,
        folder: Some("CommonStartup"),
        approved: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\StartupFolder",
    },
];

pub fn startup_source(location: StartupLocation) -> &'static StartupSource {
    STARTUP_SOURCES
        .iter()
        .find(|s| s.location == location)
        .expect("every location has a source")
}

/// A recognised program that starts with Windows. Checked against the entry's
/// name and its command line.
pub struct StartupRule {
    pub pattern: &'static str,
    pub title: &'static str,
    pub tier: Tier,
    pub why: &'static str,
}

const LAUNCHER: &str = "CarlaOS starts it when a game needs it. Off means a faster start-up and more free memory; its games update only while it is open.";

pub const STARTUP: &[StartupRule] = &[
    StartupRule {
        pattern: "*MicrosoftEdgeAutoLaunch*",
        title: "Edge at start-up",
        tier: Tier::Recommended,
        why: "Opens Edge in the background every time Windows starts.",
    },
    StartupRule {
        pattern: "*Teams*",
        title: "Teams at start-up",
        tier: Tier::Recommended,
        why: "Opens Teams in the background every time Windows starts.",
    },
    StartupRule {
        pattern: "*OneDrive*",
        title: "OneDrive at start-up",
        tier: Tier::Recommended,
        why: "Keeps OneDrive syncing in the background. Off means files in OneDrive folders stop syncing until OneDrive is opened by hand.",
    },
    StartupRule {
        pattern: "*Copilot*",
        title: "Copilot at start-up",
        tier: Tier::Recommended,
        why: "Opens Copilot in the background every time Windows starts.",
    },
    StartupRule { pattern: "*Steam*", title: "Steam at start-up", tier: Tier::Optional, why: LAUNCHER },
    StartupRule { pattern: "*EpicGamesLauncher*", title: "Epic Games at start-up", tier: Tier::Optional, why: LAUNCHER },
    StartupRule { pattern: "*EADM*", title: "EA app at start-up", tier: Tier::Optional, why: LAUNCHER },
    StartupRule { pattern: "*EADesktop*", title: "EA app at start-up", tier: Tier::Optional, why: LAUNCHER },
    StartupRule { pattern: "*Ubisoft*", title: "Ubisoft Connect at start-up", tier: Tier::Optional, why: LAUNCHER },
    StartupRule { pattern: "*GalaxyClient*", title: "GOG Galaxy at start-up", tier: Tier::Optional, why: LAUNCHER },
    StartupRule { pattern: "*Battle.net*", title: "Battle.net at start-up", tier: Tier::Optional, why: LAUNCHER },
    StartupRule {
        pattern: "*Discord*",
        title: "Discord at start-up",
        tier: Tier::Optional,
        why: "Off means a faster start-up and more free memory. Discord still works when opened by hand.",
    },
    StartupRule {
        pattern: "*Spotify*",
        title: "Spotify at start-up",
        tier: Tier::Optional,
        why: "Off means a faster start-up and more free memory.",
    },
];

/// A Windows setting, made of one or more registry values.
pub struct Setting {
    pub id: &'static str,
    pub title: &'static str,
    pub tier: Tier,
    pub why: &'static str,
    pub values: &'static [SettingRule],
}

pub struct SettingRule {
    pub path: &'static str,
    pub name: &'static str,
    pub dword: u32,
}

impl SettingRule {
    pub fn target(&self) -> RegValue {
        RegValue::Dword(self.dword)
    }
}

const CDM: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager";

pub const SETTINGS: &[Setting] = &[
    Setting {
        id: "suggested-apps",
        title: "Stop Windows installing suggested apps",
        tier: Tier::Recommended,
        why: "Windows can add promoted apps and games by itself. This keeps removed clutter from coming back.",
        values: &[
            SettingRule { path: CDM, name: "SilentInstalledAppsEnabled", dword: 0 },
            SettingRule { path: CDM, name: "PreInstalledAppsEnabled", dword: 0 },
            SettingRule { path: CDM, name: "OemPreInstalledAppsEnabled", dword: 0 },
        ],
    },
    Setting {
        id: "setup-nags",
        title: "No \"finish setting up\" screens or tips",
        tier: Tier::Recommended,
        why: "Stops the full-screen reminders that can appear before CarlaOS after an update, and tips pop-ups.",
        values: &[
            SettingRule {
                path: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\UserProfileEngagement",
                name: "ScoobeSystemSettingEnabled",
                dword: 0,
            },
            SettingRule { path: CDM, name: "SoftLandingEnabled", dword: 0 },
            SettingRule { path: CDM, name: "SubscribedContent-338389Enabled", dword: 0 },
            SettingRule { path: CDM, name: "SubscribedContent-310093Enabled", dword: 0 },
        ],
    },
    Setting {
        id: "widgets",
        title: "Turn off Widgets",
        tier: Tier::Recommended,
        why: "The news and weather panel keeps several background processes running.",
        values: &[SettingRule {
            path: "HKLM\\SOFTWARE\\Policies\\Microsoft\\Dsh",
            name: "AllowNewsAndInterests",
            dword: 0,
        }],
    },
    Setting {
        id: "edge-background",
        title: "Stop Edge running in the background",
        tier: Tier::Recommended,
        why: "Edge preloads itself at start-up and stays running after it is closed. Edge and CarlaOS keep working normally.",
        values: &[
            SettingRule { path: "HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge", name: "StartupBoostEnabled", dword: 0 },
            SettingRule { path: "HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge", name: "BackgroundModeEnabled", dword: 0 },
        ],
    },
    Setting {
        id: "usage-data",
        title: "Send only required diagnostic data",
        tier: Tier::Recommended,
        why: "Less data collected and uploaded in the background. Windows Settings will say this is managed by a policy.",
        values: &[SettingRule {
            path: "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection",
            name: "AllowTelemetry",
            dword: 0,
        }],
    },
    Setting {
        id: "web-search",
        title: "No web results in Windows search",
        tier: Tier::Recommended,
        why: "Stops the Start menu search from calling the web on every keystroke.",
        values: &[SettingRule {
            path: "HKCU\\Software\\Policies\\Microsoft\\Windows\\Explorer",
            name: "DisableSearchBoxSuggestions",
            dword: 1,
        }],
    },
    Setting {
        id: "transparency",
        title: "Turn off window transparency",
        tier: Tier::Optional,
        why: "Saves a little graphics work on the Windows desktop. No effect inside games or CarlaOS.",
        values: &[SettingRule {
            path: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
            name: "EnableTransparency",
            dword: 0,
        }],
    },
    Setting {
        id: "update-sharing",
        title: "Stop sharing updates with other PCs",
        tier: Tier::Optional,
        why: "Windows uploads updates it has downloaded to other computers. Off saves bandwidth and background work; downloads come only from Microsoft.",
        values: &[SettingRule {
            path: "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DeliveryOptimization",
            name: "DODownloadMode",
            dword: 0,
        }],
    },
];

/// Classic programs worth uninstalling by hand. CarlaOS cannot reinstall
/// these, so it only points them out.
pub struct ProgramRule {
    pub pattern: &'static str,
    pub why: &'static str,
}

pub const PROGRAMS: &[ProgramRule] = &[
    ProgramRule { pattern: "*McAfee*", why: "Trial antivirus. Windows Security takes over by itself once it is uninstalled." },
    ProgramRule { pattern: "*WebAdvisor*", why: "Browser add-on that comes with McAfee." },
    ProgramRule { pattern: "*Norton*", why: "Trial antivirus. Windows Security takes over by itself once it is uninstalled." },
    ProgramRule { pattern: "*ExpressVPN*", why: PROMO },
    ProgramRule { pattern: "Microsoft 365*", why: "Preinstalled Office trial." },
    ProgramRule { pattern: "Microsoft OneNote*", why: "Preinstalled with the Office trial." },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_match_case_insensitively_with_wildcards() {
        assert!(matches("Microsoft.Xbox*", "microsoft.xboxgamingoverlay"));
        assert!(matches("*ASUS*", "AsusOptimization"));
        assert!(matches("*", ""));
        assert!(matches("a*b*c", "aXXbYYc"));
        assert!(!matches("Microsoft.Xbox*", "Microsoft.BingNews"));
        assert!(!matches("Fax", "FaxPlus"));
    }

    #[test]
    fn nothing_changeable_is_also_protected() {
        for a in APPS.iter().filter(|a| a.tier < Tier::Manual) {
            // A rule's own pattern stands in for the names it can match.
            assert!(kept(KEEP_APPS, &[a.pattern]).is_none(), "{} is protected", a.pattern);
        }
        for s in SERVICES {
            assert!(kept(KEEP_SERVICES, &[s.name]).is_none(), "{} is protected", s.name);
        }
    }

    #[test]
    fn the_essentials_are_protected() {
        for name in [
            "B9ECED6F.ArmouryCrateSE",
            "B9ECED6F.ASUSPCAssistant",
            "Microsoft.XboxGamingOverlay",
            "Microsoft.GamingApp",
            "Microsoft.GamingServices",
            "Microsoft.XboxIdentityProvider",
            "Microsoft.SecHealthUI",
            "Microsoft.WindowsStore",
            "Microsoft.DesktopAppInstaller",
            "Microsoft.MicrosoftEdge.Stable",
            "Microsoft.Win32WebViewHost",
            "AdvancedMicroDevicesInc-2.AMDRadeonSoftware",
            "RealtekSemiconductorCorp.RealtekAudioControl",
        ] {
            assert!(kept(KEEP_APPS, &[name]).is_some(), "{name} is not protected");
        }
        for name in [
            "ArmouryCrateService",
            "ArmouryCrateControlInterface",
            "AsusAppService",
            "ASUSOptimization",
            "ASUSSystemAnalysis",
            "AsusCertService",
            "ROG Live Service",
            "XblAuthManager",
            "XblGameSave",
            "XboxGipSvc",
            "XboxNetApiSvc",
            "GamingServices",
            "GamingServicesNet",
            "GameInputSvc",
            "WinDefend",
            "SecurityHealthService",
            "wscsvc",
            "mpssvc",
            "hidserv",
            "bthserv",
            "wuauserv",
            "AMD External Events Utility",
            "Tailscale",
        ] {
            assert!(kept(KEEP_SERVICES, &[name]).is_some(), "{name} is not protected");
        }
    }

    #[test]
    fn every_removable_app_can_be_reinstalled() {
        for a in APPS {
            match a.tier {
                Tier::Recommended | Tier::Optional => {
                    let id = a.store_id.unwrap_or_else(|| panic!("{} has no Store product", a.pattern));
                    assert!(
                        id.len() >= 12 && id.chars().all(|c| c.is_ascii_alphanumeric()),
                        "{} has an odd Store product id",
                        a.pattern
                    );
                    assert!(!a.pattern.contains('*'), "{} must name one package exactly", a.pattern);
                }
                Tier::Manual => assert!(a.store_id.is_none()),
                Tier::Kept => panic!("{} belongs in KEEP_APPS", a.pattern),
            }
        }
    }

    #[test]
    fn settings_have_unique_ids_and_known_hives() {
        let mut ids: Vec<_> = SETTINGS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SETTINGS.len());
        for v in SETTINGS.iter().flat_map(|s| s.values) {
            assert!(v.path.starts_with("HKCU\\") || v.path.starts_with("HKLM\\"), "{}", v.path);
        }
    }
}
