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
mod setup;
mod stage;
mod sunshine;
mod virtual_screen;

use std::io::{BufRead, Write};
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::Arc;

use std::path::PathBuf;

use carla_core::library::emulation::{self, Console};
use carla_core::library::{achievements, downloads, owned, scan_store, steam, STORES};
use carla_core::link::{discover, enrol, wake, SUNSHINE_APP};
use carla_core::update::{self, Part};
use carla_core::{AppInfo, Role, VERSION};

use files::{Config, Files};
use server::Server;
use sunshine::{Streamer, Sunshine};

const HELP: &str = "CarlaOS desktop companion

  carla-companion            set up if needed, open linking for an Ally, and run
  carla-companion dashboard  open the dashboard: the Ally, the connection, the
                             stream and this PC at a glance
  carla-companion setup      set up again, asking for the name too
  carla-companion code       open linking again and show a new code
  carla-companion allies [--json]
                             the linked Allies: connected or not, ping, jitter,
                             lost packets and the picture streams get
  carla-companion firewall   let the Ally through this desktop's firewall
  carla-companion sunshine   show whether Sunshine is ready, and its user name
                             and password when the companion chose them
  carla-companion devices    list the linked devices
  carla-companion forget ID  stop accepting a linked device
  carla-companion games      list the games added by hand
  carla-companion add NAME PROGRAM [--args OPTIONS] [--cover PICTURE]
                             add a game that Steam does not list; put quotes
                             around a name or path that has spaces. PROGRAM
                             may be a shortcut: it is followed to its program
  carla-companion drop KEY   take a game added by hand off the list
  carla-companion consoles   list the consoles set up here (an emulator and a folder of games)
  carla-companion console add NAME EMULATOR FOLDER [--endings iso,chd] [--options OPTIONS]
                             let the Ally stream every game file in FOLDER through
                             EMULATOR; known emulators fill in endings and options
  carla-companion console drop ID
                             forget a console; no file is deleted
  carla-companion saves      list the games whose saves are kept in step with the Ally
  carla-companion saves set NAME FOLDER
                             say where a game keeps its saves on this desktop
  carla-companion saves guess NAME
                             suggest where a game keeps its saves, from folder names
  carla-companion saves find NAME
                             watch while you play and save, then suggest the folder
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

/// How busy this PC is. Processor use is what it was since the last time
/// this was asked, so the first answer has none.
fn vitals() -> server::Vitals {
    static SYSTEM: std::sync::Mutex<Option<(sysinfo::System, bool)>> = std::sync::Mutex::new(None);
    let mut system = SYSTEM.lock().unwrap_or_else(|e| e.into_inner());
    if system.is_none() {
        *system = sysinfo::System::new().ok().map(|s| (s, false));
    }
    let Some((system, asked_before)) = system.as_mut() else { return server::Vitals::default() };
    system.refresh_cpu_usage();
    system.refresh_memory();
    let cpu = asked_before.then(|| system.global_cpu_usage().round().clamp(0.0, 100.0) as u8);
    *asked_before = true;
    server::Vitals { cpu, memory_used: system.used_memory(), memory_total: system.total_memory() }
}

/// Show the dashboard in a window of its own when Edge is there (it is on
/// every Windows 11), and in the usual browser otherwise.
#[cfg(windows)]
fn open_dashboard(port: u16) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let url = format!("http://127.0.0.1:{port}/");
    let start = |what: &[&str]| std::process::Command::new("cmd").args(["/C", "start", ""]).args(what).creation_flags(CREATE_NO_WINDOW).status().is_ok_and(|s| s.success());
    if !start(&["msedge", &format!("--app={url}"), "--window-size=1240,860"]) {
        start(&[&url]);
    }
}

#[cfg(not(windows))]
fn open_dashboard(port: u16) {
    println!("The dashboard is at http://127.0.0.1:{port}/");
}

/// Whether the companion is running and answering on this machine.
fn answering(port: u16) -> bool {
    let address = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(500)).is_ok()
}

