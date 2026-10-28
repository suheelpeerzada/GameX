//! GameX projects on disk: a folder of readable JSON files (see model.rs).
//! Creating, opening, saving with undo, and checking that everything a
//! project refers to is there.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::model::*;

pub const PROJECT_FILE: &str = "project.json";
const SCENE_EXT: &str = ".scene.json";
const THING_EXT: &str = ".thing.json";
const BLOCKS_EXT: &str = ".blocks.json";
const SCRIPT_EXT: &str = ".gxs";
const ASSET_FOLDERS: &[&str] = &["models", "textures", "sprites", "animations", "audio", "fonts"];
const THING_FOLDERS: &[ThingKind] = &[ThingKind::Character, ThingKind::Item, ThingKind::Tool, ThingKind::Prop, ThingKind::Other];

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// `Documents/GameX/Projects`.
pub fn projects_root(documents: &Path) -> PathBuf {
    documents.join("GameX").join("Projects")
}

/// IDs name files and link them, so they stay simple: lowercase letters,
/// digits, `-` and `_`, at most 64.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// "Haunted House 2!" -> "haunted-house-2"
pub fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    let s: String = s.chars().take(48).collect();
    if s.is_empty() {
        "item".into()
    } else {
        s
    }
}

/// A short random suffix for new IDs.
pub fn new_id(prefix: &str) -> String {
    format!("{}-{}", slug(prefix), &uuid::Uuid::new_v4().simple().to_string()[..6])
}

fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| invalid(format!("{}: {e}", path.display())))
}

/// Writes pretty JSON through a temporary file, so a crash never leaves half a file.
pub(super) fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    text.push('\n');
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

fn check_format(found: u32, what: &str) -> io::Result<()> {
    if found > FORMAT {
        return Err(invalid(format!(
            "{what} was made by a newer GameX (format {found}; this GameX reads up to {FORMAT}). Update GameX to open it."
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Creating and listing

/// Makes a new, empty project folder under `root` and returns its path.
pub fn create(root: &Path, name: &str) -> io::Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        return Err(invalid("Give the project a name."));
    }
    // A readable folder name; a number is added if it's taken.
    let base: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let base = base.trim_end_matches(['.', ' ']).to_string();
    let mut dir = root.join(&base);
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{base} ({n})"));
        n += 1;
    }
    fs::create_dir_all(&dir)?;
    for sub in ["world", "behavior", "sources", "history"] {
        fs::create_dir_all(dir.join(sub))?;
    }
    for kind in THING_FOLDERS {
        fs::create_dir_all(dir.join("things").join(kind.folder()))?;
    }
    for sub in ASSET_FOLDERS {
        fs::create_dir_all(dir.join("assets").join(sub))?;
    }
    let info = ProjectInfo { format: FORMAT, id: new_id(name), name: name.to_string(), sources: Vec::new(), settings: ProjectSettings::default() };
    write_json(&dir.join(PROJECT_FILE), &info)?;
    Ok(dir)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub path: String,
    pub name: String,
    pub sources: usize,
    pub modified: u64,
}

/// The projects in `root`, most recently changed first.
pub fn list(root: &Path) -> io::Result<Vec<ProjectEntry>> {
    let mut out = Vec::new();
    let Ok(dirs) = fs::read_dir(root) else { return Ok(out) };
    for dir in dirs.flatten() {
        let path = dir.path();
        let Ok(info) = read_json::<ProjectInfo>(&path.join(PROJECT_FILE)) else { continue };
        let modified = fs::metadata(path.join(PROJECT_FILE))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        out.push(ProjectEntry { path: path.display().to_string(), name: info.name, sources: info.sources.len(), modified });
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.modified));
    Ok(out)
}

// ---------------------------------------------------------------------------
// Opening: an index of everything in the project

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub name: String,
    /// Relative to the project folder, with `/`.
    pub file: String,
    /// Thing kind, scene dimensions...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub file: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIndex {
    pub path: String,
    pub info: ProjectInfo,
    pub scenes: Vec<Item>,
    pub things: Vec<Item>,
    pub behaviors: Vec<Item>,
    pub scripts: Vec<String>,
    pub assets: Vec<String>,
    /// Anything wrong: unreadable files, broken links. Empty when all is well.
    pub problems: Vec<Problem>,
    /// What Undo would take back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo: Option<String>,
}

