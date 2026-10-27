//! Unity games, on Android (`libunity.so`, `assets/bin/Data/`) or desktop
//! (`UnityPlayer.dll`, `<Game>_Data/`).

use crate::model::{Capability, CapabilityStatus, Category, GameObject};

use super::classify::{file_props, Builder, Placement};
use super::tree::FileTree;
use super::{android, cap, characters_capability, movement_capability, Context, Detection, EngineIntegration};

pub struct Unity;

const ENGINE_FILES: &[&str] = &[
    "globalgamemanagers",
    "globalgamemanagers.assets",
    "mainData",
    "unity default resources",
    "unity_builtin_extra",
    "data.unity3d",
    "boot.config",
    "RuntimeInitializeOnLoads.json",
    "ScriptingAssemblies.json",
];

impl EngineIntegration for Unity {
    fn id(&self) -> &'static str {
        "unity"
    }

    fn name(&self) -> &'static str {
        "Unity"
    }

    fn detect(&self, tree: &FileTree) -> Detection {
        const MARKERS: &[(&str, &str)] = &[
            ("libunity.so", "libunity.so: Unity's Android runtime"),
            ("libil2cpp.so", "libil2cpp.so: scripts compiled with IL2CPP"),
            ("UnityPlayer.dll", "UnityPlayer.dll: Unity's Windows runtime"),
            ("globalgamemanagers", "globalgamemanagers: Unity engine settings"),
            ("data.unity3d", "data.unity3d: Unity's bundled game data"),
            ("Assembly-CSharp.dll", "Assembly-CSharp.dll: the game's C# scripts"),
        ];
        let mut evidence: Vec<String> = MARKERS
            .iter()
            .filter(|(name, _)| tree.files.iter().any(|f| f.name() == *name))
            .map(|(_, why)| why.to_string())
            .collect();
        if tree.files.iter().any(|f| f.logical.starts_with("assets/bin/Data/")) {
            evidence.push("assets/bin/Data: Unity's Android data folder".into());
        }
        Detection {
            score: if evidence.is_empty() { 0.0 } else { 0.95 },
            evidence,
        }
    }

    fn analyze(&self, tree: &FileTree, _ctx: &Context) -> Vec<GameObject> {
        let mut b = Builder::new(tree);
        for i in b.unclaimed() {
            let f = &tree.files[i];
            let Some(placement) = classify_unity_file(f.name(), &f.ext(), &f.dirs()) else {
                continue;
            };
            let props = file_props(tree, f);
            b.add(format!("file:{}", f.logical), f.name().to_string(), placement, &[i], props);
        }
        if tree.has("AndroidManifest.xml") {
            android::claim(&mut b);
        }
        b.finish()
    }

    fn capabilities(&self, _tree: &FileTree, _ctx: &Context, objects: &[GameObject]) -> Vec<Capability> {
        let has = |kind: &str| objects.iter().any(|o| o.kind == kind);
        let code = if has("game scripts (C#)") {
            cap(
                "code",
                "Code",
                CapabilityStatus::Partial,
                "The game's scripts are in Assembly-CSharp.dll (C#). Reading them needs a .NET decompiler, which GameX doesn't run yet (milestone 6).",
            )
        } else if objects.iter().any(|o| o.display_name == "libil2cpp.so") || has("IL2CPP metadata") {
            cap(
                "code",
                "Code",
                CapabilityStatus::Unsupported,
                "The scripts were compiled to native code (IL2CPP). Only class and method names can be recovered, and GameX can't do that yet.",
            )
        } else {
            cap("code", "Code", CapabilityStatus::Unknown, "The game's scripts weren't found.")
        };
        vec![
            code,
            characters_capability(objects),
            movement_capability(objects),
            cap(
                "assets",
                "Assets",
                CapabilityStatus::Unsupported,
                "Unity packs images, sounds and models into .assets files. Extracting them needs AssetRipper (milestone 6).",
            ),
            cap(
                "visualEditing",
                "Visual editing",
                CapabilityStatus::Unsupported,
                "Unity scenes are stored in binary levelN files and need AssetRipper first (milestone 6).",
            ),
            cap("rebuilding", "Rebuilding", CapabilityStatus::Unsupported, "Rebuilding Unity games isn't supported yet."),
        ]
    }
}

/// `sharedassets0.assets.split3` -> `sharedassets0.assets`
fn without_split(name: &str) -> &str {
    match name.rsplit_once(".split") {
        Some((base, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => name,
    }
}

fn numbered(name: &str, prefix: &str) -> bool {
    let base = name.split('.').next().unwrap_or(name);
    base.strip_prefix(prefix)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn classify_unity_file(name: &str, ext: &str, dirs: &[&str]) -> Option<Placement> {
    let name = without_split(name);
    if ENGINE_FILES.contains(&name) {
        return Some(Placement::new(Category::Core, "Unity engine data", 0.9, "Unity's engine settings and built-in data"));
    }
    if numbered(name, "level") {
        return Some(Placement::new(Category::Map, "Unity scene data", 0.8, "Unity stores each scene in a levelN file"));
    }
    if name.starts_with("Assembly-CSharp") && ext == "dll" {
        return Some(Placement::new(Category::Code, "game scripts (C#)", 0.95, "the game's own C# scripts"));
    }
    if name == "global-metadata.dat" {
        return Some(Placement::new(
            Category::Code,
            "IL2CPP metadata",
            0.9,
            "class and method names for scripts compiled with IL2CPP",
        ));
    }
    if ext == "dll" && dirs.contains(&"Managed") {
        return Some(Placement::new(Category::Code, "code library", 0.8, "a .NET library used by the game"));
    }
    if numbered(name, "sharedassets") || name.starts_with("resources.assets") || name.ends_with(".assets") {
        return Some(Placement::new(Category::Assets, "Unity asset file", 0.8, "packed images, sounds, models and other assets"));
    }
    match ext {
        "ress" | "resource" => Some(Placement::new(Category::Assets, "streamed asset data", 0.8, "large asset data (textures, audio) streamed by Unity")),
        "bundle" | "unity3d" => Some(Placement::new(Category::Assets, "asset bundle", 0.7, "a Unity asset bundle")),
        _ => None,
    }
}
