//! The Combined Workspace: a second game's layer merged into an open one,
//! so things from several games can be placed in the same scenes. The game
//! is translated on its own first (by the reader for its engine), then its
//! scenes, things, behaviors, assets and origin are copied in; IDs the
//! layer already uses get the game's source ID added, and every reference
//! to them follows.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;
use walkdir::WalkDir;

use super::model::*;
use super::project::{self, write_json};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Merged {
    pub source: String,
    pub scenes: usize,
    pub things: usize,
    pub assets: usize,
    /// IDs given a new name because the layer already had them: (old, new).
    pub renamed: Vec<(String, String)>,
}

/// `id`, or `id-<source>` (then `-2`, `-3`...) if `taken` has it.
fn free(id: &str, source: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(id) {
        return id.to_string();
    }
    let base = format!("{id}-{source}");
    let mut out = base.clone();
    let mut n = 2;
    while taken.contains(&out) {
        out = format!("{base}-{n}");
        n += 1;
    }
    out
}

struct Renames {
    things: HashMap<String, String>,
    behaviors: HashMap<String, String>,
}

impl Renames {
    fn value(&self, v: &mut Value) {
        if let Value::Thing(id) = v {
            if let Some(new) = self.things.get(id) {
                *id = new.clone();
            }
        }
    }

    fn uses(&self, list: &mut [BehaviorUse]) {
        for u in list {
            if let Some(new) = self.behaviors.get(&u.behavior) {
                u.behavior = new.clone();
            }
            u.params.values_mut().for_each(|v| self.value(v));
        }
    }

    fn entities(&self, list: &mut [Entity]) {
        for e in list {
            if let Some(new) = e.thing.as_ref().and_then(|t| self.things.get(t)) {
                e.thing = Some(new.clone());
            }
            self.uses(&mut e.behaviors);
            e.overrides.values_mut().for_each(|v| self.value(v));
            self.entities(&mut e.children);
        }
    }

    fn arg(&self, a: &mut Arg) {
        if let Arg::Value(v) = a {
            self.value(v);
        }
    }

    fn block(&self, b: &mut Block) {
        b.args.values_mut().for_each(|a| self.arg(a));
    }
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<usize> {
    let mut n = 0;
    if !from.is_dir() {
        return Ok(0);
    }
    for e in WalkDir::new(from).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file()) {
        let target = to.join(e.path().strip_prefix(from).unwrap_or(e.path()));
        if target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(e.path(), &target)?;
        n += 1;
    }
    Ok(n)
}

