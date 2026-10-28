//! Unity games -> GameX: reads the scenes in a Unity build's data files
//! (`level0`, `level1`...): each GameObject with its Transform and the
//! components GameX knows (meshes, sprites, tilemaps, colliders, bodies,
//! cameras, lights, sounds, scripts), into GameX scenes. Objects repeated in
//! a level ("Coin (1)", "Coin (2)"...) become things. Meshes become GLB
//! models; images and sounds come from AssetStudio's export (Unity keeps
//! textures in GPU formats), found by the object they were exported from.
//!
//! Android builds strip the type trees, so layouts are known here. Where a
//! layout changed between Unity versions, fields are read from the end of
//! the object, or a reference is recognised by the class it points to.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use super::glb;
use super::godot::{guess_kind, Report};
use super::model::*;
use super::project::{self, write_json};
use crate::unity::{DataFile, Mesh, Reader, SerializedFile};

const GAME_OBJECT: i32 = 1;
const TRANSFORM: i32 = 4;
const CAMERA: i32 = 20;
const MATERIAL: i32 = 21;
const MESH_RENDERER: i32 = 23;
const TEXTURE_2D: i32 = 28;
const MESH_FILTER: i32 = 33;
const MESH: i32 = 43;
const RIGIDBODY_2D: i32 = 50;
const RIGIDBODY: i32 = 54;
const CIRCLE_COLLIDER_2D: i32 = 58;
const POLYGON_COLLIDER_2D: i32 = 60;
const BOX_COLLIDER_2D: i32 = 61;
const MESH_COLLIDER: i32 = 64;
const BOX_COLLIDER: i32 = 65;
const COMPOSITE_COLLIDER_2D: i32 = 66;
const EDGE_COLLIDER_2D: i32 = 68;
const CAPSULE_COLLIDER_2D: i32 = 70;
const AUDIO_SOURCE: i32 = 82;
const AUDIO_CLIP: i32 = 83;
const ANIMATOR: i32 = 95;
const LIGHT: i32 = 108;
const MONO_BEHAVIOUR: i32 = 114;
const MONO_SCRIPT: i32 = 115;
const SPHERE_COLLIDER: i32 = 135;
const CAPSULE_COLLIDER: i32 = 136;
const SKINNED_MESH_RENDERER: i32 = 137;
const BUILD_SETTINGS: i32 = 141;
const CHARACTER_CONTROLLER: i32 = 143;
const GRID: i32 = 156;
const SPRITE_RENDERER: i32 = 212;
const SPRITE: i32 = 213;
const CANVAS: i32 = 223;
const RECT_TRANSFORM: i32 = 224;
const TILEMAP_COLLIDER_2D: i32 = 19719;
const TILEMAP_RENDERER: i32 = 483693784;
const TILEMAP: i32 = 1839735485;

/// What a class ID is called, for the report.
fn class_name(id: i32) -> String {
    match id {
        ANIMATOR => "Animator".into(),
        91 => "AnimatorController".into(),
        111 => "Animation".into(),
        198 => "ParticleSystem".into(),
        199 => "ParticleSystemRenderer".into(),
        96 => "TrailRenderer".into(),
        120 => "LineRenderer".into(),
        102 => "TextMesh".into(),
        195 => "NavMeshAgent".into(),
        222 => "CanvasRenderer".into(),
        225 => "CanvasGroup".into(),
        81 => "AudioListener".into(),
        92 => "GUILayer".into(),
        124 => "Behaviour".into(),
        114 => "MonoBehaviour".into(),
        59 => "HingeJoint".into(),
        144 => "CharacterJoint".into(),
        205 => "LODGroup".into(),
        208 => "NavMeshObstacle".into(),
        _ => format!("Unity class {id}"),
    }
}

/// An object: which loaded file, and its path ID there.
type Ref = (usize, i64);

struct Loaded {
    /// The file's name as references name it, lower case (`sharedassets0.assets`).
    key: String,
    /// The file's name as found.
    name: String,
    file: DataFile,
    sf: SerializedFile,
    index: HashMap<i64, usize>,
    /// The whole file, for levels (read object by object otherwise).
    blob: Option<Vec<u8>>,
}

/// Every Unity data file of one game, with references followed between them.
pub struct Game {
    dir: PathBuf,
    files: Vec<Loaded>,
    by_key: HashMap<String, usize>,
    pub version: (u32, u32),
    pub version_text: String,
}

fn file_key(name: &str) -> String {
    name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase()
}

impl Game {
    /// Opens the data files in `dir` (the folder holding `globalgamemanagers`).
    pub fn open(dir: &Path) -> io::Result<Game> {
        let mut names: Vec<String> = Vec::new();
        for e in fs::read_dir(dir)?.flatten() {
            if !e.path().is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let base = match name.rsplit_once(".split") {
                Some((base, n)) if n.chars().all(|c| c.is_ascii_digit()) => {
                    if n != "0" {
                        continue;
                    }
                    base.to_string()
                }
                _ => name,
            };
            let lower = base.to_ascii_lowercase();
            if lower.ends_with(".ress") || lower.ends_with(".resource") || lower.ends_with(".json") || lower.ends_with(".dll") {
                continue;
            }
            if !names.contains(&base) {
                names.push(base);
            }
        }
        names.sort();
        let mut files = Vec::new();
        for name in names {
            let Ok(file) = DataFile::open(&dir.join(&name)) else { continue };
            let Ok(sf) = SerializedFile::read(&file) else { continue };
            let index = sf.objects.iter().enumerate().map(|(i, o)| (o.path_id, i)).collect();
            let blob = (name.starts_with("level") && file.len() < 256 << 20).then(|| file.read_at(0, file.len() as usize).ok()).flatten();
            files.push(Loaded { key: file_key(&name), name, file, sf, index, blob });
        }
        let by_key = files.iter().enumerate().map(|(i, f)| (f.key.clone(), i)).collect();
        let version_text = files.iter().map(|f| f.sf.unity_version.clone()).find(|v| !v.is_empty() && v != "0.0.0").unwrap_or_default();
        Ok(Game { dir: dir.to_path_buf(), version: crate::unity::version_of(&version_text), version_text, files, by_key })
    }

    fn little(&self, f: usize) -> bool {
        self.files[f].sf.little
    }

    fn resolve(&self, from: usize, file_id: i32, path_id: i64) -> Option<Ref> {
        if path_id == 0 || file_id < 0 {
            return None;
        }
        let f = if file_id == 0 {
            from
        } else {
            *self.by_key.get(&file_key(self.files[from].sf.externals.get(file_id as usize - 1)?))?
        };
        self.files[f].index.contains_key(&path_id).then_some((f, path_id))
    }

    fn class(&self, r: Ref) -> Option<i32> {
        let f = &self.files[r.0];
        f.index.get(&r.1).map(|&i| f.sf.objects[i].class_id)
    }

    fn bytes(&self, r: Ref) -> Option<Vec<u8>> {
        let f = &self.files[r.0];
        let o = &f.sf.objects[*f.index.get(&r.1)?];
        match &f.blob {
            Some(b) => b.get(o.offset as usize..o.offset as usize + o.size as usize).map(<[u8]>::to_vec),
            None => f.file.read_at(o.offset, o.size as usize).ok(),
        }
    }

    /// Reads a reference (always 12 bytes).
    fn pptr(&self, from: usize, r: &mut Reader) -> Option<Option<Ref>> {
        let file_id = r.i32().ok()?;
        let path_id = r.i64().ok()?;
        Some(self.resolve(from, file_id, path_id))
    }

    fn pptr_at(&self, from: usize, data: &[u8], at: usize) -> Option<Ref> {
        let mut r = Reader::new(data.get(at..)?, self.little(from));
        self.pptr(from, &mut r).flatten()
    }

    /// The first reference from `start` on that points at one of `classes`.
    fn scan(&self, from: usize, data: &[u8], start: usize, classes: &[i32]) -> Option<Ref> {
        (start..data.len().saturating_sub(11)).step_by(4).find_map(|at| self.pptr_at(from, data, at).filter(|r| self.class(*r).is_some_and(|c| classes.contains(&c))))
    }

