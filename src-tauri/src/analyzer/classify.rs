//! Engine-independent classification: decides a category for any file from
//! its type, its name and its folders. Engine integrations claim the files
//! they understand better first; everything left over comes through here, so
//! every file ends up in exactly one object.
//!
//! The word lists are general game vocabulary. They must never contain names
//! from one particular game.

use std::collections::{BTreeMap, HashSet};

use crate::model::{Category, GameObject, Property};

use super::tree::{FileEntry, FileTree};

#[derive(Clone)]
pub struct Placement {
    pub category: Category,
    pub kind: String,
    pub confidence: f32,
    pub reason: String,
}

impl Placement {
    pub fn new(category: Category, kind: &str, confidence: f32, reason: impl Into<String>) -> Self {
        Placement {
            category,
            kind: kind.to_string(),
            confidence,
            reason: reason.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Vocabulary

const CHARACTER_WORDS: &[&str] = &[
    "player", "hero", "heroine", "enemy", "enemies", "monster", "boss", "npc", "character",
    "actor", "mob", "creature", "villain", "zombie", "ghost", "vampire", "skeleton", "goblin",
    "orc", "dragon", "alien", "robot", "avatar", "pet", "minion", "foe", "soldier", "knight",
    "warrior", "wizard", "witch", "ninja", "slime", "boy", "girl", "mummy", "demon",
];
const ITEM_WORDS: &[&str] = &[
    "item", "weapon", "sword", "gun", "bow", "arrow", "bullet", "ammo", "pickup", "powerup",
    "coin", "gem", "jewel", "diamond", "key", "potion", "chest", "loot", "treasure", "inventory",
    "collectible", "reward", "shield", "armor", "armour", "bomb", "crystal", "gold", "money",
    "food", "scroll", "orb", "relic", "weapons", "projectile",
];
const MAP_WORDS: &[&str] = &[
    "map", "level", "stage", "world", "tile", "tileset", "tilemap", "room", "maze", "terrain",
    "dungeon", "area", "zone", "background", "bg", "backdrop", "parallax", "platform", "ground",
    "floor", "wall", "door", "ladder", "stairs", "sky", "scenery", "landscape",
];
const UI_WORDS: &[&str] = &[
    "ui", "gui", "hud", "menu", "button", "btn", "icon", "ico", "logo", "font", "title",
    "splash", "dialog", "popup", "panel", "cursor", "healthbar", "score", "label", "banner",
    "pause", "loading", "joystick", "dpad", "gamepad", "touch", "controls", "overlay", "screen",
    "lang", "locale", "localization", "strings", "translation",
];
const CORE_WORDS: &[&str] = &[
    "config", "settings", "prefs", "preferences", "save", "savegame", "game", "main", "engine",
    "loop", "manager", "constants", "globals", "physics", "input", "boot", "init", "application",
];
const MUSIC_WORDS: &[&str] = &[
    "music", "bgm", "theme", "song", "track", "soundtrack", "ambient", "ambience",
];

/// Which word lists a kind of file is matched against, in priority order.
pub const MEDIA_WORDS: &[(Category, &[&str])] = &[
    (Category::Characters, CHARACTER_WORDS),
    (Category::Items, ITEM_WORDS),
    (Category::Map, MAP_WORDS),
    (Category::Ui, UI_WORDS),
];
pub const ALL_WORDS: &[(Category, &[&str])] = &[
    (Category::Characters, CHARACTER_WORDS),
    (Category::Items, ITEM_WORDS),
    (Category::Map, MAP_WORDS),
    (Category::Ui, UI_WORDS),
    (Category::Core, CORE_WORDS),
];

/// Splits `PlayerIdle_02`, `btn-play` or `com.game.ui` into lowercase words.
pub fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphabetic() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let prev = i.checked_sub(1).map(|j| chars[j]);
        let next = chars.get(i + 1);
        // camelCase and ACRONYMWord boundaries
        let boundary = c.is_uppercase()
            && prev.is_some_and(|p| {
                p.is_lowercase() || (p.is_uppercase() && next.is_some_and(|n| n.is_lowercase()))
            });
        if boundary && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub struct WordMatch {
    pub category: Category,
    pub word: &'static str,
    /// Whole-word match rather than a prefix like "player" in "playerwalk".
    pub exact: bool,
}

fn word_matches(token: &str, word: &str) -> Option<bool> {
    if token == word || token == format!("{word}s") || token == format!("{word}es") {
        Some(true)
    } else if word.len() >= 4 && token.starts_with(word) {
        Some(false)
    } else {
        None
    }
}

/// Finds the best word in `tokens`. Specific categories beat Core (so
/// "GameMenu" is UI, not Core), whole words beat prefixes, and later words
/// beat earlier ones, because the last word usually names the thing
/// ("map_icon" is an icon).
pub fn match_words(tokens: &[String], lists: &[(Category, &'static [&'static str])]) -> Option<WordMatch> {
    let mut best: Option<(u8, WordMatch)> = None;
    for token in tokens.iter().rev() {
        for &(category, words) in lists {
            for &word in words {
                let Some(exact) = word_matches(token, word) else { continue };
                let rank = u8::from(category != Category::Core) * 2 + u8::from(exact);
                if best.as_ref().is_none_or(|(r, _)| rank > *r) {
                    best = Some((rank, WordMatch { category, word, exact }));
                }
            }
        }
    }
    best.map(|(_, m)| m)
}

/// Looks for a hint in the file name first, then in its folders.
pub fn name_hint(
    stem: &str,
    dirs: &[&str],
    lists: &[(Category, &'static [&'static str])],
    kind: &str,
) -> Option<Placement> {
    if let Some(m) = match_words(&tokens(stem), lists) {
        let confidence = if m.exact { 0.65 } else { 0.5 };
        return Some(Placement::new(
            m.category,
            kind,
            confidence,
            format!("its name contains \"{}\"", m.word),
        ));
    }
    // Outermost first, so the innermost folder is the "last word".
    let dir_tokens: Vec<String> = dirs.iter().flat_map(|d| tokens(d)).collect();
    match_words(&dir_tokens, lists).map(|m| {
        let confidence = if m.exact { 0.6 } else { 0.45 };
        Placement::new(
            m.category,
            kind,
            confidence,
            format!("it is in a folder named after \"{}\"", m.word),
        )
    })
}

// ---------------------------------------------------------------------------
// File types

pub const IMAGE: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "tga", "dds", "ktx", "pkm", "astc", "psd", "svg",
    "ico",
];
pub const AUDIO: &[&str] = &[
    "ogg", "mp3", "wav", "m4a", "aac", "flac", "mid", "midi", "xm", "mod", "it", "s3m", "opus",
    "wma", "amr",
];
const VIDEO: &[&str] = &["mp4", "webm", "ogv", "mkv", "avi", "mov", "m4v"];
const FONT: &[&str] = &["ttf", "otf", "fnt", "woff", "woff2"];
const MODEL: &[&str] = &["obj", "fbx", "gltf", "glb", "dae", "blend", "3ds", "stl", "ply"];
const SHADER: &[&str] = &["glsl", "vert", "frag", "vsh", "fsh", "shader", "hlsl", "fx", "cg"];
const SOURCE: &[&str] = &[
    "java", "kt", "kts", "smali", "lua", "js", "mjs", "ts", "py", "gd", "cs", "rb", "c", "cpp",
    "cc", "h", "hpp", "m", "mm", "rs", "go", "hx", "as",
];
const MAP_DATA: &[&str] = &["tmx", "tmj", "tsx", "ldtk", "lvl", "level"];
const DATA: &[&str] = &[
    "json", "xml", "yaml", "yml", "txt", "csv", "ini", "cfg", "properties", "toml", "plist", "md",
];
const ARCHIVE: &[&str] = &[
    "zip", "pck", "pak", "obb", "bundle", "unity3d", "arc", "rar", "7z", "gz", "tar",
];

