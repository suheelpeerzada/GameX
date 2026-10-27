//! Native Android games (Java/Kotlin, no third-party engine), plus the
//! Android-specific helpers that the Godot and Unity integrations reuse when
//! their game ships as an APK.

use std::collections::BTreeMap;

use crate::model::{Capability, CapabilityStatus, Category, ContainerKind, GameObject, GameplayValue};

use super::classify::{
    self, file_props, is_image, match_words, prop, role_from_name, tokens, Builder, Placement, MEDIA_WORDS,
};
use super::java::{self, ClassInfo};
use super::tree::FileTree;
use super::{
    assets_capability, cap, characters_capability, movement_capability, visual_editing_from_maps, Context,
    Detection, EngineIntegration,
};

pub struct NativeAndroid;

impl EngineIntegration for NativeAndroid {
    fn id(&self) -> &'static str {
        "android-native"
    }

    fn name(&self) -> &'static str {
        "Native Android (Java/Kotlin)"
    }

    fn detect(&self, tree: &FileTree) -> Detection {
        if !tree.has("AndroidManifest.xml") {
            return Detection {
                score: 0.0,
                evidence: vec![],
            };
        }
        let mut evidence = vec!["AndroidManifest.xml: this is an Android app".to_string()];
        if tree.files.iter().any(|f| f.ext() == "dex" || is_java(&f.logical) || is_smali(&f.logical)) {
            evidence.push("Java/Kotlin code (classes.dex)".into());
        }
        evidence.push("No Godot or Unity engine files".into());
        for name in frameworks(tree) {
            evidence.push(format!("Uses the {name} framework"));
        }
        Detection { score: 0.6, evidence }
    }

    fn analyze(&self, tree: &FileTree, _ctx: &Context) -> Vec<GameObject> {
        let mut b = Builder::new(tree);
        claim(&mut b);
        b.finish()
    }

    fn capabilities(&self, _tree: &FileTree, ctx: &Context, objects: &[GameObject]) -> Vec<Capability> {
        vec![
            code_capability(ctx, objects),
            characters_capability(objects),
            movement_capability(objects),
            assets_capability(objects),
            visual_editing_from_maps(
                objects,
                cap(
                    "visualEditing",
                    "Visual editing",
                    CapabilityStatus::Unsupported,
                    "Native Android games usually build their levels in code, so there are no scene files to edit visually. Layout files only cover menus and screens.",
                ),
            ),
            rebuild_capability(ctx),
        ]
    }
}

/// Java code frameworks that sit on top of plain Android.
fn frameworks(tree: &FileTree) -> Vec<&'static str> {
    const KNOWN: &[(&str, &[&str])] = &[
        ("libGDX", &["com/badlogic/gdx/", "libgdx.so"]),
        ("Cocos2d-x", &["org/cocos2dx/", "libcocos2d"]),
        ("AndEngine", &["org/andengine/"]),
    ];
    KNOWN
        .iter()
        .filter(|(_, markers)| {
            tree.files
                .iter()
                .any(|f| markers.iter().any(|m| f.logical.contains(m)))
        })
        .map(|(name, _)| *name)
        .collect()
}

fn is_java(logical: &str) -> bool {
    logical.starts_with("sources/") && logical.ends_with(".java")
}

fn is_smali(logical: &str) -> bool {
    logical.starts_with("smali") && logical.ends_with(".smali")
}

/// Places the manifest, Android resources, native libraries and code
/// classes. Anything else is left for the generic classifier.
pub fn claim(b: &mut Builder) {
    let manifest = claim_manifest(b);
    claim_package_files(b);
    claim_resources(b);
    claim_native_libs(b);
    claim_classes(b, &manifest);
}

// ---------------------------------------------------------------------------
// Manifest

#[derive(Default)]
pub struct Manifest {
    pub package: Option<String>,
    pub launcher: Option<String>,
    pub application: Option<String>,
}

