//! Any other game -> GameX: the reader for games whose engine has no reader
//! of its own (Java/Kotlin Android games, and anything else). It brings in
//! the game's images, sounds, fonts and models by kind; makes things from
//! sprite names ("zombie_walk1.png", "zombie_walk2.png" -> a Zombie with its
//! frames); and reads Tiled maps (.tmx/.tmj), which many engines use for
//! levels, into scenes. Games that build their levels in code get a scene
//! with every thing laid out, to edit and place from.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use walkdir::WalkDir;

use super::godot::{guess_kind, Report};
use super::model::*;
use super::project::{self, write_json};

/// Pixels per metre, as GameX's 2D scenes use.
const PPU: f32 = 100.0;

const KINDS: &[(&str, &[&str])] = &[
    ("textures", &["png", "jpg", "jpeg", "webp", "bmp", "gif"]),
    ("audio", &["ogg", "mp3", "wav", "m4a", "aac", "flac"]),
    ("fonts", &["ttf", "otf"]),
    ("models", &["glb", "gltf", "obj"]),
];

fn kind_of(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    KINDS.iter().find(|(_, exts)| exts.contains(&ext.as_str())).map(|(k, _)| *k)
}

// ---------------------------------------------------------------------------
// The game's files

/// The files to read, by path as the game names them (`res/raw/x.ogg`, `assets/maps/1.tmx`).
fn game_files(unpacked: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut add_tree = |root: &Path, prefix: &str, skip_res: bool| {
        for e in WalkDir::new(root).into_iter().filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            e.depth() == 0 || !(n.starts_with('.') || n == "META-INF" || n == "smali" || n.starts_with("smali_") || n == "lib" || n == "kotlin" || (skip_res && e.depth() == 1 && n == "res"))
        }) {
            let Ok(e) = e else { continue };
            if !e.file_type().is_file() {
                continue;
            }
            let rel = e.path().strip_prefix(root).unwrap_or(e.path()).to_string_lossy().replace('\\', "/");
            out.push((format!("{prefix}{rel}"), e.path().to_path_buf()));
        }
    };
    let apktool_res = unpacked.join("apktool").join("res");
    let raw = unpacked.join("raw");
    if raw.is_dir() {
        // An APK: decoded resources (real names) if there, else the raw ones.
        if apktool_res.is_dir() {
            add_tree(&apktool_res, "res/", false);
        }
        add_tree(&raw, "", apktool_res.is_dir());
    } else if unpacked.join("files").is_dir() {
        add_tree(&unpacked.join("files"), "", false);
    } else {
        add_tree(unpacked, "", false);
    }
    android_best(out)
}

/// Android keeps one image per screen density (and language); the best one is kept.
fn android_best(files: Vec<(String, PathBuf)>) -> Vec<(String, PathBuf)> {
    let score = |folder: &str| -> i32 {
        let quals: Vec<&str> = folder.split('-').skip(1).collect();
        let mut s = 2;
        for q in &quals {
            s = match *q {
                "nodpi" => 7,
                "xxxhdpi" => 6,
                "xxhdpi" => 5,
                "xhdpi" => 4,
                "hdpi" => 3,
                "mdpi" => 2,
                "ldpi" => 1,
                _ => s,
            };
        }
        // A language or region (drawable-fr, drawable-b+es) is a translation.
        if quals.iter().any(|q| (q.len() == 2 && q.chars().all(|c| c.is_ascii_lowercase())) || q.starts_with("b+") || (q.len() == 3 && q.starts_with('r'))) {
            s -= 20;
        }
        s
    };
    let mut best: HashMap<String, (i32, usize)> = HashMap::new();
    let mut keep = vec![true; files.len()];
    for (i, (rel, _)) in files.iter().enumerate() {
        let parts: Vec<&str> = rel.split('/').collect();
        if parts.len() != 3 || parts[0] != "res" {
            continue;
        }
        let ty = parts[1].split('-').next().unwrap_or("");
        if ty != "drawable" && ty != "mipmap" {
            // Layouts, values, XML: not content.
            keep[i] = ty == "raw" || ty == "font";
            continue;
        }
        let stem = parts[2].rsplit_once('.').map(|(s, _)| s).unwrap_or(parts[2]);
        let key = format!("{ty}/{stem}");
        let s = score(parts[1]);
        match best.get(&key) {
            Some(&(old, _)) if old >= s => keep[i] = false,
            Some(&(_, j)) => {
                keep[j] = false;
                best.insert(key, (s, i));
            }
            None => {
                best.insert(key, (s, i));
            }
        }
    }
    files.into_iter().zip(keep).filter(|(_, k)| *k).map(|(f, _)| f).collect()
}

// ---------------------------------------------------------------------------
// Things from sprite names

const ANIMATION_WORDS: &[&str] = &[
    "walk", "walking", "run", "running", "idle", "jump", "jumping", "attack", "attacking", "die", "dying", "death", "dead", "hit", "hurt", "fall", "falling", "stand",
    "standing", "move", "moving", "left", "right", "up", "down", "anim", "animation", "frame", "frames", "f", "shoot", "shooting", "fly", "flying", "spr", "sprite", "sheet",
    "front", "back", "side", "l", "r", "a", "b",
];
/// Facing directions in sprite names (down, down-right, up...): frames of one thing that moves.
const DIRECTION_WORDS: &[&str] = &["d", "u", "dr", "dl", "ur", "ul", "n", "s", "e", "w", "ne", "nw", "se", "sw", "left", "right", "up", "down"];
const MOVING_WORDS: &[&str] = &["walk", "walking", "run", "running", "jump", "jumping", "attack", "attacking", "die", "dying", "death", "hurt", "hit", "fly", "flying", "shoot", "shooting"];
const UI_WORDS: &[&str] = &[
    "button", "btn", "menu", "bg", "background", "icon", "ic", "logo", "font", "text", "title", "hud", "ui", "gui", "splash", "banner", "screen", "loading", "dialog", "panel",
    "popup", "launcher", "number", "digit", "tutorial", "help", "credits", "pause", "play", "options", "settings", "sound", "music", "share", "rate", "lock", "locked", "star",
    "stars", "bar", "frame", "border", "shadow", "light", "fondo", "boton", "texto", "map", "mapa", "mapsym", "parchment", "pergamino", "cursor", "flag", "collision", "mask", "hitbox", "colision",
];

/// UI words also hide inside glued names ("menubg", "btnplay").
fn is_ui(token: &str) -> bool {
    UI_WORDS.contains(&token) || (token.len() > 4 && ["menu", "btn", "button", "icon", "logo"].iter().any(|w| token.starts_with(w)) || token.ends_with("bg"))
}

/// "zombieWalk_02" -> ["zombie", "walk", "02"]
fn tokens(stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for c in stem.chars() {
        let split = !c.is_alphanumeric()
            || prev.is_some_and(|p| (p.is_lowercase() && c.is_uppercase()) || (p.is_ascii_digit() != c.is_ascii_digit() && p.is_alphanumeric()));
        if split && !cur.is_empty() {
            out.push(std::mem::take(&mut cur).to_lowercase());
        }
        if c.is_alphanumeric() {
            cur.push(c);
        }
        prev = Some(c);
    }
    if !cur.is_empty() {
        out.push(cur.to_lowercase());
    }
    out
}

