//! Universal media search: finds images and sounds inside any file by their
//! content signatures, whatever the engine or file name. Each candidate is
//! walked structurally (PNG chunks, JPEG segments, Ogg pages, MP3 frames...)
//! to its exact end, so random bytes that merely start like a signature are
//! not reported.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// "image" or "audio".
    pub kind: &'static str,
    /// "png", "jpeg", "gif", "bmp", "webp", "wav", "ogg", "mp3".
    pub format: &'static str,
    pub mime: &'static str,
    pub offset: usize,
    pub len: usize,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Stop after this many finds in one file.
pub const MAX_FINDS: usize = 2000;

fn u16_be(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize)
}
fn u16_le(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize)
}
fn u32_be(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize)
}
fn u32_le(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize)
}

/// Scans `bytes` and returns every complete media item found, in order.
pub fn scan(bytes: &[u8]) -> Vec<Found> {
    let mut found = Vec::new();
    let mut i = 0;
    while i + 4 <= bytes.len() && found.len() < MAX_FINDS {
        let hit = match bytes[i] {
            0x89 => png(bytes, i),
            0xFF => jpeg(bytes, i),
            b'G' => gif(bytes, i),
            b'B' => bmp(bytes, i),
            b'R' => riff(bytes, i),
            b'O' => ogg(bytes, i),
            b'I' => mp3(bytes, i),
            _ => None,
        };
        match hit {
            Some(f) => {
                i = f.offset + f.len;
                found.push(f);
            }
            None => i += 1,
        }
    }
    found
}

fn item(kind: &'static str, format: &'static str, mime: &'static str, offset: usize, len: usize) -> Found {
    Found { kind, format, mime, offset, len, width: None, height: None }
}

fn png(b: &[u8], i: usize) -> Option<Found> {
    if b.get(i..i + 8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut pos = i + 8;
    let (mut width, mut height) = (None, None);
    for n in 0.. {
        let len = u32_be(b, pos)?;
        let kind = b.get(pos + 4..pos + 8)?;
        if !kind.iter().all(u8::is_ascii_alphabetic) {
            return None;
        }
        if n == 0 {
            if kind != b"IHDR" {
                return None;
            }
            width = u32_be(b, pos + 8).map(|w| w as u32);
            height = u32_be(b, pos + 12).map(|h| h as u32);
        }
        pos = pos.checked_add(12 + len)?;
        if pos > b.len() {
            return None;
        }
        if kind == b"IEND" {
            break;
        }
    }
    Some(Found { width, height, ..item("image", "png", "image/png", i, pos - i) })
}

fn jpeg(b: &[u8], i: usize) -> Option<Found> {
    if b.get(i..i + 3)? != [0xFF, 0xD8, 0xFF] {
        return None;
    }
    let mut pos = i + 2;
    let mut size = None;
    loop {
        if *b.get(pos)? != 0xFF {
            return None;
        }
        let mut marker = *b.get(pos + 1)?;
        while marker == 0xFF {
            pos += 1;
            marker = *b.get(pos + 1)?;
        }
        match marker {
            0xD9 => {
                // End of image; only accept real images with a frame header.
                let end = pos + 2;
                let (h, w) = size?;
                return (end - i >= 100).then(|| Found { width: Some(w), height: Some(h), ..item("image", "jpeg", "image/jpeg", i, end - i) });
            }
            0xD0..=0xD7 | 0x01 => pos += 2,
            0xDA => {
                // Start of scan: skip the header, then the entropy-coded data
                // up to the next marker that isn't a stuffed byte or restart.
                pos += 2 + u16_be(b, pos + 2)?;
                loop {
                    let next = pos + b.get(pos..)?.iter().position(|&x| x == 0xFF)?;
                    let after = *b.get(next + 1)?;
                    if after == 0x00 || (0xD0..=0xD7).contains(&after) {
                        pos = next + 2;
                    } else {
                        pos = next;
                        break;
                    }
                }
            }
            _ => {
                let len = u16_be(b, pos + 2)?;
                if len < 2 {
                    return None;
                }
                // Frame headers (SOF0-SOF15, not DHT/JPG/DAC) carry the size.
                if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    size = Some((u16_be(b, pos + 5)? as u32, u16_be(b, pos + 7)? as u32));
                }
                pos += 2 + len;
            }
        }
    }
}

