//! Running games on an emulator or phone: list targets, start an emulator,
//! install an APK, launch it, watch whether it's still running, and read
//! Android's crash log when it isn't. The game runs in its own (virtual)
//! device, so a crash never takes GameX down.
//!
//! Targets: LDPlayer instances (driven with `ldconsole`, reached with
//! LDPlayer's own adb) and any device another adb can see (phones, Google's
//! emulator).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// "ld:<index>" or "adb:<serial>".
    pub id: String,
    pub name: String,
    /// "ldplayer" or "device".
    pub kind: String,
    pub running: bool,
    /// For LDPlayer: whether ADB debugging is on (needed to install and watch games).
    pub adb_enabled: Option<bool>,
    pub detail: String,
}

/// How to reach a target with adb.
struct Adb {
    exe: PathBuf,
    serial: String,
}

struct LdPlayer {
    console: PathBuf,
    dir: PathBuf,
}

fn ldplayer() -> Option<LdPlayer> {
    let console = crate::toolbox::spec("ldplayer").and_then(|s| match s.source {
        crate::toolbox::Source::External { detect } => detect(),
        _ => None,
    })?;
    let dir = console.parent()?.to_path_buf();
    Some(LdPlayer { console, dir })
}

/// adb for phones and other emulators: GameX's platform tools, then PATH.
fn general_adb() -> Option<PathBuf> {
    crate::toolbox::installed("platform-tools").or_else(|| crate::tools::find_on_path("adb"))
}

/// Runs a program and returns its output (stdout and stderr together).
fn output(program: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("Couldn't start {}: {e}", program.display()))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(text.trim().to_string())
}

impl Adb {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let mut all = vec!["-s", self.serial.as_str()];
        all.extend_from_slice(args);
        output(&self.exe, &all)
    }

    fn shell(&self, command: &str) -> Result<String, String> {
        self.run(&["shell", command])
    }

    fn online(&self) -> bool {
        self.run(&["get-state"]).is_ok_and(|s| s.trim() == "device")
    }
}

// ---------------------------------------------------------------------------
// LDPlayer settings

fn ld_config(ld: &LdPlayer, index: u32) -> PathBuf {
    ld.dir.join("vms").join("config").join(format!("leidian{index}.config"))
}

const ADB_KEY: &str = "\"basicSettings.adbDebug\"";