/// `carla-companion dashboard`, also behind the Start menu's entry. Starts
/// the companion in the background first when it is not running.
fn dashboard(files: &Files) -> Result<i32, String> {
    let config = match files.config() {
        Some(config) => config,
        None => setup(files, false)?,
    };
    if !answering(config.port) {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // The same way Windows starts it at sign-in: without a window.
        let quiet = std::process::Command::new("conhost.exe").arg("--headless").arg(&exe).arg("serve").spawn();
        if quiet.is_err() {
            std::process::Command::new(&exe).arg("serve").spawn().map_err(|e| format!("The companion could not be started: {e}."))?;
        }
        let waited = std::time::Instant::now();
        while !answering(config.port) && waited.elapsed() < std::time::Duration::from_secs(8) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    open_dashboard(config.port);
    Ok(0)
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

/// Get Sunshine working for the companion, asking as little as possible:
/// nothing when Sunshine is new or already set up, and at most for its user
/// name and password when someone set it up by hand before.
fn sunshine_ready(config: &mut Config) -> Result<(), String> {
    if cfg!(windows) && setup::sunshine_exe().is_none() && !sunshine::answers(&config.sunshine_url) {
        println!("Sunshine does the streaming and is not installed yet. Installing it now (Windows asks to allow it)...");
        setup::install_sunshine()?;
        println!("Sunshine is installed. Waiting for it to start...");
        setup::wait_for_sunshine(&config.sunshine_url, std::time::Duration::from_secs(90));
    }
    match setup::claim(config)? {
        setup::Claimed::Already => {}
        setup::Claimed::Fresh => {
            println!("Sunshine was new, so the companion chose a user name and password for it.");
            println!("Nobody has to type them. To open Sunshine's own page yourself: carla-companion sunshine shows them.");
        }
        setup::Claimed::Silent => {
            return Err("Sunshine is not running on this desktop. Start it (Start menu, Sunshine), then open CarlaOS Desktop Host again.".to_string());
        }
        setup::Claimed::Taken => {
            println!("Sunshine already has a user name and password, chosen when it was first opened.");
            println!("Type them here, or just press Enter and CarlaOS sets new ones (Windows asks to allow it).");
            loop {
                let user = ask("Sunshine user name", "");
                if user.is_empty() {
                    let exe = setup::sunshine_exe().ok_or("Sunshine's program was not found, so its password cannot be set anew. Type the user name and password instead.")?;
                    let password = setup::new_password();
                    if setup::reset_credentials(&exe, setup::USER, &password) {
                        config.set_sunshine(setup::USER, &password)?;
                        setup::wait_for_sunshine(&config.sunshine_url, std::time::Duration::from_secs(60));
                    }
                } else {
                    config.set_sunshine(&user, &ask("Sunshine password", ""))?;
                }
                match streamer(config).and_then(|s| s.check()) {
                    Ok(()) => break,
                    Err(e) => {
                        println!("\n{e}");
                        if ask("Try again? (y/n)", "y").to_lowercase().starts_with('n') {
                            return Err("Sunshine is not set up for the companion yet. Open CarlaOS Desktop Host again to finish.".to_string());
                        }
                    }
                }
            }
        }
    }
    streamer(config)?.ensure_app(&launcher_command())?;
    println!("Sunshine is ready. The \"{SUNSHINE_APP}\" application is in place.");
    Ok(())
}

/// Let the Ally through this desktop's firewall, once.
fn firewall_ready() {
    let Ok(exe) = std::env::current_exe() else { return };
    if !cfg!(windows) || setup::firewall_open(&exe) {
        return;
    }
    println!("Opening the firewall for the Ally, from your home network and Tailscale only (Windows asks to allow it)...");
    match setup::open_firewall(&exe) {
        Ok(()) => println!("The firewall is open for the Ally."),
        Err(e) => println!("{e}"),
    }
}

/// Set this desktop up. Without `questions` nothing is asked that the
/// companion can work out or do by itself.
fn setup(files: &Files, questions: bool) -> Result<Config, String> {
    let mut config = files.config().unwrap_or_else(|| Config::new(&computer_name()));
    println!("Setting up CarlaOS on this desktop. It is called \"{}\" on the Ally.\n", config.name);
    if questions {
        config.name = ask("Name shown on the Ally", &config.name);
    }
    // Saved first, so a set-up that stops half way carries on from here.
    files.save_config(&config)?;
    let sunshine = sunshine_ready(&mut config);
    files.save_config(&config)?;
    firewall_ready();
    match autostart::set(true) {
        Ok(()) => println!("The companion will start with Windows, without a window."),
        Err(e) => println!("The companion could not be set to start with Windows. {e}"),
    }
    if let Err(e) = sunshine {
        println!("\n{e}");
    }
    println!();
    Ok(config)
}

fn show_code(files: &Files) -> Result<(), String> {
    let code = files.new_code()?;
    println!("Linking an Ally is open for {} minutes.\n", enrol::CODE_LIFETIME / 60);
    println!("On the Ally: Settings, Desktops, Add a desktop, and choose this desktop.");
    println!("This window then asks you to allow it. Nothing has to be typed.");
    println!("(The Ally first asks for its own four-digit administrator PIN, chosen on the Ally.)\n");
    println!("If the Ally asks for a code instead, it is:\n\n    {} {}\n", &code.code[..3], &code.code[3..]);
    Ok(())
}

/// Ask whoever sits at this window to allow each device that wants to be
/// linked, for as long as the window is open.
fn attend(files: &Files) {
    let mut linked = files.devices().len();
    loop {
        if let Some(asking) = files.asking(files::now()) {
            println!("\n\"{}\" (at {}) wants to be linked to this desktop.", asking.device_name, asking.from);
            println!("It shows the number  {}  on its screen.", asking.check);
            let allow = ask("Is that your Ally, showing that number? (y/n)", "").to_lowercase().starts_with('y');
            match files.answer_asking(&asking.enrol_id, allow, files::now()) {
                Ok(true) if allow => println!("Allowed. The Ally finishes by itself in a moment."),
                Ok(true) => println!("Refused. Nothing was linked, and linking is closed. Open this program again to link an Ally."),
                _ => println!("That took too long. Choose this desktop on the Ally again."),
            }
        }
        let now_linked = files.devices().len();
        if now_linked > linked {
            println!("\nAn Ally is linked. This window can be closed: the companion carries on in the background.");
        }
        linked = now_linked;
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// `carla-companion allies`: the linked Allies and how the connection to
/// each is, as the Ally last measured it.
fn allies(files: &Files, json: bool) -> Result<i32, String> {
    let now = files::now();
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "allies": files.allies(now, files.streaming(now)) })).expect("serialisable"));
        return Ok(0);
    }
    let list = files.allies(now, files.streaming(now));
    if list.is_empty() {
        println!("No Ally is linked. Open CarlaOS Desktop Host to link one.");
    }
    let ago = |at: i64| match (now - at).max(0) {
        s if s < 90 => format!("{s} s ago"),
        s if s < 5400 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    };
    for ally in list {
        let state = if ally.streaming { "streaming now" } else if ally.connected { "connected" } else { "not connected" };
        let seen = ally.last_seen.map(|at| format!(", heard from {}", ago(at))).unwrap_or_default();
        println!("{}  ({})  {state}{}{seen}", ally.name, ally.id, ally.address.map(|a| format!(" at {a}")).unwrap_or_default());
        match &ally.health {
            Some(h) => {
                let route = if h.route == carla_core::link::health::Route::Away { "through Tailscale" } else { "home network" };
                println!("  {}  (measured {}, {route})", h.words(), ago(h.at));
                let codec = h.codec.as_ref().map(|c| format!(", {}", c.words())).unwrap_or_default();
                println!("  Streams get {}x{} at {} pictures a second, {} Mbit/s{codec}", h.picture.width, h.picture.height, h.picture.fps, h.picture.kbps / 1000);
                for advice in &h.advice {
                    println!("  Tip: {advice}");
                }
            }
            None => println!("  The connection has not been measured yet. The Ally does that by itself while CarlaOS is open on it."),
        }
        if let Some(p) = &ally.pulse {
            let moonlight = match (p.moonlight, p.paired) {
                (false, _) => "Moonlight not installed",
                (true, Some(false)) => "Moonlight not paired",
                (true, _) => "Moonlight ready",
            };
            let battery = p.battery.map(|b| format!(" · battery {b} %{}", if p.charging == Some(true) { ", charging" } else { "" })).unwrap_or_default();
            let playing = p.playing.as_ref().map(|t| format!(" · playing {t}")).unwrap_or_default();
            println!("  CarlaOS {} · {moonlight}{battery}{playing}", p.version);
        }
    }
    Ok(0)
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

