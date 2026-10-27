//! Reads Android's binary XML (the compiled AndroidManifest.xml inside every
//! APK) just far enough to get the package name, so any APK can be launched
//! even when apktool didn't decode it.

use std::io::Read;
use std::path::Path;

fn u16_at(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize)
}
fn u32_at(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize)
}

const RES_XML: usize = 0x0003;
const STRING_POOL: usize = 0x0001;
const START_ELEMENT: usize = 0x0102;
const UTF8_FLAG: usize = 0x100;

/// Strings of a string pool chunk starting at `at`.
fn string_pool(b: &[u8], at: usize) -> Option<Vec<String>> {
    let header = u16_at(b, at + 2)?;
    let count = u32_at(b, at + 8)?;
    let flags = u32_at(b, at + 16)?;
    let strings_start = at + u32_at(b, at + 20)?;
    let utf8 = flags & UTF8_FLAG != 0;
    let mut out = Vec::with_capacity(count);
    for n in 0..count {
        let mut pos = strings_start + u32_at(b, at + header + n * 4)?;
        let text = if utf8 {
            // UTF-16 length, then UTF-8 length (each 1 or 2 bytes), then bytes.
            let skip_len = |pos: &mut usize| -> Option<usize> {
                let first = *b.get(*pos)? as usize;
                if first & 0x80 != 0 {
                    let len = ((first & 0x7F) << 8) | *b.get(*pos + 1)? as usize;
                    *pos += 2;
                    Some(len)
                } else {
                    *pos += 1;
                    Some(first)
                }
            };
            skip_len(&mut pos)?;
            let len = skip_len(&mut pos)?;
            String::from_utf8_lossy(b.get(pos..pos + len)?).into_owned()
        } else {
            let mut len = u16_at(b, pos)?;
            pos += 2;
            if len & 0x8000 != 0 {
                len = ((len & 0x7FFF) << 16) | u16_at(b, pos)?;
                pos += 2;
            }
            let units: Vec<u16> = (0..len).filter_map(|k| u16_at(b, pos + k * 2).map(|u| u as u16)).collect();
            String::from_utf16_lossy(&units)
        };
        out.push(text);
    }
    Some(out)
}

/// The `package` attribute of `<manifest>` in a binary AndroidManifest.xml.
pub fn manifest_package(b: &[u8]) -> Option<String> {
    if u16_at(b, 0)? != RES_XML {
        return None;
    }
    let mut pos = u16_at(b, 2)?; // after the file header
    let mut strings: Option<Vec<String>> = None;
    while pos + 8 <= b.len() {
        let kind = u16_at(b, pos)?;
        let size = u32_at(b, pos + 4)?;
        if size < 8 {
            return None;
        }
        match kind {
            STRING_POOL => strings = string_pool(b, pos),
            START_ELEMENT => {
                let strings = strings.as_ref()?;
                let header = u16_at(b, pos + 2)?;
                let body = pos + header;
                let name = strings.get(u32_at(b, body + 4)?)?;
                if name == "manifest" {
                    let attr_start = u16_at(b, body + 8)?;
                    let attr_size = u16_at(b, body + 10)?;
                    let count = u16_at(b, body + 12)?;
                    for k in 0..count {
                        let a = body + attr_start + k * attr_size;
                        if strings.get(u32_at(b, a + 4)?).map(String::as_str) == Some("package") {
                            let raw = u32_at(b, a + 8)?;
                            return strings.get(raw).cloned();
                        }
                    }
                    return None;
                }
            }
            _ => {}
        }
        pos += size;
    }
    None
}

/// The package name of an APK file on disk.
pub fn apk_package(apk: &Path) -> Result<String, String> {
    let file = std::fs::File::open(apk).map_err(|e| format!("Couldn't open {}: {e}", apk.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{} isn't a valid APK: {e}", apk.display()))?;
    let mut entry = zip
        .by_name("AndroidManifest.xml")
        .map_err(|_| format!("{} has no AndroidManifest.xml.", apk.display()))?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    manifest_package(&bytes).ok_or_else(|| "Couldn't read the package name from the APK's manifest.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal binary manifest: `<manifest package="com.example.game">`.
    fn build(utf8: bool) -> Vec<u8> {
        let strings = ["package", "manifest", "com.example.game"];
        // String pool.
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for s in strings {
            offsets.push(data.len() as u32);
            if utf8 {
                data.push(s.len() as u8);
                data.push(s.len() as u8);
                data.extend_from_slice(s.as_bytes());
                data.push(0);
            } else {
                data.extend_from_slice(&(s.len() as u16).to_le_bytes());
                for u in s.encode_utf16() {
                    data.extend_from_slice(&u.to_le_bytes());
                }
                data.extend_from_slice(&[0, 0]);
            }
        }
        while data.len() % 4 != 0 {
            data.push(0);
        }
        let pool_header = 28;
        let mut pool = Vec::new();
        pool.extend_from_slice(&(STRING_POOL as u16).to_le_bytes());
        pool.extend_from_slice(&(pool_header as u16).to_le_bytes());
        let pool_size = pool_header + offsets.len() * 4 + data.len();
        pool.extend_from_slice(&(pool_size as u32).to_le_bytes());
        pool.extend_from_slice(&(strings.len() as u32).to_le_bytes());
        pool.extend_from_slice(&0u32.to_le_bytes()); // styles
        pool.extend_from_slice(&(if utf8 { UTF8_FLAG as u32 } else { 0 }).to_le_bytes());
        pool.extend_from_slice(&((pool_header + offsets.len() * 4) as u32).to_le_bytes());
        pool.extend_from_slice(&0u32.to_le_bytes());
        for o in offsets {
            pool.extend_from_slice(&o.to_le_bytes());
        }
        pool.extend_from_slice(&data);

        // <manifest package="...">: header 16 bytes, body 20 bytes, one 20-byte attribute.
        let mut el = Vec::new();
        el.extend_from_slice(&(START_ELEMENT as u16).to_le_bytes());
        el.extend_from_slice(&16u16.to_le_bytes());
        el.extend_from_slice(&(16u32 + 20 + 20).to_le_bytes());
        el.extend_from_slice(&[0; 8]); // line, comment
        el.extend_from_slice(&u32::MAX.to_le_bytes()); // ns
        el.extend_from_slice(&1u32.to_le_bytes()); // name = "manifest"
        el.extend_from_slice(&20u16.to_le_bytes()); // attribute start
        el.extend_from_slice(&20u16.to_le_bytes()); // attribute size
        el.extend_from_slice(&1u16.to_le_bytes()); // count
        el.extend_from_slice(&[0; 6]);
        el.extend_from_slice(&u32::MAX.to_le_bytes()); // attr ns
        el.extend_from_slice(&0u32.to_le_bytes()); // attr name = "package"
        el.extend_from_slice(&2u32.to_le_bytes()); // raw value
        el.extend_from_slice(&[8, 0, 0, 3]);
        el.extend_from_slice(&2u32.to_le_bytes());

        let mut out = Vec::new();
        out.extend_from_slice(&(RES_XML as u16).to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&((8 + pool.len() + el.len()) as u32).to_le_bytes());
        out.extend_from_slice(&pool);
        out.extend_from_slice(&el);
        out
    }

    #[test]
    fn reads_the_package_in_both_string_encodings() {
        assert_eq!(manifest_package(&build(false)).as_deref(), Some("com.example.game"));
        assert_eq!(manifest_package(&build(true)).as_deref(), Some("com.example.game"));
        assert_eq!(manifest_package(b"<manifest/>"), None);
    }
}
