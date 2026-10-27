//! Import: detect the container, keep the original untouched, unpack a
//! working copy. Engine detection and analysis happen afterwards, on
//! `unpacked/`, and never touch `original/`.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use thiserror::Error;
use walkdir::WalkDir;

use crate::model::{ContainerKind, ImportRecord, StepStatus, UnpackStep};
use crate::settings::Settings;
use crate::tools::{self, Tool};
use crate::workspace::{ProjectDirs, Workspace};

/// Refuse archives that expand past this, to stop zip bombs filling the disk.
const MAX_UNPACKED_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("{0} does not exist")]
    NotFound(String),
    #[error("GameX can import an APK, a ZIP or a folder. {0} is none of these.")]
    Unsupported(String),
    #[error("The archive is damaged or not a ZIP: {0}")]
    BadArchive(String),
    #[error("The archive expands to more than {} GB, so it was not unpacked.", MAX_UNPACKED_BYTES >> 30)]
    TooLarge,
    #[error("{0}")]
    Io(#[from] io::Error),
}

impl From<zip::result::ZipError> for ImportError {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(e) => ImportError::Io(e),
            other => ImportError::BadArchive(other.to_string()),
        }
    }
}

/// Decides how to unpack `path`. Extensions are trusted first; an unknown
/// extension falls back to sniffing the ZIP signature, and a ZIP with an
/// `AndroidManifest.xml` at its root is an APK.
pub fn detect_container(path: &Path) -> Result<ContainerKind, ImportError> {
    if !path.exists() {
        return Err(ImportError::NotFound(path.display().to_string()));
    }
    if path.is_dir() {
        return Ok(ContainerKind::Folder);
    }
    let ext = path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("apk") => return Ok(ContainerKind::Apk),
        Some("zip") => return Ok(ContainerKind::Zip),
        _ => {}
    }
    let mut magic = [0u8; 4];
    let is_zip = File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && magic == *b"PK\x03\x04";
    if !is_zip {
        return Err(ImportError::Unsupported(path.display().to_string()));
    }
    let archive = zip::ZipArchive::new(File::open(path)?)?;
    let is_apk = archive.file_names().any(|n| n == "AndroidManifest.xml");
    Ok(if is_apk {
        ContainerKind::Apk
    } else {
        ContainerKind::Zip
    })
}