struct SpriteGroup {
    name: Vec<String>,
    frames: Vec<(String, Vec<String>)>,
}

fn title(words: &[String]) -> String {
    words.iter().map(|w| w[..1].to_uppercase() + &w[1..]).collect::<Vec<_>>().join(" ")
}

/// Things made from groups of images that look like one object's frames.
fn things_from_images(images: &[(String, String, [u32; 2])]) -> Vec<(Thing, String)> {
    let mut groups: BTreeMap<String, SpriteGroup> = BTreeMap::new();
    for (rel, asset, _) in images {
        let file = rel.rsplit('/').next().unwrap_or(rel);
        if file.ends_with(".9.png") || rel.contains("/mipmap") {
            continue;
        }
        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        let all = tokens(stem);
        let name: Vec<String> = all
            .iter()
            .filter(|t| !t.chars().all(|c| c.is_ascii_digit()) && !ANIMATION_WORDS.contains(&t.as_str()) && !DIRECTION_WORDS.contains(&t.as_str()))
            .cloned()
            .collect();
        // One-letter names ("t_01".."t_66") are tiles or letters, not things.
        if name.is_empty() || name.join("").len() < 2 || name.iter().any(|t| is_ui(t)) {
            continue;
        }
        let g = groups.entry(name.join("_")).or_insert_with(|| SpriteGroup { name: name.clone(), frames: Vec::new() });
        g.frames.push((asset.clone(), all));
    }
    let sizes: HashMap<&str, [u32; 2]> = images.iter().map(|(_, a, s)| (a.as_str(), *s)).collect();
    let mut out = Vec::new();
    for (key, mut g) in groups {
        let (mut kind, role) = guess_kind(&g.name.join(" "), "");
        // Walking, jumping... frames, or frames facing several ways: it moves.
        let moving = g.frames.iter().any(|(_, t)| t.iter().any(|w| MOVING_WORDS.contains(&w.as_str())))
            || g.frames.iter().filter_map(|(_, t)| t.iter().find(|w| DIRECTION_WORDS.contains(&w.as_str()))).collect::<std::collections::HashSet<_>>().len() >= 2;
        if kind == ThingKind::Other {
            if g.frames.len() < 2 {
                continue;
            }
            kind = if moving { ThingKind::Character } else { ThingKind::Prop };
        }
        g.frames.sort_by(|a, b| a.0.cmp(&b.0));
        // The still frame first: idle or standing, else the first.
        let first = g.frames.iter().position(|(_, t)| t.iter().any(|w| w == "idle" || w == "stand" || w == "standing")).unwrap_or(0);
        let asset = g.frames[first].0.clone();
        let [w, h] = sizes.get(asset.as_str()).copied().unwrap_or([100, 100]);
        let (w, h) = (w as f32 / PPU, h as f32 / PPU);
        let body = if kind == ThingKind::Character { Body::Kinematic } else { Body::Static };
        let mut parts = vec![
            Part::Sprite { asset: asset.clone(), frame: None, size: Some([w, h]), flip_x: false },
            Part::Collider { shape: ShapeKind::Box, size: [w * 0.8, h * 0.9, 1.0], body, trigger: kind == ThingKind::Item },
        ];
        if g.frames.len() > 1 {
            let mut values = BTreeMap::new();
            values.insert("frames".into(), Value::Text(g.frames.iter().map(|f| f.0.as_str()).collect::<Vec<_>>().join(",")));
            values.insert("status".into(), Value::Text("frames listed; not played yet".into()));
            parts.push(Part::Custom { name: "Animation".into(), values });
        }
        let id = project::slug(&key);
        if id.is_empty() {
            continue;
        }
        out.push((
            Thing { tags: Vec::new(),
                format: FORMAT,
                id,
                name: title(&g.name),
                kind,
                role,
                description: Some(format!("Made from {} image{} named like it", g.frames.len(), if g.frames.len() == 1 { "" } else { "s" })),
                properties: Vec::new(),
                parts,
                behaviors: Vec::new(),
                children: Vec::new(),
            },
            asset,
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// A little XML, enough for Tiled's files

#[derive(Debug, Default, Clone)]
pub struct Tag {
    pub name: String,
    pub attrs: HashMap<String, String>,
    pub children: Vec<Tag>,
    pub text: String,
}

impl Tag {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(String::as_str)
    }
    fn num(&self, k: &str) -> Option<f32> {
        self.attr(k)?.trim().parse().ok()
    }
    fn child(&self, name: &str) -> Option<&Tag> {
        self.children.iter().find(|c| c.name == name)
    }
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Tag> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Parses an XML document into its root tag (comments, declarations and doctypes skipped).
pub fn parse_xml(text: &str) -> Option<Tag> {
    let mut stack: Vec<Tag> = vec![Tag::default()];
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let before = &rest[..open];
        if !before.trim().is_empty() {
            stack.last_mut()?.text.push_str(&unescape(before));
        }
        rest = &rest[open..];
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = &r[r.find("-->")? + 3..];
            continue;
        }
        if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let end = r.find("]]>")?;
            stack.last_mut()?.text.push_str(&r[..end]);
            rest = &r[end + 3..];
            continue;
        }
        let close = rest.find('>')?;
        let inner = &rest[1..close];
        rest = &rest[close + 1..];
        if inner.starts_with('?') || inner.starts_with('!') {
            continue;
        }
        if let Some(name) = inner.strip_prefix('/') {
            let tag = stack.pop()?;
            if tag.name != name.trim() {
                return None;
            }
            stack.last_mut()?.children.push(tag);
            continue;
        }
        let self_closing = inner.ends_with('/');
        let inner = inner.trim_end_matches('/');
        let name_end = inner.find(char::is_whitespace).unwrap_or(inner.len());
        let mut tag = Tag { name: inner[..name_end].to_string(), ..Default::default() };
        let mut attrs = &inner[name_end..];
        while let Some(eq) = attrs.find('=') {
            let key = attrs[..eq].trim().to_string();
            let after = attrs[eq + 1..].trim_start();
            let quote = after.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let end = after[1..].find(quote)?;
            tag.attrs.insert(key, unescape(&after[1..1 + end]));
            attrs = &after[end + 2..];
        }
        if self_closing {
            stack.last_mut()?.children.push(tag);
        } else {
            stack.push(tag);
        }
    }
    if stack.len() != 1 {
        return None;
    }
    stack.pop()?.children.into_iter().next()
}

// ---------------------------------------------------------------------------
// Tiled maps

#[derive(Debug, Clone)]
struct TileImage {
    /// The image file, as found on disk.
    file: PathBuf,
    size: [u32; 2],
}

#[derive(Debug, Clone)]
struct Tileset {
    first: u32,
    tile: [u32; 2],
    margin: u32,
    spacing: u32,
    columns: u32,
    /// One image for the whole set, or one per tile (a collection).
    image: Option<TileImage>,
    images: HashMap<u32, TileImage>,
    solid: Vec<u32>,
}

#[derive(Debug, Clone)]
struct MapObject {
    name: String,
    kind: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    gid: Option<u32>,
    point: bool,
    visible: bool,
}

#[derive(Debug, Clone)]
enum Layer {
    Tiles { name: String, visible: bool, offset: [f32; 2], cells: Vec<(i32, i32, u32)> },
    Objects { name: String, visible: bool, objects: Vec<MapObject> },
}

#[derive(Debug, Clone)]
struct Map {
    tile: [u32; 2],
    background: Option<String>,
    tilesets: Vec<Tileset>,
    layers: Vec<Layer>,
}

const FLIP_H: u32 = 0x8000_0000;
const FLIP_V: u32 = 0x4000_0000;
const FLIP_D: u32 = 0x2000_0000;
const GID_MASK: u32 = 0x0fff_ffff;

fn truthy(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
}

const SOLID_PROPERTIES: &[&str] = &["collides", "collide", "collision", "solid", "blocked", "block", "wall", "obstacle"];

/// Tile layer data: CSV, or base64 (zlib, gzip or plain) of little-endian GIDs.
fn decode_gids(text: &str, encoding: Option<&str>, compression: Option<&str>) -> Result<Vec<u32>, String> {
    match encoding {
        Some("csv") => Ok(text.split(',').filter_map(|v| v.trim().parse::<u32>().ok()).collect()),
        Some("base64") => {
            let raw = base64::engine::general_purpose::STANDARD.decode(text.trim().as_bytes()).map_err(|e| e.to_string())?;
            let bytes = match compression {
                None | Some("") => raw,
                Some("zlib") => {
                    let mut out = Vec::new();
                    flate2::read::ZlibDecoder::new(&raw[..]).read_to_end(&mut out).map_err(|e| e.to_string())?;
                    out
                }
                Some("gzip") => {
                    let mut out = Vec::new();
                    flate2::read::GzDecoder::new(&raw[..]).read_to_end(&mut out).map_err(|e| e.to_string())?;
                    out
                }
                Some(other) => return Err(format!("{other} compression isn't supported yet")),
            };
            Ok(bytes.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect())
        }
        _ => Err("unknown tile data encoding".into()),
    }
}

/// GIDs laid out `width` wide from (x0, y0) -> cells, empty ones left out.
fn cells_of(gids: &[u32], width: i32, x0: i32, y0: i32) -> Vec<(i32, i32, u32)> {
    gids.iter().enumerate().filter(|(_, g)| **g & GID_MASK != 0).map(|(i, g)| (x0 + i as i32 % width.max(1), y0 + i as i32 / width.max(1), *g)).collect()
}

fn image_size(file: &Path, w: Option<f32>, h: Option<f32>) -> [u32; 2] {
    match (w, h) {
        (Some(w), Some(h)) => [w as u32, h as u32],
        _ => imagesize::size(file).map(|s| [s.width as u32, s.height as u32]).unwrap_or([0, 0]),
    }
}

fn tileset_xml(t: &Tag, first: u32, dir: &Path) -> Tileset {
    let tile = [t.num("tilewidth").unwrap_or(16.0) as u32, t.num("tileheight").unwrap_or(16.0) as u32];
    let image = t.child("image").and_then(|i| {
        let file = dir.join(i.attr("source")?);
        Some(TileImage { size: image_size(&file, i.num("width"), i.num("height")), file })
    });
    let mut images = HashMap::new();
    let mut solid = Vec::new();
    for tile_tag in t.all("tile") {
        let Some(id) = tile_tag.num("id").map(|v| v as u32) else { continue };
        if let Some(i) = tile_tag.child("image") {
            if let Some(src) = i.attr("source") {
                let file = dir.join(src);
                images.insert(id, TileImage { size: image_size(&file, i.num("width"), i.num("height")), file });
            }
        }
        let props = tile_tag.child("properties").map(|p| p.all("property").any(|p| SOLID_PROPERTIES.contains(&p.attr("name").unwrap_or("").to_ascii_lowercase().as_str()) && truthy(p.attr("value").unwrap_or("true")))).unwrap_or(false);
        if props || tile_tag.child("objectgroup").is_some() {
            solid.push(id);
        }
    }
    let width = image.as_ref().map(|i| i.size[0]).unwrap_or(0);
    let margin = t.num("margin").unwrap_or(0.0) as u32;
    let spacing = t.num("spacing").unwrap_or(0.0) as u32;
    let columns = t.num("columns").map(|c| c as u32).filter(|c| *c > 0).unwrap_or_else(|| if tile[0] > 0 { ((width.saturating_sub(margin) + spacing) / (tile[0] + spacing)).max(1) } else { 1 });
    Tileset { first, tile, margin, spacing, columns, image, images, solid }
}

fn tileset_json(t: &serde_json::Value, first: u32, dir: &Path) -> Tileset {
    let n = |k: &str| t[k].as_f64().map(|v| v as f32);
    let tile = [n("tilewidth").unwrap_or(16.0) as u32, n("tileheight").unwrap_or(16.0) as u32];
    let image = t["image"].as_str().map(|src| {
        let file = dir.join(src);
        TileImage { size: image_size(&file, n("imagewidth"), n("imageheight")), file }
    });
    let mut images = HashMap::new();
    let mut solid = Vec::new();
    for tile_v in t["tiles"].as_array().into_iter().flatten() {
        let Some(id) = tile_v["id"].as_u64().map(|v| v as u32) else { continue };
        if let Some(src) = tile_v["image"].as_str() {
            let file = dir.join(src);
            let size = image_size(&file, tile_v["imagewidth"].as_f64().map(|v| v as f32), tile_v["imageheight"].as_f64().map(|v| v as f32));
            images.insert(id, TileImage { file, size });
        }
        let props = tile_v["properties"].as_array().into_iter().flatten().any(|p| {
            SOLID_PROPERTIES.contains(&p["name"].as_str().unwrap_or("").to_ascii_lowercase().as_str()) && (p["value"].as_bool() == Some(true) || p["value"].as_str().is_some_and(truthy))
        });
        if props || tile_v.get("objectgroup").is_some() {
            solid.push(id);
        }
    }
    let margin = n("margin").unwrap_or(0.0) as u32;
    let spacing = n("spacing").unwrap_or(0.0) as u32;
    let columns = n("columns").map(|c| c as u32).filter(|c| *c > 0).unwrap_or(1);
    Tileset { first, tile, margin, spacing, columns, image, images, solid }
}

/// A tileset in its own file (.tsx or .tsj).
fn external_tileset(file: &Path, first: u32) -> Option<Tileset> {
    let text = fs::read_to_string(file).ok()?;
    let dir = file.parent().unwrap_or(Path::new(""));
    if text.trim_start().starts_with('{') {
        Some(tileset_json(&serde_json::from_str(&text).ok()?, first, dir))
    } else {
        Some(tileset_xml(&parse_xml(&text)?, first, dir))
    }
}

fn read_tmx(file: &Path) -> Result<Map, String> {
    let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
    let root = parse_xml(&text).filter(|t| t.name == "map").ok_or("not a Tiled map")?;
    let dir = file.parent().unwrap_or(Path::new(""));
    if root.attr("orientation").is_some_and(|o| o != "orthogonal") {
        return Err(format!("{} maps aren't supported yet", root.attr("orientation").unwrap_or("")));
    }
    let mut tilesets = Vec::new();
    for t in root.all("tileset") {
        let first = t.num("firstgid").unwrap_or(1.0) as u32;
        match t.attr("source") {
            Some(src) => tilesets.extend(external_tileset(&dir.join(src), first)),
            None => tilesets.push(tileset_xml(t, first, dir)),
        }
    }
    let mut layers = Vec::new();
    // Groups hold layers too.
    fn collect(tag: &Tag, layers: &mut Vec<Layer>, offset: [f32; 2]) -> Result<(), String> {
        for c in &tag.children {
            let visible = c.attr("visible") != Some("0");
            let off = [offset[0] + c.num("offsetx").unwrap_or(0.0), offset[1] + c.num("offsety").unwrap_or(0.0)];
            match c.name.as_str() {
                "layer" => {
                    let width = c.num("width").unwrap_or(0.0) as i32;
                    let Some(data) = c.child("data") else { continue };
                    let (enc, comp) = (data.attr("encoding"), data.attr("compression"));
                    let mut cells = Vec::new();
                    let chunks: Vec<&Tag> = data.all("chunk").collect();
                    if chunks.is_empty() {
                        let gids = if enc.is_none() { data.all("tile").map(|t| t.num("gid").unwrap_or(0.0) as u32).collect() } else { decode_gids(&data.text, enc, comp)? };
                        cells = cells_of(&gids, width, 0, 0);
                    }
                    for ch in chunks {
                        let gids = if enc.is_none() { ch.all("tile").map(|t| t.num("gid").unwrap_or(0.0) as u32).collect() } else { decode_gids(&ch.text, enc, comp)? };
                        cells.extend(cells_of(&gids, ch.num("width").unwrap_or(16.0) as i32, ch.num("x").unwrap_or(0.0) as i32, ch.num("y").unwrap_or(0.0) as i32));
                    }
                    layers.push(Layer::Tiles { name: c.attr("name").unwrap_or("Tiles").into(), visible, offset: off, cells });
                }
                "objectgroup" => {
                    let objects = c
                        .all("object")
                        .map(|o| MapObject {
                            name: o.attr("name").unwrap_or("").into(),
                            kind: o.attr("type").or(o.attr("class")).unwrap_or("").into(),
                            x: o.num("x").unwrap_or(0.0) + off[0],
                            y: o.num("y").unwrap_or(0.0) + off[1],
                            w: o.num("width").unwrap_or(0.0),
                            h: o.num("height").unwrap_or(0.0),
                            gid: o.num("gid").map(|g| g as u32),
                            point: o.child("point").is_some(),
                            visible: o.attr("visible") != Some("0"),
                        })
                        .collect();
                    layers.push(Layer::Objects { name: c.attr("name").unwrap_or("Objects").into(), visible, objects });
                }
                "group" => collect(c, layers, off)?,
                _ => {}
            }
        }
        Ok(())
    }
    collect(&root, &mut layers, [0.0, 0.0])?;
    Ok(Map {
        tile: [root.num("tilewidth").unwrap_or(16.0) as u32, root.num("tileheight").unwrap_or(16.0) as u32],
        background: root.attr("backgroundcolor").map(argb_to_rgb),
        tilesets,
        layers,
    })
}

/// Tiled writes "#aarrggbb" or "#rrggbb".
fn argb_to_rgb(c: &str) -> String {
    let hex = c.trim_start_matches('#');
    format!("#{}", if hex.len() == 8 { &hex[2..] } else { hex }).to_ascii_lowercase()
}

fn read_tmj(file: &Path) -> Result<Map, String> {
    let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
    let root: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if root["type"].as_str() != Some("map") && root.get("tiledversion").is_none() {
        return Err("not a Tiled map".into());
    }
    if root["orientation"].as_str().is_some_and(|o| o != "orthogonal") {
        return Err(format!("{} maps aren't supported yet", root["orientation"].as_str().unwrap_or("")));
    }
    let dir = file.parent().unwrap_or(Path::new(""));
    let mut tilesets = Vec::new();
    for t in root["tilesets"].as_array().into_iter().flatten() {
        let first = t["firstgid"].as_u64().unwrap_or(1) as u32;
        match t["source"].as_str() {
            Some(src) => tilesets.extend(external_tileset(&dir.join(src), first)),
            None => tilesets.push(tileset_json(t, first, dir)),
        }
    }
    fn gids_of(v: &serde_json::Value, enc: Option<&str>, comp: Option<&str>) -> Result<Vec<u32>, String> {
        match &v["data"] {
            serde_json::Value::Array(a) => Ok(a.iter().map(|g| g.as_u64().unwrap_or(0) as u32).collect()),
            serde_json::Value::String(s) => decode_gids(s, enc.or(Some("base64")), comp),
            _ => Ok(Vec::new()),
        }
    }
    fn collect(list: &serde_json::Value, layers: &mut Vec<Layer>, offset: [f32; 2]) -> Result<(), String> {
        for l in list.as_array().into_iter().flatten() {
            let visible = l["visible"].as_bool() != Some(false);
            let off = [offset[0] + l["offsetx"].as_f64().unwrap_or(0.0) as f32, offset[1] + l["offsety"].as_f64().unwrap_or(0.0) as f32];
            let name = l["name"].as_str().unwrap_or("").to_string();
            match l["type"].as_str() {
                Some("tilelayer") => {
                    let (enc, comp) = (l["encoding"].as_str(), l["compression"].as_str());
                    let mut cells = Vec::new();
                    if let Some(chunks) = l["chunks"].as_array() {
                        for ch in chunks {
                            let gids = gids_of(ch, enc, comp)?;
                            cells.extend(cells_of(&gids, ch["width"].as_i64().unwrap_or(16) as i32, ch["x"].as_i64().unwrap_or(0) as i32, ch["y"].as_i64().unwrap_or(0) as i32));
                        }
                    } else {
                        cells = cells_of(&gids_of(l, enc, comp)?, l["width"].as_i64().unwrap_or(0) as i32, 0, 0);
                    }
                    layers.push(Layer::Tiles { name, visible, offset: off, cells });
                }
                Some("objectgroup") => {
                    let objects = l["objects"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|o| {
                            let f = |k: &str| o[k].as_f64().unwrap_or(0.0) as f32;
                            MapObject {
                                name: o["name"].as_str().unwrap_or("").into(),
                                kind: o["type"].as_str().or(o["class"].as_str()).unwrap_or("").into(),
                                x: f("x") + off[0],
                                y: f("y") + off[1],
                                w: f("width"),
                                h: f("height"),
                                gid: o["gid"].as_u64().map(|g| g as u32),
                                point: o["point"].as_bool() == Some(true),
                                visible: o["visible"].as_bool() != Some(false),
                            }
                        })
                        .collect();
                    layers.push(Layer::Objects { name, visible, objects });
                }
                Some("group") => collect(&l["layers"], layers, off)?,
                _ => {}
            }
        }
        Ok(())
    }
    let mut layers = Vec::new();
    collect(&root["layers"], &mut layers, [0.0, 0.0])?;
    Ok(Map {
        tile: [root["tilewidth"].as_u64().unwrap_or(16) as u32, root["tileheight"].as_u64().unwrap_or(16) as u32],
        background: root["backgroundcolor"].as_str().map(argb_to_rgb),
        tilesets,
        layers,
    })
}

const SOLID_LAYER: &[&str] = &["collision", "collisions", "collide", "solid", "solids", "wall", "walls", "ground", "platform", "platforms", "terrain", "blocks", "obstacles"];
const TRIGGER_WORDS: &[&str] = &["coin", "item", "pickup", "goal", "exit", "finish", "trigger", "checkpoint", "door", "portal", "key", "spawn", "start", "warp", "teleport", "end"];

fn has_word(text: &str, list: &[&str]) -> bool {
    tokens(text).iter().any(|t| list.contains(&t.as_str()) || list.iter().any(|l| t == &format!("{l}s")))
}

struct MapCtx<'a> {
    /// Game files on disk -> GameX asset path, for images already brought in.
    images: &'a mut HashMap<PathBuf, String>,
    dir: &'a Path,
    source: &'a str,
    things: &'a [Thing],
    taken: std::collections::HashSet<String>,
}

