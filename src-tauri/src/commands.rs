//! Tauri commands. Each one is a thin wrapper: the work lives in the modules
//! it calls, so it can be tested without a window.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, State};

use crate::analyzer;
use crate::edits::{EditState, EditStore};
use crate::import;
use crate::model::{Category, GameProject, ImportProgress, ImportRecord};
use crate::preview::{self, Preview};
use crate::rebuild::{self, BuildInfo, BuildOptions, BuildResult};
use crate::settings::Settings;
use crate::tools::{self, Tool, ToolStatus};
use crate::workspace::Workspace;

pub struct AppState {
    workspace_root: PathBuf,
    signing_dir: PathBuf,
    settings_path: PathBuf,
    settings: Mutex<Settings>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        let settings_path = data_dir.join("settings.json");
        AppState {
            workspace_root: data_dir.join("workspace"),
            signing_dir: data_dir.join("signing"),
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

    fn edits(&self, id: &str) -> Result<EditStore, String> {
        let dir = self.workspace().project_dir(id).map_err(|e| e.to_string())?;
        Ok(EditStore::new(&dir))
    }
}

/// Runs an edit, then returns the new edit state for the UI.
fn edit_and_report(
    state: &AppState,
    id: &str,
    f: impl FnOnce(&EditStore) -> std::io::Result<()>,
) -> Result<EditState, String> {
    let store = state.edits(id)?;
    f(&store).map_err(|e| e.to_string())?;
    store.state().map_err(|e| e.to_string())
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

/// Deletes an imported project and its workspace folder (original copy,
/// unpacked files, edits and history). The source file the user imported
/// from is not touched.
#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.workspace().delete_project(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_project(state: State<'_, AppState>, id: String) -> Result<ImportRecord, String> {
    state
        .workspace()
        .load_record(&id)
        .map_err(|e| e.to_string())
}

/// The project's categorized content. Runs the analyzer the first time (or
/// when `refresh` is set) and caches the result.
#[tauri::command]
pub async fn get_analysis(
    state: State<'_, AppState>,
    id: String,
    refresh: bool,
) -> Result<GameProject, String> {
    let workspace = state.workspace();
    tauri::async_runtime::spawn_blocking(move || {
        analyzer::load_or_analyze(&workspace, &id, refresh)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Moves an object to another category by hand; `None` restores the
/// analyzer's choice.
#[tauri::command]
pub fn set_category(
    state: State<'_, AppState>,
    id: String,
    object_id: String,
    category: Option<Category>,
) -> Result<(), String> {
    let workspace = state.workspace();
    let mut overrides = workspace.load_overrides(&id).map_err(|e| e.to_string())?;
    match category {
        Some(c) => overrides.insert(object_id, c),
        None => overrides.remove(&object_id),
    };
    workspace
        .save_overrides(&id, &overrides)
        .map_err(|e| e.to_string())
}

/// Preview of one file; `path` is relative to the project's `unpacked/`.
#[tauri::command]
pub async fn read_preview(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<Preview, String> {
    let workspace = state.workspace();
    tauri::async_runtime::spawn_blocking(move || {
        preview::preview(&workspace.project_dir(&id)?, &path)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// The whole text of a file (its edited version if any), for the editor.
#[tauri::command]
pub fn read_text_file(state: State<'_, AppState>, id: String, path: String) -> Result<String, String> {
    let dir = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    preview::read_text(&dir, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_edit_state(state: State<'_, AppState>, id: String) -> Result<EditState, String> {
    edit_and_report(&state, &id, |_| Ok(()))
}

#[tauri::command]
pub fn save_text_file(
    state: State<'_, AppState>,
    id: String,
    path: String,
    text: String,
) -> Result<EditState, String> {
    edit_and_report(&state, &id, |s| s.save_text(&path, &text))
}

/// Replaces a file with one the user picked (`source` is a path on disk).
#[tauri::command]
pub fn replace_file(
    state: State<'_, AppState>,
    id: String,
    path: String,
    source: String,
) -> Result<EditState, String> {
    edit_and_report(&state, &id, |s| s.replace(&path, std::path::Path::new(&source)))
}

#[tauri::command]
pub fn revert_file(state: State<'_, AppState>, id: String, path: String) -> Result<EditState, String> {
    edit_and_report(&state, &id, |s| s.revert(&path))
}

#[tauri::command]
pub fn undo_edit(state: State<'_, AppState>, id: String) -> Result<EditState, String> {
    edit_and_report(&state, &id, |s| s.undo().map(|_| ()))
}

/// Current name, package, version, icon and permissions for the build dialog.
#[tauri::command]
pub fn get_build_info(state: State<'_, AppState>, id: String) -> Result<BuildInfo, String> {
    let workspace = state.workspace();
    let record = workspace.load_record(&id).map_err(|e| e.to_string())?;
    let dir = workspace.project_dir(&id).map_err(|e| e.to_string())?;
    Ok(rebuild::build_info(&dir, &record, &state.settings(), &state.signing_dir))
}

#[tauri::command]
pub async fn build_apk(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    options: BuildOptions,
) -> Result<BuildResult, String> {
    let dir = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    let settings = state.settings();
    let signing_dir = state.signing_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        rebuild::build(&dir, &settings, &signing_dir, &options, |stage, message| {
            let _ = app.emit(
                "build-progress",
                ImportProgress {
                    stage: stage.into(),
                    message: message.into(),
                },
            );
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A PNG the user picked, as a data URL, to preview a new icon.
#[tauri::command]
pub fn read_png(path: String) -> Result<String, String> {
    use base64::Engine;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if !bytes.starts_with(b"\x89PNG") {
        return Err("The icon must be a PNG image.".into());
    }
    if bytes.len() > 4 << 20 {
        return Err("That image is too large for an icon.".into());
    }
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
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
    [Tool::Apktool, Tool::Jadx, Tool::Signer]
        .into_iter()
        .map(|t| tools::status(t, &settings))
        .collect()
}