/// Imports `source` into a new project under `workspace`. `progress` gets
/// (stage, message) pairs for the UI. On failure the half-made project
/// folder is removed so the project list only shows complete imports.
pub fn import_game(
    source: &Path,
    workspace: &Workspace,
    settings: &Settings,
    mut progress: impl FnMut(&str, &str),
) -> Result<ImportRecord, ImportError> {
    let container = detect_container(source)?;
    let name = display_name(source);
    let id = format!(
        "{}-{}",
        slug(&name),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    progress(
        "detect",
        &format!("Importing {name} as {}", container_label(container)),
    );

    let dirs = workspace.create_project(&id)?;
    match import_into(
        source,
        container,
        &name,
        &id,
        &dirs,
        settings,
        &mut progress,
    ) {
        Ok(record) => {
            workspace.save_record(&record)?;
            progress("done", "Import finished.");
            Ok(record)
        }
        Err(e) => {
            make_writable(&dirs.root);
            let _ = fs::remove_dir_all(&dirs.root);
            Err(e)
        }
    }
}

fn import_into(
    source: &Path,
    container: ContainerKind,
    name: &str,
    id: &str,
    dirs: &ProjectDirs,
    settings: &Settings,
    progress: &mut impl FnMut(&str, &str),
) -> Result<ImportRecord, ImportError> {
    progress("original", "Keeping an untouched copy of the original");
    let file_name = source.file_name().unwrap_or_else(|| OsStr::new("game"));
    let original = dirs.original.join(file_name);
    let (original_sha256, original_bytes) = if container == ContainerKind::Folder {
        (None, copy_dir(source, &original)?)
    } else {
        let (sha, bytes) = copy_hashed(source, &original)?;
        (Some(sha), bytes)
    };
    set_read_only(&original);

    let steps = match container {
        ContainerKind::Zip => {
            progress("extract", "Extracting the ZIP");
            extract_zip(&original, &dirs.unpacked.join("files"))?;
            vec![done("extract", "Extract ZIP", "unpacked/files")]
        }
        ContainerKind::Folder => {
            progress("copy", "Copying the folder into the workspace");
            copy_dir(&original, &dirs.unpacked.join("files"))?;
            make_writable(&dirs.unpacked);
            vec![done("copy", "Copy folder", "unpacked/files")]
        }
        ContainerKind::Apk => unpack_apk(&original, &dirs.unpacked, settings, progress)?,
    };

    Ok(ImportRecord {
        id: id.to_string(),
        name: name.to_string(),
        container,
        source_path: source.display().to_string(),
        imported_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        original_sha256,
        original_bytes,
        unpacked_files: count_files(&dirs.unpacked),
        steps,
        workspace_path: dirs.root.display().to_string(),
    })
}

/// An APK is a ZIP, so the raw extract always works and is enough to spot the
/// engine (`lib/*/libunity.so`, `*.pck`...). apktool (decoded resources and
/// smali) and jadx (readable Java) add to it when installed; a missing or
/// failing tool is recorded as a step, not an import failure.
fn unpack_apk(
    apk: &Path,
    unpacked: &Path,
    settings: &Settings,
    progress: &mut impl FnMut(&str, &str),
) -> Result<Vec<UnpackStep>, ImportError> {
    progress("raw", "Extracting the APK's files");
    extract_zip(apk, &unpacked.join("raw"))?;
    let mut steps = vec![done("raw", "Extract APK files", "unpacked/raw")];

    let apk_arg = apk.as_os_str();
    let out = unpacked.join("apktool");
    steps.push(match tools::resolve(Tool::Apktool, settings) {
        None => skipped(
            "apktool",
            "Decode resources (apktool)",
            tools::status(Tool::Apktool, settings).note,
        ),
        Some(apktool) => {
            progress("apktool", "Decoding resources and smali with apktool");
            let args = [
                OsStr::new("d"),
                OsStr::new("-f"),
                OsStr::new("-o"),
                out.as_os_str(),
                apk_arg,
            ];
            match tools::run(&apktool, &args) {
                Ok(()) => done("apktool", "Decode resources (apktool)", "unpacked/apktool"),
                Err(e) => failed("apktool", "Decode resources (apktool)", e),
            }
        }
    });

    let out = unpacked.join("jadx");
    steps.push(match tools::resolve(Tool::Jadx, settings) {
        None => skipped(
            "jadx",
            "Decompile Java (jadx)",
            tools::status(Tool::Jadx, settings).note,
        ),
        Some(jadx) => {
            progress(
                "jadx",
                "Decompiling Java with jadx (this can take a few minutes)",
            );
            let args = [OsStr::new("-d"), out.as_os_str(), apk_arg];
            match tools::run(&jadx, &args) {
                Ok(()) => done("jadx", "Decompile Java (jadx)", "unpacked/jadx"),
                // jadx exits non-zero when a few methods fail but still writes
                // the rest, which is normal for obfuscated games.
                Err(_) if count_files(&out) > 0 => UnpackStep {
                    note: Some(
                        "jadx finished with errors; some classes may be missing or incomplete."
                            .into(),
                    ),
                    ..done("jadx", "Decompile Java (jadx)", "unpacked/jadx")
                },
                Err(e) => failed("jadx", "Decompile Java (jadx)", e),
            }
        }
    });
    Ok(steps)
}

/// Extracts every entry under `dest`, refusing paths that would escape it.
pub fn extract_zip(archive_path: &Path, dest: &Path) -> Result<(), ImportError> {
    let mut archive = zip::ZipArchive::new(File::open(archive_path)?)?;
    fs::create_dir_all(dest)?;
    let mut budget = MAX_UNPACKED_BYTES;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            continue; // absolute or `..` path: never written
        };
        let target = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&target)?;
        // Count real bytes rather than trusting the header's declared size.
        let written = io::copy(&mut (&mut entry).take(budget + 1), &mut out)?;
        if written > budget {
            return Err(ImportError::TooLarge);
        }
        budget -= written;
    }
    Ok(())
}

