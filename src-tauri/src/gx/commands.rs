//! The window's calls for GameX projects. Each one runs off the main
//! thread, so a big project never freezes the window.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};
use walkdir::WalkDir;

use super::model::{Behavior, ProjectInfo, Scene, Thing};
use super::project::{self, History, ProjectEntry, ProjectIndex};
use super::sample;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> std::io::Result<T> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

/// `Documents/GameX/Projects`, created if needed.
fn root(app: &AppHandle) -> Result<PathBuf, String> {
    let documents = app.path().document_dir().map_err(|e| format!("Couldn't find your Documents folder: {e}"))?;
    let root = project::projects_root(&documents);
    std::fs::create_dir_all(&root).map_err(|e| format!("Couldn't create {}: {e}", root.display()))?;
    Ok(root)
}

#[tauri::command]
pub fn gx_projects_root(app: AppHandle) -> Result<String, String> {
    root(&app).map(|p| p.display().to_string())
}

#[tauri::command]
pub async fn gx_list_projects(app: AppHandle) -> Result<Vec<ProjectEntry>, String> {
    let root = root(&app)?;
    blocking(move || project::list(&root)).await
}

/// Creates a project (or the sample project) and returns its folder.
#[tauri::command]
pub async fn gx_create_project(app: AppHandle, name: String, sample: bool) -> Result<String, String> {
    let root = root(&app)?;
    blocking(move || if sample { sample::create(&root) } else { project::create(&root, &name) })
        .await
        .map(|p| p.display().to_string())
}

#[tauri::command]
pub async fn gx_open_project(path: String) -> Result<ProjectIndex, String> {
    blocking(move || project::open(&PathBuf::from(path))).await
}

#[tauri::command]
pub async fn gx_load_scene(path: String, id: String) -> Result<Scene, String> {
    blocking(move || project::load_scene(&PathBuf::from(path), &id)).await
}

#[tauri::command]
pub async fn gx_load_thing(path: String, id: String) -> Result<Thing, String> {
    blocking(move || project::load_thing(&PathBuf::from(path), &id)).await
}

#[tauri::command]
pub async fn gx_load_behavior(path: String, id: String) -> Result<Behavior, String> {
    blocking(move || project::load_behavior(&PathBuf::from(path), &id)).await
}

/// The save calls return what Undo would now take back.
#[tauri::command]
pub async fn gx_save_scene(path: String, scene: Scene, summary: String) -> Result<Option<String>, String> {
    blocking(move || {
        let dir = PathBuf::from(path);
        project::save_scene(&dir, &scene, &summary)?;
        History::new(&dir).last()
    })
    .await
}

#[tauri::command]
pub async fn gx_save_thing(path: String, thing: Thing, summary: String) -> Result<Option<String>, String> {
    blocking(move || {
        let dir = PathBuf::from(path);
        project::save_thing(&dir, &thing, &summary)?;
        History::new(&dir).last()
    })
    .await
}

#[tauri::command]
pub async fn gx_save_behavior(path: String, behavior: Behavior, summary: String) -> Result<Option<String>, String> {
    blocking(move || {
        let dir = PathBuf::from(path);
        project::save_behavior(&dir, &behavior, &summary)?;
        History::new(&dir).last()
    })
    .await
}

/// An asset's bytes (`asset` relative to the project's `assets/`), sent as binary.
#[tauri::command]
pub async fn gx_read_asset(path: String, asset: String) -> Result<tauri::ipc::Response, String> {
    blocking(move || {
        if !crate::edits::is_plain_relative(&asset) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid asset path"));
        }
        std::fs::read(PathBuf::from(path).join("assets").join(&asset))
    })
    .await
    .map(tauri::ipc::Response::new)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Translated {
    pub path: String,
    pub report: super::godot::Report,
}

/// Where an imported game's layer is recorded, in its import folder.
const LAYER_LINK: &str = "layer.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct LayerLink {
    path: String,
}

