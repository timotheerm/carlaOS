//! CarlaOS desktop companion.
//!
//! Runs on each desktop that streams games to the Ally. It tells the Ally what
//! is installed, whether the desktop is ready, and starts the chosen game when
//! a stream begins. Streaming itself is stock Sunshine.
//!
//! Double-click it to set it up or to get a code for linking an Ally.

mod files;
mod launcher;
mod server;
mod sunshine;

use std::io::{BufRead, Write};
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::Arc;

use carla_core::library::{achievements, steam};
use carla_core::link::{discover, enrol, wake, SUNSHINE_APP};
use carla_core::{AppInfo, Role};

use files::{Config, Files};
use server::Server;
use sunshine::{Streamer, Sunshine};

const HELP: &str = "CarlaOS desktop companion

  carla-companion            set up if needed, show a code for linking an Ally, and run
  carla-companion setup      ask the set-up questions again
  carla-companion code       show a new code for linking an Ally
  carla-companion devices    list the linked devices
  carla-companion forget ID  stop accepting a linked device
  carla-companion status     show whether everything is ready
  carla-companion serve      run quietly (what happens when Windows starts)
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
    println!("On the Ally: Settings, Desktops, Add a desktop. The code works for {} minutes.", enrol::CODE_LIFETIME / 60);
    Ok(())
}

fn build(files: &Files, config: Config) -> Server {
    let streamer = streamer(&config).ok().map(|s| Box::new(s) as Box<dyn Streamer>);
    let scan = Box::new(|| steam::find_root().as_deref().map(steam::scan).unwrap_or_default());
    Server::new(files.clone(), config, streamer, scan, launcher_command(), hardware(), network, |app_id| {
        steam::find_root().and_then(|root| achievements::steam(&root, app_id)).unwrap_or_default()
    })
}

/// Run the API. `Err` when the port is taken, which means it already runs.
fn serve(files: &Files, config: Config) -> Result<(), String> {
    let port = config.port;
    let http = tiny_http::Server::http((Ipv4Addr::UNSPECIFIED, port)).map_err(|e| e.to_string())?;
    let server = Arc::new(build(files, config));
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
            let games = steam::find_root().as_deref().map(steam::scan).unwrap_or_default();
            println!("Steam games:      {}", games.len());
            let (addresses, macs) = network();
            println!("Addresses:        {}", if addresses.is_empty() { "none found".to_string() } else { addresses.join(", ") });
            println!("Wake-up adapters: {}", if macs.is_empty() { "none found".to_string() } else { macs.join(", ") });
            match streamer(&config).and_then(|s| s.ensure_app(&launcher_command())) {
                Ok(()) => println!("Sunshine:         ready"),
                Err(e) => println!("Sunshine:         {e}"),
            }
            Ok(0)
        }
        Some("serve") => {
            let config = configured(&files)?;
            serve(&files, config).map_err(|e| format!("The companion could not start listening: {e}"))?;
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
