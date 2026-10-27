//! Godot games: exported builds (a `.pck` data pack, on Android next to
//! `libgodot_android.so`), Godot 4 exports laid out as loose files
//! (`*.import` / `*.remap` pointing into `.godot/`), and Godot project
//! folders (`project.godot`).

use std::collections::HashMap;
use std::io::Read;

use crate::model::{Capability, CapabilityStatus, Category, GameObject};

use super::classify::{classify_file, file_props, format_bytes, name_hint, prop, Builder, Placement, ALL_WORDS, MEDIA_WORDS};
use super::tree::{FileEntry, FileTree};
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
        if tree.files.iter().any(|f| f.logical.contains(".godot/imported/") || f.name() == "project.binary") {
            evidence.push("A Godot 4 export (.godot/imported, project.binary)".into());
            strong = true;
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
        claim_exported_assets(&mut b);
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
                ("tres" | "res", _) => resource(stem, &dirs),
                ("import", _) => Placement::new(Category::Assets, "import settings", 0.6, "Godot's import settings for an asset"),
                ("gdshader", _) => Placement::new(Category::Code, "shader", 0.8, "a Godot shader"),
                // Exported files no .import/.remap pointed to.
                ("ctex", _) => name_hint(stem, &dirs, MEDIA_WORDS, "image").unwrap_or_else(|| {
                    Placement::new(Category::Assets, "image", 0.6, "a texture in Godot's compressed format")
                }),
                ("gdc", _) => compiled_script(stem, &dirs),
                ("oggvorbisstr" | "sample" | "mp3str", _) => {
                    Placement::new(Category::Audio, "sound", 0.8, "audio in Godot's own stream format")
                }
                ("fontdata", _) => Placement::new(Category::Ui, "font", 0.8, "a font in Godot's format"),
                ("sparsepck", _) => Placement::new(Category::Core, "Godot data pack", 0.8, "part of Godot's packed game data"),
                (_, "project.binary") => {
                    Placement::new(Category::Core, "Godot project settings", 0.95, "the project's main settings, in binary form")
                }
                (_, "global_script_class_cache.cfg" | "uid_cache.bin") => {
                    Placement::new(Category::Core, "Godot metadata", 0.9, "Godot's index of scripts and resources")
                }
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
        let has_file = |o: &GameObject, ext: &str| o.source_files.iter().any(|f| f.ends_with(ext));
        let scripts = objects.iter().filter(|o| has_file(o, ".gd")).count();
        let scenes = objects.iter().filter(|o| has_file(o, ".tscn")).count();
        let compiled_scripts = objects.iter().filter(|o| has_file(o, ".gdc")).count();
        let binary_scenes = objects.iter().filter(|o| has_file(o, ".scn")).count();
        let textures = objects.iter().filter(|o| has_file(o, ".ctex")).count();
        let godot_audio = objects
            .iter()
            .filter(|o| has_file(o, ".oggvorbisstr") || has_file(o, ".sample") || has_file(o, ".mp3str"))
            .count();
        let packed = count("Godot data pack") > 0 && textures == 0;

        let code = if scripts > 0 {
            cap("code", "Code", CapabilityStatus::Supported, format!("{scripts} GDScript files are plain source text."))
        } else if compiled_scripts > 0 {
            cap(
                "code",
                "Code",
                CapabilityStatus::Unsupported,
                format!(
                    "{compiled_scripts} scripts are compiled GDScript (.gdc). They're listed under their names, but reading them needs a GDScript decompiler (gdsdecomp), which GameX doesn't run yet (milestone 8)."
                ),
            )
        } else if packed {
            cap(
                "code",
                "Code",
                CapabilityStatus::Unsupported,
                "Scripts are packed inside the .pck. Recovering them needs gdsdecomp, which GameX doesn't run yet (milestone 8).",
            )
        } else {
            cap("code", "Code", CapabilityStatus::Unknown, "No GDScript files were found.")
        };
        // Android launcher icons don't count: the game's own art is in the pack.
        let loose_media = objects
            .iter()
            .any(|o| matches!(o.kind.as_str(), "image" | "image set" | "sound" | "music") && !o.id.starts_with("res:"));
        let assets = if textures > 0 || godot_audio > 0 {
            cap(
                "assets",
                "Assets",
                CapabilityStatus::Partial,
                format!(
                    "{textures} textures are viewable (decoded from Godot's .ctex files). Replacing them isn't supported yet. {godot_audio} sounds are in Godot's own stream format: they're listed, but can't be played or replaced yet."
                ),
            )
        } else if packed && !loose_media {
            cap(
                "assets",
                "Assets",
                CapabilityStatus::Unsupported,
                "Assets are packed inside the .pck. Recovering them needs gdsdecomp (milestone 8).",
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
        } else if binary_scenes > 0 {
            cap(
                "visualEditing",
                "Visual editing",
                CapabilityStatus::Unsupported,
                format!("{binary_scenes} scenes are exported in Godot's binary format (.scn). Viewing or editing them isn't supported yet."),
            )
        } else if packed {
            cap(
                "visualEditing",
                "Visual editing",
                CapabilityStatus::Unsupported,
                "Scenes are packed inside the .pck and need gdsdecomp first (milestone 8).",
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

fn resource(stem: &str, dirs: &[&str]) -> Placement {
    name_hint(stem, dirs, MEDIA_WORDS, "resource")
        .unwrap_or_else(|| Placement::new(Category::Assets, "resource", 0.5, "a Godot resource"))
}

fn compiled_script(stem: &str, dirs: &[&str]) -> Placement {
    name_hint(stem, dirs, ALL_WORDS, "GDScript (compiled)").unwrap_or_else(|| {
        Placement::new(Category::Code, "GDScript (compiled)", 0.7, "a compiled script with no game-specific hint")
    })
}

/// The `path=` values of a `.import` or `.remap` file (`path`, `path.s3tc`,
/// `path.etc2`...), and its `type=`.
fn link_targets(text: &str) -> (Vec<String>, Option<String>) {
    let mut paths = Vec::new();
    let mut ty = None;
    for line in text.lines().map(str::trim) {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim().trim_matches('"');
        if key == "path" || key.starts_with("path.") {
            paths.push(value.to_string());
        } else if key == "type" {
            ty = Some(value.to_string());
        }
    }
    (paths, ty)
}

/// Width and height from a `.ctex` header (`GST2`, then u32 width, height).
fn ctex_size(tree: &FileTree, entry: &FileEntry) -> Option<(u32, u32)> {
    let mut head = [0u8; 16];
    std::fs::File::open(tree.full_path(entry)).ok()?.read_exact(&mut head).ok()?;
    if &head[0..4] != b"GST2" {
        return None;
    }
    let u32_at = |i: usize| u32::from_le_bytes([head[i], head[i + 1], head[i + 2], head[i + 3]]);
    Some((u32_at(8), u32_at(12)))
}

/// Godot 4 exports without a .pck keep each original asset as a small text
/// file next to where it was (`Sprites/tile.png.import`, `Scenes/a.tscn.remap`)
/// that points to the exported file under `.godot/`. Each becomes one object
/// named after the original, holding the exported file (first, so it is what
/// gets previewed), the original if it was exported too, and the link file.
fn claim_exported_assets(b: &mut Builder) {
    let tree = b.tree;
    let by_logical: HashMap<&str, usize> = b
        .unclaimed()
        .into_iter()
        .map(|i| (tree.files[i].logical.as_str(), i))
        .collect();
    // In an APK the project root is `assets/`; in a folder it is the top.
    let base = if tree.files.iter().any(|f| f.logical.starts_with("assets/.godot/") || f.logical == "assets/project.binary") {
        "assets/"
    } else {
        ""
    };
    let mut taken = vec![false; tree.files.len()];

    for i in b.unclaimed() {
        let link = &tree.files[i];
        let Some(source) = link
            .logical
            .strip_suffix(".import")
            .or_else(|| link.logical.strip_suffix(".remap"))
        else {
            continue;
        };
        if taken[i] {
            continue;
        }
        let text = tree.read_text(link, 64 << 10).unwrap_or_default();
        let (paths, ty) = link_targets(&text);
        let mut files: Vec<usize> = paths
            .iter()
            .filter_map(|res| res.strip_prefix("res://"))
            .filter_map(|rel| by_logical.get(format!("{base}{rel}").as_str()).copied())
            .filter(|&t| !taken[t])
            .collect();
        files.dedup();
        if let Some(&original) = by_logical.get(source).filter(|&&o| !taken[o]) {
            files.push(original);
        }
        files.push(i);
        for &f in &files {
            taken[f] = true;
        }

        let exported = &tree.files[files[0]];
        let name = source.rsplit('/').next().unwrap_or(source).to_string();
        // Classify as the original asset, by its own name and folders.
        let original = FileEntry {
            path: String::new(),
            logical: source.to_string(),
            size: exported.size,
            copies: Vec::new(),
        };
        let placement = match original.ext().as_str() {
            "gd" => compiled_script(original.stem(), &original.dirs()),
            "tres" | "res" => resource(original.stem(), &original.dirs()),
            "tscn" | "scn" => name_hint(original.stem(), &original.dirs(), ALL_WORDS, "scene").unwrap_or_else(|| {
                Placement::new(
                    Category::Unsorted,
                    "scene",
                    0.3,
                    "a scene with no hint in its name; it could be a level, a character or a menu",
                )
            }),
            _ => classify_file(&original),
        };

        let mut props = Vec::new();
        if let Some(ty) = &ty {
            props.push(prop("Godot type", ty.as_str()));
        }
        if exported.ext() == "ctex" {
            if let Some((w, h)) = ctex_size(tree, exported) {
                props.push(prop("Dimensions", format!("{w} × {h}")));
            }
        }
        if files[0] != i {
            props.push(prop("Exported as", exported.name()));
            props.push(prop("File size", format_bytes(exported.size)));
        } else {
            props.extend(file_props(tree, link));
        }
        b.add(format!("godot:{source}"), name, placement, &files, props);
    }
}
