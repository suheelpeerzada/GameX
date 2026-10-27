use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// User settings, stored as `settings.json` in the app data directory.
/// Empty tool paths mean "look on PATH".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub apktool_path: Option<String>,
    pub jadx_path: Option<String>,
    /// uber-apk-signer's `.jar`.
    pub signer_path: Option<String>,
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)
    }
}
