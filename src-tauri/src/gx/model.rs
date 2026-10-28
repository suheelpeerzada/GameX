//! The GameX format's data, as stored in a project's JSON files. Every
//! engine reader translates into these types, and the workspace and the
//! GameX engine only ever read them. See docs/PLAN.md and docs/FORMAT.md.
//!
//! Conventions: right-handed, Y up, metres (as glTF); rotations are
//! quaternions `[x, y, z, w]`; colours are `#rrggbb` or `#rrggbbaa`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The format version this GameX writes. Files with a higher number were
/// made by a newer GameX and are refused rather than misread.
pub const FORMAT: u32 = 1;

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

fn one() -> f32 {
    1.0
}

// ---------------------------------------------------------------------------
// project.json

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub format: u32,
    pub id: String,
    pub name: String,
    /// The games translated into this project (empty for a hand-made one).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub settings: ProjectSettings,
}

/// A game translated into the project; its files are in `sources/<id>/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub name: String,
    /// The reader that translated it: "godot", "unity"...
    pub engine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    /// Where the game was imported from, for reference only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectSettings {
    /// The scene Play starts with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_scene: Option<String>,
    /// How many sprite pixels make one unit (metre) in 2D scenes.
    pub pixels_per_unit: f32,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings { start_scene: None, pixels_per_unit: 100.0 }
    }
}

// ---------------------------------------------------------------------------
// Values and properties

/// A typed value: a property, a block's argument, an override.
/// Stored as `{"type": "number", "value": 7.1}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum Value {
    Number(f64),
    Integer(i64),
    Bool(bool),
    Text(String),
    /// 2 to 4 numbers: a position, a size, a direction.
    Vector(Vec<f64>),
    Color(String),
    /// Another thing, by ID.
    Thing(String),
    /// A file under `assets/`.
    Asset(String),
}

/// A named, typed property of a thing or a behavior: health, chase speed...
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    pub name: String,
    #[serde(flatten)]
    pub value: Value,
    /// "m", "m/s", "s", "hp"... shown next to the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Scenes

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Dimensions {
    #[serde(rename = "2d")]
    TwoD,
    #[default]
    #[serde(rename = "3d")]
    ThreeD,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Environment {
    pub background: String,
    pub ambient: String,
    /// Metres per second squared.
    pub gravity: [f32; 3],
}

impl Default for Environment {
    fn default() -> Self {
        Environment { background: "#87a8c8".into(), ambient: "#404040".into(), gravity: [0.0, -9.81, 0.0] }
    }
}

/// One scene: `world/<id>.scene.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub format: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub dimensions: Dimensions,
    #[serde(default)]
    pub environment: Environment,
    #[serde(default)]
    pub entities: Vec<Entity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transform {
    #[serde(default)]
    pub position: [f32; 3],
    /// Quaternion `[x, y, z, w]`.
    #[serde(default = "Transform::no_rotation")]
    pub rotation: [f32; 4],
    #[serde(default = "Transform::unit_scale")]
    pub scale: [f32; 3],
}

impl Transform {
    fn no_rotation() -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }
    fn unit_scale() -> [f32; 3] {
        [1.0, 1.0, 1.0]
    }
    pub fn at(position: [f32; 3]) -> Self {
        Transform { position, ..Default::default() }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform { position: [0.0; 3], rotation: Self::no_rotation(), scale: Self::unit_scale() }
    }
}

/// Something placed in a scene. With `thing` set it's an instance of that
/// thing: it gets the thing's parts, behaviors and properties, and its own
/// `overrides` change property values for this instance only.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thing: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub transform: Transform,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub active: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Part>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub behaviors: Vec<BehaviorUse>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Entity>,
}

impl Entity {
    pub fn new(id: &str, name: &str) -> Self {
        Entity { id: id.into(), name: name.into(), active: true, ..Default::default() }
    }

