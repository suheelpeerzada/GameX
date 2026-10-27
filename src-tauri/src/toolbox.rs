//! Optional tools GameX can download and keep in its own folder
//! (`<app data>/tools/<id>/`), so they work without being on PATH. Nothing
//! is downloaded unless the user asks. Tools that come with their own
//! installer (emulators) are detected and linked instead.
//!
//! ```text
//! tools/<id>/tool.json   what was installed: version, main file
//! tools/<id>/...         the tool's files
//! ```

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

static TOOLS_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Called once at startup with `<app data>/tools`.
pub fn set_tools_dir(dir: PathBuf) {
    let _ = TOOLS_DIR.set(dir);
}

pub fn tools_dir() -> Option<&'static Path> {
    TOOLS_DIR.get().map(PathBuf::as_path)
}

/// Where a tool's files come from.
#[derive(Clone, Copy)]
pub enum Source {
    /// The newest GitHub release; the asset is chosen by name.
    GitHub { repo: &'static str, asset: fn(&str) -> bool },
    /// A fixed download link (always the latest version).
    Url(&'static str),
    /// Installed with its own installer; GameX only detects it.
    External { detect: fn() -> Option<PathBuf> },
}

#[derive(Clone, Copy)]
pub struct ToolSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub used_for: &'static str,
    pub homepage: &'static str,
    pub size: &'static str,
    pub source: Source,
    /// The main file, found anywhere under the install (e.g. "bin/jadx.bat").
    pub entry: &'static str,
}

fn jadx_asset(name: &str) -> bool {
    // "jadx-1.5.6.zip", not "jadx-gui-1.5.6-with-jre-win.zip".
    name.starts_with("jadx-") && name.ends_with(".zip") && name[5..].starts_with(|c: char| c.is_ascii_digit())
}

pub const CATALOG: &[ToolSpec] = &[
    ToolSpec {
        id: "java",
        name: "Java runtime (Temurin 21)",
        description: "The Java runtime the APK tools run on. GameX uses its own copy, so other Java versions on your PC don't matter.",
        used_for: "apktool, jadx and APK signing",
        homepage: "https://adoptium.net/",
        size: "about 45 MB",
        source: Source::Url("https://api.adoptium.net/v3/binary/latest/21/ga/windows/x64/jre/hotspot/normal/eclipse"),
        entry: "bin/java.exe",
    },
    ToolSpec {
        id: "apktool",
        name: "apktool",
        description: "Decodes an APK's resources and code (smali), and rebuilds the APK after editing.",
        used_for: "importing APKs (decoding) and Build APK",
        homepage: "https://apktool.org/",
        size: "about 25 MB",
        source: Source::GitHub {
            repo: "iBotPeaches/Apktool",
            asset: |n| n.starts_with("apktool_") && n.ends_with(".jar"),
        },
        entry: "apktool.bat",
    },
    ToolSpec {
        id: "jadx",
        name: "jadx",
        description: "Decompiles an APK's code into readable Java.",
        used_for: "importing APKs (readable code)",
        homepage: "https://github.com/skylot/jadx",
        size: "about 50 MB",
        source: Source::GitHub { repo: "skylot/jadx", asset: jadx_asset },
        entry: "bin/jadx.bat",
    },
    ToolSpec {
        id: "uber-apk-signer",
        name: "uber-apk-signer",
        description: "Aligns and signs rebuilt APKs so Android will install them.",
        used_for: "Build APK",
        homepage: "https://github.com/patrickfav/uber-apk-signer",
        size: "about 5 MB",
        source: Source::GitHub {
            repo: "patrickfav/uber-apk-signer",
            asset: |n| n.starts_with("uber-apk-signer-") && n.ends_with(".jar"),
        },
        entry: "uber-apk-signer.jar",
    },
    ToolSpec {
        id: "gdre",
        name: "GDRE Tools (Godot recovery)",
        description: "Recovers Godot games: compiled scripts (.gdc), binary scenes (.scn), textures and audio. GameX will use it for Godot decoding; that support comes in a later update.",
        used_for: "Godot games (coming later)",
        homepage: "https://github.com/GDRETools/gdsdecomp",
        size: "about 45 MB",
        source: Source::GitHub { repo: "GDRETools/gdsdecomp", asset: |n| n.ends_with("windows.zip") },
        entry: "gdre_tools.exe",
    },
    ToolSpec {
        id: "platform-tools",
        name: "Android platform tools (adb)",
        description: "Google's adb, for installing and controlling games on emulators and phones.",
        used_for: "running games on an emulator or phone",
        homepage: "https://developer.android.com/tools/releases/platform-tools",
        size: "about 7 MB",
        source: Source::Url("https://dl.google.com/android/repository/platform-tools-latest-windows.zip"),
        entry: "adb.exe",
    },
    ToolSpec {
        id: "ldplayer",
        name: "LDPlayer",
        description: "An Android emulator for Windows. Install it from its website; GameX finds it automatically.",
        used_for: "running games",
        homepage: "https://www.ldplayer.net/",
        size: "installer from the website",
        source: Source::External { detect: detect_ldplayer },
        entry: "ldconsole.exe",
    },
    ToolSpec {
        id: "bluestacks",
        name: "BlueStacks",
        description: "An Android emulator for Windows. Install it from its website and turn on \"Android Debug Bridge\" in its settings; GameX finds it automatically.",
        used_for: "running games",
        homepage: "https://www.bluestacks.com/",
        size: "installer from the website",
        source: Source::External { detect: detect_bluestacks },
        entry: "HD-Player.exe",
    },
    ToolSpec {
        id: "android-emulator",
        name: "Android Emulator (Google)",
        description: "Google's official emulator, with instant save-state snapshots. It comes with Android Studio; GameX finds it automatically.",
        used_for: "running games with save states",
        homepage: "https://developer.android.com/studio",
        size: "installed with Android Studio",
        source: Source::External { detect: detect_google_emulator },
        entry: "emulator.exe",
    },
];

pub fn spec(id: &str) -> Option<&'static ToolSpec> {
    CATALOG.iter().find(|s| s.id == id)
}