fn ld_adb_enabled(ld: &LdPlayer, index: u32) -> Option<bool> {
    let text = std::fs::read_to_string(ld_config(ld, index)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let setting = &value["basicSettings.adbDebug"];
    Some(setting.as_i64().is_some_and(|v| v != 0) || setting.as_bool() == Some(true))
}

/// Sets `"basicSettings.adbDebug": 1` in an instance's config text, keeping
/// everything else as written.
pub fn with_adb_enabled(config: &str) -> Result<String, String> {
    if let Some(start) = config.find(ADB_KEY) {
        let after = start + ADB_KEY.len();
        let colon = after + config[after..].find(':').ok_or("Unexpected LDPlayer config format.")?;
        let value_start = colon + 1;
        let value_end = value_start
            + config[value_start..]
                .find([',', '}', '\n'])
                .ok_or("Unexpected LDPlayer config format.")?;
        return Ok(format!("{} 1{}", &config[..value_start], &config[value_end..]));
    }
    // Not present yet: add it as the object's first key.
    let brace = config.find('{').ok_or("Unexpected LDPlayer config format.")?;
    Ok(format!("{}\n    {ADB_KEY}: 1,{}", &config[..=brace], &config[brace + 1..]))
}

/// Turns on ADB debugging for an LDPlayer instance (stopping it first, as
/// LDPlayer only reads its settings at start).
pub fn ld_enable_adb(index: u32) -> Result<(), String> {
    let ld = ldplayer().ok_or("LDPlayer wasn't found.")?;
    let idx = index.to_string();
    if output(&ld.console, &["isrunning", "--index", &idx])?.contains("running") {
        output(&ld.console, &["quit", "--index", &idx])?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while output(&ld.console, &["isrunning", "--index", &idx])?.contains("running") {
            if Instant::now() > deadline {
                return Err("LDPlayer didn't close. Close it yourself and try again.".into());
            }
            sleep(Duration::from_millis(500));
        }
    }
    let path = ld_config(&ld, index);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read LDPlayer's settings: {e}"))?;
    std::fs::write(&path, with_adb_enabled(&text)?).map_err(|e| format!("Couldn't change LDPlayer's settings: {e}"))
}

// ---------------------------------------------------------------------------
// Targets

pub fn targets() -> Vec<Target> {
    let mut out = Vec::new();
    let mut ld_serials = Vec::new();
    if let Some(ld) = ldplayer() {
        // index,title,top window,bind window,android started,pid,vbox pid,w,h,dpi
        for line in output(&ld.console, &["list2"]).unwrap_or_default().lines() {
            let f: Vec<&str> = line.split(',').collect();
            let Some(index) = f.first().and_then(|i| i.trim().parse::<u32>().ok()) else { continue };
            ld_serials.push(format!("emulator-{}", 5554 + 2 * index));
            out.push(Target {
                id: format!("ld:{index}"),
                name: f.get(1).unwrap_or(&"LDPlayer").to_string(),
                kind: "ldplayer".into(),
                running: f.get(4) == Some(&"1"),
                adb_enabled: ld_adb_enabled(&ld, index),
                detail: format!("LDPlayer instance {index}"),
            });
        }
    }
    if let Some(adb) = general_adb() {
        for line in output(&adb, &["devices", "-l"]).unwrap_or_default().lines().skip(1) {
            let mut words = line.split_whitespace();
            let (Some(serial), Some(state)) = (words.next(), words.next()) else { continue };
            if state != "device" || ld_serials.iter().any(|s| s == serial) {
                continue;
            }
            let model = words
                .find_map(|w| w.strip_prefix("model:"))
                .map(|m| m.replace('_', " "))
                .unwrap_or_else(|| serial.to_string());
            out.push(Target {
                id: format!("adb:{serial}"),
                name: model,
                kind: "device".into(),
                running: true,
                adb_enabled: Some(true),
                detail: serial.to_string(),
            });
        }
    }
    out
}

/// Starts the target if needed and returns adb for it once Android has booted.
fn ready(target: &str, progress: &mut impl FnMut(&str)) -> Result<Adb, String> {
    if let Some(index) = target.strip_prefix("ld:") {
        let index: u32 = index.parse().map_err(|_| "Unknown target.")?;
        let ld = ldplayer().ok_or("LDPlayer wasn't found.")?;
        if ld_adb_enabled(&ld, index) == Some(false) {
            return Err("ADB_DISABLED".into());
        }
        let idx = index.to_string();
        if !output(&ld.console, &["isrunning", "--index", &idx])?.contains("running") {
            progress("Starting LDPlayer");
            output(&ld.console, &["launch", "--index", &idx])?;
        }
        let adb = Adb { exe: ld.dir.join("adb.exe"), serial: format!("emulator-{}", 5554 + 2 * index) };
        wait_for_boot(&adb, progress)?;
        Ok(adb)
    } else if let Some(serial) = target.strip_prefix("adb:") {
        let adb = Adb { exe: general_adb().ok_or("adb wasn't found. Install Android platform tools under Settings → Tools.")?, serial: serial.into() };
        wait_for_boot(&adb, progress)?;
        Ok(adb)
    } else {
        Err("Unknown target.".into())
    }
}

fn wait_for_boot(adb: &Adb, progress: &mut impl FnMut(&str)) -> Result<(), String> {
    progress("Waiting for Android to finish starting");
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        if adb.online() && adb.shell("getprop sys.boot_completed").is_ok_and(|v| v.trim() == "1") {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err("The emulator didn't finish starting within 4 minutes.".into());
        }
        sleep(Duration::from_secs(2));
    }
}

