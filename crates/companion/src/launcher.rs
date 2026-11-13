//! What Sunshine runs when a stream starts.
//!
//! Sunshine knows one application, "CarlaOS", whose command is this launcher.
//! The Ally says which game it wants just before it starts the stream; the
//! launcher starts that game and stays alive until the game has closed, which
//! is how Sunshine knows when to end the stream.

use std::time::Duration;

use carla_core::library::launch::{self, Phase, Watch};
use carla_core::library::{steam, Install};

use crate::files::{now, Files, Playing};

const POLL: Duration = Duration::from_secs(2);

/// Returns the process exit code.
pub fn run(files: &Files) -> i32 {
    let Some(pending) = files.take_pending(now()) else {
        eprintln!("No game was asked for. Streams are started from the Ally.");
        return 1;
    };
    let install = Install {
        game_id: pending.game_id.clone(),
        title: pending.title.clone(),
        source: pending.source,
        source_id: pending.source_id,
        install_path: pending.install_path,
        exe: pending.exe,
        args: pending.args,
    };
    let child = match launch::start(&install, steam::find_root().as_deref()) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("{} could not be started: {e}", install.title);
            return 1;
        }
    };
    let mut watch = Watch::new(&install, child);
    loop {
        files.set_playing(Some(&Playing { game_id: pending.game_id.clone(), title: pending.title.clone(), at: now() }));
        std::thread::sleep(POLL);
        if matches!(watch.poll(), Phase::Finished | Phase::NeverStarted) {
            break;
        }
    }
    files.set_playing(None);
    0
}
