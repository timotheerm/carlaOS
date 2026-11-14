//! CarlaOS desktop companion.
//!
//! Runs on each desktop that streams games to the Ally. It tells the Ally what
//! is installed, whether the desktop is ready, and starts the chosen game when
//! a stream begins. Streaming itself is stock Sunshine.
//!
//! Double-click it to set it up or to get a code for linking an Ally.

mod ally;
mod files;
mod launcher;
mod screen;
mod server;
mod stage;
mod sunshine;
mod virtual_screen;

use std::io::{BufRead, Write};
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::Arc;

use carla_core::library::{achievements, scan_store, steam, STORES};
use carla_core::link::{discover, enrol, wake, SUNSHINE_APP};
use carla_core::update::{self, Part};
use carla_core::{AppInfo, Role, VERSION};

use files::{Config, Files};
use server::Server;
use sunshine::{Streamer, Sunshine};

const HELP: &str = "CarlaOS desktop companion

  carla-companion            set up if needed, show a code for linking an Ally, and run
  carla-companion setup      ask the set-up questions again
  carla-companion code       show a new code for linking an Ally
  carla-companion devices    list the linked devices
  carla-companion forget ID  stop accepting a linked device
  carla-companion games      list the games added by hand
  carla-companion add NAME PROGRAM [--args OPTIONS] [--cover PICTURE]
                             add a game that Steam does not list; put quotes
                             around a name or path that has spaces. PROGRAM
                             may be a shortcut: it is followed to its program
  carla-companion drop KEY   take a game added by hand off the list
  carla-companion saves      list the games whose saves are kept in step with the Ally
  carla-companion saves set NAME FOLDER
                             say where a game keeps its saves on this desktop
  carla-companion saves clear KEY
                             stop keeping a game's saves in step
  carla-companion saves copies KEY
                             list the safe copies made before saves were replaced
  carla-companion saves restore KEY COPY
                             put a safe copy back
  carla-companion ally ...   administer a linked Ally from here (carla-companion ally help)
  carla-companion stream     what a stream does to this desktop's screen, and its modes
  carla-companion stream screen on|off
                             switch the screen to the Ally's size during a stream
  carla-companion stream curtain on|off
                             cover the desktop with a loading screen until the game shows
  carla-companion stream virtual on|off
                             give each stream a virtual screen of its own, at the
                             Ally's size, with the desktop's monitor dark meanwhile
  carla-companion stream virtual install
                             install the Virtual Display Driver that this needs
  carla-companion stream virtual test [WIDTHxHEIGHT [RATE]]
                             try it for fifteen seconds, without an Ally
  carla-companion stream test [WIDTHxHEIGHT [RATE]]
                             try both for ten seconds, without an Ally
  carla-companion stream restore
                             put the screens back now
  carla-companion sleep-test put this desktop to sleep now, the way the Ally would
  carla-companion status     show whether everything is ready
  carla-companion serve      run quietly (what happens when Windows starts)
  carla-companion update     install the newest release, if there is one
  carla-companion remove     stop starting with Windows
  carla-companion launch     used by Sunshine when a stream starts
  carla-companion info       print version details";

fn ask(question: &str, default: &str) -> String {
    if default.is_empty() {
        print!("{question}: ");
    } else {
        print!("{question} [{default}]: ");
    }
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    let line = line.trim();
    if line.is_empty() { default } else { line }.to_string()
}

fn pause() {
    println!();
    ask("Press Enter to close", "");
}

fn computer_name() -> String {
    std::env::var("COMPUTERNAME").ok().filter(|n| !n.is_empty()).or_else(|| sysinfo::System::host_name().ok()).unwrap_or_else(|| "Desktop".to_string())
}

