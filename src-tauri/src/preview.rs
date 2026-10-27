//! File previews for the inspector: images and audio as data URLs, text as
//! a string, anything else as a short hex dump.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::Serialize;

use crate::edits::{is_plain_relative, EditStore};

const MAX_MEDIA: u64 = 25 << 20;
const MAX_TEXT: usize = 512 << 10;
const MAX_EDITABLE_TEXT: u64 = 8 << 20;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Preview {
    Image { data_url: String, size: u64 },
    Audio { data_url: String, size: u64 },
    Text { text: String, truncated: bool, size: u64 },
    Binary { head: String, size: u64 },
    TooLarge { size: u64 },
}

fn media_type(ext: &str) -> Option<(&'static str, bool)> {
    Some(match ext {
        "png" => ("image/png", true),
        "jpg" | "jpeg" => ("image/jpeg", true),
        "gif" => ("image/gif", true),
        "webp" => ("image/webp", true),
        "bmp" => ("image/bmp", true),
        "svg" => ("image/svg+xml", true),
        "ico" => ("image/x-icon", true),
        "ogg" | "opus" => ("audio/ogg", false),
        "mp3" => ("audio/mpeg", false),
        "wav" => ("audio/wav", false),
        "m4a" | "aac" => ("audio/mp4", false),
        "flac" => ("audio/flac", false),
        _ => return None,
    })
}

/// Checks that `rel` names a file inside `<project>/unpacked` and returns the
/// version to show: the user's edit if there is one, else the original.
pub fn resolve(project_root: &Path, rel: &str) -> io::Result<PathBuf> {
    if !is_plain_relative(rel) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid file path"));
    }
    let unpacked = project_root.join("unpacked");
    let real = fs::canonicalize(unpacked.join(rel))?;
    if !real.starts_with(fs::canonicalize(&unpacked)?) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "file is outside the project"));
    }
    EditStore::new(project_root).current(rel)
}

/// The whole file as text, for the editor.
pub fn read_text(project_root: &Path, rel: &str) -> io::Result<String> {
    let path = resolve(project_root, rel)?;
    if fs::metadata(&path)?.len() > MAX_EDITABLE_TEXT {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "This file is too large to edit here."));
    }
    let bytes = fs::read(path)?;
    match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => Ok(text),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "This isn't a text file.")),
    }
}

/// Previews `rel`, a path inside the project's `unpacked/`.
pub fn preview(project_root: &Path, rel: &str) -> io::Result<Preview> {
    let real = resolve(project_root, rel)?;
    let size = fs::metadata(&real)?.len();
    let ext = rel.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();

    if let Some((mime, image)) = media_type(&ext) {
        if size > MAX_MEDIA {
            return Ok(Preview::TooLarge { size });
        }
        let data_url = format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(fs::read(&real)?)
        );
        return Ok(if image {
            Preview::Image { data_url, size }
        } else {
            Preview::Audio { data_url, size }
        });
    }

    let mut buf = Vec::new();
    File::open(&real)?.take(MAX_TEXT as u64).read_to_end(&mut buf)?;
    if let Some(text) = as_text(&buf) {
        return Ok(Preview::Text {
            text,
            truncated: size > buf.len() as u64,
            size,
        });
    }
    let head = buf
        .chunks(16)
        .take(16)
        .enumerate()
        .map(|(row, chunk)| {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let ascii: String = chunk
                .iter()
                .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
                .collect();
            format!("{:08x}  {:<47}  {ascii}", row * 16, hex.join(" "))
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Preview::Binary { head, size })
}

/// UTF-8 with no NUL bytes; a character cut off at the end is tolerated.
fn as_text(buf: &[u8]) -> Option<String> {
    if buf.contains(&0) {
        return None;
    }
    match std::str::from_utf8(buf) {
        Ok(s) => Some(s.to_string()),
        Err(e) if e.error_len().is_none() => Some(String::from_utf8_lossy(&buf[..e.valid_up_to()]).into_owned()),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_by_content_and_refuses_escapes() {
        let dir = tempfile::tempdir().unwrap();
        let unpacked = dir.path().join("unpacked");
        fs::create_dir_all(unpacked.join("a")).unwrap();
        fs::write(unpacked.join("a/notes.cfg"), "speed = 3").unwrap();
        fs::write(unpacked.join("a/blob.bin"), [0u8, 1, 2, 255]).unwrap();
        fs::write(unpacked.join("a/x.png"), [137u8, 80, 78, 71]).unwrap();
        fs::write(dir.path().join("secret.txt"), "no").unwrap();

        let root = dir.path();
        assert!(matches!(preview(root, "a/notes.cfg").unwrap(), Preview::Text { ref text, .. } if text == "speed = 3"));
        assert!(matches!(preview(root, "a/blob.bin").unwrap(), Preview::Binary { size: 4, .. }));
        assert!(matches!(preview(root, "a/x.png").unwrap(), Preview::Image { ref data_url, .. } if data_url.starts_with("data:image/png;base64,")));
        assert!(preview(root, "../secret.txt").is_err());
        assert!(preview(root, "a/../../secret.txt").is_err());
        assert!(preview(root, &root.join("secret.txt").to_string_lossy()).is_err());
        assert!(read_text(root, "a/blob.bin").is_err());

        // Edited files are shown as edited.
        fs::create_dir_all(root.join("edits/a")).unwrap();
        fs::write(root.join("edits/a/notes.cfg"), "speed = 9").unwrap();
        assert_eq!(read_text(root, "a/notes.cfg").unwrap(), "speed = 9");
    }
}