fn gif(b: &[u8], i: usize) -> Option<Found> {
    let sig = b.get(i..i + 6)?;
    if sig != b"GIF87a" && sig != b"GIF89a" {
        return None;
    }
    let width = u16_le(b, i + 6)? as u32;
    let height = u16_le(b, i + 8)? as u32;
    let flags = *b.get(i + 10)?;
    let mut pos = i + 13;
    if flags & 0x80 != 0 {
        pos += 3 << ((flags & 7) + 1);
    }
    let sub_blocks = |mut pos: usize| -> Option<usize> {
        loop {
            let size = *b.get(pos)? as usize;
            pos += 1;
            if size == 0 {
                return Some(pos);
            }
            pos += size;
        }
    };
    let mut frames = 0;
    loop {
        match *b.get(pos)? {
            0x21 => pos = sub_blocks(pos + 2)?,
            0x2C => {
                let local = *b.get(pos + 9)?;
                pos += 10;
                if local & 0x80 != 0 {
                    pos += 3 << ((local & 7) + 1);
                }
                pos = sub_blocks(pos + 1)?;
                frames += 1;
            }
            0x3B => {
                let end = pos + 1;
                return (frames > 0).then(|| Found {
                    width: Some(width),
                    height: Some(height),
                    ..item("image", "gif", "image/gif", i, end - i)
                });
            }
            _ => return None,
        }
    }
}

fn bmp(b: &[u8], i: usize) -> Option<Found> {
    if b.get(i..i + 2)? != b"BM" {
        return None;
    }
    let size = u32_le(b, i + 2)?;
    let reserved = u32_le(b, i + 6)?;
    let data = u32_le(b, i + 10)?;
    let header = u32_le(b, i + 14)?;
    let width = u32_le(b, i + 18)? as i32;
    let height = u32_le(b, i + 22)? as i32;
    let sane = reserved == 0
        && [12, 40, 52, 56, 108, 124].contains(&header)
        && size >= 54
        && data < size
        && i + size <= b.len()
        && (1..=16384).contains(&width)
        && (1..=16384).contains(&height.abs());
    sane.then(|| Found {
        width: Some(width as u32),
        height: Some(height.unsigned_abs()),
        ..item("image", "bmp", "image/bmp", i, size)
    })
}

fn riff(b: &[u8], i: usize) -> Option<Found> {
    if b.get(i..i + 4)? != b"RIFF" {
        return None;
    }
    let size = u32_le(b, i + 4)?;
    let len = size.checked_add(8)? + (size & 1);
    if size < 12 || i + len > b.len() {
        return None;
    }
    match b.get(i + 8..i + 12)? {
        b"WEBP" => Some(item("image", "webp", "image/webp", i, len)),
        b"WAVE" => Some(item("audio", "wav", "audio/wav", i, len)),
        _ => None,
    }
}

fn ogg(b: &[u8], i: usize) -> Option<Found> {
    // A stream starts with a "beginning of stream" page.
    if b.get(i..i + 4)? != b"OggS" || *b.get(i + 4)? != 0 || *b.get(i + 5)? & 0x02 == 0 {
        return None;
    }
    let mut pos = i;
    let mut pages = 0;
    while b.get(pos..pos + 4) == Some(b"OggS") && b.get(pos + 4) == Some(&0) {
        let segments = *b.get(pos + 26)? as usize;
        let table = b.get(pos + 27..pos + 27 + segments)?;
        let body: usize = table.iter().map(|&s| s as usize).sum();
        let end = pos + 27 + segments + body;
        if end > b.len() {
            break;
        }
        let last = b[pos + 5] & 0x04 != 0;
        pos = end;
        pages += 1;
        if last {
            break;
        }
    }
    (pages >= 2).then(|| item("audio", "ogg", "audio/ogg", i, pos - i))
}