/// The layer (GameX project) made for an imported game, if there is one.
#[tauri::command]
pub async fn gx_layer_for_import(state: tauri::State<'_, crate::commands::AppState>, id: String) -> Result<Option<String>, String> {
    let workspace = state.workspace();
    blocking(move || {
        let link = workspace.project_dir(&id)?.join(LAYER_LINK);
        let Ok(text) = std::fs::read_to_string(link) else { return Ok(None) };
        let link: LayerLink = serde_json::from_str(&text).map_err(std::io::Error::other)?;
        Ok(PathBuf::from(&link.path).join(project::PROJECT_FILE).is_file().then_some(link.path))
    })
    .await
}

/// Reads an imported game with the reader for its engine into the new,
/// empty GameX project at `dir`.
/// A game's name as people know it: its launcher label, else the import's name.
fn game_name(workspace: &crate::workspace::Workspace, id: &str) -> std::io::Result<String> {
    let record = workspace.load_record(id)?;
    let (label, _) = crate::rebuild::identity(&workspace.project_dir(id)?);
    Ok(if label.is_empty() { record.name } else { label })
}

fn translate_import(workspace: &crate::workspace::Workspace, id: &str, dir: &std::path::Path) -> std::io::Result<super::godot::Report> {
    let mut record = workspace.load_record(id)?;
    // The source ID stays the import's (export finds the game by it); the name is the label.
    let source = project::slug(&record.name);
    record.name = game_name(workspace, id)?;
    let import_dir = workspace.project_dir(id)?;
    let analysis = crate::analyzer::load_or_analyze(workspace, id, false).map_err(|e| std::io::Error::other(e.to_string()))?;
    match analysis.engine.id.as_str() {
        "unity" => {
            let data = super::unity_scene::data_folder(&import_dir.join("unpacked")).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::Unsupported, "This Unity game's data files (globalgamemanagers, level0...) weren't found in the import.")
            })?;
            let exports = export_unity(&import_dir, &data)?;
            super::unity_scene::translate(&data, &super::unity_scene::Exports::scan(&exports), dir, &source, &record.name)
        }
        "godot" => {
            let project_dir = match godot_project(&import_dir.join("unpacked")).or_else(|| godot_project(&import_dir.join("recovered"))) {
                Some(d) => d,
                // An exported game (an APK, a .pck): packed and compiled, so it's recovered first.
                None => recover_godot(&import_dir)?,
            };
            super::godot::translate(&project_dir, dir, &source, &record.name)
        }
        // Every other engine: the generic reader (assets, things from sprite names, Tiled maps).
        _ => super::generic::translate(&import_dir.join("unpacked"), dir, &source, &record.name, &analysis.engine.name),
    }
}

/// Makes the layer for an imported game: reads how the game builds its
/// levels with the reader for its engine, and writes them as a GameX project.
#[tauri::command]
pub async fn gx_make_layer(app: AppHandle, state: tauri::State<'_, crate::commands::AppState>, id: String) -> Result<Translated, String> {
    let root = root(&app)?;
    let workspace = state.workspace();
    blocking(move || {
        let dir = project::create(&root, &game_name(&workspace, &id)?)?;
        match translate_import(&workspace, &id, &dir) {
            Ok(report) => {
                let link = LayerLink { path: dir.display().to_string() };
                std::fs::write(workspace.project_dir(&id)?.join(LAYER_LINK), serde_json::to_string_pretty(&link).map_err(std::io::Error::other)?)?;
                Ok(Translated { path: dir.display().to_string(), report })
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                Err(e)
            }
        }
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    pub merged: super::combine::Merged,
    pub report: super::godot::Report,
}

/// Combined Workspace: translates another imported game and adds it to the
/// layer at `path`, so things from both can be used together.
#[tauri::command]
pub async fn gx_add_game(app: AppHandle, state: tauri::State<'_, crate::commands::AppState>, path: String, id: String) -> Result<Added, String> {
    let root = root(&app)?;
    let workspace = state.workspace();
    blocking(move || {
        let record = workspace.load_record(&id)?;
        let into = PathBuf::from(&path);
        let source = project::slug(&record.name);
        if project::open(&into)?.info.sources.iter().any(|s| s.id == source) {
            return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("{} is already in this workspace.", record.name)));
        }
        // Translated on its own first, then merged in.
        let scratch = project::create(&root.join(".adding"), &game_name(&workspace, &id)?)?;
        let result = translate_import(&workspace, &id, &scratch).and_then(|report| Ok(Added { merged: super::combine::merge(&into, &scratch)?, report }));
        let _ = std::fs::remove_dir_all(&scratch);
        result
    })
    .await
}