pub fn is_image(ext: &str) -> bool {
    IMAGE.contains(&ext)
}

/// The engine-independent placement for one file.
pub fn classify_file(entry: &FileEntry) -> Placement {
    let ext = entry.ext();
    let ext = ext.as_str();
    let stem = entry.stem();
    let dirs = entry.dirs();
    let in_dir = |name: &str| dirs.iter().any(|d| d.eq_ignore_ascii_case(name));

    if dirs.first() == Some(&"META-INF") {
        return Placement::new(Category::Core, "package metadata", 0.9, "signing and package metadata");
    }
    if is_image(ext) {
        if dirs.iter().any(|d| d.starts_with("mipmap")) {
            return Placement::new(Category::Ui, "app icon", 0.9, "launcher icon folder");
        }
        return name_hint(stem, &dirs, MEDIA_WORDS, "image").unwrap_or_else(|| {
            Placement::new(
                Category::Assets,
                "image",
                0.7,
                "an image with no character, item, map or UI hint in its name or folder",
            )
        });
    }
    if AUDIO.contains(&ext) {
        let music = match_words(&tokens(stem), &[(Category::Audio, MUSIC_WORDS)]).is_some()
            || in_dir("music")
            || entry.size > 1_000_000;
        return if music {
            Placement::new(Category::Audio, "music", 0.9, "a long or music-named audio file")
        } else {
            Placement::new(Category::Audio, "sound", 0.9, "an audio file")
        };
    }
    if VIDEO.contains(&ext) {
        return Placement::new(Category::Assets, "video", 0.8, "a video file");
    }
    if FONT.contains(&ext) {
        return Placement::new(Category::Ui, "font", 0.9, "a font file");
    }
    if MODEL.contains(&ext) {
        return name_hint(stem, &dirs, MEDIA_WORDS, "3D model").unwrap_or_else(|| {
            Placement::new(Category::Assets, "3D model", 0.7, "a 3D model file")
        });
    }
    if SHADER.contains(&ext) {
        return Placement::new(Category::Code, "shader", 0.8, "a shader program");
    }
    if SOURCE.contains(&ext) {
        return name_hint(stem, &dirs, ALL_WORDS, "source file").unwrap_or_else(|| {
            Placement::new(Category::Code, "source file", 0.7, "source code with no game-specific hint")
        });
    }
    match ext {
        "dex" | "class" => {
            return Placement::new(Category::Code, "compiled code", 0.9, "compiled Java/Android bytecode")
        }
        "jar" | "aar" => return Placement::new(Category::Code, "code library", 0.8, "a packaged Java library"),
        "so" | "dll" | "dylib" => {
            return Placement::new(Category::Code, "native library", 0.85, "compiled native code")
        }
        "exe" => return Placement::new(Category::Core, "executable", 0.8, "the program that starts the game"),
        _ => {}
    }
    if MAP_DATA.contains(&ext) {
        return Placement::new(Category::Map, "map data", 0.85, "a level or tile-map format");
    }
    if DATA.contains(&ext) {
        return name_hint(stem, &dirs, ALL_WORDS, "data").unwrap_or_else(|| {
            Placement::new(Category::Unsorted, "data", 0.2, "a data file with no hint about what it holds")
        });
    }
    if ARCHIVE.contains(&ext) {
        return Placement::new(Category::Unsorted, "archive", 0.3, "a packed archive; its contents aren't unpacked");
    }
    Placement::new(Category::Unsorted, "file", 0.1, "an unrecognized file type")
}

