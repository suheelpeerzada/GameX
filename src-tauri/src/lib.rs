mod commands;
pub mod import;
pub mod model;
pub mod settings;
pub mod tools;
pub mod workspace;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            app.manage(commands::AppState::new(data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::import_game,
            commands::list_projects,
            commands::get_project,
            commands::get_settings,
            commands::save_settings,
            commands::tool_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GameX");
}