fn rel(dir: &Path, path: &Path) -> String {
    path.strip_prefix(dir).unwrap_or(path).components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

fn files_ending(dir: &Path, sub: &str, ext: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = WalkDir::new(dir.join(sub))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.file_name().to_string_lossy().ends_with(ext))
        .map(|e| e.into_path())
        .collect();
    out.sort();
    out
}

/// Reads a project folder: what's in it, and what's wrong with it.
pub fn open(dir: &Path) -> io::Result<ProjectIndex> {
    let info: ProjectInfo = read_json(&dir.join(PROJECT_FILE)).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => invalid(format!("{} isn't a GameX project (no {PROJECT_FILE}).", dir.display())),
        _ => e,
    })?;
    check_format(info.format, "This project")?;
    let mut problems = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();

    let mut scenes = Vec::new();
    let mut scene_data = Vec::new();
    for path in files_ending(dir, "world", SCENE_EXT) {
        let file = rel(dir, &path);
        match read_json::<Scene>(&path).and_then(|s| check_format(s.format, &file).map(|_| s)) {
            Ok(s) => {
                scenes.push(Item {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    file: file.clone(),
                    kind: Some(if s.dimensions == Dimensions::TwoD { "2d" } else { "3d" }.into()),
                });
                scene_data.push((file, s));
            }
            Err(e) => problems.push(Problem { file, message: e.to_string() }),
        }
    }

    let mut things = Vec::new();
    let mut thing_data = Vec::new();
    for path in files_ending(dir, "things", THING_EXT) {
        let file = rel(dir, &path);
        match read_json::<Thing>(&path).and_then(|t| check_format(t.format, &file).map(|_| t)) {
            Ok(t) => {
                things.push(Item {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    file: file.clone(),
                    kind: Some(t.kind.folder().trim_end_matches('s').to_string()),
                });
                thing_data.push((file, t));
            }
            Err(e) => problems.push(Problem { file, message: e.to_string() }),
        }
    }

    let mut behaviors = Vec::new();
    let mut behavior_data = Vec::new();
    for path in files_ending(dir, "behavior", BLOCKS_EXT) {
        let file = rel(dir, &path);
        match read_json::<Behavior>(&path).and_then(|b| check_format(b.format, &file).map(|_| b)) {
            Ok(b) => {
                behaviors.push(Item { id: b.id.clone(), name: b.name.clone(), file: file.clone(), kind: None });
                behavior_data.push((file, b));
            }
            Err(e) => problems.push(Problem { file, message: e.to_string() }),
        }
    }
    let scripts: Vec<String> = files_ending(dir, "behavior", SCRIPT_EXT).iter().map(|p| rel(dir, p)).collect();
    let assets: Vec<String> = WalkDir::new(dir.join("assets"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| rel(&dir.join("assets"), e.path()))
        .collect();

    // IDs: valid, and each used once per kind of file.
    for (kind, items) in [("scene", &scenes), ("thing", &things), ("behavior", &behaviors)] {
        ids.clear();
        for item in items {
            if !valid_id(&item.id) {
                problems.push(Problem { file: item.file.clone(), message: format!("The {kind} ID \"{}\" may only use a-z, 0-9, - and _.", item.id) });
            }
            if !ids.insert(item.id.clone()) {
                problems.push(Problem { file: item.file.clone(), message: format!("Another {kind} already has the ID \"{}\".", item.id) });
            }
        }
    }

    // Links: things, behaviors and assets that are referred to must exist.
    let thing_ids: HashSet<&str> = things.iter().map(|t| t.id.as_str()).collect();
    let behavior_ids: HashSet<&str> = behaviors.iter().map(|b| b.id.as_str()).collect();
    let asset_set: HashSet<&str> = assets.iter().map(String::as_str).collect();
    let check_parts = |file: &str, owner: &str, parts: &[Part], uses: &[BehaviorUse], problems: &mut Vec<Problem>| {
        for p in parts {
            for a in p.assets() {
                if !asset_set.contains(a) {
                    problems.push(Problem { file: file.into(), message: format!("{owner} uses assets/{a}, which isn't there.") });
                }
            }
        }
        for u in uses {
            if !behavior_ids.contains(u.behavior.as_str()) {
                problems.push(Problem { file: file.into(), message: format!("{owner} uses the behavior \"{}\", which isn't there.", u.behavior) });
            }
        }
    };
    for (file, s) in &scene_data {
        let mut entity_ids = HashSet::new();
        for e in s.entities.iter().flat_map(Entity::walk) {
            let owner = format!("\"{}\"", e.name);
            if !entity_ids.insert(e.id.as_str()) {
                problems.push(Problem { file: file.clone(), message: format!("Two entities share the ID \"{}\".", e.id) });
            }
            if let Some(t) = &e.thing {
                if !thing_ids.contains(t.as_str()) {
                    problems.push(Problem { file: file.clone(), message: format!("{owner} is a \"{t}\", which isn't in things/.") });
                }
            }
            check_parts(file, &owner, &e.parts, &e.behaviors, &mut problems);
        }
    }
    for (file, t) in &thing_data {
        let owner = format!("\"{}\"", t.name);
        check_parts(file, &owner, &t.parts, &t.behaviors, &mut problems);
        for e in t.children.iter().flat_map(Entity::walk) {
            check_parts(file, &owner, &e.parts, &e.behaviors, &mut problems);
        }
    }
    for (file, b) in &behavior_data {
        for r in &b.rules {
            if let Some(state) = &r.state {
                if !b.states.contains(state) {
                    problems.push(Problem { file: file.clone(), message: format!("A rule uses the state \"{state}\", which the behavior doesn't list.") });
                }
            }
        }
    }
    if let Some(start) = &info.settings.start_scene {
        if !scenes.iter().any(|s| &s.id == start) {
            problems.push(Problem { file: PROJECT_FILE.into(), message: format!("The start scene \"{start}\" isn't in world/.") });
        }
    }

    Ok(ProjectIndex {
        path: dir.display().to_string(),
        info,
        scenes,
        things,
        behaviors,
        scripts,
        assets,
        problems,
        undo: History::new(dir).last()?,
    })
}