// ---------------------------------------------------------------------------
// Building objects

pub fn prop(label: &str, value: impl Into<String>) -> Property {
    Property {
        label: label.to_string(),
        value: value.into(),
    }
}

pub fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

/// Size, plus pixel dimensions for images.
pub fn file_props(tree: &FileTree, entry: &FileEntry) -> Vec<Property> {
    let mut props = vec![prop("Format", entry.ext().to_uppercase()), prop("File size", format_bytes(entry.size))];
    if is_image(&entry.ext()) {
        if let Ok(dim) = imagesize::size(tree.full_path(entry)) {
            props.push(prop("Dimensions", format!("{} × {}", dim.width, dim.height)));
        }
    }
    props
}

/// Collects objects and tracks which files have been placed.
pub struct Builder<'a> {
    pub tree: &'a FileTree,
    claimed: Vec<bool>,
    objects: Vec<GameObject>,
}

impl<'a> Builder<'a> {
    pub fn new(tree: &'a FileTree) -> Self {
        Builder {
            tree,
            claimed: vec![false; tree.files.len()],
            objects: Vec::new(),
        }
    }

    /// Indexes of files no integration has placed yet.
    pub fn unclaimed(&self) -> Vec<usize> {
        (0..self.claimed.len()).filter(|&i| !self.claimed[i]).collect()
    }