    fn objects_of(&self, f: usize, classes: &[i32]) -> Vec<Ref> {
        self.files[f].sf.objects.iter().filter(|o| classes.contains(&o.class_id)).map(|o| (f, o.path_id)).collect()
    }

    /// The scenes in build order, as level file index and name.
    pub fn levels(&self) -> Vec<(usize, String)> {
        let mut names: Vec<String> = Vec::new();
        for (f, file) in self.files.iter().enumerate() {
            if !file.key.starts_with("globalgamemanagers") {
                continue;
            }
            for r in self.objects_of(f, &[BUILD_SETTINGS]) {
                if let Some(d) = self.bytes(r) {
                    let mut rd = Reader::new(&d, self.little(f));
                    if let Ok(n) = rd.count(4) {
                        names = (0..n).map_while(|_| rd.string().ok()).collect();
                    }
                }
            }
        }
        let mut out: Vec<(usize, u32, String)> = self
            .files
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                let n: u32 = f.key.strip_prefix("level")?.parse().ok()?;
                let name = names
                    .get(n as usize)
                    .map(|p| p.rsplit('/').next().unwrap_or(p).trim_end_matches(".unity").to_string())
                    .unwrap_or_else(|| format!("Level {n}"));
                Some((i, n, name))
            })
            .collect();
        out.sort_by_key(|x| x.1);
        out.into_iter().map(|(i, _, name)| (i, name)).collect()
    }
}

// ---------------------------------------------------------------------------
// Objects

struct GameObj {
    components: Vec<Ref>,
    name: String,
    active: bool,
}

fn read_game_object(g: &Game, r: Ref) -> Option<GameObj> {
    let d = g.bytes(r)?;
    let mut rd = Reader::new(&d, g.little(r.0));
    let n = rd.count(12).ok()?;
    let mut components = Vec::new();
    for _ in 0..n {
        if let Some(c) = g.pptr(r.0, &mut rd)? {
            components.push(c);
        }
    }
    rd.u32().ok()?; // layer
    let name = rd.string().ok()?;
    rd.i16().ok()?; // tag
    let active = rd.u8().map(|b| b != 0).unwrap_or(true);
    Some(GameObj { components, name, active })
}

struct Tf {
    go: Option<Ref>,
    rotation: [f32; 4],
    position: [f32; 3],
    scale: [f32; 3],
    children: Vec<Ref>,
    father: Option<Ref>,
}

fn read_transform(g: &Game, r: Ref) -> Option<Tf> {
    let d = g.bytes(r)?;
    let mut rd = Reader::new(&d, g.little(r.0));
    let go = g.pptr(r.0, &mut rd)?;
    let mut f = || rd.f32().ok();
    let rotation = [f()?, f()?, f()?, f()?];
    let position = [f()?, f()?, f()?];
    let scale = [f()?, f()?, f()?];
    let n = rd.count(12).ok()?;
    let mut children = Vec::new();
    for _ in 0..n {
        if let Some(c) = g.pptr(r.0, &mut rd)? {
            children.push(c);
        }
    }
    let father = g.pptr(r.0, &mut rd).flatten();
    Some(Tf { go, rotation, position, scale, children, father })
}

fn f32_at(d: &[u8], at: usize, little: bool) -> Option<f32> {
    let b: [u8; 4] = d.get(at..at + 4)?.try_into().ok()?;
    Some(if little { f32::from_le_bytes(b) } else { f32::from_be_bytes(b) })
}

fn i32_at(d: &[u8], at: usize, little: bool) -> Option<i32> {
    let b: [u8; 4] = d.get(at..at + 4)?.try_into().ok()?;
    Some(if little { i32::from_le_bytes(b) } else { i32::from_be_bytes(b) })
}

fn floats(d: &[u8], at: usize, n: usize, little: bool) -> Option<Vec<f32>> {
    (0..n).map(|i| f32_at(d, at + i * 4, little).filter(|v| v.is_finite())).collect()
}

/// A material's main colour and texture, found by their property names.
fn read_material(g: &Game, r: Ref) -> ([f32; 4], Option<Ref>) {
    let mut color = [1.0, 1.0, 1.0, 1.0];
    let mut texture = None;
    let Some(d) = g.bytes(r) else { return (color, texture) };
    let little = g.little(r.0);
    let mut at = 0;
    while at + 8 <= d.len() {
        let len = i32_at(&d, at, little).unwrap_or(0);
        if (2..=40).contains(&len) && at + 4 + len as usize <= d.len() && d[at + 4] == b'_' {
            let name = &d[at + 4..at + 4 + len as usize];
            if name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_') {
                let after = (at + 4 + len as usize + 3) & !3;
                match name {
                    b"_MainTex" | b"_BaseMap" | b"_BaseColorMap" | b"_MainTexture" | b"_Albedo" if texture.is_none() => {
                        texture = g.pptr_at(r.0, &d, after).filter(|t| g.class(*t) == Some(TEXTURE_2D));
                    }
                    b"_Color" | b"_BaseColor" | b"_MainColor" | b"_TintColor" if color == [1.0; 4] => {
                        if let Some(c) = floats(&d, after, 4, little).filter(|c| c.iter().all(|v| (0.0..=4.0).contains(v))) {
                            color = [c[0].min(1.0), c[1].min(1.0), c[2].min(1.0), c[3].min(1.0)];
                        }
                    }
                    _ => {}
                }
                at = after;
                continue;
            }
        }
        at += 4;
    }
    (color, texture)
}

/// A renderer's materials: the list of references that all point at materials.
fn renderer_materials(g: &Game, r: Ref) -> Vec<Ref> {
    let Some(d) = g.bytes(r) else { return Vec::new() };
    let little = g.little(r.0);
    for at in (12..d.len().saturating_sub(16)).step_by(4) {
        let n = i32_at(&d, at, little).unwrap_or(0);
        if !(1..=32).contains(&n) || at + 4 + n as usize * 12 > d.len() {
            continue;
        }
        let list: Vec<Option<Ref>> = (0..n as usize).map(|i| g.pptr_at(r.0, &d, at + 4 + i * 12)).collect();
        if list.iter().all(|m| m.is_some_and(|m| g.class(m) == Some(MATERIAL))) {
            return list.into_iter().flatten().collect();
        }
    }
    Vec::new()
}

struct SpriteInfo {
    /// Size in metres.
    size: [f32; 2],
    /// Where the pivot is, 0..1 from the bottom left.
    pivot: [f32; 2],
    ppu: f32,
    px: [u32; 2],
}

fn read_sprite(g: &Game, r: Ref) -> Option<SpriteInfo> {
    let d = g.bytes(r)?;
    let little = g.little(r.0);
    let mut rd = Reader::new(&d, little);
    rd.string().ok()?;
    let rect = floats(&d, rd.pos, 4, little)?;
    // rect, offset (2), border (4), then pixels per unit and the pivot.
    let ppu = f32_at(&d, rd.pos + 40, little).filter(|p| *p > 0.0 && *p < 100_000.0)?;
    let pivot = floats(&d, rd.pos + 44, 2, little).filter(|p| p.iter().all(|v| (-4.0..=5.0).contains(v))).unwrap_or(vec![0.5, 0.5]);
    Some(SpriteInfo { size: [rect[2] / ppu, rect[3] / ppu], pivot: [pivot[0], pivot[1]], ppu, px: [rect[2].round() as u32, rect[3].round() as u32] })
}

/// A script's class name (with its namespace), from its MonoScript.
fn read_script(g: &Game, r: Ref) -> Option<(String, String)> {
    let d = g.bytes(r)?;
    let mut rd = Reader::new(&d, g.little(r.0));
    let name = rd.string().ok()?;
    rd.i32().ok()?;
    rd.skip(16).ok()?;
    let class = rd.string().ok()?;
    let namespace = rd.string().unwrap_or_default();
    let class = if class.is_empty() { name } else { class };
    let full = if namespace.is_empty() { class.clone() } else { format!("{namespace}.{class}") };
    Some((class, full))
}

