//! 3D model files shipped as plain files (OBJ, glTF, FBX...): which formats
//! GameX shows in 3D, and the files a model needs next to it (materials,
//! textures, buffers) so the viewer can load it the way the game does.

use std::fs;
use std::path::{Path, PathBuf};

/// Model formats the 3D viewer reads.
pub const MODEL_EXTENSIONS: &[&str] = &["obj", "gltf", "glb", "fbx", "dae", "stl", "ply", "3ds", "md2"];

/// Files a model may load: materials, buffers and textures.
const AUX_EXTENSIONS: &[&str] = &[
    "mtl", "bin", "png", "jpg", "jpeg", "webp", "bmp", "gif", "tga", "tif", "tiff", "ktx2", "dds",
];

/// Largest model (or side file) read for the viewer.
pub const MAX_MODEL: u64 = 256 << 20;
/// Side files read for one model, in all.
const MAX_AUX_TOTAL: u64 = 128 << 20;
const MAX_AUX_FILES: usize = 300;

pub fn is_model(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    MODEL_EXTENSIONS.contains(&ext.as_str())
}

/// Side files in the model's folder and up to two folders below it
/// (`textures/`, `materials/maps/`...), smallest first, within the limits.
pub fn aux_files(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<(u64, PathBuf)> = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((folder, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if depth < 2 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if AUX_EXTENSIONS.contains(&ext.as_str()) && meta.len() <= MAX_MODEL {
                found.push((meta.len(), path));
            }
        }
    }
    found.sort();
    let mut total = 0;
    found
        .into_iter()
        .take(MAX_AUX_FILES)
        .take_while(|(size, _)| {
            total += size;
            total <= MAX_AUX_TOTAL
        })
        .map(|(_, p)| p)
        .collect()
}

/// Decodes `%XX` escapes (paths sent in request headers, which must be ASCII).
pub fn percent_decode(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3).ok_or("Bad escape in the file path.")?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| "Bad escape in the file path.")?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "The file path isn't valid text.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_models_and_their_side_files() {
        assert!(is_model("apktool/assets/hero.OBJ") && is_model("files/a.glb") && !is_model("files/a.png"));
        let dir = std::env::temp_dir().join(format!("gamex-models-{}", std::process::id()));
        fs::create_dir_all(dir.join("textures/maps/deep")).unwrap();
        fs::write(dir.join("hero.obj"), "v 0 0 0").unwrap();
        fs::write(dir.join("hero.mtl"), "newmtl a").unwrap();
        fs::write(dir.join("textures/skin.png"), "png").unwrap();
        fs::write(dir.join("textures/maps/normal.jpg"), "jpg").unwrap();
        fs::write(dir.join("textures/maps/deep/too-far.png"), "png").unwrap();
        fs::write(dir.join("notes.txt"), "x").unwrap();
        let mut names: Vec<String> = aux_files(&dir)
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        names.sort();
        assert_eq!(names, ["hero.mtl", "textures/maps/normal.jpg", "textures/skin.png"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decodes_escaped_paths() {
        assert_eq!(percent_decode("apktool/assets/h%C3%A9ro%20x.obj").unwrap(), "apktool/assets/héro x.obj");
        assert!(percent_decode("bad%zz").is_err());
        assert!(percent_decode("cut%4").is_err());
    }
}
