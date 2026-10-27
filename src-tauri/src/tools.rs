//! External tools (decompilers, apktool, the APK signer). They run as child
//! processes; GameX never links them.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Apktool,
    Jadx,
    /// uber-apk-signer: zipalign + apksigner in one jar, run with Java.
    Signer,
}

impl Tool {
    pub fn id(self) -> &'static str {
        match self {
            Tool::Apktool => "apktool",
            Tool::Jadx => "jadx",
            Tool::Signer => "uber-apk-signer",
        }
    }

    fn configured(self, settings: &Settings) -> Option<&str> {
        let path = match self {
            Tool::Apktool => settings.apktool_path.as_deref(),
            Tool::Jadx => settings.jadx_path.as_deref(),
            Tool::Signer => settings.signer_path.as_deref(),
        };
        path.map(str::trim).filter(|p| !p.is_empty())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub id: String,
    pub found: bool,
    pub path: Option<String>,
    /// Where the path came from: "settings" or "PATH".
    pub source: Option<String>,
    pub note: String,
}

/// Finds a tool: the path from settings if one is set, otherwise PATH.
pub fn resolve(tool: Tool, settings: &Settings) -> Option<PathBuf> {
    match tool.configured(settings) {
        Some(p) => Some(PathBuf::from(p)).filter(|p| p.is_file()),
        None if tool == Tool::Signer => find_jar_on_path(tool.id()),
        None => find_on_path(tool.id()),
    }
}

/// `java` or `keytool` from JAVA_HOME, falling back to PATH.
pub fn java_tool(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    env::var_os("JAVA_HOME")
        .map(|home| PathBuf::from(home).join("bin").join(&exe))
        .filter(|p| p.is_file())
        .or_else(|| find_on_path(name))
}

pub fn status(tool: Tool, settings: &Settings) -> ToolStatus {
    let configured = tool.configured(settings);
    let found = resolve(tool, settings);
    let note = match (&found, configured) {
        (Some(_), _) => "Ready.".to_string(),
        (None, Some(p)) => format!("No file at the configured path {p}."),
        (None, None) => format!(
            "{} was not found on PATH. Install it or set its path in Settings.",
            tool.id()
        ),
    };
    ToolStatus {
        id: tool.id().to_string(),
        found: found.is_some(),
        path: found.as_ref().map(|p| p.display().to_string()),
        source: found.is_some().then(|| {
            if configured.is_some() {
                "settings"
            } else {
                "PATH"
            }
            .to_string()
        }),
        note,
    }
}

/// apktool and jadx ship as `.bat` wrappers on Windows and shell scripts
/// elsewhere, so check each PATHEXT extension as well as the bare name.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.BAT;.CMD".into())
            .split(';')
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    env::split_paths(&env::var_os("PATH")?).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{name}{ext}")))
            .find(|candidate| candidate.is_file())
    })
}

/// A jar named `<prefix>*.jar` in any PATH folder (jars can't run from PATH
/// themselves, so their folder is what goes on PATH).
fn find_jar_on_path(prefix: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?).find_map(|dir| {
        std::fs::read_dir(&dir).ok()?.filter_map(Result::ok).find_map(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            (name.starts_with(prefix) && name.ends_with(".jar")).then(|| e.path())
        })
    })
}

/// Runs a tool and returns stderr (or stdout) on failure, trimmed for display.
pub fn run(program: &Path, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("Could not start {}: {e}", program.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let text = if output.stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    let text = text.trim();
    let start = text.char_indices().rev().nth(600).map_or(0, |(i, _)| i);
    let tail = &text[start..];
    Err(format!(
        "{} exited with {}: {tail}",
        program.display(),
        output.status
    ))
}
