//! On-disk layout for imported games. See ARCHITECTURE.md, "Workspace layout".
//!
//! ```text
//! <root>/<project-id>/
//!   original/      the imported file or folder, byte-for-byte, never modified
//!   unpacked/      container output (raw extract, apktool, jadx)
//!   edits/         changed files only (later)
//!   history/       snapshots for undo (later)
//!   project.json
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::ImportRecord;

pub const PROJECT_FILE: &str = "project.json";

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

    pub fn load_record(&self, id: &str) -> io::Result<ImportRecord> {
        // Ids come from the UI, so refuse anything that could leave the root.
        if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid project id",
            ));
        }
        let text = fs::read_to_string(self.root.join(id).join(PROJECT_FILE))?;
        serde_json::from_str(&text).map_err(io::Error::other)
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
