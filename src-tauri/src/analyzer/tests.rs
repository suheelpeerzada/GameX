//! End-to-end analyzer tests on small synthetic games of different shapes.
//! Each test also checks that every file on disk lands in exactly one object.

use std::fs;
use std::path::Path;

use walkdir::WalkDir;

use super::*;
use crate::model::{ContainerKind, UnpackStep};

fn touch(root: &Path, rel: &str, data: &str) {
    let path = root.join("unpacked").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn record(container: ContainerKind, done: &[&str]) -> ImportRecord {
    ImportRecord {
        id: "test".into(),
        name: "Test".into(),
        container,
        source_path: String::new(),
        imported_at: 0,
        original_sha256: None,
        original_bytes: 0,
        unpacked_files: 0,
        steps: done
            .iter()
            .map(|id| UnpackStep {
                id: id.to_string(),
                label: id.to_string(),
                status: StepStatus::Done,
                note: None,
                output: None,
            })
            .collect(),
        workspace_path: String::new(),
    }
}

/// Rule 4: nothing is dropped and nothing is counted twice.
fn assert_every_file_once(root: &Path, project: &GameProject) {
    let unpacked = root.join("unpacked");
    let mut on_disk: Vec<String> = WalkDir::new(&unpacked)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            e.path()
                .strip_prefix(&unpacked)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    let mut in_objects: Vec<String> = project
        .objects
        .iter()
        .flat_map(|o| o.source_files.iter().cloned())
        .collect();
    on_disk.sort();
    in_objects.sort();
    assert_eq!(on_disk, in_objects);
}

fn object<'a>(project: &'a GameProject, id: &str) -> &'a GameObject {
    project
        .objects
        .iter()
        .find(|o| o.id == id)
        .unwrap_or_else(|| panic!("no object {id}; have {:?}", project.objects.iter().map(|o| &o.id).collect::<Vec<_>>()))
}

fn status(project: &GameProject, feature: &str) -> CapabilityStatus {
    project
        .capabilities
        .iter()
        .find(|c| c.feature == feature)
        .unwrap()
        .status
}

const MANIFEST: &str = r#"<manifest package="com.studio.game"><application>
<activity android:name=".MainActivity"><intent-filter>
<category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity>
</application></manifest>"#;

#[test]
fn native_android_apk() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    touch(r, "raw/AndroidManifest.xml", "\u{3}\u{0}binary");
    touch(r, "raw/classes.dex", "dex");
    touch(r, "raw/res/drawable-hdpi-v4/hero_idle1.png", "a");
    touch(r, "raw/lib/arm64-v8a/libsound.so", "so");
    touch(r, "raw/lib/armeabi-v7a/libsound.so", "so");
    touch(r, "apktool/AndroidManifest.xml", MANIFEST);
    touch(r, "apktool/original/AndroidManifest.xml", "\u{3}\u{0}binary");
    touch(r, "apktool/apktool.yml", "version: 2");
    touch(r, "apktool/res/drawable-hdpi/hero_idle1.png", "a");
    touch(r, "apktool/res/drawable-hdpi/hero_idle2.png", "a");
    touch(r, "apktool/res/drawable-xhdpi/hero_idle1.png", "a");
    touch(r, "apktool/res/raw/theme.ogg", "ogg");
    touch(r, "apktool/res/values/strings.xml", "<resources/>");
    touch(r, "apktool/res/values-de/strings.xml", "<resources/>");
    touch(r, "apktool/res/layout/main.xml", "<LinearLayout/>");
    touch(r, "apktool/smali/com/studio/game/Player.smali", ".class public Lcom/studio/game/Player;");
    touch(r, "apktool/smali/com/studio/game/Player$1.smali", ".class Lcom/studio/game/Player$1;");
    touch(r, "apktool/smali_classes2/com/google/ads/Ad.smali", ".class public Lcom/google/ads/Ad;");
    touch(
        r,
        "jadx/sources/com/studio/game/Player.java",
        "package com.studio.game;\npublic class Player {\n    float velocityX;\n    void jump() {\n    }\n}\n",
    );
    touch(r, "jadx/sources/com/studio/game/MainActivity.java", "public class MainActivity extends Activity {\n}\n");
    touch(r, "jadx/sources/com/studio/game/b.java", "class b {\n}\n");
    touch(r, "jadx/resources/res/drawable-hdpi/hero_idle1.png", "a");

    let p = analyze(&record(ContainerKind::Apk, &["raw", "apktool", "jadx"]), r);
    assert_eq!(p.engine.id, "android-native");
    assert_every_file_once(r, &p);

    let player = object(&p, "class:com.studio.game.Player");
    assert_eq!(player.category, Category::Characters);
    assert_eq!(player.source_files.len(), 3, "java + two smali files");
    assert!(player.properties.iter().any(|p| p.label == "Movement hints"));
    assert_eq!(object(&p, "class:com.studio.game.MainActivity").category, Category::Core);
    assert_eq!(object(&p, "class:com.studio.game.b").category, Category::Code);
    assert_eq!(object(&p, "class:com.google.ads.Ad").kind, "library class");

    let hero = object(&p, "set:res:drawable/hero_idle");
    assert_eq!((hero.category, hero.kind.as_str()), (Category::Characters, "image set"));
    // The sharpest variant comes first, so it is the one previewed.
    assert_eq!(hero.source_files[0], "apktool/res/drawable-xhdpi/hero_idle1.png");

    let strings = object(&p, "res:values/strings");
    assert_eq!(strings.category, Category::Ui);
    assert!(strings.properties.iter().any(|p| p.value == "default, de"));
    assert_eq!(object(&p, "res:raw/theme").kind, "music");
    assert_eq!(object(&p, "lib:libsound.so").source_files.len(), 2);
    assert_eq!(object(&p, "android:manifest").source_files[0], "apktool/AndroidManifest.xml");

    assert_eq!(status(&p, "code"), CapabilityStatus::Partial);
    assert_eq!(status(&p, "rebuilding"), CapabilityStatus::Supported);
    assert_eq!(status(&p, "movement"), CapabilityStatus::Partial);
}

