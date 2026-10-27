pub mod ai;
pub mod analyzer;
mod commands;
pub mod edits;
pub mod import;
pub mod model;
pub mod preview;
pub mod rebuild;
pub mod settings;
pub mod tools;
pub mod workspace;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            app.manage(commands::AppState::new(data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::import_game,
            commands::list_projects,
            commands::get_project,
            commands::delete_project,
            commands::get_analysis,
            commands::set_category,
            commands::read_preview,
            commands::read_text_file,
            commands::get_edit_state,
            commands::save_text_file,
            commands::replace_file,
            commands::revert_file,
            commands::undo_edit,
            commands::get_build_info,
            commands::build_apk,
            commands::read_png,
            commands::ai_status,
            commands::ai_set_key,
            commands::ai_models,
            commands::ai_set_model,
            commands::ai_clear_key,
            commands::ai_explain,
            commands::ai_ask,
            commands::ai_propose,
            commands::get_settings,
            commands::save_settings,
            commands::tool_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GameX");
}
