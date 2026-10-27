//! Tauri commands. Each one is a thin wrapper: the work lives in the modules
//! it calls, so it can be tested without a window.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, State};

use crate::import;
use crate::model::{ImportProgress, ImportRecord};
use crate::settings::Settings;
use crate::tools::{self, Tool, ToolStatus};
use crate::workspace::Workspace;

pub struct AppState {
    workspace_root: PathBuf,
    settings_path: PathBuf,
    settings: Mutex<Settings>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        let settings_path = data_dir.join("settings.json");
        AppState {
            workspace_root: data_dir.join("workspace"),
            settings: Mutex::new(Settings::load(&settings_path)),
            settings_path,
        }
    }

    fn workspace(&self) -> Workspace {
        Workspace::new(self.workspace_root.clone())
    }

    fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
}

#[tauri::command]
pub async fn import_game(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ImportRecord, String> {
    let workspace = state.workspace();
    let settings = state.settings();
    // Unpacking and decompiling can take minutes; keep it off the UI thread.
    tauri::async_runtime::spawn_blocking(move || {
        import::import_game(
            PathBuf::from(path).as_path(),
            &workspace,
            &settings,
            |stage, message| {
                let _ = app.emit(
                    "import-progress",
                    ImportProgress {
                        stage: stage.into(),
                        message: message.into(),
                    },
                );
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<ImportRecord>, String> {
    state.workspace().list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_project(state: State<'_, AppState>, id: String) -> Result<ImportRecord, String> {
    state
        .workspace()
        .load_record(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    settings
        .save(&state.settings_path)
        .map_err(|e| e.to_string())?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
pub fn tool_status(state: State<'_, AppState>) -> Vec<ToolStatus> {
    let settings = state.settings();
    [Tool::Apktool, Tool::Jadx]
        .into_iter()
        .map(|t| tools::status(t, &settings))
        .collect()
}