fn detect_ldplayer() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = ["C:\\LDPlayer", "D:\\LDPlayer"].iter().map(PathBuf::from).collect();
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(p) = std::env::var_os(var) {
            roots.push(PathBuf::from(p).join("LDPlayer"));
        }
    }
    roots.into_iter().find_map(|root| {
        ["LDPlayer9", "LDPlayer4.0", "LDPlayer"]
            .iter()
            .map(|v| root.join(v).join("ldconsole.exe"))
            .chain(std::iter::once(root.join("ldconsole.exe")))
            .find(|p| p.is_file())
    })
}

/// BlueStacks 5 (`BlueStacks_nxt`): the player in Program Files.
pub fn detect_bluestacks() -> Option<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|p| PathBuf::from(p).join("BlueStacks_nxt").join("HD-Player.exe"))
        .find(|p| p.is_file())
}

fn detect_google_emulator() -> Option<PathBuf> {
    let sdk = std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Android").join("Sdk")))?;
    Some(sdk.join("emulator").join("emulator.exe")).filter(|p| p.is_file())
}

#[derive(Debug, Serialize, Deserialize)]
struct Installed {
    version: String,
    /// The main file, relative to the tool's folder.
    entry: String,
}

/// The main file of a tool GameX installed, if it's there.
pub fn installed(id: &str) -> Option<PathBuf> {
    installed_in(tools_dir()?, id).map(|(path, _)| path)
}

fn installed_in(root: &Path, id: &str) -> Option<(PathBuf, String)> {
    let dir = root.join(id);
    let info: Installed = serde_json::from_str(&fs::read_to_string(dir.join("tool.json")).ok()?).ok()?;
    let path = dir.join(&info.entry);
    path.is_file().then_some((path, info.version))
}

/// The Java home of GameX's own Java runtime, if installed.
pub fn java_home() -> Option<PathBuf> {
    // .../tools/java/<jdk-folder>/bin/java.exe -> .../<jdk-folder>
    installed("java")?.parent()?.parent().map(Path::to_path_buf)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub used_for: String,
    pub homepage: String,
    pub size: String,
    /// GameX downloads it (true) or it has its own installer (false).
    pub managed: bool,
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    /// "GameX" (installed by GameX), "detected" (own installer), "settings"
    /// (a path set in Settings) or "system" (on PATH / JAVA_HOME).
    pub origin: Option<String>,
}

