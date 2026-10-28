//! Godot text scenes (`.tscn`): reading nodes and properties, and changing a
//! property in place. Only the lines of the changed property are rewritten,
//! so the rest of the file (formatting, comments, order) stays as it was.
//!
//! ```text
//! [gd_scene load_steps=3 format=3]
//! [ext_resource type="Texture2D" path="res://egg.png" id="1_a"]
//! [node name="Egg" type="Node2D"]
//! [node name="Sprite" type="Sprite2D" parent="."]
//! position = Vector2(10, 20)
//! texture = ExtResource("1_a")
//! ```

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneProperty {
    pub key: String,
    /// The value as written; lines of a multi-line value are joined with `\n`.
    pub value: String,
    /// 1-based first and last line of the property.
    pub line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneNode {
    /// "." for the root, otherwise the path from the root: "Player/Sprite".
    pub path: String,
    pub name: String,
    pub node_type: Option<String>,
    /// The parent's path, `None` for the root.
    pub parent: Option<String>,
    /// For an instanced scene: the ext_resource id it instances.
    pub instance: Option<String>,
    pub line: usize,
    pub properties: Vec<SceneProperty>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtResource {
    pub id: String,
    pub kind: String,
    /// `res://...`
    pub path: String,
    /// The file to preview for it, relative to `unpacked/`, if found.
    pub file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    pub nodes: Vec<SceneNode>,
    pub resources: Vec<ExtResource>,
}

/// One `[section attr=value ...]` with its properties.
pub(crate) struct Section {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub line: usize,
    pub properties: Vec<SceneProperty>,
}

impl Section {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// `[node name="A" parent="." instance=ExtResource("2")]` -> tag + attributes.
fn parse_header(line: &str) -> Option<(String, Vec<(String, String)>)> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (tag, mut rest) = inner.split_once(' ').unwrap_or((inner, ""));
    let mut attrs = Vec::new();
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim().to_string();
        let after = &rest[eq + 1..];
        let (value, consumed) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"')?;
            (quoted[..end].to_string(), end + 2)
        } else {
            // Unquoted: up to the next space outside brackets, e.g. ExtResource("1").
            let mut depth = 0i32;
            let mut in_string = false;
            let mut end = after.len();
            for (i, c) in after.char_indices() {
                match c {
                    '"' => in_string = !in_string,
                    '(' | '[' | '{' if !in_string => depth += 1,
                    ')' | ']' | '}' if !in_string => depth -= 1,
                    ' ' if depth == 0 && !in_string => {
                        end = i;
                        break;
                    }
                    _ => {}
                }
            }
            (after[..end].to_string(), end)
        };
        attrs.push((key, value));
        rest = &after[consumed.min(after.len())..];
    }
    Some((tag.to_string(), attrs))
}

/// Net open brackets on a line, ignoring those inside strings.
fn bracket_delta(line: &str) -> i32 {
    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;
    for c in line.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

pub(crate) fn parse_sections(text: &str) -> Vec<Section> {
    let lines: Vec<&str> = text.lines().collect();
    let mut sections: Vec<Section> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if let Some((tag, attrs)) = parse_header(trimmed) {
                sections.push(Section { tag, attrs, line: i + 1, properties: Vec::new() });
            }
            i += 1;
            continue;
        }
        if let (Some(section), Some((key, first))) = (sections.last_mut(), line.split_once(" = ")) {
            let start = i;
            let mut value = vec![first.to_string()];
            let mut depth = bracket_delta(first);
            while depth > 0 && i + 1 < lines.len() {
                i += 1;
                value.push(lines[i].to_string());
                depth += bracket_delta(lines[i]);
            }
            section.properties.push(SceneProperty {
                key: key.trim().to_string(),
                value: value.join("\n"),
                line: start + 1,
                end_line: i + 1,
            });
        }
        i += 1;
    }
    sections
}

/// The project folder a scene belongs to (holding project.godot or
/// project.binary), relative to `unpacked/`.
fn project_base(unpacked: &Path, scene: &str) -> String {
    let mut dir = Path::new(scene).parent();
    while let Some(d) = dir {
        let full = unpacked.join(d);
        if full.join("project.godot").is_file() || full.join("project.binary").is_file() {
            return d.to_string_lossy().replace('\\', "/");
        }
        dir = d.parent();
    }
    // No marker: assume the folder after the unpack step (files/, apktool/assets/).
    let parts: Vec<&str> = scene.split('/').collect();
    match parts.as_slice() {
        [layer, "assets", ..] if *layer != "files" => format!("{layer}/assets"),
        [layer, ..] => layer.to_string(),
        [] => String::new(),
    }
}