/// Unity's left-handed axes -> GameX's. 3D: mirrored on X (as meshes are).
/// 2D: X and Y kept, depth turned round (Unity's camera looks along +Z).
fn convert(tf: &Tf, flat: bool) -> Transform {
    let [x, y, z] = tf.position;
    let [qx, qy, qz, qw] = tf.rotation;
    let (position, rotation) = if flat { ([x, y, -z], [-qx, -qy, qz, qw]) } else { ([-x, y, z], [qx, -qy, -qz, qw]) };
    let tidy = |v: f32| if v.abs() < 1e-6 { 0.0 } else { (v * 10000.0).round() / 10000.0 };
    let len = (qx * qx + qy * qy + qz * qz + qw * qw).sqrt();
    let rotation = if len > 0.5 { rotation.map(|v| tidy(v / len)) } else { [0.0, 0.0, 0.0, 1.0] };
    Transform { position: position.map(tidy), rotation, scale: tf.scale.map(tidy) }
}

/// A local offset (a collider's centre) in GameX axes.
fn offset(v: [f32; 3], flat: bool) -> [f32; 3] {
    let v = if flat { [v[0], v[1], -v[2]] } else { [-v[0], v[1], v[2]] };
    v.map(|x| (x * 10000.0).round() / 10000.0 + 0.0)
}

// ---------------------------------------------------------------------------
// Translating

/// Where AssetStudio put what it exported: (file key, path ID) -> file.
#[derive(Default)]
pub struct Exports {
    by_object: HashMap<(String, i64), PathBuf>,
    by_id: HashMap<i64, Vec<PathBuf>>,
}

impl Exports {
    /// Reads an export folder grouped by source file, files named by path ID.
    pub fn scan(dir: &Path) -> Exports {
        let mut out = Exports::default();
        for e in WalkDir::new(dir).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file()) {
            let path = e.path();
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            // "123", or "Name_123" when names are kept.
            let id = stem.parse::<i64>().ok().or_else(|| stem.rsplit_once('_').and_then(|(_, n)| n.parse().ok()));
            let Some(id) = id else { continue };
            let rel = path.strip_prefix(dir).unwrap_or(path);
            if let Some(folder) = rel.components().next().and_then(|c| c.as_os_str().to_str()).filter(|_| rel.components().count() > 1) {
                out.by_object.insert((folder.to_ascii_lowercase(), id), path.to_path_buf());
            }
            out.by_id.entry(id).or_default().push(path.to_path_buf());
        }
        out
    }

    fn find(&self, file: &str, id: i64) -> Option<&PathBuf> {
        let key = file.to_ascii_lowercase();
        let stem = key.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| key.clone());
        self.by_object
            .get(&(format!("{key}.split0"), id))
            .or_else(|| self.by_object.get(&(key, id)))
            .or_else(|| self.by_object.get(&(stem, id)))
            .or_else(|| self.by_id.get(&id).filter(|l| l.len() == 1).map(|l| &l[0]))
    }
}

struct Ctx<'a> {
    g: &'a Game,
    exports: &'a Exports,
    dir: PathBuf,
    source: String,
    report: Report,
    origin: BTreeMap<String, Origin>,
    /// Object -> GameX asset path, for images, sounds and models already written.
    assets: HashMap<(Ref, Vec<Ref>), String>,
    scripts: HashSet<String>,
    /// Entity IDs used in the scene being read.
    taken: HashSet<String>,
    flat: bool,
    background: Option<String>,
}

fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || "._- ".contains(c) { c } else { '_' }).collect();
    let s = s.trim().to_string();
    if s.is_empty() {
        "object".into()
    } else {
        s.chars().take(60).collect()
    }
}

impl Ctx<'_> {
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

    fn file_name(&self, r: Ref) -> &str {
        &self.g.files[r.0].name
    }

    /// Copies an exported image or sound into the project (once).
    fn exported(&mut self, r: Ref, kind: &str) -> Option<String> {
        if let Some(a) = self.assets.get(&(r, Vec::new())) {
            return Some(a.clone());
        }
        let file = self.file_name(r).to_string();
        let found = self.exports.find(&file, r.1)?.clone();
        let ext = found.extension().and_then(|e| e.to_str()).unwrap_or("bin").to_ascii_lowercase();
        let name = found.file_stem().and_then(|s| s.to_str()).unwrap_or("asset");
        let asset = format!("{kind}/{}/{}/{}.{ext}", self.source, safe_name(&file), safe_name(name));
        let target = self.dir.join("assets").join(&asset);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).ok()?;
        }
        fs::copy(&found, &target).ok()?;
        self.origin.insert(format!("asset:{asset}"), Origin { file: file.clone(), object: Some(r.1.to_string()), field: None, fidelity: Fidelity::Exact, note: None });
        self.report.assets += 1;
        self.assets.insert((r, Vec::new()), asset.clone());
        Some(asset)
    }

    fn read_mesh(&self, r: Ref) -> Option<Mesh> {
        let d = self.g.bytes(r)?;
        let mut mesh = Mesh::read(&d, self.g.little(r.0), self.g.version).ok()?;
        if mesh.vertex_data.is_empty() {
            if let Some((offset, size, path)) = &mesh.stream {
                let name = path.rsplit('/').next().unwrap_or(path);
                let stream = DataFile::open(&self.g.dir.join(name)).ok()?;
                mesh.vertex_data = stream.read_at(*offset, *size as usize).ok()?;
            }
        }
        Some(mesh)
    }

    /// A mesh with its materials as a GLB model in the project (once per combination).
    fn model(&mut self, mesh_ref: Ref, materials: &[Ref]) -> Option<String> {
        let key = (mesh_ref, materials.to_vec());
        if let Some(a) = self.assets.get(&key) {
            return Some(a.clone());
        }
        let mesh = self.read_mesh(mesh_ref)?;
        if mesh.compressed {
            *self.report.untranslated.entry("compressed meshes".into()).or_default() += 1;
            return None;
        }
        let (pos, dim) = mesh.channel(0).filter(|(_, d)| *d >= 3)?;
        let normals = mesh.channel(1).filter(|(_, d)| *d >= 3);
        let uvs = mesh.channel(4).filter(|(_, d)| *d >= 2);
        let file = safe_name(self.file_name(mesh_ref));
        let asset = format!("models/{}/{file}/{}-{}.glb", self.source, mesh_ref.1, safe_name(&mesh.name));
        let n = mesh.vertex_count as usize;
        let positions: Vec<[f32; 3]> = (0..n).map(|v| [-pos[v * dim], pos[v * dim + 1], pos[v * dim + 2]]).collect();
        let normals: Vec<[f32; 3]> = normals.map(|(nv, d)| (0..n).map(|v| [-nv[v * d], nv[v * d + 1], nv[v * d + 2]]).collect()).unwrap_or_default();
        let uvs: Vec<[f32; 2]> = uvs.map(|(uv, d)| (0..n).map(|v| [uv[v * d], 1.0 - uv[v * d + 1]]).collect()).unwrap_or_default();
        let mut surfaces = Vec::new();
        for (i, sub) in mesh.sub_meshes.iter().enumerate() {
            let tri = mesh.triangles(sub);
            if tri.is_empty() {
                continue;
            }
            // Mirroring turns the faces round, so the corners are swapped back.
            let indices: Vec<u32> = tri.as_chunks::<3>().0.iter().flat_map(|t| [t[0], t[2], t[1]]).filter(|&i| (i as usize) < n).collect();
            let mut material = glb::Material { name: format!("{} {}", mesh.name, i + 1), color: [0.75, 0.75, 0.78, 1.0], texture: None };
            if let Some(&m) = materials.get(i).or(materials.last()) {
                let (color, texture) = read_material(self.g, m);
                material.color = color;
                if let Some(image) = texture.and_then(|t| self.exported(t, "textures")) {
                    // Relative to the GLB: models/<src>/<file>/x.glb -> textures/...
                    material.texture = Some(format!("../../../{image}"));
                }
            }
            surfaces.push(glb::Surface { positions: positions.clone(), normals: normals.clone(), uvs: uvs.clone(), indices, material });
        }
        if surfaces.is_empty() {
            return None;
        }
        let bytes = glb::write(&mesh.name, &surfaces).ok()?;
        let target = self.dir.join("assets").join(&asset);
        fs::create_dir_all(target.parent()?).ok()?;
        fs::write(&target, bytes).ok()?;
        self.origin.insert(
            format!("asset:{asset}"),
            Origin { file: self.file_name(mesh_ref).to_string(), object: Some(mesh_ref.1.to_string()), field: None, fidelity: Fidelity::Exact, note: None },
        );
        self.report.assets += 1;
        self.assets.insert(key, asset.clone());
        Some(asset)
    }

    /// A mesh's bounds (centre, size) in GameX axes.
    fn mesh_bounds(&self, r: Ref) -> Option<([f32; 3], [f32; 3])> {
        let mesh = self.read_mesh(r)?;
        let (pos, dim) = mesh.channel(0).filter(|(_, d)| *d >= 3)?;
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for v in 0..mesh.vertex_count as usize {
            let p = [-pos[v * dim], pos[v * dim + 1], pos[v * dim + 2]];
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        (min[0] <= max[0]).then(|| ([0, 1, 2].map(|k| (min[k] + max[k]) / 2.0), [0, 1, 2].map(|k| (max[k] - min[k]).max(0.02))))
    }
}