#[test]
fn android_apk_without_decompilers() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    touch(r, "raw/AndroidManifest.xml", "\u{3}\u{0}binary");
    touch(r, "raw/classes.dex", "dex");
    let p = analyze(&record(ContainerKind::Apk, &["raw"]), r);
    assert_eq!(p.engine.id, "android-native");
    assert_every_file_once(r, &p);
    assert_eq!(status(&p, "code"), CapabilityStatus::Unsupported);
    assert_eq!(status(&p, "rebuilding"), CapabilityStatus::Unsupported);
}

#[test]
fn godot_apk() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    touch(r, "raw/AndroidManifest.xml", "\u{3}\u{0}binary");
    touch(r, "raw/lib/arm64-v8a/libgodot_android.so", "so");
    touch(r, "raw/assets/game.pck", "GDPC");
    touch(r, "raw/res/mipmap/icon.png", "png");
    let p = analyze(&record(ContainerKind::Apk, &["raw"]), r);
    assert_eq!(p.engine.id, "godot");
    assert_every_file_once(r, &p);
    assert_eq!(object(&p, "file:assets/game.pck").category, Category::Core);
    assert_eq!(status(&p, "code"), CapabilityStatus::Unsupported);
    assert_eq!(status(&p, "assets"), CapabilityStatus::Unsupported);
}

#[test]
fn godot_project_folder() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    touch(r, "files/project.godot", "[application]");
    touch(r, "files/player.gd", "extends CharacterBody2D");
    touch(r, "files/scenes/level_1.tscn", "[gd_scene]");
    touch(r, "files/scenes/thing.tscn", "[gd_scene]");
    touch(r, "files/art/sky.png", "png");
    let p = analyze(&record(ContainerKind::Folder, &["files"]), r);
    assert_eq!(p.engine.id, "godot");
    assert_every_file_once(r, &p);
    assert_eq!(object(&p, "file:player.gd").category, Category::Characters);
    assert_eq!(object(&p, "file:scenes/level_1.tscn").category, Category::Map);
    assert_eq!(object(&p, "file:scenes/thing.tscn").category, Category::Unsorted);
    assert_eq!(object(&p, "file:art/sky.png").category, Category::Map);
    assert_eq!(status(&p, "code"), CapabilityStatus::Supported);
    assert_eq!(status(&p, "visualEditing"), CapabilityStatus::Partial);
}

