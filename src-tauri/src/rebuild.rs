//! Rebuilding an APK: apktool's decoded output plus the user's edits, with
//! the choices from the build dialog applied, rebuilt with `apktool b`, then
//! zipaligned and signed with the user's own key.
//!
//! A game can't be re-signed with its developer's key: an APK only contains
//! the public certificate, never the private key. GameX creates one key per
//! user, once, and signs every build with it.
//!
//! ```text
//! <project>/build/work/        apktool output + edits, rebuilt from
//! <project>/build/unsigned.apk
//! <app data>/signing/gamex-key.jks, gamex-key.json
//! ```

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::edits::EditStore;
use crate::model::{ContainerKind, ImportRecord, StepStatus};
use crate::settings::Settings;
use crate::tools::{self, Tool};

/// Resource name GameX gives a custom icon.
const ICON_RES: &str = "gamex_icon";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub app_name: String,
    pub package: String,
    pub version_name: String,
    pub version_code: u32,
    pub min_sdk: Option<String>,
    pub permissions: Vec<String>,
    /// The current launcher icon as a data URL.
    pub icon: Option<String>,
    /// Why the game can't be built right now; empty when it can.
    pub problems: Vec<String>,
    pub key_exists: bool,
    pub key_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildOptions {
    pub app_name: String,
    pub package: String,
    pub version_name: String,
    pub version_code: u32,
    /// A PNG to use as the launcher icon; `None` keeps the game's icon.
    pub icon_path: Option<String>,
    pub removed_permissions: Vec<String>,
    pub output_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildResult {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub package: String,
    pub edited_files: usize,
}

// ---------------------------------------------------------------------------
// Reading the current values

fn read_current(store: &EditStore, rel: &str) -> String {
    store
        .current(rel)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .unwrap_or_default()
}

pub fn build_info(project_root: &Path, record: &ImportRecord, settings: &Settings, signing_dir: &Path) -> BuildInfo {
    let store = EditStore::new(project_root);
    let manifest = read_current(&store, "apktool/AndroidManifest.xml");
    let yml = read_current(&store, "apktool/apktool.yml");

    let app = first_open_tag(&manifest, "<application").map(|(s, e)| &manifest[s..e]).unwrap_or("");
    let launcher = launcher_tags(&manifest).first().map(|&(s, e)| &manifest[s..e]).unwrap_or("");
    // The launcher's own label and icon are what people see, if it has them.
    let label = attr(launcher, "android:label").or_else(|| attr(app, "android:label")).unwrap_or("");
    let icon_ref = attr(launcher, "android:icon").or_else(|| attr(app, "android:icon"));

    let package = first_open_tag(&manifest, "<manifest")
        .and_then(|(s, e)| attr(&manifest[s..e], "package").map(String::from))
        .unwrap_or_default();

    let key = signing_dir.join("gamex-key.jks");
    let mut problems = Vec::new();
    if record.container != ContainerKind::Apk {
        problems.push("Only games imported as an APK can be rebuilt.".into());
    } else if !record.steps.iter().any(|s| s.id == "apktool" && s.status == StepStatus::Done) {
        problems.push("apktool didn't decode this APK during import. Set it up in Settings, then import the game again.".into());
    }
    if tools::resolve(Tool::Apktool, settings).is_none() {
        problems.push("apktool wasn't found. Set its path in Settings.".into());
    }
    if tools::resolve(Tool::Signer, settings).is_none() {
        problems.push("uber-apk-signer wasn't found. Set its path in Settings.".into());
    }
    if tools::java_tool("java").is_none() {
        problems.push("Java wasn't found. Install Java 11 or newer.".into());
    }
    if !key.is_file() && tools::java_tool("keytool").is_none() {
        problems.push("keytool (part of Java) wasn't found, so a signing key can't be created.".into());
    }

    BuildInfo {
        app_name: resolve_string(&store, label),
        package,
        version_name: yml_value(&yml, "versionName").unwrap_or_default(),
        version_code: yml_value(&yml, "versionCode").and_then(|v| v.parse().ok()).unwrap_or(1),
        min_sdk: yml_value(&yml, "minSdkVersion"),
        permissions: permissions(&manifest),
        icon: icon_ref.and_then(|r| icon_data_url(project_root, &store, r)),
        problems,
        key_exists: key.is_file(),
        key_path: key.display().to_string(),
    }
}

/// What the phone shows for the game: the launcher's label and icon (data URL).
/// Empty label when the game isn't a decoded APK.
pub fn identity(project_root: &Path) -> (String, Option<String>) {
    let store = EditStore::new(project_root);
    let manifest = read_current(&store, "apktool/AndroidManifest.xml");
    let app = first_open_tag(&manifest, "<application").map(|(s, e)| &manifest[s..e]).unwrap_or("");
    let launcher = launcher_tags(&manifest).first().map(|&(s, e)| &manifest[s..e]).unwrap_or("");
    let label = attr(launcher, "android:label").or_else(|| attr(app, "android:label")).unwrap_or("");
    let icon_ref = attr(launcher, "android:icon").or_else(|| attr(app, "android:icon"));
    let name = if label.is_empty() { String::new() } else { resolve_string(&store, label) };
    (name.trim().to_string(), icon_ref.and_then(|r| icon_data_url(project_root, &store, r)))
}

/// `@string/app_name` -> its text in res/values/strings.xml.
fn resolve_string(store: &EditStore, label: &str) -> String {
    let Some(name) = label.strip_prefix("@string/") else {
        return xml_unescape(label);
    };
    let strings = read_current(store, "apktool/res/values/strings.xml");
    let open = format!("<string name=\"{name}\"");
    strings
        .find(&open)
        .and_then(|i| {
            let start = i + strings[i..].find('>')? + 1;
            let end = start + strings[start..].find("</string>")?;
            Some(xml_unescape(&strings[start..end]).replace("\\'", "'"))
        })
        .unwrap_or_else(|| name.to_string())
}

/// `@drawable/icon` -> the sharpest `res/drawable*/icon.png`, as a data URL.
fn icon_data_url(project_root: &Path, store: &EditStore, reference: &str) -> Option<String> {
    let (ty, name) = reference.strip_prefix('@')?.split_once('/')?;
    let res = project_root.join("unpacked/apktool/res");
    let mut best: Option<(u8, String)> = None;
    for dir in fs::read_dir(&res).ok()?.filter_map(Result::ok) {
        let dir_name = dir.file_name().to_string_lossy().into_owned();
        let (dir_ty, qualifier) = dir_name.split_once('-').unwrap_or((&dir_name, ""));
        if dir_ty != ty {
            continue;
        }
        for ext in ["png", "webp", "jpg"] {
            let rel = format!("apktool/res/{dir_name}/{name}.{ext}");
            if project_root.join("unpacked").join(&rel).is_file() {
                let rank = density_rank(qualifier);
                if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                    best = Some((rank, rel));
                }
            }
        }
    }
    let (_, rel) = best?;
    let bytes = fs::read(store.current(&rel).ok()?).ok()?;
    let mime = match rel.rsplit('.').next() {
        Some("webp") => "image/webp",
        Some("jpg") => "image/jpeg",
        _ => "image/png",
    };
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

fn density_rank(qualifier: &str) -> u8 {
    for (i, d) in ["xxxhdpi", "xxhdpi", "xhdpi", "hdpi", "nodpi", "mdpi"].iter().enumerate() {
        if qualifier.split('-').any(|q| q == *d) {
            return i as u8;
        }
    }
    if qualifier.is_empty() { 6 } else { 7 }
}

// ---------------------------------------------------------------------------
// Manifest and apktool.yml editing (plain text, so formatting is kept)

/// `(start, end)` of each opening tag `<name ...>` (end is after the `>`).
fn open_tags(xml: &str, name: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(i) = xml[from..].find(name) {
        let start = from + i;
        let after = xml[start + name.len()..].chars().next();
        if after.is_some_and(|c| c.is_whitespace() || c == '>' || c == '/') {
            if let Some(len) = xml[start..].find('>') {
                found.push((start, start + len + 1));
            }
        }
        from = start + name.len();
    }
    found
}

fn first_open_tag(xml: &str, name: &str) -> Option<(usize, usize)> {
    open_tags(xml, name).into_iter().next()
}

/// Opening tags of activities (and aliases) that appear in the launcher.
fn launcher_tags(xml: &str) -> Vec<(usize, usize)> {
    let mut tags: Vec<(usize, usize)> = open_tags(xml, "<activity")
        .into_iter()
        .chain(open_tags(xml, "<activity-alias"))
        .filter(|&(s, e)| {
            let self_closing = xml[s..e].ends_with("/>");
            let block_end = if self_closing { e } else { xml[e..].find("</activity").map_or(xml.len(), |i| e + i) };
            xml[s..block_end].contains("android.intent.category.LAUNCHER")
        })
        .collect();
    tags.sort();
    tags
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

/// Sets an attribute on an opening tag, adding it if missing.
fn set_attr(tag: &str, name: &str, value: &str) -> String {
    let key = format!(" {name}=\"");
    if let Some(i) = tag.find(&key) {
        let start = i + key.len();
        let end = start + tag[start..].find('"').unwrap_or(0);
        format!("{}{value}{}", &tag[..start], &tag[end..])
    } else {
        let insert = if tag.ends_with("/>") { tag.len() - 2 } else { tag.len() - 1 };
        format!("{} {name}=\"{value}\"{}", &tag[..insert], &tag[insert..])
    }
}

fn xml_attr_escape(value: &str) -> String {
    let escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    // A leading @ or ? would be read as a resource reference.
    if escaped.starts_with(['@', '?']) {
        format!("\\{escaped}")
    } else {
        escaped
    }
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn permissions(xml: &str) -> Vec<String> {
    let mut list: Vec<String> = open_tags(xml, "<uses-permission")
        .into_iter()
        .filter_map(|(s, e)| attr(&xml[s..e], "android:name").map(String::from))
        .collect();
    list.dedup();
    list
}

pub struct ManifestChanges<'a> {
    pub label: &'a str,
    pub icon: Option<&'a str>,
    pub removed_permissions: &'a [String],
}

pub fn edit_manifest(xml: &str, changes: &ManifestChanges) -> String {
    let label = xml_attr_escape(changes.label);
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();

    if let Some((s, e)) = first_open_tag(xml, "<application") {
        let mut tag = set_attr(&xml[s..e], "android:label", &label);
        if let Some(icon) = changes.icon {
            tag = set_attr(&tag, "android:icon", icon);
            if attr(&tag, "android:roundIcon").is_some() {
                tag = set_attr(&tag, "android:roundIcon", icon);
            }
        }
        replacements.push((s, e, tag));
    }
    // A launcher activity's own label and icon override the app's, so they
    // change too, but only where the activity sets them.
    for (s, e) in launcher_tags(xml) {
        let mut tag = xml[s..e].to_string();
        if attr(&tag, "android:label").is_some() {
            tag = set_attr(&tag, "android:label", &label);
        }
        if let Some(icon) = changes.icon {
            for name in ["android:icon", "android:roundIcon"] {
                if attr(&tag, name).is_some() {
                    tag = set_attr(&tag, name, icon);
                }
            }
        }
        replacements.push((s, e, tag));
    }
    for name in ["<uses-permission", "<uses-permission-sdk-23"] {
        for (s, mut e) in open_tags(xml, name) {
            let removed = attr(&xml[s..e], "android:name").is_some_and(|p| changes.removed_permissions.iter().any(|r| r == p));
            if !removed {
                continue;
            }
            if !xml[s..e].ends_with("/>") {
                if let Some(close) = xml[e..].find('>') {
                    e += close + 1;
                }
            }
            // Take the line break and indentation that followed it too.
            let trailing = xml[e..].len() - xml[e..].trim_start_matches([' ', '\t', '\r', '\n']).len();
            replacements.push((s, e + trailing, String::new()));
        }
    }

    // Apply from the end so earlier offsets stay valid.
    replacements.sort_by_key(|r| std::cmp::Reverse(r.0));
    let mut out = xml.to_string();
    for (s, e, text) in replacements {
        out.replace_range(s..e, &text);
    }
    out
}

/// Tags whose class-name attributes are resolved against the package.
const COMPONENT_TAGS: &[&str] = &[
    "<application", "<activity", "<activity-alias", "<service", "<receiver", "<provider", "<instrumentation",
];
const CLASS_ATTRS: &[&str] = &[
    "android:name", "android:targetActivity", "android:backupAgent", "android:manageSpaceActivity",
    "android:parentActivityName",
];

/// Changes the app's package (its identity on the phone) without moving any
/// code. apktool 3 no longer does this itself, so the manifest is edited:
///
/// - relative class names (`.MainActivity`) are made absolute first, since
///   they would otherwise resolve against the new package and crash;
/// - provider authorities and custom permissions named after the old package
///   are renamed too, or Android refuses to install both versions.
pub fn rename_package(xml: &str, old: &str, new: &str) -> String {
    let old_dot = format!("{old}.");
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    let mut edit_tag = |s: usize, e: usize, f: &dyn Fn(&str) -> String| {
        let tag = &xml[s..e];
        let changed = f(tag);
        if changed != tag {
            replacements.push((s, e, changed));
        }
    };

    for &name in COMPONENT_TAGS {
        for (s, e) in open_tags(xml, name) {
            edit_tag(s, e, &|tag| {
                let mut tag = tag.to_string();
                for &a in CLASS_ATTRS {
                    if let Some(v) = attr(&tag, a) {
                        let full = if v.starts_with('.') {
                            format!("{old}{v}")
                        } else if !v.contains('.') {
                            format!("{old}.{v}")
                        } else {
                            continue;
                        };
                        tag = set_attr(&tag, a, &full);
                    }
                }
                if let Some(v) = attr(&tag, "android:authorities") {
                    let renamed: Vec<String> = v
                        .split(';')
                        .map(|auth| match auth.strip_prefix(old) {
                            Some(rest) if rest.is_empty() || rest.starts_with('.') => format!("{new}{rest}"),
                            _ => auth.to_string(),
                        })
                        .collect();
                    tag = set_attr(&tag, "android:authorities", &renamed.join(";"));
                }
                tag
            });
        }
    }
    // Custom permissions the app defines under its own package name.
    for name in ["<permission", "<uses-permission", "<permission-tree"] {
        for (s, e) in open_tags(xml, name) {
            edit_tag(s, e, &|tag| match attr(tag, "android:name") {
                Some(v) if v.starts_with(&old_dot) => set_attr(tag, "android:name", &format!("{new}{}", &v[old.len()..])),
                _ => tag.to_string(),
            });
        }
    }
    if let Some((s, e)) = first_open_tag(xml, "<manifest") {
        edit_tag(s, e, &|tag| set_attr(tag, "package", new));
    }

    replacements.sort_by_key(|r| std::cmp::Reverse(r.0));
    let mut out = xml.to_string();
    for (s, e, text) in replacements {
        out.replace_range(s..e, &text);
    }
    out
}

fn yml_value(yml: &str, key: &str) -> Option<String> {
    yml.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix(key)?.strip_prefix(':')?.trim();
        let unquoted = if rest.len() >= 2 && rest.starts_with('\'') && rest.ends_with('\'') {
            rest[1..rest.len() - 1].replace("''", "'")
        } else {
            rest.trim_matches('"').to_string()
        };
        Some(unquoted)
    })
}

