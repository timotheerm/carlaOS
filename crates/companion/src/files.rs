//! What the companion keeps on disk, all in one folder of the Windows account
//! it runs under. Keys and the Sunshine password are sealed (see
//! `carla_core::secret`), so the files are useless to another account.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use carla_core::library::emulation::{self, Console};
use carla_core::library::saveplace::{self, Env, SavePlace};
use carla_core::display::Mode;
use carla_core::link::health::{AllyView, Asking, Health, Pulse, CONNECTED_FOR};
use carla_core::link::{self, enrol};
use carla_core::remote::{Command, Done};
use carla_core::secret;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !text.is_ascii() {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect()
}

/// The companion's folder. `CARLA_COMPANION_DIR` overrides it, for testing.
pub fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CARLA_COMPANION_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("CarlaOS").join("companion");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".local").join("share").join("carlaos-companion")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub id: String,
    pub name: String,
    pub port: u16,
    /// Sunshine's web address on this machine.
    pub sunshine_url: String,
    pub sunshine_user: String,
    /// Sealed, hex.
    pub sunshine_password: String,
    /// Switch this desktop's screen to the Ally's size during a stream.
    #[serde(default = "yes")]
    pub match_screen: bool,
    /// Cover the desktop with a loading screen until the game shows.
    #[serde(default = "yes")]
    pub curtain: bool,
    /// Give each stream a virtual screen of its own, when the Virtual
    /// Display Driver is installed.
    #[serde(default = "yes")]
    pub virtual_screen: bool,
}

fn yes() -> bool {
    true
}

impl Config {
    pub fn new(name: &str) -> Self {
        Self {
            id: link::random_hex(8),
            name: name.to_string(),
            port: link::PORT,
            sunshine_url: "https://localhost:47990".to_string(),
            sunshine_user: String::new(),
            sunshine_password: String::new(),
            match_screen: true,
            curtain: true,
            virtual_screen: true,
        }
    }

    pub fn set_sunshine(&mut self, user: &str, password: &str) -> Result<(), String> {
        self.sunshine_user = user.to_string();
        self.sunshine_password = hex(&secret::seal(password.as_bytes())?);
        Ok(())
    }

    pub fn sunshine_password(&self) -> Option<String> {
        let sealed = unhex(&self.sunshine_password)?;
        String::from_utf8(secret::open(&sealed).ok()?).ok()
    }
}

/// An Ally that has been linked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// The device key, sealed, hex.
    pub key: String,
    pub added: i64,
}

impl Device {
    pub fn key(&self) -> Option<Vec<u8>> {
        secret::open(&unhex(&self.key)?).ok()
    }
}

/// The code currently shown to the administrator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Code {
    pub code: String,
    pub expires: i64,
    pub tries: u32,
}

/// A program the administrator added on this desktop: any game that Steam
/// does not list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extra {
    /// `carla_core::library::manual_key` of the program's path.
    pub key: String,
    pub title: String,
    pub exe: PathBuf,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub cover: Option<PathBuf>,
    /// The program name of the real game (`Game-Win64-Shipping.exe`), for a
    /// launcher that hands over to it and exits. The game counts as running
    /// while any program of that name runs.
    #[serde(default)]
    pub watch: Option<String>,
}

/// Where one game keeps its saves on this desktop, said by the administrator
/// so that the Ally and this desktop can keep them in step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveFolder {
    /// Names the folder in requests and in the commands.
    pub key: String,
    /// The catalogue id, when the game is one this desktop lists.
    #[serde(default)]
    pub game_id: Option<String>,
    pub title: String,
    pub folder: PathBuf,
    /// Where the folder is in words that hold on any machine, so that another
    /// desktop can suggest its own folder. Absent in files from before.
    #[serde(default)]
    pub place: Option<SavePlace>,
}