    /// This entity and everything under it, depth first.
    pub fn walk(&self) -> Vec<&Entity> {
        let mut out = vec![self];
        for c in &self.children {
            out.extend(c.walk());
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShapeKind {
    Box,
    Sphere,
    Capsule,
    Cylinder,
    Plane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Body {
    /// Doesn't move: walls, floors.
    #[default]
    Static,
    /// Moved by physics: crates, balls.
    Dynamic,
    /// Moved by behavior, pushes others: characters, lifts.
    Kinematic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LightKind {
    Directional,
    Point,
    Spot,
}

/// What an entity is made of. Stored with a `"type"` field:
/// `{"type": "model", "asset": "models/door.glb"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Part {
    /// A simple shape, for blockouts and hand-made scenes. `size` is the
    /// full extent in metres (a sphere's diameter is its x).
    Shape { shape: ShapeKind, size: [f32; 3], color: String },
    /// A 3D model under `assets/`.
    Model { asset: String },
    Sprite {
        asset: String,
        /// A named frame of a sprite sheet (`*.sprite.json`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame: Option<String>,
        /// Width and height in metres; from the image and pixels-per-unit when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<[f32; 2]>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip_x: bool,
    },
    Collider {
        shape: ShapeKind,
        size: [f32; 3],
        #[serde(default)]
        body: Body,
        /// Detects overlaps without blocking.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        trigger: bool,
    },
    Light {
        kind: LightKind,
        color: String,
        #[serde(default = "one")]
        intensity: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        range: Option<f32>,
    },
    Camera {
        /// Vertical field of view in degrees (3D).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fov: Option<f32>,
        /// Half the view's height in metres (2D).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ortho_size: Option<f32>,
    },
    Sound {
        asset: String,
        #[serde(default = "one")]
        volume: f32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        looped: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        autoplay: bool,
    },
    Text { text: String, size: f32, color: String },
    /// A grid of tiles cut from tile sheets: a 2D level layout.
    Tiles {
        /// One cell's width and height in metres.
        cell: [f32; 2],
        sheets: Vec<TileSheet>,
        /// `[x, y, sheet, column, row, flags]`: cell (x, y) counts right and
        /// up from the entity; flags 1 = flip x, 2 = flip y, 4 = transpose.
        cells: Vec<[i32; 6]>,
    },
    /// An engine component with no GameX equivalent yet, kept so nothing
    /// is lost in translation: its name and values as the reader found them.
    Custom {
        name: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        values: BTreeMap<String, Value>,
    },
}

/// An image cut into equal tiles, `tile` pixels each, after `margin` and
/// with `separation` between them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileSheet {
    pub asset: String,
    pub tile: [u32; 2],
    #[serde(default, skip_serializing_if = "is_default")]
    pub margin: [u32; 2],
    #[serde(default, skip_serializing_if = "is_default")]
    pub separation: [u32; 2],
    /// Tiles (column, row) that block movement.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solid: Vec<[u32; 2]>,
}

impl Part {
    /// The asset files this part uses.
    pub fn assets(&self) -> Vec<&str> {
        match self {
            Part::Model { asset } | Part::Sprite { asset, .. } | Part::Sound { asset, .. } => vec![asset],
            Part::Tiles { sheets, .. } => sheets.iter().map(|s| s.asset.as_str()).collect(),
            _ => Vec::new(),
        }
    }
}

/// A behavior attached to a thing or entity, with values for its parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorUse {
    pub behavior: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// Things

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThingKind {
    Character,
    Item,
    Tool,
    Prop,
    Other,
}

impl ThingKind {
    /// The folder under `things/` its files live in.
    pub fn folder(self) -> &'static str {
        match self {
            ThingKind::Character => "characters",
            ThingKind::Item => "items",
            ThingKind::Tool => "tools",
            ThingKind::Prop => "props",
            ThingKind::Other => "other",
        }
    }
}

/// A reusable kind of thing: `things/<kind folder>/<id>.thing.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thing {
    pub format: u32,
    pub id: String,
    pub name: String,
    pub kind: ThingKind,
    /// For characters: "player", "enemy", "npc", "ally".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<Property>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Part>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub behaviors: Vec<BehaviorUse>,
    /// Sub-parts placed relative to the thing (a character's held tool...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Entity>,    /// Words for finding and for the engine: "no-move", "no-attack" and
    /// "no-collide" switch those off for every instance.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Behavior (blocks)

/// A block's argument: a value, or one of the owner's properties by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Arg {
    Property { property: String },
    Value(Value),
}

/// One block: `{"block": "near", "args": {"target": ..., "distance": ...}}`.
/// The block names are GameX's library (events, conditions, actions); the
/// engine and the behavior editor know what each one means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub block: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, Arg>,
}

