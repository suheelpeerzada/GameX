//! Unity's packed data files (`level0`, `sharedassets0.assets`...): reading
//! the object table and the meshes inside, for any Unity 2019.3+ game, with
//! or without type trees (Android builds strip them, so the Mesh layout is
//! known here instead). Files Unity split into `.split0`, `.split1`... parts
//! are read as one.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Unity's class ID for meshes.
pub const MESH: i32 = 43;

/// The marker in a virtual path naming one object inside a Unity file:
/// `apktool/assets/bin/Data/sharedassets2.assets@unity/-123/Garage.unitymesh`.
pub const VIRTUAL: &str = "@unity/";

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

// ---------------------------------------------------------------------------
// Files, whole or in parts

/// A Unity data file, read as one even when stored in `.splitN` parts.
pub struct DataFile {
    parts: Vec<(PathBuf, u64)>,
}

impl DataFile {
    /// `base` is the file without `.splitN`; whichever form exists is used.
    pub fn open(base: &Path) -> io::Result<DataFile> {
        if base.is_file() {
            return Ok(DataFile { parts: vec![(base.to_path_buf(), base.metadata()?.len())] });
        }
        let mut parts = Vec::new();
        for i in 0.. {
            let part = PathBuf::from(format!("{}.split{i}", base.display()));
            if !part.is_file() {
                break;
            }
            let size = part.metadata()?.len();
            parts.push((part, size));
        }
        if parts.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} wasn't found", base.display())));
        }
        Ok(DataFile { parts })
    }

    pub fn len(&self) -> u64 {
        self.parts.iter().map(|(_, s)| s).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn parts(&self) -> impl Iterator<Item = &Path> {
        self.parts.iter().map(|(p, _)| p.as_path())
    }

    /// `len` bytes from `offset`, across parts if needed.
    pub fn read_at(&self, mut offset: u64, len: usize) -> io::Result<Vec<u8>> {
        let mut out = Vec::with_capacity(len);
        for (path, size) in &self.parts {
            if out.len() == len {
                break;
            }
            if offset >= *size {
                offset -= size;
                continue;
            }
            let take = (len - out.len()).min((size - offset) as usize);
            let mut f = File::open(path)?;
            f.seek(SeekFrom::Start(offset))?;
            let start = out.len();
            out.resize(start + take, 0);
            f.read_exact(&mut out[start..])?;
            offset = 0;
        }
        if out.len() < len {
            return Err(bad("the file ends early"));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Reading values

pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub little: bool,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], little: bool) -> Self {
        Reader { data, pos: 0, little }
    }

    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len()).ok_or_else(|| bad("unexpected end of data"))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn arr<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut a: [u8; N] = self.take(N)?.try_into().unwrap();
        if !self.little {
            a.reverse();
        }
        Ok(a)
    }

    pub fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn i16(&mut self) -> io::Result<i16> {
        Ok(i16::from_le_bytes(self.arr()?))
    }
    pub fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    pub fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.arr()?))
    }
    pub fn i64(&mut self) -> io::Result<i64> {
        Ok(i64::from_le_bytes(self.arr()?))
    }
    pub fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    pub fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.arr()?))
    }
    pub fn align(&mut self) {
        self.pos = (self.pos + 3) & !3;
    }
    pub fn skip(&mut self, n: usize) -> io::Result<()> {
        self.take(n).map(|_| ())
    }
    pub fn cstr(&mut self) -> io::Result<String> {
        let rest = &self.data[self.pos..];
        let end = rest.iter().position(|&b| b == 0).ok_or_else(|| bad("unterminated text"))?;
        let s = String::from_utf8_lossy(&rest[..end]).into_owned();
        self.pos += end + 1;
        Ok(s)
    }
    /// A length-prefixed, 4-byte aligned string.
    pub fn string(&mut self) -> io::Result<String> {
        let n = self.count(1)?;
        let s = String::from_utf8_lossy(self.take(n)?).into_owned();
        self.align();
        Ok(s)
    }
    /// An array length, checked against what's left (each item `min_size` bytes or more).
    pub fn count(&mut self, min_size: usize) -> io::Result<usize> {
        let n = self.i32()?;
        if n < 0 || (n as usize).saturating_mul(min_size) > self.data.len() - self.pos {
            return Err(bad("bad array length"));
        }
        Ok(n as usize)
    }
    /// A length-prefixed, aligned byte array.
    pub fn bytes(&mut self) -> io::Result<&'a [u8]> {
        let n = self.count(1)?;
        let b = self.take(n)?;
        self.align();
        Ok(b)
    }
}