pub fn info(spec: &ToolSpec) -> ToolInfo {
    let (found, version, origin) = match spec.source {
        Source::External { detect } => (detect(), None, "detected"),
        _ => match tools_dir().and_then(|d| installed_in(d, spec.id)) {
            Some((path, version)) => (Some(path), Some(version), "GameX"),
            None => (None, None, "GameX"),
        },
    };
    ToolInfo {
        id: spec.id.into(),
        name: spec.name.into(),
        description: spec.description.into(),
        used_for: spec.used_for.into(),
        homepage: spec.homepage.into(),
        size: spec.size.into(),
        managed: !matches!(spec.source, Source::External { .. }),
        installed: found.is_some(),
        version,
        origin: found.as_ref().map(|_| origin.to_string()),
        path: found.map(|p| p.display().to_string()),
    }
}

pub fn catalog() -> Vec<ToolInfo> {
    CATALOG.iter().map(info).collect()
}

// ---------------------------------------------------------------------------
// Downloading and installing

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("GameX")
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(1800))
        .build()
        .map_err(|e| e.to_string())
}

/// The download link and version to install.
fn locate(spec: &ToolSpec) -> Result<(String, String, String), String> {
    match spec.source {
        Source::GitHub { repo, asset } => {
            let release: serde_json::Value = client()?
                .get(format!("https://api.github.com/repos/{repo}/releases/latest"))
                .send()
                .and_then(|r| r.error_for_status())
                .map_err(|e| format!("Couldn't check {}'s latest release: {e}", spec.name))?
                .json()
                .map_err(|e| e.to_string())?;
            let version = release["tag_name"].as_str().unwrap_or("latest").to_string();
            let found = release["assets"].as_array().into_iter().flatten().find(|a| a["name"].as_str().is_some_and(asset));
            let found = found.ok_or_else(|| format!("{}'s latest release ({version}) has no Windows download.", spec.name))?;
            Ok((
                found["browser_download_url"].as_str().unwrap_or_default().to_string(),
                found["name"].as_str().unwrap_or_default().to_string(),
                version,
            ))
        }
        Source::Url(url) => Ok((url.to_string(), format!("{}.zip", spec.id), "latest".to_string())),
        Source::External { .. } => Err(format!("{} has its own installer. Get it from {}.", spec.name, spec.homepage)),
    }
}

/// Downloads and installs a tool, reporting (bytes received, total bytes).
pub fn install(spec: &ToolSpec, mut progress: impl FnMut(u64, Option<u64>)) -> Result<ToolInfo, String> {
    let root = tools_dir().ok_or("GameX's tools folder isn't set.")?;
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let (url, file_name, version) = locate(spec)?;

    let download = root.join(format!("{}.download", spec.id));
    let result = (|| {
        let mut response = client()?
            .get(&url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("Couldn't download {}: {e}", spec.name))?;
        let total = response.content_length();
        let mut out = File::create(&download).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 256 << 10];
        let mut received = 0u64;
        loop {
            let n = response.read(&mut buf).map_err(|e| format!("The download of {} was interrupted: {e}", spec.name))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            received += n as u64;
            progress(received, total);
        }
        drop(out);
        install_archive(root, spec, &download, &file_name, &version)
    })();
    let _ = fs::remove_file(&download);
    result.map(|_| info(spec))
}