/// What one GameObject's components add.
#[derive(Default)]
struct Made {
    parts: Vec<Part>,
    /// Parts that sit off the object's origin (a collider's centre, a sprite's pivot).
    offset_parts: Vec<(String, [f32; 3], [f32; 3], Part)>,
    scripts: Vec<String>,
    canvas: bool,
    grid: Option<[f32; 3]>,
}

fn add(made: &mut Made, part: Part, center: [f32; 3], name: &str) {
    if center.iter().all(|v| v.abs() < 1e-4) {
        made.parts.push(part);
    } else {
        made.offset_parts.push((name.into(), center, [1.0; 3], part));
    }
}

/// The body the object's colliders belong to.
fn body_of(ctx: &Ctx, comps: &[(Ref, i32)]) -> Body {
    for &(r, class) in comps {
        match class {
            CHARACTER_CONTROLLER => return Body::Kinematic,
            RIGIDBODY => {
                // Before 2022.2 "is kinematic" comes right after mass, drag, angular drag and "use gravity".
                let kinematic = ctx.g.version < (2022, 2) && ctx.g.bytes(r).and_then(|d| d.get(25).copied()).is_some_and(|b| b == 1);
                return if kinematic { Body::Kinematic } else { Body::Dynamic };
            }
            RIGIDBODY_2D => {
                let little = ctx.g.little(r.0);
                return match ctx.g.bytes(r).and_then(|d| i32_at(&d, 12, little)) {
                    Some(1) => Body::Kinematic,
                    Some(2) => Body::Static,
                    _ => Body::Dynamic,
                };
            }
            _ => {}
        }
    }
    Body::Static
}

/// The trigger flag of a collider, whose header grew in 2022.2.
fn trigger_of(ctx: &Ctx, d: &[u8], two_d: bool) -> bool {
    let new = ctx.g.version >= (2022, 2);
    let at = match (two_d, new) {
        (false, false) => 24,
        (false, true) => 36,
        (true, false) => 32,
        (true, true) => 60,
    };
    d.get(at).is_some_and(|b| *b == 1)
}