/// A folder a desktop thinks a game keeps its saves in, from where the
/// Ally says they are on another one. Only a suggestion: nothing is synced
/// until the administrator accepts it here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    /// The key the game's save folder would get: `saves accept KEY`.
    pub key: String,
    #[serde(default)]
    pub game_id: Option<String>,
    pub title: String,
    pub folder: PathBuf,
    pub place: SavePlace,
    pub at: i64,
}

/// One change of the suggestions at a time: two at once would lose one.
static SUGGESTIONS: Mutex<()> = Mutex::new(());

/// Suggestions kept; the oldest go first.
pub const MOST_SUGGESTIONS: usize = 200;

/// The game the next stream is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub game_id: String,
    pub title: String,
    pub source: String,
    pub source_id: String,
    pub install_path: PathBuf,
    /// The program to start, for a game that was added by hand.
    #[serde(default)]
    pub exe: Option<PathBuf>,
    #[serde(default)]
    pub args: String,
    /// See [`Extra::watch`].
    #[serde(default)]
    pub watch: Option<String>,
    /// The picture the Ally said it would ask for.
    #[serde(default)]
    pub screen: Option<Mode>,
    pub at: i64,
}

/// Written by the launcher for as long as a stream lasts, game or not, with
/// the time it last looked. Sunshine ends the launcher without warning when
/// a stream is quit, so this going stale is how the companion in the
/// background knows to tidy up after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Streaming {
    pub at: i64,
}

/// Written by the launcher while a game runs, with the time it last looked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playing {
    pub game_id: String,
    pub title: String,
    pub at: i64,
}

/// When and from where a linked Ally was last heard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub at: i64,
    pub address: String,
    /// When it last asked for a stream.
    #[serde(default)]
    pub prepared: Option<i64>,
}

/// What a linked Ally last said about itself, and the last measurement it
/// sent, which a later pulse without one does not wipe out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pulsed {
    pub at: i64,
    pub pulse: Pulse,
    pub health: Option<Health>,
}

/// What someone at the desktop said to a device that asks to be linked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Answered {
    enrol_id: String,
    allow: bool,
}

/// A request for an Ally that nobody fetched within this time is dropped.
pub const ASK_LIFETIME: i64 = 60;

/// A pending game older than this was for a stream that never started.
pub const PENDING_LIFETIME: i64 = 120;
/// The launcher writes this often; twice as old means it is gone.
pub const PLAYING_BEAT: i64 = 5;

/// A launcher that has said nothing for this long is gone. Generous: a game
/// that is loading can hold the whole machine up for a while, and putting the
/// screen back under a running stream is worse than doing it a little late.
pub const STREAM_SILENCE: i64 = 30;

#[derive(Debug, Clone)]
pub struct Files {
    dir: PathBuf,
    /// The well-known folders, when not this machine's own (tests).
    env: Option<Env>,
}