fn hardware() -> String {
    let Ok(mut system) = sysinfo::System::new() else { return String::new() };
    system.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    system.refresh_memory();
    let cpu = system.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    let memory = (system.total_memory() as f64 / 1_073_741_824.0).round() as u64;
    match (cpu.is_empty(), memory) {
        (true, 0) => String::new(),
        (true, gb) => format!("{gb} GB memory"),
        (false, 0) => cpu,
        (false, gb) => format!("{cpu} · {gb} GB"),
    }
}

/// Addresses other machines can reach this desktop at, and the hardware
/// addresses of the adapters that carry a home-network address (the ones a
/// wake-up packet can reach).
fn network() -> (Vec<String>, Vec<String>) {
    let Ok(networks) = sysinfo::Networks::new_with_refreshed_list() else { return (Vec::new(), Vec::new()) };
    let (mut addresses, mut macs) = (Vec::new(), Vec::new());
    for (_, adapter) in &networks {
        let mut home = false;
        for ip in adapter.ip_networks() {
            let address = ip.addr.to_string();
            if wake::usable(&address) && !addresses.contains(&address) {
                home |= !wake::is_tailscale(&address);
                addresses.push(address);
            }
        }
        let mac = adapter.mac_address().to_string().to_lowercase();
        if home && wake::parse_mac(&mac).is_some() && !macs.contains(&mac) {
            macs.push(mac);
        }
    }
    addresses.sort();
    macs.sort();
    (addresses, macs)
}

/// The command Sunshine runs for a stream: this program, wherever it is.
fn launcher_command() -> String {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "carla-companion".to_string());
    format!("\"{exe}\" launch")
}

fn streamer(config: &Config) -> Result<Sunshine, String> {
    let password = config.sunshine_password().filter(|_| !config.sunshine_user.is_empty());
    let password = password.ok_or_else(|| "Sunshine's user name and password have not been given yet.".to_string())?;
    Sunshine::new(&config.sunshine_url, &config.sunshine_user, &password)
}

#[cfg(windows)]
mod autostart {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "CarlaOS Companion";

    /// Start with Windows, for this account only, without a window.
    pub fn set(on: bool) -> Result<(), String> {
        let problem = |e: std::io::Error| format!("Windows did not accept the change: {e}.");
        let (run, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN).map_err(problem)?;
        if on {
            let exe = std::env::current_exe().map_err(problem)?;
            // conhost --headless runs a console program with no window.
            run.set_value(NAME, &format!("conhost.exe --headless \"{}\" serve", exe.display())).map_err(problem)
        } else {
            match run.delete_value(NAME) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(problem(e)),
                _ => Ok(()),
            }
        }
    }
}

#[cfg(not(windows))]
mod autostart {
    pub fn set(_on: bool) -> Result<(), String> {
        Ok(())
    }
}

fn setup(files: &Files) -> Result<Config, String> {
    let mut config = files.config().unwrap_or_else(|| Config::new(&computer_name()));
    println!("Setting up the CarlaOS companion on this desktop.\n");
    config.name = ask("Name shown on the Ally", &config.name);
    println!("\nThe companion drives Sunshine for you. Give it the user name and password");
    println!("you chose when you first opened Sunshine (https://localhost:47990).");
    loop {
        let user = ask("Sunshine user name", &config.sunshine_user);
        let password = ask("Sunshine password", "");
        if password.is_empty() && config.sunshine_password().is_some() && user == config.sunshine_user {
            break; // keep what was there
        }
        config.set_sunshine(&user, &password)?;
        match streamer(&config).and_then(|s| s.ensure_app(&launcher_command())) {
            Ok(()) => {
                println!("Sunshine accepted it. The \"{SUNSHINE_APP}\" application is in place.");
                break;
            }
            Err(e) => {
                println!("\n{e}");
                if ask("Try again? (y/n)", "y").to_lowercase().starts_with('n') {
                    break;
                }
            }
        }
    }
    files.save_config(&config)?;
    match autostart::set(true) {
        Ok(()) => println!("The companion will start with Windows, without a window."),
        Err(e) => println!("The companion could not be set to start with Windows. {e}"),
    }
    println!();
    Ok(config)
}

