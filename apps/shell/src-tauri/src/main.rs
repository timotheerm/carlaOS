// No console window behind the shell in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod admin;

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
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            app.manage(admin::Admin::new(data_dir));
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
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the CarlaOS shell");
}