impl Files {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, env: None }
    }

    pub fn with_env(mut self, env: Env) -> Self {
        self.env = Some(env);
        self
    }

    /// The well-known folders of this machine.
    pub fn place_env(&self) -> Env {
        self.env.clone().unwrap_or_else(Env::real)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn read<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        serde_json::from_str(&fs::read_to_string(self.dir.join(name)).ok()?).ok()
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        let problem = |e: std::io::Error| format!("The companion could not save {name}: {e}.");
        fs::create_dir_all(&self.dir).map_err(problem)?;
        let text = serde_json::to_string_pretty(value).expect("serialisable");
        // Nothing changed: leave the file alone.
        if fs::read_to_string(self.dir.join(name)).is_ok_and(|now| now == text) {
            return Ok(());
        }
        // Write beside and rename, so a reader never sees half a file. The
        // name is unique, so two writers never share a half-written file.
        static TEMPS: AtomicU64 = AtomicU64::new(0);
        let beside = self.dir.join(format!("{name}.{}.{}.new", std::process::id(), TEMPS.fetch_add(1, Ordering::Relaxed)));
        fs::write(&beside, text).map_err(problem)?;
        fs::rename(&beside, self.dir.join(name)).map_err(|e| {
            let _ = fs::remove_file(&beside);
            problem(e)
        })
    }

    fn remove(&self, name: &str) {
        let _ = fs::remove_file(self.dir.join(name));
    }

    pub fn config(&self) -> Option<Config> {
        self.read("config.json")
    }

    pub fn save_config(&self, config: &Config) -> Result<(), String> {
        self.write("config.json", config)
    }

    pub fn devices(&self) -> Vec<Device> {
        self.read("devices.json").unwrap_or_default()
    }

    pub fn add_device(&self, name: &str, key: &[u8]) -> Result<Device, String> {
        let device = Device { id: link::random_hex(8), name: name.to_string(), key: hex(&secret::seal(key)?), added: now() };
        let mut devices = self.devices();
        devices.push(device.clone());
        self.write("devices.json", &devices)?;
        Ok(device)
    }

    /// Stop accepting a device. Returns whether it was known.
    pub fn forget_device(&self, id: &str) -> Result<bool, String> {
        let mut devices = self.devices();
        let before = devices.len();
        devices.retain(|d| d.id != id);
        self.write("devices.json", &devices)?;
        if id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            self.remove(&format!("ally-seen-{id}.json"));
            self.remove(&format!("ally-pulse-{id}.json"));
        }
        Ok(devices.len() != before)
    }

    /// Games added by hand. One that was added as a shortcut, before the
    /// companion could follow one, is given as the program behind it.
    pub fn extras(&self) -> Vec<Extra> {
        let mut extras: Vec<Extra> = self.read("games.json").unwrap_or_default();
        for extra in extras.iter_mut().filter(|e| carla_core::library::shortcut::is_shortcut(&e.exe)) {
            if let Ok(seen) = carla_core::library::ports::inspect(&extra.exe) {
                extra.exe = seen.exe;
                if extra.args.trim().is_empty() {
                    extra.args = seen.args;
                }
            }
        }
        extras
    }

    /// Add a program as a game. Adding the same program again updates it.
    pub fn add_extra(&self, title: &str, exe: &Path, args: &str, cover: Option<&Path>, watch: Option<&str>) -> Result<Extra, String> {
        let title = title.trim();
        if title.is_empty() || title.len() > 80 {
            return Err("The game needs a name of at most 80 letters.".to_string());
        }
        if !exe.is_absolute() || !exe.is_file() {
            return Err(format!("{} is not a program on this desktop. Give its full path.", exe.display()));
        }
        if let Some(cover) = cover.filter(|c| !c.is_file()) {
            return Err(format!("{} is not a picture on this desktop.", cover.display()));
        }
        // The game counts as running while this program does, and "End game"
        // closes it: only a plain game program name qualifies.
        let watch = match watch {
            None => None,
            Some(name) => Some(carla_core::library::launch::valid_watch(name).ok_or_else(|| {
                format!("{name:?} cannot be watched: give the game's own program, such as Game-Win64-Shipping.exe (an .exe file name only, not one of Windows' or a store's).")
            })?),
        };
        // Windows cannot start a shortcut as a program, so a shortcut is
        // followed to the program behind it, with the options it passes.
        let seen = carla_core::library::ports::inspect(exe)?;
        let args = if args.trim().is_empty() { seen.args.as_str() } else { args };
        let exe = seen.exe.as_path();
        let extra = Extra {
            key: carla_core::library::manual_key(&exe.to_string_lossy()),
            title: title.to_string(),
            exe: exe.to_path_buf(),
            args: args.trim().to_string(),
            cover: cover.map(Path::to_path_buf),
            watch,
        };
        let mut extras = self.extras();
        extras.retain(|e| e.key != extra.key);
        extras.push(extra.clone());
        extras.sort_by_key(|e| e.title.to_lowercase());
        self.write("games.json", &extras)?;
        Ok(extra)
    }

    /// Take a game off the list. Returns its title when it was there.
    pub fn drop_extra(&self, key: &str) -> Result<Option<String>, String> {
        let mut extras = self.extras();
        let found = extras.iter().position(|e| e.key == key).map(|i| extras.remove(i).title);
        if found.is_some() {
            self.write("games.json", &extras)?;
        }
        Ok(found)
    }

    /// The consoles set up on this desktop: an emulator and its folder of
    /// game files each.
    pub fn consoles(&self) -> Vec<Console> {
        self.read("consoles.json").unwrap_or_default()
    }

    /// Set up a console. The same emulator with the same folder again
    /// replaces the earlier one.
    pub fn add_console(&self, console: Console) -> Result<Console, String> {
        if !console.emulator.is_absolute() || !console.folder.is_absolute() {
            return Err("Give the full path of the emulator and of the folder of games.".to_string());
        }
        let mut console = emulation::checked(console)?;
        // Starts with a letter the Ally's own consoles never use, so a game
        // here and a game there cannot be taken for the same one by accident.
        let key = carla_core::library::manual_key(&format!("{}|{}", console.emulator.display(), console.folder.display()));
        console.id = format!("d{}", &key[..8]);
        let mut consoles = self.consoles();
        consoles.retain(|c| c.id != console.id);
        consoles.push(console.clone());
        consoles.sort_by_key(|c| c.name.to_lowercase());
        self.write("consoles.json", &consoles)?;
        Ok(console)
    }

    /// Forget a console. Returns its name when it was there.
    pub fn drop_console(&self, id: &str) -> Result<Option<String>, String> {
        let mut consoles = self.consoles();
        let found = consoles.iter().position(|c| c.id == id).map(|i| consoles.remove(i).name);
        if found.is_some() {
            self.write("consoles.json", &consoles)?;
        }
        Ok(found)
    }

    pub fn save_folders(&self) -> Vec<SaveFolder> {
        self.read("saves.json").unwrap_or_default()
    }

    /// Say where a game keeps its saves here. Saying it again replaces it.
    pub fn set_save_folder(&self, game_id: Option<&str>, title: &str, folder: &Path) -> Result<SaveFolder, String> {
        self.set_save_folder_in(game_id, title, folder, None)
    }

    /// As `set_save_folder`, knowing the folder the game is installed in.
    pub fn set_save_folder_in(&self, game_id: Option<&str>, title: &str, folder: &Path, game_folder: Option<&Path>) -> Result<SaveFolder, String> {
        let title = title.trim();
        if title.is_empty() || title.len() > 80 {
            return Err("The game needs a name of at most 80 letters.".to_string());
        }
        if !folder.is_absolute() || !folder.is_dir() {
            return Err(format!("{} is not a folder on this desktop. Give its full path.", folder.display()));
        }
        if !carla_core::library::saves::sensible(folder) {
            return Err("That folder is too general. Give the folder that holds only this game's saves.".to_string());
        }
        // Reading it through also shows whether it is small enough.
        carla_core::library::sync::manifest(folder)?;
        let key = carla_core::library::manual_key(&game_id.map(str::to_string).unwrap_or_else(|| format!("title:{title}")));
        let entry = SaveFolder { key, game_id: game_id.map(str::to_string), title: title.to_string(), folder: folder.to_path_buf(), place: saveplace::portable(folder, &self.place_env(), game_folder) };
        let mut folders = self.save_folders();
        folders.retain(|f| f.key != entry.key);
        folders.push(entry.clone());
        folders.sort_by_key(|f| f.title.to_lowercase());
        self.write("saves.json", &folders)?;
        // Settled: a suggestion for this game is no longer needed.
        let _ = self.dismiss_suggestion(&entry.key);
        Ok(entry)
    }

    pub fn suggestions(&self) -> Vec<Suggestion> {
        self.read("save-suggestions.json").unwrap_or_default()
    }

    /// Remember that the Ally says a game's saves are in `place`, which here
    /// is `folder`. One per game; a new one replaces the old.
    pub fn suggest(&self, game_id: Option<&str>, title: &str, folder: &Path, place: &SavePlace, at: i64) -> Result<(), String> {
        // Shown in a terminal later: no control or direction-changing characters.
        let clip = |s: &str, n: usize| carla_core::library::find::visible(s).trim().chars().take(n).collect::<String>();
        let title = clip(title, 80);
        if title.is_empty() {
            return Ok(());
        }
        let game_id = game_id.map(|g| clip(g, 200));
        let place = SavePlace { root: place.root, rel: clip(&place.rel, 1200) };
        let _lock = SUGGESTIONS.lock().unwrap_or_else(|e| e.into_inner());
        let key = carla_core::library::manual_key(&game_id.clone().unwrap_or_else(|| format!("title:{title}")));
        let mut list = self.suggestions();
        list.retain(|s| s.key != key);
        list.push(Suggestion { key, game_id, title, folder: folder.to_path_buf(), place, at });
        if list.len() > MOST_SUGGESTIONS {
            list.sort_by_key(|s| std::cmp::Reverse(s.at));
            list.truncate(MOST_SUGGESTIONS);
        }
        self.write("save-suggestions.json", &list)
    }

    /// Forget a suggestion. Returns its title when it was there.
    pub fn dismiss_suggestion(&self, key: &str) -> Result<Option<String>, String> {
        let _lock = SUGGESTIONS.lock().unwrap_or_else(|e| e.into_inner());
        let mut list = self.suggestions();
        let found = list.iter().position(|s| s.key == key).map(|i| list.remove(i).title);
        if found.is_some() {
            self.write("save-suggestions.json", &list)?;
        }
        Ok(found)
    }

    /// The administrator confirms a suggestion: the folder is named for real,
    /// with the same checks as `saves set`. It must exist.
    pub fn accept_suggestion(&self, key: &str, game_folder: Option<&Path>) -> Result<SaveFolder, String> {
        let s = self.suggestions().into_iter().find(|s| s.key == key).ok_or("No suggestion has that key (see carla-companion saves suggestions).")?;
        if !s.folder.is_dir() {
            return Err(format!("{} does not exist here. Create it or give the real folder with saves set.", s.folder.display()));
        }
        // The place is resolved again, here and now: the folder must still be
        // what it says, below no link, and not somewhere programs load from.
        let again = saveplace::resolve(&s.place, &self.place_env(), game_folder).filter(|f| *f == s.folder);
        let Some(folder) = again.filter(|f| !saveplace::loads_code(f, Some(&s.place))) else {
            return Err(format!("{} is not a folder this desktop will keep saves in step in. Give the real one with saves set.", s.folder.display()));
        };
        self.set_save_folder_in(s.game_id.as_deref(), &s.title, &folder, game_folder)
    }

    /// Stop keeping a game's saves in step. Returns its title when it was
    /// there. Neither the saves nor their safe copies are touched.
    pub fn clear_save_folder(&self, key: &str) -> Result<Option<String>, String> {
        let mut folders = self.save_folders();
        let found = folders.iter().position(|f| f.key == key).map(|i| folders.remove(i).title);
        if found.is_some() {
            self.write("saves.json", &folders)?;
        }
        Ok(found)
    }

    /// Where the safe copies of saves are kept.
    pub fn save_copies(&self) -> PathBuf {
        self.dir.join("save-copies")
    }

    /// Make a new code, replacing any earlier one.
    pub fn new_code(&self) -> Result<Code, String> {
        let code = Code { code: enrol::new_code(), expires: now() + enrol::CODE_LIFETIME, tries: 0 };
        self.write("enrol.json", &code)?;
        Ok(code)
    }

    /// The code, if one is still usable, counting this as one use of it.
    pub fn use_code(&self, now: i64) -> Option<String> {
        let mut code: Code = self.read("enrol.json")?;
        if now >= code.expires || code.tries >= enrol::CODE_TRIES {
            self.remove("enrol.json");
            return None;
        }
        code.tries += 1;
        self.write("enrol.json", &code).ok()?;
        Some(code.code)
    }

    /// The code that is on show, without using it up.
    pub fn code(&self, now: i64) -> Option<Code> {
        self.read::<Code>("enrol.json").filter(|code| now < code.expires && code.tries < enrol::CODE_TRIES)
    }

    pub fn clear_code(&self) {
        self.remove("enrol.json");
        self.remove("enrol-ask.json");
        self.remove("enrol-answer.json");
    }

    /// Until when an Ally can be linked: while a code is live. Looking does
    /// not use the code up.
    pub fn linking_until(&self, now: i64) -> Option<i64> {
        let code: Code = self.read("enrol.json")?;
        (now < code.expires && code.tries < enrol::CODE_TRIES).then_some(code.expires)
    }

    /// A device asks to be linked by being allowed here.
    pub fn set_asking(&self, asking: &Asking) -> Result<(), String> {
        self.remove("enrol-answer.json");
        self.write("enrol-ask.json", asking)
    }

    /// The device that is waiting to be allowed, if any still is.
    pub fn asking(&self, now: i64) -> Option<Asking> {
        let asking: Asking = self.read("enrol-ask.json")?;
        (now - asking.at <= enrol::APPROVE_LIFETIME && self.asking_answer(&asking.enrol_id).is_none()).then_some(asking)
    }

    /// Allow or refuse the device that is asking. False when it is no
    /// longer the one asking.
    pub fn answer_asking(&self, enrol_id: &str, allow: bool, now: i64) -> Result<bool, String> {
        if self.asking(now).is_none_or(|a| a.enrol_id != enrol_id) {
            return Ok(false);
        }
        self.write("enrol-answer.json", &Answered { enrol_id: enrol_id.to_string(), allow })?;
        Ok(true)
    }

    /// What was answered to a device that asked, if anything was yet.
    pub fn asking_answer(&self, enrol_id: &str) -> Option<bool> {
        self.read::<Answered>("enrol-answer.json").filter(|a| a.enrol_id == enrol_id).map(|a| a.allow)
    }

    /// Note that a linked Ally was heard from.
    pub fn set_ally_seen(&self, device: &str, seen: &Seen) {
        let _ = self.write(&format!("ally-seen-{device}.json"), seen);
    }

    pub fn ally_seen(&self, device: &str) -> Option<Seen> {
        self.read(&format!("ally-seen-{device}.json"))
    }

    /// Keep what an Ally said about itself. A pulse without a measurement
    /// (a game is running there) keeps the measurement from before.
    pub fn set_ally_pulse(&self, device: &str, pulse: Pulse, now: i64) -> Result<(), String> {
        let health = pulse.health.clone().or_else(|| self.ally_pulse(device).and_then(|p| p.health));
        self.write(&format!("ally-pulse-{device}.json"), &Pulsed { at: now, pulse, health })
    }

    pub fn ally_pulse(&self, device: &str) -> Option<Pulsed> {
        self.read(&format!("ally-pulse-{device}.json"))
    }

    /// Every linked Ally as a dashboard shows it. `streaming` says whether
    /// a stream is running now: it goes to the Ally that asked for one last.
    pub fn allies(&self, now: i64, streaming: bool) -> Vec<AllyView> {
        let devices = self.devices();
        let seen: Vec<Option<Seen>> = devices.iter().map(|d| self.ally_seen(&d.id)).collect();
        let prepared = |s: &Option<Seen>| s.as_ref().and_then(|s| s.prepared);
        let streamer = seen.iter().enumerate().filter(|(_, s)| prepared(s).is_some()).max_by_key(|(_, s)| prepared(s)).map(|(i, _)| i).filter(|_| streaming);
        devices
            .into_iter()
            .zip(seen)
            .enumerate()
            .map(|(i, (device, seen))| {
                let pulsed = self.ally_pulse(&device.id);
                let last_seen = seen.as_ref().map(|s| s.at).into_iter().chain(self.ally_listening(&device.id)).max();
                let streaming = streamer == Some(i);
                AllyView {
                    connected: streaming || last_seen.is_some_and(|at| now - at <= CONNECTED_FOR),
                    last_seen,
                    address: seen.map(|s| s.address),
                    streaming,
                    health: pulsed.as_ref().and_then(|p| p.health.clone()),
                    pulse: pulsed.map(|p| p.pulse),
                    id: device.id,
                    name: device.name,
                    linked_at: device.added,
                }
            })
            .collect()
    }

    /// Note what the launcher did, for whoever wonders why a stream ended.
    /// Sunshine runs the launcher unseen, so this file is all there is.
    pub fn log(&self, line: &str) {
        use std::io::Write;
        let path = self.dir.join("launcher.log");
        // Start again rather than grow without end.
        if fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
            let _ = fs::remove_file(&path);
        }
        let _ = fs::create_dir_all(&self.dir);
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{} {line}", now());
        }
    }

    pub fn set_pending(&self, pending: &Pending) -> Result<(), String> {
        self.write("pending.json", pending)
    }

    /// Take the game the stream is for. It can only be taken once.
    pub fn take_pending(&self, now: i64) -> Option<Pending> {
        let pending: Pending = self.read("pending.json")?;
        self.remove("pending.json");
        (now - pending.at <= PENDING_LIFETIME).then_some(pending)
    }

    pub fn set_playing(&self, playing: Option<&Playing>) {
        match playing {
            Some(p) => {
                let _ = self.write("playing.json", p);
            }
            None => self.remove("playing.json"),
        }
    }

    /// Leave a request for a linked Ally to pick up. One at a time per Ally.
    pub fn ask_ally(&self, device: &str, command: &Command) -> Result<(), String> {
        self.remove(&format!("ally-answer-{device}.json"));
        self.write(&format!("ally-ask-{device}.json"), &(now(), command))
    }

    /// The request waiting for this Ally, handed out once. One that waited
    /// too long is dropped: nobody is waiting for its answer any more.
    pub fn take_ally_ask(&self, device: &str, now: i64) -> Option<Command> {
        let name = format!("ally-ask-{device}.json");
        let (at, command): (i64, Command) = self.read(&name)?;
        self.remove(&name);
        (now - at <= ASK_LIFETIME).then_some(command)
    }

    /// Whether a request is still waiting to be fetched.
    pub fn ally_asked(&self, device: &str) -> bool {
        self.dir.join(format!("ally-ask-{device}.json")).is_file()
    }

    pub fn set_ally_answer(&self, device: &str, done: &Done) -> Result<(), String> {
        self.write(&format!("ally-answer-{device}.json"), done)
    }

    /// The answer to the request with this id, once it has come.
    pub fn take_ally_answer(&self, device: &str, id: &str) -> Option<Done> {
        let name = format!("ally-answer-{device}.json");
        let done: Done = self.read(&name)?;
        (done.id == id).then(|| {
            self.remove(&name);
            done
        })
    }

    /// Note that an Ally asked for requests just now.
    pub fn ally_listened(&self, device: &str, now: i64) {
        let _ = self.write(&format!("ally-listening-{device}.json"), &now);
    }

    /// When an Ally last asked for requests.
    pub fn ally_listening(&self, device: &str) -> Option<i64> {
        self.read(&format!("ally-listening-{device}.json"))
    }

    /// The launcher says a stream is going on.
    pub fn beat(&self, now: i64) {
        let _ = self.write("stream.json", &Streaming { at: now });
    }

    /// The stream is over, said by the launcher itself.
    pub fn stream_over(&self) {
        self.remove("stream.json");
    }

    /// Whether a launcher said so a moment ago.
    pub fn streaming(&self, now: i64) -> bool {
        self.read::<Streaming>("stream.json").is_some_and(|s| now - s.at <= STREAM_SILENCE)
    }

    /// Note that the screen was switched for a stream, and to what, before
    /// it is switched: whatever happens next, someone knows to put it back.
    pub fn set_switched(&self, to: Option<Mode>) -> Result<(), String> {
        match to {
            Some(mode) => self.write("screen.json", &mode),
            None => {
                self.remove("screen.json");
                Ok(())
            }
        }
    }

    /// What the screen was switched to for a stream, while it still is.
    pub fn switched(&self) -> Option<Mode> {
        self.read("screen.json")
    }

    /// Note when a virtual screen was made for a stream, before it is made;
    /// `None` once it is gone.
    pub fn set_virtual_screen(&self, at: Option<i64>) -> Result<(), String> {
        match at {
            Some(at) => self.write("virtual.json", &at),
            None => {
                self.remove("virtual.json");
                Ok(())
            }
        }
    }

    /// When the virtual screen that is on now was made for a stream.
    pub fn virtual_screen(&self) -> Option<i64> {
        self.read("virtual.json")
    }

    /// Ask the launcher to take its loading screen away.
    pub fn ask_reveal(&self, now: i64) -> Result<(), String> {
        self.write("reveal.json", &now)
    }

    /// Whether that was asked a moment ago. Asked once, done once.
    pub fn take_reveal(&self, now: i64) -> bool {
        let asked: Option<i64> = self.read("reveal.json");
        if asked.is_some() {
            self.remove("reveal.json");
        }
        asked.is_some_and(|at| now - at <= PLAYING_BEAT * 2)
    }

    /// Ask the launcher to close the game it is watching.
    pub fn ask_end(&self, now: i64) -> Result<(), String> {
        self.write("end.json", &now)
    }

    /// Whether the game was asked to close a moment ago. Asked once, closed once.
    pub fn take_end(&self, now: i64) -> bool {
        let asked: Option<i64> = self.read("end.json");
        self.remove("end.json");
        asked.is_some_and(|at| now - at <= PLAYING_BEAT * 2)
    }

    pub fn playing(&self, now: i64) -> Option<Playing> {
        let playing: Playing = self.read("playing.json")?;
        (now - playing.at <= PLAYING_BEAT * 2).then_some(playing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_games_list_from_before_watching_existed_still_loads() {
        let dir = std::env::temp_dir().join(format!("carla-files-watch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("games.json"), r#"[{"key":"abc","title":"Old Game","exe":"C:/Games/old.exe","args":"-x"}]"#).unwrap();
        let files = Files::new(dir.clone());
        let extras = files.extras();
        assert_eq!((extras.len(), extras[0].watch.clone(), extras[0].args.as_str()), (1, None, "-x"));
        // A pending game written by an older companion has none either.
        fs::write(dir.join("pending.json"), r#"{"game_id":"manual:abc","title":"Old Game","source":"manual","source_id":"abc","install_path":"C:/Games","at":1}"#).unwrap();
        let pending: Pending = files.read("pending.json").unwrap();
        assert_eq!(pending.watch, None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_watched_name_is_kept_as_a_bare_file_name() {
        let dir = std::env::temp_dir().join(format!("carla-files-watch2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let files = Files::new(dir.clone());
        let exe = dir.join("Start.exe");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&exe, b"program").unwrap();
        let added = files.add_extra("Game", &exe, "", None, Some("Game-Win64-Shipping.exe")).unwrap();
        assert_eq!(added.watch.as_deref(), Some("Game-Win64-Shipping.exe"));
        assert_eq!(files.add_extra("Game", &exe, "", None, None).unwrap().watch, None);
        // Names that would make the game run for ever, or close something that is not it.
        for bad in ["explorer.exe", "svchost.exe", "steam.exe", "a.exe", "game", "D:/Games/X/Game.exe", "  ", "\"Game.exe\"", "ga$me.exe"] {
            assert!(files.add_extra("Game", &exe, "", None, Some(bad)).is_err(), "{bad:?}");
        }
        assert_eq!(files.extras()[0].watch, None, "a refused name changes nothing");
        let _ = fs::remove_dir_all(&dir);
    }
}
