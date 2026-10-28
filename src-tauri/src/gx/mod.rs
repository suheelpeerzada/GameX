//! The GameX format: the common layer every game is translated into, and
//! that the workspace edits and the GameX engine plays (docs/PLAN.md).
//!
//! ```text
//! <Project>/
//!   project.json                 ProjectInfo
//!   world/<id>.scene.json        Scene
//!   things/<kind>/<id>.thing.json Thing
//!   behavior/<id>.blocks.json    Behavior (blocks); <id>.gxs scripts
//!   assets/...                   models, textures, sprites, audio...
//!   sources/<game>/              original, decompiled, origin.json
//!   history/                     undo
//! ```

pub mod combine;
pub mod commands;
pub mod export;
pub mod generic;
pub mod glb;
pub mod godot;
pub mod godot_res;
pub mod model;
pub mod project;
pub mod sample;
pub mod unity_scene;
