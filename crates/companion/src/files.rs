//! What the companion keeps on disk, all in one folder of the Windows account
//! it runs under. Keys and the Sunshine password are sealed (see
//! `carla_core::secret`), so the files are useless to another account.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use carla_core::display::Mode;
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
}

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
}

impl Files {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
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
        // Write beside and rename, so a reader never sees half a file.
        let beside = self.dir.join(format!("{name}.new"));
        fs::write(&beside, serde_json::to_string_pretty(value).expect("serialisable")).map_err(problem)?;
        fs::rename(&beside, self.dir.join(name)).map_err(problem)
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
    pub fn add_extra(&self, title: &str, exe: &Path, args: &str, cover: Option<&Path>) -> Result<Extra, String> {
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

    pub fn save_folders(&self) -> Vec<SaveFolder> {
        self.read("saves.json").unwrap_or_default()
    }

    /// Say where a game keeps its saves here. Saying it again replaces it.
    pub fn set_save_folder(&self, game_id: Option<&str>, title: &str, folder: &Path) -> Result<SaveFolder, String> {
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
        let entry = SaveFolder { key, game_id: game_id.map(str::to_string), title: title.to_string(), folder: folder.to_path_buf() };
        let mut folders = self.save_folders();
        folders.retain(|f| f.key != entry.key);
        folders.push(entry.clone());
        folders.sort_by_key(|f| f.title.to_lowercase());
        self.write("saves.json", &folders)?;
        Ok(entry)
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

    pub fn clear_code(&self) {
        self.remove("enrol.json");
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