fn show_code(files: &Files) -> Result<(), String> {
    let code = files.new_code()?;
    println!("Code for linking an Ally:\n\n    {} {}\n", &code.code[..3], &code.code[3..]);
    println!("On the Ally: Settings, Desktops, Add a desktop, choose this desktop, then type these six digits.");
    println!("The Ally first asks for its own four-digit administrator PIN. That one is chosen on the Ally; it is not this code.");
    println!("The code works for {} minutes.", enrol::CODE_LIFETIME / 60);
    Ok(())
}

/// Put this desktop to sleep in a moment, so the reply to the Ally still
/// leaves first.
fn sleep_soon() -> Result<(), String> {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(3));
        power::sleep();
    });
    Ok(())
}

#[cfg(windows)]
mod power {
    #[link(name = "powrprof")]
    extern "system" {
        fn SetSuspendState(hibernate: u8, force: u8, wake_up_events_disabled: u8) -> u8;
    }

    /// Sleep, not hibernate, and leave wake-up events on so the Ally can wake
    /// it again.
    pub fn sleep() {
        // SAFETY: plain call with constant arguments; returns when the machine wakes.
        unsafe { SetSuspendState(0, 0, 0) };
    }
}

#[cfg(not(windows))]
mod power {
    pub fn sleep() {}
}

/// Every game the stores on this desktop have installed.
fn installed() -> Vec<server::Found> {
    let root = steam::find_root();
    STORES.iter().flat_map(|store| scan_store(store, root.as_deref()).into_iter().map(move |game| (*store, game))).collect()
}

fn build(files: &Files, config: Config) -> Server {
    let streamer = streamer(&config).ok().map(|s| Box::new(s) as Box<dyn Streamer>);
    let scan = Box::new(installed);
    Server::new(
        files.clone(),
        config,
        streamer,
        scan,
        launcher_command(),
        hardware(),
        network,
        |app_id| steam::find_root().and_then(|root| achievements::steam(&root, app_id)).unwrap_or_default(),
        sleep_soon,
    )
    .with_updater(update_from_afar)
    .with_virtual_screens(Box::new(virtual_screen::Vdd))
}

/// A linked Ally asked this desktop to update. Answer as soon as it is known
/// whether there is a newer release, and install it once the answer is out:
/// the installer stops this program and starts the new one.
fn update_from_afar() -> Result<Option<String>, String> {
    let Some(found) = update::check(Part::DesktopHost)? else { return Ok(None) };
    let version = found.version.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if let Err(e) = update::download(&found, &update::download_dir(), &|_| {}).and_then(|installer| update::install(Part::DesktopHost, &installer)) {
            eprintln!("{e}");
        }
    });
    Ok(Some(version))
}

/// `carla-companion update`, also behind the Start menu's "Update" entry.
fn update_here() -> Result<(), String> {
    println!("CarlaOS Desktop Host {VERSION}. Looking for a newer version...");
    let megabytes = std::cell::Cell::new(0);
    let progress = |bytes: u64| {
        let now = bytes / 1_048_576;
        if now != megabytes.replace(now) {
            print!("\rDownloading: {now} MB");
            let _ = std::io::stdout().flush();
        }
    };
    match update::update_now(Part::DesktopHost, &progress)? {
        Some(found) => {
            println!("\nVersion {} is being installed. This window closes by itself,", found.version);
            println!("and the companion carries on in the background.");
            // The installer stops this program. Wait for that rather than racing it.
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
        None => {
            println!("This is the newest version.");
            pause();
        }
    }
    Ok(())
}

/// Run the API. `Err` when the port is taken, which means it already runs.
fn serve(files: &Files, config: Config) -> Result<(), String> {
    let port = config.port;
    let http = tiny_http::Server::http((Ipv4Addr::UNSPECIFIED, port)).map_err(|e| e.to_string())?;
    let server = Arc::new(build(files, config));
    // A screen left switched by a stream that was cut off is put back.
    screen::watch(files.clone());
    // Likewise a virtual screen; and the driver's own one is taken away.
    virtual_screen::watch(files.clone());
    if let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)) {
        let hello = server.hello();
        std::thread::spawn(move || discover::answer(&socket, &hello));
    }
    server::serve(server, http);
    Ok(())
}

