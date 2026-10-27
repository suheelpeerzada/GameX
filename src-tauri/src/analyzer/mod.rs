//! Game Analyzer: turns `unpacked/` into a `GameProject`. See ARCHITECTURE.md.
//!
//! Each engine is an `EngineIntegration`. Every integration scores the file
//! tree, the best one analyzes it, and the generic integration catches any
//! game no other integration recognizes. Integrations only add knowledge:
//! files they don't understand fall through to the generic classifier, so
//! every file ends up in some object.

mod android;
mod classify;
mod generic;
mod godot;
mod java;
pub mod tree;
mod unity;

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{
    Capability, CapabilityStatus, Category, EngineInfo, GameObject, GameProject, ImportRecord,
    StepStatus,
};
use crate::workspace::Workspace;
use tree::FileTree;

/// Bump when analysis output changes, so cached results are redone.
pub const ANALYZER_VERSION: u32 = 2;

pub struct Detection {
    /// 0.0 = definitely not this engine, 1.0 = certain.
    pub score: f32,
    pub evidence: Vec<String>,
}

/// What an integration may need besides the files.
pub struct Context<'a> {
    pub record: &'a ImportRecord,
}

impl Context<'_> {
    pub fn step(&self, id: &str) -> Option<&crate::model::UnpackStep> {
        self.record.steps.iter().find(|s| s.id == id)
    }

    pub fn step_done(&self, id: &str) -> bool {
        self.step(id).is_some_and(|s| s.status == StepStatus::Done)
    }
}

pub trait EngineIntegration {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn detect(&self, tree: &FileTree) -> Detection;
    fn analyze(&self, tree: &FileTree, ctx: &Context) -> Vec<GameObject>;
    fn capabilities(&self, tree: &FileTree, ctx: &Context, objects: &[GameObject]) -> Vec<Capability>;
}

/// Most specific first; on equal scores the earlier one wins.
pub fn integrations() -> Vec<Box<dyn EngineIntegration>> {
    vec![
        Box::new(godot::Godot),
        Box::new(unity::Unity),
        Box::new(android::NativeAndroid),
        Box::new(generic::Generic),
    ]
}

/// Analyzes an imported project. `project_root` holds `unpacked/`.
pub fn analyze(record: &ImportRecord, project_root: &Path) -> GameProject {
    let tree = FileTree::scan(&project_root.join("unpacked"));
    let ctx = Context { record };

    let mut best: Option<(Box<dyn EngineIntegration>, Detection)> = None;
    for integration in integrations() {
        let detection = integration.detect(&tree);
        if best.as_ref().is_none_or(|(_, b)| detection.score > b.score) {
            best = Some((integration, detection));
        }
    }
    let (integration, detection) = best.expect("the generic integration always exists");

    let objects = integration.analyze(&tree, &ctx);
    let capabilities = integration.capabilities(&tree, &ctx, &objects);

    let mut notes = Vec::new();
    if tree.folded > 0 {
        notes.push(format!(
            "{} duplicate files from different unpack steps were merged into their objects.",
            tree.folded
        ));
    }
    let unsorted = objects.iter().filter(|o| o.category == Category::Unsorted).count();
    if unsorted > 0 {
        notes.push(format!(
            "{unsorted} objects couldn't be placed automatically and are in Unsorted."
        ));
    }

    GameProject {
        analyzer_version: ANALYZER_VERSION,
        id: record.id.clone(),
        name: record.name.clone(),
        container: record.container,
        engine: EngineInfo {
            id: integration.id().into(),
            name: integration.name().into(),
            confidence: detection.score,
            evidence: detection.evidence,
        },
        workspace_path: record.workspace_path.clone(),
        analyzed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        objects,
        capabilities,
        notes,
    }
}

/// Returns the project's analysis with the user's overrides applied, reusing
/// the cached result unless `refresh` is set or the analyzer has changed.
pub fn load_or_analyze(ws: &Workspace, id: &str, refresh: bool) -> io::Result<GameProject> {
    let cached = if refresh { None } else { ws.load_analysis(id) };
    let mut project = match cached {
        Some(p) if p.analyzer_version == ANALYZER_VERSION => p,
        _ => {
            let record = ws.load_record(id)?;
            let project = analyze(&record, &ws.project_dir(id)?);
            ws.save_analysis(&project)?;
            project
        }
    };
    apply_overrides(&mut project, &ws.load_overrides(id)?);
    Ok(project)
}