/// `carla-companion console add|drop ...`
fn console(files: &Files, args: &[String]) -> Result<i32, String> {
    let usage = "carla-companion console add \"NAME\" \"C:\\path\\emulator.exe\" \"C:\\path\\games\" [--endings iso,chd] [--options \"...\"]\n  or: carla-companion console drop ID";
    match args.first().map(String::as_str) {
        Some("add") => {
            let (name, emulator, folder) = match &args[1..] {
                [name, emulator, folder, ..] => (name, PathBuf::from(emulator), PathBuf::from(folder)),
                _ => return Err(usage.to_string()),
            };
            let known = emulation::suggest(&emulator);
            let mut endings: Vec<String> = known.endings.iter().map(|e| e.to_string()).collect();
            let mut options = known.options.to_string();
            let mut rest = args[4..].iter();
            while let Some(flag) = rest.next() {
                let value = rest.next().ok_or(usage)?;
                match flag.as_str() {
                    "--endings" => endings = value.split([',', ' ']).map(str::to_string).collect(),
                    "--options" => options = value.clone(),
                    _ => return Err(usage.to_string()),
                }
            }
            let console = files.add_console(Console { id: String::new(), name: name.clone(), emulator, options, folder, endings })?;
            let games = emulation::scan(&console).len();
            println!("{} is set up with {games} games (id {}). They appear on the Ally within five minutes, or at once after \"Look for games again\" there.", console.name, console.id);
            println!("Options: {}\nEndings: {}", console.options, console.endings.join(", "));
            Ok(0)
        }
        Some("drop") => {
            let id = args.get(1).ok_or(usage)?;
            match files.drop_console(id)? {
                Some(name) => {
                    println!("{name} was forgotten. No file was deleted.");
                    Ok(0)
                }
                None => Err("No console has that id (see carla-companion consoles).".to_string()),
            }
        }
        _ => Err(usage.to_string()),
    }
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
    .with_dashboard(vitals, || update::check(Part::DesktopHost).map(|found| found.map(|f| f.version)))
    .with_virtual_screens(Box::new(virtual_screen::Vdd))
    .with_installs(server::Installs {
        offered: steam_library,
        start: |app| steam_root().and_then(|root| downloads::start_here(&root, app)),
        downloads: || steam::find_root().map(|root| downloads::steam(&root)).unwrap_or_default(),
        free: || steam::find_root().and_then(|root| downloads::roomiest(&root).1),
    })
}