/// Copies a file and returns its SHA-256 and size, hashed in the same pass.
fn copy_hashed(from: &Path, to: &Path) -> io::Result<(String, u64)> {
    let mut input = File::open(from)?;
    let mut output = File::create(to)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        output.write_all(&buf[..n])?;
        total += n as u64;
    }
    output.sync_all()?;
    let hex = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((hex, total))
}

/// Copies a folder tree and returns the bytes copied. Symlinks are skipped so
/// an import can never pull in files from outside the chosen folder.
pub(crate) fn copy_dir(from: &Path, to: &Path) -> io::Result<u64> {
    let mut total = 0;
    for entry in WalkDir::new(from).follow_links(false) {
        let entry = entry.map_err(io::Error::other)?;
        let rel = entry.path().strip_prefix(from).map_err(io::Error::other)?;
        let target = to.join(rel);
        let kind = entry.file_type();
        if kind.is_dir() {
            fs::create_dir_all(&target)?;
        } else if kind.is_file() {
            total += fs::copy(entry.path(), &target)?;
        }
    }
    Ok(total)
}

fn count_files(dir: &Path) -> u64 {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .count() as u64
}

fn set_read_only(path: &Path) {
    for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file() {
            if let Ok(meta) = entry.metadata() {
                let mut perms = meta.permissions();
                perms.set_readonly(true);
                let _ = fs::set_permissions(entry.path(), perms);
            }
        }
    }
}

/// Clears read-only flags so a tree can be removed or edited. Used on the
/// working copy (copied from a read-only original) and on failed imports.
fn make_writable(path: &Path) {
    for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
        if let Ok(meta) = entry.metadata() {
            let mut perms = meta.permissions();
            if perms.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                let _ = fs::set_permissions(entry.path(), perms);
            }
        }
    }
}

fn display_name(source: &Path) -> String {
    let stem = if source.is_dir() {
        source.file_name()
    } else {
        source.file_stem()
    };
    stem.and_then(OsStr::to_str).unwrap_or("game").to_string()
}

fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "game".into()
    } else {
        s.chars().take(40).collect()
    }
}

fn container_label(kind: ContainerKind) -> &'static str {
    match kind {
        ContainerKind::Apk => "an APK",
        ContainerKind::Zip => "a ZIP",
        ContainerKind::Folder => "a folder",
    }
}

fn done(id: &str, label: &str, output: &str) -> UnpackStep {
    UnpackStep {
        id: id.into(),
        label: label.into(),
        status: StepStatus::Done,
        note: None,
        output: Some(output.into()),
    }
}

fn skipped(id: &str, label: &str, note: String) -> UnpackStep {
    UnpackStep {
        id: id.into(),
        label: label.into(),
        status: StepStatus::Skipped,
        note: Some(note),
        output: None,
    }
}