fn claim_manifest(b: &mut Builder) -> Manifest {
    let tree = b.tree;
    let Some(i) = b
        .unclaimed()
        .into_iter()
        .find(|&i| tree.files[i].logical == "AndroidManifest.xml")
    else {
        return Manifest::default();
    };
    let entry = &tree.files[i];
    let text = tree.read_text(entry, 2 << 20).unwrap_or_default();
    // A binary manifest (no apktool) can't be read as text.
    let decoded = text.trim_start().starts_with('<');
    let manifest = if decoded { parse_manifest(&text) } else { Manifest::default() };

    let mut props = vec![];
    if let Some(p) = &manifest.package {
        props.push(prop("Package", p));
    }
    if let Some(l) = &manifest.launcher {
        props.push(prop("Starts with", l));
    }
    if decoded {
        props.push(prop("Permissions", text.matches("<uses-permission").count().to_string()));
    }
    let placement = if decoded {
        Placement::new(Category::Core, "Android manifest", 0.95, "declares the app, its screens and permissions")
    } else {
        Placement::new(
            Category::Core,
            "Android manifest",
            0.95,
            "declares the app (still in binary form; apktool decodes it)",
        )
    };
    b.add("android:manifest".into(), "AndroidManifest.xml".into(), placement, &[i], props);
    manifest
}