/// MP3 with an ID3v2 tag (the tag makes the start unambiguous), walked frame
/// by frame through MPEG Layer III headers.
fn mp3(b: &[u8], i: usize) -> Option<Found> {
    if b.get(i..i + 3)? != b"ID3" || !(2..=4).contains(b.get(i + 3)?) {
        return None;
    }
    let raw = b.get(i + 6..i + 10)?;
    if raw.iter().any(|&x| x & 0x80 != 0) {
        return None;
    }
    let tag = raw.iter().fold(0usize, |acc, &x| (acc << 7) | x as usize);
    let footer = if b[i + 5] & 0x10 != 0 { 10 } else { 0 };
    let mut pos = i + 10 + tag + footer;
    let mut frames = 0;
    const MPEG1: [usize; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
    const MPEG2: [usize; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    while let Some(h) = b.get(pos..pos + 4) {
        if h[0] != 0xFF || h[1] & 0xE0 != 0xE0 {
            break;
        }
        let version = (h[1] >> 3) & 3; // 3 = MPEG1, 2 = MPEG2, 0 = MPEG2.5
        let layer = (h[1] >> 1) & 3; // 1 = Layer III
        let bitrate = (h[2] >> 4) as usize;
        let rate = ((h[2] >> 2) & 3) as usize;
        if version == 1 || layer != 1 || bitrate == 0 || bitrate == 15 || rate == 3 {
            break;
        }
        let padding = ((h[2] >> 1) & 1) as usize;
        let len = if version == 3 {
            144_000 * MPEG1[bitrate] / [44_100, 48_000, 32_000][rate] + padding
        } else {
            let rates = if version == 2 { [22_050, 24_000, 16_000] } else { [11_025, 12_000, 8_000] };
            72_000 * MPEG2[bitrate] / rates[rate] + padding
        };
        if len < 4 || pos + len > b.len() {
            break;
        }
        pos += len;
        frames += 1;
    }
    if b.get(pos..pos + 3) == Some(b"TAG") && pos + 128 <= b.len() {
        pos += 128;
    }
    (frames >= 10).then(|| item("audio", "mp3", "audio/mpeg", i, pos - i))
}

/// Files larger than this aren't scanned.
pub const MAX_SCAN_BYTES: u64 = 512 << 20;
/// A single item larger than this isn't read into the UI.
pub const MAX_ITEM_BYTES: usize = 64 << 20;

/// Scans one project file (relative to `unpacked/`, edits applied).
pub fn scan_file(project_root: &std::path::Path, rel: &str) -> std::io::Result<Vec<Found>> {
    let path = crate::preview::resolve(project_root, rel)?;
    if std::fs::metadata(&path)?.len() > MAX_SCAN_BYTES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "This file is too large to search."));
    }
    Ok(scan(&std::fs::read(path)?))
}

/// The bytes of one found item.
pub fn read_item(project_root: &std::path::Path, rel: &str, offset: usize, len: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let invalid = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, m.to_string());
    if len == 0 || len > MAX_ITEM_BYTES {
        return Err(invalid("That item is too large to open."));
    }
    let path = crate::preview::resolve(project_root, rel)?;
    let size = std::fs::metadata(&path)?.len() as usize;
    if offset.checked_add(len).is_none_or(|end| end > size) {
        return Err(invalid("That item isn't inside this file."));
    }
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset as u64))?;
    let mut buf = vec![0; len];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// A short summary: "12 images, 3 sounds".
