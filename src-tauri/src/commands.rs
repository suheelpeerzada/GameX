//! Tauri commands. Each one is a thin wrapper: the work lives in the modules
//! it calls, so it can be tested without a window.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, State};

use crate::ai::{self, providers::ModelInfo, AiStatus, Answer, Explanation, Proposal, ValueChange};
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
    data_dir: PathBuf,
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
            data_dir: data_dir.clone(),
            signing_dir: {
                crate::toolbox::set_tools_dir(data_dir.join("tools"));
                data_dir.join("signing")
            },
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

// ---------------------------------------------------------------------------
// AI assistant (optional)

#[tauri::command]
pub fn ai_status(state: State<'_, AppState>) -> AiStatus {
    ai::status(&state.data_dir)
}

#[derive(serde::Serialize)]
pub struct AiKeyResult {
    status: AiStatus,
    models: Vec<ModelInfo>,
}

/// Checks a key with its provider, then stores it in the credential store.
#[tauri::command]
pub async fn ai_set_key(state: State<'_, AppState>, key: String) -> Result<AiKeyResult, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        ai::set_key(&data_dir, &key).map(|(status, models)| AiKeyResult { status, models })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn ai_models() -> Result<Vec<ModelInfo>, String> {
    tauri::async_runtime::spawn_blocking(ai::models)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn ai_set_model(state: State<'_, AppState>, model: String) -> Result<AiStatus, String> {
    ai::set_model(&state.data_dir, &model)
}

#[tauri::command]
pub fn ai_clear_key(state: State<'_, AppState>) -> Result<AiStatus, String> {
    ai::clear(&state.data_dir)
}

#[tauri::command]
pub async fn ai_explain(
    state: State<'_, AppState>,
    id: String,
    path: String,
    selection: Option<String>,
) -> Result<Explanation, String> {
    let data_dir = state.data_dir.clone();
    let project = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || ai::explain(&data_dir, &project, &path, selection.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn ai_ask(
    state: State<'_, AppState>,
    id: String,
    path: String,
    selection: Option<String>,
    question: String,
) -> Result<Answer, String> {
    let data_dir = state.data_dir.clone();
    let project = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        ai::ask(&data_dir, &project, &path, selection.as_deref(), &question)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Where other classes use this class. No AI involved.
#[tauri::command]
pub async fn get_references(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<ai::References, String> {
    let project = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || ai::references(&project, &path))
        .await
        .map_err(|e| e.to_string())
}

/// A Godot text scene's nodes, properties and resources.
#[tauri::command]
pub fn scene_load(state: State<'_, AppState>, id: String, path: String) -> Result<crate::scene::Scene, String> {
    let project = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    let text = preview::read_text(&project, &path).map_err(|e| e.to_string())?;
    Ok(crate::scene::parse(&project.join("unpacked"), &path, &text))
}

/// Sets properties of one node and saves them as one undoable edit.
#[tauri::command]
pub fn scene_set(
    state: State<'_, AppState>,
    id: String,
    path: String,
    node: String,
    changes: Vec<(String, String)>,
) -> Result<EditState, String> {
    let project = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    let mut updated = preview::read_text(&project, &path).map_err(|e| e.to_string())?;
    for (key, value) in &changes {
        updated = crate::scene::set_property(&updated, &node, key, value)?;
    }
    edit_and_report(&state, &id, |s| s.save_text(&path, &updated))
}

/// Exports a ZIP or folder import with the edits applied.
#[tauri::command]
pub fn export_files(state: State<'_, AppState>, id: String, dest: String) -> Result<usize, String> {
    state.edits(&id)?.export_files(std::path::Path::new(&dest)).map_err(|e| e.to_string())
}

/// Searches one file for embedded images and sounds (no AI involved).
#[tauri::command]
pub async fn scan_media(state: State<'_, AppState>, id: String, path: String) -> Result<Vec<crate::media::Found>, String> {
    let project = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || crate::media::scan_file(&project, &path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// One embedded item as a data URL, for previews and playback.
#[tauri::command]
pub fn read_media_item(
    state: State<'_, AppState>,
    id: String,
    path: String,
    offset: usize,
    len: usize,
    mime: String,
) -> Result<String, String> {
    use base64::Engine;
    let project = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    let bytes = crate::media::read_item(&project, &path, offset, len).map_err(|e| e.to_string())?;
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Saves one embedded item to a file the user chose.
#[tauri::command]
pub fn save_media_item(
    state: State<'_, AppState>,
    id: String,
    path: String,
    offset: usize,
    len: usize,
    dest: String,
) -> Result<(), String> {
    let project = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    let bytes = crate::media::read_item(&project, &path, offset, len).map_err(|e| e.to_string())?;
    std::fs::write(&dest, bytes).map_err(|e| format!("Couldn't save to {dest}: {e}"))
}

/// Asks the AI for edits. Nothing is saved: the UI shows the proposal and
/// saves it through `save_text_file` only if the user approves.
#[tauri::command]
pub async fn ai_propose(
    state: State<'_, AppState>,
    id: String,
    path: String,
    selection: Option<String>,
    changes: Vec<ValueChange>,
    instruction: String,
) -> Result<Proposal, String> {
    let data_dir = state.data_dir.clone();
    let project = state.workspace().project_dir(&id).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        ai::propose(&data_dir, &project, &path, selection.as_deref(), &changes, &instruction)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Optional tools (Settings → Tools)

#[tauri::command]
pub fn tools_catalog() -> Vec<crate::toolbox::ToolInfo> {
    crate::toolbox::catalog()
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolProgress {
    id: String,
    received: u64,
    total: Option<u64>,
}

/// Downloads and installs one tool into GameX's tools folder.
#[tauri::command]
pub async fn tool_install(app: AppHandle, id: String) -> Result<crate::toolbox::ToolInfo, String> {
    let spec = crate::toolbox::spec(&id).ok_or("Unknown tool.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut last = 0u64;
        crate::toolbox::install(spec, |received, total| {
            // About every 256 KB is plenty for a progress bar.
            if received - last >= 256 << 10 || Some(received) == total {
                last = received;
                let _ = app.emit("tool-progress", ToolProgress { id: id.clone(), received, total });
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Which of these tools can't be found anywhere (a Settings path, GameX's
/// own copy, PATH or JAVA_HOME), so the UI can offer to install them.
#[tauri::command]
pub fn tools_missing(state: State<'_, AppState>, ids: Vec<String>) -> Vec<String> {
    let settings = state.settings();
    ids.into_iter()
        .filter(|id| {
            let available = match id.as_str() {
                "java" => tools::java_tool("java").is_some(),
                "apktool" => tools::resolve(Tool::Apktool, &settings).is_some(),
                "jadx" => tools::resolve(Tool::Jadx, &settings).is_some(),
                "uber-apk-signer" => tools::resolve(Tool::Signer, &settings).is_some(),
                other => crate::toolbox::spec(other).is_some_and(|s| crate::toolbox::info(s).installed),
            };
            !available
        })
        .collect()
}

#[tauri::command]
pub fn tool_uninstall(id: String) -> Result<(), String> {
    crate::toolbox::uninstall(&id)
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