fn configured(files: &Files) -> Result<Config, String> {
    files.config().ok_or_else(|| "The companion is not set up yet. Run it without arguments first.".to_string())
}

/// The commands about keeping saves in step. Only here, at the desktop, can
/// a folder be named: the Ally can never point the companion at one.
fn saves(files: &Files, args: &[String]) -> Result<i32, String> {
    use carla_core::library::saves::Backups;
    let folders = files.save_folders();
    let entry = |key: Option<&String>| {
        let key = key.ok_or("Say which game by its key (see carla-companion saves).")?;
        folders.iter().find(|f| &f.key == key).ok_or_else(|| "No game has that key (see carla-companion saves).".to_string())
    };
    match args.first().map(String::as_str) {
        None => {
            if folders.is_empty() {
                println!("No game's saves are kept in step yet. Add one with:");
                println!("  carla-companion saves set \"NAME OF THE GAME\" \"C:\\path\\to\\its saves\"");
            }
            for f in &folders {
                let gone = if f.folder.is_dir() { "" } else { "  (folder not found)" };
                println!("{}  {}  {}{gone}", f.key, f.title, f.folder.display());
            }
            Ok(0)
        }
        Some("set") => {
            let usage = "Say the game and its save folder: carla-companion saves set \"NAME\" \"C:\\path\\to\\saves\"";
            let (name, folder) = (args.get(1).ok_or(usage)?, args.get(2).ok_or(usage)?);
            // A game this desktop lists is known by its id, like on the Ally.
            let wanted = name.trim().to_lowercase();
            let store = installed().into_iter().map(|(source, g)| (format!("{source}:{}", g.source_id), g.title));
            let by_hand = files.extras().into_iter().map(|e| (format!("manual:{}", e.key), e.title));
            let known = store.chain(by_hand).find(|(id, title)| id.to_lowercase() == wanted || title.to_lowercase() == wanted);
            let saved = match &known {
                Some((id, title)) => files.set_save_folder(Some(id), title, std::path::Path::new(folder))?,
                None => files.set_save_folder(None, name, std::path::Path::new(folder))?,
            };
            println!("Saves of {} are now kept in step with the Ally, from {}.", saved.title, saved.folder.display());
            if known.is_none() {
                println!("No game with that name is listed on this desktop, so it is matched with the Ally's game of exactly that name.");
            }
            println!("On the Ally the game needs its save folder too: Settings, Manage games.");
            Ok(0)
        }
        Some("clear") => match files.clear_save_folder(args.get(1).ok_or("Say which game: carla-companion saves clear KEY (see carla-companion saves).")?)? {
            Some(title) => {
                println!("Saves of {title} are no longer kept in step. Nothing was deleted.");
                Ok(0)
            }
            None => Err("No game has that key (see carla-companion saves).".to_string()),
        },
        Some("copies") => {
            let f = entry(args.get(1))?;
            let copies = Backups::new(&files.save_copies(), &f.key).list();
            if copies.is_empty() {
                println!("No safe copy of {} has been needed yet.", f.title);
            }
            for c in copies {
                println!("{}  {} files, {} KB, taken {} minutes ago", c.id, c.files, c.bytes / 1024, (files::now() - c.at).max(0) / 60);
            }
            Ok(0)
        }
        Some("restore") => {
            let f = entry(args.get(1))?;
            let copy = args.get(2).ok_or("Say which copy (see carla-companion saves copies KEY).")?;
            if files.playing(files::now()).is_some() {
                return Err("Close the game first.".to_string());
            }
            Backups::new(&files.save_copies(), &f.key).restore(&f.folder, copy, files::now())?;
            println!("That copy of {} is back in place. What was there before is kept as a new safe copy.", f.title);
            Ok(0)
        }
        _ => {
            println!("{HELP}");
            Ok(0)
        }
    }
}