pub fn summary(found: &[Found]) -> String {
    let images = found.iter().filter(|f| f.kind == "image").count();
    let sounds = found.len() - images;
    let mut parts = Vec::new();
    if images > 0 {
        parts.push(format!("{images} image{}", if images == 1 { "" } else { "s" }));
    }
    if sounds > 0 {
        parts.push(format!("{sounds} sound{}", if sounds == 1 { "" } else { "s" }));
    }
    parts.join(", ")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal valid PNG (1x1).
    pub fn png_bytes() -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend_from_slice(&13u32.to_be_bytes());
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        v.extend_from_slice(&[0; 4]); // crc
        v.extend_from_slice(&0u32.to_be_bytes());
        v.extend_from_slice(b"IEND");
        v.extend_from_slice(&[0; 4]);
        v
    }

    fn ogg_page(flags: u8, body: &[u8]) -> Vec<u8> {
        let mut p = b"OggS\0".to_vec();
        p.push(flags);
        p.extend_from_slice(&[0; 20]);
        p.push(1);
        p.push(body.len() as u8);
        p.extend_from_slice(body);
        p
    }

    fn wav_bytes() -> Vec<u8> {
        let mut v = b"RIFF".to_vec();
        v.extend_from_slice(&20u32.to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&[0; 12]);
        v
    }

    fn mp3_bytes() -> Vec<u8> {
        let mut v = b"ID3\x03\0\0\0\0\0\x05tagxx".to_vec();
        // MPEG1 Layer III, 128 kbps, 44.1 kHz: 417-byte frames.
        for _ in 0..12 {
            let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
            frame.resize(417, 0x11);
            v.extend_from_slice(&frame);
        }
        v
    }

    fn jpeg_bytes() -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        // SOF0 with 16x8 size.
        v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 8, 0x00, 0x08, 0x00, 0x10, 1, 1, 0x11, 0]);
        // SOS header, then data containing a stuffed FF00 and a restart marker.
        v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 1, 1, 0, 0, 0x3F, 0]);
        v.extend_from_slice(&[0x12; 60]);
        v.extend_from_slice(&[0xFF, 0x00, 0x34, 0xFF, 0xD0]);
        v.extend_from_slice(&[0x56; 40]);
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    fn gif_bytes() -> Vec<u8> {
        let mut v = b"GIF89a".to_vec();
        v.extend_from_slice(&[2, 0, 3, 0, 0x80, 0, 0]); // 2x3, 2-color table
        v.extend_from_slice(&[0; 6]);
        v.extend_from_slice(&[0x2C, 0, 0, 0, 0, 2, 0, 3, 0, 0, 2, 2, 0x4C, 0x01, 0]);
        v.push(0x3B);
        v
    }

    #[test]
    fn finds_media_inside_other_bytes() {
        let mut blob = b"header junk \x89PN not a png ".to_vec();
        let parts: Vec<(Vec<u8>, &str)> = vec![
            (png_bytes(), "png"),
            (jpeg_bytes(), "jpeg"),
            (gif_bytes(), "gif"),
            (wav_bytes(), "wav"),
            ([ogg_page(0x02, b"vorbis-id"), ogg_page(0x00, b"comments"), ogg_page(0x04, b"audio")].concat(), "ogg"),
            (mp3_bytes(), "mp3"),
        ];
        let mut expected = Vec::new();
        for (bytes, format) in &parts {
            expected.push((blob.len(), bytes.len(), *format));
            blob.extend_from_slice(bytes);
            blob.extend_from_slice(b" gap RIFF\x10\0\0\0XXXX ");
        }
        let found = scan(&blob);
        let got: Vec<_> = found.iter().map(|f| (f.offset, f.len, f.format)).collect();
        assert_eq!(got, expected);
        assert_eq!((found[0].width, found[0].height), (Some(1), Some(1)));
        assert_eq!((found[1].width, found[1].height), (Some(16), Some(8)));
        assert_eq!((found[2].width, found[2].height), (Some(2), Some(3)));
        assert_eq!(summary(&found), "3 images, 3 sounds");
    }

    #[test]
    fn rejects_truncated_and_lookalike_data() {
        let png = png_bytes();
        assert!(scan(&png[..png.len() - 6]).is_empty());
        assert!(scan(&[0xFF, 0xD8, 0xFF, 0xD9]).is_empty()); // no frame header
        assert!(scan(&ogg_page(0x00, b"middle of a stream")).is_empty()); // no BOS
        assert!(scan(b"ID3\x03\0\0\0\0\0\0").is_empty()); // tag with no frames
        assert!(scan(b"BM some text that starts like a bitmap...............................").is_empty());
    }
}
