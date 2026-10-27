//! Godot games: exported builds (a `.pck` data pack, on Android next to
//! `libgodot_android.so`) and Godot project folders (`project.godot`).

use crate::model::{Capability, CapabilityStatus, Category, GameObject};

use super::classify::{file_props, name_hint, Builder, Placement, ALL_WORDS, MEDIA_WORDS};
use super::tree::FileTree;
use super::{android, assets_capability, cap, characters_capability, movement_capability, Context, Detection, EngineIntegration};

pub struct Godot;

impl EngineIntegration for Godot {
    fn id(&self) -> &'static str {
        "godot"
    }

    fn name(&self) -> &'static str {
        "Godot"
    }

    fn detect(&self, tree: &FileTree) -> Detection {
        let mut evidence = Vec::new();
        let mut strong = false;
        if tree.files.iter().any(|f| f.name() == "libgodot_android.so") {
            evidence.push("libgodot_android.so: Godot's Android runtime".into());
            strong = true;
        }
        if tree.files.iter().any(|f| f.ext() == "pck") {
            evidence.push("A .pck file: Godot's packed game data".into());
            strong = true;
        }
        if tree.files.iter().any(|f| f.name() == "project.godot") {
            evidence.push("project.godot: a Godot project".into());
            strong = true;
        }
        if tree.files.iter().any(|f| f.ext() == "gd") {
            evidence.push("GDScript files (.gd)".into());
        }
        if tree.files.iter().any(|f| f.ext() == "tscn") {
            evidence.push("Godot scene files (.tscn)".into());
        }
        let score = match (strong, evidence.is_empty()) {
            (true, _) => 0.95,
            (false, false) => 0.8,
            (false, true) => 0.0,
        };
        Detection { score, evidence }
    }

    fn analyze(&self, tree: &FileTree, _ctx: &Context) -> Vec<GameObject> {
        let mut b = Builder::new(tree);
        for i in b.unclaimed() {
            let f = &tree.files[i];
            let stem = f.stem();
            let dirs = f.dirs();
            let placement = match (f.ext().as_str(), f.name()) {
                ("pck", _) => Placement::new(
                    Category::Core,
                    "Godot data pack",
                    0.95,
                    "holds the game's scenes, scripts and assets in Godot's packed format",
                ),
                (_, "project.godot") => Placement::new(Category::Core, "Godot project settings", 0.95, "the project's main settings"),
                ("gd", _) => name_hint(stem, &dirs, ALL_WORDS, "GDScript")
                    .unwrap_or_else(|| Placement::new(Category::Code, "GDScript", 0.7, "a script with no game-specific hint")),
                ("tscn" | "scn", _) => name_hint(stem, &dirs, ALL_WORDS, "scene").unwrap_or_else(|| {
                    Placement::new(
                        Category::Unsorted,
                        "scene",
                        0.3,
                        "a scene with no hint in its name; it could be a level, a character or a menu",
                    )
                }),
                ("tres" | "res", _) => name_hint(stem, &dirs, MEDIA_WORDS, "resource")
                    .unwrap_or_else(|| Placement::new(Category::Assets, "resource", 0.5, "a Godot resource")),
                ("import", _) => Placement::new(Category::Assets, "import settings", 0.6, "Godot's import settings for an asset"),
                ("gdshader", _) => Placement::new(Category::Code, "shader", 0.8, "a Godot shader"),
                _ => continue,
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
        let count = |kind: &str| objects.iter().filter(|o| o.kind == kind).count();
        let scripts = objects.iter().filter(|o| o.source_files.iter().any(|f| f.ends_with(".gd"))).count();
        let scenes = objects.iter().filter(|o| o.source_files.iter().any(|f| f.ends_with(".tscn"))).count();
        let packed = count("Godot data pack") > 0;

        let code = if scripts > 0 {
            cap("code", "Code", CapabilityStatus::Supported, format!("{scripts} GDScript files are plain source text."))
        } else if packed {
            cap(
                "code",
                "Code",
                CapabilityStatus::Unsupported,
                "Scripts are packed inside the .pck. Recovering them needs gdsdecomp, which GameX doesn't run yet (milestone 6).",
            )
        } else {
            cap("code", "Code", CapabilityStatus::Unknown, "No GDScript files were found.")
        };
        // Android launcher icons don't count: the game's own art is in the pack.
        let loose_media = objects
            .iter()
            .any(|o| matches!(o.kind.as_str(), "image" | "image set" | "sound" | "music") && !o.id.starts_with("res:"));
        let assets = if packed && !loose_media {
            cap(
                "assets",
                "Assets",
                CapabilityStatus::Unsupported,
                "Assets are packed inside the .pck. Recovering them needs gdsdecomp (milestone 6).",
            )
        } else {
            assets_capability(objects)
        };
        let visual = if scenes > 0 {
            cap(
                "visualEditing",
                "Visual editing",
                CapabilityStatus::Partial,
                format!("{scenes} scene files are text and can be read. A visual editor for them isn't built yet."),
            )
        } else if packed {
            cap(
                "visualEditing",
                "Visual editing",
                CapabilityStatus::Unsupported,
                "Scenes are packed inside the .pck and need gdsdecomp first (milestone 6).",
            )
        } else {
            cap("visualEditing", "Visual editing", CapabilityStatus::Unknown, "No scene files were found.")
        };
        vec![
            code,
            characters_capability(objects),
            movement_capability(objects),
            assets,
            visual,
            cap("rebuilding", "Rebuilding", CapabilityStatus::Unknown, "Repacking Godot games isn't supported yet."),
        ]
    }
}