/// The folder holding a Godot project in text form (project.godot), under dir.
fn godot_project(dir: &std::path::Path) -> Option<PathBuf> {
    WalkDir::new(dir)
        .max_depth(6)
        .into_iter()
        .filter_map(Result::ok)
        .find(|e| e.file_name() == "project.godot")
        .and_then(|e| e.path().parent().map(PathBuf::from))
}

/// Exports a Unity game's images and sounds with AssetStudio into
/// <import>/recovered-unity, one folder per data file, each file named by
/// the object it came from (done once; reused after). Unity keeps textures
/// in GPU formats, which AssetStudio decodes.
fn export_unity(import_dir: &std::path::Path, data: &std::path::Path) -> std::io::Result<PathBuf> {
    let out = import_dir.join("recovered-unity");
    let done = out.join(".done");
    if done.is_file() {
        return Ok(out);
    }
    let studio = crate::toolbox::installed("assetstudio").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "needs-tool:assetstudio|Unity games keep their images in GPU formats; AssetStudio exports them so GameX can show them.")
    })?;
    std::fs::create_dir_all(&out)?;
    let mut args: Vec<std::ffi::OsString> = vec![data.into()];
    for a in ["-m", "export", "-t", "tex2d,sprite,audio", "-g", "filename", "-f", "pathID", "--image-format", "png", "-r", "-o"] {
        args.push(a.into());
    }
    args.push(out.clone().into());
    let refs: Vec<&std::ffi::OsStr> = args.iter().map(|a| a.as_os_str()).collect();
    let result = crate::tools::run(&studio, &refs);
    let exported = WalkDir::new(&out).into_iter().filter_map(Result::ok).any(|e| e.file_type().is_file());
    if let (Err(e), false) = (&result, exported) {
        return Err(std::io::Error::other(format!("AssetStudio couldn't export this game's images: {e}")));
    }
    std::fs::write(&done, "")?;
    Ok(out)
}

/// Recovers an exported Godot game into <import>/recovered with GDRE Tools.
/// An error starting with needs-tool:gdre asks the window to offer installing it.
fn recover_godot(import_dir: &std::path::Path) -> std::io::Result<PathBuf> {
    let gdre = crate::toolbox::installed("gdre")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "needs-tool:gdre|Editing an exported Godot game needs GDRE Tools to recover it first."))?;
    let original = std::fs::read_dir(import_dir.join("original"))?
        .flatten()
        .next()
        .map(|e| e.path())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "The game's original copy is missing."))?;
    let out = import_dir.join("recovered");
    std::fs::create_dir_all(&out)?;
    let mut recover = std::ffi::OsString::from("--recover=");
    recover.push(&original);
    let mut output = std::ffi::OsString::from("--output=");
    output.push(&out);
    let result = crate::tools::run(&gdre, &[std::ffi::OsStr::new("--headless"), &recover, &output]);
    // Scenes and meshes still in Godot's binary form are converted to text,
    // so the reader can see them (best effort: whatever converts is used).
    if let Some(project) = godot_project(&out) {
        let binary: Vec<PathBuf> = WalkDir::new(&project)
            .into_iter()
            .filter_map(Result::ok)
            .map(|e| e.into_path())
            .filter(|p| {
                let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
                let twin = match ext.as_str() {
                    "scn" => "tscn",
                    "res" | "mesh" | "material" => "tres",
                    _ => return false,
                };
                !p.with_extension(twin).exists() && !p.components().any(|c| c.as_os_str() == ".godot" || c.as_os_str() == ".import")
            })
            .take(2000)
            .collect();
        // Converted files go beside the originals: one run per folder.
        let mut by_folder: std::collections::BTreeMap<PathBuf, Vec<PathBuf>> = Default::default();
        for f in binary {
            by_folder.entry(f.parent().map(PathBuf::from).unwrap_or_else(|| project.clone())).or_default().push(f);
        }
        for (folder, files) in by_folder {
            for chunk in files.chunks(40) {
                let mut args: Vec<std::ffi::OsString> = vec!["--headless".into()];
                for f in chunk {
                    let mut a = std::ffi::OsString::from("--bin-to-txt=");
                    a.push(f);
                    args.push(a);
                }
                let mut o = std::ffi::OsString::from("--output=");
                o.push(&folder);
                args.push(o);
                let refs: Vec<&std::ffi::OsStr> = args.iter().map(|a| a.as_os_str()).collect();
                let _ = crate::tools::run(&gdre, &refs);
            }
        }
    }
    // GDRE stops with an error on one bad file after recovering the rest.
    godot_project(&out).ok_or_else(|| {
        std::io::Error::other(format!(
            "GDRE Tools couldn't recover this game{}",
            result.err().map(|e| format!(": {e}")).unwrap_or_else(|| " (no project.godot came out).".into())
        ))
    })
}