#[test]
fn unity_il2cpp_apk() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    touch(r, "raw/AndroidManifest.xml", "\u{3}\u{0}binary");
    touch(r, "raw/lib/arm64-v8a/libunity.so", "so");
    touch(r, "raw/lib/arm64-v8a/libil2cpp.so", "so");
    touch(r, "raw/assets/bin/Data/globalgamemanagers", "x");
    touch(r, "raw/assets/bin/Data/level0", "x");
    touch(r, "raw/assets/bin/Data/sharedassets0.assets.split0", "x");
    touch(r, "raw/assets/bin/Data/Managed/Metadata/global-metadata.dat", "x");
    let p = analyze(&record(ContainerKind::Apk, &["raw"]), r);
    assert_eq!(p.engine.id, "unity");
    assert_every_file_once(r, &p);
    assert_eq!(object(&p, "file:assets/bin/Data/level0").category, Category::Map);
    assert_eq!(object(&p, "file:assets/bin/Data/sharedassets0.assets.split0").category, Category::Assets);
    assert_eq!(status(&p, "code"), CapabilityStatus::Unsupported);
    assert_eq!(status(&p, "assets"), CapabilityStatus::Unsupported);
}

#[test]
fn unity_windows_folder() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    touch(r, "files/Game.exe", "MZ");
    touch(r, "files/UnityPlayer.dll", "MZ");
    touch(r, "files/Game_Data/Managed/Assembly-CSharp.dll", "MZ");
    touch(r, "files/Game_Data/Managed/UnityEngine.dll", "MZ");
    touch(r, "files/Game_Data/resources.assets", "x");
    let p = analyze(&record(ContainerKind::Folder, &["files"]), r);
    assert_eq!(p.engine.id, "unity");
    assert_every_file_once(r, &p);
    assert_eq!(object(&p, "file:Game_Data/Managed/Assembly-CSharp.dll").kind, "game scripts (C#)");
    assert_eq!(object(&p, "file:Game.exe").category, Category::Core);
    assert_eq!(status(&p, "code"), CapabilityStatus::Partial);
}

#[test]
fn loose_assets_zip_uses_generic() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    touch(r, "files/gfx/enemy_walk_1.png", "png");
    touch(r, "files/gfx/enemy_walk_2.png", "png");
    touch(r, "files/gfx/gem.png", "png");
    touch(r, "files/sfx/hit.wav", "wav");
    touch(r, "files/data/stuff.bin", "\u{0}\u{1}");
    touch(r, "files/scripts/main.lua", "print()");
    let p = analyze(&record(ContainerKind::Zip, &["files"]), r);
    assert_eq!(p.engine.id, "generic");
    assert_every_file_once(r, &p);
    assert_eq!(object(&p, "set:file:gfx/enemy_walk").category, Category::Characters);
    assert_eq!(object(&p, "file:gfx/gem.png").category, Category::Items);
    assert_eq!(object(&p, "file:sfx/hit.wav").category, Category::Audio);
    assert_eq!(object(&p, "file:data/stuff.bin").category, Category::Unsorted);
    assert_eq!(object(&p, "file:scripts/main.lua").category, Category::Core);
    assert_eq!(status(&p, "code"), CapabilityStatus::Partial);
    assert_eq!(status(&p, "assets"), CapabilityStatus::Supported);
}

#[test]
fn empty_import_still_analyzes() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("unpacked/files")).unwrap();
    let p = analyze(&record(ContainerKind::Folder, &["files"]), dir.path());
    assert_eq!(p.engine.id, "generic");
    assert!(p.objects.is_empty());
    assert_eq!(p.capabilities.len(), 6);
}

#[test]
fn manual_categories_survive_reanalysis() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::new(dir.path().join("ws"));
    let src = dir.path().join("game");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("blob.json"), "{}").unwrap();
    let settings = crate::settings::Settings {
        apktool_path: Some("/nonexistent".into()),
        jadx_path: Some("/nonexistent".into()),
        signer_path: None,
    };
    let rec = crate::import::import_game(&src, &ws, &settings, |_, _| {}).unwrap();

    let p = load_or_analyze(&ws, &rec.id, false).unwrap();
    assert_eq!(object(&p, "file:blob.json").category, Category::Unsorted);

    let mut overrides = ws.load_overrides(&rec.id).unwrap();
    overrides.insert("file:blob.json".into(), Category::Items);
    ws.save_overrides(&rec.id, &overrides).unwrap();

    for refresh in [false, true] {
        let p = load_or_analyze(&ws, &rec.id, refresh).unwrap();
        let blob = object(&p, "file:blob.json");
        assert_eq!((blob.category, blob.auto_category), (Category::Items, Category::Unsorted));
    }
}