impl MapCtx<'_> {
    fn image(&mut self, file: &Path) -> Option<String> {
        let key = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        if let Some(a) = self.images.get(&key) {
            return Some(a.clone());
        }
        // An image outside the files read (beside the map): brought in now.
        let name = file.file_name()?.to_string_lossy().into_owned();
        let asset = format!("textures/{}/maps/{name}", self.source);
        let target = self.dir.join("assets").join(&asset);
        fs::create_dir_all(target.parent()?).ok()?;
        fs::copy(file, &target).ok()?;
        self.images.insert(key, asset.clone());
        Some(asset)
    }

    fn unique(&mut self, name: &str) -> String {
        let base = project::slug(name);
        let base = if base.is_empty() { "object".to_string() } else { base };
        let mut id = base.clone();
        let mut n = 2;
        while !self.taken.insert(id.clone()) {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    /// The sheet for a GID's tile within `sheets` (added once), and its column and row.
    fn sheet(&mut self, map: &Map, gid: u32, sheets: &mut Vec<TileSheet>, index: &mut HashMap<(usize, Option<u32>), i32>) -> Option<(i32, i32, i32, usize, u32)> {
        let g = gid & GID_MASK;
        let ts_i = map.tilesets.iter().enumerate().filter(|(_, t)| t.first <= g).max_by_key(|(_, t)| t.first)?.0;
        let ts = &map.tilesets[ts_i];
        let local = g - ts.first;
        let (key, image, tile, col, row) = if let Some(img) = ts.images.get(&local) {
            ((ts_i, Some(local)), img.clone(), img.size, 0, 0)
        } else {
            let img = ts.image.clone()?;
            ((ts_i, None), img, ts.tile, (local % ts.columns) as i32, (local / ts.columns) as i32)
        };
        let s = match index.get(&key) {
            Some(s) => *s,
            None => {
                let asset = self.image(&image.file)?;
                let collection = key.1.is_some();
                sheets.push(TileSheet {
                    asset,
                    tile,
                    margin: if collection { [0, 0] } else { [ts.margin; 2] },
                    separation: if collection { [0, 0] } else { [ts.spacing; 2] },
                    solid: Vec::new(),
                });
                index.insert(key, sheets.len() as i32 - 1);
                sheets.len() as i32 - 1
            }
        };
        Some((s, col, row, ts_i, local))
    }
}

fn flags(gid: u32) -> i32 {
    (if gid & FLIP_H != 0 { 1 } else { 0 }) | (if gid & FLIP_V != 0 { 2 } else { 0 }) | (if gid & FLIP_D != 0 { 4 } else { 0 })
}

/// A Tiled map as a 2D scene: one entity per tile layer, one per object.
fn map_scene(map: &Map, id: &str, name: &str, ctx: &mut MapCtx) -> Scene {
    let [tw, th] = map.tile;
    let cell = [tw as f32 / PPU, th as f32 / PPU];
    let mut entities = Vec::new();
    for layer in &map.layers {
        match layer {
            Layer::Tiles { name, visible, offset, cells } => {
                let solid_layer = has_word(name, SOLID_LAYER);
                let mut sheets = Vec::new();
                let mut index = HashMap::new();
                let mut out = Vec::new();
                for &(x, y, gid) in cells {
                    let Some((s, col, row, ts, local)) = ctx.sheet(map, gid, &mut sheets, &mut index) else { continue };
                    let solid = solid_layer || map.tilesets[ts].solid.contains(&local);
                    if solid && !sheets[s as usize].solid.contains(&[col as u32, row as u32]) {
                        sheets[s as usize].solid.push([col as u32, row as u32]);
                    }
                    // Tiled counts rows down from the top; GameX's y goes up.
                    out.push([x, -y - 1, s, col, row, flags(gid)]);
                }
                if out.is_empty() {
                    continue;
                }
                let eid = ctx.unique(name);
                let mut e = Entity::new(&eid, name);
                e.active = *visible;
                e.transform.position = [offset[0] / PPU, -offset[1] / PPU, 0.0];
                e.parts.push(Part::Tiles { cell, sheets, cells: out });
                entities.push(e);
            }
            Layer::Objects { name: layer_name, visible, objects } => {
                let gid = ctx.unique(layer_name);
                let mut group = Entity::new(&gid, layer_name);
                group.active = *visible;
                for o in objects {
                    let label = if o.name.is_empty() { if o.kind.is_empty() { "Object".to_string() } else { o.kind.clone() } } else { o.name.clone() };
                    let oid = ctx.unique(&label);
                    let mut e = Entity::new(&oid, &label);
                    e.active = o.visible;
                    let wanted = [project::slug(&o.name), project::slug(&o.kind)];
                    let thing = ctx.things.iter().find(|t| wanted.iter().any(|w| !w.is_empty() && (*w == t.id || *w == project::slug(&t.name))));
                    if let Some(t) = thing {
                        // Placed where the object's middle is (a tile object's y is its bottom).
                        let cy = if o.gid.is_some() { o.y - o.h / 2.0 } else { o.y + o.h / 2.0 };
                        e.thing = Some(t.id.clone());
                        e.transform.position = [(o.x + o.w / 2.0) / PPU, -cy / PPU, 0.0];
                    } else if let Some(g) = o.gid {
                        // A tile placed as an object: one cell, its bottom-left at (x, y).
                        let mut sheets = Vec::new();
                        let mut index = HashMap::new();
                        if let Some((s, col, row, _, _)) = ctx.sheet(map, g, &mut sheets, &mut index) {
                            let size = if o.w > 0.0 && o.h > 0.0 { [o.w / PPU, o.h / PPU] } else { cell };
                            e.transform.position = [o.x / PPU, -o.y / PPU, 0.0];
                            e.parts.push(Part::Tiles { cell: size, sheets, cells: vec![[0, 0, s, col, row, flags(g)]] });
                        }
                    } else if o.w > 0.0 && o.h > 0.0 && !o.point {
                        let trigger = has_word(&format!("{} {}", o.name, o.kind), TRIGGER_WORDS);
                        e.transform.position = [(o.x + o.w / 2.0) / PPU, -(o.y + o.h / 2.0) / PPU, 0.0];
                        e.parts.push(Part::Collider { shape: ShapeKind::Box, size: [o.w / PPU, o.h / PPU, 1.0], body: Body::Static, trigger });
                    } else {
                        // A point: a spawn place, kept as a marker.
                        e.transform.position = [o.x / PPU, -o.y / PPU, 0.0];
                    }
                    group.children.push(e);
                }
                if !group.children.is_empty() {
                    entities.push(group);
                }
            }
        }
    }
    Scene {
        format: FORMAT,
        id: id.into(),
        name: name.into(),
        dimensions: Dimensions::TwoD,
        environment: Environment { background: map.background.clone().unwrap_or_else(|| "#20263a".into()), ..Environment::default() },
        entities,
    }
}

// ---------------------------------------------------------------------------
// Levels captured from the running game

/// What a capture made.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Captured {
    pub scene: String,
    pub objects: usize,
    /// Objects placed as one of the layer's things.
    pub matched: usize,
    /// The game's screen behind them: "aligned" (by the picker's calibration),
    /// "assumed" (game units taken as screen pixels), or "none".
    pub backdrop: String,
}

