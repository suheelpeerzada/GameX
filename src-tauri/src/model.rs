//! Types shared between the backend and the UI. Field names are camelCase on
//! the wire so they match `src/lib/types.ts`.

use serde::{Deserialize, Serialize};

/// How a game arrived. Detected before (and separately from) the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerKind {
    Apk,
    Zip,
    Folder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Done,
    Skipped,
    Failed,
}

/// One unpacking step (raw extract, apktool, jadx...) and how it went.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnpackStep {
    pub id: String,
    pub label: String,
    pub status: StepStatus,
    /// Plain-language explanation, always set for skipped or failed steps.
    pub note: Option<String>,
    /// Output folder relative to the project workspace.
    pub output: Option<String>,
}

/// What `project.json` holds after import. The analyzer's `GameProject` is
/// stored separately, in `analysis.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecord {
    pub id: String,
    pub name: String,
    pub container: ContainerKind,
    pub source_path: String,
    /// Seconds since the Unix epoch.
    pub imported_at: u64,
    /// SHA-256 of the original file; `None` for folders.
    pub original_sha256: Option<String>,
    pub original_bytes: u64,
    pub unpacked_files: u64,
    pub steps: Vec<UnpackStep>,
    pub workspace_path: String,
}

/// Progress pushed to the UI while an import runs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub stage: String,
    pub message: String,
}

/// Where the analyzer files an object. `Unsorted` is for anything it couldn't
/// place, so nothing is ever dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Characters,
    Core,
    Map,
    Items,
    Assets,
    Audio,
    Ui,
    Code,
    Unsorted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    pub label: String,
    pub value: String,
}

/// One thing the user can browse: a file, a group of variants of one file,
/// a frame set, or a code class with all of its source files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameObject {
    /// Stable across re-analysis, so manual category changes can be kept.
    pub id: String,
    pub category: Category,
    /// What the analyzer chose. Differs from `category` when the user moved it.
    pub auto_category: Category,
    pub display_name: String,
    /// Plain-language kind, e.g. "image", "sound", "class", "layout".
    pub kind: String,
    /// Paths relative to `unpacked/`, best copy first.
    pub source_files: Vec<String>,
    pub properties: Vec<Property>,
    /// 0.0..1.0: how sure the analyzer is about `auto_category`.
    pub confidence: f32,
    /// Why the analyzer chose `auto_category`.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub id: String,
    pub name: String,
    pub confidence: f32,
    /// The markers that led to this engine, in plain language.
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityStatus {
    Supported,
    Partial,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    /// "code", "characters", "movement", "assets", "visualEditing", "rebuilding".
    pub feature: String,
    pub label: String,
    pub status: CapabilityStatus,
    pub note: String,
}

/// The analyzer's result, cached in `analysis.json` next to `project.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameProject {
    pub analyzer_version: u32,
    pub id: String,
    pub name: String,
    pub container: ContainerKind,
    pub engine: EngineInfo,
    pub workspace_path: String,
    /// Seconds since the Unix epoch.
    pub analyzed_at: u64,
    pub objects: Vec<GameObject>,
    pub capabilities: Vec<Capability>,
    /// Plain-language remarks about the analysis as a whole.
    pub notes: Vec<String>,
}