// ---------------------------------------------------------------------------
// The object table

#[derive(Debug, Clone)]
pub struct ObjectInfo {
    pub path_id: i64,
    /// Offset of the object's data in the (joined) file.
    pub offset: u64,
    pub size: u32,
    pub class_id: i32,
}

#[derive(Debug)]
pub struct SerializedFile {
    pub format: u32,
    pub unity_version: String,
    pub little: bool,
    pub type_trees: bool,
    pub objects: Vec<ObjectInfo>,
}

/// `2021.3.11f1` -> (2021, 3)
pub fn version_of(text: &str) -> (u32, u32) {
    let mut parts = text.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

impl SerializedFile {
    /// Reads the header and object table. Errors if this isn't a Unity
    /// serialized file (a bundle, audio, or anything else).
    pub fn read(file: &DataFile) -> io::Result<SerializedFile> {
        let total = file.len();
        if total < 48 {
            return Err(bad("too small to be a Unity data file"));
        }
        let head = file.read_at(0, 48)?;
        let mut h = Reader::new(&head, false);
        let mut metadata_size = h.u32()? as u64;
        let mut file_size = h.u32()? as u64;
        let format = h.u32()?;
        let mut data_offset = h.u32()? as u64;
        if !(9..=40).contains(&format) {
            return Err(bad("not a Unity data file"));
        }
        let little = h.u8()? == 0;
        h.skip(3)?;
        if format >= 22 {
            metadata_size = h.u32()? as u64;
            file_size = h.u64()?;
            data_offset = h.u64()?;
            h.skip(8)?;
        }
        if file_size != total || data_offset > total || metadata_size > total {
            return Err(bad("not a Unity data file"));
        }
        let meta = file.read_at(h.pos as u64, metadata_size as usize)?;
        let mut r = Reader::new(&meta, little);
        let unity_version = r.cstr()?;
        r.i32()?; // target platform
        let type_trees = r.u8()? != 0;
        if type_trees {
            // Type trees describe the layout; the layouts known here are used
            // either way, so only the class IDs are needed. Skipping a tree
            // means reading it, which isn't done yet.
            return Self::read_with_trees(unity_version, format, little, data_offset, &meta, r.pos);
        }
        let mut types = Vec::new();
        for _ in 0..r.count(23)? {
            let class_id = r.i32()?;
            r.u8()?; // stripped
            r.i16()?; // script type index
            if class_id == 114 {
                r.skip(16)?;
            }
            r.skip(16)?;
            types.push(class_id);
        }
        let objects = read_objects(&mut r, format, data_offset, &types)?;
        Ok(SerializedFile { format, unity_version, little, type_trees, objects })
    }

    fn read_with_trees(
        unity_version: String,
        format: u32,
        little: bool,
        data_offset: u64,
        meta: &[u8],
        pos: usize,
    ) -> io::Result<SerializedFile> {
        let mut r = Reader { data: meta, pos, little };
        let mut types = Vec::new();
        for _ in 0..r.count(23)? {
            let class_id = r.i32()?;
            r.u8()?;
            let script = r.i16()?;
            if class_id == 114 || (format >= 17 && script >= 0 && class_id < 0) {
                r.skip(16)?;
            }
            r.skip(16)?;
            // Blob type tree: node count, string buffer size, nodes, strings.
            let nodes = r.count(1)?;
            let strings = r.count(1)?;
            let node_size = if format >= 19 { 32 } else { 24 };
            r.skip(nodes * node_size + strings)?;
            if format >= 21 {
                let deps = r.count(4)?;
                r.skip(deps * 4)?;
            }
            types.push(class_id);
        }
        let objects = read_objects(&mut r, format, data_offset, &types)?;
        Ok(SerializedFile { format, unity_version, little, type_trees: true, objects })
    }

    pub fn version(&self) -> (u32, u32) {
        version_of(&self.unity_version)
    }
}

fn read_objects(r: &mut Reader, format: u32, data_offset: u64, types: &[i32]) -> io::Result<Vec<ObjectInfo>> {
    let mut objects = Vec::new();
    for _ in 0..r.count(20)? {
        if format >= 14 {
            r.align();
        }
        let path_id = if format >= 14 { r.i64()? } else { r.i32()? as i64 };
        let start = if format >= 22 { r.u64()? } else { r.u32()? as u64 };
        let size = r.u32()?;
        let type_index = r.i32()?;
        let class_id = if format >= 16 {
            *types.get(type_index as usize).ok_or_else(|| bad("bad type index"))?
        } else {
            type_index
        };
        if format < 16 {
            r.skip(2)?;
        }
        if (11..=16).contains(&format) {
            r.skip(2)?;
        }
        if (15..=16).contains(&format) {
            r.skip(1)?;
        }
        objects.push(ObjectInfo { path_id, offset: data_offset + start, size, class_id });
    }
    Ok(objects)
}

// ---------------------------------------------------------------------------
// Meshes

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Channel {
    pub stream: u8,
    pub offset: u8,
    pub format: u8,
    /// Components (1-4).
    pub dimension: u8,
}

#[derive(Debug, Clone)]
pub struct SubMesh {
    pub first_byte: u32,
    pub index_count: u32,
    /// 0 triangles, 2 quads, 3 lines, 4 line strip, 5 points.
    pub topology: i32,
    pub base_vertex: u32,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub name: String,
    pub sub_meshes: Vec<SubMesh>,
    pub bones: usize,
    pub blend_shapes: usize,
    pub compressed: bool,
    /// 0: 16-bit indices, 1: 32-bit.
    pub index_format: i32,
    pub index_buffer: Vec<u8>,
    pub vertex_count: u32,
    pub channels: Vec<Channel>,
    pub vertex_data: Vec<u8>,
    /// Vertex data kept in another file (`.resS`): offset, size, path.
    pub stream: Option<(u64, u32, String)>,
}

/// Meshes in files from these Unity versions can be read.
pub fn mesh_supported(version: (u32, u32)) -> bool {
    version >= (2019, 3)
}

/// Just the name, from an object's first bytes.
pub fn object_name(data: &[u8], little: bool) -> Option<String> {
    let mut r = Reader::new(data, little);
    let n = r.i32().ok()?;
    if !(0..=256).contains(&n) || (n as usize) > data.len() - 4 {
        return None;
    }
    Some(String::from_utf8_lossy(&data[4..4 + n as usize]).into_owned())
}

impl Mesh {
    /// Reads a Mesh object (Unity 2019.3 and later).
    pub fn read(data: &[u8], little: bool, version: (u32, u32)) -> io::Result<Mesh> {
        if !mesh_supported(version) {
            return Err(bad(format!(
                "Meshes from Unity {}.{} can't be read yet (2019.3 and later can).",
                version.0, version.1
            )));
        }
        let mut r = Reader::new(data, little);
        let name = r.string()?;
        let mut sub_meshes = Vec::new();
        for _ in 0..r.count(48)? {
            let first_byte = r.u32()?;
            let index_count = r.u32()?;
            let topology = r.i32()?;
            let base_vertex = r.u32()?;
            r.skip(4 + 4 + 24)?; // first vertex, vertex count, bounds
            sub_meshes.push(SubMesh { first_byte, index_count, topology, base_vertex });
        }
        // Blend shapes.
        let shape_vertices = r.count(40)?;
        r.skip(shape_vertices * 40)?;
        let blend_shapes = r.count(12)?;
        r.skip(blend_shapes * 12)?;
        for _ in 0..r.count(16)? {
            r.string()?;
            r.skip(12)?;
        }
        let weights = r.count(4)?;
        r.skip(weights * 4)?;
        // Bones.
        let bones = r.count(64)?;
        r.skip(bones * 64)?;
        let hashes = r.count(4)?;
        r.skip(hashes * 4)?;
        r.u32()?; // root bone hash
        let aabbs = r.count(24)?;
        r.skip(aabbs * 24)?;
        let variable = r.count(4)?;
        r.skip(variable * 4)?;

        let compression = r.u8()?;
        r.skip(3)?; // readable, keep vertices, keep indices
        r.align();
        let index_format = r.i32()?;
        let index_buffer = r.bytes()?.to_vec();
        let vertex_count = r.u32()?;
        let mut channels = Vec::new();
        for _ in 0..r.count(4)? {
            let (stream, offset, format, dimension) = (r.u8()?, r.u8()?, r.u8()?, r.u8()? & 0x0F);
            channels.push(Channel { stream, offset, format, dimension });
        }
        let vertex_data = r.bytes()?.to_vec();
        // Compressed mesh: 9 packed vectors, then UV info.
        let mut compressed = false;
        for kind in [0u8, 0, 0, 0, 1, 1, 1, 0, 1] {
            let items = r.u32()?;
            compressed |= items > 0;
            if kind == 0 {
                r.f32()?;
                r.f32()?;
            }
            r.bytes()?;
            r.u8()?;
            r.align();
        }
        let triangles = r.u32()?;
        compressed |= triangles > 0;
        r.bytes()?;
        r.u8()?;
        r.align();
        r.u32()?; // UV info
        r.skip(24)?; // bounds
        r.i32()?; // usage flags
        r.bytes()?; // baked convex collision mesh
        r.bytes()?; // baked triangle collision mesh
        r.skip(8)?; // metrics
        let offset = if version >= (2020, 1) { r.u64()? } else { r.u32()? as u64 };
        let size = r.u32()?;
        let path = r.string()?;
        let stream = (!path.is_empty() && size > 0).then_some((offset, size, path));
        Ok(Mesh {
            name,
            sub_meshes,
            bones,
            blend_shapes,
            compressed: compressed || compression != 0 && vertex_data.is_empty(),
            index_format,
            index_buffer,
            vertex_count,
            channels,
            vertex_data,
            stream,
        })
    }

    /// Byte size of one component of a vertex format.
    fn format_size(format: u8) -> usize {
        match format {
            0 | 10 | 11 => 4,
            1 | 4 | 5 | 8 | 9 => 2,
            _ => 1,
        }
    }

    /// Where each stream starts and its stride.
    fn streams(&self) -> Vec<(usize, usize)> {
        let count = self.channels.iter().filter(|c| c.dimension > 0).map(|c| c.stream as usize + 1).max().unwrap_or(0);
        let mut out = Vec::new();
        let mut offset = 0;
        for s in 0..count {
            let stride: usize = self
                .channels
                .iter()
                .filter(|c| c.dimension > 0 && c.stream as usize == s)
                .map(|c| Self::format_size(c.format) * c.dimension as usize)
                .sum();
            out.push((offset, stride));
            offset += self.vertex_count as usize * stride;
            offset = (offset + 15) & !15;
        }
        out
    }

    /// One vertex channel as floats (`dimension` per vertex), if present and numeric.
    pub fn channel(&self, index: usize) -> Option<(Vec<f32>, usize)> {
        let c = *self.channels.get(index).filter(|c| c.dimension > 0)?;
        let (start, stride) = *self.streams().get(c.stream as usize)?;
        let dim = c.dimension as usize;
        let size = Self::format_size(c.format);
        let mut out = Vec::with_capacity(self.vertex_count as usize * dim);
        for v in 0..self.vertex_count as usize {
            let base = start + v * stride + c.offset as usize;
            for k in 0..dim {
                let at = base + k * size;
                let b = self.vertex_data.get(at..at + size)?;
                out.push(match c.format {
                    0 => f32::from_le_bytes(b.try_into().ok()?),
                    1 => half(u16::from_le_bytes(b.try_into().ok()?)),
                    2 => b[0] as f32 / 255.0,
                    3 => (b[0] as i8 as f32 / 127.0).max(-1.0),
                    4 => u16::from_le_bytes(b.try_into().ok()?) as f32 / 65535.0,
                    5 => (i16::from_le_bytes(b.try_into().ok()?) as f32 / 32767.0).max(-1.0),
                    _ => return None,
                });
            }
        }
        Some((out, dim))
    }

    /// Triangle indices of one sub-mesh (quads split in two). Lines and
    /// points are skipped.
    pub fn triangles(&self, sub: &SubMesh) -> Vec<u32> {
        let wide = self.index_format == 1;
        let size = if wide { 4 } else { 2 };
        let start = sub.first_byte as usize;
        let read = |i: usize| -> Option<u32> {
            let at = start + i * size;
            let b = self.index_buffer.get(at..at + size)?;
            Some(if wide { u32::from_le_bytes(b.try_into().ok()?) } else { u16::from_le_bytes(b.try_into().ok()?) as u32 } + sub.base_vertex)
        };
        let n = sub.index_count as usize;
        let list: Vec<u32> = (0..n).map_while(read).collect();
        match sub.topology {
            0 => list,
            2 => list.as_chunks::<4>().0.iter().flat_map(|q| [q[0], q[1], q[2], q[0], q[2], q[3]]).collect(),
            _ => Vec::new(),
        }
    }
}

/// IEEE half float to f32.
fn half(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1F) as i32;
    let frac = (h & 0x3FF) as f32;
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

// ---------------------------------------------------------------------------
// Meshes as GLB, for the 3D view and export

/// Writes a mesh as a GLB file. Unity is left-handed with V going up, glTF
/// right-handed with V going down: X is mirrored, windings and V flipped.
pub fn mesh_to_glb(mesh: &Mesh) -> io::Result<Vec<u8>> {
    if mesh.compressed {
        return Err(bad("This mesh uses Unity's mesh compression, which GameX can't read yet."));
    }
    let (positions, dim) = mesh.channel(0).ok_or_else(|| bad("This mesh has no vertex positions GameX can read."))?;
    if dim < 3 {
        return Err(bad("This mesh's positions aren't 3D."));
    }
    let n = mesh.vertex_count as usize;
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut push_view = |bin: &mut Vec<u8>, bytes: &[u8]| -> usize {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(serde_json::json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len() }));
        bin.extend_from_slice(bytes);
        views.len() - 1
    };

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut pos_bytes = Vec::with_capacity(n * 12);
    for v in 0..n {
        let p = [-positions[v * dim], positions[v * dim + 1], positions[v * dim + 2]];
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
            pos_bytes.extend_from_slice(&p[k].to_le_bytes());
        }
    }
    if n == 0 {
        min = [0.0; 3];
        max = [0.0; 3];
    }
    let view = push_view(&mut bin, &pos_bytes);
    accessors.push(serde_json::json!({ "bufferView": view, "componentType": 5126, "count": n, "type": "VEC3", "min": min, "max": max }));
    let mut attributes = serde_json::json!({ "POSITION": 0 });

    if let Some((normals, d)) = mesh.channel(1).filter(|(_, d)| *d >= 3) {
        let mut bytes = Vec::with_capacity(n * 12);
        for v in 0..n {
            let (x, y, z) = (-normals[v * d], normals[v * d + 1], normals[v * d + 2]);
            let len = (x * x + y * y + z * z).sqrt();
            let s = if len > 0.0 { 1.0 / len } else { 0.0 };
            for c in [x * s, y * s, z * s] {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        let view = push_view(&mut bin, &bytes);
        accessors.push(serde_json::json!({ "bufferView": view, "componentType": 5126, "count": n, "type": "VEC3" }));
        attributes["NORMAL"] = (accessors.len() - 1).into();
    }
    if let Some((uvs, d)) = mesh.channel(4).filter(|(_, d)| *d >= 2) {
        let mut bytes = Vec::with_capacity(n * 8);
        for v in 0..n {
            bytes.extend_from_slice(&uvs[v * d].to_le_bytes());
            bytes.extend_from_slice(&(1.0 - uvs[v * d + 1]).to_le_bytes());
        }
        let view = push_view(&mut bin, &bytes);
        accessors.push(serde_json::json!({ "bufferView": view, "componentType": 5126, "count": n, "type": "VEC2" }));
        attributes["TEXCOORD_0"] = (accessors.len() - 1).into();
    }

    let mut primitives = Vec::new();
    let mut materials = Vec::new();
    for (i, sub) in mesh.sub_meshes.iter().enumerate() {
        let tris = mesh.triangles(sub);
        if tris.is_empty() {
            continue;
        }
        let mut bytes = Vec::with_capacity(tris.len() * 4);
        for t in tris.as_chunks::<3>().0 {
            // Mirroring X flips the winding; swap two corners back.
            for idx in [t[0], t[2], t[1]] {
                if idx as usize >= n {
                    return Err(bad("This mesh refers to vertices it doesn't have."));
                }
                bytes.extend_from_slice(&idx.to_le_bytes());
            }
        }
        let view = push_view(&mut bin, &bytes);
        accessors.push(serde_json::json!({ "bufferView": view, "componentType": 5125, "count": bytes.len() / 4, "type": "SCALAR" }));
        // A different shade per sub-mesh, so the parts can be told apart.
        let shade = 0.55 + 0.35 * ((i * 37 % 10) as f32 / 10.0);
        materials.push(serde_json::json!({
            "name": format!("{} part {}", mesh.name, i + 1),
            "pbrMetallicRoughness": { "baseColorFactor": [shade, shade * 0.95, shade * 0.9, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.8 },
        }));
        primitives.push(serde_json::json!({ "attributes": attributes, "indices": accessors.len() - 1, "material": materials.len() - 1 }));
    }
    if primitives.is_empty() {
        return Err(bad("This mesh has no triangles to show."));
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let json = serde_json::json!({
        "asset": { "version": "2.0", "generator": "GameX" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0, "name": mesh.name }],
        "meshes": [{ "name": mesh.name, "primitives": primitives }],
        "materials": materials,
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": views,
        "accessors": accessors,
    });
    let mut json = serde_json::to_vec(&json).map_err(io::Error::other)?;
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut out = Vec::with_capacity(28 + json.len() + bin.len());
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + 8 + json.len() + 8 + bin.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Finding meshes in a game, and opening one by its virtual path

/// A mesh found in a Unity file.
#[derive(Debug, Clone)]
pub struct FoundMesh {
    /// Virtual path, relative to `unpacked/`.
    pub path: String,
    pub name: String,
    pub size: u32,
}

/// `apktool/.../sharedassets2.assets` + object -> its virtual path.
pub fn virtual_path(file: &str, path_id: i64, name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || " ._-()".contains(c) { c } else { '_' })
        .collect();
    let clean = if clean.trim().is_empty() { "mesh".to_string() } else { clean.trim().to_string() };
    format!("{file}{VIRTUAL}{path_id}/{clean}.unitymesh")
}

/// The Unity file and object a virtual path names.
pub fn parse_virtual(path: &str) -> Option<(&str, i64)> {
    let (file, rest) = path.split_once(VIRTUAL)?;
    let id = rest.split('/').next()?.parse().ok()?;
    Some((file, id))
}

/// Every mesh in one Unity file (`rel` relative to `unpacked`, without `.splitN`).
pub fn find_meshes(unpacked: &Path, rel: &str) -> io::Result<Vec<FoundMesh>> {
    let file = DataFile::open(&unpacked.join(rel))?;
    let sf = SerializedFile::read(&file)?;
    let mut out = Vec::new();
    for o in sf.objects.iter().filter(|o| o.class_id == MESH) {
        let head = file.read_at(o.offset, (o.size as usize).min(300))?;
        let name = object_name(&head, sf.little).unwrap_or_default();
        out.push(FoundMesh { path: virtual_path(rel, o.path_id, &name), name, size: o.size });
    }
    Ok(out)
}

/// Reads the mesh a virtual path names. `open` maps a path relative to
/// `unpacked/` to the file to read (the user's edit, if any).
pub fn read_mesh(unpacked: &Path, virtual_rel: &str) -> io::Result<Mesh> {
    let (rel, path_id) = parse_virtual(virtual_rel).ok_or_else(|| bad("not a Unity object path"))?;
    if !crate::edits::is_plain_relative(rel) {
        return Err(bad("invalid file path"));
    }
    let file = DataFile::open(&unpacked.join(rel))?;
    let sf = SerializedFile::read(&file)?;
    let o = sf.objects.iter().find(|o| o.path_id == path_id).ok_or_else(|| bad("That object isn't in the file any more."))?;
    if o.class_id != MESH {
        return Err(bad("That object isn't a mesh."));
    }
    let data = file.read_at(o.offset, o.size as usize)?;
    let mut mesh = Mesh::read(&data, sf.little, sf.version())?;
    if mesh.vertex_data.is_empty() {
        if let Some((offset, size, path)) = &mesh.stream {
            // `archive:/CAB-x/CAB-x.resS` or `sharedassets0.assets.resS`: next to the file.
            let name = path.rsplit('/').next().unwrap_or(path);
            let dir = Path::new(rel).parent().unwrap_or(Path::new(""));
            let stream = DataFile::open(&unpacked.join(dir).join(name))?;
            mesh.vertex_data = stream.read_at(*offset, *size as usize)?;
        }
    }
    Ok(mesh)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Little-endian writer for building test files.
    #[derive(Default)]
    pub struct W(pub Vec<u8>);
    impl W {
        pub fn u8(&mut self, v: u8) -> &mut Self {
            self.0.push(v);
            self
        }
        pub fn u32(&mut self, v: u32) -> &mut Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn i32(&mut self, v: i32) -> &mut Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn f32(&mut self, v: f32) -> &mut Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn align(&mut self) -> &mut Self {
            while !self.0.len().is_multiple_of(4) {
                self.0.push(0);
            }
            self
        }
        pub fn string(&mut self, s: &str) -> &mut Self {
            self.i32(s.len() as i32);
            self.0.extend_from_slice(s.as_bytes());
            self.align()
        }
        pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
            self.i32(b.len() as i32);
            self.0.extend_from_slice(b);
            self.align()
        }
    }

    /// A quad (two triangles) as a 2021.3 Mesh object: float positions, half normals.
    pub fn quad_mesh(name: &str) -> Vec<u8> {
        let mut w = W::default();
        w.string(name);
        w.i32(1).u32(0).u32(6).i32(0).u32(0).u32(0).u32(4);
        for _ in 0..6 {
            w.f32(0.0);
        }
        w.i32(0).i32(0).i32(0).i32(0); // blend shapes
        w.i32(0).i32(0).u32(0).i32(0).i32(0); // bones
        w.u8(0).u8(1).u8(1).u8(1).align();
        w.i32(0); // 16-bit indices
        let idx: Vec<u8> = [0u16, 1, 2, 0, 2, 3].iter().flat_map(|i| i.to_le_bytes()).collect();
        w.bytes(&idx);
        w.u32(4);
        // position: float x3 at 0; normal: half x4 at 12 (dimension byte with flags, like Unity writes)
        w.i32(14);
        w.u8(0).u8(0).u8(0).u8(3);
        w.u8(0).u8(12).u8(1).u8(0x34);
        for _ in 2..14 {
            w.u8(0).u8(0).u8(0).u8(0);
        }
        let mut vd = Vec::new();
        for (x, y) in [(0.0f32, 0.0f32), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            for c in [x, y, 0.0f32] {
                vd.extend_from_slice(&c.to_le_bytes());
            }
            // normal (0,0,-1,0) as halves
            for h in [0u16, 0, 0xBC00, 0] {
                vd.extend_from_slice(&h.to_le_bytes());
            }
        }
        w.bytes(&vd);
        for kind in [0u8, 0, 0, 0, 1, 1, 1, 0, 1] {
            w.u32(0);
            if kind == 0 {
                w.f32(0.0).f32(0.0);
            }
            w.bytes(&[]).u8(0).align();
        }
        w.u32(0).bytes(&[]).u8(0).align(); // triangles
        w.u32(0);
        for _ in 0..6 {
            w.f32(0.0);
        }
        w.i32(0).bytes(&[]).bytes(&[]).f32(0.0).f32(0.0);
        w.0.extend_from_slice(&0u64.to_le_bytes());
        w.u32(0).string("");
        w.0
    }

    /// A format-22 serialized file holding these objects (class ID, data).
    pub fn serialized_file(objects: &[(i64, i32, Vec<u8>)]) -> Vec<u8> {
        let mut meta = W::default();
        meta.0.extend_from_slice(b"2021.3.11f1\0");
        meta.i32(13).u8(0);
        let classes: Vec<i32> = objects.iter().map(|o| o.1).fold(Vec::new(), |mut v, c| {
            if !v.contains(&c) {
                v.push(c);
            }
            v
        });
        meta.i32(classes.len() as i32);
        for c in &classes {
            meta.i32(*c).u8(0);
            meta.0.extend_from_slice(&(-1i16).to_le_bytes());
            meta.0.extend_from_slice(&[0; 16]);
        }
        meta.i32(objects.len() as i32);
        let header_len = 48usize;
        // Object table entries are 4-aligned relative to the metadata start.
        let mut table = Vec::new();
        let mut data = Vec::new();
        for (id, class, bytes) in objects {
            while data.len() % 8 != 0 {
                data.push(0);
            }
            table.push((*id, data.len() as u64, bytes.len() as u32, classes.iter().position(|c| c == class).unwrap() as i32));
            data.extend_from_slice(bytes);
        }
        for (id, start, size, ti) in table {
            meta.align();
            meta.0.extend_from_slice(&id.to_le_bytes());
            meta.0.extend_from_slice(&start.to_le_bytes());
            meta.u32(size).i32(ti);
        }
        meta.i32(0).i32(0).i32(0).u8(0); // script types, externals, ref types, user info
        let mut data_offset = header_len + meta.0.len();
        data_offset = (data_offset + 15) & !15;
        let total = data_offset + data.len();
        let mut out = Vec::new();
        out.extend_from_slice(&[0u8; 8]);
        out.extend_from_slice(&22u32.to_be_bytes());
        out.extend_from_slice(&[0u8; 4]);
        out.extend_from_slice(&[0, 0, 0, 0]); // little endian + reserved
        out.extend_from_slice(&(meta.0.len() as u32).to_be_bytes());
        out.extend_from_slice(&(total as u64).to_be_bytes());
        out.extend_from_slice(&(data_offset as u64).to_be_bytes());
        out.extend_from_slice(&[0u8; 8]);
        out.extend_from_slice(&meta.0);
        out.resize(data_offset, 0);
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn reads_split_files_as_one() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("sharedassets0.assets");
        std::fs::write(dir.path().join("sharedassets0.assets.split0"), b"abcd").unwrap();
        std::fs::write(dir.path().join("sharedassets0.assets.split1"), b"efgh").unwrap();
        std::fs::write(dir.path().join("sharedassets0.assets.split2"), b"ij").unwrap();
        let f = DataFile::open(&base).unwrap();
        assert_eq!(f.len(), 10);
        assert_eq!(f.read_at(2, 7).unwrap(), b"cdefghi");
        assert!(f.read_at(8, 3).is_err());
        assert!(DataFile::open(&dir.path().join("level0")).is_err());
    }

    #[test]
    fn finds_and_reads_meshes() {
        let dir = tempfile::tempdir().unwrap();
        let file = serialized_file(&[(1, 1, vec![0; 12]), (-77, MESH, quad_mesh("Door/Big")), (5, 28, vec![1; 9])]);
        let (a, b) = file.split_at(100);
        std::fs::write(dir.path().join("level1.split0"), a).unwrap();
        std::fs::write(dir.path().join("level1.split1"), b).unwrap();

        let found = find_meshes(dir.path(), "level1").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Door/Big");
        assert_eq!(found[0].path, "level1@unity/-77/Door_Big.unitymesh");
        assert_eq!(parse_virtual(&found[0].path), Some(("level1", -77)));

        let mesh = read_mesh(dir.path(), &found[0].path).unwrap();
        assert_eq!((mesh.vertex_count, mesh.sub_meshes.len(), mesh.compressed), (4, 1, false));
        let (normals, dim) = mesh.channel(1).unwrap();
        assert_eq!(dim, 4);
        assert_eq!(&normals[..4], &[0.0, 0.0, -1.0, 0.0]);
        assert_eq!(mesh.triangles(&mesh.sub_meshes[0]), [0, 1, 2, 0, 2, 3]);

        let glb = mesh_to_glb(&mesh).unwrap();
        assert!(glb.starts_with(b"glTF"));
        assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len());
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        // Mirrored on X.
        assert_eq!(json["accessors"][0]["min"], serde_json::json!([-1.0, 0.0, 0.0]));
        assert_eq!(json["accessors"][0]["max"], serde_json::json!([0.0, 1.0, 0.0]));
        assert_eq!(json["meshes"][0]["primitives"][0]["attributes"]["NORMAL"], 1);

        assert!(read_mesh(dir.path(), "level1@unity/5/x.unitymesh").unwrap_err().to_string().contains("isn't a mesh"));
        assert!(read_mesh(dir.path(), "../x@unity/5/x.unitymesh").is_err());
    }

    #[test]
    fn refuses_other_files_and_old_versions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.resource"), vec![7u8; 4000]).unwrap();
        assert!(SerializedFile::read(&DataFile::open(&dir.path().join("x.resource")).unwrap()).is_err());
        assert!(Mesh::read(&quad_mesh("a"), true, (2018, 4)).unwrap_err().to_string().contains("2018.4"));
        assert_eq!(version_of("2021.3.11f1"), (2021, 3));
        assert_eq!(half(0x3C00), 1.0);
        assert_eq!(half(0xC000), -2.0);
    }
}
