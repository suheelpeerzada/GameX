//! User edits. `unpacked/` is never changed: an edited file is written to
//! `edits/<same path>`, and every change is logged in `history/` with a
//! snapshot of what it replaced, so any change can be undone.
//!
//! ```text
//! edits/apktool/res/drawable/hero.png   the current edited version
//! history/log.json                      undo stack, newest last
//! history/<n>.bin                       the edited version a change replaced
//! ```

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Only these unpack outputs are rebuilt from, so only they can be edited.
/// jadx's Java is a read-only view: decompiled Java can't be rebuilt.
const EDITABLE_ROOTS: &[&str] = &["apktool/", "files/"];

const MAX_REPLACEMENT: u64 = 200 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogEntry {
    path: String,
    /// Snapshot of the previous edited version; `None` means it was the original.
    snapshot: Option<String>,
    summary: String,
    at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditState {
    /// Paths (relative to `unpacked/`) that currently differ from the original.
    pub edited: Vec<String>,
    /// What "Undo" would undo, if anything.
    pub undo: Option<String>,
}

pub struct EditStore {
    root: PathBuf,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.to_string())
}

/// True if `rel` is a plain relative path (no `..`, no drive or root).
pub fn is_plain_relative(rel: &str) -> bool {
    !rel.is_empty() && Path::new(rel).components().all(|c| matches!(c, Component::Normal(_)))
}

pub fn is_editable(rel: &str) -> bool {
    is_plain_relative(rel) && EDITABLE_ROOTS.iter().any(|r| rel.starts_with(r))
}

impl EditStore {
    /// `project_root` is the project folder holding `unpacked/`.
    pub fn new(project_root: &Path) -> Self {
        EditStore {
            root: project_root.to_path_buf(),
        }
    }

    fn log_path(&self) -> PathBuf {
        self.root.join("history").join("log.json")
    }

    fn load_log(&self) -> io::Result<Vec<LogEntry>> {
        match fs::read_to_string(self.log_path()) {
            Ok(text) => serde_json::from_str(&text).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    fn save_log(&self, log: &[LogEntry]) -> io::Result<()> {
        fs::create_dir_all(self.root.join("history"))?;
        fs::write(self.log_path(), serde_json::to_string_pretty(log)?)
    }

    fn edit_path(&self, rel: &str) -> PathBuf {
        self.root.join("edits").join(rel)
    }

    /// The version of a file to show or build from: the edit if there is one.
    pub fn current(&self, rel: &str) -> io::Result<PathBuf> {
        if !is_plain_relative(rel) {
            return Err(invalid("invalid file path"));
        }
        let edited = self.edit_path(rel);
        Ok(if edited.is_file() {
            edited
        } else {
            self.root.join("unpacked").join(rel)
        })
    }

    fn check_editable(&self, rel: &str) -> io::Result<()> {
        if !is_editable(rel) {
            return Err(invalid(
                "This copy can't be edited. Edit the apktool copy (smali or resources) instead.",
            ));
        }
        if !self.root.join("unpacked").join(rel).is_file() {
            return Err(invalid("That file isn't part of this game."));
        }
        Ok(())
    }

    /// Logs the change about to happen to `rel`, keeping what it replaces.
    fn record(&self, rel: &str, summary: String) -> io::Result<()> {
        let mut log = self.load_log()?;
        let current = self.edit_path(rel);
        let snapshot = if current.is_file() {
            let name = format!("{}.bin", unique_id());
            fs::create_dir_all(self.root.join("history"))?;
            fs::copy(&current, self.root.join("history").join(&name))?;
            Some(name)
        } else {
            None
        };
        log.push(LogEntry {
            path: rel.to_string(),
            snapshot,
            summary,
            at: now(),
        });
        self.save_log(&log)
    }

    fn write_edit(&self, rel: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.edit_path(rel);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, bytes)
    }

    pub fn save_text(&self, rel: &str, text: &str) -> io::Result<()> {
        self.check_editable(rel)?;
        self.record(rel, format!("Edit {}", file_name(rel)))?;
        self.write_edit(rel, text.as_bytes())
    }

    /// Replaces a file (an image, a sound...) with one the user picked. The
    /// new file must be the same format, since the game expects that format.
    pub fn replace(&self, rel: &str, source: &Path) -> io::Result<()> {
        self.check_editable(rel)?;
        let want = extension(rel);
        let got = extension(&source.to_string_lossy());
        if want != got {
            return Err(invalid(&format!(
                "Pick a .{want} file: the game expects this file in that format."
            )));
        }
        if fs::metadata(source)?.len() > MAX_REPLACEMENT {
            return Err(invalid("That file is too large."));
        }
        let bytes = fs::read(source)?;
        if !matches_magic(&want, &bytes) {
            return Err(invalid(&format!("That file isn't a valid .{want} file.")));
        }
        self.record(rel, format!("Replace {}", file_name(rel)))?;
        self.write_edit(rel, &bytes)
    }

    /// Goes back to the original. Undoable like any other change.
    pub fn revert(&self, rel: &str) -> io::Result<()> {
        self.check_editable(rel)?;
        if !self.edit_path(rel).is_file() {
            return Ok(());
        }
        self.record(rel, format!("Restore original {}", file_name(rel)))?;
        fs::remove_file(self.edit_path(rel))
    }

    /// Undoes the newest change. Returns false when there is nothing to undo.
    pub fn undo(&self) -> io::Result<bool> {
        let mut log = self.load_log()?;
        let Some(entry) = log.pop() else {
            return Ok(false);
        };
        match &entry.snapshot {
            Some(name) => {
                let snap = self.root.join("history").join(name);
                self.write_edit(&entry.path, &fs::read(&snap)?)?;
                let _ = fs::remove_file(snap);
            }
            None => match fs::remove_file(self.edit_path(&entry.path)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            },
        }
        self.save_log(&log)?;
        Ok(true)
    }

    pub fn state(&self) -> io::Result<EditState> {
        let edits = self.root.join("edits");
        let mut edited: Vec<String> = walkdir::WalkDir::new(&edits)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let rel = e.path().strip_prefix(&edits).ok()?;
                Some(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                )
            })
            .collect();
        edited.sort();
        Ok(EditState {
            edited,
            undo: self.load_log()?.pop().map(|e| e.summary),
        })
    }

    /// Copies every edited file under `prefix` (e.g. "apktool/") into `dest`,
    /// keeping paths relative to the prefix.
    pub fn overlay(&self, prefix: &str, dest: &Path) -> io::Result<usize> {
        let mut count = 0;
        for rel in self.state()?.edited {
            if let Some(inner) = rel.strip_prefix(prefix) {
                let target = dest.join(inner);
                if let Some(dir) = target.parent() {
                    fs::create_dir_all(dir)?;
                }
                fs::copy(self.edit_path(&rel), target)?;
                count += 1;
            }
        }
        Ok(count)
    }
}

fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

fn extension(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Cheap check that a replacement really is the format its name says.
fn matches_magic(ext: &str, bytes: &[u8]) -> bool {
    match ext {
        "png" => bytes.starts_with(b"\x89PNG"),
        "jpg" | "jpeg" => bytes.starts_with(b"\xFF\xD8"),
        "gif" => bytes.starts_with(b"GIF8"),
        "webp" => bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        "ogg" => bytes.starts_with(b"OggS"),
        "wav" => bytes.starts_with(b"RIFF"),
        _ => true,
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn unique_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, EditStore) {
        let dir = tempfile::tempdir().unwrap();
        let unpacked = dir.path().join("unpacked");
        fs::create_dir_all(unpacked.join("apktool/smali/a")).unwrap();
        fs::create_dir_all(unpacked.join("jadx/sources/a")).unwrap();
        fs::write(unpacked.join("apktool/smali/a/Hero.smali"), "speed 1").unwrap();
        fs::write(unpacked.join("jadx/sources/a/Hero.java"), "int speed = 1;").unwrap();
        let store = EditStore::new(dir.path());
        (dir, store)
    }

    fn read(store: &EditStore, rel: &str) -> String {
        fs::read_to_string(store.current(rel).unwrap()).unwrap()
    }

    #[test]
    fn edits_are_separate_and_undoable() {
        let (dir, store) = setup();
        let rel = "apktool/smali/a/Hero.smali";
        store.save_text(rel, "speed 2").unwrap();
        store.save_text(rel, "speed 3").unwrap();
        assert_eq!(read(&store, rel), "speed 3");
        // The unpacked original is untouched.
        assert_eq!(fs::read_to_string(dir.path().join("unpacked").join(rel)).unwrap(), "speed 1");
        assert_eq!(store.state().unwrap().edited, [rel]);

        store.revert(rel).unwrap();
        assert_eq!(read(&store, rel), "speed 1");
        assert!(store.state().unwrap().edited.is_empty());

        assert!(store.undo().unwrap()); // undo the revert
        assert_eq!(read(&store, rel), "speed 3");
        assert!(store.undo().unwrap());
        assert_eq!(read(&store, rel), "speed 2");
        assert_eq!(store.state().unwrap().undo.as_deref(), Some("Edit Hero.smali"));
        assert!(store.undo().unwrap());
        assert_eq!(read(&store, rel), "speed 1");
        assert!(!store.undo().unwrap());
    }

    #[test]
    fn refuses_read_only_and_outside_paths() {
        let (dir, store) = setup();
        assert!(store.save_text("jadx/sources/a/Hero.java", "x").is_err());
        assert!(store.save_text("apktool/../../evil.txt", "x").is_err());
        assert!(store.save_text("apktool/smali/a/New.smali", "x").is_err());

        let fake = dir.path().join("fake.png");
        fs::write(&fake, "not a png").unwrap();
        fs::write(dir.path().join("unpacked/apktool/icon.png"), b"\x89PNG").unwrap();
        assert!(store.replace("apktool/icon.png", &fake).is_err());
        let wav = dir.path().join("sound.wav");
        fs::write(&wav, "RIFF").unwrap();
        assert!(store.replace("apktool/icon.png", &wav).is_err());
    }

    #[test]
    fn overlay_copies_edits_for_a_build() {
        let (dir, store) = setup();
        store.save_text("apktool/smali/a/Hero.smali", "speed 9").unwrap();
        let work = dir.path().join("work");
        assert_eq!(store.overlay("apktool/", &work).unwrap(), 1);
        assert_eq!(fs::read_to_string(work.join("smali/a/Hero.smali")).unwrap(), "speed 9");
    }
}
