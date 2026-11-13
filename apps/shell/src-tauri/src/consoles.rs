//! Commands for consoles: an emulator the administrator installed, paired
//! with a folder of their own game files. All of it is for the administrator.

use std::path::PathBuf;

use carla_core::library::emulation::{self, Console, Suggestion};
use tauri::{AppHandle, State};

use crate::admin::Admin;
use crate::library::{now, Library};

#[tauri::command]
pub fn consoles_list(admin: State<'_, Admin>, library: State<'_, Library>) -> Result<Vec<Console>, String> {
    admin.require()?;
    library.store().consoles()
}

/// What to fill in for an emulator, going by its program's name.
#[tauri::command]
pub fn consoles_suggest(admin: State<'_, Admin>, emulator: String) -> Result<Suggestion, String> {
    admin.require()?;
    Ok(emulation::suggest(&PathBuf::from(emulator)))
}

/// Add or change a console, then look through its folder. Says how many
/// games it has.
#[tauri::command(async)]
pub fn consoles_save(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, console: Console) -> Result<usize, String> {
    admin.require()?;
    let console = emulation::checked(console)?;
    let saved = library.store().save_console(&console, now())?;
    let games = emulation::scan(&saved).len();
    library.scan(&app)?;
    Ok(games)
}

/// Forget a console. Its files are not touched.
#[tauri::command(async)]
pub fn consoles_remove(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<(), String> {
    admin.require()?;
    library.store().remove_console(&id)?;
    library.scan(&app).map(|_| ())
}