/// The file to preview for a `res://` path: the original if it's there,
/// otherwise the file its `.import` points to (a `.ctex` in exports).
fn resolve_res(unpacked: &Path, base: &str, res: &str) -> Option<String> {
    let rel = res.strip_prefix("res://")?;
    let join = |r: &str| if base.is_empty() { r.to_string() } else { format!("{base}/{r}") };
    let direct = join(rel);
    if unpacked.join(&direct).is_file() {
        return Some(direct);
    }
    let import = std::fs::read_to_string(unpacked.join(format!("{direct}.import"))).ok()?;
    import.lines().find_map(|l| {
        let value = l.trim().strip_prefix("path=")?.trim_matches('"');
        let target = join(value.strip_prefix("res://")?);
        unpacked.join(&target).is_file().then_some(target)
    })
}

/// Reads a scene. `rel` is relative to `unpacked/`; `text` is its content.
pub fn parse(unpacked: &Path, rel: &str, text: &str) -> Scene {
    let base = project_base(unpacked, rel);
    let mut nodes = Vec::new();
    let mut resources = Vec::new();
    for section in parse_sections(text) {
        match section.tag.as_str() {
            "ext_resource" => {
                let path = section.attr("path").unwrap_or_default().to_string();
                resources.push(ExtResource {
                    id: section.attr("id").unwrap_or_default().to_string(),
                    kind: section.attr("type").unwrap_or_default().to_string(),
                    file: resolve_res(unpacked, &base, &path),
                    path,
                });
            }
            "node" => {
                let name = section.attr("name").unwrap_or("Node").to_string();
                let parent = section.attr("parent").map(String::from);
                let path = match parent.as_deref() {
                    None => ".".to_string(),
                    Some(".") => name.clone(),
                    Some(p) => format!("{p}/{name}"),
                };
                let instance = section.attr("instance").and_then(|v| {
                    let inner = v.strip_prefix("ExtResource(")?.strip_suffix(')')?;
                    Some(inner.trim().trim_matches('"').to_string())
                });
                nodes.push(SceneNode {
                    path,
                    name,
                    node_type: section.attr("type").map(String::from),
                    parent,
                    instance,
                    line: section.line,
                    properties: section.properties,
                });
            }
            _ => {}
        }
    }
    Scene { nodes, resources }
}

