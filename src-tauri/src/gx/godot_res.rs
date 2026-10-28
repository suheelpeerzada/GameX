//! Godot resources the scene reader needs beyond plain node values: Godot's
//! value syntax (dictionaries, arrays, packed arrays), resource files
//! (.tres), meshes stored in Godot's own array format (written out as GLB),
//! tile sets and tile maps (2D level layouts) and mesh libraries (the cells
//! of a GridMap, 3D level layouts). Godot 3 and 4.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;

use super::glb;
use crate::scene::parse_sections;

// ---------------------------------------------------------------------------
// Godot's value syntax

#[derive(Debug, Clone, PartialEq)]
pub enum V {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    /// `Vector2(1, 2)`, `ExtResource("1")`, `PackedByteArray(...)`, `Array[int]([...])`.
    Call(String, Vec<V>),
    Array(Vec<V>),
    Dict(Vec<(V, V)>),
    Ident(String),
}

impl V {
    pub fn num(&self) -> Option<f64> {
        match self {
            V::Num(n) => Some(*n),
            V::Bool(b) => Some(*b as u8 as f64),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            V::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A dictionary entry by its string key.
    pub fn get(&self, key: &str) -> Option<&V> {
        match self {
            V::Dict(entries) => entries.iter().find(|(k, _)| k.str() == Some(key)).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn list(&self) -> &[V] {
        match self {
            V::Array(items) => items,
            // `Array[int]([1, 2])`: a typed array wraps a plain one.
            V::Call(name, args) if name.starts_with("Array") => args.first().map(|a| a.list()).unwrap_or(&[]),
            _ => &[],
        }
    }

    /// The numbers inside a call (`Vector3(1, 2, 3)`, `AABB(...)`, `PackedInt32Array(...)`).
    pub fn numbers(&self) -> Vec<f64> {
        match self {
            V::Call(_, args) => args.iter().filter_map(V::num).collect(),
            V::Array(items) => items.iter().filter_map(V::num).collect(),
            _ => Vec::new(),
        }
    }

    /// A packed byte array, written as numbers (Godot 3, 4.0-4.2) or base64 (4.3+).
    pub fn bytes(&self) -> Option<Vec<u8>> {
        match self {
            V::Call(name, args) if name.contains("ByteArray") => match args.as_slice() {
                [V::Str(b64)] => base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok(),
                list => Some(list.iter().filter_map(V::num).map(|n| n as u8).collect()),
            },
            _ => None,
        }
    }

    /// `ExtResource("id")` / `SubResource("id")` -> (kind, id).
    pub fn reference(&self) -> Option<(&str, String)> {
        match self {
            V::Call(name, args) if name == "ExtResource" || name == "SubResource" => {
                let id = match args.first()? {
                    V::Str(s) => s.clone(),
                    V::Num(n) => format!("{}", *n as i64),
                    _ => return None,
                };
                Some((name.as_str(), id))
            }
            _ => None,
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while self.at < self.s.len() && (self.s[self.at] as char).is_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_space();
        self.s.get(self.at).copied()
    }

    fn value(&mut self) -> Option<V> {
        match self.peek()? {
            b'"' => self.string().map(V::Str),
            // StringName (&"a") and NodePath (^"a") read as strings.
            b'&' | b'^' => {
                self.at += 1;
                self.string().map(V::Str)
            }
            b'[' => {
                self.at += 1;
                Some(V::Array(self.items(b']')?))
            }
            b'{' => {
                self.at += 1;
                let mut entries = Vec::new();
                loop {
                    if self.peek()? == b'}' {
                        self.at += 1;
                        break;
                    }
                    let k = self.value()?;
                    if self.peek()? != b':' {
                        return None;
                    }
                    self.at += 1;
                    let v = self.value()?;
                    entries.push((k, v));
                    match self.peek()? {
                        b',' => self.at += 1,
                        b'}' => {}
                        _ => return None,
                    }
                }
                Some(V::Dict(entries))
            }
            c if c == b'-' || c == b'+' || c == b'.' || c.is_ascii_digit() => {
                let start = self.at;
                while self.at < self.s.len() && matches!(self.s[self.at], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
                    self.at += 1;
                }
                std::str::from_utf8(&self.s[start..self.at]).ok()?.parse().ok().map(V::Num)
            }
            _ => {
                let start = self.at;
                while self.at < self.s.len() && (self.s[self.at].is_ascii_alphanumeric() || self.s[self.at] == b'_') {
                    self.at += 1;
                }
                // Typed arrays and dictionaries: `Array[int]`, `Dictionary[String, int]`.
                let word = &self.s[start..self.at];
                if self.s.get(self.at) == Some(&b'[') && (word.starts_with(b"Array") || word.starts_with(b"Dictionary")) {
                    while self.at < self.s.len() && self.s[self.at] != b']' {
                        self.at += 1;
                    }
                    self.at += 1;
                }
                let word = std::str::from_utf8(&self.s[start..self.at]).ok()?.to_string();
                if word.is_empty() {
                    return None;
                }
                if self.peek() == Some(b'(') {
                    self.at += 1;
                    return Some(V::Call(word, self.items(b')')?));
                }
                Some(match word.as_str() {
                    "true" => V::Bool(true),
                    "false" => V::Bool(false),
                    "null" | "nil" => V::Null,
                    "inf" => V::Num(f64::INFINITY),
                    _ => V::Ident(word),
                })
            }
        }
    }

    fn items(&mut self, close: u8) -> Option<Vec<V>> {
        let mut out = Vec::new();
        loop {
            if self.peek()? == close {
                self.at += 1;
                return Some(out);
            }
            out.push(self.value()?);
            match self.peek()? {
                b',' => self.at += 1,
                c if c == close => {}
                _ => return None,
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        self.at += 1;
        let mut out = Vec::new();
        while self.at < self.s.len() {
            let c = self.s[self.at];
            self.at += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.s.get(self.at)?;
                    self.at += 1;
                    out.push(match e {
                        b'n' => b'\n',
                        b't' => b'\t',
                        other => other,
                    });
                }
                _ => out.push(c),
            }
        }
        None
    }
}

/// Parses one Godot value as written in a .tscn/.tres file.
pub fn parse(text: &str) -> Option<V> {
    Parser { s: text.as_bytes(), at: 0 }.value()
}

// ---------------------------------------------------------------------------
// Resource files

/// A .tres (or a .tscn's resources): its external and internal resources
/// and the main resource's properties.
#[derive(Default)]
pub struct ResFile {
    pub res: String,
    pub ext: HashMap<String, (String, String)>,
    pub sub: HashMap<String, (String, HashMap<String, String>)>,
    /// The `[resource]` section; empty in a scene.
    pub main: HashMap<String, String>,
    pub main_type: String,
}

impl ResFile {
    pub fn read(res: &str, text: &str) -> ResFile {
        let mut f = ResFile { res: res.into(), ..Default::default() };
        for s in parse_sections(text) {
            let props = || s.properties.iter().map(|p| (p.key.clone(), p.value.clone())).collect::<HashMap<_, _>>();
            match s.tag.as_str() {
                "ext_resource" => {
                    let id = s.attr("id").unwrap_or_default().trim_matches('"').to_string();
                    f.ext.insert(id, (s.attr("path").unwrap_or_default().to_string(), s.attr("type").unwrap_or_default().to_string()));
                }
                "sub_resource" => {
                    let id = s.attr("id").unwrap_or_default().trim_matches('"').to_string();
                    f.sub.insert(id, (s.attr("type").unwrap_or_default().to_string(), props()));
                }
                "gd_resource" => f.main_type = s.attr("type").unwrap_or_default().to_string(),
                "resource" => f.main = props(),
                _ => {}
            }
        }
        f
    }
}

/// Resource files read so far, by res:// path. Binary resources (.res, .mesh,
/// .scn) are read from their text twin when GDRE Tools made one.
pub struct Resources {
    root: PathBuf,
    files: HashMap<String, Option<std::rc::Rc<ResFile>>>,
}

/// A resource value resolved to where it lives: the file and either its
/// main resource (`None`) or one of its sub-resources.
#[derive(Clone)]
pub struct Found {
    pub file: std::rc::Rc<ResFile>,
    pub sub: Option<String>,
}

impl Found {
    pub fn kind(&self) -> &str {
        match &self.sub {
            Some(id) => self.file.sub.get(id).map(|(t, _)| t.as_str()).unwrap_or(""),
            None => &self.file.main_type,
        }
    }

    pub fn props(&self) -> &HashMap<String, String> {
        match &self.sub {
            Some(id) => self.file.sub.get(id).map(|(_, p)| p).unwrap_or(&self.file.main),
            None => &self.file.main,
        }
    }

    /// A stable key for caching what was made from it.
    pub fn key(&self) -> String {
        format!("{}#{}", self.file.res, self.sub.as_deref().unwrap_or(""))
    }
}

impl Resources {
    pub fn new(root: &Path) -> Resources {
        Resources { root: root.to_path_buf(), files: HashMap::new() }
    }

    /// The disk path of a res:// path.
    pub fn path(&self, res: &str) -> PathBuf {
        self.root.join(res.trim_start_matches("res://"))
    }

    pub fn file(&mut self, res: &str) -> Option<std::rc::Rc<ResFile>> {
        if let Some(f) = self.files.get(res) {
            return f.clone();
        }
        let mut candidates = vec![res.to_string()];
        if let Some(stem) = res.strip_suffix(".res").or(res.strip_suffix(".mesh")).or(res.strip_suffix(".material")) {
            candidates.push(format!("{stem}.tres"));
        }
        if let Some(stem) = res.strip_suffix(".scn") {
            candidates.push(format!("{stem}.tscn"));
        }
        let found = candidates
            .iter()
            .find_map(|c| {
                let bytes = fs::read(self.path(c)).ok()?;
                // Binary resources start with RSRC (or RSCC, compressed): not readable as text.
                if bytes.starts_with(b"RSRC") || bytes.starts_with(b"RSCC") {
                    return None;
                }
                Some(ResFile::read(c, &String::from_utf8_lossy(&bytes)))
            })
            .map(std::rc::Rc::new);
        self.files.insert(res.into(), found.clone());
        found
    }

    /// Follows a property value (`SubResource(...)` in `within`, or `ExtResource(...)` to another file).
    pub fn resolve(&mut self, within: &std::rc::Rc<ResFile>, value: &str) -> Option<Found> {
        let v = parse(value)?;
        let (kind, id) = v.reference()?;
        if kind == "SubResource" {
            return within.sub.contains_key(&id).then(|| Found { file: within.clone(), sub: Some(id) });
        }
        let (path, _) = within.ext.get(&id)?;
        let path = path.clone();
        Some(Found { file: self.file(&path)?, sub: None })
    }
}

// ---------------------------------------------------------------------------
// Meshes

/// The albedo colour and texture (res:// path) of a material.
pub fn material_of(found: &Found) -> (Option<[f32; 4]>, Option<String>) {
    let p = found.props().clone();
    let color = p.get("albedo_color").and_then(|c| parse(c)).map(|c| c.numbers()).filter(|c| c.len() >= 3).map(|c| [c[0] as f32, c[1] as f32, c[2] as f32, c.get(3).copied().unwrap_or(1.0) as f32]);
    let texture = p.get("albedo_texture").and_then(|t| {
        let v = parse(t)?;
        let (kind, id) = v.reference()?;
        (kind == "ExtResource").then(|| found.file.ext.get(&id).map(|(path, _)| path.clone())).flatten()
    });
    (color, texture)
}

fn read_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn read_f32(b: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn half(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1F) as i32;
    let frac = (h & 0x3FF) as f32;
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => 0.0,
        e => (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

/// Godot draws clockwise faces as the front; glTF counter-clockwise.
fn indices(data: Option<Vec<u8>>, count: usize, vertices: usize) -> Vec<u32> {
    let mut out: Vec<u32> = match data {
        Some(d) if count > 0 && d.len() >= count * 4 && vertices > 65535 => (0..count).filter_map(|i| Some(u32::from_le_bytes(d.get(i * 4..i * 4 + 4)?.try_into().ok()?))).collect(),
        Some(d) if count > 0 => (0..count).filter_map(|i| read_u16(&d, i * 2).map(u32::from)).collect(),
        _ => (0..vertices as u32).collect(),
    };
    out.truncate(out.len() / 3 * 3);
    for t in out.chunks_mut(3) {
        t.swap(1, 2);
    }
    out
}

/// One surface of an ArrayMesh, Godot 4 (`_surfaces` entries).
fn surface_g4(s: &V) -> Option<(Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>)> {
    let format = s.get("format")?.num()? as u64;
    if s.get("primitive").and_then(V::num).unwrap_or(3.0) as u32 != 3 {
        return None; // only triangles
    }
    let n = s.get("vertex_count")?.num()? as usize;
    let data = s.get("vertex_data")?.bytes()?;
    if n == 0 || data.len() % n != 0 {
        return None;
    }
    let stride = data.len() / n;
    let compressed = format & (1 << 29) != 0;
    let flat = format & (1 << 25) != 0;
    let aabb = s.get("aabb").map(V::numbers).filter(|a| a.len() == 6).unwrap_or(vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    let mut positions = Vec::with_capacity(n);
    for v in 0..n {
        let at = v * stride;
        positions.push(if compressed {
            let c = |k: usize| read_u16(&data, at + k * 2).map(|u| (aabb[k] + u as f64 / 65535.0 * aabb[k + 3]) as f32);
            [c(0)?, c(1)?, if flat { 0.0 } else { c(2)? }]
        } else if flat {
            [read_f32(&data, at)?, read_f32(&data, at + 4)?, 0.0]
        } else {
            [read_f32(&data, at)?, read_f32(&data, at + 4)?, read_f32(&data, at + 8)?]
        });
    }
    // UVs: in the attribute stream, after the colour.
    let mut uvs = Vec::new();
    if format & 16 != 0 {
        if let Some(attr) = s.get("attribute_data").and_then(V::bytes).filter(|a| !a.is_empty() && a.len() % n == 0) {
            let astride = attr.len() / n;
            let offset = if format & 8 != 0 { 4 } else { 0 };
            let scale = s.get("uv_scale").map(V::numbers).filter(|u| u.len() >= 2 && (u[0] != 0.0 || u[1] != 0.0));
            for v in 0..n {
                let at = v * astride + offset;
                let uv = if compressed {
                    let (u, w) = (read_u16(&attr, at).unwrap_or(0) as f64 / 65535.0, read_u16(&attr, at + 2).unwrap_or(0) as f64 / 65535.0);
                    match &scale {
                        Some(sc) => [((u * 2.0 - 1.0) * sc[0]) as f32, ((w * 2.0 - 1.0) * sc[1]) as f32],
                        None => [u as f32, w as f32],
                    }
                } else {
                    [read_f32(&attr, at).unwrap_or(0.0), read_f32(&attr, at + 4).unwrap_or(0.0)]
                };
                uvs.push(uv);
            }
        }
    }
    let count = s.get("index_count").and_then(V::num).unwrap_or(0.0) as usize;
    let idx = indices(s.get("index_data").and_then(V::bytes), count, n);
    Some((positions, uvs, idx))
}

/// One surface of an ArrayMesh, Godot 3 (`surfaces/N` values): interleaved.
fn surface_g3(s: &V) -> Option<(Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>)> {
    let format = s.get("format")?.num()? as u64;
    if s.get("primitive").and_then(V::num).unwrap_or(4.0) as u32 != 4 {
        return None;
    }
    let n = s.get("vertex_count")?.num()? as usize;
    let data = s.get("array_data")?.bytes()?;
    if n == 0 || data.len() % n != 0 {
        return None;
    }
    let stride = data.len() / n;
    let has = |bit: u64| format & bit != 0;
    let (compress_vertex, flat, octa) = (has(1 << 9), has(1 << 18), has(1 << 21));
    let vertex = match (flat, compress_vertex) {
        (true, true) => 4,
        (true, false) => 8,
        (false, true) => 8,
        (false, false) => 12,
    };
    let normal = if !has(2) { 0 } else if has(1 << 10) { if octa { 2 } else { 4 } } else { 12 };
    let tangent = if !has(4) { 0 } else if has(1 << 11) { if octa { 2 } else { 4 } } else { 16 };
    let color = if !has(8) { 0 } else if has(1 << 12) { 4 } else { 16 };
    let uv_size = if has(1 << 13) { 4 } else { 8 };
    let uv_at = vertex + normal + tangent + color;
    let mut positions = Vec::with_capacity(n);
    let mut uvs = Vec::new();
    for v in 0..n {
        let at = v * stride;
        let c = |k: usize| if compress_vertex { read_u16(&data, at + k * 2).map(half) } else { read_f32(&data, at + k * 4) };
        positions.push([c(0)?, c(1)?, if flat { 0.0 } else { c(2)? }]);
        if has(16) && uv_at + uv_size <= stride {
            let u = at + uv_at;
            uvs.push(if uv_size == 4 { [half(read_u16(&data, u)?), half(read_u16(&data, u + 2)?)] } else { [read_f32(&data, u)?, read_f32(&data, u + 4)?] });
        }
    }
    let count = s.get("index_count").and_then(V::num).unwrap_or(0.0) as usize;
    let idx = indices(s.get("array_index_data").and_then(V::bytes), count, n);
    Some((positions, uvs, idx))
}

/// An ArrayMesh resource as GLB. `texture_uri` turns a texture's res:// path
/// into a path relative to where the GLB will be written.
pub fn array_mesh_glb(res: &mut Resources, mesh: &Found, name: &str, texture_uri: &dyn Fn(&str) -> Option<String>) -> Option<Vec<u8>> {
    let props = mesh.props().clone();
    let mut surfaces = Vec::new();
    // Godot 4: `_surfaces = [{...}, ...]`; Godot 3: `surfaces/0 = {...}`, `surfaces/1`...
    let mut list: Vec<(V, Option<String>)> = Vec::new();
    if let Some(all) = props.get("_surfaces").and_then(|s| parse(s)) {
        for s in all.list() {
            list.push((s.clone(), None));
        }
    }
    let mut keys: Vec<&String> = props.keys().filter(|k| k.starts_with("surfaces/") && !k[9..].contains('/')).collect();
    keys.sort_by_key(|k| k[9..].parse::<u32>().unwrap_or(0));
    for k in keys {
        if let Some(v) = parse(&props[k]) {
            list.push((v, Some(k.clone())));
        }
    }
    for (i, (s, _)) in list.iter().enumerate() {
        let read = if s.get("vertex_data").is_some() { surface_g4(s) } else { surface_g3(s) };
        let Some((positions, uvs, idx)) = read else { continue };
        let mut material = glb::Material { name: format!("{name} {}", i + 1), color: [0.75, 0.75, 0.78, 1.0], texture: None };
        let mat_value = s.get("material").cloned().or_else(|| props.get(&format!("surface_{}/material", i + 1)).and_then(|m| parse(m)));
        if let Some(m) = mat_value.and_then(|m| match m.reference() {
            Some(("SubResource", id)) => Some(format!("SubResource(\"{id}\")")),
            Some((_, id)) => Some(format!("ExtResource(\"{id}\")")),
            None => None,
        }) {
            if let Some(found) = res.resolve(&mesh.file, &m) {
                let (color, texture) = material_of(&found);
                if let Some(c) = color {
                    material.color = c;
                }
                material.texture = texture.as_deref().and_then(texture_uri);
                if material.texture.is_some() && color.is_none() {
                    material.color = [1.0; 4];
                }
            }
        }
        surfaces.push(glb::Surface { positions, normals: Vec::new(), uvs, indices: idx, material });
    }
    glb::write(name, &surfaces).ok()
}

// ---------------------------------------------------------------------------
// Tile maps

/// A tile set's sheets: Godot source/tile ID -> (sheet, how the source's
/// tiles map to columns and rows).
pub struct TileSet {
    /// Pixel size of one cell.
    pub cell: [f64; 2],
    pub sheets: Vec<Sheet>,
    /// Godot 4 source ID or Godot 3 tile ID -> index in `sheets`.
    pub by_id: HashMap<i64, usize>,
}

pub struct Sheet {
    /// res:// path of the image.
    pub texture: String,
    pub tile: [u32; 2],
    pub margin: [u32; 2],
    pub separation: [u32; 2],
    /// Every tile blocks (Godot 3 tiles with shapes), or only these.
    pub all_solid: bool,
    pub solid: Vec<[u32; 2]>,
}

fn ints(v: &str) -> Vec<f64> {
    parse(v).map(|v| v.numbers()).unwrap_or_default()
}

fn ext_path(file: &ResFile, value: &str) -> Option<String> {
    let v = parse(value)?;
    let (kind, id) = v.reference()?;
    (kind == "ExtResource").then(|| file.ext.get(&id).map(|(p, _)| p.clone())).flatten()
}

/// Reads a TileSet resource (Godot 3 or 4).
pub fn tile_set(res: &mut Resources, set: &Found, version: u32) -> TileSet {
    let p = set.props().clone();
    let mut out = TileSet { cell: [16.0, 16.0], sheets: Vec::new(), by_id: HashMap::new() };
    if version >= 4 {
        if let Some(s) = p.get("tile_size").map(|v| ints(v)).filter(|s| s.len() == 2) {
            out.cell = [s[0], s[1]];
        }
        for (key, value) in &p {
            let Some(id) = key.strip_prefix("sources/").and_then(|i| i.parse::<i64>().ok()) else { continue };
            let Some(source) = res.resolve(&set.file, value) else { continue };
            if source.kind() != "TileSetAtlasSource" {
                continue;
            }
            let sp = source.props();
            let Some(texture) = sp.get("texture").and_then(|t| ext_path(&source.file, t)) else { continue };
            let pair = |k: &str, d: [u32; 2]| sp.get(k).map(|v| ints(v)).filter(|v| v.len() == 2).map(|v| [v[0] as u32, v[1] as u32]).unwrap_or(d);
            let mut solid = Vec::new();
            for k in sp.keys() {
                // `3:1/0/physics_layer_0/polygon_0/points`: tile (3, 1) has a shape.
                if k.contains("/physics_layer_") && (k.ends_with("/points") || k.ends_with("polygons_count")) {
                    let coords = k.split('/').next().unwrap_or("");
                    if let Some((a, b)) = coords.split_once(':') {
                        if let (Ok(a), Ok(b)) = (a.parse(), b.parse()) {
                            if !solid.contains(&[a, b]) {
                                solid.push([a, b]);
                            }
                        }
                    }
                }
            }
            out.by_id.insert(id, out.sheets.len());
            out.sheets.push(Sheet {
                texture,
                tile: pair("texture_region_size", [16, 16]),
                margin: pair("margins", [0, 0]),
                separation: pair("separation", [0, 0]),
                all_solid: false,
                solid,
            });
        }
    } else {
        // Godot 3: `<id>/texture`, `<id>/region`, `<id>/tile_mode`, `<id>/autotile/tile_size`...
        let mut ids: Vec<i64> = p.keys().filter_map(|k| k.split('/').next()?.parse().ok()).collect();
        ids.sort();
        ids.dedup();
        for id in ids {
            let get = |k: &str| p.get(&format!("{id}/{k}"));
            let Some(texture) = get("texture").and_then(|t| ext_path(&set.file, t)) else { continue };
            let region = get("region").map(|v| ints(v)).filter(|r| r.len() == 4).unwrap_or(vec![0.0; 4]);
            let mode = get("tile_mode").map(|v| ints(v)).and_then(|v| v.first().copied()).unwrap_or(0.0) as u32;
            let tile = if mode != 0 {
                get("autotile/tile_size").map(|v| ints(v)).filter(|v| v.len() == 2).map(|v| [v[0] as u32, v[1] as u32]).unwrap_or([16, 16])
            } else {
                [region[2] as u32, region[3] as u32]
            };
            let spacing = get("autotile/spacing").map(|v| ints(v)).and_then(|v| v.first().copied()).unwrap_or(0.0) as u32;
            let shapes = get("shapes").map(|s| !s.trim().trim_start_matches('[').trim_start().starts_with(']')).unwrap_or(false) || get("shape").is_some();
            out.by_id.insert(id, out.sheets.len());
            out.sheets.push(Sheet { texture, tile, margin: [region[0] as u32, region[1] as u32], separation: [spacing, spacing], all_solid: shapes, solid: Vec::new() });
        }
    }
    out
}

/// One placed tile: Godot cell (x, y down), sheet, column, row, GameX flags.
pub type Cell = (i32, i32, usize, u32, u32, i32);

/// A TileMap's cells. Godot 4 TileMap (`layer_N/tile_data`), Godot 4.3
/// TileMapLayer (`tile_map_data`), Godot 3 TileMap (`tile_data`).
pub fn tile_cells(props: &HashMap<String, String>, set: &TileSet, version: u32) -> Vec<Cell> {
    let mut out = Vec::new();
    let split = |v: i64| ((v & 0xffff) as u16 as i16 as i32, ((v >> 16) & 0xffff) as u16 as i16 as i32);
    if let Some(data) = props.get("tile_map_data").and_then(|d| parse(d)).and_then(|d| d.bytes()) {
        // 4.3+: u16 format, then 12 bytes per cell.
        for c in data.get(2..).unwrap_or(&[]).chunks_exact(12) {
            let r = |k: usize| u16::from_le_bytes([c[k], c[k + 1]]);
            let (x, y) = (r(0) as i16 as i32, r(2) as i16 as i32);
            let alt = r(10) as i32;
            if let Some(&sheet) = set.by_id.get(&(r(4) as i64)) {
                out.push((x, y, sheet, r(6) as u32, r(8) as u32, flags_g4(alt)));
            }
        }
        return out;
    }
    let mut layers: Vec<&String> = props.keys().filter(|k| k.as_str() == "tile_data" || (k.starts_with("layer_") && k.ends_with("/tile_data"))).collect();
    layers.sort();
    for key in layers {
        let v = ints(&props[key]);
        for t in v.chunks_exact(3) {
            let (a, b, c) = (t[0] as i64, t[1] as i64, t[2] as i64);
            let (x, y) = split(a);
            if version >= 4 {
                let (source, ax) = ((b & 0xffff) as i64, ((b >> 16) & 0xffff) as u32);
                let (ay, alt) = ((c & 0xffff) as u32, ((c >> 16) & 0xffff) as i32);
                if let Some(&sheet) = set.by_id.get(&source) {
                    out.push((x, y, sheet, ax, ay, flags_g4(alt)));
                }
            } else {
                let b = b as u32;
                let id = (b & 0x1fff_ffff) as i64;
                let flags = (if b & (1 << 29) != 0 { 1 } else { 0 }) | (if b & (1 << 30) != 0 { 2 } else { 0 }) | (if b & (1 << 31) != 0 { 4 } else { 0 });
                let (ax, ay) = split(c);
                if let Some(&sheet) = set.by_id.get(&id) {
                    out.push((x, y, sheet, ax.max(0) as u32, ay.max(0) as u32, flags));
                }
            }
        }
    }
    out
}

/// Godot 4 alternative tile flags (4.2+) -> GameX flags.
fn flags_g4(alt: i32) -> i32 {
    (if alt & (1 << 12) != 0 { 1 } else { 0 }) | (if alt & (1 << 13) != 0 { 2 } else { 0 }) | (if alt & (1 << 14) != 0 { 4 } else { 0 })
}

// ---------------------------------------------------------------------------
// Grid maps

/// Godot's 24 orthogonal rotations (rows), as a GridMap cell's `rot` picks one.
pub const ORTHO: [[f64; 9]; 24] = [
    [1., 0., 0., 0., 1., 0., 0., 0., 1.],
    [0., -1., 0., 1., 0., 0., 0., 0., 1.],
    [-1., 0., 0., 0., -1., 0., 0., 0., 1.],
    [0., 1., 0., -1., 0., 0., 0., 0., 1.],
    [1., 0., 0., 0., 0., -1., 0., 1., 0.],
    [0., 0., 1., 1., 0., 0., 0., 1., 0.],
    [-1., 0., 0., 0., 0., 1., 0., 1., 0.],
    [0., 0., -1., -1., 0., 0., 0., 1., 0.],
    [1., 0., 0., 0., -1., 0., 0., 0., -1.],
    [0., 1., 0., 1., 0., 0., 0., 0., -1.],
    [-1., 0., 0., 0., 1., 0., 0., 0., -1.],
    [0., -1., 0., -1., 0., 0., 0., 0., -1.],
    [1., 0., 0., 0., 0., 1., 0., -1., 0.],
    [0., 0., -1., 1., 0., 0., 0., -1., 0.],
    [-1., 0., 0., 0., 0., -1., 0., -1., 0.],
    [0., 0., 1., -1., 0., 0., 0., -1., 0.],
    [0., 0., 1., 0., 1., 0., -1., 0., 0.],
    [0., -1., 0., 0., 0., 1., -1., 0., 0.],
    [0., 0., -1., 0., -1., 0., -1., 0., 0.],
    [0., 1., 0., 0., 0., -1., -1., 0., 0.],
    [0., 0., 1., 0., -1., 0., 1., 0., 0.],
    [0., 1., 0., 0., 0., 1., 1., 0., 0.],
    [0., 0., -1., 0., 1., 0., 1., 0., 0.],
    [0., -1., 0., 0., 0., -1., 1., 0., 0.],
];

/// A GridMap cell: position in cells, mesh library item, rotation index.
pub struct GridCell {
    pub at: [i32; 3],
    pub item: i64,
    pub rot: usize,
}

pub fn grid_cells(props: &HashMap<String, String>) -> Vec<GridCell> {
    let Some(data) = props.get("data").and_then(|d| parse(d)) else { return Vec::new() };
    let cells = data.get("cells").map(V::numbers).unwrap_or_default();
    cells
        .chunks_exact(3)
        .map(|c| {
            let (a, b, v) = (c[0] as i64 as u32, c[1] as i64 as u32, c[2] as i64 as u32);
            GridCell {
                at: [(a & 0xffff) as u16 as i16 as i32, (a >> 16) as u16 as i16 as i32, (b & 0xffff) as u16 as i16 as i32],
                item: (v & 0xffff) as i64,
                rot: ((v >> 16) & 0x1f) as usize,
            }
        })
        .collect()
}

/// One mesh library item: name, its mesh, the mesh's offset transform (12 numbers) and first collision shape.
pub struct LibItem {
    pub name: String,
    pub mesh: Option<String>,
    pub transform: Option<Vec<f64>>,
    pub shape: Option<String>,
}

pub fn mesh_library(lib: &Found) -> HashMap<i64, LibItem> {
    let mut out: HashMap<i64, LibItem> = HashMap::new();
    for (k, v) in lib.props() {
        let Some(rest) = k.strip_prefix("item/") else { continue };
        let Some((id, field)) = rest.split_once('/') else { continue };
        let Ok(id) = id.parse::<i64>() else { continue };
        let item = out.entry(id).or_insert_with(|| LibItem { name: format!("Item {id}"), mesh: None, transform: None, shape: None });
        match field {
            "name" => item.name = parse(v).and_then(|s| s.str().map(String::from)).unwrap_or(item.name.clone()),
            "mesh" => item.mesh = Some(v.clone()),
            "mesh_transform" => item.transform = Some(ints(v)).filter(|t| t.len() == 12),
            "shapes" => {
                // [shape, transform, shape, transform...]: the first shape.
                item.shape = parse(v).and_then(|list| list.list().iter().find(|x| x.reference().is_some()).cloned()).and_then(|r| r.reference().map(|(kind, id)| format!("{kind}(\"{id}\")")));
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_godot_values() {
        let v = parse("{ \"cells\": PackedInt32Array(1, -2, 3), \"n\": 2.5, \"a\": [true, &\"x\"], \"t\": Array[int]([1]) }").unwrap();
        assert_eq!(v.get("cells").unwrap().numbers(), vec![1.0, -2.0, 3.0]);
        assert_eq!(v.get("n").unwrap().num(), Some(2.5));
        assert_eq!(v.get("a").unwrap().list()[1], V::Str("x".into()));
        assert_eq!(v.get("t").unwrap().list(), &[V::Num(1.0)]);
        assert_eq!(parse("PackedByteArray(\"AQID\")").unwrap().bytes(), Some(vec![1, 2, 3]));
        assert_eq!(parse("ExtResource( 2 )").unwrap().reference(), Some(("ExtResource", "2".to_string())));
    }

    #[test]
    fn reads_tile_maps_from_godot_3_and_4() {
        let set = TileSet { cell: [16.0, 16.0], sheets: Vec::new(), by_id: HashMap::from([(0, 0), (5, 1)]) };
        // Godot 4: cell (2, -1), source 5, atlas (3, 4).
        let key = (2i64 & 0xffff) | (((-1i64) & 0xffff) << 16);
        let g4 = HashMap::from([("layer_0/tile_data".to_string(), format!("PackedInt32Array({key}, {}, 4)", 5 | (3 << 16)))]);
        assert_eq!(tile_cells(&g4, &set, 4), vec![(2, -1, 1, 3, 4, 0)]);
        // Godot 3: tile 0 flipped horizontally, autotile (1, 2).
        let g3 = HashMap::from([("tile_data".to_string(), format!("PoolIntArray( 3, {}, {} )", 1u32 << 29, 1 | (2 << 16)))]);
        assert_eq!(tile_cells(&g3, &set, 3), vec![(3, 0, 0, 1, 2, 1)]);
    }

    #[test]
    fn reads_grid_map_cells() {
        let a = (1u32 & 0xffff) | (((-2i32) as u32 & 0xffff) << 16);
        let props = HashMap::from([("data".to_string(), format!("{{ \"cells\": PackedInt32Array({}, 3, {}) }}", a as i32, 7 | (10 << 16)))]);
        let c = &grid_cells(&props)[0];
        assert_eq!((c.at, c.item, c.rot), ([1, -2, 3], 7, 10));
    }
}
