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
pub const ANALYZER_VERSION: u32 = 5;

/// Object kinds that hold other files, worth searching for embedded media.
const CONTAINER_KINDS: &[&str] = &[
    "Godot data pack", "Unity asset file", "streamed asset data", "asset bundle", "archive", "data", "file",
];
/// Total bytes the media search may read during one analysis.
const MEDIA_SCAN_BUDGET: u64 = 1 << 30;

/// Searches unknown files and containers for embedded images and sounds.
/// An unplaced file that turns out to hold media becomes a media container
/// (in Assets, or Audio if it's mostly sound) instead of staying Unsorted.
/// Returns (files with media, images, sounds).
fn find_embedded_media(root: &Path, objects: &mut [GameObject]) -> (usize, usize, usize) {
    let mut budget = MEDIA_SCAN_BUDGET;
    let (mut files, mut images, mut sounds) = (0, 0, 0);
    for obj in objects.iter_mut() {
        if !CONTAINER_KINDS.contains(&obj.kind.as_str()) {
            continue;
        }
        let Some(first) = obj.source_files.first() else { continue };
        let path = root.join(first);
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.len() < 64 || meta.len() > crate::media::MAX_SCAN_BYTES || meta.len() > budget {
            continue;
        }
        budget -= meta.len();
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let found = crate::media::scan(&bytes);
        if found.is_empty() {
            continue;
        }
        let image_count = found.iter().filter(|f| f.kind == "image").count();
        let sound_count = found.len() - image_count;
        files += 1;
        images += image_count;
        sounds += sound_count;
        let summary = crate::media::summary(&found);
        obj.properties.push(crate::model::Property {
            label: "Embedded media".into(),
            value: summary.clone(),
        });
        if obj.auto_category == Category::Unsorted {
            let category = if sound_count > image_count { Category::Audio } else { Category::Assets };
            obj.auto_category = category;
            obj.category = category;
            obj.kind = "media container".into();
            obj.confidence = 0.6;
            obj.reason = format!("it contains {summary}, found by their file signatures");
        }
    }
    (files, images, sounds)
}

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

    let mut objects = integration.analyze(&tree, &ctx);
    let (media_files, media_images, media_sounds) = find_embedded_media(&tree.root, &mut objects);
    let capabilities = integration.capabilities(&tree, &ctx, &objects);

    let mut notes = Vec::new();
    if media_files > 0 {
        notes.push(format!(
            "The media search found {media_images} images and {media_sounds} sounds inside {media_files} files; open one to see them."
        ));
    }
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
    let mut roles: BTreeMap<&str, usize> = BTreeMap::new();
    for o in objects.iter().filter(|o| o.auto_category == Category::Characters) {
        if let Some(role) = &o.role {
            *roles.entry(role.as_str()).or_default() += 1;
        }
    }
    let role_note = if roles.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = roles.iter().map(|(r, n)| format!("{n} {}", r.to_lowercase())).collect();
        format!(" By role: {}.", parts.join(", "))
    };
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
                "Found {} likely characters from names, fields and inheritance ({}).{role_note} Check them and move any that are wrong.",
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
