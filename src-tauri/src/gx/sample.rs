//! A small hand-made project in the GameX format, made from shapes so it
//! needs no asset files: a 3D courtyard and a 2D platform scene, with a
//! player, guards that patrol and chase, coins to pick up, crates and a
//! lamp. It's the test bed for the workspace and the engine before any
//! game is translated (milestone 1's gate).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use super::model::*;
use super::project::{self, write_json};

fn number(v: f64, unit: Option<&str>, name: &str) -> Property {
    Property { name: name.into(), value: Value::Number(v), unit: unit.map(Into::into), description: None }
}

fn shape(shape: ShapeKind, size: [f32; 3], color: &str) -> Part {
    Part::Shape { shape, size, color: color.into() }
}

fn collider(shape: ShapeKind, size: [f32; 3], body: Body) -> Part {
    Part::Collider { shape, size, body, trigger: false }
}

fn arg_prop(name: &str) -> Arg {
    Arg::Property { property: name.into() }
}

fn args(list: &[(&str, Arg)]) -> BTreeMap<String, Arg> {
    list.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn block(name: &str, list: &[(&str, Arg)]) -> Block {
    Block { block: name.into(), args: args(list) }
}

fn player_target() -> (&'static str, Arg) {
    ("target", Arg::Value(Value::Thing("player".into())))
}

fn uses(behavior: &str) -> Vec<BehaviorUse> {
    vec![BehaviorUse { behavior: behavior.into(), params: BTreeMap::new() }]
}

fn thing(id: &str, name: &str, kind: ThingKind, role: Option<&str>, properties: Vec<Property>, parts: Vec<Part>) -> Thing {
    Thing { tags: Vec::new(),
        format: FORMAT,
        id: id.into(),
        name: name.into(),
        kind,
        role: role.map(Into::into),
        description: None,
        properties,
        parts,
        behaviors: Vec::new(),
        children: Vec::new(),
    }
}

fn instance(id: &str, name: &str, of: &str, position: [f32; 3]) -> Entity {
    Entity { thing: Some(of.into()), transform: Transform::at(position), ..Entity::new(id, name) }
}

/// Rotation about Y by `degrees`, as a quaternion.
fn yaw(degrees: f32) -> [f32; 4] {
    let h = degrees.to_radians() / 2.0;
    [0.0, h.sin(), 0.0, h.cos()]
}

/// Rotation about X by `degrees` (negative looks down).
fn pitch(degrees: f32) -> [f32; 4] {
    let h = degrees.to_radians() / 2.0;
    [h.sin(), 0.0, 0.0, h.cos()]
}

fn things() -> Vec<Thing> {
    let mut player = thing(
        "player",
        "Player",
        ThingKind::Character,
        Some("player"),
        vec![number(4.5, Some("m/s"), "speed"), number(1.2, Some("m"), "jumpHeight"), number(100.0, Some("hp"), "health")],
        vec![shape(ShapeKind::Capsule, [0.8, 1.8, 0.8], "#3d7bd9"), collider(ShapeKind::Capsule, [0.8, 1.8, 0.8], Body::Kinematic)],
    );
    player.description = Some("The character you control.".into());

    let mut guard = thing(
        "guard",
        "Guard",
        ThingKind::Character,
        Some("enemy"),
        vec![
            number(50.0, Some("hp"), "health"),
            number(1.5, Some("m/s"), "walkSpeed"),
            number(3.5, Some("m/s"), "chaseSpeed"),
            number(8.0, Some("m"), "sightRange"),
            number(10.0, Some("hp"), "damage"),
        ],
        vec![shape(ShapeKind::Capsule, [0.8, 1.8, 0.8], "#c8483c"), collider(ShapeKind::Capsule, [0.8, 1.8, 0.8], Body::Kinematic)],
    );
    guard.behaviors = uses("patrol-and-chase");
    guard.description = Some("Walks around until it sees the player, then chases.".into());

    let mut coin = thing(
        "coin",
        "Coin",
        ThingKind::Item,
        None,
        vec![Property { name: "value".into(), value: Value::Integer(1), unit: Some("points".into()), description: None }],
        vec![
            shape(ShapeKind::Cylinder, [0.6, 0.1, 0.6], "#e8b830"),
            Part::Collider { shape: ShapeKind::Cylinder, size: [0.6, 0.1, 0.6], body: Body::Static, trigger: true },
        ],
    );
    coin.behaviors = uses("pickup");

    let crate_ = thing(
        "crate",
        "Crate",
        ThingKind::Prop,
        None,
        vec![number(20.0, Some("kg"), "mass")],
        vec![shape(ShapeKind::Box, [1.0, 1.0, 1.0], "#a0703c"), collider(ShapeKind::Box, [1.0, 1.0, 1.0], Body::Dynamic)],
    );

    let mut lamp = thing(
        "lamp",
        "Lamp",
        ThingKind::Prop,
        None,
        Vec::new(),
        vec![shape(ShapeKind::Cylinder, [0.15, 3.0, 0.15], "#5a5f66"), collider(ShapeKind::Cylinder, [0.15, 3.0, 0.15], Body::Static)],
    );
    let mut bulb = Entity::new("bulb", "Bulb");
    bulb.transform.position = [0.0, 1.6, 0.0];
    bulb.parts = vec![
        shape(ShapeKind::Sphere, [0.35, 0.35, 0.35], "#fff1c8"),
        Part::Light { kind: LightKind::Point, color: "#ffd9a0".into(), intensity: 1.5, range: Some(8.0) },
    ];
    lamp.children.push(bulb);

    vec![player, guard, coin, crate_, lamp]
}

fn behaviors() -> Vec<Behavior> {
    let near_player = |distance: Arg| block("canSee", &[player_target(), ("distance", distance)]);
    let patrol_and_chase = Behavior {
        format: FORMAT,
        id: "patrol-and-chase".into(),
        name: "Patrol and chase".into(),
        description: Some("Walks a loop around its start point; chases the player on sight and gives up when the player gets away.".into()),
        params: vec![number(12.0, Some("m"), "giveUpDistance")],
        states: vec!["patrol".into(), "chase".into()],
        initial_state: Some("patrol".into()),
        rules: vec![
            Rule {
                state: Some("patrol".into()),
                when: block("everyFrame", &[]),
                conditions: Vec::new(),
                then: vec![block("patrol", &[("speed", arg_prop("walkSpeed")), ("radius", Arg::Value(Value::Number(4.0)))])],
            },
            Rule {
                state: Some("patrol".into()),
                when: block("everyFrame", &[]),
                conditions: vec![near_player(arg_prop("sightRange"))],
                then: vec![block("setState", &[("state", Arg::Value(Value::Text("chase".into())))])],
            },
            Rule {
                state: Some("chase".into()),
                when: block("everyFrame", &[]),
                conditions: Vec::new(),
                then: vec![block("chase", &[player_target(), ("speed", arg_prop("chaseSpeed"))])],
            },
            Rule {
                state: Some("chase".into()),
                when: block("everyFrame", &[]),
                conditions: vec![block("farFrom", &[player_target(), ("distance", arg_prop("giveUpDistance"))])],
                then: vec![block("setState", &[("state", Arg::Value(Value::Text("patrol".into())))])],
            },
            Rule {
                state: None,
                when: block("touch", &[player_target()]),
                conditions: Vec::new(),
                then: vec![block("damage", &[player_target(), ("amount", arg_prop("damage"))])],
            },
        ],
    };
    let pickup = Behavior {
        format: FORMAT,
        id: "pickup".into(),
        name: "Pick up".into(),
        description: Some("Adds its value to the score when the player touches it, then disappears.".into()),
        params: Vec::new(),
        states: Vec::new(),
        initial_state: None,
        rules: vec![Rule {
            state: None,
            when: block("touch", &[player_target()]),
            conditions: Vec::new(),
            then: vec![block("addScore", &[("amount", arg_prop("value"))]), block("destroy", &[])],
        }],
    };
    vec![patrol_and_chase, pickup]
}

fn courtyard() -> Scene {
    let mut ground = Entity::new("ground", "Ground");
    ground.parts = vec![shape(ShapeKind::Plane, [30.0, 0.0, 30.0], "#6f8f4e"), collider(ShapeKind::Box, [30.0, 0.2, 30.0], Body::Static)];

    let wall = |id: &str, name: &str, position: [f32; 3], size: [f32; 3]| {
        let mut w = Entity::new(id, name);
        w.transform.position = position;
        w.parts = vec![shape(ShapeKind::Box, size, "#b8b0a2"), collider(ShapeKind::Box, size, Body::Static)];
        w
    };
    let mut walls = Entity::new("walls", "Walls");
    walls.children = vec![
        wall("wall-north", "North wall", [0.0, 1.0, -15.0], [30.0, 2.0, 0.5]),
        wall("wall-south", "South wall", [0.0, 1.0, 15.0], [30.0, 2.0, 0.5]),
        wall("wall-west", "West wall", [-15.0, 1.0, 0.0], [0.5, 2.0, 30.0]),
        wall("wall-east", "East wall", [15.0, 1.0, 0.0], [0.5, 2.0, 30.0]),
    ];

    let mut sun = Entity::new("sun", "Sun");
    sun.transform.position = [10.0, 20.0, 10.0];
    sun.transform.rotation = [-0.3536, 0.3536, 0.1464, 0.8536];
    sun.parts = vec![Part::Light { kind: LightKind::Directional, color: "#fff4e0".into(), intensity: 1.2, range: None }];

    let mut camera = Entity::new("camera", "Camera");
    camera.transform.position = [0.0, 9.0, 16.0];
    camera.transform.rotation = pitch(-30.0);
    camera.parts = vec![Part::Camera { fov: Some(60.0), ortho_size: None }];

    let mut guard2 = instance("guard-2", "Guard 2", "guard", [6.0, 0.9, -6.0]);
    guard2.overrides.insert("walkSpeed".into(), Value::Number(2.0));
    let mut crate2 = instance("crate-2", "Crate 2", "crate", [-3.0, 0.5, 2.0]);
    crate2.transform.rotation = yaw(30.0);

    let mut coins = Entity::new("coins", "Coins");
    coins.children = vec![
        instance("coin-1", "Coin 1", "coin", [-4.0, 0.6, 0.0]),
        instance("coin-2", "Coin 2", "coin", [0.0, 0.6, -4.0]),
        instance("coin-3", "Coin 3", "coin", [4.0, 0.6, 0.0]),
    ];

    Scene {
        format: FORMAT,
        id: "courtyard".into(),
        name: "Courtyard".into(),
        dimensions: Dimensions::ThreeD,
        environment: Environment::default(),
        entities: vec![
            ground,
            walls,
            sun,
            camera,
            instance("player-1", "Player", "player", [0.0, 0.9, 6.0]),
            instance("guard-1", "Guard 1", "guard", [-6.0, 0.9, -6.0]),
            guard2,
            coins,
            instance("crate-1", "Crate 1", "crate", [-2.0, 0.5, 3.0]),
            crate2,
            instance("lamp-1", "Lamp", "lamp", [5.0, 1.5, 3.0]),
        ],
    }
}

fn platforms() -> Scene {
    let block2d = |id: &str, name: &str, position: [f32; 3], size: [f32; 2]| {
        let mut e = Entity::new(id, name);
        e.transform.position = position;
        let size = [size[0], size[1], 1.0];
        e.parts = vec![shape(ShapeKind::Box, size, "#7a8aa6"), collider(ShapeKind::Box, size, Body::Static)];
        e
    };
    let mut camera = Entity::new("camera", "Camera");
    camera.transform.position = [0.0, 0.0, 10.0];
    camera.parts = vec![Part::Camera { fov: None, ortho_size: Some(6.0) }];
    let mut light = Entity::new("light", "Light");
    light.parts = vec![Part::Light { kind: LightKind::Directional, color: "#ffffff".into(), intensity: 1.0, range: None }];
    Scene {
        format: FORMAT,
        id: "platforms".into(),
        name: "Platforms".into(),
        dimensions: Dimensions::TwoD,
        environment: Environment { background: "#20263a".into(), ..Environment::default() },
        entities: vec![
            camera,
            light,
            block2d("floor", "Floor", [0.0, -4.5, 0.0], [20.0, 1.0]),
            block2d("step-1", "Low platform", [-4.0, -2.0, 0.0], [3.0, 0.5]),
            block2d("step-2", "High platform", [2.0, 0.5, 0.0], [3.0, 0.5]),
            instance("player-1", "Player", "player", [-7.0, -3.1, 0.0]),
            instance("guard-1", "Guard", "guard", [6.0, -3.1, 0.0]),
            instance("coin-1", "Coin 1", "coin", [-4.0, -1.2, 0.0]),
            instance("coin-2", "Coin 2", "coin", [2.0, 1.3, 0.0]),
        ],
    }
}

/// Creates the sample project under `root` and returns its folder.
pub fn create(root: &Path) -> io::Result<PathBuf> {
    let dir = project::create(root, "GameX Sample")?;
    for t in things() {
        write_json(&dir.join("things").join(t.kind.folder()).join(format!("{}.thing.json", t.id)), &t)?;
    }
    for b in behaviors() {
        write_json(&dir.join("behavior").join(format!("{}.blocks.json", b.id)), &b)?;
    }
    for s in [courtyard(), platforms()] {
        write_json(&dir.join("world").join(format!("{}.scene.json", s.id)), &s)?;
    }
    let mut info = project::open(&dir)?.info;
    info.settings.start_scene = Some("courtyard".into());
    write_json(&dir.join(project::PROJECT_FILE), &info)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_is_complete_and_correct() {
        let root = tempfile::tempdir().unwrap();
        let dir = create(root.path()).unwrap();
        let index = project::open(&dir).unwrap();
        assert!(index.problems.is_empty(), "{:?}", index.problems);
        assert_eq!(index.scenes.len(), 2);
        assert_eq!(index.things.len(), 5);
        assert_eq!(index.behaviors.len(), 2);
        assert_eq!(index.info.settings.start_scene.as_deref(), Some("courtyard"));
        // Nothing to undo: the sample is the starting point.
        assert!(index.undo.is_none());

        let yard = project::load_scene(&dir, "courtyard").unwrap();
        assert_eq!(yard.entities.iter().flat_map(Entity::walk).count(), 18);
        assert_eq!(project::load_scene(&dir, "platforms").unwrap().dimensions, Dimensions::TwoD);
        assert_eq!(project::load_thing(&dir, "guard").unwrap().role.as_deref(), Some("enemy"));
        let chase = project::load_behavior(&dir, "patrol-and-chase").unwrap();
        assert_eq!(chase.rules.len(), 5);

        // Readable on disk: pretty-printed, IDs as file names.
        let text = std::fs::read_to_string(dir.join("things/characters/guard.thing.json")).unwrap();
        assert!(text.contains("\n  \"name\": \"Guard\""));
        assert!(text.contains("\"unit\": \"m/s\""));
    }
}
