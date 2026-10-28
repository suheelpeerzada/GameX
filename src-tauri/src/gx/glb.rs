//! Writes simple meshes as GLB files, for readers whose engine stores meshes
//! in its own format. Input is already in GameX axes (right-handed, Y up)
//! with counter-clockwise front faces.

use std::io;

use serde_json::json;

/// One material: a colour, and optionally an image (a path relative to the
/// GLB file, so it's read from beside it).
#[derive(Debug, Clone, Default)]
pub struct Material {
    pub name: String,
    pub color: [f32; 4],
    pub texture: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Surface {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Triangles.
    pub indices: Vec<u32>,
    pub material: Material,
}

/// Normals averaged from the faces around each vertex.
fn smooth_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut out = vec![[0f32; 3]; positions.len()];
    for t in indices.chunks_exact(3) {
        let [a, b, c] = [positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]];
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &i in t {
            for k in 0..3 {
                out[i as usize][k] += n[k];
            }
        }
    }
    for n in out.iter_mut() {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        *n = if len > 0.0 { [n[0] / len, n[1] / len, n[2] / len] } else { [0.0, 1.0, 0.0] };
    }
    out
}

pub fn write(name: &str, surfaces: &[Surface]) -> io::Result<Vec<u8>> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut materials = Vec::new();
    let mut textures = Vec::new();
    let mut images: Vec<String> = Vec::new();
    let mut primitives = Vec::new();
    let mut view = |bin: &mut Vec<u8>, bytes: Vec<u8>| -> usize {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len() }));
        bin.extend_from_slice(&bytes);
        views.len() - 1
    };
    for s in surfaces {
        let n = s.positions.len();
        if n == 0 || s.indices.len() < 3 || s.indices.iter().any(|&i| i as usize >= n) {
            continue;
        }
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        let mut bytes = Vec::with_capacity(n * 12);
        for p in &s.positions {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
                bytes.extend_from_slice(&p[k].to_le_bytes());
            }
        }
        let v = view(&mut bin, bytes);
        accessors.push(json!({ "bufferView": v, "componentType": 5126, "count": n, "type": "VEC3", "min": min, "max": max }));
        let mut attributes = json!({ "POSITION": accessors.len() - 1 });
        let normals = if s.normals.len() == n { s.normals.clone() } else { smooth_normals(&s.positions, &s.indices) };
        {
            let v = view(&mut bin, normals.iter().flatten().flat_map(|c| c.to_le_bytes()).collect());
            accessors.push(json!({ "bufferView": v, "componentType": 5126, "count": n, "type": "VEC3" }));
            attributes["NORMAL"] = (accessors.len() - 1).into();
        }
        if s.uvs.len() == n {
            let v = view(&mut bin, s.uvs.iter().flatten().flat_map(|c| c.to_le_bytes()).collect());
            accessors.push(json!({ "bufferView": v, "componentType": 5126, "count": n, "type": "VEC2" }));
            attributes["TEXCOORD_0"] = (accessors.len() - 1).into();
        }
        let v = view(&mut bin, s.indices.iter().flat_map(|i| i.to_le_bytes()).collect());
        accessors.push(json!({ "bufferView": v, "componentType": 5125, "count": s.indices.len(), "type": "SCALAR" }));
        let mut pbr = json!({ "baseColorFactor": s.material.color, "metallicFactor": 0.0, "roughnessFactor": 0.85 });
        if let Some(t) = s.material.texture.as_ref().filter(|_| s.uvs.len() == n) {
            let image = images.iter().position(|i| i == t).unwrap_or_else(|| {
                images.push(t.clone());
                images.len() - 1
            });
            textures.push(json!({ "source": image }));
            pbr["baseColorTexture"] = json!({ "index": textures.len() - 1 });
        }
        materials.push(json!({ "name": s.material.name, "pbrMetallicRoughness": pbr }));
        primitives.push(json!({ "attributes": attributes, "indices": accessors.len() - 1, "material": materials.len() - 1 }));
    }
    if primitives.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "This mesh has no triangles to show."));
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let mut doc = json!({
        "asset": { "version": "2.0", "generator": "GameX" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0, "name": name }],
        "meshes": [{ "name": name, "primitives": primitives }],
        "materials": materials,
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": views,
        "accessors": accessors,
    });
    if !images.is_empty() {
        doc["images"] = images.iter().map(|uri| json!({ "uri": uri })).collect();
        doc["textures"] = textures.into();
        doc["samplers"] = json!([{}]);
    }
    let mut text = serde_json::to_vec(&doc).map_err(io::Error::other)?;
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let mut out = Vec::with_capacity(28 + text.len() + bin.len());
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + 8 + text.len() + 8 + bin.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}