/// "When <event> [in <state>] and <conditions>, do <actions>."
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// Only while in this state; any state when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub when: Block,
    #[serde(default, rename = "if", skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Block>,
    pub then: Vec<Block>,
}

/// A behavior made of blocks: `behavior/<id>.blocks.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Behavior {
    pub format: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Values that whoever uses this behavior can set (shown in the inspector).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Property>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_state: Option<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

// ---------------------------------------------------------------------------
// Origin: where translated elements came from

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Fidelity {
    /// Translated without loss.
    Exact,
    /// Rebuilt to act alike; worth checking against the original.
    Approximated,
    /// Found in the game but not translated yet.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    /// The game's file, relative to the source's `decompiled/` folder.
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub fidelity: Fidelity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `sources/<source>/origin.json`: element ID -> where it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginMap {
    pub format: u32,
    pub source: String,
    #[serde(default)]
    pub entries: BTreeMap<String, Origin>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_and_properties_read_naturally() {
        let p = Property { name: "chaseSpeed".into(), value: Value::Number(7.1), unit: Some("m/s".into()), description: None };
        assert_eq!(serde_json::to_value(&p).unwrap(), json!({"name": "chaseSpeed", "type": "number", "value": 7.1, "unit": "m/s"}));
        let back: Property = serde_json::from_value(json!({"name": "alive", "type": "bool", "value": true})).unwrap();
        assert_eq!(back.value, Value::Bool(true));
    }

    #[test]
    fn entities_leave_defaults_out() {
        let mut e = Entity::new("door-1", "Door");
        e.parts.push(Part::Model { asset: "models/door.glb".into() });
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"id": "door-1", "name": "Door", "parts": [{"type": "model", "asset": "models/door.glb"}]})
        );
        let back: Entity = serde_json::from_value(json!({"id": "a", "name": "A"})).unwrap();
        assert!(back.active);
        assert_eq!(back.transform, Transform::default());
    }

    #[test]
    fn block_args_are_values_or_properties() {
        let b: Block = serde_json::from_value(json!({
            "block": "chase",
            "args": {"speed": {"property": "chaseSpeed"}, "target": {"type": "thing", "value": "player"}}
        }))
        .unwrap();
        assert_eq!(b.args["speed"], Arg::Property { property: "chaseSpeed".into() });
        assert_eq!(b.args["target"], Arg::Value(Value::Thing("player".into())));
        let rule: Rule = serde_json::from_value(json!({"when": {"block": "everyFrame"}, "if": [{"block": "canSee"}], "then": []})).unwrap();
        assert_eq!(rule.conditions.len(), 1);
        assert!(serde_json::to_string(&rule).unwrap().contains("\"if\""));
    }

    #[test]
    fn parts_are_tagged_by_type() {
        let light: Part = serde_json::from_value(json!({"type": "light", "kind": "point", "color": "#ffeecc"})).unwrap();
        assert_eq!(light, Part::Light { kind: LightKind::Point, color: "#ffeecc".into(), intensity: 1.0, range: None });
        let sprite = Part::Sprite { asset: "sprites/hero.png".into(), frame: None, size: None, flip_x: true };
        assert_eq!(serde_json::to_value(&sprite).unwrap(), json!({"type": "sprite", "asset": "sprites/hero.png", "flipX": true}));
    }
}
