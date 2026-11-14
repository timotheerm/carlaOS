//! Cloud gaming. See `carla_core::library::cloud`.

use carla_core::library::cloud::{self, CLOUD};
use carla_core::library::Install;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::admin::Admin;
use crate::library::{now, play, Library, SessionView};

/// What CarlaOS makes of a link before it is added.
#[derive(Debug, Clone, Serialize)]
pub struct Seen {
    service: String,
    /// A name read from the link, or empty when it has none.
    title: String,
}

#[tauri::command]
pub fn cloud_inspect(admin: State<'_, Admin>, link: String) -> Result<Seen, String> {
    admin.require()?;
    let (service, _) = cloud::checked(&link)?;
    Ok(Seen { service: service.name.to_string(), title: cloud::suggest_title(&link) })
}

/// Add a cloud game. Returns its id.
#[tauri::command]
pub fn cloud_add(admin: State<'_, Admin>, library: State<'_, Library>, link: String, title: String) -> Result<String, String> {
    admin.require()?;
    library.store().add_cloud(&link, &title, now())
}

/// Open a service's own pages: to browse it, or for the administrator to
/// sign in once. Anyone may; it is played like a game.
#[tauri::command(async)]
pub fn cloud_open(app: AppHandle, library: State<'_, Library>, service: String) -> Result<SessionView, String> {
    let found = cloud::service(&service).ok_or("CarlaOS does not know that service.")?;
    let install = Install {
        game_id: format!("cloudservice:{}", found.id),
        title: found.name.to_string(),
        source: "cloud".to_string(),
        source_id: found.id.to_string(),
        install_path: Default::default(),
        exe: None,
        args: found.home.to_string(),
    };
    play(app, library.inner(), install, CLOUD.to_string())
}
