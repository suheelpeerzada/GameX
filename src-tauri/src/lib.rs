pub mod ai;
pub mod axml;
pub mod device;
pub mod analyzer;
mod commands;
pub mod edits;
pub mod import;
pub mod jdwp;
pub mod keep;
pub mod live;
pub mod media;
pub mod mirror;
pub mod model;
pub mod preview;
pub mod rebuild;
pub mod scene;
pub mod settings;
pub mod toolbox;
pub mod tools;
pub mod uiview;
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
            commands::get_references,
            commands::scan_media,
            commands::scene_load,
            commands::scene_set,
            commands::export_files,
            commands::read_media_item,
            commands::save_media_item,
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
            commands::tools_catalog,
            commands::run_targets,
            commands::run_enable_adb,
            commands::run_options,
            commands::run_game,
            commands::run_status,
            commands::run_stop,
            commands::mirror_start,
            commands::mirror_stop,
            commands::mirror_touch,
            commands::mirror_key,
            commands::live_attach,
            commands::live_detach,
            commands::live_counts,
            commands::live_instances,
            commands::live_object,
            commands::live_set,
            commands::live_positions,
            commands::live_keep,
            commands::ui_pick,
            commands::tool_install,
            commands::tool_uninstall,
            commands::tools_missing,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GameX");
}
