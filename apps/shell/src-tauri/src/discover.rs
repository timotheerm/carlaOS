//! Looking for games, ports and emulators that are on the Ally and not in
//! the library. See `carla_core::library::discover`. Administrator only.

use std::collections::HashSet;

use carla_core::library::discover::{self, Found};
use tauri::State;

use crate::admin::Admin;
use crate::library::Library;

#[tauri::command(async)]
pub fn discover_scan(admin: State<'_, Admin>, library: State<'_, Library>) -> Result<Found, String> {
    admin.require()?;
    let known: HashSet<String> = library.store().known_programs()?.into_iter().collect();
    Ok(discover::look(&discover::places(), &known))
}