/// Output A: writes the layer's changed assets into the imported game's
/// edits, ready for the usual Build.
#[tauri::command]
pub async fn gx_send_to_game(state: tauri::State<'_, crate::commands::AppState>, id: String) -> Result<super::export::Sent, String> {
    let workspace = state.workspace();
    blocking(move || {
        let import_dir = workspace.project_dir(&id)?;
        let text = std::fs::read_to_string(import_dir.join(LAYER_LINK)).map_err(|_| std::io::Error::other("This game has no visual layer yet."))?;
        let link: LayerLink = serde_json::from_str(&text).map_err(std::io::Error::other)?;
        let source = project::slug(&workspace.load_record(&id)?.name);
        super::export::send(std::path::Path::new(&link.path), &import_dir, &source)
    })
    .await
}

/// Replaces one of the layer's assets with a file the user picked.
#[tauri::command]
pub async fn gx_replace_asset(path: String, asset: String, file: String) -> Result<Option<String>, String> {
    blocking(move || {
        let dir = PathBuf::from(&path);
        super::export::replace_asset(&dir, &asset, std::path::Path::new(&file))?;
        History::new(&dir).last()
    })
    .await
}

/// Translates a Godot project folder into a new GameX project.
#[tauri::command]
pub async fn gx_translate_godot(app: AppHandle, folder: String) -> Result<Translated, String> {
    let root = root(&app)?;
    blocking(move || {
        let src = PathBuf::from(&folder);
        let name = super::godot::game_name(&src)
            .or_else(|| src.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Godot game".into());
        let dir = project::create(&root, &name)?;
        match super::godot::translate(&src, &dir, &project::slug(&name), &name) {
            Ok(report) => Ok(Translated { path: dir.display().to_string(), report }),
            Err(e) => {
                // A failed translation leaves no half-made project behind.
                let _ = std::fs::remove_dir_all(&dir);
                Err(e)
            }
        }
    })
    .await
}

#[tauri::command]
pub async fn gx_delete_scene(path: String, id: String, summary: String) -> Result<Option<String>, String> {
    blocking(move || {
        let dir = PathBuf::from(path);
        project::delete_scene(&dir, &id, &summary)?;
        History::new(&dir).last()
    })
    .await
}

/// Saves project.json (name, start scene...).
#[tauri::command]
pub async fn gx_save_info(path: String, info: ProjectInfo, summary: String) -> Result<Option<String>, String> {
    blocking(move || {
        let dir = PathBuf::from(path);
        project::save_info(&dir, &info, &summary)?;
        History::new(&dir).last()
    })
    .await
}

/// Takes back the newest change: (what was undone, what Undo would take back next).
#[tauri::command]
pub async fn gx_undo(path: String) -> Result<(Option<String>, Option<String>), String> {
    blocking(move || {
        let history = History::new(&PathBuf::from(path));
        let undone = history.undo()?;
        Ok((undone, history.last()?))
    })
    .await
}
