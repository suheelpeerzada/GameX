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

/// What `project.json` holds after import. The analyzer (next milestone)
/// adds the `GameProject` alongside it.
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