fn components(ctx: &mut Ctx, go: &GameObj, inherited: Option<[f32; 3]>) -> Made {
    let g = ctx.g;
    let flat = ctx.flat;
    let comps: Vec<(Ref, i32)> = go.components.iter().filter_map(|&r| g.class(r).map(|c| (r, c))).collect();
    let body = body_of(ctx, &comps);
    let mut made = Made::default();
    let mut sprite_size: Option<[f32; 2]> = None;
    let has = |class: i32| comps.iter().any(|c| c.1 == class);
    let cell = inherited.unwrap_or([1.0, 1.0, 0.0]);

    for &(r, class) in &comps {
        let Some(d) = g.bytes(r) else { continue };
        let little = g.little(r.0);
        let len = d.len();
        match class {
            TRANSFORM | RECT_TRANSFORM | RIGIDBODY | RIGIDBODY_2D | MESH_RENDERER | TILEMAP_RENDERER | TILEMAP_COLLIDER_2D => {}
            CANVAS => made.canvas = true,
            GRID => made.grid = floats(&d, 16, 3, little).map(|v| [v[0], v[1], v[2]]),
            MESH_FILTER | SKINNED_MESH_RENDERER => {
                let mesh = if class == MESH_FILTER { g.pptr_at(r.0, &d, 12).filter(|m| g.class(*m) == Some(MESH)) } else { g.scan(r.0, &d, 12, &[MESH]) };
                let renderer = if class == MESH_FILTER { comps.iter().find(|c| c.1 == MESH_RENDERER).map(|c| c.0) } else { Some(r) };
                let materials = renderer.map(|x| renderer_materials(g, x)).unwrap_or_default();
                if let Some(asset) = mesh.and_then(|m| ctx.model(m, &materials)) {
                    made.parts.push(Part::Model { asset });
                } else if mesh.is_some() {
                    *ctx.report.untranslated.entry("unreadable meshes".into()).or_default() += 1;
                }
            }
            SPRITE_RENDERER => {
                let Some(sprite) = g.scan(r.0, &d, 12, &[SPRITE]) else { continue };
                let Some(info) = read_sprite(g, sprite) else { continue };
                sprite_size = Some(info.size);
                let part = match ctx.exported(sprite, "textures") {
                    Some(asset) => Part::Sprite { asset, frame: None, size: Some(info.size), flip_x: false },
                    None => Part::Shape { shape: ShapeKind::Box, size: [info.size[0], info.size[1], 0.01], color: "#9aa3c0".into() },
                };
                let center = [(0.5 - info.pivot[0]) * info.size[0], (0.5 - info.pivot[1]) * info.size[1], 0.0];
                add(&mut made, part, center, "Sprite");
            }
            TILEMAP => {
                if let Some((cells, sprites)) = read_tilemap(g, r, &d) {
                    let mut sheets = Vec::new();
                    let mut sheet_of: HashMap<usize, i32> = HashMap::new();
                    let mut ppu = 100.0;
                    let solid = has(TILEMAP_COLLIDER_2D);
                    let mut out = Vec::new();
                    for (x, y, sprite) in cells {
                        let Some(&sr) = sprites.get(sprite).filter(|s| s.0 != usize::MAX) else { continue };
                        let sheet = match sheet_of.get(&sprite) {
                            Some(s) => *s,
                            None => {
                                let Some(info) = read_sprite(g, sr) else { continue };
                                let Some(asset) = ctx.exported(sr, "textures") else { continue };
                                if sheets.is_empty() {
                                    ppu = info.ppu;
                                }
                                sheets.push(TileSheet { asset, tile: info.px, margin: [0, 0], separation: [0, 0], solid: if solid { vec![[0, 0]] } else { Vec::new() } });
                                sheet_of.insert(sprite, sheets.len() as i32 - 1);
                                sheets.len() as i32 - 1
                            }
                        };
                        out.push([x, y, sheet, 0, 0, 0]);
                    }
                    if !out.is_empty() {
                        // GameX draws tiles at 100 pixels per metre; a child scaled to the
                        // sprites' own pixels per metre keeps them the size Unity shows.
                        let s = 100.0 / ppu;
                        let part = Part::Tiles { cell: [cell[0] / s, cell[1] / s], sheets, cells: out };
                        made.offset_parts.push(("Tiles".into(), [0.0; 3], [s, s, 1.0], part));
                    }
                } else {
                    *ctx.report.untranslated.entry("Tilemap (layout not recognised)".into()).or_default() += 1;
                }
            }
            BOX_COLLIDER | SPHERE_COLLIDER | CAPSULE_COLLIDER | CHARACTER_CONTROLLER | MESH_COLLIDER => {
                let trigger = class != CHARACTER_CONTROLLER && trigger_of(ctx, &d, false);
                let body = if class == CHARACTER_CONTROLLER { Body::Kinematic } else { body };
                let part = |shape, size: [f32; 3]| Part::Collider { shape, size: size.map(f32::abs), body, trigger };
                let tail = |n: usize| len.checked_sub(n);
                let (p, center) = match class {
                    BOX_COLLIDER => {
                        let Some(v) = tail(24).and_then(|at| floats(&d, at, 6, little)) else { continue };
                        (part(ShapeKind::Box, [v[0], v[1], v[2]]), [v[3], v[4], v[5]])
                    }
                    SPHERE_COLLIDER => {
                        let Some(v) = tail(16).and_then(|at| floats(&d, at, 4, little)) else { continue };
                        (part(ShapeKind::Sphere, [v[0] * 2.0; 3]), [v[1], v[2], v[3]])
                    }
                    CAPSULE_COLLIDER | CHARACTER_CONTROLLER => {
                        // Capsule: radius, height, direction, centre. Controller: height, radius, ... centre.
                        let Some(c) = tail(12).and_then(|at| floats(&d, at, 3, little)) else { continue };
                        let (radius, height, axis) = if class == CAPSULE_COLLIDER {
                            let Some(v) = tail(28).and_then(|at| floats(&d, at, 2, little)) else { continue };
                            (v[0], v[1], tail(20).and_then(|at| i32_at(&d, at, little)).unwrap_or(1))
                        } else {
                            let Some(v) = tail(36).and_then(|at| floats(&d, at, 2, little)) else { continue };
                            (v[1], v[0], 1)
                        };
                        let r2 = radius * 2.0;
                        let p = match axis {
                            0 => part(ShapeKind::Box, [height.max(r2), r2, r2]),
                            2 => part(ShapeKind::Box, [r2, r2, height.max(r2)]),
                            _ => part(ShapeKind::Capsule, [r2, height.max(r2), r2]),
                        };
                        (p, [c[0], c[1], c[2]])
                    }
                    _ => {
                        // A mesh collider: the box around its mesh (or the object's own mesh).
                        let mesh = g.scan(r.0, &d, 12, &[MESH]).or_else(|| {
                            comps.iter().find(|c| c.1 == MESH_FILTER).and_then(|c| g.bytes(c.0).and_then(|fd| g.pptr_at(c.0 .0, &fd, 12)))
                        });
                        let Some((center, size)) = mesh.and_then(|m| ctx.mesh_bounds(m)) else { continue };
                        add(&mut made, part(ShapeKind::Box, size), center.map(|x| (x * 10000.0).round() / 10000.0 + 0.0), "Collider");
                        continue;
                    }
                };
                add(&mut made, p, offset(center, flat), "Collider");
            }
            BOX_COLLIDER_2D | CIRCLE_COLLIDER_2D | CAPSULE_COLLIDER_2D | POLYGON_COLLIDER_2D | EDGE_COLLIDER_2D | COMPOSITE_COLLIDER_2D => {
                let trigger = trigger_of(ctx, &d, true);
                let collider = |shape, w: f32, h: f32| Part::Collider { shape, size: [w.abs(), h.abs(), 1.0], body, trigger };
                let v2 = |at: Option<usize>| at.and_then(|at| floats(&d, at, 2, little));
                let (p, center) = match class {
                    BOX_COLLIDER_2D if len >= 100 => {
                        let (Some(size), Some(off)) = (v2(len.checked_sub(12)), v2(len.checked_sub(76))) else { continue };
                        (collider(ShapeKind::Box, size[0], size[1]), off)
                    }
                    CIRCLE_COLLIDER_2D => {
                        let (Some(r), Some(off)) = (len.checked_sub(4).and_then(|at| f32_at(&d, at, little)), v2(len.checked_sub(12))) else { continue };
                        (collider(ShapeKind::Sphere, r * 2.0, r * 2.0), off)
                    }
                    CAPSULE_COLLIDER_2D => {
                        let (Some(size), Some(off)) = (v2(len.checked_sub(12)), v2(len.checked_sub(20))) else { continue };
                        (collider(ShapeKind::Capsule, size[0], size[1]), off)
                    }
                    _ => {
                        // Outlines (polygons, edges): the sprite's box.
                        let Some([w, h]) = sprite_size.or_else(|| {
                            comps.iter().find(|c| c.1 == SPRITE_RENDERER).and_then(|c| g.bytes(c.0)).and_then(|sd| g.scan(r.0, &sd, 12, &[SPRITE])).and_then(|s| read_sprite(g, s)).map(|i| i.size)
                        }) else {
                            *ctx.report.untranslated.entry(format!("{} without a sprite", if class == EDGE_COLLIDER_2D { "EdgeCollider2D" } else { "PolygonCollider2D" })).or_default() += 1;
                            continue;
                        };
                        (collider(ShapeKind::Box, w, h), vec![0.0, 0.0])
                    }
                };
                add(&mut made, p, [center[0], center[1], 0.0], "Collider");
            }
            CAMERA => {
                if ctx.background.is_none() {
                    if let Some(c) = floats(&d, 20, 4, little).filter(|c| c.iter().all(|v| (0.0..=1.0).contains(v))) {
                        ctx.background = Some(format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
                    }
                }
                made.parts.push(Part::Camera { fov: None, ortho_size: None });
            }
            LIGHT => {
                let kind = match i32_at(&d, 16, little) {
                    Some(0) => LightKind::Spot,
                    Some(1) => LightKind::Directional,
                    Some(2) => LightKind::Point,
                    _ => continue,
                };
                // The colour comes next, or after a shape field (newer versions): its alpha is 1.
                let at = [20, 24].into_iter().find(|&at| floats(&d, at, 4, little).is_some_and(|c| (c[3] - 1.0).abs() < 1e-3 && c[..3].iter().all(|v| (0.0..=1.0).contains(v))));
                let (color, intensity, range) = match at {
                    Some(at) => {
                        let c = floats(&d, at, 6, little).unwrap_or(vec![1.0; 6]);
                        (format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8), c[4].clamp(0.0, 8.0), Some(c[5]).filter(|r| *r > 0.0 && *r < 1000.0))
                    }
                    None => ("#ffffff".into(), 1.0, None),
                };
                made.parts.push(Part::Light { kind, color, intensity, range: if kind == LightKind::Directional { None } else { range } });
            }
            AUDIO_SOURCE => {
                let Some(clip) = g.pptr_at(r.0, &d, 28).filter(|c| g.class(*c) == Some(AUDIO_CLIP)) else { continue };
                let Some(asset) = ctx.exported(clip, "audio") else { continue };
                let autoplay = d.get(40).is_some_and(|b| *b == 1);
                let volume = f32_at(&d, 44, little).filter(|v| (0.0..=1.0).contains(v)).unwrap_or(1.0);
                let looped = d.get(52).is_some_and(|b| *b == 1);
                made.parts.push(Part::Sound { asset, volume, looped, autoplay });
            }
            MONO_BEHAVIOUR => {
                let script = g.pptr_at(r.0, &d, 16).filter(|s| g.class(*s) == Some(MONO_SCRIPT)).and_then(|s| read_script(g, s));
                let (class, full) = script.unwrap_or(("MonoBehaviour".into(), "unknown script".into()));
                ctx.scripts.insert(full.clone());
                made.scripts.push(class.clone());
                let mut values = BTreeMap::new();
                values.insert("script".into(), Value::Text(full));
                values.insert("status".into(), Value::Text("not translated yet".into()));
                made.parts.push(Part::Custom { name: class, values });
            }
            other => {
                *ctx.report.untranslated.entry(class_name(other)).or_default() += 1;
            }
        }
    }
    made
}