/// Sets `key = value` on a node, replacing the existing property's lines or
/// adding it after the node's last property. Returns the new file text.
pub fn set_property(text: &str, node_path: &str, key: &str, value: &str) -> Result<String, String> {
    let key_ok = !key.is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | ':'));
    if !key_ok {
        return Err(format!("\"{key}\" isn't a valid property name."));
    }
    let value = value.trim();
    if value.is_empty() || value.contains('\n') || bracket_delta(value) != 0 {
        return Err("Enter a single-line value with balanced brackets.".into());
    }
    let scene = parse(Path::new(""), "", text);
    let node = scene
        .nodes
        .iter()
        .find(|n| n.path == node_path)
        .ok_or_else(|| format!("The scene has no node \"{node_path}\"."))?;

    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let new_line = format!("{key} = {value}");
    match node.properties.iter().find(|p| p.key == key) {
        Some(p) => {
            lines.splice(p.line - 1..p.end_line, [new_line]);
        }
        None => {
            let after = node.properties.last().map_or(node.line, |p| p.end_line);
            lines.insert(after, new_line);
        }
    }
    let mut out = lines.join(newline);
    if text.ends_with('\n') {
        out.push_str(newline);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &str = "[gd_scene load_steps=4 format=3 uid=\"uid://abc\"]\n\
\n\
[ext_resource type=\"Texture2D\" uid=\"uid://t\" path=\"res://art/egg.png\" id=\"1_egg\"]\n\
[ext_resource type=\"PackedScene\" path=\"res://enemy.tscn\" id=\"2_en\"]\n\
\n\
[sub_resource type=\"SpriteFrames\" id=\"SpriteFrames_1\"]\n\
animations = [{\n\
\"frames\": [],\n\
\"name\": &\"default\"\n\
}]\n\
\n\
[node name=\"Level\" type=\"Node2D\"]\n\
\n\
[node name=\"Egg\" type=\"Sprite2D\" parent=\".\"]\n\
position = Vector2(10, 20)\n\
texture = ExtResource(\"1_egg\")\n\
\n\
[node name=\"Shell\" type=\"Sprite2D\" parent=\"Egg\"]\n\
scale = Vector2(2, 2)\n\
\n\
[node name=\"Enemy\" parent=\".\" instance=ExtResource(\"2_en\")]\n\
position = Vector2(300, 40)\n\
\n\
[connection signal=\"body_entered\" from=\"Egg\" to=\".\" method=\"_hit\"]\n";

    #[test]
    fn reads_nodes_resources_and_multiline_values() {
        let dir = tempfile::tempdir().unwrap();
        let unpacked = dir.path();
        std::fs::create_dir_all(unpacked.join("files/game/art")).unwrap();
        std::fs::write(unpacked.join("files/game/project.godot"), "").unwrap();
        std::fs::write(unpacked.join("files/game/art/egg.png"), "png").unwrap();

        let scene = parse(unpacked, "files/game/levels/one.tscn", SCENE);
        let paths: Vec<_> = scene.nodes.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(paths, [".", "Egg", "Egg/Shell", "Enemy"]);
        let egg = &scene.nodes[1];
        assert_eq!(egg.node_type.as_deref(), Some("Sprite2D"));
        assert_eq!(egg.properties[0].value, "Vector2(10, 20)");
        assert_eq!(egg.properties[0].line, 15);
        assert_eq!(scene.nodes[3].instance.as_deref(), Some("2_en"));
        assert_eq!(scene.resources[0].file.as_deref(), Some("files/game/art/egg.png"));
        assert_eq!(scene.resources[1].file, None);

        // The multi-line sub_resource value doesn't leak into nodes.
        let sections = parse_sections(SCENE);
        let frames = &sections[3].properties[0];
        assert_eq!((frames.line, frames.end_line), (7, 10));
    }

    #[test]
    fn changes_only_the_property_lines() {
        let moved = set_property(SCENE, "Egg", "position", "Vector2(15.5, 20)").unwrap();
        assert!(moved.contains("[node name=\"Egg\" type=\"Sprite2D\" parent=\".\"]\nposition = Vector2(15.5, 20)\ntexture"));
        assert_eq!(moved.lines().count(), SCENE.lines().count());
        // Everything else is untouched.
        let changed: Vec<_> = SCENE.lines().zip(moved.lines()).filter(|(a, b)| a != b).collect();
        assert_eq!(changed.len(), 1);

        let added = set_property(SCENE, "Egg/Shell", "rotation", "0.5").unwrap();
        assert!(added.contains("scale = Vector2(2, 2)\nrotation = 0.5\n\n[node name=\"Enemy\""));
        let root = set_property(SCENE, ".", "visible", "false").unwrap();
        assert!(root.contains("[node name=\"Level\" type=\"Node2D\"]\nvisible = false\n"));

        assert!(set_property(SCENE, "Nope", "position", "Vector2(0, 0)").is_err());
        assert!(set_property(SCENE, "Egg", "bad key", "1").is_err());
        assert!(set_property(SCENE, "Egg", "position", "Vector2(1,").is_err());

        let crlf = SCENE.replace('\n', "\r\n");
        let out = set_property(&crlf, "Egg", "position", "Vector2(1, 1)").unwrap();
        assert!(out.contains("position = Vector2(1, 1)\r\ntexture"));
    }

    #[test]
    fn resolves_textures_through_imports() {
        let dir = tempfile::tempdir().unwrap();
        let unpacked = dir.path();
        std::fs::create_dir_all(unpacked.join("apktool/assets/.godot/imported")).unwrap();
        std::fs::create_dir_all(unpacked.join("apktool/assets/art")).unwrap();
        std::fs::write(unpacked.join("apktool/assets/project.binary"), "").unwrap();
        std::fs::write(
            unpacked.join("apktool/assets/art/egg.png.import"),
            "[remap]\npath=\"res://.godot/imported/egg.png-1.ctex\"\n",
        )
        .unwrap();
        std::fs::write(unpacked.join("apktool/assets/.godot/imported/egg.png-1.ctex"), "GST2").unwrap();
        let scene = parse(unpacked, "apktool/assets/levels/one.tscn", SCENE);
        assert_eq!(scene.resources[0].file.as_deref(), Some("apktool/assets/.godot/imported/egg.png-1.ctex"));
    }
}