fn yml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Sets the version apktool builds with.
pub fn edit_apktool_yml(yml: &str, version_code: u32, version_name: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let (mut code_set, mut name_set) = (false, false);
    for line in yml.lines() {
        let indent = &line[..line.len() - line.trim_start().len()];
        let trimmed = line.trim_start();
        if trimmed.starts_with("versionCode:") {
            lines.push(format!("{indent}versionCode: {version_code}"));
            code_set = true;
        } else if trimmed.starts_with("versionName:") {
            lines.push(format!("{indent}versionName: {}", yml_quote(version_name)));
            name_set = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !code_set || !name_set {
        lines.retain(|l| l.trim() != "versionInfo:");
        lines.push("versionInfo:".into());
        lines.push(format!("  versionCode: {version_code}"));
        lines.push(format!("  versionName: {}", yml_quote(version_name)));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// Building

pub fn validate(options: &BuildOptions) -> Result<(), String> {
    if options.app_name.trim().is_empty() {
        return Err("Enter an app name.".into());
    }
    let valid_package = options.package.split('.').count() >= 2
        && options.package.split('.').all(|part| {
            part.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    if !valid_package {
        return Err("The package name must look like com.example.game: two or more parts, each starting with a letter.".into());
    }
    if options.version_code == 0 || options.version_code > 2_100_000_000 {
        return Err("The version code must be a whole number from 1 to 2100000000.".into());
    }
    if options.version_name.trim().is_empty() {
        return Err("Enter a version name, like 1.0.".into());
    }
    let out = Path::new(&options.output_path);
    if !out.is_absolute() || !options.output_path.to_lowercase().ends_with(".apk") {
        return Err("Choose where to save the .apk file.".into());
    }
    Ok(())
}

/// Builds `build/work`: apktool's output, the user's edits, and the build
/// options applied. Returns the work folder and how many edits it holds.
pub fn prepare(project_root: &Path, options: &BuildOptions) -> Result<(PathBuf, usize), String> {
    let io = |e: io::Error| e.to_string();
    let store = EditStore::new(project_root);
    let work = project_root.join("build").join("work");
    if work.exists() {
        fs::remove_dir_all(&work).map_err(io)?;
    }
    crate::import::copy_dir(&project_root.join("unpacked").join("apktool"), &work).map_err(io)?;
    let edited = store.overlay("apktool/", &work).map_err(io)?;

    let icon = match &options.icon_path {
        Some(path) => {
            let bytes = fs::read(path).map_err(|e| format!("Couldn't read the icon: {e}"))?;
            if !bytes.starts_with(b"\x89PNG") {
                return Err("The icon must be a PNG image.".into());
            }
            let dir = work.join("res").join("drawable-nodpi");
            fs::create_dir_all(&dir).map_err(io)?;
            fs::write(dir.join(format!("{ICON_RES}.png")), bytes).map_err(io)?;
            Some(format!("@drawable/{ICON_RES}"))
        }
        None => None,
    };

    let manifest_path = work.join("AndroidManifest.xml");
    let manifest = fs::read_to_string(&manifest_path).map_err(io)?;
    let original_package = first_open_tag(&manifest, "<manifest")
        .and_then(|(s, e)| attr(&manifest[s..e], "package").map(String::from))
        .unwrap_or_default();
    let manifest = if options.package != original_package && !original_package.is_empty() {
        rename_package(&manifest, &original_package, &options.package)
    } else {
        manifest
    };
    let manifest = edit_manifest(
        &manifest,
        &ManifestChanges {
            label: options.app_name.trim(),
            icon: icon.as_deref(),
            removed_permissions: &options.removed_permissions,
        },
    );
    fs::write(&manifest_path, manifest).map_err(io)?;

    let yml_path = work.join("apktool.yml");
    let yml = fs::read_to_string(&yml_path).map_err(io)?;
    let yml = edit_apktool_yml(&yml, options.version_code, options.version_name.trim());
    fs::write(&yml_path, yml).map_err(io)?;
    Ok((work, edited))
}

struct SigningKey {
    path: PathBuf,
    password: String,
    alias: String,
}

#[derive(Serialize, Deserialize)]
struct KeyFile {
    password: String,
    alias: String,
}

/// The user's signing key, created on first use.
fn ensure_key(signing_dir: &Path) -> Result<SigningKey, String> {
    let path = signing_dir.join("gamex-key.jks");
    let meta = signing_dir.join("gamex-key.json");
    if path.is_file() {
        let text = fs::read_to_string(&meta).map_err(|e| format!("The signing key's password file is missing: {e}"))?;
        let key: KeyFile = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        return Ok(SigningKey {
            path,
            password: key.password,
            alias: key.alias,
        });
    }
    let keytool = tools::java_tool("keytool").ok_or("keytool (part of Java) wasn't found.")?;
    fs::create_dir_all(signing_dir).map_err(|e| e.to_string())?;
    let key = KeyFile {
        password: uuid::Uuid::new_v4().simple().to_string(),
        alias: "gamex".into(),
    };
    let keystore = path.to_string_lossy().into_owned();
    let args = [
        "-genkeypair", "-keystore", &keystore, "-storetype", "PKCS12", "-storepass", &key.password,
        "-keypass", &key.password, "-alias", &key.alias, "-keyalg", "RSA", "-keysize", "2048", "-validity",
        "10000", "-dname", "CN=GameX user, O=GameX",
    ];
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    tools::run(&keytool, &args)?;
    fs::write(&meta, serde_json::to_string_pretty(&key).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(SigningKey {
        path,
        password: key.password,
        alias: key.alias,
    })
}

pub fn build(
    project_root: &Path,
    settings: &Settings,
    signing_dir: &Path,
    options: &BuildOptions,
    mut progress: impl FnMut(&str, &str),
) -> Result<BuildResult, String> {
    validate(options)?;
    let apktool = tools::resolve(Tool::Apktool, settings).ok_or("apktool wasn't found. Set its path in Settings.")?;
    let signer = tools::resolve(Tool::Signer, settings).ok_or("uber-apk-signer wasn't found. Set its path in Settings.")?;
    let java = tools::java_tool("java").ok_or("Java wasn't found.")?;

    progress("prepare", "Applying your edits and build settings");
    let (work, edited_files) = prepare(project_root, options)?;
    let build_dir = project_root.join("build");
    let unsigned = build_dir.join("unsigned.apk");
    let signed_dir = build_dir.join("signed");
    let _ = fs::remove_file(&unsigned);
    let _ = fs::remove_dir_all(&signed_dir);

    progress("apktool", "Rebuilding the APK with apktool (this can take a minute)");
    tools::run(&apktool, &[OsStr::new("b"), work.as_os_str(), OsStr::new("-o"), unsigned.as_os_str()])
        .map_err(|e| format!("apktool couldn't rebuild the game. If you edited smali or XML, check it for mistakes.\n\n{e}"))?;
    if !unsigned.is_file() {
        return Err("apktool finished but didn't produce an APK.".into());
    }

    progress("key", "Getting your signing key");
    let key = ensure_key(signing_dir)?;

    progress("sign", "Aligning and signing");
    let args: Vec<&OsStr> = vec![
        OsStr::new("-jar"),
        signer.as_os_str(),
        OsStr::new("-a"),
        unsigned.as_os_str(),
        OsStr::new("-o"),
        signed_dir.as_os_str(),
        OsStr::new("--ks"),
        key.path.as_os_str(),
        OsStr::new("--ksAlias"),
        OsStr::new(&key.alias),
        OsStr::new("--ksPass"),
        OsStr::new(&key.password),
        OsStr::new("--ksKeyPass"),
        OsStr::new(&key.password),
    ];
    tools::run(&java, &args)?;
    let signed = fs::read_dir(&signed_dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "apk"))
        .ok_or("The signer finished but didn't produce an APK.")?;

    progress("save", "Saving the APK");
    let out = PathBuf::from(&options.output_path);
    fs::copy(&signed, &out).map_err(|e| format!("Couldn't save to {}: {e}", out.display()))?;
    let (sha256, size) = hash_file(&out).map_err(|e| e.to_string())?;

    // Remember the build so it can be run on an emulator.
    let last = serde_json::json!({ "path": out.display().to_string(), "package": options.package });
    let _ = fs::write(build_dir.join("last.json"), last.to_string());

    // The work copy can be large; the saved APK is what matters.
    let _ = fs::remove_dir_all(&work);
    let _ = fs::remove_dir_all(&signed_dir);
    let _ = fs::remove_file(&unsigned);

    Ok(BuildResult {
        path: out.display().to_string(),
        size,
        sha256,
        package: options.package.clone(),
        edited_files,
    })
}

fn hash_file(path: &Path) -> io::Result<(String, u64)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((hasher.finalize().iter().map(|b| format!("{b:02x}")).collect(), total))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.studio.game">
    <uses-permission android:name="android.permission.INTERNET"/>
    <uses-permission android:name="android.permission.VIBRATE"/>
    <application android:icon="@drawable/icon" android:label="@string/app_name">
        <activity android:name=".Settings" android:label="Settings"/>
        <activity android:icon="@drawable/icon" android:label="Old" android:name=".Main">
            <intent-filter>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter>
        </activity>
    </application>
</manifest>"#;

    #[test]
    fn edits_the_manifest_in_place() {
        let removed = vec!["android.permission.INTERNET".to_string()];
        let out = edit_manifest(
            MANIFEST,
            &ManifestChanges {
                label: "My \"Mod\" & more",
                icon: Some("@drawable/gamex_icon"),
                removed_permissions: &removed,
            },
        );
        assert!(!out.contains("INTERNET"));
        assert!(out.contains("android.permission.VIBRATE"));
        assert!(out.contains(r#"<application android:icon="@drawable/gamex_icon" android:label="My &quot;Mod&quot; &amp; more">"#));
        assert!(out.contains(r#"<activity android:icon="@drawable/gamex_icon" android:label="My &quot;Mod&quot; &amp; more" android:name=".Main">"#));
        // Activities that aren't in the launcher keep their own label.
        assert!(out.contains(r#"android:label="Settings""#));
        assert_eq!(permissions(&out), ["android.permission.VIBRATE"]);
    }

    #[test]
    fn edits_apktool_yml() {
        let yml = "version: 3.0.3\nversionInfo:\n  versionCode: 11\n  versionName: 1.7.3\nsdkInfo:\n  minSdkVersion: 21\n";
        let out = edit_apktool_yml(yml, 12, "1.7.4-mod");
        assert_eq!(yml_value(&out, "versionCode").as_deref(), Some("12"));
        assert_eq!(yml_value(&out, "versionName").as_deref(), Some("1.7.4-mod"));
        assert_eq!(yml_value(&out, "minSdkVersion").as_deref(), Some("21"));
    }

    #[test]
    fn renames_the_package_without_breaking_classes() {
        let xml = r#"<manifest package="com.old.game">
    <permission android:name="com.old.game.permission.C2D"/>
    <uses-permission android:name="com.old.game.permission.C2D"/>
    <uses-permission android:name="android.permission.VIBRATE"/>
    <application android:name=".App" android:backupAgent="Backup">
        <activity android:name=".Main"/>
        <activity android:name="com.lib.ads.AdActivity"/>
        <activity-alias android:name=".Alias" android:targetActivity=".Main"/>
        <provider android:authorities="com.old.game.files;com.old.gamex;other.auth" android:name="com.lib.Prov"/>
    </application>
</manifest>"#;
        let out = rename_package(xml, "com.old.game", "com.new.game");
        for expected in [
            r#"package="com.new.game""#,
            r#"<permission android:name="com.new.game.permission.C2D"/>"#,
            r#"<uses-permission android:name="com.new.game.permission.C2D"/>"#,
            r#"android:name="android.permission.VIBRATE""#,
            r#"android:name="com.old.game.App" android:backupAgent="com.old.game.Backup""#,
            r#"<activity android:name="com.old.game.Main"/>"#,
            r#"<activity android:name="com.lib.ads.AdActivity"/>"#,
            r#"android:name="com.old.game.Alias" android:targetActivity="com.old.game.Main""#,
            r#"android:authorities="com.new.game.files;com.old.gamex;other.auth""#,
        ] {
            assert!(out.contains(expected), "missing {expected}\n{out}");
        }
    }

    fn options(dir: &Path) -> BuildOptions {
        BuildOptions {
            app_name: "Modded".into(),
            package: "com.studio.game".into(),
            version_name: "2.0".into(),
            version_code: 20,
            icon_path: None,
            removed_permissions: vec![],
            output_path: dir.join("out.apk").display().to_string(),
        }
    }

    #[test]
    fn prepare_applies_edits_and_options() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let apktool = root.join("unpacked/apktool");
        fs::create_dir_all(apktool.join("smali/a")).unwrap();
        fs::write(apktool.join("AndroidManifest.xml"), MANIFEST).unwrap();
        fs::write(apktool.join("apktool.yml"), "versionInfo:\n  versionCode: 1\n  versionName: 1.0\n").unwrap();
        fs::write(apktool.join("smali/a/Hero.smali"), "speed 1").unwrap();
        EditStore::new(root).save_text("apktool/smali/a/Hero.smali", "speed 5").unwrap();

        let icon = root.join("icon.png");
        fs::write(&icon, b"\x89PNG....").unwrap();
        let opts = BuildOptions {
            icon_path: Some(icon.display().to_string()),
            package: "com.me.modded".into(),
            ..options(root)
        };
        let (work, edited) = prepare(root, &opts).unwrap();
        assert_eq!(edited, 1);
        assert_eq!(fs::read_to_string(work.join("smali/a/Hero.smali")).unwrap(), "speed 5");
        assert!(work.join("res/drawable-nodpi/gamex_icon.png").is_file());
        let manifest = fs::read_to_string(work.join("AndroidManifest.xml")).unwrap();
        assert!(manifest.contains(r#"android:label="Modded""#));
        assert!(manifest.contains(r#"package="com.me.modded""#));
        // Code keeps its package, so relative names now point at it in full.
        assert!(manifest.contains(r#"android:name="com.studio.game.Main""#));
        let yml = fs::read_to_string(work.join("apktool.yml")).unwrap();
        assert_eq!(yml_value(&yml, "versionCode").as_deref(), Some("20"));
        // The original apktool output is untouched.
        assert_eq!(fs::read_to_string(apktool.join("smali/a/Hero.smali")).unwrap(), "speed 1");
    }

    #[test]
    fn validates_options() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate(&options(dir.path())).is_ok());
        for bad in ["game", "com..game", "1com.game", "com.ga-me"] {
            let o = BuildOptions { package: bad.into(), ..options(dir.path()) };
            assert!(validate(&o).is_err(), "{bad}");
        }
        assert!(validate(&BuildOptions { version_code: 0, ..options(dir.path()) }).is_err());
        assert!(validate(&BuildOptions { output_path: "out.apk".into(), ..options(dir.path()) }).is_err());
    }
}