/// The game's screen at the moment of capture: a PNG, and how the game's
/// coordinates map onto its pixels (screen = a * game + b), when known.
pub struct Snapshot {
    pub png: Vec<u8>,
    pub calibration: Option<[f64; 4]>,
}

/// The thing a game class stands for: "com.g.ZombieEnemy" -> "zombie" (the longest thing named in it).
fn thing_for_class<'a>(class: &str, things: &'a [Thing]) -> Option<&'a Thing> {
    let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
    let words = tokens(simple);
    let joined = words.join("");
    things
        .iter()
        .filter(|t| {
            let id = t.id.replace('-', "");
            id.len() >= 3 && joined.contains(&id)
        })
        .max_by_key(|t| t.id.len())
}

/// Builds a scene from where the running game's objects are (game units, y down),
/// one group per class, and saves it in the layer at `dir` as one undoable step.
pub fn capture(dir: &Path, positions: &[(String, f64, f64)], ppu: f32, snapshot: Option<&Snapshot>) -> io::Result<Captured> {
    let index = project::open(dir)?;
    let things: Vec<Thing> = index.things.iter().filter_map(|t| project::load_thing(dir, &t.id).ok()).collect();
    let mut taken: std::collections::HashSet<String> = Default::default();
    let mut unique = |name: &str| {
        let base = project::slug(name);
        let base = if base.is_empty() { "object".to_string() } else { base };
        let mut id = base.clone();
        let mut n = 2;
        while !taken.insert(id.clone()) {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    };
    let mut groups: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    for (class, x, y) in positions {
        groups.entry(class.clone()).or_default().push((*x, *y));
    }
    let mut entities = Vec::new();
    let mut matched = 0;
    for (class, list) in &groups {
        let simple = class.rsplit(['.', '$']).next().unwrap_or(class).to_string();
        let thing = thing_for_class(class, &things);
        let gid = unique(&format!("{simple} group"));
        let mut group = Entity::new(&gid, &format!("{simple} ({})", list.len()));
        for (i, (x, y)) in list.iter().enumerate() {
            let id = unique(&simple);
            let mut e = Entity::new(&id, &format!("{simple} {}", i + 1));
            e.transform.position = [(*x as f32 / ppu * 1000.0).round() / 1000.0, (-*y as f32 / ppu * 1000.0).round() / 1000.0 + 0.0, 0.0];
            match thing {
                Some(t) => {
                    e.thing = Some(t.id.clone());
                    matched += 1;
                }
                // Not one of the layer's things yet: a marker to see and select.
                None => e.parts.push(Part::Shape { shape: ShapeKind::Box, size: [0.3, 0.3, 0.05], color: "#7a8cff".into() }),
            }
            let mut values = BTreeMap::new();
            values.insert("class".into(), Value::Text(class.clone()));
            e.parts.push(Part::Custom { name: "Game object".into(), values });
            group.children.push(e);
        }
        entities.push(group);
    }
    let base = "captured";
    let mut id = base.to_string();
    let mut n = 2;
    while index.scenes.iter().any(|s| s.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    let name = if id == base { "Captured level".to_string() } else { format!("Captured level {}", n - 1) };

    // The game's screen as the level's backdrop, under its objects, lined up with them.
    let mut backdrop = "none".to_string();
    if let Some(shot) = snapshot.filter(|s| !s.png.is_empty()) {
        if let Ok(size) = imagesize::blob_size(&shot.png) {
            let (w, h) = (size.width as f64, size.height as f64);
            let [ax, bx, ay, by] = shot.calibration.filter(|c| c[0].abs() > 1e-9 && c[2].abs() > 1e-9).unwrap_or([1.0, 0.0, 1.0, 0.0]);
            backdrop = if shot.calibration.is_some() { "aligned" } else { "assumed" }.into();
            let source = index.info.sources.first().map(|s| s.id.clone()).unwrap_or_else(|| "captured".into());
            let asset = format!("textures/{source}/captured/{id}.png");
            let target = dir.join("assets").join(&asset);
            fs::create_dir_all(target.parent().unwrap_or(dir))?;
            fs::write(&target, &shot.png)?;
            // The picture's middle and size, in the game's units, then in metres.
            let (cx, cy) = ((w / 2.0 - bx) / ax, (h / 2.0 - by) / ay);
            let (gw, gh) = (w / ax.abs(), h / ay.abs());
            let r = |v: f64| ((v / ppu as f64) * 1000.0).round() as f32 / 1000.0;
            let mut e = Entity::new(&unique("screen"), "Game screen (backdrop)");
            e.transform.position = [r(cx), -r(cy) + 0.0, -0.5];
            e.parts.push(Part::Sprite { asset, frame: None, size: Some([r(gw), r(gh)]), flip_x: ax < 0.0 });
            e.tags.push("no-collide".into());
            entities.insert(0, e);
        }
    }
    let scene = Scene {
        format: FORMAT,
        id: id.clone(),
        name: name.clone(),
        dimensions: Dimensions::TwoD,
        environment: Environment { background: "#20263a".into(), ..Environment::default() },
        entities,
    };
    project::save_scene(dir, &scene, &format!("Capture {name} from the running game"))?;
    Ok(Captured { scene: id, objects: positions.len(), matched, backdrop })
}

// ---------------------------------------------------------------------------

/// Translates any game (by its unpacked files) into the GameX project at `dir`.
pub fn translate(unpacked: &Path, dir: &Path, source: &str, name: &str, engine: &str) -> io::Result<Report> {
    let mut report = Report { source: source.into(), engine: engine.into(), ..Default::default() };
    let mut origin: BTreeMap<String, Origin> = BTreeMap::new();
    let files = game_files(unpacked);

    // Assets by kind; images remembered by where they are on disk (maps refer to them so).
    let mut images: Vec<(String, String, [u32; 2])> = Vec::new();
    let mut by_file: HashMap<PathBuf, String> = HashMap::new();
    let mut maps: Vec<(String, PathBuf)> = Vec::new();
    for (rel, path) in &files {
        let lower = rel.to_ascii_lowercase();
        if lower.ends_with(".tmx") || lower.ends_with(".tmj") {
            maps.push((rel.clone(), path.clone()));
            continue;
        }
        let Some(kind) = kind_of(rel) else { continue };
        let asset = format!("{kind}/{source}/{rel}");
        let target = dir.join("assets").join(&asset);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(path, &target)?;
        origin.insert(format!("asset:{asset}"), Origin { file: rel.clone(), object: None, field: None, fidelity: Fidelity::Exact, note: None });
        report.assets += 1;
        if kind == "textures" {
            let size = imagesize::size(path).map(|s| [s.width as u32, s.height as u32]).unwrap_or([100, 100]);
            images.push((rel.clone(), asset.clone(), size));
            by_file.insert(fs::canonicalize(path).unwrap_or_else(|_| path.clone()), asset);
        }
    }
    report.scripts = files.iter().filter(|(r, _)| r.ends_with(".dex") || r.ends_with(".java") || r.ends_with(".kt") || r.ends_with(".lua") || r.ends_with(".js")).count();

    // Things from sprite names.
    let made = things_from_images(&images);
    let mut info = project::open(dir)?.info;
    for (t, asset) in &made {
        write_json(&dir.join("things").join(t.kind.folder()).join(format!("{}.thing.json", t.id)), t)?;
        origin.insert(
            format!("thing:{}", t.id),
            Origin { file: asset.clone(), object: None, field: None, fidelity: Fidelity::Approximated, note: Some("made from image names; kind and role guessed".into()) },
        );
    }
    let things: Vec<Thing> = made.into_iter().map(|(t, _)| t).collect();
    report.things = things.len();

    // Scenes: the game's Tiled maps.
    let mut start = None;
    let mut used = std::collections::HashSet::new();
    let mut ctx = MapCtx { images: &mut by_file, dir, source, things: &things, taken: Default::default() };
    for (rel, path) in &maps {
        let read = if rel.to_ascii_lowercase().ends_with(".tmx") { read_tmx(path) } else { read_tmj(path) };
        let map = match read {
            Ok(m) => m,
            Err(e) => {
                report.problems.push(format!("{rel}: {e}"));
                continue;
            }
        };
        let stem = rel.rsplit('/').next().unwrap_or(rel).rsplit_once('.').map(|(s, _)| s).unwrap_or(rel);
        let base = project::slug(stem);
        let base = if base.is_empty() { "map".to_string() } else { base };
        let mut id = base.clone();
        let mut n = 2;
        while !used.insert(id.clone()) {
            id = format!("{base}-{n}");
            n += 1;
        }
        ctx.taken.clear();
        let scene = map_scene(&map, &id, stem, &mut ctx);
        report.entities += scene.entities.iter().map(|e| e.walk().len()).sum::<usize>();
        write_json(&dir.join("world").join(format!("{id}.scene.json")), &scene)?;
        origin.insert(format!("scene:{id}"), Origin { file: rel.clone(), object: None, field: None, fidelity: Fidelity::Exact, note: None });
        report.scenes += 1;
        start.get_or_insert(id);
    }

    // No maps: the game builds its levels in code. Its things, laid out to see and place.
    if report.scenes == 0 {
        let mut entities = Vec::new();
        let per_row = 8;
        let (mut x, mut y, mut row_h) = (0.0f32, 0.0f32, 0.0f32);
        for (i, t) in things.iter().enumerate() {
            let [w, h] = t.parts.iter().find_map(|p| if let Part::Sprite { size, .. } = p { *size } else { None }).unwrap_or([1.0, 1.0]);
            if i > 0 && i % per_row == 0 {
                x = 0.0;
                y -= row_h + 0.5;
                row_h = 0.0;
            }
            let mut e = Entity::new(&t.id, &t.name);
            e.thing = Some(t.id.clone());
            e.transform.position = [x + w / 2.0, y - h / 2.0, 0.0];
            entities.push(e);
            x += w + 0.5;
            row_h = row_h.max(h);
        }
        report.entities += entities.len();
        let scene = Scene {
            format: FORMAT,
            id: "catalogue".into(),
            name: "Sprite catalogue".into(),
            dimensions: Dimensions::TwoD,
            environment: Environment { background: "#20263a".into(), ..Environment::default() },
            entities,
        };
        write_json(&dir.join("world").join("catalogue.scene.json"), &scene)?;
        origin.insert(
            "scene:catalogue".into(),
            Origin { file: String::new(), object: None, field: None, fidelity: Fidelity::Missing, note: Some("the game builds its levels in code; this scene lays out its things".into()) },
        );
        report.scenes += 1;
        start = Some("catalogue".into());
        report.problems.push("This game builds its levels in code, so no level layout was found in its files. Its sprites are in \"Sprite catalogue\" (not a level): start the real game in Live Preview and use Capture level to bring a level in.".into());
    }

    write_json(&dir.join("sources").join(source).join("origin.json"), &OriginMap { format: FORMAT, source: source.into(), entries: origin })?;
    info.sources.retain(|s| s.id != source);
    info.sources.push(Source { id: source.into(), name: name.into(), engine: engine.into(), engine_version: None, imported_from: Some(unpacked.display().to_string()) });
    if info.settings.start_scene.is_none() {
        info.settings.start_scene = start;
    }
    write_json(&dir.join(project::PROJECT_FILE), &info)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprite_names_make_things() {
        assert_eq!(tokens("zombieWalk_02"), ["zombie", "walk", "02"]);
        assert_eq!(tokens("coin3"), ["coin", "3"]);
        let images: Vec<(String, String, [u32; 2])> = ["zombie_walk1.png", "zombie_walk2.png", "zombie_idle.png", "coin.png", "btn_play.png", "bg_forest.png", "tree_a.png", "tree_b.png", "sky.png"]
            .iter()
            .map(|f| (format!("res/drawable/{f}"), format!("textures/g/res/drawable/{f}"), [64, 32]))
            .collect();
        let things = things_from_images(&images);
        let ids: Vec<&str> = things.iter().map(|(t, _)| t.id.as_str()).collect();
        assert_eq!(ids, ["coin", "tree", "zombie"]);
        let zombie = &things[2].0;
        assert_eq!(zombie.kind, ThingKind::Character);
        assert_eq!(zombie.role.as_deref(), Some("enemy"));
        // The idle frame is its look.
        assert!(matches!(&zombie.parts[0], Part::Sprite { asset, size: Some([w, _]), .. } if asset.ends_with("zombie_idle.png") && (*w - 0.64).abs() < 1e-6));
        assert_eq!(things[0].0.kind, ThingKind::Item);
        assert_eq!(things[1].0.kind, ThingKind::Prop);
    }

    #[test]
    fn android_keeps_the_best_density() {
        let files = ["res/drawable-hdpi/a.png", "res/drawable-xxhdpi/a.png", "res/drawable-fr-nodpi/a.png", "res/drawable-nodpi/b.png", "res/layout/main.xml", "res/raw/s.ogg", "assets/x.png"]
            .iter()
            .map(|r| (r.to_string(), PathBuf::from(r)))
            .collect();
        let kept: Vec<String> = android_best(files).into_iter().map(|(r, _)| r).collect();
        assert_eq!(kept, ["res/drawable-xxhdpi/a.png", "res/drawable-nodpi/b.png", "res/raw/s.ogg", "assets/x.png"]);
    }

    #[test]
    fn reads_xml() {
        let t = parse_xml("<?xml version=\"1.0\"?><!-- hi --><map a='1' b=\"x &amp; y\"><layer name=\"G\"/><data>1,2</data></map>").unwrap();
        assert_eq!(t.name, "map");
        assert_eq!(t.attr("b"), Some("x & y"));
        assert_eq!(t.children.len(), 2);
        assert_eq!(t.child("data").unwrap().text, "1,2");
        assert!(parse_xml("<a><b></a>").is_none());
    }

    #[test]
    fn translates_a_game_with_a_tiled_map() {
        let dir = tempfile::tempdir().unwrap();
        let unpacked = dir.path().join("unpacked");
        let files = unpacked.join("files");
        fs::create_dir_all(files.join("maps")).unwrap();
        // A 2x1 tile sheet, 16 px tiles: tile 0 grass (solid), tile 1 flower.
        fs::write(files.join("maps").join("tiles.png"), png(32, 16)).unwrap();
        fs::write(files.join("coin.png"), png(16, 16)).unwrap();
        fs::write(files.join("jump.ogg"), b"OggS").unwrap();
        fs::write(
            files.join("maps").join("tiles.tsx"),
            r#"<tileset name="t" tilewidth="16" tileheight="16" columns="2"><image source="tiles.png" width="32" height="16"/><tile id="0"><properties><property name="solid" type="bool" value="true"/></properties></tile></tileset>"#,
        )
        .unwrap();
        // Zlib-compressed base64 is also read; CSV here: ground row, a flipped flower, a coin, a goal.
        let flipped = (2u32 | FLIP_H).to_string();
        fs::write(
            files.join("maps").join("level1.tmx"),
            format!(
                r##"<map orientation="orthogonal" width="3" height="2" tilewidth="16" tileheight="16" backgroundcolor="#ff336699">
                 <tileset firstgid="1" source="tiles.tsx"/>
                 <layer name="Level" width="3" height="2"><data encoding="csv">0,{flipped},0,1,1,1</data></layer>
                 <objectgroup name="Things"><object name="Coin" x="16" y="0" width="16" height="16"/><object name="Goal" x="32" y="0" width="16" height="16"/></objectgroup>
                </map>"##
            ),
        )
        .unwrap();

        let root = dir.path().join("projects");
        let out = project::create(&root, "Tiled Test").unwrap();
        let report = translate(&unpacked, &out, "t", "Tiled Test", "Other").unwrap();
        assert_eq!((report.scenes, report.things, report.assets), (1, 1, 3), "{report:?}");
        let scene = project::load_scene(&out, "level1").unwrap();
        assert_eq!(scene.environment.background, "#336699");
        let ground = &scene.entities[0];
        let Part::Tiles { cell, sheets, cells } = &ground.parts[0] else { panic!("no tiles") };
        assert_eq!(*cell, [0.16, 0.16]);
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].solid, vec![[0, 0]]);
        // Row 0 is the top: y -1; the flower flipped; the ground row at y -2.
        assert!(cells.contains(&[1, -1, 0, 1, 0, 1]));
        assert!(cells.contains(&[0, -2, 0, 0, 0, 0]));
        assert_eq!(cells.len(), 4);
        let objects = &scene.entities[1];
        assert_eq!(objects.children[0].thing.as_deref(), Some("coin"));
        assert_eq!(objects.children[0].transform.position, [0.24, -0.08, 0.0]);
        assert!(matches!(objects.children[1].parts[0], Part::Collider { trigger: true, .. }));
        assert_eq!(project::open(&out).unwrap().info.settings.start_scene.as_deref(), Some("level1"));
    }

    #[test]
    fn a_game_without_maps_gets_its_things_laid_out() {
        let dir = tempfile::tempdir().unwrap();
        let res = dir.path().join("unpacked").join("raw").join("res").join("drawable-nodpi-v4");
        fs::create_dir_all(&res).unwrap();
        for f in ["hero_walk1.png", "hero_walk2.png", "gem.png", "menu_bg.png"] {
            fs::write(res.join(f), png(20, 40)).unwrap();
        }
        let out = project::create(&dir.path().join("p"), "Code Game").unwrap();
        let report = translate(&dir.path().join("unpacked"), &out, "c", "Code Game", "Java/Kotlin").unwrap();
        assert_eq!((report.scenes, report.things, report.assets), (1, 2, 4));
        let scene = project::load_scene(&out, "catalogue").unwrap();
        assert_eq!(scene.entities.len(), 2);
        assert_eq!(project::load_thing(&out, "hero").unwrap().role.as_deref(), Some("player"));
    }

    #[test]
    fn captures_a_level_from_object_positions() {
        let dir = tempfile::tempdir().unwrap();
        let out = project::create(dir.path(), "Cap").unwrap();
        let zombie = Thing { tags: Vec::new(), format: FORMAT, id: "zombi".into(), name: "Zombi".into(), kind: ThingKind::Character, role: Some("enemy".into()), description: None, properties: Vec::new(), parts: Vec::new(), behaviors: Vec::new(), children: Vec::new() };
        write_json(&out.join("things").join("characters").join("zombi.thing.json"), &zombie).unwrap();
        let positions = vec![("com.g.ZombiAI".to_string(), 100.0, 50.0), ("com.g.ZombiAI".to_string(), 300.0, 50.0), ("com.g.Lamp".to_string(), 10.0, 20.0)];
        let c = capture(&out, &positions, 100.0, None).unwrap();
        assert_eq!((c.scene.as_str(), c.objects, c.matched), ("captured", 3, 2));
        let scene = project::load_scene(&out, "captured").unwrap();
        let names: Vec<&str> = scene.entities.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Lamp (1)", "ZombiAI (2)"]);
        assert_eq!(scene.entities[1].children[1].transform.position, [3.0, -0.5, 0.0]);
        assert_eq!(scene.entities[1].children[1].thing.as_deref(), Some("zombi"));
        assert!(matches!(scene.entities[0].children[0].parts[0], Part::Shape { .. }));
        // A second capture is a new scene.
        assert_eq!(capture(&out, &positions, 100.0, None).unwrap().scene, "captured-2");
        // With the screen: a backdrop behind the objects, lined up by the calibration
        // (screen = 2 * game + 10): a 400 x 200 picture covers game x -5..195, y -5..95.
        let shot = Snapshot { png: png(400, 200), calibration: Some([2.0, 10.0, 2.0, 10.0]) };
        let c = capture(&out, &positions, 100.0, Some(&shot)).unwrap();
        assert_eq!(c.backdrop, "aligned");
        let scene = project::load_scene(&out, &c.scene).unwrap();
        let back = &scene.entities[0];
        assert_eq!(back.transform.position, [0.95, -0.45, -0.5]);
        assert!(matches!(&back.parts[0], Part::Sprite { size: Some([w, h]), .. } if (*w - 2.0).abs() < 1e-6 && (*h - 1.0).abs() < 1e-6));
        assert!(out.join("assets").join(format!("textures/captured/captured/{}.png", c.scene)).is_file());
        assert_eq!(project::load_scene(&out, "captured-2").unwrap().name, "Captured level 2");
    }

    /// The smallest PNG header imagesize reads.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&[8, 6, 0, 0, 0]);
        v
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// A real game: `GX_GAME_UNPACKED=<an import's unpacked folder> cargo test -- --ignored real_generic_game --nocapture`.
    #[test]
    #[ignore]
    fn real_generic_game() {
        let unpacked = PathBuf::from(std::env::var("GX_GAME_UNPACKED").expect("set GX_GAME_UNPACKED"));
        let root = tempfile::tempdir().unwrap();
        let out = project::create(root.path(), "Real").unwrap();
        let report = translate(&unpacked, &out, "real", "Real", "Java/Kotlin").unwrap();
        println!("{report:#?}");
        for t in project::open(&out).unwrap().things {
            let thing = project::load_thing(&out, &t.id).unwrap();
            println!("  {:?} {} ({:?}) {}", thing.kind, thing.name, thing.role, thing.description.unwrap_or_default());
        }
    }
}
