//! Output A: the layer's changes back into the game. Images, sounds and
//! other files replaced in the layer are written as the import's edits, on
//! the game file each came from (origin.json), so the usual Build makes a new
//! APK of the same game, on its own engine. This works where the game keeps
//! them as plain files (Android resources and assets, ZIP and folder
//! imports); files Unity or an exported Godot game packs into its own
//! formats are listed as not writable yet.

use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

use super::model::OriginMap;
use super::project;
use crate::edits::EditStore;

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    /// Game files written (as edits).
    pub written: Vec<String>,
    /// Assets the same as the game's.
    pub unchanged: usize,
    /// Assets changed in the layer that can't go back into the game yet, with why.
    pub not_writable: Vec<(String, String)>,
    /// Objects moved, turned, resized or hidden, written into the game's scene files.
    pub objects: usize,
}

/// A Godot project imported as files (a folder or ZIP): its folder, relative to `unpacked/`.
fn godot_files_project(unpacked: &Path) -> Option<String> {
    walkdir::WalkDir::new(unpacked.join("files"))
        .max_depth(6)
        .into_iter()
        .filter_map(Result::ok)
        .find(|e| e.file_name() == "project.godot")
        .and_then(|e| e.path().parent()?.strip_prefix(unpacked).ok().map(|p| p.to_string_lossy().replace('\\', "/")))
}

/// Where a layer asset's game file is in the import's `unpacked/`, if it's
/// one GameX rebuilds from: `apktool/<path>` for APKs, `files/<path>` otherwise.
fn game_path(unpacked: &Path, file: &str) -> Option<String> {
    ["apktool/", "files/"].iter().map(|root| format!("{root}{file}")).find(|rel| crate::edits::is_editable(rel) && unpacked.join(rel).is_file())
}

/// Writes every asset of the game `source` (a layer can hold several) that
/// differs from the game's file into that game's import edits.
pub fn send(layer: &Path, import_dir: &Path, source: &str) -> io::Result<Sent> {
    let info = project::open(layer)?.info;
    let unpacked = import_dir.join("unpacked");
    let store = EditStore::new(import_dir);
    let mut sent = Sent::default();
    for source in info.sources.iter().filter(|s| s.id == source) {
        // Godot projects kept as text: scene changes go back too.
        if source.engine == "godot" {
            if let Some(project_rel) = godot_files_project(&unpacked) {
                let rel_of = |res: &str| format!("{project_rel}/{}", res.trim_start_matches("res://"));
                let read = |res: &str| store.current(&rel_of(res)).ok().and_then(|p| fs::read_to_string(p).ok());
                let back = super::godot::write_back(layer, &source.id, &unpacked.join(&project_rel), &read)?;
                for (res, text) in back.files {
                    let rel = rel_of(&res);
                    store.save_text_as(&rel, &text, format!("From the visual layer: objects in {res}"))?;
                    sent.written.push(rel);
                }
                sent.objects += back.objects;
            }
        }
        let Ok(text) = fs::read_to_string(layer.join("sources").join(&source.id).join("origin.json")) else { continue };
        let Ok(origin) = serde_json::from_str::<OriginMap>(&text) else { continue };
        for (key, entry) in &origin.entries {
            let Some(asset) = key.strip_prefix("asset:") else { continue };
            let Ok(now) = fs::read(layer.join("assets").join(asset)) else { continue };
            // A Godot project's files are named res://...: inside its folder.
            let godot_file = entry
                .file
                .strip_prefix("res://")
                .zip(if source.engine == "godot" { godot_files_project(&unpacked) } else { None })
                .map(|(rest, project)| format!("{project}/{rest}"))
                .filter(|rel| crate::edits::is_editable(rel) && unpacked.join(rel).is_file());
            match godot_file.or_else(|| game_path(&unpacked, &entry.file)) {
                Some(rel) => {
                    // Compared with what the game has now (an earlier edit, or the original).
                    let current = store.current(&rel).ok().and_then(|p| fs::read(p).ok());
                    if current.as_deref() == Some(&now[..]) {
                        sent.unchanged += 1;
                        continue;
                    }
                    store.save_bytes(&rel, &now, format!("From the visual layer: {asset}"))?;
                    sent.written.push(rel);
                }
                None => {
                    // Only worth mentioning if it really changed: compare with the first copy kept.
                    if is_replaced(layer, asset) {
                        let why = match source.engine.as_str() {
                            "unity" => "kept in Unity's packed data files, which GameX can't write yet",
                            "godot" => "kept in the game's Godot pack (.pck), which GameX can't rebuild yet",
                            _ => "its game file wasn't found among the files GameX rebuilds",
                        };
                        sent.not_writable.push((asset.to_string(), why.to_string()));
                    } else {
                        sent.unchanged += 1;
                    }
                }
            }
        }
    }
    Ok(sent)
}

/// Where a replaced asset's first version is kept, beside the layer's assets.
fn original_copy(layer: &Path, asset: &str) -> std::path::PathBuf {
    layer.join("sources").join("replaced").join(asset)
}

fn is_replaced(layer: &Path, asset: &str) -> bool {
    original_copy(layer, asset).is_file()
}

