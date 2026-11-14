// No console window behind the shell in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod admin;
mod art;
mod boot;
mod consoles;
mod details;
mod device;
mod hosts;
mod installs;
mod library;
mod quick;
mod remote;
mod savesync;
mod updates;

use carla_core::{AppInfo, Role};
use tauri::Manager;

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo::current(Role::Shell)
}

#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    let context = tauri::generate_context!();
    // Before anything is shown: started by Windows, CarlaOS may decide to
    // leave the desktop alone this time.
    let guard = carla_core::boot::Guard::new(boot::record_path(&context.config().identifier));
    if !boot::should_start(&guard) {
        return;
    }
    tauri::Builder::default()
        .manage(boot::Boot(guard))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            let library = library::Library::open(&data_dir)?;
            quick::watch(app.handle().clone(), library.clone());
            app.manage(admin::Admin::new(data_dir.clone()));
            remote::watch(app.handle().clone(), library.clone());
            installs::watch(app.handle().clone(), library.clone());
            app.manage(library);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            quit,
            admin::admin_status,
            admin::admin_set_pin,
            admin::admin_unlock,
            admin::admin_lock,
            admin::cleanup_scan,
            admin::cleanup_apply,
            admin::cleanup_undo,
            library::library_scan,
            library::library_list,
            library::library_set_favourite,
            library::library_session,
            library::library_end_session,
            library::library_launch,
            library::library_pick,
            library::library_add_manual,
            library::library_remove_manual,
            consoles::consoles_list,
            consoles::consoles_suggest,
            consoles::consoles_save,
            consoles::consoles_remove,
            hosts::hosts_list,
            hosts::hosts_refresh,
            hosts::hosts_discover,
            hosts::hosts_link,
            hosts::hosts_remove,
            hosts::hosts_wake,
            hosts::hosts_rescan,
            hosts::hosts_sleep,
            details::library_details,
            details::saves_backup,
            details::saves_restore,
            details::saves_pick_folder,
            details::saves_forget_folder,
            savesync::saves_sync,
            boot::boot_ready,
            boot::boot_set,
            remote::hosts_admin,
            remote::hosts_set_admin,
            hosts::hosts_picture,
            hosts::hosts_set_picture,
            quick::quick_state,
            quick::quick_set_sound,
            quick::quick_set_brightness,
            quick::quick_close,
            quick::quick_end_game,
            quick::quick_show_desktop,
            details::library_updates,
            art::art_find,
            art::art_use,
            art::art_pick,
            art::art_clear,
            art::art_key,
            art::art_set_key,
            art::library_inspect,
            details::meta_search,
            details::meta_fetch,
            hosts::hosts_update,
            hosts::hosts_check,
            hosts::hosts_repair,
            updates::update_check,
            updates::update_install,
            installs::installs_start,
            installs::installs_list,
            installs::installs_room,
            installs::storage_overview,
            installs::storage_uninstall,
        ])
        .run(context)
        .expect("failed to start the CarlaOS shell");
}
