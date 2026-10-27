//! On-disk layout for imported games. See ARCHITECTURE.md, "Workspace layout".
//!
//! ```text
//! <root>/<project-id>/
//!   original/      the imported file or folder, byte-for-byte, never modified
//!   unpacked/      container output (raw extract, apktool, jadx)
//!   edits/         changed files only (later)
//!   history/       snapshots for undo (later)
//!   project.json   what was imported and how each step went
//!   analysis.json  cached analyzer result (rebuilt whenever needed)
//!   overrides.json the user's manual category changes (never rebuilt)
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::{Category, GameProject, ImportRecord};

pub const PROJECT_FILE: &str = "project.json";
pub const ANALYSIS_FILE: &str = "analysis.json";
pub const OVERRIDES_FILE: &str = "overrides.json";

pub struct Workspace {
    root: PathBuf,
}

pub struct ProjectDirs {
    pub root: PathBuf,
    pub original: PathBuf,
    pub unpacked: PathBuf,
}

impl Workspace {
    pub fn new(root: PathBuf) -> Self {
        Workspace { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn create_project(&self, id: &str) -> io::Result<ProjectDirs> {
        let root = self.root.join(id);
        if root.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("project folder {} already exists", root.display()),
            ));
        }
        for sub in ["original", "unpacked", "edits", "history"] {
            fs::create_dir_all(root.join(sub))?;
        }
        Ok(ProjectDirs {
            original: root.join("original"),
            unpacked: root.join("unpacked"),
            root,
        })
    }

    pub fn save_record(&self, record: &ImportRecord) -> io::Result<()> {
        let path = self.root.join(&record.id).join(PROJECT_FILE);
        fs::write(path, serde_json::to_string_pretty(record)?)
    }

    /// The folder of an existing project.
    pub fn project_dir(&self, id: &str) -> io::Result<PathBuf> {
        // Ids come from the UI, so refuse anything that could leave the root.
        if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid project id",
            ));
        }
        Ok(self.root.join(id))
    }

    /// Deletes a project folder and everything in it. The kept original is
    /// read-only, so files are made writable first or Windows refuses.
    pub fn delete_project(&self, id: &str) -> io::Result<()> {
        let dir = self.project_dir(id)?;
        for entry in walkdir::WalkDir::new(&dir).into_iter().filter_map(Result::ok) {
            if let Ok(meta) = entry.metadata() {
                let mut perms = meta.permissions();
                if perms.readonly() {
                    #[allow(clippy::permissions_set_readonly_false)]
                    perms.set_readonly(false);
                    let _ = fs::set_permissions(entry.path(), perms);
                }
            }
        }
        fs::remove_dir_all(dir)
    }

    pub fn load_record(&self, id: &str) -> io::Result<ImportRecord> {
        let text = fs::read_to_string(self.project_dir(id)?.join(PROJECT_FILE))?;
        serde_json::from_str(&text).map_err(io::Error::other)
    }

    /// The cached analysis, or `None` if there is none or it can't be read.
    pub fn load_analysis(&self, id: &str) -> Option<GameProject> {
        let text = fs::read_to_string(self.project_dir(id).ok()?.join(ANALYSIS_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save_analysis(&self, project: &GameProject) -> io::Result<()> {
        let path = self.project_dir(&project.id)?.join(ANALYSIS_FILE);
        fs::write(path, serde_json::to_string(project)?)
    }

    /// Manual category changes, keyed by object id.
    pub fn load_overrides(&self, id: &str) -> io::Result<BTreeMap<String, Category>> {
        match fs::read_to_string(self.project_dir(id)?.join(OVERRIDES_FILE)) {
            Ok(text) => serde_json::from_str(&text).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e),
        }
    }

    pub fn save_overrides(&self, id: &str, overrides: &BTreeMap<String, Category>) -> io::Result<()> {
        let path = self.project_dir(id)?.join(OVERRIDES_FILE);
        fs::write(path, serde_json::to_string_pretty(overrides)?)
    }

    /// Every project with a readable `project.json`, newest first.
    pub fn list(&self) -> io::Result<Vec<ImportRecord>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut records: Vec<ImportRecord> = entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|id| self.load_record(&id).ok())
            .collect();
        records.sort_by_key(|r| std::cmp::Reverse(r.imported_at));
        Ok(records)
    }
}