// ---------------------------------------------------------------------------
// Loading and saving one file

/// The file a scene, thing or behavior with this ID is stored in.
fn find(dir: &Path, sub: &str, ext: &str, id: &str) -> io::Result<PathBuf> {
    let wanted = format!("{id}{ext}");
    files_ending(dir, sub, ext)
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy() == wanted))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("\"{id}\" isn't in {sub}/.")))
}

pub fn load_scene(dir: &Path, id: &str) -> io::Result<Scene> {
    let s: Scene = read_json(&find(dir, "world", SCENE_EXT, id)?)?;
    check_format(s.format, "This scene")?;
    Ok(s)
}

pub fn load_thing(dir: &Path, id: &str) -> io::Result<Thing> {
    let t: Thing = read_json(&find(dir, "things", THING_EXT, id)?)?;
    check_format(t.format, "This thing")?;
    Ok(t)
}

pub fn load_behavior(dir: &Path, id: &str) -> io::Result<Behavior> {
    let b: Behavior = read_json(&find(dir, "behavior", BLOCKS_EXT, id)?)?;
    check_format(b.format, "This behavior")?;
    Ok(b)
}

fn check_id(id: &str) -> io::Result<()> {
    if valid_id(id) {
        Ok(())
    } else {
        Err(invalid(format!("The ID \"{id}\" may only use a-z, 0-9, - and _.")))
    }
}