/// Replaces a layer asset with another file (same kind), keeping its first
/// version so the change can be told apart and undone.
pub fn replace_asset(layer: &Path, asset: &str, file: &Path) -> io::Result<()> {
    if !crate::edits::is_plain_relative(asset) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid asset path"));
    }
    let target = layer.join("assets").join(asset);
    if !target.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "That asset isn't in the project."));
    }
    let ext = |p: &Path| p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let (old, new) = (ext(&target), ext(file));
    let same_kind = old == new || (["png", "jpg", "jpeg", "webp"].contains(&old.as_str()) && ["png", "jpg", "jpeg", "webp"].contains(&new.as_str()));
    if !same_kind {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("Pick a .{old} file to replace this asset (the game reads it as one).")));
    }
    let first = original_copy(layer, asset);
    if !first.is_file() {
        fs::create_dir_all(first.parent().unwrap_or(layer))?;
        fs::copy(&target, &first)?;
    }
    project::save_file(layer, &target, &fs::read(file)?, &format!("Replace {}", asset.rsplit('/').next().unwrap_or(asset)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gx::model::*;
    use std::collections::BTreeMap;

    /// A PNG signature, then `rest` (enough for the edit store's format check).
    fn png(rest: &[u8]) -> Vec<u8> {
        [&[0x89u8, b'P', b'N', b'G', 13, 10, 26, 10][..], rest].concat()
    }

    #[test]
    fn moved_objects_go_back_into_a_godot_project() {
        let dir = tempfile::tempdir().unwrap();
        let import = dir.path().join("import");
        let game = import.join("unpacked").join("files").join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("project.godot"), "config_version=5\n\n[application]\nrun/main_scene=\"res://main.tscn\"\n").unwrap();
        fs::write(
            game.join("main.tscn"),
            "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n\n[node name=\"Rock\" type=\"Node2D\" parent=\".\"]\nposition = Vector2(100, 50)\n\n[node name=\"Tree\" type=\"Node2D\" parent=\".\"]\nposition = Vector2(10, 10)\n",
        )
        .unwrap();
        let layer = project::create(&dir.path().join("p"), "Game").unwrap();
        crate::gx::godot::translate(&game, &layer, "g", "Game").unwrap();

        // Nothing moved: nothing written.
        let s = send(&layer, &import, "g").unwrap();
        assert_eq!((s.written.len(), s.objects), (0, 0), "{:?}", s.written);

        let mut scene = project::load_scene(&layer, "main").unwrap();
        let rock = scene.entities.iter_mut().find(|e| e.name == "Rock").unwrap();
        rock.transform.position = [3.0, -0.5, 0.0];
        project::save_scene(&layer, &scene, "Move Rock").unwrap();
        let s = send(&layer, &import, "g").unwrap();
        assert_eq!(s.written, ["files/game/main.tscn"]);
        assert_eq!(s.objects, 1);
        let text = fs::read_to_string(EditStore::new(&import).current("files/game/main.tscn").unwrap()).unwrap();
        assert!(text.contains("position = Vector2(300, 50)"), "{text}");
        assert!(text.contains("position = Vector2(10, 10)"));
        // Sent once: nothing new the second time.
        assert_eq!(send(&layer, &import, "g").unwrap().objects, 0);
    }

    #[test]
    fn replaced_assets_go_back_into_the_game() {
        let dir = tempfile::tempdir().unwrap();
        let import = dir.path().join("import");
        let res = import.join("unpacked").join("apktool").join("res").join("drawable");
        fs::create_dir_all(&res).unwrap();
        fs::write(res.join("hero.png"), png(b"old hero")).unwrap();
        fs::write(res.join("coin.png"), png(b"coin")).unwrap();

        let layer = project::create(&dir.path().join("p"), "Game").unwrap();
        let mut info = project::open(&layer).unwrap().info;
        info.sources.push(Source { id: "g".into(), name: "Game".into(), engine: "Java/Kotlin".into(), engine_version: None, imported_from: None });
        project::write_json(&layer.join(project::PROJECT_FILE), &info).unwrap();
        let mut entries = BTreeMap::new();
        for (asset, file) in [("textures/g/res/drawable/hero.png", "res/drawable/hero.png"), ("textures/g/res/drawable/coin.png", "res/drawable/coin.png"), ("textures/g/packed.png", "data.unity3d")] {
            entries.insert(format!("asset:{asset}"), Origin { file: file.into(), object: None, field: None, fidelity: Fidelity::Exact, note: None });
            let target = layer.join("assets").join(asset);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, fs::read(import.join("unpacked/apktool").join(file)).unwrap_or(b"packed".to_vec())).unwrap();
        }
        project::write_json(&layer.join("sources/g/origin.json"), &OriginMap { format: FORMAT, source: "g".into(), entries }).unwrap();

        // Nothing changed yet.
        let s = send(&layer, &import, "g").unwrap();
        assert_eq!((s.written.len(), s.unchanged, s.not_writable.len()), (0, 3, 0));

        let new = dir.path().join("new.png");
        fs::write(&new, png(b"new hero")).unwrap();
        replace_asset(&layer, "textures/g/res/drawable/hero.png", &new).unwrap();
        replace_asset(&layer, "textures/g/packed.png", &new).unwrap();
        assert!(replace_asset(&layer, "textures/g/res/drawable/coin.png", Path::new("x.ogg")).is_err());

        let s = send(&layer, &import, "g").unwrap();
        assert_eq!(s.written, ["apktool/res/drawable/hero.png"]);
        assert_eq!(s.not_writable.len(), 1);
        assert_eq!(fs::read(EditStore::new(&import).current("apktool/res/drawable/hero.png").unwrap()).unwrap(), png(b"new hero"));
        // Sent once: the second time it's the same as the game's.
        assert!(send(&layer, &import, "g").unwrap().written.is_empty());
        // The replacement is one undo step in the layer.
        assert_eq!(project::History::new(&layer).undo().unwrap().as_deref(), Some("Replace packed.png"));
    }
}