    /// Adds an object for these files and returns it, so callers can fill
    /// in a role or gameplay values.
    pub fn add(
        &mut self,
        id: String,
        display_name: String,
        placement: Placement,
        files: &[usize],
        properties: Vec<Property>,
    ) -> &mut GameObject {
        let mut source_files = Vec::new();
        for &i in files {
            self.claimed[i] = true;
            let entry = &self.tree.files[i];
            source_files.push(entry.path.clone());
            source_files.extend(entry.copies.iter().cloned());
        }
        self.objects.push(GameObject {
            id,
            category: placement.category,
            auto_category: placement.category,
            display_name,
            kind: placement.kind,
            source_files,
            properties,
            confidence: placement.confidence,
            reason: placement.reason,
            role: None,
            gameplay: Vec::new(),
        });
        self.objects.last_mut().expect("just pushed")
    }

    /// Adds one object per file that is still unplaced, groups numbered
    /// image frames, and returns the objects sorted for display.
    pub fn finish(mut self) -> Vec<GameObject> {
        for i in self.unclaimed() {
            let entry = &self.tree.files[i];
            let placement = classify_file(entry);
            let props = file_props(self.tree, entry);
            self.add(
                format!("file:{}", entry.logical),
                entry.name().to_string(),
                placement,
                &[i],
                props,
            );
        }
        let mut objects = group_numbered_images(self.objects);
        mark_directional_sprites(&mut objects);
        for obj in &mut objects {
            if obj.auto_category == Category::Characters && obj.role.is_none() {
                obj.role = role_from_name(&obj.display_name).map(|(role, _)| role.to_string());
            }
        }
        let mut seen = HashSet::new();
        for obj in &mut objects {
            if !seen.insert(obj.id.clone()) {
                let mut n = 2;
                while !seen.insert(format!("{}#{n}", obj.id)) {
                    n += 1;
                }
                obj.id = format!("{}#{n}", obj.id);
            }
        }
        objects.sort_by(|a, b| {
            (a.category, &a.kind, a.display_name.to_lowercase())
                .cmp(&(b.category, &b.kind, b.display_name.to_lowercase()))
        });
        objects
    }
}

/// `run_01`, `run-2`, `run3` -> ("run", n).
fn split_frame(name: &str) -> Option<(&str, u32)> {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits > 4 {
        return None;
    }
    let (base, num) = name.split_at(name.len() - digits);
    let base = base.trim_end_matches(['_', '-', ' ', '.']);
    if base.is_empty() {
        return None;
    }
    Some((base, num.parse().ok()?))
}

