//! The Godot reader: translates a Godot 3 or 4 project (as made in the
//! editor, or as GDRE Tools recovers it from an exported game) into the
//! GameX format. Scenes become scenes or things, nodes become entities with
//! parts, exported script variables become properties, and every element
//! records where it came from. Scripts stay attached as GDScript, marked
//! "not translated yet" (behavior is translated in milestone 4).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use super::godot_res::{self as gres, Resources};
use super::model::*;
use super::project::{self, slug, write_json};
use crate::scene::{parse_sections, Section};

/// Godot 2D works in pixels; GameX in metres.
const PIXELS_PER_UNIT: f64 = 100.0;

/// What a translation did, for the user and for checking it against the game.
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub source: String,
    /// The engine it was read from, for people: "Godot 4", "Unity 2021.3.11f1".
    pub engine: String,
    pub godot_version: u32,
    pub scenes: usize,
    pub things: usize,
    pub entities: usize,
    pub assets: usize,
    pub scripts: usize,
    /// Node types kept as "custom" parts (no GameX equivalent yet), with counts.
    pub untranslated: BTreeMap<String, usize>,
    pub problems: Vec<String>,
}

// ---------------------------------------------------------------------------
// Reading Godot's text values

/// `Vector2(1, 2)`, `Vector3( 1, 2, 3 )`, `Color(...)`, `Transform3D(...)`: the numbers inside.
fn numbers(value: &str) -> Option<Vec<f64>> {
    let open = value.find('(')?;
    let inner = value[open + 1..].trim_end().strip_suffix(')')?;
    inner.split(',').map(|n| n.trim().parse::<f64>().ok()).collect()
}

fn number(value: &str) -> Option<f64> {
    value.trim().parse().ok()
}

/// `ExtResource("1_x")` / `ExtResource( 1 )` / `SubResource(...)` -> the ID.
fn resource_id(value: &str, kind: &str) -> Option<String> {
    let inner = value.trim().strip_prefix(kind)?.trim().strip_prefix('(')?.strip_suffix(')')?;
    Some(inner.trim().trim_matches('"').to_string())
}

fn text(value: &str) -> Option<String> {
    let v = value.trim();
    let inner = v.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\"", "\"").replace("\\n", "\n").replace("\\\\", "\\"))
}

fn hex(c: &[f64]) -> String {
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

/// A literal from a scene or a script (`300`, `1.5`, `true`, `"Hi"`, `Vector2(1, 2)`) as a typed value.
fn literal(value: &str) -> Option<Value> {
    let v = value.trim();
    if v == "true" || v == "false" {
        return Some(Value::Bool(v == "true"));
    }
    if let Some(s) = text(v) {
        return Some(Value::Text(s));
    }
    if v.starts_with("Color") {
        return numbers(v).filter(|n| n.len() >= 3).map(|n| Value::Color(hex(&n)));
    }
    if v.starts_with("Vector") {
        return numbers(v).map(Value::Vector);
    }
    if !v.contains('.') && !v.contains('e') {
        if let Ok(i) = v.parse::<i64>() {
            return Some(Value::Integer(i));
        }
    }
    v.parse::<f64>().ok().filter(|f| f.is_finite()).map(Value::Number)
}

// ---------------------------------------------------------------------------
// Scenes as read

struct Node {
    name: String,
    ty: Option<String>,
    /// Path from the scene root ("." for the root).
    path: String,
    parent: Option<String>,
    instance: Option<String>,
    props: HashMap<String, String>,
}

struct Tscn {
    /// res:// path of this scene.
    res: String,
    /// The same file as a resource file, for following its resources.
    file: std::rc::Rc<gres::ResFile>,
    ext: HashMap<String, (String, String)>,
    sub: HashMap<String, (String, HashMap<String, String>)>,
    nodes: Vec<Node>,
}

impl Tscn {
    fn read(res: &str, text: &str) -> Tscn {
        let file = std::rc::Rc::new(gres::ResFile::read(res, text));
        let mut t = Tscn { res: res.into(), file, ext: HashMap::new(), sub: HashMap::new(), nodes: Vec::new() };
        for s in parse_sections(text) {
            let props = || s.properties.iter().map(|p| (p.key.clone(), p.value.clone())).collect::<HashMap<_, _>>();
            match s.tag.as_str() {
                "ext_resource" => {
                    let id = s.attr("id").unwrap_or_default().trim_matches('"').to_string();
                    t.ext.insert(id, (s.attr("path").unwrap_or_default().to_string(), s.attr("type").unwrap_or_default().to_string()));
                }
                "sub_resource" => {
                    let id = s.attr("id").unwrap_or_default().trim_matches('"').to_string();
                    t.sub.insert(id, (s.attr("type").unwrap_or_default().to_string(), props()));
                }
                "node" => t.nodes.push(node(&s, props())),
                _ => {}
            }
        }
        t
    }

    fn ext_path(&self, value: &str) -> Option<&(String, String)> {
        self.ext.get(&resource_id(value, "ExtResource")?)
    }

    fn sub(&self, value: &str) -> Option<&(String, HashMap<String, String>)> {
        self.sub.get(&resource_id(value, "SubResource")?)
    }

    fn root(&self) -> Option<&Node> {
        self.nodes.iter().find(|n| n.parent.is_none())
    }

    /// The nodes whose `parent` is `path` ("." for the root's children).
    fn children<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.nodes.iter().filter(move |n| n.parent.as_deref() == Some(path))
    }
}

fn node(s: &Section, props: HashMap<String, String>) -> Node {
    let name = s.attr("name").unwrap_or("Node").to_string();
    let parent = s.attr("parent").map(String::from);
    let path = match parent.as_deref() {
        None => ".".to_string(),
        Some(".") => name.clone(),
        Some(p) => format!("{p}/{name}"),
    };
    Node {
        name,
        ty: s.attr("type").map(String::from),
        path,
        parent,
        instance: s.attr("instance").and_then(|v| resource_id(v, "ExtResource")),
        props,
    }
}

// ---------------------------------------------------------------------------
// Node types

fn is_3d(ty: &str) -> bool {
    ty.ends_with("3D")
        || matches!(
            ty,
            "Spatial" | "MeshInstance" | "KinematicBody" | "RigidBody" | "StaticBody" | "Area" | "CollisionShape" | "Camera"
                | "DirectionalLight" | "OmniLight" | "SpotLight" | "GridMap" | "Position3D" | "CSGBox" | "CSGSphere" | "CSGCylinder"
                | "CSGCombiner" | "Skeleton" | "VehicleBody" | "Particles" | "CPUParticles"
        )
}

fn is_2d(ty: &str) -> bool {
    ty.ends_with("2D")
        || matches!(
            ty,
            "Sprite" | "AnimatedSprite" | "Light2D" | "Control" | "Label" | "Button" | "Panel" | "TextureRect" | "ColorRect" | "CanvasLayer"
                | "MarginContainer" | "VBoxContainer" | "HBoxContainer" | "TileMap" | "Polygon2D" | "RichTextLabel" | "TextureButton"
        )
}

#[derive(Clone, Copy, PartialEq)]
enum Holder {
    None,
    Body(Body),
    Area,
}

fn holder_of(ty: &str) -> Option<Holder> {
    let base = ty.trim_end_matches("2D").trim_end_matches("3D");
    Some(match base {
        "StaticBody" | "AnimatableBody" => Holder::Body(Body::Static),
        "RigidBody" | "VehicleBody" => Holder::Body(Body::Dynamic),
        "CharacterBody" | "KinematicBody" => Holder::Body(Body::Kinematic),
        "Area" => Holder::Area,
        _ => return None,
    })
}

/// Node types that only group or place others: no part needed.
fn plain(ty: &str) -> bool {
    matches!(
        ty,
        "Node" | "Node2D" | "Node3D" | "Spatial" | "Position2D" | "Position3D" | "Marker2D" | "Marker3D" | "CanvasLayer" | "Control"
            | "StaticBody2D" | "StaticBody3D" | "StaticBody" | "RigidBody2D" | "RigidBody3D" | "RigidBody" | "CharacterBody2D"
            | "CharacterBody3D" | "KinematicBody2D" | "KinematicBody" | "Area2D" | "Area3D" | "Area" | "AnimatableBody2D" | "AnimatableBody3D"
            | "MarginContainer" | "VBoxContainer" | "HBoxContainer" | "CenterContainer"
    )
}