/// Installs a downloaded file into `root/<id>`. The old version is replaced
/// only once the new one is complete, so a failure leaves nothing half-done.
pub fn install_archive(root: &Path, spec: &ToolSpec, archive: &Path, file_name: &str, version: &str) -> Result<PathBuf, String> {
    let io_err = |e: io::Error| e.to_string();
    let staging = root.join(format!("{}.new", spec.id));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(io_err)?;
    }
    fs::create_dir_all(&staging).map_err(io_err)?;
    let result = (|| {
        if file_name.to_ascii_lowercase().ends_with(".jar") {
            let jar_name = if spec.id == "apktool" { "apktool.jar".to_string() } else { format!("{}.jar", spec.id) };
            fs::copy(archive, staging.join(&jar_name)).map_err(io_err)?;
            if spec.id == "apktool" {
                // A plain launcher (the official apktool.bat pauses when run by another program).
                fs::write(staging.join("apktool.bat"), "@echo off\r\njava -jar \"%~dp0apktool.jar\" %*\r\n").map_err(io_err)?;
            }
        } else {
            crate::import::extract_zip(archive, &staging).map_err(|e| format!("Couldn't unpack the download: {e}"))?;
        }
        let entry = find_entry(&staging, spec.entry)
            .ok_or_else(|| format!("The download doesn't contain {} as expected.", spec.entry))?;
        let rel = entry.strip_prefix(&staging).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
        let meta = Installed { version: version.to_string(), entry: rel };
        fs::write(staging.join("tool.json"), serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?).map_err(io_err)?;
        let target = root.join(spec.id);
        if target.exists() {
            fs::remove_dir_all(&target).map_err(io_err)?;
        }
        fs::rename(&staging, &target).map_err(io_err)?;
        Ok(target.join(meta.entry))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// The shallowest file whose path ends with `entry` ("bin/jadx.bat").
fn find_entry(dir: &Path, entry: &str) -> Option<PathBuf> {
    let wanted: Vec<&str> = entry.split('/').collect();
    walkdir::WalkDir::new(dir)
        .sort_by_key(|e| e.depth())
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .find(|e| {
            let parts: Vec<String> = e.path().components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
            parts.len() >= wanted.len()
                && parts[parts.len() - wanted.len()..].iter().zip(&wanted).all(|(a, b)| *a == b.to_lowercase())
        })
        .map(|e| e.into_path())
}

pub fn uninstall(id: &str) -> Result<(), String> {
    let spec = spec(id).ok_or("Unknown tool.")?;
    if let Source::External { .. } = spec.source {
        return Err(format!("{} was installed with its own installer. Remove it from Windows settings.", spec.name));
    }
    let dir = tools_dir().ok_or("GameX's tools folder isn't set.")?.join(id);
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Couldn't remove {}: {e}", spec.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn zip_with(path: &Path, entries: &[&str]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for name in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn picks_the_right_release_assets() {
        assert!(jadx_asset("jadx-1.5.6.zip"));
        assert!(!jadx_asset("jadx-gui-1.5.6-with-jre-win.zip"));
        let apktool = match spec("apktool").unwrap().source {
            Source::GitHub { asset, .. } => asset,
            _ => unreachable!(),
        };
        assert!(apktool("apktool_3.0.3.jar"));
        assert!(!apktool("apktool_3.0.3.jar.sha256"));
        let gdre = match spec("gdre").unwrap().source {
            Source::GitHub { asset, .. } => asset,
            _ => unreachable!(),
        };
        assert!(gdre("GDRE_tools-v2.7.4-windows.zip"));
        assert!(!gdre("GDRE_tools-v2.7.4-linux.zip"));
    }

    #[test]
    fn installs_zips_and_jars_and_replaces_old_versions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tools");
        fs::create_dir_all(&root).unwrap();

        // A zip whose main file is nested ("jadx-1.5.6/bin/jadx.bat").
        let zip = dir.path().join("jadx.zip");
        zip_with(&zip, &["jadx-1.5.6/bin/jadx.bat", "jadx-1.5.6/lib/jadx.jar"]);
        let jadx = spec("jadx").unwrap();
        let entry = install_archive(&root, jadx, &zip, "jadx-1.5.6.zip", "v1.5.6").unwrap();
        assert!(entry.ends_with("jadx-1.5.6/bin/jadx.bat"));
        assert_eq!(installed_in(&root, "jadx").unwrap().1, "v1.5.6");

        // A newer version replaces it.
        zip_with(&zip, &["jadx-1.6.0/bin/jadx.bat"]);
        install_archive(&root, jadx, &zip, "jadx-1.6.0.zip", "v1.6.0").unwrap();
        assert_eq!(installed_in(&root, "jadx").unwrap().1, "v1.6.0");
        assert!(!root.join("jadx/jadx-1.5.6").exists());

        // A jar gets a launcher for apktool.
        let jar = dir.path().join("apktool_3.0.3.jar");
        fs::write(&jar, "PK").unwrap();
        let entry = install_archive(&root, spec("apktool").unwrap(), &jar, "apktool_3.0.3.jar", "v3.0.3").unwrap();
        assert!(entry.ends_with("apktool.bat"));
        assert!(fs::read_to_string(&entry).unwrap().contains("apktool.jar"));

        // A download missing the expected file fails and leaves no trace.
        zip_with(&zip, &["readme.txt"]);
        assert!(install_archive(&root, spec("gdre").unwrap(), &zip, "gdre.zip", "v1").is_err());
        assert!(!root.join("gdre").exists() && !root.join("gdre.new").exists());
        // ...and doesn't touch what was installed before.
        assert_eq!(installed_in(&root, "jadx").unwrap().1, "v1.6.0");
    }
}