fn strip_ext(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// Numbered images in the same folder (`walk1.png`, `walk2.png`...) become
/// one "image set" object, which is how most games store animation frames.
fn group_numbered_images(objects: Vec<GameObject>) -> Vec<GameObject> {
    let mut groups: BTreeMap<(String, String, Category), Vec<(u32, usize)>> = BTreeMap::new();
    for (i, obj) in objects.iter().enumerate() {
        if obj.kind != "image" {
            continue;
        }
        let dir = obj.id.rsplit_once('/').map_or("", |(d, _)| d);
        if let Some((base, n)) = split_frame(strip_ext(&obj.display_name)) {
            groups
                .entry((dir.to_string(), base.to_lowercase(), obj.category))
                .or_default()
                .push((n, i));
        }
    }
    let mut merged_into: Vec<Option<usize>> = vec![None; objects.len()];
    let mut sets = Vec::new();
    for ((dir, _, _), mut members) in groups {
        if members.len() < 2 {
            continue;
        }
        members.sort();
        let first = &objects[members[0].1];
        let base = split_frame(strip_ext(&first.display_name)).unwrap().0.to_string();
        let mut set = GameObject {
            id: format!("set:{dir}/{base}"),
            display_name: base.clone(),
            kind: "image set".into(),
            source_files: Vec::new(),
            properties: vec![prop("Frames", members.len().to_string())],
            confidence: members
                .iter()
                .map(|&(_, i)| objects[i].confidence)
                .fold(1.0, f32::min),
            reason: format!(
                "{} numbered images named \"{base}\"; {}",
                members.len(),
                first.reason
            ),
            ..first.clone()
        };
        set.properties
            .extend(first.properties.iter().filter(|p| p.label != "File size").cloned());
        for &(_, i) in &members {
            set.source_files.extend(objects[i].source_files.iter().cloned());
            merged_into[i] = Some(sets.len());
        }
        sets.push(set);
    }
    objects
        .into_iter()
        .zip(merged_into)
        .filter_map(|(obj, merged)| merged.is_none().then_some(obj))
        .chain(sets)
        .collect()
}

/// Character roles, from general game vocabulary.
const ROLE_WORDS: &[(&str, &[&str])] = &[
    ("Player", &["player", "hero", "heroine", "protagonist", "avatar"]),
    (
        "Enemy",
        &[
            "enemy", "enemies", "monster", "boss", "mob", "foe", "villain", "zombie", "ghost", "vampire",
            "skeleton", "goblin", "orc", "demon", "slime", "mummy", "minion",
        ],
    ),
    ("NPC", &["npc", "villager", "merchant", "shopkeeper", "vendor", "trader", "citizen", "townsfolk"]),
    ("Ally", &["ally", "allies", "companion", "pet", "partner", "sidekick"]),
];

/// The role a character's name points to, and the word that showed it.
/// Later words win, like elsewhere: "player_enemy_marker" is an enemy thing.
pub fn role_from_name(name: &str) -> Option<(&'static str, &'static str)> {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let mut prefix = None;
    for token in tokens(stem).iter().rev() {
        for &(role, words) in ROLE_WORDS {
            for &word in words {
                match word_matches(token, word) {
                    Some(true) => return Some((role, word)),
                    Some(false) if prefix.is_none() => prefix = Some((role, word)),
                    _ => {}
                }
            }
        }
    }
    prefix
}

/// Name endings for the direction a sprite faces.
const DIRECTIONS: &[&str] = &[
    "u", "d", "l", "r", "ul", "ur", "dl", "dr", "up", "down", "left", "right", "n", "s", "e", "w",
    "ne", "nw", "se", "sw", "north", "south", "east", "west", "front", "back", "side",
];

/// `hero_up`, `hero_down`, `hero_left`... A sprite drawn facing three or
/// more directions is almost always something that moves around: a
/// character. Only images with no stronger hint are moved.
fn mark_directional_sprites(objects: &mut [GameObject]) {
    let key = |obj: &GameObject| -> Option<(String, String, String)> {
        if obj.kind != "image" && obj.kind != "image set" {
            return None;
        }
        let dir = obj.id.trim_start_matches("set:").rsplit_once('/').map_or("", |(d, _)| d);
        let name = strip_ext(&obj.display_name).to_lowercase();
        let (base, facing) = name.rsplit_once(['_', '-'])?;
        (!base.is_empty() && DIRECTIONS.contains(&facing))
            .then(|| (dir.to_string(), base.to_string(), facing.to_string()))
    };
    let mut facings: BTreeMap<(String, String), HashSet<String>> = BTreeMap::new();
    for obj in objects.iter() {
        if let Some((dir, base, facing)) = key(obj) {
            facings.entry((dir, base)).or_default().insert(facing);
        }
    }
    for obj in objects.iter_mut() {
        let Some((dir, base, _)) = key(obj) else { continue };
        let n = facings[&(dir, base.clone())].len();
        if n >= 3 && obj.auto_category == Category::Assets {
            obj.auto_category = Category::Characters;
            obj.category = Category::Characters;
            obj.confidence = 0.55;
            obj.reason = format!(
                "\"{base}\" is drawn facing {n} directions, which usually means a character that moves"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(logical: &str, size: u64) -> FileEntry {
        FileEntry {
            path: logical.into(),
            logical: logical.into(),
            size,
            copies: vec![],
        }
    }

    #[test]
    fn splits_names_into_words() {
        assert_eq!(tokens("PlayerIdle_02"), ["player", "idle"]);
        assert_eq!(tokens("HUDButton"), ["hud", "button"]);
        assert_eq!(tokens("btn-play.9"), ["btn", "play"]);
        assert_eq!(tokens("com.game.ui"), ["com", "game", "ui"]);
    }

    #[test]
    fn places_files_by_type_name_and_folder() {
        let cases = [
            ("sprites/hero_run.png", Category::Characters, "image"),
            ("art/enemies/a.png", Category::Characters, "image"),
            ("img/coin.png", Category::Items, "image"),
            ("gfx/tileset_forest.png", Category::Map, "image"),
            ("res/drawable/btn_play.png", Category::Ui, "image"),
            ("res/mipmap-hdpi/ic_launcher.png", Category::Ui, "app icon"),
            ("img/sparkle.png", Category::Assets, "image"),
            ("sfx/jump.ogg", Category::Audio, "sound"),
            ("audio/level_theme.mp3", Category::Audio, "music"),
            ("fonts/main.ttf", Category::Ui, "font"),
            ("levels/one.tmx", Category::Map, "map data"),
            ("data/items.json", Category::Items, "data"),
            ("data/config.ini", Category::Core, "data"),
            ("data/blob.json", Category::Unsorted, "data"),
            ("scripts/player.lua", Category::Characters, "source file"),
            ("scripts/util.lua", Category::Code, "source file"),
            ("whatever.xyz", Category::Unsorted, "file"),
        ];
        for (path, category, kind) in cases {
            let p = classify_file(&entry(path, 10));
            assert_eq!((p.category, p.kind.as_str()), (category, kind), "{path}");
        }
        assert_eq!(classify_file(&entry("a/wind.ogg", 2_000_000)).kind, "music");
    }

    #[test]
    fn prefers_specific_and_last_words() {
        let place = |path: &str| classify_file(&entry(path, 10)).category;
        assert_eq!(place("gfx/map_icon.png"), Category::Ui);
        assert_eq!(place("gfx/icon_map.png"), Category::Map);
        assert_eq!(place("data/game_items.json"), Category::Items);
        assert_eq!(place("items/ui/x.json"), Category::Ui);
    }

    #[test]
    fn sprites_with_facing_directions_are_characters() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["knave_u.png", "knave_d.png", "knave_l.png", "knave_r.png", "lamp_u.png", "lamp_d.png", "sign_left.png"] {
            let p = dir.path().join("files/gfx").join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        let tree = FileTree::scan(dir.path());
        let objects = Builder::new(&tree).finish();
        let cat = |n: &str| objects.iter().find(|o| o.display_name == n).unwrap().category;
        assert_eq!(cat("knave_u.png"), Category::Characters);
        assert_eq!(cat("knave_r.png"), Category::Characters);
        assert_eq!(cat("lamp_u.png"), Category::Assets);
        assert_eq!(cat("sign_left.png"), Category::Assets);
    }

    #[test]
    fn character_roles_come_from_names() {
        assert_eq!(role_from_name("PlayerSprite"), Some(("Player", "player")));
        assert_eq!(role_from_name("boss_idle.png"), Some(("Enemy", "boss")));
        assert_eq!(role_from_name("ShopkeeperNpc"), Some(("NPC", "npc")));
        assert_eq!(role_from_name("petDog"), Some(("Ally", "pet")));
        assert_eq!(role_from_name("actor"), None);
    }

    #[test]
    fn groups_numbered_frames() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["walk1.png", "walk2.png", "walk_10.png", "idle.png", "other/walk3.png"] {
            let p = dir.path().join("files/sprites").join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        let tree = FileTree::scan(dir.path());
        let objects = Builder::new(&tree).finish();
        let names: Vec<_> = objects.iter().map(|o| (o.display_name.as_str(), o.kind.as_str())).collect();
        assert_eq!(names, [("idle.png", "image"), ("walk3.png", "image"), ("walk", "image set")]);
        let set = objects.iter().find(|o| o.kind == "image set").unwrap();
        assert_eq!(
            set.source_files,
            ["files/sprites/walk1.png", "files/sprites/walk2.png", "files/sprites/walk_10.png"]
        );
    }
}