/// Saves with an undo step. `summary` names the change ("Move Crate").
pub fn save_scene(dir: &Path, scene: &Scene, summary: &str) -> io::Result<()> {
    check_id(&scene.id)?;
    let mut seen = HashSet::new();
    for e in scene.entities.iter().flat_map(Entity::walk) {
        if !seen.insert(e.id.as_str()) {
            return Err(invalid(format!("Two entities share the ID \"{}\".", e.id)));
        }
    }
    let path = find(dir, "world", SCENE_EXT, &scene.id).unwrap_or_else(|_| dir.join("world").join(format!("{}{SCENE_EXT}", scene.id)));
    History::new(dir).save(&path, summary, &Scene { format: FORMAT, ..scene.clone() })
}

pub fn save_thing(dir: &Path, thing: &Thing, summary: &str) -> io::Result<()> {
    check_id(&thing.id)?;
    let folder = dir.join("things").join(thing.kind.folder());
    let path = folder.join(format!("{}{THING_EXT}", thing.id));
    // A thing whose kind changed moves to its new folder.
    if let Ok(old) = find(dir, "things", THING_EXT, &thing.id) {
        if old != path {
            let history = History::new(dir);
            history.save(&path, summary, &Thing { format: FORMAT, ..thing.clone() })?;
            return history.remove(&old, summary);
        }
    }
    History::new(dir).save(&path, summary, &Thing { format: FORMAT, ..thing.clone() })
}

pub fn save_behavior(dir: &Path, behavior: &Behavior, summary: &str) -> io::Result<()> {
    check_id(&behavior.id)?;
    let path = find(dir, "behavior", BLOCKS_EXT, &behavior.id).unwrap_or_else(|_| dir.join("behavior").join(format!("{}{BLOCKS_EXT}", behavior.id)));
    History::new(dir).save(&path, summary, &Behavior { format: FORMAT, ..behavior.clone() })
}

/// Deletes a scene (undoable). If it was the start scene, the project then has none.
/// Writes a file in the project (an asset) as one undoable step.
pub fn save_file(dir: &Path, path: &Path, bytes: &[u8], summary: &str) -> io::Result<()> {
    let history = History::new(dir);
    history.keep(path, summary, history.next_group()?)?;
    fs::write(path, bytes)
}

pub fn delete_scene(dir: &Path, id: &str, summary: &str) -> io::Result<()> {
    let path = find(dir, "world", SCENE_EXT, id)?;
    let history = History::new(dir);
    let mut info: ProjectInfo = read_json(&dir.join(PROJECT_FILE))?;
    if info.settings.start_scene.as_deref() == Some(id) {
        info.settings.start_scene = None;
        history.save(&dir.join(PROJECT_FILE), summary, &info)?;
        return history.remove(&path, summary);
    }
    history.keep(&path, summary, history.next_group()?)?;
    fs::remove_file(path)
}

pub fn save_info(dir: &Path, info: &ProjectInfo, summary: &str) -> io::Result<()> {
    History::new(dir).save(&dir.join(PROJECT_FILE), summary, &ProjectInfo { format: FORMAT, ..info.clone() })
}

// ---------------------------------------------------------------------------
// Undo: each save keeps what it replaced in history/

#[derive(Debug, Serialize, Deserialize)]
struct Step {
    /// Relative to the project folder.
    file: String,
    /// The file's previous contents (in history/), or none if it didn't exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<String>,
    summary: String,
    at: u64,
    /// Steps saved together are undone together.
    group: u64,
}

pub struct History {
    dir: PathBuf,
}

impl History {
    pub fn new(project: &Path) -> Self {
        History { dir: project.to_path_buf() }
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join("history").join("log.json")
    }

