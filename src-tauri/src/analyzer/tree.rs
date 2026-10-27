//! A flat listing of `unpacked/` where each logical file appears once.
//!
//! An APK is unpacked several ways (raw extract, apktool, jadx), so the same
//! image can exist three times. Each copy maps to a *logical* path (its path
//! inside the game, without the unpack step's folder). Copies with the same
//! logical path are folded into one entry: the best-decoded copy is kept as
//! `path` and the others are listed in `copies`, so nothing is lost.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Path relative to `unpacked/`, with forward slashes.
    pub path: String,
    /// Path inside the game, e.g. `res/drawable/hero.png`.
    pub logical: String,
    pub size: u64,
    /// Lower-quality copies of the same file from other unpack steps.
    pub copies: Vec<String>,
}

impl FileEntry {
    pub fn name(&self) -> &str {
        self.logical.rsplit('/').next().unwrap_or(&self.logical)
    }

    /// Lowercase extension without the dot; empty when there is none.
    pub fn ext(&self) -> String {
        match self.name().rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
            _ => String::new(),
        }
    }

    /// File name without its extension (and without Android's `.9` marker).
    pub fn stem(&self) -> &str {
        let name = self.name();
        let stem = match name.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => stem,
            _ => name,
        };
        stem.strip_suffix(".9").unwrap_or(stem)
    }

    /// Folders of the logical path, outermost first.
    pub fn dirs(&self) -> Vec<&str> {
        let mut parts: Vec<&str> = self.logical.split('/').collect();
        parts.pop();
        parts
    }
}

pub struct FileTree {
    pub root: PathBuf,
    pub files: Vec<FileEntry>,
    /// How many copies were folded into other entries.
    pub folded: usize,
}

/// Unpack step folders as `(prefix, priority, keep prefix)`. Higher priority
/// means better decoded; each step has its own so copies always fold.
const LAYERS: &[(&str, u8, bool)] = &[
    ("apktool/original/", 3, false),
    ("apktool/", 5, false),
    ("jadx/sources/", 5, true),
    ("jadx/resources/", 4, false),
    ("raw/", 2, false),
    ("files/", 1, false),
];

impl FileTree {
    pub fn scan(root: &Path) -> FileTree {
        let mut found: Vec<(u8, String, String, u64)> = WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let rel = e.path().strip_prefix(root).ok()?;
                let path = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                let (priority, logical) = logical_path(&path);
                Some((priority, path, logical, size))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        let mut files: Vec<FileEntry> = Vec::new();
        let mut kept: HashMap<String, (usize, u8)> = HashMap::new();
        let mut folded = 0;
        for (priority, path, logical, size) in found {
            let key = dedupe_key(&logical);
            match kept.get(&key) {
                // Only fold copies from different unpack steps; two files
                // from the same step are genuinely different files.
                Some(&(index, p)) if p != priority => {
                    files[index].copies.push(path);
                    folded += 1;
                }
                _ => {
                    kept.insert(key, (files.len(), priority));
                    files.push(FileEntry {
                        path,
                        logical,
                        size,
                        copies: Vec::new(),
                    });
                }
            }
        }
        files.sort_by(|a, b| a.logical.cmp(&b.logical));
        FileTree {
            root: root.to_path_buf(),
            files,
            folded,
        }
    }

    pub fn has(&self, logical: &str) -> bool {
        self.files.iter().any(|f| f.logical == logical)
    }

    pub fn find(&self, logical: &str) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.logical == logical)
    }

    /// Reads up to `max` bytes of a file as (lossy) UTF-8.
    pub fn read_text(&self, entry: &FileEntry, max: usize) -> Option<String> {
        let mut buf = Vec::new();
        fs::File::open(self.root.join(&entry.path))
            .ok()?
            .take(max as u64)
            .read_to_end(&mut buf)
            .ok()?;
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    pub fn full_path(&self, entry: &FileEntry) -> PathBuf {
        self.root.join(&entry.path)
    }
}

fn logical_path(path: &str) -> (u8, String) {
    for &(prefix, priority, keep) in LAYERS {
        if let Some(rest) = path.strip_prefix(prefix) {
            let logical = if keep {
                // "jadx/sources/a/B.java" -> "sources/a/B.java"
                path[prefix.find('/').map_or(0, |i| i + 1)..].to_string()
            } else {
                rest.to_string()
            };
            return (priority, logical);
        }
    }
    (0, path.to_string())
}

/// Android build tools add API-level qualifiers (`drawable-hdpi-v4`) that
/// apktool removes, so they are ignored when matching copies.
fn dedupe_key(logical: &str) -> String {
    let mut parts: Vec<&str> = logical.split('/').collect();
    if parts.len() >= 3 && parts[0] == "res" {
        if let Some((dir, version)) = parts[1].rsplit_once("-v") {
            if !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit()) {
                parts[1] = dir;
            }
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, rel: &str, data: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }

    #[test]
    fn folds_copies_from_other_unpack_steps() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "raw/res/drawable-hdpi-v4/hero.png", "raw");
        touch(root, "apktool/res/drawable-hdpi/hero.png", "decoded");
        touch(root, "jadx/resources/res/drawable-hdpi/hero.png", "jadx");
        touch(root, "raw/classes.dex", "dex");
        touch(root, "apktool/original/META-INF/CERT.RSA", "sig");
        touch(root, "raw/META-INF/CERT.RSA", "sig");
        touch(root, "jadx/sources/com/x/Game.java", "class Game {}");

        let tree = FileTree::scan(root);
        let logical: Vec<_> = tree.files.iter().map(|f| f.logical.as_str()).collect();
        assert_eq!(
            logical,
            [
                "META-INF/CERT.RSA",
                "classes.dex",
                "res/drawable-hdpi/hero.png",
                "sources/com/x/Game.java"
            ]
        );
        let hero = tree.find("res/drawable-hdpi/hero.png").unwrap();
        assert_eq!(hero.path, "apktool/res/drawable-hdpi/hero.png");
        assert_eq!(hero.copies.len(), 2);
        assert_eq!(tree.find("META-INF/CERT.RSA").unwrap().copies, ["raw/META-INF/CERT.RSA"]);
        assert_eq!(tree.files.len() + tree.folded, 7);
    }

    #[test]
    fn plain_folders_keep_their_paths() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "files/sprites/Hero.PNG", "x");
        let tree = FileTree::scan(dir.path());
        let f = &tree.files[0];
        assert_eq!(f.logical, "sprites/Hero.PNG");
        assert_eq!(f.ext(), "png");
        assert_eq!(f.stem(), "Hero");
        assert_eq!(f.dirs(), ["sprites"]);
    }
}