/// Turns adb's install failure codes into plain language. "REPLACE" means a
/// different copy of the game is installed (another signing key, a newer
/// version, or a newer target SDK) and it must be removed first.
fn install_error(out: &str) -> String {
    let conflicts = [
        "INSTALL_FAILED_UPDATE_INCOMPATIBLE",
        "signatures do not match",
        "VERSION_DOWNGRADE",
        "doesn't support runtime permissions",
        "Failure [-26",
    ];
    if conflicts.iter().any(|c| out.contains(c)) {
        "REPLACE".into()
    } else if out.contains("NO_MATCHING_ABIS") {
        "This game needs a CPU type the emulator doesn't provide (for example arm64 only). Try another emulator or a phone.".into()
    } else if out.contains("OLDER_SDK") {
        "This game needs a newer Android version than this emulator has.".into()
    } else if out.contains("INSUFFICIENT_STORAGE") {
        "The emulator is out of storage space.".into()
    } else {
        format!("Installing failed: {}", out.lines().last().unwrap_or(out))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launched {
    pub target: String,
    pub package: String,
}

/// Starts the target, installs the APK (optionally removing an old copy
/// first) and launches the game.
pub fn run(
    target: &str,
    apk: &Path,
    package: &str,
    replace: bool,
    mut progress: impl FnMut(&str),
) -> Result<Launched, String> {
    let adb = ready(target, &mut progress)?;
    if replace {
        progress("Removing the installed copy");
        let _ = adb.run(&["uninstall", package]);
    }
    progress("Installing the game");
    let apk_arg = apk.to_string_lossy().into_owned();
    let out = adb.run(&["install", "-r", "-d", &apk_arg])?;
    if !out.contains("Success") {
        return Err(install_error(&out));
    }
    progress("Starting the game");
    let _ = adb.shell(&format!("am force-stop {package}"));
    let _ = adb.run(&["logcat", "-b", "crash", "-c"]);
    let out = adb.shell(&format!("monkey -p {package} -c android.intent.category.LAUNCHER 1"))?;
    if out.contains("No activities found") {
        return Err("The game has no launcher screen, so it can't be started directly.".into());
    }
    Ok(Launched { target: target.into(), package: package.into() })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    pub running: bool,
    pub pid: Option<u32>,
    /// When the game isn't running: Android's crash report, if there is one.
    pub crash_log: Option<String>,
}

fn adb_for(target: &str) -> Result<Adb, String> {
    if let Some(index) = target.strip_prefix("ld:") {
        let index: u32 = index.parse().map_err(|_| "Unknown target.")?;
        let ld = ldplayer().ok_or("LDPlayer wasn't found.")?;
        Ok(Adb { exe: ld.dir.join("adb.exe"), serial: format!("emulator-{}", 5554 + 2 * index) })
    } else if let Some(serial) = target.strip_prefix("adb:") {
        Ok(Adb { exe: general_adb().ok_or("adb wasn't found.")?, serial: serial.into() })
    } else {
        Err("Unknown target.".into())
    }
}

pub fn status(target: &str, package: &str) -> Result<GameStatus, String> {
    let adb = adb_for(target)?;
    if !adb.online() {
        return Ok(GameStatus { running: false, pid: None, crash_log: Some("The emulator or device is no longer connected.".into()) });
    }
    let pid = adb.shell(&format!("pidof {package}"))?.split_whitespace().next().and_then(|p| p.parse().ok());
    if pid.is_some() {
        return Ok(GameStatus { running: true, pid, crash_log: None });
    }
    let crash = adb.run(&["logcat", "-d", "-b", "crash"]).unwrap_or_default();
    let tail: Vec<&str> = crash.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect();
    Ok(GameStatus {
        running: false,
        pid: None,
        crash_log: (!tail.is_empty() && crash.contains(package)).then(|| tail.join("\n")),
    })
}

/// Makes a running game's Java debug port reachable on this PC (through adb)
/// and returns (local port, process id).
pub fn forward_jdwp(target: &str, package: &str) -> Result<(u16, u32), String> {
    let adb = adb_for(target)?;
    let pid: u32 = adb
        .shell(&format!("pidof {package}"))?
        .split_whitespace()
        .next()
        .and_then(|p| p.parse().ok())
        .ok_or("The game isn't running. Start it with Run first.")?;
    // Let the OS pick a free local port.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_err(|e| e.to_string())?
        .port();
    let out = adb.run(&["forward", &format!("tcp:{port}"), &format!("jdwp:{pid}")])?;
    if out.contains("error") {
        return Err(format!("Couldn't reach the game's debug port: {out}"));
    }
    Ok((port, pid))
}

/// Removes a port forward made by `forward_jdwp`.
pub fn unforward(target: &str, port: u16) {
    if let Ok(adb) = adb_for(target) {
        let _ = adb.run(&["forward", "--remove", &format!("tcp:{port}")]);
    }
}

pub fn stop(target: &str, package: &str) -> Result<(), String> {
    adb_for(target)?.shell(&format!("am force-stop {package}")).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_on_adb_in_ldplayer_config() {
        let cfg = "{\n    \"basicSettings.rootMode\": true,\n    \"basicSettings.adbDebug\": 0,\n    \"x\": 2\n}";
        let out = with_adb_enabled(cfg).unwrap();
        assert!(out.contains("\"basicSettings.adbDebug\": 1,"));
        assert!(out.contains("\"basicSettings.rootMode\": true"));
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["basicSettings.adbDebug"], 1);
        // Missing key: added; the file stays valid JSON.
        let added = with_adb_enabled("{\"a\": 1}").unwrap();
        let value: serde_json::Value = serde_json::from_str(&added).unwrap();
        assert_eq!((value["basicSettings.adbDebug"].as_i64(), value["a"].as_i64()), (Some(1), Some(1)));
        // Last key without a trailing comma.
        let last = with_adb_enabled("{\"a\": 1, \"basicSettings.adbDebug\": 0}").unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&last).unwrap()["basicSettings.adbDebug"], 1);
    }

    #[test]
    fn explains_install_failures() {
        assert_eq!(install_error("Failure [INSTALL_FAILED_UPDATE_INCOMPATIBLE: Package x signatures do not match]"), "REPLACE");
        assert_eq!(
            install_error("Failure [-26: Package x new target SDK 7 doesn't support runtime permissions but the old target SDK 34 does.]"),
            "REPLACE"
        );
        assert_eq!(install_error("Failure [INSTALL_FAILED_VERSION_DOWNGRADE]"), "REPLACE");
        assert!(install_error("Failure [INSTALL_FAILED_NO_MATCHING_ABIS: ...]").contains("CPU type"));
        assert!(install_error("Failure [INSTALL_FAILED_OLDER_SDK]").contains("newer Android"));
    }
}