// ---------------------------------------------------------------------------
// The translation

struct Ctx<'a> {
    version: u32,
    source: &'a str,
    /// res:// path -> asset path under assets/.
    assets: HashMap<String, String>,
    /// res:// image path -> pixel size.
    images: HashMap<String, (u32, u32)>,
    /// res:// scene path -> thing ID, for scenes that are things.
    things: HashMap<String, String>,
    /// Thing ID -> its properties (for typing instance overrides).
    thing_props: HashMap<String, Vec<Property>>,
    /// res:// script path -> exported properties.
    exports: HashMap<String, Vec<Property>>,
    viewport_height: f64,
    report: Report,
    origin: BTreeMap<String, Origin>,
    /// The game's resource files, read as needed.
    res: Resources,
    /// The GameX project being written.
    dir: PathBuf,
    /// Meshes already written as GLB: resource key -> asset path.
    meshes: HashMap<String, Option<String>>,
    /// Mesh library items already made into things: library key -> item -> thing ID.
    lib_things: HashMap<String, HashMap<i64, String>>,
    thing_ids: HashSet<String>,
}

impl Ctx<'_> {
    fn untranslated(&mut self, what: &str) {
        *self.report.untranslated.entry(what.to_string()).or_insert(0) += 1;
    }

    /// A mesh value (`SubResource`/`ExtResource`) in `file` as a part: a
    /// shape for Godot's primitive meshes, a model for imported ones and for
    /// meshes in Godot's own array format (written out as GLB).
    fn mesh_part(&mut self, file: &std::rc::Rc<gres::ResFile>, value: &str, color: Option<String>) -> Option<Part> {
        if let Some((path, _)) = gres::parse(value).and_then(|v| v.reference().filter(|(k, _)| *k == "ExtResource").and_then(|(_, id)| file.ext.get(&id).cloned())) {
            if let Some(asset) = self.assets.get(&path) {
                return Some(Part::Model { asset: asset.clone() });
            }
        }
        let found = self.res.resolve(file, value)?;
        if let Some((shape, size)) = shape_of(found.kind(), found.props(), self.version, false) {
            let color = color.or_else(|| {
                let m = found.props().get("material")?.clone();
                let f = self.res.resolve(&found.file, &m)?;
                gres::material_of(&f).0.map(|c| hex(&[c[0] as f64, c[1] as f64, c[2] as f64]))
            });
            return Some(Part::Shape { shape, size, color: color.unwrap_or_else(|| DEFAULT_COLOR.into()) });
        }
        if !found.kind().is_empty() && found.kind() != "ArrayMesh" && found.kind() != "Mesh" {
            return None;
        }
        let key = found.key();
        if let Some(done) = self.meshes.get(&key) {
            return done.clone().map(|asset| Part::Model { asset });
        }
        let stem = {
            let file_stem = found.file.res.rsplit('/').next().unwrap_or("mesh").split('.').next().unwrap_or("mesh").to_string();
            slug(&match &found.sub {
                Some(id) => format!("{file_stem}-{id}"),
                None => file_stem,
            })
        };
        let asset = format!("models/{}/meshes/{stem}.glb", self.source);
        let assets = self.assets.clone();
        let uri = move |res: &str| assets.get(res).map(|a| format!("../../../{a}"));
        let made = gres::array_mesh_glb(&mut self.res, &found, &stem, &uri).and_then(|bytes| {
            let target = self.dir.join("assets").join(&asset);
            fs::create_dir_all(target.parent()?).ok()?;
            fs::write(&target, bytes).ok()
        });
        let result = made.map(|_| asset.clone());
        if result.is_some() {
            self.origin.insert(
                format!("asset:{asset}"),
                Origin { file: found.file.res.clone(), object: found.sub.clone(), field: None, fidelity: Fidelity::Approximated, note: Some("mesh converted from Godot's array format".into()) },
            );
            self.report.assets += 1;
        }
        self.meshes.insert(key, result.clone());
        result.map(|asset| Part::Model { asset })
    }

    /// A TileMap / TileMapLayer node's tiles.
    fn tiles(&mut self, t: &Tscn, n: &Node) -> Option<Part> {
        let set_value = n.props.get("tile_set")?;
        let set = self.res.resolve(&t.file, set_value)?;
        let set = gres::tile_set(&mut self.res, &set, self.version);
        let cells = gres::tile_cells(&n.props, &set, self.version);
        let cell_px = if self.version >= 4 { set.cell } else { vec(&n.props, "cell_size").filter(|c| c.len() == 2).map(|c| [c[0], c[1]]).unwrap_or([64.0, 64.0]) };
        let mut sheets = Vec::new();
        let mut index = HashMap::new();
        for (i, sheet) in set.sheets.iter().enumerate() {
            let Some(asset) = self.assets.get(&sheet.texture).cloned() else { continue };
            let mut solid = sheet.solid.clone();
            if sheet.all_solid {
                for c in cells.iter().filter(|c| c.2 == i) {
                    if !solid.contains(&[c.3, c.4]) {
                        solid.push([c.3, c.4]);
                    }
                }
            }
            index.insert(i, sheets.len() as i32);
            sheets.push(TileSheet { asset, tile: sheet.tile, margin: sheet.margin, separation: sheet.separation, solid });
        }
        if sheets.is_empty() {
            return None;
        }
        // Godot counts rows down from the top-left; GameX up from the bottom-left.
        let cells: Vec<[i32; 6]> = cells.iter().filter_map(|c| Some([c.0, -(c.1 + 1), *index.get(&c.2)?, c.3 as i32, c.4 as i32, c.5])).collect();
        Some(Part::Tiles { cell: [round(cell_px[0] / PIXELS_PER_UNIT), round(cell_px[1] / PIXELS_PER_UNIT)], sheets, cells })
    }

    /// A GridMap's cells as entities, each an instance of its mesh library item (made a thing).
    fn grid(&mut self, t: &Tscn, n: &Node, id: &str) -> Vec<Entity> {
        let Some(lib_value) = n.props.get("mesh_library").or(n.props.get("theme")) else { return Vec::new() };
        let Some(lib) = self.res.resolve(&t.file, lib_value) else {
            self.untranslated("GridMap (mesh library not found)");
            return Vec::new();
        };
        let items = gres::mesh_library(&lib);
        let size = vec(&n.props, "cell_size").filter(|c| c.len() == 3).unwrap_or(vec![2.0, 2.0, 2.0]);
        let centered = |axis: &str| n.props.get(&format!("cell_center_{axis}")).is_none_or(|v| v.trim() != "false");
        let offset = [centered("x"), centered("y"), centered("z")].map(|c| if c { 0.5 } else { 0.0 });
        let cell_scale = get(&n.props, "cell_scale").unwrap_or(1.0);
        let lib_key = lib.key();
        let lib_stem = slug(lib.file.res.rsplit('/').next().unwrap_or("library").split('.').next().unwrap_or("library"));
        let mut out = Vec::new();
        for (k, cell) in gres::grid_cells(&n.props).into_iter().enumerate() {
            let thing = match self.lib_things.get(&lib_key).and_then(|m| m.get(&cell.item)) {
                Some(id) => id.clone(),
                None => {
                    let Some(item) = items.get(&cell.item) else { continue };
                    let id = self.lib_thing(&lib, item, &lib_stem);
                    self.lib_things.entry(lib_key.clone()).or_default().insert(cell.item, id.clone());
                    id
                }
            };
            let m = gres::ORTHO.get(cell.rot).copied().unwrap_or(gres::ORTHO[0]);
            let mut e = Entity::new(&format!("{id}-cell-{}", k + 1), &format!("{} {}", n.name, k + 1));
            e.thing = Some(thing);
            e.transform = Transform {
                position: [0, 1, 2].map(|a| round((cell.at[a] as f64 + offset[a]) * size[a])),
                rotation: quat_from_rows([[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]]),
                scale: [round(cell_scale); 3],
            };
            out.push(e);
        }
        self.report.entities += out.len();
        out
    }

    /// Makes a mesh library item into a prop thing: its mesh and its collision shape.
    fn lib_thing(&mut self, lib: &gres::Found, item: &gres::LibItem, lib_stem: &str) -> String {
        let base = slug(&format!("{lib_stem}-{}", item.name));
        let mut id = base.clone();
        let mut k = 2;
        while !self.thing_ids.insert(id.clone()) {
            id = format!("{base}-{k}");
            k += 1;
        }
        let mesh = item.mesh.as_ref().and_then(|m| self.mesh_part(&lib.file, m, None));
        let mut parts = Vec::new();
        let mut children = Vec::new();
        let identity = [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.];
        match (mesh, &item.transform) {
            // A mesh offset inside its cell sits in a child, so the thing's own transform stays the cell's.
            (Some(part), Some(tf)) if tf.iter().zip(identity).any(|(a, b)| (a - b).abs() > 1e-6) => {
                let node = Node {
                    name: "Mesh".into(),
                    ty: Some("Node3D".into()),
                    path: "Mesh".into(),
                    parent: Some(".".into()),
                    instance: None,
                    props: HashMap::from([("transform".into(), format!("Transform3D({})", tf.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")))]),
                };
                let mut child = Entity::new("mesh", "Mesh");
                child.transform = transform(&node, false);
                child.parts.push(part);
                children.push(child);
            }
            (Some(part), _) => parts.push(part),
            (None, _) => {
                parts.push(Part::Custom { name: "MeshLibrary item".into(), values: BTreeMap::from([("note".to_string(), Value::Text("mesh stored in Godot's own format".into()))]) });
                self.untranslated("GridMap item (Godot mesh file)");
            }
        }
        if let Some(found) = item.shape.as_ref().and_then(|s| self.res.resolve(&lib.file, s)) {
            if let Some((shape, size)) = shape_of(found.kind(), found.props(), self.version, false) {
                parts.push(Part::Collider { shape, size, body: Body::Static, trigger: false });
            }
        }
        let thing = Thing { tags: Vec::new(),
            format: FORMAT,
            id: id.clone(),
            name: item.name.clone(),
            kind: ThingKind::Prop,
            role: None,
            description: Some(format!("Level piece from {}", lib.file.res)),
            properties: Vec::new(),
            parts,
            behaviors: Vec::new(),
            children,
        };
        let _ = write_json(&self.dir.join("things").join(ThingKind::Prop.folder()).join(format!("{id}.thing.json")), &thing);
        self.origin.insert(format!("thing:{id}"), Origin { file: lib.file.res.clone(), object: Some(item.name.clone()), field: None, fidelity: Fidelity::Exact, note: Some("GridMap mesh library item".into()) });
        self.report.things += 1;
        id
    }
}

fn quat_from_rows(m: [[f64; 3]; 3]) -> [f32; 4] {
    let trace = m[0][0] + m[1][1] + m[2][2];
    let (x, y, z, w) = if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        ((m[2][1] - m[1][2]) * s, (m[0][2] - m[2][0]) * s, (m[1][0] - m[0][1]) * s, 0.25 / s)
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = 2.0 * (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt();
        (0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s, (m[2][1] - m[1][2]) / s)
    } else if m[1][1] > m[2][2] {
        let s = 2.0 * (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt();
        ((m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s, (m[0][2] - m[2][0]) / s)
    } else {
        let s = 2.0 * (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt();
        ((m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s, (m[1][0] - m[0][1]) / s)
    };
    let r = |v: f64| ((v * 1e6).round() / 1e6) as f32;
    [r(x), r(y), r(z), r(w)]
}

fn round(v: f64) -> f32 {
    ((v * 1e4).round() / 1e4) as f32
}

/// A node's transform in GameX terms.
fn transform(n: &Node, flat: bool) -> Transform {
    let mut t = Transform::default();
    if flat {
        if let Some(p) = n.props.get("position").or(n.props.get("rect_position")).and_then(|v| numbers(v)) {
            t.position = [round(p[0] / PIXELS_PER_UNIT), round(-p[1] / PIXELS_PER_UNIT), 0.0];
        }
        let angle = n
            .props
            .get("rotation")
            .and_then(|v| number(v))
            .or_else(|| n.props.get("rotation_degrees").and_then(|v| number(v)).map(f64::to_radians));
        if let Some(a) = angle {
            // Y points down in Godot 2D and up in GameX, so turns go the other way.
            let h = -a / 2.0;
            t.rotation = [0.0, 0.0, round(h.sin()), round(h.cos())];
        }
        if let Some(s) = n.props.get("scale").and_then(|v| numbers(v)) {
            t.scale = [round(s[0]), round(s[1]), 1.0];
        }
        return t;
    }
    if let Some(v) = n.props.get("transform").and_then(|v| numbers(v)).filter(|v| v.len() == 12) {
        let rows = [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]];
        let scale: Vec<f64> = (0..3).map(|c| (0..3).map(|r| rows[r][c] * rows[r][c]).sum::<f64>().sqrt()).collect();
        let mut unit = rows;
        for row in unit.iter_mut() {
            for (cell, s) in row.iter_mut().zip(&scale) {
                if *s > 1e-9 {
                    *cell /= s;
                }
            }
        }
        t.position = [round(v[9]), round(v[10]), round(v[11])];
        t.rotation = quat_from_rows(unit);
        t.scale = [round(scale[0]), round(scale[1]), round(scale[2])];
    } else if let Some(p) = n.props.get("position").and_then(|v| numbers(v)).filter(|p| p.len() == 3) {
        t.position = [round(p[0]), round(p[1]), round(p[2])];
    }
    t
}

fn get(props: &HashMap<String, String>, key: &str) -> Option<f64> {
    props.get(key).and_then(|v| number(v))
}

fn vec(props: &HashMap<String, String>, key: &str) -> Option<Vec<f64>> {
    props.get(key).and_then(|v| numbers(v))
}

/// A Godot shape resource as a GameX shape and its full size in metres.
fn shape_of(kind: &str, p: &HashMap<String, String>, version: u32, flat: bool) -> Option<(ShapeKind, [f32; 3])> {
    let px = |v: f64| if flat { v / PIXELS_PER_UNIT } else { v };
    let r32 = |v: f64| round(px(v));
    Some(match kind {
        "RectangleShape2D" => {
            let s = vec(p, "size").or_else(|| vec(p, "extents").map(|e| e.iter().map(|x| x * 2.0).collect()))?;
            (ShapeKind::Box, [r32(s[0]), r32(s[1]), 1.0])
        }
        "CircleShape2D" => {
            let d = 2.0 * get(p, "radius").unwrap_or(10.0);
            (ShapeKind::Sphere, [r32(d), r32(d), r32(d)])
        }
        "CapsuleShape2D" | "CapsuleShape3D" | "CapsuleShape" => {
            let radius = get(p, "radius").unwrap_or(if version >= 4 { 0.5 } else { 1.0 });
            let h = get(p, "height").unwrap_or(if version >= 4 { 2.0 } else { 1.0 });
            // Godot 3 measured the middle part only; Godot 4 the whole capsule.
            let total = if version >= 4 { h } else { h + 2.0 * radius };
            (ShapeKind::Capsule, [r32(2.0 * radius), r32(total), r32(2.0 * radius)])
        }
        "BoxShape3D" | "BoxShape" => {
            let s = vec(p, "size").or_else(|| vec(p, "extents").map(|e| e.iter().map(|x| x * 2.0).collect())).unwrap_or(vec![if version >= 4 { 1.0 } else { 2.0 }; 3]);
            (ShapeKind::Box, [r32(s[0]), r32(s[1]), r32(s[2])])
        }
        "SphereShape3D" | "SphereShape" => {
            let d = 2.0 * get(p, "radius").unwrap_or(if version >= 4 { 0.5 } else { 1.0 });
            (ShapeKind::Sphere, [r32(d); 3])
        }
        "CylinderShape3D" | "CylinderShape" => {
            let d = 2.0 * get(p, "radius").unwrap_or(if version >= 4 { 0.5 } else { 1.0 });
            (ShapeKind::Cylinder, [r32(d), r32(get(p, "height").unwrap_or(2.0)), r32(d)])
        }
        "BoxMesh" | "CubeMesh" => {
            let s = vec(p, "size").unwrap_or(vec![if version >= 4 { 1.0 } else { 2.0 }; 3]);
            (ShapeKind::Box, [r32(s[0]), r32(s[1]), r32(s[2])])
        }
        "SphereMesh" => {
            let d = 2.0 * get(p, "radius").unwrap_or(if version >= 4 { 0.5 } else { 1.0 });
            (ShapeKind::Sphere, [r32(d); 3])
        }
        "CapsuleMesh" => {
            let radius = get(p, "radius").unwrap_or(if version >= 4 { 0.5 } else { 1.0 });
            let total = if version >= 4 { get(p, "height").unwrap_or(2.0) } else { get(p, "mid_height").unwrap_or(1.0) + 2.0 * radius };
            (ShapeKind::Capsule, [r32(2.0 * radius), r32(total), r32(2.0 * radius)])
        }
        "CylinderMesh" => {
            let r = get(p, "bottom_radius").or(get(p, "top_radius")).unwrap_or(if version >= 4 { 0.5 } else { 1.0 });
            (ShapeKind::Cylinder, [r32(2.0 * r), r32(get(p, "height").unwrap_or(2.0)), r32(2.0 * r)])
        }
        "PlaneMesh" | "QuadMesh" => {
            let s = vec(p, "size").unwrap_or(vec![if kind == "QuadMesh" { 1.0 } else { 2.0 }; 2]);
            (ShapeKind::Plane, [r32(s[0]), 0.0, r32(s[1])])
        }
        _ => return None,
    })
}

const DEFAULT_COLOR: &str = "#b0b6c8";

/// The albedo colour of a node's material, if it has a simple one.
fn albedo(t: &Tscn, n: &Node, mesh: Option<&HashMap<String, String>>) -> String {
    let material = n
        .props
        .get("material_override")
        .or(n.props.get("surface_material_override/0"))
        .or(n.props.get("material/0"))
        .or(mesh.and_then(|m| m.get("material")));
    material
        .and_then(|m| t.sub(m))
        .and_then(|(_, p)| p.get("albedo_color"))
        .and_then(|c| numbers(c))
        .filter(|c| c.len() >= 3)
        .map(|c| hex(&c))
        .unwrap_or_else(|| DEFAULT_COLOR.into())
}

/// The parts one node is made of.
fn parts_of(ctx: &mut Ctx, t: &Tscn, n: &Node, holder: Holder, flat: bool) -> Vec<Part> {
    let mut parts = Vec::new();
    let ty = n.ty.as_deref().unwrap_or("Node");
    let asset_of = |ctx: &Ctx, value: &str| t.ext_path(value).and_then(|(path, _)| ctx.assets.get(path).cloned());
    match ty {
        "Sprite2D" | "Sprite" | "Sprite3D" => {
            if let Some((res, _)) = n.props.get("texture").and_then(|v| t.ext_path(v)) {
                if let Some(asset) = ctx.assets.get(res).cloned() {
                    let frames = (get(&n.props, "hframes").unwrap_or(1.0).max(1.0), get(&n.props, "vframes").unwrap_or(1.0).max(1.0));
                    let ppu = if ty == "Sprite3D" { get(&n.props, "pixel_size").map(|p| 1.0 / p).unwrap_or(100.0) } else { PIXELS_PER_UNIT };
                    let size = ctx.images.get(res).map(|(w, h)| [round(*w as f64 / frames.0 / ppu), round(*h as f64 / frames.1 / ppu)]);
                    let flip_x = n.props.get("flip_h").is_some_and(|v| v.trim() == "true");
                    parts.push(Part::Sprite { asset, frame: None, size, flip_x });
                }
            }
        }
        "AnimatedSprite2D" | "AnimatedSprite" | "AnimatedSprite3D" => {
            // The first frame of its animations, so it shows; the animations themselves wait for the animation step.
            let first = n.props.get("frames").or(n.props.get("sprite_frames")).and_then(|v| t.sub(v)).and_then(|(_, p)| {
                let anims = p.get("animations")?;
                let at = anims.find("ExtResource")?;
                let end = anims[at..].find(')')? + at + 1;
                t.ext_path(&anims[at..end]).map(|(path, _)| path.clone())
            });
            if let Some(asset) = first.as_ref().and_then(|res| ctx.assets.get(res)).cloned() {
                let res = first.unwrap();
                let size = ctx.images.get(&res).map(|(w, h)| [round(*w as f64 / PIXELS_PER_UNIT), round(*h as f64 / PIXELS_PER_UNIT)]);
                parts.push(Part::Sprite { asset, frame: None, size, flip_x: false });
            }
            parts.push(custom(ty, n, "animations kept for the animation step"));
            ctx.report.untranslated.entry(format!("{ty} (animations)")).and_modify(|c| *c += 1).or_insert(1);
        }
        "MeshInstance3D" | "MeshInstance" => {
            let mesh = n.props.get("mesh").cloned();
            let own = n.props.get("material_override").or(n.props.get("surface_material_override/0")).or(n.props.get("material/0")).map(|_| albedo(t, n, None));
            let color = own.or_else(|| mesh.as_deref().and_then(|m| t.sub(m)).map(|(_, p)| albedo(t, n, Some(p)))).filter(|c| c != DEFAULT_COLOR);
            if let Some(part) = mesh.as_deref().and_then(|m| ctx.mesh_part(&t.file, m, color)) {
                parts.push(part);
            } else if let Some(m) = mesh {
                let kind = t.sub(&m).map(|(k, _)| k.clone()).unwrap_or_else(|| "mesh stored in Godot's own format".into());
                parts.push(custom(ty, n, &kind));
                ctx.untranslated("MeshInstance (Godot mesh file)");
            }
        }
        "TileMap" | "TileMapLayer" => match ctx.tiles(t, n) {
            Some(part) => parts.push(part),
            None => {
                parts.push(custom(ty, n, "tile set not found"));
                ctx.untranslated("TileMap (tile set not found)");
            }
        },
        "GridMap" => {}
        "CSGBox3D" | "CSGBox" => {
            let s = vec(&n.props, "size")
                .or_else(|| Some(vec![get(&n.props, "width")?, get(&n.props, "height")?, get(&n.props, "depth")?]))
                .unwrap_or(vec![if ctx.version >= 4 { 1.0 } else { 2.0 }; 3]);
            parts.push(Part::Shape { shape: ShapeKind::Box, size: [round(s[0]), round(s[1]), round(s[2])], color: albedo(t, n, None) });
        }
        "CollisionShape2D" | "CollisionShape3D" | "CollisionShape" => {
            let shape = n.props.get("shape").and_then(|v| t.sub(v));
            match shape.and_then(|(kind, p)| shape_of(kind, p, ctx.version, flat)) {
                Some((shape, size)) => {
                    let (body, trigger) = match holder {
                        Holder::Body(b) => (b, false),
                        Holder::Area => (Body::Static, true),
                        Holder::None => (Body::Static, false),
                    };
                    parts.push(Part::Collider { shape, size, body, trigger });
                }
                None => {
                    let kind = shape.map(|(k, _)| k.as_str()).unwrap_or("unknown");
                    parts.push(custom(ty, n, &format!("{kind} shape")));
                    ctx.report.untranslated.entry(format!("collision shape {kind}")).and_modify(|c| *c += 1).or_insert(1);
                }
            }
        }
        "DirectionalLight3D" | "DirectionalLight" | "OmniLight3D" | "OmniLight" | "SpotLight3D" | "SpotLight" | "PointLight2D" | "Light2D" => {
            let kind = match ty {
                "DirectionalLight3D" | "DirectionalLight" => LightKind::Directional,
                "SpotLight3D" | "SpotLight" => LightKind::Spot,
                _ => LightKind::Point,
            };
            let color = vec(&n.props, "light_color").or(vec(&n.props, "color")).filter(|c| c.len() >= 3).map(|c| hex(&c)).unwrap_or("#ffffff".into());
            let intensity = get(&n.props, "light_energy").or(get(&n.props, "energy")).unwrap_or(1.0) as f32;
            let range = get(&n.props, "omni_range").or(get(&n.props, "spot_range")).map(|r| r as f32).or((kind == LightKind::Point).then_some(5.0));
            parts.push(Part::Light { kind, color, intensity, range });
        }
        "Camera3D" | "Camera" => parts.push(Part::Camera { fov: Some(get(&n.props, "fov").unwrap_or(if ctx.version >= 4 { 75.0 } else { 70.0 }) as f32), ortho_size: None }),
        "Camera2D" => {
            let zoom = vec(&n.props, "zoom").map(|z| z[1]).unwrap_or(1.0).max(0.01);
            // Godot 3 zooms out with bigger numbers, Godot 4 in.
            let factor = if ctx.version >= 4 { 1.0 / zoom } else { zoom };
            parts.push(Part::Camera { fov: None, ortho_size: Some(round(ctx.viewport_height / 2.0 / PIXELS_PER_UNIT * factor)) });
        }
        "AudioStreamPlayer" | "AudioStreamPlayer2D" | "AudioStreamPlayer3D" => {
            if let Some(asset) = n.props.get("stream").and_then(|v| asset_of(ctx, v)) {
                let volume = get(&n.props, "volume_db").map(|db| 10f64.powf(db / 20.0)).unwrap_or(1.0) as f32;
                let autoplay = n.props.get("autoplay").is_some_and(|v| v.trim() == "true");
                parts.push(Part::Sound { asset, volume: round(volume as f64), looped: false, autoplay });
            }
        }
        "Label" | "Label3D" | "RichTextLabel" => {
            let words = n.props.get("text").or(n.props.get("bbcode_text")).and_then(|v| text(v)).unwrap_or_default();
            let px = get(&n.props, "font_size").unwrap_or(16.0);
            parts.push(Part::Text { text: words, size: round(if ty == "Label3D" { px * 0.005 } else { px / PIXELS_PER_UNIT }), color: "#ffffff".into() });
        }
        _ if plain(ty) => {}
        _ => {
            parts.push(custom(ty, n, ""));
            *ctx.report.untranslated.entry(ty.to_string()).or_insert(0) += 1;
        }
    }
    if let Some((script, _)) = n.props.get("script").and_then(|v| t.ext_path(v)) {
        let file = format!("sources/{}/decompiled/{}", ctx.source, script.trim_start_matches("res://"));
        let mut values = BTreeMap::new();
        values.insert("file".into(), Value::Text(file));
        values.insert("status".into(), Value::Text("not translated yet".into()));
        parts.push(Part::Custom { name: "GDScript".into(), values });
    }
    parts
}

/// A node GameX has no part for: kept with its simple values, so nothing is lost.
fn custom(ty: &str, n: &Node, note: &str) -> Part {
    let mut values: BTreeMap<String, Value> = n
        .props
        .iter()
        .filter(|(k, v)| !k.contains('/') && v.len() < 80 && !matches!(k.as_str(), "transform" | "position" | "rotation" | "scale" | "script"))
        .filter_map(|(k, v)| literal(v).map(|lit| (k.clone(), lit)))
        .take(24)
        .collect();
    if !note.is_empty() {
        values.insert("note".into(), Value::Text(note.into()));
    }
    Part::Custom { name: ty.into(), values }
}

/// Exported script variables, Godot 3 (`export var x = 1`, `export(int) var x = 1`) and 4 (`@export var x := 1`).
pub fn exported(script: &str) -> Vec<Property> {
    let mut out = Vec::new();
    for line in script.lines() {
        let l = line.trim();
        if !(l.starts_with("export") || l.starts_with("@export")) {
            continue;
        }
        let Some(at) = l.find("var ") else { continue };
        let decl = &l[at + 4..];
        let name: String = decl.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if name.is_empty() {
            continue;
        }
        let Some(eq) = decl.find('=') else { continue };
        let raw = decl[eq + 1..].split('#').next().unwrap_or("").trim();
        if let Some(value) = literal(raw) {
            out.push(Property { name, value, unit: None, description: None });
        }
    }
    out
}

/// "PauseMenu_singleplayer" -> ["pause", "menu", "singleplayer"]: whole words, so "singleplayer" isn't "player".
fn words_of(name: &str) -> Vec<String> {
    let spaced: String = name.chars().fold(String::new(), |mut s, c| {
        if c.is_uppercase() && s.chars().last().is_some_and(|p| p.is_lowercase()) {
            s.push(' ');
        }
        s.push(if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' });
        s
    });
    spaced.split_whitespace().map(|w| w.trim_end_matches(|c: char| c.is_ascii_digit()).to_string()).filter(|w| !w.is_empty()).collect()
}

pub(super) fn guess_kind(name: &str, root_ty: &str) -> (ThingKind, Option<String>) {
    let words = words_of(name);
    let has = |list: &[&str]| words.iter().any(|w| list.contains(&w.as_str()) || list.iter().any(|l| w.as_str() == format!("{l}s")));
    // Screens and levels first: a "coins counter" is a label, not a coin.
    if has(&["menu", "hud", "ui", "gui", "button", "counter", "label", "background", "parallax", "level", "stage", "world", "music", "joystick"]) {
        return (ThingKind::Other, None);
    }
    // What it is beats who uses it: a "player bullet" is a bullet.
    if has(&["gun", "weapon", "sword", "bullet", "tool", "projectile", "bomb"]) {
        return (ThingKind::Tool, None);
    }
    if has(&["coin", "gem", "key", "pickup", "collectible", "item", "potion", "heart", "powerup", "moneda", "llave", "pocion", "gema", "corazon"]) {
        return (ThingKind::Item, None);
    }
    if has(&["platform", "door", "crate", "box", "wall", "block", "trap", "spike", "spikes", "barrel", "chest", "lever", "switch"]) {
        return (ThingKind::Prop, None);
    }
    if has(&["enemy", "mob", "monster", "zombie", "bat", "slime", "ghost", "boss", "creep", "zombi", "enemigo", "monstruo", "fantasma", "murcielago"]) {
        return (ThingKind::Character, Some("enemy".into()));
    }
    if has(&["player", "hero", "jugador", "heroe"]) {
        return (ThingKind::Character, Some("player".into()));
    }
    if has(&["npc", "villager"]) {
        return (ThingKind::Character, Some("npc".into()));
    }
    match holder_of(root_ty) {
        Some(Holder::Body(Body::Kinematic)) => (ThingKind::Character, None),
        Some(_) => (ThingKind::Prop, None),
        None => (ThingKind::Other, None),
    }
}

/// Builds the entity for node `n` and everything under it.
fn entity_tree(ctx: &mut Ctx, t: &Tscn, n: &Node, holder: Holder, flat: bool, taken: &mut HashSet<String>) -> Entity {
    let id = {
        let base = if n.path == "." { slug(&n.name) } else { slug(&n.path.replace('/', "-")) };
        let mut id = base.clone();
        let mut k = 2;
        while !taken.insert(id.clone()) {
            id = format!("{base}-{k}");
            k += 1;
        }
        id
    };
    let mut e = Entity::new(&id, &n.name);
    let tf = transform(n, flat);
    if tf != Transform::default() {
        e.transform = tf;
    }
    if n.props.get("visible").is_some_and(|v| v.trim() == "false") {
        e.active = false;
    }
    let mut fidelity = Fidelity::Exact;
    if let Some((res, _)) = n.instance.as_ref().and_then(|i| t.ext.get(i)) {
        let twin = res.strip_suffix(".scn").map(|s| format!("{s}.tscn")).unwrap_or_default();
        if let Some(thing) = ctx.things.get(res).or(ctx.things.get(&twin)).cloned() {
            // Values the instance sets for the thing's exported variables.
            for p in ctx.thing_props.get(&thing).cloned().unwrap_or_default() {
                if let Some(v) = n.props.get(&p.name).and_then(|v| literal(v)) {
                    e.overrides.insert(p.name.clone(), v);
                }
            }
            e.thing = Some(thing);
        } else if let Some(asset) = ctx.assets.get(res).cloned() {
            e.parts.push(Part::Model { asset });
        } else {
            ctx.report.problems.push(format!("{}: {} places {res}, which wasn't found.", t.res, n.path));
            fidelity = Fidelity::Missing;
        }
    }
    let here = n.ty.as_deref().and_then(holder_of).unwrap_or(holder);
    e.parts.extend(parts_of(ctx, t, n, holder, flat));
    if n.ty.as_deref() == Some("GridMap") {
        let cells = ctx.grid(t, n, &id);
        e.children.extend(cells);
    }
    if e.parts.iter().any(|p| matches!(p, Part::Custom { name, .. } if name != "GDScript")) {
        fidelity = Fidelity::Missing;
    }
    ctx.origin.insert(
        format!("entity:{}:{id}", slug(&t.res)),
        Origin { file: t.res.clone(), object: Some(n.path.clone()), field: None, fidelity, note: n.ty.clone() },
    );
    ctx.report.entities += 1;
    let kids: Vec<&Node> = t.children(&n.path).collect();
    for child in kids {
        e.children.push(entity_tree(ctx, t, child, here, flat, taken));
    }
    e
}

/// (2D-ish nodes, 3D-ish nodes) in a scene.
fn dims_count(t: &Tscn) -> (u32, u32) {
    t.nodes.iter().filter_map(|n| n.ty.as_deref()).fold((0, 0), |(a, b), ty| (a + is_2d(ty) as u32, b + is_3d(ty) as u32))
}

/// 2D or 3D: by its own nodes, or by the rest of the game when it has neither (a scene that only plays music).
fn flat_scene(t: &Tscn, game_flat: bool) -> bool {
    match dims_count(t) {
        (0, 0) => game_flat,
        (two, three) => two > three,
    }
}

fn files_with(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().is_some_and(|x| exts.iter().any(|w| x.eq_ignore_ascii_case(w))))
        .map(|e| e.into_path())
        .collect()
}

fn res_of(root: &Path, path: &Path) -> String {
    format!("res://{}", path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/"))
}

/// project.godot: (config version, main scene, window height).
fn settings(godot: &Path) -> io::Result<(u32, Option<String>, f64)> {
    let text = fs::read_to_string(godot.join("project.godot")).map_err(|_| io::Error::new(io::ErrorKind::NotFound, "This folder has no project.godot: it isn't a Godot project."))?;
    let mut config = 0;
    let mut main = None;
    let mut height = None;
    for line in text.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("config_version=") {
            config = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = l.strip_prefix("run/main_scene=") {
            main = text_value(v);
        } else if let Some(v) = l.strip_prefix("window/size/viewport_height=").or(l.strip_prefix("window/size/height=")) {
            height = v.trim().parse().ok();
        }
    }
    // config_version 4 = Godot 3, 5 = Godot 4.
    Ok((if config >= 5 { 4 } else { 3 }, main, height.unwrap_or(if config >= 5 { 648.0 } else { 600.0 })))
}

// ---------------------------------------------------------------------------
// Writing changes back (output A for Godot projects kept as text)

/// A number as Godot writes it: no trailing zeros.
fn godot_number(v: f64) -> String {
    let r = (v * 1e5).round() / 1e5;
    let r = if r == 0.0 { 0.0 } else { r };
    let s = format!("{r:.5}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// The properties to set on `n` so it matches `t` (only those that changed).
fn changed_props(n: &Node, t: &Transform, active: bool, flat: bool, version: u32) -> Vec<(String, String)> {
    let old = transform(n, flat);
    let mut out = Vec::new();
    if flat {
        if !(close(old.position[0], t.position[0]) && close(old.position[1], t.position[1])) {
            let key = if n.props.contains_key("rect_position") { "rect_position" } else { "position" };
            out.push((key.into(), format!("Vector2({}, {})", godot_number(t.position[0] as f64 * PIXELS_PER_UNIT), godot_number(-t.position[1] as f64 * PIXELS_PER_UNIT))));
        }
        let angle = |r: &[f32; 4]| -2.0 * (r[2] as f64).atan2(r[3] as f64);
        if (angle(&old.rotation) - angle(&t.rotation)).abs() > 1e-3 {
            let a = angle(&t.rotation);
            if version < 4 || n.props.contains_key("rotation_degrees") {
                out.push(("rotation_degrees".into(), godot_number(a.to_degrees())));
            } else {
                out.push(("rotation".into(), godot_number(a)));
            }
        }
        if !(close(old.scale[0], t.scale[0]) && close(old.scale[1], t.scale[1])) {
            out.push(("scale".into(), format!("Vector2({}, {})", godot_number(t.scale[0] as f64), godot_number(t.scale[1] as f64))));
        }
    } else {
        let same = (0..3).all(|i| close(old.position[i], t.position[i]) && close(old.scale[i], t.scale[i]))
            && (0..4).all(|i| close(old.rotation[i], t.rotation[i]) || close(old.rotation[i], -t.rotation[i]));
        if !same {
            let [x, y, z, w] = t.rotation.map(|v| v as f64);
            let m = [
                [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
                [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
                [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
            ];
            // Godot keeps the basis rows with each column scaled.
            let mut v: Vec<String> = Vec::new();
            for row in &m {
                for (c, cell) in row.iter().enumerate() {
                    v.push(godot_number(cell * t.scale[c] as f64));
                }
            }
            v.extend(t.position.iter().map(|p| godot_number(*p as f64)));
            let ty = if version >= 4 { "Transform3D" } else { "Transform" };
            out.push(("transform".into(), format!("{ty}({})", v.join(", "))));
        }
    }
    let was_active = !n.props.get("visible").is_some_and(|v| v.trim() == "false");
    if was_active != active {
        out.push(("visible".into(), active.to_string()));
    }
    out
}

/// What writing back found: files changed (res:// path, new text) and how many objects.
pub struct WrittenBack {
    pub files: Vec<(String, String)>,
    pub objects: usize,
}

/// Objects moved, turned, resized or hidden in the layer's scenes, back into
/// the Godot scene files they came from. `read` gives a scene file's current
/// text by its res:// path. Objects added or removed in the layer aren't
/// written (yet): only changes to objects the game already has.
pub fn write_back(layer: &Path, source: &str, godot: &Path, read: &dyn Fn(&str) -> Option<String>) -> io::Result<WrittenBack> {
    let (version, _, _) = settings(godot)?;
    let text = fs::read_to_string(layer.join("sources").join(source).join("origin.json"))?;
    let origin: OriginMap = serde_json::from_str(&text).map_err(io::Error::other)?;
    let mut out = WrittenBack { files: Vec::new(), objects: 0 };
    for item in project::open(layer)?.scenes {
        let Some(res) = origin.entries.get(&format!("scene:{}", item.id)).map(|o| o.file.clone()) else { continue };
        let Some(mut text) = read(&res) else { continue };
        let scene = project::load_scene(layer, &item.id)?;
        let flat = scene.dimensions == Dimensions::TwoD;
        let tscn = Tscn::read(&res, &text);
        let prefix = format!("entity:{}:", slug(&res));
        let mut changed = false;
        for e in scene.entities.iter().flat_map(|e| e.walk()) {
            let Some(path) = origin.entries.get(&format!("{prefix}{}", e.id)).and_then(|o| o.object.clone()) else { continue };
            let Some(node) = tscn.nodes.iter().find(|n| n.path == path) else { continue };
            let props = changed_props(node, &e.transform, e.active, flat, version);
            if props.is_empty() {
                continue;
            }
            for (key, value) in props {
                text = crate::scene::set_property(&text, &path, &key, &value).map_err(io::Error::other)?;
            }
            out.objects += 1;
            changed = true;
        }
        if changed {
            out.files.push((res, text));
        }
    }
    Ok(out)
}

/// The game's name from project.godot (`config/name`), if it has one.
pub fn game_name(godot: &Path) -> Option<String> {
    let text = fs::read_to_string(godot.join("project.godot")).ok()?;
    text.lines().find_map(|l| text_value(l.trim().strip_prefix("config/name=")?)).filter(|n| !n.trim().is_empty())
}

fn text_value(v: &str) -> Option<String> {
    text(v).or_else(|| Some(v.trim().to_string()))
}

/// Translates the Godot project in `godot` into the GameX project at `dir`,
/// as the source `source` (a slug: its files go in `sources/<source>/`).
pub fn translate(godot: &Path, dir: &Path, source: &str, name: &str) -> io::Result<Report> {
    let (version, main, viewport_height) = settings(godot)?;
    let mut ctx = Ctx {
        version,
        source,
        assets: HashMap::new(),
        images: HashMap::new(),
        things: HashMap::new(),
        thing_props: HashMap::new(),
        exports: HashMap::new(),
        viewport_height,
        report: Report { source: source.into(), godot_version: version, engine: format!("Godot {version}"), ..Default::default() },
        origin: BTreeMap::new(),
        res: Resources::new(godot),
        dir: dir.to_path_buf(),
        meshes: HashMap::new(),
        lib_things: HashMap::new(),
        thing_ids: HashSet::new(),
    };

    // The game's files, kept as read (scripts are read from here).
    let decompiled = dir.join("sources").join(source).join("decompiled");
    // Godot's own cache folders (.godot, .import) are left out.
    for entry in WalkDir::new(godot).into_iter().filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.')).filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let target = decompiled.join(entry.path().strip_prefix(godot).unwrap_or(entry.path()));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), &target)?;
    }

    // Assets, by kind.
    let kinds: &[(&str, &[&str])] = &[
        ("textures", &["png", "jpg", "jpeg", "webp", "svg"]),
        ("audio", &["wav", "ogg", "mp3"]),
        ("fonts", &["ttf", "otf", "woff", "woff2"]),
        ("models", &["glb", "gltf", "obj"]),
    ];
    for (kind, exts) in kinds {
        for f in files_with(godot, exts) {
            let res = res_of(godot, &f);
            let asset = format!("{kind}/{source}/{}", res.trim_start_matches("res://"));
            let target = dir.join("assets").join(&asset);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&f, &target)?;
            // A glTF's buffers and textures sit next to it.
            if *kind == "textures" {
                if let Ok(size) = imagesize::size(&f) {
                    ctx.images.insert(res.clone(), (size.width as u32, size.height as u32));
                }
            }
            ctx.origin.insert(format!("asset:{asset}"), Origin { file: res.clone(), object: None, field: None, fidelity: Fidelity::Exact, note: None });
            ctx.assets.insert(res, asset);
            ctx.report.assets += 1;
        }
    }
    for f in files_with(godot, &["bin"]) {
        // glTF buffers, copied beside their model.
        let res = res_of(godot, &f);
        let target = dir.join("assets").join("models").join(source).join(res.trim_start_matches("res://"));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&f, &target)?;
    }

    // Scripts and what they export.
    for f in files_with(godot, &["gd"]) {
        let res = res_of(godot, &f);
        ctx.exports.insert(res, exported(&fs::read_to_string(&f).unwrap_or_default()));
        ctx.report.scripts += 1;
    }

    // Scenes: which are things (placed by others, or bodies) and which are scenes.
    let mut tscns: Vec<Tscn> = Vec::new();
    for f in files_with(godot, &["tscn"]) {
        let res = res_of(godot, &f);
        match fs::read_to_string(&f) {
            Ok(text) => tscns.push(Tscn::read(&res, &text)),
            Err(e) => ctx.report.problems.push(format!("{res}: {e}")),
        }
    }
    let mut placed: HashSet<String> = HashSet::new();
    for t in &tscns {
        for n in &t.nodes {
            if let Some((res, _)) = n.instance.as_ref().and_then(|i| t.ext.get(i)) {
                placed.insert(res.clone());
            }
            // Scenes held by an exported variable (spawned in code): `mob_scene = ExtResource(...)`.
            for v in n.props.values() {
                if let Some((res, kind)) = t.ext_path(v) {
                    if kind == "PackedScene" {
                        placed.insert(res.clone());
                    }
                }
            }
        }
    }
    let (two, three) = tscns.iter().map(dims_count).fold((0, 0), |(a, b), (x, y)| (a + x, b + y));
    let game_flat = two > three;
    // Things are named by their file; the folder is added only to tell two apart.
    let stems: Vec<String> = tscns.iter().map(|t| slug(t.res.rsplit('/').next().unwrap_or(&t.res).trim_end_matches(".tscn"))).collect();
    for (i, t) in tscns.iter().enumerate() {
        let root_ty = t.root().and_then(|r| r.ty.as_deref()).unwrap_or("Node");
        let is_main = main.as_deref() == Some(t.res.as_str());
        let is_thing = !is_main && (placed.contains(&t.res) || holder_of(root_ty).is_some());
        let unique = stems.iter().filter(|s| **s == stems[i]).count() == 1;
        let stem = if unique { stems[i].clone() } else { slug(t.res.trim_start_matches("res://").trim_end_matches(".tscn").replace('/', "-").as_str()) };
        if is_thing {
            let mut id = stem.clone();
            let mut k = 2;
            while !ctx.thing_ids.insert(id.clone()) {
                id = format!("{stem}-{k}");
                k += 1;
            }
            ctx.things.insert(t.res.clone(), id);
        }
    }

    // Things first, so instances can be typed by their properties.
    let mut info = project::open(dir)?.info;
    for t in &tscns {
        let Some(id) = ctx.things.get(&t.res).cloned() else { continue };
        let Some(root) = t.root() else { continue };
        let script = root.props.get("script").and_then(|v| t.ext_path(v)).map(|(p, _)| p.clone());
        let props = script.as_ref().and_then(|s| ctx.exports.get(s)).cloned().unwrap_or_default();
        ctx.thing_props.insert(id.clone(), props);
    }
    for t in &tscns {
        let Some(id) = ctx.things.get(&t.res).cloned() else { continue };
        let Some(root) = t.root() else { continue };
        let flat = flat_scene(t, game_flat);
        let root_ty = root.ty.as_deref().unwrap_or("Node");
        let (kind, role) = guess_kind(&format!("{} {}", root.name, t.res.rsplit('/').next().unwrap_or(&t.res).trim_end_matches(".tscn")), root_ty);
        let holder = holder_of(root_ty).unwrap_or(Holder::None);
        let mut taken = HashSet::new();
        let mut properties = ctx.thing_props.get(&id).cloned().unwrap_or_default();
        // Values the scene itself sets for its script's variables.
        for p in properties.iter_mut() {
            if let Some(v) = root.props.get(&p.name).and_then(|v| literal(v)) {
                p.value = v;
            }
        }
        let parts = parts_of(&mut ctx, t, root, holder, flat);
        let mut children = Vec::new();
        for child in t.children(".") {
            children.push(entity_tree(&mut ctx, t, child, holder, flat, &mut taken));
        }
        let thing = Thing { tags: Vec::new(),
            format: FORMAT,
            id: id.clone(),
            name: root.name.clone(),
            kind,
            role,
            description: Some(format!("Translated from {}", t.res)),
            properties,
            parts,
            behaviors: Vec::new(),
            children,
        };
        ctx.origin.insert(
            format!("thing:{id}"),
            Origin { file: t.res.clone(), object: Some(".".into()), field: None, fidelity: Fidelity::Approximated, note: Some("kind and role guessed from names and body type".into()) },
        );
        write_json(&dir.join("things").join(kind.folder()).join(format!("{id}.thing.json")), &thing)?;
        ctx.report.things += 1;
    }

    let mut start = None;
    let mut used = HashSet::new();
    for t in &tscns {
        if ctx.things.contains_key(&t.res) {
            continue;
        }
        let Some(root) = t.root() else { continue };
        let flat = flat_scene(t, game_flat);
        let i = tscns.iter().position(|x| std::ptr::eq(x, t)).unwrap_or(0);
        let unique = stems.iter().filter(|s| **s == stems[i]).count() == 1;
        let base = if unique { stems[i].clone() } else { slug(t.res.trim_start_matches("res://").trim_end_matches(".tscn").replace('/', "-").as_str()) };
        let mut id = base.clone();
        let mut k = 2;
        while !used.insert(id.clone()) {
            id = format!("{base}-{k}");
            k += 1;
        }
        let mut taken = HashSet::new();
        let top = entity_tree(&mut ctx, t, root, Holder::None, flat, &mut taken);
        // A bare root (just a container) isn't kept: its children are the scene.
        let entities = if top.parts.is_empty() && top.thing.is_none() && top.transform == Transform::default() { top.children } else { vec![top] };
        let scene = Scene {
            format: FORMAT,
            id: id.clone(),
            name: root.name.clone(),
            dimensions: if flat { Dimensions::TwoD } else { Dimensions::ThreeD },
            environment: Environment { background: if flat { "#20263a".into() } else { "#87a8c8".into() }, ..Environment::default() },
            entities,
        };
        if main.as_deref() == Some(t.res.as_str()) {
            start = Some(id.clone());
        }
        ctx.origin.insert(format!("scene:{id}"), Origin { file: t.res.clone(), object: None, field: None, fidelity: Fidelity::Exact, note: None });
        write_json(&dir.join("world").join(format!("{id}.scene.json")), &scene)?;
        ctx.report.scenes += 1;
    }

    write_json(
        &dir.join("sources").join(source).join("origin.json"),
        &OriginMap { format: FORMAT, source: source.into(), entries: std::mem::take(&mut ctx.origin) },
    )?;
    info.sources.retain(|s| s.id != source);
    info.sources.push(Source {
        id: source.into(),
        name: name.into(),
        engine: "godot".into(),
        engine_version: Some(format!("{version}.x")),
        imported_from: Some(godot.display().to_string()),
    });
    if info.settings.start_scene.is_none() {
        info.settings.start_scene = start;
    }
    write_json(&dir.join(project::PROJECT_FILE), &info)?;
    write_json(&dir.join("sources").join(source).join("report.json"), &ctx.report)?;
    Ok(ctx.report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_godot_values() {
        assert_eq!(numbers("Vector2( 1, -2.5 )"), Some(vec![1.0, -2.5]));
        assert_eq!(resource_id("ExtResource( 3 )", "ExtResource").as_deref(), Some("3"));
        assert_eq!(resource_id("ExtResource(\"1_ab\")", "ExtResource").as_deref(), Some("1_ab"));
        assert_eq!(literal("300"), Some(Value::Integer(300)));
        assert_eq!(literal("14.5"), Some(Value::Number(14.5)));
        assert_eq!(literal("Color(1, 0, 0, 1)"), Some(Value::Color("#ff0000".into())));
        assert_eq!(literal("\"Hi\""), Some(Value::Text("Hi".into())));
    }

    #[test]
    fn finds_exported_variables_in_godot_3_and_4() {
        let g3 = "extends KinematicBody2D\nexport var speed = 400 # px/s\nexport(int) var lives = 3\nvar hidden = 1\nexport (PackedScene) var Mob\n";
        let g4 = "extends CharacterBody3D\n@export var jump: float = 7.5\n@export_range(0, 10) var gems := 2\n@export var mob_scene: PackedScene\n";
        let names = |p: Vec<Property>| p.into_iter().map(|p| (p.name, p.value)).collect::<Vec<_>>();
        assert_eq!(names(exported(g3)), vec![("speed".into(), Value::Integer(400)), ("lives".into(), Value::Integer(3))]);
        assert_eq!(names(exported(g4)), vec![("jump".into(), Value::Number(7.5)), ("gems".into(), Value::Integer(2))]);
    }

    #[test]
    fn turns_godot_transforms_into_gamex_ones() {
        // 90 degrees about Y, scaled 2, at (1, 2, 3).
        let n = Node {
            name: "A".into(),
            ty: Some("Node3D".into()),
            path: "A".into(),
            parent: Some(".".into()),
            instance: None,
            props: HashMap::from([("transform".into(), "Transform3D(0, 0, 2, 0, 2, 0, -2, 0, 0, 1, 2, 3)".into())]),
        };
        let t = transform(&n, false);
        assert_eq!(t.position, [1.0, 2.0, 3.0]);
        assert_eq!(t.scale, [2.0, 2.0, 2.0]);
        assert!((t.rotation[1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3 && (t.rotation[3] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3, "{:?}", t.rotation);
        // 2D: pixels to metres, Y up.
        let n2 = Node { props: HashMap::from([("position".into(), "Vector2(250, 100)".into())]), ..n };
        assert_eq!(transform(&n2, true).position, [2.5, -1.0, 0.0]);
    }

    /// Real games: `GX_GODOT_GAMES="dir1;dir2" cargo test -- --ignored real_godot_games --nocapture`.
    #[test]
    #[ignore]
    fn real_godot_games() {
        let list = std::env::var("GX_GODOT_GAMES").expect("set GX_GODOT_GAMES to Godot project folders, separated by ;");
        for game in list.split(';').filter(|s| !s.trim().is_empty()) {
            let root = tempfile::tempdir().unwrap();
            let dir = project::create(root.path(), "Real").unwrap();
            let report = translate(Path::new(game.trim()), &dir, "game", "Game").unwrap();
            let index = project::open(&dir).unwrap();
            println!("\n== {game}\n{}", serde_json::to_string(&report).unwrap());
            println!("problems on open: {:?}", index.problems.iter().map(|p| format!("{}: {}", p.file, p.message)).collect::<Vec<_>>());
            println!("scenes: {:?}", index.scenes.iter().map(|s| format!("{} ({})", s.id, s.kind.as_deref().unwrap_or(""))).collect::<Vec<_>>());
            println!("things: {:?}", index.things.iter().map(|t| format!("{} ({})", t.id, t.kind.as_deref().unwrap_or(""))).collect::<Vec<_>>());
            println!("start: {:?}", index.info.settings.start_scene);
        }
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    #[test]
    fn translates_a_small_godot_4_game() {
        let game = tempfile::tempdir().unwrap();
        let g = game.path();
        write(g, "project.godot", "config_version=5\n[application]\nrun/main_scene=\"res://main.tscn\"\n");
        write(g, "player.gd", "extends CharacterBody2D\n@export var speed = 400\n");
        write(
            g,
            "player.tscn",
            "[gd_scene load_steps=3 format=3]\n[ext_resource type=\"Script\" path=\"res://player.gd\" id=\"1\"]\n[sub_resource type=\"CapsuleShape2D\" id=\"s1\"]\nradius = 20.0\nheight = 60.0\n[node name=\"Player\" type=\"CharacterBody2D\"]\nscript = ExtResource(\"1\")\n[node name=\"Shape\" type=\"CollisionShape2D\" parent=\".\"]\nshape = SubResource(\"s1\")\n",
        );
        write(
            g,
            "main.tscn",
            "[gd_scene format=3]\n[ext_resource type=\"PackedScene\" path=\"res://player.tscn\" id=\"2\"]\n[node name=\"Main\" type=\"Node2D\"]\n[node name=\"Player\" parent=\".\" instance=ExtResource(\"2\")]\nposition = Vector2(100, 200)\nspeed = 250\n[node name=\"Timer\" type=\"Timer\" parent=\".\"]\nwait_time = 0.5\n",
        );
        let root = tempfile::tempdir().unwrap();
        let dir = project::create(root.path(), "Translated").unwrap();
        let report = translate(g, &dir, "demo", "Demo").unwrap();
        assert_eq!((report.godot_version, report.scenes, report.things, report.scripts), (4, 1, 1, 1));
        assert_eq!(report.untranslated.get("Timer"), Some(&1));

        let index = project::open(&dir).unwrap();
        assert!(index.problems.is_empty(), "{:?}", index.problems);
        assert_eq!(index.info.settings.start_scene.as_deref(), Some("main"));
        assert_eq!(index.info.sources[0].engine, "godot");

        let player = project::load_thing(&dir, "player").unwrap();
        assert_eq!((player.kind, player.role.as_deref()), (ThingKind::Character, Some("player")));
        assert_eq!(player.properties[0].value, Value::Integer(400));
        let shape = &player.children[0].parts[0];
        assert_eq!(shape, &Part::Collider { shape: ShapeKind::Capsule, size: [0.4, 0.6, 0.4], body: Body::Kinematic, trigger: false });
        assert!(player.parts.iter().any(|p| matches!(p, Part::Custom { name, .. } if name == "GDScript")));

        let main = project::load_scene(&dir, "main").unwrap();
        assert_eq!(main.dimensions, Dimensions::TwoD);
        let p = &main.entities[0];
        assert_eq!(p.thing.as_deref(), Some("player"));
        assert_eq!(p.transform.position, [1.0, -2.0, 0.0]);
        assert_eq!(p.overrides.get("speed"), Some(&Value::Integer(250)));
        assert!(matches!(&main.entities[1].parts[0], Part::Custom { name, .. } if name == "Timer"));
        assert!(dir.join("sources/demo/decompiled/player.gd").is_file());
        assert!(dir.join("sources/demo/origin.json").is_file());
    }
}