fn steam_root() -> Result<std::path::PathBuf, String> {
    steam::find_root().ok_or_else(|| "Steam is not installed on this desktop.".to_string())
}

/// The games of the Steam library that are not installed on this desktop.
fn steam_library() -> Result<Vec<owned::App>, String> {
    owned::not_installed(&steam_root()?)
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
    // One socket finds this desktop, answers the measuring packets and sends
    // the bursts, so answers always come from the port they were asked at.
    let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).ok();
    let mut built = build(files, config).with_graphics(carla_core::link::codec::graphics());
    if let Some(socket) = udp.as_ref().and_then(|s| s.try_clone().ok()) {
        built = built.with_probe(socket);
    }
    let server = Arc::new(built);
    // A screen left switched by a stream that was cut off is put back.
    screen::watch(files.clone());
    // Likewise a virtual screen; and the driver's own one is taken away.
    virtual_screen::watch(files.clone());
    if let Some(socket) = udp {
        let answering = server.clone();
        std::thread::spawn(move || discover::answer(&socket, &|| answering.hello()));
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
        Some(word @ ("guess" | "find")) => find_saves(files, word == "find", args.get(1).ok_or("Say which game: carla-companion saves guess \"NAME\" or saves find \"NAME\".")?),
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

/// Suggest where a game keeps its saves. Only suggests: the administrator
/// confirms with `saves set`, and nothing is written or deleted here.
fn find_saves(files: &Files, play: bool, name: &str) -> Result<i32, String> {
    use carla_core::library::savefind::{self, Limits, Snapshot};
    let wanted = name.trim().to_lowercase();
    let store = installed().into_iter().map(|(source, g)| (format!("{source}:{}", g.source_id), g.title, Some(g.install_path)));
    let by_hand = files.extras().into_iter().map(|e| (format!("manual:{}", e.key), e.title, e.exe.parent().map(std::path::Path::to_path_buf)));
    let known = store.chain(by_hand).find(|(id, title, _)| id.to_lowercase() == wanted || title.to_lowercase() == wanted);
    let (title, install) = match known {
        Some((_, title, folder)) => (title, folder),
        None => (name.trim().to_string(), None),
    };
    let roots = savefind::known_roots(|v| std::env::var(v).ok(), install.as_deref());
    let found = if play {
        println!("Looking for where {title} keeps its saves. This only looks; nothing is changed.");
        let before = Snapshot::take(&roots, Limits::default());
        println!("Now start {title}, play a little, save the game, then quit it.");
        println!("Press Enter here when you have quit the game.");
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(n) if n > 0 => {}
            _ => return Err("This needs a console to press Enter in. Run it in a terminal window.".to_string()),
        }
        let after = Snapshot::take(&roots, Limits::default());
        if before.cut_short || after.cut_short {
            println!("The search was cut short by its limits, so the answer may be incomplete.");
        }
        savefind::diff(&before, &after, Some(&title))
    } else {
        savefind::guess(&title, &roots)
    };
    if found.is_empty() {
        println!("No likely folder found.");
        if play {
            println!("Some games write their saves only when they quit, keep them in the cloud or in the registry. Try again after quitting the game fully, or look for the folder by hand.");
        } else {
            println!("Try: carla-companion saves find \"{title}\" (it watches the game save).");
        }
        return Ok(0);
    }
    println!("Likely folders, best first:");
    for c in &found {
        let what = if play { format!("{} files changed, {} KB", c.changed, c.bytes / 1024) } else { format!("{} KB", c.bytes / 1024) };
        println!("\n  {}\n  {what}; {}", c.folder.display(), c.reasons.join(", "));
        if title.contains('"') {
            println!("  (the name has a double quote; type the saves set command by hand with this folder)");
        } else {
            println!("  carla-companion saves set \"{title}\" \"{}\"", c.folder.display());
        }
    }
    println!("\nNothing was set. Check a folder, then run its line above to keep it in step with the Ally.");
    Ok(0)
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
                None => setup(&files, false)?,
            };
            show_code(&files)?;
            println!("Leave this window open until the Ally is linked. After that the companion");
            println!("runs in the background by itself, also after you sign in to Windows.");
            println!("The dashboard that opens now shows the same code, and the Ally once it is linked.");
            let attending = files.clone();
            std::thread::spawn(move || attend(&attending));
            let port = config.port;
            std::thread::spawn(move || {
                // Once this window's own companion, or the one in the background, answers.
                std::thread::sleep(std::time::Duration::from_millis(800));
                open_dashboard(port);
            });
            if serve(&files, config).is_err() {
                // Already running in the background, which does the linking;
                // this window only answers its questions.
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            }
            Ok(0)
        }
        Some("dashboard") => dashboard(&files),
        Some("setup") => {
            setup(&files, true)?;
            println!("Done. If the companion is already running, sign out and in again (or restart it) to use the new settings.");
            Ok(0)
        }
        Some("code") => {
            configured(&files)?;
            show_code(&files)?;
            Ok(0)
        }
        Some("allies") => allies(&files, args.get(1).is_some_and(|a| a == "--json")),
        Some("firewall") => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            if setup::firewall_open(&exe) {
                println!("The firewall is already open for the Ally.");
            } else {
                setup::open_firewall(&exe)?;
                println!("The firewall is open for the Ally, from your home network and Tailscale only.");
            }
            Ok(0)
        }
        Some("sunshine") => {
            let mut config = configured(&files)?;
            let ready = sunshine_ready(&mut config);
            files.save_config(&config)?;
            ready?;
            if config.sunshine_user == setup::USER {
                println!("Sunshine's own page is https://localhost:47990");
                println!("User name: {}\nPassword:  {}", config.sunshine_user, config.sunshine_password().unwrap_or_default());
            }
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
            match steam_library() {
                Ok(apps) => println!("Not installed:    {} more in the Steam library, which a linked Ally can install here", apps.len()),
                Err(e) => println!("Not installed:    {e}"),
            }
            for d in steam::find_root().map(|root| downloads::steam(&root)).unwrap_or_default() {
                let part = if d.total > 0 { format!("{} %", d.done * 100 / d.total) } else { "waiting for Steam".to_string() };
                println!("Steam is fetching: {} ({part})", d.title);
            }
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
        Some("consoles") => {
            let consoles = files.consoles();
            if consoles.is_empty() {
                println!("No console is set up here. See carla-companion console add.");
            }
            for c in consoles {
                let games = emulation::scan(&c).len();
                let gone = if c.emulator.is_file() { String::new() } else { "  (emulator not found, hidden from the Ally)".to_string() };
                println!("{}  {}  {} games{gone}\n    {}\n    {}  {}", c.id, c.name, games, c.emulator.display(), c.folder.display(), c.endings.join(", "));
            }
            Ok(0)
        }
        Some("console") => console(&files, &args[1..]),
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