/// Files that describe the package rather than the game.
fn claim_package_files(b: &mut Builder) {
    let tree = b.tree;
    for i in b.unclaimed() {
        let f = &tree.files[i];
        let placement = match f.logical.as_str() {
            "resources.arsc" => Placement::new(
                Category::Core,
                "resource table",
                0.9,
                "Android's compiled index of every resource (apktool decodes it into res/values)",
            ),
            "apktool.yml" => Placement::new(Category::Core, "unpack metadata", 0.9, "written by apktool; needed to rebuild the APK"),
            _ if f.ext() == "kotlin_builtins"
                || matches!(f.name(), "kotlin-tooling-metadata.json" | "DebugProbesKt.bin") =>
            {
                Placement::new(Category::Code, "library data", 0.85, "data for the Kotlin standard library, not the game")
            }
            _ if matches!(f.name(), "baseline.prof" | "baseline.profm") => Placement::new(
                Category::Core,
                "Android runtime profile",
                0.85,
                "tells Android which code to precompile; not game content",
            ),
            _ => continue,
        };
        let props = file_props(tree, f);
        b.add(format!("file:{}", f.logical), f.name().to_string(), placement, &[i], props);
    }
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

fn qualify(package: Option<&str>, name: &str) -> String {
    match package {
        Some(p) if name.starts_with('.') => format!("{p}{name}"),
        Some(p) if !name.contains('.') => format!("{p}.{name}"),
        _ => name.to_string(),
    }
}

pub fn parse_manifest(text: &str) -> Manifest {
    let package = text
        .find("<manifest")
        .and_then(|i| attr(&text[i..text[i..].find('>').map_or(text.len(), |e| i + e)], "package"))
        .map(String::from);
    let pkg = package.as_deref();

    let application = text.find("<application").and_then(|i| {
        let end = text[i..].find('>').map_or(text.len(), |e| i + e);
        attr(&text[i..end], "android:name").map(|n| qualify(pkg, n))
    });

    let mut launcher = None;
    for tag in ["<activity-alias", "<activity "] {
        let mut rest = text;
        while let Some(i) = rest.find(tag) {
            let block = &rest[i..];
            let open_end = block.find('>').unwrap_or(block.len());
            let end = if block[..open_end].ends_with('/') {
                open_end
            } else {
                block.find("</activity").unwrap_or(block.len())
            };
            let body = &block[..end];
            if launcher.is_none() && body.contains("android.intent.category.LAUNCHER") {
                let open = &block[..open_end];
                let name = attr(open, "android:targetActivity").or_else(|| attr(open, "android:name"));
                launcher = name.map(|n| qualify(pkg, n));
            }
            rest = &block[end.max(1)..];
        }
    }
    Manifest {
        package,
        launcher,
        application,
    }
}

// ---------------------------------------------------------------------------
// Resources: res/<type>[-qualifiers]/<name>

/// Lower is a better copy to preview.
fn qualifier_rank(qualifier: &str) -> u8 {
    match qualifier {
        "" => 0,
        "nodpi" => 1,
        "xxxhdpi" => 2,
        "xxhdpi" => 3,
        "xhdpi" => 4,
        "hdpi" => 5,
        "mdpi" => 6,
        "ldpi" => 7,
        _ => 10,
    }
}

/// (type, name) -> variants as (preview rank, file index, qualifier).
type ResourceUnits = BTreeMap<(String, String), Vec<(u8, usize, String)>>;

fn claim_resources(b: &mut Builder) {
    let tree = b.tree;
    let mut units = ResourceUnits::new();
    for i in b.unclaimed() {
        let f = &tree.files[i];
        let parts: Vec<&str> = f.logical.split('/').collect();
        if parts.len() != 3 || parts[0] != "res" {
            continue;
        }
        let (ty, qualifier) = parts[1].split_once('-').unwrap_or((parts[1], ""));
        units
            .entry((ty.to_string(), f.stem().to_string()))
            .or_default()
            .push((qualifier_rank(qualifier), i, qualifier.to_string()));
    }
    for ((ty, stem), mut members) in units {
        members.sort();
        let primary = &tree.files[members[0].1];
        let placement = classify_resource(&ty, &stem, primary);
        let mut props = file_props(tree, primary);
        if members.len() > 1 {
            let variants: Vec<&str> = members
                .iter()
                .map(|(_, _, q)| if q.is_empty() { "default" } else { q.as_str() })
                .collect();
            props.push(prop("Variants", variants.join(", ")));
        }
        let name = if ty == "values" { stem.clone() } else { primary.name().to_string() };
        let files: Vec<usize> = members.iter().map(|m| m.1).collect();
        b.add(format!("res:{ty}/{stem}"), name, placement, &files, props);
    }
}

fn classify_resource(ty: &str, stem: &str, primary: &super::tree::FileEntry) -> Placement {
    let ext = primary.ext();
    match ty {
        "drawable" | "mipmap" if is_image(&ext) => classify::classify_file(primary),
        "drawable" | "mipmap" => Placement::new(
            Category::Ui,
            "drawable (xml)",
            0.6,
            "an Android shape or state drawable, mostly used for UI",
        ),
        "layout" => Placement::new(Category::Ui, "layout", 0.9, "an Android screen layout"),
        "menu" => Placement::new(Category::Ui, "menu", 0.9, "an Android menu"),
        "anim" | "animator" | "transition" => {
            Placement::new(Category::Ui, "UI animation", 0.7, "an Android view animation")
        }
        "font" => Placement::new(Category::Ui, "font", 0.9, "a font resource"),
        "color" => Placement::new(Category::Ui, "colors", 0.8, "a color state list"),
        "values" => match stem {
            "strings" | "plurals" => {
                Placement::new(Category::Ui, "text strings", 0.85, "the game's text, one variant per language")
            }
            "colors" | "styles" | "themes" | "dimens" => {
                Placement::new(Category::Ui, "style values", 0.75, "colors, sizes and styles for the UI")
            }
            _ => Placement::new(Category::Core, "resource values", 0.6, "Android resource values"),
        },
        "xml" => Placement::new(Category::Core, "config xml", 0.6, "Android configuration XML"),
        _ => classify::classify_file(primary),
    }
}

// ---------------------------------------------------------------------------
// Native libraries: lib/<abi>/<name>.so

fn claim_native_libs(b: &mut Builder) {
    let tree = b.tree;
    let mut libs: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    for i in b.unclaimed() {
        let parts: Vec<&str> = tree.files[i].logical.split('/').collect();
        if parts.len() == 3 && parts[0] == "lib" && parts[2].ends_with(".so") {
            libs.entry(parts[2].to_string()).or_default().push((i, parts[1].to_string()));
        }
    }
    for (name, members) in libs {
        let reason = match name.as_str() {
            "libgdx.so" => "native code of the libGDX framework",
            "libunity.so" | "libil2cpp.so" => "native code of the Unity engine",
            "libgodot_android.so" => "native code of the Godot engine",
            n if n.starts_with("libcocos2d") => "native code of the Cocos2d-x engine",
            _ => "compiled native code (C/C++)",
        };
        let abis: Vec<&str> = members.iter().map(|(_, abi)| abi.as_str()).collect();
        let props = vec![
            prop("CPU types", abis.join(", ")),
            prop("File size", classify::format_bytes(tree.files[members[0].0].size)),
        ];
        let files: Vec<usize> = members.iter().map(|m| m.0).collect();
        b.add(
            format!("lib:{name}"),
            name,
            Placement::new(Category::Code, "native library", 0.85, reason),
            &files,
            props,
        );
    }
}

// ---------------------------------------------------------------------------
// Code classes: jadx .java and apktool .smali, merged per top-level class

/// Third-party packages that are libraries, not the game itself.
const LIBRARY_PACKAGES: &[&str] = &[
    "android.", "androidx.", "kotlin.", "kotlinx.", "java.", "javax.", "dalvik.", "com.google.",
    "com.android.", "org.jetbrains.", "org.intellij.", "com.facebook.", "com.unity3d.",
    "org.godotengine.", "com.badlogic.gdx.", "org.cocos2dx.", "org.andengine.", "okhttp3.",
    "okio.", "retrofit2.", "com.squareup.", "io.reactivex.", "org.apache.", "org.json.",
    "com.applovin.", "com.ironsource.", "com.chartboost.", "com.adcolony.", "com.vungle.",
    "com.mbridge.", "com.bytedance.", "com.appsflyer.", "com.adjust.", "com.onesignal.",
    "com.crashlytics.", "io.fabric.", "com.amazon.", "com.huawei.", "bolts.", "dagger.",
    "com.fasterxml.", "org.slf4j.", "com.startapp.", "com.inmobi.", "com.tapjoy.", "org.fmod.",
];

/// Supertypes that usually hold a game's entry point, loop or renderer.
const CORE_BASES: &[&str] = &[
    "Activity", "AppCompatActivity", "FragmentActivity", "NativeActivity", "Application",
    "SurfaceView", "GLSurfaceView", "TextureView", "Thread", "Game", "ApplicationAdapter",
    "AndroidApplication", "Renderer", "ApplicationListener", "Runnable",
];

const HEALTH_WORDS: &[&str] = &["hp", "health", "life", "lives", "hitpoints", "energy"];
const POSITION_WORDS: &[&str] = &["x", "y", "pos", "position", "posx", "posy", "coords"];
const MOVEMENT_WORDS: &[&str] = &[
    "move", "moving", "velocity", "vel", "speed", "jump", "jumping", "gravity", "accel",
    "acceleration", "vx", "vy", "dx", "dy", "walk", "friction", "knockback",
];

/// Field-name words for numbers that change how the game plays. A value is
/// only reported when its field name contains one of these.
const GAMEPLAY_WORDS: &[&str] = &[
    "speed", "velocity", "vel", "jump", "gravity", "health", "hp", "life", "lives", "damage", "attack",
    "power", "strength", "defense", "defence", "armor", "armour", "ammo", "bullets", "cooldown", "delay",
    "timer", "time", "duration", "range", "radius", "distance", "score", "points", "coins", "gold", "money",
    "accel", "acceleration", "friction", "mass", "weight", "energy", "mana", "stamina", "spawn", "rate",
    "chance", "probability", "lifetime", "reload", "knockback",
];

/// Names that mark identifiers rather than tuning: `STATE_ATTACK`, `ENEMY_ID`.
const NOT_GAMEPLAY_WORDS: &[&str] = &[
    "state", "type", "id", "dir", "mode", "index", "idx", "event", "key", "action", "sfx", "sound", "color",
    "colour", "flag",
];

/// Field names for 2D arrays that hold a level layout.
const GRID_MAP_WORDS: &[&str] = &[
    "map", "level", "tile", "tiles", "grid", "cells", "cell", "board", "layout", "room", "terrain", "world",
    "maze", "blocks",
];

/// Field names for animation data (frame lists, animation tables).
const ANIMATION_WORDS: &[&str] = &["anim", "anims", "animation", "animations", "frames"];

fn gameplay_values(info: &ClassInfo, java_path: &str) -> Vec<GameplayValue> {
    let mut seen = std::collections::HashSet::new();
    info.values
        .iter()
        .filter(|v| {
            let words = tokens(&v.field);
            words.iter().any(|t| GAMEPLAY_WORDS.contains(&t.as_str()))
                && !words.iter().any(|t| NOT_GAMEPLAY_WORDS.contains(&t.as_str()))
        })
        // A constructor resetting a field to 0 or false isn't a tuning value.
        .filter(|v| !(v.in_constructor && matches!(v.value.as_str(), "0" | "0.0" | "false")))
        .filter(|v| seen.insert((v.field.clone(), v.value.clone())))
        .map(|v| GameplayValue {
            name: v.field.clone(),
            value: v.value.clone(),
            file: java_path.to_string(),
            line: v.line,
            set_in: if v.in_constructor { "constructor" } else { "field" }.to_string(),
        })
        .collect()
}

/// One code class while the classes are being analyzed together.
struct ClassEntry {
    fqcn: String,
    info: ClassInfo,
    java_file: Option<usize>,
    files: Vec<usize>,
    library: bool,
    placement: Placement,
    role: Option<String>,
}

impl ClassEntry {
    /// Evidence strong enough to pass on to subclasses: a character with a
    /// known role, an item or part of the map. A generic base such as
    /// "actor" passes nothing on, since many kinds of objects extend it.
    fn passes_on(&self) -> bool {
        self.placement.confidence >= 0.5
            && match self.placement.category {
                Category::Characters => self.role.is_some(),
                Category::Items | Category::Map => true,
                _ => false,
            }
    }

    /// Placed in Characters, Items or Map by its own evidence.
    fn game_category(&self) -> Option<Category> {
        (self.placement.confidence >= 0.5
            && matches!(self.placement.category, Category::Characters | Category::Items | Category::Map))
        .then_some(self.placement.category)
    }

    /// No real signal of its own: generic code, or a guess below 0.5.
    fn is_weak(&self) -> bool {
        !self.library
            && (self.placement.confidence < 0.5
                || (self.placement.category == Category::Code && self.placement.kind == "class"))
    }
}

/// The class a class extends, if it's one of the game's own classes.
fn parent_index(entries: &[ClassEntry], index: &BTreeMap<String, usize>, i: usize) -> Option<usize> {
    let parent = entries[i].info.extends.as_deref()?;
    if parent.contains('.') {
        return index.get(parent).copied();
    }
    // jadx writes same-package superclasses without their package.
    let package = entries[i].fqcn.rsplit_once('.').map_or("", |(p, _)| p);
    index
        .get(&format!("{package}.{parent}"))
        .copied()
        .or_else(|| {
            let mut matches = entries.iter().enumerate().filter(|(_, e)| java::simple_name(&e.fqcn) == parent);
            match (matches.next(), matches.next()) {
                (Some((j, _)), None) => Some(j),
                _ => None,
            }
        })
}

fn claim_classes(b: &mut Builder, manifest: &Manifest) {
    let tree = b.tree;
    // fqcn -> (java file, smali files)
    let mut classes: BTreeMap<String, (Option<usize>, Vec<usize>)> = BTreeMap::new();
    for i in b.unclaimed() {
        let logical = &tree.files[i].logical;
        if is_java(logical) {
            let fqcn = logical["sources/".len()..logical.len() - 5].replace('/', ".");
            classes.entry(fqcn).or_default().0 = Some(i);
        } else if is_smali(logical) {
            let Some((_, rest)) = logical.split_once('/') else { continue };
            let name = rest[..rest.len() - 6].replace('/', ".");
            let outer = name.split('$').next().unwrap_or(&name).to_string();
            classes.entry(outer).or_default().1.push(i);
        }
    }
    // Pass 1: each class on its own evidence.
    let mut entries: Vec<ClassEntry> = Vec::new();
    for (fqcn, (java_file, mut smali_files)) in classes {
        smali_files.sort_by_key(|&i| tree.files[i].logical.contains('$'));
        let info = match java_file {
            Some(i) => tree.read_text(&tree.files[i], 1 << 20).map(|t| java::parse_java(&t)),
            None => tree.read_text(&tree.files[smali_files[0]], 1 << 20).map(|t| java::parse_smali(&t)),
        }
        .unwrap_or_default();
        let library = is_library(&fqcn);
        let placement = classify_class(&fqcn, &info, manifest, library);
        let role = (placement.category == Category::Characters)
            .then(|| role_from_name(java::simple_name(&fqcn)).map(|(r, _)| r.to_string()))
            .flatten();
        entries.push(ClassEntry {
            fqcn,
            info,
            java_file,
            files: java_file.into_iter().chain(smali_files).collect(),
            library,
            placement,
            role,
        });
    }

    // Pass 2: a class with no signal of its own takes the category and role
    // of the nearest ancestor that has strong evidence (bat extends enemy).
    let index: BTreeMap<String, usize> = entries.iter().enumerate().map(|(i, e)| (e.fqcn.clone(), i)).collect();
    let parents: Vec<Option<usize>> = (0..entries.len()).map(|i| parent_index(&entries, &index, i)).collect();
    // A base whose own subclasses are clearly other kinds of objects (a
    // "roomObject" that enemies and items extend) is generic: it passes
    // nothing on, whatever its name says.
    let mut generic_base = vec![false; entries.len()];
    for i in 0..entries.len() {
        let Some(category) = entries[i].game_category() else { continue };
        let mut current = parents[i];
        for _ in 0..12 {
            let Some(ancestor) = current.filter(|&a| a != i) else { break };
            if entries[ancestor].game_category().is_some_and(|c| c != category) {
                generic_base[ancestor] = true;
            }
            current = parents[ancestor];
        }
    }
    let mut inherited: Vec<(usize, Placement, Option<String>)> = Vec::new();
    for i in 0..entries.len() {
        let own_role_missing = entries[i].placement.category == Category::Characters && entries[i].role.is_none();
        if !entries[i].is_weak() && !own_role_missing {
            continue;
        }
        let mut path = vec![java::simple_name(&entries[i].fqcn).to_string()];
        let mut current = i;
        for _ in 0..12 {
            let Some(parent) = parents[current] else { break };
            if parent == i {
                break; // a cycle; shouldn't happen in real code
            }
            path.push(java::simple_name(&entries[parent].fqcn).to_string());
            let ancestor = &entries[parent];
            if ancestor.passes_on() && !generic_base[parent] {
                let chain = path[1..].join(" → ");
                if own_role_missing {
                    // Already a character by its own name; only the role is new.
                    inherited.push((i, entries[i].placement.clone(), ancestor.role.clone()));
                } else {
                    let what = match (ancestor.placement.category, ancestor.role.as_deref()) {
                        (Category::Characters, Some(role)) => format!("a character ({})", role.to_lowercase()),
                        (Category::Items, _) => "an item".to_string(),
                        _ => "part of the map".to_string(),
                    };
                    let placement = Placement::new(
                        ancestor.placement.category,
                        "class",
                        0.55,
                        format!("it extends {chain}, which is {what}"),
                    );
                    inherited.push((i, placement, ancestor.role.clone()));
                }
                break;
            }
            current = parent;
        }
    }
    for (i, placement, role) in inherited {
        entries[i].placement = placement;
        entries[i].role = role;
    }

    for entry in entries {
        let ClassEntry { fqcn, info, java_file, files, library, placement, role } = entry;
        let mut props = vec![];
        if let Some((package, _)) = fqcn.rsplit_once('.') {
            props.push(prop("Package", package));
        }
        if !info.kind.is_empty() {
            props.push(prop("Type", &info.kind));
        }
        if let Some(e) = &info.extends {
            props.push(prop("Extends", e));
        }
        if !info.implements.is_empty() {
            props.push(prop("Implements", info.implements.join(", ")));
        }
        props.push(prop("Fields", info.fields.len().to_string()));
        let animation_fields: Vec<&str> = info
            .fields
            .iter()
            .filter(|f| tokens(f).iter().any(|t| ANIMATION_WORDS.contains(&t.as_str())))
            .map(String::as_str)
            .take(6)
            .collect();
        if !animation_fields.is_empty() {
            props.push(prop("Animation fields", animation_fields.join(", ")));
        }
        props.push(prop("Methods", info.methods.len().to_string()));
        if !library {
            let hints = movement_hints(&info);
            if hints.len() >= 2 {
                props.push(prop("Movement hints", hints.join(", ")));
            }
        }
        let gameplay = match (library, java_file) {
            (false, Some(j)) => gameplay_values(&info, &tree.files[j].path),
            _ => Vec::new(),
        };
        let object = b.add(
            format!("class:{fqcn}"),
            java::simple_name(&fqcn).to_string(),
            placement,
            &files,
            props,
        );
        object.role = role;
        object.gameplay = gameplay;
    }
}

fn is_library(fqcn: &str) -> bool {
    LIBRARY_PACKAGES.iter().any(|p| fqcn.starts_with(p))
}

fn member_has(names: &[String], words: &[&str]) -> bool {
    names.iter().any(|n| tokens(n).iter().any(|t| words.contains(&t.as_str())) || words.contains(&n.to_lowercase().as_str()))
}

fn movement_hints(info: &ClassInfo) -> Vec<String> {
    let mut hits: Vec<String> = info
        .fields
        .iter()
        .chain(&info.methods)
        .filter(|n| {
            let lower = n.to_lowercase();
            MOVEMENT_WORDS.contains(&lower.as_str())
                || tokens(n).iter().any(|t| MOVEMENT_WORDS.contains(&t.as_str()) && t.len() > 2)
        })
        .cloned()
        .collect();
    hits.sort();
    hits.dedup();
    hits.truncate(6);
    hits
}

fn classify_class(fqcn: &str, info: &ClassInfo, manifest: &Manifest, library: bool) -> Placement {
    let simple = java::simple_name(fqcn);
    if library {
        let package = fqcn.splitn(3, '.').take(2).collect::<Vec<_>>().join(".");
        return Placement::new(
            Category::Code,
            "library class",
            0.9,
            format!("{package} is a known third-party library, not game code"),
        );
    }
    if simple == "R" || simple == "BuildConfig" {
        return Placement::new(Category::Code, "generated class", 0.9, "generated by the Android build tools");
    }
    if manifest.launcher.as_deref() == Some(fqcn) {
        return Placement::new(Category::Core, "class", 0.9, "the screen Android opens when the game starts");
    }
    if manifest.application.as_deref() == Some(fqcn) {
        return Placement::new(Category::Core, "class", 0.85, "the app's Application class, created before anything else");
    }
    if let Some(m) = match_words(&tokens(simple), super::classify::ALL_WORDS) {
        let confidence = if m.exact { 0.6 } else { 0.45 };
        return Placement::new(m.category, "class", confidence, format!("its name contains \"{}\"", m.word));
    }
    // Package names like com.studio.game.enemies only count for whole words,
    // since company names often contain words like "game".
    let package_tokens: Vec<String> = fqcn
        .split('.')
        .rev()
        .skip(1)
        .take(2)
        .map(str::to_lowercase)
        .collect();
    if let Some(m) = match_words(&package_tokens, MEDIA_WORDS).filter(|m| m.exact) {
        return Placement::new(m.category, "class", 0.5, format!("its package is named after \"{}\"", m.word));
    }
    if member_has(&info.fields, HEALTH_WORDS) && member_has(&info.fields, POSITION_WORDS) {
        return Placement::new(Category::Characters, "class", 0.55, "it has both health and position fields");
    }
    let map_grid = info
        .grid_fields
        .iter()
        .find(|f| tokens(f).iter().any(|t| GRID_MAP_WORDS.contains(&t.as_str())));
    if let Some(grid) = map_grid {
        return Placement::new(
            Category::Map,
            "class",
            0.5,
            format!("it holds a 2D grid field ({grid}), the usual way a level layout is stored"),
        );
    }
    let base = info.extends.as_deref().map(java::simple_name);
    if let Some(base) = base.filter(|b| CORE_BASES.contains(b)) {
        return Placement::new(
            Category::Core,
            "class",
            0.6,
            format!("it extends {base}, which usually runs a game's screen, loop or rendering"),
        );
    }
    if let Some(i) = info.implements.iter().map(|i| java::simple_name(i)).find(|i| CORE_BASES.contains(i)) {
        return Placement::new(
            Category::Core,
            "class",
            0.5,
            format!("it implements {i}, which often drives a game loop or renderer"),
        );
    }
    if simple.len() <= 2 && simple.chars().all(|c| c.is_ascii_lowercase()) {
        return Placement::new(Category::Code, "class", 0.3, "its name is obfuscated, so there is nothing to go on");
    }
    Placement::new(Category::Code, "class", 0.4, "no game-specific hint in its name or fields")
}

// ---------------------------------------------------------------------------
// Capabilities

fn code_capability(ctx: &Context, objects: &[GameObject]) -> Capability {
    let game_classes: Vec<&GameObject> = objects.iter().filter(|o| o.id.starts_with("class:") && o.kind == "class").collect();
    let java = game_classes
        .iter()
        .filter(|o| o.source_files.first().is_some_and(|f| f.ends_with(".java")))
        .count();
    let obfuscated = game_classes.iter().filter(|o| o.reason.contains("obfuscated")).count();
    let has_dex = objects.iter().any(|o| o.kind == "compiled code");

    let mut note;
    let status;
    if java > 0 {
        status = CapabilityStatus::Partial;
        note = format!(
            "{java} game classes are readable as decompiled Java. Java is shown for reading; to change a class, edit its smali file, which is what gets rebuilt."
        );
        if ctx.step("jadx").and_then(|s| s.note.as_deref()).is_some_and(|n| n.contains("error")) {
            note.push_str(" jadx reported errors, so some methods may be missing.");
        }
    } else if !game_classes.is_empty() {
        status = CapabilityStatus::Partial;
        note = "Only smali (low-level bytecode) is available. Install jadx in Settings and re-import for readable Java.".into();
    } else if has_dex {
        status = CapabilityStatus::Unsupported;
        note = "The code is still compiled (classes.dex). Install apktool and jadx in Settings and re-import to read it.".into();
    } else {
        return cap("code", "Code", CapabilityStatus::Unknown, "No Java or Kotlin code was found.");
    }
    if obfuscated * 2 > game_classes.len() {
        note.push_str(" Most class names are obfuscated, so names won't say much.");
    }
    cap("code", "Code", status, note)
}

fn rebuild_capability(ctx: &Context) -> Capability {
    if ctx.record.container != ContainerKind::Apk {
        return cap(
            "rebuilding",
            "Rebuilding",
            CapabilityStatus::Unknown,
            "This was imported as a ZIP or folder, so there is no original APK to rebuild from.",
        );
    }
    if ctx.step_done("apktool") {
        cap(
            "rebuilding",
            "Rebuilding",
            CapabilityStatus::Supported,
            "apktool decoded the APK, so it can be rebuilt with your edits and signed with your own key. Use Build APK.",
        )
    } else {
        cap(
            "rebuilding",
            "Rebuilding",
            CapabilityStatus::Unsupported,
            "apktool didn't run. Set it up in Settings and re-import to make rebuilding possible.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_launcher_and_package() {
        let xml = r#"<?xml version="1.0"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.example.game">
    <uses-permission android:name="android.permission.VIBRATE"/>
    <application android:name=".GameApp" android:label="Game">
        <activity android:name=".SettingsActivity"/>
        <activity android:name="com.example.game.MainActivity" android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN"/>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter>
        </activity>
    </application>
</manifest>"#;
        let m = parse_manifest(xml);
        assert_eq!(m.package.as_deref(), Some("com.example.game"));
        assert_eq!(m.launcher.as_deref(), Some("com.example.game.MainActivity"));
        assert_eq!(m.application.as_deref(), Some("com.example.game.GameApp"));
    }

    fn info(fields: &[&str], extends: Option<&str>) -> ClassInfo {
        ClassInfo {
            kind: "class".into(),
            extends: extends.map(String::from),
            fields: fields.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn classifies_classes_by_role() {
        let m = Manifest {
            launcher: Some("com.s.Main".into()),
            ..Default::default()
        };
        let place = |fqcn: &str, i: ClassInfo| {
            let lib = is_library(fqcn);
            classify_class(fqcn, &i, &m, lib).category
        };
        assert_eq!(place("com.s.Main", info(&[], None)), Category::Core);
        assert_eq!(place("com.s.EnemyBat", info(&[], None)), Category::Characters);
        assert_eq!(place("com.s.Thing", info(&["hp", "x", "y"], None)), Category::Characters);
        assert_eq!(place("com.s.Coin", info(&[], None)), Category::Items);
        assert_eq!(place("com.s.LevelLoader", info(&[], None)), Category::Map);
        assert_eq!(place("com.s.PauseMenu", info(&[], None)), Category::Ui);
        assert_eq!(place("com.s.Board", info(&[], Some("android.view.SurfaceView"))), Category::Core);
        assert_eq!(place("com.s.ab", info(&[], None)), Category::Code);
        assert_eq!(place("com.google.ads.Banner", info(&[], None)), Category::Code);
        // A company name containing "game" is not a hint.
        assert_eq!(place("com.megagames.app.Util", info(&[], None)), Category::Code);
    }
}