/// Applies the user's manual category changes (object id -> category).
/// Overrides for objects that no longer exist are ignored, not deleted, so
/// they come back if a later analysis produces the object again.
pub fn apply_overrides(project: &mut GameProject, overrides: &BTreeMap<String, Category>) {
    for obj in &mut project.objects {
        obj.category = overrides.get(&obj.id).copied().unwrap_or(obj.auto_category);
    }
}

// ---------------------------------------------------------------------------
// Capability helpers shared by integrations

pub(crate) fn cap(feature: &str, label: &str, status: CapabilityStatus, note: impl Into<String>) -> Capability {
    Capability {
        feature: feature.into(),
        label: label.into(),
        status,
        note: note.into(),
    }
}

fn count_kind(objects: &[GameObject], kinds: &[&str]) -> usize {
    objects.iter().filter(|o| kinds.contains(&o.kind.as_str())).count()
}

fn list_names(names: &[&str], max: usize) -> String {
    let shown = names.iter().take(max).copied().collect::<Vec<_>>().join(", ");
    if names.len() > max {
        format!("{shown} and {} more", names.len() - max)
    } else {
        shown
    }
}

pub(crate) fn characters_capability(objects: &[GameObject]) -> Capability {
    let found: Vec<&str> = objects
        .iter()
        .filter(|o| o.auto_category == Category::Characters)
        .map(|o| o.display_name.as_str())
        .collect();
    if found.is_empty() {
        cap(
            "characters",
            "Characters",
            CapabilityStatus::Unknown,
            "No characters were recognized automatically. They may be in Code, Assets or Unsorted; you can move them by hand.",
        )
    } else {
        cap(
            "characters",
            "Characters",
            CapabilityStatus::Partial,
            format!(
                "Found {} likely characters from names and fields ({}). Check them and move any that are wrong.",
                found.len(),
                list_names(&found, 4)
            ),
        )
    }
}

/// Code classes tagged with "Movement hints" by the code reader.
pub(crate) fn movement_capability(objects: &[GameObject]) -> Capability {
    let found: Vec<&str> = objects
        .iter()
        .filter(|o| o.properties.iter().any(|p| p.label == "Movement hints"))
        .map(|o| o.display_name.as_str())
        .collect();
    if found.is_empty() {
        cap(
            "movement",
            "Movement",
            CapabilityStatus::Unknown,
            "No movement code was located. It may be obfuscated or inside data GameX can't read yet.",
        )
    } else {
        cap(
            "movement",
            "Movement",
            CapabilityStatus::Partial,
            format!(
                "Movement-related code (speed, velocity, jumping...) found in {}. To change it, edit those classes' smali files.",
                list_names(&found, 5)
            ),
        )
    }
}

pub(crate) fn assets_capability(objects: &[GameObject]) -> Capability {
    let images = count_kind(objects, &["image", "image set", "app icon"]);
    let audio = count_kind(objects, &["sound", "music"]);
    if images + audio == 0 {
        cap(
            "assets",
            "Assets",
            CapabilityStatus::Unknown,
            "No loose images or audio were found. Assets may be packed inside the game's data files.",
        )
    } else {
        cap(
            "assets",
            "Assets",
            CapabilityStatus::Supported,
            format!(
                "{images} images and {audio} audio files in standard formats. Open one and use Replace to swap it for your own."
            ),
        )
    }
}

pub(crate) fn visual_editing_from_maps(objects: &[GameObject], otherwise: Capability) -> Capability {
    let maps = count_kind(objects, &["map data"]);
    if maps > 0 {
        cap(
            "visualEditing",
            "Visual editing",
            CapabilityStatus::Partial,
            format!("{maps} level or tile-map files were found and can be read. A visual editor for them isn't built yet."),
        )
    } else {
        otherwise
    }
}

#[cfg(test)]
mod tests;