/// Copies the layer at `from` (one game, just translated) into the layer at `into`.
pub fn merge(into: &Path, from: &Path) -> io::Result<Merged> {
    let target = project::open(into)?;
    let incoming = project::open(from)?;
    let source = incoming.info.sources.first().ok_or_else(|| io::Error::other("The added game wasn't translated."))?.clone();
    if target.info.sources.iter().any(|s| s.id == source.id) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} is already in this workspace.", source.name)));
    }
    let mut merged = Merged { source: source.id.clone(), ..Default::default() };
    let mut renames = Renames { things: HashMap::new(), behaviors: HashMap::new() };

    let mut taken: HashSet<String> = target.things.iter().map(|t| t.id.clone()).collect();
    for t in &incoming.things {
        let id = free(&t.id, &source.id, &taken);
        taken.insert(id.clone());
        if id != t.id {
            merged.renamed.push((t.id.clone(), id.clone()));
            renames.things.insert(t.id.clone(), id);
        }
    }
    let mut taken: HashSet<String> = target.behaviors.iter().map(|b| b.id.clone()).collect();
    for b in &incoming.behaviors {
        let id = free(&b.id, &source.id, &taken);
        taken.insert(id.clone());
        if id != b.id {
            merged.renamed.push((b.id.clone(), id.clone()));
            renames.behaviors.insert(b.id.clone(), id);
        }
    }
    let mut scene_ids: HashMap<String, String> = HashMap::new();
    let mut taken: HashSet<String> = target.scenes.iter().map(|s| s.id.clone()).collect();
    for s in &incoming.scenes {
        let id = free(&s.id, &source.id, &taken);
        taken.insert(id.clone());
        if id != s.id {
            merged.renamed.push((s.id.clone(), id.clone()));
        }
        scene_ids.insert(s.id.clone(), id);
    }

    for t in &incoming.things {
        let mut thing = project::load_thing(from, &t.id)?;
        thing.id = renames.things.get(&t.id).cloned().unwrap_or(thing.id);
        renames.uses(&mut thing.behaviors);
        renames.entities(&mut thing.children);
        for p in thing.properties.iter_mut() {
            if let Value::Thing(id) = &mut p.value {
                if let Some(new) = renames.things.get(id) {
                    *id = new.clone();
                }
            }
        }
        write_json(&into.join("things").join(thing.kind.folder()).join(format!("{}.thing.json", thing.id)), &thing)?;
        merged.things += 1;
    }
    for b in &incoming.behaviors {
        let mut behavior = project::load_behavior(from, &b.id)?;
        behavior.id = renames.behaviors.get(&b.id).cloned().unwrap_or(behavior.id);
        for r in behavior.rules.iter_mut() {
            renames.block(&mut r.when);
            r.conditions.iter_mut().chain(r.then.iter_mut()).for_each(|b| renames.block(b));
        }
        write_json(&into.join("behavior").join(format!("{}.blocks.json", behavior.id)), &behavior)?;
    }
    for s in &incoming.scenes {
        let mut scene = project::load_scene(from, &s.id)?;
        scene.id = scene_ids[&s.id].clone();
        renames.entities(&mut scene.entities);
        write_json(&into.join("world").join(format!("{}.scene.json", scene.id)), &scene)?;
        merged.scenes += 1;
    }

    merged.assets = copy_tree(&from.join("assets"), &into.join("assets"))?;
    copy_tree(&from.join("sources").join(&source.id), &into.join("sources").join(&source.id))?;
    // Origin entries follow the new IDs.
    let origin_file = into.join("sources").join(&source.id).join("origin.json");
    if let Ok(text) = fs::read_to_string(&origin_file) {
        if let Ok(mut origin) = serde_json::from_str::<OriginMap>(&text) {
            let entries: BTreeMap<String, Origin> = std::mem::take(&mut origin.entries)
                .into_iter()
                .map(|(k, v)| {
                    let key = match k.split_once(':') {
                        Some(("thing", id)) => renames.things.get(id).map(|n| format!("thing:{n}")),
                        Some(("scene", id)) => scene_ids.get(id).map(|n| format!("scene:{n}")),
                        _ => None,
                    };
                    (key.unwrap_or(k), v)
                })
                .collect();
            origin.entries = entries;
            write_json(&origin_file, &origin)?;
        }
    }
    let mut info = target.info;
    info.sources.push(source);
    if info.settings.start_scene.is_none() {
        info.settings.start_scene = incoming.info.settings.start_scene.as_ref().and_then(|s| scene_ids.get(s)).cloned();
    }
    write_json(&into.join(project::PROJECT_FILE), &info)?;
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(root: &Path, name: &str, source: &str) -> std::path::PathBuf {
        let dir = project::create(root, name).unwrap();
        let mut info = project::open(&dir).unwrap().info;
        info.sources.push(Source { id: source.into(), name: name.into(), engine: "godot".into(), engine_version: None, imported_from: None });
        info.settings.start_scene = Some("main".into());
        write_json(&dir.join(project::PROJECT_FILE), &info).unwrap();
        let player = Thing { tags: Vec::new(), format: FORMAT, id: "player".into(), name: format!("{name} player"), kind: ThingKind::Character, role: Some("player".into()), description: None, properties: Vec::new(), parts: Vec::new(), behaviors: Vec::new(), children: Vec::new() };
        write_json(&dir.join("things/characters/player.thing.json"), &player).unwrap();
        let mut e = Entity::new("p1", "Player");
        e.thing = Some("player".into());
        let scene = Scene { format: FORMAT, id: "main".into(), name: "Main".into(), dimensions: Dimensions::TwoD, environment: Environment::default(), entities: vec![e] };
        write_json(&dir.join("world/main.scene.json"), &scene).unwrap();
        let asset = dir.join("assets").join("textures").join(source).join("p.png");
        fs::create_dir_all(asset.parent().unwrap()).unwrap();
        fs::write(&asset, b"x").unwrap();
        let mut entries = BTreeMap::new();
        entries.insert("thing:player".to_string(), Origin { file: "player.tscn".into(), object: None, field: None, fidelity: Fidelity::Exact, note: None });
        write_json(&dir.join("sources").join(source).join("origin.json"), &OriginMap { format: FORMAT, source: source.into(), entries }).unwrap();
        dir
    }

    #[test]
    fn a_second_game_joins_with_clashing_ids_renamed() {
        let root = tempfile::tempdir().unwrap();
        let a = layer(root.path(), "Egg", "egg");
        let b = layer(root.path(), "Dodge", "dodge");
        let m = merge(&a, &b).unwrap();
        assert_eq!((m.scenes, m.things, m.assets), (1, 1, 1));
        assert!(m.renamed.contains(&("player".into(), "player-dodge".into())));

        let index = project::open(&a).unwrap();
        assert!(index.problems.is_empty(), "{:?}", index.problems);
        assert_eq!(index.info.sources.len(), 2);
        assert_eq!(index.info.settings.start_scene.as_deref(), Some("main"));
        let added = project::load_scene(&a, "main-dodge").unwrap();
        assert_eq!(added.entities[0].thing.as_deref(), Some("player-dodge"));
        assert_eq!(project::load_thing(&a, "player").unwrap().name, "Egg player");
        let origin: OriginMap = serde_json::from_str(&fs::read_to_string(a.join("sources/dodge/origin.json")).unwrap()).unwrap();
        assert!(origin.entries.contains_key("thing:player-dodge"));
        assert!(a.join("assets/textures/dodge/p.png").is_file());

        // The same game twice is refused.
        assert!(merge(&a, &b).is_err());
    }
}