/// A tilemap's cells (x, y, sprite index) and its sprites. The cell record
/// changed size between versions, so each known size is tried and the one
/// whose sprite list reads back as sprites is kept.
/// Cells (x, y, index into the sprites), and the sprites.
type TileCells = (Vec<(i32, i32, usize)>, Vec<Ref>);

fn read_tilemap(g: &Game, r: Ref, d: &[u8]) -> Option<TileCells> {
    let little = g.little(r.0);
    let start = 16; // GameObject, enabled
    let count = i32_at(d, start, little)?;
    if !(0..=1_000_000).contains(&count) {
        return None;
    }
    let count = count as usize;
    // (record size, width of the index fields)
    for (size, wide) in [(28usize, 2usize), (24, 2), (36, 4), (32, 4)] {
        let after = start + 4 + count * size;
        if after > d.len() {
            continue;
        }
        let Some((sprites, ok)) = tile_sprites(g, r.0, d, after, little) else { continue };
        if !ok {
            continue;
        }
        let mut cells = Vec::with_capacity(count);
        let mut valid = true;
        for i in 0..count {
            let at = start + 4 + i * size;
            let (Some(x), Some(y)) = (i32_at(d, at, little), i32_at(d, at + 4, little)) else { valid = false; break };
            let sprite = if wide == 2 {
                d.get(at + 14..at + 16).map(|b| if little { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) } as usize)
            } else {
                i32_at(d, at + 16, little).map(|v| v as usize)
            };
            match sprite {
                Some(s) if s < sprites.len() || s == 0xffff || s == u32::MAX as usize => {
                    if s < sprites.len() {
                        cells.push((x, y, s));
                    }
                }
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if valid {
            return Some((cells, sprites));
        }
    }
    None
}

/// After the cells: animated tiles, the tile assets, then the sprites.
fn tile_sprites(g: &Game, from: usize, d: &[u8], at: usize, little: bool) -> Option<(Vec<Ref>, bool)> {
    let mut rd = Reader::new(d.get(at..)?, little);
    for _ in 0..rd.count(12).ok()? {
        rd.skip(12).ok()?;
        let n = rd.count(12).ok()?;
        rd.skip(n * 12 + 12).ok()?;
    }
    let assets = rd.count(16).ok()?;
    rd.skip(assets * 16).ok()?;
    let n = rd.count(16).ok()?;
    let mut sprites = Vec::new();
    let mut ok = n > 0 || assets == 0;
    for _ in 0..n {
        rd.u32().ok()?;
        match g.pptr(from, &mut rd)? {
            Some(s) if g.class(s) == Some(SPRITE) => sprites.push(s),
            Some(_) => ok = false,
            // A removed sprite: its slot is kept so indices still line up.
            None => sprites.push((usize::MAX, 0)),
        }
    }
    Some((sprites, ok))
}

/// Reads one GameObject (by its Transform) and everything under it.
fn entity(ctx: &mut Ctx, tf_ref: Ref, depth: usize, cell: Option<[f32; 3]>) -> Option<Entity> {
    let tf = read_transform(ctx.g, tf_ref)?;
    let go = tf.go.filter(|r| ctx.g.class(*r) == Some(GAME_OBJECT)).and_then(|r| read_game_object(ctx.g, r))?;
    let made = components(ctx, &go, cell);
    if made.canvas {
        *ctx.report.untranslated.entry("UI canvas".into()).or_default() += 1;
        return None;
    }
    let id = ctx.unique(&go.name);
    let mut e = Entity::new(&id, &go.name);
    e.transform = convert(&tf, ctx.flat);
    e.active = go.active;
    e.parts = made.parts;
    for (name, at, scale, part) in made.offset_parts {
        let cid = ctx.unique(&format!("{id}-{name}"));
        let mut c = Entity::new(&cid, &name);
        c.transform = Transform { position: at, scale, ..Transform::default() };
        c.parts.push(part);
        e.children.push(c);
    }
    if !made.scripts.is_empty() {
        e.tags = made.scripts.iter().map(|s| format!("script:{s}")).collect();
    }
    ctx.report.entities += 1;
    if depth < 64 {
        let cell = made.grid.or(cell);
        for c in tf.children {
            if let Some(child) = entity(ctx, c, depth + 1, cell) {
                e.children.push(child);
            }
        }
    }
    Some(e)
}

/// "Coin (3)", "Coin 3", "Coin(Clone)" -> "Coin".
fn base_name(name: &str) -> &str {
    let mut s = name.trim();
    loop {
        let before = s;
        s = s.strip_suffix("(Clone)").unwrap_or(s).trim_end();
        if let Some(open) = s.rfind(" (").filter(|_| s.ends_with(')')) {
            if s[open + 2..s.len() - 1].chars().all(|c| c.is_ascii_digit()) {
                s = s[..open].trim_end();
            }
        }
        let digits = s.trim_end_matches(|c: char| c.is_ascii_digit());
        if digits.len() < s.len() && (digits.ends_with(' ') || digits.ends_with('_')) {
            s = digits.trim_end_matches([' ', '_']);
        }
        if s == before {
            return if s.is_empty() { name } else { s };
        }
    }
}

/// A child made here to hold a part off the object's origin.
fn is_helper(e: &Entity) -> bool {
    e.children.is_empty() && e.thing.is_none() && matches!(e.name.as_str(), "Collider" | "Sprite" | "Tiles")
}

/// Part types, to tell apart two different objects that share a name.
fn signature(e: &Entity) -> String {
    let mut kinds: Vec<String> = e
        .walk()
        .iter()
        .flat_map(|x| x.parts.iter())
        .map(|p| match p {
            Part::Custom { name, .. } => format!("custom:{name}"),
            Part::Model { asset } | Part::Sprite { asset, .. } => asset.clone(),
            other => serde_json::to_value(other).ok().and_then(|v| v["type"].as_str().map(String::from)).unwrap_or_default(),
        })
        .collect();
    kinds.sort();
    kinds.dedup();
    kinds.join(",")
}

struct ThingMaker {
    things: Vec<Thing>,
    by_key: HashMap<(String, String), String>,
    ids: HashSet<String>,
}

impl ThingMaker {
    /// Turns repeated siblings (and players, enemies, items) into things placed by instances.
    fn visit(&mut self, list: &mut [Entity], scene: &str, origin: &mut BTreeMap<String, Origin>) {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for e in list.iter() {
            *counts.entry(base_name(&e.name).to_string()).or_default() += 1;
        }
        for e in list.iter_mut() {
            let base = base_name(&e.name).to_string();
            // Its own name, scripts and colliders say what it is (not those of objects inside it).
            let scripts: Vec<String> = e.tags.iter().filter_map(|t| t.strip_prefix("script:")).map(String::from).collect();
            let (mut kind, role) = guess_kind(&format!("{base} {}", scripts.join(" ")), "");
            let own: Vec<&Part> = e.parts.iter().chain(e.children.iter().filter(|c| is_helper(c)).flat_map(|c| c.parts.iter())).collect();
            let moving = own.iter().any(|p| matches!(p, Part::Collider { body: Body::Kinematic | Body::Dynamic, .. } | Part::Collider { trigger: true, .. }));
            // Something to see or touch: empty folders of objects stay plain entities.
            let has_look = e.walk().iter().flat_map(|x| x.parts.iter()).any(|p| matches!(p, Part::Model { .. } | Part::Sprite { .. } | Part::Shape { .. } | Part::Collider { .. } | Part::Tiles { .. }));
            let repeated = counts.get(&base).copied().unwrap_or(0) >= 2;
            let wanted = has_look && (repeated || role.is_some() || kind == ThingKind::Item || moving);
            if !wanted {
                self.visit(&mut e.children, scene, origin);
                continue;
            }
            if kind == ThingKind::Other {
                kind = if moving { ThingKind::Character } else { ThingKind::Prop };
            }
            let key = (base.clone(), signature(e));
            let id = match self.by_key.get(&key) {
                Some(id) => id.clone(),
                None => {
                    let stem = project::slug(&base);
                    let stem = if stem.is_empty() { "thing".to_string() } else { stem };
                    let mut id = stem.clone();
                    let mut n = 2;
                    while !self.ids.insert(id.clone()) {
                        id = format!("{stem}-{n}");
                        n += 1;
                    }
                    self.things.push(Thing { tags: Vec::new(),
                        format: FORMAT,
                        id: id.clone(),
                        name: base.clone(),
                        kind,
                        role,
                        description: Some(format!("Translated from Unity ({scene}: {})", e.name)),
                        properties: Vec::new(),
                        parts: e.parts.clone(),
                        behaviors: Vec::new(),
                        children: e.children.clone(),
                    });
                    origin.insert(
                        format!("thing:{id}"),
                        Origin { file: scene.into(), object: Some(e.name.clone()), field: None, fidelity: Fidelity::Approximated, note: Some("made from the first of its copies; kind and role guessed from names and scripts".into()) },
                    );
                    self.by_key.insert(key, id.clone());
                    id
                }
            };
            e.thing = Some(id);
            e.parts.clear();
            e.children.clear();
            e.tags.clear();
        }
    }
}

/// Translates the Unity game whose data files are in `data` into the GameX
/// project at `dir`. `exports` is AssetStudio's export of its images and sounds.
pub fn translate(data: &Path, exports: &Exports, dir: &Path, source: &str, name: &str) -> io::Result<Report> {
    let game = Game::open(data)?;
    let levels = game.levels();
    if levels.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "No Unity scenes were found in this game's data files. It may keep them in asset bundles (data.unity3d), which GameX can't read yet.",
        ));
    }
    let mut ctx = Ctx {
        g: &game,
        exports,
        dir: dir.to_path_buf(),
        source: source.into(),
        report: Report { source: source.into(), engine: format!("Unity {}", game.version_text), ..Default::default() },
        origin: BTreeMap::new(),
        assets: HashMap::new(),
        scripts: HashSet::new(),
        taken: HashSet::new(),
        flat: false,
        background: None,
    };
    if game.version < (2019, 3) {
        ctx.report.problems.push(format!("Unity {} is older than 2019.3: meshes from it can't be read yet.", game.version_text));
    }
    let mut info = project::open(dir)?.info;
    let mut maker = ThingMaker { things: Vec::new(), by_key: HashMap::new(), ids: HashSet::new() };
    let mut used = HashSet::new();
    let mut start = None;
    for (f, level_name) in &levels {
        let f = *f;
        let flat2 = game.objects_of(f, &[SPRITE_RENDERER, TILEMAP, BOX_COLLIDER_2D, CIRCLE_COLLIDER_2D, POLYGON_COLLIDER_2D]).len();
        let flat3 = game.objects_of(f, &[MESH_FILTER, SKINNED_MESH_RENDERER]).len();
        ctx.flat = flat2 > flat3;
        ctx.taken.clear();
        ctx.background = None;
        let mut roots: Vec<Ref> = game.objects_of(f, &[TRANSFORM, RECT_TRANSFORM]).into_iter().filter(|r| read_transform(&game, *r).is_some_and(|t| t.father.is_none())).collect();
        roots.sort_by_key(|r| r.1);
        let mut entities: Vec<Entity> = roots.into_iter().filter_map(|r| entity(&mut ctx, r, 0, None)).collect();
        let file = game.files[f].name.clone();
        maker.visit(&mut entities, &file, &mut ctx.origin);
        let base = project::slug(level_name);
        let base = if base.is_empty() { "level".to_string() } else { base };
        let mut id = base.clone();
        let mut n = 2;
        while !used.insert(id.clone()) {
            id = format!("{base}-{n}");
            n += 1;
        }
        let env = Environment::default();
        let scene = Scene {
            format: FORMAT,
            id: id.clone(),
            name: level_name.clone(),
            dimensions: if ctx.flat { Dimensions::TwoD } else { Dimensions::ThreeD },
            environment: Environment { background: ctx.background.clone().unwrap_or_else(|| if ctx.flat { "#20263a".into() } else { "#87a8c8".into() }), ..env },
            entities,
        };
        if start.is_none() {
            start = Some(id.clone());
        }
        ctx.origin.insert(format!("scene:{id}"), Origin { file, object: None, field: None, fidelity: Fidelity::Approximated, note: Some("GameObjects and known components; scripts kept by name".into()) });
        write_json(&dir.join("world").join(format!("{id}.scene.json")), &scene)?;
        ctx.report.scenes += 1;
    }
    for t in &maker.things {
        write_json(&dir.join("things").join(t.kind.folder()).join(format!("{}.thing.json", t.id)), t)?;
    }
    ctx.report.things = maker.things.len();
    ctx.report.scripts = ctx.scripts.len();
    if exports.by_id.is_empty() {
        ctx.report.problems.push("No images or sounds were exported, so sprites show as grey boxes and models without textures.".into());
    }
    write_json(&dir.join("sources").join(source).join("origin.json"), &OriginMap { format: FORMAT, source: source.into(), entries: std::mem::take(&mut ctx.origin) })?;
    info.sources.retain(|s| s.id != source);
    info.sources.push(Source { id: source.into(), name: name.into(), engine: "unity".into(), engine_version: Some(game.version_text.clone()), imported_from: Some(data.display().to_string()) });
    if info.settings.start_scene.is_none() {
        info.settings.start_scene = start;
    }
    write_json(&dir.join(project::PROJECT_FILE), &info)?;
    Ok(ctx.report)
}