fn failed(id: &str, label: &str, note: String) -> UnpackStep {
    UnpackStep {
        id: id.into(),
        label: label.into(),
        status: StepStatus::Failed,
        note: Some(note),
        output: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use zip::write::SimpleFileOptions;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    fn no_tools() -> Settings {
        // Point both tools at files that don't exist so PATH is never used.
        Settings {
            apktool_path: Some("/nonexistent/apktool".into()),
            jadx_path: Some("/nonexistent/jadx".into()),
            signer_path: None,
        }
    }

    #[test]
    fn detects_containers() {
        let dir = tempfile::tempdir().unwrap();
        let apk = dir.path().join("game.bin");
        write_zip(
            &apk,
            &[("AndroidManifest.xml", b"x"), ("classes.dex", b"x")],
        );
        let zip = dir.path().join("data.bin");
        write_zip(&zip, &[("readme.txt", b"x")]);
        let junk = dir.path().join("notes.txt");
        fs::write(&junk, "hello").unwrap();

        assert_eq!(detect_container(&apk).unwrap(), ContainerKind::Apk);
        assert_eq!(detect_container(&zip).unwrap(), ContainerKind::Zip);
        assert_eq!(detect_container(dir.path()).unwrap(), ContainerKind::Folder);
        assert!(matches!(
            detect_container(&junk),
            Err(ImportError::Unsupported(_))
        ));
    }

    #[test]
    fn apk_import_keeps_original_and_records_missing_tools() {
        let dir = tempfile::tempdir().unwrap();
        let apk = dir.path().join("Draky.apk");
        write_zip(
            &apk,
            &[
                ("AndroidManifest.xml", b"m"),
                ("lib/arm64-v8a/libgame.so", b"so"),
            ],
        );
        let ws = Workspace::new(dir.path().join("ws"));

        let record = import_game(&apk, &ws, &no_tools(), |_, _| {}).unwrap();

        let root = PathBuf::from(&record.workspace_path);
        assert_eq!(
            fs::read(root.join("original/Draky.apk")).unwrap(),
            fs::read(&apk).unwrap()
        );
        assert!(fs::metadata(root.join("original/Draky.apk"))
            .unwrap()
            .permissions()
            .readonly());
        assert!(root.join("unpacked/raw/lib/arm64-v8a/libgame.so").is_file());
        assert_eq!(record.name, "Draky");
        assert_eq!(record.unpacked_files, 2);
        let statuses: Vec<_> = record
            .steps
            .iter()
            .map(|s| (s.id.as_str(), s.status))
            .collect();
        assert_eq!(
            statuses,
            [
                ("raw", StepStatus::Done),
                ("apktool", StepStatus::Skipped),
                ("jadx", StepStatus::Skipped)
            ]
        );
        assert_eq!(ws.list().unwrap().len(), 1);
        assert_eq!(ws.load_record(&record.id).unwrap().id, record.id);
    }

    #[test]
    fn folder_import_copies_tree() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("My Game");
        fs::create_dir_all(game.join("assets/sprites")).unwrap();
        fs::write(game.join("assets/sprites/hero.png"), b"png").unwrap();
        fs::write(game.join("main.lua"), b"print()").unwrap();
        let ws = Workspace::new(dir.path().join("ws"));

        let record = import_game(&game, &ws, &no_tools(), |_, _| {}).unwrap();

        assert!(record.id.starts_with("my-game-"));
        assert_eq!(record.original_sha256, None);
        let root = PathBuf::from(&record.workspace_path);
        assert!(root.join("original/My Game/main.lua").is_file());
        assert!(root
            .join("unpacked/files/assets/sprites/hero.png")
            .is_file());
        assert!(!fs::metadata(root.join("unpacked/files/main.lua"))
            .unwrap()
            .permissions()
            .readonly());
    }

    #[test]
    fn zip_slip_entries_are_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("evil.zip");
        write_zip(&zip, &[("../escaped.txt", b"x"), ("ok/inside.txt", b"x")]);
        let dest = dir.path().join("out");

        extract_zip(&zip, &dest).unwrap();

        assert!(dest.join("ok/inside.txt").is_file());
        assert!(!dir.path().join("escaped.txt").exists());
    }

    #[test]
    fn failed_import_leaves_no_project() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("broken.zip");
        fs::write(&bad, b"PK\x03\x04 not really a zip").unwrap();
        let ws = Workspace::new(dir.path().join("ws"));

        assert!(import_game(&bad, &ws, &no_tools(), |_, _| {}).is_err());
        assert!(ws.list().unwrap().is_empty());
        assert_eq!(fs::read_dir(ws.root()).unwrap().count(), 0);
    }

    #[test]
    fn slugs_are_safe_folder_names() {
        assert_eq!(slug("Draky: Dragon Run!"), "draky-dragon-run");
        assert_eq!(slug("???"), "game");
    }
}