fn on_off(word: Option<&String>) -> Result<bool, String> {
    match word.map(String::as_str) {
        Some("on") => Ok(true),
        Some("off") => Ok(false),
        _ => Err("Say on or off.".to_string()),
    }
}

/// "1920x1080" and an optional rate.
fn parse_mode(size: Option<&String>, rate: Option<&String>) -> Result<carla_core::display::Mode, String> {
    let wrong = || "Say the size like this: carla-companion stream test 1920x1080 60".to_string();
    let Some(size) = size else { return Ok(carla_core::display::Mode::new(1920, 1080, 60)) };
    let (w, h) = size.to_lowercase().split_once('x').map(|(w, h)| (w.parse::<u32>(), h.parse::<u32>())).ok_or_else(wrong)?;
    let hz = rate.map(|r| r.parse::<u32>()).unwrap_or(Ok(60)).map_err(|_| wrong())?;
    Ok(carla_core::display::Mode::new(w.map_err(|_| wrong())?, h.map_err(|_| wrong())?, hz))
}

/// The commands about what a stream does to this desktop.
fn stream(files: &Files, args: &[String]) -> Result<i32, String> {
    use carla_core::display;
    let mut config = configured(files)?;
    let words = |on: bool| if on { "on" } else { "off" };
    match args.first().map(String::as_str) {
        None => {
            println!("Switch the screen to the Ally's size during a stream: {}", words(config.match_screen));
            println!("Cover the desktop until the game shows:               {}", words(config.curtain));
            use virtual_screen::Driver;
            let installed = virtual_screen::Vdd.present();
            println!(
                "A virtual screen of its own for each stream:          {}{}",
                words(config.virtual_screen),
                if installed { "" } else { " (the driver is not installed: carla-companion stream virtual install)" }
            );
            if files.virtual_screen().is_some() {
                println!("A virtual screen is on for a stream. It is taken away when the stream ends.");
            }
            match display::current() {
                Some(now) => println!("The screen now:  {}", now.words()),
                None => println!("The screen now:  not known"),
            }
            if let Some(to) = files.switched() {
                println!("Switched for a stream to {}. It is put back when the stream ends.", to.words());
            }
            let modes = display::modes();
            for (name, want) in [("the Ally (1920x1080 at 60)", display::Mode::new(1920, 1080, 60)), ("a 16:10 laptop (1920x1200 at 60)", display::Mode::new(1920, 1200, 60))] {
                match display::choose(&modes, want) {
                    Some(to) => println!("For {name} it would take {}.", to.words()),
                    None => println!("For {name} it has no suitable mode and would be left as it is."),
                }
            }
            let mut sizes: Vec<(u32, u32)> = modes.iter().map(|m| (m.width, m.height)).collect();
            sizes.sort_unstable();
            sizes.dedup();
            println!("Sizes it has:    {}", sizes.iter().rev().map(|(w, h)| format!("{w}x{h}")).collect::<Vec<_>>().join(", "));
            Ok(0)
        }
        Some("screen") => {
            config.match_screen = on_off(args.get(1))?;
            files.save_config(&config)?;
            println!("From the next stream on, the screen is {}.", if config.match_screen { "switched to the Ally's size" } else { "left as it is" });
            Ok(0)
        }
        Some("curtain") => {
            config.curtain = on_off(args.get(1))?;
            files.save_config(&config)?;
            println!("From the next stream on, the desktop is {}.", if config.curtain { "covered until the game shows" } else { "shown as it is" });
            Ok(0)
        }
        Some("virtual") => match args.get(1).map(String::as_str) {
            Some("install") => {
                println!("Installing the Virtual Display Driver (open source, signed; Windows asks to allow it)...");
                virtual_screen::install()?;
                config.virtual_screen = true;
                files.save_config(&config)?;
                // The driver starts with a screen of its own switched on: off with it.
                std::thread::sleep(std::time::Duration::from_secs(3));
                virtual_screen::settle(&virtual_screen::Vdd, files, files::now());
                println!("Done. From the next stream on, it has a screen of its own at the Ally's size.");
                println!("After a graphics driver update, run this again if streams fall back to the desktop's screen.");
                Ok(0)
            }
            Some("test") => {
                let want = parse_mode(args.get(2), args.get(3))?;
                println!("For fifteen seconds: a virtual screen as for a {}x{} stream. The monitor goes dark meanwhile.", want.width, want.height);
                let to = virtual_screen::enter(&virtual_screen::Vdd, files, want)?;
                std::thread::sleep(std::time::Duration::from_secs(15));
                virtual_screen::leave(&virtual_screen::Vdd, files);
                println!("The virtual screen was on at {} and is gone; the desktop's screens are back.", to.words());
                Ok(0)
            }
            _ => {
                config.virtual_screen = on_off(args.get(1))?;
                files.save_config(&config)?;
                if config.virtual_screen {
                    use virtual_screen::Driver;
                    if virtual_screen::Vdd.present() {
                        println!("From the next stream on, it has a virtual screen of its own.");
                    } else {
                        println!("On, but the driver is not installed yet: carla-companion stream virtual install");
                    }
                } else {
                    virtual_screen::leave(&virtual_screen::Vdd, files);
                    println!("From the next stream on, the desktop's own screen is used.");
                }
                Ok(0)
            }
        },
        Some("test") => {
            let want = parse_mode(args.get(1), args.get(2))?;
            println!("For ten seconds: the screen as for a {}x{} stream, and the loading screen.", want.width, want.height);
            match launcher::rehearse(files, &screen::Main, want, std::time::Duration::from_secs(10))? {
                Some(to) => println!("The screen was switched to {} and put back.", to.words()),
                None => println!("The screen was left as it is: it was already right, or has no suitable mode (see carla-companion stream)."),
            }
            Ok(0)
        }
        Some("restore") => {
            virtual_screen::leave(&virtual_screen::Vdd, files);
            display::restore()?;
            let _ = files.set_switched(None);
            println!("The screen is back to what the Windows settings say.");
            Ok(0)
        }
        _ => {
            println!("{HELP}");
            Ok(0)
        }
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    let files = Files::new(files::default_dir());
    match args.first().map(String::as_str) {
        None => {
            let config = match files.config() {
                Some(config) => config,
                None => setup(&files)?,
            };
            show_code(&files)?;
            println!("\nLeave this window open, or close it once the Ally is linked:");
            println!("the companion also runs in the background after you sign in to Windows.");
            if serve(&files, config).is_err() {
                // Already running in the background, which serves the code.
                pause();
            }
            Ok(0)
        }
        Some("setup") => {
            setup(&files)?;
            println!("Done. If the companion is already running, sign out and in again (or restart it) to use the new settings.");
            Ok(0)
        }
        Some("code") => {
            configured(&files)?;
            show_code(&files)?;
            Ok(0)
        }
        Some("devices") => {
            let devices = files.devices();
            if devices.is_empty() {
                println!("No device is linked.");
            }
            for d in devices {
                println!("{}  {}", d.id, d.name);
            }
            Ok(0)
        }
        Some("forget") => {
            let id = args.get(1).ok_or("Say which device: carla-companion forget ID (see carla-companion devices).")?;
            if files.forget_device(id)? {
                println!("That device is no longer accepted.");
                Ok(0)
            } else {
                Err("No linked device has that id.".to_string())
            }
        }
        Some("status") => {
            let config = configured(&files)?;
            println!("Name on the Ally: {}", config.name);
            println!("Linked devices:   {}", files.devices().len());
            let games = installed();
            for (store, name) in [("steam", "Steam games:   "), ("epic", "Epic games:    "), ("gog", "GOG games:     ")] {
                println!("{name}   {}", games.iter().filter(|(s, _)| *s == store).count());
            }
            println!("Added by hand:    {}", files.extras().len());
            let (addresses, macs) = network();
            println!("Addresses:        {}", if addresses.is_empty() { "none found".to_string() } else { addresses.join(", ") });
            println!("Wake-up adapters: {}", if macs.is_empty() { "none found".to_string() } else { macs.join(", ") });
            match streamer(&config).and_then(|s| s.ensure_app(&launcher_command())) {
                Ok(()) => println!("Sunshine:         ready"),
                Err(e) => println!("Sunshine:         {e}"),
            }
            Ok(0)
        }
        Some("games") => {
            let extras = files.extras();
            if extras.is_empty() {
                println!("No game has been added by hand. Steam games are found by themselves.");
            }
            for e in extras {
                let gone = if e.exe.is_file() { "" } else { "  (program not found, hidden from the Ally)" };
                println!("{}  {}  {}{gone}", e.key, e.title, e.exe.display());
            }
            Ok(0)
        }
        Some("add") => {
            let usage = "Say the name and the program: carla-companion add \"NAME\" \"C:\\path\\game.exe\" [--args \"...\"] [--cover \"C:\\path\\picture.jpg\"]";
            let (title, exe) = (args.get(1).ok_or(usage)?, args.get(2).ok_or(usage)?);
            let (mut game_args, mut cover) = (String::new(), None);
            let mut rest = args[3..].iter();
            while let Some(flag) = rest.next() {
                let value = rest.next().ok_or(usage)?;
                match flag.as_str() {
                    "--args" => game_args = value.clone(),
                    "--cover" => cover = Some(std::path::PathBuf::from(value)),
                    _ => return Err(usage.to_string()),
                }
            }
            let added = files.add_extra(title, std::path::Path::new(exe), &game_args, cover.as_deref())?;
            println!("{} was added. It appears on the Ally within five minutes, or at once after \"Look for games again\" there.", added.title);
            Ok(0)
        }
        Some("drop") => {
            let key = args.get(1).ok_or("Say which game: carla-companion drop KEY (see carla-companion games).")?;
            match files.drop_extra(key)? {
                Some(title) => {
                    println!("{title} was taken off the list. Nothing was deleted from the disk.");
                    Ok(0)
                }
                None => Err("No game added by hand has that key.".to_string()),
            }
        }
        Some("saves") => saves(&files, &args[1..]),
        Some("stream") => stream(&files, &args[1..]),
        Some("ally") => {
            // The companion in the background hands the request to the Ally.
            // If its port is free, it is not running.
            let serving = files.config().is_some_and(|c| std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, c.port)).is_err());
            ally::run(&files, &args[1..], serving)
        }
        Some("sleep-test") => {
            println!("Going to sleep in three seconds. Wake it from the Ally to check that waking works.");
            sleep_soon()?;
            std::thread::sleep(std::time::Duration::from_secs(5));
            Ok(0)
        }
        Some("serve") => {
            let config = configured(&files)?;
            serve(&files, config).map_err(|e| format!("The companion could not start listening: {e}"))?;
            Ok(0)
        }
        Some("update") => {
            if let Err(e) = update_here() {
                eprintln!("\n{e}");
                pause();
                return Ok(1);
            }
            Ok(0)
        }
        Some("remove") => {
            autostart::set(false)?;
            println!("The companion no longer starts with Windows. Its settings are in {}.", files.dir().display());
            Ok(0)
        }
        Some("launch") => Ok(launcher::run(&files)),
        Some("info") => {
            println!("{}", serde_json::to_string_pretty(&AppInfo::current(Role::Companion)).expect("serialisable"));
            Ok(0)
        }
        _ => {
            println!("{HELP}");
            Ok(0)
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            if args.is_empty() {
                pause();
            }
            1
        }
    };
    std::process::exit(code);
}