/// The folder holding a Unity game's data files, under `dir`.
pub fn data_folder(dir: &Path) -> Option<PathBuf> {
    WalkDir::new(dir)
        .max_depth(8)
        .into_iter()
        .filter_map(Result::ok)
        .find(|e| {
            let n = e.file_name().to_string_lossy();
            e.file_type().is_file() && (n == "globalgamemanagers" || n == "globalgamemanagers.split0" || n == "level0" || n == "level0.split0")
        })
        .and_then(|e| e.path().parent().map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unity::tests::{serialized_file_with, W};

    fn pptr(w: &mut W, file: i32, id: i64) {
        w.i32(file);
        w.0.extend_from_slice(&id.to_le_bytes());
    }

    fn game_object(components: &[i64], name: &str) -> Vec<u8> {
        let mut w = W::default();
        w.i32(components.len() as i32);
        for c in components {
            pptr(&mut w, 0, *c);
        }
        w.u32(0).string(name);
        w.0.extend_from_slice(&0u16.to_le_bytes());
        w.u8(1);
        w.0
    }

    fn transform(go: i64, position: [f32; 3], children: &[i64], father: i64) -> Vec<u8> {
        let mut w = W::default();
        pptr(&mut w, 0, go);
        for v in [0.0, 0.0, 0.0, 1.0] {
            w.f32(v);
        }
        for v in position {
            w.f32(v);
        }
        for v in [1.0; 3] {
            w.f32(v);
        }
        w.i32(children.len() as i32);
        for c in children {
            pptr(&mut w, 0, *c);
        }
        pptr(&mut w, 0, father);
        w.0
    }

    /// A BoxCollider (before 2022.2): GameObject, material, trigger, enabled, size, centre.
    fn box_collider(go: i64, trigger: bool, size: [f32; 3], center: [f32; 3]) -> Vec<u8> {
        let mut w = W::default();
        pptr(&mut w, 0, go);
        pptr(&mut w, 0, 0);
        w.u8(trigger as u8).u8(1).align();
        for v in size.into_iter().chain(center) {
            w.f32(v);
        }
        w.0
    }

    fn mono_behaviour(go: i64, script_file: i32, script: i64) -> Vec<u8> {
        let mut w = W::default();
        pptr(&mut w, 0, go);
        w.u8(1).align();
        pptr(&mut w, script_file, script);
        w.string("");
        w.0
    }

    fn mono_script(class: &str) -> Vec<u8> {
        let mut w = W::default();
        w.string(class).i32(0);
        w.0.extend_from_slice(&[0; 16]);
        w.string(class).string("Game").string("Assembly-CSharp.dll");
        w.0
    }

    #[test]
    fn names_of_copies() {
        assert_eq!(base_name("Coin (12)"), "Coin");
        assert_eq!(base_name("Coin(Clone)"), "Coin");
        assert_eq!(base_name("Enemy 3"), "Enemy");
        assert_eq!(base_name("Enemy_3"), "Enemy");
        assert_eq!(base_name("Level2"), "Level2");
        assert_eq!(base_name("(1)"), "(1)");
    }

    #[test]
    fn reads_a_level_into_a_scene_with_things() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("Data");
        fs::create_dir_all(&data).unwrap();
        // globalgamemanagers.assets holds the scripts; level0 refers to it as file 1.
        fs::write(data.join("globalgamemanagers.assets"), serialized_file_with(&[(7, MONO_SCRIPT, mono_script("PlayerController"))], &[])).unwrap();
        let level = serialized_file_with(
            &[
                // Ground: a box collider 20 x 1 x 20 centred half a metre down.
                (1, GAME_OBJECT, game_object(&[2, 3], "Ground")),
                (2, TRANSFORM, transform(1, [0.0, 0.0, 0.0], &[], 0)),
                (3, BOX_COLLIDER, box_collider(1, false, [20.0, 1.0, 20.0], [0.0, -0.5, 0.0])),
                // Two coins, trigger boxes.
                (10, GAME_OBJECT, game_object(&[11, 12], "Coin (1)")),
                (11, TRANSFORM, transform(10, [2.0, 1.0, 0.0], &[], 0)),
                (12, BOX_COLLIDER, box_collider(10, true, [0.5, 0.5, 0.5], [0.0; 3])),
                (20, GAME_OBJECT, game_object(&[21, 22], "Coin (2)")),
                (21, TRANSFORM, transform(20, [4.0, 1.0, 0.0], &[], 0)),
                (22, BOX_COLLIDER, box_collider(20, true, [0.5, 0.5, 0.5], [0.0; 3])),
                // The player, with a script, inside a container.
                (30, GAME_OBJECT, game_object(&[31], "Actors")),
                (31, TRANSFORM, transform(30, [1.0, 0.0, 0.0], &[41], 0)),
                (40, GAME_OBJECT, game_object(&[41, 42, 43], "Hero")),
                (41, TRANSFORM, transform(40, [0.0, 2.0, 3.0], &[], 31)),
                (42, BOX_COLLIDER, box_collider(40, false, [1.0, 2.0, 1.0], [0.0; 3])),
                (43, MONO_BEHAVIOUR, mono_behaviour(40, 1, 7)),
            ],
            &["globalgamemanagers.assets"],
        );
        fs::write(data.join("level0"), level).unwrap();

        let game = Game::open(&data).unwrap();
        assert_eq!(game.levels(), vec![(1, "Level 0".to_string())]);
        let root = dir.path().join("projects");
        let out = project::create(&root, "Unity Test").unwrap();
        let report = translate(&data, &Exports::default(), &out, "test", "Unity Test").unwrap();
        assert_eq!((report.scenes, report.scripts), (1, 1));
        assert!(report.engine.starts_with("Unity 2021.3"));

        let scene = project::load_scene(&out, "level-0").unwrap();
        assert_eq!(scene.dimensions, Dimensions::ThreeD);
        let names: Vec<&str> = scene.entities.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Ground", "Coin (1)", "Coin (2)", "Actors"]);
        // The ground's collider sits off its origin: on a child, mirrored axes kept.
        let ground = &scene.entities[0];
        assert_eq!(ground.children[0].transform.position, [0.0, -0.5, 0.0]);
        assert!(matches!(ground.children[0].parts[0], Part::Collider { body: Body::Static, trigger: false, .. }));
        // Coins: one thing, two instances, X mirrored.
        assert_eq!(scene.entities[1].thing.as_deref(), Some("coin"));
        assert_eq!(scene.entities[2].thing.as_deref(), Some("coin"));
        assert_eq!(scene.entities[2].transform.position, [-4.0, 1.0, 0.0]);
        let coin = project::load_thing(&out, "coin").unwrap();
        assert_eq!(coin.kind, ThingKind::Item);
        // The hero (a player by its script's name) is a thing inside the container.
        let hero = &scene.entities[3].children[0];
        assert_eq!(hero.thing.as_deref(), Some("hero"));
        let thing = project::load_thing(&out, "hero").unwrap();
        assert_eq!(thing.role.as_deref(), Some("player"));
        assert!(thing.parts.iter().any(|p| matches!(p, Part::Custom { name, .. } if name == "PlayerController")));
        assert_eq!(project::open(&out).unwrap().info.settings.start_scene.as_deref(), Some("level-0"));
    }

    /// A real game: `GX_UNITY_DATA=<folder with globalgamemanagers> cargo test -- --ignored real_unity_game --nocapture`.
    #[test]
    #[ignore]
    fn real_unity_game() {
        let data = PathBuf::from(std::env::var("GX_UNITY_DATA").expect("set GX_UNITY_DATA"));
        let exports = std::env::var("GX_UNITY_EXPORTS").map(|d| Exports::scan(Path::new(&d))).unwrap_or_default();
        let root = tempfile::tempdir().unwrap();
        let out = project::create(root.path(), "Real").unwrap();
        let started = std::time::Instant::now();
        let report = translate(&data, &exports, &out, "real", "Real").unwrap();
        println!("{report:#?}\ntook {:?}", started.elapsed());
        for s in project::open(&out).unwrap().scenes {
            let scene = project::load_scene(&out, &s.id).unwrap();
            let all: Vec<&Entity> = scene.entities.iter().flat_map(|e| e.walk()).collect();
            let count = |f: &dyn Fn(&Part) -> bool| all.iter().flat_map(|e| e.parts.iter()).filter(|p| f(p)).count();
            println!(
                "{} ({:?}): {} top-level, {} entities, {} models, {} sprites, {} colliders, {} instances",
                scene.name,
                scene.dimensions,
                scene.entities.len(),
                all.len(),
                count(&|p| matches!(p, Part::Model { .. })),
                count(&|p| matches!(p, Part::Sprite { .. })),
                count(&|p| matches!(p, Part::Collider { .. })),
                all.iter().filter(|e| e.thing.is_some()).count()
            );
            for e in scene.entities.iter().take(12) {
                println!("   {} at {:?}", e.name, e.transform.position);
            }
            for e in all.iter().filter(|e| e.parts.iter().any(|p| matches!(p, Part::Collider { .. } | Part::Light { .. } | Part::Sound { .. }))).take(14) {
                println!("   - {} {:?} {}", e.name, e.transform.position, serde_json::to_string(&e.parts.iter().find(|p| !matches!(p, Part::Custom { .. }))).unwrap());
            }
        }
    }

    #[test]
    fn exports_are_found_by_file_and_object() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sharedassets0.assets")).unwrap();
        fs::create_dir_all(dir.path().join("level0")).unwrap();
        fs::write(dir.path().join("sharedassets0.assets").join("12.png"), b"x").unwrap();
        fs::write(dir.path().join("level0").join("Hero_12.png"), b"y").unwrap();
        fs::write(dir.path().join("level0").join("99.wav"), b"z").unwrap();
        let e = Exports::scan(dir.path());
        assert!(e.find("sharedassets0.assets", 12).unwrap().ends_with(Path::new("sharedassets0.assets").join("12.png")));
        assert!(e.find("level0", 12).unwrap().to_string_lossy().contains("Hero_12"));
        // Only one export with this ID: found whatever its folder.
        assert!(e.find("other.assets", 99).is_some());
        assert!(e.find("other.assets", 12).is_none());
    }
}