    fn load(&self) -> io::Result<Vec<Step>> {
        match fs::read_to_string(self.log_path()) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| invalid(format!("history/log.json: {e}"))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    fn keep(&self, path: &Path, summary: &str, group: u64) -> io::Result<()> {
        let mut log = self.load()?;
        let before = if path.is_file() {
            let name = format!("{}.json", uuid::Uuid::new_v4().simple());
            fs::create_dir_all(self.dir.join("history"))?;
            fs::copy(path, self.dir.join("history").join(&name))?;
            Some(name)
        } else {
            None
        };
        let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        log.push(Step { file: rel(&self.dir, path), before, summary: summary.into(), at, group });
        write_json(&self.log_path(), &log)
    }

    fn next_group(&self) -> io::Result<u64> {
        Ok(self.load()?.last().map_or(1, |s| s.group + 1))
    }

    fn save<T: Serialize>(&self, path: &Path, summary: &str, value: &T) -> io::Result<()> {
        self.keep(path, summary, self.next_group()?)?;
        write_json(path, value)
    }

    /// Removes a file as part of the change just saved (same undo step).
    fn remove(&self, path: &Path, summary: &str) -> io::Result<()> {
        let group = self.load()?.last().map_or(1, |s| s.group);
        self.keep(path, summary, group)?;
        fs::remove_file(path)
    }

    /// What Undo would take back.
    pub fn last(&self) -> io::Result<Option<String>> {
        Ok(self.load()?.pop().map(|s| s.summary))
    }

    /// Takes back the newest change. Returns its summary, or None if there's nothing to undo.
    pub fn undo(&self) -> io::Result<Option<String>> {
        let mut log = self.load()?;
        let Some(group) = log.last().map(|s| s.group) else { return Ok(None) };
        let mut summary = None;
        while log.last().is_some_and(|s| s.group == group) {
            let step = log.pop().unwrap();
            let path = self.dir.join(&step.file);
            match &step.before {
                Some(name) => {
                    let snap = self.dir.join("history").join(name);
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::copy(&snap, &path)?;
                    let _ = fs::remove_file(snap);
                }
                None => match fs::remove_file(&path) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                },
            }
            summary = Some(step.summary);
        }
        write_json(&self.log_path(), &log)?;
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn scene(id: &str, entities: Vec<Entity>) -> Scene {
        Scene { format: FORMAT, id: id.into(), name: id.into(), dimensions: Dimensions::ThreeD, environment: Environment::default(), entities }
    }

    fn crate_thing() -> Thing {
        Thing { tags: Vec::new(),
            format: FORMAT,
            id: "crate".into(),
            name: "Crate".into(),
            kind: ThingKind::Prop,
            role: None,
            description: None,
            properties: vec![],
            parts: vec![Part::Shape { shape: ShapeKind::Box, size: [1.0; 3], color: "#a0703c".into() }],
            behaviors: vec![],
            children: vec![],
        }
    }

    #[test]
    fn creates_lists_and_opens_projects() {
        let root = tempfile::tempdir().unwrap();
        let a = create(root.path(), "My Game").unwrap();
        let b = create(root.path(), "My Game").unwrap();
        assert_eq!(a.file_name().unwrap(), "My Game");
        assert_eq!(b.file_name().unwrap(), "My Game (2)");
        assert!(a.join("things/characters").is_dir() && a.join("assets/models").is_dir());
        assert_eq!(list(root.path()).unwrap().len(), 2);
        let index = open(&a).unwrap();
        assert_eq!(index.info.name, "My Game");
        assert!(index.problems.is_empty() && index.scenes.is_empty() && index.undo.is_none());
        assert!(open(root.path()).unwrap_err().to_string().contains("isn't a GameX project"));
    }

    #[test]
    fn saves_and_undoes_changes() {
        let root = tempfile::tempdir().unwrap();
        let dir = create(root.path(), "P").unwrap();
        save_thing(&dir, &crate_thing(), "Add Crate").unwrap();
        let mut box1 = Entity::new("crate-1", "Crate");
        box1.thing = Some("crate".into());
        save_scene(&dir, &scene("yard", vec![box1.clone()]), "Add Yard").unwrap();

        let mut moved = load_scene(&dir, "yard").unwrap();
        moved.entities[0].transform.position = [2.0, 0.0, 0.0];
        save_scene(&dir, &moved, "Move Crate").unwrap();
        assert_eq!(open(&dir).unwrap().undo.as_deref(), Some("Move Crate"));

        let history = History::new(&dir);
        assert_eq!(history.undo().unwrap().as_deref(), Some("Move Crate"));
        assert_eq!(load_scene(&dir, "yard").unwrap().entities[0].transform.position, [0.0; 3]);
        assert_eq!(history.undo().unwrap().as_deref(), Some("Add Yard"));
        assert!(load_scene(&dir, "yard").is_err());
        assert_eq!(history.undo().unwrap().as_deref(), Some("Add Crate"));
        assert_eq!(history.undo().unwrap(), None);
        assert!(open(&dir).unwrap().things.is_empty());
    }

    #[test]
    fn a_thing_that_changes_kind_moves_folder_in_one_step() {
        let root = tempfile::tempdir().unwrap();
        let dir = create(root.path(), "P").unwrap();
        save_thing(&dir, &crate_thing(), "Add Crate").unwrap();
        save_thing(&dir, &Thing { kind: ThingKind::Item, ..crate_thing() }, "Crate is an item").unwrap();
        assert!(dir.join("things/items/crate.thing.json").is_file());
        assert!(!dir.join("things/props/crate.thing.json").exists());
        History::new(&dir).undo().unwrap();
        assert!(dir.join("things/props/crate.thing.json").is_file());
        assert!(!dir.join("things/items/crate.thing.json").exists());
    }

    #[test]
    fn deleting_the_start_scene_is_one_undoable_step() {
        let root = tempfile::tempdir().unwrap();
        let dir = create(root.path(), "P").unwrap();
        save_scene(&dir, &scene("a", vec![]), "Add A").unwrap();
        let mut info = open(&dir).unwrap().info;
        info.settings.start_scene = Some("a".into());
        save_info(&dir, &info, "Start with A").unwrap();
        delete_scene(&dir, "a", "Delete A").unwrap();
        let index = open(&dir).unwrap();
        assert!(index.scenes.is_empty() && index.info.settings.start_scene.is_none() && index.problems.is_empty());
        assert_eq!(History::new(&dir).undo().unwrap().as_deref(), Some("Delete A"));
        let index = open(&dir).unwrap();
        assert_eq!(index.scenes.len(), 1);
        assert_eq!(index.info.settings.start_scene.as_deref(), Some("a"));
    }

    #[test]
    fn reports_broken_links_and_bad_ids() {
        let root = tempfile::tempdir().unwrap();
        let dir = create(root.path(), "P").unwrap();
        let mut ghost = Entity::new("g", "Ghost");
        ghost.thing = Some("ghost".into());
        ghost.parts.push(Part::Model { asset: "models/ghost.glb".into() });
        ghost.behaviors.push(BehaviorUse { behavior: "haunt".into(), params: BTreeMap::new() });
        save_scene(&dir, &scene("hall", vec![ghost]), "Add Hall").unwrap();
        let problems: Vec<String> = open(&dir).unwrap().problems.into_iter().map(|p| p.message).collect();
        assert!(problems.iter().any(|m| m.contains("\"ghost\", which isn't in things/")), "{problems:?}");
        assert!(problems.iter().any(|m| m.contains("assets/models/ghost.glb")));
        assert!(problems.iter().any(|m| m.contains("behavior \"haunt\"")));

        // Refused before anything is written.
        assert!(save_scene(&dir, &scene("Bad Id", vec![]), "x").is_err());
        let twins = vec![Entity::new("a", "A"), Entity::new("a", "B")];
        assert!(save_scene(&dir, &scene("twins", twins), "x").unwrap_err().to_string().contains("share the ID"));
    }

    #[test]
    fn refuses_files_from_a_newer_gamex() {
        let root = tempfile::tempdir().unwrap();
        let dir = create(root.path(), "P").unwrap();
        fs::write(dir.join("world/next.scene.json"), r#"{"format": 99, "id": "next", "name": "Next"}"#).unwrap();
        let index = open(&dir).unwrap();
        assert!(index.problems[0].message.contains("newer GameX"));
        assert!(index.scenes.is_empty());
    }

    #[test]
    fn ids_and_slugs() {
        assert!(valid_id("enemy_2") && valid_id("level-1"));
        assert!(!valid_id("Enemy") && !valid_id("a/b") && !valid_id(""));
        assert_eq!(slug("Haunted House 2!"), "haunted-house-2");
        assert!(valid_id(&new_id("Big Crate")));
    }
}
